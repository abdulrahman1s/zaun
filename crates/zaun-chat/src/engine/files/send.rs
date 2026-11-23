//! SendFile: the message goes at once with the file's invitation, the upload runs on the pool, and the description follows in parts (Commands.hs `APISendMessages` with `xftpSndFileTransfer`; Subscriber.hs `SFDONE`).

use std::fs;
use std::path::{Path, PathBuf};

use zaun_chat_wire::model::{ChatRef, FileState, Media, MessageAuthor};
use zaun_chat_wire::{Event, FileKind};

use super::body::{content, show};
use super::transfers::{FileKey, Sent, UploadReq};
use crate::chat::container::MsgContainer;
use crate::chat::file::{FileDescr, FileInvitation};
use crate::chat::ids::SharedMsgId;
use crate::chat::{ChatEvent, ChatMessage, encode_chat_message};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::file::{FileRecord, Way};
use crate::engine::records::{ItemRecord, keys};
use crate::engine::time::iso;
use crate::xftp::MAX_FILE_SIZE;
use crate::xftp::description::{DESCR_PART_SIZE, split_description};

/// Every connection this engine makes is PQ.
const PQ: bool = true;

/// The file as the engine may send it: inside its own data folder (a path from outside is
/// never read), a regular file, not empty, within the size limit (`checkSndFile`).
fn checked(dir: &Path, path: &Path) -> Result<(PathBuf, u64)> {
    let own = fs::canonicalize(dir).map_err(|_| fail(Code::RequestFailed, "no data folder"))?;
    let file = fs::canonicalize(path).map_err(|_| fail(Code::RequestFailed, "file not found"))?;
    if !file.starts_with(&own) {
        return Err(fail(Code::RequestFailed, "file is outside the data folder"));
    }
    let meta = fs::metadata(&file).map_err(|_| fail(Code::RequestFailed, "file not found"))?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_FILE_SIZE {
        return Err(fail(Code::RequestFailed, "file size is not allowed"));
    }
    Ok((file, meta.len()))
}

impl<A: AgentPort> Engine<A> {
    pub(crate) fn send_file(
        &mut self,
        chat: ChatRef,
        path: &Path,
        kind: FileKind,
        preview: Option<&[u8]>,
        duration: Option<u32>,
        text: &str,
    ) -> Result<()> {
        let friend = Self::friend_of_chat(chat)?;
        let f = self.friend(friend)?;
        if !f.reachable() {
            return Err(fail(Code::RequestFailed, "not connected"));
        }
        self.room_for_transfer()?;
        let (source, size) = checked(&self.files.dir, path)?;
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| fail(Code::RequestFailed, "file has no name"))?;
        let mut container = MsgContainer::simple(content(kind, text, preview, duration));
        container.file = Some(FileInvitation::xftp(&name, size));
        let msg = ChatMessage::new(ChatEvent::MsgNew(container))?;
        let agent_id = self
            .agent
            .send(&mut self.store, &f.conn, encode_chat_message(&msg, PQ)?)?;
        let media = Media {
            name: name.clone(),
            size,
            preview: preview.map(<[u8]>::to_vec),
            duration,
            caption: text.to_owned(),
            local: Some(source.clone()),
            state: FileState::Sending { percent: 0 },
        };
        let mut item = ItemRecord::new(MessageAuthor::Me, show(kind, media), self.clock.wall_ms());
        item.shared_id = msg.msg_id.map(|m| m.0);
        let m = self.add_item(friend, &item)?;
        self.put_index(&keys::sent(&f.conn, agent_id), m.id)?;
        self.emit(Event::Messages(vec![m.clone()]));
        let mut rec = FileRecord::new(Way::Sent, kind, Some(source.clone()));
        rec.active = true;
        self.save_file(friend, m.id, &rec)?;
        self.start_upload(FileKey { friend, item: m.id }, source, name);
        Ok(())
    }

    fn start_upload(&mut self, key: FileKey, source: PathBuf, name: String) {
        let (job, cancel) = self.files.start(key);
        let req = UploadReq {
            key,
            job,
            source,
            name,
            work_dir: self.files.dir.join("xftp-tmp"),
            servers: self.files.servers.clone(),
            cancel,
        };
        self.files.transfers.upload(req, self.jobs.tx.clone());
    }

    pub(super) fn room_for_transfer(&self) -> Result<()> {
        if self.files.active.len() >= super::MAX_ACTIVE {
            return Err(fail(Code::RequestFailed, "too many files in transfer"));
        }
        Ok(())
    }

    /// The upload is up: the recipient description goes to the friend in parts, and the file is sent.
    pub(super) fn on_uploaded(&mut self, key: FileKey, sent: Sent) -> Result<()> {
        let FileKey { friend, item } = key;
        let (it, mut rec) = self.file_of(friend, item)?;
        if Self::file_state(&it, &rec).is_none() {
            // Deleted while it went up: the friend gets no description (`itemDeleted`).
            rec.active = false;
            return self.save_file(friend, item, &rec);
        }
        let shared = it
            .shared_id
            .clone()
            .ok_or_else(|| fail(Code::RequestFailed, "file message has no id"))?;
        let f = self.friend(friend)?;
        // A time on a whole second has no fraction in Haskell's JSON.
        let expires = sent
            .expires
            .map(|s| iso(s.saturating_mul(1000)).replace(".000Z", "Z"));
        let parts = split_description(&sent.description, DESCR_PART_SIZE, DESCR_PART_SIZE);
        for p in parts {
            let part = ChatEvent::MsgFileDescr {
                msg_id: SharedMsgId(shared.clone()),
                file_descr: FileDescr {
                    file_descr_text: p.text,
                    file_descr_part_no: i64::from(p.part_no),
                    file_descr_complete: p.complete,
                },
                file_expires: expires.clone(),
                file_badge: None,
            };
            let body = encode_chat_message(&ChatMessage::new(part)?, PQ)?;
            self.agent.send(&mut self.store, &f.conn, body)?;
        }
        rec.active = false;
        self.save_file(friend, item, &rec)?;
        self.show_file(friend, item, &rec, FileState::Sent)
    }
}
