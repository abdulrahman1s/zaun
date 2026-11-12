//! Decide which game an executable is, and what directory is "the game".
//!
//! Layer 4 (pure recognition, reads only through `fsx`). See MODULES §5 and GAME_IDENTITY.
//!
//! | File | Content |
//! |---|---|
//! | [`fingerprint`] | size, mtime, quick hash, optional full hash (§3) |
//! | [`pe`] | [`PeInspector`], [`PeLimits`], bounded PE reading and string sanitising (§6) |
//! | [`matcher`] | the resolution procedure of §4 over [`LibraryIndex`] |
//! | [`game_root`] | root detection (§5) |
//! | [`sensitive`] | shared locations and the loose-executable rule (§5.1) |
//! | [`naming`] | display and sort names (§7) |
//! | [`hints`] | setup hints, folder findings, `steam_appid.txt` (§8, §10) |
//! | [`lnk`] | Windows shortcut targets for the installer flow (LOGIC_SPECS §13) |
//!
//! Nothing here writes to the database, modifies files or executes anything.

pub mod fingerprint;
pub mod game_root;
pub mod hints;
pub mod lnk;
pub mod matcher;
pub mod naming;
pub mod pe;
pub mod sensitive;

pub use game_root::{GameRootProposal, RootKind, RootReason};
pub use hints::{AntiCheat, ExeHints, SetupHints, read_steam_appid};
pub use matcher::{Change, IndexedExe, LibraryIndex, MatchOutcome, PathProbe, UnknownHint};
pub use pe::{ExeFormat, FsPeInspector, IconBytes, PeError, PeInspector, PeLimits};
pub use sensitive::{SharedLocations, SharedReason};
