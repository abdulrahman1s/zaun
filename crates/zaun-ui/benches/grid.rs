//! `grid_frame_5000` (PERFORMANCE §1, V-304, PERF-001): UI-thread time per frame of the
//! library grid with 5 000 games while scrolling at several speeds, measured headlessly
//! through `egui_kittest` like PERFORMANCE §12 describes: the frame (logic, layout, paint)
//! plus tessellation, which eframe also runs on the UI thread. GPU work is not included.
//!
//! Also `list_frame_5000` (the list view) and `search_frame_5000` (a keystroke in the
//! search field, UI side).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Event, Modifiers, MouseWheelSource, MouseWheelUnit, TouchPhase, Vec2, pos2, vec2};
use egui_kittest::Harness;
use zaun_core::clock::Timestamp;
use zaun_core::library::index::{self, DefaultRuntimeInfo, Overlays};
use zaun_core::model::ArtKey;
use zaun_core::storage::{Db, GameRepo, StorageError};
use zaun_testkit::FakeClock;
use zaun_testkit::bench::{Bench, Report, finish};
use zaun_testkit::fixtures::games::{BASE_MS, populate};
use zaun_ui::ZaunApp;
use zaun_ui::bridge::{FakeBackend, LibraryView, UiBridge};
use zaun_ui::route::Route;
use zaun_ui::textures::synthetic;
use zaun_ui::theme::{self, Appearance};
use zaun_ui::vm::library::ViewMode;

fn library(n: u32) -> LibraryView {
    let mut db = Db::open_in_memory().unwrap();
    db.transaction(|tx| Ok::<_, StorageError>(populate(tx, n, 4, 3)))
        .unwrap();
    let rows = db.load_library().unwrap();
    let mut s = index::build(
        &rows,
        &DefaultRuntimeInfo,
        &Overlays::default(),
        1,
        Timestamp::from_millis(BASE_MS),
    );
    for (i, g) in s.games.iter_mut().enumerate() {
        let mut h = [0u8; 32];
        h[..8].copy_from_slice(&(i as u64 + 1).to_le_bytes());
        g.cover = Some(ArtKey(h));
    }
    s
}

fn harness(lib: LibraryView, view: ViewMode) -> Harness<'static, ZaunApp> {
    // `FakeBackend` serves the sample preferences (onboarding done).
    let fake = FakeBackend::new(lib);
    let appearance = Appearance::default();
    let mut app = ZaunApp::new(
        UiBridge::new(Box::new(fake)),
        Arc::new(FakeClock::new(Timestamp::from_millis(BASE_MS))),
        appearance,
        Route::Library,
        0,
    )
    .with_art(Box::new(|_| Box::new(synthetic::Inline::tiny())));
    app.state.library.view = view;
    let mut h = Harness::builder()
        .with_size(Vec2::new(1920.0, 1080.0))
        .with_context_setup(move |ctx| theme::install(ctx, &appearance))
        .build_eframe(move |_cc| app);
    for _ in 0..20 {
        h.step();
    }
    h
}

/// One frame as the UI thread pays for it: the pass and tessellation.
fn frame(h: &mut Harness<'static, ZaunApp>) -> Duration {
    let t = Instant::now();
    h.step();
    let shapes = h.output().shapes.clone();
    let ppp = h.output().pixels_per_point;
    let before_clone = t.elapsed();
    let t2 = Instant::now();
    let prims = h.ctx.tessellate(shapes, ppp);
    std::hint::black_box(prims);
    before_clone + t2.elapsed()
}

fn wheel(h: &Harness<'static, ZaunApp>, dy: f32) {
    h.event(Event::PointerMoved(pos2(1100.0, 600.0)));
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, dy),
        phase: TouchPhase::Move,
        source: MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
}

/// Scroll down then up at slow, medium and fling speeds; one sample per frame.
fn scroll_samples(h: &mut Harness<'static, ZaunApp>, frames_per_speed: usize) -> Vec<Duration> {
    let mut out = Vec::new();
    for speed in [-40.0, -400.0, -4000.0, 4000.0, 400.0, 40.0] {
        for _ in 0..frames_per_speed {
            wheel(h, speed);
            out.push(frame(h));
        }
    }
    out
}

fn main() {
    let n = if zaun_testkit::bench::optimized() {
        5_000
    } else {
        300
    };
    let frames = if zaun_testkit::bench::optimized() {
        60
    } else {
        2
    };
    let lib = library(n);
    let mut reports: Vec<Option<Report>> = Vec::new();

    for (name, view) in [
        ("grid_frame_5000", ViewMode::Grid),
        ("list_frame_5000", ViewMode::List),
    ] {
        let b = Bench::new(name, Duration::from_millis(4));
        if b.selected() {
            let mut h = harness(lib.clone(), view);
            let samples = scroll_samples(&mut h, frames);
            reports.push(Some(b.report(samples.clone())));
            let p99 = Bench::new(format!("{name}_p99"), Duration::from_millis(8)).p99();
            reports.push(Some(p99.report(samples)));
        }
    }

    let b = Bench::new("idle_frame_5000", Duration::from_millis(4));
    if b.selected() {
        let mut h = harness(lib.clone(), ViewMode::Grid);
        let samples = (0..frames * 2).map(|_| frame(&mut h)).collect();
        reports.push(Some(b.report(samples)));
    }

    finish(&reports);
}
