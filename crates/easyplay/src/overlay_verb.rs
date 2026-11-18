//! `easyplay overlay [toggle|open|close]` (ADR-0037 §7): the key users bind in their
//! compositor to show the in-game overlay. Wayland gives ordinary programs no global
//! hotkeys, so the compositor runs this verb and it reaches the running EasyPlay through the
//! instance socket. It never starts EasyPlay or the overlay by itself.
use easyplay_core::chat::overlay::OverlayStatus;
use easyplay_core::desktop::instance::OverlayVerb;
use easyplay_core::model::{ChatStatus, SessionKind};
use easyplay_core::service::{OverlayAction, SnapshotHub};
use easyplay_core::{Error, err};

use crate::cli::OverlayArg;

pub fn wire(a: OverlayArg) -> OverlayVerb {
    match a {
        OverlayArg::Toggle => OverlayVerb::Toggle,
        OverlayArg::Open => OverlayVerb::Open,
        OverlayArg::Close => OverlayVerb::Close,
    }
}

pub fn action(v: OverlayVerb) -> OverlayAction {
    match v {
        OverlayVerb::Toggle => OverlayAction::Toggle,
        OverlayVerb::Open => OverlayAction::Open,
        OverlayVerb::Close => OverlayAction::Close,
    }
}

/// Why the running EasyPlay cannot show the overlay now (`None` = it can). The answer is
/// `EP-CHT-013` with these words; the CLI prints them, and Settings reads the same status
/// (`SnapshotHub::overlay`).
pub fn refusal(hub: &SnapshotHub) -> Option<&'static str> {
    let prefs = hub.prefs.load();
    let running = matches!(
        hub.chat.load().status,
        ChatStatus::Starting | ChatStatus::Ready
    );
    if !prefs.chat.enabled || !running {
        return Some("Chat is off or not running");
    }
    if !prefs.chat.overlay {
        return Some("the overlay is off (chat.overlay)");
    }
    let status = *hub.overlay.load();
    let game = hub
        .sessions
        .load()
        .sessions
        .iter()
        .any(|s| s.kind == SessionKind::Game && s.is_open());
    match status {
        OverlayStatus::Unsupported => Some(
            "your desktop can't show the overlay (it has no layer shell); messages and calls \
             arrive as notifications",
        ),
        OverlayStatus::NoGpu => Some(
            "nothing on this computer can draw the overlay (no GPU path with transparency); \
             overlay.log says why",
        ),
        OverlayStatus::Missing => {
            Some("this build has no overlay, or EasyPlay runs without a Wayland display")
        }
        _ if !game => Some("no game is running"),
        OverlayStatus::Failing => {
            Some("the overlay stopped several times in a row; it tries again with the next game")
        }
        OverlayStatus::Idle | OverlayStatus::Running => None,
    }
}

/// Without a running EasyPlay there is nothing to show.
pub fn without_app() -> Result<u8, Error> {
    Err(err!(
        ChtOverlayUnavailable,
        detail = "EasyPlay is not running"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyplay_core::config::Preferences;
    use easyplay_core::model::{ChatSnapshot, Session};
    use easyplay_core::service::ActiveSessions;
    use std::sync::Arc;

    fn hub(status: OverlayStatus, game: bool) -> SnapshotHub {
        let mut prefs = Preferences::default();
        prefs.chat.enabled = true;
        prefs.chat.overlay = true;
        let library = easyplay_core::library::LibrarySnapshot::empty(
            easyplay_core::clock::Timestamp::from_millis(0),
        );
        let hub = SnapshotHub::new(library, prefs);
        hub.chat.store(Arc::new(ChatSnapshot {
            status: ChatStatus::Ready,
            ..ChatSnapshot::default()
        }));
        if game {
            let g = easyplay_testkit::fixtures::games::game(1, 1);
            let s: Session = easyplay_testkit::fixtures::games::session(
                &g,
                1,
                1,
                easyplay_core::clock::Timestamp::from_millis(0),
            );
            hub.sessions.store(Arc::new(ActiveSessions {
                revision: 1,
                sessions: vec![s],
            }));
        }
        hub.overlay.store(Arc::new(status));
        hub
    }

    #[test]
    fn the_hotkey_says_why_the_overlay_cannot_open() {
        assert!(
            refusal(&hub(OverlayStatus::Unsupported, true))
                .is_some_and(|w| w.contains("layer shell"))
        );
        assert!(refusal(&hub(OverlayStatus::NoGpu, true)).is_some_and(|w| w.contains("GPU")));
        assert!(
            refusal(&hub(OverlayStatus::Failing, true))
                .is_some_and(|w| w.contains("several times"))
        );
        assert!(refusal(&hub(OverlayStatus::Missing, false)).is_some_and(|w| w.contains("build")));
        assert_eq!(
            refusal(&hub(OverlayStatus::Idle, false)),
            Some("no game is running")
        );
        assert_eq!(refusal(&hub(OverlayStatus::Running, true)), None);
    }
}
