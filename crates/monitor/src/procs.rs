//! "Top 5" processos por CPU, memória e disco, agrupados por aplicativo (D50).
//!
//! Por processo, só o `/proc/[pid]/stat` (que já traz `comm`, pai, tempos de
//! CPU e RSS) e, nos do próprio usuário, o `/proc/[pid]/io`. O `cmdline` só é
//! lido uma vez por grupo, para o nome. As threads não aparecem: o `/proc` só
//! lista os processos.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::apps::Apps;
use crate::model::{ProcGroup, ProcTop};

/// Linhas de cada lista.
pub(crate) const TOP: usize = 5;

/// `PF_KTHREAD` nas flags do `stat`: thread do kernel (`kworker`, `kswapd`),
/// que não é um aplicativo.
const PF_KTHREAD: u64 = 0x0020_0000;

/// O que interessa do `/proc/[pid]/stat`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stat {
    pub comm: String,
    pub state: char,
    pub ppid: u32,
    pub flags: u64,
    /// utime + stime, em ticks (`CLK_TCK`).
    pub ticks: u64,
    /// Em ticks desde o boot: com o pid, identifica o processo (o pid é reaproveitado).
    pub start: u64,
    /// Páginas residentes.
    pub rss_pages: u64,
}

/// `pid (comm) estado ppid …`. O `comm` pode ter espaços e parênteses
/// (`(Web Content)`, `a) b`): os campos contam a partir do último `)`.
pub(crate) fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let comm = text.get(open + 1..close)?.to_string();
    let f: Vec<&str> = text.get(close + 1..)?.split_ascii_whitespace().collect();
    // f[0] é o campo 3 do `man proc_pid_stat`: o campo n fica em f[n - 3].
    let num = |n: usize| f.get(n - 3)?.parse::<u64>().ok();
    Some(Stat {
        comm,
        state: f.first()?.chars().next()?,
        ppid: num(4)? as u32,
        flags: num(9)?,
        ticks: num(14)? + num(15)?,
        start: num(22)?,
        rss_pages: num(24)?,
    })
}

/// Lê um arquivo do `/proc/[pid]` com um `read` só: os arquivos do procfs
/// entregam tudo de uma vez quando cabe no buffer, e a leitura extra para
/// achar o fim custaria uma chamada a mais por arquivo (são centenas). Se
/// encher o buffer, ele cresce e a leitura continua.
fn read_proc_file<'a>(path: &Path, buf: &'a mut Vec<u8>) -> io::Result<&'a [u8]> {
    let mut file = File::open(path)?;
    let mut len = 0;
    loop {
        match file.read(&mut buf[len..]) {
            Ok(0) => break,
            Ok(n) => {
                len += n;
                if len < buf.len() {
                    break;
                }
                buf.resize(buf.len() * 2, 0);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(&buf[..len])
}

/// `read_bytes` e `write_bytes`: o que chegou ao disco de fato.
pub(crate) fn parse_io(text: &str) -> Option<(u64, u64)> {
    let get = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.trim().parse::<u64>().ok())
    };
    Some((get("read_bytes:")?, get("write_bytes:")?))
}

/// Contadores da leitura anterior de um processo.
struct Prev {
    start: u64,
    ticks: u64,
    /// `None`: o `io` não abre (processo de outro usuário).
    io: Option<(u64, u64)>,
    /// Quando o `io` foi lido de fato (ver `Proc::io_fresh`).
    io_at: Option<Instant>,
}

/// Um processo lido agora.
struct Proc {
    pid: u32,
    stat: Stat,
    io: Option<(u64, u64)>,
    /// `false`: o processo não usou CPU desde a leitura anterior, então não
    /// leu nem gravou nada, e o `io` não foi relido (é o anterior). A maioria
    /// dos processos está parada, e isso poupa centenas de leituras.
    io_fresh: bool,
}

/// Nome e chave de um grupo, guardados pela raiz (pid, início).
#[derive(Clone)]
struct Label {
    key: String,
    name: String,
}

pub(crate) struct Procs {
    root: PathBuf,
    clk_tck: f64,
    page: u64,
    app_dirs: Vec<PathBuf>,
    /// Lidos na primeira leitura.
    apps: Option<Apps>,
    prev: HashMap<u32, Prev>,
    prev_at: Option<Instant>,
    labels: HashMap<(u32, u64), Label>,
    /// Processos cujo `io` não abriu: não tenta de novo.
    io_denied: HashSet<(u32, u64)>,
    buf: Vec<u8>,
    path: String,
}

impl Procs {
    pub fn new(root: &Path, app_dirs: Vec<PathBuf>) -> Self {
        Self {
            root: root.to_path_buf(),
            clk_tck: rustix::param::clock_ticks_per_second() as f64,
            page: rustix::param::page_size() as u64,
            app_dirs,
            apps: None,
            prev: HashMap::new(),
            prev_at: None,
            labels: HashMap::new(),
            io_denied: HashSet::new(),
            buf: vec![0; 4096],
            path: String::new(),
        }
    }

    /// Lê todos os processos e devolve os três "Top 5". CPU e disco vêm `None`
    /// na primeira leitura.
    pub fn read(&mut self, now: Instant) -> ProcTop {
        let procs = self.scan();
        let dt = self
            .prev_at
            .map(|t| now.saturating_duration_since(t).as_secs_f64())
            .filter(|&dt| dt > 0.0);

        // Raiz de cada processo: sobe enquanto o pai tem o mesmo `comm`
        // (as abas do Chrome, os processos do Electron, `node` que abre `node`).
        let by_pid: HashMap<u32, usize> =
            procs.iter().enumerate().map(|(i, p)| (p.pid, i)).collect();
        let root_of = |mut i: usize| {
            // Limite contra um ciclo impossível, por garantia.
            for _ in 0..64 {
                let p = &procs[i];
                match by_pid.get(&p.stat.ppid) {
                    Some(&j) if j != i && procs[j].stat.comm == p.stat.comm => i = j,
                    _ => break,
                }
            }
            i
        };

        let mut groups: HashMap<String, ProcGroup> = HashMap::new();
        let mut alive = HashSet::new();
        for (i, p) in procs.iter().enumerate() {
            let before = self.prev.get(&p.pid).filter(|b| b.start == p.stat.start);
            // Processo novo desde a leitura anterior: tudo o que ele usou cabe no intervalo.
            let cpu_pct = dt.map(|dt| {
                let ticks = p.stat.ticks - before.map_or(0, |b| b.ticks.min(p.stat.ticks));
                (ticks as f64 / self.clk_tck / dt * 100.0) as f32
            });
            let io_rate = dt.and_then(|dt| {
                let (r, w) = p.io?;
                if !p.io_fresh {
                    return Some((0.0, 0.0));
                }
                let Some(b) = before else {
                    return Some((r as f64 / dt, w as f64 / dt));
                };
                let (r0, w0) = b.io?;
                // Desde a última leitura de fato, que pode ter sido antes da anterior.
                let span = b
                    .io_at
                    .map_or(dt, |t| now.saturating_duration_since(t).as_secs_f64())
                    .max(dt);
                Some((
                    r.saturating_sub(r0) as f64 / span,
                    w.saturating_sub(w0) as f64 / span,
                ))
            });

            let r = root_of(i);
            let label = self.label(&procs[r]);
            let g = groups
                .entry(label.key.clone())
                .or_insert_with(|| ProcGroup {
                    key: label.key,
                    name: label.name,
                    count: 0,
                    cpu_pct: dt.map(|_| 0.0),
                    mem_bytes: 0,
                    read_bps: None,
                    write_bps: None,
                });
            g.count += 1;
            if let (Some(sum), Some(c)) = (&mut g.cpu_pct, cpu_pct) {
                *sum += c;
            }
            g.mem_bytes += p.stat.rss_pages * self.page;
            if let Some((r, w)) = io_rate {
                *g.read_bps.get_or_insert(0.0) += r;
                *g.write_bps.get_or_insert(0.0) += w;
            }
            alive.insert((p.pid, p.stat.start));
        }

        self.prev = procs
            .iter()
            .map(|p| {
                let io_at = if p.io_fresh {
                    Some(now)
                } else {
                    self.prev.get(&p.pid).and_then(|b| b.io_at)
                };
                (
                    p.pid,
                    Prev {
                        start: p.stat.start,
                        ticks: p.stat.ticks,
                        io: p.io,
                        io_at,
                    },
                )
            })
            .collect();
        self.prev_at = Some(now);
        // Esquece os processos que acabaram.
        self.labels.retain(|k, _| alive.contains(k));
        self.io_denied.retain(|k| alive.contains(k));

        top(procs.len() as u32, groups.into_values().collect())
    }

    /// Os processos de agora; os que somem no meio da leitura ficam de fora.
    fn scan(&mut self) -> Vec<Proc> {
        let Ok(entries) = std::fs::read_dir(self.root.join("proc")) else {
            return Vec::new();
        };
        let base = self.root.join("proc");
        let mut out = Vec::with_capacity(512);
        for e in entries.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            self.path.clear();
            let _ = write!(self.path, "{}/{pid}/stat", base.display());
            let Some(stat) = read_proc_file(Path::new(&self.path), &mut self.buf)
                .ok()
                .and_then(|b| parse_stat(&String::from_utf8_lossy(b)))
            else {
                continue;
            };
            // Zumbis já não usam nada; threads do kernel não são aplicativos.
            if stat.state == 'Z' || stat.flags & PF_KTHREAD != 0 {
                continue;
            }
            // O `io` é `0400` do dono: os de outros usuários falham, e não tentamos de novo.
            let id = (pid, stat.start);
            let idle = self
                .prev
                .get(&pid)
                .filter(|b| b.start == stat.start && b.ticks == stat.ticks && b.io.is_some());
            let mut io_fresh = false;
            let io = if let Some(b) = idle {
                b.io
            } else if self.io_denied.contains(&id) {
                None
            } else {
                io_fresh = true;
                self.path.truncate(self.path.len() - "stat".len());
                self.path.push_str("io");
                let io = read_proc_file(Path::new(&self.path), &mut self.buf)
                    .ok()
                    .and_then(|b| parse_io(&String::from_utf8_lossy(b)));
                if io.is_none() {
                    self.io_denied.insert(id);
                }
                io
            };
            out.push(Proc {
                pid,
                stat,
                io,
                io_fresh,
            });
        }
        out
    }

    /// Nome do grupo cuja raiz é `p`: o do `.desktop` (e aí os grupos do mesmo
    /// app se juntam, como o Chrome e o `chrome_crashpad_handler`), senão o do
    /// executável.
    fn label(&mut self, p: &Proc) -> Label {
        let id = (p.pid, p.stat.start);
        if let Some(l) = self.labels.get(&id) {
            return l.clone();
        }
        let apps = self
            .apps
            .get_or_insert_with(|| Apps::load(&self.root, &self.app_dirs));
        let cmdline =
            std::fs::read(self.root.join(format!("proc/{}/cmdline", p.pid))).unwrap_or_default();
        let argv0 = argv0(&cmdline);
        let label = match apps.find(&argv0) {
            Some(app) => Label {
                key: format!("app:{}", app.id),
                name: app.name.clone(),
            },
            None => Label {
                key: format!("pid:{}", p.pid),
                name: exe_name(&argv0, &p.stat.comm),
            },
        };
        self.labels.insert(id, label.clone());
        label
    }
}

/// O primeiro argumento. O Chrome e o Electron reescrevem a linha de comando
/// dos processos filhos numa string só (`/opt/google/chrome/chrome --type=renderer …`).
fn argv0(cmdline: &[u8]) -> String {
    let first = cmdline.split(|&b| b == 0).next().unwrap_or_default();
    let s = String::from_utf8_lossy(first);
    match s.find(" --") {
        Some(i) => s[..i].to_string(),
        None => s.into_owned(),
    }
}

/// O `comm` tem no máximo 15 caracteres (`gnome-terminal-`): quando o nome do
/// executável começa com ele, o nome inteiro.
fn exe_name(argv0: &str, comm: &str) -> String {
    let base = argv0.rsplit('/').next().unwrap_or_default();
    if base.len() > comm.len() && base.starts_with(comm) {
        base.to_string()
    } else {
        comm.to_string()
    }
}

/// As três listas, com até [`TOP`] grupos cada. A de disco só tem quem leu ou
/// gravou alguma coisa.
fn top(total: u32, groups: Vec<ProcGroup>) -> ProcTop {
    let pick = |key: &dyn Fn(&ProcGroup) -> f64, keep: &dyn Fn(&ProcGroup) -> bool| {
        let mut list: Vec<ProcGroup> = groups.iter().filter(|g| keep(g)).cloned().collect();
        list.sort_by(|a, b| key(b).total_cmp(&key(a)).then_with(|| a.name.cmp(&b.name)));
        list.truncate(TOP);
        list
    };
    let disk = |g: &ProcGroup| g.read_bps.unwrap_or(0.0) + g.write_bps.unwrap_or(0.0);
    ProcTop {
        total,
        cpu: pick(&|g| g.cpu_pct.unwrap_or(0.0) as f64, &|g| {
            g.cpu_pct.is_some()
        }),
        mem: pick(&|g| g.mem_bytes as f64, &|_| true),
        disk: pick(&disk, &|g| disk(g) > 0.0),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::fixture::Fixture;

    /// Linha do `stat` com os campos que interessam; o resto é zero.
    fn stat(
        pid: u32,
        comm: &str,
        state: char,
        ppid: u32,
        ticks: u64,
        start: u64,
        rss: u64,
    ) -> String {
        let mut f = vec!["0".to_string(); 50];
        f[0] = state.to_string(); // 3
        f[1] = ppid.to_string(); // 4
        f[11] = ticks.to_string(); // 14 (utime)
        f[19] = start.to_string(); // 22
        f[21] = rss.to_string(); // 24
        format!("{pid} ({comm}) {}\n", f.join(" "))
    }

    fn proc(fx: &Fixture, pid: u32, comm: &str, ppid: u32, ticks: u64, rss: u64) {
        fx.write(
            &format!("proc/{pid}/stat"),
            &stat(pid, comm, 'S', ppid, ticks, 1000 + pid as u64, rss),
        );
    }

    fn cmdline(fx: &Fixture, pid: u32, args: &[&str]) {
        fx.write(&format!("proc/{pid}/cmdline"), &(args.join("\0") + "\0"));
    }

    fn io(fx: &Fixture, pid: u32, read: u64, write: u64) {
        fx.write(
            &format!("proc/{pid}/io"),
            &format!("rchar: 9\nwchar: 9\nsyscr: 1\nsyscw: 1\nread_bytes: {read}\nwrite_bytes: {write}\ncancelled_write_bytes: 0\n"),
        );
    }

    #[test]
    fn stat_with_spaces_and_parens_in_comm() {
        let s = parse_stat(&stat(42, "Web Content", 'R', 7, 150, 9000, 2048)).unwrap();
        assert_eq!(s.comm, "Web Content");
        assert_eq!(
            (s.state, s.ppid, s.ticks, s.start, s.rss_pages),
            ('R', 7, 150, 9000, 2048)
        );
        let s = parse_stat(&stat(43, "a) b (c", 'S', 1, 0, 1, 1)).unwrap();
        assert_eq!(s.comm, "a) b (c");
        assert_eq!(s.ppid, 1);
        assert!(parse_stat("43 (truncado) S 1").is_none());
    }

    #[test]
    fn io_fields() {
        assert_eq!(
            parse_io("rchar: 5\nread_bytes: 4096\nwrite_bytes: 8192\ncancelled_write_bytes: 0\n"),
            Some((4096, 8192))
        );
        assert_eq!(parse_io("rchar: 5\n"), None);
    }

    fn reader(fx: &Fixture) -> Procs {
        Procs::new(fx.path(), vec![fx.path().join("usr/share/applications")])
    }

    #[test]
    fn cpu_memory_and_disk_with_two_reads() {
        let fx = Fixture::new("procs");
        let tck = rustix::param::clock_ticks_per_second() as u64;
        let page = rustix::param::page_size() as u64;
        proc(&fx, 1, "systemd", 0, 0, 100);
        proc(&fx, 200, "yes", 1, 0, 50);
        io(&fx, 200, 0, 0);
        proc(&fx, 300, "dd", 1, 0, 10);
        io(&fx, 300, 0, 0);
        cmdline(&fx, 300, &["dd", "if=/dev/zero"]);
        let mut p = reader(&fx);
        let t0 = Instant::now();
        let first = p.read(t0);
        assert_eq!(first.total, 3);
        assert!(first.cpu.is_empty(), "primeira leitura sem CPU");
        assert_eq!(first.mem[0].name, "systemd");
        assert_eq!(first.mem[0].mem_bytes, 100 * page);
        assert!(first.disk.is_empty());

        // 2 s: `yes` usou 2 s de CPU (100% de um núcleo), dd gravou 200 MB.
        proc(&fx, 200, "yes", 1, 2 * tck, 50);
        proc(&fx, 300, "dd", 1, tck / 2, 10);
        io(&fx, 300, 0, 200_000_000);
        let t = p.read(t0 + Duration::from_secs(2));
        assert_eq!(t.cpu[0].name, "yes");
        assert!((t.cpu[0].cpu_pct.unwrap() - 100.0).abs() < 0.5);
        assert!((t.cpu[1].cpu_pct.unwrap() - 25.0).abs() < 0.5);
        assert_eq!(t.disk.len(), 1);
        assert_eq!(t.disk[0].name, "dd");
        assert_eq!(t.disk[0].write_bps, Some(100_000_000.0));
        assert_eq!(t.disk[0].read_bps, Some(0.0));
    }

    #[test]
    fn vanished_new_and_reused_pids() {
        let fx = Fixture::new("procs-gone");
        let tck = rustix::param::clock_ticks_per_second() as u64;
        proc(&fx, 10, "velho", 1, 5 * tck, 1);
        proc(&fx, 11, "some", 1, 0, 1);
        let mut p = reader(&fx);
        let t0 = Instant::now();
        p.read(t0);
        // 11 acabou; 12 começou e já usou 1 s; o pid 10 agora é outro processo.
        std::fs::remove_dir_all(fx.path().join("proc/11")).unwrap();
        proc(&fx, 12, "novo", 1, tck, 1);
        fx.write(
            "proc/10/stat",
            &stat(10, "outro", 'S', 1, tck / 2, 99_999, 1),
        );
        let t = p.read(t0 + Duration::from_secs(1));
        assert_eq!(t.total, 2);
        let cpu = |name: &str| {
            t.cpu
                .iter()
                .find(|g| g.name == name)
                .unwrap()
                .cpu_pct
                .unwrap()
        };
        assert!((cpu("novo") - 100.0).abs() < 0.5);
        assert!(
            (cpu("outro") - 50.0).abs() < 0.5,
            "pid reaproveitado não herda os tempos"
        );
    }

    #[test]
    fn groups_children_with_the_parents_comm() {
        let fx = Fixture::new("procs-group");
        let page = rustix::param::page_size() as u64;
        proc(&fx, 1, "systemd", 0, 0, 1);
        // Chrome: principal → zygote → renderers, todos `chrome`.
        proc(&fx, 100, "chrome", 1, 0, 100);
        proc(&fx, 101, "chrome", 100, 0, 10);
        proc(&fx, 102, "chrome", 101, 0, 10);
        proc(&fx, 103, "chrome", 101, 0, 10);
        // Um `chrome` filho de outro programa não entra no grupo.
        proc(&fx, 200, "bash", 1, 0, 1);
        proc(&fx, 201, "chrome", 200, 0, 5);
        // Mesmo nome, árvores diferentes, sem .desktop: linhas separadas.
        proc(&fx, 300, "node", 1, 0, 3);
        proc(&fx, 301, "node", 1, 0, 3);
        let t = reader(&fx).read(Instant::now());
        let chrome: Vec<&ProcGroup> = t.mem.iter().filter(|g| g.name == "chrome").collect();
        assert_eq!(chrome[0].count, 4);
        assert_eq!(chrome[0].mem_bytes, 130 * page);
        assert_eq!(chrome[1].count, 1);
        assert_eq!(t.mem.iter().filter(|g| g.name == "node").count(), 2);
    }

    #[test]
    fn desktop_name_merges_groups_of_the_same_app() {
        let fx = Fixture::new("procs-app");
        fx.write("opt/google/chrome/chrome", "")
            .write("opt/google/chrome/chrome_crashpad_handler", "")
            .write("usr/bin/google-chrome-stable", "")
            .write(
                "usr/share/applications/google-chrome.desktop",
                "[Desktop Entry]\nType=Application\nName=Google Chrome\nExec=/opt/google/chrome/chrome %U\n",
            );
        proc(&fx, 1, "systemd", 0, 0, 1);
        proc(&fx, 100, "chrome", 1, 0, 100);
        cmdline(&fx, 100, &["/opt/google/chrome/chrome"]);
        proc(&fx, 101, "chrome", 100, 0, 10);
        cmdline(
            &fx,
            101,
            &["/opt/google/chrome/chrome --type=renderer --lang=pt-BR"],
        );
        proc(&fx, 150, "chrome_crashpa", 1, 0, 2);
        cmdline(
            &fx,
            150,
            &[
                "/opt/google/chrome/chrome_crashpad_handler",
                "--monitor-self",
            ],
        );
        // `gnome-terminal-server` tem o `comm` cortado em 15.
        proc(&fx, 400, "gnome-terminal-", 1, 0, 1);
        cmdline(&fx, 400, &["/usr/libexec/gnome-terminal-server"]);
        let t = reader(&fx).read(Instant::now());
        let chrome = t.mem.iter().find(|g| g.name == "Google Chrome").unwrap();
        assert_eq!(
            (chrome.count, chrome.key.as_str()),
            (3, "app:google-chrome")
        );
        assert!(t.mem.iter().any(|g| g.name == "gnome-terminal-server"));
    }

    #[test]
    fn idle_process_io_is_not_reread() {
        let fx = Fixture::new("procs-idle-io");
        let tck = rustix::param::clock_ticks_per_second() as u64;
        proc(&fx, 1, "systemd", 0, 0, 1);
        proc(&fx, 70, "parado", 1, 10, 1);
        io(&fx, 70, 0, 0);
        let mut p = reader(&fx);
        let t0 = Instant::now();
        p.read(t0);
        // Sem CPU: o `io` não é relido (mudar o arquivo não aparece) e a taxa é zero.
        io(&fx, 70, 0, 4_000_000);
        let t = p.read(t0 + Duration::from_secs(2));
        assert!(t.disk.is_empty());
        let g = t.mem.iter().find(|g| g.name == "parado").unwrap();
        assert_eq!(g.write_bps, Some(0.0));
        // Voltou a usar CPU: os 4 MB contam desde a última leitura de fato (4 s).
        proc(&fx, 70, "parado", 1, 10 + tck, 1);
        let t = p.read(t0 + Duration::from_secs(4));
        assert_eq!(t.disk[0].write_bps, Some(1_000_000.0));
    }

    #[test]
    fn io_of_other_users_is_unknown() {
        let fx = Fixture::new("procs-io");
        proc(&fx, 1, "systemd", 0, 0, 1);
        proc(&fx, 50, "meu", 1, 0, 1);
        io(&fx, 50, 0, 0);
        // Processo de outro usuário: o `io` existe, mas não abre.
        proc(&fx, 60, "root", 1, 0, 1);
        io(&fx, 60, 0, 0);
        let denied = fx.path().join("proc/60/io");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&denied).is_ok() {
            return; // Rodando como root: tudo abre.
        }
        let mut p = reader(&fx);
        let t0 = Instant::now();
        p.read(t0);
        proc(&fx, 50, "meu", 1, 1, 1);
        io(&fx, 50, 4096, 0);
        let t = p.read(t0 + Duration::from_secs(1));
        assert_eq!(t.disk.len(), 1);
        assert_eq!(t.disk[0].name, "meu");
        let other = t.mem.iter().find(|g| g.name == "root").unwrap();
        assert_eq!((other.read_bps, other.write_bps), (None, None));
        assert!(p.io_denied.contains(&(60, 1060)), "não tenta abrir de novo");
    }

    #[test]
    fn kernel_threads_and_zombies_are_skipped() {
        let fx = Fixture::new("procs-kthread");
        proc(&fx, 1, "systemd", 0, 0, 1);
        // Campo 9 (flags) com PF_KTHREAD; o `comm` não tem espaço, então é o 9º pedaço.
        let kw = stat(2, "kworker/u8:2-events", 'I', 0, 0, 5, 0);
        let mut f: Vec<String> = kw.split(' ').map(str::to_string).collect();
        f[8] = PF_KTHREAD.to_string();
        fx.write("proc/2/stat", &f.join(" "));
        fx.write("proc/3/stat", &stat(3, "defunto", 'Z', 1, 0, 6, 0));
        fx.write("proc/self/stat", "não é número");
        let t = reader(&fx).read(Instant::now());
        assert_eq!(t.total, 1);
        assert_eq!(t.mem[0].name, "systemd");
    }

    #[test]
    fn lists_are_capped_and_sorted() {
        let fx = Fixture::new("procs-top");
        for pid in 1..=9 {
            proc(&fx, pid, &format!("p{pid}"), 0, 0, pid as u64);
        }
        let t = reader(&fx).read(Instant::now());
        let names: Vec<&str> = t.mem.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["p9", "p8", "p7", "p6", "p5"]);
    }

    #[test]
    fn real_system_reads_processes() {
        let mut p = Procs::new(Path::new("/"), Vec::new());
        let t0 = Instant::now();
        p.read(t0);
        std::thread::sleep(Duration::from_millis(50));
        let t = p.read(Instant::now());
        assert!(t.total > 1);
        assert!(!t.mem.is_empty());
        assert!(t.cpu.iter().all(|g| g.cpu_pct.is_some()));
    }
}
