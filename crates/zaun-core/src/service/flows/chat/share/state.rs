//! What sharing and watching keep, what their media and timers send back, and the snapshot
//! made from them (ADR-0036).
use std::collections::BTreeMap;

use crate::chat::Request;
use crate::chat::media::{StreamEvent, StreamFactory, StreamMedia, StreamRequest, StreamSink};
use crate::clock::Timestamp;
use crate::model::{Playing, Share, Watching};
use crate::service::flows::chat::ChatDone;
use crate::service::prelude::*;

/// Answered but not connected this long, a stream failed.
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Where the user's share is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SharePhase {
    /// The portal asks the user for the game's window.
    Choosing,
    /// The friend was offered the stream; their app has not taken it yet.
    Offered,
    /// Their offer arrived and was answered; connecting.
    Connecting,
    Live,
}

/// The game the user shares (one at a time).
pub(crate) struct MyShare {
    pub(super) number: u64,
    pub(super) friend: i64,
    pub(super) game: GameId,
    pub(super) title: String,
    pub(super) media: Box<dyn StreamMedia>,
    pub(super) phase: SharePhase,
    pub(super) since: Timestamp,
    /// The friend watches now.
    pub(super) watched: bool,
    pub(super) paused: bool,
    /// What is sent now (the media says).
    pub(super) sending: Option<crate::model::StreamInfo>,
}

/// A friend's stream this app took (watch-only), watched or not.
pub(crate) struct TheirStream {
    pub(super) number: u64,
    pub(super) name: String,
    /// The game, once their app said it.
    pub(super) title: Option<String>,
    pub(super) media: Box<dyn StreamMedia>,
    pub(super) connected: bool,
    pub(super) paused: bool,
}

#[derive(Default)]
pub(crate) struct ShareState {
    /// Numbers streams: late events of an earlier one are ignored.
    pub(super) next: u64,
    pub(super) mine: Option<MyShare>,
    /// Friends' streams by friend. Open items only: a stream leaves when it ends.
    pub(super) theirs: BTreeMap<i64, TheirStream>,
    /// The friend whose stream the user watches (one at a time).
    pub(super) watched: Option<i64>,
    pub(super) muted: bool,
}

/// What streams' media and timers send back.
pub(crate) enum ShareDone {
    Media {
        number: u64,
        event: StreamEvent,
    },
    /// Stream `number` should be connected by now.
    Timeout {
        number: u64,
    },
}

impl AppService {
    pub(super) fn stream_factory(&self) -> Result<Arc<dyn StreamFactory>> {
        self.deps
            .media
            .as_ref()
            .and_then(|m| m.streams())
            .ok_or_else(|| err!(ChtCaptureFailed, detail = "this build cannot stream"))
    }

    pub(super) fn stream_number(&mut self) -> u64 {
        let st = &mut self.flow.chat.share;
        st.next += 1;
        st.next
    }

    /// Where stream `number`'s events go.
    pub(super) fn stream_sink(&self, number: u64) -> StreamSink {
        let post = self.poster();
        Arc::new(move |event| {
            post(Internal::Flow(Box::new(FlowDone::Chat(ChatDone::Stream(
                ShareDone::Media { number, event },
            )))));
        })
    }

    /// After the connect timeout, check that stream `number` connected.
    pub(super) fn stream_timer(&self, number: u64) {
        let wait = self
            .deps
            .media
            .as_ref()
            .map_or(CONNECT_TIMEOUT, |m| m.wait(CONNECT_TIMEOUT));
        let post = self.poster();
        let spawned = std::thread::Builder::new()
            .name("stream-timer".into())
            .spawn(move || {
                std::thread::sleep(wait);
                post(Internal::Flow(Box::new(FlowDone::Chat(ChatDone::Stream(
                    ShareDone::Timeout { number },
                )))));
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "stream: timer not started");
        }
    }

    /// Stop the user's share and tell the friend's app (`tell`).
    pub(super) fn share_close(&mut self, tell: bool) {
        let Some(s) = self.flow.chat.share.mine.take() else {
            return;
        };
        drop(s.media);
        if tell && s.phase != SharePhase::Choosing {
            self.chat_request(Request::Stream(StreamRequest::End(s.friend)));
        }
        self.stream_publish();
    }

    /// Let a friend's stream go and tell their app (`tell`); `ended`: they stopped it, and
    /// the stage says so if it was watched.
    pub(super) fn their_close(&mut self, friend: i64, tell: bool, ended: bool) {
        let st = &mut self.flow.chat.share;
        let Some(t) = st.theirs.remove(&friend) else {
            return;
        };
        drop(t.media);
        if st.watched == Some(friend) {
            st.watched = None;
            self.hub.frames.clear();
            self.flow.chat.snapshot.live.stream.ended = ended.then_some(ChatRef::Direct(friend));
        }
        if tell {
            self.chat_request(Request::Stream(StreamRequest::End(friend)));
        }
        self.stream_publish();
    }

    /// The share card, the stage and friends' "live" marks, from the streams.
    pub(super) fn stream_publish(&mut self) {
        let st = &self.flow.chat.share;
        let snap = &mut self.flow.chat.snapshot;
        snap.live.share = st
            .mine
            .as_ref()
            .filter(|s| s.phase != SharePhase::Choosing)
            .map(|s| Share {
                chat: ChatRef::Direct(s.friend),
                title: s.title.clone(),
                since: s.since,
                watching: u32::from(s.watched),
            });
        snap.live.stream.share_paused = st.mine.as_ref().is_some_and(|s| s.paused);
        snap.live.stream.sending = st.mine.as_ref().and_then(|s| s.sending);
        let p = &self.prefs.chat;
        snap.live.stream.share_height = super::start::share_height(p.share_quality);
        snap.live.stream.share_fps = super::start::share_fps(p.share_fps);
        let watched = st.watched.and_then(|f| st.theirs.get(&f).map(|t| (f, t)));
        snap.live.watch = watched.map(|(f, t)| Watching {
            chat: ChatRef::Direct(f),
            name: t.name.clone(),
            title: t.title.clone().unwrap_or_default(),
            // 1:1 streams: the user is the one watching.
            watching: 1,
            muted: st.muted,
        });
        snap.live.stream.watch_paused = watched.is_some_and(|(_, t)| t.paused);
        snap.live.stream.watch_waiting = watched.is_some() && !self.hub.frames.has_frame();
        self.stream_mark_friends();
        self.chat_publish();
    }

    /// A friend who shares a game with the user is "playing it, live" in the snapshot (the
    /// engine knows nothing of it; the list it reports replaces the friends).
    pub(crate) fn stream_mark_friends(&mut self) {
        let st = &self.flow.chat.share;
        for f in &mut self.flow.chat.snapshot.friends {
            let title = st
                .theirs
                .get(&f.id)
                .filter(|t| t.connected)
                .and_then(|t| t.title.clone());
            match title {
                Some(title) => {
                    f.playing = Some(Playing {
                        title,
                        minutes: 0,
                        live: true,
                    });
                }
                None if f.playing.as_ref().is_some_and(|p| p.live) => f.playing = None,
                None => {}
            }
        }
    }

    pub(crate) fn on_share_done(&mut self, done: ShareDone) -> Result<()> {
        match done {
            ShareDone::Media { number, event } => self.on_stream_media(number, event),
            ShareDone::Timeout { number } => self.on_stream_timeout(number),
        }
    }
}
