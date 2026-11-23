//! The upload is up: each recipient's description goes in `x.msg.file.descr` parts, to the friend or to each member the file was offered to (Subscriber.hs `SFDONE`, `sendFileDescriptions`).

use zaun_chat_wire::model::{ChatRef, FileState};

use super::send::PQ;
use super::transfers::FileKey;
use crate::chat::file::FileDescr;
use crate::chat::ids::SharedMsgId;
use crate::chat::{ChatEvent, ChatMessage, encode_chat_message};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::{NS, keys};
use crate::engine::time::iso;
use crate::xftp::description::{DESCR_PART_SIZE, split_description};

impl<A: AgentPort> Engine<A> {
    /// `descriptions`: one for a friend, one per member for a group, in the order the file
    /// was offered to them.
    pub(super) fn on_uploaded(
        &mut self,
        key: FileKey,
        descriptions: Vec<String>,
        expires: Option<i64>,
    ) -> Result<()> {
        let (chat, item) = (key.chat(), key.item);
        let members = self.take_file_members(chat, item)?;
        let (it, mut rec) = self.file_of(chat, item)?;
        if Self::file_state(&it, &rec).is_none() {
            // Deleted while it went up: nobody gets a description (`itemDeleted`).
            rec.active = false;
            return self.save_file(chat, item, &rec);
        }
        let shared = it
            .shared_id
            .clone()
            .ok_or_else(|| fail(Code::RequestFailed, "file message has no id"))?;
        // A time on a whole second has no fraction in Haskell's JSON.
        let expires = expires.map(|s| iso(s.saturating_mul(1000)).replace(".000Z", "Z"));
        match chat {
            ChatRef::Direct(friend) => {
                let d = descriptions.first().ok_or_else(no_descriptions)?;
                let conn = self.friend(friend)?.conn;
                for part in descr_parts(&shared, d, expires.as_ref()) {
                    let body = encode_chat_message(&ChatMessage::new(part)?, PQ)?;
                    self.agent.send(&mut self.store, &conn, body)?;
                }
            }
            ChatRef::Group(group) => {
                if descriptions.len() < members.len() {
                    return Err(no_descriptions());
                }
                for (member, d) in members.into_iter().zip(&descriptions) {
                    for part in descr_parts(&shared, d, expires.as_ref()) {
                        let msg = ChatMessage::new(part)?;
                        if let Err(e) = self.send_to_member(group, member, &msg) {
                            self.log
                                .line(format_args!("a file description not sent: {}", e.detail));
                        }
                    }
                }
            }
        }
        rec.active = false;
        self.save_file(chat, item, &rec)?;
        self.show_file(chat, item, &rec, FileState::Sent)
    }

    /// The members a group file was offered to, read once: the row goes with the upload.
    pub(super) fn take_file_members(&mut self, chat: ChatRef, item: i64) -> Result<Vec<i64>> {
        let ChatRef::Group(group) = chat else {
            return Ok(Vec::new());
        };
        let key = keys::file_members(group, item);
        let Some(bytes) = self.store.get(NS, &key)? else {
            return Ok(Vec::new());
        };
        self.store.delete(NS, &key)?;
        Ok(crate::engine::records::decode(&bytes)?)
    }

    /// An event about the user's file to whoever has it: the friend when connected, or every
    /// member reachable now.
    pub(super) fn tell_file_event(&mut self, chat: ChatRef, event: ChatEvent) -> Result<()> {
        let msg = ChatMessage::new(event)?;
        match chat {
            ChatRef::Direct(friend) => {
                let f = self.friend(friend)?;
                if f.reachable() {
                    let body = encode_chat_message(&msg, PQ)?;
                    self.agent.send(&mut self.store, &f.conn, body)?;
                }
            }
            ChatRef::Group(group) => {
                self.send_to_group(group, &msg, None)?;
            }
        }
        Ok(())
    }
}

fn no_descriptions() -> crate::engine::error::EngineError {
    fail(Code::RequestFailed, "not enough file descriptions to send")
}

fn descr_parts(shared: &[u8], description: &str, expires: Option<&String>) -> Vec<ChatEvent> {
    let parts = split_description(description, DESCR_PART_SIZE, DESCR_PART_SIZE);
    parts
        .into_iter()
        .map(|p| ChatEvent::MsgFileDescr {
            msg_id: SharedMsgId(shared.to_vec()),
            file_descr: FileDescr {
                file_descr_text: p.text,
                file_descr_part_no: i64::from(p.part_no),
                file_descr_complete: p.complete,
            },
            file_expires: expires.cloned(),
            file_badge: None,
        })
        .collect()
}
