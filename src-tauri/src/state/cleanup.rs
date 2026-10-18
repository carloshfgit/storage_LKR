//! Ações de limpeza sobre o último scan: mover para a Lixeira, esvaziar a Lixeira e
//! limpar o Docker. Toda validação acontece aqui, nunca na UI (DECISIONS D19). As
//! operações de sistema (Lixeira, `docker`, `statvfs`) chegam como parâmetros, então
//! os testes rodam sem tocar na Lixeira nem no Docker de verdade.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use scanner::{
    Category, Classifier, DiskInfo, DockerMode, DockerTarget, DockerUsage, FileEntry, ScanResult,
};
use serde::Serialize;

use super::AppState;

#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    #[error("nenhuma varredura concluída ainda")]
    NoScan,
    #[error("aguarde o fim da varredura")]
    Busy,
    #[error("a ação precisa de confirmação")]
    NotConfirmed,
    #[error("esse Docker não foi encontrado na última varredura")]
    NoDocker,
    #[error("falha ao ler o disco: {0}")]
    Disk(#[from] io::Error),
    #[error("{0}")]
    Failed(String),
}

/// Um arquivo da lista dos maiores, com o veredito da mesma validação do `move_to_trash`.
#[derive(Debug, Clone, Serialize)]
pub struct FileItem {
    #[serde(flatten)]
    pub entry: FileEntry,
    pub trashable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Rejected {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrashReport {
    pub moved: u64,
    pub moved_bytes: u64,
    pub rejected: Vec<Rejected>,
    /// Último resultado já atualizado: a UI troca o dela por este.
    pub result: ScanResult,
}

#[derive(Debug, Clone, Serialize)]
pub struct Cleanup {
    /// Diferença do `f_bavail` antes e depois: o que o `df` também vai mostrar.
    pub freed_bytes: u64,
    pub result: ScanResult,
}

impl AppState {
    pub fn category_files(&self, category: Category) -> Result<Vec<FileItem>, ActionError> {
        let last = self.lock_last();
        let result = last.as_ref().ok_or(ActionError::NoScan)?;
        let top = result
            .categories
            .get(&category)
            .map(|c| c.top.as_slice())
            .unwrap_or_default();
        Ok(top
            .iter()
            .map(|entry| FileItem {
                trashable: check_trashable(result, &entry.path).is_ok(),
                entry: entry.clone(),
            })
            .collect())
    }

    /// Valida cada caminho e manda os aprovados para a Lixeira com `trash`. Os recusados
    /// voltam no relatório com o motivo; os outros seguem.
    pub fn move_to_trash(
        &self,
        paths: &[PathBuf],
        trash: impl Fn(&Path) -> Result<(), String>,
    ) -> Result<TrashReport, ActionError> {
        self.ensure_idle()?;
        let mut last = self.lock_last();
        let result = last.as_mut().ok_or(ActionError::NoScan)?;

        let mut moved = 0;
        let mut moved_bytes = 0;
        let mut rejected = Vec::new();
        for path in paths {
            match check_trashable(result, path).and_then(|()| trash(path)) {
                Ok(()) => {
                    if let Some(entry) = result.record_trashed(path) {
                        moved += 1;
                        moved_bytes += entry.bytes;
                    }
                }
                Err(reason) => rejected.push(Rejected {
                    path: path.to_string_lossy().into_owned(),
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

    /// Apaga permanentemente o conteúdo da Lixeira. O único apagamento do app (D17).
    pub fn empty_trash(
        &self,
        confirm: bool,
        purge: impl FnOnce() -> Result<(), String>,
        disk: impl Fn() -> io::Result<DiskInfo>,
    ) -> Result<Cleanup, ActionError> {
        self.measured_cleanup(confirm, purge, disk, |r, ()| r.clear_trash())
    }

    /// Limpa build cache ou imagens sem uso no daemon de `mode` e relê só ele para
    /// atualizar o card.
    pub fn docker_prune(
        &self,
        mode: DockerMode,
        target: DockerTarget,
        confirm: bool,
        run: impl FnOnce(DockerMode, DockerTarget) -> Result<(), String>,
        disk: impl Fn() -> io::Result<DiskInfo>,
        docker: impl FnOnce(DockerMode) -> Option<DockerUsage>,
    ) -> Result<Cleanup, ActionError> {
        if confirm
            && self
                .lock_last()
                .as_ref()
                .is_some_and(|r| r.docker.get(mode).is_none())
        {
            return Err(ActionError::NoDocker);
        }
        self.measured_cleanup(
            confirm,
            || run(mode, target).map(|()| docker(mode)),
            disk,
            |r, usage| r.docker.set(mode, usage),
        )
    }

    /// Roda `clean` entre duas leituras do disco, passa o que ela devolveu para `update`
    /// aplicar ao último resultado e recalcula o restante com o disco novo. O lock não
    /// fica preso durante a limpeza, que pode levar minutos.
    fn measured_cleanup<T>(
        &self,
        confirm: bool,
        clean: impl FnOnce() -> Result<T, String>,
        disk: impl Fn() -> io::Result<DiskInfo>,
        update: impl FnOnce(&mut ScanResult, T),
    ) -> Result<Cleanup, ActionError> {
        if !confirm {
            return Err(ActionError::NotConfirmed);
        }
        self.ensure_idle()?;
        if self.lock_last().is_none() {
            return Err(ActionError::NoScan);
        }

        let before = disk()?;
        let cleaned = clean().map_err(ActionError::Failed)?;
        let after = disk()?;
        let freed_bytes = after.available_bytes.saturating_sub(before.available_bytes);

        let mut last = self.lock_last();
        let result = last.as_mut().ok_or(ActionError::NoScan)?;
        update(result, cleaned);
        result.set_disk(after);
        self.persist(result);
        Ok(Cleanup {
            freed_bytes,
            result: result.clone(),
        })
    }

    /// Um scan em andamento vai substituir o resultado: agir sobre o antigo confundiria a UI.
    fn ensure_idle(&self) -> Result<(), ActionError> {
        if self.running.load(Ordering::SeqCst) {
            return Err(ActionError::Busy);
        }
        Ok(())
    }
}

/// Regras do `move_to_trash` (ARCHITECTURE §7): o arquivo consta no último scan, o caminho
/// não mudou desde então (sem links simbólicos no meio), fica sob o `$HOME`, não pertence
/// a sistema nem a app gerenciado e ainda não está na Lixeira.
fn check_trashable(result: &ScanResult, path: &Path) -> Result<(), String> {
    if result.find_file(path).is_none() {
        return Err("não está na última varredura".into());
    }
    let canonical = fs::canonicalize(path).map_err(|_| "o arquivo não existe mais".to_string())?;
    if canonical != path {
        return Err("o caminho mudou desde a varredura".into());
    }
    let home = fs::canonicalize(&result.home).unwrap_or_else(|_| result.home.clone());
    if home.as_os_str().is_empty() || canonical == home || !canonical.starts_with(&home) {
        return Err("fora da pasta pessoal".into());
    }
    let classifier = Classifier::new(&home);
    if classifier.is_app_path(&canonical) {
        return Err("pertence ao sistema ou a um app instalado".into());
    }
    if classifier.in_trash(&canonical) {
        return Err("já está na Lixeira".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::sync::Arc;

    use scanner::{DockerItem, DockerStatus, ScanOptions, scan};

    use super::*;

    struct Fixture {
        dir: PathBuf,
        home: PathBuf,
        state: Arc<AppState>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    /// `dir/` é a raiz do scan e `dir/home/` o `$HOME`, com um arquivo de cada caso.
    fn fixture(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("tauri-cleanup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let home = dir.join("home");
        let files = [
            home.join("foto.jpg"),
            home.join("doc.pdf"),
            home.join(".local/share/Trash/files/velho.mp4"),
            home.join("snap/app/musica.mp3"),
            home.join(".local/share/docker/volumes/db/_data/dump.pdf"),
            dir.join("fora.jpg"),
        ];
        for f in &files {
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, vec![0u8; 20_000]).unwrap();
        }

        let mut opts = ScanOptions::new(&dir);
        opts.home = home.clone();
        let result = scan(&opts, |_| {}).unwrap();
        let state = Arc::new(AppState::default());
        *state.lock_last() = Some(result);
        Fixture { dir, home, state }
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

    fn reason(report: &TrashReport, path: &Path) -> String {
        let path = path.to_string_lossy();
        report
            .rejected
            .iter()
            .find(|r| r.path == path)
            .map(|r| r.reason.clone())
            .unwrap_or_else(|| panic!("{path} não foi recusado: {report:?}"))
    }

    #[test]
    fn moves_valid_files_and_rejects_the_rest() {
        let f = fixture("move");
        let foto = f.home.join("foto.jpg");
        let fora = f.dir.join("fora.jpg");
        let lixeira = f.home.join(".local/share/Trash/files/velho.mp4");
        let snap = f.home.join("snap/app/musica.mp3");
        let rootless = f.home.join(".local/share/docker/volumes/db/_data/dump.pdf");
        let inventado = f.home.join("inventado.jpg");
        let before = f.state.last_result().unwrap();
        let photos_before = before.categories[&Category::Photos].bytes;

        let trashed = RefCell::new(Vec::new());
        let report = f
            .state
            .move_to_trash(
                &[
                    foto.clone(),
                    fora.clone(),
                    lixeira.clone(),
                    snap.clone(),
                    rootless.clone(),
                    inventado.clone(),
                ],
                |p| {
                    trashed.borrow_mut().push(p.to_path_buf());
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(*trashed.borrow(), std::slice::from_ref(&foto));
        assert_eq!(report.moved, 1);
        assert_eq!(reason(&report, &fora), "fora da pasta pessoal");
        assert_eq!(reason(&report, &lixeira), "já está na Lixeira");
        assert_eq!(
            reason(&report, &snap),
            "pertence ao sistema ou a um app instalado"
        );
        assert_eq!(
            reason(&report, &rootless),
            "pertence ao sistema ou a um app instalado"
        );
        assert_eq!(reason(&report, &inventado), "não está na última varredura");

        // Os bytes saem de Fotos e entram em Outros/Lixeira.
        let r = &report.result;
        assert_eq!(
            r.categories[&Category::Photos].bytes,
            photos_before - report.moved_bytes
        );
        assert_eq!(r.trash_bytes, before.trash_bytes + report.moved_bytes);
        assert!(r.find_file(&foto).is_none());
        // E o mesmo arquivo não pode ser movido de novo.
        let again = f
            .state
            .move_to_trash(std::slice::from_ref(&foto), |_| Ok(()))
            .unwrap();
        assert_eq!(reason(&again, &foto), "não está na última varredura");
    }

    #[test]
    fn changed_or_failed_files_are_rejected_without_accounting() {
        let f = fixture("changed");
        let doc = f.home.join("doc.pdf");
        let foto = f.home.join("foto.jpg");
        fs::remove_file(&doc).unwrap();

        let report = f
            .state
            .move_to_trash(
                &[doc.clone(), foto.clone()],
                |_| Err("sem permissão".into()),
            )
            .unwrap();
        assert_eq!(reason(&report, &doc), "o arquivo não existe mais");
        assert_eq!(reason(&report, &foto), "sem permissão");
        assert_eq!(report.moved, 0);
        assert!(report.result.find_file(&foto).is_some());
    }

    #[test]
    fn symlinked_parent_is_rejected() {
        let f = fixture("symlink");
        // A pasta do arquivo virou um link para fora do $HOME depois do scan.
        let pasta = f.home.join("pasta");
        fs::create_dir_all(&pasta).unwrap();
        fs::write(pasta.join("a.jpg"), vec![0u8; 20_000]).unwrap();
        let mut opts = ScanOptions::new(&f.dir);
        opts.home = f.home.clone();
        *f.state.lock_last() = Some(scan(&opts, |_| {}).unwrap());

        fs::remove_dir_all(&pasta).unwrap();
        fs::create_dir_all(f.dir.join("alvo")).unwrap();
        fs::write(f.dir.join("alvo/a.jpg"), b"x").unwrap();
        std::os::unix::fs::symlink(f.dir.join("alvo"), &pasta).unwrap();

        let report = f
            .state
            .move_to_trash(&[pasta.join("a.jpg")], |_| Ok(()))
            .unwrap();
        assert_eq!(
            reason(&report, &pasta.join("a.jpg")),
            "o caminho mudou desde a varredura"
        );
    }

    #[test]
    fn category_files_marks_trashable() {
        let f = fixture("files");
        let flag = |c: Category, p: PathBuf| {
            let files = f.state.category_files(c).unwrap();
            files.iter().find(|i| i.entry.path == p).unwrap().trashable
        };
        assert!(flag(Category::Photos, f.home.join("foto.jpg")));
        assert!(!flag(Category::AppsSystem, f.dir.join("fora.jpg")));
        assert!(!flag(
            Category::AppsSystem,
            f.home.join("snap/app/musica.mp3")
        ));
        assert!(!flag(
            Category::Other,
            f.home.join(".local/share/Trash/files/velho.mp4")
        ));
    }

    #[test]
    fn actions_are_refused_while_scanning() {
        let f = fixture("busy");
        f.state.running.store(true, Ordering::SeqCst);
        let err = f
            .state
            .move_to_trash(&[f.home.join("foto.jpg")], |_| Ok(()));
        assert!(matches!(err, Err(ActionError::Busy)));
    }

    #[test]
    fn empty_trash_needs_confirmation_and_measures_freed_space() {
        let f = fixture("empty");
        let available = Cell::new(100);

        let purged = Cell::new(false);
        let purge = || {
            purged.set(true);
            Ok(())
        };
        let err = f.state.empty_trash(false, purge, fake_disk(&available));
        assert!(matches!(err, Err(ActionError::NotConfirmed)));
        assert!(!purged.get());

        let cleanup = f
            .state
            .empty_trash(
                true,
                || {
                    available.set(100 + 40_000);
                    Ok(())
                },
                fake_disk(&available),
            )
            .unwrap();
        assert_eq!(cleanup.freed_bytes, 40_000);
        assert_eq!(cleanup.result.trash_bytes, 0);
        assert_eq!(cleanup.result.disk.available_bytes, 40_100);
        let lixeira = f.home.join(".local/share/Trash/files/velho.mp4");
        assert!(cleanup.result.find_file(&lixeira).is_none());
    }

    fn usage(images: u64) -> DockerUsage {
        DockerUsage {
            images: DockerItem {
                size: images,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Fixture com os dois daemons no último resultado.
    fn with_docker(f: &Fixture) {
        f.state.lock_last().as_mut().unwrap().docker = DockerStatus {
            rootful: Some(usage(10)),
            rootless: Some(usage(20)),
        };
    }

    #[test]
    fn docker_prune_runs_target_and_refreshes_usage() {
        let f = fixture("docker");
        with_docker(&f);
        let available = Cell::new(100);
        let ran = Cell::new(None);

        let err = f.state.docker_prune(
            DockerMode::Rootless,
            DockerTarget::BuildCache,
            false,
            |m, t| {
                ran.set(Some((m, t)));
                Ok(())
            },
            fake_disk(&available),
            |_| None,
        );
        assert!(matches!(err, Err(ActionError::NotConfirmed)));
        assert_eq!(ran.get(), None);

        let cleanup = f
            .state
            .docker_prune(
                DockerMode::Rootless,
                DockerTarget::UnusedImages,
                true,
                |m, t| {
                    ran.set(Some((m, t)));
                    available.set(700);
                    Ok(())
                },
                fake_disk(&available),
                |m| {
                    assert_eq!(m, DockerMode::Rootless, "relê só o daemon limpo");
                    Some(usage(5))
                },
            )
            .unwrap();
        assert_eq!(
            ran.get(),
            Some((DockerMode::Rootless, DockerTarget::UnusedImages))
        );
        assert_eq!(cleanup.freed_bytes, 600);
        let expected = DockerStatus {
            rootful: Some(usage(10)),
            rootless: Some(usage(5)),
        };
        assert_eq!(cleanup.result.docker, expected);
        assert_eq!(f.state.last_result().unwrap().docker, expected);
    }

    #[test]
    fn prune_of_missing_daemon_is_refused() {
        let f = fixture("prune-missing");
        let available = Cell::new(100);
        let err = f.state.docker_prune(
            DockerMode::Rootful,
            DockerTarget::BuildCache,
            true,
            |_, _| panic!("não deveria rodar o docker"),
            fake_disk(&available),
            |_| panic!("não deveria reler o Docker"),
        );
        assert!(matches!(err, Err(ActionError::NoDocker)));
    }

    #[test]
    fn failed_prune_leaves_result_untouched() {
        let f = fixture("prune-fail");
        with_docker(&f);
        let available = Cell::new(100);
        let err = f.state.docker_prune(
            DockerMode::Rootful,
            DockerTarget::BuildCache,
            true,
            |_, _| Err("daemon parado".into()),
            fake_disk(&available),
            |_| panic!("não deveria reler o Docker"),
        );
        assert!(matches!(err, Err(ActionError::Failed(m)) if m == "daemon parado"));
        assert_eq!(
            f.state.last_result().unwrap().docker.rootful,
            Some(usage(10))
        );
    }
}
