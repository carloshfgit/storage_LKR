//! Leitura e gravação por disco: `/proc/diskstats` filtrado pelos discos
//! físicos de `/sys/block`.

use std::fs;
use std::path::Path;

use crate::model::DiskKind;
use crate::sensors::{self, TempSensor};

/// O `diskstats` sempre conta setores de 512 bytes, qualquer que seja o
/// setor físico do disco.
pub(crate) const SECTOR: u64 = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiskStat {
    pub name: String,
    pub read_sectors: u64,
    pub write_sectors: u64,
    /// Milissegundos com I/O em andamento.
    pub io_ms: u64,
}

/// `major minor nome leituras _ setores_lidos _ gravações _ setores_gravados _ _ io_ticks …`
pub(crate) fn parse_diskstats(text: &str) -> Vec<DiskStat> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_ascii_whitespace().collect();
            let num = |i: usize| f.get(i)?.parse::<u64>().ok();
            Some(DiskStat {
                name: (*f.get(2)?).to_string(),
                read_sectors: num(5)?,
                write_sectors: num(9)?,
                io_ms: num(12)?,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DiskMeta {
    pub model: Option<String>,
    pub kind: DiskKind,
    pub rotational: Option<bool>,
    pub sensor: Option<TempSensor>,
    pub temp_hint: Option<String>,
}

/// O SATA só tem temperatura com o módulo `drivetemp` (o Ubuntu traz, mas não carrega).
pub(crate) const DRIVETEMP_HINT: &str = "sudo modprobe drivetemp";

/// Fabricantes dos discos virtuais: não têm sensor, com ou sem `drivetemp`.
const VIRTUAL_VENDORS: [&str; 4] = ["QEMU", "VMware", "VBOX", "Msft"];

/// Nomes que nunca são um disco físico próprio: `dm-*` (LVM, LUKS) e `md*`
/// (RAID) repetem o I/O dos discos embaixo deles; os outros não são discos.
fn is_excluded(name: &str) -> bool {
    const PREFIXES: [&str; 8] = ["loop", "ram", "zram", "sr", "fd", "dm-", "md", "nbd"];
    PREFIXES.iter().any(|p| name.starts_with(p))
        // Partições de boot e RPMB do eMMC aparecem em /sys/block, mas são o mesmo chip.
        || (name.starts_with("mmcblk") && (name.contains("boot") || name.ends_with("rpmb")))
}

/// Metadados de `name` se ele for um disco físico inteiro: está em
/// `/sys/block` (partições não estão) e tem `device/` (dispositivos virtuais
/// não têm).
pub(crate) fn disk_meta(root: &Path, name: &str) -> Option<DiskMeta> {
    if is_excluded(name) {
        return None;
    }
    let dir = root.join("sys/block").join(name);
    if !dir.join("device").exists() {
        return None;
    }
    let read = |rel: &str| {
        fs::read_to_string(dir.join(rel))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    // /sys/block/sdb -> ../devices/pci0000:00/…/usb2/2-1/…/block/sdb
    let usb = fs::read_link(&dir)
        .map(|t| t.to_string_lossy().contains("/usb"))
        .unwrap_or(false);
    let kind = if usb {
        DiskKind::Usb
    } else if name.starts_with("nvme") {
        DiskKind::Nvme
    } else if name.starts_with("mmcblk") {
        DiskKind::Mmc
    } else if name.starts_with("vd") || name.starts_with("xvd") {
        DiskKind::Virtual
    } else if name.starts_with("sd") {
        DiskKind::Sata
    } else {
        DiskKind::Other
    };
    let sensor = sensors::disk_sensor(root, name);
    // Com o módulo carregado e sem sensor, é o disco que não informa: sem dica.
    let is_virtual =
        read("device/vendor").is_some_and(|v| VIRTUAL_VENDORS.iter().any(|p| v.starts_with(p)));
    let temp_hint = (kind == DiskKind::Sata
        && sensor.is_none()
        && !is_virtual
        && !root.join("sys/module/drivetemp").exists())
    .then(|| DRIVETEMP_HINT.to_string());
    Some(DiskMeta {
        // eMMC e SD: `device/name`; o resto: `device/model`.
        model: read("device/model").or_else(|| read("device/name")),
        kind,
        rotational: read("queue/rotational").map(|r| r == "1"),
        sensor,
        temp_hint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    const DISKSTATS: &str = "\
   7       0 loop0 14 0 34 4 0 0 0 0 0 4 4 0 0 0 0 0 0
   8       0 sda 101 0 2000 50 202 0 4000 80 0 120 130 0 0 0 0 0 0
   8       1 sda1 100 0 1900 50 200 0 3900 80 0 110 130 0 0 0 0 0 0
   8      16 sdb 10 0 200 5 20 0 400 8 0 12 13 0 0 0 0 0 0
   9       0 md0 50 0 1000 0 100 0 2000 0 0 0 0 0 0 0 0 0 0
 259       0 nvme0n1 300 0 6000 90 400 0 8000 70 0 150 160 0 0 0 0 0 0
 259       1 nvme0n1p1 300 0 6000 90 400 0 8000 70 0 150 160 0 0 0 0 0 0
 253       0 dm-0 300 0 6000 90 400 0 8000 70 0 150 160 0 0 0 0 0 0
 179       0 mmcblk0 30 0 600 9 40 0 800 7 0 15 16 0 0 0 0 0 0
 179       8 mmcblk0boot0 1 0 8 0 0 0 0 0 0 1 1 0 0 0 0 0 0
 252       0 zram0 1 0 8 0 0 0 0 0 0 1 1 0 0 0 0 0 0
";

    #[test]
    fn parses_all_lines() {
        let s = parse_diskstats(DISKSTATS);
        assert_eq!(s.len(), 11);
        assert_eq!(
            s[1],
            DiskStat {
                name: "sda".into(),
                read_sectors: 2000,
                write_sectors: 4000,
                io_ms: 120
            }
        );
    }

    #[test]
    fn kernel_4_with_fewer_fields() {
        // Antes do 4.18 não havia os campos de discard (14 campos).
        let s = parse_diskstats("   8       0 sda 1 0 2 3 4 0 5 6 0 7 8\n");
        assert_eq!(s[0].io_ms, 7);
    }

    /// Árvore com dois `sd*` sob um RAID `md0`, um NVMe sob LVM (`dm-0`), um
    /// pendrive USB, um eMMC com partições de boot, `loop` e `zram`.
    fn machine() -> Fixture {
        let fx = Fixture::new("disks");
        fx.write(
            "sys/devices/pci0000:00/ata1/block/sda/device/model",
            "KINGSTON SA400S37240G   \n",
        )
        .write(
            "sys/devices/pci0000:00/ata1/block/sda/queue/rotational",
            "0\n",
        )
        .symlink("sys/block/sda", "../devices/pci0000:00/ata1/block/sda")
        .write(
            "sys/devices/pci0000:00/usb2/2-1/block/sdb/device/model",
            "Flash Disk\n",
        )
        .symlink("sys/block/sdb", "../devices/pci0000:00/usb2/2-1/block/sdb")
        .write("sys/block/nvme0n1/device/model", "Samsung SSD 980 1TB\n")
        .write("sys/block/nvme0n1/queue/rotational", "0\n")
        .write("sys/block/mmcblk0/device/name", "DA4064\n")
        .dir("sys/block/mmcblk0boot0/device")
        .dir("sys/block/md0/md")
        .dir("sys/block/dm-0/dm")
        .dir("sys/block/loop0/loop")
        .dir("sys/block/zram0");
        fx
    }

    #[test]
    fn keeps_only_physical_disks() {
        let fx = machine();
        let names: Vec<String> = parse_diskstats(DISKSTATS)
            .into_iter()
            .filter(|d| disk_meta(fx.path(), &d.name).is_some())
            .map(|d| d.name)
            .collect();
        assert_eq!(names, ["sda", "sdb", "nvme0n1", "mmcblk0"]);
    }

    #[test]
    fn classifies_kind_and_model() {
        let fx = machine();
        let sda = disk_meta(fx.path(), "sda").unwrap();
        assert_eq!(sda.kind, DiskKind::Sata);
        assert_eq!(sda.model.as_deref(), Some("KINGSTON SA400S37240G"));
        assert_eq!(sda.rotational, Some(false));
        assert_eq!(disk_meta(fx.path(), "sdb").unwrap().kind, DiskKind::Usb);
        assert_eq!(
            disk_meta(fx.path(), "nvme0n1").unwrap().kind,
            DiskKind::Nvme
        );
        let mmc = disk_meta(fx.path(), "mmcblk0").unwrap();
        assert_eq!(
            (mmc.kind, mmc.model.as_deref()),
            (DiskKind::Mmc, Some("DA4064"))
        );
        assert_eq!(mmc.rotational, None);
    }

    #[test]
    fn drivetemp_hint_only_for_real_sata_without_module() {
        let fx = machine();
        // sda: SATA sem sensor e sem o módulo → dica; o pendrive USB não.
        assert_eq!(
            disk_meta(fx.path(), "sda").unwrap().temp_hint.as_deref(),
            Some(DRIVETEMP_HINT)
        );
        assert_eq!(disk_meta(fx.path(), "sdb").unwrap().temp_hint, None);
        assert_eq!(disk_meta(fx.path(), "nvme0n1").unwrap().temp_hint, None);
        // Com o módulo carregado, é o disco que não informa.
        fx.dir("sys/module/drivetemp");
        assert_eq!(disk_meta(fx.path(), "sda").unwrap().temp_hint, None);
    }

    #[test]
    fn virtual_scsi_disk_has_no_hint() {
        let fx = Fixture::new("qemu");
        fx.write("sys/block/sda/device/vendor", "QEMU    \n");
        assert_eq!(disk_meta(fx.path(), "sda").unwrap().temp_hint, None);
    }

    #[test]
    fn virtio_disk_without_model() {
        let fx = Fixture::new("virtio");
        fx.dir("sys/block/vda/device");
        let vda = disk_meta(fx.path(), "vda").unwrap();
        assert_eq!((vda.kind, vda.model), (DiskKind::Virtual, None));
    }
}
