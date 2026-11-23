//! Work done off the engine thread (XFTP transfers, relay tests), its results posted back
//! here and handled one at a time like everything else (ADR-0012).

use crossbeam_channel::{Receiver, Sender, unbounded};

use super::agent_port::AgentPort;
use super::files::FileDone;
use super::machine::Engine;

#[derive(Debug)]
pub enum JobDone {
    File(FileDone),
}

/// The channel jobs answer on. Unbounded, but bounded in practice: one result per job, and the
/// jobs in flight are capped by whoever starts them.
#[derive(Debug, Clone)]
pub struct Jobs {
    pub tx: Sender<JobDone>,
    pub rx: Receiver<JobDone>,
}

impl Default for Jobs {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self { tx, rx }
    }
}

impl<A: AgentPort> Engine<A> {
    pub fn on_job(&mut self, done: JobDone) {
        match done {
            JobDone::File(d) => self.on_file_done(d),
        }
        self.pump();
    }
}
