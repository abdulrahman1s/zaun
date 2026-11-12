//! The Doctor in Settings → Advanced (ERROR_HANDLING §6; ERR-002): every check D-01…D-17
//! of the last full report, failures and warnings first with their fix, *Check again*,
//! *Create a report…*. The same rows feed the onboarding system check.

use egui::Ui;

use super::super::Env;
use crate::app::UiState;
use crate::bridge::{CheckState, UiCommand};
use crate::strings::debug as s;
use crate::theme::tokens::space;
use crate::theme::{self, Type, icons};
use crate::vm::toasts::ToastKind;
use crate::widgets::button::Button;

/// The section.
pub fn section(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.add_space(space::S4);
    ui.label(Type::Heading.rich(s::DOCTOR).color(p.text_primary));
    ui.label(Type::Caption.rich(s::DOCTOR_HELP).color(p.text_secondary));
    ui.add_space(space::S2);
    let host = env.bridge.host().clone();
    let full = env.bridge.diagnostics().doctor.is_some();
    if !full {
        ui.label(Type::Body.rich(s::DOCTOR_PENDING).color(p.text_secondary));
    }
    let mut rows: Vec<_> = host.checks.iter().collect();
    rows.sort_by_key(|c| match c.state {
        CheckState::Fail => 0,
        CheckState::Warn => 1,
        CheckState::Pending => 2,
        CheckState::Pass => 3,
    });
    for c in rows {
        let (icon, color) = match c.state {
            CheckState::Pass => (icons::CHECK, p.success),
            CheckState::Warn => (icons::WARNING, p.warning),
            CheckState::Fail => (icons::ERROR, p.danger),
            CheckState::Pending => (icons::CLOCK, p.text_muted),
        };
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(icon.text(true))
                    .font(icons::font(16.0))
                    .color(color),
            );
            ui.label(Type::BodyStrong.rich(&c.name).color(p.text_primary));
            ui.label(Type::Caption.rich(&c.id).color(p.text_muted));
        });
        ui.indent(("doctor", c.id.as_str()), |ui| {
            ui.add(egui::Label::new(Type::Body.rich(&c.result).color(p.text_secondary)).wrap());
            if c.state != CheckState::Pass {
                if let Some(d) = &c.detail {
                    ui.add(egui::Label::new(Type::Body.rich(d).color(p.text_primary)).wrap());
                }
                if let Some(cmd) = &c.fix_command {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(Type::Mono.rich(cmd).color(p.text_primary))
                                .selectable(true),
                        );
                        if ui.add(Button::ghost(s::COPY_COMMAND)).clicked() {
                            env.ctx.copy_text(cmd.clone());
                            state.toast(
                                crate::strings::feedback::COPIED,
                                ToastKind::Success,
                                None,
                                env.mono,
                            );
                        }
                    });
                }
            }
        });
        ui.add_space(space::S1);
    }
    ui.add_space(space::S2);
    ui.horizontal(|ui| {
        if ui.add(Button::new(s::CHECK_AGAIN)).clicked() {
            env.bridge.send(UiCommand::RefreshHostFacts);
            env.bridge.send(UiCommand::RunDoctor);
        }
        if ui.add(Button::new(s::CREATE_REPORT)).clicked() {
            super::open_report(state, env, None, None);
        }
    });
}
