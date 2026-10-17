//! Motor de varredura do disco: percorre o filesystem, classifica cada arquivo
//! e agrega os totais por categoria. Não depende do Tauri.

mod classify;
mod disk;
mod model;
mod walk;

pub use classify::classify;
pub use disk::disk_info;
pub use model::{Category, CategoryStats, DiskInfo, FileEntry, ScanProgress, ScanResult};
pub use walk::{ScanError, ScanOptions, scan};
