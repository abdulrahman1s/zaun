//! React: the user adds or takes back a reaction, to a friend or a group (Commands.hs `APIChatItemReaction`).

use zaun_chat_wire::model::{ChatRef, Reaction};

use crate::chat::ChatEvent;
use crate::chat::ids::SharedMsgId;
use crate::chat::reaction::{MsgReaction, REACTION_EMOJI};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::groups::policy;
use crate::engine::machine::Engine;
use crate::engine::records::friend::Feature;

use super::change::has_content;

/// Internal.hs `maxMsgReactions`.
const MAX_REACTIONS: usize = 3;

/// The page's emoji in the form it travels: ❤ without its variation selector, one of the eight.
fn wire_emoji(emoji: &str) -> Option<char> {
    let mut it = emoji.chars().filter(|&c| c != '\u{fe0f}');
    let c = it.next()?;
    (it.next().is_none() && REACTION_EMOJI.contains(c)).then_some(c)
}

/// The user's part of a reaction count, taken or given.
fn react_mine(reactions: &mut Vec<Reaction>, emoji: String, on: bool) {
    let i = reactions.iter().position(|r| r.emoji == emoji);
    match (i, on) {
        (Some(i), true) => {
            reactions[i].mine = true;
            reactions[i].count += 1;
        }
        (None, true) => reactions.push(Reaction {
            emoji,
            count: 1,
            mine: true,
            who: Vec::new(),
        }),
        (Some(i), false) => {
            reactions[i].mine = false;
            reactions[i].count = reactions[i].count.saturating_sub(1);
            if reactions[i].count == 0 {
                reactions.remove(i);
            }
        }
        (None, false) => {}
    }
}

impl<A: AgentPort> Engine<A> {
    /// Whether the user may react in the chat: the friend's preferences, or the group's.
    fn reactions_on(&self, chat: ChatRef) -> Result<bool> {
        Ok(match chat {
            ChatRef::Direct(friend) => self.friend(friend)?.enabled(Feature::Reactions).for_user,
            ChatRef::Group(group) => policy::on(&self.group(group)?, policy::Feature::Reactions),
        })
    }

    pub(crate) fn react(&mut self, chat: ChatRef, item: i64, emoji: &str, on: bool) -> Result<()> {
        if !self.reactions_on(chat)? {
            return Err(fail(Code::RequestFailed, "feature not allowed: reactions"));
        }
        let c = wire_emoji(emoji).ok_or_else(|| {
            fail(
                Code::RequestFailed,
                format!("not a reaction the engine carries: {emoji}"),
            )
        })?;
        let mut it = self
            .item(chat, item)?
            .ok_or_else(|| fail(Code::RequestFailed, "no such item"))?;
        let shown = c.to_string().replace('\u{2764}', "\u{2764}\u{fe0f}");
        let mine: Vec<&Reaction> = it.reactions.iter().filter(|r| r.mine).collect();
        let had = mine.iter().any(|r| r.emoji == shown);
        let full = on && mine.len() >= MAX_REACTIONS;
        let shared = match it.shared_id.clone() {
            Some(s) if has_content(&it.body) && had != on && !full => s,
            Some(_) | None => return Err(fail(Code::RequestFailed, "reaction not allowed")),
        };
        // In a group the reaction names the item's author (Commands.hs `chatItemMember`).
        let member_id = match chat {
            ChatRef::Direct(_) => None,
            ChatRef::Group(group) => {
                let g = self.group(group)?;
                Some(self.member_id_of(&g, it.member.unwrap_or(0))?)
            }
        };
        self.send_to_chat(
            chat,
            ChatEvent::MsgReact {
                msg_id: SharedMsgId(shared),
                member_id,
                scope: None,
                reaction: MsgReaction::Emoji(c),
                add: on,
            },
        )?;
        react_mine(&mut it.reactions, shown, on);
        self.save_changed(chat, item, &it)
    }
}
