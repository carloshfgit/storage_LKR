use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};

use crate::docker::DockerUsage;

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
    #[serde(serialize_with = "lossy_path")]
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
    #[serde(serialize_with = "lossy_path")]
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
    /// Parte de `Other` que está na Lixeira do usuário.
    pub trash_bytes: u64,
    /// Preenchido por [`ScanResult::apply_docker`]; a consulta roda à parte porque é lenta.
    pub docker: Option<DockerUsage>,
    /// Quanto do Docker foi descontado do restante (limitado ao próprio restante).
    pub docker_bytes: u64,
    /// Usado no disco menos o escaneado e o Docker: dirs sem permissão, metadados e blocos reservados.
    pub unaccounted_bytes: u64,
    pub denied_dirs: u64,
    pub files: u64,
    pub elapsed_ms: u64,
}

impl ScanResult {
    pub fn scanned_bytes(&self) -> u64 {
        self.categories.values().map(|c| c.bytes).sum()
    }

    /// Move o uso do Docker do restante para um subitem de Apps/Sistema.
    ///
    /// O Docker relata tamanho lógico, não blocos alocados, então o desconto é limitado
    /// ao restante para a barra continuar somando 100% do disco. Chamar de novo substitui
    /// o valor anterior.
    pub fn apply_docker(&mut self, usage: Option<DockerUsage>) {
        let restante = self.unaccounted_bytes + self.docker_bytes;
        self.docker_bytes = usage.as_ref().map_or(0, |u| u.total().min(restante));
        self.unaccounted_bytes = restante - self.docker_bytes;
        self.docker = usage;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub bytes: HashMap<Category, u64>,
    pub files: u64,
}

/// Nomes de arquivo no Linux podem não ser UTF-8, e o serde recusa esses `PathBuf`. Um nome
/// assim derrubaria o resultado inteiro; com a conversão com perdas, só ele sai com `�`.
fn lossy_path<S: Serializer>(path: &Path, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(unaccounted: u64) -> ScanResult {
        ScanResult {
            disk: DiskInfo {
                mount: "/".into(),
                device: String::new(),
                total_bytes: 0,
                available_bytes: 0,
            },
            categories: HashMap::new(),
            trash_bytes: 0,
            docker: None,
            docker_bytes: 0,
            unaccounted_bytes: unaccounted,
            denied_dirs: 0,
            files: 0,
            elapsed_ms: 0,
        }
    }

    fn docker(total: u64) -> DockerUsage {
        DockerUsage {
            images: total,
            ..Default::default()
        }
    }

    #[test]
    fn non_utf8_paths_serialize_lossy() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let entry = FileEntry {
            path: PathBuf::from(OsStr::from_bytes(b"/home/u/foto\xff.jpg")),
            bytes: 1,
            modified: 0,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("/home/u/foto\u{fffd}.jpg"), "{json}");
    }

    #[test]
    fn docker_moves_out_of_unaccounted() {
        let mut r = result(100);
        r.apply_docker(Some(docker(70)));
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (70, 30));
    }

    #[test]
    fn docker_is_capped_at_unaccounted() {
        let mut r = result(100);
        r.apply_docker(Some(docker(150)));
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (100, 0));
    }

    #[test]
    fn reapplying_replaces_previous_value() {
        let mut r = result(100);
        r.apply_docker(Some(docker(70)));
        r.apply_docker(Some(docker(20)));
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (20, 80));
        r.apply_docker(None);
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (0, 100));
    }
}
