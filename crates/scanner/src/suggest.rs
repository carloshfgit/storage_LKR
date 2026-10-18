//! Sugestões de limpeza montadas durante o walk: downloads antigos e caches por app
//! (ARCHITECTURE §4). Nenhuma leitura extra: só prefixos de caminho e o mtime do stat.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::model::{CacheEntry, Category, FileEntry, OldDownloads, SuggestedFile, Suggestions};

/// Um download sem modificação há mais que isso conta como antigo.
pub const OLD_DOWNLOAD_DAYS: i64 = 90;

/// Quantos itens de `~/.cache` vão para o resultado.
pub const MAX_CACHE_ENTRIES: usize = 20;

const DAY_SECS: i64 = 24 * 60 * 60;

/// Pasta de Downloads do usuário, como o Nautilus a mostra: `XDG_DOWNLOAD_DIR` de
/// `<config>/user-dirs.dirs` (`~/Downloads`, `~/Transferências`…), ou `~/Downloads` sem o
/// arquivo. `None` quando a pasta está desligada (apontando para o próprio `$HOME`).
pub fn download_dir(home: &Path, config_dir: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(config_dir.join("user-dirs.dirs")).unwrap_or_default();
    match parse_user_dirs(&text, home) {
        Some(dir) => (dir != home).then_some(dir),
        None => Some(home.join("Downloads")),
    }
}

/// `$XDG_CONFIG_HOME`, ou `~/.config` sem a variável.
pub fn config_dir(home: &Path) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map_or_else(|| home.join(".config"), PathBuf::from)
}

/// Formato do `xdg-user-dirs`: `XDG_DOWNLOAD_DIR="$HOME/Downloads"` ou um caminho absoluto.
fn parse_user_dirs(text: &str, home: &Path) -> Option<PathBuf> {
    let value = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("XDG_DOWNLOAD_DIR="))?;
    let value = value.trim().trim_matches('"');
    if value == "$HOME" {
        return Some(home.to_path_buf());
    }
    if let Some(rest) = value.strip_prefix("$HOME/") {
        return Some(home.join(rest.trim_end_matches('/')));
    }
    value
        .starts_with('/')
        .then(|| PathBuf::from(value.trim_end_matches('/')))
}

/// Decide, nas threads do walk, se uma entrada interessa às sugestões.
#[derive(Debug, Clone)]
pub(crate) struct Suggester {
    downloads: Option<PathBuf>,
    cache: Option<PathBuf>,
    old_before: i64,
}

impl Suggester {
    /// `now`: início do scan em segundos Unix; o corte dos downloads antigos é relativo a ele.
    pub(crate) fn new(home: &Path, downloads: Option<PathBuf>, now: i64) -> Self {
        let has_home = !home.as_os_str().is_empty();
        Self {
            downloads,
            cache: has_home.then(|| home.join(".cache")),
            old_before: now - OLD_DOWNLOAD_DAYS * DAY_SECS,
        }
    }

    pub(crate) fn is_old_download(&self, path: &Path, is_file: bool, modified: i64) -> bool {
        is_file
            && modified < self.old_before
            && self
                .downloads
                .as_ref()
                .is_some_and(|d| strictly_under(path, d))
    }

    pub(crate) fn in_cache(&self, path: &Path) -> bool {
        self.cache.as_ref().is_some_and(|c| strictly_under(path, c))
    }
}

/// Bytes e arquivos por item direto de uma pasta (`~/.cache`, `Trash/files`), somados na
/// thread que consome o walk.
pub(crate) struct ChildTotals {
    dir: Option<PathBuf>,
    /// Nome do item direto → (bytes, arquivos).
    totals: HashMap<OsString, (u64, u64)>,
}

impl ChildTotals {
    /// `None` desliga a soma.
    pub(crate) fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            totals: HashMap::new(),
        }
    }

    /// Nome do item direto de `dir` que contém a entrada `parent/name`, sem alocar.
    fn key<'a>(&self, parent: &'a Path, name: &'a OsStr) -> Option<&'a OsStr> {
        let rel = parent.strip_prefix(self.dir.as_ref()?).ok()?;
        Some(rel.components().next().map_or(name, |c| c.as_os_str()))
    }

    /// Soma a entrada `parent/name` (arquivo ou diretório) ao item que a contém. Fora de
    /// `dir`, não faz nada.
    pub(crate) fn add(&mut self, parent: &Path, name: &OsStr, bytes: u64, is_file: bool) {
        let Some(key) = self.key(parent, name) else {
            return;
        };
        let slot = match self.totals.get_mut(key) {
            Some(slot) => slot,
            None => self.totals.entry(key.to_owned()).or_default(),
        };
        slot.0 += bytes;
        slot.1 += u64::from(is_file);
    }

    /// (caminho, bytes, arquivos) de cada item, em ordem decrescente de tamanho.
    pub(crate) fn finish(self) -> Vec<(PathBuf, u64, u64)> {
        let Some(dir) = self.dir else {
            return Vec::new();
        };
        let mut items: Vec<_> = self
            .totals
            .into_iter()
            .map(|(name, (bytes, files))| (dir.join(name), bytes, files))
            .collect();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        items
    }
}

/// `path` está dentro de `dir`, sem ser o próprio. Compara bytes, como o `Classifier`.
fn strictly_under(path: &Path, dir: &Path) -> bool {
    let (p, d) = (path.as_os_str().as_bytes(), dir.as_os_str().as_bytes());
    p.len() > d.len() && p.starts_with(d) && p[d.len()] == b'/'
}

/// Item do heap dos maiores downloads antigos, ordenado por tamanho.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct OldItem {
    bytes: u64,
    path: PathBuf,
    modified: i64,
    category: usize,
}

/// Agrega as sugestões na thread que consome o walk.
pub(crate) struct SuggestionsAgg {
    suggester: Suggester,
    top_n: usize,
    old_bytes: u64,
    old_files: u64,
    /// Min-heap, como o das categorias.
    old_top: BinaryHeap<Reverse<OldItem>>,
    caches: ChildTotals,
}

impl SuggestionsAgg {
    pub(crate) fn new(suggester: Suggester, top_n: usize) -> Self {
        Self {
            caches: ChildTotals::new(suggester.cache.clone()),
            suggester,
            top_n,
            old_bytes: 0,
            old_files: 0,
            old_top: BinaryHeap::new(),
        }
    }

    pub(crate) fn add_old_download(
        &mut self,
        category: Category,
        bytes: u64,
        modified: i64,
        path: impl FnOnce() -> PathBuf,
    ) {
        self.old_bytes += bytes;
        self.old_files += 1;
        let full = self.old_top.len() >= self.top_n;
        if full
            && self
                .old_top
                .peek()
                .is_some_and(|Reverse(min)| bytes <= min.bytes)
        {
            return;
        }
        if full {
            self.old_top.pop();
        }
        self.old_top.push(Reverse(OldItem {
            bytes,
            path: path(),
            modified,
            category: category.index(),
        }));
    }

    /// Soma a entrada `parent/name` (arquivo ou diretório) ao item de `~/.cache` que a contém.
    pub(crate) fn add_cache(&mut self, parent: &Path, name: &OsStr, bytes: u64, is_file: bool) {
        self.caches.add(parent, name, bytes, is_file);
    }

    pub(crate) fn finish(self) -> Suggestions {
        let Self {
            suggester,
            old_bytes,
            old_files,
            old_top,
            caches,
            ..
        } = self;
        let top = old_top
            .into_sorted_vec()
            .into_iter()
            .map(|Reverse(t)| SuggestedFile {
                entry: FileEntry {
                    path: t.path,
                    bytes: t.bytes,
                    modified: t.modified,
                },
                category: Category::ALL[t.category],
            })
            .collect();

        let caches = caches
            .finish()
            .into_iter()
            .take(MAX_CACHE_ENTRIES)
            .map(|(path, bytes, files)| CacheEntry { path, bytes, files })
            .collect();

        Suggestions {
            downloads_dir: suggester.downloads,
            old_before: suggester.old_before,
            old_downloads: OldDownloads {
                bytes: old_bytes,
                files: old_files,
                top,
            },
            caches,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/u";

    fn parse(text: &str) -> Option<PathBuf> {
        parse_user_dirs(text, Path::new(HOME))
    }

    #[test]
    fn parses_user_dirs() {
        let text = "# comentário\nXDG_DESKTOP_DIR=\"$HOME/Área de trabalho\"\nXDG_DOWNLOAD_DIR=\"$HOME/Transferências\"\n";
        assert_eq!(parse(text), Some(PathBuf::from("/home/u/Transferências")));
        assert_eq!(
            parse("XDG_DOWNLOAD_DIR=\"/dados/baixados/\""),
            Some(PathBuf::from("/dados/baixados"))
        );
        assert_eq!(
            parse("XDG_DOWNLOAD_DIR=\"$HOME\""),
            Some(PathBuf::from(HOME))
        );
        assert_eq!(parse("XDG_MUSIC_DIR=\"$HOME/Músicas\""), None);
        assert_eq!(parse("XDG_DOWNLOAD_DIR=\"relativo\""), None);
    }

    #[test]
    fn download_dir_falls_back_and_can_be_disabled() {
        let dir = std::env::temp_dir().join(format!("scanner-userdirs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let home = Path::new(HOME);

        // Sem o arquivo: ~/Downloads.
        assert_eq!(
            download_dir(home, &dir),
            Some(PathBuf::from("/home/u/Downloads"))
        );
        fs::write(
            dir.join("user-dirs.dirs"),
            "XDG_DOWNLOAD_DIR=\"$HOME/Baixados\"\n",
        )
        .unwrap();
        assert_eq!(
            download_dir(home, &dir),
            Some(PathBuf::from("/home/u/Baixados"))
        );
        // Apontar para o $HOME desliga a pasta (padrão do xdg-user-dirs).
        fs::write(dir.join("user-dirs.dirs"), "XDG_DOWNLOAD_DIR=\"$HOME/\"\n").unwrap();
        assert_eq!(download_dir(home, &dir), None);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn suggester_matches_whole_components() {
        let now = 1_000 * DAY_SECS;
        let s = Suggester::new(Path::new(HOME), Some("/home/u/Downloads".into()), now);
        let old = now - 91 * DAY_SECS;
        let recent = now - 89 * DAY_SECS;
        assert!(s.is_old_download(Path::new("/home/u/Downloads/a.iso"), true, old));
        assert!(s.is_old_download(Path::new("/home/u/Downloads/sub/a.iso"), true, old));
        assert!(!s.is_old_download(Path::new("/home/u/Downloads/a.iso"), true, recent));
        assert!(!s.is_old_download(Path::new("/home/u/Downloads/sub"), false, old));
        assert!(!s.is_old_download(Path::new("/home/u/Downloads2/a.iso"), true, old));

        assert!(s.in_cache(Path::new("/home/u/.cache/pip")));
        assert!(!s.in_cache(Path::new("/home/u/.cache")));
        assert!(!s.in_cache(Path::new("/home/u/.cachex/a")));
        let totals = ChildTotals::new(s.cache.clone());
        let key = |parent: &str, name: &str| {
            totals
                .key(Path::new(parent), OsStr::new(name))
                .map(|k| k.to_string_lossy().into_owned())
        };
        assert_eq!(key("/home/u/.cache", "pip").as_deref(), Some("pip"));
        assert_eq!(
            key("/home/u/.cache/mozilla/x/y", "z").as_deref(),
            Some("mozilla")
        );
        assert_eq!(key("/home/u/Downloads", "z"), None);
    }

    #[test]
    fn caches_are_ranked_and_truncated() {
        let s = Suggester::new(Path::new(HOME), None, 0);
        let mut agg = SuggestionsAgg::new(s, 10);
        let cache = Path::new("/home/u/.cache");
        for i in 0..(MAX_CACHE_ENTRIES + 5) {
            let name = format!("app{i:02}");
            agg.add_cache(cache, name.as_ref(), 4_096, false);
            agg.add_cache(
                &cache.join(&name),
                "f".as_ref(),
                (i as u64 + 1) * 1_000,
                true,
            );
        }
        let out = agg.finish();
        assert_eq!(out.caches.len(), MAX_CACHE_ENTRIES);
        let last = MAX_CACHE_ENTRIES + 4;
        assert_eq!(out.caches[0].path, cache.join(format!("app{last:02}")));
        assert_eq!(out.caches[0].files, 1);
        assert_eq!(out.caches[0].bytes, 4_096 + (last as u64 + 1) * 1_000);
        assert!(out.caches.windows(2).all(|w| w[0].bytes >= w[1].bytes));
    }
}
