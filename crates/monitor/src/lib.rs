//! Telemetria do computador em tempo real: CPU, memória, disco e rede, lidos
//! direto de `/proc` e `/sys`. Não depende do Tauri.
//!
//! Cada coletor tem um parser puro sobre o texto do arquivo, e o [`Sampler`]
//! recebe a raiz do sistema (`/` em produção, uma árvore falsa nos testes).

mod cpu;
mod disk_io;
mod info;
mod mem;
mod model;
mod net;
mod sampler;

#[cfg(test)]
mod fixture;

pub use info::{SystemInfo, system_info};
pub use model::{
    CoreSample, CpuSample, DiskIo, DiskKind, MemSample, NetIface, NetSample, Psi, Sample, Zram,
};
pub use sampler::Sampler;
