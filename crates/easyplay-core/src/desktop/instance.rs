//! Single instance: the protocol on `<run>/instance.sock` and its server and client
//! (DESKTOP_INTEGRATION §5).
//!
//! One request per connection; messages are single JSON objects terminated by a newline,
//! at most [`MAX_MESSAGE`] bytes. The server accepts only peers with the same UID
//! (`SO_PEERCRED`), reads with a 2 s timeout, serves at most [`MAX_CONNECTIONS`] connections
//! at once, resolves relative paths against the request's `cwd` without touching the
//! filesystem, and hands every valid request to a [`RequestHandler`] that must not block on
//! the AppService (it queues commands).
//!
//! Becoming the primary instance ([`claim`]): take `flock` on `<run>/gui.lock` (exclusive,
//! non-blocking), remove a stale socket, bind, `chmod 0600`, listen. A process that loses the
//! race waits up to 2 s for the winner's socket and forwards to it.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode};
use crate::fsx::{Fs, RealFs};
use crate::paths::AppPaths;
use crate::storage::StorageError;
use crate::storage::lock::{LibraryLock, LockMode};

/// Protocol version (`"v"`).
pub const PROTOCOL_VERSION: u32 = 1;
/// Largest message in bytes, newline included.
pub const MAX_MESSAGE: usize = 64 * 1024;
/// Most connections served at once; more are refused.
pub const MAX_CONNECTIONS: usize = 16;
/// How long the server waits for a request line.
pub const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a client waits for `ping` to be answered before treating the GUI as absent.
pub const PING_TIMEOUT: Duration = Duration::from_millis(300);
/// How long a process that lost the start race waits for the winner's socket.
pub const RACE_WAIT: Duration = Duration::from_secs(2);
/// How long a client waits for a reply to anything but `ping`.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// `<run>/gui.lock`.
pub fn gui_lock_path(paths: &AppPaths) -> PathBuf {
    paths.runtime_dir.join("gui.lock")
}

/// Where opened paths come from, on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireOrigin {
    /// A file manager (`easyplay open %F`).
    FileManager,
    /// `easyplay open` typed in a terminal.
    Cli,
    /// `easyplay add`: show the setup sheet, never start a known game.
    Add,
}

/// What to do after raising.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RaiseThen {
    /// Open the add-game picker (`easyplay --add`).
    AddGame,
}

/// What `easyplay overlay` asks of the in-game overlay (ADR-0037).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayVerb {
    Toggle,
    Open,
    Close,
}

/// A request (the `op` field selects the variant).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Open programs.
    Open {
        /// Paths (relative ones are resolved against `cwd`).
        paths: Vec<PathBuf>,
        /// Origin.
        origin: WireOrigin,
        /// The client's working directory.
        #[serde(default)]
        cwd: Option<PathBuf>,
        /// Desktop activation token of the client.
        #[serde(default)]
        activation_token: Option<String>,
    },
    /// Launch a game.
    Launch {
        /// `<GAME>` (ID, ID prefix or name).
        game: String,
        /// Program label or ID.
        #[serde(default)]
        exe: Option<String>,
        /// Extra arguments for this run.
        #[serde(default)]
        extra_args: Vec<String>,
        /// Stream state events until the session ends.
        #[serde(default)]
        wait: bool,
    },
    /// Stop a game.
    Stop {
        /// `<GAME>`.
        game: String,
        /// Force.
        #[serde(default)]
        force: bool,
    },
    /// Raise the window.
    Raise {
        /// Desktop activation token of the client.
        #[serde(default)]
        activation_token: Option<String>,
        /// What to do afterwards.
        #[serde(default)]
        then: Option<RaiseThen>,
    },
    /// Liveness check.
    Ping,
    /// Open or close the in-game overlay (the hotkey; ADR-0037).
    Overlay { action: OverlayVerb },
}

impl Request {
    /// The `op` name (logs).
    pub fn op(&self) -> &'static str {
        match self {
            Request::Open { .. } => "open",
            Request::Launch { .. } => "launch",
            Request::Stop { .. } => "stop",
            Request::Raise { .. } => "raise",
            Request::Ping => "ping",
            Request::Overlay { .. } => "overlay",
        }
    }
}

#[derive(Serialize)]
struct OutEnvelope<'a> {
    v: u32,
    #[serde(flatten)]
    req: &'a Request,
}

#[derive(Deserialize)]
struct InEnvelope {
    v: u32,
    #[serde(flatten)]
    req: serde_json::Value,
}

/// The reply to a request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    /// Accepted.
    pub ok: bool,
    /// Error code (`EP-…`) when not accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Message when not accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Response {
    /// `{"ok":true}`.
    pub fn ok() -> Self {
        Self {
            ok: true,
            code: None,
            message: None,
        }
    }

    /// `{"ok":false,…}` for `err`.
    pub fn error(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            code: Some(code.as_str().to_owned()),
            message: Some(message.into()),
        }
    }
}

/// Streamed after an accepted `launch` with `wait: true`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum StreamEvent {
    /// The session reached a state (`"running"`).
    State {
        /// State name.
        state: String,
    },
    /// The session ended.
    Ended {
        /// End reason (`EndReason::as_str`, or `failed_to_start`).
        reason: String,
        /// Exit status of the main program, when known.
        #[serde(default)]
        exit_code: Option<i32>,
    },
}

/// Encode a request as one line.
pub fn encode_request(req: &Request) -> Vec<u8> {
    let mut out = serde_json::to_vec(&OutEnvelope {
        v: PROTOCOL_VERSION,
        req,
    })
    .unwrap_or_default();
    out.push(b'\n');
    out
}

/// Why a message was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    /// Longer than [`MAX_MESSAGE`].
    #[error("message too large")]
    TooLarge,
    /// Not JSON, or not a known request.
    #[error("malformed message: {0}")]
    Malformed(String),
    /// Unknown `v`.
    #[error("unsupported protocol version {0}")]
    Version(u32),
    /// The connection closed or timed out before a full line arrived.
    #[error("incomplete message")]
    Incomplete,
}

/// Decode one request line (without or with its newline).
pub fn decode_request(line: &[u8]) -> Result<Request, ProtocolError> {
    if line.len() > MAX_MESSAGE {
        return Err(ProtocolError::TooLarge);
    }
    let env: InEnvelope =
        serde_json::from_slice(line).map_err(|e| ProtocolError::Malformed(e.to_string()))?;
    if env.v != PROTOCOL_VERSION {
        return Err(ProtocolError::Version(env.v));
    }
    serde_json::from_value(env.req).map_err(|e| ProtocolError::Malformed(e.to_string()))
}

/// Read one newline-terminated line of at most [`MAX_MESSAGE`] bytes.
pub fn read_line(r: &mut impl BufRead) -> Result<Vec<u8>, ProtocolError> {
    let mut buf = Vec::new();
    let n = r
        .take(MAX_MESSAGE as u64 + 1)
        .read_until(b'\n', &mut buf)
        .map_err(|_| ProtocolError::Incomplete)?;
    if n == 0 {
        return Err(ProtocolError::Incomplete);
    }
    if buf.last() != Some(&b'\n') {
        return Err(if buf.len() > MAX_MESSAGE {
            ProtocolError::TooLarge
        } else {
            ProtocolError::Incomplete
        });
    }
    buf.pop();
    Ok(buf)
}

/// Rule 3: relative paths against `cwd`; afterwards every path must be absolute. Nothing is
/// opened or stat-ed; `..` components are folded lexically.
pub fn resolve_paths(paths: &[PathBuf], cwd: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    paths
        .iter()
        .map(|p| {
            let abs = if p.is_absolute() {
                p.clone()
            } else {
                match cwd.filter(|c| c.is_absolute()) {
                    Some(c) => c.join(p),
                    None => return Err(format!("relative path without cwd: {}", p.display())),
                }
            };
            Ok(crate::paths::normalize(&abs))
        })
        .collect()
}

/// Writes replies on one connection.
pub trait Replier {
    /// Send the final reply (or the acceptance of a streamed request).
    fn reply(&mut self, response: &Response);
    /// Send one streamed event; returns `false` when the client is gone.
    fn event(&mut self, event: &StreamEvent) -> bool;
}

/// Turns requests into commands; runs on a connection thread, never blocks on the service.
pub trait RequestHandler: Send + Sync {
    /// Handle one validated request (paths already absolute).
    fn handle(&self, request: Request, reply: &mut dyn Replier);
}

struct StreamReplier<'a> {
    stream: &'a UnixStream,
}

impl Replier for StreamReplier<'_> {
    fn reply(&mut self, response: &Response) {
        let mut line = serde_json::to_vec(response).unwrap_or_default();
        line.push(b'\n');
        let _ = (&*self.stream).write_all(&line);
    }

    fn event(&mut self, event: &StreamEvent) -> bool {
        let mut line = serde_json::to_vec(event).unwrap_or_default();
        line.push(b'\n');
        (&*self.stream).write_all(&line).is_ok()
    }
}

/// Validate and dispatch one decoded request (shared by the socket server and the in-process
/// transport).
pub fn dispatch(handler: &dyn RequestHandler, request: Request, reply: &mut dyn Replier) {
    let request = match request {
        Request::Open {
            paths,
            origin,
            cwd,
            activation_token,
        } => match resolve_paths(&paths, cwd.as_deref()) {
            Ok(paths) => Request::Open {
                paths,
                origin,
                cwd,
                activation_token,
            },
            Err(m) => return reply.reply(&Response::error(ErrorCode::IntTaskFailed, m)),
        },
        Request::Ping => return reply.reply(&Response::ok()),
        other => other,
    };
    handler.handle(request, reply);
}

fn serve_connection(stream: &UnixStream, uid: u32, handler: &dyn RequestHandler) {
    let mut out = StreamReplier { stream };
    // Rule 1: same UID only (the socket is 0600 inside a 0700 directory as well).
    match rustix::net::sockopt::socket_peercred(stream) {
        Ok(c) if c.uid.as_raw() == uid => {}
        Ok(c) => {
            tracing::warn!(
                peer_uid = c.uid.as_raw(),
                "instance socket: foreign peer refused"
            );
            return;
        }
        Err(e) => {
            tracing::warn!(error = %e, "instance socket: no peer credentials");
            return;
        }
    }
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(READ_TIMEOUT));
    let mut r = BufReader::new(stream);
    let req = read_line(&mut r).and_then(|l| decode_request(&l));
    match req {
        Ok(req) => {
            tracing::debug!(op = req.op(), "instance request");
            dispatch(handler, req, &mut out);
        }
        Err(e) => {
            tracing::info!(error = %e, "instance request refused");
            out.reply(&Response::error(ErrorCode::IntTaskFailed, e.to_string()));
        }
    }
}

/// The running server; dropping it stops the listener and removes the socket.
pub struct InstanceServer {
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _lock: LibraryLock,
}

impl std::fmt::Debug for InstanceServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstanceServer")
            .field("socket", &self.socket)
            .finish_non_exhaustive()
    }
}

impl InstanceServer {
    /// The socket path.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Start serving `handler` on the bound listener.
    fn start(
        listener: UnixListener,
        socket: PathBuf,
        lock: LibraryLock,
        uid: u32,
        handler: Arc<dyn RequestHandler>,
    ) -> Result<Self, Error> {
        let stop = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicUsize::new(0));
        let stop2 = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("instance-ipc".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    if stop2.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = conn else { continue };
                    // Rule 5: at most 16 at once; the excess is refused by closing.
                    if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                        active.fetch_sub(1, Ordering::SeqCst);
                        tracing::warn!("instance socket: too many connections");
                        continue;
                    }
                    let h = Arc::clone(&handler);
                    let a = Arc::clone(&active);
                    let spawned = std::thread::Builder::new()
                        .name("instance-conn".into())
                        .spawn(move || {
                            serve_connection(&stream, uid, &*h);
                            a.fetch_sub(1, Ordering::SeqCst);
                        });
                    if spawned.is_err() {
                        active.fetch_sub(1, Ordering::SeqCst);
                    }
                }
            })
            .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with_source(e))?;
        Ok(Self {
            socket,
            stop,
            thread: Some(thread),
            _lock: lock,
        })
    }
}

impl Drop for InstanceServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake `accept`.
        let _ = UnixStream::connect(&self.socket);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let _ = RealFs::new().remove_file(&self.socket);
    }
}

/// Sends requests to the primary instance.
pub trait InstanceTransport {
    /// Whether a primary instance answers `ping` within [`PING_TIMEOUT`].
    fn ping(&self) -> bool;
    /// Send `request`; streamed events go to `on_event`. Errors mean "not delivered", or
    /// that a stream broke before its final event (the instance went away).
    fn send(
        &self,
        request: &Request,
        on_event: &mut dyn FnMut(&StreamEvent),
    ) -> Result<Response, Error>;
}

/// The Unix-socket transport.
#[derive(Clone, Debug)]
pub struct SocketTransport {
    socket: PathBuf,
}

impl SocketTransport {
    /// A client of the socket at `socket`.
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
        }
    }

    fn connect(&self, timeout: Duration) -> std::io::Result<UnixStream> {
        let s = UnixStream::connect(&self.socket)?;
        s.set_read_timeout(Some(timeout))?;
        s.set_write_timeout(Some(timeout))?;
        Ok(s)
    }
}

fn unresponsive(e: impl std::fmt::Display) -> Error {
    Error::new(ErrorCode::DskInstanceUnresponsive).with("detail", e.to_string())
}

impl InstanceTransport for SocketTransport {
    fn ping(&self) -> bool {
        let Ok(mut s) = self.connect(PING_TIMEOUT) else {
            return false;
        };
        if s.write_all(&encode_request(&Request::Ping)).is_err() {
            return false;
        }
        let mut r = BufReader::new(&s);
        read_line(&mut r)
            .ok()
            .and_then(|l| serde_json::from_slice::<Response>(&l).ok())
            .is_some_and(|r| r.ok)
    }

    fn send(
        &self,
        request: &Request,
        on_event: &mut dyn FnMut(&StreamEvent),
    ) -> Result<Response, Error> {
        let mut s = self.connect(REPLY_TIMEOUT).map_err(unresponsive)?;
        s.write_all(&encode_request(request))
            .map_err(unresponsive)?;
        let mut r = BufReader::new(&s);
        let line = read_line(&mut r).map_err(unresponsive)?;
        let resp: Response = serde_json::from_slice(&line).map_err(unresponsive)?;
        let streams = matches!(request, Request::Launch { wait: true, .. });
        if resp.ok && streams {
            // A game session may run for hours.
            let _ = s.set_read_timeout(None);
            loop {
                // The instance closing is not the session ending: the game runs on.
                let line = read_line(&mut r).map_err(unresponsive)?;
                let ev: StreamEvent = serde_json::from_slice(&line).map_err(unresponsive)?;
                let end = matches!(ev, StreamEvent::Ended { .. });
                on_event(&ev);
                if end {
                    break;
                }
            }
        }
        Ok(resp)
    }
}

/// A transport that calls a handler in the same process (tests).
pub struct InProcessTransport {
    /// The handler; `None` behaves like "no GUI running".
    pub handler: Option<Arc<dyn RequestHandler>>,
}

#[derive(Default)]
struct Collect {
    response: Option<Response>,
    events: Vec<StreamEvent>,
}

impl Replier for Collect {
    fn reply(&mut self, response: &Response) {
        self.response = Some(response.clone());
    }
    fn event(&mut self, event: &StreamEvent) -> bool {
        self.events.push(event.clone());
        true
    }
}

impl InstanceTransport for InProcessTransport {
    fn ping(&self) -> bool {
        self.handler.is_some()
    }

    fn send(
        &self,
        request: &Request,
        on_event: &mut dyn FnMut(&StreamEvent),
    ) -> Result<Response, Error> {
        let h = self
            .handler
            .as_ref()
            .ok_or_else(|| unresponsive("no instance"))?;
        // Through the wire format, so encoding bugs show up here too.
        let req = decode_request(&encode_request(request))
            .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with("detail", e.to_string()))?;
        let mut c = Collect::default();
        dispatch(&**h, req, &mut c);
        for e in &c.events {
            on_event(e);
        }
        c.response
            .ok_or_else(|| unresponsive("the handler did not reply"))
    }
}

/// Result of [`claim`].
pub enum Claim {
    /// This process is the primary instance; serve requests until dropped.
    Primary(InstanceServer),
    /// Another instance is running; forward to it.
    Forward(SocketTransport),
}

impl std::fmt::Debug for Claim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Claim::Primary(s) => write!(f, "Primary({})", s.socket().display()),
            Claim::Forward(t) => write!(f, "Forward({})", t.socket.display()),
        }
    }
}

/// Become the primary instance, or find the one that is (DESKTOP_INTEGRATION §5.1).
/// `handler` is only used when this process becomes primary.
pub fn claim(
    paths: &AppPaths,
    uid: u32,
    handler: impl FnOnce() -> Arc<dyn RequestHandler>,
) -> Result<Claim, Error> {
    let client = SocketTransport::new(&paths.instance_socket);
    if client.ping() {
        return Ok(Claim::Forward(client));
    }
    let dir = &paths.runtime_dir;
    let fs = RealFs::new();
    fs.create_dir_all(dir, 0o700)?;
    let lock = match LibraryLock::try_acquire(&gui_lock_path(paths), LockMode::Exclusive) {
        Ok(l) => l,
        Err(StorageError::LockHeld) => {
            // Another process is starting: wait for its socket.
            let step = Duration::from_millis(50);
            for _ in 0..(RACE_WAIT.as_millis() / step.as_millis()) {
                if client.ping() {
                    return Ok(Claim::Forward(client));
                }
                std::thread::sleep(step);
            }
            return Err(unresponsive(
                "another EasyPlay is starting but does not answer",
            ));
        }
        Err(e) => return Err(e.into()),
    };
    // We hold the lock, so any socket file is stale.
    let _ = fs.remove_file(&paths.instance_socket);
    let listener = UnixListener::bind(&paths.instance_socket)
        .map_err(|e| Error::new(ErrorCode::FsWriteDenied).with_source(e))?;
    fs.set_mode(&paths.instance_socket, 0o600)?;
    let server = InstanceServer::start(
        listener,
        paths.instance_socket.clone(),
        lock,
        uid,
        handler(),
    )?;
    Ok(Claim::Primary(server))
}

/// The client's activation token from its environment (`XDG_ACTIVATION_TOKEN` on Wayland,
/// `DESKTOP_STARTUP_ID` on X11).
pub fn activation_token(env: &crate::paths::PathEnv) -> Option<String> {
    ["XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"]
        .iter()
        .filter_map(|k| env.get(k))
        .filter_map(|v| v.to_str())
        .find(|v| !v.is_empty() && v.len() <= 1024)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn requests_round_trip_in_the_documented_shape() {
        let r = Request::Open {
            paths: vec!["/mnt/games/Hollow Knight/hollow_knight.exe".into()],
            origin: WireOrigin::FileManager,
            cwd: Some("/home/sam".into()),
            activation_token: Some("tok".into()),
        };
        let line = encode_request(&r);
        let text = String::from_utf8(line.clone()).unwrap();
        assert!(
            text.starts_with(r#"{"v":1,"op":"open","paths":["#),
            "{text}"
        );
        assert!(text.contains(r#""origin":"file_manager""#));
        assert!(text.ends_with('\n'));
        assert_eq!(decode_request(&line[..line.len() - 1]).unwrap(), r);
        for l in [
            r#"{"v":1,"op":"launch","game":"0192f0c2","exe":null,"extra_args":[],"wait":false}"#,
            r#"{"v":1,"op":"stop","game":"hollow knight","force":false}"#,
            r#"{"v":1,"op":"raise","activation_token":"x","then":"add_game"}"#,
            r#"{"v":1,"op":"ping"}"#,
            r#"{"v":1,"op":"overlay","action":"toggle"}"#,
        ] {
            decode_request(l.as_bytes()).unwrap();
        }
    }

    #[test]
    fn bad_messages_are_refused() {
        assert_eq!(
            decode_request(br#"{"v":2,"op":"ping"}"#),
            Err(ProtocolError::Version(2))
        );
        assert!(matches!(
            decode_request(br#"{"v":1,"op":"format_disk"}"#),
            Err(ProtocolError::Malformed(_))
        ));
        assert!(matches!(
            decode_request(b"not json"),
            Err(ProtocolError::Malformed(_))
        ));
        let big = vec![b'a'; MAX_MESSAGE + 10];
        assert_eq!(
            read_line(&mut &big[..]).unwrap_err(),
            ProtocolError::TooLarge
        );
        assert_eq!(
            read_line(&mut &b"{\"v\":1"[..]).unwrap_err(),
            ProtocolError::Incomplete
        );
    }

    #[test]
    fn paths_resolve_against_cwd_without_io() {
        let r = resolve_paths(
            &["a.exe".into(), "../b/c.exe".into(), "/abs.exe".into()],
            Some(Path::new("/home/sam/games")),
        )
        .unwrap();
        assert_eq!(
            r,
            vec![
                PathBuf::from("/home/sam/games/a.exe"),
                PathBuf::from("/home/sam/b/c.exe"),
                PathBuf::from("/abs.exe")
            ]
        );
        assert!(resolve_paths(&["a.exe".into()], None).is_err());
        assert!(resolve_paths(&["a.exe".into()], Some(Path::new("rel"))).is_err());
    }

    struct Recorder(Mutex<Vec<Request>>);
    impl RequestHandler for Recorder {
        fn handle(&self, request: Request, reply: &mut dyn Replier) {
            let wait = matches!(request, Request::Launch { wait: true, .. });
            self.0.lock().unwrap().push(request);
            reply.reply(&Response::ok());
            if wait {
                reply.event(&StreamEvent::State {
                    state: "running".into(),
                });
                reply.event(&StreamEvent::Ended {
                    reason: "exited".into(),
                    exit_code: Some(0),
                });
            }
        }
    }

    #[test]
    fn in_process_transport_dispatches() {
        let rec = Arc::new(Recorder(Mutex::new(Vec::new())));
        let t = InProcessTransport {
            handler: Some(rec.clone()),
        };
        assert!(t.ping());
        let mut evs = Vec::new();
        let r = t
            .send(
                &Request::Open {
                    paths: vec!["x.exe".into()],
                    origin: WireOrigin::Cli,
                    cwd: Some("/w".into()),
                    activation_token: None,
                },
                &mut |e| evs.push(e.clone()),
            )
            .unwrap();
        assert!(r.ok);
        assert_eq!(
            rec.0.lock().unwrap()[0],
            Request::Open {
                paths: vec!["/w/x.exe".into()],
                origin: WireOrigin::Cli,
                cwd: Some("/w".into()),
                activation_token: None,
            }
        );
        let none = InProcessTransport { handler: None };
        assert!(!none.ping());
    }
}
