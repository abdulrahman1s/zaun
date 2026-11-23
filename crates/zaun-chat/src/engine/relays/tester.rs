//! What the engine asks of the network to test a relay, and how an answer comes back (ADR-0012: never on the engine thread).

use crossbeam_channel::Sender;

use crate::engine::jobs::JobDone;

/// One relay's answer to the check of `run`; an answer of an older run is ignored.
#[derive(Debug)]
pub struct RelayDone {
    pub run: u64,
    pub server: String,
    /// What failed, for the log, or nothing when the relay passed.
    pub failure: Option<String>,
}

/// The network side of a check. Each call returns at once; the answer arrives on `tx`.
pub trait RelayTester: Send {
    fn test(&mut self, server: String, run: u64, tx: Sender<JobDone>);
}
