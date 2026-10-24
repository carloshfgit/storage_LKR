//! Junta os coletores numa [`Sample`] e calcula as taxas contra a anterior.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::battery;
use crate::cpu::{self, CpuStat};
use crate::disk_io::{self, DiskMeta, DiskStat, SECTOR};
use crate::gpu::Gpus;
use crate::mem;
use crate::model::{DiskIo, NetIface, NetSample, Sample};
use crate::net::{self, IfaceStat, counter_delta};
use crate::procs::Procs;
use crate::sensors::{self, TempSensor};

/// O tempo restante da bateria só aparece depois disso: a potência das
/// primeiras leituras oscila (o firmware atualiza a cada poucos segundos).
const BATTERY_SETTLE: Duration = Duration::from_secs(10);

/// Os processos são lidos a cada tantas amostras (a primeira inclusive): é a
/// leitura mais cara, e a lista não precisa mudar a cada segundo.
pub const PROCS_EVERY: u64 = 2;

/// Pastas de `.desktop` do sistema, relativas à raiz (as do usuário entram em
/// [`Sampler::system`]).
const APP_DIRS: [&str; 4] = [
    "usr/local/share/applications",
    "usr/share/applications",
    "var/lib/snapd/desktop/applications",
    "var/lib/flatpak/exports/share/applications",
];

/// Contadores da amostra anterior.
struct Prev {
    at: Instant,
    cpu: CpuStat,
    disks: HashMap<String, DiskStat>,
    net: HashMap<String, IfaceStat>,
}

pub struct Sampler {
    root: PathBuf,
    start: Instant,
    prev: Option<Prev>,
    /// Resultado de `disk_meta` por nome (`None` = não é disco físico), para
    /// não reler o sysfs a cada amostra. Sai do mapa quando o nome some do
    /// `diskstats` (pendrive removido), e é relido se voltar.
    disk_meta: HashMap<String, Option<DiskMeta>>,
    /// Mesma ideia para as interfaces: `Some(wireless)` se for física.
    iface_meta: HashMap<String, Option<bool>>,
    /// Sensor de temperatura da CPU, procurado na primeira amostra.
    cpu_sensor: Option<Option<TempSensor>>,
    /// Placas de vídeo, procuradas na primeira amostra. Com uma NVIDIA, guarda
    /// o `nvidia-smi`, que morre junto com o `Sampler`.
    gpus: Option<Gpus>,
    procs: Procs,
    /// Amostras feitas, para ler os processos a cada [`PROCS_EVERY`].
    ticks: u64,
    /// Buffer reaproveitado para os arquivos do `/proc` (ver `read_into`).
    buf: Vec<u8>,
    rx_session: u64,
    tx_session: u64,
}

impl Sampler {
    /// `root` é a raiz onde estão `proc/` e `sys/`: `/` no sistema real.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root: PathBuf = root.into();
        let app_dirs = APP_DIRS.iter().map(|d| root.join(d)).collect();
        Self::with_app_dirs(root, app_dirs)
    }

    /// No sistema real, os `.desktop` do usuário vêm antes dos do sistema.
    pub fn system() -> Self {
        let mut dirs = Vec::new();
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
        if let Some(d) = data_home {
            dirs.push(d.join("applications"));
        }
        dirs.extend(APP_DIRS.iter().map(|d| Path::new("/").join(d)));
        Self::with_app_dirs("/".into(), dirs)
    }

    fn with_app_dirs(root: PathBuf, app_dirs: Vec<PathBuf>) -> Self {
        Self {
            procs: Procs::new(&root, app_dirs),
            ticks: 0,
            root,
            start: Instant::now(),
            prev: None,
            disk_meta: HashMap::new(),
            iface_meta: HashMap::new(),
            cpu_sensor: None,
            gpus: None,
            buf: vec![0; 16 * 1024],
            rx_session: 0,
            tx_session: 0,
        }
    }

    /// Uma amostra agora. Só falha sem `/proc/stat` ou `/proc/meminfo`; o
    /// resto, se faltar, vem vazio.
    pub fn sample(&mut self) -> io::Result<Sample> {
        self.sample_at(Instant::now())
    }

    /// `now` à parte para os testes controlarem o tempo entre as amostras.
    pub(crate) fn sample_at(&mut self, now: Instant) -> io::Result<Sample> {
        let root = self.root.clone();
        let buf = &mut self.buf;
        let cpu_stat = cpu::parse_stat(read_into(&root, "proc/stat", buf)?)
            .ok_or_else(|| invalid("proc/stat"))?;
        let meminfo = mem::parse_meminfo(read_into(&root, "proc/meminfo", buf)?)
            .ok_or_else(|| invalid("proc/meminfo"))?;
        let diskstats = read_into(&root, "proc/diskstats", buf)
            .map(disk_io::parse_diskstats)
            .unwrap_or_default();
        let net_dev = read_into(&root, "proc/net/dev", buf)
            .map(net::parse_net_dev)
            .unwrap_or_default();
        let uptime_s = read_into(&root, "proc/uptime", buf)
            .ok()
            .and_then(|t| t.split_ascii_whitespace().next()?.parse::<f64>().ok())
            .map_or(0, |s| s as u64);

        let prev = self.prev.as_ref();
        // Tempo real decorrido, não o intervalo nominal: um tick atrasado não
        // vira um pico.
        let dt = prev
            .map(|p| now.saturating_duration_since(p.at).as_secs_f64())
            .filter(|&dt| dt > 0.0);

        let mut cpu = cpu::cpu_sample(&root, prev.map(|p| &p.cpu), &cpu_stat);
        cpu.temp = self
            .cpu_sensor
            .get_or_insert_with(|| sensors::cpu_sensor(&root))
            .as_ref()
            .and_then(TempSensor::read);
        let mem = mem::mem_sample(&root, meminfo);

        self.disk_meta
            .retain(|name, _| diskstats.iter().any(|d| &d.name == name));
        let mut disks = Vec::new();
        let mut disk_now = HashMap::new();
        for stat in diskstats {
            let meta = self
                .disk_meta
                .entry(stat.name.clone())
                .or_insert_with(|| disk_io::disk_meta(&root, &stat.name));
            let Some(meta) = meta else {
                continue;
            };
            let before = prev.and_then(|p| p.disks.get(&stat.name));
            let rate = |f: fn(&DiskStat) -> u64, scale: f64| {
                Some(counter_delta(f(before?), f(&stat)) as f64 * scale / dt?)
            };
            disks.push(DiskIo {
                name: stat.name.clone(),
                model: meta.model.clone(),
                kind: meta.kind,
                rotational: meta.rotational,
                read_bps: rate(|d| d.read_sectors, SECTOR as f64),
                write_bps: rate(|d| d.write_sectors, SECTOR as f64),
                busy_pct: rate(|d| d.io_ms, 0.1).map(|p| p.min(100.0) as f32),
                temp: meta.sensor.as_ref().and_then(TempSensor::read),
                temp_hint: meta.temp_hint.clone(),
            });
            disk_now.insert(stat.name.clone(), stat);
        }

        let mut interfaces = Vec::new();
        let mut net_now = HashMap::new();
        let (mut rx_total, mut tx_total) = (0u64, 0u64);
        self.iface_meta
            .retain(|name, _| net_dev.iter().any(|i| &i.name == name));
        for stat in net_dev {
            let meta = self.iface_meta.entry(stat.name.clone()).or_insert_with(|| {
                net::is_physical(&root, &stat.name).then(|| net::is_wireless(&root, &stat.name))
            });
            let Some(wireless) = *meta else {
                continue;
            };
            let delta = prev
                .and_then(|p| p.net.get(&stat.name))
                .map(|b| (counter_delta(b.rx, stat.rx), counter_delta(b.tx, stat.tx)));
            if let Some((rx, tx)) = delta {
                rx_total += rx;
                tx_total += tx;
            }
            interfaces.push(NetIface {
                name: stat.name.clone(),
                wireless,
                rx_bps: delta.and_then(|(rx, _)| Some(rx as f64 / dt?)),
                tx_bps: delta.and_then(|(_, tx)| Some(tx as f64 / dt?)),
            });
            net_now.insert(stat.name.clone(), stat);
        }
        self.rx_session += rx_total;
        self.tx_session += tx_total;
        let net = NetSample {
            rx_bps: dt.map(|dt| rx_total as f64 / dt),
            tx_bps: dt.map(|dt| tx_total as f64 / dt),
            rx_session: self.rx_session,
            tx_session: self.tx_session,
            interfaces,
        };

        self.prev = Some(Prev {
            at: now,
            cpu: cpu_stat,
            disks: disk_now,
            net: net_now,
        });
        let batteries = battery::batteries(&root);
        let settled = now.saturating_duration_since(self.start) >= BATTERY_SETTLE;
        let battery_total = battery::total(&batteries, settled);
        let gpus = self
            .gpus
            .get_or_insert_with(|| Gpus::discover(&root))
            .sample(now);
        let procs = self
            .ticks
            .is_multiple_of(PROCS_EVERY)
            .then(|| self.procs.read(now));
        self.ticks += 1;

        Ok(Sample {
            at_ms: now.saturating_duration_since(self.start).as_millis() as u64,
            uptime_s,
            cpu,
            mem,
            disks,
            net,
            batteries,
            battery_total,
            gpus,
            procs,
        })
    }
}

/// Lê `rel` inteiro para `buf` e devolve o texto. O procfs informa tamanho 0,
/// então o `fs::read_to_string` começa com 32 bytes e faz uma leitura por
/// pedaço (e cada leitura do `/proc/stat` custa caro); aqui é uma só, num
/// buffer que cresce se precisar e é reaproveitado entre as amostras.
fn read_into<'a>(root: &Path, rel: &str, buf: &'a mut Vec<u8>) -> io::Result<&'a str> {
    read_path_into(&root.join(rel), buf).map_err(|e| {
        if e.kind() == io::ErrorKind::InvalidData {
            invalid(rel)
        } else {
            e
        }
    })
}

/// O mesmo que [`read_into`], com o caminho inteiro.
fn read_path_into<'a>(path: &Path, buf: &'a mut Vec<u8>) -> io::Result<&'a str> {
    let bytes = read_bytes_into(path, buf)?;
    std::str::from_utf8(bytes).map_err(|_| io::ErrorKind::InvalidData.into())
}

fn read_bytes_into<'a>(path: &Path, buf: &'a mut Vec<u8>) -> io::Result<&'a [u8]> {
    let mut file = File::open(path)?;
    let mut len = 0;
    loop {
        if len == buf.len() {
            buf.resize(buf.len() * 2, 0);
        }
        match file.read(&mut buf[len..]) {
            Ok(0) => break,
            Ok(n) => len += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(&buf[..len])
}

fn invalid(rel: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("/{rel} em formato inesperado"),
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::fixture::Fixture;
    use crate::model::DiskKind;

    fn machine() -> Fixture {
        let fx = Fixture::new("sampler");
        fx.write("proc/stat", "cpu  100 0 100 800 0 0 0 0\ncpu0 50 0 50 400 0 0 0 0\ncpu1 50 0 50 400 0 0 0 0\n")
            .write("proc/meminfo", "MemTotal: 1000 kB\nMemAvailable: 250 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n")
            .write("proc/diskstats", "   8       0 sda 0 0 1000 0 0 0 2000 0 0 100 0\n   7       0 loop0 0 0 9 0 0 0 9 0 0 9 0\n")
            .write("proc/net/dev", "h\nh\n    lo: 50 0 0 0 0 0 0 0 50 0 0 0 0 0 0 0\nenp3s0: 1000 0 0 0 0 0 0 0 500 0 0 0 0 0 0 0\nwlp2s0: 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n")
            .write("proc/uptime", "12345.67 40000.00\n")
            .write("sys/block/sda/device/model", "Disco\n")
            .dir("sys/block/loop0")
            .dir("sys/class/net/enp3s0/device")
            .dir("sys/class/net/wlp2s0/device")
            .dir("sys/class/net/lo");
        fx
    }

    #[test]
    fn first_sample_has_no_rates() {
        let fx = machine();
        let s = Sampler::new(fx.path()).sample().unwrap();
        assert_eq!(s.cpu.usage_pct, None);
        assert_eq!(s.mem.used, 750 * 1024);
        assert_eq!(s.uptime_s, 12345);
        assert_eq!(s.disks.len(), 1);
        assert_eq!(s.disks[0].kind, DiskKind::Sata);
        assert_eq!(s.disks[0].read_bps, None);
        assert_eq!(s.net.rx_bps, None);
        assert_eq!(s.net.interfaces.len(), 2);
        assert_eq!((s.net.rx_session, s.net.tx_session), (0, 0));
    }

    #[test]
    fn rates_use_elapsed_time() {
        let fx = machine();
        let mut sampler = Sampler::new(fx.path());
        let t0 = Instant::now();
        sampler.sample_at(t0).unwrap();

        // 2 s depois: CPU com 50% de uso, sda leu 4000 setores e gravou 2000,
        // enp3s0 recebeu 4000 B e enviou 1000 B, wlp2s0 recebeu 2000 B.
        fx.write("proc/stat", "cpu  150 0 150 900 0 0 0 0\ncpu0 100 0 100 400 0 0 0 0\ncpu1 50 0 50 500 0 0 0 0\n")
            .write("proc/diskstats", "   8       0 sda 0 0 5000 0 0 0 4000 0 0 1100 0\n")
            .write("proc/net/dev", "h\nh\nenp3s0: 5000 0 0 0 0 0 0 0 1500 0 0 0 0 0 0 0\nwlp2s0: 2000 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n");
        let s = sampler.sample_at(t0 + Duration::from_secs(2)).unwrap();

        assert_eq!(s.cpu.usage_pct, Some(50.0));
        assert_eq!(s.cpu.cores[0].usage_pct, Some(100.0));
        assert_eq!(s.cpu.cores[1].usage_pct, Some(0.0));
        assert_eq!(s.disks[0].read_bps, Some(4000.0 * 512.0 / 2.0));
        assert_eq!(s.disks[0].write_bps, Some(2000.0 * 512.0 / 2.0));
        assert_eq!(s.disks[0].busy_pct, Some(50.0));
        assert_eq!(s.net.rx_bps, Some(3000.0));
        assert_eq!(s.net.tx_bps, Some(500.0));
        assert_eq!((s.net.rx_session, s.net.tx_session), (6000, 1000));
        assert!(s.at_ms >= 2000);
    }

    #[test]
    fn reset_counter_and_new_interface_do_not_spike() {
        let fx = machine();
        let mut sampler = Sampler::new(fx.path());
        let t0 = Instant::now();
        sampler.sample_at(t0).unwrap();

        // enp3s0 caiu e voltou (contador zerado); um pendrive (sdb) apareceu.
        fx.write("proc/net/dev", "h\nh\nenp3s0: 10 0 0 0 0 0 0 0 10 0 0 0 0 0 0 0\n")
            .write("proc/diskstats", "   8       0 sda 0 0 1000 0 0 0 2000 0 0 100 0\n   8      16 sdb 0 0 64 0 0 0 0 0 0 1 0\n")
            .dir("sys/block/sdb/device");
        let s = sampler.sample_at(t0 + Duration::from_secs(1)).unwrap();
        assert_eq!(s.net.rx_bps, Some(0.0));
        assert_eq!(s.net.rx_session, 0);
        assert_eq!(s.disks.len(), 2);
        assert_eq!(s.disks[1].read_bps, None, "disco novo ainda sem anterior");
    }

    #[test]
    fn processes_every_other_sample() {
        let fx = machine();
        fx.write(
            "proc/1/stat",
            "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 5 5 0 0 20 0 1 0 10 1000 300 0\n",
        );
        let mut sampler = Sampler::new(fx.path());
        let t0 = Instant::now();
        let got: Vec<bool> = (0..5)
            .map(|s| {
                let sample = sampler.sample_at(t0 + Duration::from_secs(s)).unwrap();
                sample.procs.is_some()
            })
            .collect();
        assert_eq!(got, [true, false, true, false, true]);
    }

    #[test]
    fn missing_optional_files_are_empty() {
        // Container ou sistema mínimo: só stat e meminfo.
        let fx = Fixture::new("minimal");
        fx.write("proc/stat", "cpu 1 0 1 8\n")
            .write("proc/meminfo", "MemTotal: 1000 kB\nMemAvailable: 500 kB\n");
        let s = Sampler::new(fx.path()).sample().unwrap();
        assert!(s.disks.is_empty());
        assert!(s.net.interfaces.is_empty());
        assert!(s.cpu.cores.is_empty());
        assert_eq!(s.uptime_s, 0);
    }

    #[test]
    fn without_proc_stat_is_an_error() {
        let fx = Fixture::new("noproc");
        assert!(Sampler::new(fx.path()).sample().is_err());
    }

    #[test]
    fn real_system_sample() {
        let mut sampler = Sampler::system();
        let a = sampler.sample().unwrap();
        assert!(a.mem.total > 0);
        assert!(!a.cpu.cores.is_empty());
        std::thread::sleep(Duration::from_millis(50));
        let b = sampler.sample().unwrap();
        assert!(b.at_ms > a.at_ms);
    }
}
