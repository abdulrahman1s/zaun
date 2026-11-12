//! PERF-002: blocking calls from the UI thread fail UI tests (PERFORMANCE §2).
//!
//! A backend that performs real I/O while the frame runs (a stand-in for a screen or
//! bridge that forgot to go through the service) must panic in debug builds; the same calls
//! between frames, on the test thread, must not.

#![cfg(debug_assertions)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;

use easyplay_core::config::Preferences;
use easyplay_core::fsx::{Fs, RealFs};
use easyplay_core::ids::GameId;
use easyplay_core::library::LibrarySnapshot;
use easyplay_core::model::{BasePreset, SandboxPolicy};
use easyplay_core::storage::Db;
use easyplay_ui::bridge::{
    Backend, FakeBackend, HostView, PolicyAssessment, RuntimesView, UiCommand, UiEvent,
};
use easyplay_ui::route::Route;
use easyplay_ui::theme::ThemeChoice;
use egui::Vec2;

#[derive(Clone, Copy)]
enum Blocking {
    ReadDir,
    Sql,
}

/// Delegates to a [`FakeBackend`] but does blocking work in `try_event`.
struct IoBackend {
    inner: FakeBackend,
    what: Blocking,
}

impl Backend for IoBackend {
    fn send(&self, cmd: UiCommand) {
        self.inner.send(cmd);
    }
    fn try_event(&self) -> Option<UiEvent> {
        match self.what {
            Blocking::ReadDir => {
                let _ = RealFs::new().read_dir(std::path::Path::new("/"));
            }
            Blocking::Sql => {
                let _ = Db::open_in_memory();
            }
        }
        self.inner.try_event()
    }
    fn library(&self) -> Arc<LibrarySnapshot> {
        self.inner.library()
    }
    fn preferences(&self) -> Arc<Preferences> {
        self.inner.preferences()
    }
    fn host(&self) -> Arc<HostView> {
        self.inner.host()
    }
    fn runtimes(&self) -> Arc<RuntimesView> {
        self.inner.runtimes()
    }
    fn assess(
        &self,
        game: Option<GameId>,
        name: &str,
        root: Option<&std::path::Path>,
        policy: &SandboxPolicy,
    ) -> PolicyAssessment {
        self.inner.assess(game, name, root, policy)
    }
    fn preset_policy(&self, preset: BasePreset, policy: &SandboxPolicy) -> SandboxPolicy {
        self.inner.preset_policy(preset, policy)
    }
    fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        self.inner.set_waker(waker);
    }
}

fn run_frames(what: Blocking) {
    let backend = IoBackend {
        inner: FakeBackend::new(common::settled_library()),
        what,
    };
    // Outside a frame the test thread is not the UI thread.
    let _ = backend.try_event();
    let mut r = common::rig_backend(
        Box::new(backend),
        Route::Library,
        Vec2::new(800.0, 600.0),
        ThemeChoice::Dark,
    );
    r.run_steps(2);
}

#[test]
#[should_panic(expected = "blocking call on the UI thread: Fs::read_dir")]
fn file_access_from_a_frame_fails() {
    run_frames(Blocking::ReadDir);
}

#[test]
#[should_panic(expected = "blocking call on the UI thread: Store::open")]
fn sql_from_a_frame_fails() {
    run_frames(Blocking::Sql);
}

#[test]
fn io_between_frames_is_allowed() {
    let mut r = common::rig(Route::Library);
    r.h.run_steps(2);
    assert!(!easyplay_ui::app::is_ui_thread());
    RealFs::new().read_dir(std::path::Path::new("/")).unwrap();
    r.h.run_steps(1);
}
