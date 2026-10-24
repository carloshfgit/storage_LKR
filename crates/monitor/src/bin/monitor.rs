//! CLI de validação do monitor: imprime uma amostra por intervalo, para
//! comparar com `top`, `free -b`, `iostat -d 1`, `ifstat` e as ferramentas da GPU.
//!
//! Uso: `monitor [--once] [--interval MS] [--count N] [--json] [--cores] [--procs]`
//!
//! Com `--procs`, imprime os "Top 5" a cada leitura de processos, e no fim o
//! custo dela (média das amostras com processos menos a das sem).

use std::process::ExitCode;
use std::time::{Duration, Instant};

use monitor::{GpuState, ProcGroup, Sample, Sampler};

fn main() -> ExitCode {
    let mut interval = Duration::from_secs(1);
    let mut count: Option<u64> = None;
    let mut json = false;
    let mut cores = false;
    let mut procs = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            // Duas leituras: a primeira não tem taxas.
            "--once" => count = Some(1),
            "--interval" => {
                if let Some(ms) = args.next().and_then(|v| v.parse().ok()) {
                    interval = Duration::from_millis(ms);
                }
            }
            "--count" => count = args.next().and_then(|v| v.parse().ok()),
            "--json" => json = true,
            "--cores" => cores = true,
            "--procs" => procs = true,
            _ => {
                eprintln!(
                    "uso: monitor [--once] [--interval MS] [--count N] [--json] [--cores] [--procs]"
                );
                return ExitCode::FAILURE;
            }
        }
    }

    let mut sampler = Sampler::system();
    if let Err(e) = sampler.sample() {
        eprintln!("erro: {e}");
        return ExitCode::FAILURE;
    }
    let start_cpu = process_cpu_s();
    let started = Instant::now();
    let mut cost = Duration::ZERO;
    let mut worst = Duration::ZERO;
    let mut n = 0u64;
    // Amostras com e sem a leitura de processos: (soma, quantas, pior).
    let mut with_procs = (Duration::ZERO, 0u32, Duration::ZERO);
    let mut without = (Duration::ZERO, 0u32);
    let mut total_procs = 0;

    while count.is_none_or(|c| n < c) {
        std::thread::sleep(interval);
        let t = Instant::now();
        let sample = match sampler.sample() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("erro: {e}");
                return ExitCode::FAILURE;
            }
        };
        let took = t.elapsed();
        cost += took;
        worst = worst.max(took);
        n += 1;
        if let Some(p) = &sample.procs {
            with_procs.0 += took;
            with_procs.1 += 1;
            with_procs.2 = with_procs.2.max(took);
            total_procs = p.total;
        } else {
            without.0 += took;
            without.1 += 1;
        }

        if json {
            println!("{}", serde_json::to_string(&sample).unwrap_or_default());
        } else {
            print_line(&sample, cores);
            if procs {
                print_procs(&sample);
            }
        }
    }

    // Só com `--count`: o laço infinito termina no Ctrl+C.
    if n > 1 {
        let wall = started.elapsed().as_secs_f64();
        let cpu = process_cpu_s() - start_cpu;
        eprintln!(
            "\n{n} amostras: média {:.0} µs, pior {:.0} µs; CPU do processo {:.3}% de um núcleo",
            cost.as_secs_f64() * 1e6 / n as f64,
            worst.as_secs_f64() * 1e6,
            100.0 * cpu / wall
        );
        if with_procs.1 > 0 && without.1 > 0 {
            let avg = |sum: Duration, k: u32| sum.as_secs_f64() * 1e6 / k as f64;
            eprintln!(
                "processos ({total_procs}): +{:.0} µs por leitura (com {:.0} µs, sem {:.0} µs; pior com {:.0} µs)",
                avg(with_procs.0, with_procs.1) - avg(without.0, without.1),
                avg(with_procs.0, with_procs.1),
                avg(without.0, without.1),
                with_procs.2.as_secs_f64() * 1e6,
            );
        }
    }
    ExitCode::SUCCESS
}

fn print_line(s: &Sample, cores: bool) {
    let mut line = format!(
        "CPU {:>5} {:>8} | RAM {}/{} swap {}",
        pct(s.cpu.usage_pct),
        s.cpu
            .freq_avg_mhz
            .map_or("—".into(), |m| format!("{:.2} GHz", m as f64 / 1000.0)),
        gb(s.mem.used),
        gb(s.mem.total),
        gb(s.mem.swap_used),
    );
    if let Some(t) = s.cpu.temp {
        line += &format!(
            " {:.0} °C{}",
            t.celsius,
            if t.approximate { "~" } else { "" }
        );
    }
    if let Some(z) = &s.mem.zram {
        line += &format!(" zram {}→{}", gb(z.orig_bytes), gb(z.compr_bytes));
    }
    if let Some(p) = s.mem.psi {
        line += &format!(" psi {:.1}%", p.some_avg10);
    }
    for d in &s.disks {
        line += &format!(
            " | {} ↓{} ↑{} {}",
            d.name,
            rate(d.read_bps),
            rate(d.write_bps),
            pct(d.busy_pct)
        );
        if let Some(t) = d.temp {
            line += &format!(" {:.0} °C", t.celsius);
        }
    }
    line += &format!(
        " | rede ↓{} ↑{} (sessão ↓{} ↑{})",
        rate(s.net.rx_bps),
        rate(s.net.tx_bps),
        mb(s.net.rx_session),
        mb(s.net.tx_session)
    );
    if let Some(b) = &s.battery_total {
        line += &format!(
            " | bateria {} {:?} {}",
            pct(b.pct),
            b.status,
            b.power_w.map_or("—".into(), |w| format!("{w:.1} W"))
        );
        if let Some(t) = b.time_left_s {
            line += &format!(" {}h{:02}", t / 3600, t / 60 % 60);
        }
    }
    for g in &s.gpus {
        line += &format!(" | {}", g.spec.card);
        if g.state == GpuState::Suspended {
            line += " em repouso";
            continue;
        }
        line += &format!(" {}", pct(g.busy_pct));
        if let Some(f) = g.freq_mhz {
            line += &format!(" {f} MHz");
        }
        if let (Some(used), Some(total)) = (g.vram_used, g.spec.vram_total) {
            line += &format!(" vram {}/{}", gb(used), gb(total));
        }
        if let Some(t) = g.temp {
            line += &format!(" {:.0} °C", t.celsius);
        }
        if let Some(w) = g.power_w {
            line += &format!(" {w:.1} W");
        }
    }
    println!("{line}");
    if cores {
        let per: Vec<String> = s
            .cpu
            .cores
            .iter()
            .map(|c| format!("cpu{} {}", c.id, pct(c.usage_pct)))
            .collect();
        println!("    {}", per.join("  "));
    }
}

fn print_procs(s: &Sample) {
    let Some(p) = &s.procs else {
        return;
    };
    let name = |g: &ProcGroup| {
        if g.count > 1 {
            format!("{} ×{}", g.name, g.count)
        } else {
            g.name.clone()
        }
    };
    let list = |groups: &[ProcGroup], value: &dyn Fn(&ProcGroup) -> String| {
        groups
            .iter()
            .map(|g| format!("{} {}", name(g), value(g)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!("    CPU: {}", list(&p.cpu, &|g| pct(g.cpu_pct)));
    println!("    RAM: {}", list(&p.mem, &|g| gb(g.mem_bytes)));
    println!(
        "    Disco: {}",
        list(&p.disk, &|g| format!(
            "↓{} ↑{}",
            rate(g.read_bps),
            rate(g.write_bps)
        ))
    );
}

/// CPU (user + system) do próprio processo, em segundos.
fn process_cpu_s() -> f64 {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
        return 0.0;
    };
    // O nome (`comm`) vem entre parênteses e pode ter espaços: os campos
    // contam a partir do último `)`. utime e stime são os campos 14 e 15.
    let Some((_, rest)) = stat.rsplit_once(')') else {
        return 0.0;
    };
    let f: Vec<u64> = rest
        .split_ascii_whitespace()
        .skip(11)
        .take(2)
        .filter_map(|v| v.parse().ok())
        .collect();
    let ticks = rustix::param::clock_ticks_per_second() as f64;
    f.iter().sum::<u64>() as f64 / ticks
}

/// Unidades decimais, como o resto do app.
fn gb(bytes: u64) -> String {
    format!("{:.2} GB", bytes as f64 / 1e9)
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

fn rate(bps: Option<f64>) -> String {
    bps.map_or("—".into(), |b| format!("{:.2} MB/s", b / 1e6))
}

fn pct(p: Option<f32>) -> String {
    p.map_or("—".into(), |p| format!("{p:.1}%"))
}
