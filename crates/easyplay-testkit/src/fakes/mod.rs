//! Fakes of the seams in ARCHITECTURE §7 that exist so far: [`FakeClock`], [`MemFs`],
//! [`FakeProc`], [`ScriptedRunner`] and [`FakeLauncher`]. Fakes for later seams (`host`, `session`, `desktop`)
//! are added beside them as those traits are written.

mod clock;
mod fs;
mod http_server;
mod launcher;
mod proc;
mod runner;
mod trusted;

pub use clock::FakeClock;
pub use fs::{MemFs, ROOT_DEVICE};
pub use http_server::{FakeHttpServer, Reply};
pub use launcher::{FakeLauncher, FakeStart};
pub use proc::{FakeProc, proc_entry};
pub use runner::{CannedOutput, ScriptedRunner, UnexpectedCommand};
pub use trusted::{PlanHook, RowLauncher, RowScript, ScriptedSteps, StepScript};
