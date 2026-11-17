//! Review renders of the controls (throwaway): `cargo test -p easyplay-ui
//! --features review --test review review_controls:: -- --ignored
//! --test-threads=1`. Images land in `target/review/`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use easyplay_ui::theme::{self, Appearance, ThemeChoice, Type, icons};
use easyplay_ui::widgets::button::{
    Button, PillSize, PlayState, Size, Variant, play_disc, play_disc_blocked, play_pill,
    play_pill_blocked,
};
use easyplay_ui::widgets::choice::{PickKind, check_mark, checkbox, pick, radio_mark, swatch};
use easyplay_ui::widgets::icon_button::IconButton;
use easyplay_ui::widgets::key_hint::key_hint;
use easyplay_ui::widgets::progress;
use easyplay_ui::widgets::segmented::{Segment, segmented};
use easyplay_ui::widgets::select::{select, select_inline};
use easyplay_ui::widgets::slider::slider;
use easyplay_ui::widgets::tabs::Tabs;
use easyplay_ui::widgets::text_field::TextField;
use easyplay_ui::widgets::toggle::switch;
use easyplay_ui::widgets::word::{Word, word_on, word_rule, word_strong, word_text};
use egui::{Color32, CornerRadius, Rect, Ui, Vec2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

#[derive(Default)]
struct Page {
    theme_choice: usize,
    motion: usize,
    view: usize,
    size: f32,
    on: bool,
    off: bool,
    version: usize,
    pads: usize,
    tab: usize,
    query: String,
    filter: String,
    field: String,
    notes: String,
    wrap: bool,
}

fn caption(ui: &mut Ui, text: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.add_sized(
        vec2(120.0, 30.0),
        egui::Label::new(Type::Caption.rich(text).color(p.t3)),
    );
}

fn row(ui: &mut Ui, name: &str, f: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        caption(ui, name);
        f(ui);
    });
    ui.add_space(6.0);
}

fn page(ui: &mut Ui, s: &mut Page) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.painter()
        .rect_filled(ui.max_rect().expand(40.0), 0.0, p.bg);
    ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
    let variants = [
        ("Ghost", Variant::Ghost),
        ("Box", Variant::Secondary),
        ("Primary", Variant::Primary),
        ("Danger", Variant::Danger),
        ("Warn", Variant::Warn),
        ("Link", Variant::Link),
    ];
    for (name, v) in variants {
        row(ui, name, |ui| {
            let _ = ui.add(Button::new("Copy").variant(v));
            let _ = ui.add(
                Button::new("Settings")
                    .icon(icons::PLAY)
                    .variant(v)
                    .size(Size::Sm),
            );
            let _ = ui.add(Button::new("Show as table").variant(v).size(Size::Xs));
            let _ = ui.add(Button::new("Disabled").variant(v).disabled(Some("Why")));
            let _ = ui.add(Button::new("Starting…").variant(v).busy(true));
            let _ = ui.add(Button::new("Sort").variant(v).caret());
            let _ = ui.add(
                Button::new("More")
                    .icon(icons::DOTS)
                    .icon_only(true)
                    .variant(v),
            );
        });
    }
    row(ui, "Play", |ui| {
        for st in [
            PlayState::Ready,
            PlayState::Launching("Starting…"),
            PlayState::Running,
            PlayState::Unavailable,
        ] {
            let _ = play_pill(ui, &st, "Lantern Keep", PillSize::Page, 1.0);
        }
        let _ = play_pill_blocked(
            ui,
            "Lantern Keep",
            "Stop Moss & Iron first",
            PillSize::Page,
            1.0,
        );
        let _ = ui.add(
            IconButton::new(icons::STAR, "Favourite")
                .round(true)
                .toggle(true),
        );
        let _ = ui.add(IconButton::new(icons::DOTS, "More").round(true));
    });
    row(ui, "Compact, row", |ui| {
        for st in [
            PlayState::Ready,
            PlayState::Running,
            PlayState::Launching("Starting…"),
        ] {
            let _ = play_pill(ui, &st, "Lantern Keep", PillSize::Compact, 1.0);
        }
        for st in [
            PlayState::Ready,
            PlayState::Running,
            PlayState::Launching("Starting…"),
        ] {
            let _ = play_pill(ui, &st, "Lantern Keep", PillSize::Row, 1.0);
        }
        let _ = ui.add(IconButton::new(icons::GRID, "Covers"));
        let _ = ui.add(IconButton::new(icons::LIST, "List").selected(true));
    });
    row(ui, "On covers", |ui| {
        let papers = [
            Color32::from_rgb(0xD9, 0xD3, 0xC4),
            Color32::from_rgb(0x2A, 0x33, 0x3D),
        ];
        for (i, st) in [
            PlayState::Ready,
            PlayState::Running,
            PlayState::Launching("Starting…"),
            PlayState::Unavailable,
        ]
        .iter()
        .enumerate()
        {
            for paper in papers {
                let (r, _) = ui.allocate_exact_size(vec2(60.0, 60.0), egui::Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(4), paper);
                let disc = Rect::from_center_size(r.center(), Vec2::splat(30.0));
                let _ = play_disc(
                    ui,
                    disc,
                    egui::Id::unique(("disc", i, paper.r())),
                    st,
                    "Lantern Keep",
                    1.0,
                );
            }
        }
        let (r, _) = ui.allocate_exact_size(vec2(60.0, 60.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(r, CornerRadius::same(4), papers[0]);
        let disc = Rect::from_center_size(r.center(), Vec2::splat(40.0));
        let _ = play_disc_blocked(
            ui,
            disc,
            egui::Id::unique("blocked"),
            "Lantern Keep",
            "Stop it first",
            1.0,
        );
    });
    row(ui, "Filter line", |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let _ = ui.add(Word::new("All").count(17).pressed(true));
        let _ = ui.add(Word::new("Favourites").count(4).pressed(false));
        let _ = ui.add(Word::new("Hidden").count(1));
        word_rule(ui);
        let _ = ui.add(Word::new("Filter").open(false));
        let _ = word_on(ui, "Offline", None, "Remove filter");
        let _ = word_on(ui, "rpg", Some(p.tag_dot("rpg")), "Remove filter");
        let _ = word_text(ui, "13 of 17");
        let _ = ui.add(
            Word::new("Recently played")
                .caret()
                .tip("Sort and group", None),
        );
    });
    row(ui, "Selection, meta", |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let _ = ui.add(
            Word::new("Clear selection")
                .icon(icons::CLOSE)
                .icon_only()
                .tip("Clear selection", Some("Esc")),
        );
        let _ = word_strong(ui, "3 selected");
        let _ = ui.add(Word::new("Tags"));
        let _ = ui.add(Word::new("Remove…"));
        word_rule(ui);
        let _ = ui.add(Word::new("Offline").icon(icons::SHIELD_STANDARD).strong());
        let _ = ui.add(Word::new("rpg").dot(p.tag_dot("rpg")).strong());
        let _ = ui.add(Word::new("Tag").icon(icons::PLUS));
    });
    row(ui, "Segmented", |ui| {
        let opts = ["System", "Dark", "Light", "High contrast"].map(|l| Segment {
            icon: None,
            label: l,
        });
        let _ = segmented(ui, "Theme", &mut s.theme_choice, &opts, false);
        let opts = ["System", "On", "Off"].map(|l| Segment {
            icon: None,
            label: l,
        });
        let _ = segmented(ui, "Reduce motion", &mut s.motion, &opts, false);
        let opts = [
            Segment {
                icon: Some(icons::GRID),
                label: "Covers",
            },
            Segment {
                icon: Some(icons::LIST),
                label: "List",
            },
        ];
        let _ = segmented(ui, "Show as", &mut s.view, &opts, true);
        let mut none = 9;
        let _ = segmented(ui, "None", &mut none, &opts[..1], false);
    });
    row(ui, "Swatch, slider", |ui| {
        for (i, a) in theme::Accent::ALL.iter().enumerate() {
            let _ = swatch(ui, a.swatch(), i == 0, a.as_str());
        }
        ui.add_space(20.0);
        let _ = slider(
            ui,
            &mut s.size,
            75.0..=200.0,
            5.0,
            150.0,
            "Interface size",
            "100 %",
        );
        let _ = ui.add(Button::ghost("Reset").disabled(Some("Already 100 %")));
    });
    row(ui, "Switch, select", |ui| {
        let _ = switch(ui, &mut s.on, "Switch A");
        let _ = switch(ui, &mut s.off, "Switch B");
        let _ = select(
            ui,
            "Version",
            None,
            None,
            &mut s.version,
            &["UMU-Proton 10.0-4", "Wine 9"],
            false,
        );
        let _ = select_inline(
            ui,
            "Which controllers",
            &mut s.pads,
            &["All controllers", "First"],
        );
        let _ = key_hint(ui, "/");
        let _ = key_hint(ui, "Ctrl+K");
        let _ = progress::spinner_widget(ui, p.t1);
        let _ = progress::live(ui, p.accent_text);
    });
    row(ui, "Fields", |ui| {
        let _ = TextField::new(&mut s.query, "search")
            .search()
            .hint("Search")
            .shortcut_hint("/")
            .accessible_name("Search games")
            .width(208.0)
            .show(ui);
        let _ = TextField::new(&mut s.filter, "filter")
            .search()
            .hint("Filter")
            .accessible_name("Filter log lines")
            .width(208.0)
            .show(ui);
        let _ = TextField::new(&mut s.field, "field")
            .hint("NAME")
            .mono()
            .accessible_name("Variable name")
            .width(200.0)
            .show(ui);
        let _ = checkbox(ui, &mut s.wrap, "Wrap");
        let _ = check_mark(ui, true);
        let _ = check_mark(ui, false);
        let _ = radio_mark(ui, true);
        let _ = radio_mark(ui, false);
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        caption(ui, "Notes, picks");
        ui.vertical(|ui| {
            ui.set_width(360.0);
            let _ = TextField::new(&mut s.notes, "notes")
                .multiline()
                .accessible_name("Notes")
                .show(ui);
        });
        ui.vertical(|ui| {
            ui.set_width(360.0);
            let _ = pick(ui, PickKind::Radio, true, "Restore a backup", |ui| {
                ui.label(Type::Body.rich("Restore a backup").color(p.t1));
                ui.label(Type::Caption.rich("Yesterday, 12 games").color(p.t3));
            });
            let _ = pick(ui, PickKind::Radio, false, "Start empty", |ui| {
                ui.label(Type::Body.rich("Start with an empty library").color(p.t1));
            });
            let _ = pick(ui, PickKind::Check, true, "Moss.exe", |ui| {
                ui.label(Type::Body.rich("Moss.exe").color(p.t1));
            });
        });
    });
    ui.add_space(10.0);
    ui.allocate_ui(vec2(720.0, 40.0), |ui| {
        let names = [
            "Overview",
            "Protection",
            "Compatibility",
            "Launch options",
            "Activity",
            "Logs",
        ];
        let _ = Tabs::new("Game", &names).show_with(ui, &mut s.tab, |ui| {
            play_pill(
                ui,
                &PlayState::Ready,
                "Lantern Keep",
                PillSize::Compact,
                1.0,
            )
        });
    });
    let _ = pos2(0.0, 0.0);
}

fn harness(choice: ThemeChoice) -> Harness<'static, Page> {
    harness_with(choice, false, 0.25)
}

fn harness_with(choice: ThemeChoice, reduced: bool, step: f32) -> Harness<'static, Page> {
    let appearance = Appearance {
        theme: choice,
        reduce_motion: reduced,
        ..Appearance::default()
    };
    Harness::builder()
        .with_step_dt(step)
        .with_size(Vec2::new(1280.0, 860.0))
        .with_theme(if choice == ThemeChoice::Light {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        })
        .with_context_setup(move |ctx| theme::install(ctx, &appearance))
        .build_ui_state(
            page,
            Page {
                size: 100.0,
                on: true,
                notes: "Finished the main story. Try the hard mode next.".into(),
                theme_choice: 1,
                ..Page::default()
            },
        )
}

fn save(h: &mut Harness<'static, Page>, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/review");
    std::fs::create_dir_all(&dir).unwrap();
    let image = h.render().expect("render");
    image.save(dir.join(format!("{name}.png"))).unwrap();
}

fn shot(choice: ThemeChoice, name: &str) {
    let mut h = harness(choice);
    h.run_steps(20);
    save(&mut h, name);
}

#[test]
#[ignore = "review render"]
fn controls_dark() {
    shot(ThemeChoice::Dark, "controls-dark");
}

#[test]
#[ignore = "review render"]
fn controls_light() {
    shot(ThemeChoice::Light, "controls-light");
}

#[test]
#[ignore = "review render"]
fn controls_hc() {
    shot(ThemeChoice::HighContrast, "controls-hc");
}

#[test]
#[ignore = "review render"]
fn controls_focused() {
    let mut h = harness(ThemeChoice::Dark);
    h.run_steps(5);
    h.get_by_label("Search games").focus();
    h.run_steps(20);
    save(&mut h, "controls-focus-search");
    h.key_press(egui::Key::Tab);
    h.run_steps(20);
    save(&mut h, "controls-focus-next");
}

#[test]
#[ignore = "review render"]
fn controls_hover() {
    let mut h = harness(ThemeChoice::Dark);
    h.run_steps(5);
    h.get_all_by_label("Play Lantern Keep")
        .next()
        .unwrap()
        .hover();
    h.run_steps(20);
    save(&mut h, "controls-hover-play");
}

/// Mid-flight: the thumb, the knob, the tab underline and a radio, 60 ms after a click.
fn mid_flight(reduced: bool, name: &str) {
    let mut h = harness_with(ThemeChoice::Dark, reduced, 0.03);
    h.run_steps(10);
    h.get_by_label("High contrast").click();
    h.get_by_label("Switch B").click();
    h.get_by_label("Logs").click();
    h.run_steps(2);
    save(&mut h, name);
    h.run_steps(20);
    save(&mut h, &format!("{name}-settled"));
}

#[test]
#[ignore = "review render"]
fn controls_motion() {
    mid_flight(false, "controls-motion");
}

#[test]
#[ignore = "review render"]
fn controls_motion_reduced() {
    mid_flight(true, "controls-motion-reduced");
}
