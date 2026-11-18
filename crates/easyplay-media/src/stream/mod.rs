//! Game streams through the call media (ADR-0036): `RtcMedia` shares a game (capture, H.264,
//! its own peer connection once the friend's app takes the stream) and takes friends'
//! streams in watch-only mode (decoder into the page's frame slot).

mod share;
mod watch;

use easyplay_core::chat::media::{
    MediaConfig, Negotiation, ShareConfig, StreamFactory, StreamMedia, StreamSink, TrackKind,
    WatchConfig,
};
use easyplay_core::error::Result;
use easyplay_core::model::IceServer;

use crate::factory::RtcMedia;

impl StreamFactory for RtcMedia {
    fn share(&self, config: ShareConfig, sink: StreamSink) -> Result<Box<dyn StreamMedia>> {
        share::start(self, config, sink)
    }

    fn watch(&self, config: WatchConfig, sink: StreamSink) -> Result<Box<dyn StreamMedia>> {
        watch::start(self, config, sink)
    }

    fn can_share(&self) -> bool {
        self.capture().available()
    }
}

/// A stream's connection: video only, no sound devices.
fn media_config(negotiation: Negotiation, ice: Vec<IceServer>, relay_only: bool) -> MediaConfig {
    MediaConfig {
        negotiation,
        tracks: vec![TrackKind::Video],
        ice,
        relay_only,
        audio: Default::default(),
        // Streams are EasyPlay's own; the apps' frame encryption is for calls.
        key: None,
    }
}
