//! Nome comercial da placa pelo `pci.ids` (do pacote `pci.ids`, que o
//! `pciutils` traz), lido direto do arquivo. O `lspci` lê o espaço de
//! configuração PCI de cada placa, e isso acorda uma GPU em D3cold (medido:
//! `runtime_status` de `suspended` para `active`); os IDs vêm do `uevent`,
//! que não acorda.

use std::fs;
use std::path::Path;

use crate::model::GpuVendor;

const PATHS: [&str; 2] = ["usr/share/misc/pci.ids", "usr/share/hwdata/pci.ids"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PciId {
    pub vendor: u16,
    pub device: u16,
    /// Subsistema (fabricante do notebook ou da placa): `None` se o `uevent` não informa.
    pub subsys: Option<(u16, u16)>,
}

/// `uevent`: `PCI_ID=1002:6900` e `PCI_SUBSYS_ID=1028:0767`.
pub(super) fn parse_id(id: &str, subsys: Option<&str>) -> Option<PciId> {
    let pair = |s: &str| {
        let (a, b) = s.split_once(':')?;
        Some((
            u16::from_str_radix(a, 16).ok()?,
            u16::from_str_radix(b, 16).ok()?,
        ))
    };
    let (vendor, device) = pair(id)?;
    Some(PciId {
        vendor,
        device,
        subsys: subsys.and_then(pair),
    })
}

/// O nome de cada `id`, na mesma ordem; `None` sem o arquivo ou fora dele.
pub(super) fn names(root: &Path, ids: &[(GpuVendor, PciId)]) -> Vec<Option<String>> {
    if ids.is_empty() {
        return Vec::new();
    }
    let Some(text) = PATHS
        .iter()
        .find_map(|p| fs::read_to_string(root.join(p)).ok())
    else {
        return vec![None; ids.len()];
    };
    let only_ids: Vec<PciId> = ids.iter().map(|&(_, id)| id).collect();
    lookup(&text, &only_ids)
        .into_iter()
        .zip(ids)
        .map(|((device, subsys), &(vendor, _))| device.map(|d| display(vendor, d, subsys)))
        .collect()
}

/// Nome do dispositivo e do subsistema de cada `id` no texto do `pci.ids`.
///
/// Formato: `1002  Fabricante`, `\t6900  Dispositivo`, `\t\t1028 0767  Subsistema`;
/// a seção de classes (`C 00  …`) fica no fim e é ignorada.
pub(super) fn lookup<'a>(text: &'a str, ids: &[PciId]) -> Vec<(Option<&'a str>, Option<&'a str>)> {
    let mut out = vec![(None, None); ids.len()];
    let mut vendor: Option<u16> = None;
    let mut device: Option<u16> = None;
    for line in text.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("\t\t") {
            let (Some(v), Some(d)) = (vendor, device) else {
                continue;
            };
            let Some((sv, rest)) = rest.split_once(' ') else {
                continue;
            };
            let Some((sd, name)) = rest.split_once("  ") else {
                continue;
            };
            let sub = (
                u16::from_str_radix(sv, 16).ok(),
                u16::from_str_radix(sd, 16).ok(),
            );
            for (i, id) in ids.iter().enumerate() {
                if (id.vendor, id.device) == (v, d)
                    && id.subsys.map(|(a, b)| (Some(a), Some(b))) == Some(sub)
                {
                    out[i].1 = Some(name.trim());
                }
            }
        } else if let Some(rest) = line.strip_prefix('\t') {
            device = None;
            let Some(v) = vendor else {
                continue;
            };
            let Some((d, name)) = rest.split_once("  ") else {
                continue;
            };
            let Ok(d) = u16::from_str_radix(d, 16) else {
                continue;
            };
            for (i, id) in ids.iter().enumerate() {
                if (id.vendor, id.device) == (v, d) {
                    out[i].0 = Some(name.trim());
                    device = Some(d);
                }
            }
        } else {
            if line.starts_with("C ") {
                break;
            }
            device = None;
            // Só interessa entrar nos fabricantes das placas procuradas.
            vendor = line
                .split_once("  ")
                .and_then(|(v, _)| u16::from_str_radix(v, 16).ok())
                .filter(|v| ids.iter().any(|id| id.vendor == *v));
        }
    }
    out
}

/// "Kaby Lake-U GT2 [HD Graphics 620]" vira "Intel HD Graphics 620": o nome
/// comercial entre colchetes, com o fabricante na frente.
///
/// O subsistema é mais preciso quando é o nome da placa ("Radeon R7 M445" no
/// lugar de "Radeon R7 M260/M265 / … / 620/625 Mobile"), mas às vezes é o nome
/// do notebook ("ThinkPad T570"). Ele só vale quando começa com a mesma palavra
/// do nome comercial.
pub(super) fn display(vendor: GpuVendor, device: &str, subsys: Option<&str>) -> String {
    let base = device
        .strip_suffix(']')
        .and_then(|d| d.rsplit_once('['))
        .map_or(device, |(_, inner)| inner)
        .trim();
    let first = base.split_whitespace().next();
    let name = subsys
        .filter(|s| s.split_whitespace().next() == first)
        .unwrap_or(base);
    let prefix = match vendor {
        GpuVendor::Amd => "AMD",
        GpuVendor::Nvidia => "NVIDIA",
        GpuVendor::Intel => "Intel",
        GpuVendor::Other => return name.to_string(),
    };
    if name
        .get(..prefix.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
    {
        name.to_string()
    } else {
        format!("{prefix} {name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PCI_IDS: &str = "\
# Comentário
1002  Advanced Micro Devices, Inc. [AMD/ATI]
\t15bf  Phoenix1
\t6900  Topaz XT [Radeon R7 M260/M265 / M340/M360 / M440/M445 / 530/535 / 620/625 Mobile]
\t\t1028 0640  Radeon R7 M260/M265
\t\t1028 0767  Radeon R7 M445
10de  NVIDIA Corporation
\t2520  GA106M [GeForce RTX 3060 Mobile / Max-Q]
8086  Intel Corporation
\t5916  Kaby Lake-U GT2 [HD Graphics 620]
\t\t17aa 2248  ThinkPad T570
\t7d55  Meteor Lake-P [Intel Arc Graphics]
C 03  Display controller
\t00  VGA compatible controller
";

    fn id(v: u16, d: u16, sub: Option<(u16, u16)>) -> PciId {
        PciId {
            vendor: v,
            device: d,
            subsys: sub,
        }
    }

    #[test]
    fn device_and_subsystem() {
        let ids = [
            id(0x1002, 0x6900, Some((0x1028, 0x0767))),
            id(0x8086, 0x5916, Some((0x17aa, 0x2248))),
            id(0x10de, 0x2520, Some((0x1043, 0x16e2))),
            id(0x1af4, 0x1050, None),
        ];
        let found = lookup(PCI_IDS, &ids);
        assert_eq!(found[0].1, Some("Radeon R7 M445"));
        assert_eq!(found[1].1, Some("ThinkPad T570"));
        assert_eq!(
            found[2],
            (Some("GA106M [GeForce RTX 3060 Mobile / Max-Q]"), None)
        );
        assert_eq!(found[3], (None, None), "fabricante fora do arquivo");
    }

    #[test]
    fn display_names() {
        let amd =
            "Topaz XT [Radeon R7 M260/M265 / M340/M360 / M440/M445 / 530/535 / 620/625 Mobile]";
        assert_eq!(
            display(GpuVendor::Amd, amd, Some("Radeon R7 M445")),
            "AMD Radeon R7 M445"
        );
        // Subsistema com o nome do notebook: fica o do dispositivo.
        assert_eq!(
            display(
                GpuVendor::Intel,
                "Kaby Lake-U GT2 [HD Graphics 620]",
                Some("ThinkPad T570")
            ),
            "Intel HD Graphics 620"
        );
        // Sem repetir o fabricante.
        assert_eq!(
            display(GpuVendor::Intel, "Meteor Lake-P [Intel Arc Graphics]", None),
            "Intel Arc Graphics"
        );
        // Sem colchetes: o nome inteiro.
        assert_eq!(display(GpuVendor::Amd, "Phoenix1", None), "AMD Phoenix1");
        assert_eq!(
            display(GpuVendor::Other, "Virtio 1.0 GPU", None),
            "Virtio 1.0 GPU"
        );
    }

    #[test]
    fn uevent_ids() {
        assert_eq!(
            parse_id("1002:6900", Some("1028:0767")),
            Some(id(0x1002, 0x6900, Some((0x1028, 0x0767))))
        );
        assert_eq!(parse_id("8086:5916", None), Some(id(0x8086, 0x5916, None)));
        assert_eq!(parse_id("x", None), None);
    }
}
