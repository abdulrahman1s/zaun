//! Voice calls (ADR-0035): the seam between the call flow and the media of a call, the
//! signals the chat engine carries, and a fake for tests. The ICE servers are a value type
//! in `model` (`config` checks the setting with it). Game streams (ADR-0036) add their own
//! seam, signals, the frame slot the page draws from, and a test picture.

pub mod call_key;
pub mod fake;
pub mod fake_stream;
pub mod frame;
pub mod pattern;
pub mod seam;
pub mod signal;
pub mod stream;
pub mod stream_signal;

pub use crate::model::{DEFAULT_ICE_SERVERS, IceKind, IceServer, IceTransport, ice_servers};
pub use call_key::CallKey;
pub use frame::{FrameSlot, VideoFrame};
pub use seam::{
    AudioSettings, CONNECT_TIMEOUT, CallMedia, MediaConfig, MediaEvent, MediaFactory, MediaSink,
    MicTest, MicTestEvent, MicTestSink, Negotiation, RING_GRACE, RING_TIMEOUT, TrackKind,
};
pub use signal::{CallRequest, CallSignal};
pub use stream::{ShareConfig, StreamEvent, StreamFactory, StreamMedia, StreamSink, WatchConfig};
pub use stream_signal::{StreamRequest, StreamSignal};
