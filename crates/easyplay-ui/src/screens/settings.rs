//! Settings (SCREEN_SPECS_2 §13, UI-023): a vertical tab list and a pane of sections with a
//! control for every preference of CONFIGURATION §2. Every control applies at once
//! (`SetPreference` with the UI's optimistic result) and shows a brief "Saved" tick;
//! Appearance restyles the window immediately. Each control records its key so a test can
//! check that the whole schema is covered.

use std::time::Duration;

use easyplay_core::config::schema::{
    Accent, ArtworkOnline, CloseBehavior, ControllerDefault, GridDensity, HandleExe, LibraryView,
    LogLevel, ReduceMotion, RendererPref, RuntimeDefault, Theme, WeekStart,
};
use easyplay_core::config::{PreferencePatch, Preferences};
use easyplay_core::model::{BasePreset, Channel};
use egui::{Align, Key, Layout, Modifiers, Panel, ScrollArea, Sense, Ui, vec2};

use super::Env;
use crate::app::{UiState, appearance_of};
use crate::bridge::{HandlerState, UiCommand};
use crate::input::picker::PickPurpose;
use crate::route::{Route, SettingsTab};
use crate::strings;
use crate::strings::settings as s;
use crate::theme::tokens::{control, icon as icon_size, layout, space};
use crate::theme::{self, Type, icons};
use crate::vm::dialogs::{Confirm, Dialog};
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::select::select;
use crate::widgets::text_field::TextField;
use crate::widgets::toggle::toggle;

/// Tabs in order.
pub const TABS: [SettingsTab; 7] = [
    SettingsTab::General,
    SettingsTab::Appearance,
    SettingsTab::Protection,
    SettingsTab::CompatibilityLayers,
    SettingsTab::Integration,
    SettingsTab::Advanced,
    SettingsTab::About,
];

fn keys_id() -> egui::Id {
    egui::Id::unique("settings.keys")
}

/// The preference keys that had a control in the frames drawn so far (tests).
pub fn rendered_keys(ctx: &egui::Context) -> Vec<String> {
    ctx.data(|d| d.get_temp::<Vec<String>>(keys_id()).unwrap_or_default())
}

fn note_key(ui: &Ui, key: &str) {
    ui.ctx().data_mut(|d| {
        let v: &mut Vec<String> = d.get_temp_mut_or_default(keys_id());
        if !v.iter().any(|k| k == key) {
            v.push(key.to_owned());
        }
    });
}

/// Apply a change: optimistic local preferences plus `SetPreference`.
fn commit(
    state: &mut UiState,
    env: &mut Env<'_>,
    key: &str,
    patch: PreferencePatch,
    next: Preferences,
) {
    if next.appearance != env.bridge.prefs().appearance {
        let a = appearance_of(&next, theme::appearance(&env.ctx), &theme::system(&env.ctx));
        theme::apply(&env.ctx, &a);
        env.ctx.set_zoom_factor(theme::zoom(&env.ctx, &a));
    }
    env.bridge.send(UiCommand::SetPreference {
        patch,
        preview: Box::new(next.clone()),
    });
    env.bridge.set_prefs_preview(next);
    state.settings.saved.insert(key.to_owned(), env.mono);
}

/// Set a boolean preference.
pub fn set_bool(
    state: &mut UiState,
    env: &mut Env<'_>,
    key: &str,
    value: bool,
    apply: impl FnOnce(&mut Preferences, bool),
) {
    let mut next = (**env.bridge.prefs()).clone();
    apply(&mut next, value);
    commit(
        state,
        env,
        key,
        PreferencePatch::new().set(key, value),
        next,
    );
}

fn saved_tick(ui: &mut Ui, state: &UiState, env: &Env<'_>, key: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    if let Some(t) = state.settings.saved.get(key) {
        let since = env.mono.saturating_duration_since(*t);
        if since < Duration::from_secs(2) {
            ui.label(icons::icon(icons::CHECK, icon_size::INLINE).color(p.success));
            ui.label(
                Type::Caption
                    .rich(strings::feedback::SAVED)
                    .color(p.text_secondary),
            );
            ui.ctx()
                .request_repaint_after(Duration::from_secs(2) - since);
        }
    }
}

fn heading(ui: &mut Ui, text: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    // Sections read as groups: more room above a heading than below it (UX-004).
    ui.add_space(space::S6);
    let r = ui.label(Type::Heading.rich(text).color(p.text_primary));
    crate::a11y::labelled(ui.ctx(), r.id, egui::Role::Heading, text);
    ui.add_space(space::S2);
}

fn caption(ui: &mut Ui, text: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.add(egui::Label::new(Type::Caption.rich(text).color(p.text_secondary)).wrap());
}

#[allow(clippy::too_many_arguments)]
fn bool_row(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    key: &'static str,
    label: &str,
    desc: Option<&str>,
    get: fn(&Preferences) -> bool,
    set: fn(&mut Preferences, bool),
) {
    note_key(ui, key);
    let mut v = get(env.bridge.prefs());
    let r = ui.horizontal(|ui| {
        let w = ui.available_width() - space::S16 - space::S6;
        let t = ui
            .allocate_ui_with_layout(
                vec2(w.max(200.0), 0.0),
                Layout::top_down(Align::Min),
                |ui| toggle(ui, &mut v, label, desc),
            )
            .inner;
        saved_tick(ui, state, env, key);
        t
    });
    if r.inner.changed() {
        set_bool(state, env, key, v, set);
    }
}

/// The label of a settings row. On pages wide enough the labels form a fixed column, so the
/// controls of consecutive rows line up (UX-004); narrow pages keep the natural width.
fn row_label(ui: &mut Ui, text: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let col = 184.0;
    if ui.available_width() >= 480.0 {
        ui.allocate_ui_with_layout(
            vec2(col, control::MD),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(col);
                ui.label(Type::Body.rich(text).color(p.text_primary));
            },
        );
    } else {
        ui.label(Type::Body.rich(text).color(p.text_primary));
    }
}

#[allow(clippy::too_many_arguments)]
fn choice_row(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    key: &'static str,
    label: &str,
    options: &[&str],
    words: &[&'static str],
    current: usize,
    set: impl FnOnce(&mut Preferences, usize),
) {
    note_key(ui, key);
    let mut i = current;
    let changed = ui
        .horizontal_wrapped(|ui| {
            row_label(ui, label);
            let segs: Vec<Segment<'_>> = options
                .iter()
                .map(|l| Segment {
                    icon: None,
                    label: l,
                })
                .collect();
            let c = if options.len() <= 4 {
                segmented(ui, label, &mut i, &segs, false).changed()
            } else {
                select(ui, label, None, None, &mut i, options, false).changed()
            };
            saved_tick(ui, state, env, key);
            c
        })
        .inner;
    if changed {
        let mut next = (**env.bridge.prefs()).clone();
        set(&mut next, i);
        commit(
            state,
            env,
            key,
            PreferencePatch::new().set(key, words[i]),
            next,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn number_row(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    key: &'static str,
    label: &str,
    values: &[i64],
    unit: fn(i64) -> String,
    current: i64,
    set: impl FnOnce(&mut Preferences, i64),
) {
    note_key(ui, key);
    let labels: Vec<String> = values.iter().map(|v| unit(*v)).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut i = values.iter().position(|v| *v == current).unwrap_or(0);
    let changed = ui
        .horizontal(|ui| {
            row_label(ui, label);
            let c = select(ui, label, None, None, &mut i, &refs, false).changed();
            saved_tick(ui, state, env, key);
            c
        })
        .inner;
    if changed {
        let mut next = (**env.bridge.prefs()).clone();
        set(&mut next, values[i]);
        commit(
            state,
            env,
            key,
            PreferencePatch::new().set(key, values[i]),
            next,
        );
    }
}

fn idx<T: PartialEq + Copy>(all: &[T], v: T) -> usize {
    all.iter().position(|x| *x == v).unwrap_or(0)
}

fn general(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let prefs = env.bridge.prefs().clone();
    heading(ui, s::GENERAL);
    choice_row(
        ui,
        state,
        env,
        "general.language",
        s::LANGUAGE,
        &[s::LANG_SYSTEM, "English"],
        &["system", "en"],
        usize::from(prefs.general.language == "en"),
        |p, i| {
            p.general.language = ["system", "en"][i].to_owned();
        },
    );
    choice_row(
        ui,
        state,
        env,
        "general.close_behavior",
        s::CLOSE,
        &[s::CLOSE_QUIT, s::CLOSE_ASK],
        &["quit", "ask-if-running"],
        idx(CloseBehavior::ALL, prefs.general.close_behavior),
        |p, i| {
            p.general.close_behavior = CloseBehavior::ALL[i];
        },
    );
    caption(ui, s::CLOSE_HINT);
    bool_row(
        ui,
        state,
        env,
        "general.confirm_remove",
        s::CONFIRM_REMOVE,
        None,
        |p| p.general.confirm_remove,
        |p, v| p.general.confirm_remove = v,
    );
    bool_row(
        ui,
        state,
        env,
        "general.check_missing_on_focus",
        s::CHECK_MISSING,
        Some(s::CHECK_MISSING_HINT),
        |p| p.general.check_missing_on_focus,
        |p, v| p.general.check_missing_on_focus = v,
    );
    heading(ui, s::NOTIFICATIONS);
    bool_row(
        ui,
        state,
        env,
        "launch.notify_on_exit",
        s::NOTIFY_EXIT,
        None,
        |p| p.launch.notify_on_exit,
        |p, v| p.launch.notify_on_exit = v,
    );
    bool_row(
        ui,
        state,
        env,
        "launch.notify_on_crash",
        s::NOTIFY_CRASH,
        None,
        |p| p.launch.notify_on_crash,
        |p, v| p.launch.notify_on_crash = v,
    );
    heading(ui, s::STATISTICS);
    bool_row(
        ui,
        state,
        env,
        "statistics.enabled",
        s::STATS_ON,
        Some(s::STATS_ON_HINT),
        |p| p.statistics.enabled,
        |p, v| p.statistics.enabled = v,
    );
    bool_row(
        ui,
        state,
        env,
        "statistics.keep_history_of_removed",
        s::KEEP_REMOVED,
        None,
        |p| p.statistics.keep_history_of_removed,
        |p, v| p.statistics.keep_history_of_removed = v,
    );
    bool_row(
        ui,
        state,
        env,
        "general.show_tool_sessions_in_history",
        s::TOOL_SESSIONS,
        None,
        |p| p.general.show_tool_sessions_in_history,
        |p, v| p.general.show_tool_sessions_in_history = v,
    );
    choice_row(
        ui,
        state,
        env,
        "statistics.week_starts_on",
        s::WEEK_START,
        &[s::SYSTEM, s::MONDAY, s::SUNDAY],
        &["system", "monday", "sunday"],
        idx(WeekStart::ALL, prefs.statistics.week_starts_on),
        |p, i| {
            p.statistics.week_starts_on = WeekStart::ALL[i];
        },
    );
    number_row(
        ui,
        state,
        env,
        "statistics.min_session_secs",
        s::MIN_SESSION,
        &[0, 30, 60, 120, 300],
        s::seconds,
        i64::from(prefs.statistics.min_session_secs),
        |p, v| {
            p.statistics.min_session_secs = v as u32;
        },
    );
    heading(ui, s::ARTWORK);
    choice_row(
        ui,
        state,
        env,
        "artwork.online",
        s::ARTWORK_ONLINE,
        &[s::ARTWORK_OFF, s::ARTWORK_STEAM],
        &["off", "steam"],
        idx(ArtworkOnline::ALL, prefs.artwork.online),
        |p, i| {
            p.artwork.online = ArtworkOnline::ALL[i];
        },
    );
    caption(ui, s::ARTWORK_HINT);
    bool_row(
        ui,
        state,
        env,
        "artwork.name_search",
        s::NAME_SEARCH,
        Some(s::NAME_SEARCH_HINT),
        |p| p.artwork.name_search,
        |p, v| p.artwork.name_search = v,
    );
    ui.add_space(space::S4);
    if ui
        .add(Button::new(s::CLEAR_STATS).variant(Variant::Danger))
        .clicked()
    {
        state.show_dialog(Dialog::Confirm(Confirm::ClearStatistics));
    }
}

fn appearance(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let prefs = env.bridge.prefs().clone();
    let p = theme::theme(ui.ctx()).palette.clone();
    heading(ui, s::APPEARANCE);
    choice_row(
        ui,
        state,
        env,
        "appearance.theme",
        s::THEME,
        &[s::SYSTEM, s::DARK, s::LIGHT, s::HIGH_CONTRAST],
        &["system", "dark", "light", "high-contrast"],
        idx(Theme::ALL, prefs.appearance.theme),
        |p, i| {
            p.appearance.theme = Theme::ALL[i];
        },
    );
    choice_row(
        ui,
        state,
        env,
        "appearance.accent",
        s::ACCENT,
        &["Tide", "Ember", "Iris", "Moss"],
        &["tide", "ember", "iris", "moss"],
        idx(Accent::ALL, prefs.appearance.accent),
        |p, i| {
            p.appearance.accent = Accent::ALL[i];
        },
    );
    note_key(ui, "appearance.ui_scale");
    let mut scale = prefs.appearance.ui_scale as f32;
    let changed = ui
        .horizontal(|ui| {
            row_label(ui, s::UI_SCALE);
            let shown = s::percent(f64::from(scale));
            let r = crate::widgets::slider::slider(
                ui,
                &mut scale,
                0.75..=2.0,
                0.05,
                // Leave room for the value and *Reset* on narrow pages.
                (ui.available_width() - 136.0).clamp(80.0, 200.0),
                s::UI_SCALE,
                &shown,
            );
            ui.label(
                Type::Body
                    .numeric(s::percent(f64::from(scale)))
                    .color(p.text_secondary),
            );
            let reset = ui.add(Button::ghost(s::RESET).size(Size::Sm));
            if reset.clicked() {
                scale = 1.0;
            }
            saved_tick(ui, state, env, "appearance.ui_scale");
            r.drag_stopped() || reset.clicked() || (r.changed() && !r.dragged())
        })
        .inner;
    if changed && (f64::from(scale) - prefs.appearance.ui_scale).abs() > 1e-3 {
        let mut next = (*prefs).clone();
        next.appearance.ui_scale = f64::from(scale);
        commit(
            state,
            env,
            "appearance.ui_scale",
            PreferencePatch::new().set("appearance.ui_scale", f64::from(scale)),
            next,
        );
    }
    choice_row(
        ui,
        state,
        env,
        "appearance.reduce_motion",
        s::REDUCE_MOTION,
        &[s::SYSTEM, s::ON, s::OFF],
        &["system", "on", "off"],
        idx(ReduceMotion::ALL, prefs.appearance.reduce_motion),
        |p, i| {
            p.appearance.reduce_motion = ReduceMotion::ALL[i];
        },
    );
    choice_row(
        ui,
        state,
        env,
        "appearance.library_view",
        s::LIBRARY_VIEW,
        &[s::GRID, s::LIST],
        &["grid", "list"],
        idx(LibraryView::ALL, prefs.appearance.library_view),
        |p, i| {
            p.appearance.library_view = LibraryView::ALL[i];
        },
    );
    choice_row(
        ui,
        state,
        env,
        "appearance.grid_density",
        s::DENSITY,
        &[s::COMPACT, s::COMFORTABLE, s::LARGE],
        &["compact", "comfortable", "large"],
        idx(GridDensity::ALL, prefs.appearance.grid_density),
        |p, i| {
            p.appearance.grid_density = GridDensity::ALL[i];
        },
    );
    bool_row(
        ui,
        state,
        env,
        "appearance.show_playtime_on_cards",
        s::PLAYTIME_ON_CARDS,
        None,
        |p| p.appearance.show_playtime_on_cards,
        |p, v| p.appearance.show_playtime_on_cards = v,
    );
    // Widths are set by dragging list headers; here they can only be reset.
    let key = "appearance.list_column_widths";
    note_key(ui, key);
    let reset = ui
        .horizontal(|ui| {
            row_label(ui, s::LIST_COLUMNS);
            let custom = !prefs.appearance.list_column_widths.is_empty();
            let r = ui.add_enabled(custom, Button::ghost(s::RESET).size(Size::Sm));
            saved_tick(ui, state, env, key);
            r.clicked()
        })
        .inner;
    if reset {
        state.library.widths.clear();
        state.library.widths_loaded = false;
        let mut next = (*prefs).clone();
        next.appearance.list_column_widths.clear();
        commit(
            state,
            env,
            key,
            PreferencePatch::new().set(key, easyplay_core::config::ColumnWidth::to_value(&[])),
            next,
        );
    }
}

fn protection(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let prefs = env.bridge.prefs().clone();
    heading(ui, s::PROTECTION);
    choice_row(
        ui,
        state,
        env,
        "protection.default_preset",
        s::DEFAULT_PRESET,
        &["Offline", "Online", "Standard"],
        &["offline", "online", "standard"],
        idx(&BasePreset::ALL, prefs.protection.default_preset),
        |p, i| {
            p.protection.default_preset = BasePreset::ALL[i];
        },
    );
    choice_row(
        ui,
        state,
        env,
        "protection.controller_access_default",
        s::CONTROLLERS_DEFAULT,
        &[s::CONNECTED_ONLY, s::ALL_WHEN_ALLOWED],
        &["detected", "all-when-safe"],
        idx(
            ControllerDefault::ALL,
            prefs.protection.controller_access_default,
        ),
        |p, i| {
            p.protection.controller_access_default = ControllerDefault::ALL[i];
        },
    );
    heading(ui, s::ISOLATION);
    caption(
        ui,
        if env.bridge.host().reflinks {
            s::ISOLATION_REFLINK
        } else {
            s::ISOLATION_SHARED
        },
    );
    bool_row(
        ui,
        state,
        env,
        "protection.always_isolate_runtime",
        s::ALWAYS_ISOLATE,
        Some(s::ALWAYS_ISOLATE_HINT),
        |p| p.protection.always_isolate_runtime,
        |p, v| p.protection.always_isolate_runtime = v,
    );
    bool_row(
        ui,
        state,
        env,
        "protection.prepare_online",
        s::PREPARE_ONLINE,
        Some(s::PREPARE_ONLINE_HINT),
        |p| p.protection.prepare_online,
        |p, v| p.protection.prepare_online = v,
    );
    bool_row(
        ui,
        state,
        env,
        "protection.seccomp_default",
        s::SECCOMP_DEFAULT,
        None,
        |p| p.protection.seccomp_default,
        |p, v| p.protection.seccomp_default = v,
    );
    bool_row(
        ui,
        state,
        env,
        "protection.warn_on_x11",
        s::WARN_X11,
        None,
        |p| p.protection.warn_on_x11,
        |p, v| p.protection.warn_on_x11 = v,
    );
}

fn layers(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let prefs = env.bridge.prefs().clone();
    let rt = env.bridge.runtimes().clone();
    heading(ui, s::LAYERS);
    note_key(ui, "runtime.default");
    let mut options: Vec<(String, RuntimeDefault)> = vec![
        (
            s::channel_latest(Channel::UmuProton.umu_codename()),
            RuntimeDefault::UmuProton,
        ),
        (
            s::channel_latest(Channel::GeProton.umu_codename()),
            RuntimeDefault::GeProton,
        ),
    ];
    for l in &rt.layers {
        options.push((
            l.build.display_name.clone(),
            RuntimeDefault::Pinned(l.build.name.clone()),
        ));
    }
    let labels: Vec<&str> = options.iter().map(|o| o.0.as_str()).collect();
    let mut i = options
        .iter()
        .position(|o| o.1 == prefs.runtime.default)
        .unwrap_or(0);
    let users: u32 = rt.layers.iter().map(|l| l.used_by).sum();
    ui.horizontal_wrapped(|ui| {
        row_label(ui, s::DEFAULT_LAYER);
        if select(
            ui,
            s::DEFAULT_LAYER,
            None,
            Some(icons::LAYERS),
            &mut i,
            &labels,
            false,
        )
        .changed()
        {
            let mut next = (*prefs).clone();
            next.runtime.default = options[i].1.clone();
            commit(
                state,
                env,
                "runtime.default",
                PreferencePatch::new().set("runtime.default", next.runtime.default.to_string()),
                next,
            );
        }
        ui.label(
            Type::Caption
                .rich(s::affects(users))
                .color(p.text_secondary),
        );
        saved_tick(ui, state, env, "runtime.default");
    });
    heading(ui, s::INSTALLED);
    if rt.layers.is_empty() {
        caption(ui, s::NONE_YET);
    }
    let mut remove = None;
    if ui.available_width() < 520.0 {
        // Narrow: one block per layer, details on a second line.
        for l in &rt.layers {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        Type::BodyStrong
                            .rich(&l.build.display_name)
                            .color(p.text_primary),
                    );
                    ui.label(
                        Type::Caption
                            .rich(s::layer_details(
                                &l.build.origin_label,
                                &l.size.map_or_else(|| "—".to_owned(), strings::fmt::bytes),
                                &s::used_by(l.used_by),
                            ))
                            .color(p.text_secondary),
                    );
                });
                if l.build.origin == easyplay_core::model::RuntimeOrigin::Store {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(Button::ghost(s::REMOVE).size(Size::Sm)).clicked() {
                            remove = Some(l.clone());
                        }
                    });
                }
            });
            ui.add_space(space::S1);
        }
    } else {
        egui::Grid::new("settings.layers")
            .num_columns(5)
            .spacing(vec2(space::S5, space::S2))
            .show(ui, |ui| {
                for l in &rt.layers {
                    ui.label(
                        Type::BodyStrong
                            .rich(&l.build.display_name)
                            .color(p.text_primary),
                    );
                    ui.label(
                        Type::Body
                            .rich(&l.build.origin_label)
                            .color(p.text_secondary),
                    );
                    ui.label(
                        Type::Body
                            .numeric(l.size.map_or_else(|| "—".to_owned(), strings::fmt::bytes))
                            .color(p.text_secondary),
                    );
                    ui.label(
                        Type::Body
                            .rich(s::used_by(l.used_by))
                            .color(p.text_secondary),
                    );
                    if l.build.origin == easyplay_core::model::RuntimeOrigin::Store
                        && ui.add(Button::ghost(s::REMOVE).size(Size::Sm)).clicked()
                    {
                        remove = Some(l.clone());
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
    }
    if let Some(l) = remove {
        state.show_dialog(Dialog::Confirm(Confirm::RemoveLayer {
            name: l.build.name.clone(),
            label: l.build.display_name.clone(),
            size: l.size,
            used_by: l.used_by,
        }));
    }
    heading(ui, s::RUNTIME);
    for (name, version, verified) in &rt.slr {
        ui.horizontal_wrapped(|ui| {
            ui.label(Type::Body.rich(name).color(p.text_primary));
            ui.label(Type::Body.rich(version).color(p.text_secondary));
            if let Some(t) = verified {
                ui.label(
                    Type::Caption
                        .rich(s::verified(&strings::fmt::relative_inline(
                            *t,
                            env.now,
                            env.utc_offset_s,
                        )))
                        .color(p.text_muted),
                );
            }
            if ui.add(Button::ghost(s::VERIFY).size(Size::Sm)).clicked() {
                env.bridge.send(UiCommand::VerifyRuntime);
            }
        });
    }
    heading(ui, s::UPDATES);
    ui.horizontal(|ui| {
        if ui
            .add(
                Button::new(s::CHECK_UPDATES)
                    .icon(icons::REFRESH)
                    .size(Size::Sm),
            )
            .clicked()
        {
            env.bridge.send(UiCommand::RefreshRuntimes);
        }
        if let Some(t) = rt.last_checked {
            ui.label(
                Type::Caption
                    .rich(s::last_checked(&strings::fmt::relative_inline(
                        t,
                        env.now,
                        env.utc_offset_s,
                    )))
                    .color(p.text_muted),
            );
        }
    });
    for d in &rt.downloads {
        ui.label(
            Type::Body
                .numeric(s::downloading(&d.label, &strings::fmt::bytes(d.bytes)))
                .color(p.text_secondary),
        );
    }
    bool_row(
        ui,
        state,
        env,
        "runtime.auto_update",
        s::AUTO_UPDATE,
        None,
        |p| p.runtime.auto_update,
        |p, v| p.runtime.auto_update = v,
    );
    number_row(
        ui,
        state,
        env,
        "runtime.auto_update_interval_hours",
        s::INTERVAL,
        &[6, 12, 24, 72, 168],
        s::hours,
        i64::from(prefs.runtime.auto_update_interval_hours),
        |p, v| {
            p.runtime.auto_update_interval_hours = v as u32;
        },
    );
    bool_row(
        ui,
        state,
        env,
        "runtime.auto_remove_unused",
        s::AUTO_REMOVE,
        None,
        |p| p.runtime.auto_remove_unused,
        |p, v| p.runtime.auto_remove_unused = v,
    );
    heading(ui, s::SEARCH_FOLDERS);
    note_key(ui, "runtime.extra_search_dirs");
    let mut drop_dir = None;
    for (i, d) in prefs.runtime.extra_search_dirs.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(
                Type::Mono
                    .rich(d.display().to_string())
                    .color(p.text_secondary),
            );
            if ui
                .add(crate::widgets::icon_button::IconButton::new(
                    icons::CLOSE,
                    &s::remove_folder(&d.display().to_string()),
                ))
                .clicked()
            {
                drop_dir = Some(i);
            }
        });
    }
    if let Some(i) = drop_dir {
        let mut next = (*prefs).clone();
        next.runtime.extra_search_dirs.remove(i);
        let list: Vec<String> = next
            .runtime
            .extra_search_dirs
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        commit(
            state,
            env,
            "runtime.extra_search_dirs",
            PreferencePatch::new().set("runtime.extra_search_dirs", list),
            next,
        );
    }
    if ui
        .add(
            Button::new(s::ADD_SEARCH_FOLDER)
                .icon(icons::FOLDER_PLUS)
                .size(Size::Sm),
        )
        .clicked()
    {
        state.picks.push(PickPurpose::LayerFolder);
    }
    let total: u64 = rt.layers.iter().filter_map(|l| l.size).sum();
    caption(ui, &s::disk_total(&strings::fmt::bytes(total)));
}

fn integration(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let prefs = env.bridge.prefs().clone();
    let host = env.bridge.host().clone();
    heading(ui, s::FILE_TYPES);
    ui.horizontal_wrapped(|ui| {
        row_label(ui, s::EXE_FILES);
        ui.label(
            Type::Body
                .rich(s::handler(&host.handler))
                .color(p.text_secondary),
        );
    });
    // The action on its own line, so a long handler name never pushes it out of view.
    let ours = host.handler == HandlerState::EasyPlay;
    let label = if ours {
        s::RESTORE_PREVIOUS
    } else {
        s::USE_EASYPLAY
    };
    if ui.add(Button::new(label).size(Size::Sm)).clicked() {
        env.bridge
            .send(UiCommand::SetDefaultHandler { enable: !ours });
    }
    choice_row(
        ui,
        state,
        env,
        "integration.handle_exe",
        s::HANDLE_EXE,
        &[s::ASK, s::YES, s::NO],
        &["ask", "yes", "no"],
        idx(HandleExe::ALL, prefs.integration.handle_exe),
        |p, i| {
            p.integration.handle_exe = HandleExe::ALL[i];
        },
    );
    bool_row(
        ui,
        state,
        env,
        "launch.quick_launch_window",
        s::QUICK_WINDOW,
        Some(s::QUICK_WINDOW_HINT),
        |p| p.launch.quick_launch_window,
        |p, v| p.launch.quick_launch_window = v,
    );
    bool_row(
        ui,
        state,
        env,
        "integration.single_click_play",
        s::SINGLE_CLICK,
        None,
        |p| p.integration.single_click_play,
        |p, v| p.integration.single_click_play = v,
    );
    bool_row(
        ui,
        state,
        env,
        "integration.gamepad_navigation",
        s::GAMEPAD,
        None,
        |p| p.integration.gamepad_navigation,
        |p, v| p.integration.gamepad_navigation = v,
    );
    bool_row(
        ui,
        state,
        env,
        "integration.single_key_shortcuts",
        s::SINGLE_KEYS,
        Some(s::SINGLE_KEYS_HINT),
        |p| p.integration.single_key_shortcuts,
        |p, v| p.integration.single_key_shortcuts = v,
    );
    heading(ui, s::MENU_SHORTCUTS);
    if host.shortcuts.is_empty() {
        caption(ui, s::NO_SHORTCUTS);
    }
    for sc in &host.shortcuts {
        ui.horizontal(|ui| {
            ui.label(Type::Body.rich(&sc.name).color(p.text_primary));
            if ui.add(Button::ghost(s::REMOVE).size(Size::Sm)).clicked() {
                env.bridge.send(UiCommand::SetShortcut {
                    id: sc.game,
                    enable: false,
                });
            }
        });
    }
}

fn advanced(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let prefs = env.bridge.prefs().clone();
    super::debug::doctor::section(ui, state, env);
    heading(ui, s::DEVELOPER);
    note_key(ui, "advanced.developer_mode");
    let mut dev = prefs.advanced.developer_mode;
    if toggle(ui, &mut dev, s::DEVELOPER_MODE, Some(s::DEVELOPER_HINT)).changed() {
        if dev {
            state.show_dialog(Dialog::Confirm(Confirm::DeveloperMode));
        } else {
            set_bool(state, env, "advanced.developer_mode", false, |p, v| {
                p.advanced.developer_mode = v
            });
        }
    }
    choice_row(
        ui,
        state,
        env,
        "logging.level",
        s::LOG_LEVEL,
        &["Error", "Warn", "Info", "Debug", "Trace"],
        &["error", "warn", "info", "debug", "trace"],
        idx(LogLevel::ALL, prefs.logging.level),
        |p, i| {
            p.logging.level = LogLevel::ALL[i];
        },
    );
    number_row(
        ui,
        state,
        env,
        "logging.session_log_cap_mb",
        s::LOG_CAP,
        &[8, 16, 32, 64, 128],
        s::megabytes,
        i64::from(prefs.logging.session_log_cap_mb),
        |p, v| {
            p.logging.session_log_cap_mb = v as u32;
        },
    );
    number_row(
        ui,
        state,
        env,
        "logging.keep_session_logs",
        s::KEEP_LOGS,
        &[5, 10, 20, 50, 100],
        s::count,
        i64::from(prefs.logging.keep_session_logs),
        |p, v| {
            p.logging.keep_session_logs = v as u32;
        },
    );
    number_row(
        ui,
        state,
        env,
        "logging.keep_session_logs_days",
        s::KEEP_LOG_DAYS,
        &[7, 14, 30, 90, 365],
        s::days,
        i64::from(prefs.logging.keep_session_logs_days),
        |p, v| {
            p.logging.keep_session_logs_days = v as u32;
        },
    );
    bool_row(
        ui,
        state,
        env,
        "launch.verbose_launch_log",
        s::VERBOSE,
        None,
        |p| p.launch.verbose_launch_log,
        |p, v| p.launch.verbose_launch_log = v,
    );
    number_row(
        ui,
        state,
        env,
        "launch.stop_grace_secs",
        s::STOP_GRACE,
        &[5, 10, 20, 30, 60],
        s::seconds,
        i64::from(prefs.launch.stop_grace_secs),
        |p, v| {
            p.launch.stop_grace_secs = v as u8;
        },
    );
    choice_row(
        ui,
        state,
        env,
        "advanced.renderer",
        s::RENDERER,
        &[s::AUTOMATIC, "wgpu", "OpenGL"],
        &["auto", "wgpu", "glow"],
        idx(RendererPref::ALL, prefs.advanced.renderer),
        |p, i| {
            p.advanced.renderer = RendererPref::ALL[i];
        },
    );
    if state.settings.saved.contains_key("advanced.renderer") {
        caption(ui, s::RESTART);
    }
    heading(ui, s::GLOBAL_ENV);
    note_key(ui, "launch.global_env");
    let mut drop = None;
    for (i, v) in prefs.launch.global_env.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(
                Type::Mono
                    .rich(s::env_pair(&v.name, &v.value))
                    .color(p.text_secondary),
            );
            if ui
                .add(crate::widgets::icon_button::IconButton::new(
                    icons::CLOSE,
                    &strings::details::remove_var(i + 1),
                ))
                .clicked()
            {
                drop = Some(i);
            }
        });
    }
    let (name, value) = &mut state.settings.new_env;
    let mut add = false;
    ui.horizontal(|ui| {
        TextField::new(name, "settings.env.name")
            .accessible_name(&strings::details::var_name(
                prefs.launch.global_env.len() + 1,
            ))
            .mono()
            .width(180.0)
            .show(ui);
        ui.label(Type::Mono.rich("=").color(p.text_muted));
        TextField::new(value, "settings.env.value")
            .accessible_name(&strings::details::var_value(
                prefs.launch.global_env.len() + 1,
            ))
            .mono()
            .width(220.0)
            .show(ui);
        add = ui
            .add(Button::new(strings::details::ADD_VAR).size(Size::Sm))
            .clicked();
    });
    let candidate = easyplay_core::model::EnvVar {
        name: state.settings.new_env.0.trim().to_owned(),
        value: state.settings.new_env.1.clone(),
    };
    let problem =
        super::details::launch_options::env_problem(&candidate, prefs.advanced.developer_mode);
    if let Some(pr) = &problem {
        super::details::permissions::issue_line(ui, pr, true);
    }
    if drop.is_some() || (add && problem.is_none() && !candidate.name.is_empty()) {
        let mut next = (*prefs).clone();
        if let Some(i) = drop {
            next.launch.global_env.remove(i);
        } else {
            next.launch.global_env.push(candidate);
            state.settings.new_env = Default::default();
        }
        let list: Vec<std::collections::HashMap<String, String>> = next
            .launch
            .global_env
            .iter()
            .map(|v| {
                [
                    ("name".to_owned(), v.name.clone()),
                    ("value".to_owned(), v.value.clone()),
                ]
                .into_iter()
                .collect()
            })
            .collect();
        commit(
            state,
            env,
            "launch.global_env",
            PreferencePatch::new().set("launch.global_env", list),
            next,
        );
    }
    heading(ui, s::PATHS);
    for (key, label, current) in [
        ("paths.umu_run", s::UMU_RUN, prefs.paths.umu_run.clone()),
        ("paths.bwrap", s::BWRAP, prefs.paths.bwrap.clone()),
    ] {
        note_key(ui, key);
        let mut text = current.map(|p| p.display().to_string()).unwrap_or_default();
        let r = TextField::new(&mut text, key)
            .label(label)
            .hint(s::AUTOMATIC)
            .mono()
            .show(ui);
        if r.response.lost_focus() && text.trim() != current_text(&prefs, key) {
            let mut next = (*prefs).clone();
            let v = (!text.trim().is_empty()).then(|| std::path::PathBuf::from(text.trim()));
            if key == "paths.umu_run" {
                next.paths.umu_run = v;
            } else {
                next.paths.bwrap = v;
            }
            commit(
                state,
                env,
                key,
                PreferencePatch::new().set(key, text.trim().to_owned()),
                next,
            );
            state.settings.restart = true;
        }
    }
    heading(ui, s::MAINTENANCE);
    ui.horizontal_wrapped(|ui| {
        if ui
            .add(
                Button::new(s::CHECK_SYSTEM)
                    .icon(icons::WRENCH)
                    .size(Size::Sm),
            )
            .clicked()
        {
            env.bridge.send(UiCommand::RefreshHostFacts);
            state.onboarding.reduced = true;
            state.onboarding.step = crate::vm::screens::OnboardingStep::SystemCheck;
            state.nav.go_to(Route::Onboarding);
        }
        if ui
            .add(Button::new(s::OPEN_DATA).icon(icons::FOLDER).size(Size::Sm))
            .clicked()
        {
            env.bridge.send(UiCommand::OpenFolder(
                crate::bridge::FolderTarget::DataFolder,
            ));
        }
    });
    note_key(ui, "onboarding.completed");
    if ui
        .add(Button::ghost(s::RUN_WELCOME).size(Size::Sm))
        .clicked()
    {
        set_bool(state, env, "onboarding.completed", false, |p, v| {
            p.onboarding.completed = v
        });
        state.onboarding = Default::default();
        state.nav.go_to(Route::Onboarding);
    }
}

fn current_text(prefs: &Preferences, key: &str) -> String {
    let v = if key == "paths.umu_run" {
        &prefs.paths.umu_run
    } else {
        &prefs.paths.bwrap
    };
    v.as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

fn about(ui: &mut Ui, env: &Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    heading(ui, strings::APP_NAME);
    ui.label(
        Type::Body
            .numeric(s::version(&env.bridge.host().version))
            .color(p.text_secondary),
    );
    ui.add_space(space::S3);
    ui.add(egui::Label::new(Type::Body.rich(s::PRIVACY).color(p.text_primary)).wrap());
    caption(ui, s::PRIVACY_ENDPOINTS);
    heading(ui, s::LICENCES);
    for l in s::LICENCE_LINES {
        caption(ui, l);
    }
}

/// Show the screen.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, tab: SettingsTab) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let mut chosen = None;
    // Narrow windows keep more room for the content (A11Y-005).
    let nav_w = if ui.ctx().content_rect().width() < layout::COMPACT_BELOW {
        168.0
    } else {
        200.0
    };
    Panel::left("settings_tabs")
        .exact_size(nav_w)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(space::S3 as i8, space::S6 as i8)),
        )
        .show(ui, |ui| {
            ui.label(
                Type::Title
                    .rich(strings::nav::SETTINGS)
                    .color(p.text_primary),
            );
            ui.add_space(space::S4);
            let list = ui.scope(|ui| {
                for t in TABS {
                    let sel = t == tab;
                    let (rect, resp) = ui.allocate_exact_size(
                        vec2(ui.available_width(), layout::SIDEBAR_ITEM),
                        Sense::click(),
                    );
                    if sel || resp.hovered() || resp.has_focus() {
                        // The same plates as the sidebar's items.
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(theme::tokens::radius::ROW),
                            if sel { p.fill_selected } else { p.fill_hover },
                        );
                    }
                    if resp.has_focus() {
                        crate::widgets::focus_ring(
                            ui.painter(),
                            rect,
                            egui::CornerRadius::same(theme::tokens::radius::ROW),
                            &theme::theme(ui.ctx()),
                        );
                        crate::widgets::own_arrow_keys(&resp);
                        let (up, down) = ui.input_mut(|i| {
                            (
                                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                            )
                        });
                        let i = TABS.iter().position(|x| *x == t).unwrap_or(0);
                        if up && i > 0 {
                            chosen = Some(TABS[i - 1]);
                        }
                        if down && i + 1 < TABS.len() {
                            chosen = Some(TABS[i + 1]);
                        }
                    }
                    ui.painter().text(
                        rect.left_center() + vec2(space::S3, 0.0),
                        egui::Align2::LEFT_CENTER,
                        s::tab(t),
                        Type::Body.font_id(),
                        if sel {
                            p.text_primary
                        } else {
                            p.text_secondary
                        },
                    );
                    crate::a11y::option(&resp, egui::Role::Tab, s::tab(t), sel);
                    if sel && state.settings.tab != t {
                        state.settings.tab = t;
                        resp.request_focus();
                    }
                    if resp.clicked() {
                        chosen = Some(t);
                    }
                }
            });
            crate::a11y::labelled(
                ui.ctx(),
                list.response.id,
                egui::Role::TabList,
                strings::nav::SETTINGS,
            );
        });
    if let Some(t) = chosen {
        state.nav.replace(Route::Settings(t));
    }
    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(space::S6 as i8, space::S4 as i8)),
        )
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt(("settings", tab))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let w = ui.available_width().min(720.0);
                    ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
                        ui.set_width(w);
                        match tab {
                            SettingsTab::General => general(ui, state, env),
                            SettingsTab::Appearance => appearance(ui, state, env),
                            SettingsTab::Protection => protection(ui, state, env),
                            SettingsTab::CompatibilityLayers => layers(ui, state, env),
                            SettingsTab::Integration => integration(ui, state, env),
                            SettingsTab::Advanced => advanced(ui, state, env),
                            SettingsTab::About => about(ui, env),
                        }
                        ui.add_space(space::S8);
                    });
                });
        });
}
