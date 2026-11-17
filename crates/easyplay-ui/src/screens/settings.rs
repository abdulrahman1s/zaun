//! Settings (SCREEN_SPECS_2 §13, UI-023): a bar of seven tabs under the top bar, and under
//! it a scrolling page with a 620 column of setting rows (label and help at the left, one
//! control at the right) in groups under headings. The rows are the design's `SETTINGS`
//! table bound to the real preferences; a row whose condition is unmet is drawn faint and
//! takes no input. Every control applies at once (`SetPreference` with the UI's optimistic
//! result); Appearance restyles the window immediately. Each control records its key, so a
//! test can check that the whole schema is covered. A change of tab brings the column in
//! with a short rise.

use easyplay_core::config::schema::{
    ArtworkOnline, CloseBehavior, ControllerDefault, GridDensity, HandleExe, LibraryView, LogLevel,
    ReduceMotion, RendererPref, RuntimeDefault, Theme, WeekStart,
};
use easyplay_core::config::{PreferencePatch, Preferences};
use easyplay_core::model::{BasePreset, Channel, EnvVar, RuntimeOrigin};
use egui::{Align, Id, Layout, ScrollArea, Sense, Ui, pos2, vec2};

use super::Env;
use crate::app::{UiState, appearance_of};
use crate::bridge::{FolderTarget, HandlerState, UiCommand};
use crate::i18n::{Language, LanguageChoice};
use crate::input::picker::PickPurpose;
use crate::route::{Overlay, Route, SettingsTab};
use crate::strings;
use crate::strings::settings as s;
use crate::theme::tokens::{control, layout, space, travel};
use crate::theme::{self, Dur, Type, motion};
use crate::vm::dialogs::{Confirm, Dialog};
use crate::widgets::button::{Button, Variant};
use crate::widgets::choice::swatch;
use crate::widgets::rows::{self, Col, SettingRow};
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::select::select;
use crate::widgets::slider::slider;
use crate::widgets::tabs::Tabs;
use crate::widgets::text_field::TextField;
use crate::widgets::toggle::switch;

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

/// The interface size's range and step, in percent.
const SCALE: (f32, f32, f32) = (80.0, 150.0, 5.0);
/// "Count sessions longer than", in seconds.
const MIN_SESSIONS: [i64; 5] = [0, 30, 60, 120, 300];
/// "Check every", in hours.
const INTERVALS: [i64; 5] = [6, 12, 24, 72, 168];
/// "Largest session log", in MB.
const LOG_CAPS: [i64; 5] = [8, 16, 32, 64, 128];
/// "Session logs to keep per game".
const KEEP_LOGS: [i64; 5] = [5, 10, 20, 50, 100];
/// "Delete session logs older than", in days.
const LOG_DAYS: [i64; 5] = [7, 14, 30, 90, 365];
/// "When stopping a game, wait", in seconds.
const STOP_WAITS: [i64; 5] = [5, 10, 20, 30, 60];

fn keys_id() -> Id {
    Id::unique("settings.keys")
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

/// A row with a switch.
#[allow(clippy::too_many_arguments)]
fn switch_row(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    key: &'static str,
    label: &str,
    help: &str,
    enabled: bool,
    get: fn(&Preferences) -> bool,
    set: fn(&mut Preferences, bool),
) {
    note_key(ui, key);
    let mut v = get(env.bridge.prefs());
    let changed = SettingRow::new(label)
        .help(help)
        .enabled(enabled)
        .show(ui, |ui| {
            band(ui, control::TOGGLE_H, |ui| {
                switch(ui, &mut v, label).changed()
            })
        });
    if changed {
        set_bool(state, env, key, v, set);
    }
}

/// A row with a segmented control over `options`; `words` are their configuration values.
#[allow(clippy::too_many_arguments)]
fn seg_row(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    key: &'static str,
    label: &str,
    help: &str,
    enabled: bool,
    options: &[&str],
    words: &[&'static str],
    current: usize,
    set: impl FnOnce(&mut Preferences, usize),
) {
    note_key(ui, key);
    let mut i = current;
    let segs: Vec<Segment<'_>> = options
        .iter()
        .map(|l| Segment {
            icon: None,
            label: l,
        })
        .collect();
    let changed = SettingRow::new(label)
        .help(help)
        .enabled(enabled)
        .show(ui, |ui| {
            band(ui, control::SM, |ui| {
                segmented(ui, label, &mut i, &segs, false).changed()
            })
        });
    if changed && i != current {
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

/// A row with a select over numbers; the current value is offered even when it is not one
/// of `values`.
#[allow(clippy::too_many_arguments)]
fn number_row(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    key: &'static str,
    label: &str,
    enabled: bool,
    values: &[i64],
    unit: fn(i64) -> String,
    current: i64,
    set: impl FnOnce(&mut Preferences, i64),
) {
    note_key(ui, key);
    let mut values = values.to_vec();
    if !values.contains(&current) {
        values.push(current);
        values.sort_unstable();
    }
    let labels: Vec<String> = values.iter().map(|v| unit(*v)).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut i = values.iter().position(|v| *v == current).unwrap_or(0);
    let changed = SettingRow::new(label).enabled(enabled).show(ui, |ui| {
        band(ui, control::SM, |ui| {
            select(ui, label, None, None, &mut i, &refs, false).changed()
        })
    });
    if changed && values[i] != current {
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

/// A row whose control is one button. True when it was pressed.
fn button_row(ui: &mut Ui, label: &str, help: &str, enabled: bool, button: Button<'_>) -> bool {
    SettingRow::new(label)
        .help(help)
        .enabled(enabled)
        .show(ui, |ui| {
            band(ui, control::BUTTON, |ui| ui.add(button).clicked())
        })
}

/// A row's control laid out in a band `h` high (the control's own height). A centred
/// layout counts its whole height as used, so without the band the setting row would
/// measure the row rather than its control and grow by its padding every time.
pub(super) fn band<R>(ui: &mut Ui, h: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let avail = ui.available_rect_before_wrap();
    let rect = egui::Rect::from_min_size(
        pos2(avail.left(), avail.center().y - h / 2.0),
        vec2(avail.width(), h),
    );
    let gap = ui.spacing().item_spacing.x;
    let mut c = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    c.spacing_mut().item_spacing.x = gap;
    let out = add(&mut c);
    let used = egui::Rect::from_min_size(rect.min, vec2(c.min_rect().width(), h));
    ui.advance_cursor_after_rect(used);
    out
}

fn idx<T: PartialEq + Copy>(all: &[T], v: T) -> usize {
    all.iter().position(|x| *x == v).unwrap_or(0)
}

/// Quiet text the width of the column (an empty list, fine print).
fn quiet(ui: &mut Ui, text: &str, style: Type) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width();
    let g = style.galley(ui, text, p.t3, w, usize::MAX);
    let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
    ui.painter().galley(r.min, g, p.t3);
}

/// A ghost button in a table cell, its text ending at the cell's right edge.
fn cell_button(ui: &mut Ui, label: &str, name: &str) -> bool {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(-control::BUTTON_PAD);
        ui.add(Button::ghost(label).accessible_name(name)).clicked()
    })
    .inner
}

fn general(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let prefs = env.bridge.prefs().clone();
    seg_row(
        ui,
        state,
        env,
        "general.language",
        s::LANGUAGE(),
        "",
        true,
        &[
            s::LANG_SYSTEM(),
            Language::English.name(),
            Language::Arabic.name(),
        ],
        &["system", "en", "ar"],
        match LanguageChoice::parse(&prefs.general.language) {
            LanguageChoice::System => 0,
            LanguageChoice::Is(Language::English) => 1,
            LanguageChoice::Is(Language::Arabic) => 2,
        },
        |p, i| p.general.language = ["system", "en", "ar"][i].to_owned(),
    );
    seg_row(
        ui,
        state,
        env,
        "general.close_behavior",
        s::CLOSE(),
        s::CLOSE_HINT(),
        true,
        &[s::CLOSE_QUIT(), s::CLOSE_ASK()],
        &["quit", "ask-if-running"],
        idx(CloseBehavior::ALL, prefs.general.close_behavior),
        |p, i| p.general.close_behavior = CloseBehavior::ALL[i],
    );
    switch_row(
        ui,
        state,
        env,
        "general.confirm_remove",
        s::CONFIRM_REMOVE(),
        "",
        true,
        |p| p.general.confirm_remove,
        |p, v| p.general.confirm_remove = v,
    );
    switch_row(
        ui,
        state,
        env,
        "general.check_missing_on_focus",
        s::CHECK_MISSING(),
        s::CHECK_MISSING_HINT(),
        true,
        |p| p.general.check_missing_on_focus,
        |p, v| p.general.check_missing_on_focus = v,
    );
    rows::heading(ui, s::NOTIFICATIONS(), false);
    switch_row(
        ui,
        state,
        env,
        "launch.notify_on_exit",
        s::NOTIFY_EXIT(),
        "",
        true,
        |p| p.launch.notify_on_exit,
        |p, v| p.launch.notify_on_exit = v,
    );
    switch_row(
        ui,
        state,
        env,
        "launch.notify_on_crash",
        s::NOTIFY_CRASH(),
        "",
        true,
        |p| p.launch.notify_on_crash,
        |p, v| p.launch.notify_on_crash = v,
    );
    rows::heading(ui, s::STATISTICS(), false);
    let recording = prefs.statistics.enabled;
    switch_row(
        ui,
        state,
        env,
        "statistics.enabled",
        s::STATS_ON(),
        s::STATS_ON_HINT(),
        true,
        |p| p.statistics.enabled,
        |p, v| p.statistics.enabled = v,
    );
    switch_row(
        ui,
        state,
        env,
        "statistics.keep_history_of_removed",
        s::KEEP_REMOVED(),
        "",
        recording,
        |p| p.statistics.keep_history_of_removed,
        |p, v| p.statistics.keep_history_of_removed = v,
    );
    switch_row(
        ui,
        state,
        env,
        "general.show_tool_sessions_in_history",
        s::TOOL_SESSIONS(),
        "",
        recording,
        |p| p.general.show_tool_sessions_in_history,
        |p, v| p.general.show_tool_sessions_in_history = v,
    );
    seg_row(
        ui,
        state,
        env,
        "statistics.week_starts_on",
        s::WEEK_START(),
        "",
        true,
        &[s::SYSTEM(), s::MONDAY(), s::SUNDAY()],
        &["system", "monday", "sunday"],
        idx(WeekStart::ALL, prefs.statistics.week_starts_on),
        |p, i| p.statistics.week_starts_on = WeekStart::ALL[i],
    );
    number_row(
        ui,
        state,
        env,
        "statistics.min_session_secs",
        s::MIN_SESSION(),
        true,
        &MIN_SESSIONS,
        s::seconds,
        i64::from(prefs.statistics.min_session_secs),
        |p, v| p.statistics.min_session_secs = v as u32,
    );
    if button_row(
        ui,
        s::CLEAR_STATS(),
        s::CLEAR_STATS_HINT(),
        true,
        Button::new(s::CLEAR()).variant(Variant::Warn),
    ) {
        state.show_dialog(Dialog::Confirm(Confirm::ClearStatistics));
    }
    rows::heading(ui, s::ARTWORK(), false);
    seg_row(
        ui,
        state,
        env,
        "artwork.online",
        s::ARTWORK_ONLINE(),
        s::ARTWORK_HINT(),
        true,
        &[s::ARTWORK_OFF(), s::ARTWORK_STEAM()],
        &["off", "steam"],
        idx(ArtworkOnline::ALL, prefs.artwork.online),
        |p, i| p.artwork.online = ArtworkOnline::ALL[i],
    );
    switch_row(
        ui,
        state,
        env,
        "artwork.name_search",
        s::NAME_SEARCH(),
        s::NAME_SEARCH_HINT(),
        prefs.artwork.online != ArtworkOnline::Off,
        |p| p.artwork.name_search,
        |p, v| p.artwork.name_search = v,
    );
    // Steam games (ADR-0031): off by default; "Check now" only while it is on.
    rows::heading(ui, strings::steam::GROUP(), false);
    switch_row(
        ui,
        state,
        env,
        "steam.show_games",
        strings::steam::SHOW_OPTION(),
        strings::steam::SHOW_OPTION_HELP(),
        true,
        |p| p.steam.show_games,
        |p, v| p.steam.show_games = v,
    );
    if button_row(
        ui,
        strings::steam::RESCAN(),
        strings::steam::RESCAN_HELP(),
        env.bridge.prefs().steam.show_games,
        Button::new(strings::steam::CHECK_NOW()).variant(Variant::Secondary),
    ) {
        env.bridge
            .send(UiCommand::Steam(crate::bridge::SteamAction::Rescan));
    }
}

fn appearance(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let prefs = env.bridge.prefs().clone();
    seg_row(
        ui,
        state,
        env,
        "appearance.theme",
        s::THEME(),
        "",
        true,
        &[s::SYSTEM(), s::DARK(), s::LIGHT(), s::HIGH_CONTRAST()],
        &["system", "dark", "light", "high-contrast"],
        idx(Theme::ALL, prefs.appearance.theme),
        |p, i| p.appearance.theme = Theme::ALL[i],
    );
    // Accent: four swatches.
    note_key(ui, "appearance.accent");
    let current = idx(
        easyplay_core::config::schema::Accent::ALL,
        prefs.appearance.accent,
    );
    let picked = SettingRow::new(s::ACCENT()).show(ui, |ui| {
        let group = band(ui, control::SWATCH, |ui| {
            ui.spacing_mut().item_spacing.x = layout::SWATCH_GAP;
            let mut picked = None;
            for (i, a) in theme::palette::Accent::ALL.iter().enumerate() {
                if swatch(ui, a.swatch(), i == current, s::ACCENTS()[i]).clicked() {
                    picked = Some(i);
                }
            }
            (picked, ui.scope_id())
        });
        crate::a11y::labelled(ui.ctx(), group.1, egui::Role::RadioGroup, s::ACCENT());
        group.0
    });
    if let Some(i) = picked
        && i != current
    {
        let a = easyplay_core::config::schema::Accent::ALL[i];
        let mut next = (*prefs).clone();
        next.appearance.accent = a;
        commit(
            state,
            env,
            "appearance.accent",
            PreferencePatch::new().set("appearance.accent", ["tide", "ember", "iris", "moss"][i]),
            next,
        );
    }
    // Interface size: 80–150 % in steps of 5, the value, and Reset.
    note_key(ui, "appearance.ui_scale");
    let stored = (prefs.appearance.ui_scale * 100.0).round() as f32;
    let mut pct = stored;
    let (lo, hi, step) = SCALE;
    let done = SettingRow::new(s::UI_SCALE()).show(ui, |ui| {
        band(ui, control::BUTTON, |ui| {
            let shown = s::percent(f64::from(pct) / 100.0);
            let r = slider(
                ui,
                &mut pct,
                lo..=hi,
                step,
                control::SLIDER_W,
                s::UI_SCALE(),
                &shown,
            );
            let text = s::percent(f64::from(pct) / 100.0);
            let (rect, _) = ui.allocate_exact_size(
                vec2(layout::SLIDER_VALUE_W, Type::Body.line_height()),
                Sense::hover(),
            );
            let g = Type::Body.numeric_galley(ui, &text, p.t1);
            ui.painter().galley(
                pos2(
                    rect.right() - g.size().x,
                    rect.center().y - g.size().y / 2.0,
                ),
                g,
                p.t1,
            );
            let reset = ui
                .add_enabled((stored - 100.0).abs() > 0.5, Button::ghost(s::RESET()))
                .clicked();
            if reset {
                pct = 100.0;
            }
            reset || r.drag_stopped() || (r.changed() && !r.dragged())
        })
    });
    if done && (pct - stored).abs() > 0.5 {
        let mut next = (*prefs).clone();
        next.appearance.ui_scale = f64::from(pct) / 100.0;
        commit(
            state,
            env,
            "appearance.ui_scale",
            PreferencePatch::new().set("appearance.ui_scale", next.appearance.ui_scale),
            next,
        );
    }
    seg_row(
        ui,
        state,
        env,
        "appearance.reduce_motion",
        s::REDUCE_MOTION(),
        s::REDUCE_MOTION_HINT(),
        true,
        &[s::SYSTEM(), s::ON(), s::OFF()],
        &["system", "on", "off"],
        idx(ReduceMotion::ALL, prefs.appearance.reduce_motion),
        |p, i| p.appearance.reduce_motion = ReduceMotion::ALL[i],
    );
    rows::heading(ui, s::LIBRARY(), false);
    seg_row(
        ui,
        state,
        env,
        "appearance.library_view",
        s::LIBRARY_VIEW(),
        "",
        true,
        &[s::COVERS(), s::LIST()],
        &["grid", "list"],
        idx(LibraryView::ALL, prefs.appearance.library_view),
        |p, i| p.appearance.library_view = LibraryView::ALL[i],
    );
    seg_row(
        ui,
        state,
        env,
        "appearance.grid_density",
        s::DENSITY(),
        "",
        true,
        &[s::COMPACT(), s::COMFORTABLE(), s::LARGE()],
        &["compact", "comfortable", "large"],
        idx(GridDensity::ALL, prefs.appearance.grid_density),
        |p, i| p.appearance.grid_density = GridDensity::ALL[i],
    );
    switch_row(
        ui,
        state,
        env,
        "appearance.show_playtime_on_cards",
        s::PLAYTIME_ON_CARDS(),
        "",
        true,
        |p| p.appearance.show_playtime_on_cards,
        |p, v| p.appearance.show_playtime_on_cards = v,
    );
}

fn protection(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let prefs = env.bridge.prefs().clone();
    rows::heading(ui, s::NEW_GAMES(), true);
    seg_row(
        ui,
        state,
        env,
        "protection.default_preset",
        s::DEFAULT_PRESET(),
        s::preset_help(prefs.protection.default_preset),
        true,
        &s::PRESETS(),
        &["offline", "online", "standard"],
        idx(&BasePreset::ALL, prefs.protection.default_preset),
        |p, i| p.protection.default_preset = BasePreset::ALL[i],
    );
    seg_row(
        ui,
        state,
        env,
        "protection.controller_access_default",
        s::CONTROLLERS_DEFAULT(),
        "",
        true,
        &[s::CONNECTED_ONLY(), s::ALL_WHEN_ALLOWED()],
        &["detected", "all-when-safe"],
        idx(
            ControllerDefault::ALL,
            prefs.protection.controller_access_default,
        ),
        |p, i| p.protection.controller_access_default = ControllerDefault::ALL[i],
    );
    rows::heading(ui, s::ISOLATION(), false);
    let help = if env.bridge.host().reflinks {
        s::ISOLATION_REFLINK
    } else {
        s::ISOLATION_SHARED
    };
    switch_row(
        ui,
        state,
        env,
        "protection.always_isolate_runtime",
        s::ALWAYS_ISOLATE(),
        help(),
        true,
        |p| p.protection.always_isolate_runtime,
        |p, v| p.protection.always_isolate_runtime = v,
    );
    rows::heading(ui, s::SAFETY(), false);
    switch_row(
        ui,
        state,
        env,
        "protection.prepare_online",
        s::PREPARE_ONLINE(),
        s::PREPARE_ONLINE_HINT(),
        true,
        |p| p.protection.prepare_online,
        |p, v| p.protection.prepare_online = v,
    );
    switch_row(
        ui,
        state,
        env,
        "protection.seccomp_default",
        s::SECCOMP_DEFAULT(),
        "",
        true,
        |p| p.protection.seccomp_default,
        |p, v| p.protection.seccomp_default = v,
    );
    switch_row(
        ui,
        state,
        env,
        "protection.warn_on_x11",
        s::WARN_X11(),
        "",
        true,
        |p| p.protection.warn_on_x11,
        |p, v| p.protection.warn_on_x11 = v,
    );
}

fn layers(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let prefs = env.bridge.prefs().clone();
    let rt = env.bridge.runtimes().clone();
    // The default layer.
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
    let users = rt
        .default_label
        .as_ref()
        .and_then(|d| rt.layers.iter().find(|l| &l.build.display_name == d))
        .map(|l| l.used_by);
    let help = s::default_used(users);
    let changed = SettingRow::new(s::DEFAULT_LAYER())
        .help(&help)
        .show(ui, |ui| {
            band(ui, control::SM, |ui| {
                select(ui, s::DEFAULT_LAYER(), None, None, &mut i, &labels, false).changed()
            })
        });
    if changed {
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
    // What is installed.
    rows::heading(ui, s::INSTALLED(), false);
    let [from_w, size_w, act_w] = layout::LAYER_COLS;
    let [name_share, use_share] = layout::LAYER_SHARES;
    let cols = [
        Col::Flex(name_share),
        Col::Fixed(from_w),
        Col::Fixed(size_w),
        Col::Flex(use_share),
        Col::Fixed(act_w),
    ];
    if rt.layers.is_empty() && rt.slr.is_empty() {
        quiet(ui, s::NONE_YET(), Type::Body);
    } else {
        rows::table_row(ui, &cols, true, |ui, c| {
            if let Some(h) = s::LAYER_COLUMNS().get(c) {
                rows::table_text(ui, h, c, true, false);
            }
        });
    }
    let mut remove = None;
    for l in &rt.layers {
        let size = l.size.map_or_else(String::new, strings::fmt::bytes);
        let used = s::used_by(l.used_by);
        let ours = l.build.origin == RuntimeOrigin::Store && l.used_by == 0;
        rows::table_row(ui, &cols, false, |ui, c| match c {
            0 => {
                rows::table_text(ui, &l.build.display_name, c, false, false);
            }
            1 => {
                rows::table_text(ui, &l.build.origin_label, c, false, false);
            }
            2 => {
                rows::table_text(ui, &size, c, false, true);
            }
            3 => {
                rows::table_text(ui, &used, c, false, false);
            }
            _ => {
                if ours && cell_button(ui, s::REMOVE(), &s::remove_folder(&l.build.display_name)) {
                    remove = Some(l.clone());
                }
            }
        });
    }
    let mut verify = false;
    for (name, _version, verified) in &rt.slr {
        let when = verified.map_or_else(String::new, |t| {
            s::verified(&strings::fmt::relative_inline(t, env.now, env.utc_offset_s))
        });
        rows::table_row(ui, &cols, false, |ui, c| match c {
            0 => {
                rows::table_text(ui, name, c, false, false);
            }
            1 => {
                rows::table_text(ui, s::FROM_STEAM(), c, false, false);
            }
            3 => {
                rows::table_text(ui, &when, c, false, false);
            }
            4 => {
                verify |= cell_button(ui, s::VERIFY(), s::VERIFY());
            }
            _ => {}
        });
    }
    if verify {
        env.bridge.send(UiCommand::VerifyRuntime);
    }
    if let Some(l) = remove {
        state.show_dialog(Dialog::Confirm(Confirm::RemoveLayer {
            name: l.build.name.clone(),
            label: l.build.display_name.clone(),
            size: l.size,
            used_by: l.used_by,
        }));
    }
    if !rt.layers.is_empty() {
        ui.add_space(space::S2);
        let total: u64 = rt.layers.iter().filter_map(|l| l.size).sum();
        quiet(
            ui,
            &s::disk_total(&strings::fmt::bytes(total)),
            Type::Caption,
        );
    }
    for d in &rt.downloads {
        quiet(
            ui,
            &s::downloading(&d.label, &strings::fmt::bytes(d.bytes)),
            Type::Caption,
        );
    }
    // Updates.
    rows::heading(ui, s::UPDATES(), false);
    let checked = rt.last_checked.map_or_else(String::new, |t| {
        s::last_checked(&strings::fmt::relative_inline(t, env.now, env.utc_offset_s))
    });
    if button_row(
        ui,
        s::CHECK_UPDATES(),
        &checked,
        true,
        Button::new(s::CHECK_NOW()),
    ) {
        env.bridge.send(UiCommand::RefreshRuntimes);
    }
    switch_row(
        ui,
        state,
        env,
        "runtime.auto_update",
        s::AUTO_UPDATE(),
        "",
        true,
        |p| p.runtime.auto_update,
        |p, v| p.runtime.auto_update = v,
    );
    number_row(
        ui,
        state,
        env,
        "runtime.auto_update_interval_hours",
        s::INTERVAL(),
        prefs.runtime.auto_update,
        &INTERVALS,
        s::hours,
        i64::from(prefs.runtime.auto_update_interval_hours),
        |p, v| p.runtime.auto_update_interval_hours = v as u32,
    );
    switch_row(
        ui,
        state,
        env,
        "runtime.auto_remove_unused",
        s::AUTO_REMOVE(),
        "",
        true,
        |p| p.runtime.auto_remove_unused,
        |p, v| p.runtime.auto_remove_unused = v,
    );
    note_key(ui, "runtime.extra_search_dirs");
    if button_row(
        ui,
        s::SEARCH_FOLDERS(),
        s::SEARCH_FOLDERS_HINT(),
        true,
        Button::new(s::ADD_SEARCH_FOLDER()),
    ) {
        state.picks.push(PickPurpose::LayerFolder);
    }
    let mut drop_dir = None;
    for (i, d) in prefs.runtime.extra_search_dirs.iter().enumerate() {
        let shown = d.display().to_string();
        let gone = SettingRow::new(&shown).mono().show(ui, |ui| {
            band(ui, control::BUTTON, |ui| {
                ui.add(Button::ghost(s::REMOVE()).accessible_name(s::remove_folder(&shown)))
                    .clicked()
            })
        });
        if gone {
            drop_dir = Some(i);
        }
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
}

fn integration(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let prefs = env.bridge.prefs().clone();
    let host = env.bridge.host().clone();
    let ours = host.handler == HandlerState::EasyPlay;
    rows::heading(ui, s::FILE_TYPES(), true);
    let help = s::handler(&host.handler);
    if button_row(
        ui,
        s::EXE_FILES(),
        &help,
        !ours,
        Button::new(s::USE_EASYPLAY()),
    ) {
        env.bridge
            .send(UiCommand::SetDefaultHandler { enable: true });
    }
    // Kept from before: once EasyPlay is the default, the program it replaced can be
    // given back.
    if ours
        && button_row(
            ui,
            s::PREVIOUS_HANDLER(),
            s::PREVIOUS_HANDLER_HINT(),
            true,
            Button::new(s::RESTORE_PREVIOUS()),
        )
    {
        env.bridge
            .send(UiCommand::SetDefaultHandler { enable: false });
    }
    seg_row(
        ui,
        state,
        env,
        "integration.handle_exe",
        s::HANDLE_EXE(),
        "",
        !ours,
        &[s::ASK(), s::YES(), s::NO()],
        &["ask", "yes", "no"],
        idx(HandleExe::ALL, prefs.integration.handle_exe),
        |p, i| p.integration.handle_exe = HandleExe::ALL[i],
    );
    switch_row(
        ui,
        state,
        env,
        "launch.quick_launch_window",
        s::QUICK_WINDOW(),
        s::QUICK_WINDOW_HINT(),
        true,
        |p| p.launch.quick_launch_window,
        |p, v| p.launch.quick_launch_window = v,
    );
    rows::heading(ui, s::CONTROLS(), false);
    switch_row(
        ui,
        state,
        env,
        "integration.single_click_play",
        s::SINGLE_CLICK(),
        "",
        true,
        |p| p.integration.single_click_play,
        |p, v| p.integration.single_click_play = v,
    );
    switch_row(
        ui,
        state,
        env,
        "integration.gamepad_navigation",
        s::GAMEPAD(),
        "",
        true,
        |p| p.integration.gamepad_navigation,
        |p, v| p.integration.gamepad_navigation = v,
    );
    switch_row(
        ui,
        state,
        env,
        "integration.single_key_shortcuts",
        s::SINGLE_KEYS(),
        s::SINGLE_KEYS_HINT(),
        true,
        |p| p.integration.single_key_shortcuts,
        |p, v| p.integration.single_key_shortcuts = v,
    );
    if button_row(ui, s::ALL_KEYS(), "", true, Button::new(s::SHOW_ALL())) {
        state.nav.open(Overlay::Shortcuts);
    }
    rows::heading(ui, s::MENU_SHORTCUTS(), false);
    if host.shortcuts.is_empty() {
        quiet(ui, s::NO_SHORTCUTS(), Type::Body);
    }
    let cols = [Col::Flex(1.0), Col::Fixed(layout::LAYER_COLS[2])];
    for sc in &host.shortcuts {
        let mut gone = false;
        rows::table_row(ui, &cols, false, |ui, c| {
            if c == 0 {
                rows::table_text(ui, &sc.name, c, false, false);
            } else {
                gone = cell_button(ui, s::REMOVE(), &s::remove_folder(&sc.name));
            }
        });
        if gone {
            env.bridge.send(UiCommand::SetShortcut {
                id: sc.game,
                enable: false,
            });
        }
    }
}

fn advanced(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let prefs = env.bridge.prefs().clone();
    super::debug::doctor::section(ui, state, env);
    rows::heading(ui, s::DEVELOPER(), false);
    note_key(ui, "advanced.developer_mode");
    let mut dev = prefs.advanced.developer_mode;
    let flipped = SettingRow::new(s::DEVELOPER_MODE())
        .help(s::DEVELOPER_HINT())
        .show(ui, |ui| {
            band(ui, control::TOGGLE_H, |ui| {
                switch(ui, &mut dev, s::DEVELOPER_MODE()).changed()
            })
        });
    if flipped {
        if dev {
            state.show_dialog(Dialog::Confirm(Confirm::DeveloperMode));
        } else {
            set_bool(state, env, "advanced.developer_mode", false, |p, v| {
                p.advanced.developer_mode = v
            });
        }
    }
    note_key(ui, "logging.level");
    let current = idx(LogLevel::ALL, prefs.logging.level);
    let mut i = current;
    let changed = SettingRow::new(s::LOG_LEVEL()).show(ui, |ui| {
        band(ui, control::SM, |ui| {
            select(
                ui,
                s::LOG_LEVEL(),
                None,
                None,
                &mut i,
                &s::LOG_LEVELS(),
                false,
            )
            .changed()
        })
    });
    if changed && i != current {
        let mut next = (*prefs).clone();
        next.logging.level = LogLevel::ALL[i];
        commit(
            state,
            env,
            "logging.level",
            PreferencePatch::new().set(
                "logging.level",
                ["error", "warn", "info", "debug", "trace"][i],
            ),
            next,
        );
    }
    number_row(
        ui,
        state,
        env,
        "logging.session_log_cap_mb",
        s::LOG_CAP(),
        true,
        &LOG_CAPS,
        s::megabytes,
        i64::from(prefs.logging.session_log_cap_mb),
        |p, v| p.logging.session_log_cap_mb = v as u32,
    );
    number_row(
        ui,
        state,
        env,
        "logging.keep_session_logs",
        s::KEEP_LOGS(),
        true,
        &KEEP_LOGS,
        s::count,
        i64::from(prefs.logging.keep_session_logs),
        |p, v| p.logging.keep_session_logs = v as u32,
    );
    number_row(
        ui,
        state,
        env,
        "logging.keep_session_logs_days",
        s::KEEP_LOG_DAYS(),
        true,
        &LOG_DAYS,
        s::days,
        i64::from(prefs.logging.keep_session_logs_days),
        |p, v| p.logging.keep_session_logs_days = v as u32,
    );
    switch_row(
        ui,
        state,
        env,
        "launch.verbose_launch_log",
        s::VERBOSE(),
        "",
        true,
        |p| p.launch.verbose_launch_log,
        |p, v| p.launch.verbose_launch_log = v,
    );
    number_row(
        ui,
        state,
        env,
        "launch.stop_grace_secs",
        s::STOP_GRACE(),
        true,
        &STOP_WAITS,
        s::seconds,
        i64::from(prefs.launch.stop_grace_secs),
        |p, v| p.launch.stop_grace_secs = v as u8,
    );
    seg_row(
        ui,
        state,
        env,
        "advanced.renderer",
        s::RENDERER(),
        s::RENDERER_HINT(),
        true,
        &[s::AUTOMATIC(), "wgpu", "OpenGL"],
        &["auto", "wgpu", "glow"],
        idx(RendererPref::ALL, prefs.advanced.renderer),
        |p, i| p.advanced.renderer = RendererPref::ALL[i],
    );
    // Environment variables for every game: the ones set, then a line to add one.
    rows::heading(ui, s::GLOBAL_ENV(), false);
    note_key(ui, "launch.global_env");
    let mut drop = None;
    for (i, v) in prefs.launch.global_env.iter().enumerate() {
        let pair = s::env_pair(&v.name, &v.value);
        let gone = SettingRow::new(&pair)
            .mono()
            .id_salt(("env", i))
            .show(ui, |ui| {
                band(ui, control::BUTTON, |ui| {
                    ui.add(Button::ghost(s::REMOVE()).accessible_name(s::remove_var(&v.name)))
                        .clicked()
                })
            });
        if gone {
            drop = Some(i);
        }
    }
    let n = prefs.launch.global_env.len() + 1;
    let name_label = strings::details::var_name(n);
    let value_label = strings::details::var_value(n);
    let add = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let add = Button::new(s::ADD());
            let add_w = add.width(ui);
            let eq = Type::Body.line(ui, s::ENV_EQUALS, p.t3);
            let [a, b] = layout::ENV_SHARES;
            let free = (ui.available_width() - add_w - eq.size().x - space::S2 * 3.0).max(2.0);
            let (name, value) = &mut state.settings.new_env;
            TextField::new(name, "settings.env.name")
                .accessible_name(&name_label)
                .hint(s::ENV_NAME())
                .mono()
                .width(free * a / (a + b))
                .show(ui);
            let (r, _) = ui.allocate_exact_size(eq.size(), Sense::hover());
            ui.painter().galley(r.min, eq, p.t3);
            TextField::new(value, "settings.env.value")
                .accessible_name(&value_label)
                .hint(s::ENV_VALUE())
                .mono()
                .width(free * b / (a + b))
                .show(ui);
            ui.add(add).clicked()
        })
        .inner;
    let candidate = EnvVar {
        name: state.settings.new_env.0.trim().to_owned(),
        value: state.settings.new_env.1.clone(),
    };
    let problem =
        super::details::launch_options::env_problem(&candidate, prefs.advanced.developer_mode);
    if let Some(pr) = &problem {
        ui.add_space(space::S1);
        let w = ui.available_width();
        let g = Type::Caption.galley(ui, pr, p.coral, w, usize::MAX);
        let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
        ui.painter().galley(r.min, g, p.coral);
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
    // Where umu-run and bwrap are.
    rows::heading(ui, s::PATHS(), false);
    for (key, label) in [("paths.umu_run", s::UMU_RUN), ("paths.bwrap", s::BWRAP)] {
        note_key(ui, key);
        let current = current_text(&prefs, key);
        let name = s::path_to(label);
        let buf = state
            .settings
            .paths
            .entry(key.to_owned())
            .or_insert_with(|| current.clone());
        let r = SettingRow::new(label).mono().show(ui, |ui| {
            band(ui, control::BUTTON, |ui| {
                TextField::new(buf, key)
                    .accessible_name(&name)
                    .hint(s::AUTOMATIC())
                    .mono()
                    .width(layout::PATH_FIELD_W)
                    .show(ui)
                    .response
            })
        });
        let typed = buf.trim().to_owned();
        if r.lost_focus() && typed != current {
            let mut next = (*prefs).clone();
            let v = (!typed.is_empty()).then(|| std::path::PathBuf::from(&typed));
            if key == "paths.umu_run" {
                next.paths.umu_run = v;
            } else {
                next.paths.bwrap = v;
            }
            commit(
                state,
                env,
                key,
                PreferencePatch::new().set(key, typed),
                next,
            );
            state.settings.restart = true;
        } else if !r.has_focus() {
            // At rest the field shows the saved value.
            state.settings.paths.insert(key.to_owned(), current);
        }
    }
    rows::heading(ui, s::MAINTENANCE(), false);
    if button_row(
        ui,
        s::DATA_FOLDER(),
        s::DATA_FOLDER_HINT(),
        true,
        Button::new(s::OPEN()),
    ) {
        env.bridge
            .send(UiCommand::OpenFolder(FolderTarget::DataFolder));
    }
    note_key(ui, "onboarding.completed");
    if button_row(
        ui,
        s::WELCOME_STEPS(),
        "",
        true,
        Button::new(s::SHOW_AGAIN()),
    ) {
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
    let w = ui.available_width();
    let g = super::statistics::balanced(ui, s::PRIVACY(), Type::Hero, false, p.t1, w);
    let (r, resp) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
    ui.painter().galley(r.min, g, p.t1);
    crate::a11y::labelled(ui.ctx(), resp.id, egui::Role::Heading, s::PRIVACY());
    ui.add_space(layout::ABOUT_GAP);
    let g = Type::Lead.galley(ui, s::PRIVACY_ENDPOINTS(), p.t2, w, usize::MAX);
    let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
    ui.painter().galley(r.min, g, p.t2);
    ui.add_space(layout::ABOUT_GAP);
    quiet(ui, &s::version(&env.bridge.host().version), Type::Body);
    rows::heading(ui, s::LICENCES(), false);
    let cols = [Col::Flex(1.0), Col::Flex(1.0)];
    for (what, licence) in s::LICENCE_ROWS() {
        rows::table_row(ui, &cols, false, |ui, c| {
            if c == 0 {
                rows::table_text(ui, what, c, false, false);
            } else {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    rows::table_text(ui, licence, c, false, false);
                });
            }
        });
    }
}

/// Show the screen: the tab bar, then the page of the tab.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, tab: SettingsTab) {
    let gutter = super::shell::gutter(ui.ctx());
    state.settings.tab = tab;
    ui.spacing_mut().item_spacing.y = 0.0;
    // The tab bar, in the gutter, 4 under the top bar.
    let names: Vec<&str> = TABS.iter().map(|t| s::tab(*t)).collect();
    let mut i = TABS.iter().position(|t| *t == tab).unwrap_or(0);
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: gutter as i8,
            right: gutter as i8,
            top: layout::SETTINGS_TABS_TOP as i8,
            bottom: 0,
        })
        .show(ui, |ui| {
            // The words stand on the column, which stands in the middle of a wider page;
            // the rule under them spans the page.
            let indent = super::centre_indent(ui.available_width(), layout::COL_MAX);
            Tabs::new(s::TABS(), &names).indent(indent).show(ui, &mut i);
        });
    if TABS[i] != tab {
        state.nav.replace(Route::Settings(TABS[i]));
    }
    ScrollArea::vertical()
        .id_salt(("settings", tab))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: gutter as i8,
                    right: gutter as i8,
                    top: layout::SETTINGS_TOP as i8,
                    bottom: layout::PAGE_PAD[1] as i8,
                })
                .show(ui, |ui| {
                    let w = ui.available_width().min(layout::COL_MAX);
                    let indent = super::centre_indent(ui.available_width(), layout::COL_MAX);
                    let at = ui.cursor().min + vec2(indent, 0.0);
                    let mut column = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(egui::Rect::from_min_size(at, vec2(w, f32::INFINITY)))
                            .layout(Layout::top_down(Align::Min)),
                    );
                    let filled = column.allocate_ui_with_layout(
                        vec2(w, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(w);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            // A new tab's column comes in with a short rise.
                            let m = theme::theme(ui.ctx()).motion;
                            let (alpha, below) = motion::arrive(
                                ui.ctx(),
                                Id::unique(("settings.tab", tab)),
                                m,
                                0.0,
                                Dur::Base,
                                travel::RISE,
                            );
                            ui.multiply_opacity(alpha);
                            ui.add_space(below);
                            match tab {
                                SettingsTab::General => general(ui, state, env),
                                SettingsTab::Appearance => appearance(ui, state, env),
                                SettingsTab::Protection => protection(ui, state, env),
                                SettingsTab::CompatibilityLayers => layers(ui, state, env),
                                SettingsTab::Integration => integration(ui, state, env),
                                SettingsTab::Advanced => advanced(ui, state, env),
                                SettingsTab::About => about(ui, env),
                            }
                        },
                    );
                    let h = filled.response.rect.height();
                    ui.allocate_space(vec2(ui.available_width(), h));
                });
        });
}
