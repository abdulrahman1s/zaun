//! The user's profile: the name and the line about them go to the engine, the avatar pair and
//! the status stay here (ADR-0033).
use super::state::PAPER_FILE;
use crate::chat::Request;
use crate::config::PreferencePatch;
use crate::model::{ChatProfile, ChatStatus};
use crate::service::prelude::*;

impl AppService {
    pub(super) fn chat_set_enabled(&mut self, on: bool, name: Option<String>) {
        let mut patch = PreferencePatch::new().set("chat.enabled", on);
        if let Some(n) = name {
            patch = patch.set("chat.name", n.trim().to_owned());
        }
        // Turning on again after a failure is the way to retry.
        if on && matches!(self.flow.chat.snapshot.status, ChatStatus::Failed(_)) {
            self.flow.chat.snapshot.status = ChatStatus::Off;
            self.flow.chat.failures = 0;
        }
        self.handle(Command::SetPreference(patch));
        if on && self.flow.chat.started_with.is_none() {
            self.chat_preference_changed();
        }
    }

    pub(super) fn chat_set_name(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let mut p = self.flow.chat.snapshot.profile.clone().unwrap_or_default();
        p.name = name.to_owned();
        self.chat_set_profile(p);
    }

    pub(super) fn chat_set_profile(&mut self, p: ChatProfile) {
        let name = p.name.trim().to_owned();
        if self.prefs.chat.name != name {
            let patch = PreferencePatch::new().set("chat.name", name.clone());
            self.handle(Command::SetPreference(patch));
        }
        let paper = p.paper.filter(|x| *x < 10);
        if paper != self.flow.chat.paper {
            self.flow.chat.paper = paper;
            self.save_paper(paper);
        }
        if let Some(cur) = &mut self.flow.chat.snapshot.profile {
            cur.paper = paper;
            cur.status = p.status;
        }
        self.chat_publish();
        let current = self.flow.chat.snapshot.profile.clone().unwrap_or_default();
        if current.name != name || current.about != p.about.trim() {
            self.chat_request(Request::SetProfile(ChatProfile {
                name,
                about: p.about.trim().to_owned(),
                ..p
            }));
        }
    }

    fn save_paper(&self, paper: Option<u8>) {
        let (fs, dir) = (&self.deps.fs, &self.deps.paths.chat_dir);
        let file = dir.join(PAPER_FILE);
        let written = match paper {
            Some(x) => fs
                .create_dir_all(dir, 0o700)
                .and_then(|()| fs.atomic_write(&file, x.to_string().as_bytes(), 0o600)),
            None => fs.remove_file(&file).or(Ok(())),
        };
        if let Err(e) = written {
            tracing::warn!(error = %e, "chat: the avatar pair was not saved");
        }
    }
}
