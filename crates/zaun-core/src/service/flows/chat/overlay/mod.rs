//! The in-game overlay (ADR-0037): started with a game when Chat and `chat.overlay` are on,
//! fed with snapshots of Chat, and its messages turned into Chat commands.

mod done;
mod history;
mod life;
mod look;
mod push;
mod said;
mod state;
mod toggle;

pub(crate) use state::{OverlayDone, OverlayState};
