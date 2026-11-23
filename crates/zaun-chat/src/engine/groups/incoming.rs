//! What a member's message names: the item it quotes and the members it mentions (Store/Messages.hs `getChatItemQuote_`; Internal.hs `getRcvCIMentions`, `uniqueMsgMentions`).

use zaun_chat_wire::model::{ChatRef, Mention, MessageAuthor, Quote};

use super::mention::mentioned_names;
use crate::chat::container::{MsgMentions, QuotedMsg};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::quote::shown_quote;
use crate::engine::records::GroupRecord;

/// Internal.hs `maxRcvMentions`.
const MAX_RECEIVED_MENTIONS: usize = 5;

impl<A: AgentPort> Engine<A> {
    /// The quote names its author by member id: the user, a member, or someone unknown here.
    pub(crate) fn group_incoming_quote(&mut self, g: &GroupRecord, q: &QuotedMsg) -> Result<Quote> {
        let chat = ChatRef::Group(g.id);
        // Where the quoted item is: `Some(None)` the user's, `Some(Some(m))` member m's.
        let (author, at) = match &q.msg_ref.member_id {
            Some(mid) if *mid == g.my_member_id => (MessageAuthor::Me, Some(None)),
            Some(mid) => match self.member_by_id(g.id, mid) {
                Some(m) => (
                    MessageAuthor::Member(m.profile.display_name.clone()),
                    Some(Some(m.id)),
                ),
                None => (MessageAuthor::Member(String::new()), None),
            },
            None => (MessageAuthor::Member(String::new()), None),
        };
        let id = match (&q.msg_ref.msg_id, at) {
            (Some(shared), Some(member)) => self
                .item_by_shared(chat, member, &shared.0)?
                .map(|(id, _)| id),
            (Some(_) | None, _) => None,
        };
        Ok(shown_quote(id, author, q.content.text()))
    }

    /// `uniqueMsgMentions`: in the order the text names them, at most five, one per member;
    /// a member unknown here is left out. The user's own member number is 0.
    pub(crate) fn group_incoming_mentions(
        &self,
        g: &GroupRecord,
        mentions: &MsgMentions,
        text: &str,
    ) -> Vec<Mention> {
        let mut out: Vec<Mention> = Vec::new();
        let mut seen = Vec::new();
        for name in mentioned_names(text) {
            if seen.len() >= MAX_RECEIVED_MENTIONS {
                break;
            }
            let Some(mm) = mentions.get(name) else {
                continue;
            };
            if seen.contains(&&mm.member_id) {
                continue;
            }
            seen.push(&mm.member_id);
            let member = if mm.member_id == g.my_member_id {
                Some(0)
            } else {
                self.member_by_id(g.id, &mm.member_id).map(|m| m.id)
            };
            if let Some(member) = member {
                out.push(Mention {
                    name: name.to_owned(),
                    member,
                    me: member == 0,
                });
            }
        }
        out
    }
}
