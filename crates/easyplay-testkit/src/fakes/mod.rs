//! Fakes of the seams in ARCHITECTURE §7 (TEST-001): [`FakeClock`], [`MemFs`], [`FakeProc`],
//! [`ScriptedRunner`], [`FakeLauncher`], and in [`seams`] [`ScriptedHostProbe`],
//! [`TablePeInspector`], [`SolidCodec`], [`RecordingMime`] and [`ScriptedSpawner`]. Every
//! fake records its calls and can be scripted to fail; `crate::inject` adds counted
//! failure injection for any `Fs`.

mod clock;
mod fs;
mod http_server;
mod launcher;
mod proc;
mod runner;
pub mod seams;
mod trusted;

pub use clock::FakeClock;
pub use fs::{MemFs, ROOT_DEVICE};
pub use http_server::{FakeHttpServer, Reply};
pub use launcher::{FakeLauncher, FakeStart};
pub use proc::{FakeProc, proc_entry};
pub use runner::{CannedOutput, ScriptedRunner, UnexpectedCommand};
pub use seams::{
    RecordingMime, ScriptedHostProbe, ScriptedSpawner, SolidCodec, SpawnScript, TablePeInspector,
};
pub use trusted::{PlanHook, RowLauncher, RowScript, ScriptedSteps, StepScript};
