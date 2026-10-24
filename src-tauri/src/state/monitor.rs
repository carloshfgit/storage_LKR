//! Ciclo de vida do monitor de recursos (aba "Recursos"). Separado do
//! `AppState` do scan: não divide lock com ele e roda inclusive durante um
//! scan, que é quando ver o disco trabalhando interessa.
//!
//! Só há coleta enquanto a UI pede (PLAN-V2, D46): a aba chama `start` ao
//! aparecer e `stop` ao sumir. A amostra e o destino dos eventos são
//! parâmetros, para o ciclo ser testado sem janela (como o scan, D24).

use std::io;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use ::monitor::Sample;
use serde::Serialize;

/// Intervalo entre as amostras na UI.
pub const INTERVAL: Duration = Duration::from_secs(1);

/// Mensagens de uma sessão do monitor: `sample`* → `stopped` | `error`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MonitorEvent {
    Sample {
        sample: Box<Sample>,
    },
    /// Fim normal: `stop` ou outro `start`.
    Stopped,
    Error {
        message: String,
    },
}

#[derive(Default)]
pub struct MonitorState {
    /// Incrementa a cada `start` e `stop`; uma thread só segue enquanto a
    /// geração dela for a atual.
    generation: Mutex<u64>,
    /// Acorda a thread que espera o próximo tick, para ela parar na hora.
    wake: Condvar,
}

impl MonitorState {
    /// Começa uma sessão numa thread própria e retorna na hora. A anterior,
    /// se houver, para (emite `stopped`). A primeira amostra sai logo, sem
    /// taxas; as outras, a cada `interval`. `send` devolve `false` quando não
    /// há mais quem receba (janela fechada ou recarregada), e a thread para.
    pub fn start(
        self: &Arc<Self>,
        interval: Duration,
        mut sample: impl FnMut() -> io::Result<Sample> + Send + 'static,
        send: impl Fn(MonitorEvent) -> bool + Send + 'static,
    ) {
        let generation = self.bump();
        let state = Arc::clone(self);
        std::thread::spawn(move || {
            let mut next = Instant::now();
            loop {
                let event = match sample() {
                    Ok(s) => MonitorEvent::Sample {
                        sample: Box::new(s),
                    },
                    Err(e) => {
                        send(MonitorEvent::Error {
                            message: e.to_string(),
                        });
                        return;
                    }
                };
                if !state.is_current(generation) {
                    break;
                }
                if !send(event) {
                    return;
                }

                // Cadência fixa; um tick atrasado não acumula: o próximo é contado de agora.
                next += interval;
                let now = Instant::now();
                if next < now {
                    next = now;
                }
                let guard = state.lock();
                let (guard, _) = state
                    .wake
                    .wait_timeout_while(guard, next - now, |g| *g == generation)
                    .unwrap_or_else(PoisonError::into_inner);
                if *guard != generation {
                    break;
                }
            }
            send(MonitorEvent::Stopped);
        });
    }

    /// Para a sessão atual, se houver. A thread acorda e sai sem esperar o tick.
    pub fn stop(&self) {
        self.bump();
    }

    fn bump(&self) -> u64 {
        let mut g = self.lock();
        *g += 1;
        let generation = *g;
        drop(g);
        self.wake.notify_all();
        generation
    }

    fn is_current(&self, generation: u64) -> bool {
        *self.lock() == generation
    }

    fn lock(&self) -> MutexGuard<'_, u64> {
        self.generation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    use ::monitor::{CpuSample, MemSample, NetSample};

    use super::*;

    const TICK: Duration = Duration::from_millis(10);
    const TIMEOUT: Duration = Duration::from_secs(5);

    fn fake(at_ms: u64) -> Sample {
        Sample {
            at_ms,
            uptime_s: 0,
            cpu: CpuSample {
                usage_pct: None,
                freq_avg_mhz: None,
                freq_max_mhz: None,
                temp: None,
                cores: Vec::new(),
            },
            mem: MemSample {
                total: 0,
                used: 0,
                available: 0,
                swap_total: 0,
                swap_used: 0,
                zram: None,
                psi: None,
            },
            disks: Vec::new(),
            net: NetSample {
                rx_bps: None,
                tx_bps: None,
                rx_session: 0,
                tx_session: 0,
                interfaces: Vec::new(),
            },
            gpus: Vec::new(),
            procs: None,
        }
    }

    /// Amostrador falso que conta as chamadas.
    fn counter() -> (
        Arc<AtomicUsize>,
        impl FnMut() -> io::Result<Sample> + Send + 'static,
    ) {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&calls);
        (calls, move || {
            Ok(fake(c.fetch_add(1, Ordering::SeqCst) as u64))
        })
    }

    fn channel() -> (
        mpsc::Receiver<MonitorEvent>,
        impl Fn(MonitorEvent) -> bool + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        (rx, move |e| tx.send(e).is_ok())
    }

    fn is_sample(e: &MonitorEvent) -> bool {
        matches!(e, MonitorEvent::Sample { .. })
    }

    /// Recebe até o `stopped`; devolve quantas amostras vieram antes.
    fn until_stopped(rx: &mpsc::Receiver<MonitorEvent>) -> usize {
        let mut samples = 0;
        loop {
            match rx.recv_timeout(TIMEOUT).expect("evento não chegou") {
                MonitorEvent::Sample { .. } => samples += 1,
                MonitorEvent::Stopped => return samples,
                MonitorEvent::Error { message } => panic!("erro inesperado: {message}"),
            }
        }
    }

    #[test]
    fn samples_until_stopped() {
        let state = Arc::new(MonitorState::default());
        let (rx, send) = channel();
        let (_, sample) = counter();
        state.start(TICK, sample, send);

        for _ in 0..3 {
            assert!(is_sample(&rx.recv_timeout(TIMEOUT).unwrap()));
        }
        state.stop();
        until_stopped(&rx);
        // Depois do `stopped`, a thread terminou: o canal fecha.
        assert!(rx.recv_timeout(TIMEOUT).is_err());
    }

    #[test]
    fn stop_does_not_wait_for_the_tick() {
        let state = Arc::new(MonitorState::default());
        let (rx, send) = channel();
        let (_, sample) = counter();
        state.start(Duration::from_secs(3600), sample, send);
        assert!(is_sample(&rx.recv_timeout(TIMEOUT).unwrap()));

        let t = Instant::now();
        state.stop();
        assert!(matches!(
            rx.recv_timeout(TIMEOUT).unwrap(),
            MonitorEvent::Stopped
        ));
        assert!(t.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn new_start_stops_the_old_session() {
        let state = Arc::new(MonitorState::default());
        let (old_rx, old_send) = channel();
        let (_, old_sample) = counter();
        state.start(TICK, old_sample, old_send);
        assert!(is_sample(&old_rx.recv_timeout(TIMEOUT).unwrap()));

        let (new_rx, new_send) = channel();
        let (_, new_sample) = counter();
        state.start(TICK, new_sample, new_send);

        until_stopped(&old_rx);
        assert!(
            old_rx.recv_timeout(TIMEOUT).is_err(),
            "sessão antiga continuou"
        );
        for _ in 0..3 {
            assert!(is_sample(&new_rx.recv_timeout(TIMEOUT).unwrap()));
        }
        state.stop();
        until_stopped(&new_rx);
    }

    #[test]
    fn failed_send_ends_the_thread() {
        let state = Arc::new(MonitorState::default());
        let (calls, sample) = counter();
        let sent = Arc::new(AtomicUsize::new(0));
        let s = Arc::clone(&sent);
        // A janela "fecha" depois de duas amostras.
        state.start(TICK, sample, move |_| s.fetch_add(1, Ordering::SeqCst) < 2);

        std::thread::sleep(TICK * 20);
        let n = calls.load(Ordering::SeqCst);
        std::thread::sleep(TICK * 10);
        assert_eq!(n, 3, "parou na primeira falha");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            n,
            "thread continuou amostrando"
        );
    }

    #[test]
    fn sampler_error_ends_with_error() {
        let state = Arc::new(MonitorState::default());
        let (rx, send) = channel();
        state.start(
            TICK,
            || Err(io::Error::new(io::ErrorKind::NotFound, "sem /proc/stat")),
            send,
        );
        match rx.recv_timeout(TIMEOUT).unwrap() {
            MonitorEvent::Error { message } => assert!(message.contains("/proc/stat")),
            e => panic!("esperava erro, veio {e:?}"),
        }
        assert!(rx.recv_timeout(TIMEOUT).is_err());
    }

    #[test]
    fn real_sampler_first_sample_without_rates() {
        let state = Arc::new(MonitorState::default());
        let (rx, send) = channel();
        let mut sampler = ::monitor::Sampler::system();
        state.start(TICK, move || sampler.sample(), send);
        let MonitorEvent::Sample { sample: first } = rx.recv_timeout(TIMEOUT).unwrap() else {
            panic!("esperava amostra");
        };
        assert!(first.cpu.usage_pct.is_none());
        assert!(first.mem.total > 0);
        state.stop();
        until_stopped(&rx);
    }
}
