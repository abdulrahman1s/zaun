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
use crate::bridge::{Command, FolderRequest, HandlerState, UiCommand};
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
pub const TABS: [SettingsTab; 8] = [
    SettingsTab::General,
    SettingsTab::Appearance,
    SettingsTab::Protection,
    SettingsTab::Chat,
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

/// Chat (ADR-0033): off by default. The switch goes through the service (turning it off and
/// on again is also how a stopped engine is retried); the name and relays are saved when
/// their field loses focus.
fn chat(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    use easyplay_core::service::ChatCommand;
    use strings::chat as c;
    let prefs = env.bridge.prefs().clone();
    rows::heading(ui, c::GROUP(), true);
    note_key(ui, "chat.enabled");
    let mut on = prefs.chat.enabled;
    let changed = SettingRow::new(c::ENABLE())
        .help(c::ENABLE_HELP())
        .show(ui, |ui| {
            band(ui, control::TOGGLE_H, |ui| {
                switch(ui, &mut on, c::ENABLE()).changed()
            })
        });
    if changed {
        let mut next = (*prefs).clone();
        next.chat.enabled = on;
        env.bridge
            .send(Command::Chat(ChatCommand::SetEnabled { on, name: None }));
        env.bridge.set_prefs_preview(next);
        state
            .settings
            .saved
            .insert("chat.enabled".to_owned(), env.mono);
    }
    for (key, label, help) in [
        ("chat.name", c::NAME(), c::NAME_HELP()),
        ("chat.relays", c::RELAYS(), c::RELAYS_HELP()),
    ] {
        note_key(ui, key);
        let current = if key == "chat.name" {
            prefs.chat.name.clone()
        } else {
            prefs.chat.relays.join(" ")
        };
        let buf = state
            .settings
            .paths
            .entry(key.to_owned())
            .or_insert_with(|| current.clone());
        let mut field = TextField::new(buf, key)
            .accessible_name(label)
            .width(layout::PATH_FIELD_W);
        if key == "chat.relays" {
            field = field.hint(c::RELAYS_PRESET()).mono();
        }
        let r = SettingRow::new(label).help(help).show(ui, |ui| {
            band(ui, control::BUTTON, |ui| field.show(ui).response)
        });
        let typed = buf.trim().to_owned();
        if r.lost_focus() && typed != current {
            let mut next = (*prefs).clone();
            if key == "chat.name" {
                if typed.is_empty() {
                    continue;
                }
                next.chat.name = typed.clone();
                env.bridge.send(Command::Chat(ChatCommand::SetName(typed)));
                env.bridge.set_prefs_preview(next);
                state.settings.saved.insert(key.to_owned(), env.mono);
            } else {
                let list: Vec<String> = typed.split_whitespace().map(str::to_owned).collect();
                next.chat.relays = list.clone();
                commit(state, env, key, PreferencePatch::new().set(key, list), next);
            }
        } else if !r.has_focus() {
            state.settings.paths.insert(key.to_owned(), current);
        }
    }
    switch_row(
        ui,
        state,
        env,
        "chat.notify_preview",
        c::NOTIFY_PREVIEW(),
        c::NOTIFY_PREVIEW_HELP(),
        prefs.chat.enabled && prefs.chat.notify,
        |p| p.chat.notify_preview,
        |p, v| p.chat.notify_preview = v,
    );
}

/// Settings › Chat (chat.html SETTINGS `chat`, image 40): Chat itself, what friends see,
/// messages, calls, sharing a game, notifications. The rows of what this build's chat
/// engine cannot do are not drawn (`ChatSnapshot::features`, published whether Chat is on
/// or off).
fn chat_tab(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    use easyplay_core::config::schema::{ChatEmoji, ChatSendWith};
    use strings::chat::settings as cs;
    chat(ui, state, env);
    let prefs = env.bridge.prefs().clone();
    let f = env.bridge.chat().features;

    if f.presence {
        rows::heading(ui, cs::FRIENDS_SEE(), false);
        switch_row(
            ui,
            state,
            env,
            "chat.show_playing",
            cs::SHOW_PLAYING(),
            cs::SHOW_PLAYING_HELP(),
            true,
            |p| p.chat.show_playing,
            |p, v| p.chat.show_playing = v,
        );
    }

    rows::heading(ui, cs::MESSAGES(), false);
    seg_row(
        ui,
        state,
        env,
        "chat.emoji",
        cs::EMOJI(),
        cs::EMOJI_HELP(),
        true,
        &[cs::EMOJI_TWEMOJI(), cs::EMOJI_SYSTEM()],
        &["twemoji", "system"],
        idx(ChatEmoji::ALL, prefs.chat.emoji),
        |p, i| p.chat.emoji = ChatEmoji::ALL[i],
    );
    seg_row(
        ui,
        state,
        env,
        "chat.send_with",
        cs::SEND_WITH(),
        "",
        true,
        &[cs::SEND_ENTER(), cs::SEND_CTRL_ENTER()],
        &["enter", "ctrl_enter"],
        idx(ChatSendWith::ALL, prefs.chat.send_with),
        |p, i| p.chat.send_with = ChatSendWith::ALL[i],
    );
    if f.gifs {
        switch_row(
            ui,
            state,
            env,
            "chat.gif_autoplay",
            cs::GIF_AUTOPLAY(),
            "",
            true,
            |p| p.chat.gif_autoplay,
            |p, v| p.chat.gif_autoplay = v,
        );
        switch_row(
            ui,
            state,
            env,
            "chat.gif_search",
            cs::GIF_SEARCH(),
            cs::GIF_SEARCH_HELP(),
            true,
            |p| p.chat.gif_search,
            |p, v| p.chat.gif_search = v,
        );
        {
            use strings::chat::settings as cs;
            gif_text_row(
                ui,
                state,
                env,
                prefs.chat.gif_search,
                "chat.gif_key",
                (cs::GIF_KEY(), cs::GIF_KEY_HELP()),
                |p| &mut p.chat.gif_key,
            );
            gif_text_row(
                ui,
                state,
                env,
                prefs.chat.gif_search,
                "chat.gif_proxy",
                (cs::GIF_PROXY(), cs::GIF_PROXY_HELP()),
                |p| &mut p.chat.gif_proxy,
            );
        }
    }
    if f.link_previews {
        switch_row(
            ui,
            state,
            env,
            "chat.link_previews",
            cs::LINK_PREVIEWS(),
            cs::LINK_PREVIEWS_HELP(),
            true,
            |p| p.chat.link_previews,
            |p, v| p.chat.link_previews = v,
        );
    }
    switch_row(
        ui,
        state,
        env,
        "chat.check_links",
        cs::CHECK_LINKS(),
        cs::CHECK_LINKS_HELP(),
        true,
        |p| p.chat.check_links,
        |p, v| p.chat.check_links = v,
    );

    if f.calls {
        voice_and_video(ui, state, env);
    }

    if f.share {
        sharing_group(ui, state, env);
    }

    overlay_group(ui, state, env);

    rows::heading(ui, cs::NOTIFICATIONS(), false);
    switch_row(
        ui,
        state,
        env,
        "chat.notify",
        cs::NOTIFY_MESSAGES(),
        "",
        true,
        |p| p.chat.notify,
        |p, v| p.chat.notify = v,
    );
    if f.calls {
        switch_row(
            ui,
            state,
            env,
            "chat.notify_calls",
            cs::NOTIFY_CALLS(),
            "",
            true,
            |p| p.chat.notify_calls,
            |p, v| p.chat.notify_calls = v,
        );
    }
}

/// "GIF search key" and "GIF search relay" (`chat.gif_key`, `chat.gif_proxy`): text saved
/// when the field loses focus (like the relays); faint while GIF search is off.
fn gif_text_row(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    enabled: bool,
    key: &'static str,
    (label, help): (&str, &str),
    field: fn(&mut Preferences) -> &mut String,
) {
    use strings::chat::settings as cs;
    note_key(ui, key);
    let current = field(&mut (**env.bridge.prefs()).clone()).clone();
    let buf = state
        .settings
        .paths
        .entry(key.to_owned())
        .or_insert_with(|| current.clone());
    let text = TextField::new(buf, key)
        .accessible_name(label)
        .hint(cs::GIF_NOT_SET())
        .mono()
        .width(layout::PATH_FIELD_W);
    let r = SettingRow::new(label)
        .help(help)
        .enabled(enabled)
        .show(ui, |ui| {
            band(ui, control::BUTTON, |ui| text.show(ui).response)
        });
    let typed = buf.trim().to_owned();
    if r.lost_focus() && typed != current {
        let mut next = (**env.bridge.prefs()).clone();
        field(&mut next).clone_from(&typed);
        commit(
            state,
            env,
            key,
            PreferencePatch::new().set(key, typed),
            next,
        );
    } else if !r.has_focus() {
        state.settings.paths.insert(key.to_owned(), current);
    }
}

/// The choices of a device row: stored names and what the select shows, kept until the
/// snapshot's devices, the stored device or the language change.
///
/// Eviction rule: one entry per row key (two rows), replaced when its stamp changes.
#[derive(Clone, Default)]
struct DeviceChoices {
    /// The device list it was made from (its address), the stored name, the language.
    stamp: (usize, String, u64),
    names: Vec<String>,
    labels: Vec<String>,
}

/// A row with a select over sound devices: the system default (stored as ""), the devices
/// the sound server listed (their descriptions shown, their names stored), and the stored
/// one when it is not among them ("USB headset (not connected)", kept until changed).
fn device_row(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    key: &'static str,
    label: &str,
    known: &[easyplay_core::model::AudioDevice],
    field: fn(&mut Preferences) -> &mut String,
) {
    note_key(ui, key);
    let current = field(&mut (**env.bridge.prefs()).clone()).clone();
    let id = Id::unique(("settings.devices", key));
    let stamp = (known.as_ptr() as usize, current, crate::i18n::generation());
    let mut choices: DeviceChoices = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
    if choices.stamp != stamp || choices.names.is_empty() {
        let mut names = vec![String::new()];
        let mut labels = vec![strings::chat::settings::SYSTEM_DEFAULT().to_owned()];
        for d in known {
            names.push(d.name.clone());
            labels.push(d.description.clone());
        }
        if !names.contains(&stamp.1) {
            names.push(stamp.1.clone());
            labels.push(strings::chat::settings::not_connected(&stamp.1));
        }
        choices = DeviceChoices {
            stamp,
            names,
            labels,
        };
        ui.ctx().data_mut(|d| d.insert_temp(id, choices.clone()));
    }
    let current = &choices.stamp.1;
    let labels: Vec<&str> = choices.labels.iter().map(String::as_str).collect();
    let mut i = choices.names.iter().position(|v| v == current).unwrap_or(0);
    let changed = SettingRow::new(label).show(ui, |ui| {
        band(ui, control::SM, |ui| {
            select(ui, label, None, None, &mut i, &labels, false).changed()
        })
    });
    let picked = &choices.names[i.min(choices.names.len() - 1)];
    if changed && picked != current {
        let mut next = (**env.bridge.prefs()).clone();
        field(&mut next).clone_from(picked);
        commit(
            state,
            env,
            key,
            PreferencePatch::new().set(key, picked.clone()),
            next,
        );
    }
}

/// Settings › Chat › Voice and video (chat-calls.html notes H): microphone, speakers, the
/// input level with "Let's check", microphone volume, noise, the relay switch and, under
/// Advanced, the relay servers. Opening it asks the sound server for its devices again.
fn voice_and_video(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    use easyplay_core::service::ChatCommand;
    use strings::chat::settings as cs;
    let pass = ui.ctx().cumulative_pass_nr();
    let last: Option<u64> = ui.ctx().data(|d| d.get_temp(voice_id()));
    if last.is_none_or(|n| n + 2 < pass) {
        env.bridge
            .send(Command::Chat(ChatCommand::RefreshAudioDevices));
    }
    ui.ctx().data_mut(|d| d.insert_temp(voice_id(), pass));
    rows::heading(ui, cs::CALLS(), false);
    let before = mic_test_inputs(env.bridge.prefs());
    let chat = std::sync::Arc::clone(env.bridge.chat());
    let (mics, outs) = chat
        .audio_devices
        .as_ref()
        .map_or((&[][..], &[][..]), |d| (&d.inputs[..], &d.outputs[..]));
    device_row(ui, state, env, "chat.mic", cs::MIC(), mics, |p| {
        &mut p.chat.mic
    });
    device_row(ui, state, env, "chat.speakers", cs::SPEAKERS(), outs, |p| {
        &mut p.chat.speakers
    });
    input_level_row(ui, env);
    mic_volume_row(ui, state, env);
    switch_row(
        ui,
        state,
        env,
        "chat.noise_suppression",
        cs::NOISE(),
        cs::NOISE_HELP(),
        true,
        |p| p.chat.noise_suppression,
        |p, v| p.chat.noise_suppression = v,
    );
    // The test reads these when it starts: a change restarts a running one.
    if chat.live.mic_test.is_some() && mic_test_inputs(env.bridge.prefs()) != before {
        env.bridge.send(Command::Chat(ChatCommand::MicTest(false)));
        env.bridge.send(Command::Chat(ChatCommand::MicTest(true)));
    }
    switch_row(
        ui,
        state,
        env,
        "chat.relay_calls",
        cs::RELAY_CALLS(),
        cs::RELAY_CALLS_HELP(),
        true,
        |p| p.chat.relay_calls,
        |p, v| p.chat.relay_calls = v,
    );
    relay_servers(ui, state, env);
}

fn voice_id() -> Id {
    Id::unique("settings.chat.voice_drawn")
}

/// What the microphone test is started with.
fn mic_test_inputs(p: &Preferences) -> (String, String, u8, bool) {
    (
        p.chat.mic.clone(),
        p.chat.speakers.clone(),
        p.chat.mic_volume,
        p.chat.noise_suppression,
    )
}

/// The longest a microphone test runs before it stops by itself, in seconds.
const MIC_TEST_FOR: f64 = 30.0;

/// The meter: 160 × 6, an `o2` track and the level in `t2`.
const METER: [f32; 2] = [160.0, 6.0];

/// "Input level": the meter and "Let's check" / "Stop". The microphone opens only when the
/// check is asked for; the meter moves while it runs and rests empty otherwise.
fn input_level_row(ui: &mut Ui, env: &mut Env<'_>) {
    use easyplay_core::service::ChatCommand;
    use strings::chat::settings as cs;
    let live = &env.bridge.chat().live;
    let level = live.mic_test;
    let in_call = live.call.is_some();
    let running = level.is_some();
    let p = theme::theme(ui.ctx()).palette.clone();
    let help = if running {
        cs::CHECKING()
    } else {
        cs::LEVEL_HELP()
    };
    let pressed = SettingRow::new(cs::LEVEL()).help(help).show(ui, |ui| {
        band(ui, control::BUTTON, |ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(METER[0], METER[1]), Sense::hover());
            // Between the service's updates (about 20 a second) the bar glides to the level.
            let target = f32::from(level.unwrap_or(0)) / 100.0;
            let shown = ui
                .ctx()
                .animate_value_with_time(resp.id.with("level"), target, 0.05);
            let corner = egui::CornerRadius::same(3);
            ui.painter().rect_filled(r, corner, p.o2);
            if shown > 0.0 {
                let fill = egui::Rect::from_min_size(r.min, vec2(r.width() * shown, r.height()));
                ui.painter().rect_filled(fill, corner, p.t2);
            }
            crate::a11y::item(&resp, egui::Role::Meter, cs::LEVEL());
            ui.ctx().accesskit_node_builder(resp.id, |n| {
                n.set_min_numeric_value(0.0);
                n.set_max_numeric_value(100.0);
                n.set_numeric_value(f64::from(level.unwrap_or(0)));
            });
            ui.add_space(space::S2 - ui.spacing().item_spacing.x);
            let label = if running {
                cs::STOP_CHECK()
            } else {
                cs::CHECK()
            };
            ui.add_enabled(running || !in_call, Button::new(label))
                .clicked()
        })
    });
    if pressed {
        env.bridge
            .send(Command::Chat(ChatCommand::MicTest(!running)));
        let now = ui.input(|i| i.time);
        ui.ctx()
            .data_mut(|d| d.insert_temp(mic_test_id(), (!running).then_some(now)));
    }
}

fn mic_test_id() -> Id {
    Id::unique("settings.chat.mic_test_started")
}

/// Every frame, wherever the user is: a running microphone test stops when Settings ›
/// Chat is no longer drawn (another tab or page) and after 30 s.
pub fn watch_mic_test(ctx: &egui::Context, env: &mut Env<'_>) {
    use easyplay_core::service::ChatCommand;
    let running = env.bridge.chat().live.mic_test.is_some();
    let started: Option<f64> = ctx.data(|d| d.get_temp(mic_test_id())).flatten();
    if !running && started.is_none() {
        return;
    }
    let now = ctx.input(|i| i.time);
    let pass = ctx.cumulative_pass_nr();
    let drawn = ctx
        .data(|d| d.get_temp::<u64>(voice_id()))
        .is_some_and(|n| n + 2 >= pass);
    let too_long = started.is_some_and(|t| now - t >= MIC_TEST_FOR);
    if let Some(t) = started {
        ctx.request_repaint_after_secs((MIC_TEST_FOR - (now - t)).max(0.1) as f32);
    }
    if drawn && !too_long {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(mic_test_id(), None::<f64>));
    // Once a second at most: the snapshot says it stopped a moment later.
    let sent: Option<f64> = ctx.data(|d| d.get_temp(mic_test_id().with("stop")));
    if running && sent.is_none_or(|t| now - t >= 1.0) {
        env.bridge.send(Command::Chat(ChatCommand::MicTest(false)));
        ctx.data_mut(|d| d.insert_temp(mic_test_id().with("stop"), now));
    }
}

/// Settings › Chat › Sharing a game (chat-calls.html notes J): where sharing starts
/// (picture and frame rate, both selects), the game's sound, pausing, and the encoder.
fn sharing_group(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    use easyplay_core::config::schema::{ShareFps, ShareQuality};
    use strings::chat::settings as cs;
    let prefs = env.bridge.prefs().clone();
    rows::heading(ui, cs::SHARING(), false);
    const PICTURES: [ShareQuality; 4] = [
        ShareQuality::P720,
        ShareQuality::P1080,
        ShareQuality::P1440,
        ShareQuality::Source,
    ];
    let labels = [
        cs::PICTURE_720(),
        cs::PICTURE_1080(),
        cs::PICTURE_1440(),
        cs::PICTURE_SOURCE(),
    ];
    if let Some(q) = select_row(
        ui,
        "chat.share_quality",
        (cs::PICTURE(), cs::PICTURE_HELP()),
        &labels,
        idx(&PICTURES, prefs.chat.share_quality),
    ) {
        let q = PICTURES[q];
        let mut next = (*prefs).clone();
        next.chat.share_quality = q;
        let patch = PreferencePatch::new().set("chat.share_quality", q.as_str());
        commit(state, env, "chat.share_quality", patch, next);
    }
    const RATES: [ShareFps; 2] = [ShareFps::F30, ShareFps::F60];
    if let Some(i) = select_row(
        ui,
        "chat.share_fps",
        (cs::FRAME_RATE(), ""),
        &[cs::FPS_30(), cs::FPS_60()],
        idx(&RATES, prefs.chat.share_fps),
    ) {
        let fps = RATES[i.min(1)];
        let mut next = (*prefs).clone();
        next.chat.share_fps = fps;
        let patch = PreferencePatch::new().set("chat.share_fps", fps.as_str());
        commit(state, env, "chat.share_fps", patch, next);
    }
    switch_row(
        ui,
        state,
        env,
        "chat.share_sound",
        cs::SHARE_SOUND(),
        cs::SHARE_SOUND_LATER(),
        // Streams are video only (ADR-0036): a live switch here would do nothing.
        false,
        |p| p.chat.share_sound,
        |p, v| p.chat.share_sound = v,
    );
    switch_row(
        ui,
        state,
        env,
        "chat.share_pause",
        cs::SHARE_PAUSE(),
        cs::SHARE_PAUSE_AUTO(),
        // The media pauses on its own; it does not read this switch yet (ADR-0036).
        false,
        |p| p.chat.share_pause,
        |p, v| p.chat.share_pause = v,
    );
    // On: the graphics card when there is one (NVENC, VA-API), else software.
    switch_row(
        ui,
        state,
        env,
        "chat.share_hw_encode",
        cs::HW_ENCODE(),
        cs::HW_ENCODE_HELP(),
        true,
        |p| p.chat.share_hw_encode,
        |p, v| p.chat.share_hw_encode = v,
    );
}

/// A row with a select over `options` (`current` chosen); the newly chosen index.
fn select_row(
    ui: &mut Ui,
    key: &'static str,
    (label, help): (&str, &str),
    options: &[&str],
    current: usize,
) -> Option<usize> {
    note_key(ui, key);
    let mut i = current;
    let changed = SettingRow::new(label).help(help).show(ui, |ui| {
        band(ui, control::SM, |ui| {
            select(ui, label, None, None, &mut i, options, false).changed()
        })
    });
    (changed && i != current).then_some(i)
}

/// "Microphone volume": a 150 slider and its value, stored when the drag ends
/// (`chat.mic_volume`).
fn mic_volume_row(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    use strings::chat::settings as cs;
    const KEY: &str = "chat.mic_volume";
    note_key(ui, KEY);
    let p = theme::theme(ui.ctx()).palette.clone();
    let stored = f32::from(env.bridge.prefs().chat.mic_volume);
    let mut v = stored;
    let done = SettingRow::new(cs::MIC_VOLUME()).show(ui, |ui| {
        band(ui, control::BUTTON, |ui| {
            let shown = s::percent(f64::from(v) / 100.0);
            let r = slider(
                ui,
                &mut v,
                0.0..=100.0,
                1.0,
                control::SLIDER_W,
                cs::MIC_VOLUME(),
                &shown,
            );
            let text = s::percent(f64::from(v) / 100.0);
            let (rect, _) = ui.allocate_exact_size(
                vec2(layout::SLIDER_VALUE_W, Type::Label.line_height()),
                Sense::hover(),
            );
            let g = Type::Caption.numeric_galley(ui, &text, p.t2);
            ui.painter().galley(
                pos2(
                    rect.right() - g.size().x,
                    rect.center().y - g.size().y / 2.0,
                ),
                g,
                p.t2,
            );
            r.drag_stopped() || (r.changed() && !r.dragged())
        })
    });
    if done && (v - stored).abs() > 0.5 {
        let mut next = (**env.bridge.prefs()).clone();
        next.chat.mic_volume = v.round().clamp(0.0, 100.0) as u8;
        let value = i64::from(next.chat.mic_volume);
        commit(
            state,
            env,
            KEY,
            PreferencePatch::new().set(KEY, value),
            next,
        );
    }
}

/// "Advanced": a 30 high text button whose 13 caret turns 90° (220 out, 220 back), and
/// under it the relay servers (`chat.ice_servers`), saved when the field loses focus.
fn relay_servers(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    use strings::chat::settings as cs;
    const KEY: &str = "chat.ice_servers";
    // The field is one click away behind the disclosure: it counts as this key's control.
    note_key(ui, KEY);
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let id = Id::unique("settings.chat.advanced");
    let open: bool = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(false);
    let t =
        ui.ctx()
            .animate_bool_with_time(id.with("turn"), open, theme.motion.movement(Dur::Base));
    let label = Type::Body.galley(ui, cs::ADVANCED(), p.t2, f32::INFINITY, 1);
    let caret = 13.0;
    let w = caret + space::S2 + label.size().x;
    let (r, resp) = ui.allocate_exact_size(vec2(w, control::BUTTON), Sense::click());
    // Settings is laid out left to right in every language (as its other rows are).
    let c = pos2(r.left() + caret / 2.0, r.center().y);
    let g = ui.painter().layout_no_wrap(
        theme::icons::CARET_RIGHT.text(false).to_string(),
        theme::icons::font(caret),
        p.t2,
    );
    let angle = t * std::f32::consts::FRAC_PI_2;
    let half = egui::emath::Rot2::from_angle(angle) * (g.size() / 2.0);
    ui.painter()
        .add(egui::epaint::TextShape::new(c - half, g, p.t2).with_angle(angle));
    let lx = r.left() + caret + space::S2;
    ui.painter()
        .galley(pos2(lx, r.center().y - label.size().y / 2.0), label, p.t2);
    crate::a11y::button(&resp, cs::ADVANCED());
    ui.ctx()
        .accesskit_node_builder(resp.id, |n| n.set_expanded(open));
    if resp.has_focus() && crate::widgets::focus_visible(ui.ctx()) {
        crate::widgets::focus_ring(
            ui.painter(),
            r.expand(2.0),
            egui::CornerRadius::same(theme::tokens::radius::SM),
            &theme,
        );
    }
    if resp.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(id, !open));
    }
    if !open {
        return;
    }
    let current = env.bridge.prefs().chat.ice_servers.join(" ");
    let buf = state
        .settings
        .paths
        .entry(KEY.to_owned())
        .or_insert_with(|| current.clone());
    let field = TextField::new(buf, KEY)
        .accessible_name(cs::RELAY_SERVERS())
        .hint(cs::RELAY_SERVERS_EMPTY())
        .mono()
        .width(layout::PATH_FIELD_W);
    let r = SettingRow::new(cs::RELAY_SERVERS())
        .help(cs::RELAY_SERVERS_HELP())
        .show(ui, |ui| {
            band(ui, control::BUTTON, |ui| field.show(ui).response)
        });
    let typed = buf.split_whitespace().collect::<Vec<_>>().join(" ");
    if r.lost_focus() && typed != current {
        let list: Vec<String> = typed.split_whitespace().map(str::to_owned).collect();
        let mut next = (**env.bridge.prefs()).clone();
        next.chat.ice_servers = list.clone();
        commit(state, env, KEY, PreferencePatch::new().set(KEY, list), next);
    } else if !r.has_focus() {
        state.settings.paths.insert(KEY.to_owned(), current);
    }
}

/// Settings › Chat › Overlay (notes H): the switch, the key the hints show, toasts, the
/// call's widget and its corner. Where the desktop cannot show the overlay, one note line
/// under the heading and every row dims in place (the switch shows off); the rows after
/// the switch dim while it is off, and the corner while the widget is.
fn overlay_group(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    use easyplay_core::config::schema::OverlayCorner;
    use easyplay_core::service::OverlayStatus;
    use strings::chat::settings as cs;
    let prefs = env.bridge.prefs().clone();
    let cannot = match env.bridge.overlay() {
        OverlayStatus::Unsupported | OverlayStatus::NoGpu | OverlayStatus::Missing => true,
        OverlayStatus::Idle | OverlayStatus::Running | OverlayStatus::Failing => false,
    };
    rows::heading(ui, cs::OVERLAY(), false);
    if cannot {
        note_line(ui, cs::OVERLAY_NONE());
    }
    note_key(ui, "chat.overlay");
    let mut on = prefs.chat.overlay && !cannot;
    let changed = SettingRow::new(cs::OVERLAY_ON())
        .help(cs::OVERLAY_HELP())
        .enabled(!cannot)
        .show(ui, |ui| {
            band(ui, control::TOGGLE_H, |ui| {
                switch(ui, &mut on, cs::OVERLAY_ON()).changed()
            })
        });
    if changed {
        set_bool(state, env, "chat.overlay", on, |p, v| p.chat.overlay = v);
    }
    let live = !cannot && prefs.chat.overlay;
    key_row(ui, state, env, live);
    switch_row(
        ui,
        state,
        env,
        "chat.overlay_toasts",
        cs::TOASTS(),
        cs::TOASTS_HELP(),
        live,
        |p| p.chat.overlay_toasts,
        |p, v| p.chat.overlay_toasts = v,
    );
    switch_row(
        ui,
        state,
        env,
        "chat.overlay_voice_widget",
        cs::WIDGET(),
        cs::WIDGET_HELP(),
        live,
        |p| p.chat.overlay_voice_widget,
        |p, v| p.chat.overlay_voice_widget = v,
    );
    const CORNER: &str = "chat.overlay_corner";
    note_key(ui, CORNER);
    let labels = [
        cs::CORNER_TL(),
        cs::CORNER_TR(),
        cs::CORNER_BL(),
        cs::CORNER_BR(),
    ];
    let current = idx(OverlayCorner::ALL, prefs.chat.overlay_corner);
    let mut i = current;
    let changed = SettingRow::new(cs::CORNER())
        .enabled(live && prefs.chat.overlay_voice_widget)
        .show(ui, |ui| {
            band(ui, control::SM, |ui| {
                select(ui, cs::CORNER(), None, None, &mut i, &labels, false).changed()
            })
        });
    if changed && i != current {
        let corner = OverlayCorner::ALL[i.min(3)];
        let mut next = (*prefs).clone();
        next.chat.overlay_corner = corner;
        commit(
            state,
            env,
            CORNER,
            PreferencePatch::new().set(CORNER, corner.as_str()),
            next,
        );
    }
}

/// One quiet line under a heading: a 13 info glyph and 12 / 16 text in `t3`.
fn note_line(ui: &mut Ui, text: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let glyph = 13.0;
    let w = ui.available_width();
    let g = Type::Caption.galley(ui, text, p.t3, w - glyph - space::S2, usize::MAX);
    let h = g.size().y.max(Type::Caption.line_height());
    let (r, resp) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let (gx, tx) = (r.left() + glyph / 2.0, r.left() + glyph + space::S2);
    ui.painter().text(
        pos2(gx, r.top() + Type::Caption.line_height() / 2.0),
        egui::Align2::CENTER_CENTER,
        theme::icons::INFO.text(false),
        theme::icons::font(glyph),
        p.t3,
    );
    ui.painter().galley(pos2(tx, r.top()), g, p.t3);
    crate::a11y::item(&resp, egui::Role::Label, text);
    ui.add_space(space::S2);
}

/// The overlay key's row (`chat.overlay_key`): under the label "Bind a key in your desktop
/// to run [easyplay overlay] [Copy]"; at the end the key as a key cap. The key is only a
/// label for the overlay's hints: the desktop owns the binding.
fn key_row(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, enabled: bool) {
    use strings::chat::settings as cs;
    const KEY: &str = "chat.overlay_key";
    note_key(ui, KEY);
    let id = Id::unique("settings.chat.overlay_key");
    let now = ui.input(|i| i.time);
    let copied_at: Option<f64> = ui.ctx().data(|d| d.get_temp(id.with("copied")));
    let copied = copied_at.is_some_and(|t| now - t < COPIED_FOR);
    if copied {
        ui.ctx().request_repaint_after_secs(0.1);
    }
    let mut copy = false;
    let current = env.bridge.prefs().chat.overlay_key.clone();
    let mut picked: Option<Pick> = None;
    SettingRow::new(cs::KEY()).enabled(enabled).show_with(
        ui,
        |ui| copy = key_help_line(ui, copied),
        |ui| {
            band(ui, control::BUTTON_SM, |ui| {
                picked = key_cap(ui, &current, enabled);
            })
        },
    );
    if copy {
        ui.ctx().copy_text(cs::OVERLAY_COMMAND.to_owned());
        ui.ctx().data_mut(|d| d.insert_temp(id.with("copied"), now));
    }
    let label = match picked {
        Some(Pick::Key(k)) => k,
        Some(Pick::Clear) => String::new(),
        None => return,
    };
    if label != current {
        let mut next = (**env.bridge.prefs()).clone();
        next.chat.overlay_key.clone_from(&label);
        commit(
            state,
            env,
            KEY,
            PreferencePatch::new().set(KEY, label),
            next,
        );
    }
}

/// How long "Copied" shows after Copy, in seconds.
const COPIED_FOR: f64 = 1.6;

/// What the key cap recorded.
enum Pick {
    Key(String),
    Clear,
}

/// The key row's help: the sentence around the command chip (mono, 22 high, `o2`, radius 5)
/// and a small Copy button that reads "Copied" for a moment. True when Copy was pressed.
fn key_help_line(ui: &mut Ui, copied: bool) -> bool {
    use strings::chat::settings as cs;
    let p = theme::theme(ui.ctx()).palette.clone();
    let (before, after) = cs::key_help();
    let mut pressed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S1 + 2.0;
        let h = control::BUTTON_XS;
        let text = |ui: &mut Ui, t: &str| {
            if t.is_empty() {
                return;
            }
            let g = Type::Caption.galley(ui, t, p.t3, f32::INFINITY, 1);
            let (r, _) = ui.allocate_exact_size(vec2(g.size().x, h), Sense::hover());
            ui.painter()
                .galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, p.t3);
        };
        text(ui, &before);
        let g = Type::Mono.galley(ui, cs::OVERLAY_COMMAND, p.t1, f32::INFINITY, 1);
        let (r, _) = ui.allocate_exact_size(vec2(g.size().x + space::S2 * 2.0, h), Sense::hover());
        ui.painter()
            .rect_filled(r, egui::CornerRadius::same(5), p.o2);
        ui.painter().galley(
            pos2(r.left() + space::S2, r.center().y - g.size().y / 2.0),
            g,
            p.t1,
        );
        let (icon, label) = if copied {
            (theme::icons::CHECK, cs::COPIED())
        } else {
            (theme::icons::COPY, cs::COPY())
        };
        pressed = ui
            .add(
                Button::new(label)
                    .icon(icon)
                    .size(crate::widgets::button::Size::Xs),
            )
            .clicked();
        text(ui, &after);
    });
    pressed
}

/// The key as a key cap ("Shift+F1"; "None" without one). A click or Enter records: the cap
/// reads "Press the key…" in accent text with a 1.5 accent ring, the next combination with
/// a key that is not a modifier becomes the label, Esc cancels, Backspace or Delete clears.
fn key_cap(ui: &mut Ui, current: &str, enabled: bool) -> Option<Pick> {
    use strings::chat::settings as cs;
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let mut recording = enabled && listening_for_key(ui.ctx());
    let text = if recording {
        cs::KEY_PRESS()
    } else if current.is_empty() {
        cs::KEY_NONE()
    } else {
        current
    };
    let color = if recording { p.accent_text } else { p.t1 };
    let g = Type::Label.galley(ui, text, color, f32::INFINITY, 1);
    let size = vec2(g.size().x + space::S2 * 2.0, control::BUTTON_SM - 4.0);
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let corner = egui::CornerRadius::same(theme::tokens::radius::SM);
    let ring = if recording {
        egui::Stroke::new(1.5, p.accent_mark)
    } else {
        egui::Stroke::new(1.0, p.line2)
    };
    ui.painter()
        .rect(r, corner, p.o1, ring, egui::StrokeKind::Inside);
    ui.painter().galley(
        pos2(
            r.center().x - g.size().x / 2.0,
            r.center().y - g.size().y / 2.0,
        ),
        g,
        color,
    );
    crate::a11y::item(&resp, egui::Role::Button, cs::KEY_CHANGE());
    if resp.has_focus() && crate::widgets::focus_visible(ui.ctx()) {
        crate::widgets::focus_ring(ui.painter(), r.expand(2.0), corner, &theme);
    }
    let mut picked = None;
    if recording {
        // Every key goes to the recorder while it listens.
        let keys: Vec<(egui::Key, egui::Modifiers)> = ui.input_mut(|i| {
            let keys = i
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect();
            i.events.retain(|e| !matches!(e, egui::Event::Key { .. }));
            keys
        });
        if let Some((key, mods)) = keys.first() {
            recording = false;
            picked = match key {
                egui::Key::Escape => None,
                egui::Key::Backspace | egui::Key::Delete if mods.is_none() => Some(Pick::Clear),
                _ => Some(Pick::Key(key_label(*key, *mods))),
            };
        } else if resp.lost_focus() || resp.clicked_elsewhere() {
            recording = false;
        }
    } else if resp.clicked() && enabled {
        recording = true;
        resp.request_focus();
    }
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx()
        .data_mut(|d| d.insert_temp(recorder_id(), recording.then_some(pass)));
    picked
}

fn recorder_id() -> Id {
    Id::unique("settings.chat.overlay_key.recording")
}

/// The overlay key's cap is listening: every key goes to it, so the window's shortcuts
/// (Esc, Backspace, F-keys) stand aside. It stops listening when it is no longer drawn.
pub fn listening_for_key(ctx: &egui::Context) -> bool {
    let last: Option<u64> = ctx.data(|d| d.get_temp(recorder_id())).flatten();
    last.is_some_and(|n| n + 2 >= ctx.cumulative_pass_nr())
}

/// "Ctrl+Shift+F2": the modifiers held, then the key.
fn key_label(key: egui::Key, mods: egui::Modifiers) -> String {
    let mut out = String::new();
    for (on, name) in [
        (mods.ctrl, "Ctrl"),
        (mods.alt, "Alt"),
        (mods.shift, "Shift"),
        (mods.mac_cmd, "Super"),
    ] {
        if on {
            out.push_str(name);
            out.push('+');
        }
    }
    out.push_str(key.name());
    out
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
        env.bridge.send(Command::VerifyRuntime);
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
        env.bridge.send(Command::RefreshRuntimes);
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
        env.bridge.send(Command::SetDefaultHandler { enable: true });
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
            .send(Command::SetDefaultHandler { enable: false });
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
            env.bridge.send(Command::SetShortcut {
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
            .send(Command::OpenFolder(FolderRequest::DataFolder));
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
                                SettingsTab::Chat => chat_tab(ui, state, env),
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
