//! Construct the real implementations of every trait, start the AppService and call
//! `easyplay_ui::run`. Never reachable from the supervisor path. Task UI-001.
//!
//! Until the AppService exists (SVC-002) the window runs on the UI crate's preview backend:
//! a sample library with generated covers. Window geometry is read from `window.json` before
//! the window opens and written by a background thread (PERSISTENCE §1). The renderer is
//! `wgpu`, or `glow` when `EASYPLAY_RENDERER=glow` or when `wgpu` fails to start (V-301).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};

use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::consts::ENV_RENDERER;
use easyplay_core::fsx::{Fs, RealFs};
use easyplay_core::paths::{AppPaths, PathEnv};
use easyplay_core::{Error, ErrorCode};
use easyplay_ui::textures::synthetic;
use easyplay_ui::theme::Appearance;
use easyplay_ui::{Renderer, StartMode, UiDeps, WindowGeometry, WindowStore, demo};
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

fn deps(window: WindowGeometry, store: Option<Box<dyn WindowStore>>, renderer: Renderer) -> UiDeps {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let now = clock.now();
    UiDeps {
        backend: Box::new(demo::DemoBackend::new(now)),
        clock,
        appearance: Appearance {
            ui_scale: window.ui_scale,
            ..Appearance::default()
        },
        window,
        window_store: store,
        utc_offset_s: utc_offset_seconds(),
        renderer,
        art: Some(Box::new(|ctx| {
            let ctx = ctx.clone();
            Box::new(synthetic::Threaded::spawn(move || ctx.request_repaint()))
        })),
    }
}

/// Start the GUI (optionally with files to open in the quick-launch layout).
pub fn run(open: Vec<PathBuf>) -> Result<(), Error> {
    if !open.is_empty() {
        // Quick launch needs the AppService (SVC-003).
        return Err(Error::not_available("SVC-003"));
    }
    let env = PathEnv::from_process();
    let paths = AppPaths::resolve(&env).ok();
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

    match easyplay_ui::run(StartMode::Library, deps(window, store(), first)) {
        Ok(()) => Ok(()),
        Err(e) if first == Renderer::Wgpu => {
            // V-301: fall back to GL. eframe keeps its event loop for a second run.
            tracing::warn!("wgpu renderer failed ({e}); retrying with glow");
            easyplay_ui::run(StartMode::Library, deps(window, store(), Renderer::Glow))
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
}
