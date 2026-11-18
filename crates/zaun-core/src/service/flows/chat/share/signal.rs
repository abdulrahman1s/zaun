//! What the friend's side sends about a stream (ADR-0036). A friend's stream is taken at
//! once in watch-only mode, so the user can watch without waiting; the user's share answers
//! the friend's offer.
use super::state::{SharePhase, TheirStream};
use crate::chat::Request;
use crate::chat::media::{StreamRequest, StreamSignal, WatchConfig, ice_servers};
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn on_stream_signal(&mut self, signal: StreamSignal) -> Result<()> {
        match signal {
            StreamSignal::Invited(friend) => return self.on_stream_invited(friend),
            StreamSignal::Offer {
                friend,
                sdp,
                candidates,
            } => {
                let Some(s) = self.my_share_with(friend) else {
                    tracing::info!(friend, "stream: an offer for no share of ours");
                    return Ok(());
                };
                if s.phase != SharePhase::Offered {
                    return Ok(());
                }
                s.media.offer(sdp);
                if !candidates.is_empty() {
                    s.media.candidates(candidates);
                }
                s.phase = SharePhase::Connecting;
                let number = s.number;
                self.stream_timer(number);
            }
            StreamSignal::Answer {
                friend,
                sdp,
                candidates,
            } => {
                if let Some(t) = self.flow.chat.share.theirs.get(&friend) {
                    t.media.answer(sdp);
                    if !candidates.is_empty() {
                        t.media.candidates(candidates);
                    }
                    let number = t.number;
                    self.stream_timer(number);
                }
            }
            StreamSignal::Extra { friend, candidates } => {
                if let Some(s) = self.my_share_with(friend) {
                    s.media.candidates(candidates);
                } else if let Some(t) = self.flow.chat.share.theirs.get(&friend) {
                    t.media.candidates(candidates);
                }
            }
            StreamSignal::Ended(friend) => {
                if let Some(s) = self.my_share_with(friend) {
                    let live = s.phase == SharePhase::Live;
                    tracing::info!(friend, live, "stream: the friend's app let the stream go");
                    self.share_close(false);
                    // Before it connected: their app does not take streams (sharing off
                    // there, or an older app).
                    ensure!(
                        live,
                        ChtStreamFailed,
                        detail = "the friend's app did not take the stream"
                    );
                }
                self.their_close(friend, false, true);
            }
            StreamSignal::Refused { friend, detail } => {
                if self.my_share_with(friend).is_some() {
                    self.share_close(false);
                    bail!(ChtStreamFailed, detail = detail);
                }
                self.their_close(friend, false, false);
            }
        }
        Ok(())
    }

    /// A friend shares a game: take it in watch-only mode (the media writes the offer).
    fn on_stream_invited(&mut self, friend: i64) -> Result<()> {
        let snap = &self.flow.chat.snapshot;
        let Some(name) = snap
            .friend(friend)
            .filter(|f| !f.blocked && snap.features.share)
            .map(|f| f.name.clone())
        else {
            // Streams are off here: let it go (the friend's share sees nobody take it).
            self.chat_request(Request::Stream(StreamRequest::End(friend)));
            return Ok(());
        };
        // A new invitation replaces the friend's earlier stream (the engine's one call).
        self.their_close(friend, false, false);
        let factory = self.stream_factory()?;
        let number = self.stream_number();
        let p = &self.prefs.chat;
        let config = WatchConfig {
            ice: ice_servers(&p.ice_servers),
            // Taken without the user's say: never with the user's own address, whatever
            // `chat.relay_calls` says.
            relay_only: true,
            frames: self.hub.frames.clone(),
        };
        let media = factory
            .watch(config, self.stream_sink(number))
            .inspect_err(|_| self.chat_request(Request::Stream(StreamRequest::End(friend))))?;
        tracing::info!(friend, "stream: a friend shares a game; taking it");
        self.flow.chat.share.theirs.insert(
            friend,
            TheirStream {
                number,
                name,
                title: None,
                media,
                connected: false,
                paused: false,
            },
        );
        Ok(())
    }

    pub(super) fn my_share_with(&mut self, friend: i64) -> Option<&mut super::state::MyShare> {
        self.flow
            .chat
            .share
            .mine
            .as_mut()
            .filter(|s| s.friend == friend)
    }
}
