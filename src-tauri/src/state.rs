//! Estado do app e ciclo de vida de uma varredura, sem dependência do Tauri
//! (os commands só repassam eventos para o `Channel`), o que permite testar tudo
//! com `cargo test`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use scanner::{DockerUsage, ScanError, ScanOptions, ScanProgress, ScanResult, scan};
use serde::Serialize;

/// Mensagens enviadas ao frontend durante uma varredura, nesta ordem:
/// `progress`* → (`done` → `docker`) | `cancelled` | `error`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScanEvent {
    Progress(ScanProgress),
    Done {
        result: ScanResult,
    },
    /// Chega depois do `done`: `docker system df` leva ~5 s e não pode atrasar a barra.
    Docker {
        usage: Option<DockerUsage>,
        docker_bytes: u64,
        unaccounted_bytes: u64,
    },
    Cancelled,
    Error {
        message: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("já existe uma varredura em andamento")]
    AlreadyRunning,
}

pub type DockerQuery = Box<dyn FnOnce() -> Option<DockerUsage> + Send>;

#[derive(Default)]
pub struct AppState {
    last: Mutex<Option<ScanResult>>,
    cancel: Arc<AtomicBool>,
    running: AtomicBool,
    /// Incrementa a cada varredura; descarta o resultado do Docker de uma varredura antiga.
    generation: AtomicU64,
}

impl AppState {
    /// Inicia a varredura numa thread própria e retorna na hora. `docker` roda depois do
    /// `done`; um novo scan pode começar enquanto ele ainda está rodando.
    pub fn start_scan(
        self: &Arc<Self>,
        mut opts: ScanOptions,
        docker: DockerQuery,
        send: impl Fn(ScanEvent) + Send + 'static,
    ) -> Result<(), StartError> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(StartError::AlreadyRunning);
        }
        // Zerado antes de retornar: um cancel_scan logo após o start já vale para este scan.
        self.cancel.store(false, Ordering::SeqCst);
        opts.cancel = Arc::clone(&self.cancel);
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        let state = Arc::clone(self);
        std::thread::spawn(move || {
            let outcome = scan(&opts, |p| send(ScanEvent::Progress(p.clone())));
            state.running.store(false, Ordering::SeqCst);

            match outcome {
                Ok(result) => {
                    *state.lock_last() = Some(result.clone());
                    send(ScanEvent::Done { result });
                }
                Err(ScanError::Cancelled) => return send(ScanEvent::Cancelled),
                Err(e) => {
                    return send(ScanEvent::Error {
                        message: e.to_string(),
                    });
                }
            }

            let usage = docker();
            let mut last = state.lock_last();
            let Some(result) = last.as_mut() else { return };
            if state.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            result.apply_docker(usage.clone());
            send(ScanEvent::Docker {
                usage,
                docker_bytes: result.docker_bytes,
                unaccounted_bytes: result.unaccounted_bytes,
            });
        });
        Ok(())
    }

    pub fn cancel_scan(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn last_result(&self) -> Option<ScanResult> {
        self.lock_last().clone()
    }

    fn lock_last(&self) -> MutexGuard<'_, Option<ScanResult>> {
        self.last.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::time::Duration;

    use scanner::Category;

    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(30);

    fn temp_home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tauri-state-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("foto.jpg"), vec![0u8; 20_000]).unwrap();
        dir
    }

    fn options(root: &Path) -> ScanOptions {
        let mut opts = ScanOptions::new(root);
        opts.home = root.to_path_buf();
        opts
    }

    fn fake_docker(total: u64) -> DockerQuery {
        Box::new(move || {
            Some(DockerUsage {
                images: total,
                ..Default::default()
            })
        })
    }

    /// Coleta eventos até o terminal esperado (`docker`, `cancelled` ou `error`).
    fn collect(rx: &mpsc::Receiver<ScanEvent>) -> Vec<ScanEvent> {
        let mut events = Vec::new();
        loop {
            let e = rx.recv_timeout(TIMEOUT).expect("evento não chegou");
            let last = matches!(
                e,
                ScanEvent::Docker { .. } | ScanEvent::Cancelled | ScanEvent::Error { .. }
            );
            events.push(e);
            if last {
                return events;
            }
        }
    }

    #[test]
    fn emits_progress_done_then_docker() {
        let dir = temp_home("flow");
        let state = Arc::new(AppState::default());
        let (tx, rx) = mpsc::channel();

        state
            .start_scan(options(&dir), fake_docker(1_000), move |e| {
                tx.send(e).unwrap()
            })
            .unwrap();
        let events = collect(&rx);

        let kinds: Vec<_> = events
            .iter()
            .map(|e| match e {
                ScanEvent::Progress(_) => "progress",
                ScanEvent::Done { .. } => "done",
                ScanEvent::Docker { .. } => "docker",
                ScanEvent::Cancelled => "cancelled",
                ScanEvent::Error { .. } => "error",
            })
            .collect();
        assert!(kinds.len() >= 3 && kinds[..kinds.len() - 2].iter().all(|k| *k == "progress"));
        assert_eq!(kinds[kinds.len() - 2..], ["done", "docker"]);

        let ScanEvent::Docker { docker_bytes, .. } = events.last().unwrap() else {
            unreachable!()
        };
        assert_eq!(*docker_bytes, 1_000);

        let last = state.last_result().unwrap();
        assert_eq!(last.docker_bytes, 1_000);
        assert_eq!(
            last.categories[&Category::Photos].top[0].path,
            dir.join("foto.jpg")
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancel_right_after_start_cancels_and_allows_restart() {
        let state = Arc::new(AppState::default());
        let (tx, rx) = mpsc::channel();

        // `/` é grande o bastante para o cancelamento chegar antes do fim.
        let tx2 = tx.clone();
        state
            .start_scan(ScanOptions::new("/"), fake_docker(0), move |e| {
                tx2.send(e).unwrap()
            })
            .unwrap();
        state.cancel_scan();
        assert!(matches!(collect(&rx).last(), Some(ScanEvent::Cancelled)));
        assert!(state.last_result().is_none());

        let dir = temp_home("restart");
        state
            .start_scan(options(&dir), fake_docker(0), move |e| tx.send(e).unwrap())
            .unwrap();
        assert!(matches!(
            collect(&rx).last(),
            Some(ScanEvent::Docker { .. })
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn second_start_while_running_is_rejected() {
        let state = Arc::new(AppState::default());
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();
        state
            .start_scan(ScanOptions::new("/"), fake_docker(0), move |e| {
                tx2.send(e).unwrap()
            })
            .unwrap();

        let again = state.start_scan(ScanOptions::new("/"), fake_docker(0), move |e| {
            tx.send(e).unwrap()
        });
        assert!(matches!(again, Err(StartError::AlreadyRunning)));

        state.cancel_scan();
        collect(&rx);
    }

    #[test]
    fn stale_docker_result_is_dropped() {
        let dir = temp_home("stale");
        let state = Arc::new(AppState::default());
        let (tx, rx) = mpsc::channel();

        // O Docker do primeiro scan só responde depois que o segundo começou.
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let slow_docker: DockerQuery = Box::new(move || {
            release_rx.recv().unwrap();
            Some(DockerUsage {
                images: 999,
                ..Default::default()
            })
        });
        let tx1 = tx.clone();
        state
            .start_scan(options(&dir), slow_docker, move |e| tx1.send(e).unwrap())
            .unwrap();
        while !matches!(rx.recv_timeout(TIMEOUT).unwrap(), ScanEvent::Done { .. }) {}

        state
            .start_scan(options(&dir), fake_docker(7), move |e| tx.send(e).unwrap())
            .unwrap();
        let events = collect(&rx);
        release_tx.send(()).unwrap();

        let ScanEvent::Docker { docker_bytes, .. } = events.last().unwrap() else {
            unreachable!()
        };
        assert_eq!(*docker_bytes, 7);
        // Dá tempo para o Docker antigo terminar e confirma que ele não sobrescreveu nada.
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        assert_eq!(state.last_result().unwrap().docker_bytes, 7);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
