//! Itens da Lixeira: listar com o tamanho, restaurar e apagar os escolhidos. A UI só manda
//! de volta o `id` que recebeu; cada um é conferido com a listagem do sistema relida na
//! hora, e nada vindo da UI vira caminho (DECISIONS D41). A listagem, a restauração e o
//! apagamento chegam como parâmetros, como nas outras ações.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use scanner::{DiskInfo, LockedDirs, ScanResult};
use serde::Serialize;

use super::AppState;
use super::cleanup::{ActionError, Rejected, TrashReport};

/// Item da Lixeira como o sistema o lista (o `.trashinfo`).
#[derive(Debug, Clone, PartialEq)]
pub struct TrashListing {
    /// Caminho do `.trashinfo`.
    pub id: PathBuf,
    pub name: OsString,
    pub original_parent: PathBuf,
    /// Segundos Unix.
    pub deleted_at: i64,
}

impl TrashListing {
    pub fn original_path(&self) -> PathBuf {
        self.original_parent.join(&self.name)
    }

    /// Onde o item está guardado: `<lixeira>/files/<nome do .trashinfo sem a extensão>`.
    pub fn stored_path(&self) -> PathBuf {
        let trash = self
            .id
            .parent()
            .and_then(Path::parent)
            .unwrap_or(Path::new(""));
        trash
            .join("files")
            .join(self.id.file_stem().unwrap_or_default())
    }
}

/// Um item para a UI, com o tamanho do scan (ou medido agora, se o scan não o viu).
#[derive(Debug, Clone, Serialize)]
pub struct TrashItem {
    /// O que a UI manda de volta para restaurar ou apagar.
    pub id: String,
    pub name: String,
    pub original_path: String,
    pub stored_path: String,
    pub deleted_at: i64,
    pub bytes: u64,
    pub files: u64,
    /// Contado no último scan, ou seja, dentro do `trash_bytes` da barra.
    pub counted: bool,
    /// Tem pastas que o usuário não consegue esvaziar: apagar pelo app vai falhar.
    pub blocked: Option<Blocked>,
}

/// Por que um item não pode ser apagado pelo app, e como resolver pelo terminal.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Blocked {
    pub reason: String,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PurgeReport {
    pub purged: u64,
    /// Diferença do `f_bavail` antes e depois, como no `empty_trash`.
    pub freed_bytes: u64,
    pub rejected: Vec<Rejected>,
    pub result: ScanResult,
}

impl AppState {
    /// A Lixeira de agora, os maiores primeiro. O tamanho vem do último scan quando ele
    /// contou o item; senão, `size` mede (algo movido para lá por fora, ou de outro disco).
    /// `locked` procura pastas que o usuário não consegue esvaziar em cada item.
    pub fn trash_items(
        &self,
        list: impl FnOnce() -> Result<Vec<TrashListing>, String>,
        size: impl Fn(&Path) -> (u64, u64),
        locked: impl Fn(&Path) -> LockedDirs,
    ) -> Result<Vec<TrashItem>, ActionError> {
        let listing = list().map_err(ActionError::Failed)?;
        let hints = self.hints();
        let counted: Vec<_> = {
            let last = self.lock_last();
            listing
                .iter()
                .map(|l| {
                    last.as_ref()
                        .and_then(|r| r.find_trash_item(&l.stored_path(), &l.original_path()))
                        .map(|t| (t.bytes, t.files))
                })
                .collect()
        };
        let mut items: Vec<TrashItem> = listing
            .iter()
            .zip(counted)
            .map(|(l, counted)| {
                let stored = l.stored_path();
                let (bytes, files) = counted.unwrap_or_else(|| size(&stored));
                TrashItem {
                    id: l.id.to_string_lossy().into_owned(),
                    name: l.name.to_string_lossy().into_owned(),
                    original_path: l.original_path().to_string_lossy().into_owned(),
                    stored_path: stored.to_string_lossy().into_owned(),
                    deleted_at: l.deleted_at,
                    bytes,
                    files,
                    counted: counted.is_some(),
                    blocked: blocked(l, locked(&stored), hints),
                }
            })
            .collect();
        items.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
        Ok(items)
    }

    /// Devolve os itens ao local original com `restore`, que nunca sobrescreve: um nome já
    /// ocupado lá volta como recusado. Os bytes saem da Lixeira e voltam para a categoria
    /// do caminho original.
    pub fn restore_from_trash(
        &self,
        ids: &[String],
        list: impl FnOnce() -> Result<Vec<TrashListing>, String>,
        size: impl Fn(&Path) -> (u64, u64),
        restore: impl Fn(&TrashListing) -> Result<(), String>,
    ) -> Result<TrashReport, ActionError> {
        self.ensure_idle()?;
        let listing = list().map_err(ActionError::Failed)?;
        let mut last = self.lock_last();
        let result = last.as_mut().ok_or(ActionError::NoScan)?;

        let (found, mut rejected) = select(&listing, ids);
        let mut moved = 0;
        let mut moved_bytes = 0;
        for item in found {
            let (stored, original) = (item.stored_path(), item.original_path());
            let bytes = result
                .find_trash_item(&stored, &original)
                .map_or_else(|| size(&stored).0, |t| t.bytes);
            match restore(item) {
                Ok(()) => {
                    result.record_restored(&stored, &original);
                    moved += 1;
                    moved_bytes += bytes;
                }
                Err(reason) => rejected.push(Rejected {
                    path: original.to_string_lossy().into_owned(),
                    reason,
                    ..Default::default()
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

    /// O que o usuário tem à mão para resolver um item travado: o Docker do sistema (se o
    /// último scan o achou) e o `sudo`.
    fn hints(&self) -> Hints {
        let docker = self
            .lock_last()
            .as_ref()
            .is_some_and(|r| r.docker.rootful.is_some());
        Hints {
            docker,
            admin: user_is_admin(),
        }
    }

    /// Apaga permanentemente só os itens escolhidos. Exige `confirm == true` e mede o
    /// liberado pelo disco.
    pub fn purge_from_trash(
        &self,
        ids: &[String],
        confirm: bool,
        list: impl FnOnce() -> Result<Vec<TrashListing>, String>,
        purge: impl Fn(&TrashListing) -> Result<(), String>,
        disk: impl Fn() -> io::Result<DiskInfo>,
        locked: impl Fn(&Path) -> LockedDirs,
    ) -> Result<PurgeReport, ActionError> {
        self.purge_items(Some(ids), confirm, list, purge, disk, locked)
    }

    /// Esvazia a Lixeira: apaga item por item, como o [`AppState::purge_from_trash`], para
    /// que um item travado (pastas do root, por exemplo) não impeça o resto. Exige
    /// `confirm == true`.
    pub fn empty_trash(
        &self,
        confirm: bool,
        list: impl FnOnce() -> Result<Vec<TrashListing>, String>,
        purge: impl Fn(&TrashListing) -> Result<(), String>,
        disk: impl Fn() -> io::Result<DiskInfo>,
        locked: impl Fn(&Path) -> LockedDirs,
    ) -> Result<PurgeReport, ActionError> {
        self.purge_items(None, confirm, list, purge, disk, locked)
    }

    /// Apaga os itens de `ids` (ou todos, com `None`). Um item que falha volta recusado e
    /// os outros seguem; se a falha vem de pastas travadas, o motivo explica e traz os
    /// comandos para o terminal.
    fn purge_items(
        &self,
        ids: Option<&[String]>,
        confirm: bool,
        list: impl FnOnce() -> Result<Vec<TrashListing>, String>,
        purge: impl Fn(&TrashListing) -> Result<(), String>,
        disk: impl Fn() -> io::Result<DiskInfo>,
        locked: impl Fn(&Path) -> LockedDirs,
    ) -> Result<PurgeReport, ActionError> {
        let mut rejected = Vec::new();
        let mut purged = 0;
        let hints = self.hints();
        let cleanup = self.measured_cleanup(
            confirm,
            || {
                let listing = list()?;
                let (found, mut refused) = match ids {
                    Some(ids) => select(&listing, ids),
                    None => (listing.iter().collect(), Vec::new()),
                };
                let mut done = Vec::new();
                for item in found {
                    match purge(item) {
                        Ok(()) => done.push((item.stored_path(), item.original_path())),
                        Err(error) => {
                            let blocked = blocked(item, locked(&item.stored_path()), hints);
                            refused.push(Rejected {
                                path: item.original_path().to_string_lossy().into_owned(),
                                reason: blocked.as_ref().map_or(error, |b| b.reason.clone()),
                                commands: blocked.map(|b| b.commands).unwrap_or_default(),
                            });
                        }
                    }
                }
                Ok((done, refused))
            },
            disk,
            |r, (done, refused)| {
                purged = done.len() as u64;
                if ids.is_none() && refused.is_empty() {
                    // Tudo apagado: zera também o que não é de nenhum item (os `.trashinfo`).
                    r.clear_trash();
                } else {
                    for (stored, original) in &done {
                        r.record_purged(stored, original);
                    }
                }
                rejected = refused;
            },
        )?;
        Ok(PurgeReport {
            purged,
            freed_bytes: cleanup.freed_bytes,
            rejected,
            result: cleanup.result,
        })
    }
}

/// O que entra nas sugestões de um item travado.
#[derive(Debug, Clone, Copy)]
struct Hints {
    /// Docker do sistema acessível: um container roda como root de verdade e apaga pela
    /// pasta montada, sem sudo. O rootless não serve: o root dele é o próprio usuário.
    docker: bool,
    /// No grupo `sudo`, `admin` ou `wheel`: o `sudo` vai funcionar.
    admin: bool,
}

/// Explica um item com pastas travadas e monta os comandos que resolvem. O caminho vem da
/// listagem do sistema, nunca da UI.
fn blocked(item: &TrashListing, locked: LockedDirs, hints: Hints) -> Option<Blocked> {
    if locked.count == 0 {
        return None;
    }
    let stored = item.stored_path();
    let dirs = if locked.count == 1 {
        "1 pasta".to_string()
    } else {
        format!("{} pastas", locked.count)
    };
    // Um nome que não é UTF-8 viraria `�` no comando, e o caminho copiado não seria o real.
    if stored.to_str().is_none() || item.id.to_str().is_none() {
        return Some(Blocked {
            reason: format!(
                "Tem {dirs} que o seu usuário não pode esvaziar, e o nome do item tem \
                 caracteres que não dá para copiar como comando. Apague pelo gerenciador de \
                 arquivos como administrador."
            ),
            commands: Vec::new(),
        });
    }
    Some(match locked.foreign_owner {
        Some(uid) => {
            let owner = user_name(uid);
            let mut commands = vec![format!(
                "sudo rm -rf -- {} {}",
                sh_quote(&stored),
                sh_quote(&item.id)
            )];
            let docker = uid == 0 && hints.docker;
            if let (true, Some(files), Some(name)) = (docker, stored.parent(), stored.file_name()) {
                let mount = format!("{}:/lixeira", files.display());
                let target = Path::new("/lixeira").join(name);
                commands.push(format!(
                    "docker run --rm -v {} alpine rm -rf -- {} && rm -- {}",
                    sh_quote(Path::new(&mount)),
                    sh_quote(&target),
                    sh_quote(&item.id)
                ));
            }
            let mut reason = format!(
                "Tem {dirs} de {owner}, que o seu usuário não pode esvaziar. Isso costuma \
                 acontecer quando um programa rodou como administrador (um container Docker ou \
                 um comando com sudo) numa pasta do projeto. Apague pelo terminal"
            );
            reason.push_str(match (hints.admin, commands.len() > 1) {
                (true, true) => " com um dos comandos (o segundo usa o Docker e dispensa o sudo):",
                (true, false) => ":",
                (false, true) => {
                    ". Sua conta não é de administrador, então o primeiro comando só funciona \
                     com a senha de quem for; o segundo usa o Docker e dispensa o sudo:"
                }
                (false, false) => {
                    ". Sua conta não é de administrador: peça a quem for para rodar o comando:"
                }
            });
            Blocked { reason, commands }
        }
        None => Blocked {
            reason: format!(
                "Tem {dirs} sem permissão de escrita (comum no cache de módulos do Go). \
                 Libere a escrita e apague de novo pelo app:"
            ),
            commands: vec![format!("chmod -R u+w -- {}", sh_quote(&stored))],
        },
    })
}

/// O processo está num dos grupos que o Ubuntu (`sudo`, `admin`) e outras distros (`wheel`)
/// liberam no sudoers. O root também conta.
fn user_is_admin() -> bool {
    if rustix::process::getuid().is_root() {
        return true;
    }
    let mut gids = vec![rustix::process::getgid().as_raw()];
    if let Ok(groups) = rustix::process::getgroups() {
        gids.extend(groups.iter().map(|g| g.as_raw()));
    }
    let group = std::fs::read_to_string("/etc/group").unwrap_or_default();
    admin_in(&group, &gids)
}

fn admin_in(group: &str, gids: &[u32]) -> bool {
    group.lines().any(|line| {
        let mut fields = line.split(':');
        let name = fields.next().unwrap_or_default();
        let gid = fields.nth(1).and_then(|g| g.parse::<u32>().ok());
        matches!(name, "sudo" | "admin" | "wheel") && gid.is_some_and(|g| gids.contains(&g))
    })
}

/// Nome do usuário pelo `/etc/passwd`; senão, o uid.
fn user_name(uid: u32) -> String {
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    passwd
        .lines()
        .find_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            (fields.nth(1)?.parse() == Ok(uid)).then(|| name.to_string())
        })
        .unwrap_or_else(|| format!("outro usuário (uid {uid})"))
}

/// Entre aspas simples para o shell: nenhum caractere do nome é interpretado.
fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

/// Os itens da listagem com os `ids` pedidos, uma vez cada. Um id que não está na listagem
/// (inventado, ou já restaurado ou apagado por fora) volta recusado.
fn select<'a>(
    listing: &'a [TrashListing],
    ids: &[String],
) -> (Vec<&'a TrashListing>, Vec<Rejected>) {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    let mut rejected = Vec::new();
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        // Comparado como a UI o recebeu; um nome não UTF-8 que colida com outro é recusado.
        let mut matches = listing.iter().filter(|l| l.id.to_string_lossy() == *id);
        match (matches.next(), matches.next()) {
            (Some(item), None) => found.push(item),
            _ => rejected.push(Rejected {
                path: id.clone(),
                reason: "não está mais na Lixeira".into(),
                ..Default::default()
            }),
        }
    }
    (found, rejected)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use scanner::{Category, ScanOptions, scan, tree_size};

    use super::*;

    struct Fixture {
        dir: PathBuf,
        home: PathBuf,
        trash: PathBuf,
        state: Arc<AppState>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    /// `$HOME` com uma Lixeira de verdade no formato freedesktop: uma foto e uma pasta,
    /// cada uma com o seu `.trashinfo`.
    fn fixture(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("tauri-trash-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let home = dir.join("home");
        let trash = home.join(".local/share/Trash");
        fs::create_dir_all(trash.join("files/pasta")).unwrap();
        fs::create_dir_all(trash.join("info")).unwrap();
        fs::create_dir_all(home.join("Imagens")).unwrap();
        fs::write(trash.join("files/foto.jpg"), vec![0u8; 40_000]).unwrap();
        fs::write(trash.join("files/pasta/a.pdf"), vec![0u8; 20_000]).unwrap();
        for n in ["foto.jpg", "pasta"] {
            fs::write(trash.join(format!("info/{n}.trashinfo")), "[Trash Info]\n").unwrap();
        }

        let mut opts = ScanOptions::new(&dir);
        opts.home = home.clone();
        let state = Arc::new(AppState::default());
        *state.lock_last() = Some(scan(&opts, |_| {}).unwrap());
        Fixture {
            dir,
            home,
            trash,
            state,
        }
    }

    impl Fixture {
        fn listing(&self, name: &str, original_parent: &Path) -> TrashListing {
            TrashListing {
                id: self.trash.join(format!("info/{name}.trashinfo")),
                name: name.into(),
                original_parent: original_parent.to_path_buf(),
                deleted_at: 1_700_000_000,
            }
        }

        /// O que o sistema listaria agora.
        fn system(&self) -> Vec<TrashListing> {
            vec![
                self.listing("foto.jpg", &self.home.join("Imagens")),
                self.listing("pasta", &self.home),
            ]
        }

        fn id(&self, name: &str) -> String {
            self.trash
                .join(format!("info/{name}.trashinfo"))
                .to_string_lossy()
                .into_owned()
        }
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

    fn unlocked(_: &Path) -> LockedDirs {
        LockedDirs::default()
    }

    /// Como o `frontend` da Lixeira real: pastas `.next` e `node_modules` do root.
    fn root_locked(_: &Path) -> LockedDirs {
        LockedDirs {
            count: 2,
            foreign_owner: Some(0),
        }
    }

    fn no_size(_: &Path) -> (u64, u64) {
        panic!("o scan já contou esse item")
    }

    /// Soma tudo o que a barra mostra, menos o livre.
    fn bar(r: &ScanResult) -> u64 {
        r.scanned_bytes() + r.docker_bytes + r.unaccounted_bytes
    }

    #[test]
    fn lists_items_with_scanned_sizes() {
        let f = fixture("list");
        // Um item que o scan não viu: o tamanho é medido na hora.
        let mut system = f.system();
        let novo = f.listing("novo.iso", &f.home);
        fs::write(novo.stored_path(), vec![0u8; 90_000]).unwrap();
        system.push(novo.clone());

        let items = f
            .state
            .trash_items(|| Ok(system), tree_size, unlocked)
            .unwrap();
        let names: Vec<_> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["novo.iso", "foto.jpg", "pasta"]);
        assert!(!items[0].counted);
        assert_eq!(
            (items[0].bytes, items[0].files),
            tree_size(&novo.stored_path())
        );
        assert!(items[1].counted && items[2].counted);
        assert_eq!(items[2].files, 1);
        assert_eq!(
            items[1].original_path,
            f.home.join("Imagens/foto.jpg").to_string_lossy()
        );
        assert_eq!(
            items[1].stored_path,
            f.trash.join("files/foto.jpg").to_string_lossy()
        );
    }

    #[test]
    fn restore_moves_bytes_back_to_the_category() {
        let f = fixture("restore");
        let before = f.state.last_result().unwrap();
        let restored = RefCell::new(Vec::new());
        let report = f
            .state
            .restore_from_trash(
                &[f.id("foto.jpg"), f.id("foto.jpg"), f.id("inventado")],
                || Ok(f.system()),
                no_size,
                |l| {
                    restored.borrow_mut().push(l.name.clone());
                    Ok(())
                },
            )
            .unwrap();

        // Pedido duas vezes, restaurado uma.
        assert_eq!(*restored.borrow(), [OsString::from("foto.jpg")]);
        assert_eq!(report.moved, 1);
        assert_eq!(report.rejected.len(), 1);
        assert_eq!(report.rejected[0].reason, "não está mais na Lixeira");

        let r = &report.result;
        assert_eq!(r.trash_bytes, before.trash_bytes - report.moved_bytes);
        assert_eq!(
            r.categories[&Category::Photos].bytes,
            before.categories[&Category::Photos].bytes + report.moved_bytes
        );
        assert!(report.moved_bytes >= 40_000);
        assert_eq!(bar(r), bar(&before));
        assert_eq!(r.trash_items.len(), 1);
    }

    #[test]
    fn restore_collision_is_rejected_without_accounting() {
        let f = fixture("collision");
        let before = f.state.last_result().unwrap();
        let report = f
            .state
            .restore_from_trash(
                &[f.id("pasta")],
                || Ok(f.system()),
                no_size,
                |_| Err("já existe um item com esse nome no local original".into()),
            )
            .unwrap();
        assert_eq!(report.moved, 0);
        assert_eq!(
            report.rejected[0].path,
            f.home.join("pasta").to_string_lossy()
        );
        assert_eq!(report.result.trash_bytes, before.trash_bytes);
    }

    #[test]
    fn purge_needs_confirmation_and_measures_freed_space() {
        let f = fixture("purge");
        let available = Cell::new(100);
        let err = f.state.purge_from_trash(
            &[f.id("foto.jpg")],
            false,
            || panic!("não deveria listar"),
            |_| panic!("não deveria apagar"),
            fake_disk(&available),
            unlocked,
        );
        assert!(matches!(err, Err(ActionError::NotConfirmed)));

        let before = f.state.last_result().unwrap();
        let report = f
            .state
            .purge_from_trash(
                &[f.id("pasta"), f.id("inventado")],
                true,
                || Ok(f.system()),
                |l| {
                    assert_eq!(l.name, "pasta");
                    available.set(100 + 24_000);
                    Ok(())
                },
                fake_disk(&available),
                unlocked,
            )
            .unwrap();
        assert_eq!(report.purged, 1);
        assert_eq!(report.freed_bytes, 24_000);
        assert_eq!(report.rejected.len(), 1);
        let r = &report.result;
        assert!(r.trash_bytes < before.trash_bytes);
        assert_eq!(r.trash_items.len(), 1);
        assert_eq!(r.disk.available_bytes, 24_100);
        // A barra continua fechando o disco: usado = escaneado + Docker + restante.
        assert_eq!(bar(r), r.disk.used_bytes());
    }

    #[test]
    fn actions_are_refused_while_scanning() {
        let f = fixture("busy");
        f.state.running.store(true, Ordering::SeqCst);
        let restore = f.state.restore_from_trash(
            &[f.id("foto.jpg")],
            || Ok(f.system()),
            no_size,
            |_| panic!("não deveria restaurar"),
        );
        assert!(matches!(restore, Err(ActionError::Busy)));
        let available = Cell::new(0);
        let purge = f.state.purge_from_trash(
            &[f.id("foto.jpg")],
            true,
            || Ok(f.system()),
            |_| panic!("não deveria apagar"),
            fake_disk(&available),
            unlocked,
        );
        assert!(matches!(purge, Err(ActionError::Busy)));
    }

    #[test]
    fn lists_blocked_items_with_commands() {
        let f = fixture("blocked");
        let locked = |p: &Path| {
            if p.ends_with("pasta") {
                root_locked(p)
            } else {
                LockedDirs::default()
            }
        };
        // Sem Docker no último scan: só o sudo.
        let items = f
            .state
            .trash_items(|| Ok(f.system()), no_size, locked)
            .unwrap();
        let pasta = items.iter().find(|i| i.name == "pasta").unwrap();
        assert_eq!(pasta.blocked.as_ref().unwrap().commands.len(), 1);

        // Com o Docker do sistema acessível, a versão sem sudo também.
        f.state.lock_last().as_mut().unwrap().docker.rootful = Some(Default::default());
        let items = f
            .state
            .trash_items(|| Ok(f.system()), no_size, locked)
            .unwrap();
        let pasta = items.iter().find(|i| i.name == "pasta").unwrap();
        let foto = items.iter().find(|i| i.name == "foto.jpg").unwrap();
        assert!(foto.blocked.is_none());

        let b = pasta.blocked.as_ref().unwrap();
        assert!(b.reason.contains("2 pastas de root"), "{}", b.reason);
        let stored = f.trash.join("files/pasta");
        let info = f.trash.join("info/pasta.trashinfo");
        assert_eq!(
            b.commands[0],
            format!("sudo rm -rf -- '{}' '{}'", stored.display(), info.display())
        );
        assert_eq!(
            b.commands[1],
            format!(
                "docker run --rm -v '{}:/lixeira' alpine rm -rf -- '/lixeira/pasta' && rm -- '{}'",
                f.trash.join("files").display(),
                info.display()
            )
        );
    }

    #[test]
    fn own_read_only_dirs_suggest_chmod() {
        let f = fixture("chmod");
        let item = f.listing("pasta", &f.home);
        let b = blocked(
            &item,
            LockedDirs {
                count: 1,
                foreign_owner: None,
            },
            ADMIN_WITH_DOCKER,
        )
        .unwrap();
        assert!(
            b.reason.starts_with("Tem 1 pasta sem permissão"),
            "{}",
            b.reason
        );
        assert_eq!(
            b.commands,
            [format!(
                "chmod -R u+w -- '{}'",
                item.stored_path().display()
            )]
        );
        // Outro dono que não o root: só o sudo, o container não ajudaria.
        let b = blocked(
            &item,
            LockedDirs {
                count: 3,
                foreign_owner: Some(4_000_000),
            },
            ADMIN_WITH_DOCKER,
        )
        .unwrap();
        assert!(b.reason.contains("3 pastas de outro usuário (uid 4000000)"));
        assert_eq!(b.commands.len(), 1);
        assert!(blocked(&item, LockedDirs::default(), ADMIN_WITH_DOCKER).is_none());
    }

    const ADMIN_WITH_DOCKER: Hints = Hints {
        docker: true,
        admin: true,
    };

    #[test]
    fn root_dirs_adapt_to_docker_and_sudo() {
        let f = fixture("hints");
        let item = f.listing("pasta", &f.home);
        let root = LockedDirs {
            count: 2,
            foreign_owner: Some(0),
        };
        let with = |docker, admin| blocked(&item, root, Hints { docker, admin }).unwrap();

        let b = with(true, true);
        assert_eq!(b.commands.len(), 2);
        assert!(
            b.reason.ends_with("usa o Docker e dispensa o sudo):"),
            "{}",
            b.reason
        );

        let b = with(false, true);
        assert_eq!(b.commands.len(), 1);
        assert!(b.commands[0].starts_with("sudo rm -rf -- "));
        assert!(b.reason.ends_with("Apague pelo terminal:"), "{}", b.reason);

        let b = with(false, false);
        assert_eq!(b.commands.len(), 1);
        assert!(b.reason.contains("peça a quem for"), "{}", b.reason);

        let b = with(true, false);
        assert_eq!(b.commands.len(), 2);
        assert!(b.reason.contains("não é de administrador"), "{}", b.reason);
    }

    #[test]
    fn non_utf8_names_get_no_commands() {
        use std::os::unix::ffi::OsStrExt;
        let f = fixture("utf8");
        let name = std::ffi::OsStr::from_bytes(b"caf\xe9");
        let mut item = f.listing("x", &f.home);
        item.id = f.trash.join("info").join(name).with_extension("trashinfo");
        item.name = name.to_os_string();
        let b = blocked(
            &item,
            LockedDirs {
                count: 1,
                foreign_owner: Some(0),
            },
            ADMIN_WITH_DOCKER,
        )
        .unwrap();
        assert!(b.commands.is_empty());
        assert!(b.reason.contains("gerenciador de arquivos"));
    }

    #[test]
    fn admin_groups_from_etc_group() {
        let group = "root:x:0:\nsudo:x:27:carlos\ndocker:x:999:carlos\nwheel:x:10:\n";
        assert!(admin_in(group, &[1000, 27]));
        assert!(admin_in(group, &[10]));
        assert!(!admin_in(group, &[1000, 999]));
        assert!(!admin_in("", &[27]));
    }

    #[test]
    fn shell_quoting_survives_quotes_in_names() {
        assert_eq!(sh_quote(Path::new("/a/it's")), r"'/a/it'\''s'");
        assert_eq!(sh_quote(Path::new("/a/$(x) b")), "'/a/$(x) b'");
    }

    #[test]
    fn empty_trash_skips_failures_and_purges_the_rest() {
        let f = fixture("empty-partial");
        let available = Cell::new(100);
        let err = f.state.empty_trash(
            false,
            || panic!("não deveria listar"),
            |_| panic!("não deveria apagar"),
            fake_disk(&available),
            unlocked,
        );
        assert!(matches!(err, Err(ActionError::NotConfirmed)));

        let before = f.state.last_result().unwrap();
        let tried = RefCell::new(Vec::new());
        let report = f
            .state
            .empty_trash(
                true,
                || Ok(f.system()),
                |l| {
                    tried.borrow_mut().push(l.name.clone());
                    if l.name == "pasta" {
                        return Err("Permissão negada".into());
                    }
                    available.set(100 + 40_000);
                    Ok(())
                },
                fake_disk(&available),
                root_locked,
            )
            .unwrap();

        // A pasta travada não impediu a foto.
        assert_eq!(tried.borrow().len(), 2);
        assert_eq!(report.purged, 1);
        assert_eq!(report.freed_bytes, 40_000);
        assert_eq!(report.rejected.len(), 1);
        let r = &report.rejected[0];
        assert_eq!(r.path, f.home.join("pasta").to_string_lossy());
        assert!(r.reason.contains("de root"), "{}", r.reason);
        assert!(r.commands[0].starts_with("sudo rm -rf -- "));

        // Só a foto saiu da conta; a pasta continua na Lixeira e a barra fecha o disco.
        let res = &report.result;
        assert_eq!(res.trash_items.len(), 1);
        assert!(res.trash_bytes > 0 && res.trash_bytes < before.trash_bytes);
        assert_eq!(bar(res), res.disk.used_bytes());
    }

    #[test]
    fn failure_without_locked_dirs_keeps_the_error() {
        let f = fixture("empty-error");
        let available = Cell::new(100);
        let report = f
            .state
            .empty_trash(
                true,
                || Ok(f.system()),
                |l| {
                    if l.name == "foto.jpg" {
                        Err("disco somente leitura".into())
                    } else {
                        Ok(())
                    }
                },
                fake_disk(&available),
                unlocked,
            )
            .unwrap();
        assert_eq!(report.rejected[0].reason, "disco somente leitura");
        assert!(report.rejected[0].commands.is_empty());
    }

    #[test]
    fn empty_trash_clears_everything_when_all_succeed() {
        let f = fixture("empty-all");
        let available = Cell::new(100);
        let report = f
            .state
            .empty_trash(
                true,
                || Ok(f.system()),
                |_| {
                    available.set(available.get() + 30_000);
                    Ok(())
                },
                fake_disk(&available),
                unlocked,
            )
            .unwrap();
        assert_eq!(report.purged, 2);
        assert!(report.rejected.is_empty());
        let r = &report.result;
        assert_eq!((r.trash_bytes, r.trash_files), (0, 0));
        assert!(r.trash_items.is_empty());
        assert_eq!(r.disk.available_bytes, 60_100);
        assert_eq!(bar(r), r.disk.used_bytes());
    }
}
