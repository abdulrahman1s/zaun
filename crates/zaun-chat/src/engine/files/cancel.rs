//! CancelFile, and the sender's `x.file.cancel` (Commands.hs `CancelFile`; Subscriber.hs `xFileCancel`, `xFileCancelGroup`).

use std::sync::atomic::Ordering;

use zaun_chat_wire::model::{ChatRef, FileState, MessageAuthor};

use super::descr::from_sender;
use super::transfers::FileKey;
use crate::chat::ChatEvent;
use crate::chat::ids::SharedMsgId;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::file::Way;

impl<A: AgentPort> Engine<A> {
    /// Stops the job of a transfer, if one runs.
    pub(super) fn stop_transfer(&mut self, key: FileKey) {
        if let Some(a) = self.files.active.remove(&key) {
            a.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The user cancels. A file they sent is cancelled for the others too (`x.file.cancel`); one
    /// they receive only stops here (nothing is sent for XFTP) and can be asked for again.
    pub(crate) fn cancel_file(&mut self, chat: ChatRef, item: i64) -> Result<()> {
        let (it, mut rec) = self.file_of(chat, item)?;
        match Self::file_state(&it, &rec) {
            Some(FileState::Cancelled) => {
                return Err(fail(Code::RequestFailed, "file already cancelled"));
            }
            Some(FileState::Received(_)) if rec.way == Way::Received => {
                return Err(fail(Code::RequestFailed, "file transfer is complete"));
            }
            Some(_) | None => {}
        }
        self.stop_transfer(FileKey::of(chat, item));
        rec.active = false;
        rec.accepted = false;
        self.save_file(chat, item, &rec)?;
        self.show_file(chat, item, &rec, FileState::Cancelled)?;
        if rec.way == Way::Received {
            return Ok(());
        }
        let Some(shared) = it.shared_id.filter(|_| it.author == MessageAuthor::Me) else {
            return Ok(());
        };
        let cancel = ChatEvent::FileCancel {
            msg_id: SharedMsgId(shared),
        };
        self.tell_file_event(chat, cancel)
    }

    /// The sender cancelled the file they offered: whatever was fetched is dropped.
    pub(super) fn on_sender_cancel(
        &mut self,
        chat: ChatRef,
        from: Option<i64>,
        shared: &[u8],
    ) -> Result<()> {
        let Some((item, it)) = self.item_by_shared(chat, from, shared)? else {
            return Ok(());
        };
        let Some(mut rec) = self.file_record(chat, item)? else {
            return Ok(());
        };
        let state = Self::file_state(&it, &rec);
        let over = matches!(state, None | Some(FileState::Received(_)));
        if rec.way != Way::Received || !from_sender(&it, from) || over {
            return Ok(());
        }
        self.stop_transfer(FileKey::of(chat, item));
        rec.active = false;
        rec.accepted = false;
        rec.sender_cancelled = true;
        rec.descr.clear();
        self.save_file(chat, item, &rec)?;
        self.show_file(chat, item, &rec, FileState::Cancelled)
    }
}
