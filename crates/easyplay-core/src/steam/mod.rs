//! Facts about the Steam installations on this computer (ADR-0031, GAME_LIBRARY §15).
//!
//! Layer 3: reads Steam's own files through [`crate::fsx::Fs`] and its processes through
//! [`crate::procfs::ProcSource`]; starts nothing, and writes only [`launch_options::write`]
//! (below). Nothing here runs unless
//! the user turned on `steam.show_games` (CONFIGURATION §2).
//!
//! | File | Content |
//! |---|---|
//! | [`keyvalues`] | tolerant, bounded reader of Valve's text KeyValues (`.vdf`, `.acf`) |
//! | [`roots`] | Steam roots (native, Flatpak, Snap) and their library folders |
//! | [`manifest`] | `appmanifest_<appid>.acf`, update state, what is not a game |
//! | [`account`] | play time of the most recently used account (`localconfig.vdf`) |
//! | [`compat`] | Proton use, the compatibility data and the prefix |
//! | [`art`] | covers and banners in Steam's local picture cache |
//! | [`scan`] | one read-only pass over everything above |
//! | [`client`] | `steam://` requests, the command per installation, running games, the client running |
//! | [`launch_options`] | a game's launch options in `localconfig.vdf`: read, and the one value EasyPlay writes |
//!
//! Steam owns all of these files. EasyPlay writes under a Steam root only at the user's
//! request, in two places: a program the user runs in a game's prefix
//! (`service::flows::steam`, ADR-0031 §6) writes to that prefix; and a game's launch options
//! ([`launch_options`], ADR-0031 §6a) — that one value in `localconfig.vdf`, with its
//! backup `localconfig.vdf.easyplay-bak` beside it, only while Steam is closed. Everything
//! else here only reads.

pub mod account;
pub mod art;
pub mod client;
pub mod compat;
pub mod keyvalues;
pub mod launch_options;
pub mod manifest;
pub mod roots;
pub mod scan;

pub use client::{Handoff, SteamRequest, running_games};
pub use manifest::NotAGame;
pub use roots::SteamRoot;
pub use scan::{Skipped, SteamGame, SteamScan, scan};
