//! Desktop notifications of the GUI process (DESKTOP_INTEGRATION §7): a thread watches the
//! open sessions in the snapshot hub; when one ends, its row is read (read-only) and a crash,
//! a failure to start or (when enabled) a normal end is announced while the window is not
//! focused. The supervisor sends nothing.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

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
    thread: Option<JoinHandle<()>>,
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
        let s2 = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("notify-watch".into())
            .spawn(move || {
                let mut open: BTreeMap<SessionId, ()> = BTreeMap::new();
                while !s2.load(Ordering::Relaxed) {
                    let now = hub.sessions.load();
                    let ended: Vec<SessionId> = open
                        .keys()
                        .filter(|id| now.get(**id).is_none())
                        .copied()
                        .collect();
                    open = now.sessions.iter().map(|s| (s.id, ())).collect();
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
                    std::thread::sleep(Duration::from_millis(500));
                }
            })
            .ok()?;
        Some(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for NotifyWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
