use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::model::Category;

/// Diretórios de sistema e de apps fora do `$HOME`. Um `.png` de ícone em `/usr/share`
/// é Apps/Sistema, não Fotos.
const SYSTEM_PREFIXES: &[&str] = &[
    "/usr",
    "/opt",
    "/boot",
    "/etc",
    "/bin",
    "/sbin",
    "/var/lib/snapd",
    "/var/lib/flatpak",
    "/var/lib/apt",
    "/var/lib/dpkg",
    "/var/cache/apt",
];

/// Diretórios de apps gerenciados dentro do `$HOME` (relativos a ele). Apagar arquivos
/// daqui pelo gerenciador de arquivos quebra o app, por isso também bloqueiam a limpeza.
const HOME_APP_DIRS: &[&str] = &[
    "snap",
    ".var/app",
    ".local/share/flatpak",
    ".local/share/Steam",
];

/// Pastas convencionais de apps portáteis, logo abaixo do `$HOME`. Diferente de
/// `HOME_APP_DIRS`, o usuário pode apagar o que estiver aqui.
const PORTABLE_APP_DIRS: &[&[u8]] = &[b"Applications", b"Apps", b"Aplicativos", b"bin"];

const TRASH_DIR: &str = ".local/share/Trash";

/// Classifica caminhos absolutos. Os caminhos derivados do `$HOME` são montados uma vez.
#[derive(Debug, Clone)]
pub struct Classifier {
    home: Option<PathBuf>,
    home_app_dirs: Vec<PathBuf>,
    trash: Option<PathBuf>,
}

impl Classifier {
    /// `home` vazio desliga as regras de `$HOME`: tudo vira Apps/Sistema.
    pub fn new(home: &Path) -> Self {
        let home = (!home.as_os_str().is_empty()).then(|| home.to_path_buf());
        Self {
            home_app_dirs: home
                .iter()
                .flat_map(|h| HOME_APP_DIRS.iter().map(|d| h.join(d)))
                .collect(),
            trash: home.as_ref().map(|h| h.join(TRASH_DIR)),
            home,
        }
    }

    /// Regras em ordem; a primeira que casar vence (ARCHITECTURE §5). Arquivo visível no
    /// `$HOME` sem extensão vai para Outros; o walk usa [`Classifier::classify_by_path`]
    /// para olhar o conteúdo antes.
    pub fn classify(&self, path: &Path) -> Category {
        self.classify_by_path(path).unwrap_or(Category::Other)
    }

    /// Como [`Classifier::classify`], mas devolve `None` quando só o conteúdo pode decidir:
    /// arquivo visível no `$HOME`, fora de build/dev e sem extensão (regra 6b). Pura, sem I/O.
    pub fn classify_by_path(&self, path: &Path) -> Option<Category> {
        // 1. Sistema e apps gerenciados.
        if self.is_app_path(path) {
            return Some(Category::AppsSystem);
        }
        let Some(rel) = self.home.as_ref().and_then(|h| path.strip_prefix(h).ok()) else {
            // 8. Fora do $HOME.
            return Some(Category::AppsSystem);
        };
        // 2. Lixeira: vem antes dos apps portáteis para `trash_bytes` caber em Outros.
        if self.in_trash(path) {
            return Some(Category::Other);
        }
        // 3. AppImage em qualquer lugar do $HOME.
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("appimage"))
        {
            return Some(Category::AppsSystem);
        }

        // Uma passada pelos componentes decide as regras 3 a 5. App portátil vence
        // mesmo aparecendo depois de um oculto ou de um node_modules (Electron traz o seu).
        let mut hidden = false;
        let mut dev = false;
        let mut prev: &[u8] = b"";
        for (i, c) in rel.components().enumerate() {
            let name = c.as_os_str().as_bytes();
            // 3. App portátil: pasta convencional no $HOME ou app Electron descompactado.
            if (i == 0 && PORTABLE_APP_DIRS.contains(&name))
                || (prev == b"resources"
                    && matches!(name, b"app" | b"app.asar" | b"app.asar.unpacked"))
            {
                return Some(Category::AppsSystem);
            }
            hidden |= name.first() == Some(&b'.');
            dev |= is_dev_component(prev, name);
            prev = name;
        }
        // 4 e 5. Caches e ocultos, depois artefatos de build/dev.
        if hidden || dev {
            return Some(Category::Other);
        }
        // 6 e 7. Arquivo visível no $HOME: decide pela extensão. Sem extensão (ou terminado
        // em `.`), fica para o conteúdo (6b).
        if path.extension().is_none_or(|e| e.is_empty()) {
            return None;
        }
        Some(by_extension(path).unwrap_or(Category::Other))
    }

    /// Caminho de sistema ou de app gerenciado (regra 1). Também usado para recusar ações
    /// de limpeza; apps portáteis (regra 3) ficam de fora porque podem ser apagados.
    pub fn is_app_path(&self, path: &Path) -> bool {
        let bytes = path.as_os_str().as_bytes();
        SYSTEM_PREFIXES
            .iter()
            .any(|p| is_under(bytes, p.as_bytes()))
            || is_root_lib_dir(path)
            || self
                .home_app_dirs
                .iter()
                .any(|d| is_under(bytes, d.as_os_str().as_bytes()))
    }

    pub fn in_trash(&self, path: &Path) -> bool {
        self.trash
            .as_ref()
            .is_some_and(|t| is_under(path.as_os_str().as_bytes(), t.as_os_str().as_bytes()))
    }
}

/// `path` é `prefix` ou está dentro dele. Compara bytes, mais barato que `Path::starts_with`
/// para o milhão de chamadas do walk.
fn is_under(path: &[u8], prefix: &[u8]) -> bool {
    path.starts_with(prefix) && (path.len() == prefix.len() || path[prefix.len()] == b'/')
}

/// Pastas de dependências e build que só fazem sentido como "Outros". `target` sozinho é
/// nome comum demais; só conta como build do Cargo quando vem seguido de `debug`/`release`.
fn is_dev_component(prev: &[u8], name: &[u8]) -> bool {
    matches!(name, b"node_modules" | b"__pycache__" | b"site-packages")
        || (prev == b"target" && matches!(name, b"debug" | b"release"))
}

/// `/lib`, `/lib32`, `/lib64`, `/libx32` e o que estiver dentro.
fn is_root_lib_dir(path: &Path) -> bool {
    let mut components = path.components();
    components.next() == Some(std::path::Component::RootDir)
        && components.next().is_some_and(|c| {
            let name = c.as_os_str().as_bytes();
            name.starts_with(b"lib") && matches!(&name[3..], b"" | b"32" | b"64" | b"x32")
        })
}

fn by_extension(path: &Path) -> Option<Category> {
    let ext = path.extension()?.to_str()?;
    // Extensões têm no máximo 4 caracteres na tabela; evita alocar para as longas.
    if ext.len() > 4 {
        return None;
    }
    let mut buf = [0u8; 4];
    let lower = &mut buf[..ext.len()];
    lower.copy_from_slice(ext.as_bytes());
    lower.make_ascii_lowercase();

    let category = match &*lower {
        b"jpg" | b"jpeg" | b"png" | b"gif" | b"webp" | b"heic" | b"heif" | b"avif" | b"bmp"
        | b"tif" | b"tiff" | b"svg" | b"raw" | b"dng" | b"cr2" | b"cr3" | b"nef" | b"arw"
        | b"orf" | b"rw2" => Category::Photos,
        b"mp4" | b"m4v" | b"mkv" | b"mov" | b"avi" | b"webm" | b"wmv" | b"flv" | b"mpg"
        | b"mpeg" | b"3gp" => Category::Videos,
        b"mp3" | b"flac" | b"ogg" | b"oga" | b"opus" | b"m4a" | b"aac" | b"wav" | b"aiff"
        | b"wma" => Category::Audio,
        b"pdf" | b"doc" | b"docx" | b"odt" | b"rtf" | b"txt" | b"md" | b"xls" | b"xlsx"
        | b"ods" | b"csv" | b"ppt" | b"pptx" | b"odp" | b"epub" => Category::Documents,
        _ => return None,
    };
    Some(category)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classifier() -> Classifier {
        Classifier::new(Path::new("/home/u"))
    }

    fn cat(p: &str) -> Category {
        classifier().classify(Path::new(p))
    }

    #[test]
    fn plan_cases() {
        assert_eq!(cat("/usr/share/icons/x.png"), Category::AppsSystem);
        assert_eq!(cat("/home/u/.cache/mozilla/a.jpg"), Category::Other);
        assert_eq!(
            cat("/home/u/.local/share/Trash/files/v.mp4"),
            Category::Other
        );
        assert_eq!(cat("/home/u/Vídeos/Férias.MKV"), Category::Videos);
        assert_eq!(cat("/home/u/snap/spotify/foo.mp3"), Category::AppsSystem);
        assert_eq!(cat("/home/u/Downloads/ubuntu.iso"), Category::Other);
        assert_eq!(cat("/home/u/Documentos/cv.pdf"), Category::Documents);
    }

    #[test]
    fn home_app_dirs_win_over_hidden_rule() {
        assert_eq!(
            cat("/home/u/.var/app/com.spotify.Client/cache/x.jpg"),
            Category::AppsSystem
        );
        assert_eq!(
            cat("/home/u/.local/share/Steam/steamapps/game.bin"),
            Category::AppsSystem
        );
        assert_eq!(
            cat("/home/u/.local/share/flatpak/repo/x"),
            Category::AppsSystem
        );
    }

    #[test]
    fn hidden_anywhere_in_home_is_other() {
        assert_eq!(cat("/home/u/.bashrc"), Category::Other);
        assert_eq!(cat("/home/u/projeto/.git/objects/ab.png"), Category::Other);
    }

    #[test]
    fn outside_home_is_system() {
        assert_eq!(cat("/var/log/syslog"), Category::AppsSystem);
        assert_eq!(cat("/swap.img"), Category::AppsSystem);
        assert_eq!(cat("/srv/fotos/a.jpg"), Category::AppsSystem);
        assert_eq!(cat("/lib64/ld-linux.so"), Category::AppsSystem);
    }

    #[test]
    fn prefixes_match_whole_components() {
        // /usrdata não é /usr; ~/snapshots não é ~/snap.
        assert!(!classifier().is_app_path(Path::new("/usrdata/x")));
        assert!(!classifier().is_app_path(Path::new("/library/x")));
        assert_eq!(cat("/home/u/snapshots/a.jpg"), Category::Photos);
        assert_eq!(cat("/home/u"), Category::Other);
    }

    #[test]
    fn trash_detection() {
        let c = classifier();
        assert!(c.in_trash(Path::new("/home/u/.local/share/Trash/files/v.mp4")));
        assert!(!c.in_trash(Path::new("/home/u/.local/share/Trashy/x")));
        assert!(!c.in_trash(Path::new("/home/u/Vídeos/v.mp4")));
    }

    #[test]
    fn portable_apps_are_apps() {
        let ide = "/home/u/Área de trabalho/Antigravity IDE/resources";
        // Electron traz node_modules: o app vence a regra de dev.
        assert_eq!(
            cat(&format!(
                "{ide}/app/node_modules/cytoscape-fcose/demo/demo.gif"
            )),
            Category::AppsSystem
        );
        assert_eq!(cat(&format!("{ide}/app.asar")), Category::AppsSystem);
        assert_eq!(
            cat(&format!("{ide}/app.asar.unpacked/icone.png")),
            Category::AppsSystem
        );
        assert_eq!(
            cat("/home/u/Downloads/Kdenlive.AppImage"),
            Category::AppsSystem
        );
        assert_eq!(
            cat("/home/u/Downloads/outro.appimage"),
            Category::AppsSystem
        );
        assert_eq!(
            cat("/home/u/Applications/a/icone.png"),
            Category::AppsSystem
        );
        assert_eq!(cat("/home/u/Aplicativos/x"), Category::AppsSystem);
        assert_eq!(cat("/home/u/bin/script"), Category::AppsSystem);
        // App portátil vence oculto.
        assert_eq!(
            cat("/home/u/.config/x/resources/app/a.png"),
            Category::AppsSystem
        );
    }

    #[test]
    fn portable_rules_match_exact_components() {
        assert_eq!(cat("/home/u/Fotos/bin/a.jpg"), Category::Photos);
        assert_eq!(cat("/home/u/Fotos/Apps/a.jpg"), Category::Photos);
        assert_eq!(cat("/home/u/myresources/app/a.jpg"), Category::Photos);
        assert_eq!(cat("/home/u/resources/apps/a.jpg"), Category::Photos);
    }

    #[test]
    fn portable_apps_can_be_cleaned() {
        // Só apps gerenciados bloqueiam a limpeza.
        let c = classifier();
        assert!(!c.is_app_path(Path::new("/home/u/Applications/x.AppImage")));
        assert!(!c.is_app_path(Path::new(
            "/home/u/Área de trabalho/IDE/resources/app/a.png"
        )));
        assert!(c.is_app_path(Path::new("/home/u/snap/spotify/x")));
    }

    #[test]
    fn trash_wins_over_portable_apps() {
        assert_eq!(
            cat("/home/u/.local/share/Trash/files/IDE/resources/app/a.png"),
            Category::Other
        );
        assert_eq!(
            cat("/home/u/.local/share/Trash/files/velho.AppImage"),
            Category::Other
        );
    }

    #[test]
    fn dev_artifacts_are_other() {
        assert_eq!(
            cat("/home/u/projeto/node_modules/lib/logo.png"),
            Category::Other
        );
        assert_eq!(
            cat("/home/u/storage_LKR/target/debug/deps/libx.a"),
            Category::Other
        );
        assert_eq!(
            cat("/home/u/storage_LKR/target/release/scan"),
            Category::Other
        );
        assert_eq!(cat("/home/u/py/__pycache__/m.pyc"), Category::Other);
        assert_eq!(
            cat("/home/u/venv/lib/python3.12/site-packages/pkg/a.png"),
            Category::Other
        );
    }

    #[test]
    fn dev_rules_avoid_false_positives() {
        assert_eq!(
            cat("/home/u/Documentos/target/relatorio.pdf"),
            Category::Documents
        );
        assert_eq!(
            cat("/home/u/Documentos/target/debugging.pdf"),
            Category::Documents
        );
        assert_eq!(
            cat("/home/u/Documentos/vendor/nota.pdf"),
            Category::Documents
        );
    }

    #[test]
    fn ts_is_typescript_not_video() {
        assert_eq!(cat("/home/u/projeto/src/main.ts"), Category::Other);
    }

    #[test]
    fn extension_is_case_insensitive() {
        assert_eq!(cat("/home/u/a.JpEg"), Category::Photos);
        assert_eq!(cat("/home/u/Música/x.FLAC"), Category::Audio);
    }

    #[test]
    fn unknown_or_missing_extension_is_other() {
        assert_eq!(cat("/home/u/Makefile"), Category::Other);
        assert_eq!(cat("/home/u/arquivo.extensaolonga"), Category::Other);
    }

    #[test]
    fn only_visible_home_files_without_extension_go_to_content() {
        let c = classifier();
        let by_path = |p: &str| c.classify_by_path(Path::new(p));
        // 6b: só o conteúdo decide.
        assert_eq!(by_path("/home/u/Downloads/foto"), None);
        assert_eq!(by_path("/home/u/Makefile"), None);
        assert_eq!(by_path("/home/u/terminado."), None);
        // Decididos pelo caminho ou pela extensão, mesmo desconhecida: sem leitura.
        assert_eq!(by_path("/home/u/a.xyz"), Some(Category::Other));
        assert_eq!(by_path("/home/u/.cache/foto"), Some(Category::Other));
        assert_eq!(
            by_path("/home/u/proj/node_modules/x"),
            Some(Category::Other)
        );
        assert_eq!(
            by_path("/home/u/.local/share/Trash/files/foto"),
            Some(Category::Other)
        );
        assert_eq!(by_path("/home/u/snap/app/foto"), Some(Category::AppsSystem));
        assert_eq!(
            by_path("/home/u/bin/ferramenta"),
            Some(Category::AppsSystem)
        );
        assert_eq!(by_path("/usr/bin/ls"), Some(Category::AppsSystem));
        assert_eq!(by_path("/var/log/syslog"), Some(Category::AppsSystem));
    }

    #[test]
    fn empty_home_disables_home_rules() {
        let c = Classifier::new(Path::new(""));
        assert_eq!(c.classify(Path::new("/home/u/a.jpg")), Category::AppsSystem);
        assert!(!c.in_trash(Path::new("/.local/share/Trash/x")));
    }
}
