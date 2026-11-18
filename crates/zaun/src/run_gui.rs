//! Construct the real implementations of every trait, start the AppService and call
//! `zaun_ui::run`. Never reachable from the supervisor path. Tasks UI-001, SVC-002.
//!
//! The window runs on the application service (`zaun_ui::service_backend`) with
//! thumbnails from `artwork::ThumbService`. `--preview` or `ZAUN_PREVIEW=1` shows the UI
//! crate's sample library with generated covers instead (no library is opened). The GUI
//! holds `library.lock` shared for its lifetime (PERSISTENCE §6). Window geometry is read
//! from `window.json` before the window opens and written by a background thread
//! (PERSISTENCE §1). The renderer is `wgpu`, or `glow` when `ZAUN_RENDERER=glow` or when
//! `wgpu` fails to start (V-301).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use zaun_core::artwork::ThumbService;
use zaun_core::artwork::thumbs::ThumbDeps;
use zaun_core::clock::{Clock, SystemClock};
use zaun_core::consts::ENV_RENDERER;
use zaun_core::diagnostics::crash::{self, CrashPhase};
use zaun_core::fsx::{Fs, RealFs};
use zaun_core::paths::{AppPaths, PathEnv};
use zaun_core::service::AppService;
use zaun_core::storage::StorageError;
use zaun_core::storage::lock::{LibraryLock, LockMode};
use zaun_core::{Error, ErrorCode};
use zaun_ui::app::ArtFactory;
use zaun_ui::bridge::Backend;
use zaun_ui::service_backend::ServiceBackend;
use zaun_ui::textures::{disk, synthetic};
use zaun_ui::theme::Appearance;
use zaun_ui::{Renderer, StartMode, UiDeps, WindowGeometry, WindowStore, demo};

use zaun_core::desktop::instance::{
    self, Claim, InstanceServer, InstanceTransport, RaiseThen, Request, RequestHandler, WireOrigin,
};
use zaun_core::desktop::notify::DesktopNotifier;

use crate::cli::exit;
use crate::display_libs;
use crate::instance_gui::{GuiRequests, LateHandler, UiWaker, Windows};
use crate::notify_watch::NotifyWatch;
use crate::wiring::{self, Wiring};

/// `ZAUN_PREVIEW=1`: the sample library instead of the real one.
pub const ENV_PREVIEW: &str = "ZAUN_PREVIEW";
/// Start a second GUI (development; refused with the default data directories).
pub const ENV_NO_SINGLE_INSTANCE: &str = "ZAUN_NO_SINGLE_INSTANCE";
use serde::{Deserialize, Serialize};

/// `window.json` (PERSISTENCE §1).
#[derive(Debug, Serialize, Deserialize)]
struct WindowFile {
    size: [f32; 2],
    #[serde(default)]
    position: Option<[f32; 2]>,
    #[serde(default)]
    maximized: bool,
    #[serde(default = "one")]
    ui_scale: f32,
}

fn one() -> f32 {
    1.0
}

impl From<&WindowGeometry> for WindowFile {
    fn from(g: &WindowGeometry) -> Self {
        Self {
            size: g.size,
            position: g.position,
            maximized: g.maximized,
            ui_scale: g.ui_scale,
        }
    }
}

/// Read saved geometry; missing or invalid → defaults.
pub(crate) fn load_geometry(path: &Path) -> WindowGeometry {
    let parsed = RealFs::new()
        .read(path, 64 * 1024)
        .ok()
        .and_then(|b| serde_json::from_slice::<WindowFile>(&b).ok());
    match parsed {
        Some(f) if f.size.iter().all(|v| v.is_finite() && *v >= 100.0) => WindowGeometry {
            size: f.size,
            position: f.position,
            maximized: f.maximized,
            ui_scale: f.ui_scale.clamp(0.75, 2.0),
        },
        _ => WindowGeometry::default(),
    }
}

/// Writes geometry on a background thread so the UI never touches the disk.
pub(crate) struct FileWindowStore {
    tx: Sender<WindowGeometry>,
}

impl FileWindowStore {
    pub(crate) fn spawn(path: PathBuf) -> Self {
        let (tx, rx) = channel::<WindowGeometry>();
        let _ = std::thread::Builder::new()
            .name("window-geometry".into())
            .spawn(move || {
                let fs = RealFs::new();
                while let Ok(mut g) = rx.recv() {
                    // Coalesce bursts: only the latest value matters.
                    while let Ok(newer) = rx.try_recv() {
                        g = newer;
                    }
                    let Ok(json) = serde_json::to_vec_pretty(&WindowFile::from(&g)) else {
                        continue;
                    };
                    if let Some(dir) = path.parent() {
                        let _ = fs.create_dir_all(dir, 0o700);
                    }
                    if let Err(e) = fs.atomic_write(&path, &json, 0o600) {
                        tracing::warn!("window geometry not saved: {e}");
                    }
                }
            });
        Self { tx }
    }
}

impl WindowStore for FileWindowStore {
    fn save(&self, geometry: &WindowGeometry) {
        let _ = self.tx.send(*geometry);
    }
}

/// The local UTC offset now, for date formatting on the UI thread without I/O.
pub(crate) fn utc_offset_seconds() -> i32 {
    jiff::Zoned::now().offset().seconds()
}

/// What the window shows: the real service, or the preview.
enum Source {
    Service(Box<ServiceBackend>, Box<Wiring>),
    Preview,
}

pub(crate) fn art_factory(w: &Wiring) -> ArtFactory {
    let pool = Arc::clone(&w.jobs);
    let fs = Arc::clone(&w.fs);
    let codec = Arc::clone(&w.codec);
    let thumbs_dir = w.paths.thumbs_dir.clone();
    let index = Arc::clone(&w.art_index);
    Box::new(move |ctx| {
        let ctx = ctx.clone();
        let service = ThumbService::new(ThumbDeps {
            pool,
            fs,
            codec,
            thumbs_dir,
            index,
            wake: Arc::new(move || ctx.request_repaint()),
        });
        Box::new(disk::ThumbSource::new(service))
    })
}

fn deps(
    source: Source,
    open: Vec<PathBuf>,
    window: WindowGeometry,
    store: Option<Box<dyn WindowStore>>,
    renderer: Renderer,
) -> UiDeps {
    let (backend, art): (Box<dyn Backend>, ArtFactory) = match source {
        Source::Service(b, w) => (b, art_factory(&w)),
        Source::Preview => (
            Box::new(demo::DemoBackend::new(SystemClock::new().now())),
            Box::new(|ctx| {
                let ctx = ctx.clone();
                Box::new(synthetic::Threaded::spawn(move || ctx.request_repaint()))
            }),
        ),
    };
    ui_deps(backend, art, open, window, store, renderer)
}

/// The window's inputs over `backend`; the caller fills in activations, focus and wakers.
pub(crate) fn ui_deps(
    backend: Box<dyn Backend>,
    art: ArtFactory,
    open: Vec<PathBuf>,
    window: WindowGeometry,
    store: Option<Box<dyn WindowStore>>,
    renderer: Renderer,
) -> UiDeps {
    UiDeps {
        backend,
        clock: Arc::new(SystemClock::new()),
        appearance: Appearance {
            ui_scale: window.ui_scale,
            ..Appearance::default()
        },
        window,
        window_store: store,
        utc_offset_s: utc_offset_seconds(),
        renderer,
        art: Some(art),
        open,
        open_sent: false,
        add_game: false,
        activations: None,
        focus: None,
        on_waker: None,
        display: zaun_ui::DisplayBackend::Auto,
        system: zaun_ui::theme::SystemPrefs {
            locale: desktop_settings::locale(),
            ..zaun_ui::theme::SystemPrefs::default()
        },
        system_changes: None,
        on_close: None,
    }
}

/// Start the service (or the preview) for one window. Opened programs are not sent here:
/// the UI's quick-launch layout does that (`UiDeps::open`).
fn source(preview: bool) -> Result<Source, Error> {
    if preview {
        return Ok(Source::Preview);
    }
    let mut w = wiring::build()?;
    let deps = w.take_deps()?;
    let home = deps.launch_env.home.clone();
    let user_dirs = deps.launch_env.user_dirs.clone();
    let (handle, events) = AppService::start(deps)?;
    Ok(Source::Service(
        Box::new(ServiceBackend::new(handle, events).with_user_dirs(home, user_dirs)),
        Box::new(w),
    ))
}

fn preview_requested(env: &PathEnv, flag: bool) -> bool {
    flag || env
        .get(ENV_PREVIEW)
        .and_then(|v| v.to_str())
        .is_some_and(|v| matches!(v, "1" | "true" | "yes"))
}

/// Single instance: become the primary (serving the socket) or hand over to the running
/// GUI. `None` means "do not take part" (preview, or `ZAUN_NO_SINGLE_INSTANCE` with
/// non-default data directories).
enum Instance {
    Primary(InstanceServer, Arc<LateHandler>),
    HandedOver(u8),
    Alone,
}

fn claim_instance(
    env: &PathEnv,
    paths: &AppPaths,
    preview: bool,
    open: &[PathBuf],
    add: bool,
) -> Result<Instance, Error> {
    if preview {
        return Ok(Instance::Alone);
    }
    if env.get(ENV_NO_SINGLE_INSTANCE).is_some() {
        if paths.uses_default_roots(env) {
            tracing::warn!("{ENV_NO_SINGLE_INSTANCE} refused with the default data directories");
        } else {
            return Ok(Instance::Alone);
        }
    }
    let late = Arc::new(LateHandler::default());
    let h = Arc::clone(&late);
    match instance::claim(paths, env.uid(), move || h as Arc<dyn RequestHandler>)? {
        Claim::Primary(server) => Ok(Instance::Primary(server, late)),
        Claim::Forward(t) => {
            // Lost the start race: hand over like a second process would.
            let token = instance::activation_token(env);
            let req = if open.is_empty() {
                Request::Raise {
                    activation_token: token,
                    then: add.then_some(RaiseThen::AddGame),
                }
            } else {
                Request::Open {
                    paths: open.to_vec(),
                    origin: WireOrigin::FileManager,
                    cwd: std::env::current_dir().ok(),
                    activation_token: token,
                }
            };
            let r = t.send(&req, &mut |_| {})?;
            Ok(Instance::HandedOver(if r.ok {
                exit::OK
            } else {
                exit::FAILED
            }))
        }
    }
}

/// ERROR_HANDLING §4.11: after crashes, start with defaults and/or the GL renderer.
pub(crate) fn crash_advice(paths: &AppPaths) -> crash::StartAdvice {
    let fs = RealFs::new();
    crash::prune(&fs, &paths.crash_dir, SystemClock::new().now());
    let recent: Vec<_> = crash::unacknowledged(&fs, &paths.crash_dir)
        .into_iter()
        .map(|(_, r)| r)
        .collect();
    let advice = crash::start_advice(&recent);
    if advice != crash::StartAdvice::default() {
        tracing::warn!(
            reset_geometry = advice.reset_geometry,
            prefer_glow = advice.prefer_glow,
            "starting with safe defaults after a crash"
        );
    }
    advice
}

/// `wgpu`, or `glow` when `ZAUN_RENDERER=glow` or after a graphics crash.
pub(crate) fn first_renderer(env: &PathEnv, advice: &crash::StartAdvice) -> Renderer {
    let forced_glow = env
        .get(ENV_RENDERER)
        .and_then(|v| v.to_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("glow"));
    if forced_glow || advice.prefer_glow {
        Renderer::Glow
    } else {
        Renderer::Wgpu
    }
}

/// Start the GUI (optionally with files to open, or the add-game picker); returns the exit
/// code (another process may have become the GUI first).
pub fn run(open: Vec<PathBuf>, add: bool, preview: bool) -> Result<u8, Error> {
    let env = PathEnv::from_process();
    let preview = preview_requested(&env, preview);
    let paths = AppPaths::resolve(&env).ok();
    let open: Vec<PathBuf> = open
        .into_iter()
        .map(|p| std::path::absolute(&p).unwrap_or(p))
        .collect();
    let instance = match &paths {
        Some(p) => claim_instance(&env, p, preview, &open, add)?,
        None => Instance::Alone,
    };
    let (_server, late) = match instance {
        Instance::HandedOver(code) => return Ok(code),
        Instance::Primary(s, l) => (Some(s), Some(l)),
        Instance::Alone => (None, None),
    };
    // Before the service starts: the event loop exists once per process, so the windowing
    // backend is chosen from what can be loaded (V-301 covers the renderer only).
    let display = display_libs::choose(&env, display_libs::Loadable::probe())?;
    // PERSISTENCE §6: shared for the GUI's lifetime; headless writers take it exclusively.
    let _lock = match (&paths, preview) {
        (Some(p), false) => {
            RealFs::new().create_dir_all(&p.data_dir, 0o700)?;
            Some(
                LibraryLock::try_acquire(&p.library_lock, LockMode::Shared).map_err(
                    |e| match e {
                        StorageError::LockHeld => Error::new(ErrorCode::LibBusy),
                        other => other.into(),
                    },
                )?,
            )
        }
        _ => None,
    };
    let advice = paths
        .as_ref()
        .filter(|_| !preview)
        .map(crash_advice)
        .unwrap_or_default();
    let window_file = paths.as_ref().map(|p| p.window_state_file.clone());
    let window = window_file
        .as_deref()
        .filter(|_| !advice.reset_geometry)
        .map(load_geometry)
        .unwrap_or_default();
    let store = || -> Option<Box<dyn WindowStore>> {
        window_file
            .clone()
            .map(|p| Box::new(FileWindowStore::spawn(p)) as Box<dyn WindowStore>)
    };
    let first = first_renderer(&env, &advice);

    let focus = Arc::new(AtomicBool::new(true));
    let waker: UiWaker = Arc::new(Mutex::new(None));
    let slot = Arc::new(Windows::new(Arc::clone(&waker)));
    let make = |renderer: Renderer| -> Result<(UiDeps, Option<NotifyWatch>), Error> {
        let (tx, rx) = channel();
        slot.opened(tx);
        let src = source(preview)?;
        let mut watch = None;
        if let (Source::Service(b, _), Some(p)) = (&src, &paths) {
            let hub = Arc::clone(&b.handle().hub);
            if let Some(l) = &late {
                l.attach(GuiRequests {
                    cmds: b.handle().sender(),
                    hub: Arc::clone(&hub),
                    windows: Arc::clone(&slot),
                    db_file: p.db_file.clone(),
                });
            }
            let w2 = Arc::clone(&slot);
            let notifier = Arc::new(DesktopNotifier::new(Some(Arc::new(move || {
                w2.raise(None, false);
            }))));
            watch = NotifyWatch::spawn(hub, p.db_file.clone(), Arc::clone(&focus), notifier);
        }
        let mut d = deps(src, open.clone(), window, store(), renderer);
        d.add_game = add;
        d.activations = Some(rx);
        d.focus = Some(Arc::clone(&focus));
        let w = Arc::clone(&waker);
        d.on_waker = Some(Box::new(move |f| {
            // Called once the renderer is up: a later panic is not a graphics failure.
            crate::panic::set_phase(CrashPhase::Running);
            *w.lock().unwrap_or_else(PoisonError::into_inner) = Some(f);
        }));
        d.display = display;
        // The desktop's appearance (A11Y-004): wait briefly for the first reading so the
        // window opens in the right theme; later changes wake the UI.
        let w = Arc::clone(&waker);
        let rx = desktop_settings::follow(Arc::new(move || {
            if let Some(f) = w.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
                f();
            }
        }));
        if let Ok(first) = rx.recv_timeout(std::time::Duration::from_millis(150)) {
            d.appearance.reduce_motion = first.reduced_motion.unwrap_or(false);
            d.system = first;
        }
        d.system_changes = Some(rx);
        Ok((d, watch))
    };

    // First launch runs onboarding in a compact window; once it is completed the library
    // opens in a full one, with a fresh service that reads the saved preferences (the first
    // service finishes its queued writes before it stops).
    let windows = |renderer: Renderer| -> Result<_, Error> {
        let (d, watch) = make(renderer)?;
        let onboarding = zaun_ui::opens_onboarding(&d);
        crate::panic::set_phase(CrashPhase::Graphics);
        let r = zaun_ui::run(StartMode::Library, d);
        drop(watch);
        if !onboarding || r.is_err() {
            return Ok(r);
        }
        let (d, watch) = make(renderer)?;
        if zaun_ui::opens_onboarding(&d) {
            // Closed without finishing: the next start asks again.
            return Ok(r);
        }
        let r = zaun_ui::run(StartMode::Library, d);
        drop(watch);
        Ok(r)
    };

    match windows(first)? {
        Ok(()) => Ok(exit::OK),
        Err(e) if first == Renderer::Wgpu && !zaun_ui::is_event_loop_error(&e) => {
            // V-301: fall back to GL on the same event loop (eframe keeps it per thread).
            tracing::warn!("wgpu renderer failed ({e}); retrying with glow");
            windows(Renderer::Glow)?
                .map(|()| exit::OK)
                .map_err(|e| Error::new(ErrorCode::HostNoDisplay).with("detail", e))
        }
        Err(e) => Err(Error::new(ErrorCode::HostNoDisplay).with("detail", e)),
    }
}

pub(crate) mod desktop_settings {
    //! Desktop appearance preferences (task A11Y-004): colour scheme, high contrast, reduced
    //! motion and text scale, read from the XDG desktop portal's Settings interface on a
    //! background thread and followed live through its `SettingChanged` signal. Desktops whose
    //! portal lacks a key fall back to the GNOME / KDE keys the portal also exposes. Without a
    //! portal (or a session bus) nothing is sent and the preferences decide alone.

    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::mpsc::{Receiver, Sender, channel};

    use zaun_ui::theme::{SystemPrefs, SystemScheme};
    use zbus::zvariant::{OwnedValue, Value};

    const DEST: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const IFACE: &str = "org.freedesktop.portal.Settings";

    /// The (namespace, key) pairs read.
    const KEYS: [(&str, &str); 7] = [
        ("org.freedesktop.appearance", "color-scheme"),
        ("org.freedesktop.appearance", "contrast"),
        ("org.freedesktop.appearance", "reduced-motion"),
        ("org.gnome.desktop.a11y.interface", "high-contrast"),
        ("org.gnome.desktop.interface", "enable-animations"),
        ("org.gnome.desktop.interface", "text-scaling-factor"),
        ("org.kde.kdeglobals.KDE", "AnimationDurationFactor"),
    ];

    /// Raw values by "namespace key".
    type Raw = HashMap<String, OwnedValue>;

    fn unwrap_variant(v: &Value<'_>) -> Value<'static> {
        match v {
            Value::Value(inner) => unwrap_variant(inner),
            other => other
                .try_to_owned()
                .map(Value::from)
                .unwrap_or(Value::Bool(false)),
        }
    }

    fn as_u32(v: &Value<'_>) -> Option<u32> {
        match unwrap_variant(v) {
            Value::U32(n) => Some(n),
            Value::I32(n) => u32::try_from(n).ok(),
            Value::U8(n) => Some(u32::from(n)),
            _ => None,
        }
    }

    fn as_bool(v: &Value<'_>) -> Option<bool> {
        match unwrap_variant(v) {
            Value::Bool(b) => Some(b),
            Value::Str(s) => match s.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    fn as_f64(v: &Value<'_>) -> Option<f64> {
        match unwrap_variant(v) {
            Value::F64(f) => Some(f),
            Value::Str(s) => s.as_str().parse().ok(),
            Value::U32(n) => Some(f64::from(n)),
            Value::I32(n) => Some(f64::from(n)),
            _ => None,
        }
    }

    /// Interpret the raw portal values (portal keys first, desktop keys as fallback).
    fn interpret(raw: &Raw) -> SystemPrefs {
        let get = |ns: &str, key: &str| {
            raw.get(&format!("{ns} {key}"))
                .map(|v| Value::from(v.clone()))
        };
        let scheme = get("org.freedesktop.appearance", "color-scheme")
            .as_ref()
            .and_then(as_u32)
            .and_then(|n| match n {
                1 => Some(SystemScheme::Dark),
                2 => Some(SystemScheme::Light),
                _ => None,
            });
        let high_contrast = get("org.freedesktop.appearance", "contrast")
            .as_ref()
            .and_then(as_u32)
            .map(|n| n == 1)
            .or_else(|| {
                get("org.gnome.desktop.a11y.interface", "high-contrast")
                    .as_ref()
                    .and_then(as_bool)
            })
            .unwrap_or(false);
        let reduced_motion = get("org.freedesktop.appearance", "reduced-motion")
            .as_ref()
            .and_then(as_u32)
            .map(|n| n == 1)
            .or_else(|| {
                get("org.gnome.desktop.interface", "enable-animations")
                    .as_ref()
                    .and_then(as_bool)
                    .map(|on| !on)
            })
            .or_else(|| {
                get("org.kde.kdeglobals.KDE", "AnimationDurationFactor")
                    .as_ref()
                    .and_then(as_f64)
                    .map(|f| f <= 0.0)
            });
        let text_scale = get("org.gnome.desktop.interface", "text-scaling-factor")
            .as_ref()
            .and_then(as_f64)
            .filter(|f| f.is_finite())
            .map_or(1.0, |f| f.clamp(0.75, 2.0) as f32);
        SystemPrefs {
            scheme,
            high_contrast,
            reduced_motion,
            text_scale,
            locale: None,
        }
    }

    fn read_all(proxy: &zbus::blocking::Proxy<'_>) -> Raw {
        let mut raw = Raw::new();
        for (ns, key) in KEYS {
            let v: zbus::Result<OwnedValue> = proxy.call("ReadOne", &(ns, key));
            let v = match v {
                Ok(v) => Ok(v),
                // Portals before 1.15 only have the deprecated `Read`.
                Err(_) => proxy.call("Read", &(ns, key)),
            };
            if let Ok(v) = v {
                raw.insert(format!("{ns} {key}"), v);
            }
        }
        raw
    }

    fn watch(tx: &Sender<SystemPrefs>, wake: &(dyn Fn() + Send + Sync)) -> zbus::Result<()> {
        let conn = zbus::blocking::Connection::session()?;
        let proxy = zbus::blocking::Proxy::new(&conn, DEST, PATH, IFACE)?;
        let mut raw = read_all(&proxy);
        let locale = locale();
        let with_locale = |p: SystemPrefs| SystemPrefs {
            locale: locale.clone(),
            ..p
        };
        let mut last = with_locale(interpret(&raw));
        tracing::debug!(?last, "desktop appearance preferences");
        if tx.send(last.clone()).is_err() {
            return Ok(());
        }
        wake();
        let signals = proxy.receive_signal("SettingChanged")?;
        for msg in signals {
            let Ok((ns, key, value)) = msg.body().deserialize::<(String, String, OwnedValue)>()
            else {
                continue;
            };
            if !KEYS.iter().any(|(n, k)| *n == ns && *k == key) {
                continue;
            }
            raw.insert(format!("{ns} {key}"), value);
            let now = with_locale(interpret(&raw));
            if now != last {
                last = now.clone();
                if tx.send(now).is_err() {
                    return Ok(());
                }
                wake();
            }
        }
        Ok(())
    }

    /// The first non-empty of `LC_ALL`, `LC_MESSAGES`, `LANG` that is not "C" or "POSIX".
    pub fn locale_of(get: impl Fn(&str) -> Option<String>) -> Option<String> {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(get)
            .map(|v| v.trim().to_owned())
            .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
    }

    /// The desktop's locale from the process environment, when it names one.
    pub fn locale() -> Option<String> {
        locale_of(|k| std::env::var(k).ok())
    }

    /// Start following the desktop's appearance preferences; `wake` is called after each
    /// change.
    pub fn follow(wake: Arc<dyn Fn() + Send + Sync>) -> Receiver<SystemPrefs> {
        let (tx, rx) = channel();
        let _ = std::thread::Builder::new()
            .name("desktop-settings".into())
            .spawn(move || {
                if let Err(e) = watch(&tx, &*wake) {
                    tracing::debug!("desktop appearance settings unavailable: {e}");
                }
            });
        rx
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn raw(pairs: &[(&str, Value<'static>)]) -> Raw {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), OwnedValue::try_from(v.clone()).unwrap()))
                .collect()
        }

        #[test]
        fn the_locale_comes_from_the_first_usable_variable() {
            let env = |pairs: &'static [(&'static str, &'static str)]| {
                move |k: &str| {
                    pairs
                        .iter()
                        .find(|(n, _)| *n == k)
                        .map(|(_, v)| (*v).to_owned())
                }
            };
            assert_eq!(locale_of(env(&[])), None);
            assert_eq!(
                locale_of(env(&[("LANG", "ar_EG.UTF-8")])).as_deref(),
                Some("ar_EG.UTF-8")
            );
            assert_eq!(
                locale_of(env(&[("LC_ALL", "en_GB"), ("LANG", "ar_EG.UTF-8")])).as_deref(),
                Some("en_GB")
            );
            assert_eq!(
                locale_of(env(&[("LC_ALL", "C"), ("LC_MESSAGES", ""), ("LANG", "ar")])).as_deref(),
                Some("ar")
            );
            assert_eq!(locale_of(env(&[("LANG", "POSIX")])), None);
        }

        #[test]
        fn portal_keys_win_and_desktop_keys_fill_in() {
            let none = interpret(&Raw::new());
            assert_eq!(none, SystemPrefs::default());
            let p = interpret(&raw(&[
                ("org.freedesktop.appearance color-scheme", Value::U32(1)),
                ("org.freedesktop.appearance contrast", Value::U32(1)),
                (
                    "org.gnome.desktop.interface enable-animations",
                    Value::Bool(false),
                ),
                (
                    "org.gnome.desktop.interface text-scaling-factor",
                    Value::F64(1.25),
                ),
            ]));
            assert_eq!(p.scheme, Some(SystemScheme::Dark));
            assert!(p.high_contrast);
            assert_eq!(p.reduced_motion, Some(true));
            assert!((p.text_scale - 1.25).abs() < 1e-6);
            let kde = interpret(&raw(&[
                ("org.freedesktop.appearance color-scheme", Value::U32(2)),
                (
                    "org.gnome.desktop.a11y.interface high-contrast",
                    Value::Bool(true),
                ),
                (
                    "org.kde.kdeglobals.KDE AnimationDurationFactor",
                    Value::F64(0.0),
                ),
                // The portal's own key wins over the desktop's.
                ("org.freedesktop.appearance reduced-motion", Value::U32(0)),
            ]));
            assert_eq!(kde.scheme, Some(SystemScheme::Light));
            assert!(kde.high_contrast);
            assert_eq!(kde.reduced_motion, Some(false));
            // Wrapped in a variant (the deprecated `Read`) still works.
            let wrapped = interpret(&raw(&[(
                "org.freedesktop.appearance color-scheme",
                Value::Value(Box::new(Value::U32(1))),
            )]));
            assert_eq!(wrapped.scheme, Some(SystemScheme::Dark));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_round_trips_and_rejects_nonsense() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("window.json");
        assert_eq!(load_geometry(&p), WindowGeometry::default());
        let g = WindowGeometry {
            size: [900.0, 700.0],
            position: Some([10.0, 20.0]),
            maximized: true,
            ui_scale: 1.25,
        };
        RealFs::new()
            .atomic_write(
                &p,
                &serde_json::to_vec(&WindowFile::from(&g)).unwrap(),
                0o600,
            )
            .unwrap();
        assert_eq!(load_geometry(&p), g);
        RealFs::new()
            .atomic_write(&p, br#"{"size":[1,1]}"#, 0o600)
            .unwrap();
        assert_eq!(load_geometry(&p), WindowGeometry::default());
    }

    /// Real-host check (not run by default): the GUI with the real AppService, driven by
    /// `egui_kittest`. Needs `ZAUN_E2E_GUI=1`, throw-away `ZAUN_*_DIR`s holding a
    /// library with a game named by `ZAUN_E2E_GAME` and an installed runtime, a working
    /// `umu-run` (`ZAUN_UMU_RUN`) and `zaun-init` next to the test binary or in
    /// `ZAUN_INIT`, and the `zaun` binary in `ZAUN_E2E_BIN`. Clicks the game, Play, waits for `Running`, Stop, waits for the end
    /// and checks the recorded session.
    #[test]
    #[ignore = "real host: starts a game"]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn e2e_gui_play_and_stop_with_the_real_service() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        use std::time::Duration;
        use zaun_core::model::SessionState;
        use zaun_ui::app::ZaunApp;
        use zaun_ui::route::Route;

        if std::env::var_os("ZAUN_E2E_GUI").is_none() {
            return;
        }
        let name = std::env::var("ZAUN_E2E_GAME").unwrap_or_else(|_| "Cmd Test".into());
        let bin = std::env::var_os("ZAUN_E2E_BIN").map(PathBuf::from);
        let mut w = wiring::build_with(bin).unwrap();
        let (handle, events) = AppService::start(w.take_deps().unwrap()).unwrap();
        let hub = Arc::clone(&handle.hub);
        let backend = ServiceBackend::new(handle, events);
        let app = ZaunApp::new(
            zaun_ui::bridge::UiBridge::new(Box::new(backend)),
            Arc::new(SystemClock::new()),
            Appearance::default(),
            Route::Library,
            0,
        );
        let mut h = Harness::builder()
            .with_size([1280.0, 800.0])
            .build_eframe(move |_cc| app);
        // A fresh library opens on onboarding; this check starts from the library.
        h.state_mut().state.nav.go_to(Route::Library);
        let pump = |h: &mut Harness<'_, ZaunApp>, secs: u64, done: &dyn Fn() -> bool| {
            let clock = SystemClock::new();
            let t = clock.mono();
            while !done() {
                assert!(
                    clock.mono().saturating_duration_since(t) < Duration::from_secs(secs),
                    "timed out"
                );
                h.step();
                std::thread::sleep(Duration::from_millis(100));
            }
            h.step();
        };
        pump(&mut h, 10, &|| {
            hub.library.load().games.iter().any(|g| *g.name == *name)
        });
        h.get_by_label_contains(&format!("{name}. ")).click();
        h.step();
        h.step();
        h.get_by_label(&format!("Play {name}")).click();
        h.step();
        let running = || {
            hub.sessions
                .load()
                .sessions
                .iter()
                .any(|s| s.game_name == name && s.state == SessionState::Running)
        };
        pump(&mut h, 120, &running);
        eprintln!(
            "running: {:?}",
            hub.sessions.load().sessions.first().map(|s| s.id)
        );
        std::thread::sleep(Duration::from_secs(5));
        h.step();
        h.step();
        h.get_by_label(&format!("Stop {name}")).click();
        h.step();
        pump(&mut h, 60, &|| hub.sessions.load().sessions.is_empty());
        let g = hub
            .library
            .load()
            .games
            .iter()
            .find(|g| *g.name == *name)
            .cloned()
            .unwrap();
        eprintln!(
            "after stop: sessions={} launches={} playtime_ms={}",
            g.session_count, g.launch_count, g.total_playtime_ms
        );
        assert!(g.launch_count >= 1);
    }

    /// Real-host look (not run by default): the window over the real AppService in
    /// throw-away directories (`ZAUN_*_DIR`), rendered by `egui_kittest` to the PNG named
    /// by `ZAUN_E2E_SHOT` after the service settles. Needs `ZAUN_E2E_GUI=1`.
    #[test]
    #[ignore = "real host: renders the real service"]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn e2e_gui_snapshot_of_the_real_service() {
        use egui_kittest::Harness;
        use std::time::Duration;
        use zaun_ui::app::ZaunApp;
        use zaun_ui::route::Route;

        let (Some(_), Some(shot)) = (
            std::env::var_os("ZAUN_E2E_GUI"),
            std::env::var_os("ZAUN_E2E_SHOT"),
        ) else {
            return;
        };
        let mut w = wiring::build_with(None).unwrap();
        let (handle, events) = AppService::start(w.take_deps().unwrap()).unwrap();
        let backend = ServiceBackend::new(handle, events);
        let app = ZaunApp::new(
            zaun_ui::bridge::UiBridge::new(Box::new(backend)),
            Arc::new(SystemClock::new()),
            Appearance {
                theme: zaun_ui::theme::ThemeChoice::Dark,
                ..Appearance::default()
            },
            Route::Library,
            0,
        );
        let mut h = Harness::builder()
            .with_size([1280.0, 800.0])
            .build_eframe(move |_cc| app);
        for _ in 0..30 {
            h.step();
            std::thread::sleep(Duration::from_millis(100));
        }
        let image = h.render().unwrap();
        image.save(PathBuf::from(shot)).unwrap();
    }
}
