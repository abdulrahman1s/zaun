//! The call's line in the conversation: one item when the call is over (Internal.hs `callStatusItemContent`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{
    CallKind, CallRecord, ChatMessage, ChatRef, Delivery, MessageAuthor, MessageBody,
};

use super::state::{Call, Kind, Phase};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::ItemRecord;

/// How a call is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Over {
    /// Hung up, by either side, or let go.
    Ended,
    /// Refused while it rang: the callee's line is read, the caller's reads declined too.
    Declined,
}

fn record(call: &Call, over: Over, now: i64) -> CallRecord {
    let kind = match (over, call.phase) {
        (Over::Declined, _) => CallKind::Declined,
        (Over::Ended, Phase::InviteSent | Phase::InviteReceived) => CallKind::Missed,
        (Over::Ended, Phase::OfferSent | Phase::OfferReceived | Phase::Negotiated) => {
            CallKind::Ended
        }
    };
    let secs = call.connected.map_or(0, |c| (now - c).max(0) / 1000);
    let minutes = u32::try_from((secs + 59) / 60).unwrap_or(u32::MAX);
    CallRecord { kind, minutes }
}

impl<A: AgentPort> Engine<A> {
    /// Writes the line of a call that is over. A stream has none (ADR-0036); a failure to save
    /// is logged, as the call is over either way.
    pub(super) fn close_call(&mut self, friend: i64, call: &Call, over: Over) {
        if call.kind == Kind::Stream {
            return;
        }
        if let Err(e) = self.add_call_line(friend, call, over) {
            self.log
                .line(format_args!("a call line was not saved: {}", e.detail));
        }
    }

    fn add_call_line(&mut self, friend: i64, call: &Call, over: Over) -> Result<()> {
        let now = self.clock.wall_ms();
        let rec = record(call, over, now);
        let author = if call.outgoing {
            MessageAuthor::Me
        } else {
            MessageAuthor::Friend
        };
        let mut item = ItemRecord::new(author.clone(), MessageBody::Call(rec), now);
        if call.outgoing {
            item.delivery = Delivery::Sent;
        }
        let mut m = self.add_item(ChatRef::Direct(friend), &item)?;
        if author == MessageAuthor::Friend {
            self.count_call_line(friend, &mut m, rec.kind != CallKind::Missed)?;
        }
        self.state.chats_dirty = true;
        self.emit(Event::Messages(vec![m]));
        Ok(())
    }

    /// A line of theirs is unread only when missed. The read mark is a position, so a read line
    /// moves it only when nothing unread sits before it; otherwise it counts as unread.
    fn count_call_line(&mut self, friend: i64, m: &mut ChatMessage, read: bool) -> Result<()> {
        let mut f = self.friend(friend)?;
        if read && f.unread == 0 {
            f.read_up_to = m.id;
            m.unread = false;
            self.state.last.insert(ChatRef::Direct(friend), m.clone());
        } else {
            f.unread += 1;
        }
        self.save_friend(f)?;
        Ok(())
    }
}
