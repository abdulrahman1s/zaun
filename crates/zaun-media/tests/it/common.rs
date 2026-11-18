//! What the media tests share: start a call's media and wait for its events.

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zaun_core::chat::media::{
    AudioSettings, CallMedia, IceServer, MediaConfig, MediaEvent, MediaFactory, MediaSink,
    Negotiation, TrackKind,
};
use zaun_media::RtcMedia;

pub struct Side {
    pub media: Box<dyn CallMedia>,
    pub events: Receiver<MediaEvent>,
}

pub fn start(
    f: &RtcMedia,
    negotiation: Negotiation,
    relay_only: bool,
    ice: Vec<IceServer>,
) -> Side {
    start_full(f, negotiation, relay_only, ice, false)
}

/// Direct, without ICE servers, with or without noise suppression.
pub fn start_with(f: &RtcMedia, negotiation: Negotiation, noise_suppression: bool) -> Side {
    start_full(f, negotiation, false, Vec::new(), noise_suppression)
}

fn start_full(
    f: &RtcMedia,
    negotiation: Negotiation,
    relay_only: bool,
    ice: Vec<IceServer>,
    noise_suppression: bool,
) -> Side {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let sink: MediaSink = Arc::new(move |e| {
        let _ = tx.lock().unwrap().send(e);
    });
    let config = MediaConfig {
        negotiation,
        tracks: vec![TrackKind::Audio],
        ice,
        relay_only,
        audio: AudioSettings {
            noise_suppression,
            ..AudioSettings::default()
        },
        key: None,
    };
    Side {
        media: f.start(config, sink).unwrap(),
        events: rx,
    }
}

pub fn wait<T>(s: &Side, what: &str, mut f: impl FnMut(&MediaEvent) -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let left = end.saturating_duration_since(Instant::now());
        match s.events.recv_timeout(left) {
            Ok(MediaEvent::Failed { code, detail }) => panic!("{what}: failed {code:?}: {detail}"),
            Ok(e) => {
                if let Some(t) = f(&e) {
                    return t;
                }
            }
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
}

pub fn description(s: &Side) -> String {
    wait(s, "a description", |e| match e {
        MediaEvent::Description(d) => Some(d.clone()),
        _ => None,
    })
}

pub fn connected(s: &Side) {
    wait(s, "connected", |e| {
        (*e == MediaEvent::Connected).then_some(())
    });
}
