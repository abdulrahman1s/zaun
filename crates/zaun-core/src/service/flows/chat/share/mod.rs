//! Game streams (ADR-0036): the user shares one game's window with a friend, or watches a
//! friend's. Each stream is its own connection, signalled through the engine as a video
//! call and carried by `ServiceDeps::media`'s streams; one share and one watched stream at
//! a time, 1:1 only.

mod media;
mod quality;
mod signal;
mod start;
mod state;
mod stop;
mod watch;

pub(crate) use state::{ShareDone, ShareState};
