use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};

use crate::classify::Classifier;
use crate::docker::DockerStatus;

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

/// Arquivo sugerido para limpeza. Guarda a categoria em que foi contado, porque pode não
/// estar na lista dos maiores dela e a barra precisa saber de onde tirar os bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedFile {
    #[serde(flatten)]
    pub entry: FileEntry,
    pub category: Category,
}

/// Arquivos da pasta de Downloads sem modificação há mais de `OLD_DOWNLOAD_DAYS` dias.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OldDownloads {
    pub bytes: u64,
    pub files: u64,
    /// Os maiores, em ordem decrescente de tamanho.
    pub top: Vec<SuggestedFile>,
}

/// Item direto de `~/.cache` (normalmente a pasta de cache de um app), com tudo o que há dentro.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    #[serde(serialize_with = "lossy_path")]
    pub path: PathBuf,
    pub bytes: u64,
    pub files: u64,
}

/// Item direto da Lixeira (`Trash/files/<nome>`), com tudo o que há dentro.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashEntry {
    /// Onde o item está guardado na Lixeira ou, se `moved`, o caminho original dele.
    #[serde(serialize_with = "lossy_path")]
    pub path: PathBuf,
    pub bytes: u64,
    pub files: u64,
    /// Movido pelo app depois do scan. O nome dentro da Lixeira só o sistema sabe
    /// (`foto.jpg`, `foto.2.jpg`…), então o item é achado pelo caminho original.
    pub moved: bool,
}

impl TrashEntry {
    fn is(&self, stored: &Path, original: &Path) -> bool {
        self.path == if self.moved { original } else { stored }
    }
}

/// Sugestões de limpeza calculadas no walk (ARCHITECTURE §4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Suggestions {
    /// Pasta de Downloads do `user-dirs.dirs`; `None` se não houver.
    #[serde(serialize_with = "lossy_opt_path")]
    pub downloads_dir: Option<PathBuf>,
    /// mtime (segundos Unix) abaixo do qual um download conta como antigo.
    pub old_before: i64,
    pub old_downloads: OldDownloads,
    /// Os maiores itens de `~/.cache`, em ordem decrescente.
    pub caches: Vec<CacheEntry>,
}

impl Suggestions {
    fn is_old_download(&self, entry: &FileEntry) -> bool {
        entry.modified < self.old_before
            && self
                .downloads_dir
                .as_ref()
                .is_some_and(|d| entry.path.starts_with(d))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub disk: DiskInfo,
    pub categories: HashMap<Category, CategoryStats>,
    /// Parte de `Other` que está na Lixeira do usuário.
    pub trash_bytes: u64,
    /// Arquivos (não diretórios) dentro da Lixeira, já contados em `Other`.
    pub trash_files: u64,
    /// Itens da Lixeira, em ordem decrescente de tamanho no fim do scan; o que o app move
    /// para lá depois entra no fim.
    pub trash_items: Vec<TrashEntry>,
    /// Arquivos sem extensão classificados pelo conteúdo (regra 6b), já contados nas categorias.
    pub sniffed_files: u64,
    pub sniffed_bytes: u64,
    /// `$HOME` usado na classificação; as ações de limpeza validam contra ele.
    #[serde(skip)]
    pub home: PathBuf,
    /// Preenchido por [`ScanResult::apply_docker`]; a consulta roda à parte porque é lenta.
    pub docker: DockerStatus,
    /// Quanto do Docker foi descontado do restante (limitado ao próprio restante).
    pub docker_bytes: u64,
    /// Lido pelo walk em `~/.local/share/docker` (Docker rootless), já em Apps/Sistema.
    pub rootless_scanned_bytes: u64,
    /// Usado no disco menos o escaneado e o Docker: dirs sem permissão, metadados e blocos reservados.
    pub unaccounted_bytes: u64,
    pub denied_dirs: u64,
    pub files: u64,
    pub suggestions: Suggestions,
    pub elapsed_ms: u64,
    /// Fim do scan em segundos Unix: a UI mostra "há 2 h" para um resultado do cache.
    pub finished_at: i64,
}

impl ScanResult {
    pub fn scanned_bytes(&self) -> u64 {
        self.categories.values().map(|c| c.bytes).sum()
    }

    /// Se `path` é um arquivo listado no último scan: nos maiores de uma categoria ou nos
    /// downloads antigos. Toda ação sobre um arquivo vindo da UI passa por aqui.
    pub fn contains_file(&self, path: &Path) -> bool {
        self.find_file(path).is_some()
            || self
                .suggestions
                .old_downloads
                .top
                .iter()
                .any(|f| f.entry.path == path)
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

    /// Contabiliza um arquivo movido para a Lixeira: sai da categoria, da lista dos
    /// maiores e dos downloads antigos, e entra em `Other`/`trash_bytes`. O espaço usado
    /// não muda, então o restante e o Docker ficam como estão.
    pub fn record_trashed(&mut self, path: &Path) -> Option<FileEntry> {
        let (cat, entry) = match self.find_file(path) {
            Some((cat, i)) => (cat, self.categories.get_mut(&cat)?.top.remove(i)),
            None => {
                let old = &self.suggestions.old_downloads;
                let f = old.top.iter().find(|f| f.entry.path == path)?;
                (f.category, f.entry.clone())
            }
        };
        let stats = self.categories.get_mut(&cat)?;
        stats.bytes = stats.bytes.saturating_sub(entry.bytes);
        stats.files = stats.files.saturating_sub(1);

        if self.suggestions.is_old_download(&entry) {
            let old = &mut self.suggestions.old_downloads;
            old.bytes = old.bytes.saturating_sub(entry.bytes);
            old.files = old.files.saturating_sub(1);
            old.top.retain(|f| f.entry.path != path);
        }

        let other = self.categories.entry(Category::Other).or_default();
        other.bytes += entry.bytes;
        other.files += 1;
        self.trash_bytes += entry.bytes;
        self.trash_files += 1;
        self.trash_items.push(TrashEntry {
            path: entry.path.clone(),
            bytes: entry.bytes,
            files: 1,
            moved: true,
        });
        Some(entry)
    }

    /// Contabiliza um item de `~/.cache` movido para a Lixeira. Os bytes já estavam em
    /// `Other` (pasta oculta, regra 4) e continuam lá, agora como Lixeira; os arquivos de
    /// dentro dele saem da lista dos maiores, porque mudaram de caminho.
    pub fn record_trashed_cache(&mut self, path: &Path) -> Option<CacheEntry> {
        let i = self
            .suggestions
            .caches
            .iter()
            .position(|c| c.path == path)?;
        let entry = self.suggestions.caches.remove(i);
        for stats in self.categories.values_mut() {
            stats.top.retain(|f| !f.path.starts_with(path));
        }
        self.trash_bytes += entry.bytes;
        self.trash_files += entry.files;
        self.trash_items.push(TrashEntry {
            path: entry.path.clone(),
            bytes: entry.bytes,
            files: entry.files,
            moved: true,
        });
        Some(entry)
    }

    /// Item da Lixeira guardado em `stored` e que veio de `original`, se o scan o contou.
    pub fn find_trash_item(&self, stored: &Path, original: &Path) -> Option<&TrashEntry> {
        self.trash_items.iter().find(|t| t.is(stored, original))
    }

    /// Contabiliza um item restaurado da Lixeira: sai de `Other`/`trash_bytes` e entra na
    /// categoria do caminho original (fora da lista dos maiores; o próximo scan acerta).
    /// `None` se o scan não o contou, e aí nada muda.
    pub fn record_restored(&mut self, stored: &Path, original: &Path) -> Option<TrashEntry> {
        let entry = self.take_trash_item(stored, original)?;
        let category = Classifier::new(&self.home).classify(original);
        let stats = self.categories.entry(category).or_default();
        stats.bytes += entry.bytes;
        stats.files += entry.files;
        Some(entry)
    }

    /// Contabiliza um item apagado da Lixeira. Chamar [`ScanResult::set_disk`] depois, com
    /// o `statvfs` relido.
    pub fn record_purged(&mut self, stored: &Path, original: &Path) -> Option<TrashEntry> {
        self.take_trash_item(stored, original)
    }

    /// Tira o item de `trash_items`, de `Other`/`trash_bytes` e, com o que havia dentro
    /// dele, da lista dos maiores.
    fn take_trash_item(&mut self, stored: &Path, original: &Path) -> Option<TrashEntry> {
        let i = self
            .trash_items
            .iter()
            .position(|t| t.is(stored, original))?;
        let entry = self.trash_items.remove(i);
        let other = self.categories.entry(Category::Other).or_default();
        other.bytes = other.bytes.saturating_sub(entry.bytes);
        other.files = other.files.saturating_sub(entry.files);
        other.top.retain(|f| !f.path.starts_with(stored));
        self.trash_bytes = self.trash_bytes.saturating_sub(entry.bytes);
        self.trash_files = self.trash_files.saturating_sub(entry.files);
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
        self.trash_items.clear();
    }

    /// Troca o `DiskInfo` depois de uma limpeza e recalcula o restante, mantendo o Docker
    /// limitado a ele (como em [`ScanResult::apply_docker`]): a barra continua somando 100%.
    pub fn set_disk(&mut self, disk: DiskInfo) {
        self.unaccounted_bytes = disk.used_bytes().saturating_sub(self.scanned_bytes());
        self.docker_bytes = 0;
        self.disk = disk;
        let docker = std::mem::take(&mut self.docker);
        self.apply_docker(docker);
    }

    /// Move o uso do Docker do restante para um subitem de Apps/Sistema.
    ///
    /// O Docker relata tamanho lógico, não blocos alocados, então o desconto é limitado
    /// ao restante para a barra continuar somando 100% do disco. Do rootless, só sai do
    /// restante o que o walk não leu em `~/.local/share/docker` (o resto já está em
    /// Apps/Sistema). Chamar de novo substitui o valor anterior.
    pub fn apply_docker(&mut self, docker: DockerStatus) {
        let restante = self.unaccounted_bytes + self.docker_bytes;
        let rootful = docker
            .rootful
            .as_ref()
            .map_or(0, |u| u.total().min(restante));
        let rootless = docker.rootless.as_ref().map_or(0, |u| {
            u.total()
                .saturating_sub(self.rootless_scanned_bytes)
                .min(restante - rootful)
        });
        self.docker_bytes = rootful + rootless;
        self.unaccounted_bytes = restante - self.docker_bytes;
        self.docker = docker;
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

fn lossy_opt_path<S: Serializer>(path: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
    match path {
        Some(p) => lossy_path(p, s),
        None => s.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::{DockerItem, DockerUsage};

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
            trash_items: Vec::new(),
            sniffed_files: 0,
            sniffed_bytes: 0,
            home: PathBuf::new(),
            docker: DockerStatus::default(),
            docker_bytes: 0,
            rootless_scanned_bytes: 0,
            unaccounted_bytes: unaccounted,
            denied_dirs: 0,
            files: 0,
            suggestions: Suggestions::default(),
            elapsed_ms: 0,
            finished_at: 0,
        }
    }

    fn usage(total: u64) -> DockerUsage {
        DockerUsage {
            images: DockerItem {
                size: total,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Só o Docker do sistema.
    fn docker(total: u64) -> DockerStatus {
        DockerStatus {
            rootful: Some(usage(total)),
            rootless: None,
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
        r.apply_docker(docker(70));
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (70, 30));
    }

    #[test]
    fn rootless_only_discounts_what_the_walk_did_not_read() {
        // 40 do rootless já foram lidos pelo walk (estão em Apps/Sistema); só 60 saem do restante.
        let mut r = result(100);
        r.rootless_scanned_bytes = 40;
        r.apply_docker(DockerStatus {
            rootful: None,
            rootless: Some(usage(100)),
        });
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (60, 40));

        // Lido por inteiro: nada sai do restante.
        r.rootless_scanned_bytes = 150;
        r.apply_docker(r.docker.clone());
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (0, 100));
    }

    #[test]
    fn both_daemons_share_the_cap() {
        let mut r = result(100);
        r.rootless_scanned_bytes = 10;
        r.apply_docker(DockerStatus {
            rootful: Some(usage(70)),
            rootless: Some(usage(50)),
        });
        // Rootful 70; rootless 50 − 10 lidos = 40, limitado aos 30 que sobram.
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (100, 0));

        r.apply_docker(DockerStatus {
            rootful: Some(usage(20)),
            rootless: Some(usage(50)),
        });
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (60, 40));
    }

    #[test]
    fn docker_is_capped_at_unaccounted() {
        let mut r = result(100);
        r.apply_docker(docker(150));
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (100, 0));
    }

    #[test]
    fn reapplying_replaces_previous_value() {
        let mut r = result(100);
        r.apply_docker(docker(70));
        r.apply_docker(docker(20));
        assert_eq!((r.docker_bytes, r.unaccounted_bytes), (20, 80));
        r.apply_docker(DockerStatus::default());
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
        r.trash_items = vec![TrashEntry {
            path: "/home/u/.local/share/Trash/files/v.mp4".into(),
            bytes: 40,
            files: 1,
            moved: false,
        }];
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

    /// `scanned()` com `/home/u/Downloads` e dois downloads antigos: `a.jpg` (também nos
    /// maiores de Fotos) e `velho.iso`, que não entrou nos maiores de Outros.
    fn with_old_downloads() -> ScanResult {
        let mut r = scanned();
        r.categories
            .get_mut(&Category::Photos)
            .unwrap()
            .top
            .push(entry("/home/u/Downloads/c.jpg", 0));
        let old = |path: &str, bytes, category| SuggestedFile {
            entry: entry(path, bytes),
            category,
        };
        r.suggestions = Suggestions {
            downloads_dir: Some("/home/u/Downloads".into()),
            old_before: 10,
            old_downloads: OldDownloads {
                bytes: 80,
                files: 2,
                top: vec![
                    old("/home/u/Downloads/c.jpg", 0, Category::Photos),
                    old("/home/u/Downloads/velho.iso", 80, Category::Other),
                ],
            },
            caches: vec![CacheEntry {
                path: "/home/u/.cache/pip".into(),
                bytes: 30,
                files: 3,
            }],
        };
        r
    }

    #[test]
    fn old_download_outside_top_is_trashed_from_its_category() {
        let mut r = with_old_downloads();
        let iso = Path::new("/home/u/Downloads/velho.iso");
        assert!(r.contains_file(iso));
        let moved = r.record_trashed(iso).unwrap();
        assert_eq!(moved.bytes, 80);

        let old = &r.suggestions.old_downloads;
        assert_eq!((old.bytes, old.files, old.top.len()), (0, 1, 1));
        // Saiu de Outros como arquivo comum e voltou como Lixeira: o total de Outros fica igual.
        let other = &r.categories[&Category::Other];
        assert_eq!((other.bytes, other.files), (100, 2));
        assert_eq!(r.trash_bytes, 120);
        assert!(!r.contains_file(iso));
    }

    #[test]
    fn trashing_from_category_also_updates_old_downloads() {
        let mut r = with_old_downloads();
        r.record_trashed(Path::new("/home/u/Downloads/c.jpg"))
            .unwrap();
        let old = &r.suggestions.old_downloads;
        assert_eq!(old.files, 1);
        assert_eq!(old.top.len(), 1);
        assert_eq!(r.categories[&Category::Photos].top.len(), 2);
    }

    #[test]
    fn trashed_cache_becomes_trash_inside_other() {
        let mut r = with_old_downloads();
        r.categories
            .get_mut(&Category::Other)
            .unwrap()
            .top
            .push(entry("/home/u/.cache/pip/wheel.whl", 20));
        let before = r.categories[&Category::Other].bytes;

        let moved = r
            .record_trashed_cache(Path::new("/home/u/.cache/pip"))
            .unwrap();
        assert_eq!(moved.bytes, 30);
        assert!(r.suggestions.caches.is_empty());
        assert_eq!((r.trash_bytes, r.trash_files), (70, 4));
        let other = &r.categories[&Category::Other];
        assert_eq!(other.bytes, before);
        assert!(
            !other
                .top
                .iter()
                .any(|f| f.path.starts_with("/home/u/.cache/pip"))
        );
        assert!(
            r.record_trashed_cache(Path::new("/home/u/.cache/pip"))
                .is_none()
        );
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
        r.apply_docker(docker(450));
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

    #[test]
    fn restored_item_goes_back_to_its_category() {
        let mut r = scanned();
        let total = r.scanned_bytes() + r.unaccounted_bytes;
        let stored = Path::new("/home/u/.local/share/Trash/files/v.mp4");
        let restored = r
            .record_restored(stored, Path::new("/home/u/Vídeos/v.mp4"))
            .unwrap();
        assert_eq!(restored.bytes, 40);

        assert_eq!((r.trash_bytes, r.trash_files), (0, 0));
        assert!(r.trash_items.is_empty());
        let other = &r.categories[&Category::Other];
        assert_eq!((other.bytes, other.files, other.top.len()), (60, 1, 1));
        let videos = &r.categories[&Category::Videos];
        assert_eq!((videos.bytes, videos.files), (40, 1));
        // Nada saiu do disco: a barra soma o mesmo.
        assert_eq!(r.scanned_bytes() + r.unaccounted_bytes, total);

        assert!(
            r.record_restored(stored, Path::new("/home/u/Vídeos/v.mp4"))
                .is_none()
        );
    }

    #[test]
    fn item_moved_by_the_app_is_found_by_original_path() {
        let mut r = scanned();
        r.record_trashed(Path::new("/home/u/a.jpg")).unwrap();
        assert_eq!(r.trash_items.len(), 2);
        // O sistema guardou como `a.2.jpg`; o app só sabe o caminho original.
        let stored = Path::new("/home/u/.local/share/Trash/files/a.2.jpg");
        let original = Path::new("/home/u/a.jpg");
        assert_eq!(r.find_trash_item(stored, original).unwrap().bytes, 200);

        r.record_restored(stored, original).unwrap();
        assert_eq!(r.categories[&Category::Photos].bytes, 300);
        assert_eq!((r.trash_bytes, r.trash_files), (40, 1));
    }

    #[test]
    fn purged_item_leaves_other_and_trash() {
        let mut r = with_old_downloads();
        r.record_trashed_cache(Path::new("/home/u/.cache/pip"))
            .unwrap();
        let purged = r
            .record_purged(
                Path::new("/home/u/.local/share/Trash/files/pip"),
                Path::new("/home/u/.cache/pip"),
            )
            .unwrap();
        assert_eq!((purged.bytes, purged.files), (30, 3));
        assert_eq!((r.trash_bytes, r.trash_files), (40, 1));
        let other = &r.categories[&Category::Other];
        assert_eq!((other.bytes, other.files), (70, 0));

        // Algo que o scan não viu (movido para a Lixeira por fora): nada muda.
        assert!(
            r.record_purged(
                Path::new("/home/u/.local/share/Trash/files/x"),
                Path::new("/home/u/x")
            )
            .is_none()
        );
        assert_eq!(r.trash_bytes, 40);
    }
}
