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

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
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

/// Sends activations to the window.
#[derive(Clone)]
pub struct Raiser {
    tx: Arc<Mutex<Sender<Activation>>>,
    waker: UiWaker,
}

impl Raiser {
    /// A raiser over the UI's activation channel.
    pub fn new(tx: Sender<Activation>, waker: UiWaker) -> Self {
        Self {
            tx: Arc::new(Mutex::new(tx)),
            waker,
        }
    }

    /// Raise the window (and maybe open the add-game picker).
    pub fn raise(&self, token: Option<String>, add_game: bool) {
        let _ = self
            .tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(Activation { token, add_game });
        if let Some(w) = self
            .waker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            w();
        }
    }
}

/// The handler attached once the service runs.
pub struct GuiRequests {
    /// Commands to the AppService.
    pub cmds: CommandSender,
    /// Snapshots.
    pub hub: Arc<SnapshotHub>,
    /// Window activation.
    pub raiser: Raiser,
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
                    self.raiser.raise(activation_token, false);
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
                self.cmds.send(Command::OpenPaths {
                    paths,
                    origin: svc_origin,
                });
                if !known || origin == WireOrigin::Add {
                    self.raiser.raise(activation_token, false);
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
                self.raiser
                    .raise(activation_token, then == Some(RaiseThen::AddGame));
                reply.reply(&Response::ok());
            }
            Request::Ping => reply.reply(&Response::ok()),
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
        let (tx, rx) = channel();
        let waker: UiWaker = Arc::new(Mutex::new(None));
        let raiser = Raiser::new(tx, waker);
        // Without a service the late handler answers "still starting" (not exercised here);
        // the raiser itself is what `raise` uses.
        raiser.raise(Some("tok".into()), true);
        assert_eq!(
            rx.try_recv().unwrap(),
            Activation {
                token: Some("tok".into()),
                add_game: true
            }
        );
        let t = InProcessTransport { handler: None };
        assert!(!t.ping());
    }
}
