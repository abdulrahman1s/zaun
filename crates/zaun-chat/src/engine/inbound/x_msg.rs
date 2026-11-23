//! A friend's messages: `x.msg.new` (with its quote), and their edits, deletions and reactions.

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatRef, MessageAuthor, MessageBody, Quote, Reaction};

use crate::chat::container::{MsgContainer, QuotedMsg};
use crate::chat::content::MsgContent;
use crate::chat::reaction::MsgReaction;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::quote::shown_quote;
use crate::engine::records::ItemRecord;
use crate::engine::records::friend::Feature;

/// The page's form of an emoji: ❤ travels without its variation selector.
fn display(c: char) -> String {
    if c == '\u{2764}' {
        "\u{2764}\u{fe0f}".to_owned()
    } else {
        c.to_string()
    }
}

/// The friend's reaction added or taken away; the user's own part of the count stays.
fn react(reactions: &mut Vec<Reaction>, emoji: String, add: bool) {
    let i = reactions.iter().position(|r| r.emoji == emoji);
    let theirs = i.is_some_and(|i| reactions[i].count > u32::from(reactions[i].mine));
    match (i, add) {
        (Some(i), true) if !theirs => reactions[i].count += 1,
        (None, true) => reactions.push(Reaction {
            emoji,
            count: 1,
            mine: false,
            who: Vec::new(),
        }),
        (Some(i), false) if theirs => {
            reactions[i].count -= 1;
            if reactions[i].count == 0 {
                reactions.remove(i);
            }
        }
        (Some(_) | None, true | false) => {}
    }
}

impl<A: AgentPort> Engine<A> {
    /// A blocked friend's message is deleted on arrival: never kept, never shown.
    pub(crate) fn on_msg_new(
        &mut self,
        friend: i64,
        shared: Option<Vec<u8>>,
        c: MsgContainer,
    ) -> Result<()> {
        let mut f = self.friend(friend)?;
        if f.blocked {
            return Ok(());
        }
        // A message not acked before a restart comes again; it is kept once.
        if let Some(s) = &shared
            && self
                .item_by_shared(ChatRef::Direct(friend), None, s)?
                .is_some()
        {
            return Ok(());
        }
        let size = c.file.as_ref().map_or(0, |i| i.file_size);
        let offer = c
            .file
            .as_ref()
            .map(|inv| crate::engine::files::offer(&c.content, inv))
            .transpose()?;
        let body = offer.as_ref().map_or_else(
            || MessageBody::Text(c.content.text().to_owned()),
            |(b, _)| b.clone(),
        );
        let mut item = ItemRecord::new(MessageAuthor::Friend, body, self.clock.wall_ms());
        item.shared_id = shared;
        item.reply = c
            .quote
            .map(|q| self.incoming_quote(friend, &q))
            .transpose()?;
        let m = self.add_item(ChatRef::Direct(friend), &item)?;
        f.unread += 1;
        self.save_friend(f)?;
        let id = m.id;
        self.emit(Event::Messages(vec![m]));
        if let Some((_, rec)) = offer {
            self.on_offered(friend, id, rec, size)?;
        }
        Ok(())
    }

    /// `sent` is from the sender's side: true means the friend wrote the quoted message.
    fn incoming_quote(&mut self, friend: i64, q: &QuotedMsg) -> Result<Quote> {
        let id = match &q.msg_ref.msg_id {
            Some(shared) => self
                .item_by_shared(ChatRef::Direct(friend), None, &shared.0)?
                .map(|(id, _)| id),
            None => None,
        };
        let author = if q.msg_ref.sent {
            MessageAuthor::Friend
        } else {
            MessageAuthor::Me
        };
        Ok(shown_quote(id, author, q.content.text()))
    }

    /// Their own message, changed by `f`; anything else is left alone.
    fn change_theirs(
        &mut self,
        friend: i64,
        shared: &[u8],
        f: impl FnOnce(&mut ItemRecord),
    ) -> Result<()> {
        let Some((id, mut item)) = self.item_by_shared(ChatRef::Direct(friend), None, shared)?
        else {
            return Ok(());
        };
        if item.author != MessageAuthor::Friend || item.body == MessageBody::Deleted {
            return Ok(());
        }
        f(&mut item);
        let m = self.change_item(ChatRef::Direct(friend), id, &item)?;
        self.emit(Event::Updated(vec![m]));
        Ok(())
    }

    pub(crate) fn on_msg_update(
        &mut self,
        friend: i64,
        shared: &[u8],
        content: &MsgContent,
    ) -> Result<()> {
        let text = content.text().to_owned();
        self.change_theirs(friend, shared, |i| {
            crate::engine::files::edit_caption(&mut i.body, text);
            i.edited = true;
        })
    }

    /// Subscriber.hs `messageDelete`: gone for good when the preferences allow their full delete.
    pub(crate) fn on_msg_del(&mut self, friend: i64, shared: &[u8]) -> Result<()> {
        let Some((id, item)) = self.item_by_shared(ChatRef::Direct(friend), None, shared)? else {
            return Ok(());
        };
        if item.author != MessageAuthor::Friend || item.body == MessageBody::Deleted {
            return Ok(());
        }
        let full = self
            .friend(friend)?
            .enabled(Feature::FullDelete)
            .for_contact;
        self.finish_delete(friend, id, item, full)
    }

    pub(crate) fn on_msg_react(
        &mut self,
        friend: i64,
        shared: &[u8],
        reaction: &MsgReaction,
        add: bool,
    ) -> Result<()> {
        let emoji = match reaction {
            MsgReaction::Emoji(c) => display(*c),
            MsgReaction::Unknown(_) => return Ok(()),
        };
        if !self.friend(friend)?.enabled(Feature::Reactions).for_contact {
            return Ok(());
        }
        let Some((id, mut item)) = self.item_by_shared(ChatRef::Direct(friend), None, shared)?
        else {
            return Ok(());
        };
        if item.body == MessageBody::Deleted {
            return Ok(());
        }
        react(&mut item.reactions, emoji, add);
        let m = self.change_item(ChatRef::Direct(friend), id, &item)?;
        self.emit(Event::Updated(vec![m]));
        Ok(())
    }
}
