//! Árvore falsa de `proc/` e `sys/` numa pasta temporária, para os testes.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) struct Fixture {
    pub root: PathBuf,
}

impl Fixture {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("monitor-test-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    /// Escreve `text` em `rel` (relativo à raiz), criando as pastas.
    pub fn write(&self, rel: &str, text: &str) -> &Self {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        self
    }

    pub fn dir(&self, rel: &str) -> &Self {
        fs::create_dir_all(self.root.join(rel)).unwrap();
        self
    }

    /// Link simbólico em `rel` apontando para `target` (relativo, como no sysfs).
    pub fn symlink(&self, rel: &str, target: &str) -> &Self {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(target, path).unwrap();
        self
    }

    pub fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
