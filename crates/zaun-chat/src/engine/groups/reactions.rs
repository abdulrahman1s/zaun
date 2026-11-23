//! Members' reactions in a group, counted per member (Subscriber.hs `groupMsgReaction`, `reactionAllowed`; Store/Messages.hs `getGroupReactions`).

use zaun_chat_wire::model::{ChatRef, Reaction};

use super::inbound::GroupCtx;
use super::policy::{Feature, on};
use crate::chat::ids::MemberId;
use crate::chat::reaction::MsgReaction;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::{ItemRecord, NS, decode, keys};
use crate::engine::requests::has_content;

/// Internal.hs `maxMsgReactions`.
const MAX_REACTIONS: usize = 3;

/// The page's form of an emoji: ❤ travels without its variation selector.
fn display(c: char) -> String {
    if c == '\u{2764}' {
        "\u{2764}\u{fe0f}".to_owned()
    } else {
        c.to_string()
    }
}

/// One more or one less of `emoji` in the item's counts; the user's own part stays.
fn count(reactions: &mut Vec<Reaction>, emoji: String, add: bool) {
    let i = reactions.iter().position(|r| r.emoji == emoji);
    match (i, add) {
        (Some(i), true) => reactions[i].count += 1,
        (None, true) => reactions.push(Reaction {
            emoji,
            count: 1,
            mine: false,
            who: Vec::new(),
        }),
        (Some(i), false) => {
            reactions[i].count = reactions[i].count.saturating_sub(1);
            if reactions[i].count == 0 {
                reactions.remove(i);
            }
        }
        (None, false) => {}
    }
}

impl<A: AgentPort> Engine<A> {
    pub(crate) fn forget_group_reactions(&mut self, chat: ChatRef, item: i64) -> Result<()> {
        let ChatRef::Group(g) = chat else {
            return Ok(());
        };
        for key in self
            .store
            .keys_with_prefix(NS, &keys::group_reaction_item_prefix(g, item))?
        {
            self.store.delete(NS, &key)?;
        }
        Ok(())
    }

    /// The item a group event names by its author's member id: the user's own, or a member's.
    pub(crate) fn group_item_by(
        &mut self,
        group: i64,
        author: &MemberId,
        shared: &[u8],
    ) -> Result<Option<(i64, ItemRecord)>> {
        let mine = self.group(group)?.my_member_id == *author;
        let member = match self.member_by_id(group, author) {
            _ if mine => None,
            Some(m) => Some(m.id),
            None => return Ok(None),
        };
        Ok(self.item_by_shared(ChatRef::Group(group), member, shared)?)
    }

    /// A reaction the group's preferences allow, within the member's three, on an item with
    /// content. One that is not allowed is dropped (and not forwarded).
    pub(crate) fn on_group_react(
        &mut self,
        ctx: GroupCtx,
        shared: &[u8],
        item_author: Option<&MemberId>,
        reaction: &MsgReaction,
        add: bool,
    ) -> Result<()> {
        let MsgReaction::Emoji(c) = reaction else {
            return Ok(());
        };
        if !on(&self.group(ctx.group)?, Feature::Reactions) {
            return Err(fail(Code::RequestFailed, "reactions are off in the group"));
        }
        let found = match item_author {
            Some(a) => self.group_item_by(ctx.group, a, shared)?,
            None => None,
        };
        let Some((id, mut item)) = found.filter(|(_, i)| has_content(&i.body)) else {
            return Ok(());
        };
        let key = keys::group_reaction(ctx.group, id, ctx.author);
        let mut theirs: Vec<String> = match self.store.get(NS, &key)? {
            Some(b) => decode(&b)?,
            None => Vec::new(),
        };
        let emoji = display(*c);
        let had = theirs.contains(&emoji);
        if had == add || (add && theirs.len() >= MAX_REACTIONS) {
            return Err(fail(Code::RequestFailed, "reaction not allowed"));
        }
        if add {
            theirs.push(emoji.clone());
        } else {
            theirs.retain(|e| *e != emoji);
        }
        self.put(&key, &theirs)?;
        count(&mut item.reactions, emoji, add);
        self.save_changed(ChatRef::Group(ctx.group), id, &item)
    }
}
