//! What the overlay holds between frames: the last snapshot, whether the panel is open,
//! the message being typed, the toasts on screen and what to tell the app after the frame.
use std::sync::Arc;

use easyplay_core::chat::overlay::{FromOverlay, OverlaySnapshot};

use crate::labels::Labels;
use crate::look::Look;
use crate::motion::Fade;
use crate::pictures::Pictures;

/// What a toast on screen stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKey {
    /// A message toast (its ID in the snapshot).
    Message(u64),
    /// The call that rings.
    Call,
}

/// A toast on screen: when it came, and when it started to leave.
#[derive(Clone, Copy, Debug)]
pub struct LiveToast {
    pub key: ToastKey,
    pub born: f64,
    pub leaving: Option<f64>,
}

/// Where the keyboard goes when the panel opens or the answer bar appears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Composer,
    Answer,
}

pub struct Model {
    /// The latest snapshot, shared: a frame borrows it instead of copying its parts.
    pub snap: Arc<OverlaySnapshot>,
    /// Counts snapshots (and local changes to one): what the label cache keys on.
    pub rev: u64,
    /// The words drawn, made once per snapshot (`labels`).
    pub labels: Labels,
    /// A snapshot has arrived.
    pub has_state: bool,
    pub open: Fade,
    pub draft: String,
    /// Toasts on screen, newest first; at most `MAX_TOASTS` that are not leaving, so this
    /// never holds more than twice that.
    pub toasts: Vec<LiveToast>,
    /// Message toasts already shown (only IDs still in the snapshot are kept).
    pub seen: Vec<u64>,
    /// The "Show while playing" popover, and its corner menu.
    pub pins: bool,
    pub corner_menu: bool,
    pub focus: Option<Focus>,
    /// Messages for the app, sent after the frame.
    pub said: Vec<FromOverlay>,
    pub look: Look,
    pub pictures: Pictures,
    /// The local UTC offset for times (read once at start).
    pub offset_s: i32,
}

impl Model {
    pub fn new(offset_s: i32) -> Self {
        Self {
            snap: Arc::new(OverlaySnapshot::default()),
            rev: 0,
            labels: Labels::default(),
            has_state: false,
            open: Fade::default(),
            draft: String::new(),
            toasts: Vec::new(),
            seen: Vec::new(),
            pins: false,
            corner_menu: false,
            focus: None,
            said: Vec::new(),
            look: Look::new("tide", false),
            pictures: Pictures::default(),
            offset_s,
        }
    }

    /// Change the snapshot in place (the popover's optimistic switches, tests).
    pub fn snap_mut(&mut self) -> &mut OverlaySnapshot {
        self.rev += 1;
        Arc::make_mut(&mut self.snap)
    }

    pub fn reduced(&self) -> bool {
        self.snap.look.reduced_motion
    }
}
