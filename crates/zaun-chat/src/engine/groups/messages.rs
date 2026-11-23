//! Messages in a group: each content event to its handler, and the user's own messages' delivery (Subscriber.hs `processGroupMessage` content events, `updateGroupItemsStatus`, `membersGroupItemStatus`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatRef, Delivery};

use super::inbound::GroupCtx;
use crate::chat::ChatEvent;
use crate::engine::agent_port::{AgentMsgId, AgentPort, ConnId};
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::{NS, keys};

/// How far a group item got with any member: `membersGroupItemStatus` shows the furthest
/// (partial counts as reached), and a member's error never changes the item.
fn rank(d: Delivery) -> u8 {
    match d {
        Delivery::Sending | Delivery::Received | Delivery::Failed => 0,
        Delivery::Sent => 1,
        Delivery::Delivered => 2,
    }
}

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_group_message_event(
        &mut self,
        ctx: GroupCtx,
        shared: Option<Vec<u8>>,
        e: ChatEvent,
    ) -> Result<()> {
        let now = self.clock.wall_ms();
        self.on_group_message_at(ctx, shared, e, now)
    }

    /// `at`: when it was sent (a forward names it). The legacy SMP file events (`x.file`,
    /// `x.file.acpt*`, inline chunks) fall through: this engine only speaks XFTP.
    pub(crate) fn on_group_message_at(
        &mut self,
        ctx: GroupCtx,
        shared: Option<Vec<u8>>,
        e: ChatEvent,
        at: i64,
    ) -> Result<()> {
        if let ChatEvent::MsgNew(c) = e {
            return self.on_group_msg_new(ctx, shared, c, at);
        }
        if let ChatEvent::MsgUpdate {
            msg_id,
            content,
            mentions,
            scope,
            ..
        } = e
        {
            return self.on_group_msg_update(ctx, &msg_id.0, content, mentions, scope.as_ref(), at);
        }
        if let ChatEvent::MsgDel {
            msg_id,
            member_id,
            only_history,
            ..
        } = e
        {
            return self.on_group_msg_del(ctx, &msg_id.0, member_id.as_ref(), only_history, at);
        }
        if let ChatEvent::MsgReact {
            msg_id,
            member_id,
            reaction,
            add,
            ..
        } = e
        {
            return self.on_group_react(ctx, &msg_id.0, member_id.as_ref(), &reaction, add);
        }
        if let e @ (ChatEvent::MsgFileDescr { .. } | ChatEvent::FileCancel { .. }) = e {
            return self.on_file_event_in(ChatRef::Group(ctx.group), Some(ctx.author), e);
        }
        Ok(())
    }

    /// Sent, Rcvd or Merr for a message on member `_member`'s connection. The index row goes
    /// with the receipt or the error: nothing follows them on that connection.
    pub(crate) fn on_group_delivery(
        &mut self,
        group: i64,
        _member: i64,
        conn: &ConnId,
        msg: AgentMsgId,
        to: Delivery,
    ) -> Result<()> {
        let key = keys::sent(conn, msg);
        let Some(id) = self.get_index(&key)? else {
            return Ok(());
        };
        if matches!(to, Delivery::Delivered | Delivery::Failed) {
            self.store.delete(NS, &key)?;
        }
        let chat = ChatRef::Group(group);
        let Some(mut item) = self.item(chat, id)? else {
            return Ok(());
        };
        if rank(to) <= rank(item.delivery) {
            return Ok(());
        }
        item.delivery = to;
        let m = self.change_item(chat, id, &item)?;
        self.emit(Event::Updated(vec![m]));
        Ok(())
    }
}
