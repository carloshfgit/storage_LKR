//! Motor de varredura do disco: percorre o filesystem, classifica cada arquivo
//! e agrega os totais por categoria. Não depende do Tauri.

mod classify;
mod disk;
mod docker;
mod model;
mod sniff;
mod walk;

pub use classify::Classifier;
pub use disk::disk_info;
pub use docker::{
    DEFAULT_DOCKER_TIMEOUT, DockerItem, DockerMode, DockerStatus, DockerTarget, DockerUsage,
    docker_prune, docker_status, docker_usage,
};
pub use model::{Category, CategoryStats, DiskInfo, FileEntry, ScanProgress, ScanResult};
pub use walk::{ScanError, ScanOptions, scan};
