//! `zaun-chat`: hosts the SimpleX chat engine (`libsimplex`) for Zaun's Chat
//! page (ADR-0033) and speaks to Zaun over its standard input and output only.
//!
//! The engine's own API server (a WebSocket on localhost) has no authentication, and games
//! with network access share the host's localhost, so Zaun never opens it: this helper
//! loads the library the official apps use and relays its C interface to pipes. It holds no
//! logic of its own; the protocol is in `zaun_core::chat::simplex`.
//!
//! **Protocol** (one JSON object per line, UTF-8):
//!
//! - in, first line: `{"v":1,"lib":"<path to libsimplex.so>","db":"<database prefix>","key":"<key>"}`.
//!   The key never travels on the command line or in the environment.
//! - out: `{"t":"ready"}` once the database is open; `{"t":"init_error","body":<json>}` (exit
//!   3) when it cannot be opened; `{"t":"fatal","msg":"<text>"}` (exit 4) when the library
//!   cannot be loaded or the first line is not understood.
//! - in, then: `{"corr":<n>,"cmd":"<chat command>"}`; out: `{"t":"resp","corr":<n>,"body":<json>}`.
//!   Commands run one after another in the order given, except one marked `"bg":true`,
//!   which runs on a thread of its own (a server test can wait for a network timeout and
//!   must not hold up messages); its answer comes whenever it is done.
//! - out, at any time: `{"t":"event","body":<json>}` for what the engine reports by itself.
//! - End of input closes the database and exits 0.
//!
//! Files are created with mode 0600 (`umask 077`): the engine would otherwise create its
//! databases readable by every local user.

// The FFI to libsimplex is the fourth place allowed to use `unsafe` (ARCHITECTURE §3 rule 5).
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_long, c_void};
use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Protocol version of the first line.
const PROTOCOL: u32 = 1;
/// How long one wait for an engine event lasts, in microseconds.
const RECV_WAIT_US: c_int = 1_000_000;
/// How long the end of input waits for commands running beside, in milliseconds.
const BG_GRACE_MS: u64 = 3000;
/// Exit status: the database could not be opened.
const EXIT_INIT: u8 = 3;
/// Exit status: the library could not be loaded, or the first line was not understood.
const EXIT_FATAL: u8 = 4;
/// GHC runtime options, as the official desktop app passes them (`simplex-api.c`).
const RTS_ARGS: [&str; 5] = ["zaun-chat", "+RTS", "-A64m", "-H64m", "-xn"];

type Ctrl = *mut c_long;
type HsInit = unsafe extern "C" fn(*mut c_int, *mut *mut *mut c_char);
type MigrateInitKey = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    c_int,
    *const c_char,
    c_int,
    *mut Ctrl,
) -> *mut c_char;
type SendCmd = unsafe extern "C" fn(Ctrl, *const c_char) -> *mut c_char;
type RecvMsgWait = unsafe extern "C" fn(Ctrl, c_int) -> *mut c_char;
type CloseStore = unsafe extern "C" fn(Ctrl) -> *mut c_char;

#[derive(serde::Deserialize)]
struct Init {
    v: u32,
    lib: String,
    db: String,
    key: String,
}

#[derive(serde::Deserialize)]
struct Request {
    corr: u64,
    cmd: String,
    /// Run on a thread of its own, beside the commands in order.
    #[serde(default)]
    bg: bool,
}

/// The engine's functions, resolved once.
#[derive(Clone, Copy)]
struct Api {
    send: SendCmd,
    recv: RecvMsgWait,
    close: CloseStore,
}

/// The controller pointer, shared by the command loop and the event thread. The engine's
/// C interface is used this way by every official client (one thread waits for events while
/// others send commands).
#[derive(Clone, Copy)]
struct Controller(Ctrl);
// SAFETY: the pointer is an opaque Haskell stable pointer; the engine synchronises access
// to the controller internally, and it is never freed while the process runs.
unsafe impl Send for Controller {}
// SAFETY: as above; the pointer itself is never written after initialisation.
unsafe impl Sync for Controller {}

impl Controller {
    /// The pointer (a method, so closures capture the `Send` wrapper and not the field).
    fn ptr(self) -> Ctrl {
        self.0
    }
}

/// Writes whole lines to standard output from both threads.
#[derive(Clone)]
struct Out(Arc<Mutex<()>>);

impl Out {
    fn line(&self, s: &str) {
        let _g = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut o = std::io::stdout().lock();
        // A closed output means Zaun is gone; the next read of stdin ends the process.
        let _ = writeln!(o, "{s}");
        let _ = o.flush();
    }

    /// `{"t":<tag>[,"corr":n],"body":<json>}`, where `body` is the engine's JSON as is when
    /// it is valid JSON, and otherwise a string.
    fn body(&self, tag: &str, corr: Option<u64>, json: &str) {
        let body = if serde_json::from_str::<serde::de::IgnoredAny>(json).is_ok() {
            json.replace('\n', " ")
        } else {
            serde_json::Value::String(json.to_owned()).to_string()
        };
        let corr = corr.map(|c| format!(",\"corr\":{c}")).unwrap_or_default();
        self.line(&format!("{{\"t\":\"{tag}\"{corr},\"body\":{body}}}"));
    }

    fn fatal(&self, msg: &str) -> ExitCode {
        let v = serde_json::json!({ "t": "fatal", "msg": msg });
        self.line(&v.to_string());
        ExitCode::from(EXIT_FATAL)
    }
}

/// Copies a string the engine allocated with `malloc`, then frees it.
fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: the engine returns NUL-terminated strings it allocated with malloc.
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    // SAFETY: ownership of the allocation passed to us; it is freed exactly once.
    unsafe { libc::free(p.cast::<c_void>()) };
    s
}

fn symbol<T: Copy>(lib: &libloading::Library, name: &[u8]) -> Result<T, String> {
    // SAFETY: each name is looked up with the signature the engine exports
    // (`Simplex.Chat.Mobile`, `foreign export ccall`); the library is never unloaded.
    unsafe { lib.get::<T>(name) }
        .map(|s| *s)
        .map_err(|e| format!("{}: {e}", String::from_utf8_lossy(name)))
}

type Lines = std::io::Lines<std::io::StdinLock<'static>>;

fn main() -> ExitCode {
    // SAFETY: umask only changes this process's file-creation mask.
    unsafe { libc::umask(0o077) };
    let out = Out(Arc::new(Mutex::new(())));
    match run(&out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

/// The helper's life: read the first line, load the library, start the engine, serve
/// commands until input ends, close the database. An `Err` is the exit status.
fn run(out: &Out) -> Result<(), ExitCode> {
    let mut lines = std::io::stdin().lock().lines();
    let init = read_init(out, &mut lines)?;
    let (hs_init, migrate, api) = load_library(out, &init.lib)?;
    // `args` and `argv` must stay alive as long as the runtime might read them.
    let _rts = init_runtime(out, hs_init)?;
    let ctrl = open_database(out, &migrate, init.db, init.key)?;
    out.line("{\"t\":\"ready\"}");
    spawn_event_thread(out, api, ctrl)?;
    let running = serve(out, api, ctrl, lines);
    // Commands still running beside get a moment to finish before the database closes.
    let mut waited = 0;
    while running.load(Ordering::SeqCst) > 0 && waited < BG_GRACE_MS {
        std::thread::sleep(std::time::Duration::from_millis(50));
        waited += 50;
    }
    // SAFETY: the controller is valid; no command runs after this.
    let _ = take(unsafe { (api.close)(ctrl.ptr()) });
    Ok(())
}

fn read_init(out: &Out, lines: &mut Lines) -> Result<Init, ExitCode> {
    let Some(Ok(first)) = lines.next() else {
        return Err(out.fatal("no first line"));
    };
    let init: Init =
        serde_json::from_str(&first).map_err(|e| out.fatal(&format!("first line: {e}")))?;
    if init.v != PROTOCOL {
        return Err(out.fatal(&format!("protocol {} is not {PROTOCOL}", init.v)));
    }
    Ok(init)
}

/// Load `libsimplex` and resolve the functions the helper uses.
fn load_library(out: &Out, path: &str) -> Result<(HsInit, MigrateInitKey, Api), ExitCode> {
    // SAFETY: loading the library runs its initialisers (the GHC runtime's constructors);
    // this is the library's documented use.
    let lib = unsafe { libloading::Library::new(path) }
        .map_err(|e| out.fatal(&format!("loading {path}: {e}")))?;
    let resolved = (|| -> Result<(HsInit, MigrateInitKey, Api), String> {
        Ok((
            symbol(&lib, b"hs_init_with_rtsopts\0")?,
            symbol(&lib, b"chat_migrate_init_key\0")?,
            Api {
                send: symbol(&lib, b"chat_send_cmd\0")?,
                recv: symbol(&lib, b"chat_recv_msg_wait\0")?,
                close: symbol(&lib, b"chat_close_store\0")?,
            },
        ))
    })()
    .map_err(|e| out.fatal(&e))?;
    // The functions above stay valid for the life of the process.
    std::mem::forget(lib);
    Ok(resolved)
}

/// The runtime's arguments, kept alive after `hs_init` returns.
struct RtsArgs {
    _args: Vec<CString>,
    _argv: Vec<*mut c_char>,
}

fn init_runtime(out: &Out, hs_init: HsInit) -> Result<RtsArgs, ExitCode> {
    let args = RTS_ARGS
        .iter()
        .map(|a| CString::new(*a))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| out.fatal("runtime options"))?;
    let mut argv: Vec<*mut c_char> = args
        .iter()
        .map(|a| a.as_ptr().cast_mut())
        .chain([std::ptr::null_mut()])
        .collect();
    let mut argc = c_int::try_from(args.len()).unwrap_or(0);
    let mut pargv = argv.as_mut_ptr();
    // SAFETY: argc/argv describe NUL-terminated strings that outlive the call (`args` is kept
    // in the returned `RtsArgs`); the runtime is initialised once, before any other engine call.
    unsafe { hs_init(&mut argc, &mut pargv) };
    Ok(RtsArgs {
        _args: args,
        _argv: argv,
    })
}

/// Open (and migrate) the encrypted database; its controller, or the `init_error` exit.
fn open_database(
    out: &Out,
    migrate: &MigrateInitKey,
    db: String,
    key: String,
) -> Result<Controller, ExitCode> {
    let (Ok(db), Ok(key), Ok(confirm)) =
        (CString::new(db), CString::new(key), CString::new("yesUp"))
    else {
        return Err(out.fatal("a NUL byte in the first line"));
    };
    let mut ctrl: Ctrl = std::ptr::null_mut();
    // SAFETY: all strings are NUL-terminated and live across the call; `ctrl` receives the
    // controller on success.
    let r = take(unsafe { migrate(db.as_ptr(), key.as_ptr(), 0, confirm.as_ptr(), 0, &mut ctrl) });
    let ok = serde_json::from_str::<serde_json::Value>(&r)
        .is_ok_and(|v| v.get("type").and_then(|t| t.as_str()) == Some("ok"));
    if !ok || ctrl.is_null() {
        out.body("init_error", None, &r);
        return Err(ExitCode::from(EXIT_INIT));
    }
    Ok(Controller(ctrl))
}

/// What the engine reports by itself goes out as `event` lines.
fn spawn_event_thread(out: &Out, api: Api, ctrl: Controller) -> Result<(), ExitCode> {
    let ev_out = out.clone();
    std::thread::Builder::new()
        .name("engine-events".into())
        .spawn(move || {
            loop {
                // SAFETY: the controller is valid for the life of the process.
                let ev = take(unsafe { (api.recv)(ctrl.ptr(), RECV_WAIT_US) });
                if !ev.is_empty() {
                    ev_out.body("event", None, &ev);
                }
            }
        })
        .map_err(|e| out.fatal(&format!("event thread: {e}")))?;
    Ok(())
}

/// Run commands from input until it ends; the count of those still running beside.
fn serve(out: &Out, api: Api, ctrl: Controller, lines: Lines) -> Arc<AtomicUsize> {
    let running = Arc::new(AtomicUsize::new(0));
    for line in lines {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let v = serde_json::json!({ "t": "bad_request", "msg": e.to_string() });
                out.line(&v.to_string());
                continue;
            }
        };
        let Ok(cmd) = CString::new(req.cmd) else {
            let v = serde_json::json!({ "t": "bad_request", "corr": req.corr, "msg": "NUL byte" });
            out.line(&v.to_string());
            continue;
        };
        if req.bg {
            spawn_background(out, api, ctrl, &running, req.corr, cmd);
            continue;
        }
        // SAFETY: the command is NUL-terminated and lives across the call.
        let r = take(unsafe { (api.send)(ctrl.ptr(), cmd.as_ptr()) });
        out.body("resp", Some(req.corr), &r);
    }
    running
}

/// A command marked `bg`: run on a thread of its own, counted in `running`.
fn spawn_background(
    out: &Out,
    api: Api,
    ctrl: Controller,
    running: &Arc<AtomicUsize>,
    corr: u64,
    cmd: CString,
) {
    let (bg_out, bg_running) = (out.clone(), Arc::clone(running));
    running.fetch_add(1, Ordering::SeqCst);
    let spawned = std::thread::Builder::new()
        .name("engine-bg".into())
        .spawn(move || {
            // SAFETY: the command is NUL-terminated and lives across the call; the engine
            // accepts commands from several threads.
            let r = take(unsafe { (api.send)(ctrl.ptr(), cmd.as_ptr()) });
            bg_out.body("resp", Some(corr), &r);
            bg_running.fetch_sub(1, Ordering::SeqCst);
        });
    if spawned.is_err() {
        running.fetch_sub(1, Ordering::SeqCst);
        let v = serde_json::json!({ "t": "bad_request", "corr": corr, "msg": "no thread" });
        out.line(&v.to_string());
    }
}
