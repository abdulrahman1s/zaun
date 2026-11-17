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
    /// A friend wrote (ADR-0033).
    FriendMessage {
        /// Who: the friend, or "member · group".
        from: String,
        /// The text, only with `friends.notify_preview` on.
        text: Option<String>,
    },
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

/// A friend's message as a notification body: no control characters, at most 200
/// characters, and markup escaped (notification servers may read the body as markup).
fn clean_text(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .filter(|c| !c.is_control())
        .take(200)
        .collect();
    flat.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
        NotifyEvent::FriendMessage { from, text } => Notification {
            summary: clean_name(from),
            body: text
                .as_deref()
                .map(clean_text)
                .unwrap_or_else(|| "New message".to_owned()),
            icon,
            open_action: true,
        },
    }
}

/// The messages to announce between two Friends snapshots: one per conversation whose
/// unread count rose and whose latest message is not the user's own.
pub fn friend_messages(
    before: &crate::model::FriendsSnapshot,
    now: &crate::model::FriendsSnapshot,
    preview: bool,
) -> Vec<NotifyEvent> {
    use crate::model::{MessageAuthor, MessageBody};
    now.chats
        .iter()
        .filter(|c| {
            let was = before
                .chats
                .iter()
                .find(|b| b.chat == c.chat)
                .map_or(0, |b| b.unread);
            c.unread > was
        })
        .filter_map(|c| {
            let last = c.last.as_ref()?;
            let from = match &last.author {
                MessageAuthor::Me => return None,
                MessageAuthor::Friend => c.name.clone(),
                MessageAuthor::Member(m) => format!("{m} · {}", c.name),
            };
            let text = match (&last.body, preview) {
                (MessageBody::Text(t), true) => Some(t.clone()),
                _ => None,
            };
            Some(NotifyEvent::FriendMessage { from, text })
        })
        .collect()
}

/// Whether `event` is notified with these preferences while the window has (or lacks) focus.
pub fn wanted(event: &NotifyEvent, prefs: &Preferences, focused: bool) -> bool {
    match event {
        NotifyEvent::Crashed { .. } => !focused && prefs.launch.notify_on_crash,
        NotifyEvent::LayerReady => !focused,
        NotifyEvent::Ended { .. } => !focused && prefs.launch.notify_on_exit,
        NotifyEvent::TamperRemoved => true,
        NotifyEvent::FriendMessage { .. } => {
            !focused && prefs.friends.enabled && prefs.friends.notify
        }
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
    use crate::model::{
        ChatPreview, ChatRef, Delivery, FriendMessage, FriendsSnapshot, MessageAuthor, MessageBody,
    };

    fn chat(
        chat: ChatRef,
        name: &str,
        unread: u32,
        author: MessageAuthor,
        text: &str,
    ) -> ChatPreview {
        ChatPreview {
            chat,
            name: name.into(),
            unread,
            last: Some(FriendMessage {
                id: 1,
                chat,
                author,
                body: MessageBody::Text(text.into()),
                at: crate::clock::Timestamp::EPOCH,
                delivery: Delivery::Received,
                unread: true,
            }),
        }
    }

    #[test]
    fn a_friend_message_is_announced_by_name_and_its_text_only_when_asked() {
        let before = FriendsSnapshot {
            chats: vec![chat(
                ChatRef::Direct(1),
                "Kai",
                0,
                MessageAuthor::Friend,
                "old",
            )],
            ..FriendsSnapshot::default()
        };
        let now = FriendsSnapshot {
            chats: vec![
                chat(
                    ChatRef::Direct(1),
                    "Kai",
                    1,
                    MessageAuthor::Friend,
                    "<b>gg</b> & go",
                ),
                chat(
                    ChatRef::Group(2),
                    "Squad",
                    2,
                    MessageAuthor::Member("Ana".into()),
                    "hi",
                ),
                chat(
                    ChatRef::Direct(3),
                    "Rin",
                    0,
                    MessageAuthor::Friend,
                    "read already",
                ),
            ],
            ..FriendsSnapshot::default()
        };
        let quiet = friend_messages(&before, &now, false);
        assert_eq!(
            quiet,
            vec![
                NotifyEvent::FriendMessage {
                    from: "Kai".into(),
                    text: None
                },
                NotifyEvent::FriendMessage {
                    from: "Ana · Squad".into(),
                    text: None
                },
            ]
        );
        let n = compose(&quiet[0], None);
        assert_eq!(
            (n.summary.as_str(), n.body.as_str()),
            ("Kai", "New message")
        );

        let shown = friend_messages(&before, &now, true);
        let n = compose(&shown[0], None);
        assert_eq!(n.body, "&lt;b&gt;gg&lt;/b&gt; &amp; go");

        // The user's own message, or nothing new: nothing.
        let mine = FriendsSnapshot {
            chats: vec![chat(
                ChatRef::Direct(1),
                "Kai",
                1,
                MessageAuthor::Me,
                "sent",
            )],
            ..FriendsSnapshot::default()
        };
        assert!(friend_messages(&before, &mine, true).is_empty());
        assert!(friend_messages(&now, &now, true).is_empty());
    }

    #[test]
    fn friend_messages_are_not_announced_while_focused_or_turned_off() {
        let ev = NotifyEvent::FriendMessage {
            from: "Kai".into(),
            text: None,
        };
        let mut p = Preferences::default();
        assert!(!wanted(&ev, &p, false), "Friends is off");
        p.friends.enabled = true;
        assert!(wanted(&ev, &p, false));
        assert!(!wanted(&ev, &p, true));
        p.friends.notify = false;
        assert!(!wanted(&ev, &p, false));
    }

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
