//! Setting files up at the start: the data folder, the file servers, and the transfers a restart cut short.

use std::fs;

use zaun_chat_wire::Hello;

use super::real::XftpTransfers;
use crate::engine::agent_port::AgentPort;
use crate::engine::machine::Engine;
use crate::engine::records::file::{FileRecord, Way};
use crate::engine::records::{NS, decode, keys};
use crate::store::StoreError;
use crate::xftp::{XftpServer, preset_servers};
use zaun_chat_wire::model::FileState;

/// The file servers of a Hello: its `xftp://` relays, or the presets when it names none.
fn servers_of(relays: &[String]) -> Vec<XftpServer> {
    let own: Vec<XftpServer> = relays
        .iter()
        .filter(|r| r.starts_with("xftp://"))
        .filter_map(|r| r.parse().ok())
        .collect();
    if own.is_empty() {
        preset_servers()
    } else {
        own
    }
}

/// `(friend, item)` of a `file/{friend}/{item}` key.
fn ids_of(key: &[u8]) -> Option<(i64, i64)> {
    let rest = std::str::from_utf8(key).ok()?.strip_prefix("file/")?;
    let (friend, item) = rest.split_once('/')?;
    Some((friend.parse().ok()?, item.parse().ok()?))
}

impl<A: AgentPort> Engine<A> {
    /// Resuming a half-done transfer is left out: a received file goes back to being offered
    /// (its description is kept), a sent one is marked failed.
    pub(crate) fn configure_files(&mut self, hello: &Hello) -> Result<(), StoreError> {
        self.files.dir.clone_from(&hello.dir);
        self.files.servers = servers_of(&hello.relays);
        self.files.transfers = Box::new(XftpTransfers::new());
        if let Err(e) = fs::create_dir_all(self.files.dir.join("files")) {
            self.log.line(format_args!("files folder not made: {e}"));
        }
        // Nothing resumes, so what a killed transfer left is only in the way.
        let _ = fs::remove_dir_all(self.files.dir.join("xftp-tmp")); // not there is fine
        for key in self.store.keys_with_prefix(NS, keys::FILE_PREFIX)? {
            let (Some((friend, item)), Some(bytes)) = (ids_of(&key), self.store.get(NS, &key)?)
            else {
                continue;
            };
            let rec: FileRecord = decode(&bytes)?;
            if rec.active {
                self.end_cut_short(friend, item, rec)?;
            }
        }
        Ok(())
    }

    fn end_cut_short(
        &mut self,
        friend: i64,
        item: i64,
        mut rec: FileRecord,
    ) -> Result<(), StoreError> {
        rec.active = false;
        rec.accepted = false;
        let state = match rec.way {
            Way::Sent => FileState::Failed,
            Way::Received => FileState::Offered,
        };
        self.put(&keys::file(friend, item), &rec)?;
        if let Err(e) = self.show_file(friend, item, &rec, state) {
            self.log
                .line(format_args!("a cut-short transfer not shown: {}", e.detail));
        }
        Ok(())
    }
}
