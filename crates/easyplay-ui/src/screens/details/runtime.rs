//! Compatibility tab (easyplay-redesign.html `tabCompat`; SCREEN_SPECS_2 §10, UI-019):
//! setting rows in a 620 column under four headings. *Compatibility layer*: the layer (a
//! select of its groups) with what was last used, *Follow updates*; a missing or older
//! layer and a catalogue with no layer yet say so in rows of their own. *Game fixes*: the
//! fix with *Change…* (the picker sheet), *Turn off fixes*. *Options*: graphics, window
//! system, the performance overlay (greyed out without MangoHud), the game's language, and
//! the finer options (synchronisation, DLL overrides, DXVK overlay, Proton log). *Private
//! Windows storage*: its size and date with *Open*, *Windows settings*, *Reset…*.

use easyplay_core::ids::GameId;
use easyplay_core::model::{
    Channel, GameSummary, Renderer, RuntimeOrigin, RuntimeRef, SyncMode, Tool, WindowSystem,
};
use egui::{Context, Id, Rect, Sense, Ui, vec2};

use super::column;
use crate::app::UiState;
use crate::bridge::{FolderTarget, UiCommand};
use crate::route::Overlay;
use crate::screens::Env;
use crate::strings::details as s;
use crate::theme::tokens::{control, layout, space};
use crate::theme::{self, Type, icons};
use crate::vm::AsyncValue;
use crate::vm::dialogs::{Confirm, Dialog};
use crate::widgets::button::{Button, Variant};
use crate::widgets::context_menu::item;
use crate::widgets::rows::{self, SettingRow};
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::select::select;
use crate::widgets::sheet::sheet;
use crate::widgets::text_field::TextField;
use crate::widgets::toggle::switch;

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

/// The value a layer select shows: "Default, UMU-Proton 10.0-4" for the default.
fn layer_value(r: &RuntimeRef, env: &Env<'_>) -> String {
    match r {
        RuntimeRef::Default => {
            s::default_layer_value(env.bridge.runtimes().default_label.as_deref())
        }
        other => runtime_label(other, env),
    }
}

/// The layers to choose from, in the order of their groups: the default, the channels,
/// EasyPlay's builds, builds from other sources.
fn layer_choices(env: &Env<'_>) -> Vec<RuntimeRef> {
    let rt = env.bridge.runtimes();
    let mut out = vec![
        RuntimeRef::Default,
        RuntimeRef::Channel(Channel::UmuProton),
        RuntimeRef::Channel(Channel::GeProton),
    ];
    for origin in [RuntimeOrigin::Store, RuntimeOrigin::External] {
        out.extend(
            rt.layers
                .iter()
                .filter(|l| l.build.origin == origin)
                .map(|l| RuntimeRef::Pinned(l.build.name.clone())),
        );
    }
    out
}

/// The layer select. Returns whether the layer changed.
fn layer_select(ui: &mut Ui, env: &Env<'_>, current: &mut RuntimeRef) -> bool {
    let mut choices = layer_choices(env);
    if !choices.contains(current) {
        choices.push(current.clone());
    }
    let labels: Vec<String> = choices.iter().map(|r| layer_value(r, env)).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut k = choices.iter().position(|r| r == current).unwrap_or(0);
    let changed = select(ui, s::LAYER(), None, None, &mut k, &refs, false).changed();
    if changed && let Some(r) = choices.get(k) {
        *current = r.clone();
    }
    changed
}

fn locales() -> [(Option<&'static str>, &'static str); 6] {
    [
        (None, s::LANG_SYSTEM()),
        (Some("en_US.UTF-8"), "English"),
        (Some("de_DE.UTF-8"), "Deutsch"),
        (Some("fr_FR.UTF-8"), "Français"),
        (Some("es_ES.UTF-8"), "Español"),
        (Some("ja_JP.UTF-8"), "日本語"),
    ]
}

/// Show the Compatibility tab.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    column(ui, |ui| body(ui, state, env, g));
}

fn body(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
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

    // The layer.
    rows::heading(ui, s::LAYER(), true);
    let prepared = d
        .game
        .prefix_prepared_with
        .as_ref()
        .map(|n| n.as_str().to_owned());
    let help = s::layer_help(
        d.runtime_resolved.as_deref(),
        prepared.as_deref(),
        d.runtime_older,
    );
    SettingRow::new(s::LAYER_ROW())
        .help(&help)
        .show(ui, |ui| layer_select(ui, env, &mut c.runtime));
    if d.runtime_missing {
        let use_default = SettingRow::new(s::LAYER_MISSING())
            .id_salt("layer-missing")
            .show(ui, |ui| ui.add(Button::new(s::USE_DEFAULT())).clicked());
        if use_default {
            c.runtime = RuntimeRef::Default;
        }
    }
    if d.runtime_older {
        super::fine(ui, s::LAYER_OLDER(), p.amber);
    }
    if env.bridge.runtimes().default_label.is_none() {
        let download = SettingRow::new(s::NONE_INSTALLED())
            .id_salt("layer-none")
            .show(ui, |ui| {
                ui.add(Button::new(s::DOWNLOAD_NOW()).icon(icons::ARROW_DOWN))
                    .clicked()
            });
        if download {
            env.bridge.send(UiCommand::ProvisionRuntime {
                target: RuntimeRef::Default,
            });
        }
    }
    let followable = matches!(c.runtime, RuntimeRef::Default | RuntimeRef::Channel(_));
    SettingRow::new(s::FOLLOW_UPDATES())
        .help(s::FOLLOW_HELP())
        .enabled(followable)
        .show(ui, |ui| switch(ui, &mut c.follow, s::FOLLOW_UPDATES()));

    // Game fixes.
    rows::heading(ui, s::FIXES(), false);
    if d.runtime_without_fixes {
        SettingRow::new(s::NO_FIXES_IN_LAYER()).show(ui, |_| {});
    } else {
        let (label, help) = match (&c.umu_id, &d.fix_title) {
            (Some(_), Some(title)) => (title.clone(), s::FIX_HELP().to_owned()),
            (Some(u), None) => (u.clone(), s::FIX_HELP().to_owned()),
            (None, _) => (s::NO_FIX().to_owned(), String::new()),
        };
        let change = SettingRow::new(&label)
            .help(&help)
            .id_salt("fix")
            .show(ui, |ui| ui.add(Button::new(s::CHANGE())).clicked());
        if change {
            state.nav.open(Overlay::FixPicker(id));
        }
        if let Some(c) = state.details.compat.as_mut() {
            SettingRow::new(s::TURN_OFF_FIXES()).show(ui, |ui| {
                switch(ui, &mut c.compat.disable_fixes, s::TURN_OFF_FIXES())
            });
        }
    }
    let Some(c) = state.details.compat.as_mut() else {
        return;
    };

    // Options.
    rows::heading(ui, s::OPTIONS(), false);
    SettingRow::new(s::GRAPHICS()).show(ui, |ui| {
        let mut r = usize::from(c.compat.renderer == Renderer::OpenGl);
        if segmented(
            ui,
            s::GRAPHICS(),
            &mut r,
            &segs(&["Vulkan", "OpenGL"]),
            false,
        )
        .changed()
        {
            c.compat.renderer = if r == 1 {
                Renderer::OpenGl
            } else {
                Renderer::Vulkan
            };
        }
    });
    SettingRow::new(s::WINDOW_SYSTEM()).show(ui, |ui| {
        let mut w = match c.compat.window_system {
            WindowSystem::Auto => 0,
            WindowSystem::X11 => 1,
            WindowSystem::Wayland => 2,
        };
        if segmented(
            ui,
            s::WINDOW_SYSTEM(),
            &mut w,
            &segs(&[s::AUTOMATIC(), "X11", "Wayland"]),
            false,
        )
        .changed()
        {
            c.compat.window_system =
                [WindowSystem::Auto, WindowSystem::X11, WindowSystem::Wayland][w.min(2)];
        }
    });
    let mango = env.bridge.host().mangohud;
    let mut overlay = SettingRow::new(s::OVERLAY()).enabled(mango);
    if !mango {
        overlay = overlay.help(s::MANGOHUD_MISSING());
    }
    overlay.show(ui, |ui| switch(ui, &mut c.compat.mangohud, s::OVERLAY()));
    SettingRow::new(s::LANGUAGE()).show(ui, |ui| {
        let locales = locales();
        let mut li = locales
            .iter()
            .position(|(code, _)| *code == c.compat.locale.as_deref())
            .unwrap_or(0);
        let labels: Vec<&str> = locales.iter().map(|x| x.1).collect();
        if select(ui, s::LANGUAGE(), None, None, &mut li, &labels, false).changed() {
            c.compat.locale = locales[li.min(locales.len() - 1)].0.map(str::to_owned);
        }
    });
    SettingRow::new(s::SYNC()).show(ui, |ui| {
        let mut sm = match c.compat.sync {
            SyncMode::Default => 0,
            SyncMode::NoFsync => 1,
            SyncMode::NoEsyncNoFsync => 2,
        };
        if segmented(
            ui,
            s::SYNC(),
            &mut sm,
            &segs(&[s::AUTOMATIC(), s::NO_FSYNC(), s::NO_ESYNC()]),
            false,
        )
        .changed()
        {
            c.compat.sync = [
                SyncMode::Default,
                SyncMode::NoFsync,
                SyncMode::NoEsyncNoFsync,
            ][sm.min(2)];
        }
    });
    let field_w = layout::COL_MAX / 2.0 - space::S8;
    SettingRow::new(s::DLL_OVERRIDES()).show(ui, |ui| {
        TextField::new(&mut c.compat.dll_overrides, "compat.dll")
            .accessible_name(s::DLL_OVERRIDES())
            .mono()
            .width(field_w)
            .show(ui)
    });
    let mut hud = c.compat.dxvk_hud.clone().unwrap_or_default();
    let hud_changed = SettingRow::new(s::DXVK_HUD()).show(ui, |ui| {
        TextField::new(&mut hud, "compat.hud")
            .accessible_name(s::DXVK_HUD())
            .mono()
            .width(field_w)
            .show(ui)
            .response
            .changed()
    });
    if hud_changed {
        c.compat.dxvk_hud = (!hud.is_empty()).then_some(hud);
    }
    SettingRow::new(s::PROTON_LOG_ROW())
        .help(s::PROTON_LOG_HINT())
        .show(ui, |ui| {
            switch(ui, &mut c.compat.proton_log, s::PROTON_LOG_ROW())
        });

    // Private Windows storage.
    rows::heading(ui, s::STORAGE(), false);
    let size = match &state.details.prefix_size {
        AsyncValue::Ready(n) => crate::strings::fmt::bytes(*n),
        AsyncValue::Failed(_) => s::SIZE_UNKNOWN().to_owned(),
        _ => s::CALCULATING().to_owned(),
    };
    let created = d
        .prefix_created_at
        .map(|t| s::date_label(t, env.now, env.utc_offset_s));
    let value = if d.prefix_created_at.is_none() && d.game.prefix_prepared_with.is_none() {
        s::STORAGE_NOT_YET().to_owned()
    } else {
        s::storage_value(&size, created.as_deref())
    };
    // The row's label is what it tells: it is read as such (registered before the row's
    // buttons, which stand over it).
    let top = ui.cursor().min;
    let said = ui.interact(
        Rect::from_min_size(top, vec2(ui.available_width(), control::SETTING_ROW)),
        ui.make_persistent_id("storage.said"),
        Sense::hover(),
    );
    crate::a11y::item(&said, egui::Role::Label, &value);
    let pressed = SettingRow::new(&value)
        .help(s::STORAGE_HELP())
        .id_salt("storage")
        .show(ui, |ui| {
            // Every button is drawn whichever is pressed.
            let open = ui.add(Button::new(s::OPEN())).clicked();
            let settings = ui.add(Button::new(s::WINDOWS_SETTINGS())).clicked();
            let reset = ui
                .add(Button::new(s::RESET()).variant(Variant::Warn))
                .clicked();
            [open, settings, reset].iter().position(|x| *x)
        });
    match pressed {
        Some(0) => env
            .bridge
            .send(UiCommand::OpenFolder(FolderTarget::Prefix(id))),
        Some(1) => env.bridge.send(UiCommand::RunTool {
            id,
            tool: Tool::Winecfg,
        }),
        Some(_) => state.show_dialog(Dialog::Confirm(Confirm::ResetPrefix {
            id,
            options: easyplay_core::library::ResetOptions::default(),
        })),
        None => {}
    }
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
        s::FIX_PICKER(),
        false,
        |ui| {
            let p = theme::theme(ui.ctx()).palette.clone();
            let f = TextField::new(&mut state.details.fix_search, "fix.search")
                .search()
                .hint(s::FIX_SEARCH())
                .accessible_name(s::FIX_SEARCH())
                .show(ui);
            if f.response.changed() {
                search(state, env);
            }
            ui.add_space(space::S2);
            match &state.details.fixes {
                AsyncValue::Ready(list) if list.is_empty() => {
                    ui.label(Type::Body.rich(s::NO_FIX_FOUND()).color(p.t2));
                }
                AsyncValue::Ready(list) => {
                    let list = list.clone();
                    egui::ScrollArea::vertical()
                        .max_height(layout::MENU_MAX_H / 2.0 + space::S10)
                        .show_rows(ui, control::MENU_ITEM, list.len(), |ui, range| {
                            for e in &list[range] {
                                let label = s::fix_row(&e.title, &e.store, &e.umu_id);
                                if item(ui, None, &label, None, false).clicked() {
                                    picked = Some(Some(e.umu_id.clone()));
                                }
                            }
                        });
                }
                AsyncValue::Failed(why) => {
                    Type::Body.paragraph(ui, why, p.t2);
                }
                _ => {}
            }
            if ui.add(Button::ghost(s::NO_FIX_USE())).clicked() {
                picked = Some(None);
            }
        },
        |ui| {
            ui.add(Button::new(crate::strings::dialogs::CANCEL()))
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
