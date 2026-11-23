//! Send in a group: one item, the message to every member reachable now, with its quote and mentions (Commands.hs `sendGroupContentMessages_`; Internal.hs `prepareGroupMsg`, `getCIMentions`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatRef, Mention, MessageAuthor, MessageBody, Quote};

use super::mention::{mentioned_names, quote_mentions};
use super::policy::at_least;
use crate::chat::container::{MsgContainer, MsgMention, MsgMentions, QuotedMsg};
use crate::chat::content::MsgContent;
use crate::chat::group::GroupMemberRole;
use crate::chat::ids::MemberId;
use crate::chat::{ChatEvent, ChatMessage};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::quote::outgoing;
use crate::engine::records::{GroupRecord, ItemRecord, keys};

/// Internal.hs `maxSndMentions`.
const MAX_SENT_MENTIONS: usize = 3;

impl<A: AgentPort> Engine<A> {
    /// Commands.hs `assertUserGroupRole`: the user is in the group with at least `min`.
    pub(crate) fn assert_my_role(g: &GroupRecord, min: &GroupMemberRole) -> Result<()> {
        if !g.status.current() {
            return Err(fail(
                Code::RequestFailed,
                "not an active member of the group",
            ));
        }
        if !at_least(&g.my_role, min) {
            return Err(fail(
                Code::RequestFailed,
                "the user's group role is too low",
            ));
        }
        Ok(())
    }

    /// The member id of member number `member`; 0 is the user.
    pub(crate) fn member_id_of(&self, g: &GroupRecord, member: i64) -> Result<MemberId> {
        if member == 0 {
            return Ok(g.my_member_id.clone());
        }
        Ok(self.member(g.id, member)?.member_id)
    }

    pub(crate) fn send_group_text(
        &mut self,
        group: i64,
        text: &str,
        reply_to: Option<i64>,
        mentions: &[(String, i64)],
    ) -> Result<()> {
        let g = self.group(group)?;
        Self::assert_my_role(&g, &GroupMemberRole::Author)?;
        let wire_text = quote_mentions(text, mentions.iter().map(|(n, _)| n.as_str()));
        let msg_mentions = self.outgoing_mentions(&g, &wire_text, mentions)?;
        let content = MsgContent::Text { text: wire_text };
        let (mut container, reply) = match reply_to {
            Some(item) => {
                let (q, shown) = self.group_quote(&g, item)?;
                (MsgContainer::quote(q, content), Some(shown))
            }
            None => (MsgContainer::simple(content), None),
        };
        container.mentions = msg_mentions;
        let msg = ChatMessage::new(ChatEvent::MsgNew(container))?;
        let sent = self.send_to_group(group, &msg, None)?;
        let now = self.clock.wall_ms();
        let mut item = ItemRecord::new(MessageAuthor::Me, MessageBody::Text(text.to_owned()), now);
        item.reply = reply;
        item.shared_id = msg.msg_id.map(|m| m.0);
        item.mentions = mentions
            .iter()
            .map(|(name, member)| Mention {
                name: name.clone(),
                member: *member,
                me: *member == 0,
            })
            .collect();
        let m = self.add_item(ChatRef::Group(group), &item)?;
        for (_, conn, agent_id) in sent {
            self.put_index(&keys::sent(&conn, agent_id), m.id)?;
        }
        self.emit(Event::Messages(vec![m]));
        Ok(())
    }

    /// `getCIMentions`: at most three, each named in the text, each a different member.
    fn outgoing_mentions(
        &self,
        g: &GroupRecord,
        wire_text: &str,
        mentions: &[(String, i64)],
    ) -> Result<MsgMentions> {
        let named = mentioned_names(wire_text);
        let mut out = MsgMentions::new();
        let mut members = Vec::with_capacity(mentions.len());
        for (name, member) in mentions {
            if !named.contains(&name.as_str()) || members.contains(member) {
                return Err(fail(Code::RequestFailed, "invalid mention"));
            }
            members.push(*member);
            let member_id = self.member_id_of(g, *member)?;
            out.insert(name.clone(), MsgMention { member_id });
        }
        if out.len() > MAX_SENT_MENTIONS {
            return Err(fail(Code::RequestFailed, "invalid mention"));
        }
        Ok(out)
    }

    /// `prepareGroupMsg` `quoteData`: a deleted item or a notice cannot be quoted; the quote
    /// names its author's member id (the user's own for the user's).
    fn group_quote(&mut self, g: &GroupRecord, item: i64) -> Result<(QuotedMsg, Quote)> {
        let q = self
            .item(ChatRef::Group(g.id), item)?
            .filter(|q| q.body != MessageBody::Deleted)
            .ok_or_else(|| fail(Code::RequestFailed, "no such item"))?;
        let (mut quoted, shown) =
            outgoing(item, &q).ok_or_else(|| fail(Code::RequestFailed, "no such item"))?;
        quoted.msg_ref.member_id = Some(self.member_id_of(g, q.member.unwrap_or(0))?);
        Ok((quoted, shown))
    }
}
