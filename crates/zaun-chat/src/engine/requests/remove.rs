//! RemoveFriend: tell them (`x.direct.del`), delete the connection, forget the conversation.

use crate::chat::{ChatEvent, ChatMessage, encode_chat_message};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::{NS, keys};
use crate::store::Op;

/// Items deleted per page while forgetting a conversation.
const PAGE: u32 = 500;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn remove_friend(&mut self, id: i64) -> Result<()> {
        let f = self
            .state
            .friends
            .get(&id)
            .cloned()
            .ok_or_else(|| fail(Code::RequestFailed, "no such friend"))?;
        if f.reachable() {
            let msg = ChatMessage::new(ChatEvent::DirectDel)?;
            let body = encode_chat_message(&msg, true)?;
            if let Err(e) = self.agent.send(&mut self.store, &f.conn, body) {
                self.log
                    .line(format_args!("x.direct.del not sent: {}", e.detail()));
            }
        }
        self.forget_conn(&f.conn)?;
        self.forget_files(id)?;
        self.forget_items(id)?;
        let mut ops = vec![Op::Delete {
            ns: NS,
            key: keys::friend(id),
        }];
        let prefixes = [
            keys::shared_prefix(id),
            keys::sent_prefix(&f.conn),
            keys::file_prefix(id),
        ];
        for prefix in prefixes {
            for key in self.store.keys_with_prefix(NS, &prefix)? {
                ops.push(Op::Delete { ns: NS, key });
            }
        }
        self.store.apply(ops)?;
        self.state.friends.remove(&id);
        self.state.last.remove(&id);
        self.state.chats_dirty = true;
        Ok(())
    }

    fn forget_items(&mut self, id: i64) -> Result<()> {
        let chat = keys::direct_chat(id);
        loop {
            let page = self.store.page(&chat, None, PAGE)?;
            if page.is_empty() {
                return Ok(());
            }
            let ops = page
                .into_iter()
                .map(|(item, _)| Op::DeleteItem {
                    chat_ref: chat.clone(),
                    id: item,
                })
                .collect();
            self.store.apply(ops)?;
        }
    }
}
