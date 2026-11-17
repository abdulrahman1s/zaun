//! Start a supervisor; be the supervisor; track, stop and recover sessions.
//!
//! Layer 5. See MODULES §6 and PROCESS_LIFECYCLE.
//!
//! | File | Content |
//! |---|---|
//! | [`launcher`] | `SessionLauncher`: write the plan, start `easyplay supervise` detached, wait for the row (§2) |
//! | [`supervisor`] | the supervisor loop (§3), `Running`/`Lingering` detection (§5), the closing transaction |
//! | [`spawn`] | `SandboxSpawner`: host actions and the `bwrap` process with its descriptor table |
//! | [`status`] | parsers of bwrap's `--json-status-fd` and of the init shim's status lines |
//! | [`log_pump`] | `game.log`, `events.jsonl`, launch phases (§3.1) |
//! | [`heartbeat`] | `active_ms` and `last_heartbeat_at` every 30 s |
//! | [`stop`] | stop requests and the graceful → forced stages (§6) |
//! | [`classify`] | the end-reason table (§7) |
//! | [`recovery`] | closing rows left open (§9) |
//! | [`registry`] | active sessions and their processes for the UI and CLI (§11) |
//! | [`lock`] | the per-prefix `flock` |
//! | [`sys`] | signals by identity, `fork`/`exec` hooks (the only `unsafe`) |
//! | [`handoff`] | starting a program EasyPlay does not own (the Steam client, ADR-0031) |

pub mod classify;
pub mod handoff;
pub mod heartbeat;
pub mod launcher;
pub mod lock;
pub mod log_pump;
pub mod recovery;
pub mod registry;
pub mod spawn;
pub mod status;
pub mod stop;
pub mod supervisor;
pub mod sys;

pub use classify::{EndClass, EndFacts, StopOrigin, classify_end};
pub use handoff::RealHandoff;
pub use launcher::{RealLauncher, SessionHandle, SessionLauncher};
pub use lock::{FlockLocks, LockError, PrefixLock};
pub use recovery::{RecoveredSession, RecoveryOptions, recover};
pub use registry::{ActiveSession, ProcessInfo, ProcessRole, Registry, process_list};
pub use spawn::{
    PipedHelper, RealSpawner, SandboxChild, SandboxIo, SandboxSpawner, SoundServer,
    spawn_piped_helper, write_sound_config,
};
pub use status::{BwrapStatus, BwrapStatusParser, InitStatus, InitStatusParser};
pub use stop::{StopMode, StopOutcome, request_stop};
pub use supervisor::{
    FinalRecord, SessionStore, StopTrigger, SupervisorConfig, SupervisorDeps, SupervisorOutcome,
};
