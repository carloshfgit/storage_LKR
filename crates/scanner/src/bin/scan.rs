//! CLI de benchmark/validação do motor.
//!
//! Uso: `scan [CAMINHO] [--top N] [--threads N]`   (padrão: `/`)

use std::path::PathBuf;
use std::process::ExitCode;

use scanner::{Category, ScanOptions, scan};

fn main() -> ExitCode {
    let mut root = PathBuf::from("/");
    let mut show_top = 0usize;
    let mut threads = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--top" => show_top = args.next().and_then(|v| v.parse().ok()).unwrap_or(5),
            "--threads" => threads = args.next().and_then(|v| v.parse().ok()),
            "-h" | "--help" => {
                eprintln!("uso: scan [CAMINHO] [--top N] [--threads N]");
                return ExitCode::SUCCESS;
            }
            _ => root = PathBuf::from(arg),
        }
    }

    let mut opts = ScanOptions::new(&root);
    if let Some(t) = threads {
        opts.threads = t;
    }

    let result = match scan(&opts, |p| eprint!("\r{:>10} arquivos", p.files)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("\nerro: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!();

    let disk = &result.disk;
    let total = disk.total_bytes;
    println!(
        "{} em {}  —  {} usados de {} ({} livres)",
        disk.device,
        disk.mount.display(),
        gb(disk.used_bytes()),
        gb(total),
        gb(disk.available_bytes)
    );
    println!("raiz escaneada: {}  ({} threads)\n", opts.root.display(), opts.threads);

    let row = |name: &str, bytes: u64, files: Option<u64>| {
        let files = files.map_or(String::new(), |f| f.to_string());
        println!("{name:<24} {:>10} {:>6.1}% {files:>10}", gb(bytes), pct(bytes, total));
    };
    println!("{:<24} {:>10} {:>7} {:>10}", "categoria", "tamanho", "disco", "arquivos");
    for c in Category::ALL {
        let s = &result.categories[&c];
        row(c.label(), s.bytes, Some(s.files));
    }
    row("Restante (sem acesso)", result.unaccounted_bytes, None);
    row("Livre", disk.available_bytes, None);
    println!();

    let scanned = result.scanned_bytes();
    println!("escaneado:      {} ({} bytes)", gb(scanned), scanned);
    println!("usado (df):     {} ({} bytes)", gb(disk.used_bytes()), disk.used_bytes());
    println!("dirs negados:   {}", result.denied_dirs);
    let secs = result.elapsed_ms as f64 / 1000.0;
    println!(
        "tempo:          {secs:.2} s  ({} arquivos, {:.0} arq/s)",
        result.files,
        result.files as f64 / secs.max(0.001)
    );

    if show_top > 0 {
        for c in Category::ALL {
            println!("\n{} — maiores:", c.label());
            for f in result.categories[&c].top.iter().take(show_top) {
                println!("  {:>10}  {}", gb(f.bytes), f.path.display());
            }
        }
    }
    ExitCode::SUCCESS
}

/// Unidades decimais, como GNOME/iOS.
fn gb(bytes: u64) -> String {
    format!("{:.2} GB", bytes as f64 / 1e9)
}

fn pct(part: u64, total: u64) -> f64 {
    if total == 0 { 0.0 } else { part as f64 * 100.0 / total as f64 }
}
