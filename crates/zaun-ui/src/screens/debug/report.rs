//! The diagnostics report (script `reportDialog`; ERROR_HANDLING §7, PRIVACY §5; ERR-003): a
//! wide dialog that says what the report is, then one row per file (its name in mono, its
//! size, *View* / *Hide*, which opens the file's text in a detail block while the panel's
//! height morphs), the fine print of what is never included, and *Cancel* / *Save report…*
//! (the system save dialog; the service writes the archive).

use egui::{Align, Context, Id, Layout, Rect, Sense, Stroke, UiBuilder, pos2, vec2};

use super::super::Env;
use crate::app::UiState;
use crate::bridge::Command;
use crate::input::picker::PickPurpose;
use crate::route::Overlay;
use crate::strings::debug as s;
use crate::strings::dialogs as d;
use crate::theme::tokens::{control, layout, overlay, stroke};
use crate::theme::{self, Type};
use crate::widgets::button::{Button, Variant};
use crate::widgets::modal::{Spec, detail_block, fine_print, show as dialog};

/// Most lines of a file shown in its detail block.
const MAX_LINES: usize = 2_000;

/// Show the dialog (overlay `Report`).
pub fn show(ctx: &Context, id: Id, state: &mut UiState, _env: &mut Env<'_>) {
    let Some(mut vm) = state.debug.report.take() else {
        state.nav.close(Overlay::Report);
        return;
    };
    let ready = vm.files.is_some();
    let r = dialog(
        ctx,
        id,
        Spec::new(s::REPORT())
            .text(d::REPORT_TEXT())
            .wide()
            .close_button(),
        |ui| {
            let theme = theme::theme(ui.ctx());
            let p = theme.palette.clone();
            if let Some(e) = &vm.error {
                ui.label(Type::Body.rich(&e.message).color(p.t2));
                return;
            }
            let Some(files) = &vm.files else {
                fine_print(ui, s::REPORT_BUILDING());
                return;
            };
            for (i, f) in files.iter().enumerate() {
                let open = vm.viewing == Some(i);
                let w = ui.available_width();
                let (rect, _) = ui.allocate_exact_size(vec2(w, layout::TABLE_ROW), Sense::hover());
                let cy = rect.center().y;
                // The button reaches past the column by its padding, so its label ends on
                // the column's edge.
                let label = if open { s::HIDE } else { s::VIEW };
                let bw = Button::new(label()).variant(Variant::Ghost).width(ui);
                let b_rect = Rect::from_min_size(
                    pos2(
                        rect.right() + control::BUTTON_PAD - bw,
                        cy - control::BUTTON / 2.0,
                    ),
                    vec2(bw, control::BUTTON),
                );
                let mut b = ui.new_child(
                    UiBuilder::new()
                        .id_salt(("report-view", i))
                        .max_rect(b_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                let clicked = b
                    .add(
                        Button::new(label())
                            .variant(Variant::Ghost)
                            .accessible_name(format!("{} {}", label(), f.name)),
                    )
                    .clicked();
                if clicked {
                    vm.viewing = if open { None } else { Some(i) };
                }
                let size = Type::Body.numeric_galley(ui, &s::bytes(f.bytes.len() as u64), p.t2);
                let size_x = b_rect.left() - layout::LIST_COL_GAP - size.size().x;
                let name_w = (size_x - layout::LIST_COL_GAP - rect.left()).max(1.0);
                let name = Type::Mono.galley(ui, &f.name, p.t1, name_w, 1);
                let painter = ui.painter();
                painter.galley(pos2(rect.left(), cy - name.size().y / 2.0), name, p.t1);
                painter.galley(pos2(size_x, cy - size.size().y / 2.0), size, p.t2);
                painter.hline(
                    rect.x_range(),
                    rect.bottom() - stroke::BORDER / 2.0,
                    Stroke::new(stroke::BORDER, p.line),
                );
                if vm.viewing == Some(i) {
                    let text: String = String::from_utf8_lossy(&f.bytes)
                        .lines()
                        .take(MAX_LINES)
                        .map(|l| crate::vm::debug::sanitize(l.as_bytes()))
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.add_space(overlay::PRE[0] - overlay::DIALOG_BODY_PAD[1]);
                    detail_block(ui, &text, false);
                    ui.add_space(overlay::PRE[0]);
                }
            }
            ui.add_space(overlay::BLANK_FINE_GAP - overlay::DIALOG_BODY_PAD[1]);
            fine_print(ui, s::REPORT_EXCLUDES());
        },
        |ui| {
            let save = ui
                .add(
                    Button::primary(s::SAVE_REPORT())
                        .disabled((!ready).then_some(s::REPORT_BUILDING())),
                )
                .clicked();
            let cancel = ui.add(Button::new(d::CANCEL())).clicked();
            (save, cancel)
        },
    );
    let (save, cancel) = r.footer;
    if save {
        state.picks.push(PickPurpose::SaveReport);
    }
    if r.close_requested || cancel {
        if cancel {
            crate::widgets::modal::closed(ctx, id);
        }
        state.nav.close(Overlay::Report);
    } else {
        state.debug.report = Some(vm);
    }
}

/// The save dialog returned a destination.
pub fn save_to(state: &mut UiState, bridge: &crate::bridge::UiBridge, dest: std::path::PathBuf) {
    if let Some(files) = state.debug.report.as_ref().and_then(|r| r.files.clone()) {
        bridge.send(Command::SaveReport { files, dest });
        state.debug.report = None;
        state.nav.close(Overlay::Report);
    }
}
