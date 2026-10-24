//! Memória: `/proc/meminfo`, zRAM (`/sys/block/zram*`) e PSI
//! (`/proc/pressure/memory`).

use std::fs;
use std::path::Path;

use crate::model::{MemSample, Psi, Zram};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Meminfo {
    pub total: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}

/// Valores em bytes. `None` sem `MemTotal`.
pub(crate) fn parse_meminfo(text: &str) -> Option<Meminfo> {
    let (mut total, mut available) = (None, None);
    let (mut free, mut buffers, mut cached) = (0, 0, 0);
    let mut info = Meminfo::default();
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        // "MemTotal:       16303392 kB"
        let Some(kb) = rest
            .split_ascii_whitespace()
            .next()
            .and_then(|v| v.parse::<u64>().ok())
        else {
            continue;
        };
        let bytes = kb * 1024;
        match key {
            "MemTotal" => total = Some(bytes),
            "MemAvailable" => available = Some(bytes),
            "MemFree" => free = bytes,
            "Buffers" => buffers = bytes,
            "Cached" => cached = bytes,
            "SwapTotal" => info.swap_total = bytes,
            "SwapFree" => info.swap_free = bytes,
            _ => {}
        }
    }
    info.total = total?;
    // `MemAvailable` existe desde o kernel 3.14; antes, a estimativa clássica.
    info.available = available.unwrap_or(free + buffers + cached).min(info.total);
    info.swap_free = info.swap_free.min(info.swap_total);
    Some(info)
}

/// `some avg10=0.00 avg60=0.01 avg300=0.01 total=471390` (e a linha `full`).
pub(crate) fn parse_psi(text: &str) -> Option<Psi> {
    let avg10 = |kind: &str| {
        text.lines()
            .find(|l| l.starts_with(kind))?
            .split_ascii_whitespace()
            .find_map(|f| f.strip_prefix("avg10="))?
            .parse::<f32>()
            .ok()
    };
    Some(Psi {
        some_avg10: avg10("some ")?,
        full_avg10: avg10("full "),
    })
}

/// Swap em disco (partições e arquivos) do `/proc/swaps`, em bytes:
/// `(total, usado)`. O zRAM usado como swap fica de fora: ele já aparece em
/// [`Zram`], e o `SwapTotal` do `meminfo` somaria os dois.
pub(crate) fn parse_swaps(text: &str) -> (u64, u64) {
    // `Filename Type Size Used Priority`, tamanhos em KiB. Espaço no nome vem
    // escapado (`\040`), então os campos se separam por espaço. O cabeçalho
    // cai fora porque `Size` não é número.
    text.lines()
        .filter_map(|line| {
            let mut f = line.split_ascii_whitespace();
            let name = f.next()?;
            let size: u64 = f.nth(1)?.parse().ok()?;
            let used: u64 = f.next()?.parse().ok()?;
            (!name.starts_with("/dev/zram")).then_some((size * 1024, used * 1024))
        })
        .fold((0, 0), |(total, used), (s, u)| (total + s, used + u))
}

/// `orig_data_size compr_data_size mem_used_total …` do `mm_stat`.
fn parse_mm_stat(text: &str) -> Option<(u64, u64, u64)> {
    let mut f = text.split_ascii_whitespace().map(|v| v.parse::<u64>().ok());
    Some((f.next()??, f.next()??, f.next()??))
}

/// Soma dos zRAM configurados (`disksize > 0`). `None` sem nenhum.
fn zram(root: &Path) -> Option<Zram> {
    let block = root.join("sys/block");
    let mut out = Zram {
        devices: 0,
        disksize: 0,
        orig_bytes: 0,
        compr_bytes: 0,
        mem_used: 0,
    };
    for entry in fs::read_dir(block).ok()?.flatten() {
        if !entry.file_name().as_encoded_bytes().starts_with(b"zram") {
            continue;
        }
        let dir = entry.path();
        let disksize: u64 = fs::read_to_string(dir.join("disksize"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        if disksize == 0 {
            continue;
        }
        let (orig, compr, used) = fs::read_to_string(dir.join("mm_stat"))
            .ok()
            .and_then(|s| parse_mm_stat(&s))
            .unwrap_or_default();
        out.devices += 1;
        out.disksize += disksize;
        out.orig_bytes += orig;
        out.compr_bytes += compr;
        out.mem_used += used;
    }
    (out.devices > 0).then_some(out)
}

/// `swaps`: o resultado de [`parse_swaps`]; sem o `/proc/swaps` (kernel sem
/// swap), vale o `meminfo`.
pub(crate) fn mem_sample(root: &Path, info: Meminfo, swaps: Option<(u64, u64)>) -> MemSample {
    let (swap_total, swap_used) =
        swaps.unwrap_or((info.swap_total, info.swap_total - info.swap_free));
    MemSample {
        total: info.total,
        used: info.total - info.available,
        available: info.available,
        swap_total,
        swap_used: swap_used.min(swap_total),
        zram: zram(root),
        psi: fs::read_to_string(root.join("proc/pressure/memory"))
            .ok()
            .and_then(|s| parse_psi(&s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    const MEMINFO: &str = "\
MemTotal:       16303392 kB
MemFree:         1234567 kB
MemAvailable:    9000000 kB
Buffers:          200000 kB
Cached:          5000000 kB
SwapCached:          100 kB
SwapTotal:       4194300 kB
SwapFree:        4104600 kB
HugePages_Total:       0
";

    #[test]
    fn parses_meminfo_in_bytes() {
        let m = parse_meminfo(MEMINFO).unwrap();
        assert_eq!(m.total, 16303392 * 1024);
        assert_eq!(m.available, 9000000 * 1024);
        assert_eq!(m.swap_total - m.swap_free, (4194300 - 4104600) * 1024);
    }

    #[test]
    fn without_swap() {
        let m = parse_meminfo(
            "MemTotal: 1000 kB\nMemAvailable: 400 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n",
        )
        .unwrap();
        let fx = Fixture::new("noswap");
        let s = mem_sample(fx.path(), m, Some(parse_swaps(SWAPS_HEADER)));
        assert_eq!((s.swap_total, s.swap_used), (0, 0));
        assert_eq!(s.used, 600 * 1024);
        assert!(s.zram.is_none());
        assert!(s.psi.is_none(), "kernel sem PSI");
    }

    #[test]
    fn old_kernel_without_memavailable() {
        let m =
            parse_meminfo("MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 50 kB\nCached: 250 kB\n")
                .unwrap();
        assert_eq!(m.available, 400 * 1024);
    }

    #[test]
    fn without_memtotal_is_invalid() {
        assert!(parse_meminfo("MemFree: 1 kB\n").is_none());
    }

    #[test]
    fn psi_some_and_full() {
        let p = parse_psi("some avg10=12.50 avg60=0.01 avg300=0.01 total=471390\nfull avg10=3.25 avg60=0.01 avg300=0.00 total=357014\n").unwrap();
        assert_eq!(
            p,
            Psi {
                some_avg10: 12.5,
                full_avg10: Some(3.25)
            }
        );
        // Kernels antigos só têm a linha `some`.
        let p = parse_psi("some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
        assert_eq!(p.full_avg10, None);
    }

    #[test]
    fn zram_without_disk_swap() {
        // Fedora/Pop!_OS: o swap é só o zRAM; zram1 existe mas não foi configurado.
        let fx = Fixture::new("zram");
        fx.write("sys/block/zram0/disksize", "8589934592\n")
            .write("sys/block/zram0/mm_stat", "  1073741824   268435456   285212672        0   300000000     1234        0        0        0\n")
            .write("sys/block/zram1/disksize", "0\n")
            .dir("sys/block/sda")
            .write("proc/pressure/memory", "some avg10=0.00 avg60=0.00 avg300=0.00 total=1\n");
        let m = parse_meminfo("MemTotal: 1000 kB\nMemAvailable: 400 kB\nSwapTotal: 8388608 kB\nSwapFree: 7340032 kB\n").unwrap();
        let swaps = format!("{SWAPS_HEADER}/dev/zram0 partition 8388604 1048576 100\n");
        let s = mem_sample(fx.path(), m, Some(parse_swaps(&swaps)));
        assert_eq!(
            s.zram,
            Some(Zram {
                devices: 1,
                disksize: 8589934592,
                orig_bytes: 1073741824,
                compr_bytes: 268435456,
                mem_used: 285212672,
            })
        );
        // O swap do `meminfo` (1 GiB usado) é o próprio zRAM: não conta de novo.
        assert_eq!((s.swap_total, s.swap_used), (0, 0));
        assert!(s.psi.is_some());
    }

    const SWAPS_HEADER: &str = "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n";

    #[test]
    fn swaps_on_disk_without_zram() {
        // Arquivo do instalador do Ubuntu, partição e zRAM juntos.
        let text = format!(
            "{SWAPS_HEADER}/swap.img                               file\t\t4194300\t\t1024\t\t-2\n\
             /dev/nvme0n1p3                          partition\t2097148\t\t0\t\t-3\n\
             /dev/zram0                              partition\t8388604\t\t524288\t\t100\n\
             /home/ana/meu\\040swap                   file\t\t1024\t\t0\t\t-4\n"
        );
        assert_eq!(
            parse_swaps(&text),
            ((4194300 + 2097148 + 1024) * 1024, 1024 * 1024)
        );
        assert_eq!(parse_swaps(SWAPS_HEADER), (0, 0));
    }

    #[test]
    fn without_proc_swaps_uses_meminfo() {
        let m = parse_meminfo(MEMINFO).unwrap();
        let fx = Fixture::new("noswaps");
        let s = mem_sample(fx.path(), m, None);
        assert_eq!(s.swap_total, 4194300 * 1024);
        assert_eq!(s.swap_used, (4194300 - 4104600) * 1024);
    }
}
