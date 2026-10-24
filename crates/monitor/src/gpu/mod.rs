//! GPUs (DECISIONS D49): uma por placa em `/sys/class/drm/card*`, cada uma com
//! o que o driver dela expõe e, no resto, o motivo de faltar.
//!
//! Uma placa em repouso (`power/runtime_status = suspended`, comum na dedicada
//! de notebook) não é lida: os arquivos de uso, memória, temperatura e consumo
//! a acordariam (kernel 5.15) ou falham com EBUSY (kernels novos). A
//! descoberta lê só o que não acorda (conferido nesta máquina com a Radeon em
//! D3cold): `uevent`, `power/*`, a lista e os rótulos do hwmon, os limites de
//! temperatura e `mem_info_vram_total`. O nome vem do `pci.ids`, sem `lspci`.

mod amd;
mod intel;
mod nvidia;
mod pci_ids;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::model::{GpuNote, GpuSample, GpuSpec, GpuState, GpuVendor, Temp};
use crate::sensors::{self, TempSensor};

/// Limiar de alerta quando o sensor não informa o próprio.
pub(crate) const GPU_HOT_C: f32 = 85.0;

/// Consumo acima do limite da placa (`power1_cap_max`, ou `power1_cap`) com
/// esta folga é defeito do sensor: a Radeon R7 M445 desta máquina, com limite
/// de 30 W, informa 572 W e 1.088 W parada. Sem limite, vale o teto fixo.
const POWER_CAP_SLACK: f64 = 1.25;
const MAX_POWER_W: f64 = 1000.0;

/// Drivers DRM que não são GPU: o framebuffer do firmware (antes do driver de
/// verdade, ou no lugar dele) e os monitores USB.
const NOT_GPU: [&str; 10] = [
    "simple-framebuffer",
    "simpledrm",
    "efi-framebuffer",
    "efidrm",
    "vesa-framebuffer",
    "vesadrm",
    "ofdrm",
    "evdi",
    "udl",
    "gud",
];

/// GPUs de máquina virtual: nem integradas nem dedicadas.
const VIRTUAL: [&str; 8] = [
    "virtio_gpu",
    "vmwgfx",
    "qxl",
    "bochs-drm",
    "cirrus",
    "vboxvideo",
    "hyperv_drm",
    "xen",
];

/// Leituras seguidas sem uso antes da pausa (ver [`Rest`]).
const IDLE_READS: u32 = 3;
/// Sem `power/autosuspend_delay_ms`: o padrão do `amdgpu` e do `nouveau`.
const DEFAULT_AUTOSUSPEND: Duration = Duration::from_secs(5);
/// O kernel suspende um pouco depois do atraso (medido: ~6 s com 5 s).
const REST_MARGIN: Duration = Duration::from_secs(3);

/// Uma leitura dos números que mudam.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Reading {
    pub busy_pct: Option<f32>,
    pub vram_used: Option<u64>,
    /// Só a NVIDIA informa aqui (pelo `nvidia-smi`); a AMD, na descoberta.
    pub vram_total: Option<u64>,
    pub gtt_used: Option<u64>,
    pub gtt_total: Option<u64>,
    pub temp: Option<Temp>,
    pub power_w: Option<f64>,
    pub freq_mhz: Option<u32>,
    pub freq_max_mhz: Option<u32>,
}

enum Backend {
    Amd,
    Nvidia,
    Intel(Option<intel::Freq>),
    /// Só o hwmon, se houver: `nouveau`, `radeon`, GPUs virtuais e de ARM.
    Basic,
}

/// Arquivos do hwmon da placa, escolhidos na descoberta.
#[derive(Default)]
struct Hwmon {
    temp: Option<TempSensor>,
    /// µW.
    power: Option<PathBuf>,
    /// Acima disso (W), a leitura é descartada. Lido na primeira leitura com a
    /// placa ativa: no kernel 5.15, ler o `power1_cap` a acordaria.
    power_max: Option<f64>,
    /// Hz (`amdgpu`: `sclk`).
    freq: Option<PathBuf>,
    dir: PathBuf,
}

impl Hwmon {
    fn discover(dir: Option<&Path>) -> Self {
        let Some(dir) = dir else {
            return Self::default();
        };
        Self {
            // `amdgpu`: edge (temp1), junction (temp2) e memória (temp3).
            temp: sensors::pick(dir, &["edge"]).map(|n| sensors::sensor(dir, n, GPU_HOT_C, false)),
            power: ["power1_average", "power1_input"]
                .iter()
                .map(|f| dir.join(f))
                .find(|p| p.exists()),
            power_max: None,
            freq: Some(dir.join("freq1_input")).filter(|p| p.exists()),
            dir: dir.to_path_buf(),
        }
    }

    /// Só com a placa ativa.
    fn read_into(&mut self, r: &mut Reading) {
        if r.temp.is_none() {
            r.temp = self.temp.as_ref().and_then(TempSensor::read);
        }
        if r.power_w.is_none()
            && let Some(power) = &self.power
        {
            let dir = &self.dir;
            let max = *self.power_max.get_or_insert_with(|| {
                ["power1_cap_max", "power1_cap"]
                    .iter()
                    .find_map(|f| read_u64(&dir.join(f)).filter(|&uw| uw > 0))
                    .map_or(MAX_POWER_W, |uw| uw as f64 / 1e6 * POWER_CAP_SLACK)
            });
            r.power_w = read_u64(power)
                .map(|uw| uw as f64 / 1e6)
                .filter(|&w| w <= max);
        }
        if r.freq_mhz.is_none() {
            r.freq_mhz = self
                .freq
                .as_deref()
                .and_then(read_u64)
                .map(|hz| (hz / 1_000_000) as u32);
        }
    }
}

/// Pausa nas leituras de uma dedicada ociosa, para ela poder dormir.
///
/// Cada leitura do sysfs (e o `nvidia-smi` aberto) renova o atraso de
/// autosuspend da placa: lendo a cada segundo, ela nunca dormiria (medido: a
/// 1 Hz, a Radeon desta máquina ficou ativa o tempo todo; sem leituras, dormiu
/// ~6 s depois). Depois de [`IDLE_READS`] leituras sem uso, a coleta para por
/// `autosuspend + margem` e mostra a última leitura. Se a placa dormiu, fica
/// "em repouso"; se continua ativa, outra coisa a usa (um monitor ligado nela),
/// e a pausa é desligada até ela dormir, para o gráfico não ter buracos.
struct Rest {
    window: Duration,
    idle: u32,
    until: Option<Instant>,
    off: bool,
}

impl Rest {
    fn new(autosuspend: Duration) -> Self {
        Self {
            window: autosuspend + REST_MARGIN,
            idle: 0,
            until: None,
            off: false,
        }
    }

    fn should_read(&mut self, now: Instant) -> bool {
        match self.until {
            Some(t) if now < t => false,
            Some(_) => {
                self.until = None;
                self.off = true;
                true
            }
            None => true,
        }
    }

    fn after_read(&mut self, idle: bool, now: Instant) {
        if self.off {
            return;
        }
        self.idle = if idle { self.idle + 1 } else { 0 };
        if self.idle >= IDLE_READS {
            self.idle = 0;
            self.until = Some(now + self.window);
        }
    }

    /// A placa dormiu: da próxima vez que acordar, a pausa volta a valer.
    fn reset(&mut self) {
        self.idle = 0;
        self.until = None;
        self.off = false;
    }
}

pub(crate) struct Card {
    pub spec: GpuSpec,
    dev: PathBuf,
    /// `0000:01:00.0`; liga a placa à linha do `nvidia-smi`.
    slot: Option<String>,
    backend: Backend,
    hwmon: Hwmon,
    /// Só nas dedicadas com runtime PM (`power/control = auto`).
    rest: Option<Rest>,
    last: Reading,
}

impl Card {
    fn state(&self) -> GpuState {
        match fs::read_to_string(self.dev.join("power/runtime_status")) {
            Ok(s) if matches!(s.trim(), "suspended" | "suspending") => GpuState::Suspended,
            // Sem runtime PM (`unsupported`) ou sem o arquivo: sempre ligada.
            _ => GpuState::Active,
        }
    }

    fn should_read(&mut self, state: GpuState, now: Instant) -> bool {
        match state {
            GpuState::Suspended => {
                if let Some(r) = &mut self.rest {
                    r.reset();
                }
                self.last = Reading::default();
                false
            }
            GpuState::Active => self.rest.as_mut().is_none_or(|r| r.should_read(now)),
        }
    }

    fn read(&mut self, smi: Option<&nvidia::Smi>, now: Instant) {
        let mut r = match &self.backend {
            Backend::Amd => amd::read(&self.dev, self.spec.integrated == Some(true)),
            Backend::Intel(freq) => freq.as_ref().map(intel::Freq::read).unwrap_or_default(),
            Backend::Nvidia => {
                match smi
                    .zip(self.slot.as_deref())
                    .and_then(|(s, slot)| s.reading(slot))
                {
                    Some(r) => r,
                    // `nvidia-smi` ainda sem a primeira linha: não conta para a pausa.
                    None => {
                        self.last = Reading::default();
                        return;
                    }
                }
            }
            Backend::Basic => Reading::default(),
        };
        self.hwmon.read_into(&mut r);
        if self.spec.vram_total.is_none() {
            self.spec.vram_total = r.vram_total;
        }
        if let Some(rest) = &mut self.rest {
            rest.after_read(r.busy_pct.is_none_or(|b| b < 1.0), now);
        }
        self.last = r;
    }

    fn sample(&self, state: GpuState, note: Option<GpuNote>) -> GpuSample {
        let r = &self.last;
        GpuSample {
            spec: self.spec.clone(),
            state,
            busy_pct: r.busy_pct,
            vram_used: r.vram_used,
            gtt_used: r.gtt_used,
            gtt_total: r.gtt_total,
            temp: r.temp,
            power_w: r.power_w,
            freq_mhz: r.freq_mhz,
            freq_max_mhz: r.freq_max_mhz,
            note,
        }
    }
}

/// As placas, na ordem do número do `card`. Os conectores (`card0-HDMI-A-1`)
/// e os nós de render ficam de fora, e também o que não é GPU ([`NOT_GPU`]).
pub(crate) fn cards(root: &Path) -> Vec<Card> {
    let drm = root.join("sys/class/drm");
    let Ok(entries) = fs::read_dir(&drm) else {
        return Vec::new();
    };
    let mut found: Vec<(u32, String)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let n = name.strip_prefix("card")?.parse().ok()?;
            Some((n, name))
        })
        .collect();
    found.sort_unstable();

    let mut cards = Vec::new();
    let mut ids = Vec::new();
    for (_, name) in found {
        let card_dir = drm.join(&name);
        let dev = card_dir.join("device");
        let Ok(uevent) = fs::read_to_string(dev.join("uevent")) else {
            continue;
        };
        let field = |key: &str| {
            uevent
                .lines()
                .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        let driver = field("DRIVER").unwrap_or_default().to_string();
        if NOT_GPU.contains(&driver.as_str()) {
            continue;
        }
        let pci = field("PCI_ID").and_then(|id| pci_ids::parse_id(id, field("PCI_SUBSYS_ID")));
        let vendor = match pci.map(|p| p.vendor) {
            Some(0x1002) => GpuVendor::Amd,
            Some(0x10de) => GpuVendor::Nvidia,
            Some(0x8086) => GpuVendor::Intel,
            _ => GpuVendor::Other,
        };
        let hwmon_dir = first_hwmon(&dev);
        let backend = match driver.as_str() {
            "amdgpu" => Backend::Amd,
            "nvidia" => Backend::Nvidia,
            "i915" | "xe" => Backend::Intel(intel::Freq::discover(&card_dir, &dev, &driver)),
            _ => Backend::Basic,
        };
        let integrated = match driver.as_str() {
            "amdgpu" => Some(hwmon_dir.as_deref().is_some_and(amd::is_integrated)),
            // O `i915` e o `xe` só registram hwmon nas dedicadas (Arc).
            "i915" | "xe" => Some(hwmon_dir.is_none()),
            "nvidia" | "nouveau" => Some(false),
            d if VIRTUAL.contains(&d) => None,
            // Fora do PCI: a GPU do SoC (ARM).
            _ if pci.is_none() => Some(true),
            _ => None,
        };
        let power = dev.join("power");
        let runtime_pm =
            fs::read_to_string(power.join("control")).is_ok_and(|c| c.trim() == "auto");
        let rest = (runtime_pm
            && integrated != Some(true)
            && !matches!(backend, Backend::Intel(_)))
        .then(|| {
            Rest::new(
                read_u64(&power.join("autosuspend_delay_ms"))
                    .map_or(DEFAULT_AUTOSUSPEND, Duration::from_millis),
            )
        });
        if let Some(p) = pci {
            ids.push((cards.len(), vendor, p));
        }
        cards.push(Card {
            spec: GpuSpec {
                card: name,
                model: None,
                vendor,
                pci_id: pci.map(|p| format!("{:04x}:{:04x}", p.vendor, p.device)),
                integrated,
                vram_total: (driver == "amdgpu")
                    .then(|| read_u64(&dev.join("mem_info_vram_total")))
                    .flatten(),
                driver: (!driver.is_empty()).then_some(driver),
            },
            slot: field("PCI_SLOT_NAME").map(str::to_string),
            hwmon: Hwmon::discover(hwmon_dir.as_deref()),
            dev,
            backend,
            rest,
            last: Reading::default(),
        });
    }

    let wanted: Vec<_> = ids.iter().map(|&(_, v, id)| (v, id)).collect();
    for ((i, _, _), model) in ids.iter().zip(pci_ids::names(root, &wanted)) {
        cards[*i].spec.model = model;
    }
    cards
}

fn first_hwmon(dev: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(dev.join("hwmon"))
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("hwmon"))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs.into_iter().next()
}

pub(crate) fn read_u64(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// As placas do sistema e o `nvidia-smi`, quando há NVIDIA proprietária.
pub(crate) struct Gpus {
    cards: Vec<Card>,
    smi_program: Option<PathBuf>,
    smi: Option<nvidia::Smi>,
    /// O `nvidia-smi` não subiu ou saiu sozinho (driver sem resposta): não
    /// tenta de novo nesta sessão.
    smi_failed: bool,
}

impl Gpus {
    pub fn discover(root: &Path) -> Self {
        let cards = cards(root);
        let has_nvidia = cards.iter().any(|c| matches!(c.backend, Backend::Nvidia));
        Self {
            smi_program: has_nvidia.then(|| nvidia::find(root)).flatten(),
            cards,
            smi: None,
            smi_failed: false,
        }
    }

    pub fn sample(&mut self, now: Instant) -> Vec<GpuSample> {
        let states: Vec<GpuState> = self.cards.iter().map(Card::state).collect();
        let read: Vec<bool> = self
            .cards
            .iter_mut()
            .zip(&states)
            .map(|(card, &state)| card.should_read(state, now))
            .collect();
        self.manage_smi(&read);

        let note_nvidia = if self.smi_program.is_none() {
            Some(GpuNote::NoNvidiaSmi)
        } else if self.smi_failed {
            Some(GpuNote::NotExposed)
        } else {
            None
        };
        let smi = self.smi.as_ref();
        self.cards
            .iter_mut()
            .zip(states)
            .zip(read)
            .map(|((card, state), read)| {
                if read {
                    card.read(smi, now);
                }
                let note = match card.backend {
                    Backend::Amd => None,
                    Backend::Nvidia => note_nvidia,
                    Backend::Intel(_) => Some(GpuNote::NeedsRoot),
                    Backend::Basic => Some(GpuNote::NotExposed),
                };
                card.sample(state, note)
            })
            .collect()
    }

    /// O `nvidia-smi` roda só enquanto alguma NVIDIA vai ser lida: com todas
    /// em repouso ou em pausa, ele sai (o `Drop` mata), e a placa pode dormir.
    fn manage_smi(&mut self, read: &[bool]) {
        if self.smi.as_ref().is_some_and(nvidia::Smi::ended) {
            self.smi = None;
            self.smi_failed = true;
        }
        let wanted = self
            .cards
            .iter()
            .zip(read)
            .any(|(c, &r)| r && matches!(c.backend, Backend::Nvidia));
        if !wanted {
            self.smi = None;
        } else if self.smi.is_none()
            && !self.smi_failed
            && let Some(program) = &self.smi_program
        {
            self.smi = nvidia::Smi::spawn(program);
            self.smi_failed = self.smi.is_none();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    const PCI_IDS: &str = "\
1002  Advanced Micro Devices, Inc. [AMD/ATI]
\t15bf  Phoenix1
\t6900  Topaz XT [Radeon R7 M260/M265 / M340/M360 / M440/M445 / 530/535 / 620/625 Mobile]
\t\t1028 0767  Radeon R7 M445
\t73df  Navi 22 [Radeon RX 6700/6700 XT/6750 XT / 6800M/6850M XT]
10de  NVIDIA Corporation
\t2520  GA106M [GeForce RTX 3060 Mobile / Max-Q]
8086  Intel Corporation
\t5916  Kaby Lake-U GT2 [HD Graphics 620]
\t\t17aa 2248  ThinkPad T570
\ta7a0  Raptor Lake-P [Iris Xe Graphics]
";

    fn card<'a>(fx: &'a Fixture, n: u32, uevent: &str) -> &'a Fixture {
        fx.write(&format!("sys/class/drm/card{n}/device/uevent"), uevent)
    }

    fn dev(n: u32, rel: &str) -> String {
        format!("sys/class/drm/card{n}/device/{rel}")
    }

    /// A Radeon dedicada: arquivos do `amdgpu` e do hwmon dela.
    fn amdgpu(fx: &Fixture, n: u32, status: &str) {
        card(
            fx,
            n,
            "DRIVER=amdgpu\nPCI_ID=1002:6900\nPCI_SUBSYS_ID=1028:0767\nPCI_SLOT_NAME=0000:01:00.0\n",
        );
        fx.write(&dev(n, "power/runtime_status"), &format!("{status}\n"))
            .write(&dev(n, "power/control"), "auto\n")
            .write(&dev(n, "power/autosuspend_delay_ms"), "5000\n")
            .write(&dev(n, "gpu_busy_percent"), "37\n")
            .write(&dev(n, "mem_info_vram_used"), "1392640\n")
            .write(&dev(n, "mem_info_vram_total"), "4294967296\n")
            .write(&dev(n, "mem_info_gtt_used"), "7294976\n")
            .write(&dev(n, "mem_info_gtt_total"), "8050298880\n")
            .write(&dev(n, "hwmon/hwmon6/name"), "amdgpu\n")
            .write(&dev(n, "hwmon/hwmon6/temp1_input"), "42000\n")
            .write(&dev(n, "hwmon/hwmon6/temp1_label"), "edge\n")
            .write(&dev(n, "hwmon/hwmon6/temp1_crit"), "104000\n")
            .write(&dev(n, "hwmon/hwmon6/power1_average"), "8250000\n")
            .write(&dev(n, "hwmon/hwmon6/freq1_input"), "300000000\n")
            .write(&dev(n, "hwmon/hwmon6/in0_input"), "900\n");
    }

    fn i915(fx: &Fixture, n: u32) {
        card(
            fx,
            n,
            "DRIVER=i915\nPCI_ID=8086:5916\nPCI_SUBSYS_ID=1028:0767\nPCI_SLOT_NAME=0000:00:02.0\n",
        );
        let c = format!("sys/class/drm/card{n}");
        fx.write(&dev(n, "power/runtime_status"), "active\n")
            .write(&dev(n, "power/control"), "auto\n")
            .write(&format!("{c}/gt_act_freq_mhz"), "350\n")
            .write(&format!("{c}/gt_cur_freq_mhz"), "300\n")
            .write(&format!("{c}/gt_max_freq_mhz"), "1050\n")
            .write(&format!("{c}/gt_RP0_freq_mhz"), "1100\n")
            .dir(&format!("{c}-eDP-1"))
            .dir(&format!("{c}-HDMI-A-1"));
    }

    fn with_pci_ids(fx: &Fixture) {
        fx.write("usr/share/misc/pci.ids", PCI_IDS);
    }

    fn sample(fx: &Fixture) -> Vec<GpuSample> {
        Gpus::discover(fx.path()).sample(Instant::now())
    }

    #[test]
    fn hybrid_intel_and_suspended_radeon() {
        // Esta máquina: HD 620 ativa e a Radeon dedicada em D3cold.
        let fx = Fixture::new("gpu-hybrid");
        with_pci_ids(&fx);
        amdgpu(&fx, 0, "suspended");
        i915(&fx, 1);
        fx.dir("sys/class/drm/renderD128");
        let g = sample(&fx);
        assert_eq!(g.len(), 2, "conectores e render ficam de fora");

        let amd = &g[0];
        assert_eq!(amd.spec.card, "card0");
        assert_eq!(amd.spec.model.as_deref(), Some("AMD Radeon R7 M445"));
        assert_eq!(amd.spec.integrated, Some(false));
        assert_eq!(amd.spec.vram_total, Some(4294967296), "lido sem acordar");
        assert_eq!(amd.state, GpuState::Suspended);
        // Os arquivos existem com valores: vazio quer dizer que não foram lidos.
        assert_eq!(
            (
                amd.busy_pct,
                amd.vram_used,
                amd.temp,
                amd.power_w,
                amd.freq_mhz
            ),
            (None, None, None, None, None)
        );
        assert_eq!(amd.note, None);

        let intel = &g[1];
        assert_eq!(intel.spec.model.as_deref(), Some("Intel HD Graphics 620"));
        assert_eq!(intel.spec.integrated, Some(true));
        assert_eq!(intel.state, GpuState::Active);
        assert_eq!(
            (intel.freq_mhz, intel.freq_max_mhz),
            (Some(350), Some(1050))
        );
        assert_eq!(intel.busy_pct, None);
        assert_eq!(intel.note, Some(GpuNote::NeedsRoot));
    }

    #[test]
    fn active_dedicated_radeon() {
        let fx = Fixture::new("gpu-amd");
        amdgpu(&fx, 0, "active");
        fx.write(&dev(0, "hwmon/hwmon6/temp2_input"), "45000\n")
            .write(&dev(0, "hwmon/hwmon6/temp2_label"), "junction\n");
        let g = &sample(&fx)[0];
        assert_eq!(g.state, GpuState::Active);
        assert_eq!(g.busy_pct, Some(37.0));
        assert_eq!(g.vram_used, Some(1392640));
        assert_eq!(g.spec.vram_total, Some(4294967296));
        assert_eq!(
            (g.gtt_used, g.gtt_total),
            (None, None),
            "GTT só nas integradas"
        );
        let t = g.temp.unwrap();
        assert_eq!(t.celsius, 42.0, "edge, não junction");
        assert_eq!(t.hot_c, GPU_HOT_C, "crit de 104 °C não sobe o limiar");
        assert_eq!(g.power_w, Some(8.25));
        assert_eq!(g.freq_mhz, Some(300));
        assert_eq!(g.spec.model, None, "sem pci.ids");
        assert_eq!(g.spec.pci_id.as_deref(), Some("1002:6900"));
    }

    #[test]
    fn implausible_power_is_dropped() {
        let fx = Fixture::new("gpu-power");
        amdgpu(&fx, 0, "active");
        fx.write(&dev(0, "hwmon/hwmon6/power1_average"), "1088301664\n");
        assert_eq!(sample(&fx)[0].power_w, None, "acima do teto fixo");
        // Com o limite da placa (30 W): 572 W é lixo; 31 W ainda vale (picos).
        fx.write(&dev(0, "hwmon/hwmon6/power1_cap"), "30000000\n")
            .write(&dev(0, "hwmon/hwmon6/power1_average"), "572153664\n");
        assert_eq!(sample(&fx)[0].power_w, None, "acima do limite da placa");
        fx.write(&dev(0, "hwmon/hwmon6/power1_average"), "31000000\n");
        assert_eq!(sample(&fx)[0].power_w, Some(31.0));
        // Kernels novos: power1_input no lugar do average.
        let fx = Fixture::new("gpu-power-input");
        amdgpu(&fx, 0, "active");
        fs::remove_file(fx.path().join(dev(0, "hwmon/hwmon6/power1_average"))).unwrap();
        fx.write(&dev(0, "hwmon/hwmon6/power1_input"), "15000000\n");
        assert_eq!(sample(&fx)[0].power_w, Some(15.0));
    }

    #[test]
    fn integrated_apu_uses_gtt() {
        let fx = Fixture::new("gpu-apu");
        with_pci_ids(&fx);
        card(
            &fx,
            0,
            "DRIVER=amdgpu\nPCI_ID=1002:15BF\nPCI_SLOT_NAME=0000:c4:00.0\n",
        );
        fx.write(&dev(0, "gpu_busy_percent"), "5\n")
            .write(&dev(0, "mem_info_vram_used"), "100000000\n")
            .write(&dev(0, "mem_info_vram_total"), "536870912\n")
            .write(&dev(0, "mem_info_gtt_used"), "900000000\n")
            .write(&dev(0, "mem_info_gtt_total"), "16000000000\n")
            .write(&dev(0, "power/control"), "auto\n")
            .write(&dev(0, "hwmon/hwmon3/temp1_input"), "51000\n")
            .write(&dev(0, "hwmon/hwmon3/in1_input"), "1000\n")
            .write(&dev(0, "hwmon/hwmon3/in1_label"), "vddnb\n");
        let mut gpus = Gpus::discover(fx.path());
        assert!(gpus.cards[0].rest.is_none(), "integrada não pausa");
        let g = &gpus.sample(Instant::now())[0];
        assert_eq!(g.spec.integrated, Some(true));
        assert_eq!(g.spec.model.as_deref(), Some("AMD Phoenix1"));
        assert_eq!(
            (g.gtt_used, g.gtt_total),
            (Some(900000000), Some(16000000000))
        );
        assert_eq!(g.temp.unwrap().celsius, 51.0);
    }

    #[test]
    fn idle_dedicated_rests_so_it_can_sleep() {
        let fx = Fixture::new("gpu-rest");
        amdgpu(&fx, 0, "active");
        fx.write(&dev(0, "gpu_busy_percent"), "0\n");
        let mut gpus = Gpus::discover(fx.path());
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        for s in 0..3 {
            assert_eq!(gpus.sample(at(s))[0].busy_pct, Some(0.0));
        }
        // Três leituras sem uso: pausa de 5 + 3 s, com a última leitura na tela.
        fx.write(&dev(0, "gpu_busy_percent"), "50\n");
        assert_eq!(
            gpus.sample(at(3))[0].busy_pct,
            Some(0.0),
            "leu durante a pausa"
        );
        assert_eq!(gpus.sample(at(9))[0].busy_pct, Some(0.0));

        // Dormiu durante a pausa: em repouso, sem ler.
        fx.write(&dev(0, "power/runtime_status"), "suspended\n");
        let g = &gpus.sample(at(10))[0];
        assert_eq!((g.state, g.busy_pct), (GpuState::Suspended, None));

        // Acordou de novo (um jogo abriu): volta a ler na hora.
        fx.write(&dev(0, "power/runtime_status"), "active\n");
        assert_eq!(gpus.sample(at(11))[0].busy_pct, Some(50.0));
    }

    #[test]
    fn rest_turns_off_when_something_keeps_it_awake() {
        let fx = Fixture::new("gpu-awake");
        amdgpu(&fx, 0, "active");
        fx.write(&dev(0, "gpu_busy_percent"), "0\n");
        let mut gpus = Gpus::discover(fx.path());
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        for s in 0..3 {
            gpus.sample(at(s));
        }
        // Depois da pausa, ainda ativa: monitor ligado nela. Lê sem parar.
        fx.write(&dev(0, "gpu_busy_percent"), "1\n");
        assert_eq!(gpus.sample(at(10))[0].busy_pct, Some(1.0));
        for s in 11..20 {
            fx.write(&dev(0, "gpu_busy_percent"), &format!("{}\n", s % 2));
            assert_eq!(gpus.sample(at(s))[0].busy_pct, Some((s % 2) as f32));
        }
    }

    #[test]
    fn busy_dedicated_never_rests() {
        let fx = Fixture::new("gpu-busy");
        amdgpu(&fx, 0, "active");
        let mut gpus = Gpus::discover(fx.path());
        let t0 = Instant::now();
        for s in 0..6 {
            fx.write(&dev(0, "gpu_busy_percent"), &format!("{}\n", 40 + s));
            let g = &gpus.sample(t0 + Duration::from_secs(s))[0];
            assert_eq!(g.busy_pct, Some((40 + s) as f32));
        }
    }

    #[test]
    fn intel_xe() {
        let fx = Fixture::new("gpu-xe");
        with_pci_ids(&fx);
        card(
            &fx,
            0,
            "DRIVER=xe\nPCI_ID=8086:A7A0\nPCI_SLOT_NAME=0000:00:02.0\n",
        );
        fx.write(&dev(0, "tile0/gt0/freq0/act_freq"), "1200\n")
            .write(&dev(0, "tile0/gt0/freq0/cur_freq"), "1250\n")
            .write(&dev(0, "tile0/gt0/freq0/max_freq"), "1500\n");
        let g = &sample(&fx)[0];
        assert_eq!(g.spec.model.as_deref(), Some("Intel Iris Xe Graphics"));
        assert_eq!((g.freq_mhz, g.freq_max_mhz), (Some(1200), Some(1500)));
        assert_eq!(g.spec.integrated, Some(true));
        assert_eq!(g.note, Some(GpuNote::NeedsRoot));
    }

    #[test]
    fn i915_without_act_freq_uses_cur_and_rp0() {
        // Kernels antigos: sem gt_act_freq_mhz; max só pelo RP0.
        let fx = Fixture::new("gpu-i915-old");
        card(&fx, 0, "DRIVER=i915\nPCI_ID=8086:5916\n");
        fx.write("sys/class/drm/card0/gt_cur_freq_mhz", "300\n")
            .write("sys/class/drm/card0/gt_RP0_freq_mhz", "1100\n");
        let g = &sample(&fx)[0];
        assert_eq!((g.freq_mhz, g.freq_max_mhz), (Some(300), Some(1100)));
    }

    #[test]
    fn nouveau_only_temperature() {
        let fx = Fixture::new("gpu-nouveau");
        card(
            &fx,
            1,
            "DRIVER=nouveau\nPCI_ID=10DE:2520\nPCI_SLOT_NAME=0000:01:00.0\n",
        );
        fx.write(&dev(1, "hwmon/hwmon4/name"), "nouveau\n")
            .write(&dev(1, "hwmon/hwmon4/temp1_input"), "38000\n")
            .write(&dev(1, "hwmon/hwmon4/temp1_max"), "95000\n");
        let g = &sample(&fx)[0];
        assert_eq!(g.spec.vendor, GpuVendor::Nvidia);
        assert_eq!(g.spec.integrated, Some(false));
        assert_eq!(g.temp.unwrap().celsius, 38.0);
        assert_eq!(g.busy_pct, None);
        assert_eq!(g.note, Some(GpuNote::NotExposed));
    }

    fn nvidia(fx: &Fixture, n: u32, slot: &str, status: &str) {
        card(
            fx,
            n,
            &format!("DRIVER=nvidia\nPCI_ID=10DE:2520\nPCI_SLOT_NAME={slot}\n"),
        );
        fx.write(&dev(n, "power/runtime_status"), &format!("{status}\n"))
            .write(&dev(n, "power/control"), "auto\n");
    }

    #[test]
    fn nvidia_without_smi() {
        let fx = Fixture::new("gpu-nv-nosmi");
        with_pci_ids(&fx);
        nvidia(&fx, 1, "0000:01:00.0", "active");
        let g = &sample(&fx)[0];
        assert_eq!(
            g.spec.model.as_deref(),
            Some("NVIDIA GeForce RTX 3060 Mobile / Max-Q")
        );
        assert_eq!(g.state, GpuState::Active);
        assert_eq!(g.busy_pct, None);
        assert_eq!(g.note, Some(GpuNote::NoNvidiaSmi));
    }

    #[test]
    fn nvidia_with_smi_and_hybrid_intel() {
        let fx = Fixture::new("gpu-nv");
        i915(&fx, 0);
        nvidia(&fx, 1, "0000:02:00.0", "active");
        nvidia::tests::fake_smi(&fx);
        let mut gpus = Gpus::discover(fx.path());
        let t0 = Instant::now();
        let mut s = 0;
        let g = loop {
            let g = gpus.sample(t0 + Duration::from_millis(s));
            if g[1].busy_pct.is_some() {
                break g;
            }
            assert!(s < 5000, "nvidia-smi não respondeu");
            std::thread::sleep(Duration::from_millis(10));
            s += 10;
        };
        // A linha do 02:00.0, não a do 01:00.0.
        let nv = &g[1];
        assert_eq!(nv.busy_pct, Some(0.0));
        assert_eq!(nv.spec.vram_total, Some(4096 * 1024 * 1024));
        assert_eq!(nv.power_w, None);
        assert_eq!(nv.note, None);
        assert_eq!(g[0].freq_mhz, Some(350));

        let pid = gpus.smi.as_ref().unwrap().pid();
        drop(gpus);
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "nvidia-smi sobrou"
        );
    }

    #[test]
    fn suspended_nvidia_does_not_start_smi() {
        let fx = Fixture::new("gpu-nv-sleep");
        nvidia(&fx, 0, "0000:01:00.0", "suspended");
        nvidia::tests::fake_smi(&fx);
        let mut gpus = Gpus::discover(fx.path());
        let g = &gpus.sample(Instant::now())[0];
        assert_eq!(g.state, GpuState::Suspended);
        assert!(gpus.smi.is_none(), "nvidia-smi acordaria a placa");
    }

    #[test]
    fn idle_nvidia_stops_smi() {
        let fx = Fixture::new("gpu-nv-idle");
        nvidia(&fx, 0, "0000:02:00.0", "active");
        nvidia::tests::fake_smi(&fx);
        let mut gpus = Gpus::discover(fx.path());
        let t0 = Instant::now();
        gpus.sample(t0);
        nvidia::tests::wait_for(|| {
            gpus.smi
                .as_ref()
                .is_some_and(|s| s.reading("0000:02:00.0").is_some())
        });
        // Três leituras com 0%: a pausa fecha o nvidia-smi.
        for s in 1..=3 {
            gpus.sample(t0 + Duration::from_secs(s));
        }
        assert!(gpus.smi.is_some());
        gpus.sample(t0 + Duration::from_secs(4));
        assert!(gpus.smi.is_none(), "nvidia-smi segurou a placa acordada");
    }

    #[test]
    fn no_gpu_and_framebuffer_only() {
        let fx = Fixture::new("gpu-none");
        assert!(sample(&fx).is_empty());
        // VM ou live USB sem driver de vídeo: só o framebuffer do firmware.
        card(&fx, 0, "DRIVER=simple-framebuffer\nOF_NAME=framebuffer\n");
        fx.write("sys/class/drm/version", "drm 1.1.0 20060810\n");
        assert!(sample(&fx).is_empty());
    }

    #[test]
    fn arm_soc_gpu_without_pci() {
        let fx = Fixture::new("gpu-arm");
        card(
            &fx,
            0,
            "DRIVER=v3d\nOF_NAME=v3d\nOF_COMPATIBLE_0=brcm,2711-v3d\n",
        );
        let g = &sample(&fx)[0];
        assert_eq!(g.spec.vendor, GpuVendor::Other);
        assert_eq!(
            (g.spec.model.as_deref(), g.spec.pci_id.as_deref()),
            (None, None)
        );
        assert_eq!(g.spec.driver.as_deref(), Some("v3d"));
        assert_eq!(g.spec.integrated, Some(true));
        assert_eq!(g.note, Some(GpuNote::NotExposed));
    }

    #[test]
    fn virtual_gpu_is_listed_without_numbers() {
        let fx = Fixture::new("gpu-virtio");
        // virtio-gpu: o card fica no dispositivo virtio, fora do PCI.
        card(
            &fx,
            0,
            "DRIVER=virtio_gpu\nMODALIAS=virtio:d00000010v00001AF4\n",
        );
        card(&fx, 1, "DRIVER=vmwgfx\nPCI_ID=15AD:0405\n");
        for g in sample(&fx) {
            assert_eq!(g.spec.vendor, GpuVendor::Other);
            assert_eq!(g.spec.integrated, None);
            assert_eq!(g.note, Some(GpuNote::NotExposed));
        }
    }

    #[test]
    fn real_system_does_not_wake_suspended_gpus() {
        // Na máquina de teste (Radeon em D3cold), conferir que ela continua dormindo.
        let before: Vec<(PathBuf, bool)> = cards(Path::new("/"))
            .iter()
            .map(|c| (c.dev.clone(), c.state() == GpuState::Suspended))
            .collect();
        let mut gpus = Gpus::discover(Path::new("/"));
        gpus.sample(Instant::now());
        for (dev, was_suspended) in before {
            if was_suspended {
                let now = fs::read_to_string(dev.join("power/runtime_status")).unwrap_or_default();
                assert_ne!(now.trim(), "active", "{} acordou", dev.display());
            }
        }
    }
}
