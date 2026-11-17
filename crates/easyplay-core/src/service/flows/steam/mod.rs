//! Steam games (ADR-0031): the option, scans, Play / Update / Open in Steam, the running
//! state, the Windows-storage actions, and the general commands addressed to a Steam game.
//!
//! - **Scans** run on the job pool (`steam::scan`, read-only) when the option is turned on,
//!   at start, with every "check for missing games" and on request.
//! - **Requests to Steam** are handed to the Steam client of the installation that lists the
//!   game, detached ([`Handoff`](crate::steam::Handoff)); EasyPlay never waits for, owns or
//!   stops what Steam starts.
//! - **Running** is seen through `/proc` (`steam::running_games`) by a small polling thread
//!   that runs only after a Play request or while a Steam game is seen running, so an idle
//!   EasyPlay does no work for Steam.
//! - **Windows storage**: a tool runs as an EasyPlay `Tool` session in the game's
//!   `compatdata/<appid>` with the Proton build that last ran it — inside EasyPlay's
//!   protection (Offline preset), and only while the game is not running.
//! - **Launch options** (ADR-0031 §6a): Steam's own per-app value in the account's
//!   `localconfig.vdf`, read for the page and written on the job pool only while no Steam
//!   client runs (`steam::launch_options`); "Close Steam" asks the client to close itself.

mod ask;
mod close;
mod details;
mod done;
mod edit;
mod folder;
mod launch_options;
mod prefix_job;
mod route;
mod run_in_prefix;
mod scan;
mod state;
mod tool_game;
mod watch;

pub use state::{STEAM_POLL, STEAM_START_WINDOW};
pub(crate) use state::{SteamDone, SteamState};
pub use tool_game::{steam_tool_plan, tool_game};
