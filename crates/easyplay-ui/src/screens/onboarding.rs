//! Onboarding (SCREEN_SPECS §1, UI-024, UF-1; ADR-0018 opt-in): Welcome · System check ·
//! Compatibility layer · Opening Windows programs. A centred 560 px column, no sidebar. A
//! blocking failure cannot be passed with Continue (the reason is beside it); *Skip for now*
//! leaves a banner. The reduced form (started from a problem) shows the check only.

use std::time::Duration;

use easyplay_core::config::PreferencePatch;
use easyplay_core::config::schema::ArtworkOnline;
use easyplay_core::model::{RuntimeRef, SlrVariant};
use egui::{Align, Key, Layout, Modifiers, Sense, Ui, vec2};

use super::Env;
use crate::app::UiState;
use crate::bridge::{CheckState, HandlerState, HostCheck, UiCommand};
use crate::route::Route;
use crate::strings::onboarding as s;
use crate::theme::tokens::{icon as icon_size, radius, space};
use crate::theme::{self, Type, icons};
use crate::vm::screens::OnboardingStep;
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::progress;

fn dots(ui: &mut Ui, step: OnboardingStep, total: usize) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), space::S6), Sense::hover());
    let w = total as f32 * space::S4;
    let x0 = rect.center().x - w / 2.0;
    for i in 0..total {
        let c = egui::pos2(x0 + i as f32 * space::S4 + space::S2, rect.center().y);
        if i == step.index() {
            ui.painter().circle_filled(c, 4.0, p.accent);
        } else {
            ui.painter()
                .circle_stroke(c, 4.0, egui::Stroke::new(1.5, p.border_strong));
        }
    }
}

/// "Use umu's runtime", or a note while the copy runs. True when pressed.
fn adopt_button(ui: &mut Ui, adoptable: &[SlrVariant], adopting: bool) -> bool {
    if adoptable.is_empty() {
        return false;
    }
    if adopting {
        let p = theme::theme(ui.ctx()).palette.clone();
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(Type::Body.rich(s::ADOPTING).color(p.text_secondary));
        });
        return false;
    }
    ui.add(Button::primary(s::USE_UMU_RUNTIME).icon(icons::DOWNLOAD))
        .clicked()
}

/// Rows of the check; true when the runtime row's adopt button was pressed.
fn check_row(ui: &mut Ui, c: &HostCheck, adoptable: &[SlrVariant], adopting: bool) -> bool {
    let mut adopt = false;
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let (icon, color) = match c.state {
        CheckState::Pass => (icons::CHECK, p.success),
        CheckState::Warn => (icons::WARNING, p.warning),
        CheckState::Fail => (icons::ERROR, p.danger),
        CheckState::Pending => (icons::CLOCK, p.text_muted),
    };
    let r = ui.horizontal(|ui| {
        ui.set_min_height(space::S8);
        if c.state == CheckState::Pending {
            let (rr, _) =
                ui.allocate_exact_size(vec2(icon_size::BUTTON, icon_size::BUTTON), Sense::hover());
            crate::widgets::skeleton::block(ui, rr, Some(egui::CornerRadius::same(radius::XS)));
        } else {
            ui.label(icons::icon(icon, icon_size::BUTTON).color(color));
        }
        ui.allocate_ui_with_layout(
            vec2(200.0, space::S6),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_width(200.0);
                ui.add(
                    egui::Label::new(Type::BodyStrong.rich(&c.name).color(p.text_primary))
                        .truncate(),
                );
            },
        );
        ui.add(egui::Label::new(Type::Body.rich(&c.result).color(p.text_secondary)).wrap());
    });
    crate::a11y::labelled(
        ui.ctx(),
        r.response.id,
        egui::Role::ListItem,
        &s::check_name(c),
    );
    if c.state != CheckState::Pass {
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: (icon_size::BUTTON + space::S2) as i8,
                ..Default::default()
            })
            .show(ui, |ui| {
                // D-15's fix is "adopt umu's runtime": offer the button instead of the command.
                if c.id == "D-15" && !adoptable.is_empty() {
                    adopt = adopt_button(ui, adoptable, adopting);
                } else if let Some(d) = &c.detail {
                    ui.add(egui::Label::new(Type::Body.rich(d).color(p.text_secondary)).wrap());
                }
                if let Some(cmd) = &c.fix_command {
                    ui.label(Type::Caption.rich(s::TO_FIX).color(p.text_secondary));
                    ui.horizontal(|ui| {
                        egui::Frame::NONE
                            .fill(p.bg_sunken)
                            .corner_radius(egui::CornerRadius::same(radius::SM))
                            .inner_margin(space::S2)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(Type::Mono.rich(cmd).color(p.text_primary))
                                        .truncate(),
                                );
                            });
                        if ui
                            .add(
                                Button::new(s::COPY)
                                    .icon(icons::COPY)
                                    .size(Size::Sm)
                                    .accessible_name(s::COPY_COMMAND),
                            )
                            .clicked()
                        {
                            ui.ctx().copy_text(cmd.clone());
                        }
                    });
                }
            });
    }
    adopt
}

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
}

/// Show the screen.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
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
    let mut next = false;
    let mut back = false;
    let mut skip = false;
    let mut done = false;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(space::S12);
                let w = ui.available_width().min(560.0);
                ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(w);
                    ui.spacing_mut().item_spacing.y = space::S3;
                    if !reduced {
                        dots(ui, step, total);
                    }
                    ui.add_space(space::S4);
                    let t = ui.label(
                        Type::Display
                            .rich(s::step_title(step))
                            .color(p.text_primary),
                    );
                    crate::a11y::labelled(ui.ctx(), t.id, egui::Role::Heading, s::step_title(step));
                    ui.add(
                        egui::Label::new(
                            Type::Body.rich(s::step_body(step)).color(p.text_secondary),
                        )
                        .wrap(),
                    );
                    ui.add_space(space::S4);
                    match step {
                        OnboardingStep::Welcome => {
                            for line in s::WELCOME_POINTS {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        icons::icon(icons::CHECK, icon_size::INLINE)
                                            .color(p.accent),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            Type::Body.rich(line).color(p.text_primary),
                                        )
                                        .wrap(),
                                    );
                                });
                            }
                        }
                        OnboardingStep::SystemCheck => {
                            if host.checks.is_empty()
                                || host.checks.iter().any(|c| c.state == CheckState::Pending)
                            {
                                let since = state.onboarding.check_started.get_or_insert(env.mono);
                                if env.mono.saturating_duration_since(*since)
                                    > Duration::from_secs(5)
                                {
                                    ui.label(
                                        Type::Caption
                                            .rich(s::STILL_CHECKING)
                                            .color(p.text_secondary),
                                    );
                                }
                            }
                            let rt = env.bridge.runtimes().clone();
                            if rt.adoptable.is_empty() {
                                state.onboarding.adopting = false;
                            }
                            let mut adopt = false;
                            for c in &host.checks {
                                adopt |= check_row(ui, c, &rt.adoptable, state.onboarding.adopting);
                            }
                            if adopt {
                                state.onboarding.adopting = true;
                                for v in &rt.adoptable {
                                    env.bridge.send(UiCommand::AdoptRuntime { variant: *v });
                                }
                            }
                            if ui
                                .add(
                                    Button::new(s::CHECK_AGAIN)
                                        .icon(icons::REFRESH)
                                        .size(Size::Sm),
                                )
                                .clicked()
                            {
                                state.onboarding.check_started = Some(env.mono);
                                env.bridge.send(UiCommand::RefreshHostFacts);
                            }
                        }
                        OnboardingStep::Layer => {
                            let rt = env.bridge.runtimes().clone();
                            match (&rt.default_label, rt.downloads.first()) {
                                (_, Some(d)) => {
                                    ui.label(
                                        Type::BodyStrong
                                            .rich(s::downloading(&d.label))
                                            .color(p.text_primary),
                                    );
                                    progress::linear(ui, None, s::DOWNLOAD);
                                    ui.label(
                                        Type::Caption
                                            .rich(crate::strings::fmt::bytes(d.bytes))
                                            .color(p.text_secondary),
                                    );
                                }
                                (Some(label), None) => {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            icons::icon(icons::CHECK, icon_size::BUTTON)
                                                .color(p.success),
                                        );
                                        ui.label(
                                            Type::BodyStrong
                                                .rich(s::found_layer(label))
                                                .color(p.text_primary),
                                        );
                                    });
                                }
                                (None, None) => {
                                    ui.label(Type::Body.rich(s::NO_LAYER).color(p.text_secondary));
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add(Button::primary(s::DOWNLOAD).icon(icons::DOWNLOAD))
                                            .clicked()
                                        {
                                            state.onboarding.downloading = true;
                                            env.bridge.send(UiCommand::ProvisionRuntime {
                                                target: RuntimeRef::Default,
                                            });
                                        }
                                        if ui.add(Button::ghost(s::LATER)).clicked() {
                                            state.onboarding.later = true;
                                            next = true;
                                        }
                                    });
                                }
                            }
                        }
                        OnboardingStep::Handler => {
                            let ours = host.handler == HandlerState::EasyPlay;
                            ui.label(
                                Type::Body
                                    .rich(s::handler_now(&host.handler))
                                    .color(p.text_secondary),
                            );
                            ui.horizontal(|ui| {
                                if ours {
                                    ui.label(
                                        icons::icon(icons::CHECK, icon_size::BUTTON)
                                            .color(p.success),
                                    );
                                    ui.label(
                                        Type::BodyStrong.rich(s::IS_DEFAULT).color(p.text_primary),
                                    );
                                    if ui
                                        .add(
                                            Button::ghost(crate::strings::feedback::UNDO)
                                                .size(Size::Sm),
                                        )
                                        .clicked()
                                    {
                                        env.bridge
                                            .send(UiCommand::SetDefaultHandler { enable: false });
                                    }
                                } else if ui
                                    .add(Button::new(s::MAKE_DEFAULT).icon(icons::FILE_EXE))
                                    .clicked()
                                {
                                    env.bridge
                                        .send(UiCommand::SetDefaultHandler { enable: true });
                                }
                            });
                            ui.add_space(space::S4);
                            let h = ui
                                .label(Type::Heading.rich(s::ARTWORK_TITLE).color(p.text_primary));
                            crate::a11y::labelled(
                                ui.ctx(),
                                h.id,
                                egui::Role::Heading,
                                s::ARTWORK_TITLE,
                            );
                            ui.checkbox(&mut state.onboarding.artwork_online, s::ARTWORK_OPT_IN);
                            ui.add(
                                egui::Label::new(
                                    Type::Caption
                                        .rich(s::ARTWORK_DETAIL)
                                        .color(p.text_secondary),
                                )
                                .wrap(),
                            );
                        }
                    }
                    ui.add_space(space::S8);
                    let blocking =
                        step == OnboardingStep::SystemCheck && host.has_blocking_failure();
                    ui.horizontal(|ui| {
                        if step.index() > 0
                            && !reduced
                            && ui.add(Button::ghost(s::BACK).icon(icons::BACK)).clicked()
                        {
                            back = true;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let last = reduced || step == OnboardingStep::Handler;
                            let label = if last { s::FINISH } else { s::CONTINUE };
                            let reason = blocking.then_some(s::BLOCKED);
                            let c = ui.add(Button::primary(label).disabled(reason));
                            if ui.memory(|m| m.focused().is_none()) && !blocking {
                                c.request_focus();
                            }
                            if c.clicked() {
                                if last {
                                    done = true;
                                } else {
                                    next = true;
                                }
                            }
                            if step == OnboardingStep::SystemCheck
                                && ui
                                    .add(Button::new(s::SKIP).variant(Variant::Secondary))
                                    .clicked()
                            {
                                skip = true;
                            }
                            if let Some(r) = reason {
                                ui.label(Type::Caption.rich(r).color(p.danger));
                            }
                        });
                    });
                });
            });
        });
    if back {
        state.onboarding.step = OnboardingStep::ALL[step.index().saturating_sub(1)];
    }
    if next {
        let i = (step.index() + 1).min(OnboardingStep::ALL.len() - 1);
        state.onboarding.step = OnboardingStep::ALL[i];
        if state.onboarding.step == OnboardingStep::SystemCheck {
            env.bridge.send(UiCommand::RefreshHostFacts);
        }
    }
    if skip {
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
                text: s::BANNER.to_owned(),
                action: Some(s::CHECK_AGAIN.to_owned()),
                dismissible: true,
            });
        }
    }
    if done {
        let skipped = host.has_blocking_failure();
        finish(state, env, skipped);
    }
}
