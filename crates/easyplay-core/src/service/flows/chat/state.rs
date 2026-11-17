//! What Chat keeps between messages and what its jobs send back (ADR-0033).
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use crate::chat::{ChatEvent, Engine, Request};
use crate::error::Error;
use crate::ids::QueryId;
use crate::model::ChatSnapshot;

/// Exits in a row (without a successful start between) before Chat gives up.
pub const MAX_FAILURES: u32 = 5;
pub const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Messages per page of a conversation.
pub const PAGE: u32 = 50;
/// The user's avatar pair, kept on this computer (`<data>/chat/paper`): the engine's profile
/// has no place for it.
pub const PAPER_FILE: &str = "paper";
/// Sent pictures are copied here inside the Chat folder (the engine's sandbox sees no other).
pub const OUTBOX_DIR: &str = "files/sent";

/// Completions that re-enter the loop.
pub(crate) enum ChatDone {
    /// The engine of `generation` reported.
    Event { generation: u64, event: ChatEvent },
    /// A copy for `ChatCommand::SaveFile` finished: the folder's name, or why not.
    Saved(Result<String, Error>),
    /// The wait before restarting the engine of `generation` is over.
    Restart { generation: u64 },
    /// A file to send was copied into the Chat folder (and its preview made).
    Prepared {
        generation: u64,
        result: Result<Request, Error>,
    },
}

#[derive(Default)]
pub(crate) struct ChatState {
    pub(super) engine: Option<Box<dyn Engine>>,
    /// Number of the engine start; events of an older one are dropped.
    pub(super) generation: u64,
    /// What the engine was started with: (name, relays).
    pub(super) started_with: Option<(String, Vec<String>)>,
    pub(super) failures: u32,
    pub(super) snapshot: ChatSnapshot,
    pub(super) next_tag: u64,
    /// Pages asked for, by tag; the engine answers in order.
    pub(super) pages: BTreeMap<u64, QueryId>,
    /// Numbers for copies of sent files.
    pub(super) next_file: u64,
    /// The user's avatar pair (`PAPER_FILE`).
    pub(super) paper: Option<u8>,
    /// Copies asked for with `ChatCommand::SaveFile`, by the tag of their
    /// `Request::LocateFile`: where each copy goes.
    pub(super) saves: BTreeMap<u64, PathBuf>,
    /// Numbers for the temporary files of copies (two saves never share one).
    pub(super) next_save: u64,
}

/// The wait before restart number `failures` (1 → 1 s, 2 → 2 s, … at most [`MAX_BACKOFF`]).
pub fn backoff(failures: u32) -> Duration {
    let secs = 1u64 << failures.saturating_sub(1).min(6);
    Duration::from_secs(secs).min(MAX_BACKOFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_wait_longer_each_time_up_to_a_minute() {
        let waits: Vec<u64> = (1..=9).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(waits, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
    }
}
