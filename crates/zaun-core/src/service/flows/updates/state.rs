//! What the update flows keep between messages, and what their jobs send back.

use crate::update::{Release, UpdateError, UpdateState};

/// Completions and timers of the update flows.
pub(crate) enum UpdatesDone {
    /// The daily timer of `generation` ran out (stale generations are ignored).
    Due { generation: u64 },
    /// A check finished; `manual` = "Check now".
    Checked {
        manual: bool,
        result: Result<Box<Release>, UpdateError>,
    },
    /// A download finished: the staged version.
    Downloaded(Result<String, UpdateError>),
}

#[derive(Default)]
pub(crate) struct UpdatesState {
    /// Bumped whenever the schedule changes, so older timers do nothing.
    pub generation: u64,
    /// `updates.check` as last seen.
    pub enabled: bool,
    pub checking: bool,
    pub downloading: bool,
    /// The verified version waiting in `.update/staged`.
    pub staged: Option<String>,
    pub failure: Option<crate::error::ErrorCode>,
    /// `<state>/update.json` as last read or written.
    pub file: UpdateState,
}
