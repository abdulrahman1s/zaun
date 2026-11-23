//! Chat (ADR-0033): the engine's life, the snapshot, and the page's commands.

mod calls;
mod command;
mod done;
mod engine;
mod events;
mod failure;
mod files;
mod friends;
mod gifs;
mod groups;
mod links;
mod messages;
mod old_data;
mod overlay;
mod picture;
mod preview;
mod profile;
mod saves;
mod services;
mod share;
mod state;

pub(crate) use overlay::{OverlayDone, OverlayState};
pub use preview::{PREVIEW_BUDGET, PREVIEW_MAX_SOURCE, PREVIEW_SIDE, data_uri_len, preview_jpeg};
pub(crate) use services::{ServicesDone, ServicesState};
pub(crate) use state::{ChatDone, ChatState};
pub use state::{MAX_BACKOFF, MAX_FAILURES, OUTBOX_DIR, PAGE, PAPER_FILE, backoff};
