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
