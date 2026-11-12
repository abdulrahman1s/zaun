//! Fakes of the seams in ARCHITECTURE §7 that exist so far: [`FakeClock`], [`MemFs`],
//! [`FakeProc`] and [`ScriptedRunner`]. Fakes for later seams (`host`, `session`, `desktop`)
//! are added beside them as those traits are written.

mod clock;
mod fs;
mod proc;
mod runner;

pub use clock::FakeClock;
pub use fs::{MemFs, ROOT_DEVICE};
pub use proc::{FakeProc, proc_entry};
pub use runner::{CannedOutput, ScriptedRunner, UnexpectedCommand};
