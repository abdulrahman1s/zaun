//! The updates snapshot, and the small helpers every update flow shares.

use crate::model::{UpdateDownload, UpdateSnapshot};
use crate::service::prelude::*;
use crate::update::{UpdateDeps, Version, state};

/// The running version (`0.0.0` if the build's own version does not parse).
pub(super) fn running() -> Version {
    Version::parse(crate::consts::APP_VERSION).unwrap_or(Version {
        major: 0,
        minor: 0,
        patch: 0,
        pre: Vec::new(),
    })
}

impl AppService {
    pub(super) fn update_deps(&self) -> Result<&UpdateDeps> {
        let u = self
            .deps
            .updates
            .as_ref()
            .ok_or_else(|| err!(UpdNotConfigured))?;
        ensure!(!u.keys.is_empty(), UpdNotConfigured);
        Ok(u)
    }

    /// Nothing is requested while `updates.check` is off (ADR-0040 §4).
    pub(super) fn ensure_update_consent(&self) -> Result<()> {
        ensure!(self.prefs.updates.check, UpdChecksOff);
        Ok(())
    }

    pub(super) fn save_update_state(&self) {
        let path = state::state_file(&self.deps.paths);
        if let Err(e) = self.flow.updates.file.save(&*self.deps.fs, &path) {
            tracing::warn!(error = %e, "update state not saved");
        }
    }

    pub(crate) fn publish_updates(&self) {
        let u = &self.flow.updates;
        let deps = self.deps.updates.as_ref();
        let download = match (&u.staged, u.downloading) {
            (Some(v), _) => UpdateDownload::Staged { version: v.clone() },
            (None, true) => UpdateDownload::Downloading,
            (None, false) => UpdateDownload::Idle,
        };
        let snap = UpdateSnapshot {
            configured: deps.is_some_and(|d| !d.keys.is_empty()),
            channel: deps.map(|d| d.install.channel).unwrap_or_default(),
            current: crate::consts::APP_VERSION.to_owned(),
            last_checked: u.file.last_checked,
            checking: u.checking,
            available: u.file.offer(&running()),
            download,
            failure: u.failure,
            rolled_back: u.file.rolled_back.clone(),
        };
        self.hub.updates.store(Arc::new(snap));
        self.emit(Event::UpdatesChanged);
    }
}
