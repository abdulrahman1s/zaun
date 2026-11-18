//! The preview's updates (ADR-0040): an install Zaun owns, a newer version found two hours
//! ago; "Download" stages it at once and "Restart now" says what would happen.

use std::time::Duration;

use zaun_core::clock::Timestamp;
use zaun_core::model::{InstallChannel, UpdateDownload, UpdateOffer, UpdateSnapshot};
use zaun_core::service::UpdateCommand;

/// The sample: 0.2.0 available for an owned tarball install.
pub fn sample(now: Timestamp) -> UpdateSnapshot {
    UpdateSnapshot {
        configured: true,
        channel: InstallChannel::Tarball { owned: true },
        current: zaun_core::consts::APP_VERSION.to_owned(),
        last_checked: Some(now.saturating_sub(Duration::from_secs(2 * 3600))),
        checking: false,
        available: Some(UpdateOffer {
            version: "0.2.0".into(),
            date: "2026-12-01".into(),
            notes: "Game streams in friends' calls, a quieter library and faster starts.".into(),
            has_notes: true,
        }),
        download: UpdateDownload::Idle,
        failure: None,
        rolled_back: None,
    }
}

/// The sample after `c` (the preview neither downloads nor restarts anything).
pub fn handle(u: &UpdateSnapshot, c: UpdateCommand, now: Timestamp) -> UpdateSnapshot {
    let mut next = u.clone();
    match c {
        UpdateCommand::Check => next.last_checked = Some(now),
        UpdateCommand::Download => {
            if let Some(a) = &u.available {
                next.download = UpdateDownload::Staged {
                    version: a.version.clone(),
                };
            }
        }
        UpdateCommand::Restart | UpdateCommand::OpenNotes => {}
    }
    next
}
