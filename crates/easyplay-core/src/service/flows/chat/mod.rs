//! Chat (ADR-0033): the engine's life, the snapshot, and the page's commands.

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
mod preview;
mod profile;
mod saves;
mod services;
mod state;

pub use preview::{PREVIEW_BUDGET, PREVIEW_MAX_SOURCE, PREVIEW_SIDE, data_uri_len, preview_jpeg};
pub(crate) use services::{ServicesDone, ServicesState};
pub(crate) use state::{ChatDone, ChatState};
pub use state::{MAX_BACKOFF, MAX_FAILURES, OUTBOX_DIR, PAGE, PAPER_FILE, backoff};
