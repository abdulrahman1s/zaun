mod atomic;
mod disk;
mod items;
mod reopen;

use std::path::PathBuf;

use super::Store;

/// A scratch directory removed on drop.
pub(super) struct Dir(pub PathBuf);

impl Dir {
    pub(super) fn new() -> Dir {
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).expect("os randomness");
        let name: String = id.iter().map(|b| format!("{b:02x}")).collect();
        let dir = std::env::temp_dir().join(format!("zaun-store-{name}"));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Dir(dir)
    }

    pub(super) fn db(&self) -> PathBuf {
        self.0.join("chat.db")
    }

    pub(super) fn store(&self, key: &str) -> Store {
        Store::open(&self.db(), key).expect("open store")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
