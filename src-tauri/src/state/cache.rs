//! Último resultado salvo em `~/.cache/storage-lkr/last.json`: o app abre mostrando
//! a análise anterior e reescaneia em segundo plano (ARCHITECTURE §9).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use scanner::ScanResult;
use serde::{Deserialize, Serialize};

/// Muda quando o formato do `ScanResult` muda; um cache de outra versão é ignorado.
const VERSION: u32 = 3;

#[derive(Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    /// O `ScanResult` não serializa o `home`, mas as ações validam contra ele.
    home: PathBuf,
    result: ScanResult,
}

/// `$XDG_CACHE_HOME/storage-lkr/last.json`, ou `~/.cache/…` sem a variável.
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("storage-lkr").join("last.json"))
}

/// `None` se o arquivo não existe, está corrompido ou é de outra versão.
pub fn load(path: &Path) -> Option<ScanResult> {
    let text = fs::read_to_string(path).ok()?;
    let file: CacheFile = serde_json::from_str(&text).ok()?;
    if file.version != VERSION {
        return None;
    }
    let mut result = file.result;
    result.home = file.home;
    Some(result)
}

/// Escreve num arquivo temporário e renomeia: um app fechado no meio da escrita não
/// deixa um cache pela metade.
pub fn save(path: &Path, result: &ScanResult) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let file = CacheFile {
        version: VERSION,
        home: result.home.clone(),
        result: result.clone(),
    };
    let json = serde_json::to_vec(&file).map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use scanner::{Category, ScanOptions, scan};

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tauri-cache-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn roundtrip_keeps_result_and_home() {
        let dir = temp_dir("roundtrip");
        let home = dir.join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("foto.jpg"), vec![0u8; 20_000]).unwrap();
        let mut opts = ScanOptions::new(&dir);
        opts.home = home.clone();
        let result = scan(&opts, |_| {}).unwrap();

        let path = dir.join("cache/storage-lkr/last.json");
        save(&path, &result).unwrap();
        let loaded = load(&path).unwrap();

        assert_eq!(loaded.home, home);
        assert_eq!(loaded.finished_at, result.finished_at);
        assert_eq!(
            loaded.categories[&Category::Photos].top[0].path,
            home.join("foto.jpg")
        );
        assert!(!path.with_extension("json.tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_corrupt_or_old_cache_is_ignored() {
        let dir = temp_dir("invalid");
        let path = dir.join("last.json");
        assert!(load(&path).is_none());

        fs::write(&path, "{ não é json").unwrap();
        assert!(load(&path).is_none());

        fs::write(&path, r#"{"version":0,"home":"/home/u","result":{}}"#).unwrap();
        assert!(load(&path).is_none());
        fs::remove_dir_all(&dir).unwrap();
    }
}
