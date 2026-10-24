//! Temperaturas: `/sys/class/hwmon` (e `/sys/class/thermal` como alternativa).
//!
//! A busca roda uma vez (CPU no primeiro `sample`, cada disco quando aparece)
//! e guarda o caminho do `temp*_input`; as amostras seguintes só leem esse
//! arquivo. A busca lê só `name` e `temp*_label`: ler um `temp*_input` de uma
//! GPU dedicada em repouso a acordaria (Etapa 18).

use std::fs;
use std::path::{Path, PathBuf};

use crate::model::Temp;

/// Limiar de alerta quando o sensor não informa o próprio.
pub(crate) const CPU_HOT_C: f32 = 85.0;
pub(crate) const DISK_HOT_C: f32 = 70.0;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TempSensor {
    pub input: PathBuf,
    pub hot_c: f32,
    /// Sensor da placa (ACPI), não da CPU: serve de indicação, não de medida.
    pub approximate: bool,
}

impl TempSensor {
    pub fn read(&self) -> Option<Temp> {
        let milli: i64 = fs::read_to_string(&self.input).ok()?.trim().parse().ok()?;
        Some(Temp {
            celsius: milli as f32 / 1000.0,
            hot_c: self.hot_c,
            approximate: self.approximate,
        })
    }
}

struct Chip {
    dir: PathBuf,
    name: String,
}

fn chips(root: &Path) -> Vec<Chip> {
    let Ok(entries) = fs::read_dir(root.join("sys/class/hwmon")) else {
        return Vec::new();
    };
    let mut chips: Vec<Chip> = entries
        .flatten()
        .filter_map(|e| {
            let dir = e.path();
            let name = fs::read_to_string(dir.join("name"))
                .ok()?
                .trim()
                .to_string();
            Some(Chip { dir, name })
        })
        .collect();
    // hwmon2 antes de hwmon10.
    chips.sort_by_key(|c| {
        let n = c
            .dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        (n.len(), n)
    });
    chips
}

/// `temp*_input` do chip, em ordem, com o rótulo de cada um.
fn temps(dir: &Path) -> Vec<(u32, Option<String>)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(u32, Option<String>)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let n = name
                .strip_prefix("temp")?
                .strip_suffix("_input")?
                .parse()
                .ok()?;
            let label = fs::read_to_string(dir.join(format!("temp{n}_label")))
                .ok()
                .map(|l| l.trim().to_string());
            Some((n, label))
        })
        .collect();
    out.sort_unstable_by_key(|&(n, _)| n);
    out
}

/// O primeiro `temp*` cujo rótulo está em `labels` (na ordem de `labels`); sem
/// nenhum, o primeiro do chip.
fn pick(dir: &Path, labels: &[&str]) -> Option<u32> {
    let temps = temps(dir);
    labels
        .iter()
        .find_map(|want| {
            temps
                .iter()
                .find(|(_, l)| l.as_deref().is_some_and(|l| l.starts_with(want)))
        })
        .or_else(|| temps.first())
        .map(|&(n, _)| n)
}

/// Limiar de alerta: 10 °C abaixo do `max` (ou do `crit`) do sensor, sem
/// passar do padrão. O `max` do `coretemp` é o TjMax (100 °C), alto demais
/// para avisar.
fn sensor(dir: &Path, n: u32, default_hot: f32, approximate: bool) -> TempSensor {
    let limit = ["max", "crit"].iter().find_map(|k| {
        let milli: i64 = fs::read_to_string(dir.join(format!("temp{n}_{k}")))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        (milli > 0).then_some(milli as f32 / 1000.0)
    });
    TempSensor {
        input: dir.join(format!("temp{n}_input")),
        hot_c: limit.map_or(default_hot, |l| default_hot.min(l - 10.0)),
        approximate,
    }
}

/// Sensor da CPU, pela ordem de preferência: `coretemp` (Intel), `k10temp` e
/// `zenpower` (AMD), `cpu_thermal` (ARM), a zona `x86_pkg_temp` e, por último,
/// o `acpitz` da placa (aproximado).
pub(crate) fn cpu_sensor(root: &Path) -> Option<TempSensor> {
    let chips = chips(root);
    let find = |name: &str| chips.iter().find(|c| c.name == name);

    if let Some(c) = find("coretemp") {
        let n = pick(&c.dir, &["Package id"])?;
        return Some(sensor(&c.dir, n, CPU_HOT_C, false));
    }
    // No Zen, o Tctl pode ter um deslocamento; o Tdie (quando existe) é a medida real.
    for name in ["k10temp", "zenpower"] {
        if let Some(c) = find(name) {
            let n = pick(&c.dir, &["Tdie", "Tctl"])?;
            return Some(sensor(&c.dir, n, CPU_HOT_C, false));
        }
    }
    if let Some(c) = find("cpu_thermal") {
        let n = pick(&c.dir, &[])?;
        return Some(sensor(&c.dir, n, CPU_HOT_C, false));
    }
    if let Some(zone) = thermal_zone(root, "x86_pkg_temp") {
        return Some(TempSensor {
            input: zone.join("temp"),
            hot_c: CPU_HOT_C,
            approximate: false,
        });
    }
    let c = chips.iter().find(|c| c.name.starts_with("acpitz"))?;
    let n = pick(&c.dir, &[])?;
    Some(sensor(&c.dir, n, CPU_HOT_C, true))
}

fn thermal_zone(root: &Path, kind: &str) -> Option<PathBuf> {
    fs::read_dir(root.join("sys/class/thermal"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|dir| fs::read_to_string(dir.join("type")).is_ok_and(|t| t.trim() == kind))
}

/// Sensor do disco `name`: o chip `nvme` ou `drivetemp` cujo `device` é o mesmo
/// do disco em `/sys/block/<name>/device` (o controlador NVMe ou o dispositivo SCSI).
pub(crate) fn disk_sensor(root: &Path, name: &str) -> Option<TempSensor> {
    let disk_dev = fs::canonicalize(root.join("sys/block").join(name).join("device")).ok()?;
    let chip = chips(root).into_iter().find(|c| {
        (c.name == "nvme" || c.name == "drivetemp")
            && fs::canonicalize(c.dir.join("device")).is_ok_and(|d| d == disk_dev)
    })?;
    // NVMe: "Composite" é a temperatura do controlador que o fabricante reporta.
    let n = pick(&chip.dir, &["Composite"])?;
    Some(sensor(&chip.dir, n, DISK_HOT_C, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    fn temp(fx: &Fixture, hwmon: &str, name: &str, sensors: &[(u32, Option<&str>, i64)]) {
        fx.write(
            &format!("sys/class/hwmon/{hwmon}/name"),
            &format!("{name}\n"),
        );
        for &(n, label, milli) in sensors {
            fx.write(
                &format!("sys/class/hwmon/{hwmon}/temp{n}_input"),
                &format!("{milli}\n"),
            );
            if let Some(l) = label {
                fx.write(
                    &format!("sys/class/hwmon/{hwmon}/temp{n}_label"),
                    &format!("{l}\n"),
                );
            }
        }
    }

    fn read(s: &TempSensor) -> f32 {
        s.read().unwrap().celsius
    }

    #[test]
    fn coretemp_package_with_many_cores() {
        let fx = Fixture::new("coretemp");
        // Placa Dell: o dell_smm também tem "CPU", mas o coretemp vem antes.
        temp(&fx, "hwmon4", "dell_smm", &[(1, Some("CPU"), 44000)]);
        temp(
            &fx,
            "hwmon5",
            "coretemp",
            &[
                (2, Some("Core 0"), 53000),
                (1, Some("Package id 0"), 48000),
                (3, Some("Core 1"), 56000),
            ],
        );
        fx.write("sys/class/hwmon/hwmon5/temp1_max", "100000\n");
        let s = cpu_sensor(fx.path()).unwrap();
        assert_eq!(read(&s), 48.0);
        assert_eq!(s.hot_c, CPU_HOT_C, "TjMax de 100 °C não sobe o limiar");
        assert!(!s.approximate);
    }

    #[test]
    fn k10temp_prefers_tdie() {
        let fx = Fixture::new("k10");
        temp(
            &fx,
            "hwmon1",
            "k10temp",
            &[
                (1, Some("Tctl"), 72000),
                (2, Some("Tdie"), 62000),
                (3, Some("Tccd1"), 60000),
            ],
        );
        assert_eq!(read(&cpu_sensor(fx.path()).unwrap()), 62.0);
        // Ryzen recentes só têm Tctl.
        let fx = Fixture::new("k10-tctl");
        temp(
            &fx,
            "hwmon1",
            "k10temp",
            &[(1, Some("Tctl"), 55500), (3, Some("Tccd1"), 50000)],
        );
        assert_eq!(read(&cpu_sensor(fx.path()).unwrap()), 55.5);
    }

    #[test]
    fn arm_cpu_thermal() {
        let fx = Fixture::new("arm-temp");
        temp(&fx, "hwmon0", "cpu_thermal", &[(1, None, 47234)]);
        temp(&fx, "hwmon1", "rp1_adc", &[(1, None, 30000)]);
        assert!((read(&cpu_sensor(fx.path()).unwrap()) - 47.234).abs() < 0.01);
    }

    #[test]
    fn x86_pkg_temp_zone_without_hwmon() {
        let fx = Fixture::new("zone");
        fx.write("sys/class/thermal/thermal_zone0/type", "acpitz\n")
            .write("sys/class/thermal/thermal_zone0/temp", "25000\n")
            .write("sys/class/thermal/thermal_zone7/type", "x86_pkg_temp\n")
            .write("sys/class/thermal/thermal_zone7/temp", "51000\n");
        temp(&fx, "hwmon1", "acpitz", &[(1, None, 25000)]);
        assert_eq!(read(&cpu_sensor(fx.path()).unwrap()), 51.0);
    }

    #[test]
    fn only_acpitz_is_approximate() {
        let fx = Fixture::new("acpitz");
        // Kernels novos numeram: "acpitz_0".
        temp(&fx, "hwmon1", "acpitz_0", &[(1, None, 25000)]);
        temp(&fx, "hwmon3", "pch_skylake_5", &[(1, None, 37500)]);
        let s = cpu_sensor(fx.path()).unwrap();
        assert_eq!(read(&s), 25.0);
        assert!(s.approximate);
    }

    #[test]
    fn without_any_sensor() {
        let fx = Fixture::new("nosensor");
        temp(&fx, "hwmon0", "AC", &[]);
        assert!(cpu_sensor(fx.path()).is_none());
        let empty = Fixture::new("nohwmon");
        assert!(cpu_sensor(empty.path()).is_none());
    }

    /// NVMe com hwmon, SATA com `drivetemp` (sda), SATA sem (sdb) e eMMC.
    fn disks() -> Fixture {
        let fx = Fixture::new("disktemp");
        fx.dir("sys/devices/pci0/nvme/nvme0")
            .dir("sys/devices/pci0/ata1/host0/target0:0:0/0:0:0:0")
            .dir("sys/devices/pci0/ata2/host1/target1:0:0/1:0:0:0")
            .dir("sys/devices/mmc0/mmc0:0001")
            .symlink("sys/block/nvme0n1/device", "../../devices/pci0/nvme/nvme0")
            .symlink(
                "sys/block/sda/device",
                "../../devices/pci0/ata1/host0/target0:0:0/0:0:0:0",
            )
            .symlink(
                "sys/block/sdb/device",
                "../../devices/pci0/ata2/host1/target1:0:0/1:0:0:0",
            )
            .symlink("sys/block/mmcblk0/device", "../../devices/mmc0/mmc0:0001");
        temp(
            &fx,
            "hwmon1",
            "nvme",
            &[(1, Some("Composite"), 41850), (2, Some("Sensor 1"), 45850)],
        );
        fx.write("sys/class/hwmon/hwmon1/temp1_max", "81850\n")
            .symlink(
                "sys/class/hwmon/hwmon1/device",
                "../../../devices/pci0/nvme/nvme0",
            );
        temp(&fx, "hwmon2", "drivetemp", &[(1, None, 33000)]);
        fx.write("sys/class/hwmon/hwmon2/temp1_max", "60000\n")
            .symlink(
                "sys/class/hwmon/hwmon2/device",
                "../../../devices/pci0/ata1/host0/target0:0:0/0:0:0:0",
            );
        fx
    }

    #[test]
    fn nvme_and_sata_with_drivetemp() {
        let fx = disks();
        let nvme = disk_sensor(fx.path(), "nvme0n1").unwrap();
        assert!((read(&nvme) - 41.85).abs() < 0.01);
        assert_eq!(nvme.hot_c, DISK_HOT_C);
        let sda = disk_sensor(fx.path(), "sda").unwrap();
        assert_eq!(read(&sda), 33.0);
        assert_eq!(sda.hot_c, 50.0, "limiar 10 °C abaixo do max do disco");
    }

    #[test]
    fn sata_without_drivetemp_and_emmc_have_none() {
        let fx = disks();
        assert!(disk_sensor(fx.path(), "sdb").is_none());
        assert!(disk_sensor(fx.path(), "mmcblk0").is_none());
    }

    #[test]
    fn unreadable_input_is_none() {
        // GPU suspensa ou sensor que some: o arquivo existe mas não lê.
        let s = TempSensor {
            input: "/nao/existe".into(),
            hot_c: 85.0,
            approximate: false,
        };
        assert!(s.read().is_none());
    }
}
