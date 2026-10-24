//! Nome amigável dos processos pelos arquivos `.desktop` (D50): "Google Chrome"
//! no lugar de `chrome`. Lidos uma vez, na primeira leitura de processos.
//!
//! Um processo é ligado a um `.desktop` pelo `argv[0]`, nesta ordem:
//! - o caminho real (sem links) é o do executável do `Exec`;
//! - o nome do arquivo é o mesmo do `Exec` (`/snap/bin/firefox` e
//!   `/snap/firefox/123/usr/lib/firefox/firefox`);
//! - a pasta é a mesma do executável real do `Exec`, quando é a pasta própria
//!   do app: o `Exec=/usr/bin/google-chrome-stable` é um script em
//!   `/opt/google/chrome/`, e o processo é `/opt/google/chrome/chrome`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Programas que só abrem outro: o nome deles não diz qual app é.
const WRAPPERS: [&str; 16] = [
    "sh",
    "bash",
    "dash",
    "env",
    "python",
    "python3",
    "perl",
    "java",
    "node",
    "flatpak",
    "snap",
    "gio",
    "xdg-open",
    "pkexec",
    "sudo",
    "gtk-launch",
];

/// Onde procurar um `Exec` sem caminho, relativo à raiz.
const PATH: [&str; 5] = ["usr/local/bin", "usr/bin", "bin", "snap/bin", "usr/games"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct App {
    /// Nome do arquivo sem `.desktop`: identifica o app.
    pub id: String,
    pub name: String,
}

#[derive(Default)]
pub(crate) struct Apps {
    apps: Vec<App>,
    by_path: HashMap<PathBuf, (usize, u8)>,
    by_name: HashMap<String, (usize, u8)>,
    by_dir: HashMap<PathBuf, (usize, u8)>,
    root: PathBuf,
}

/// O que importa de um `.desktop`.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    name: String,
    program: String,
    /// `Exec` com argumentos além dos códigos `%U`, `%f`…: um modo ou uma ação
    /// do app (`libreoffice --writer`), e não o app em si.
    has_args: bool,
}

impl Apps {
    /// `dirs` são as pastas de `.desktop` em ordem de prioridade (a do
    /// usuário primeiro): um id repetido vale a primeira vez.
    pub fn load(root: &Path, dirs: &[PathBuf]) -> Self {
        let mut apps = Self {
            // Real, para comparar com os caminhos sem links.
            root: fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()),
            ..Self::default()
        };
        let mut seen = std::collections::HashSet::new();
        for dir in dirs {
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };
            let mut files: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "desktop"))
                .collect();
            files.sort();
            for file in files {
                let Some(id) = file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
                else {
                    continue;
                };
                if !seen.insert(id.clone()) {
                    continue;
                }
                if let Some(entry) = fs::read_to_string(&file).ok().and_then(|t| parse(&t)) {
                    apps.add(id, entry);
                }
            }
        }
        apps
    }

    fn add(&mut self, id: String, e: Entry) {
        let base = basename(&e.program);
        if WRAPPERS.contains(&base) {
            return;
        }
        // Sem argumentos vence: é o app, não um modo dele.
        let score = u8::from(!e.has_args);
        let i = self.apps.len();
        self.apps.push(App { id, name: e.name });

        let token = if e.program.contains('/') {
            Some(self.root.join(e.program.trim_start_matches('/')))
        } else {
            PATH.iter()
                .map(|d| self.root.join(d).join(&e.program))
                .find(|p| p.exists())
        };
        put(&mut self.by_name, base.to_string(), i, score);
        if let Some(token) = token {
            let real = fs::canonicalize(&token).unwrap_or_else(|_| token.clone());
            put(&mut self.by_path, token, i, score);
            // `/snap/bin/firefox` aponta para `/usr/bin/snap`: o caminho real não identifica.
            if !WRAPPERS.contains(&file_name(&real)) {
                if let Some(dir) = real.parent().filter(|d| !self.is_generic(d)) {
                    put(&mut self.by_dir, dir.to_path_buf(), i, score);
                }
                put(&mut self.by_path, real, i, score);
            }
        }
    }

    /// Pastas de muitos programas: a pasta não diz qual app é.
    fn is_generic(&self, dir: &Path) -> bool {
        let rel = dir.strip_prefix(&self.root).unwrap_or(dir);
        let last = file_name(rel);
        matches!(last, "bin" | "sbin" | "libexec" | "games" | "")
            || ["usr/lib", "usr/lib64", "usr/share", "opt", "home", "snap"]
                .iter()
                .any(|g| rel == Path::new(g))
            || rel.parent() == Some(Path::new("home"))
    }

    /// O app do processo com este `argv[0]`.
    pub fn find(&self, argv0: &str) -> Option<&App> {
        let hit = |(i, _): &(usize, u8)| &self.apps[*i];
        if argv0.starts_with('/') {
            let path = self.root.join(argv0.trim_start_matches('/'));
            let real = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if let Some(a) = self.by_path.get(&path).or_else(|| self.by_path.get(&real)) {
                return Some(hit(a));
            }
            if let Some(a) = self.by_name.get(basename(argv0)) {
                return Some(hit(a));
            }
            return real.parent().and_then(|d| self.by_dir.get(d)).map(hit);
        }
        self.by_name.get(basename(argv0)).map(hit)
    }
}

/// Guarda o app `i` em `key`, a menos que já haja um com pontuação maior ou igual.
fn put<K: std::hash::Hash + Eq>(map: &mut HashMap<K, (usize, u8)>, key: K, i: usize, score: u8) {
    let slot = map.entry(key).or_insert((i, score));
    if score > slot.1 {
        *slot = (i, score);
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("")
}

/// A seção `[Desktop Entry]` de um app (`Type=Application`), sem `Hidden` nem
/// `NoDisplay`: os ocultos são ajudantes e painéis ("Claude Code URL Handler",
/// "Wi-Fi" do `gnome-control-center`), e o nome deles enganaria. Nome em pt-BR
/// quando houver.
fn parse(text: &str) -> Option<Entry> {
    let mut in_entry = false;
    let (mut name, mut name_pt, mut name_br, mut exec) = (None, None, None, None);
    let (mut app, mut hidden, mut no_display) = (false, false, false);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            // As ações (`[Desktop Action new-window]`) têm Name e Exec próprios.
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Name" => name = Some(value),
            "Name[pt]" => name_pt = Some(value),
            "Name[pt_BR]" => name_br = Some(value),
            "Exec" => exec = Some(value),
            "Type" => app = value == "Application",
            "Hidden" => hidden = value == "true",
            "NoDisplay" => no_display = value == "true",
            _ => {}
        }
    }
    if !app || hidden || no_display {
        return None;
    }
    let args = split_exec(exec?);
    // `env VAR=valor programa …`
    let mut rest = args
        .iter()
        .skip_while(|a| *a == "env" || (a.contains('=') && !a.starts_with('/')));
    let program = rest.next()?.clone();
    let has_args = rest.any(|a| !a.starts_with('%'));
    Some(Entry {
        name: name_br.or(name_pt).or(name)?.to_string(),
        program,
        has_args,
    })
}

/// Separa o `Exec` em argumentos, com as aspas da especificação
/// (`"/home/eu/Área de trabalho/App/app" --flag %F`).
fn split_exec(exec: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut has = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                has = true;
            }
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if has || !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        args.push(cur);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    #[test]
    fn exec_with_quotes_env_and_codes() {
        assert_eq!(
            split_exec(r#""/home/eu/Área de trabalho/App/app" --no-sandbox %F"#),
            ["/home/eu/Área de trabalho/App/app", "--no-sandbox", "%F"]
        );
        let e = parse(
            "[Desktop Entry]\nType=Application\nName=Firefox\nName[pt_BR]=Navegador Firefox\n\
             Exec=env BAMF_DESKTOP_FILE_HINT=/var/lib/snapd/desktop/applications/firefox_firefox.desktop /snap/bin/firefox %u\n\
             [Desktop Action new-window]\nName=Nova janela\nExec=/snap/bin/firefox --new-window\n",
        )
        .unwrap();
        assert_eq!(e.name, "Navegador Firefox");
        assert_eq!(e.program, "/snap/bin/firefox");
        assert!(!e.has_args, "%u não conta, e a ação não entra");
        assert!(parse("[Desktop Entry]\nType=Link\nName=Site\nExec=x\n").is_none());
        assert!(
            parse("[Desktop Entry]\nType=Application\nName=X\nExec=x\nHidden=true\n").is_none()
        );
    }

    fn desktop(fx: &Fixture, dir: &str, id: &str, body: &str) {
        fx.write(
            &format!("{dir}/{id}.desktop"),
            &format!("[Desktop Entry]\nType=Application\n{body}\n"),
        );
    }

    fn machine() -> (Fixture, Apps) {
        let fx = Fixture::new("apps");
        let sys = "usr/share/applications";
        // Chrome: o Exec é um link para um script na pasta do app.
        fx.write("opt/google/chrome/google-chrome", "#!/bin/sh\n")
            .write("opt/google/chrome/chrome", "")
            .symlink(
                "usr/bin/google-chrome-stable",
                "../../opt/google/chrome/google-chrome",
            );
        desktop(
            &fx,
            sys,
            "google-chrome",
            "Name=Google Chrome\nExec=/usr/bin/google-chrome-stable %U",
        );
        // Configurações: um app e dezenas de painéis ocultos com o mesmo programa.
        fx.write("usr/bin/gnome-control-center", "");
        desktop(
            &fx,
            sys,
            "gnome-wifi-panel",
            "Name=Wi-Fi\nExec=gnome-control-center wifi\nNoDisplay=true",
        );
        desktop(
            &fx,
            sys,
            "org.gnome.Settings",
            "Name=Settings\nName[pt_BR]=Configurações\nExec=gnome-control-center",
        );
        // LibreOffice: um .desktop por modo, todos visíveis; o sem argumentos é o app.
        fx.write("usr/bin/libreoffice", "");
        desktop(
            &fx,
            sys,
            "libreoffice-calc",
            "Name=LibreOffice Calc\nExec=libreoffice --calc %U",
        );
        desktop(
            &fx,
            sys,
            "libreoffice-startcenter",
            "Name=LibreOffice\nExec=libreoffice %U",
        );
        // URL handler oculto: o processo fica com o nome do executável.
        fx.write("home/eu/.local/bin/handler", "");
        desktop(
            &fx,
            "home/eu/.local/share/applications",
            "handler",
            "Name=URL Handler\nExec=/home/eu/.local/bin/handler %u\nNoDisplay=true",
        );
        // Snap: /snap/bin/firefox -> /usr/bin/snap.
        fx.write("usr/bin/snap", "")
            .symlink("snap/bin/firefox", "../../usr/bin/snap");
        desktop(
            &fx,
            "var/lib/snapd/desktop/applications",
            "firefox_firefox",
            "Name=Firefox\nExec=env BAMF_DESKTOP_FILE_HINT=x /snap/bin/firefox %u",
        );
        // Programa do usuário em ~/.local/bin, e um script `sh -c`.
        fx.write("home/eu/.local/bin/claude", "");
        desktop(
            &fx,
            "home/eu/.local/share/applications",
            "claude",
            "Name=Claude\nExec=\"/home/eu/.local/bin/claude\" --handle-uri %u",
        );
        desktop(&fx, sys, "script", "Name=Script\nExec=sh -c \"echo oi\"");
        let dirs = [
            "home/eu/.local/share/applications",
            sys,
            "var/lib/snapd/desktop/applications",
        ]
        .map(|d| fx.path().join(d));
        let apps = Apps::load(fx.path(), &dirs);
        (fx, apps)
    }

    fn name(apps: &Apps, argv0: &str) -> Option<String> {
        apps.find(argv0).map(|a| a.name.clone())
    }

    #[test]
    fn chrome_by_its_own_folder() {
        let (_fx, apps) = machine();
        assert_eq!(
            name(&apps, "/opt/google/chrome/chrome").as_deref(),
            Some("Google Chrome")
        );
        assert_eq!(
            apps.find("/usr/bin/google-chrome-stable")
                .map(|a| a.id.as_str()),
            Some("google-chrome")
        );
    }

    #[test]
    fn hidden_entries_and_modes_do_not_name_the_app() {
        let (_fx, apps) = machine();
        assert_eq!(
            name(&apps, "gnome-control-center").as_deref(),
            Some("Configurações")
        );
        assert_eq!(
            name(&apps, "/usr/bin/gnome-control-center").as_deref(),
            Some("Configurações")
        );
        assert_eq!(
            name(&apps, "/usr/bin/libreoffice").as_deref(),
            Some("LibreOffice")
        );
        assert_eq!(name(&apps, "/home/eu/.local/bin/handler"), None);
    }

    #[test]
    fn snap_by_program_name() {
        let (_fx, apps) = machine();
        assert_eq!(
            name(&apps, "/snap/firefox/7084/usr/lib/firefox/firefox").as_deref(),
            Some("Firefox")
        );
        assert_eq!(name(&apps, "/usr/bin/snap"), None, "o snap não é o Firefox");
    }

    #[test]
    fn generic_folders_and_wrappers_do_not_match() {
        let (_fx, apps) = machine();
        assert_eq!(
            name(&apps, "/home/eu/.local/bin/claude").as_deref(),
            Some("Claude")
        );
        // Outro programa na mesma ~/.local/bin não é o Claude.
        assert_eq!(name(&apps, "/home/eu/.local/bin/outro"), None);
        assert_eq!(name(&apps, "/usr/bin/sh"), None);
        assert_eq!(name(&apps, "sh"), None);
    }
}
