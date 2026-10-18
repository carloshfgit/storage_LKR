//! Itens da Lixeira: listar com o tamanho, restaurar e apagar os escolhidos. A UI só manda
//! de volta o `id` que recebeu; cada um é conferido com a listagem do sistema relida na
//! hora, e nada vindo da UI vira caminho (DECISIONS D41). A listagem, a restauração e o
//! apagamento chegam como parâmetros, como nas outras ações.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use scanner::{DiskInfo, ScanResult};
use serde::Serialize;

use super::AppState;
use super::cleanup::{ActionError, Rejected, TrashReport};

/// Item da Lixeira como o sistema o lista (o `.trashinfo`).
#[derive(Debug, Clone, PartialEq)]
pub struct TrashListing {
    /// Caminho do `.trashinfo`.
    pub id: PathBuf,
    pub name: OsString,
    pub original_parent: PathBuf,
    /// Segundos Unix.
    pub deleted_at: i64,
}

impl TrashListing {
    pub fn original_path(&self) -> PathBuf {
        self.original_parent.join(&self.name)
    }

    /// Onde o item está guardado: `<lixeira>/files/<nome do .trashinfo sem a extensão>`.
    pub fn stored_path(&self) -> PathBuf {
        let trash = self
            .id
            .parent()
            .and_then(Path::parent)
            .unwrap_or(Path::new(""));
        trash
            .join("files")
            .join(self.id.file_stem().unwrap_or_default())
    }
}

/// Um item para a UI, com o tamanho do scan (ou medido agora, se o scan não o viu).
#[derive(Debug, Clone, Serialize)]
pub struct TrashItem {
    /// O que a UI manda de volta para restaurar ou apagar.
    pub id: String,
    pub name: String,
    pub original_path: String,
    pub stored_path: String,
    pub deleted_at: i64,
    pub bytes: u64,
    pub files: u64,
    /// Contado no último scan, ou seja, dentro do `trash_bytes` da barra.
    pub counted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PurgeReport {
    pub purged: u64,
    /// Diferença do `f_bavail` antes e depois, como no `empty_trash`.
    pub freed_bytes: u64,
    pub rejected: Vec<Rejected>,
    pub result: ScanResult,
}

impl AppState {
    /// A Lixeira de agora, os maiores primeiro. O tamanho vem do último scan quando ele
    /// contou o item; senão, `size` mede (algo movido para lá por fora, ou de outro disco).
    pub fn trash_items(
        &self,
        list: impl FnOnce() -> Result<Vec<TrashListing>, String>,
        size: impl Fn(&Path) -> (u64, u64),
    ) -> Result<Vec<TrashItem>, ActionError> {
        let listing = list().map_err(ActionError::Failed)?;
        let counted: Vec<_> = {
            let last = self.lock_last();
            listing
                .iter()
                .map(|l| {
                    last.as_ref()
                        .and_then(|r| r.find_trash_item(&l.stored_path(), &l.original_path()))
                        .map(|t| (t.bytes, t.files))
                })
                .collect()
        };
        let mut items: Vec<TrashItem> = listing
            .iter()
            .zip(counted)
            .map(|(l, counted)| {
                let stored = l.stored_path();
                let (bytes, files) = counted.unwrap_or_else(|| size(&stored));
                TrashItem {
                    id: l.id.to_string_lossy().into_owned(),
                    name: l.name.to_string_lossy().into_owned(),
                    original_path: l.original_path().to_string_lossy().into_owned(),
                    stored_path: stored.to_string_lossy().into_owned(),
                    deleted_at: l.deleted_at,
                    bytes,
                    files,
                    counted: counted.is_some(),
                }
            })
            .collect();
        items.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
        Ok(items)
    }

    /// Devolve os itens ao local original com `restore`, que nunca sobrescreve: um nome já
    /// ocupado lá volta como recusado. Os bytes saem da Lixeira e voltam para a categoria
    /// do caminho original.
    pub fn restore_from_trash(
        &self,
        ids: &[String],
        list: impl FnOnce() -> Result<Vec<TrashListing>, String>,
        size: impl Fn(&Path) -> (u64, u64),
        restore: impl Fn(&TrashListing) -> Result<(), String>,
    ) -> Result<TrashReport, ActionError> {
        self.ensure_idle()?;
        let listing = list().map_err(ActionError::Failed)?;
        let mut last = self.lock_last();
        let result = last.as_mut().ok_or(ActionError::NoScan)?;

        let (found, mut rejected) = select(&listing, ids);
        let mut moved = 0;
        let mut moved_bytes = 0;
        for item in found {
            let (stored, original) = (item.stored_path(), item.original_path());
            let bytes = result
                .find_trash_item(&stored, &original)
                .map_or_else(|| size(&stored).0, |t| t.bytes);
            match restore(item) {
                Ok(()) => {
                    result.record_restored(&stored, &original);
                    moved += 1;
                    moved_bytes += bytes;
                }
                Err(reason) => rejected.push(Rejected {
                    path: original.to_string_lossy().into_owned(),
                    reason,
                }),
            }
        }
        if moved > 0 {
            self.persist(result);
        }
        Ok(TrashReport {
            moved,
            moved_bytes,
            rejected,
            result: result.clone(),
        })
    }

    /// Apaga permanentemente só os itens escolhidos. Exige `confirm == true` e mede o
    /// liberado pelo disco, como o `empty_trash`.
    pub fn purge_from_trash(
        &self,
        ids: &[String],
        confirm: bool,
        list: impl FnOnce() -> Result<Vec<TrashListing>, String>,
        purge: impl Fn(&TrashListing) -> Result<(), String>,
        disk: impl Fn() -> io::Result<DiskInfo>,
    ) -> Result<PurgeReport, ActionError> {
        let mut rejected = Vec::new();
        let mut purged = 0;
        let cleanup = self.measured_cleanup(
            confirm,
            || {
                let listing = list()?;
                let (found, refused) = select(&listing, ids);
                rejected = refused;
                let mut done = Vec::new();
                for item in found {
                    match purge(item) {
                        Ok(()) => done.push((item.stored_path(), item.original_path())),
                        Err(reason) => rejected.push(Rejected {
                            path: item.original_path().to_string_lossy().into_owned(),
                            reason,
                        }),
                    }
                }
                Ok(done)
            },
            disk,
            |r, done| {
                purged = done.len() as u64;
                for (stored, original) in &done {
                    r.record_purged(stored, original);
                }
            },
        )?;
        Ok(PurgeReport {
            purged,
            freed_bytes: cleanup.freed_bytes,
            rejected,
            result: cleanup.result,
        })
    }
}

/// Os itens da listagem com os `ids` pedidos, uma vez cada. Um id que não está na listagem
/// (inventado, ou já restaurado ou apagado por fora) volta recusado.
fn select<'a>(
    listing: &'a [TrashListing],
    ids: &[String],
) -> (Vec<&'a TrashListing>, Vec<Rejected>) {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    let mut rejected = Vec::new();
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        // Comparado como a UI o recebeu; um nome não UTF-8 que colida com outro é recusado.
        let mut matches = listing.iter().filter(|l| l.id.to_string_lossy() == *id);
        match (matches.next(), matches.next()) {
            (Some(item), None) => found.push(item),
            _ => rejected.push(Rejected {
                path: id.clone(),
                reason: "não está mais na Lixeira".into(),
            }),
        }
    }
    (found, rejected)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use scanner::{Category, ScanOptions, scan, tree_size};

    use super::*;

    struct Fixture {
        dir: PathBuf,
        home: PathBuf,
        trash: PathBuf,
        state: Arc<AppState>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    /// `$HOME` com uma Lixeira de verdade no formato freedesktop: uma foto e uma pasta,
    /// cada uma com o seu `.trashinfo`.
    fn fixture(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("tauri-trash-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let home = dir.join("home");
        let trash = home.join(".local/share/Trash");
        fs::create_dir_all(trash.join("files/pasta")).unwrap();
        fs::create_dir_all(trash.join("info")).unwrap();
        fs::create_dir_all(home.join("Imagens")).unwrap();
        fs::write(trash.join("files/foto.jpg"), vec![0u8; 40_000]).unwrap();
        fs::write(trash.join("files/pasta/a.pdf"), vec![0u8; 20_000]).unwrap();
        for n in ["foto.jpg", "pasta"] {
            fs::write(trash.join(format!("info/{n}.trashinfo")), "[Trash Info]\n").unwrap();
        }

        let mut opts = ScanOptions::new(&dir);
        opts.home = home.clone();
        let state = Arc::new(AppState::default());
        *state.lock_last() = Some(scan(&opts, |_| {}).unwrap());
        Fixture {
            dir,
            home,
            trash,
            state,
        }
    }

    impl Fixture {
        fn listing(&self, name: &str, original_parent: &Path) -> TrashListing {
            TrashListing {
                id: self.trash.join(format!("info/{name}.trashinfo")),
                name: name.into(),
                original_parent: original_parent.to_path_buf(),
                deleted_at: 1_700_000_000,
            }
        }

        /// O que o sistema listaria agora.
        fn system(&self) -> Vec<TrashListing> {
            vec![
                self.listing("foto.jpg", &self.home.join("Imagens")),
                self.listing("pasta", &self.home),
            ]
        }

        fn id(&self, name: &str) -> String {
            self.trash
                .join(format!("info/{name}.trashinfo"))
                .to_string_lossy()
                .into_owned()
        }
    }

    fn fake_disk(available: &Cell<u64>) -> impl Fn() -> io::Result<DiskInfo> + '_ {
        || {
            Ok(DiskInfo {
                mount: "/".into(),
                device: String::new(),
                total_bytes: 1_000_000_000,
                available_bytes: available.get(),
            })
        }
    }

    fn no_size(_: &Path) -> (u64, u64) {
        panic!("o scan já contou esse item")
    }

    /// Soma tudo o que a barra mostra, menos o livre.
    fn bar(r: &ScanResult) -> u64 {
        r.scanned_bytes() + r.docker_bytes + r.unaccounted_bytes
    }

    #[test]
    fn lists_items_with_scanned_sizes() {
        let f = fixture("list");
        // Um item que o scan não viu: o tamanho é medido na hora.
        let mut system = f.system();
        let novo = f.listing("novo.iso", &f.home);
        fs::write(novo.stored_path(), vec![0u8; 90_000]).unwrap();
        system.push(novo.clone());

        let items = f.state.trash_items(|| Ok(system), tree_size).unwrap();
        let names: Vec<_> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["novo.iso", "foto.jpg", "pasta"]);
        assert!(!items[0].counted);
        assert_eq!(
            (items[0].bytes, items[0].files),
            tree_size(&novo.stored_path())
        );
        assert!(items[1].counted && items[2].counted);
        assert_eq!(items[2].files, 1);
        assert_eq!(
            items[1].original_path,
            f.home.join("Imagens/foto.jpg").to_string_lossy()
        );
        assert_eq!(
            items[1].stored_path,
            f.trash.join("files/foto.jpg").to_string_lossy()
        );
    }

    #[test]
    fn restore_moves_bytes_back_to_the_category() {
        let f = fixture("restore");
        let before = f.state.last_result().unwrap();
        let restored = RefCell::new(Vec::new());
        let report = f
            .state
            .restore_from_trash(
                &[f.id("foto.jpg"), f.id("foto.jpg"), f.id("inventado")],
                || Ok(f.system()),
                no_size,
                |l| {
                    restored.borrow_mut().push(l.name.clone());
                    Ok(())
                },
            )
            .unwrap();

        // Pedido duas vezes, restaurado uma.
        assert_eq!(*restored.borrow(), [OsString::from("foto.jpg")]);
        assert_eq!(report.moved, 1);
        assert_eq!(report.rejected.len(), 1);
        assert_eq!(report.rejected[0].reason, "não está mais na Lixeira");

        let r = &report.result;
        assert_eq!(r.trash_bytes, before.trash_bytes - report.moved_bytes);
        assert_eq!(
            r.categories[&Category::Photos].bytes,
            before.categories[&Category::Photos].bytes + report.moved_bytes
        );
        assert!(report.moved_bytes >= 40_000);
        assert_eq!(bar(r), bar(&before));
        assert_eq!(r.trash_items.len(), 1);
    }

    #[test]
    fn restore_collision_is_rejected_without_accounting() {
        let f = fixture("collision");
        let before = f.state.last_result().unwrap();
        let report = f
            .state
            .restore_from_trash(
                &[f.id("pasta")],
                || Ok(f.system()),
                no_size,
                |_| Err("já existe um item com esse nome no local original".into()),
            )
            .unwrap();
        assert_eq!(report.moved, 0);
        assert_eq!(
            report.rejected[0].path,
            f.home.join("pasta").to_string_lossy()
        );
        assert_eq!(report.result.trash_bytes, before.trash_bytes);
    }

    #[test]
    fn purge_needs_confirmation_and_measures_freed_space() {
        let f = fixture("purge");
        let available = Cell::new(100);
        let err = f.state.purge_from_trash(
            &[f.id("foto.jpg")],
            false,
            || panic!("não deveria listar"),
            |_| panic!("não deveria apagar"),
            fake_disk(&available),
        );
        assert!(matches!(err, Err(ActionError::NotConfirmed)));

        let before = f.state.last_result().unwrap();
        let report = f
            .state
            .purge_from_trash(
                &[f.id("pasta"), f.id("inventado")],
                true,
                || Ok(f.system()),
                |l| {
                    assert_eq!(l.name, "pasta");
                    available.set(100 + 24_000);
                    Ok(())
                },
                fake_disk(&available),
            )
            .unwrap();
        assert_eq!(report.purged, 1);
        assert_eq!(report.freed_bytes, 24_000);
        assert_eq!(report.rejected.len(), 1);
        let r = &report.result;
        assert!(r.trash_bytes < before.trash_bytes);
        assert_eq!(r.trash_items.len(), 1);
        assert_eq!(r.disk.available_bytes, 24_100);
        // A barra continua fechando o disco: usado = escaneado + Docker + restante.
        assert_eq!(bar(r), r.disk.used_bytes());
    }

    #[test]
    fn actions_are_refused_while_scanning() {
        let f = fixture("busy");
        f.state.running.store(true, Ordering::SeqCst);
        let restore = f.state.restore_from_trash(
            &[f.id("foto.jpg")],
            || Ok(f.system()),
            no_size,
            |_| panic!("não deveria restaurar"),
        );
        assert!(matches!(restore, Err(ActionError::Busy)));
        let available = Cell::new(0);
        let purge = f.state.purge_from_trash(
            &[f.id("foto.jpg")],
            true,
            || Ok(f.system()),
            |_| panic!("não deveria apagar"),
            fake_disk(&available),
        );
        assert!(matches!(purge, Err(ActionError::Busy)));
    }
}
