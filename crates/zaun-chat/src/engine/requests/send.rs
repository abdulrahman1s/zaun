//! Send: a text message to a friend, maybe replying to one of theirs or the user's.

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatRef, MessageAuthor, MessageBody};

use crate::chat::container::MsgContainer;
use crate::chat::content::MsgContent;
use crate::chat::{ChatEvent, ChatMessage, encode_chat_message};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail, not_yet};
use crate::engine::machine::Engine;
use crate::engine::quote::outgoing;
use crate::engine::records::{ItemRecord, keys};

use super::invite::PQ;

impl<A: AgentPort> Engine<A> {
    /// Mentions are carried in groups only (codec `render_text`), so a direct chat drops them.
    pub(crate) fn send_text(
        &mut self,
        chat: ChatRef,
        text: &str,
        reply_to: Option<i64>,
        _mentions: &[(String, i64)],
    ) -> Result<()> {
        let id = match chat {
            ChatRef::Direct(id) => id,
            ChatRef::Group(_) => return Err(not_yet(Code::RequestFailed)),
        };
        let f = self.friend(id)?;
        if !f.reachable() {
            return Err(fail(Code::RequestFailed, "not connected"));
        }
        let content = MsgContent::Text {
            text: text.to_owned(),
        };
        let (container, reply) = match reply_to {
            Some(item) => {
                let q = self.item(id, item)?.and_then(|q| outgoing(item, &q));
                let (quoted, shown) = q.ok_or_else(|| fail(Code::RequestFailed, "no such item"))?;
                (MsgContainer::quote(quoted, content), Some(shown))
            }
            None => (MsgContainer::simple(content), None),
        };
        let msg = ChatMessage::new(ChatEvent::MsgNew(container))?;
        let body = encode_chat_message(&msg, PQ)?;
        let agent_id = self.agent.send(&mut self.store, &f.conn, body)?;
        let now = self.clock.wall_ms();
        let mut item = ItemRecord::new(MessageAuthor::Me, MessageBody::Text(text.to_owned()), now);
        item.reply = reply;
        item.shared_id = msg.msg_id.map(|m| m.0);
        let m = self.add_item(id, &item)?;
        self.put_index(&keys::sent(&f.conn, agent_id), m.id)?;
        self.emit(Event::Messages(vec![m]));
        Ok(())
    }
}
