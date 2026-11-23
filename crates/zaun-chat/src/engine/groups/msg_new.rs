//! A member's new message (`x.msg.new`): checked, kept once, shown with its author, and counted unless the member is blocked (Subscriber.hs `newGroupContentMessage`, `memberCanSend`; Internal.hs `prohibitedGroupContent`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatRef, MessageAuthor, MessageBody};

use super::inbound::GroupCtx;
use super::mention::unquote_mentions;
use super::policy::{Feature, allowed, at_least};
use crate::chat::container::{MsgContainer, MsgScope};
use crate::chat::content::MsgContent;
use crate::chat::group::GroupMemberRole;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::files::offer;
use crate::engine::machine::Engine;
use crate::engine::records::{GroupRecord, ItemRecord, MemberRecord, MemberStatus};

fn dropped(why: &str) -> crate::engine::error::EngineError {
    fail(Code::RequestFailed, why.to_owned())
}

/// Subscriber.hs `memberCanSend`: an observer may not post, and a member support chat
/// (`scope`) is not carried by this engine.
pub(crate) fn member_can_send(m: &MemberRecord, scope: Option<&MsgScope>) -> Result<()> {
    let pending = matches!(
        m.status,
        MemberStatus::PendingApproval | MemberStatus::PendingReview
    );
    match scope {
        Some(MsgScope::Member { .. }) => Err(dropped("member support chats are not carried")),
        None if pending || at_least(&m.role, &GroupMemberRole::Author) => Ok(()),
        None => Err(dropped("member is not allowed to send messages")),
    }
}

/// Internal.hs `prohibitedGroupContent` for what this engine shows: voice and files by role.
fn prohibited(g: &GroupRecord, m: &MemberRecord, c: &MsgContainer) -> Option<&'static str> {
    let voice = matches!(c.content, MsgContent::Voice { .. });
    let media = matches!(
        c.content,
        MsgContent::Image { .. } | MsgContent::Video { .. }
    );
    if voice && !allowed(g, Feature::Voice, &m.role) {
        return Some("voice is not allowed in the group");
    }
    if !voice && (c.file.is_some() || media) && !allowed(g, Feature::Files, &m.role) {
        return Some("files are not allowed in the group");
    }
    None
}

impl<A: AgentPort> Engine<A> {
    /// `at`: when it was sent, for a forwarded one the time its forward names. A check that
    /// fails drops it with an error, so it is not forwarded either.
    pub(crate) fn on_group_msg_new(
        &mut self,
        ctx: GroupCtx,
        shared: Option<Vec<u8>>,
        c: MsgContainer,
        at: i64,
    ) -> Result<()> {
        let chat = ChatRef::Group(ctx.group);
        let mut g = self.group(ctx.group)?;
        let m = self.member(ctx.group, ctx.author)?;
        member_can_send(&m, c.scope.as_ref())?;
        // Subscriber.hs: only an owner may post as the group.
        if c.as_group == Some(true) && !at_least(&m.role, &GroupMemberRole::Owner) {
            return Err(dropped("member is not allowed to send as group"));
        }
        if let Some(s) = &shared
            && self.item_by_shared(chat, Some(m.id), s)?.is_some()
        {
            return Err(dropped("duplicate group message"));
        }
        if let Some(why) = prohibited(&g, &m, &c) {
            return Err(dropped(why));
        }
        let size = c.file.as_ref().map_or(0, |i| i.file_size);
        let offer = c
            .file
            .as_ref()
            .map(|inv| offer(&c.content, inv))
            .transpose()?;
        // A blocked member's mentions are not kept (Subscriber.hs `createNonLive`).
        let mentions = if m.blocked {
            Vec::new()
        } else {
            self.group_incoming_mentions(&g, &c.mentions, c.content.text())
        };
        let names = mentions.iter().map(|x| x.name.as_str());
        let body = offer.as_ref().map_or_else(
            || MessageBody::Text(unquote_mentions(c.content.text(), names)),
            |(b, _)| b.clone(),
        );
        let author = MessageAuthor::Member(m.profile.display_name.clone());
        let mut item = ItemRecord::new(author, body, at);
        item.shared_id = shared;
        item.member = Some(m.id);
        item.mentions = mentions;
        item.reply = c
            .quote
            .as_ref()
            .map(|q| self.group_incoming_quote(&g, q))
            .transpose()?;
        let added = self.add_item(chat, &item)?;
        let shown = self.shown_last(chat, &item, added);
        let id = shown.id;
        if !m.blocked {
            g.unread += 1;
            self.save_group(g)?;
        }
        self.emit(Event::Messages(vec![shown]));
        if let Some((_, rec)) = offer {
            self.on_offered_in(chat, id, rec, size, !m.blocked)?;
        }
        Ok(())
    }
}
