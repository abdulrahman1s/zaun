//! A transfer job's result back on the engine thread (Subscriber.hs `processAgentMsgSndFile`, `processAgentMsgRcvFile`).

use std::path::PathBuf;

use zaun_chat_wire::model::FileState;

use super::transfers::{FileDone, FileKey};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_file_done(&mut self, done: FileDone) {
        let (key, outcome) = match done {
            FileDone::Progress { key, job, percent } => (key, self.on_progress(key, job, percent)),
            FileDone::Uploaded { key, job, result } => {
                let result = result.map(|s| (vec![s.description], s.expires));
                (key, self.on_finished(key, job, result))
            }
            FileDone::UploadedForMembers { key, job, result } => {
                let result = result.map(|s| (s.descriptions, s.expires));
                (key, self.on_finished(key, job, result))
            }
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
        let (it, rec) = self.file_of(key.chat(), key.item)?;
        let now = match Self::file_state(&it, &rec) {
            Some(FileState::Sending { percent: p }) if p < percent => {
                FileState::Sending { percent }
            }
            Some(FileState::Receiving { percent: p }) if p < percent => {
                FileState::Receiving { percent }
            }
            Some(_) | None => return Ok(()),
        };
        self.show_file(key.chat(), key.item, &rec, now)
    }

    /// `result`: each recipient description (one, or one per member) and when they expire.
    fn on_finished(
        &mut self,
        key: FileKey,
        job: u64,
        result: Result<(Vec<String>, Option<i64>), String>,
    ) -> Result<()> {
        if !self.files.finish(key, job) {
            return Ok(());
        }
        let outcome = match result {
            Ok((descriptions, expires)) => self.on_uploaded(key, descriptions, expires),
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
        let (chat, item) = (key.chat(), key.item);
        let (_, mut rec) = self.file_of(chat, item)?;
        rec.active = false;
        match result {
            Ok(path) => {
                rec.local = Some(path.clone());
                self.save_file(chat, item, &rec)?;
                self.show_file(chat, item, &rec, FileState::Received(path))
            }
            Err(e) => {
                self.save_file(chat, item, &rec)?;
                self.show_file(chat, item, &rec, FileState::Failed)?;
                Err(fail(Code::RequestFailed, format!("download failed: {e}")))
            }
        }
    }

    fn end_failed(&mut self, key: FileKey) -> Result<()> {
        let (chat, item) = (key.chat(), key.item);
        self.take_file_members(chat, item)?;
        let (_, mut rec) = self.file_of(chat, item)?;
        rec.active = false;
        self.save_file(chat, item, &rec)?;
        self.show_file(chat, item, &rec, FileState::Failed)
    }
}
