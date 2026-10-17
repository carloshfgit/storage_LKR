use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Categoria de um arquivo na barra. "Livre" não é uma categoria: vem do `statvfs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Photos,
    Videos,
    Audio,
    Documents,
    AppsSystem,
    Other,
}

impl Category {
    pub const COUNT: usize = 6;
    pub const ALL: [Category; Self::COUNT] = [
        Category::Photos,
        Category::Videos,
        Category::Audio,
        Category::Documents,
        Category::AppsSystem,
        Category::Other,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::Photos => "Fotos",
            Category::Videos => "Vídeos",
            Category::Audio => "Áudio",
            Category::Documents => "Documentos",
            Category::AppsSystem => "Apps/Sistema",
            Category::Other => "Outros",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub mount: PathBuf,
    pub device: String,
    pub total_bytes: u64,
    /// Espaço utilizável pelo usuário (`f_bavail`), já descontando os blocos reservados.
    pub available_bytes: u64,
}

impl DiskInfo {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.available_bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: PathBuf,
    /// Espaço alocado em disco (`st_blocks * 512`).
    pub bytes: u64,
    /// mtime em segundos Unix.
    pub modified: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CategoryStats {
    pub bytes: u64,
    pub files: u64,
    /// Maiores arquivos da categoria, em ordem decrescente de tamanho.
    pub top: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub disk: DiskInfo,
    pub categories: HashMap<Category, CategoryStats>,
    /// Usado no disco menos o que foi escaneado: dirs sem permissão, metadados e blocos reservados.
    pub unaccounted_bytes: u64,
    pub denied_dirs: u64,
    pub files: u64,
    pub elapsed_ms: u64,
}

impl ScanResult {
    pub fn scanned_bytes(&self) -> u64 {
        self.categories.values().map(|c| c.bytes).sum()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub bytes: HashMap<Category, u64>,
    pub files: u64,
}
