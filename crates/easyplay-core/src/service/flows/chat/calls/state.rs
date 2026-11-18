//! What a call keeps, what its timers and media send back, and the call card made from it
//! (ADR-0035).
use crate::chat::Request;
use crate::chat::media::{
    AudioSettings, CallKey, CallMedia, CallRequest, MediaConfig, MediaEvent, MediaSink,
    Negotiation, TrackKind, ice_servers,
};
use crate::clock::Timestamp;
use crate::model::{Call, CallLevels, CallLink, CallQuality, Ringing};
use crate::service::flows::chat::ChatDone;
use crate::service::prelude::*;

/// A friend's level above this counts as speaking.
const SPEAKING: u8 = 12;

/// Where a call is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// A friend calls the user.
    RingingIn,
    /// The user calls a friend.
    RingingOut,
    /// Answered; the sound is being set up.
    Connecting,
    /// Sound gets through (or got through and is being restored).
    Live,
}

/// The call (one at a time).
pub(crate) struct CallFlow {
    /// Numbers calls: timers and media of an earlier call are ignored.
    pub(super) number: u64,
    pub(super) friend: i64,
    pub(super) name: String,
    pub(super) phase: Phase,
    /// The user answered a friend's call, so this side writes the offer.
    pub(super) offerer: bool,
    pub(super) media: Option<Box<dyn CallMedia>>,
    /// Ringing since.
    pub(super) rang: Timestamp,
    /// Sound got through since.
    pub(super) since: Option<Timestamp>,
    pub(super) muted: bool,
    pub(super) deafened: bool,
    pub(super) link: CallLink,
    pub(super) levels: CallLevels,
    pub(super) quality: Option<CallQuality>,
    /// The engine's key for the media's frame encryption (from the invitation or the offer).
    pub(super) key: Option<CallKey>,
}

/// What a call's timers and media send back.
pub(crate) enum CallDone {
    Media {
        number: u64,
        event: MediaEvent,
    },
    /// The sound devices were listed.
    Devices(Result<crate::model::AudioDevices, Error>),
    /// The microphone test `number` reported.
    MicTest {
        number: u64,
        event: crate::chat::media::MicTestEvent,
    },
    /// The ring or connect timeout of call `number`, set in `phase`.
    Timeout {
        number: u64,
        phase: Phase,
    },
}

impl AppService {
    /// A new call with `friend` (replacing none: callers check first).
    pub(super) fn call_new(&mut self, friend: i64, phase: Phase) -> u64 {
        // A call takes the microphone and the speakers from the test.
        if self.mic_test_stop() {
            self.chat_publish();
        }
        let st = &mut self.flow.chat;
        st.next_call += 1;
        let number = st.next_call;
        let name = st
            .snapshot
            .chat_name(ChatRef::Direct(friend))
            .unwrap_or_default()
            .to_owned();
        st.call = Some(CallFlow {
            number,
            friend,
            name,
            phase,
            offerer: false,
            media: None,
            rang: self.deps.clock.now(),
            since: None,
            muted: false,
            deafened: false,
            link: CallLink::Ringing,
            levels: CallLevels::default(),
            quality: None,
            key: None,
        });
        number
    }

    /// The call with `friend`, if that is the call.
    pub(super) fn call_with(&mut self, friend: i64) -> Option<&mut CallFlow> {
        self.flow.chat.call.as_mut().filter(|c| c.friend == friend)
    }

    /// End the call here (its media stops) and say so to the engine with `request`.
    pub(super) fn call_close(&mut self, request: Option<fn(i64) -> CallRequest>) {
        let Some(c) = self.flow.chat.call.take() else {
            return;
        };
        drop(c.media);
        if let Some(r) = request {
            self.chat_request(Request::Call(r(c.friend)));
        }
        self.call_publish();
    }

    /// Chat stopped, or its engine went away: the call is gone without a word to anyone.
    pub(crate) fn call_reset(&mut self) {
        self.mic_test_stop();
        self.flow.chat.call = None;
        self.flow.chat.snapshot.live.call = None;
        self.flow.chat.snapshot.live.ringing = None;
    }

    /// Start the call's media; its events come back tagged with the call's number.
    pub(super) fn call_media(
        &mut self,
        number: u64,
        negotiation: Negotiation,
    ) -> Result<Box<dyn CallMedia>> {
        let Some(factory) = self.deps.media.clone() else {
            bail!(ChtMediaFailed, detail = "this build has no call media");
        };
        let key = self
            .flow
            .chat
            .call
            .as_ref()
            .filter(|c| c.number == number)
            .and_then(|c| c.key);
        let p = &self.prefs.chat;
        let config = MediaConfig {
            negotiation,
            tracks: vec![TrackKind::Audio],
            ice: ice_servers(&p.ice_servers),
            relay_only: p.relay_calls,
            audio: self.audio_settings(),
            key,
        };
        let post = self.poster();
        let sink: MediaSink = Arc::new(move |event| {
            post(Internal::Flow(Box::new(FlowDone::Chat(ChatDone::Call(
                CallDone::Media { number, event },
            )))));
        });
        factory.start(config, sink)
    }

    /// `chat.mic`, `chat.speakers`, `chat.noise_suppression`, `chat.mic_volume`.
    pub(super) fn audio_settings(&self) -> AudioSettings {
        let p = &self.prefs.chat;
        AudioSettings {
            mic: p.mic.clone(),
            speakers: p.speakers.clone(),
            noise_suppression: p.noise_suppression,
            mic_volume: p.mic_volume,
        }
    }

    /// After `wait`, check whether call `number` is still in `phase`.
    pub(super) fn call_timer(&self, number: u64, phase: Phase, wait: Duration) {
        let wait = self.deps.media.as_ref().map_or(wait, |m| m.wait(wait));
        let post = self.poster();
        let spawned = std::thread::Builder::new()
            .name("call-timer".into())
            .spawn(move || {
                std::thread::sleep(wait);
                post(Internal::Flow(Box::new(FlowDone::Chat(ChatDone::Call(
                    CallDone::Timeout { number, phase },
                )))));
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "call: timer not started");
        }
    }

    /// The call card and the incoming card, from the call.
    pub(super) fn call_publish(&mut self) {
        let live = &mut self.flow.chat.snapshot.live;
        live.call = None;
        live.ringing = None;
        live.quality = None;
        if let Some(c) = &self.flow.chat.call {
            let chat = ChatRef::Direct(c.friend);
            if c.phase == Phase::RingingIn {
                live.ringing = Some(Ringing {
                    chat,
                    name: c.name.clone(),
                    since: c.rang,
                });
            } else {
                let speaking = c.levels.friend >= SPEAKING && !c.deafened;
                live.call = Some(Call {
                    chat,
                    since: c.since,
                    with: vec![c.name.clone()],
                    speaking: if speaking {
                        vec![c.name.clone()]
                    } else {
                        Vec::new()
                    },
                    muted: c.muted,
                    deafened: c.deafened,
                });
            }
            live.link = c.link;
            live.levels = c.levels;
            live.quality = c.quality;
        }
        self.chat_publish();
    }

    pub(crate) fn on_call_done(&mut self, done: CallDone) -> Result<()> {
        match done {
            CallDone::Media { number, event } => self.on_call_media(number, event),
            CallDone::Timeout { number, phase } => self.on_call_timeout(number, phase),
            CallDone::Devices(r) => self.on_audio_devices(r),
            CallDone::MicTest { number, event } => self.on_mic_test(number, event),
        }
    }
}
