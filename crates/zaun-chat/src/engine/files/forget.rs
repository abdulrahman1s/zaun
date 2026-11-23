//! A removed friend's files: transfers stopped, local copies deleted (Commands.hs
//! `APIDeleteChat` → `deleteFilesLocally`, `cancelFilesInProgress`).

use std::sync::atomic::Ordering;

use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::file::FileRecord;
use crate::engine::records::{NS, decode, keys};

impl<A: AgentPort> Engine<A> {
    /// Before the friend's records go: their records stay for the caller to delete. A group's
    /// files go the same way by its `files::owner` number.
    pub(crate) fn forget_files(&mut self, friend: i64) -> Result<()> {
        let running: Vec<_> = self
            .files
            .active
            .keys()
            .filter(|k| k.friend == friend)
            .copied()
            .collect();
        for key in running {
            if let Some(a) = self.files.active.remove(&key) {
                a.cancel.store(true, Ordering::Relaxed);
            }
        }
        for key in self
            .store
            .keys_with_prefix(NS, &keys::file_prefix(friend))?
        {
            let Some(bytes) = self.store.get(NS, &key)? else {
                continue;
            };
            let rec: FileRecord = decode(&bytes)?;
            // Only copies in the engine's own folder: a path outside it is never ours to delete.
            // Local paths are canonical (send and receive canonicalise them); compare like with like.
            let dir =
                &std::fs::canonicalize(&self.files.dir).unwrap_or_else(|_| self.files.dir.clone());
            let ours = |p: &std::path::PathBuf| !dir.as_os_str().is_empty() && p.starts_with(dir);
            if let Some(p) = rec.local.filter(ours)
                && let Err(e) = std::fs::remove_file(&p)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                self.log.line(format_args!("a file was not deleted: {e}"));
            }
        }
        Ok(())
    }
}
