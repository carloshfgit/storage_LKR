//! Uso de disco do Docker, que fica em `/var/lib/docker` (só root) e por isso
//! não aparece no walk. O daemon informa os tamanhos sem root para quem está
//! no grupo `docker`.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// `docker system df` levou ~5 s nesta máquina; o padrão dá folga.
pub const DEFAULT_DOCKER_TIMEOUT: Duration = Duration::from_secs(20);

/// Uma linha do `docker system df`: imagens, containers, volumes ou build cache.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerItem {
    pub size: u64,
    /// O que o Docker considera removível (sem container/uso ativo).
    pub reclaimable: u64,
    pub count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerUsage {
    pub images: DockerItem,
    pub containers: DockerItem,
    pub volumes: DockerItem,
    pub build_cache: DockerItem,
}

impl DockerUsage {
    fn items(&self) -> [&DockerItem; 4] {
        [
            &self.images,
            &self.containers,
            &self.volumes,
            &self.build_cache,
        ]
    }

    pub fn total(&self) -> u64 {
        self.items().iter().map(|i| i.size).sum()
    }

    pub fn reclaimable(&self) -> u64 {
        self.items().iter().map(|i| i.reclaimable).sum()
    }
}

/// Roda `docker system df --format json`. `None` se o Docker não estiver instalado,
/// o daemon estiver parado, faltar permissão ou o comando estourar o `timeout`.
pub fn docker_usage(timeout: Duration) -> Option<DockerUsage> {
    let mut child = Command::new("docker")
        .args(["system", "df", "--format", "json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    // A saída tem 4 linhas curtas: cabe no buffer do pipe, então ler depois do exit não trava.
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    parse_df(&out)
}

#[derive(Deserialize)]
struct DfLine {
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "Size")]
    size: String,
    #[serde(rename = "Reclaimable")]
    reclaimable: String,
    #[serde(rename = "TotalCount", default)]
    count: String,
}

fn parse_df(out: &str) -> Option<DockerUsage> {
    let mut usage = DockerUsage::default();
    let mut any = false;
    for line in out.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(l) = serde_json::from_str::<DfLine>(line) else {
            continue;
        };
        let item = match l.kind.as_str() {
            "Images" => &mut usage.images,
            "Containers" => &mut usage.containers,
            "Local Volumes" => &mut usage.volumes,
            "Build Cache" => &mut usage.build_cache,
            _ => continue,
        };
        *item = DockerItem {
            size: parse_size(&l.size)?,
            reclaimable: parse_size(&l.reclaimable)?,
            count: l.count.parse().unwrap_or(0),
        };
        any = true;
    }
    any.then_some(usage)
}

/// Tamanhos do Docker (go-units `HumanSize`, unidades decimais): `"58.68GB"`,
/// `"113.1MB"`, `"0B"`, `"39.88GB (68%)"`.
fn parse_size(s: &str) -> Option<u64> {
    let s = s.split_whitespace().next()?;
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (number, unit) = s.split_at(split);
    let factor = match unit {
        "B" => 1e0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "PB" => 1e15,
        _ => return None,
    };
    Some((number.parse::<f64>().ok()? * factor).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("58.68GB"), Some(58_680_000_000));
        assert_eq!(parse_size("113.1MB"), Some(113_100_000));
        assert_eq!(parse_size("0B"), Some(0));
        assert_eq!(parse_size("39.88GB (68%)"), Some(39_880_000_000));
        assert_eq!(parse_size("1.5kB"), Some(1_500));
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("12XB"), None);
    }

    #[test]
    fn parses_df_output() {
        let out = r#"{"Active":"1","Reclaimable":"18.18GB (83%)","Size":"21.65GB","TotalCount":"37","Type":"Images"}
{"Active":"0","Reclaimable":"113.1MB (100%)","Size":"113.1MB","TotalCount":"1","Type":"Containers"}
{"Active":"0","Reclaimable":"16.71GB (100%)","Size":"16.71GB","TotalCount":"56","Type":"Local Volumes"}
{"Active":"0","Reclaimable":"39.88GB","Size":"58.68GB","TotalCount":"252","Type":"Build Cache"}
"#;
        let u = parse_df(out).unwrap();
        assert_eq!(
            u.images,
            DockerItem {
                size: 21_650_000_000,
                reclaimable: 18_180_000_000,
                count: 37
            }
        );
        assert_eq!(u.containers.size, 113_100_000);
        assert_eq!(u.volumes.size, 16_710_000_000);
        assert_eq!(u.volumes.count, 56);
        assert_eq!(u.build_cache.size, 58_680_000_000);
        assert_eq!(u.build_cache.reclaimable, 39_880_000_000);
        assert_eq!(
            u.reclaimable(),
            18_180_000_000 + 113_100_000 + 16_710_000_000 + 39_880_000_000
        );
        assert_eq!(u.total(), 97_153_100_000);
    }

    #[test]
    fn empty_or_garbage_output_is_none() {
        assert_eq!(parse_df(""), None);
        assert_eq!(parse_df("Cannot connect to the Docker daemon"), None);
    }
}
