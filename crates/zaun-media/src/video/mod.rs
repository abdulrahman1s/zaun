//! Game streams' pictures (ADR-0036): I420 conversions, H.264 on the GPU (feature `hwenc`)
//! or through OpenH264, frame pacing, the sources
//! a share reads from, and the threads that encode a share and decode a watched stream.

pub mod codec;
pub mod encoder;
#[cfg(feature = "hwenc")]
pub mod hw;
pub mod i420;
pub mod pace;
pub mod source;
