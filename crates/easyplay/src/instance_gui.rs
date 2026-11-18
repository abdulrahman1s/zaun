//! The GUI side of the instance socket (DESKTOP_INTEGRATION §5.2): requests from other
//! `easyplay` processes become AppService commands and window activations.
//!
//! The socket is claimed before the AppService starts (so a second process never opens the
//! library), and the handler is attached once the service runs: requests that arrive in
//! between wait for it on their connection thread.
//!
//! Raising policy: `open` of a known game does not raise (the game takes focus), `open` of
//! anything else, `raise` and `add` do. `launch` with `wait` streams the session's state by
//! watching the snapshot hub, and its end from the database (read-only).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use easyplay_core::desktop::instance::{
    RaiseThen, Replier, Request, RequestHandler, Response, StreamEvent, WireOrigin,
};
use easyplay_core::error::ErrorCode;
use easyplay_core::ids::{ExecutableId, GameId, SessionId};
use easyplay_core::library::resolve::resolve_game;
use easyplay_core::model::{Activity, EndReason};
use easyplay_core::service::{
    Command, CommandSender, LaunchOrigin, OpenOrigin, SnapshotHub, StopMode,
};
use easyplay_core::storage::{Db, SessionRepo};
use easyplay_ui::Activation;

/// How often `launch --wait` looks at the snapshots.
const POLL: Duration = Duration::from_millis(250);
/// How long a forwarded launch may stay without a session before it counts as failed.
const START_DEADLINE: Duration = Duration::from_secs(180);

/// Wakes the UI after an activation was queued.
pub type UiWaker = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;

/// What the resident service opens a window for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowRequest {
    /// The full window: `raise`, `--add`, an added program, a flow waiting for an answer.
    Library {
        /// Open the add-game picker.
        add_game: bool,
    },
    /// The compact quick-launch window following `path`, already sent to the service.
    QuickLaunch {
        /// The program.
        path: PathBuf,
        /// Not a known game: the window would be raised if it were already open.
        raise: bool,
    },
}

impl WindowRequest {
    /// The activation this request becomes when a window is already open.
    fn activation(&self) -> Option<Activation> {
        match self {
            WindowRequest::Library { add_game } => Some(Activation {
                token: None,
                add_game: *add_game,
            }),
            WindowRequest::QuickLaunch { raise: true, .. } => Some(Activation::default()),
            WindowRequest::QuickLaunch { raise: false, .. } => None,
        }
    }
}

enum Slot {
    /// No window (only the resident service gets here).
    Closed,
    /// A window is being created; activations wait for it.
    Opening(Vec<Activation>),
    /// A window is open.
    Open(Sender<Activation>),
}

struct WindowState {
    slot: Slot,
    requests: VecDeque<WindowRequest>,
    poked: bool,
}

/// The window of this process as other threads see it. The GUI has one window for its
/// lifetime; the resident service opens windows on demand, one at a time, on the main
/// thread ([`Windows::next`]), and wakes that thread for service events ([`Windows::poke`]).
pub struct Windows {
    state: Mutex<WindowState>,
    cv: Condvar,
    waker: UiWaker,
}

impl Windows {
    /// No window yet.
    pub fn new(waker: UiWaker) -> Self {
        Self {
            state: Mutex::new(WindowState {
                slot: Slot::Closed,
                requests: VecDeque::new(),
                poked: false,
            }),
            cv: Condvar::new(),
            waker,
        }
    }

    fn lock(&self) -> MutexGuard<'_, WindowState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wake_ui(&self) {
        if let Some(w) = self
            .waker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            w();
        }
    }

    /// Deliver `a` to the window, or ask for one showing `otherwise`.
    fn deliver(&self, a: Option<Activation>, otherwise: WindowRequest) {
        let mut s = self.lock();
        match &mut s.slot {
            Slot::Open(tx) => {
                let Some(a) = a else { return };
                if tx.send(a).is_ok() {
                    drop(s);
                    self.wake_ui();
                    return;
                }
                // The window went away without saying so.
                s.slot = Slot::Closed;
                s.requests.push_back(otherwise);
            }
            Slot::Opening(buf) => buf.extend(a),
            Slot::Closed => s.requests.push_back(otherwise),
        }
        self.cv.notify_all();
    }

    /// Raise the window (and maybe open the add-game picker); without one, open it.
    pub fn raise(&self, token: Option<String>, add_game: bool) {
        self.deliver(
            Some(Activation { token, add_game }),
            WindowRequest::Library { add_game },
        );
    }

    /// A program was opened and already sent to the service: an open window is raised for
    /// an unknown program only (a known game takes focus itself); without a window, the
    /// compact quick-launch window follows it.
    pub fn program_opened(&self, path: PathBuf, raise: bool, token: Option<String>) {
        self.deliver(
            raise.then_some(Activation {
                token,
                add_game: false,
            }),
            WindowRequest::QuickLaunch { path, raise },
        );
    }

    /// Whether a window is open or being opened.
    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        !matches!(self.lock().slot, Slot::Closed)
    }

    /// The resident loop: wait up to `timeout` for a window request or a [`poke`]. A
    /// request returned marks a window as opening; requests queued behind it become its
    /// activations.
    ///
    /// [`poke`]: Windows::poke
    pub fn next(&self, timeout: Duration) -> Option<WindowRequest> {
        let mut s = self.lock();
        if s.requests.is_empty() && !s.poked {
            s = self
                .cv
                .wait_timeout_while(s, timeout, |s| s.requests.is_empty() && !s.poked)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        s.poked = false;
        let first = s.requests.pop_front()?;
        let rest: Vec<Activation> = s
            .requests
            .drain(..)
            .filter_map(|r| r.activation())
            .collect();
        s.slot = Slot::Opening(rest);
        Some(first)
    }

    /// The resident loop opens a window of its own accord (a flow waits for an answer).
    pub fn begin(&self) {
        let mut s = self.lock();
        let rest: Vec<Activation> = s
            .requests
            .drain(..)
            .filter_map(|r| r.activation())
            .collect();
        s.slot = Slot::Opening(rest);
    }

    /// Wake the resident loop (the service has events).
    pub fn poke(&self) {
        self.lock().poked = true;
        self.cv.notify_all();
    }

    /// The window exists and reads activations from the receiver of `tx`.
    pub fn opened(&self, tx: Sender<Activation>) {
        let mut s = self.lock();
        if let Slot::Opening(buf) = &mut s.slot {
            for a in buf.drain(..) {
                let _ = tx.send(a);
            }
        }
        s.slot = Slot::Open(tx);
        drop(s);
        self.wake_ui();
    }

    /// The window closed; activations it did not read open the next one.
    pub fn closed(&self, unread: Vec<Activation>) {
        let mut s = self.lock();
        s.slot = Slot::Closed;
        if let Some(add_game) = unread.iter().map(|a| a.add_game).reduce(|a, b| a || b) {
            s.requests.push_back(WindowRequest::Library { add_game });
        }
        drop(s);
        *self.waker.lock().unwrap_or_else(PoisonError::into_inner) = None;
        self.cv.notify_all();
    }
}

/// The handler attached once the service runs.
pub struct GuiRequests {
    /// Commands to the AppService.
    pub cmds: CommandSender,
    /// Snapshots.
    pub hub: Arc<SnapshotHub>,
    /// The window.
    pub windows: Arc<Windows>,
    /// The library database (read-only access for session ends).
    pub db_file: PathBuf,
}

fn err(code: ErrorCode, m: impl Into<String>) -> Response {
    Response::error(code, m)
}

impl GuiRequests {
    fn resolve(&self, game: &str) -> Result<GameId, Response> {
        let lib = self.hub.library.load();
        resolve_game(lib.games.iter().map(|g| (g.id, &*g.name)), game).map_err(|e| {
            err(
                e.code,
                e.context_value("detail")
                    .unwrap_or("no such game")
                    .to_owned(),
            )
        })
    }

    fn activity(&self, id: GameId) -> Activity {
        self.hub
            .library
            .load()
            .get(id)
            .map(|g| g.activity)
            .unwrap_or_default()
    }

    fn session_of(&self, id: GameId) -> Option<SessionId> {
        self.hub.sessions.load().for_game(id).map(|s| s.id)
    }

    /// `launch --wait`: stream until the session ends or the client goes away.
    fn stream(&self, id: GameId, reply: &mut dyn Replier) {
        let mut waited = Duration::ZERO;
        let mut seen_launching = false;
        let sid = loop {
            if let Some(s) = self.session_of(id) {
                break s;
            }
            match self.activity(id) {
                Activity::Idle if seen_launching || waited > Duration::from_secs(5) => {
                    reply.event(&StreamEvent::Ended {
                        reason: "failed_to_start".into(),
                        exit_code: None,
                    });
                    return;
                }
                Activity::Idle => {}
                _ => seen_launching = true,
            }
            if waited > START_DEADLINE {
                reply.event(&StreamEvent::Ended {
                    reason: "failed_to_start".into(),
                    exit_code: None,
                });
                return;
            }
            std::thread::sleep(POLL);
            waited += POLL;
        };
        if !reply.event(&StreamEvent::State {
            state: "running".into(),
        }) {
            return;
        }
        while self.hub.sessions.load().get(sid).is_some() {
            std::thread::sleep(POLL);
        }
        let row = Db::open_ro(&self.db_file)
            .ok()
            .and_then(|mut db| db.session(sid).ok().flatten());
        let (reason, exit_code) = match row {
            Some(s) => (
                match s.end_reason {
                    Some(EndReason::FailedToStart { .. }) => "failed_to_start".to_owned(),
                    Some(EndReason::Crashed { .. }) => "crashed".to_owned(),
                    Some(r) => r.as_str().to_owned(),
                    None => "exited".to_owned(),
                },
                s.exit_status,
            ),
            None => ("exited".to_owned(), None),
        };
        reply.event(&StreamEvent::Ended { reason, exit_code });
    }
}

impl RequestHandler for GuiRequests {
    fn handle(&self, request: Request, reply: &mut dyn Replier) {
        match request {
            Request::Open {
                paths,
                origin,
                activation_token,
                ..
            } => {
                if paths.is_empty() {
                    self.windows.raise(activation_token, false);
                    return reply.reply(&Response::ok());
                }
                let lib = self.hub.library.load();
                let known = paths
                    .iter()
                    .all(|p| lib.games.iter().any(|g| *g.primary_exe_path == **p));
                let svc_origin = match origin {
                    WireOrigin::FileManager => OpenOrigin::FileManager,
                    WireOrigin::Cli => OpenOrigin::Cli,
                    WireOrigin::Add => OpenOrigin::Picker,
                };
                let first = paths[0].clone();
                self.cmds.send(Command::OpenPaths {
                    paths,
                    origin: svc_origin,
                });
                if origin == WireOrigin::Add {
                    // An added program shows its game page or setup sheet in the full window.
                    self.windows.raise(activation_token, false);
                } else {
                    self.windows.program_opened(first, !known, activation_token);
                }
                reply.reply(&Response::ok());
            }
            Request::Launch {
                game,
                exe,
                extra_args,
                wait,
            } => {
                if !extra_args.is_empty() {
                    return reply.reply(&err(
                        ErrorCode::LibBusy,
                        "extra arguments cannot be passed to the running EasyPlay; close it \
                         or set them on the game page",
                    ));
                }
                let id = match self.resolve(&game) {
                    Ok(id) => id,
                    Err(r) => return reply.reply(&r),
                };
                let exe = match exe.as_deref().map(str::parse::<ExecutableId>) {
                    None => None,
                    Some(Ok(e)) => Some(e),
                    Some(Err(_)) => {
                        return reply.reply(&err(
                            ErrorCode::LibInvalidField,
                            "the program must be given by its ID",
                        ));
                    }
                };
                if self.session_of(id).is_some() {
                    return reply.reply(&err(ErrorCode::SesAlreadyRunning, "already running"));
                }
                self.cmds.send(Command::Launch {
                    id,
                    exe,
                    origin: LaunchOrigin::Cli,
                });
                reply.reply(&Response::ok());
                if wait {
                    self.stream(id, reply);
                }
            }
            Request::Stop { game, force } => {
                let id = match self.resolve(&game) {
                    Ok(id) => id,
                    Err(r) => return reply.reply(&r),
                };
                if let Some(session) = self.session_of(id) {
                    self.cmds.send(Command::StopSession {
                        session,
                        mode: if force {
                            StopMode::Force
                        } else {
                            StopMode::Graceful
                        },
                    });
                }
                reply.reply(&Response::ok());
            }
            Request::Raise {
                activation_token,
                then,
            } => {
                self.windows
                    .raise(activation_token, then == Some(RaiseThen::AddGame));
                reply.reply(&Response::ok());
            }
            Request::Ping => reply.reply(&Response::ok()),
            Request::Overlay { action } => {
                if let Some(why) = crate::overlay_verb::refusal(&self.hub) {
                    return reply.reply(&err(ErrorCode::ChtOverlayUnavailable, why));
                }
                self.cmds
                    .send(Command::Overlay(crate::overlay_verb::action(action)));
                reply.reply(&Response::ok());
            }
        }
    }
}

/// The handler bound to the socket before the service exists.
#[derive(Default)]
pub struct LateHandler {
    inner: Mutex<Option<Arc<GuiRequests>>>,
}

impl LateHandler {
    /// Attach (or replace, after a renderer fallback) the real handler.
    pub fn attach(&self, h: GuiRequests) {
        *self.inner.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(h));
    }

    fn get(&self) -> Option<Arc<GuiRequests>> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl RequestHandler for LateHandler {
    fn handle(&self, request: Request, reply: &mut dyn Replier) {
        // The window and service start within seconds; wait for them on this thread.
        for _ in 0..200 {
            if let Some(h) = self.get() {
                return h.handle(request, reply);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        reply.reply(&err(
            ErrorCode::DskInstanceUnresponsive,
            "EasyPlay is still starting",
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyplay_core::desktop::instance::{InProcessTransport, InstanceTransport};
    use std::sync::mpsc::channel;

    #[test]
    fn raise_and_unknown_open_activate_the_window() {
        let waker: UiWaker = Arc::new(Mutex::new(None));
        let windows = Windows::new(waker);
        let (tx, rx) = channel();
        windows.opened(tx);
        windows.raise(Some("tok".into()), true);
        assert_eq!(
            rx.try_recv().unwrap(),
            Activation {
                token: Some("tok".into()),
                add_game: true
            }
        );
        // A known game does not raise an open window; an unknown program does.
        windows.program_opened(PathBuf::from("/g/a.exe"), false, None);
        assert!(rx.try_recv().is_err());
        windows.program_opened(PathBuf::from("/g/b.exe"), true, None);
        assert_eq!(rx.try_recv().unwrap(), Activation::default());
        // Without a service the late handler answers "still starting" (not exercised here).
        let t = InProcessTransport { handler: None };
        assert!(!t.ping());
    }

    #[test]
    fn without_a_window_requests_open_one_in_turn() {
        let windows = Windows::new(Arc::new(Mutex::new(None)));
        assert!(!windows.is_open());
        assert_eq!(windows.next(Duration::ZERO), None);
        windows.program_opened(PathBuf::from("/g/a.exe"), false, None);
        windows.raise(None, true);
        windows.program_opened(PathBuf::from("/g/b.exe"), false, None);
        assert_eq!(
            windows.next(Duration::ZERO),
            Some(WindowRequest::QuickLaunch {
                path: PathBuf::from("/g/a.exe"),
                raise: false
            })
        );
        // Opening: what came later waits for that window instead of opening more.
        assert!(windows.is_open());
        windows.raise(None, false);
        let (tx, rx) = channel();
        windows.opened(tx);
        let got: Vec<Activation> = rx.try_iter().collect();
        assert_eq!(
            got,
            vec![
                Activation {
                    token: None,
                    add_game: true
                },
                Activation::default()
            ]
        );
        assert_eq!(windows.next(Duration::ZERO), None);
        // Closing with an unread raise opens the full window next.
        windows.closed(vec![Activation::default()]);
        assert!(!windows.is_open());
        assert_eq!(
            windows.next(Duration::ZERO),
            Some(WindowRequest::Library { add_game: false })
        );
        // A poke wakes the loop without a request.
        windows.closed(Vec::new());
        windows.poke();
        assert_eq!(windows.next(Duration::from_secs(5)), None);
    }
}
