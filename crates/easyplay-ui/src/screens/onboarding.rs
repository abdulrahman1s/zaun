//! The welcome steps (SCREEN_SPECS §1, UI-024, UF-1; ADR-0018 opt-in): Welcome · System
//! check · Compatibility layer · Opening Windows programs. They cover the whole window: one
//! 460 column in the middle (padding 32 / 24), the mark, a display title, a sentence, the
//! step's body (at least 210 high), and a foot with "2 of 4" at the left and the buttons at
//! the right, the main one a Play-sized pill. A failed required check keeps *Continue* off;
//! *Skip for now* passes it and leaves a banner. Steps change with a short rise. The reduced
//! form (started from a problem) shows the check only.

use std::time::Duration;

use easyplay_core::config::PreferencePatch;
use easyplay_core::config::schema::ArtworkOnline;
use easyplay_core::model::RuntimeRef;
use egui::{
    Align, Align2, CornerRadius, Id, Key, Layout, Modifiers, Rect, Sense, Ui, UiBuilder, pos2, vec2,
};

use super::Env;
use super::debug::doctor::{self, Adopt};
use super::settings::band;
use crate::app::UiState;
use crate::bridge::{CheckState, Command, HandlerState, UiCommand};
use crate::route::Route;
use crate::strings::onboarding as s;
use crate::theme::tokens::{control, icon as icon_size, layout, stroke, travel};
use crate::theme::{self, Dur, Type, icons, motion};
use crate::vm::screens::OnboardingStep;
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::rows::SettingRow;
use crate::widgets::toggle::switch;
use crate::widgets::{brand, progress, tooltip};

/// The size of onboarding's own window on first launch. A compositor that marks windows
/// tiled (niri with `prefer-no-csd`) ignores later resizes, so onboarding does not share
/// the library's window.
pub const WINDOW_SIZE: [f32; 2] = [720.0, 680.0];

fn finish(state: &mut UiState, env: &mut Env<'_>, skipped: bool) {
    env.bridge.send(UiCommand::CompleteOnboarding { skipped });
    if state.onboarding.artwork_online && !state.onboarding.reduced {
        let mut next = (**env.bridge.prefs()).clone();
        next.artwork.online = ArtworkOnline::Steam;
        env.bridge.send(UiCommand::SetPreference {
            patch: PreferencePatch::new().set("artwork.online", "steam"),
            preview: Box::new(next.clone()),
        });
        env.bridge.set_prefs_preview(next);
    }
    state.onboarding = Default::default();
    state.nav.go_to(Route::Library);
    if state.onboarding_window {
        // The library opens in a full window of its own (`easyplay` binary, run_gui).
        env.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// What the foot's buttons asked for.
#[derive(Default)]
struct Foot {
    next: bool,
    back: bool,
    skip: bool,
    later: bool,
    done: bool,
}

/// Show the steps.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.painter()
        .rect_filled(ui.max_rect(), CornerRadius::ZERO, p.bg);
    let host = env.bridge.host().clone();
    let reduced = state.onboarding.reduced;
    if reduced && state.onboarding.step != OnboardingStep::SystemCheck {
        state.onboarding.step = OnboardingStep::SystemCheck;
    }
    let step = state.onboarding.step;
    let total = if reduced {
        1
    } else {
        OnboardingStep::ALL.len()
    };
    if state.onboarding.announced != Some(step) {
        state.onboarding.announced = Some(step);
        crate::a11y::Announcer::announce(
            ui.ctx(),
            s::step_announcement(step.index() + 1, total, s::step_title(step)),
            crate::a11y::Politeness::Polite,
        );
    }
    if ui.input_mut(|i| i.consume_key(Modifiers::ALT, Key::ArrowLeft))
        && step.index() > 0
        && !reduced
    {
        state.onboarding.step = OnboardingStep::ALL[step.index() - 1];
    }
    let mut foot = Foot::default();
    let viewport_h = ui.available_height();
    let [pad_y, pad_x] = layout::WELCOME_PAD;
    egui::ScrollArea::vertical()
        .id_salt("welcome")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // The column is centred both ways while it fits, and scrolls from the top
            // otherwise. Its height is known from the last pass.
            let measured = Id::unique("welcome.height");
            let h: f32 = ui
                .ctx()
                .data(|d| d.get_temp(measured))
                .unwrap_or(viewport_h - pad_y * 2.0);
            let top = ((viewport_h - h) / 2.0).max(pad_y);
            let avail = ui.available_width();
            let w = (avail - pad_x * 2.0).clamp(1.0, layout::WELCOME_W);
            let left = ui.cursor().min.x + (avail - w) / 2.0;
            let origin = pos2(left, ui.cursor().min.y + top);
            let mut col = ui.new_child(
                UiBuilder::new()
                    .id_salt("welcome.column")
                    .max_rect(Rect::from_min_size(origin, vec2(w, f32::INFINITY)))
                    .layout(Layout::top_down(Align::Min)),
            );
            col.set_width(w);
            col.spacing_mut().item_spacing.y = 0.0;
            column(&mut col, state, env, &host, step, reduced, total, &mut foot);
            let used = col.min_rect().height();
            if (used - h).abs() > 0.5 {
                ui.ctx().data_mut(|d| d.insert_temp(measured, used));
                ui.ctx().request_discard("welcome-column-measured");
            }
            ui.allocate_rect(
                Rect::from_min_size(
                    pos2(ui.cursor().min.x, origin.y - top),
                    vec2(avail, top + used + pad_y),
                ),
                Sense::hover(),
            );
        });
    if foot.back {
        state.onboarding.step = OnboardingStep::ALL[step.index().saturating_sub(1)];
    }
    if foot.later {
        state.onboarding.later = true;
        foot.next = true;
    }
    if foot.next {
        let i = (step.index() + 1).min(OnboardingStep::ALL.len() - 1);
        state.onboarding.step = OnboardingStep::ALL[i];
        if state.onboarding.step == OnboardingStep::SystemCheck {
            env.bridge.send(Command::RefreshHostFacts);
        }
    }
    if foot.skip {
        if reduced {
            state.onboarding = Default::default();
            state.nav.go_to(Route::Library);
        } else {
            state.onboarding.step = OnboardingStep::Layer;
        }
        if host.has_blocking_failure() {
            state.banners.set(super::shell::Banner {
                key: "host",
                severity: crate::widgets::banner::Severity::Warning,
                text: s::BANNER().to_owned(),
                action: Some(s::CHECK_AGAIN().to_owned()),
                dismissible: true,
            });
        }
    }
    if foot.done {
        let skipped = host.has_blocking_failure();
        finish(state, env, skipped);
    }
}

#[allow(clippy::too_many_arguments)]
fn column(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    host: &crate::bridge::HostView,
    step: OnboardingStep,
    reduced: bool,
    total: usize,
    foot: &mut Foot,
) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let w = ui.available_width();
    // The mark.
    let (mark, _) = ui.allocate_exact_size(
        vec2(layout::WELCOME_MARK, layout::WELCOME_MARK),
        Sense::hover(),
    );
    brand::paint_mark(ui.painter(), mark, p.fill, p.bg);
    ui.add_space(layout::WELCOME_MARK_GAP);
    // The step rises into place whenever it changes.
    let (alpha, below) = motion::arrive(
        ui.ctx(),
        Id::unique(("welcome.step", step)),
        theme.motion,
        0.0,
        Dur::Move,
        travel::RISE,
    );
    let blocking = step == OnboardingStep::SystemCheck && host.has_blocking_failure();
    ui.scope(|ui| {
        ui.multiply_opacity(alpha);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(below);
        let title = s::step_title(step);
        let g = Type::Welcome.galley(ui, title, p.t1, w, usize::MAX);
        let (r, resp) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
        ui.painter().galley(r.min, g, p.t1);
        crate::a11y::labelled(ui.ctx(), resp.id, egui::Role::Heading, title);
        ui.add_space(layout::LEDE_GAP);
        let g = Type::Lead.galley(ui, s::step_body(step), p.t2, w, usize::MAX);
        let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
        ui.painter().galley(r.min, g, p.t2);
        ui.add_space(layout::WELCOME_BODY_GAP);
        let top = ui.cursor().min.y;
        match step {
            OnboardingStep::Welcome => points(ui),
            OnboardingStep::SystemCheck => check(ui, state, env, host),
            OnboardingStep::Layer => layer(ui, state, env),
            OnboardingStep::Handler => handler(ui, state, env, host),
        }
        let used = ui.cursor().min.y - top;
        if used < layout::WELCOME_BODY_MIN {
            ui.add_space(layout::WELCOME_BODY_MIN - used);
        }
    });
    ui.add_space(layout::WELCOME_FOOT_GAP);
    // The foot: where you are at the left, the buttons at the right.
    let (row, _) = ui.allocate_exact_size(vec2(w, control::PLAY), Sense::hover());
    if !reduced {
        let g = Type::Body.numeric_galley(ui, &s::step_of(step.index() + 1, total), p.t3);
        ui.painter()
            .galley(pos2(row.left(), row.center().y - g.size().y / 2.0), g, p.t3);
    }
    let mut right = ui.new_child(
        UiBuilder::new()
            .id_salt("welcome.foot")
            .max_rect(row)
            .layout(Layout::right_to_left(Align::Center)),
    );
    right.spacing_mut().item_spacing.x = theme::tokens::space::S2;
    let last = reduced || step == OnboardingStep::Handler;
    let label = if last { s::FINISH } else { s::CONTINUE };
    // The main button: Play's size, off (with why) while a required check fails.
    let main = right.add(
        Button::primary(label())
            .size(Size::Lg)
            .disabled(blocking.then_some(s::SKIP_TIP())),
    );
    if right.memory(|m| m.focused().is_none()) && !blocking {
        main.request_focus();
    }
    if main.clicked() && !blocking {
        if last {
            foot.done = true;
        } else {
            foot.next = true;
        }
    }
    if blocking {
        let skip = right.add(Button::ghost(s::SKIP()));
        if tooltip::tooltip(skip, s::SKIP_TIP(), None).clicked() {
            foot.skip = true;
        }
    }
    if step == OnboardingStep::Layer
        && nothing_installed(env)
        && !state.onboarding.downloading
        && right.add(Button::ghost(s::LATER())).clicked()
    {
        foot.later = true;
    }
    if step.index() > 0 && !reduced && right.add(Button::ghost(s::BACK())).clicked() {
        foot.back = true;
    }
}

/// The first step's three lines, each over a hairline, the first under one too.
fn points(ui: &mut Ui) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width();
    let pad = layout::WELCOME_POINT_PAD;
    let rule = |ui: &Ui, y: f32| {
        ui.painter().hline(
            ui.max_rect().x_range(),
            y,
            egui::Stroke::new(stroke::BORDER, p.line),
        );
    };
    rule(ui, ui.cursor().min.y + stroke::BORDER / 2.0);
    for line in s::WELCOME_POINTS() {
        let g = Type::Lead.galley(ui, line, p.t1, w, usize::MAX);
        let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y + pad * 2.0), Sense::hover());
        ui.painter().galley(pos2(r.left(), r.top() + pad), g, p.t1);
        rule(ui, r.bottom() - stroke::BORDER / 2.0);
    }
}

/// The system check: the rows, and *Check again* under them.
fn check(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, host: &crate::bridge::HostView) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let rt = env.bridge.runtimes().clone();
    if rt.adoptable.is_empty() {
        state.onboarding.adopting = false;
    }
    let out = doctor::check_rows(
        ui,
        &host.checks,
        Adopt {
            available: !rt.adoptable.is_empty(),
            busy: state.onboarding.adopting,
        },
    );
    if out.adopt {
        state.onboarding.adopting = true;
        for v in &rt.adoptable {
            env.bridge.send(Command::AdoptRuntime { variant: *v });
        }
    }
    if host.checks.is_empty() || host.checks.iter().any(|c| c.state == CheckState::Pending) {
        let since = *state.onboarding.check_started.get_or_insert(env.mono);
        let waited = env.mono.saturating_duration_since(since);
        if waited > Duration::from_secs(5) {
            ui.label(Type::Caption.rich(s::STILL_CHECKING()).color(p.t3));
        } else {
            ui.ctx()
                .request_repaint_after(Duration::from_secs(5).saturating_sub(waited));
        }
    }
    ui.add_space(theme::tokens::space::S2);
    let again = super::statistics::on_axis(ui, |ui| ui.add(Button::ghost(s::CHECK_AGAIN())));
    if again.clicked() {
        state.onboarding.check_started = Some(env.mono);
        env.bridge.send(Command::RefreshHostFacts);
    }
}

fn nothing_installed(env: &Env<'_>) -> bool {
    let rt = env.bridge.runtimes();
    rt.default_label.is_none() && rt.downloads.is_empty()
}

/// The compatibility layer (a step the design does not draw): one row saying what there is,
/// with *Download now* while there is nothing.
fn layer(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let rt = env.bridge.runtimes().clone();
    match (&rt.default_label, rt.downloads.first()) {
        (_, Some(d)) => {
            let label = s::downloading(&d.label);
            let size = crate::strings::fmt::bytes(d.bytes);
            SettingRow::new(&label)
                .help(&format!("{}: {size}", s::DOWNLOADED()))
                .show(ui, |ui| {
                    band(ui, control::SPINNER, |ui| {
                        progress::spinner_widget(ui, p.t3)
                    })
                });
        }
        (Some(label), None) => {
            let found = s::found_layer(label);
            SettingRow::new(&found)
                .help(s::LAYER_READY())
                .show(ui, |ui| {
                    band(ui, icon_size::INLINE, |ui| done_glyph(ui, &found))
                });
        }
        (None, None) => {
            let pressed = SettingRow::new(s::NO_LAYER_TITLE())
                .help(s::NO_LAYER())
                .show(ui, |ui| {
                    band(ui, control::BUTTON, |ui| {
                        ui.add(Button::new(s::DOWNLOAD()).variant(Variant::Secondary))
                            .clicked()
                    })
                });
            if pressed {
                state.onboarding.downloading = true;
                env.bridge.send(Command::ProvisionRuntime {
                    target: RuntimeRef::Default,
                });
            }
        }
    }
}

/// Opening Windows programs: who opens `.exe` files now, with *Make EasyPlay the default*,
/// and the official artwork switch.
fn handler(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, host: &crate::bridge::HostView) {
    let ours = host.handler == HandlerState::EasyPlay;
    let now = s::handler_now(&host.handler);
    let make = SettingRow::new(s::EXE_FILES()).help(&now).show(ui, |ui| {
        band(ui, control::BUTTON, |ui| {
            if ours {
                done_glyph(ui, s::DEFAULT_DONE());
                false
            } else {
                ui.add(Button::new(s::MAKE_DEFAULT()).variant(Variant::Secondary))
                    .clicked()
            }
        })
    });
    if make {
        env.bridge.send(Command::SetDefaultHandler { enable: true });
    }
    SettingRow::new(s::ARTWORK_TITLE())
        .help(s::ARTWORK_HELP())
        .show(ui, |ui| {
            band(ui, control::TOGGLE_H, |ui| {
                switch(ui, &mut state.onboarding.artwork_online, s::ARTWORK_TITLE());
            })
        });
}

/// A check where a row's control was: the thing is done.
fn done_glyph(ui: &mut Ui, name: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let (rect, resp) =
        ui.allocate_exact_size(vec2(icon_size::INLINE, icon_size::INLINE), Sense::hover());
    crate::a11y::item(&resp, egui::Role::Image, name);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icons::CHECK.text(false),
        icons::font(icon_size::INLINE),
        p.t1,
    );
}
