//! Ficha técnica: o que não muda enquanto a aba está aberta. Lida uma vez.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemInfo {
    pub cpu_model: Option<String>,
    /// Núcleos físicos; `None` quando o `cpuinfo` não diz (ARM, algumas VMs).
    pub cores: Option<u32>,
    /// Threads online.
    pub threads: u32,
    pub ram_total: u64,
    pub kernel: Option<String>,
    pub distro: Option<String>,
    pub hostname: Option<String>,
}

pub fn system_info(root: &Path) -> SystemInfo {
    let read = |rel: &str| fs::read_to_string(root.join(rel)).ok();
    let cpuinfo = read("proc/cpuinfo").unwrap_or_default();
    let cpu = parse_cpuinfo(&cpuinfo);
    let threads = if cpu.threads > 0 {
        cpu.threads
    } else {
        // Sem `processor` no cpuinfo: conta as linhas `cpuN` do /proc/stat.
        read("proc/stat")
            .map(|s| {
                s.lines()
                    .filter(|l| {
                        l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(u8::is_ascii_digit)
                    })
                    .count() as u32
            })
            .unwrap_or(0)
    };
    SystemInfo {
        // ARM: o cpuinfo não tem `model name`; o modelo da placa vem do devicetree.
        cpu_model: cpu.model.or_else(|| {
            read("sys/firmware/devicetree/base/model").and_then(|m| clean(m.trim_end_matches('\0')))
        }),
        cores: cpu.cores,
        threads,
        ram_total: read("proc/meminfo")
            .and_then(|m| crate::mem::parse_meminfo(&m))
            .map_or(0, |m| m.total),
        kernel: read("proc/sys/kernel/osrelease").and_then(clean),
        distro: read("etc/os-release")
            .or_else(|| read("usr/lib/os-release"))
            .and_then(|t| parse_os_release(&t)),
        hostname: read("proc/sys/kernel/hostname").and_then(clean),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct CpuInfo {
    model: Option<String>,
    cores: Option<u32>,
    threads: u32,
}

fn parse_cpuinfo(text: &str) -> CpuInfo {
    let mut info = CpuInfo::default();
    let mut physical = None;
    let mut cores = HashSet::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "processor" => info.threads += 1,
            "model name" if info.model.is_none() => info.model = clean(value),
            "physical id" => physical = Some(value.to_string()),
            // Um par (soquete, núcleo) por núcleo físico; as threads do SMT repetem o par.
            "core id" => {
                cores.insert((physical.clone(), value.to_string()));
            }
            _ => {}
        }
    }
    info.cores = (!cores.is_empty()).then_some(cores.len() as u32);
    info
}

/// `PRETTY_NAME="Ubuntu 24.04.3 LTS"`; sem ele, `NAME` + `VERSION`.
fn parse_os_release(text: &str) -> Option<String> {
    let get = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|v| !v.is_empty())
    };
    get("PRETTY_NAME").or_else(|| {
        let name = get("NAME")?;
        Some(match get("VERSION") {
            Some(v) => format!("{name} {v}"),
            None => name,
        })
    })
}

/// Tira espaços repetidos ("Intel(R) Core(TM)  i7") e devolve `None` se vazio.
fn clean(s: impl AsRef<str>) -> Option<String> {
    let s = s.as_ref().split_whitespace().collect::<Vec<_>>().join(" ");
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    const CPUINFO_X86: &str = "\
processor\t: 0
model name\t: Intel(R) Core(TM) i7-7500U CPU @ 2.70GHz
physical id\t: 0
core id\t\t: 0

processor\t: 1
model name\t: Intel(R) Core(TM) i7-7500U CPU @ 2.70GHz
physical id\t: 0
core id\t\t: 1

processor\t: 2
model name\t: Intel(R) Core(TM) i7-7500U CPU @ 2.70GHz
physical id\t: 0
core id\t\t: 0

processor\t: 3
model name\t: Intel(R) Core(TM) i7-7500U CPU @ 2.70GHz
physical id\t: 0
core id\t\t: 1
";

    #[test]
    fn x86_with_smt() {
        let c = parse_cpuinfo(CPUINFO_X86);
        assert_eq!(
            c.model.as_deref(),
            Some("Intel(R) Core(TM) i7-7500U CPU @ 2.70GHz")
        );
        assert_eq!((c.cores, c.threads), (Some(2), 4));
    }

    #[test]
    fn two_sockets_count_cores_per_socket() {
        let text = "processor : 0\nphysical id : 0\ncore id : 0\nprocessor : 1\nphysical id : 1\ncore id : 0\n";
        assert_eq!(parse_cpuinfo(text).cores, Some(2));
    }

    #[test]
    fn arm_board_from_devicetree() {
        let fx = Fixture::new("arm");
        fx.write(
            "proc/cpuinfo",
            "processor\t: 0\nBogoMIPS\t: 108.00\n\nprocessor\t: 1\nBogoMIPS\t: 108.00\n",
        )
        .write(
            "sys/firmware/devicetree/base/model",
            "Raspberry Pi 4 Model B Rev 1.4\0",
        )
        .write("proc/meminfo", "MemTotal: 1000 kB\n");
        let i = system_info(fx.path());
        assert_eq!(
            i.cpu_model.as_deref(),
            Some("Raspberry Pi 4 Model B Rev 1.4")
        );
        assert_eq!((i.cores, i.threads), (None, 2));
        assert_eq!(i.ram_total, 1000 * 1024);
    }

    #[test]
    fn full_machine() {
        let fx = Fixture::new("info");
        fx.write("proc/cpuinfo", CPUINFO_X86)
            .write("proc/meminfo", "MemTotal: 16000000 kB\n")
            .write("proc/sys/kernel/osrelease", "6.8.0-45-generic\n")
            .write("proc/sys/kernel/hostname", "notebook\n")
            .write(
                "etc/os-release",
                "NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.3 LTS\"\n",
            );
        let i = system_info(fx.path());
        assert_eq!(i.kernel.as_deref(), Some("6.8.0-45-generic"));
        assert_eq!(i.distro.as_deref(), Some("Ubuntu 24.04.3 LTS"));
        assert_eq!(i.hostname.as_deref(), Some("notebook"));
        assert_eq!(i.threads, 4);
    }

    #[test]
    fn os_release_without_pretty_name() {
        assert_eq!(
            parse_os_release("NAME=Ubuntu\nVERSION='22.04 LTS (Jammy Jellyfish)'\n").as_deref(),
            Some("Ubuntu 22.04 LTS (Jammy Jellyfish)")
        );
        assert_eq!(parse_os_release("ID=ubuntu\n"), None);
    }

    #[test]
    fn empty_system_falls_back() {
        // Sem cpuinfo (container): threads pelo /proc/stat; o resto fica vazio.
        let fx = Fixture::new("emptyinfo");
        fx.write(
            "proc/stat",
            "cpu  1 2 3 4\ncpu0 1 2 3 4\ncpu1 1 2 3 4\nintr 0\n",
        );
        let i = system_info(fx.path());
        assert_eq!(i.threads, 2);
        assert_eq!((i.cpu_model, i.kernel, i.distro), (None, None, None));
    }
}
