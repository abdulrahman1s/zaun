//! `zaun service`: the resident Zaun (ADR-0022). It is the single instance with the
//! library open and the application service warm, but no window. A program opened from the
//! file manager is classified and launched the moment the request arrives (the launch never
//! waits for a window); windows open on demand on this thread, one at a time, over the same
//! service, and closing one returns here instead of ending the process.
//!
//! While no window is open the service's events are compacted
//! (`ServiceBackend::settle_unattended`): a setup sheet or prompt that waits for an answer
//! opens the full window; everything else waits for the next window or is dropped. The
//! process ends when the service stops, when the windowing event loop cannot be created
//! (it exists once per process), or on a signal. Started by the `zaun.service` systemd
//! user unit, whose `KillMode=process` leaves running games alone when it stops.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use zaun_core::desktop::instance::{self, Claim, InstanceServer, RequestHandler};
use zaun_core::desktop::notify::DesktopNotifier;
use zaun_core::diagnostics::crash::CrashPhase;
use zaun_core::fsx::{Fs, RealFs};
use zaun_core::paths::{AppPaths, PathEnv};
use zaun_core::service::AppService;
use zaun_core::storage::StorageError;
use zaun_core::storage::lock::{LibraryLock, LockMode};
use zaun_core::{Error, ErrorCode};
use zaun_ui::service_backend::ServiceBackend;
use zaun_ui::theme::SystemPrefs;
use zaun_ui::{DisplayBackend, Renderer, StartMode, WindowStore};

use crate::cli::exit;
use crate::display_libs;
use crate::instance_gui::{GuiRequests, LateHandler, UiWaker, WindowRequest, Windows};
use crate::notify_watch::NotifyWatch;
use crate::run_gui::{self, FileWindowStore, desktop_settings};
use crate::wiring::{self, Wiring};

/// How long the loop sleeps without a request or service event (a safety net: both wake it).
const IDLE: Duration = Duration::from_secs(30);
/// How often to try again while an Zaun window started on its own holds the instance.
const CLAIM_RETRY: Duration = Duration::from_secs(5);
/// How long to wait for finished onboarding to reach the preferences snapshot.
const ONBOARDING_SAVED: Duration = Duration::from_secs(2);

/// Become the single instance. An Zaun window started without the service holds it until
/// it closes; wait for that rather than exiting, so the service is there afterwards.
fn claim(paths: &AppPaths, uid: u32, late: &Arc<LateHandler>) -> Result<InstanceServer, Error> {
    let mut said = false;
    loop {
        let h = Arc::clone(late);
        match instance::claim(paths, uid, move || h as Arc<dyn RequestHandler>) {
            Ok(Claim::Primary(server)) => return Ok(server),
            Ok(Claim::Forward(_)) => {}
            Err(e) if e.code == ErrorCode::DskInstanceUnresponsive => {}
            Err(e) => return Err(e),
        }
        if !std::mem::replace(&mut said, true) {
            tracing::info!("another Zaun is open; the service starts when it closes");
        }
        std::thread::sleep(CLAIM_RETRY);
    }
}

/// The desktop's appearance, followed once for the process and handed to each window.
struct SystemRelay {
    latest: Arc<Mutex<SystemPrefs>>,
    current: Arc<Mutex<Option<Sender<SystemPrefs>>>>,
}

impl SystemRelay {
    fn start(waker: UiWaker) -> Self {
        let latest = Arc::new(Mutex::new(SystemPrefs {
            locale: desktop_settings::locale(),
            ..SystemPrefs::default()
        }));
        let current: Arc<Mutex<Option<Sender<SystemPrefs>>>> = Arc::new(Mutex::new(None));
        let w = Arc::clone(&waker);
        let rx = desktop_settings::follow(Arc::new(move || {
            if let Some(f) = w.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
                f();
            }
        }));
        let (l, c) = (Arc::clone(&latest), Arc::clone(&current));
        let _ = std::thread::Builder::new()
            .name("desktop-settings-relay".into())
            .spawn(move || {
                for prefs in rx {
                    *l.lock().unwrap_or_else(PoisonError::into_inner) = prefs.clone();
                    if let Some(tx) = c.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
                        let _ = tx.send(prefs);
                    }
                }
            });
        Self { latest, current }
    }

    /// The preferences now, and their changes for one window.
    fn for_window(&self) -> (SystemPrefs, Receiver<SystemPrefs>) {
        let (tx, rx) = channel();
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = Some(tx);
        (
            self.latest
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            rx,
        )
    }
}

/// How a window ended.
enum Shown {
    /// It closed.
    Closed {
        /// It was onboarding's own window.
        onboarding: bool,
    },
    /// The windowing event loop failed; no window can open in this process again.
    EventLoop(String),
}

/// Everything a window is made from.
struct Resident {
    env: PathEnv,
    paths: AppPaths,
    backend: Arc<ServiceBackend>,
    wiring: Wiring,
    windows: Arc<Windows>,
    waker: UiWaker,
    focus: Arc<AtomicBool>,
    system: SystemRelay,
    renderer: Renderer,
    reset_geometry: bool,
    display: Option<DisplayBackend>,
}

impl Resident {
    /// Open the window for `req` and run it until it closes (with the wgpu → glow fallback
    /// of V-301 on the same event loop).
    fn show(&mut self, req: &WindowRequest) -> Result<Shown, Error> {
        let display = match self.display {
            Some(d) => d,
            None => {
                let d = display_libs::choose(&self.env, display_libs::Loadable::probe())?;
                *self.display.insert(d)
            }
        };
        loop {
            let renderer = self.renderer;
            let d = self.deps(req, display, renderer);
            let onboarding = zaun_ui::opens_onboarding(&d);
            crate::panic::set_phase(CrashPhase::Graphics);
            let r = zaun_ui::run(StartMode::Library, d);
            crate::panic::set_phase(CrashPhase::Running);
            // Also when the window never got to `on_close` (it failed to open).
            self.windows.closed(Vec::new());
            match r {
                Ok(()) => return Ok(Shown::Closed { onboarding }),
                Err(e) if zaun_ui::is_event_loop_error(&e) => {
                    return Ok(Shown::EventLoop(e.to_string()));
                }
                Err(e) if renderer == Renderer::Wgpu => {
                    tracing::warn!("wgpu renderer failed ({e}); retrying with glow");
                    self.renderer = Renderer::Glow;
                    self.windows.begin();
                }
                Err(e) => return Err(Error::new(ErrorCode::HostNoDisplay).with("detail", e)),
            }
        }
    }

    fn deps(
        &self,
        req: &WindowRequest,
        display: DisplayBackend,
        renderer: Renderer,
    ) -> zaun_ui::UiDeps {
        let file = self.paths.window_state_file.clone();
        let window = if self.reset_geometry {
            zaun_ui::WindowGeometry::default()
        } else {
            run_gui::load_geometry(&file)
        };
        let store = Box::new(FileWindowStore::spawn(file)) as Box<dyn WindowStore>;
        let (open, add_game) = match req {
            WindowRequest::QuickLaunch { path, .. } => (vec![path.clone()], false),
            WindowRequest::Library { add_game } => (Vec::new(), *add_game),
        };
        let mut d = run_gui::ui_deps(
            Box::new(Arc::clone(&self.backend)),
            run_gui::art_factory(&self.wiring),
            open,
            window,
            Some(store),
            renderer,
        );
        d.open_sent = true;
        d.add_game = add_game;
        let (tx, rx) = channel();
        self.windows.opened(tx);
        d.activations = Some(rx);
        d.focus = Some(Arc::clone(&self.focus));
        let w = Arc::clone(&self.waker);
        d.on_waker = Some(Box::new(move |f| {
            // Called once the renderer is up: a later panic is not a graphics failure.
            crate::panic::set_phase(CrashPhase::Running);
            *w.lock().unwrap_or_else(PoisonError::into_inner) = Some(f);
        }));
        let windows = Arc::clone(&self.windows);
        d.on_close = Some(Box::new(move |unread| windows.closed(unread)));
        d.display = display;
        let (system, changes) = self.system.for_window();
        d.appearance.reduce_motion = system.reduced_motion.unwrap_or(false);
        d.system = system;
        d.system_changes = Some(changes);
        d
    }

    /// Wait (briefly) until finished onboarding shows in the preferences snapshot.
    fn onboarding_saved(&self) -> bool {
        let step = Duration::from_millis(50);
        let mut waited = Duration::ZERO;
        loop {
            if self.backend.handle().hub.prefs.load().onboarding.completed {
                return true;
            }
            if waited >= ONBOARDING_SAVED {
                return false;
            }
            std::thread::sleep(step);
            waited += step;
        }
    }
}

/// Run the resident service until the application service stops.
pub fn run() -> Result<u8, Error> {
    let env = PathEnv::from_process();
    let paths = AppPaths::resolve(&env)
        .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with("paths", e))?;
    let late = Arc::new(LateHandler::default());
    let _server = claim(&paths, env.uid(), &late)?;
    // PERSISTENCE §6: shared for the service's lifetime, as for the GUI.
    RealFs::new().create_dir_all(&paths.data_dir, 0o700)?;
    let _lock =
        LibraryLock::try_acquire(&paths.library_lock, LockMode::Shared).map_err(|e| match e {
            StorageError::LockHeld => Error::new(ErrorCode::LibBusy),
            other => other.into(),
        })?;
    let advice = run_gui::crash_advice(&paths);
    let renderer = run_gui::first_renderer(&env, &advice);

    let mut wiring = wiring::build()?;
    let deps = wiring.take_deps()?;
    let home = deps.launch_env.home.clone();
    let user_dirs = deps.launch_env.user_dirs.clone();
    let (handle, events) = AppService::start(deps)?;
    let backend = Arc::new(ServiceBackend::new(handle, events).with_user_dirs(home, user_dirs));

    let waker: UiWaker = Arc::new(Mutex::new(None));
    let windows = Arc::new(Windows::new(Arc::clone(&waker)));
    let hub = Arc::clone(&backend.handle().hub);
    late.attach(GuiRequests {
        cmds: backend.handle().sender(),
        hub: Arc::clone(&hub),
        windows: Arc::clone(&windows),
        db_file: paths.db_file.clone(),
    });
    // Without a window nothing has focus, so crashes and failures to start are announced.
    let focus = Arc::new(AtomicBool::new(false));
    let w2 = Arc::clone(&windows);
    let notifier = Arc::new(DesktopNotifier::new(Some(Arc::new(move || {
        w2.raise(None, false);
    }))));
    let _watch = NotifyWatch::spawn(hub, paths.db_file.clone(), Arc::clone(&focus), notifier);
    let poke: Arc<dyn Fn() + Send + Sync> = {
        let w = Arc::clone(&windows);
        Arc::new(move || w.poke())
    };
    backend.handle().set_waker(Arc::clone(&poke));
    crate::panic::set_phase(CrashPhase::Running);
    tracing::info!("service ready");

    let mut r = Resident {
        system: SystemRelay::start(Arc::clone(&waker)),
        env,
        paths,
        backend,
        wiring,
        windows,
        waker,
        focus,
        renderer,
        reset_geometry: advice.reset_geometry,
        display: None,
    };
    loop {
        let unattended = r.backend.settle_unattended();
        if unattended.quit {
            tracing::info!("application service stopped; service ends");
            return Ok(exit::OK);
        }
        let req = if unattended.needs_window {
            r.windows.begin();
            WindowRequest::Library { add_game: false }
        } else {
            match r.windows.next(IDLE) {
                Some(req) => req,
                None => continue,
            }
        };
        tracing::debug!(?req, "opening a window");
        let shown = r.show(&req);
        // Service events wake this loop again, not the closed window.
        r.backend.handle().set_waker(Arc::clone(&poke));
        r.focus.store(false, Ordering::Relaxed);
        match shown {
            Ok(Shown::Closed { onboarding }) => {
                // Finished onboarding in its own compact window: the library opens next.
                if onboarding && r.onboarding_saved() {
                    r.windows.raise(None, false);
                }
            }
            Ok(Shown::EventLoop(e)) => {
                r.backend.abandon_unattended();
                return Err(Error::new(ErrorCode::HostNoDisplay).with("detail", e));
            }
            Err(e) => {
                // No window this time; launches go on, waiting flows are cancelled.
                tracing::warn!(code = e.code.as_str(), "window not opened: {e}");
                r.windows.closed(Vec::new());
                r.backend.abandon_unattended();
                if e.code.area() == "HOST" && r.display.is_none() {
                    return Err(e);
                }
            }
        }
    }
}
