//! What the overlay flow keeps between messages and what its threads send back (ADR-0037).
use std::collections::VecDeque;

use crate::chat::overlay::snapshot::Toast;
use crate::chat::overlay::{Overlay, OverlayEvent, OverlaySnapshot};
use crate::clock::MonoInstant;
use crate::model::{ChatMessage, ChatRef};

/// Overlay pages remembered while they are on their way.
pub const PAGES_IN_FLIGHT: usize = 8;
/// At most one snapshot per this long; changes in between are sent together after it.
pub const PUSH_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

/// Completions that re-enter the loop.
pub(crate) enum OverlayDone {
    /// The overlay of `generation` reported.
    Event {
        generation: u64,
        event: OverlayEvent,
    },
    /// The wait before restarting the overlay of `generation` is over.
    Restart { generation: u64 },
    /// The throttle's wait is over: send what changed.
    Flush { generation: u64 },
    /// The desktop was asked whether it has a layer shell (`None` = it could not tell).
    LayerShell(Option<bool>),
}

#[derive(Default)]
pub(crate) struct OverlayState {
    pub(super) overlay: Option<Box<dyn Overlay>>,
    /// Number of the overlay start; events of an older one are dropped.
    pub(super) generation: u64,
    /// Exits in a row without the overlay saying it was ready.
    pub(super) failures: u32,
    /// Not started again until the game ends or the setting changes: this desktop cannot
    /// show it, it speaks another protocol, or it kept failing.
    pub(super) given_up: bool,
    /// This desktop cannot show it (no layer shell, no GPU path): kept across games until
    /// `chat.overlay` is turned off.
    pub(super) cannot: Option<crate::chat::overlay::OverlayStatus>,
    /// The desktop was asked once at start whether it has a layer shell (before any game,
    /// so Settings can dim its rows on GNOME); `Some(false)` = it cannot show an overlay.
    pub(super) asked: bool,
    pub(super) layer_shell: Option<bool>,
    /// A restart is waiting for its back-off.
    pub(super) restarting: bool,
    /// The panel is open (as last told to, or as the overlay said).
    pub(super) open: bool,
    /// The conversation the panel shows.
    pub(super) chat: Option<ChatRef>,
    /// Its latest messages, oldest first; at most `snapshot::MAX_MESSAGES` (older ones are
    /// dropped as new ones arrive).
    pub(super) messages: VecDeque<ChatMessage>,
    /// The page request whose answer fills `messages`.
    pub(super) history: Option<crate::ids::QueryId>,
    /// Every page the overlay asked for and was not answered yet, so the answer to one asked
    /// before a switch of conversation is taken (and dropped) here instead of reaching the
    /// UI. At most [`PAGES_IN_FLIGHT`]; the oldest is forgotten.
    pub(super) pages: VecDeque<crate::ids::QueryId>,
    /// The newest toasts; at most `snapshot::MAX_TOASTS`.
    pub(super) toasts: VecDeque<Toast>,
    pub(super) next_toast: u64,
    /// The picture keys the overlay holds: those the last snapshot pointed at.
    pub(super) pictures: std::collections::BTreeSet<u64>,
    /// The snapshot sent last (only a different one is sent).
    pub(super) sent: Option<OverlaySnapshot>,
    pub(super) last_push: Option<MonoInstant>,
    /// A flush is scheduled.
    pub(super) flush_pending: bool,
}
