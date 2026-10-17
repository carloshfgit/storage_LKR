use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::DiskInfo;

/// Tamanho e espaço livre do filesystem que contém `path`.
pub fn disk_info(path: &Path) -> io::Result<DiskInfo> {
    let st = rustix::fs::statvfs(path)?;
    let path = fs::canonicalize(path)?;
    let (mount, device) = find_mount(&path).unwrap_or_else(|| (PathBuf::from("/"), String::new()));

    Ok(DiskInfo {
        mount,
        device,
        total_bytes: st.f_blocks * st.f_frsize,
        available_bytes: st.f_bavail * st.f_frsize,
    })
}

/// Ponto de montagem mais específico que contém `path`, lido de `/proc/self/mounts`.
fn find_mount(path: &Path) -> Option<(PathBuf, String)> {
    let mounts = fs::read_to_string("/proc/self/mounts").ok()?;
    mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let device = fields.next()?;
            let mount = PathBuf::from(unescape_mount(fields.next()?));
            path.starts_with(&mount)
                .then(|| (mount, device.to_string()))
        })
        .max_by_key(|(mount, _)| mount.as_os_str().len())
}

/// `/proc/self/mounts` escapa espaço, tab, `\n` e `\` como octal (`\040`).
fn unescape_mount(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && let Ok(code) = u8::from_str_radix(&raw[i + 1..i + 4], 8)
        {
            out.push(code);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_octal_sequences() {
        assert_eq!(unescape_mount(r"/media/meu\040disco"), "/media/meu disco");
        assert_eq!(unescape_mount("/"), "/");
    }

    #[test]
    fn root_has_space() {
        let info = disk_info(Path::new("/")).unwrap();
        assert!(info.total_bytes > 0);
        assert!(info.available_bytes <= info.total_bytes);
    }
}
