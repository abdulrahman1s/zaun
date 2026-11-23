//! The sender's description arriving in `x.msg.file.descr` parts (Subscriber.hs `messageFileDescription`, `groupMessageFileDescription`, `processFDMessage`; Store/Files.hs `appendRcvFD`).

use zaun_chat_wire::model::{ChatRef, FileState, MessageAuthor};

use super::body::media_of;
use super::transfers::FileKey;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::ItemRecord;
use crate::engine::records::file::Way;

/// `validSender`: the item was received from `from` (`None`: the friend of a direct chat).
pub(super) fn from_sender(it: &ItemRecord, from: Option<i64>) -> bool {
    it.author != MessageAuthor::Me && it.member == from
}

impl<A: AgentPort> Engine<A> {
    /// A part is added to the file of the message it names; the last one starts the fetch
    /// when the user already asked for the file. An unknown message, someone else's, a file
    /// already here or cancelled by the sender: ignored.
    pub(super) fn on_descr_part(
        &mut self,
        chat: ChatRef,
        from: Option<i64>,
        shared: &[u8],
        text: &str,
        part: Option<u32>,
        complete: bool,
    ) -> Result<()> {
        let Some((item, it)) = self.item_by_shared(chat, from, shared)? else {
            self.log
                .line(format_args!("a file description for no message"));
            return Ok(());
        };
        let Some(mut rec) = self.file_record(chat, item)? else {
            return Ok(());
        };
        let state = Self::file_state(&it, &rec);
        let done = matches!(state, None | Some(FileState::Received(_))) || rec.sender_cancelled;
        if rec.way != Way::Received || !from_sender(&it, from) || done {
            return Ok(());
        }
        let size = media_of(&it.body).map_or(0, |m| m.size);
        let added = part
            .ok_or(())
            .and_then(|n| rec.append(text, n, complete).map_err(|_| ()));
        if added.is_err() {
            rec.accepted = false;
            self.save_file(chat, item, &rec)?;
            self.show_file(chat, item, &rec, FileState::Failed)?;
            self.log
                .line(format_args!("an invalid file description part"));
            return Ok(());
        }
        if rec.descr_complete && rec.accepted && !rec.active {
            return self.begin_download(FileKey::of(chat, item), rec, size);
        }
        self.save_file(chat, item, &rec)
    }
}
