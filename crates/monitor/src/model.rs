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
}

#[derive(Debug, Clone, Serialize)]
pub struct CpuSample {
    pub usage_pct: Option<f32>,
    pub freq_avg_mhz: Option<u32>,
    pub freq_max_mhz: Option<u32>,
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
