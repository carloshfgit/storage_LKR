//! Motor de varredura do disco: percorre o filesystem, classifica cada arquivo
//! e agrega os totais por categoria. Não depende do Tauri.

mod classify;
mod disk;
mod docker;
mod model;
mod sniff;
mod suggest;
mod walk;

pub use classify::Classifier;
pub use disk::disk_info;
pub use docker::{
    DEFAULT_DOCKER_TIMEOUT, DockerCacheEntry, DockerContainer, DockerDetails, DockerImage,
    DockerItem, DockerMode, DockerStatus, DockerTarget, DockerUsage, DockerVolume, docker_details,
    docker_prune, docker_status, docker_usage,
};
pub use model::{
    CacheEntry, Category, CategoryStats, DiskInfo, FileEntry, OldDownloads, ScanProgress,
    ScanResult, SuggestedFile, Suggestions, TrashEntry,
};
pub use suggest::{MAX_CACHE_ENTRIES, OLD_DOWNLOAD_DAYS};
pub use walk::{LockedDirs, ScanError, ScanOptions, locked_dirs, scan, tree_size};
