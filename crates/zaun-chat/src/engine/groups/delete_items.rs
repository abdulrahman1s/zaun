//! A removed member's messages show as deleted (Subscriber.hs `xGrpMemDel` `deleteMessages`, Store `markGroupMemberCIsDeleted`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatRef, MessageAuthor, MessageBody};

use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::{ItemRecord, decode, keys};

/// Items read per page.
const PAGE: u32 = 500;

impl<A: AgentPort> Engine<A> {
    /// What `member` wrote (`None`: the user) is marked deleted; notices stay.
    pub(crate) fn mark_member_items_deleted(
        &mut self,
        group: i64,
        member: Option<i64>,
    ) -> Result<()> {
        let chat = ChatRef::Group(group);
        let mut changed = Vec::new();
        let mut before = None;
        loop {
            let page = self.store.page(&keys::chat(chat), before, PAGE)?;
            let Some(&(oldest, _)) = page.first() else {
                break;
            };
            before = Some(oldest);
            for (id, bytes) in page {
                let mut item: ItemRecord = decode(&bytes)?;
                let theirs =
                    item.member == member && (member.is_some() || item.author == MessageAuthor::Me);
                if !theirs || matches!(item.body, MessageBody::Notice(_) | MessageBody::Deleted) {
                    continue;
                }
                item.body = MessageBody::Deleted;
                changed.push(self.change_item(chat, id, &item)?);
            }
        }
        if !changed.is_empty() {
            self.emit(Event::Updated(changed));
        }
        Ok(())
    }
}
