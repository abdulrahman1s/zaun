//! A11Y-005: every screen at 100 %, 150 % and 200 % UI scale, at 640 × 480 and 1280 × 800
//! logical points (ACCESSIBILITY §3), with an assertion that no text is cut off sideways by
//! its container, and snapshots of the key screens at 150 % and 200 %.
//!
//! Sizes are logical: at 200 % a 1280 × 800 layout renders into 2560 × 1600 pixels, as on a
//! high-density screen. Horizontal clipping is the failure that matters: content never
//! scrolls sideways (USER_EXPERIENCE §3), so text cut at a container's left or right edge is
//! text that should have wrapped or been ellipsized. Vertical cuts at a scroll viewport's
//! edge are normal scrolling and are not reported. The one exception is a tab bar, whose
//! words scroll sideways when they do not fit, as the design's do.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::common;

use common::{Rig, game_id, rig_scaled, settle, settled_library};
use egui::epaint::{ClippedShape, Shape};
use egui::{Rect, Vec2};
use zaun_ui::route::{Overlay, Route, SettingsTab};
use zaun_ui::theme::ThemeChoice;
use zaun_ui::vm::details::DetailsTab;

const SCALES: [f32; 3] = [1.0, 1.5, 2.0];
/// The two window sizes of ACCESSIBILITY §8, plus a tall narrow one so that content below
/// the first screenful is checked too.
const SIZES: [Vec2; 3] = [
    Vec2::new(640.0, 480.0),
    Vec2::new(1280.0, 800.0),
    Vec2::new(640.0, 2400.0),
];

/// Text shapes cut off at the left or right edge of their clip rectangle, as
/// `"text" (bounds) in (clip)`.
fn clipped_text(shapes: &[ClippedShape], screen: Rect) -> Vec<String> {
    fn walk(
        shape: &Shape,
        clip: Rect,
        tf: egui::emath::TSTransform,
        screen: Rect,
        out: &mut Vec<String>,
    ) {
        match shape {
            Shape::Vec(v) => {
                for s in v {
                    walk(s, clip, tf, screen, out);
                }
            }
            Shape::Text(t) => {
                if t.galley.is_empty() || t.galley.text().trim().is_empty() {
                    return;
                }
                let b = tf * t.galley.mesh_bounds.translate(t.pos.to_vec2());
                let visible = clip.intersect(screen);
                // Only text that is (partly) on screen vertically counts.
                if b.max.y <= visible.min.y || b.min.y >= visible.max.y || b.width() < 1.0 {
                    return;
                }
                const SLACK: f32 = 1.0;
                if b.min.x < visible.min.x - SLACK || b.max.x > visible.max.x + SLACK {
                    out.push(format!("{:?} {:?} in {:?}", t.galley.text(), b, visible));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in shapes {
        walk(
            &s.shape,
            s.clip_rect,
            s.transform_after_tessellation,
            screen,
            &mut out,
        );
    }
    out
}

fn check(s: &Rig, what: &str) -> Vec<String> {
    use egui_kittest::kittest::{NodeT, Queryable};
    let screen = s.h.ctx.content_rect();
    // The one strip that scrolls sideways by design: a tab bar whose words do not fit
    // (CSS `overflow-x: auto`). Its words stay reachable: arrow keys move along them and
    // the one reached scrolls into view.
    let tabs: Vec<String> =
        s.h.query_all_by(|n| n.role() == egui::accesskit::Role::Tab)
            .filter_map(|n| n.accesskit_node().label().map(|l| format!("{l:?} ")))
            .collect();
    clipped_text(&s.h.output().shapes, screen)
        .into_iter()
        .filter(|c| !tabs.iter().any(|t| c.starts_with(t.as_str())))
        .map(|c| format!("{what}: {c}"))
        .collect()
}

type Setup = fn(&mut Rig);

/// egui rescales the window rectangle once when the zoom factor changes (to avoid jitter);
/// the harness keeps sizes in points, so put the logical size back and let it settle.
fn fix_size(r: &mut Rig, logical: Vec2) {
    r.h.set_size(logical);
    r.h.run_steps(4);
    let got = r.h.ctx.content_rect().size();
    assert!(
        (got - logical).length() < 1.0,
        "window is {got:?}, wanted {logical:?}"
    );
}

fn screens() -> Vec<(&'static str, Route, Setup)> {
    let lantern = game_id(&settled_library(), "Lantern Keep");
    let tab = |t: DetailsTab| -> Setup {
        match t {
            DetailsTab::Overview => |s| s.h.state_mut().state.details.tab = DetailsTab::Overview,
            DetailsTab::Protection => {
                |s| s.h.state_mut().state.details.tab = DetailsTab::Protection
            }
            DetailsTab::Compatibility => {
                |s| s.h.state_mut().state.details.tab = DetailsTab::Compatibility
            }
            DetailsTab::LaunchOptions => {
                |s| s.h.state_mut().state.details.tab = DetailsTab::LaunchOptions
            }
            DetailsTab::Activity => |s| s.h.state_mut().state.details.tab = DetailsTab::Activity,
            DetailsTab::Logs => |s| s.h.state_mut().state.details.tab = DetailsTab::Logs,
        }
    };
    let mut v: Vec<(&'static str, Route, Setup)> = vec![
        ("library", Route::Library, |_| {}),
        ("library list", Route::Library, |s| {
            s.h.state_mut().state.library.view = zaun_ui::vm::library::ViewMode::List;
        }),
        ("statistics", Route::Statistics, |_| {}),
        ("shortcuts", Route::Library, |s| {
            s.h.state_mut().state.nav.open(Overlay::Shortcuts);
        }),
    ];
    for t in DetailsTab::ALL {
        v.push(("game page", Route::Game(lantern), tab(t)));
    }
    for t in zaun_ui::screens::settings::TABS {
        v.push(("settings", Route::Settings(t), |_| {}));
    }
    v
}

#[test]
fn no_text_is_clipped_at_any_scale_or_size() {
    let mut problems = Vec::new();
    for (name, route, setup) in screens() {
        for size in SIZES {
            for scale in SCALES {
                let mut s = rig_scaled(route, size, scale, ThemeChoice::Dark);
                setup(&mut s);
                settle(&mut s);
                fix_size(&mut s, size);
                problems.extend(check(
                    &s,
                    &format!("{name} {route:?} {}x{} @{scale}", size.x, size.y),
                ));
            }
        }
    }
    assert!(problems.is_empty(), "clipped text: {problems:#?}");
}

/// All snapshots of this file in one test: rendering several large harnesses from parallel
/// tests at once has crashed GPU drivers.
#[test]
fn snapshots() {
    let mut results = egui_kittest::SnapshotResults::new();
    scaled_snapshots(&mut results);
    narrow_snapshots(&mut results);
}

fn scaled_snapshots(results: &mut egui_kittest::SnapshotResults) {
    let lantern = game_id(&settled_library(), "Lantern Keep");
    for (name, route) in [
        ("library", Route::Library),
        ("game_overview", Route::Game(lantern)),
        (
            "settings_appearance",
            Route::Settings(SettingsTab::Appearance),
        ),
    ] {
        for (scale, tag) in [(1.5, "150"), (2.0, "200")] {
            let mut s = rig_scaled(route, Vec2::new(1280.0, 800.0), scale, ThemeChoice::Dark);
            settle(&mut s);
            fix_size(&mut s, Vec2::new(1280.0, 800.0));
            s.h.snapshot(format!("scale{tag}_{name}_dark_1280x800"));
            results.extend_harness(&mut s.h);
        }
        let mut s = rig_scaled(route, Vec2::new(640.0, 480.0), 2.0, ThemeChoice::Light);
        settle(&mut s);
        fix_size(&mut s, Vec2::new(640.0, 480.0));
        s.h.snapshot(format!("scale200_{name}_light_640x480"));
        results.extend_harness(&mut s.h);
    }
}

/// The narrow layouts the clipping check covers, at 100 %, for review.
fn narrow_snapshots(results: &mut egui_kittest::SnapshotResults) {
    let lantern = game_id(&settled_library(), "Lantern Keep");
    let cases: [(&str, Route, Setup); 3] = [
        ("game_protection_dark_640x480", Route::Game(lantern), |s| {
            s.h.state_mut().state.details.tab = DetailsTab::Protection;
        }),
        (
            "settings_layers_dark_640x480",
            Route::Settings(SettingsTab::CompatibilityLayers),
            |_| {},
        ),
        ("statistics_dark_960x700", Route::Statistics, |_| {}),
    ];
    for (name, route, setup) in cases {
        let size = if name.ends_with("960x700") {
            Vec2::new(960.0, 700.0)
        } else {
            Vec2::new(640.0, 480.0)
        };
        let mut s = rig_scaled(route, size, 1.0, ThemeChoice::Dark);
        setup(&mut s);
        settle(&mut s);
        fix_size(&mut s, size);
        s.h.snapshot(name);
        results.extend_harness(&mut s.h);
    }
}

#[test]
fn the_helper_reports_text_cut_at_the_side() {
    let galley_shape = |ctx: &egui::Context| {
        ctx.fonts_mut(|f| {
            f.layout_no_wrap(
                "A long sentence".into(),
                egui::FontId::proportional(14.0),
                egui::Color32::WHITE,
            )
        })
    };
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
    out.textures_delta.clear();
    let g = galley_shape(&ctx);
    let w = g.size().x;
    let screen = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(500.0));
    let shape = Shape::galley(egui::pos2(10.0, 10.0), g, egui::Color32::WHITE);
    let inside = ClippedShape::new(
        Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w + 40.0, 40.0)),
        shape.clone(),
    );
    let cut = ClippedShape::new(
        Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w / 2.0, 40.0)),
        shape,
    );
    assert!(clipped_text(&[inside], screen).is_empty());
    assert_eq!(clipped_text(&[cut], screen).len(), 1);
}
