use serde::Serialize;

/// Uma amostra do monitor. As taxas (`*_pct`, `*_bps`) são calculadas contra a
/// amostra anterior e vêm `None` na primeira.
#[derive(Debug, Clone, Serialize)]
pub struct Sample {
    /// Milissegundos desde a criação do `Sampler` (relógio monotônico): o eixo X.
    pub at_ms: u64,
    pub uptime_s: u64,
    pub cpu: CpuSample,
    pub mem: MemSample,
    /// Só discos físicos inteiros (sem partições, `loop`, `dm` nem `md`).
    pub disks: Vec<DiskIo>,
    pub net: NetSample,
    /// Só baterias do sistema (sem mouse e fone Bluetooth); vazio no desktop.
    pub batteries: Vec<BatterySample>,
    pub battery_total: Option<BatteryTotal>,
    /// Uma por placa em `/sys/class/drm`; vazio sem GPU (VM, servidor).
    pub gpus: Vec<GpuSample>,
    /// "Top 5" processos: a cada [`PROCS_EVERY`](crate::PROCS_EVERY) amostras,
    /// `None` nas outras.
    pub procs: Option<ProcTop>,
}

/// Uma temperatura com o limiar de alerta do sensor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Temp {
    pub celsius: f32,
    /// Acima disso, a UI destaca o valor.
    pub hot_c: f32,
    /// Sensor da placa (ACPI), não da peça: indicação, não medida.
    pub approximate: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CpuSample {
    pub usage_pct: Option<f32>,
    pub freq_avg_mhz: Option<u32>,
    pub freq_max_mhz: Option<u32>,
    pub temp: Option<Temp>,
    /// Só os núcleos online, na ordem do kernel.
    pub cores: Vec<CoreSample>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoreSample {
    pub id: u32,
    pub usage_pct: Option<f32>,
    pub freq_mhz: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemSample {
    pub total: u64,
    /// `total − available`, como o `free` e o GNOME.
    pub used: u64,
    pub available: u64,
    /// Só o swap em disco (partições e arquivos, do `/proc/swaps`): o zRAM
    /// usado como swap vem em `zram`, para não contar duas vezes.
    pub swap_total: u64,
    pub swap_used: u64,
    pub zram: Option<Zram>,
    /// `None` em kernel sem PSI (`/proc/pressure/memory`).
    pub psi: Option<Psi>,
}

/// Soma dos dispositivos zRAM em uso.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Zram {
    pub devices: u32,
    pub disksize: u64,
    /// Dados guardados, antes da compressão.
    pub orig_bytes: u64,
    pub compr_bytes: u64,
    /// Memória que o zRAM ocupa de fato (dados comprimidos + metadados).
    pub mem_used: u64,
}

/// Pressão de memória: % do tempo em que alguma tarefa (`some`) ou todas
/// (`full`) esperaram por memória nos últimos 10 s.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Psi {
    pub some_avg10: f32,
    pub full_avg10: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskKind {
    Nvme,
    Sata,
    Usb,
    Mmc,
    Virtual,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskIo {
    pub name: String,
    pub model: Option<String>,
    pub kind: DiskKind,
    /// `None` quando o kernel não informa (`queue/rotational` ausente).
    pub rotational: Option<bool>,
    pub read_bps: Option<f64>,
    pub write_bps: Option<f64>,
    /// % do tempo com I/O em andamento (`io_ticks`), como o `%util` do `iostat`.
    pub busy_pct: Option<f32>,
    pub temp: Option<Temp>,
    /// Comando que liga o sensor, quando falta só ele (SATA sem o `drivetemp`).
    pub temp_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetSample {
    pub rx_bps: Option<f64>,
    pub tx_bps: Option<f64>,
    /// Bytes desde a primeira amostra.
    pub rx_session: u64,
    pub tx_session: u64,
    /// Só interfaces físicas (com `device/`).
    pub interfaces: Vec<NetIface>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetIface {
    pub name: String,
    pub wireless: bool,
    pub rx_bps: Option<f64>,
    pub tx_bps: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryStatus {
    Charging,
    Discharging,
    Full,
    /// Na tomada sem carregar (limite de carga configurado, por exemplo).
    NotCharging,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatterySample {
    pub name: String,
    pub model: Option<String>,
    pub pct: Option<f32>,
    pub status: BatteryStatus,
    pub energy_now_wh: Option<f64>,
    pub energy_full_wh: Option<f64>,
    pub energy_full_design_wh: Option<f64>,
    pub power_w: Option<f64>,
    /// Capacidade atual / capacidade de fábrica.
    pub health_pct: Option<f32>,
    /// `None` quando o firmware não conta (informa 0).
    pub cycles: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatteryTotal {
    /// Ponderado pela energia de cada bateria, como o UPower.
    pub pct: Option<f32>,
    pub status: BatteryStatus,
    pub power_w: Option<f64>,
    /// Até esvaziar (descarregando) ou até encher (carregando).
    pub time_left_s: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    Amd,
    Nvidia,
    Intel,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuState {
    Active,
    /// Desligada pelo kernel para economizar (`runtime_status = suspended`):
    /// não é lida, para não acordar.
    Suspended,
}

/// Por que faltam números, quando faltam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuNote {
    /// Intel: uso e consumo só pelo `perf`, com root.
    NeedsRoot,
    /// NVIDIA proprietária sem o `nvidia-smi`.
    NoNvidiaSmi,
    /// O driver não expõe (`nouveau`, `radeon`, GPU virtual, ARM).
    NotExposed,
}

/// O que não muda numa placa: vai para a ficha técnica e para cada amostra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GpuSpec {
    /// `card0`, `card1`…: identifica a placa entre as amostras.
    pub card: String,
    /// Nome comercial pelo `pci.ids` ("AMD Radeon R7 M445"); `None` sem o
    /// arquivo ou fora do PCI (a UI monta "GPU AMD (amdgpu)").
    pub model: Option<String>,
    pub vendor: GpuVendor,
    pub driver: Option<String>,
    /// `vendor:device` em hexadecimal ("1002:6900"); `None` fora do PCI.
    pub pci_id: Option<String>,
    /// `None` quando não dá para saber (GPU virtual, driver desconhecido).
    pub integrated: Option<bool>,
    pub vram_total: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuSample {
    #[serde(flatten)]
    pub spec: GpuSpec,
    pub state: GpuState,
    pub busy_pct: Option<f32>,
    pub vram_used: Option<u64>,
    /// Integradas AMD: memória do sistema que a GPU usa (GTT).
    pub gtt_used: Option<u64>,
    pub gtt_total: Option<u64>,
    pub temp: Option<Temp>,
    pub power_w: Option<f64>,
    pub freq_mhz: Option<u32>,
    pub freq_max_mhz: Option<u32>,
    pub note: Option<GpuNote>,
}

/// Os processos de um aplicativo: os que têm o mesmo `comm` do pai (as abas do
/// Chrome) e, com nome pelo `.desktop`, todos os do mesmo app.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProcGroup {
    /// Estável entre as leituras, para a lista animar a troca de posição:
    /// `app:<id do .desktop>` ou `pid:<raiz do grupo>`.
    pub key: String,
    pub name: String,
    pub count: u32,
    /// % de um núcleo, como o `top` (passa de 100 com várias threads). `None`
    /// na primeira leitura.
    pub cpu_pct: Option<f32>,
    /// Soma do RSS: a memória compartilhada entre os processos conta em cada um.
    pub mem_bytes: u64,
    /// Só os processos do próprio usuário (o `/proc/[pid]/io` dos outros não
    /// abre); `None` se nenhum do grupo abre, ou na primeira leitura.
    pub read_bps: Option<f64>,
    pub write_bps: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProcTop {
    /// Processos lidos (sem threads do kernel).
    pub total: u32,
    /// Até 5 por lista; a de CPU vem vazia na primeira leitura, e a de disco
    /// só tem quem leu ou gravou.
    pub cpu: Vec<ProcGroup>,
    pub mem: Vec<ProcGroup>,
    pub disk: Vec<ProcGroup>,
}
