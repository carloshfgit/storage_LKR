use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};

use crate::classify::Classifier;
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
    /// Arquivos (não diretórios) dentro da Lixeira, já contados em `Other`.
    pub trash_files: u64,
    /// Arquivos sem extensão classificados pelo conteúdo (regra 6b), já contados nas categorias.
    pub sniffed_files: u64,
    pub sniffed_bytes: u64,
    /// `$HOME` usado na classificação; as ações de limpeza validam contra ele.
    #[serde(skip)]
    pub home: PathBuf,
    /// Preenchido por [`ScanResult::apply_docker`]; a consulta roda à parte porque é lenta.
    pub docker: Option<DockerUsage>,
    /// Quanto do Docker foi descontado do restante (limitado ao próprio restante).
    pub docker_bytes: u64,
    /// Usado no disco menos o escaneado e o Docker: dirs sem permissão, metadados e blocos reservados.
    pub unaccounted_bytes: u64,
    pub denied_dirs: u64,
    pub files: u64,
    pub elapsed_ms: u64,
    /// Fim do scan em segundos Unix: a UI mostra "há 2 h" para um resultado do cache.
    pub finished_at: i64,
}

impl ScanResult {
    pub fn scanned_bytes(&self) -> u64 {
        self.categories.values().map(|c| c.bytes).sum()
    }

    /// Categoria e posição de `path` na lista dos maiores.
    pub fn find_file(&self, path: &Path) -> Option<(Category, usize)> {
        self.categories.iter().find_map(|(&cat, stats)| {
            stats
                .top
                .iter()
                .position(|f| f.path == path)
                .map(|i| (cat, i))
        })
    }

    /// Contabiliza um arquivo movido para a Lixeira: sai da categoria e da lista dos
    /// maiores e entra em `Other`/`trash_bytes`. O espaço usado não muda, então o
    /// restante e o Docker ficam como estão.
    pub fn record_trashed(&mut self, path: &Path) -> Option<FileEntry> {
        let (cat, i) = self.find_file(path)?;
        let stats = self.categories.get_mut(&cat)?;
        let entry = stats.top.remove(i);
        stats.bytes = stats.bytes.saturating_sub(entry.bytes);
        stats.files = stats.files.saturating_sub(1);

        let other = self.categories.entry(Category::Other).or_default();
        other.bytes += entry.bytes;
        other.files += 1;
        self.trash_bytes += entry.bytes;
        self.trash_files += 1;
        Some(entry)
    }

    /// Contabiliza a Lixeira esvaziada: `Other` perde o que estava nela, inclusive os
    /// itens dela na lista dos maiores. Chamar [`ScanResult::set_disk`] depois, com o
    /// `statvfs` relido.
    pub fn clear_trash(&mut self) {
        let classifier = Classifier::new(&self.home);
        let other = self.categories.entry(Category::Other).or_default();
        other.bytes = other.bytes.saturating_sub(self.trash_bytes);
        other.files = other.files.saturating_sub(self.trash_files);
        other.top.retain(|f| !classifier.in_trash(&f.path));
        self.trash_bytes = 0;
        self.trash_files = 0;
    }

    /// Troca o `DiskInfo` depois de uma limpeza e recalcula o restante, mantendo o Docker
    /// limitado a ele (como em [`ScanResult::apply_docker`]): a barra continua somando 100%.
    pub fn set_disk(&mut self, disk: DiskInfo) {
        self.unaccounted_bytes = disk.used_bytes().saturating_sub(self.scanned_bytes());
        self.docker_bytes = 0;
        self.disk = disk;
        let usage = self.docker.take();
        self.apply_docker(usage);
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
    use crate::docker::DockerItem;

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
            trash_files: 0,
            sniffed_files: 0,
            sniffed_bytes: 0,
            home: PathBuf::new(),
            docker: None,
            docker_bytes: 0,
            unaccounted_bytes: unaccounted,
            denied_dirs: 0,
            files: 0,
            elapsed_ms: 0,
            finished_at: 0,
        }
    }

    fn docker(total: u64) -> DockerUsage {
        DockerUsage {
            images: DockerItem {
                size: total,
                ..Default::default()
            },
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

    fn entry(path: &str, bytes: u64) -> FileEntry {
        FileEntry {
            path: path.into(),
            bytes,
            modified: 0,
        }
    }

    /// Disco de 1000 B com 100 livres; fotos 300, outros 100 (40 na Lixeira).
    fn scanned() -> ScanResult {
        let mut r = result(0);
        r.home = "/home/u".into();
        r.disk.total_bytes = 1000;
        r.disk.available_bytes = 100;
        r.categories.insert(
            Category::Photos,
            CategoryStats {
                bytes: 300,
                files: 2,
                top: vec![entry("/home/u/a.jpg", 200), entry("/home/u/b.jpg", 100)],
            },
        );
        r.categories.insert(
            Category::Other,
            CategoryStats {
                bytes: 100,
                files: 2,
                top: vec![
                    entry("/home/u/x.zip", 60),
                    entry("/home/u/.local/share/Trash/files/v.mp4", 40),
                ],
            },
        );
        r.trash_bytes = 40;
        r.trash_files = 1;
        r.unaccounted_bytes = 900 - 400;
        r
    }

    #[test]
    fn trashed_file_moves_to_other() {
        let mut r = scanned();
        let moved = r.record_trashed(Path::new("/home/u/a.jpg")).unwrap();
        assert_eq!(moved.bytes, 200);

        let photos = &r.categories[&Category::Photos];
        assert_eq!((photos.bytes, photos.files, photos.top.len()), (100, 1, 1));
        let other = &r.categories[&Category::Other];
        assert_eq!((other.bytes, other.files), (300, 3));
        assert_eq!((r.trash_bytes, r.trash_files), (240, 2));
        // Nada saiu do disco: o restante não muda.
        assert_eq!(r.unaccounted_bytes, 500);

        assert!(r.record_trashed(Path::new("/home/u/a.jpg")).is_none());
    }

    #[test]
    fn clear_trash_removes_trash_from_other() {
        let mut r = scanned();
        r.clear_trash();
        let other = &r.categories[&Category::Other];
        assert_eq!((other.bytes, other.files), (60, 1));
        assert_eq!(other.top.len(), 1);
        assert_eq!((r.trash_bytes, r.trash_files), (0, 0));
    }

    #[test]
    fn set_disk_recomputes_remainder_and_caps_docker() {
        let mut r = scanned();
        r.apply_docker(Some(docker(450)));
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (450, 50));

        // Lixeira esvaziada: 40 B a mais livres, e 40 B a menos escaneados.
        r.clear_trash();
        let mut disk = r.disk.clone();
        disk.available_bytes += 40;
        r.set_disk(disk);
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (450, 50));

        // Prune do Docker liberou 400 B, mas o Docker ainda relata 450: limitado ao restante.
        let mut disk = r.disk.clone();
        disk.available_bytes += 400;
        r.set_disk(disk);
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (100, 0));
        assert_eq!(r.disk.available_bytes, 540);
    }
}
