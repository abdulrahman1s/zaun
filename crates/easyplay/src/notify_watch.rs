//! Desktop notifications of the GUI process (DESKTOP_INTEGRATION §7): a thread follows the
//! open sessions in the snapshot hub, asleep until they are published again; when one ends,
//! its row is read (read-only) and a crash, a failure to start or (when enabled) a normal
//! end is announced while the window is not focused. The supervisor sends nothing.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use easyplay_core::desktop::notify::{self, Notifier, NotifyEvent};
use easyplay_core::ids::SessionId;
use easyplay_core::model::{EndReason, Session, SessionKind};
use easyplay_core::service::SnapshotHub;
use easyplay_core::storage::{Db, SessionRepo};

/// The event for an ended session row.
pub fn event_for(s: &Session) -> Option<NotifyEvent> {
    if s.kind != SessionKind::Game {
        return None;
    }
    match s.end_reason? {
        EndReason::Crashed { .. } | EndReason::FailedToStart { .. } => Some(NotifyEvent::Crashed {
            game: s.game_name.clone(),
        }),
        EndReason::Exited { .. } | EndReason::StoppedByUser => Some(NotifyEvent::Ended {
            game: s.game_name.clone(),
            played_ms: s.duration_ms.unwrap_or(s.active_ms),
        }),
        _ => None,
    }
}

/// The running watcher; stops when dropped.
pub struct NotifyWatch {
    stop: Arc<AtomicBool>,
    hub: Arc<SnapshotHub>,
    thread: Option<JoinHandle<()>>,
    friends: Option<JoinHandle<()>>,
}

impl NotifyWatch {
    /// Watch `hub`; `focus` mirrors the window's focus.
    pub fn spawn(
        hub: Arc<SnapshotHub>,
        db_file: PathBuf,
        focus: Arc<AtomicBool>,
        notifier: Arc<dyn Notifier>,
    ) -> Option<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let friends = Self::spawn_friends(
            Arc::clone(&hub),
            Arc::clone(&stop),
            Arc::clone(&focus),
            Arc::clone(&notifier),
        );
        let (s2, h2) = (Arc::clone(&stop), Arc::clone(&hub));
        let thread = std::thread::Builder::new()
            .name("notify-watch".into())
            .spawn(move || {
                let hub = h2;
                let mut seen = hub.sessions.load();
                loop {
                    // Asleep until the service publishes sessions again (or the watcher is
                    // dropped): nothing wakes up while nothing happens.
                    let now = hub.sessions.wait_until(|current| {
                        s2.load(Ordering::Relaxed) || !Arc::ptr_eq(current, &seen)
                    });
                    if s2.load(Ordering::Relaxed) {
                        return;
                    }
                    let ended: Vec<SessionId> = seen
                        .sessions
                        .iter()
                        .map(|s| s.id)
                        .filter(|id| now.get(*id).is_none())
                        .collect();
                    seen = now;
                    if !ended.is_empty()
                        && let Ok(mut db) = Db::open_ro(&db_file)
                    {
                        let prefs = hub.prefs.load();
                        let focused = focus.load(Ordering::Relaxed);
                        for id in ended {
                            let Some(ev) =
                                db.session(id).ok().flatten().as_ref().and_then(event_for)
                            else {
                                continue;
                            };
                            if notify::wanted(&ev, &prefs, focused) {
                                notifier.notify(&notify::compose(&ev, None));
                            }
                        }
                    }
                }
            })
            .ok()?;
        Some(Self {
            stop,
            hub,
            thread: Some(thread),
            friends,
        })
    }

    /// New friend messages (ADR-0033), from the Friends snapshot: one notification per
    /// conversation whose unread count rose, while no window has focus.
    fn spawn_friends(
        hub: Arc<SnapshotHub>,
        stop: Arc<AtomicBool>,
        focus: Arc<AtomicBool>,
        notifier: Arc<dyn Notifier>,
    ) -> Option<JoinHandle<()>> {
        std::thread::Builder::new()
            .name("notify-friends".into())
            .spawn(move || {
                let mut seen = hub.friends.load();
                loop {
                    let now = hub.friends.wait_until(|current| {
                        stop.load(Ordering::Relaxed) || !Arc::ptr_eq(current, &seen)
                    });
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let prefs = hub.prefs.load();
                    let focused = focus.load(Ordering::Relaxed);
                    for ev in notify::friend_messages(&seen, &now, prefs.friends.notify_preview) {
                        if notify::wanted(&ev, &prefs, focused) {
                            notifier.notify(&notify::compose(&ev, None));
                        }
                    }
                    seen = now;
                }
            })
            .ok()
    }
}

impl Drop for NotifyWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.hub.sessions.wake();
        self.hub.friends.wake();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let Some(t) = self.friends.take() {
            let _ = t.join();
        }
    }
}
