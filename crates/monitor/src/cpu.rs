//! Uso e frequência da CPU: `/proc/stat` e `cpufreq` (ou `/proc/cpuinfo`).

use std::fs;
use std::path::Path;

use crate::model::{CoreSample, CpuSample};

/// Jiffies acumulados de uma linha `cpu`/`cpuN` do `/proc/stat`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CpuTimes {
    pub total: u64,
    /// `idle + iowait`: tempo em que a CPU não trabalhou.
    pub idle: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CpuStat {
    pub all: CpuTimes,
    /// Só os núcleos online, em ordem crescente de id: um núcleo desligado não
    /// tem linha.
    pub cores: Vec<(u32, CpuTimes)>,
}

/// Lê as linhas `cpu` e `cpuN`. `None` sem a linha `cpu` (arquivo inválido).
pub(crate) fn parse_stat(text: &str) -> Option<CpuStat> {
    let mut all = None;
    let mut cores = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("cpu") else {
            // As linhas `cpu*` vêm primeiro; o resto do arquivo não interessa.
            if all.is_some() {
                break;
            }
            continue;
        };
        let Some((id, nums)) = rest.split_once(' ') else {
            continue;
        };
        let Some(times) = parse_times(nums) else {
            continue;
        };
        if id.is_empty() {
            all = Some(times);
        } else if let Ok(id) = id.parse() {
            cores.push((id, times));
        }
    }
    cores.sort_unstable_by_key(|&(id, _)| id);
    Some(CpuStat { all: all?, cores })
}

/// `user nice system idle iowait irq softirq steal [guest guest_nice]`. O
/// `guest` já está somado em `user`, então só os 8 primeiros entram no total.
fn parse_times(nums: &str) -> Option<CpuTimes> {
    let mut f = [0u64; 8];
    let mut n = 0;
    for (slot, v) in f.iter_mut().zip(nums.split_ascii_whitespace()) {
        *slot = v.parse().ok()?;
        n += 1;
    }
    (n >= 4).then(|| CpuTimes {
        total: f.iter().sum(),
        idle: f[3] + f[4],
    })
}

/// % de uso entre duas leituras. `None` se o contador não andou (leituras
/// próximas demais) ou voltou (núcleo religado).
pub(crate) fn usage(prev: CpuTimes, cur: CpuTimes) -> Option<f32> {
    let dt = cur.total.checked_sub(prev.total).filter(|&d| d > 0)?;
    let di = cur.idle.saturating_sub(prev.idle).min(dt);
    Some((100.0 * (1.0 - di as f64 / dt as f64)) as f32)
}

/// Frequência atual de um núcleo pelo `cpufreq`, em MHz.
fn cpufreq_mhz(root: &Path, id: u32) -> Option<u32> {
    let path = root.join(format!(
        "sys/devices/system/cpu/cpu{id}/cpufreq/scaling_cur_freq"
    ));
    let khz: u64 = fs::read_to_string(path).ok()?.trim().parse().ok()?;
    Some((khz / 1000) as u32)
}

/// `(processor, cpu MHz)` do `/proc/cpuinfo`, para máquinas sem `cpufreq`
/// (VMs, alguns ARM). Em ARM o `cpuinfo` não tem `cpu MHz` e a lista fica vazia.
pub(crate) fn parse_cpuinfo_mhz(text: &str) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut current = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "processor" => current = value.trim().parse().ok(),
            "cpu MHz" => {
                if let (Some(id), Ok(mhz)) = (current, value.trim().parse::<f64>()) {
                    out.push((id, mhz.round() as u32));
                }
            }
            _ => {}
        }
    }
    out
}

pub(crate) fn cpu_sample(root: &Path, prev: Option<&CpuStat>, cur: &CpuStat) -> CpuSample {
    let mut freqs: Vec<Option<u32>> = cur
        .cores
        .iter()
        .map(|&(id, _)| cpufreq_mhz(root, id))
        .collect();
    if freqs.iter().all(Option::is_none)
        && let Ok(text) = fs::read_to_string(root.join("proc/cpuinfo"))
    {
        let mhz = parse_cpuinfo_mhz(&text);
        for (slot, &(id, _)) in freqs.iter_mut().zip(&cur.cores) {
            *slot = mhz.iter().find(|&&(i, _)| i == id).map(|&(_, m)| m);
        }
    }

    let cores: Vec<CoreSample> = cur
        .cores
        .iter()
        .zip(&freqs)
        .map(|(&(id, times), &freq_mhz)| {
            let before = prev.and_then(|p| {
                p.cores
                    .binary_search_by_key(&id, |&(i, _)| i)
                    .ok()
                    .map(|i| p.cores[i].1)
            });
            CoreSample {
                id,
                usage_pct: before.and_then(|b| usage(b, times)),
                freq_mhz,
            }
        })
        .collect();

    let known: Vec<u32> = freqs.iter().flatten().copied().collect();
    CpuSample {
        usage_pct: prev.and_then(|p| usage(p.all, cur.all)),
        // Preenchida pelo `Sampler`, que guarda o sensor encontrado.
        temp: None,
        freq_avg_mhz: (!known.is_empty())
            .then(|| (known.iter().map(|&m| m as u64).sum::<u64>() / known.len() as u64) as u32),
        freq_max_mhz: known.iter().max().copied(),
        cores,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    const STAT: &str = "\
cpu  4705 356 584 3699 23 23 0 0 0 0
cpu0 1393 280 307 1802 13 12 0 0 0 0
cpu1 3312 76 277 1897 10 11 0 0 0 0
intr 114930548 113199788 3 0 5 263 0 4 [...]
ctxt 1990473
cpu_bogus 1 2 3 4
";

    #[test]
    fn parses_total_and_cores() {
        let s = parse_stat(STAT).unwrap();
        assert_eq!(
            s.all,
            CpuTimes {
                total: 4705 + 356 + 584 + 3699 + 23 + 23,
                idle: 3699 + 23
            }
        );
        assert_eq!(s.cores.len(), 2);
        assert_eq!(s.cores[1].0, 1);
    }

    #[test]
    fn offline_core_has_no_line() {
        // cpu1 desligado (`echo 0 > /sys/devices/system/cpu/cpu1/online`).
        let text = "cpu  10 0 10 80 0 0 0 0\ncpu0 5 0 5 40 0 0 0 0\ncpu2 5 0 5 40 0 0 0 0\n";
        let s = parse_stat(text).unwrap();
        let ids: Vec<u32> = s.cores.iter().map(|&(id, _)| id).collect();
        assert_eq!(ids, [0, 2]);
    }

    #[test]
    fn old_kernel_with_four_fields() {
        let s = parse_stat("cpu 1 2 3 4\n").unwrap();
        assert_eq!(s.all, CpuTimes { total: 10, idle: 4 });
    }

    #[test]
    fn without_cpu_line_is_invalid() {
        assert!(parse_stat("intr 1 2 3\n").is_none());
    }

    #[test]
    fn usage_between_two_reads() {
        let a = CpuTimes {
            total: 1000,
            idle: 800,
        };
        let b = CpuTimes {
            total: 1100,
            idle: 825,
        };
        assert_eq!(usage(a, b), Some(75.0));
        assert_eq!(usage(a, a), None);
        // Contador que voltou (núcleo religado) não vira uso negativo.
        assert_eq!(usage(b, a), None);
    }

    #[test]
    fn cpuinfo_mhz_x86_and_arm() {
        let x86 = "processor\t: 0\ncpu MHz\t\t: 3499.974\n\nprocessor\t: 1\ncpu MHz\t\t: 800.000\n";
        assert_eq!(parse_cpuinfo_mhz(x86), [(0, 3500), (1, 800)]);
        let arm = "processor\t: 0\nBogoMIPS\t: 108.00\nFeatures\t: fp asimd\n";
        assert!(parse_cpuinfo_mhz(arm).is_empty());
    }

    #[test]
    fn freq_from_cpufreq() {
        let fx = Fixture::new("cpufreq");
        fx.write(
            "sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq",
            "2400000\n",
        )
        .write(
            "sys/devices/system/cpu/cpu1/cpufreq/scaling_cur_freq",
            "3600000\n",
        )
        .write("proc/cpuinfo", "processor : 0\ncpu MHz : 100.0\n");
        let s = parse_stat(STAT).unwrap();
        let c = cpu_sample(fx.path(), None, &s);
        assert_eq!(c.freq_avg_mhz, Some(3000));
        assert_eq!(c.freq_max_mhz, Some(3600));
        assert_eq!(c.cores[0].freq_mhz, Some(2400));
        assert_eq!(c.usage_pct, None, "primeira amostra não tem uso");
    }

    #[test]
    fn vm_without_cpufreq_uses_cpuinfo() {
        let fx = Fixture::new("vm");
        fx.write(
            "proc/cpuinfo",
            "processor : 0\ncpu MHz : 2199.998\nprocessor : 1\ncpu MHz : 2199.998\n",
        );
        let c = cpu_sample(fx.path(), None, &parse_stat(STAT).unwrap());
        assert_eq!(c.freq_avg_mhz, Some(2200));
    }

    #[test]
    fn without_any_frequency_source() {
        let fx = Fixture::new("nofreq");
        let c = cpu_sample(fx.path(), None, &parse_stat(STAT).unwrap());
        assert_eq!((c.freq_avg_mhz, c.freq_max_mhz), (None, None));
        assert_eq!(c.cores.len(), 2);
    }
}
