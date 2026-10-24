//! NVIDIA com o driver proprietário: o driver não expõe nada no sysfs, então
//! os números vêm do `nvidia-smi`. Um processo de vida longa (`-lms 1000`)
//! enquanto o monitor roda: subir um por segundo custaria caro. A saída é lida
//! numa thread, como a do Docker no `scanner`, e o processo morre no `Drop`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use super::{GPU_HOT_C, Reading};
use crate::model::Temp;

/// Uma linha por GPU a cada intervalo; o `pci.bus_id` liga a linha à placa.
const QUERY: &str = "--query-gpu=pci.bus_id,utilization.gpu,memory.used,memory.total,\
temperature.gpu,power.draw,clocks.gr,clocks.max.gr";

const MIB: u64 = 1024 * 1024;

/// Onde o pacote `nvidia-utils` (e o instalador `.run`) põe o programa.
pub(super) fn find(root: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    ["usr/bin/nvidia-smi", "usr/local/bin/nvidia-smi"]
        .iter()
        .map(|p| root.join(p))
        .find(|p| {
            p.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// `00000000:01:00.0, 35, 512, 6144, 45, 12.34, 1395, 2100`. Campo sem valor
/// (`[N/A]`, `[Not Supported]`) vira `None`.
pub(super) fn parse_line(line: &str) -> Option<(String, Reading)> {
    let f: Vec<&str> = line.split(',').map(str::trim).collect();
    if f.len() < 8 {
        return None;
    }
    let num = |i: usize| f[i].parse::<f64>().ok().filter(|v| v.is_finite());
    let reading = Reading {
        busy_pct: num(1).map(|v| v.clamp(0.0, 100.0) as f32),
        vram_used: num(2).map(|v| v as u64 * MIB),
        vram_total: num(3).map(|v| v as u64 * MIB),
        temp: num(4).map(|c| Temp {
            celsius: c as f32,
            hot_c: GPU_HOT_C,
            approximate: false,
        }),
        power_w: num(5),
        freq_mhz: num(6).map(|v| v as u32),
        freq_max_mhz: num(7).map(|v| v as u32),
        ..Reading::default()
    };
    Some((bus_key(f[0])?, reading))
}

/// O `nvidia-smi` escreve o domínio com 8 dígitos (`00000000:01:00.0`) e o
/// sysfs com 4 (`0000:01:00.0`): compara pelos 4 últimos, em minúsculas.
pub(super) fn bus_key(bus: &str) -> Option<String> {
    let (domain, rest) = bus.trim().split_once(':')?;
    let domain = domain.get(domain.len().checked_sub(4)?..)?;
    Some(format!("{domain}:{rest}").to_ascii_lowercase())
}

#[derive(Default)]
struct Shared {
    readings: HashMap<String, Reading>,
    /// A saída fechou sem um `kill` nosso: o driver não responde.
    ended: bool,
}

pub(super) struct Smi {
    child: Child,
    reader: Option<JoinHandle<()>>,
    shared: Arc<Mutex<Shared>>,
}

impl Smi {
    pub fn spawn(program: &Path) -> Option<Self> {
        let mut child = Command::new(program)
            .args([QUERY, "--format=csv,noheader,nounits", "-lms", "1000"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdout = child.stdout.take()?;
        let shared = Arc::new(Mutex::new(Shared::default()));
        let sink = Arc::clone(&shared);
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some((bus, r)) = parse_line(&line) {
                    lock(&sink).readings.insert(bus, r);
                }
            }
            lock(&sink).ended = true;
        });
        Some(Self {
            child,
            reader: Some(reader),
            shared,
        })
    }

    /// A última leitura da placa no `slot` (`0000:01:00.0`).
    pub fn reading(&self, slot: &str) -> Option<Reading> {
        lock(&self.shared).readings.get(&bus_key(slot)?).cloned()
    }

    pub fn ended(&self) -> bool {
        lock(&self.shared).ended
    }

    #[cfg(test)]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for Smi {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Com o processo morto, o pipe fecha e a thread termina.
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
    }
}

fn lock(m: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
pub(super) mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::fixture::Fixture;

    #[test]
    fn csv_line() {
        let (bus, r) =
            parse_line("00000000:01:00.0, 35, 512, 6144, 45, 12.34, 1395, 2100").unwrap();
        assert_eq!(bus, "0000:01:00.0");
        assert_eq!(r.busy_pct, Some(35.0));
        assert_eq!(r.vram_used, Some(512 * MIB));
        assert_eq!(r.vram_total, Some(6144 * MIB));
        assert_eq!(r.temp.unwrap().celsius, 45.0);
        assert_eq!(r.power_w, Some(12.34));
        assert_eq!((r.freq_mhz, r.freq_max_mhz), (Some(1395), Some(2100)));
    }

    #[test]
    fn missing_fields() {
        // Placa de notebook sem medidor de potência; driver antigo sem clocks.max.
        let (_, r) =
            parse_line("00000000:02:00.0, 0, 5, 4096, 40, [N/A], 300, [Not Supported]").unwrap();
        assert_eq!(r.power_w, None);
        assert_eq!(r.freq_max_mhz, None);
        assert_eq!(r.freq_mhz, Some(300));
        assert!(parse_line("No devices were found").is_none());
    }

    #[test]
    fn bus_keys_match_sysfs() {
        assert_eq!(bus_key("00000000:0A:00.0").as_deref(), Some("0000:0a:00.0"));
        assert_eq!(bus_key("0000:0a:00.0").as_deref(), Some("0000:0a:00.0"));
        assert_eq!(bus_key("sem"), None);
    }

    /// `nvidia-smi` falso: duas GPUs, depois fica parado como o `-lms`.
    pub(crate) fn fake_smi(fx: &Fixture) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        fx.write(
            "usr/bin/nvidia-smi",
            "#!/bin/sh\n\
             echo '00000000:01:00.0, 35, 512, 6144, 45, 12.34, 1395, 2100'\n\
             echo '00000000:02:00.0, 0, 5, 4096, 40, [N/A], 300, 1500'\n\
             exec sleep 60\n",
        );
        let path = fx.path().join("usr/bin/nvidia-smi");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        wait_executable(&path);
        path
    }

    /// Um `fork` de outro teste logo depois do `write` herda o arquivo aberto
    /// para escrita, e o `exec` falha com ETXTBSY até ele fechar.
    pub(crate) fn wait_executable(path: &Path) {
        wait_for(|| match Command::new(path).stdout(Stdio::null()).spawn() {
            Ok(mut c) => {
                let _ = c.kill();
                let _ = c.wait();
                true
            }
            Err(e) if e.raw_os_error() == Some(26) => false,
            Err(e) => panic!("{e}"),
        });
    }

    pub(crate) fn wait_for(mut ok: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ok() {
            assert!(Instant::now() < deadline, "tempo esgotado");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn long_lived_process_dies_on_drop() {
        let fx = Fixture::new("smi");
        let program = fake_smi(&fx);
        assert_eq!(find(fx.path()).as_deref(), Some(program.as_path()));

        let smi = Smi::spawn(&program).unwrap();
        wait_for(|| smi.reading("0000:02:00.0").is_some());
        assert_eq!(smi.reading("0000:01:00.0").unwrap().busy_pct, Some(35.0));
        assert!(!smi.ended());

        let proc_dir = PathBuf::from(format!("/proc/{}", smi.pid()));
        assert!(proc_dir.exists());
        drop(smi);
        assert!(!proc_dir.exists(), "nvidia-smi continuou rodando");
    }

    #[test]
    fn process_that_exits_is_ended() {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fixture::new("smi-fail");
        // "NVIDIA-SMI has failed because it couldn't communicate with the NVIDIA driver."
        fx.write(
            "usr/bin/nvidia-smi",
            "#!/bin/sh\necho 'NVIDIA-SMI has failed'\nexit 9\n",
        );
        let path = fx.path().join("usr/bin/nvidia-smi");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        wait_executable(&path);
        let smi = Smi::spawn(&path).unwrap();
        wait_for(|| smi.ended());
        assert!(smi.reading("0000:01:00.0").is_none());
    }

    #[test]
    fn not_executable_is_not_found() {
        let fx = Fixture::new("smi-noexec");
        fx.write("usr/bin/nvidia-smi", "texto");
        assert!(find(fx.path()).is_none());
    }
}
