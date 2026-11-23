//! React: the user adds or takes back a reaction (Commands.hs `APIChatItemReaction`).

use zaun_chat_wire::model::{ChatRef, Reaction};

use crate::chat::ChatEvent;
use crate::chat::ids::SharedMsgId;
use crate::chat::reaction::{MsgReaction, REACTION_EMOJI};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
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
    pub(crate) fn react(&mut self, chat: ChatRef, item: i64, emoji: &str, on: bool) -> Result<()> {
        let friend = Self::direct_friend(chat)?;
        if !self.friend(friend)?.enabled(Feature::Reactions).for_user {
            return Err(fail(Code::RequestFailed, "feature not allowed: reactions"));
        }
        let c = wire_emoji(emoji).ok_or_else(|| {
            fail(
                Code::RequestFailed,
                format!("not a reaction the engine carries: {emoji}"),
            )
        })?;
        let mut it = self
            .item(friend, item)?
            .ok_or_else(|| fail(Code::RequestFailed, "no such item"))?;
        let shown = c.to_string().replace('\u{2764}', "\u{2764}\u{fe0f}");
        let mine: Vec<&Reaction> = it.reactions.iter().filter(|r| r.mine).collect();
        let had = mine.iter().any(|r| r.emoji == shown);
        let full = on && mine.len() >= MAX_REACTIONS;
        let shared = match it.shared_id.clone() {
            Some(s) if has_content(&it.body) && had != on && !full => s,
            Some(_) | None => return Err(fail(Code::RequestFailed, "reaction not allowed")),
        };
        self.send_event(
            friend,
            ChatEvent::MsgReact {
                msg_id: SharedMsgId(shared),
                member_id: None,
                scope: None,
                reaction: MsgReaction::Emoji(c),
                add: on,
            },
        )?;
        react_mine(&mut it.reactions, shown, on);
        self.save_changed(friend, item, &it)
    }
}
