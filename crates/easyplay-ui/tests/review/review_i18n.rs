//! Review renders (throwaway): `cargo test -p easyplay-ui
//! --features review --test review review_i18n:: -- --ignored
//! --test-threads=1`. Images land in `target/review/`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::common;

use common::*;
use easyplay_ui::bridge::FakeBackend;
use easyplay_ui::i18n::{self, Language};
use easyplay_ui::route::{Route, SettingsTab};
use easyplay_ui::theme::{self, Appearance, ThemeChoice, Type};
use easyplay_ui::vm::details::DetailsTab;
use egui::{Pos2, Ui, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

const AR: [&str; 5] = [
    "إضافة لعبة جديدة",
    "يحتفظ إيزي بلاي بكل لعبة في بيئة معزولة خاصة بها.",
    "تمت إضافة Lantern Keep إلى المكتبة (12 لعبة)",
    "لعبت 12 ساعة و 30 دقيقة منذ 3 أيام",
    "يحتفظ إيزي بلاي بكل لعبة في بيئة معزولة خاصة بها، فلا تستطيع لعبة قراءة ملفات لعبة أخرى، ولا تصل إلى مجلد المنزل أو إلى الشبكة إلا إذا سمحت بذلك.",
];
const EN: [&str; 5] = [
    "Add a new game",
    "EasyPlay keeps every game in its own sandbox.",
    "Lantern Keep was added to the library (12 games)",
    "Played 12 hours and 30 minutes, 3 days ago",
    "EasyPlay keeps every game in its own sandbox, so one game cannot read the files of another, and it cannot reach your home folder or the network unless you allow it.",
];

fn page(ui: &mut Ui) {
    let ink = ui.visuals().text_color();
    ui.painter()
        .rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
    let (left, right) = (24.0, 24.0 + 460.0);
    let mut y = 20.0;
    for (strings, x0, rtl) in [(&AR, left, true), (&EN, right + 40.0, false)] {
        let mut y2 = y;
        let mut put = |ui: &mut Ui, t: Type, text: &str, wrap: Option<f32>| {
            let g = match wrap {
                Some(w) => t.galley(ui, text, ink, w, 8),
                None => t.line(ui, text, ink),
            };
            let x = if rtl { x0 + 460.0 - g.size().x } else { x0 };
            let h = g.size().y;
            ui.painter().galley(Pos2::new(x, y2), g, ink);
            y2 += h + 14.0;
        };
        put(ui, Type::Hero, strings[0], None);
        put(ui, Type::Body, strings[1], None);
        put(ui, Type::Body, strings[2], None);
        put(ui, Type::BodyStrong, strings[3], None);
        put(ui, Type::Lead, strings[4], Some(300.0));
        put(ui, Type::Body, strings[4], Some(300.0));
        if !rtl {
            y = y2;
        }
    }
}

fn render(choice: ThemeChoice, name: &str) {
    i18n::set(Language::Arabic);
    let appearance = Appearance {
        theme: choice,
        ..Appearance::default()
    };
    let mut h = Harness::builder()
        .with_size(Vec2::new(1060.0, 420.0))
        .with_pixels_per_point(1.6)
        .with_theme(match choice {
            ThemeChoice::Light => egui::Theme::Light,
            _ => egui::Theme::Dark,
        })
        .with_context_setup(move |ctx| theme::install(ctx, &appearance))
        .build_ui(page);
    h.run_steps(3);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/review");
    std::fs::create_dir_all(&dir).unwrap();
    h.render()
        .expect("render")
        .save(dir.join(format!("{name}.png")))
        .unwrap();
    i18n::set(Language::English);
}

#[test]
#[ignore = "review render"]
fn i18n_dark() {
    render(ThemeChoice::Dark, "i18n-dark");
}

#[test]
#[ignore = "review render"]
fn i18n_light() {
    render(ThemeChoice::Light, "i18n-light");
}

// ---- the screens, in Arabic ----------------------------------------------------------

fn arabic(route: Route, size: Vec2, choice: ThemeChoice) -> Rig {
    let fake = FakeBackend::new(settled_library());
    let mut prefs = easyplay_ui::demo::preferences();
    prefs.general.language = "ar".into();
    fake.publish_prefs(prefs);
    let mut r = rig_on(fake, route, size, choice);
    settle(&mut r);
    r.h.run_steps(30);
    serve(&mut r);
    r.h.run_steps(30);
    r
}

fn shot(r: &mut Rig, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/review");
    std::fs::create_dir_all(&dir).unwrap();
    r.h.render()
        .expect("render")
        .save(dir.join(format!("i18n-ar-{name}.png")))
        .unwrap();
    i18n::set(Language::English);
}

fn big() -> Vec2 {
    Vec2::new(1280.0, 800.0)
}

#[test]
#[ignore = "review render"]
fn ar_library() {
    let mut r = arabic(Route::Library, big(), ThemeChoice::Dark);
    shot(&mut r, "library");
}

#[test]
#[ignore = "review render"]
fn ar_game_overview() {
    let lib = settled_library();
    let id = game_id(&lib, "Lantern Keep");
    let mut r = arabic(Route::Game(id), big(), ThemeChoice::Dark);
    shot(&mut r, "game-overview");
}

#[test]
#[ignore = "review render"]
fn ar_game_protection() {
    let lib = settled_library();
    let id = game_id(&lib, "Lantern Keep");
    let mut r = arabic(Route::Game(id), big(), ThemeChoice::Dark);
    r.h.state_mut().state.details.tab = DetailsTab::Protection;
    r.h.run_steps(30);
    shot(&mut r, "game-protection");
}

#[test]
#[ignore = "review render"]
fn ar_statistics() {
    let mut r = arabic(Route::Statistics, big(), ThemeChoice::Dark);
    shot(&mut r, "statistics");
}

#[test]
#[ignore = "review render"]
fn ar_settings_general() {
    let mut r = arabic(
        Route::Settings(SettingsTab::General),
        big(),
        ThemeChoice::Dark,
    );
    shot(&mut r, "settings-general");
}

#[test]
#[ignore = "review render"]
fn ar_add_game_step_three() {
    let mut r = arabic(Route::Library, big(), ThemeChoice::Dark);
    let folder = std::path::PathBuf::from("/home/sam/Games/Celeste");
    let mut vm = easyplay_ui::vm::screens::AddGameVm {
        folder: folder.clone(),
        show_filtered: true,
        ..Default::default()
    };
    vm.set_scan(easyplay_ui::demo::folder_scan(&folder));
    r.h.state_mut().state.add_game = Some(vm);
    r.h.state_mut()
        .state
        .nav
        .open(easyplay_ui::route::Overlay::AddGame);
    r.h.run_steps(30);
    shot(&mut r, "add-game");
}

#[test]
#[ignore = "review render"]
fn ar_menu() {
    let mut r = arabic(Route::Library, big(), ThemeChoice::Dark);
    r.h.get_by_label_contains("Lantern Keep. ")
        .click_secondary();
    r.h.run_steps(30);
    shot(&mut r, "menu");
}
