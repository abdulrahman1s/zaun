//! Compatibility tab (SCREEN_SPECS_2 §10, UI-019): layer select with groups, follow
//! updates, status line with missing/older warnings, game fixes with the picker sheet,
//! options, advanced options, and the private Windows storage row.

use easyplay_core::ids::GameId;
use easyplay_core::model::{
    Channel, GameSummary, Renderer, RuntimeOrigin, RuntimeRef, SyncMode, Tool, WindowSystem,
};
use egui::{Context, Id, Popup, PopupCloseBehavior, Ui};

use super::card;
use super::permissions::{issue_line, section};
use crate::app::UiState;
use crate::bridge::{FolderTarget, UiCommand};
use crate::route::Overlay;
use crate::screens::Env;
use crate::strings::details as s;
use crate::theme::tokens::space;
use crate::theme::{self, Type, icons};
use crate::vm::AsyncValue;
use crate::vm::dialogs::{Confirm, Dialog};
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::context_menu::item;
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::select::select;
use crate::widgets::sheet::sheet;
use crate::widgets::text_field::TextField;
use crate::widgets::toggle::toggle;

fn segs<'a>(labels: &[&'a str]) -> Vec<Segment<'a>> {
    labels
        .iter()
        .map(|l| Segment {
            icon: None,
            label: l,
        })
        .collect()
}

/// The label of a reference.
pub fn runtime_label(r: &RuntimeRef, env: &Env<'_>) -> String {
    let rt = env.bridge.runtimes();
    match r {
        RuntimeRef::Default => s::default_layer(rt.default_label.as_deref()),
        RuntimeRef::Channel(c) => s::channel(c.umu_codename()),
        RuntimeRef::Pinned(n) => rt
            .layers
            .iter()
            .find(|l| &l.build.name == n)
            .map_or_else(|| n.as_str().to_owned(), |l| l.build.display_name.clone()),
        RuntimeRef::External(p) => p.display().to_string(),
    }
}

fn layer_select(ui: &mut Ui, env: &Env<'_>, current: &mut RuntimeRef) -> bool {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let label = runtime_label(current, env);
    let resp = ui.add(
        Button::new(&label)
            .icon(icons::LAYERS)
            .accessible_name(format!("{}: {label}", s::LAYER)),
    );
    let mut changed = false;
    let rt = env.bridge.runtimes().clone();
    Popup::menu(&resp)
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .width(300.0)
        .show(|ui| {
            let mut opt = |ui: &mut Ui, r: RuntimeRef| {
                let l = runtime_label(&r, env);
                if item(ui, None, &l, None, *current == r).clicked() {
                    *current = r;
                    changed = true;
                }
            };
            ui.label(Type::Micro.rich(s::GROUP_DEFAULT).color(p.text_muted));
            opt(ui, RuntimeRef::Default);
            ui.label(Type::Micro.rich(s::GROUP_CHANNELS).color(p.text_muted));
            opt(ui, RuntimeRef::Channel(Channel::UmuProton));
            opt(ui, RuntimeRef::Channel(Channel::GeProton));
            for (title, origin) in [
                (s::GROUP_INSTALLED, RuntimeOrigin::Store),
                (s::GROUP_OTHER, RuntimeOrigin::External),
            ] {
                let builds: Vec<_> = rt
                    .layers
                    .iter()
                    .filter(|l| l.build.origin == origin)
                    .collect();
                if builds.is_empty() {
                    continue;
                }
                ui.label(Type::Micro.rich(title).color(p.text_muted));
                for l in builds {
                    opt(ui, RuntimeRef::Pinned(l.build.name.clone()));
                }
            }
        });
    changed
}

const LOCALES: [(Option<&str>, &str); 6] = [
    (None, s::LANG_SYSTEM),
    (Some("en_US.UTF-8"), "English"),
    (Some("de_DE.UTF-8"), "Deutsch"),
    (Some("fr_FR.UTF-8"), "Français"),
    (Some("es_ES.UTF-8"), "Español"),
    (Some("ja_JP.UTF-8"), "日本語"),
];

/// Show the Compatibility tab.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let id = g.id;
    let Some(d) = state.details.record().cloned() else {
        if state.details.detail.show_skeleton(env.mono) {
            super::skeleton_lines(ui, 6);
        }
        return;
    };
    if matches!(state.details.prefix_size, AsyncValue::Idle) {
        let q = env.bridge.query_id();
        state.details.size_q = Some(q);
        state.details.prefix_size = AsyncValue::Loading { since: env.mono };
        env.bridge
            .send(UiCommand::MeasurePrefix { id, reply_to: q });
    }
    let Some(c) = state.details.compat.as_mut() else {
        return;
    };
    ui.set_max_width(840.0);
    section(ui, s::LAYER, None);
    ui.horizontal(|ui| {
        layer_select(ui, env, &mut c.runtime);
        let followable = matches!(c.runtime, RuntimeRef::Default | RuntimeRef::Channel(_));
        if followable {
            ui.add_space(space::S4);
            ui.checkbox(&mut c.follow, s::FOLLOW_UPDATES);
        }
    });
    let used = d
        .runtime_resolved
        .clone()
        .unwrap_or_else(|| s::NOT_DOWNLOADED.to_owned());
    let prepared = d
        .game
        .prefix_prepared_with
        .as_ref()
        .map(|n| n.as_str().to_owned());
    ui.label(
        Type::Caption
            .rich(s::status_line(&used, prepared.as_deref(), d.runtime_older))
            .color(p.text_secondary),
    );
    if d.runtime_missing {
        ui.horizontal_wrapped(|ui| {
            issue_line(ui, s::LAYER_MISSING, false);
            if ui.add(Button::new(s::USE_DEFAULT).size(Size::Sm)).clicked() {
                c.runtime = RuntimeRef::Default;
            }
        });
    }
    if d.runtime_older {
        issue_line(ui, s::LAYER_OLDER, false);
    }
    if env.bridge.runtimes().default_label.is_none() {
        ui.horizontal(|ui| {
            ui.label(
                Type::Caption
                    .rich(s::NONE_INSTALLED)
                    .color(p.text_secondary),
            );
            if ui
                .add(
                    Button::new(s::DOWNLOAD_NOW)
                        .size(Size::Sm)
                        .icon(icons::DOWNLOAD),
                )
                .clicked()
            {
                env.bridge.send(UiCommand::ProvisionRuntime {
                    target: RuntimeRef::Default,
                });
            }
        });
    }
    // Game fixes.
    section(ui, s::FIXES, None);
    if d.runtime_without_fixes {
        ui.label(Type::Caption.rich(s::NO_FIXES_IN_LAYER).color(p.text_muted));
    } else {
        ui.horizontal_wrapped(|ui| {
            match (&c.umu_id, &d.fix_title) {
                (Some(u), title) => {
                    ui.label(
                        Type::BodyStrong
                            .rich(title.clone().unwrap_or_default())
                            .color(p.text_primary),
                    );
                    ui.label(Type::Mono.rich(u).color(p.text_muted));
                }
                (None, _) => {
                    ui.label(Type::Body.rich(s::NO_FIX).color(p.text_secondary));
                }
            }
            if ui.add(Button::new(s::CHANGE).size(Size::Sm)).clicked() {
                state.nav.open(Overlay::FixPicker(id));
            }
        });
        let Some(c) = state.details.compat.as_mut() else {
            return;
        };
        toggle(ui, &mut c.compat.disable_fixes, s::TURN_OFF_FIXES, None);
    }
    let Some(c) = state.details.compat.as_mut() else {
        return;
    };
    // Options.
    section(ui, s::OPTIONS, None);
    egui::Grid::new("compat.options")
        .num_columns(2)
        .spacing(egui::vec2(space::S6, space::S3))
        .show(ui, |ui| {
            ui.label(Type::Body.rich(s::GRAPHICS).color(p.text_primary));
            let mut r = usize::from(c.compat.renderer == Renderer::OpenGl);
            if segmented(ui, s::GRAPHICS, &mut r, &segs(&["Vulkan", "OpenGL"]), false).changed() {
                c.compat.renderer = if r == 1 {
                    Renderer::OpenGl
                } else {
                    Renderer::Vulkan
                };
            }
            ui.end_row();
            ui.label(Type::Body.rich(s::WINDOW_SYSTEM).color(p.text_primary));
            let mut w = match c.compat.window_system {
                WindowSystem::Auto => 0,
                WindowSystem::X11 => 1,
                WindowSystem::Wayland => 2,
            };
            if segmented(
                ui,
                s::WINDOW_SYSTEM,
                &mut w,
                &segs(&[s::AUTOMATIC, "X11", "Wayland"]),
                false,
            )
            .changed()
            {
                c.compat.window_system =
                    [WindowSystem::Auto, WindowSystem::X11, WindowSystem::Wayland][w];
            }
            ui.end_row();
            ui.label(Type::Body.rich(s::OVERLAY).color(p.text_primary));
            let mango = env.bridge.host().mangohud;
            ui.horizontal(|ui| {
                ui.add_enabled_ui(mango, |ui| ui.checkbox(&mut c.compat.mangohud, "MangoHud"));
                if !mango {
                    ui.label(Type::Caption.rich(s::NOT_INSTALLED).color(p.text_muted));
                }
            });
            ui.end_row();
            ui.label(Type::Body.rich(s::LANGUAGE).color(p.text_primary));
            let mut li = LOCALES
                .iter()
                .position(|(code, _)| *code == c.compat.locale.as_deref())
                .unwrap_or(0);
            let labels: Vec<&str> = LOCALES.iter().map(|x| x.1).collect();
            if select(ui, s::LANGUAGE, None, None, &mut li, &labels, false).changed() {
                c.compat.locale = LOCALES[li].0.map(str::to_owned);
            }
            ui.end_row();
        });
    // Advanced.
    ui.add_space(space::S4);
    let adv = state.details.compat_advanced;
    if ui
        .add(Button::ghost(s::ADVANCED_COMPAT).icon(if adv {
            icons::CARET_DOWN
        } else {
            icons::CARET_RIGHT
        }))
        .clicked()
    {
        state.details.compat_advanced = !adv;
    }
    if state.details.compat_advanced
        && let Some(c) = state.details.compat.as_mut()
    {
        ui.label(Type::Body.rich(s::SYNC).color(p.text_primary));
        let mut sm = match c.compat.sync {
            SyncMode::Default => 0,
            SyncMode::NoFsync => 1,
            SyncMode::NoEsyncNoFsync => 2,
        };
        if segmented(
            ui,
            s::SYNC,
            &mut sm,
            &segs(&[s::AUTOMATIC, s::NO_FSYNC, s::NO_ESYNC]),
            false,
        )
        .changed()
        {
            c.compat.sync = [
                SyncMode::Default,
                SyncMode::NoFsync,
                SyncMode::NoEsyncNoFsync,
            ][sm];
        }
        TextField::new(&mut c.compat.dll_overrides, "compat.dll")
            .label(s::DLL_OVERRIDES)
            .mono()
            .show(ui);
        let mut hud = c.compat.dxvk_hud.clone().unwrap_or_default();
        if TextField::new(&mut hud, "compat.hud")
            .label(s::DXVK_HUD)
            .mono()
            .show(ui)
            .response
            .changed()
        {
            c.compat.dxvk_hud = (!hud.is_empty()).then_some(hud);
        }
        toggle(
            ui,
            &mut c.compat.proton_log,
            s::PROTON_LOG,
            Some(s::PROTON_LOG_HINT),
        );
    }
    // Storage.
    ui.add_space(space::S4);
    card(ui, s::STORAGE, |ui| {
        let size = match &state.details.prefix_size {
            AsyncValue::Ready(n) => crate::strings::fmt::bytes(*n),
            _ => s::CALCULATING.to_owned(),
        };
        let created = d
            .prefix_created_at
            .map(|t| crate::strings::fmt::relative_inline(t, env.now, env.utc_offset_s));
        ui.label(
            Type::Body
                .numeric(s::storage_line(&size, created.as_deref()))
                .color(p.text_secondary),
        );
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(
                    Button::new(s::OPEN_FOLDER)
                        .size(Size::Sm)
                        .icon(icons::FOLDER),
                )
                .clicked()
            {
                env.bridge
                    .send(UiCommand::OpenFolder(FolderTarget::Prefix(id)));
            }
            if ui
                .add(
                    Button::new(s::WINDOWS_SETTINGS)
                        .size(Size::Sm)
                        .icon(icons::WRENCH),
                )
                .clicked()
            {
                env.bridge.send(UiCommand::RunTool {
                    id,
                    tool: Tool::Winecfg,
                });
            }
            if ui
                .add(
                    Button::new(s::RESET)
                        .size(Size::Sm)
                        .variant(Variant::Danger),
                )
                .clicked()
            {
                state.show_dialog(Dialog::Confirm(Confirm::ResetPrefix {
                    id,
                    options: easyplay_core::library::ResetOptions::default(),
                }));
            }
        });
    });
}

/// The game-fix picker (a sheet with a search field and results).
pub fn fix_picker(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>, game: GameId) {
    if state.details.fixes_q.is_none() && matches!(state.details.fixes, AsyncValue::Idle) {
        let name = env
            .bridge
            .library()
            .get(game)
            .map(|g| g.name.to_string())
            .unwrap_or_default();
        state.details.fix_search = name;
        search(state, env);
    }
    let mut picked: Option<Option<String>> = None;
    let r = sheet(
        ctx,
        id,
        s::FIX_PICKER,
        false,
        |ui| {
            let p = theme::theme(ui.ctx()).palette.clone();
            let f = TextField::new(&mut state.details.fix_search, "fix.search")
                .search()
                .hint(s::FIX_SEARCH)
                .accessible_name(s::FIX_SEARCH)
                .show(ui);
            if f.response.changed() {
                search(state, env);
            }
            ui.add_space(space::S2);
            match &state.details.fixes {
                AsyncValue::Ready(list) if list.is_empty() => {
                    ui.label(Type::Body.rich(s::NO_FIX_FOUND).color(p.text_secondary));
                }
                AsyncValue::Ready(list) => {
                    let list = list.clone();
                    egui::ScrollArea::vertical().max_height(320.0).show_rows(
                        ui,
                        36.0,
                        list.len(),
                        |ui, range| {
                            for e in &list[range] {
                                let label = s::fix_row(&e.title, &e.store, &e.umu_id);
                                if item(ui, Some(icons::WRENCH), &label, None, false).clicked() {
                                    picked = Some(Some(e.umu_id.clone()));
                                }
                            }
                        },
                    );
                }
                _ => {}
            }
            if ui.add(Button::ghost(s::NO_FIX_USE)).clicked() {
                picked = Some(None);
            }
        },
        |ui| {
            ui.add(Button::new(crate::strings::dialogs::CANCEL))
                .clicked()
        },
    );
    let chosen = picked.is_some();
    if let Some(u) = picked
        && let Some(c) = state.details.compat.as_mut()
    {
        c.umu_id = u;
    }
    if r.close_requested || r.footer || chosen {
        crate::widgets::modal::closed(ctx, id);
        state.details.fixes = AsyncValue::Idle;
        state.nav.close(Overlay::FixPicker(game));
    }
}

fn search(state: &mut UiState, env: &mut Env<'_>) {
    let q = env.bridge.query_id();
    state.details.fixes_q = Some(q);
    env.bridge.send(UiCommand::SearchFixes {
        text: state.details.fix_search.clone(),
        reply_to: q,
    });
}
