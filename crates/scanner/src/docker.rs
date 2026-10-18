//! Uso de disco do Docker, que fica em `/var/lib/docker` (só root) e por isso
//! não aparece no walk. O daemon informa os tamanhos sem root para quem está
//! no grupo `docker`. O Docker rootless guarda os dados em `~/.local/share/docker`
//! e tem um daemon próprio, consultado à parte (ARCHITECTURE §6.1).

use std::io::Read;
use std::path::PathBuf;
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

/// Qual daemon: o do sistema ou o rootless do usuário.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockerMode {
    Rootful,
    Rootless,
}

impl DockerMode {
    pub const ALL: [DockerMode; 2] = [DockerMode::Rootful, DockerMode::Rootless];

    /// Socket fixo de cada daemon, montado aqui e nunca vindo da UI. O contexto atual do
    /// `docker` é ignorado de propósito: depois de `docker context use rootless`, o comando
    /// sem `--host` falaria com o rootless, e o mesmo daemon seria contado duas vezes.
    pub fn socket(self) -> PathBuf {
        match self {
            DockerMode::Rootful => PathBuf::from("/var/run/docker.sock"),
            DockerMode::Rootless => std::env::var_os("XDG_RUNTIME_DIR")
                .filter(|v| !v.is_empty())
                .map_or_else(
                    || PathBuf::from(format!("/run/user/{}", rustix::process::getuid().as_raw())),
                    PathBuf::from,
                )
                .join("docker.sock"),
        }
    }

    fn command(self) -> Command {
        let mut cmd = Command::new("docker");
        cmd.arg("--host")
            .arg(format!("unix://{}", self.socket().display()))
            .stdin(Stdio::null());
        cmd
    }
}

/// Uso dos dois daemons; `None` onde não há daemon acessível.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerStatus {
    pub rootful: Option<DockerUsage>,
    pub rootless: Option<DockerUsage>,
}

impl DockerStatus {
    pub fn get(&self, mode: DockerMode) -> Option<&DockerUsage> {
        match mode {
            DockerMode::Rootful => self.rootful.as_ref(),
            DockerMode::Rootless => self.rootless.as_ref(),
        }
    }

    pub fn set(&mut self, mode: DockerMode, usage: Option<DockerUsage>) {
        match mode {
            DockerMode::Rootful => self.rootful = usage,
            DockerMode::Rootless => self.rootless = usage,
        }
    }
}

/// Consulta os dois daemons em paralelo, cada um com o seu `timeout`.
pub fn docker_status(timeout: Duration) -> DockerStatus {
    std::thread::scope(|s| {
        let rootless = s.spawn(|| docker_usage(DockerMode::Rootless, timeout));
        DockerStatus {
            rootful: docker_usage(DockerMode::Rootful, timeout),
            rootless: rootless.join().ok().flatten(),
        }
    })
}

/// Roda `docker system df --format json` no daemon de `mode`. `None` se o socket não
/// existir, o Docker não estiver instalado, o daemon estiver parado, faltar permissão ou
/// o comando estourar o `timeout`.
pub fn docker_usage(mode: DockerMode, timeout: Duration) -> Option<DockerUsage> {
    parse_df(&run_docker(
        mode,
        &["system", "df", "--format", "json"],
        timeout,
    )?)
}

/// Roda um `docker` só de leitura no daemon de `mode` e devolve a saída. Sem socket, nem
/// roda o `docker`: o rootless não existe na maioria das máquinas.
fn run_docker(mode: DockerMode, args: &[&str], timeout: Duration) -> Option<String> {
    if !mode.socket().exists() {
        return None;
    }
    let mut cmd = mode.command();
    cmd.args(args);
    run_with_timeout(cmd, timeout)
}

/// A saída é lida numa thread enquanto o comando roda: a do `df -v` passa dos 64 KiB do
/// pipe, e esperar o exit antes de ler travaria o `docker` até o `timeout`.
fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Option<String> {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        stdout.read_to_string(&mut out).map(|_| out)
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    // Com o processo encerrado, o pipe fecha e a leitura termina.
    let out = reader.join().ok()?.ok()?;
    status?.success().then_some(out)
}

/// Cada imagem, container, volume e entrada de build cache de um daemon, como o
/// `docker system df -v` lista. Cada lista vem do maior para o menor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerDetails {
    /// Versão do daemon (`"29.9.1"`). Antes da 23, o `docker volume prune` apaga também
    /// volumes com nome; `None` se não deu para saber.
    pub server_version: Option<String>,
    pub images: Vec<DockerImage>,
    pub containers: Vec<DockerContainer>,
    pub volumes: Vec<DockerVolume>,
    pub build_cache: Vec<DockerCacheEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerImage {
    pub id: String,
    /// `<none>` numa imagem sem nome (dangling).
    pub repository: String,
    pub tag: String,
    pub size: u64,
    /// Camadas que outras imagens também usam; removê-la não libera esta parte.
    pub shared_size: u64,
    /// Containers (rodando ou parados) que usam a imagem.
    pub containers: u64,
    /// Segundos Unix; 0 se o Docker não informar.
    pub created: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerContainer {
    pub id: String,
    pub name: String,
    pub image: String,
    /// `running`, `exited`, `created`, `paused`…
    pub state: String,
    /// Camada gravável do container.
    pub size: u64,
    /// Projeto do Docker Compose, pela label `com.docker.compose.project`.
    pub project: Option<String>,
    pub created: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerVolume {
    pub name: String,
    pub size: u64,
    /// Containers que montam o volume; 0 = sem uso.
    pub links: u64,
    /// Criado sem nome (`VOLUME` no Dockerfile, `-v /caminho`); o `docker volume prune`
    /// remove só esses.
    pub anonymous: bool,
    pub project: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerCacheEntry {
    pub id: String,
    /// Passo do build (`[builder 3/5] COPY …`, `pulled from …`).
    pub description: String,
    pub size: u64,
    pub in_use: bool,
    pub shared: bool,
    pub last_used: i64,
    pub usage_count: u64,
}

/// Roda `docker system df -v --format json` no daemon de `mode`. `None` nos mesmos casos
/// do `docker_usage`.
pub fn docker_details(mode: DockerMode, timeout: Duration) -> Option<DockerDetails> {
    let mut details = parse_details(&run_docker(
        mode,
        &["system", "df", "-v", "--format", "json"],
        timeout,
    )?)?;
    details.server_version = run_docker(
        mode,
        &["version", "--format", "{{.Server.Version}}"],
        timeout,
    )
    .map(|v| v.trim().to_string())
    .filter(|v| !v.is_empty());
    Some(details)
}

/// O que o app sabe limpar no Docker. Volumes ficam de fora de propósito: guardam dados
/// (DECISIONS D18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockerTarget {
    BuildCache,
    UnusedImages,
}

impl DockerTarget {
    /// Argumentos fixos do `docker`: nada vindo da UI entra na linha de comando.
    pub fn args(self) -> &'static [&'static str] {
        match self {
            DockerTarget::BuildCache => &["builder", "prune", "-f"],
            DockerTarget::UnusedImages => &["image", "prune", "-a", "-f"],
        }
    }
}

/// Roda a limpeza no daemon de `mode` e espera terminar, sem timeout: um prune grande
/// leva minutos.
pub fn docker_prune(mode: DockerMode, target: DockerTarget) -> Result<(), String> {
    let out = mode
        .command()
        .args(target.args())
        .output()
        .map_err(|e| format!("não foi possível rodar o docker: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(match stderr.trim() {
        "" => format!("docker terminou com {}", out.status),
        msg => msg.to_string(),
    })
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

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DfVerbose {
    #[serde(default)]
    images: Vec<VImage>,
    #[serde(default)]
    containers: Vec<VContainer>,
    #[serde(default)]
    volumes: Vec<VVolume>,
    #[serde(default)]
    build_cache: Vec<VCache>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct VImage {
    #[serde(rename = "ID")]
    id: String,
    repository: String,
    tag: String,
    size: String,
    shared_size: String,
    containers: String,
    created_at: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct VContainer {
    #[serde(rename = "ID")]
    id: String,
    names: String,
    image: String,
    state: String,
    size: String,
    labels: String,
    created_at: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct VVolume {
    name: String,
    size: String,
    links: String,
    labels: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct VCache {
    #[serde(rename = "ID")]
    id: String,
    description: String,
    size: String,
    in_use: String,
    shared: String,
    last_used_at: String,
    usage_count: String,
}

fn parse_details(out: &str) -> Option<DockerDetails> {
    let v: DfVerbose = serde_json::from_str(out.trim()).ok()?;
    // "N/A" (driver que não mede) e campos ausentes viram 0.
    let size = |s: &str| parse_size(s).unwrap_or(0);
    let count = |s: &str| s.trim().parse::<u64>().unwrap_or(0);

    let mut d = DockerDetails {
        server_version: None,
        images: v
            .images
            .into_iter()
            .map(|i| DockerImage {
                size: size(&i.size),
                shared_size: size(&i.shared_size),
                containers: count(&i.containers),
                created: parse_time(&i.created_at),
                id: i.id,
                repository: i.repository,
                tag: i.tag,
            })
            .collect(),
        containers: v
            .containers
            .into_iter()
            .map(|c| DockerContainer {
                size: size(&c.size),
                project: label(&c.labels, "com.docker.compose.project"),
                created: parse_time(&c.created_at),
                id: c.id,
                name: c.names,
                image: c.image,
                state: c.state,
            })
            .collect(),
        volumes: v
            .volumes
            .into_iter()
            .map(|vol| DockerVolume {
                size: size(&vol.size),
                links: count(&vol.links),
                anonymous: label(&vol.labels, "com.docker.volume.anonymous").is_some()
                    || is_anonymous_name(&vol.name),
                project: label(&vol.labels, "com.docker.compose.project"),
                name: vol.name,
            })
            .collect(),
        build_cache: v
            .build_cache
            .into_iter()
            .map(|b| DockerCacheEntry {
                size: size(&b.size),
                in_use: b.in_use == "true",
                shared: b.shared == "true",
                last_used: parse_time(&b.last_used_at),
                usage_count: count(&b.usage_count),
                id: b.id,
                description: b.description,
            })
            .collect(),
    };
    d.images.sort_by_key(|i| std::cmp::Reverse(i.size));
    d.containers.sort_by_key(|i| std::cmp::Reverse(i.size));
    d.volumes.sort_by_key(|i| std::cmp::Reverse(i.size));
    d.build_cache.sort_by_key(|i| std::cmp::Reverse(i.size));
    Some(d)
}

/// Antes do Docker 23 não havia a label `com.docker.volume.anonymous`; um volume anônimo
/// se reconhece pelo nome, um id de 64 dígitos hexadecimais.
fn is_anonymous_name(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Valor de uma label no formato `chave=valor,chave=valor` do Docker. Uma label sem
/// valor (`com.docker.volume.anonymous=`) devolve `Some("")`.
fn label(labels: &str, key: &str) -> Option<String> {
    labels.split(',').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

/// Datas do Docker (`"2026-07-17 13:22:11 -0300 -03"`,
/// `"2026-03-25 13:49:24.2698 +0000 UTC"`) em segundos Unix; 0 se não reconhecer.
fn parse_time(s: &str) -> i64 {
    let mut parts = s.split_whitespace();
    let (Some(date), Some(time), Some(offset)) = (parts.next(), parts.next(), parts.next()) else {
        return 0;
    };
    let nums =
        |s: &str, sep: char| -> Option<Vec<i64>> { s.split(sep).map(|n| n.parse().ok()).collect() };
    let parsed = (|| {
        let [y, mo, d] = nums(date, '-')?[..] else {
            return None;
        };
        let [h, mi, sec] = nums(time.split('.').next()?, ':')?[..] else {
            return None;
        };
        let sign = match offset.as_bytes().first()? {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let off: i64 = offset.get(1..5)?.parse().ok()?;
        let off = sign * ((off / 100) * 3600 + (off % 100) * 60);
        Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec - off)
    })();
    parsed.unwrap_or(0)
}

/// Dias desde 1970-01-01 no calendário gregoriano (algoritmo de Howard Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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
    fn prune_never_touches_volumes() {
        for target in [DockerTarget::BuildCache, DockerTarget::UnusedImages] {
            let args = target.args();
            assert!(args.ends_with(&["-f"]));
            assert!(!args.iter().any(|a| a.contains("volume")), "{args:?}");
            assert!(!args.contains(&"--volumes"));
        }
    }

    #[test]
    fn sockets_are_fixed_per_mode() {
        assert_eq!(
            DockerMode::Rootful.socket(),
            PathBuf::from("/var/run/docker.sock")
        );
        let rootless = DockerMode::Rootless.socket();
        assert!(
            rootless.is_absolute() && rootless.ends_with("docker.sock"),
            "{rootless:?}"
        );
        assert_ne!(rootless, DockerMode::Rootful.socket());

        let args: Vec<_> = DockerMode::Rootful
            .command()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["--host", "unix:///var/run/docker.sock"]);
    }

    #[test]
    fn status_get_and_set_by_mode() {
        let usage = DockerUsage {
            images: DockerItem {
                size: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut s = DockerStatus::default();
        s.set(DockerMode::Rootless, Some(usage.clone()));
        assert_eq!(s.get(DockerMode::Rootless), Some(&usage));
        assert_eq!(s.get(DockerMode::Rootful), None);
        assert_eq!(s.rootless, Some(usage));
    }

    #[test]
    fn empty_or_garbage_output_is_none() {
        assert_eq!(parse_df(""), None);
        assert_eq!(parse_df("Cannot connect to the Docker daemon"), None);
    }

    #[test]
    fn parses_docker_times() {
        assert_eq!(parse_time("1970-01-01 00:00:00 +0000 UTC"), 0);
        // 2026-07-17 16:22:11 UTC
        assert_eq!(parse_time("2026-07-17 13:22:11 -0300 -03"), 1_784_305_331);
        assert_eq!(
            parse_time("2026-03-25 13:49:24.269845557 +0000 UTC"),
            1_774_446_564
        );
        assert_eq!(parse_time("N/A"), 0);
        assert_eq!(parse_time(""), 0);
    }

    #[test]
    fn reads_labels() {
        let labels = "com.docker.compose.project=udrive,com.docker.volume.anonymous=,x=a=b";
        assert_eq!(
            label(labels, "com.docker.compose.project").as_deref(),
            Some("udrive")
        );
        assert_eq!(
            label(labels, "com.docker.volume.anonymous").as_deref(),
            Some("")
        );
        assert_eq!(label(labels, "x").as_deref(), Some("a=b"));
        assert_eq!(label(labels, "com.docker.compose"), None);
        assert_eq!(label("", "x"), None);
    }

    #[test]
    fn parses_verbose_df() {
        let out = r#"{"Images":[
  {"Containers":"0","CreatedAt":"2026-07-17 13:22:11 -0300 -03","ID":"sha256:aa","Repository":"<none>","SharedSize":"0B","Size":"1.2GB","Tag":"<none>"},
  {"Containers":"1","CreatedAt":"2026-07-17 13:22:11 -0300 -03","ID":"sha256:bb","Repository":"squidfunk/mkdocs-material","SharedSize":"10MB","Size":"302MB","Tag":"latest"}],
 "Containers":[{"CreatedAt":"2026-10-06 23:34:07 -0300 -03","ID":"d01","Image":"squidfunk/mkdocs-material","Labels":"com.docker.compose.project=20262-grupo05,org.opencontainers.image.title=mkdocs","Names":"semob_df_docs","Size":"113MB","State":"exited","Platform":null}],
 "Volumes":[
  {"Labels":"com.docker.volume.anonymous=","Links":"0","Name":"4a92","Size":"316.1MB"},
  {"Labels":"com.docker.compose.project=udrive","Links":"1","Name":"udrive_postgres_data","Size":"234.1MB"},
  {"Labels":"","Links":"N/A","Name":"nfs","Size":"N/A"}],
 "BuildCache":[
  {"Description":"pulled from docker.io/library/python:3.10-slim","ID":"s4z","InUse":"false","LastUsedAt":"2026-03-25 13:49:24.269845557 +0000 UTC","Shared":"false","Size":"117MB","UsageCount":"1"},
  {"Description":"[builder 3/5] COPY --from=deps /app/node_modules ./node_modules","ID":"s13","InUse":"true","LastUsedAt":"2026-03-25 13:49:24 +0000 UTC","Shared":"true","Size":"889MB","UsageCount":"6"}]}"#;
        let d = parse_details(out).unwrap();

        assert_eq!(d.images.len(), 2);
        assert_eq!(d.images[0].repository, "<none>", "maior primeiro");
        assert_eq!(d.images[0].size, 1_200_000_000);
        assert_eq!(d.images[1].containers, 1);
        assert_eq!(d.images[1].shared_size, 10_000_000);
        assert_eq!(d.images[1].created, 1_784_305_331);

        let c = &d.containers[0];
        assert_eq!(c.name, "semob_df_docs");
        assert_eq!(c.state, "exited");
        assert_eq!(c.size, 113_000_000);
        assert_eq!(c.project.as_deref(), Some("20262-grupo05"));

        let names: Vec<_> = d.volumes.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["4a92", "udrive_postgres_data", "nfs"]);
        assert!(is_anonymous_name(&"a3".repeat(32)));
        assert!(!is_anonymous_name("udrive_postgres_data"));
        assert!(!is_anonymous_name(&"A3".repeat(32)));
        assert!(d.volumes[0].anonymous && d.volumes[0].links == 0);
        assert!(!d.volumes[1].anonymous);
        assert_eq!(d.volumes[1].project.as_deref(), Some("udrive"));
        assert_eq!((d.volumes[2].size, d.volumes[2].links), (0, 0));

        assert_eq!(d.build_cache[0].size, 889_000_000);
        assert!(d.build_cache[0].in_use && d.build_cache[0].shared);
        assert_eq!(d.build_cache[0].usage_count, 6);
        assert!(!d.build_cache[1].in_use);
        assert_eq!(d.build_cache[1].last_used, 1_774_446_564);
    }

    #[test]
    fn verbose_df_tolerates_missing_lists_and_garbage() {
        assert_eq!(
            parse_details(r#"{"Images":[]}"#),
            Some(DockerDetails::default())
        );
        assert_eq!(parse_details(""), None);
        assert_eq!(parse_details("Cannot connect to the Docker daemon"), None);
    }

    #[test]
    fn reads_output_larger_than_the_pipe() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "head -c 300000 /dev/zero | tr '\\0' x"]);
        let out = run_with_timeout(cmd, Duration::from_secs(10)).unwrap();
        assert_eq!(out.len(), 300_000);
    }

    #[test]
    fn kills_command_after_timeout_or_failure() {
        let mut cmd = Command::new("sleep");
        cmd.arg("5");
        let start = Instant::now();
        assert_eq!(run_with_timeout(cmd, Duration::from_millis(100)), None);
        assert!(start.elapsed() < Duration::from_secs(2));

        assert_eq!(
            run_with_timeout(Command::new("false"), Duration::from_secs(5)),
            None
        );
    }
}
