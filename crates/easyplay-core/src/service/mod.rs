//! The application façade: one thread, one command loop, all state transitions.
//!
//! Layer 6. See MODULES §7 and ARCHITECTURE §5–6.
//!
//! | File | Content |
//! |---|---|
//! | [`command`] | [`Command`] and its payload types |
//! | [`event`] | [`Event`], [`Notice`], [`SetupContext`] |
//! | [`snapshot`] | [`SnapshotHub`], [`Swap`], [`ActiveSessions`] |
//! | [`app_service`] | [`AppService`], [`ServiceDeps`], [`ServiceHandle`], the loop |
//! | [`prompts`] | parked flows |
//! | [`session_watch`] | the session watcher thread (PROC-011) |
//! | [`flows`] | open, setup, launch, remove, runtime, artwork |

pub mod app_service;
pub mod command;
pub mod event;
pub mod flows;
pub mod prompts;
pub mod session_watch;
pub mod snapshot;

pub use app_service::{AppService, LaunchEnv, ServiceDeps, ServiceHandle, Waker, open_db};
pub use command::{AfterCreate, Command, LaunchOrigin, OpenOrigin, StopMode};
pub use event::{Event, Notice, SetupContext};
pub use snapshot::{ActiveSessions, SnapshotHub, Swap};
