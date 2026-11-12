//! Desktop notifications (DESKTOP_INTEGRATION §7).
//!
//! Sent by the GUI process only, through the desktop notification service (`notify-rust`).
//! Every notification carries the application name, its icon and the desktop-entry hint, and
//! never a file path. Failure to send is logged at `debug` and otherwise ignored.

use std::sync::{Arc, Mutex, PoisonError};

use crate::config::Preferences;
use crate::consts::APP_ID;

/// What happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotifyEvent {
    /// A game crashed or failed to start.
    Crashed {
        /// Game name.
        game: String,
    },
    /// A compatibility-layer download finished.
    LayerReady,
    /// A game ended normally.
    Ended {
        /// Game name.
        game: String,
        /// Played time in milliseconds.
        played_ms: u64,
    },
    /// The shared runtime was found modified and removed.
    TamperRemoved,
}

/// A composed notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    /// Title line.
    pub summary: String,
    /// Body (may be empty).
    pub body: String,
    /// Icon name in the icon theme.
    pub icon: String,
    /// Offer the *Open EasyPlay* action.
    pub open_action: bool,
}

fn played(ms: u64) -> String {
    let min = ms / 60_000;
    match (min / 60, min % 60) {
        (0, 0) => "less than a minute".to_owned(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// Strip control characters and anything that looks like a path from a game name.
fn clean_name(name: &str) -> String {
    let s: String = name.chars().filter(|c| !c.is_control()).take(80).collect();
    match s.rsplit('/').next() {
        Some(last) if s.contains('/') => last.to_owned(),
        _ => s,
    }
}

/// The text of an event (§7 table).
pub fn compose(event: &NotifyEvent, icon: Option<&str>) -> Notification {
    let icon = icon.unwrap_or(APP_ID).to_owned();
    match event {
        NotifyEvent::Crashed { game } => Notification {
            summary: format!("{} closed unexpectedly", clean_name(game)),
            body: String::new(),
            icon,
            open_action: true,
        },
        NotifyEvent::LayerReady => Notification {
            summary: "Ready to play".to_owned(),
            body: "The compatibility layer finished downloading.".to_owned(),
            icon,
            open_action: false,
        },
        NotifyEvent::Ended { game, played_ms } => Notification {
            summary: format!("{} · played {}", clean_name(game), played(*played_ms)),
            body: String::new(),
            icon,
            open_action: false,
        },
        NotifyEvent::TamperRemoved => Notification {
            summary: "EasyPlay removed a damaged copy of the compatibility layer".to_owned(),
            body: String::new(),
            icon,
            open_action: false,
        },
    }
}

/// Whether `event` is notified with these preferences while the window has (or lacks) focus.
pub fn wanted(event: &NotifyEvent, prefs: &Preferences, focused: bool) -> bool {
    match event {
        NotifyEvent::Crashed { .. } => !focused && prefs.launch.notify_on_crash,
        NotifyEvent::LayerReady => !focused,
        NotifyEvent::Ended { .. } => !focused && prefs.launch.notify_on_exit,
        NotifyEvent::TamperRemoved => true,
    }
}

/// Sends notifications (ARCHITECTURE §7 `Notifier`).
pub trait Notifier: Send + Sync {
    /// Show `n`; errors are swallowed by the implementation.
    fn notify(&self, n: &Notification);
}

/// `notify-rust` over D-Bus. `on_open` runs when the user picks *Open EasyPlay*.
pub struct DesktopNotifier {
    on_open: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl DesktopNotifier {
    /// A notifier calling `on_open` for the *Open EasyPlay* action.
    pub fn new(on_open: Option<Arc<dyn Fn() + Send + Sync>>) -> Self {
        Self { on_open }
    }
}

impl Notifier for DesktopNotifier {
    fn notify(&self, n: &Notification) {
        let n = n.clone();
        let on_open = self.on_open.clone();
        // D-Bus calls (and waiting for the action) never run on the UI or service thread.
        let spawned = std::thread::Builder::new()
            .name("notify".into())
            .spawn(move || {
                let mut note = notify_rust::Notification::new();
                note.appname(crate::consts::APP_NAME)
                    .summary(&n.summary)
                    .icon(&n.icon)
                    .hint(notify_rust::Hint::DesktopEntry(APP_ID.to_owned()));
                if !n.body.is_empty() {
                    note.body(&n.body);
                }
                if n.open_action && on_open.is_some() {
                    note.action("default", "Open EasyPlay");
                }
                match note.show() {
                    Ok(handle) => {
                        if let (true, Some(f)) = (n.open_action, on_open) {
                            handle.wait_for_action(|a| {
                                if a == "default" {
                                    f();
                                }
                            });
                        }
                    }
                    Err(e) => tracing::debug!(error = %e, "notification not shown"),
                }
            });
        if let Err(e) = spawned {
            tracing::debug!(error = %e, "notification thread not started");
        }
    }
}

/// Records notifications (tests).
#[derive(Default)]
pub struct RecordingNotifier {
    sent: Mutex<Vec<Notification>>,
}

impl RecordingNotifier {
    /// What was sent.
    pub fn sent(&self) -> Vec<Notification> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Notifier for RecordingNotifier {
    fn notify(&self, n: &Notification) {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(n.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_follow_the_table_and_carry_no_paths() {
        let n = compose(
            &NotifyEvent::Crashed {
                game: "/home/sam/Games/Evil\nGame".into(),
            },
            None,
        );
        assert_eq!(n.summary, "EvilGame closed unexpectedly");
        assert!(!n.summary.contains('/'));
        assert!(n.open_action);
        assert_eq!(n.icon, APP_ID);
        let e = compose(
            &NotifyEvent::Ended {
                game: "Hollow Knight".into(),
                played_ms: 72 * 60_000,
            },
            Some("easyplay-game-x"),
        );
        assert_eq!(e.summary, "Hollow Knight · played 1 h 12 min");
        assert_eq!(e.icon, "easyplay-game-x");
    }

    #[test]
    fn defaults_and_focus() {
        let p = Preferences::default();
        let crash = NotifyEvent::Crashed { game: "g".into() };
        let end = NotifyEvent::Ended {
            game: "g".into(),
            played_ms: 1,
        };
        assert!(wanted(&crash, &p, false));
        assert!(!wanted(&crash, &p, true));
        assert!(!wanted(&end, &p, false));
        let r = RecordingNotifier::default();
        r.notify(&compose(&crash, None));
        assert_eq!(r.sent().len(), 1);
    }
}
