//! Desktop commands (DESKTOP_INTEGRATION §4, §6, §8): the `.exe` default handler, menu shortcuts, opening folders and logs.
//!
//! `xdg-mime`, `xdg-open` and file writes run on the job pool; results come back as
//! `Internal::Desktop`. The integration state is published in the snapshot hub's
//! `integration` cell.

mod default_handler;
mod integration;
mod job;
mod open_folder;
mod shortcut;
mod view_log;

pub(crate) use job::DesktopDone;
