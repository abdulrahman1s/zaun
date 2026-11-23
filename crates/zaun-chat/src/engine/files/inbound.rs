//! A friend's or a member's file: the offer in `x.msg.new`, the description parts, and `x.file.cancel` (Subscriber.hs `processFileInvitation`, `messageFileDescription`, `processFDMessage`, `xFileCancel`).

use zaun_chat_wire::FileKind;
use zaun_chat_wire::model::{ChatRef, FileState, Media, MessageBody};

use super::body::{duration_of, kind_of, preview_of, show};
use super::transfers::FileKey;
use crate::chat::ChatEvent;
use crate::chat::content::MsgContent;
use crate::chat::file::FileInvitation;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::file::{FileRecord, Way};
use crate::xftp::safe_file_name;

/// Pictures up to this size are fetched without asking (the old engine's rule).
const AUTO_BYTES: u64 = 1024 * 1024;

/// What a message with a file invitation makes: the item's body and the file's record.
/// A size of 0 is refused (`validateFileInvitation`); a file offered the old way, by a
/// connection or inline chunks, is shown as failed because this engine cannot fetch it.
pub(crate) fn offer(
    content: &MsgContent,
    inv: &FileInvitation,
) -> Result<(MessageBody, FileRecord)> {
    if inv.file_size == 0 {
        return Err(fail(Code::RequestFailed, "file size is 0"));
    }
    let name = safe_file_name(&inv.file_name);
    let kind = kind_of(content, &name);
    let mut rec = FileRecord::new(Way::Received, kind, None);
    let mut state = FileState::Failed;
    if let Some(d) = &inv.file_descr {
        state = FileState::Offered;
        // Part 0 is the empty placeholder that announces parts to come.
        let placeholder = d.file_descr_part_no == 0 && d.file_descr_text.is_empty();
        if !placeholder {
            // Taken as it stands, as `createRcvFD_` does; later parts follow its number.
            match u32::try_from(d.file_descr_part_no) {
                Ok(n) => {
                    rec.descr.clone_from(&d.file_descr_text);
                    (rec.last_part, rec.descr_complete) = (n, d.file_descr_complete);
                }
                Err(_) => state = FileState::Failed,
            }
        }
    }
    let media = Media {
        name,
        size: inv.file_size,
        preview: preview_of(content),
        duration: duration_of(content),
        caption: content.text().to_owned(),
        local: None,
        state,
    };
    Ok((show(kind, media), rec))
}

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_offered(
        &mut self,
        friend: i64,
        item: i64,
        rec: FileRecord,
        size: u64,
    ) -> Result<()> {
        self.on_offered_in(ChatRef::Direct(friend), item, rec, size, true)
    }

    /// The offer is kept, and a small picture is asked for at once when `auto` allows it
    /// (`accepted`: it starts when the description is whole).
    pub(crate) fn on_offered_in(
        &mut self,
        chat: ChatRef,
        item: i64,
        mut rec: FileRecord,
        size: u64,
        auto: bool,
    ) -> Result<()> {
        let picture = rec.kind == FileKind::Image || rec.kind == FileKind::Gif;
        if auto && picture && size <= AUTO_BYTES && self.item_is_offered(chat, item)? {
            rec.accepted = true;
            self.save_file(chat, item, &rec)?;
            if rec.descr_complete {
                let key = FileKey::of(chat, item);
                if let Err(e) = self.begin_download(key, rec, size) {
                    self.log
                        .line(format_args!("a picture not fetched: {}", e.detail));
                }
                return Ok(());
            }
            return self.show_file(chat, item, &rec, FileState::Receiving { percent: 0 });
        }
        self.save_file(chat, item, &rec)
    }

    fn item_is_offered(&mut self, chat: ChatRef, item: i64) -> Result<bool> {
        let Some(it) = self.item(chat, item)? else {
            return Ok(false);
        };
        Ok(super::body::media_of(&it.body).is_some_and(|m| m.state == FileState::Offered))
    }

    /// The friend's `x.msg.file.descr` and `x.file.cancel`.
    pub(crate) fn on_file_event(
        &mut self,
        friend: i64,
        shared: Option<Vec<u8>>,
        event: ChatEvent,
    ) -> Result<()> {
        let _ = shared;
        self.on_file_event_in(ChatRef::Direct(friend), None, event)
    }

    /// `x.msg.file.descr` and `x.file.cancel` from the friend, or in a group from member
    /// `from`, about a file they offered. The legacy direct-file events (`x.file`,
    /// `x.file.acpt*`, inline chunks) fall through: this engine only speaks XFTP.
    pub(crate) fn on_file_event_in(
        &mut self,
        chat: ChatRef,
        from: Option<i64>,
        event: ChatEvent,
    ) -> Result<()> {
        if let ChatEvent::MsgFileDescr {
            msg_id, file_descr, ..
        } = event
        {
            let part = u32::try_from(file_descr.file_descr_part_no).ok();
            let text = &file_descr.file_descr_text;
            let complete = file_descr.file_descr_complete;
            return self.on_descr_part(chat, from, &msg_id.0, text, part, complete);
        }
        if let ChatEvent::FileCancel { msg_id } = event {
            return self.on_sender_cancel(chat, from, &msg_id.0);
        }
        Ok(())
    }
}
