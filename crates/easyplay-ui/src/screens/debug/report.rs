//! The diagnostics report preview (ERROR_HANDLING §7, PRIVACY §5; ERR-003): what the report
//! contains, each file with its size and a *View* action, then *Save report…* (the system
//! save dialog; the service writes the archive).

use egui::{Context, Id, ScrollArea};

use super::super::Env;
use crate::app::UiState;
use crate::input::picker::PickPurpose;
use crate::route::Overlay;
use crate::strings::debug as s;
use crate::theme::tokens::space;
use crate::theme::{self, Type};
use crate::widgets::button::Button;
use crate::widgets::sheet::sheet;

/// Show the sheet (overlay `Report`).
pub fn show(ctx: &Context, id: Id, state: &mut UiState, _env: &mut Env<'_>) {
    let Some(mut vm) = state.debug.report.take() else {
        state.nav.close(Overlay::Report);
        return;
    };
    let r = sheet(
        ctx,
        id,
        s::REPORT,
        false,
        |ui| {
            let p = theme::theme(ui.ctx()).palette.clone();
            ui.label(Type::Body.rich(s::REPORT_INTRO).color(p.text_secondary));
            ui.add_space(space::S1);
            ui.label(Type::Caption.rich(s::REPORT_EXCLUDES).color(p.text_muted));
            ui.add_space(space::S3);
            if let Some(e) = &vm.error {
                ui.label(Type::Body.rich(&e.message).color(p.text_secondary));
                return;
            }
            let Some(files) = &vm.files else {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(Type::Body.rich(s::REPORT_BUILDING).color(p.text_secondary));
                });
                return;
            };
            let total = easyplay_core::diagnostics::report::total_size(files);
            ui.label(
                Type::Caption
                    .rich(s::report_total(files.len(), total))
                    .color(p.text_secondary),
            );
            for (i, f) in files.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(Type::Mono.rich(&f.name).color(p.text_primary));
                    ui.label(
                        Type::Caption
                            .numeric(s::bytes(f.bytes.len() as u64))
                            .color(p.text_secondary),
                    );
                    let open = vm.viewing == Some(i);
                    let label = if open { s::HIDE } else { s::VIEW };
                    if ui
                        .add(Button::ghost(label).accessible_name(format!("{label} {}", f.name)))
                        .clicked()
                    {
                        vm.viewing = if open { None } else { Some(i) };
                    }
                });
                if vm.viewing == Some(i) {
                    let text = String::from_utf8_lossy(&f.bytes);
                    egui::Frame::NONE
                        .fill(p.bg_sunken)
                        .inner_margin(egui::Margin::same(space::S2 as i8))
                        .show(ui, |ui| {
                            ScrollArea::both()
                                .id_salt(("report-file", i))
                                .max_height(240.0)
                                .show(ui, |ui| {
                                    for line in text.lines().take(2_000) {
                                        ui.add(
                                            egui::Label::new(
                                                Type::Mono
                                                    .rich(crate::vm::debug::sanitize(
                                                        line.as_bytes(),
                                                    ))
                                                    .color(p.text_primary),
                                            )
                                            .selectable(true)
                                            .extend(),
                                        );
                                    }
                                });
                        });
                }
            }
        },
        |ui| {
            ui.add(
                Button::primary(s::SAVE_REPORT)
                    .disabled(vm.files.is_none().then_some(s::REPORT_BUILDING)),
            )
            .clicked()
        },
    );
    if r.footer {
        state.picks.push(PickPurpose::SaveReport);
    }
    if r.close_requested {
        state.nav.close(Overlay::Report);
    } else {
        state.debug.report = Some(vm);
    }
}

/// The save dialog returned a destination.
pub fn save_to(state: &mut UiState, bridge: &crate::bridge::UiBridge, dest: std::path::PathBuf) {
    if let Some(files) = state.debug.report.as_ref().and_then(|r| r.files.clone()) {
        bridge.send(crate::bridge::UiCommand::SaveReport { files, dest });
        state.debug.report = None;
        state.nav.close(Overlay::Report);
    }
}
