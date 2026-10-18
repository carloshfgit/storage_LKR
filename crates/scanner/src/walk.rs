use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jwalk::{Parallelism, WalkDirGeneric};

use crate::classify::Classifier;
use crate::disk::disk_info;
use crate::model::{Category, CategoryStats, FileEntry, ScanProgress, ScanResult};
use crate::sniff::{MIN_SNIFF_BYTES, sniff};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("falha ao ler {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("varredura cancelada")]
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub root: PathBuf,
    pub home: PathBuf,
    /// Quantos arquivos guardar por categoria na lista dos maiores.
    pub top_n: usize,
    pub threads: usize,
    /// Lê os magic bytes dos arquivos sem extensão no `$HOME` (regra 6b).
    pub sniff: bool,
    pub cancel: Arc<AtomicBool>,
}

impl ScanOptions {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        Self {
            root: root.into(),
            home: std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default(),
            top_n: 200,
            // stat é limitado por I/O: mais threads que núcleos mantém a fila do SSD cheia.
            threads: threads * 2,
            sniff: true,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Metadados de uma entrada, calculados nas threads do walk.
/// `None` significa que a entrada não conta (raiz ou outro filesystem).
#[derive(Debug, Clone, Copy)]
struct Counted {
    category: Category,
    bytes: u64,
    modified: i64,
    ino: u64,
    nlink: u64,
    in_trash: bool,
    /// A categoria veio do conteúdo, não do caminho.
    sniffed: bool,
}

type Walk = WalkDirGeneric<((), Option<Counted>)>;

/// Percorre `opts.root` sem cruzar filesystems nem seguir symlinks e agrega o
/// espaço alocado por categoria. `on_progress` é chamado a cada ~100 ms.
pub fn scan(
    opts: &ScanOptions,
    mut on_progress: impl FnMut(&ScanProgress),
) -> Result<ScanResult, ScanError> {
    let started = Instant::now();
    let io_err = |source| ScanError::Io {
        path: opts.root.clone(),
        source,
    };

    let disk = disk_info(&opts.root).map_err(io_err)?;
    let root_meta = fs::symlink_metadata(&opts.root).map_err(io_err)?;
    let root_dev = root_meta.dev();

    let classifier = Arc::new(Classifier::new(&opts.home));
    let walk_classifier = Arc::clone(&classifier);
    let use_sniff = opts.sniff;
    let walk = Walk::new(&opts.root)
        .skip_hidden(false)
        .follow_links(false)
        .parallelism(Parallelism::RayonNewPool(opts.threads))
        .process_read_dir(move |depth, _dir, _state, children| {
            // depth == None: chamada que entrega a própria raiz, contada à parte.
            if depth.is_none() {
                return;
            }
            for child in children.iter_mut().flatten() {
                let path = child.parent_path.join(&child.file_name);
                let Ok(meta) = fs::symlink_metadata(&path) else {
                    child.read_children = None;
                    continue;
                };
                if meta.dev() != root_dev {
                    // Ponto de montagem de outro filesystem (/proc, /run, /snap/*, pendrives).
                    child.read_children = None;
                    continue;
                }
                // 6b. Sem extensão no $HOME: o conteúdo decide. A leitura roda aqui, nas
                // threads do walk, junto com o stat.
                let by_content = || {
                    (use_sniff && meta.is_file() && meta.len() >= MIN_SNIFF_BYTES)
                        .then(|| sniff(&path))
                        .flatten()
                };
                let (category, sniffed) = match walk_classifier.classify_by_path(&path) {
                    Some(c) => (c, false),
                    None => by_content().map_or((Category::Other, false), |c| (c, true)),
                };
                child.client_state = Some(Counted {
                    category,
                    bytes: meta.blocks() * 512,
                    modified: meta.mtime(),
                    ino: meta.ino(),
                    nlink: meta.nlink(),
                    in_trash: walk_classifier.in_trash(&path),
                    sniffed,
                });
            }
        });

    let mut agg = Aggregator::new(opts.top_n);
    agg.add_bytes(classifier.classify(&opts.root), root_meta.blocks() * 512);

    let mut seen_hardlinks: HashSet<u64> = HashSet::new();
    let mut denied_dirs = 0u64;
    let mut last_progress = Instant::now();

    for entry in walk {
        if opts.cancel.load(Ordering::Relaxed) {
            return Err(ScanError::Cancelled);
        }
        // Erros de entrada individual (arquivo sumiu durante o scan etc.) são ignorados.
        let Ok(entry) = entry else { continue };

        if entry
            .read_children
            .as_ref()
            .is_some_and(|r| r.error().is_some())
        {
            denied_dirs += 1;
        }
        let Some(c) = entry.client_state else {
            continue;
        };

        let is_dir = entry.file_type.is_dir();
        // Mesmo filesystem em todo o walk, então o inode sozinho identifica o arquivo.
        if !is_dir && c.nlink > 1 && !seen_hardlinks.insert(c.ino) {
            continue;
        }

        if c.sniffed {
            agg.sniffed_files += 1;
            agg.sniffed_bytes += c.bytes;
        }
        if c.in_trash {
            agg.trash_bytes += c.bytes;
            agg.trash_files += u64::from(!is_dir);
        }
        if is_dir {
            agg.add_bytes(c.category, c.bytes);
        } else {
            agg.add_file(c, || entry.parent_path.join(&entry.file_name));
        }

        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            on_progress(&agg.progress());
            last_progress = Instant::now();
        }
    }

    on_progress(&agg.progress());
    let files = agg.files;
    let trash_bytes = agg.trash_bytes;
    let trash_files = agg.trash_files;
    let (sniffed_files, sniffed_bytes) = (agg.sniffed_files, agg.sniffed_bytes);
    let categories = agg.finish();
    let scanned: u64 = categories.values().map(|c| c.bytes).sum();

    Ok(ScanResult {
        unaccounted_bytes: disk.used_bytes().saturating_sub(scanned),
        disk,
        categories,
        trash_bytes,
        trash_files,
        sniffed_files,
        sniffed_bytes,
        home: opts.home.clone(),
        docker: None,
        docker_bytes: 0,
        denied_dirs,
        files,
        elapsed_ms: started.elapsed().as_millis() as u64,
        finished_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64),
    })
}

/// Item do heap de maiores arquivos, ordenado por tamanho.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct TopItem {
    bytes: u64,
    path: PathBuf,
    modified: i64,
}

struct Aggregator {
    top_n: usize,
    bytes: [u64; Category::COUNT],
    files_per_cat: [u64; Category::COUNT],
    files: u64,
    trash_bytes: u64,
    trash_files: u64,
    sniffed_files: u64,
    sniffed_bytes: u64,
    /// Min-heap: o topo é o menor dos N maiores, descartado quando aparece um maior.
    top: [BinaryHeap<Reverse<TopItem>>; Category::COUNT],
}

impl Aggregator {
    fn new(top_n: usize) -> Self {
        Self {
            top_n,
            bytes: [0; Category::COUNT],
            files_per_cat: [0; Category::COUNT],
            files: 0,
            trash_bytes: 0,
            trash_files: 0,
            sniffed_files: 0,
            sniffed_bytes: 0,
            top: Default::default(),
        }
    }

    fn add_bytes(&mut self, category: Category, bytes: u64) {
        self.bytes[category.index()] += bytes;
    }

    /// `path` só é montado quando o arquivo entra no top-N, evitando alocar para todos.
    fn add_file(&mut self, c: Counted, path: impl FnOnce() -> PathBuf) {
        let i = c.category.index();
        self.bytes[i] += c.bytes;
        self.files_per_cat[i] += 1;
        self.files += 1;

        let heap = &mut self.top[i];
        if heap.len() < self.top_n {
            heap.push(Reverse(TopItem {
                bytes: c.bytes,
                path: path(),
                modified: c.modified,
            }));
        } else if heap.peek().is_some_and(|Reverse(min)| c.bytes > min.bytes) {
            heap.pop();
            heap.push(Reverse(TopItem {
                bytes: c.bytes,
                path: path(),
                modified: c.modified,
            }));
        }
    }

    fn progress(&self) -> ScanProgress {
        ScanProgress {
            bytes: Category::ALL
                .iter()
                .map(|&c| (c, self.bytes[c.index()]))
                .collect(),
            files: self.files,
        }
    }

    fn finish(self) -> HashMap<Category, CategoryStats> {
        let Self {
            bytes,
            files_per_cat,
            top,
            ..
        } = self;
        Category::ALL
            .into_iter()
            .zip(top)
            .map(|(c, heap)| {
                // into_sorted_vec de Reverse = ordem decrescente de tamanho.
                let top = heap
                    .into_sorted_vec()
                    .into_iter()
                    .map(|Reverse(t)| FileEntry {
                        path: t.path,
                        bytes: t.bytes,
                        modified: t.modified,
                    })
                    .collect();
                let stats = CategoryStats {
                    bytes: bytes[c.index()],
                    files: files_per_cat[c.index()],
                    top,
                };
                (c, stats)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("scanner-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, len: usize) {
        fs::write(path, vec![1u8; len]).unwrap();
    }

    #[test]
    fn aggregates_by_category_and_dedupes_hardlinks() {
        let dir = temp_dir("agg");
        fs::create_dir(dir.join("sub")).unwrap();
        write(&dir.join("a.jpg"), 10_000);
        write(&dir.join("sub/b.mp4"), 50_000);
        write(&dir.join("c.bin"), 5_000);
        fs::hard_link(dir.join("sub/b.mp4"), dir.join("b-link.mp4")).unwrap();
        std::os::unix::fs::symlink(dir.join("a.jpg"), dir.join("link.jpg")).unwrap();

        let mut opts = ScanOptions::new(&dir);
        opts.home = dir.clone();
        let r = scan(&opts, |_| {}).unwrap();

        let photos = &r.categories[&Category::Photos];
        let videos = &r.categories[&Category::Videos];
        // a.jpg conta; o symlink link.jpg também é uma entrada, mas não ocupa blocos.
        assert_eq!(photos.top[0].path, dir.join("a.jpg"));
        assert!(photos.bytes >= 10_000);
        // Hardlink: o vídeo só conta uma vez.
        assert_eq!(videos.files, 1);
        assert!(videos.bytes >= 50_000 && videos.bytes < 100_000);
        assert_eq!(r.denied_dirs, 0);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn top_n_keeps_largest_in_descending_order() {
        let dir = temp_dir("top");
        for (i, len) in [4_096usize, 40_960, 409_600, 8_192].into_iter().enumerate() {
            write(&dir.join(format!("{i}.pdf")), len);
        }
        let mut opts = ScanOptions::new(&dir);
        opts.home = dir.clone();
        opts.top_n = 2;
        let r = scan(&opts, |_| {}).unwrap();

        let docs = &r.categories[&Category::Documents];
        assert_eq!(docs.files, 4);
        let names: Vec<_> = docs
            .top
            .iter()
            .map(|f| f.path.file_name().unwrap().to_owned())
            .collect();
        assert_eq!(names, ["2.pdf", "1.pdf"]);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn classifies_by_path_and_counts_trash() {
        let dir = temp_dir("rules");
        let home = dir.join("home");
        let trash = home.join(".local/share/Trash/files");
        fs::create_dir_all(&trash).unwrap();
        fs::create_dir_all(home.join(".cache")).unwrap();
        fs::create_dir_all(dir.join("usr")).unwrap();
        write(&trash.join("velho.mp4"), 40_000);
        write(&home.join(".cache/thumb.jpg"), 8_000);
        write(&home.join("foto.jpg"), 8_000);
        write(&dir.join("usr/icone.png"), 8_000);

        let mut opts = ScanOptions::new(&dir);
        opts.home = home.clone();
        let r = scan(&opts, |_| {}).unwrap();

        let files = |c: Category| -> Vec<PathBuf> {
            r.categories[&c]
                .top
                .iter()
                .map(|f| f.path.clone())
                .collect()
        };
        assert_eq!(files(Category::Photos), [home.join("foto.jpg")]);
        assert!(files(Category::Other).contains(&trash.join("velho.mp4")));
        assert!(files(Category::Other).contains(&home.join(".cache/thumb.jpg")));
        // Fora do $HOME de teste: Apps/Sistema.
        assert!(files(Category::AppsSystem).contains(&dir.join("usr/icone.png")));
        // A Lixeira conta o vídeo e os diretórios Trash/ e files/.
        assert!(r.trash_bytes >= 40_000 && r.trash_bytes <= r.categories[&Category::Other].bytes);
        assert!(r.trash_files >= 1 && r.trash_files <= r.categories[&Category::Other].files);
        assert_eq!(r.home, home);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn files_without_extension_are_classified_by_content() {
        let dir = temp_dir("sniff");
        fs::create_dir_all(dir.join(".cache")).unwrap();
        fs::create_dir_all(dir.join("proj/node_modules")).unwrap();
        let mut jpeg = b"\xFF\xD8\xFF\xE0".to_vec();
        jpeg.resize(8_192, 0);
        for p in ["foto", ".cache/foto", "proj/node_modules/foto"] {
            fs::write(dir.join(p), &jpeg).unwrap();
        }
        // Abaixo de MIN_SNIFF_BYTES: nem é lido.
        fs::write(dir.join("mini"), &jpeg[..1_000]).unwrap();

        let mut opts = ScanOptions::new(&dir);
        opts.home = dir.clone();
        let r = scan(&opts, |_| {}).unwrap();
        let photos: Vec<_> = r.categories[&Category::Photos]
            .top
            .iter()
            .map(|f| f.path.clone())
            .collect();
        assert_eq!(photos, [dir.join("foto")]);
        assert_eq!(r.sniffed_files, 1);
        assert_eq!(r.sniffed_bytes, r.categories[&Category::Photos].bytes);
        let other = &r.categories[&Category::Other].top;
        for p in [".cache/foto", "proj/node_modules/foto", "mini"] {
            assert!(other.iter().any(|f| f.path == dir.join(p)), "{p}");
        }

        // Desligado: tudo fica em Outros, e o total escaneado não muda.
        opts.sniff = false;
        let off = scan(&opts, |_| {}).unwrap();
        assert_eq!(off.categories[&Category::Photos].files, 0);
        assert_eq!(off.sniffed_files, 0);
        assert_eq!(off.scanned_bytes(), r.scanned_bytes());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancel_stops_scan() {
        let dir = temp_dir("cancel");
        write(&dir.join("x.txt"), 10);
        let opts = ScanOptions::new(&dir);
        opts.cancel.store(true, Ordering::Relaxed);
        assert!(matches!(scan(&opts, |_| {}), Err(ScanError::Cancelled)));
        fs::remove_dir_all(&dir).unwrap();
    }
}
