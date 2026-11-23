//! A member's edit (`x.msg.update`) and deletion (`x.msg.del`), a moderator's deletion included (Subscriber.hs `groupMessageUpdate`, `groupMessageDelete`, `rcvItemDeletable`).

use zaun_chat_wire::model::{ChatRef, MessageBody};

use super::inbound::GroupCtx;
use super::mention::unquote_mentions;
use super::msg_new::member_can_send;
use super::policy::{Feature, allowed, at_least, rank};
use crate::chat::container::{MsgContainer, MsgMentions, MsgScope};
use crate::chat::content::MsgContent;
use crate::chat::group::GroupMemberRole;
use crate::chat::ids::MemberId;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::files::edit_caption;
use crate::engine::machine::Engine;
use crate::engine::records::{ItemRecord, MemberRecord};

/// `rcvItemDeletable`: 78 hours, a margin for delays in sending.
const AUTHOR_DELETE_MS: i64 = 78 * 3600 * 1000;

fn dropped(why: &str) -> crate::engine::error::EngineError {
    fail(Code::RequestFailed, why.to_owned())
}

impl<A: AgentPort> Engine<A> {
    /// The author's own message, edited; one not here yet arrives as new.
    pub(crate) fn on_group_msg_update(
        &mut self,
        ctx: GroupCtx,
        shared: &[u8],
        content: MsgContent,
        mentions: MsgMentions,
        scope: Option<&MsgScope>,
        at: i64,
    ) -> Result<()> {
        let chat = ChatRef::Group(ctx.group);
        let m = self.member(ctx.group, ctx.author)?;
        member_can_send(&m, scope)?;
        let Some((id, item)) = self.item_by_shared(chat, Some(m.id), shared)? else {
            // `catchCINotFound`: an edit of a message never received is kept as the message.
            let mut c = MsgContainer::simple(content);
            c.mentions = mentions;
            return self.on_group_msg_new(ctx, Some(shared.to_vec()), c, at);
        };
        if item.body == MessageBody::Deleted {
            return Err(dropped("x.msg.update: invalid message update"));
        }
        let g = self.group(ctx.group)?;
        let found = if m.blocked {
            Vec::new()
        } else {
            self.group_incoming_mentions(&g, &mentions, content.text())
        };
        let text = unquote_mentions(content.text(), found.iter().map(|x| x.name.as_str()));
        let mut changed = item.clone();
        edit_caption(&mut changed.body, text);
        changed.mentions = found;
        if changed.body == item.body && changed.mentions == item.mentions {
            return Err(dropped("x.msg.update: not changed"));
        }
        changed.edited = true;
        self.save_changed(chat, id, &changed)
    }

    /// The author deletes within 78 hours; a moderator deletes a member's message (or the
    /// user's) whose role is not above theirs.
    pub(crate) fn on_group_msg_del(
        &mut self,
        ctx: GroupCtx,
        shared: &[u8],
        of: Option<&MemberId>,
        only_history: bool,
        at: i64,
    ) -> Result<()> {
        let sender = self.member(ctx.group, ctx.author)?;
        let author = of.unwrap_or(&sender.member_id).clone();
        let Some((id, item)) = self.group_item_by(ctx.group, &author, shared)? else {
            // A moderation of a message not received yet is not kept for later (`CIModeration`).
            return Ok(());
        };
        if item.body == MessageBody::Deleted {
            return Err(dropped("x.msg.del: already deleted"));
        }
        let by_author = item.member == Some(sender.id);
        let allowed_now = match of {
            None => by_author && !only_history && at - item.at < AUTHOR_DELETE_MS,
            Some(_) if by_author => true,
            Some(_) => self.may_moderate(ctx.group, &sender, &item)?,
        };
        if !allowed_now {
            return Err(dropped(
                "x.msg.del: member attempted invalid message delete",
            ));
        }
        let g = self.group(ctx.group)?;
        let full = allowed(&g, Feature::FullDelete, &sender.role);
        self.finish_delete_in(ChatRef::Group(ctx.group), id, item, full)
    }

    /// `checkRole`: the sender is a moderator at least, and not below the author.
    fn may_moderate(&self, group: i64, sender: &MemberRecord, item: &ItemRecord) -> Result<bool> {
        let author_role = match item.member {
            None => self.group(group)?.my_role,
            Some(a) => self.member(group, a)?.role,
        };
        Ok(at_least(&sender.role, &GroupMemberRole::Moderator)
            && rank(&sender.role) >= rank(&author_role))
    }
}
