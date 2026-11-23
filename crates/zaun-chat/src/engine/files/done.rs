//! A transfer job's result back on the engine thread (Subscriber.hs `processAgentMsgSndFile`, `processAgentMsgRcvFile`).

use std::path::PathBuf;

use zaun_chat_wire::model::FileState;

use super::transfers::{FileDone, FileKey, Sent};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_file_done(&mut self, done: FileDone) {
        let (key, outcome) = match done {
            FileDone::Progress { key, job, percent } => (key, self.on_progress(key, job, percent)),
            FileDone::Uploaded { key, job, result } => (key, self.on_finished(key, job, result)),
            FileDone::Downloaded { key, job, result } => {
                (key, self.on_downloaded(key, job, result))
            }
        };
        if let Err(e) = outcome {
            self.log.line(format_args!(
                "file {}/{}: {}",
                key.friend, key.item, e.detail
            ));
        }
    }

    /// Progress of a run still wanted; a cancelled or older run says nothing more.
    fn on_progress(&mut self, key: FileKey, job: u64, percent: u8) -> Result<()> {
        if !self.files.is_current(key, job) {
            return Ok(());
        }
        let (it, rec) = self.file_of(key.friend, key.item)?;
        let now = match Self::file_state(&it, &rec) {
            Some(FileState::Sending { percent: p }) if p < percent => {
                FileState::Sending { percent }
            }
            Some(FileState::Receiving { percent: p }) if p < percent => {
                FileState::Receiving { percent }
            }
            Some(_) | None => return Ok(()),
        };
        self.show_file(key.friend, key.item, &rec, now)
    }

    fn on_finished(&mut self, key: FileKey, job: u64, result: Result<Sent, String>) -> Result<()> {
        if !self.files.finish(key, job) {
            return Ok(());
        }
        let outcome = match result {
            Ok(sent) => self.on_uploaded(key, sent),
            Err(e) => Err(fail(Code::RequestFailed, format!("upload failed: {e}"))),
        };
        if outcome.is_err() {
            self.end_failed(key)?;
        }
        outcome
    }

    fn on_downloaded(
        &mut self,
        key: FileKey,
        job: u64,
        result: Result<PathBuf, String>,
    ) -> Result<()> {
        if !self.files.finish(key, job) {
            // Cancelled, or an older run: the copy it made is not wanted.
            if let Ok(p) = result {
                let _ = std::fs::remove_file(p); // already gone is fine
            }
            return Ok(());
        }
        let (_, mut rec) = self.file_of(key.friend, key.item)?;
        rec.active = false;
        let FileKey { friend, item } = key;
        match result {
            Ok(path) => {
                rec.local = Some(path.clone());
                self.save_file(friend, item, &rec)?;
                self.show_file(friend, item, &rec, FileState::Received(path))
            }
            Err(e) => {
                self.save_file(friend, item, &rec)?;
                self.show_file(friend, item, &rec, FileState::Failed)?;
                Err(fail(Code::RequestFailed, format!("download failed: {e}")))
            }
        }
    }

    fn end_failed(&mut self, key: FileKey) -> Result<()> {
        let (_, mut rec) = self.file_of(key.friend, key.item)?;
        rec.active = false;
        self.save_file(key.friend, key.item, &rec)?;
        self.show_file(key.friend, key.item, &rec, FileState::Failed)
    }
}
