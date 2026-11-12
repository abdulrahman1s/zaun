//! Construct the real implementations of every trait, start the AppService and call
//! `easyplay_ui::run`. Never reachable from the supervisor path. Tasks UI-001, SVC-002.
//!
//! The window runs on the application service (`easyplay_ui::service_backend`) with
//! thumbnails from `artwork::ThumbService`. `--preview` or `EASYPLAY_PREVIEW=1` shows the UI
//! crate's sample library with generated covers instead (no library is opened). The GUI
//! holds `library.lock` shared for its lifetime (PERSISTENCE §6). Window geometry is read
//! from `window.json` before the window opens and written by a background thread
//! (PERSISTENCE §1). The renderer is `wgpu`, or `glow` when `EASYPLAY_RENDERER=glow` or when
//! `wgpu` fails to start (V-301).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use easyplay_core::artwork::ThumbService;
use easyplay_core::artwork::thumbs::ThumbDeps;
use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::consts::ENV_RENDERER;
use easyplay_core::fsx::{Fs, RealFs};
use easyplay_core::paths::{AppPaths, PathEnv};
use easyplay_core::service::AppService;
use easyplay_core::storage::StorageError;
use easyplay_core::storage::lock::{LibraryLock, LockMode};
use easyplay_core::{Error, ErrorCode};
use easyplay_ui::app::ArtFactory;
use easyplay_ui::bridge::Backend;
use easyplay_ui::service_backend::ServiceBackend;
use easyplay_ui::textures::{disk, synthetic};
use easyplay_ui::theme::Appearance;
use easyplay_ui::{Renderer, StartMode, UiDeps, WindowGeometry, WindowStore, demo};

use easyplay_core::desktop::instance::{
    self, Claim, InstanceServer, InstanceTransport, RaiseThen, Request, RequestHandler, WireOrigin,
};
use easyplay_core::desktop::notify::DesktopNotifier;

use crate::cli::exit;
use crate::display_libs;
use crate::instance_gui::{GuiRequests, LateHandler, Raiser, UiWaker};
use crate::notify_watch::NotifyWatch;
use crate::wiring::{self, Wiring};

/// `EASYPLAY_PREVIEW=1`: the sample library instead of the real one.
pub const ENV_PREVIEW: &str = "EASYPLAY_PREVIEW";
/// Start a second GUI (development; refused with the default data directories).
pub const ENV_NO_SINGLE_INSTANCE: &str = "EASYPLAY_NO_SINGLE_INSTANCE";
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
fn load_geometry(path: &Path) -> WindowGeometry {
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
struct FileWindowStore {
    tx: Sender<WindowGeometry>,
}

impl FileWindowStore {
    fn spawn(path: PathBuf) -> Self {
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
fn utc_offset_seconds() -> i32 {
    jiff::Zoned::now().offset().seconds()
}

/// What the window shows: the real service, or the preview.
enum Source {
    Service(Box<ServiceBackend>, Box<Wiring>),
    Preview,
}

fn art_factory(w: &Wiring) -> ArtFactory {
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
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let now = clock.now();
    let (backend, art): (Box<dyn Backend>, ArtFactory) = match source {
        Source::Service(b, w) => (b, art_factory(&w)),
        Source::Preview => (
            Box::new(demo::DemoBackend::new(now)),
            Box::new(|ctx| {
                let ctx = ctx.clone();
                Box::new(synthetic::Threaded::spawn(move || ctx.request_repaint()))
            }),
        ),
    };
    UiDeps {
        backend,
        clock,
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
        add_game: false,
        activations: None,
        focus: None,
        on_waker: None,
        display: easyplay_ui::DisplayBackend::Auto,
    }
}

/// Start the service (or the preview) for one window.
fn source(preview: bool, open: &[PathBuf]) -> Result<Source, Error> {
    if preview {
        return Ok(Source::Preview);
    }
    let mut w = wiring::build()?;
    let deps = w.take_deps()?;
    // Opened programs are sent by the UI's quick-launch layout (`UiDeps::open`).
    let _ = open;
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
/// GUI. `None` means "do not take part" (preview, or `EASYPLAY_NO_SINGLE_INSTANCE` with
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
    let window_file = paths.as_ref().map(|p| p.window_state_file.clone());
    let window = window_file
        .as_deref()
        .map(load_geometry)
        .unwrap_or_default();
    let store = || -> Option<Box<dyn WindowStore>> {
        window_file
            .clone()
            .map(|p| Box::new(FileWindowStore::spawn(p)) as Box<dyn WindowStore>)
    };
    let forced_glow = env
        .get(ENV_RENDERER)
        .and_then(|v| v.to_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("glow"));
    let first = if forced_glow {
        Renderer::Glow
    } else {
        Renderer::Wgpu
    };

    let focus = Arc::new(AtomicBool::new(true));
    let waker: UiWaker = Arc::new(Mutex::new(None));
    let make = |renderer: Renderer| -> Result<(UiDeps, Option<NotifyWatch>), Error> {
        let (tx, rx) = channel();
        let raiser = Raiser::new(tx, Arc::clone(&waker));
        let src = source(preview, &open)?;
        let mut watch = None;
        if let (Source::Service(b, _), Some(p)) = (&src, &paths) {
            let hub = Arc::clone(&b.handle().hub);
            if let Some(l) = &late {
                l.attach(GuiRequests {
                    cmds: b.handle().sender(),
                    hub: Arc::clone(&hub),
                    raiser: raiser.clone(),
                    db_file: p.db_file.clone(),
                });
            }
            let r2 = raiser.clone();
            let notifier = Arc::new(DesktopNotifier::new(Some(Arc::new(move || {
                r2.raise(None, false);
            }))));
            watch = NotifyWatch::spawn(hub, p.db_file.clone(), Arc::clone(&focus), notifier);
        }
        let mut d = deps(src, open.clone(), window, store(), renderer);
        d.add_game = add;
        d.activations = Some(rx);
        d.focus = Some(Arc::clone(&focus));
        let w = Arc::clone(&waker);
        d.on_waker = Some(Box::new(move |f| {
            *w.lock().unwrap_or_else(PoisonError::into_inner) = Some(f);
        }));
        d.display = display;
        Ok((d, watch))
    };

    let (d, watch) = make(first)?;
    let result = easyplay_ui::run(StartMode::Library, d);
    drop(watch);
    match result {
        Ok(()) => Ok(exit::OK),
        Err(e) if first == Renderer::Wgpu && !easyplay_ui::is_event_loop_error(&e) => {
            // V-301: fall back to GL on the same event loop (eframe keeps it per thread).
            tracing::warn!("wgpu renderer failed ({e}); retrying with glow");
            let (d, watch) = make(Renderer::Glow)?;
            let r = easyplay_ui::run(StartMode::Library, d);
            drop(watch);
            r.map(|()| exit::OK)
                .map_err(|e| Error::new(ErrorCode::HostNoDisplay).with("detail", e))
        }
        Err(e) => Err(Error::new(ErrorCode::HostNoDisplay).with("detail", e)),
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
    /// `egui_kittest`. Needs `EASYPLAY_E2E_GUI=1`, throw-away `EASYPLAY_*_DIR`s holding a
    /// library with a game named by `EASYPLAY_E2E_GAME` and an installed runtime, a working
    /// `umu-run` (`EASYPLAY_UMU_RUN`) and `easyplay-init` next to the test binary or in
    /// `EASYPLAY_INIT`, and the `easyplay` binary in `EASYPLAY_E2E_BIN`. Clicks the game, Play, waits for `Running`, Stop, waits for the end
    /// and checks the recorded session.
    #[test]
    #[ignore = "real host: starts a game"]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn e2e_gui_play_and_stop_with_the_real_service() {
        use easyplay_core::model::SessionState;
        use easyplay_ui::app::EasyPlayApp;
        use easyplay_ui::route::Route;
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        use std::time::Duration;

        if std::env::var_os("EASYPLAY_E2E_GUI").is_none() {
            return;
        }
        let name = std::env::var("EASYPLAY_E2E_GAME").unwrap_or_else(|_| "Cmd Test".into());
        let bin = std::env::var_os("EASYPLAY_E2E_BIN").map(PathBuf::from);
        let mut w = wiring::build_with(bin).unwrap();
        let (handle, events) = AppService::start(w.take_deps().unwrap()).unwrap();
        let hub = Arc::clone(&handle.hub);
        let backend = ServiceBackend::new(handle, events);
        let app = EasyPlayApp::new(
            easyplay_ui::bridge::UiBridge::new(Box::new(backend)),
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
        let pump = |h: &mut Harness<'_, EasyPlayApp>, secs: u64, done: &dyn Fn() -> bool| {
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
    /// throw-away directories (`EASYPLAY_*_DIR`), rendered by `egui_kittest` to the PNG named
    /// by `EASYPLAY_E2E_SHOT` after the service settles. Needs `EASYPLAY_E2E_GUI=1`.
    #[test]
    #[ignore = "real host: renders the real service"]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn e2e_gui_snapshot_of_the_real_service() {
        use easyplay_ui::app::EasyPlayApp;
        use easyplay_ui::route::Route;
        use egui_kittest::Harness;
        use std::time::Duration;

        let (Some(_), Some(shot)) = (
            std::env::var_os("EASYPLAY_E2E_GUI"),
            std::env::var_os("EASYPLAY_E2E_SHOT"),
        ) else {
            return;
        };
        let mut w = wiring::build_with(None).unwrap();
        let (handle, events) = AppService::start(w.take_deps().unwrap()).unwrap();
        let backend = ServiceBackend::new(handle, events);
        let app = EasyPlayApp::new(
            easyplay_ui::bridge::UiBridge::new(Box::new(backend)),
            Arc::new(SystemClock::new()),
            Appearance {
                theme: easyplay_ui::theme::ThemeChoice::Dark,
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
