//! The mic caret's device menu (`chat-calls.html` notes D "DEVICE MENU"): microphone and
//! speaker in submenus, the microphone volume with a live meter, the noise switch, and Voice
//! settings. It writes the same preferences as Settings › Chat (`chat.mic`, `chat.speakers`,
//! `chat.mic_volume`, `chat.noise_suppression`); the media reads them when a call starts.

use easyplay_core::config::schema::{PreferencePatch, Preferences};
use egui::{
    Align2, Id, Popup, PopupAnchor, PopupCloseBehavior, Rect, RectAlign, Response, Sense, Ui, pos2,
    vec2,
};

use super::Env;
use crate::app::UiState;
use crate::bridge::{Command, UiCommand};
use crate::route::{Route, SettingsTab};
use crate::strings::chat::calls as cs;
use crate::theme::{self, Type};
use crate::vm::dialogs::AfterDiscard;
use crate::widgets::context_menu;
use easyplay_core::model::{AudioDevice, AudioDevices};
use easyplay_core::service::ChatCommand;

/// The menu's width, and the meter: height, radius.
const W: f32 = 300.0;
const METER: f32 = 6.0;
/// The menu's start edge stands this far before the caret's.
const LEAD: f32 = 40.0;

/// The menu hanging from the mic capsule's `caret`.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, caret: &Response) {
    let close = std::mem::take(&mut state.chat.call.close_menu);
    let popup_id = Popup::default_response_id(caret);
    if close {
        Popup::close_id(ui.ctx(), popup_id);
    }
    let prefs = env.bridge.prefs().clone();
    let snap = env.bridge.chat().clone();
    let level = f32::from(snap.live.levels.me) / 100.0;
    // The sound server's devices (read again each time the menu opens, as Settings does).
    let none = AudioDevices::default();
    let devs = snap.audio_devices.as_ref().unwrap_or(&none);
    // The menu's start edge stands 40 before the caret's (mirrored right to left).
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let (shift, align) = if rtl {
        (LEAD, RectAlign::BOTTOM_END)
    } else {
        (-LEAD, RectAlign::BOTTOM_START)
    };
    let popup = Popup::menu(caret)
        .anchor(PopupAnchor::ParentRect(
            caret.rect.translate(vec2(shift, 0.0)),
        ))
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(align)
        .width(W);
    let mut change: Option<Change> = None;
    let open = context_menu::popup(popup, |ui| {
        ui.set_width(W - 10.0);
        match context_menu::open_sub(ui).as_deref() {
            Some("mic") => {
                context_menu::back(ui, cs::MICROPHONE());
                devices(ui, &prefs.chat.mic, &devs.inputs, &mut change, Change::Mic);
            }
            Some("speaker") => {
                context_menu::back(ui, cs::SPEAKER());
                devices(
                    ui,
                    &prefs.chat.speakers,
                    &devs.outputs,
                    &mut change,
                    Change::Speaker,
                );
            }
            _ => top(ui, &prefs, devs, level, &mut change),
        }
    });
    if open.is_some() && !state.chat.call.menu_open {
        env.bridge
            .send(Command::Chat(ChatCommand::RefreshAudioDevices));
    }
    state.chat.call.menu_open = open.is_some();
    if let Some(c) = change {
        apply(state, env, c);
    }
}

/// What the menu changed.
enum Change {
    Mic(String),
    Speaker(String),
    Volume(u8),
    Noise(bool),
    Settings,
}

fn top(
    ui: &mut Ui,
    prefs: &Preferences,
    devs: &AudioDevices,
    level: f32,
    change: &mut Option<Change>,
) {
    valued(
        ui,
        "mic",
        cs::MICROPHONE(),
        &label_of(&prefs.chat.mic, &devs.inputs),
    );
    valued(
        ui,
        "speaker",
        cs::SPEAKER(),
        &label_of(&prefs.chat.speakers, &devs.outputs),
    );
    context_menu::separator(ui);
    volume(ui, prefs.chat.mic_volume, level, change);
    context_menu::separator(ui);
    let mut noise = prefs.chat.noise_suppression;
    let row = ui.horizontal(|ui| {
        ui.set_min_height(34.0);
        ui.add_space(8.0);
        ui.label(Type::Body.rich(cs::NOISE()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            crate::widgets::toggle::switch(ui, &mut noise, cs::NOISE()).changed()
        })
        .inner
    });
    // The whole row toggles; the switch is the accessible element.
    let whole = ui.interact(
        row.response.rect,
        Id::unique("chat.call.menu.noise"),
        Sense::click(),
    );
    if row.inner || whole.clicked() {
        *change = Some(Change::Noise(if row.inner { noise } else { !noise }));
    }
    context_menu::separator(ui);
    if context_menu::sub_item(ui, "settings-door", cs::SETTINGS()).clicked() {
        *change = Some(Change::Settings);
        ui.close();
    }
}

/// A row that opens a device list, with the chosen device in quiet text before the caret.
fn valued(ui: &mut Ui, key: &str, label: &str, shown: &str) {
    let r = context_menu::sub_item(ui, key, label);
    let p = theme::theme(ui.ctx()).palette.clone();
    // Menu rows keep their label first and the caret last in either direction: the value
    // stands just before the caret.
    let g = Type::Caption.galley(ui, shown, p.t3, r.rect.width() / 2.0, 1);
    let x = r.rect.right() - 28.0 - g.size().x;
    ui.painter()
        .galley(pos2(x, r.rect.center().y - g.size().y / 2.0), g, p.t3);
}

/// How the stored device `name` reads, as in Settings: "System default", the device's
/// description, or "<name> (not connected)" while it is not among `known`.
fn label_of(name: &str, known: &[AudioDevice]) -> String {
    if name.is_empty() {
        return cs::SYSTEM_DEFAULT().to_owned();
    }
    known.iter().find(|d| d.name == name).map_or_else(
        || crate::strings::chat::settings::not_connected(name),
        |d| d.description.clone(),
    )
}

/// The devices to choose from: the system's default, the known ones by their descriptions,
/// and the stored one while it is not connected (kept until changed, as in Settings).
fn devices(
    ui: &mut Ui,
    current: &str,
    known: &[AudioDevice],
    change: &mut Option<Change>,
    make: fn(String) -> Change,
) {
    if context_menu::option_item(ui, cs::SYSTEM_DEFAULT(), current.is_empty()).clicked() {
        *change = Some(make(String::new()));
    }
    for d in known {
        if context_menu::option_item(ui, &d.description, d.name == current).clicked() {
            *change = Some(make(d.name.clone()));
        }
    }
    if !current.is_empty() && !known.iter().any(|d| d.name == current) {
        let label = crate::strings::chat::settings::not_connected(current);
        context_menu::option_item(ui, &label, true);
    }
}

/// "Microphone volume" and its value, the slider, and the live meter under it.
fn volume(ui: &mut Ui, saved: u8, level: f32, change: &mut Option<Change>) {
    let th = theme::theme(ui.ctx());
    let p = &th.palette;
    let id = Id::unique("chat.call.menu.volume");
    let mut v = ui
        .data(|d| d.get_temp::<f32>(id))
        .unwrap_or(f32::from(saved));
    let w = ui.available_width() - 16.0;
    let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    let inner = head.shrink2(vec2(8.0, 0.0));
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let (a, b) = if rtl {
        (Align2::RIGHT_CENTER, Align2::LEFT_CENTER)
    } else {
        (Align2::LEFT_CENTER, Align2::RIGHT_CENTER)
    };
    let start = if rtl {
        inner.right_center()
    } else {
        inner.left_center()
    };
    let end = if rtl {
        inner.left_center()
    } else {
        inner.right_center()
    };
    ui.painter()
        .text(start, a, cs::MIC_VOLUME(), Type::Caption.font_id(), p.t2);
    let value = cs::percent(v.round() as u8);
    ui.painter()
        .text(end, b, &value, Type::Caption.numeric_font_id(), p.t2);
    let resp = ui
        .horizontal(|ui| {
            ui.add_space(8.0);
            crate::widgets::slider::slider(
                ui,
                &mut v,
                0.0..=100.0,
                5.0,
                w,
                cs::MIC_VOLUME(),
                &value,
            )
        })
        .inner;
    if resp.dragged() {
        ui.data_mut(|d| d.insert_temp(id, v));
    } else {
        ui.data_mut(|d| d.remove_temp::<f32>(id));
        if resp.changed() || resp.drag_stopped() {
            *change = Some(Change::Volume(v.round() as u8));
        }
    }
    ui.add_space(8.0);
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), METER), Sense::hover());
    let track = row.shrink2(vec2(8.0, 0.0));
    let shown = ui
        .ctx()
        .animate_value_with_time(id.with("meter"), level, 0.06);
    let corner = egui::CornerRadius::same(3);
    ui.painter().rect_filled(track, corner, p.o2);
    let fill_w = track.width() * shown.clamp(0.0, 1.0);
    let fill = if rtl {
        Rect::from_min_max(pos2(track.right() - fill_w, track.top()), track.max)
    } else {
        Rect::from_min_size(track.min, vec2(fill_w, METER))
    };
    // Status is neutral: the level is t2, never accent.
    ui.painter().rect_filled(fill, corner, p.t2);
    ui.add_space(4.0);
}

fn apply(state: &mut UiState, env: &mut Env<'_>, c: Change) {
    let mut next = (**env.bridge.prefs()).clone();
    let patch = match c {
        Change::Mic(v) => {
            next.chat.mic = v.clone();
            PreferencePatch::new().set("chat.mic", v)
        }
        Change::Speaker(v) => {
            next.chat.speakers = v.clone();
            PreferencePatch::new().set("chat.speakers", v)
        }
        Change::Volume(v) => {
            next.chat.mic_volume = v;
            PreferencePatch::new().set("chat.mic_volume", i64::from(v))
        }
        Change::Noise(on) => {
            next.chat.noise_suppression = on;
            PreferencePatch::new().set("chat.noise_suppression", on)
        }
        Change::Settings => {
            state.navigate(AfterDiscard::Route(Route::Settings(SettingsTab::Chat)));
            return;
        }
    };
    env.bridge.send(UiCommand::SetPreference {
        patch,
        preview: Box::new(next.clone()),
    });
    env.bridge.set_prefs_preview(next);
}
