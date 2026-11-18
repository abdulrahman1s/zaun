//! The ScreenCast portal over D-Bus, blocking (ADR-0036): window sources only, the cursor
//! drawn into the picture, the choice remembered while EasyPlay runs (per game, so a
//! restored pick is never another program's window). EasyPlay runs outside the game's
//! sandbox (ADR-0016 is about the game's), so it talks to the user's session bus.
use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use super::pipewire::PipeWireSource;
use crate::video::source::{Capture, CaptureError, CaptureRequest, VideoSource};

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const SCREENCAST: &str = "org.freedesktop.portal.ScreenCast";
/// `types`: a window (1 would be a monitor, 4 a virtual screen: never asked for).
const WINDOW: u32 = 2;
/// `cursor_mode`: drawn into the picture.
const CURSOR_EMBEDDED: u32 = 2;
/// `persist_mode`: until revoked (the token is only kept while EasyPlay runs).
const PERSIST: u32 = 2;
/// Remembered choices (one per game shared in this run); the oldest goes first.
const TOKENS: usize = 32;

/// Shares' pictures through the portal.
pub struct PortalCapture {
    /// The portal's restore token by game id.
    tokens: Mutex<Vec<(String, String)>>,
    next: AtomicU64,
}

impl PortalCapture {
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(Vec::new()),
            next: AtomicU64::new(0),
        }
    }

    fn token(&self) -> String {
        format!("easyplay{}", self.next.fetch_add(1, Ordering::Relaxed))
    }

    fn remembered(&self, game: &str) -> Option<String> {
        let t = self.tokens.lock().unwrap_or_else(PoisonError::into_inner);
        t.iter().find(|(g, _)| g == game).map(|(_, t)| t.clone())
    }

    fn remember(&self, game: &str, token: String) {
        let mut t = self.tokens.lock().unwrap_or_else(PoisonError::into_inner);
        t.retain(|(g, _)| g != game);
        if t.len() == TOKENS {
            t.remove(0);
        }
        t.push((game.to_owned(), token));
    }
}

impl Default for PortalCapture {
    fn default() -> Self {
        Self::new()
    }
}

fn failed(e: impl std::fmt::Display) -> CaptureError {
    CaptureError::Failed(e.to_string())
}

/// The ScreenCast session; closed when dropped (the picture stops).
struct Session {
    conn: Connection,
    handle: OwnedObjectPath,
}

impl Drop for Session {
    fn drop(&mut self) {
        let closed = Proxy::new(
            &self.conn,
            DEST,
            self.handle.as_str(),
            "org.freedesktop.portal.Session",
        )
        .and_then(|p| p.call_method("Close", &()).map(|_| ()));
        if let Err(e) = closed {
            tracing::debug!(error = %e, "share: the portal session did not close");
        }
    }
}

impl Capture for PortalCapture {
    fn open(&self, req: &CaptureRequest) -> Result<Box<dyn VideoSource>, CaptureError> {
        let conn = Connection::session().map_err(failed)?;
        let sc = Proxy::new(&conn, DEST, PATH, SCREENCAST).map_err(failed)?;
        let types: u32 = sc
            .get_property("AvailableSourceTypes")
            .map_err(|e| failed(format!("no ScreenCast portal: {e}")))?;
        if types & WINDOW == 0 {
            return Err(failed("this desktop cannot share a single window"));
        }
        let cursors: u32 = sc.get_property("AvailableCursorModes").unwrap_or(0);

        let token = self.token();
        let mut opts: HashMap<&str, Value<'_>> = HashMap::new();
        opts.insert("handle_token", Value::from(token.as_str()));
        let session_token = self.token();
        opts.insert("session_handle_token", Value::from(session_token.as_str()));
        let r = request(&conn, "CreateSession", &token, || {
            sc.call("CreateSession", &(opts,))
        })?;
        let handle: String = take(&r, "session_handle").ok_or_else(|| failed("no session"))?;
        let handle = OwnedObjectPath::try_from(handle).map_err(failed)?;
        let session = Session {
            conn: conn.clone(),
            handle,
        };
        let path = ObjectPath::from(&session.handle);

        let token = self.token();
        let restore = self.remembered(&req.game);
        let mut opts: HashMap<&str, Value<'_>> = HashMap::new();
        opts.insert("handle_token", Value::from(token.as_str()));
        opts.insert("types", Value::from(WINDOW));
        opts.insert("multiple", Value::from(false));
        if cursors & CURSOR_EMBEDDED != 0 {
            opts.insert("cursor_mode", Value::from(CURSOR_EMBEDDED));
        }
        opts.insert("persist_mode", Value::from(PERSIST));
        if let Some(t) = &restore {
            opts.insert("restore_token", Value::from(t.as_str()));
        }
        request(&conn, "SelectSources", &token, || {
            sc.call("SelectSources", &(&path, opts))
        })?;

        let token = self.token();
        let mut opts: HashMap<&str, Value<'_>> = HashMap::new();
        opts.insert("handle_token", Value::from(token.as_str()));
        let r = request(&conn, "Start", &token, || {
            sc.call("Start", &(&path, "", opts))
        })?;
        if let Some(t) = take::<String>(&r, "restore_token") {
            self.remember(&req.game, t);
        }
        let streams: Vec<(u32, HashMap<String, OwnedValue>)> =
            take(&r, "streams").ok_or_else(|| failed("the portal gave no stream"))?;
        let [(node, props)] = streams.as_slice() else {
            return Err(failed("the portal gave more than one stream"));
        };
        // Never a screen: a stream that says it is something else is refused.
        let kind: Option<u32> = props.get("source_type").and_then(|v| u32::try_from(v).ok());
        if kind.is_some_and(|k| k != WINDOW) {
            return Err(CaptureError::Refused(
                "the portal offered a screen, not a window".into(),
            ));
        }
        let mut opts: HashMap<&str, Value<'_>> = HashMap::new();
        opts.insert("handle_token", Value::from(self.token()));
        let fd: zbus::zvariant::OwnedFd = sc
            .call("OpenPipeWireRemote", &(&path, opts))
            .map_err(failed)?;
        let fd: OwnedFd = fd.into();
        let source = PipeWireSource::open(fd, *node).map_err(failed)?;
        tracing::info!(node, "share: the game's window is captured");
        Ok(Box::new(Captured {
            source,
            _session: session,
        }))
    }
}

/// The picture source and the portal session it depends on.
struct Captured {
    source: PipeWireSource,
    _session: Session,
}

impl VideoSource for Captured {
    fn next(
        &mut self,
        pic: &mut crate::video::i420::I420,
        max_height: Option<u32>,
        timeout: std::time::Duration,
    ) -> crate::video::source::Next {
        self.source.next(pic, max_height, timeout)
    }
}

/// Call `method` and wait for its `Response`: 0 is success, 1 the user cancelled, 2 other.
fn request(
    conn: &Connection,
    method: &str,
    token: &str,
    call: impl FnOnce() -> zbus::Result<OwnedObjectPath>,
) -> Result<HashMap<String, OwnedValue>, CaptureError> {
    let sender = conn
        .unique_name()
        .map(|n| n.trim_start_matches(':').replace('.', "_"))
        .ok_or_else(|| failed("no name on the bus"))?;
    let path = format!("{PATH}/request/{sender}/{token}");
    let req =
        Proxy::new(conn, DEST, path.as_str(), "org.freedesktop.portal.Request").map_err(failed)?;
    // Listen before asking, so a quick answer is not missed.
    let mut answers = req.receive_signal("Response").map_err(failed)?;
    call().map_err(failed)?;
    let msg = answers
        .next()
        .ok_or_else(|| failed("the portal went away"))?;
    let (code, results): (u32, HashMap<String, OwnedValue>) =
        msg.body().deserialize().map_err(failed)?;
    match code {
        0 => Ok(results),
        1 => Err(CaptureError::Refused(format!("{method}: cancelled"))),
        _ => Err(CaptureError::Refused(format!("{method}: refused ({code})"))),
    }
}

/// A value from a portal answer.
fn take<T>(r: &HashMap<String, OwnedValue>, key: &str) -> Option<T>
where
    T: TryFrom<OwnedValue>,
{
    r.get(key)
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| T::try_from(v).ok())
}
