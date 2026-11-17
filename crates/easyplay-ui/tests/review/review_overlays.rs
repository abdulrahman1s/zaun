//! Review renders of menus, dialogs, toasts, tooltips, notes, empty states and loading
//! blocks (throwaway): `cargo test -p easyplay-ui
//! --features review --test review review_overlays:: -- --ignored
//! --test-threads=1`. Images land in `target/review/overlays-*.png`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::common;

use std::path::Path;

use common::*;
use easyplay_core::ids::PromptId;
use easyplay_core::library::RemoveOptions;
use easyplay_core::model::{ProgramCandidate, PromptKind, PromptRequest};
use easyplay_ui::bridge::{ReportFile, SetupContext, SetupOrigin, UiEvent};
use easyplay_ui::demo;
use easyplay_ui::route::{Overlay, Route};
use easyplay_ui::theme::{self, Appearance, ThemeChoice};
use easyplay_ui::vm::debug::ReportVm;
use easyplay_ui::vm::dialogs::{Confirm, Dialog};
use easyplay_ui::vm::screens::AddGameVm;
use easyplay_ui::vm::toasts::ToastKind;
use easyplay_ui::widgets::{banner, context_menu, empty_state, skeleton};
use egui::Vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn dir() -> std::path::PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/review");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save(r: &mut Rig, name: &str) {
    let image = r.h.render().expect("render");
    image
        .save(dir().join(format!("overlays-{name}.png")))
        .unwrap();
}

fn at(choice: ThemeChoice) -> Rig {
    let mut r = rig_with(
        settled_library(),
        Route::Library,
        Vec2::new(1280.0, 800.0),
        choice,
    );
    settle(&mut r);
    r
}

fn dialog(r: &mut Rig, d: Dialog) {
    r.h.state_mut().state.show_dialog(d);
    r.h.run_steps(40);
}

fn remove(choice: ThemeChoice, name: &str) {
    let mut r = at(choice);
    let id = game_id(&settled_library(), "Lantern Keep");
    dialog(
        &mut r,
        Dialog::Confirm(Confirm::RemoveGames {
            ids: vec![id],
            options: RemoveOptions {
                delete_private_storage: true,
                keep_save_data: true,
                delete_shortcuts: true,
                keep_history: true,
                ..RemoveOptions::default()
            },
        }),
    );
    save(&mut r, name);
}

#[test]
#[ignore = "review render"]
fn dlg_clearstats() {
    let mut r = at(ThemeChoice::Dark);
    dialog(&mut r, Dialog::Confirm(Confirm::ClearStatistics));
    save(&mut r, "dlg-clearstats");
}

#[test]
#[ignore = "review render"]
fn dlg_remove() {
    remove(ThemeChoice::Dark, "dlg-remove");
}

#[test]
#[ignore = "review render"]
fn dlg_remove_light() {
    remove(ThemeChoice::Light, "dlg-remove-light");
}

#[test]
#[ignore = "review render"]
fn dlg_error() {
    let mut r = at(ThemeChoice::Dark);
    let id = game_id(&settled_library(), "Tidewater");
    dialog(&mut r, Dialog::Error(demo::launch_failure(id, "Tidewater")));
    save(&mut r, "dlg-error");
}

#[test]
#[ignore = "review render"]
fn dlg_installer() {
    let mut r = at(ThemeChoice::Dark);
    let game = game_id(&settled_library(), "Setup Glassworks");
    r.fake.push_event(UiEvent::Prompt(PromptRequest {
        id: PromptId::from_uuid(uuid::Uuid::from_u128(5)),
        kind: PromptKind::InstallerFinished {
            game,
            programs: vec![
                ProgramCandidate {
                    path: "/p/Glassworks.exe".into(),
                    label: "Glassworks".into(),
                    from_shortcut: true,
                    size: 48_000_000,
                },
                ProgramCandidate {
                    path: "/p/Launcher.exe".into(),
                    label: "Glassworks Launcher".into(),
                    from_shortcut: false,
                    size: 2_000_000,
                },
            ],
        },
    }));
    r.h.run_steps(40);
    save(&mut r, "dlg-installer");
}

#[test]
#[ignore = "review render"]
fn dlg_shortcuts() {
    let mut r = at(ThemeChoice::Dark);
    r.h.state_mut().state.nav.open(Overlay::Shortcuts);
    r.h.run_steps(40);
    save(&mut r, "dlg-shortcuts");
}

#[test]
#[ignore = "review render"]
fn dlg_artwork() {
    let mut r = at(ThemeChoice::Dark);
    let id = game_id(&settled_library(), "Lantern Keep");
    r.h.state_mut().state.nav.open(Overlay::Artwork(id));
    r.h.run_steps(10);
    serve(&mut r);
    r.h.run_steps(40);
    save(&mut r, "dlg-artwork");
}

fn report_rig() -> Rig {
    let mut r = at(ThemeChoice::Dark);
    {
        let s = &mut r.h.state_mut().state;
        s.debug.report = Some(ReportVm {
            files: Some(vec![
                ReportFile {
                    name: "version.txt".into(),
                    bytes: b"EasyPlay 0.1.0\n".to_vec(),
                },
                ReportFile {
                    name: "doctor.json".into(),
                    bytes: br#"{ "sandbox": "bubblewrap 0.11", "launcher": "umu 1.4.4" }"#.to_vec(),
                },
                ReportFile {
                    name: "easyplay.log".into(),
                    bytes: b"2026-11-12T21:04:11Z INFO start version=0.1.0\n2026-11-12T21:04:12Z WARN Library repaired path=~/x\n".to_vec(),
                },
            ]),
            ..ReportVm::default()
        });
        s.nav.open(Overlay::Report);
    }
    r.h.run_steps(40);
    r
}

#[test]
#[ignore = "review render"]
fn dlg_report() {
    let mut r = report_rig();
    save(&mut r, "dlg-report");
    r.h.state_mut().state.debug.report.as_mut().unwrap().viewing = Some(1);
    r.h.run_steps(40);
    save(&mut r, "dlg-report-open");
}

#[test]
#[ignore = "review render"]
fn dlg_add_1() {
    let mut r = at(ThemeChoice::Dark);
    {
        let s = &mut r.h.state_mut().state;
        s.add_game = Some(AddGameVm::default());
        s.nav.open(Overlay::AddGame);
    }
    r.h.run_steps(40);
    save(&mut r, "dlg-add-1");
}

#[test]
#[ignore = "review render"]
fn dlg_add_2() {
    let mut r = at(ThemeChoice::Dark);
    {
        let s = &mut r.h.state_mut().state;
        let folder = Path::new("/home/sam/Games/Untitled game");
        let mut vm = AddGameVm {
            folder: folder.to_path_buf(),
            ..AddGameVm::default()
        };
        vm.set_scan(demo::folder_scan(folder));
        s.add_game = Some(vm);
        s.nav.open(Overlay::AddGame);
    }
    r.h.run_steps(40);
    save(&mut r, "dlg-add-2");
}

#[test]
#[ignore = "review render"]
fn dlg_add_3() {
    let mut r = at(ThemeChoice::Dark);
    r.fake.push_event(UiEvent::SetupRequested {
        draft: Box::new(demo::draft(
            Path::new("/home/sam/Games/Untitled game/Game.exe"),
            now(),
        )),
        context: SetupContext {
            origin: SetupOrigin::InsideApp,
            queue: (1, 1),
        },
    });
    r.h.run_steps(40);
    save(&mut r, "dlg-add-3");
}

#[test]
#[ignore = "review render"]
fn toasts() {
    let mut r = at(ThemeChoice::Dark);
    let mono = easyplay_core::clock::Clock::mono(&*r.clock);
    r.h.state_mut()
        .state
        .toast("Added to Favourites", ToastKind::Success, None, mono);
    r.h.run_steps(20);
    save(&mut r, "toast");
    let id = game_id(&settled_library(), "Lantern Keep");
    r.h.state_mut().state.toast(
        "Hid Lantern Keep",
        ToastKind::Info,
        Some(easyplay_ui::bridge::UiCommand::SetHidden { id, hidden: false }),
        mono,
    );
    r.h.run_steps(20);
    save(&mut r, "toast-action");
}

#[test]
#[ignore = "review render"]
fn tooltip() {
    let mut r = at(ThemeChoice::Dark);
    r.h.get_by_label("Add game").hover();
    r.h.run_steps(60);
    save(&mut r, "tooltip");
}

// ── The parts on their own, outside the app ─────────────────────────────────────────────

fn parts(name: &str, size: Vec2, choice: ThemeChoice, f: impl FnMut(&mut egui::Ui) + 'static) {
    let mut f = f;
    let mut h = Harness::builder().with_size(size).build_ui(move |ui| {
        let ctx = ui.ctx().clone();
        if ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("installed")).is_none()) {
            theme::install(
                &ctx,
                &Appearance {
                    theme: choice,
                    ..Appearance::default()
                },
            );
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique("installed"), true));
        }
        let bg = theme::theme(&ctx).palette.bg;
        egui::Frame::NONE
            .fill(bg)
            .inner_margin(egui::Margin::same(28))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                f(ui);
            });
    });
    h.run_steps(60);
    let image = h.render().expect("render");
    image
        .save(dir().join(format!("overlays-{name}.png")))
        .unwrap();
}

#[test]
#[ignore = "review render"]
fn notes() {
    parts("notes", Vec2::new(800.0, 420.0), ThemeChoice::Dark, |ui| {
        ui.set_max_width(720.0);
        banner::Note::problem(
            "This game needs a folder that isn't there",
            "Tidewater is allowed to use “Mods”, but that folder can't be found. It may have been moved, or its drive is not connected.",
        )
        .code("EP-SBX-010")
        .actions(&["Edit permissions", "Try again", "View log", "Dismiss"])
        .show(ui);
        banner::Note::missing(
            "This game's program can't be found.",
            "It may have been moved, renamed or deleted. Locate it to keep your settings and playtime.",
        )
        .show(ui);
        banner::Note::lingering(
            "Still running in the background.",
            "The window closed, but some of the game's programs are still going.",
        )
        .show(ui);
    });
}

#[test]
#[ignore = "review render"]
fn blank() {
    parts("blank", Vec2::new(1280.0, 700.0), ThemeChoice::Dark, |ui| {
        empty_state::blank(
            ui,
            "EasyPlay couldn't read your library",
            "The library file didn't open. Your games and their saves haven't been touched.",
            |ui| {
                ui.add(easyplay_ui::widgets::button::Button::primary("Try again"));
                ui.add(easyplay_ui::widgets::button::Button::new(
                    "Open library folder",
                ));
            },
            Some("Permission denied (os error 13)"),
        );
    });
}

#[test]
#[ignore = "review render"]
fn loading() {
    parts(
        "loading",
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
        |ui| {
            let w = 189.0;
            for k in 0..14 {
                let x = 28.0 + (k % 6) as f32 * (w + 16.0);
                let y = 6.0 + (k / 6) as f32 * (w * 1.5 + 36.0);
                let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, w * 1.5));
                skeleton::cover(ui, rect, k % 7);
            }
        },
    );
}

#[test]
#[ignore = "review render"]
fn menu() {
    parts("menu", Vec2::new(420.0, 520.0), ThemeChoice::Dark, |ui| {
        context_menu::frame(ui, |ui| {
            ui.set_width(220.0);
            context_menu::section(ui, "Sort by");
            context_menu::option_item(ui, "Recently played", true);
            context_menu::option_item(ui, "Name", false);
            context_menu::separator(ui);
            context_menu::item(ui, None, "Play", Some("Ctrl+Enter"), false);
            context_menu::item(ui, None, "Tags", None, false);
            context_menu::danger_item(ui, None, "Remove…");
        });
    });
}

#[test]
#[ignore = "review render"]
fn dlg_exit() {
    let mut r = at(ThemeChoice::Dark);
    dialog(&mut r, Dialog::Confirm(Confirm::ClearStatistics));
    r.h.key_press(egui::Key::Escape);
    r.h.run_steps(3);
    save(&mut r, "dlg-exit-midway");
    r.h.run_steps(20);
    save(&mut r, "dlg-exit-done");
}

#[test]
#[ignore = "review render"]
fn dlg_step_morph() {
    let mut r = at(ThemeChoice::Dark);
    {
        let s = &mut r.h.state_mut().state;
        s.add_game = Some(AddGameVm::default());
        s.nav.open(Overlay::AddGame);
    }
    r.h.run_steps(40);
    {
        let s = &mut r.h.state_mut().state;
        let folder = Path::new("/home/sam/Games/Untitled game");
        let mut vm = AddGameVm {
            folder: folder.to_path_buf(),
            ..AddGameVm::default()
        };
        vm.set_scan(demo::folder_scan(folder));
        s.add_game = Some(vm);
    }
    r.h.run_steps(4);
    save(&mut r, "dlg-step-morph");
}

#[test]
#[ignore = "review render"]
fn dialog_leaving() {
    use easyplay_ui::widgets::button::Variant;
    use easyplay_ui::widgets::modal::{self, Action, Spec};
    let mut h = Harness::builder()
        .with_size(Vec2::new(800.0, 500.0))
        .with_step_dt(0.02)
        .build_ui_state(
            |ui, open: &mut bool| {
                let ctx = ui.ctx().clone();
                if ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("installed")).is_none()) {
                    theme::install(&ctx, &Appearance::default());
                    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("installed"), true));
                }
                let bg = theme::theme(&ctx).palette.bg;
                ui.painter().rect_filled(ctx.content_rect(), 0.0, bg);
                if *open {
                    modal::choose(
                        &ctx,
                        egui::Id::unique("leaving"),
                        Spec::new("Clear all statistics?")
                            .text("Every recorded session is deleted from this computer."),
                        &[
                            Action {
                                label: "Cancel",
                                variant: Variant::Secondary,
                            },
                            Action {
                                label: "Clear statistics",
                                variant: Variant::Danger,
                            },
                        ],
                        0,
                        |_| {},
                    );
                }
                modal::paint_exits(&ctx);
            },
            true,
        );
    h.run_steps(30);
    h.state_mut().clone_from(&false);
    h.run_steps(3);
    let image = h.render().expect("render");
    image
        .save(dir().join("overlays-dialog-leaving.png"))
        .unwrap();
}

fn setup_rig(path: &str, more: bool) -> Rig {
    let mut r = at(ThemeChoice::Dark);
    r.fake.push_event(UiEvent::SetupRequested {
        draft: Box::new(demo::draft(Path::new(path), now())),
        context: SetupContext {
            origin: SetupOrigin::InsideApp,
            queue: (1, 1),
        },
    });
    r.h.run_steps(10);
    if more {
        r.h.state_mut().state.setup.as_mut().unwrap().more = true;
    }
    r.h.run_steps(20);
    r
}

#[test]
#[ignore = "review render"]
fn dlg_setup_states() {
    let mut r = setup_rig("/home/sam/Downloads/Setup Foo.exe", false);
    save(&mut r, "dlg-setup-installer-loose");
    let mut r = setup_rig("/home/sam/Games/Lantern Keep/LanternKeep.exe", true);
    save(&mut r, "dlg-setup-more");
    r.h.state_mut().state.setup.as_mut().unwrap().phase =
        easyplay_ui::vm::setup::SetupPhase::Launching { game: None };
    r.h.run_steps(20);
    save(&mut r, "dlg-setup-launching");
}

#[test]
#[ignore = "review render"]
fn dlg_artwork_steam() {
    let fake = easyplay_ui::bridge::FakeBackend::new(settled_library());
    let mut prefs = demo::preferences();
    prefs.artwork.online = easyplay_core::config::schema::ArtworkOnline::Steam;
    fake.publish_prefs(prefs);
    let mut r = rig_on(
        fake,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    settle(&mut r);
    let id = game_id(&settled_library(), "Lantern Keep");
    r.h.state_mut().state.nav.open(Overlay::Artwork(id));
    r.h.run_steps(10);
    serve(&mut r);
    r.h.run_steps(40);
    save(&mut r, "dlg-artwork-steam");
}

/// A menu in a real popup: a submenu of tags, then back.
#[test]
#[ignore = "review render"]
fn menu_popup() {
    use easyplay_ui::screens::dialogs::tags::tag_items;
    #[derive(Default)]
    struct S {
        text: String,
    }
    let lib = settled_library();
    let game = game_id(&lib, "Lantern Keep");
    let mut h = Harness::builder()
        .with_size(Vec2::new(560.0, 520.0))
        .with_step_dt(0.02)
        .build_ui_state(
            move |ui, s: &mut S| {
                let ctx = ui.ctx().clone();
                if ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("installed")).is_none()) {
                    theme::install(&ctx, &Appearance::default());
                    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("installed"), true));
                }
                let bg = theme::theme(&ctx).palette.bg;
                ui.painter().rect_filled(ctx.content_rect(), 0.0, bg);
                ui.add_space(40.0);
                ui.horizontal(|ui| {
                    ui.add_space(40.0);
                    let b = ui.add(easyplay_ui::widgets::button::Button::new("More"));
                    context_menu::popup(
                        egui::Popup::menu(&b)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
                        |ui| match context_menu::open_sub(ui).as_deref() {
                            Some("tags") => {
                                context_menu::back(ui, "Tags");
                                let _ = tag_items(ui, &lib, &[game], &mut s.text);
                            }
                            _ => {
                                context_menu::item(ui, None, "Play", Some("Ctrl+Enter"), false);
                                context_menu::separator(ui);
                                context_menu::sub_item(ui, "tags", "Tags");
                                context_menu::danger_item_with(ui, None, "Remove…", Some("Del"));
                            }
                        },
                    );
                });
                modal::paint_exits(&ctx);
            },
            S::default(),
        );
    use easyplay_ui::widgets::modal;
    h.run_steps(5);
    h.get_by_label("More").click();
    h.run_steps(20);
    let image = h.render().expect("render");
    image.save(dir().join("overlays-menu-popup.png")).unwrap();
    h.get_by_label("Tags").click();
    h.run_steps(20);
    let image = h.render().expect("render");
    image
        .save(dir().join("overlays-menu-popup-tags.png"))
        .unwrap();
    h.get_by_label("Tags").click();
    h.run_steps(20);
    let image = h.render().expect("render");
    image
        .save(dir().join("overlays-menu-popup-back.png"))
        .unwrap();
}

/// The panel's height mid-way through a change of step.
#[test]
#[ignore = "review render"]
fn dialog_morphing() {
    use easyplay_ui::widgets::modal::{self, Spec};
    let mut h = Harness::builder()
        .with_size(Vec2::new(800.0, 600.0))
        .with_step_dt(0.02)
        .build_ui_state(
            |ui, step: &mut u64| {
                let ctx = ui.ctx().clone();
                if ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("installed")).is_none()) {
                    theme::install(&ctx, &Appearance::default());
                    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("installed"), true));
                }
                let bg = theme::theme(&ctx).palette.bg;
                ui.painter().rect_filled(ctx.content_rect(), 0.0, bg);
                let s = *step;
                modal::show(
                    &ctx,
                    egui::Id::unique("morph"),
                    Spec::new("Add a game")
                        .text("Choose the folder the game lives in.")
                        .wide()
                        .close_button()
                        .step(s),
                    |ui| {
                        let lines = if s == 0 { 2 } else { 8 };
                        for i in 0..lines {
                            ui.label(format!("Line {i}"));
                        }
                    },
                    |ui| {
                        ui.add(easyplay_ui::widgets::button::Button::primary("Continue"));
                    },
                );
                modal::paint_exits(&ctx);
            },
            0,
        );
    h.run_steps(30);
    *h.state_mut() = 1;
    h.run_steps(5);
    let image = h.render().expect("render");
    image
        .save(dir().join("overlays-dialog-morphing.png"))
        .unwrap();
}

/// The quick-launch window: starting, a failure, and "already running".
#[test]
#[ignore = "review render"]
fn quick_launch_states() {
    use easyplay_core::launch::LaunchState;
    let lib = settled_library();
    let g = lib
        .games
        .iter()
        .find(|g| &*g.name == "Lantern Keep")
        .unwrap()
        .clone();
    for state in ["starting", "failed", "already"] {
        let path = g.primary_exe_path.to_path_buf();
        let mut r = rig_app(
            easyplay_ui::bridge::FakeBackend::new(settled_library()),
            Route::Library,
            Vec2::new(420.0, 150.0),
            ThemeChoice::Dark,
            move |a| a.quick_launch(path),
        );
        r.h.run_steps(2);
        let launch = match state {
            "failed" => LaunchState::Failed(Box::new(demo::launch_failure(g.id, "Lantern Keep"))),
            _ => LaunchState::Validating,
        };
        r.fake.push_event(UiEvent::LaunchProgress {
            game: g.id,
            state: launch,
        });
        r.h.run_steps(4);
        if state == "already" {
            let now = easyplay_core::clock::Clock::mono(&*r.clock);
            if let Some(q) = r.h.state_mut().state.quick.as_mut() {
                q.already_at = Some(now);
            }
        }
        r.h.run_steps(10);
        save(&mut r, &format!("quick-{state}"));
    }
}
