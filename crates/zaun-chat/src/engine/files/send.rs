//! SendFile: the message goes at once with the file's invitation and the upload runs on the pool (Commands.hs `APISendMessages` with `xftpSndFileTransfer`); the description follows (`uploaded.rs`).

use std::path::{Path, PathBuf};

use zaun_chat_wire::model::{ChatRef, FileState, Media, MessageAuthor};
use zaun_chat_wire::{Event, FileKind};

use super::body::{content, show};
use super::offer::checked;
use super::transfers::{FileKey, UploadReq};
use crate::chat::container::MsgContainer;
use crate::chat::file::FileInvitation;
use crate::chat::{ChatEvent, ChatMessage};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::file::{FileRecord, Way};
use crate::engine::records::{ItemRecord, keys};

/// Every friend's connection this engine makes is PQ.
pub(super) const PQ: bool = true;

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
        if self.files.servers.is_empty() {
            return Err(fail(Code::RequestFailed, "no file server"));
        }
        self.may_send_file(chat)?;
        self.room_for_transfer()?;
        let (source, size) = checked(&self.files.dir, path)?;
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| fail(Code::RequestFailed, "file has no name"))?;
        let mut container = MsgContainer::simple(content(kind, text, preview, duration));
        container.file = Some(FileInvitation::xftp(&name, size));
        let msg = ChatMessage::new(ChatEvent::MsgNew(container))?;
        let sent = self.send_offer(chat, &msg)?;
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
        let m = self.add_item(chat, &item)?;
        for (_, conn, agent_id) in &sent {
            self.put_index(&keys::sent(conn, *agent_id), m.id)?;
        }
        self.emit(Event::Messages(vec![m.clone()]));
        let mut rec = FileRecord::new(Way::Sent, kind, Some(source.clone()));
        rec.active = true;
        self.save_file(chat, m.id, &rec)?;
        let members = match chat {
            ChatRef::Direct(_) => None,
            ChatRef::Group(g) => {
                let to: Vec<i64> = sent.iter().map(|(member, _, _)| *member).collect();
                self.put(&keys::file_members(g, m.id), &to)?;
                Some(to.len())
            }
        };
        self.start_upload(FileKey::of(chat, m.id), source, name, members);
        Ok(())
    }

    fn start_upload(
        &mut self,
        key: FileKey,
        source: PathBuf,
        name: String,
        members: Option<usize>,
    ) {
        let (job, cancel) = self.files.start(key);
        let req = UploadReq {
            key,
            members,
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
}
