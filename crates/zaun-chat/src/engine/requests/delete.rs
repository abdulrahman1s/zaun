//! Delete: the user deletes a message for everyone, and how a deleted item goes (Commands.hs
//! `APIDeleteChatItem` broadcast; Subscriber.hs `deleteGroupCIs`, `markGroupCIsDeleted`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatRef, MessageBody};

use crate::chat::ChatEvent;
use crate::chat::group::GroupMemberRole;
use crate::chat::ids::SharedMsgId;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::groups::policy;
use crate::engine::machine::Engine;
use crate::engine::records::friend::Feature;
use crate::engine::records::{ItemRecord, NS, keys};

use super::change::DELETE_MS;

impl<A: AgentPort> Engine<A> {
    /// Commands.hs `APIDeleteChatItem` broadcast: the item goes for good when the preferences
    /// allow full deletion for the user, else it stays, marked deleted.
    pub(crate) fn delete(&mut self, chat: ChatRef, item: i64) -> Result<()> {
        self.may_change(chat, &GroupMemberRole::Observer)?;
        let (it, shared) = self.own_item(chat, item, DELETE_MS)?;
        self.send_to_chat(
            chat,
            ChatEvent::MsgDel {
                msg_id: SharedMsgId(shared),
                member_id: None,
                scope: None,
                only_history: false,
            },
        )?;
        let full = match chat {
            ChatRef::Direct(friend) => self.friend(friend)?.enabled(Feature::FullDelete).for_user,
            ChatRef::Group(group) => {
                let g = self.group(group)?;
                policy::allowed(&g, policy::Feature::FullDelete, &g.my_role)
            }
        };
        self.finish_delete_in(chat, item, it, full)
    }

    pub(crate) fn finish_delete(
        &mut self,
        friend: i64,
        item: i64,
        it: ItemRecord,
        full: bool,
    ) -> Result<()> {
        self.finish_delete_in(ChatRef::Direct(friend), item, it, full)
    }

    /// Marks the item deleted, or removes it from the store; the page is told it is deleted either way.
    pub(crate) fn finish_delete_in(
        &mut self,
        chat: ChatRef,
        item: i64,
        mut it: ItemRecord,
        full: bool,
    ) -> Result<()> {
        it.body = MessageBody::Deleted;
        it.reactions.clear();
        if !full {
            return self.save_changed(chat, item, &it);
        }
        self.store.delete_item(&keys::chat(chat), item)?;
        if let Some(shared) = &it.shared_id {
            self.store
                .delete(NS, &keys::shared(chat, it.member, shared))?;
        }
        self.forget_group_reactions(chat, item)?;
        let m = it.to_wire(item, chat, self.read_mark(chat));
        let m = self.shown_last(chat, &it, m);
        self.emit(Event::Updated(vec![m]));
        Ok(())
    }
}
