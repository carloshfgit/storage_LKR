//! Telemetria do computador em tempo real: CPU, memória, disco, rede,
//! temperaturas, bateria e GPU, lidos direto de `/proc` e `/sys`. Não depende do Tauri.
//!
//! Cada coletor tem um parser puro sobre o texto do arquivo, e o [`Sampler`]
//! recebe a raiz do sistema (`/` em produção, uma árvore falsa nos testes).

mod battery;
mod cpu;
mod disk_io;
mod gpu;
mod info;
mod mem;
mod model;
mod net;
mod sampler;
mod sensors;

#[cfg(test)]
mod fixture;

pub use info::{DiskSpec, SystemInfo, system_info};
pub use model::{
    BatterySample, BatteryStatus, BatteryTotal, CoreSample, CpuSample, DiskIo, DiskKind, GpuNote,
    GpuSample, GpuSpec, GpuState, GpuVendor, MemSample, NetIface, NetSample, Psi, Sample, Temp,
    Zram,
};
pub use sampler::Sampler;
