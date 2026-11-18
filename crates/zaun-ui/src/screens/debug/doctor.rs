//! The system check (ERROR_HANDLING §6; ERR-002): one row per check of this computer, in the
//! order the host gives them. A row is aligned to its top: a 16 pt glyph (a quiet check when
//! fine, a coral cross when it fails, an amber warning), the check's name with its result at
//! the right in quiet text, and, when there is something to do, help under it and the command
//! to run as a selectable line with *Copy*. While a check runs, its result is a spinner.
//!
//! The same rows make the welcome steps' system check and the group at the top of Settings →
//! Advanced, which adds *Check again* and *Create a report…*.

use egui::{Align, Align2, CornerRadius, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use super::super::Env;
use crate::app::UiState;
use crate::bridge::{CheckState, Command, HostCheck};
use crate::strings::debug as s;
use crate::theme::tokens::{control, icon as icon_size, layout, radius};
use crate::theme::{self, Type, icons};
use crate::vm::toasts::ToastKind;
use crate::widgets::button::{Button, Variant};
use crate::widgets::progress;

/// What the rows were asked to do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckOutcome {
    /// *Use umu's runtime* was pressed (the runtime check's fix).
    pub adopt: bool,
    /// A command was copied to the clipboard.
    pub copied: bool,
}

/// How the runtime check's fix stands: umu's runtime can be adopted, and whether it is
/// being copied now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Adopt {
    /// umu installed a runtime the store lacks.
    pub available: bool,
    /// It is being copied.
    pub busy: bool,
}

/// The ID of the check whose fix is adopting umu's runtime.
const RUNTIME_CHECK: &str = "D-15";

/// The rows of `checks`, in their order.
pub fn check_rows(ui: &mut Ui, checks: &[HostCheck], adopt: Adopt) -> CheckOutcome {
    let mut out = CheckOutcome::default();
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for c in checks {
            check_row(ui, c, adopt, &mut out);
        }
    });
    out
}

fn check_row(ui: &mut Ui, c: &HostCheck, adopt: Adopt, out: &mut CheckOutcome) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let top = ui.cursor().min;
    let w = ui.available_width();
    let pad = layout::FACT_PAD_Y;
    let text_x = top.x + icon_size::INLINE + layout::CHECK_GLYPH_GAP;
    let text_w = (w - (text_x - top.x)).max(1.0);
    let mut col = ui.new_child(
        UiBuilder::new()
            .id_salt(("check", c.id.as_str()))
            .max_rect(Rect::from_min_size(
                pos2(text_x, top.y + pad),
                vec2(text_w, f32::INFINITY),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    col.spacing_mut().item_spacing.y = 0.0;
    // The name, and the result at the right (or the spinner while the check runs).
    let line_h = Type::Body.line_height();
    let (line, _) = col.allocate_exact_size(vec2(text_w, line_h), Sense::hover());
    let pending = c.state == CheckState::Pending;
    let result_w = if pending {
        progress::spinner(
            &col,
            pos2(line.right() - control::SPINNER / 2.0, line.center().y),
            control::SPINNER / 2.0,
            p.t3,
        );
        control::SPINNER
    } else {
        let g = Type::Body.galley(
            &col,
            &c.result,
            p.t3,
            (text_w * layout::CHECK_RESULT_SHARE).max(1.0),
            1,
        );
        let gw = g.size().x;
        col.painter().galley(
            pos2(line.right() - gw, line.center().y - g.size().y / 2.0),
            g,
            p.t3,
        );
        gw
    };
    let name_w = (text_w - result_w - layout::CHECK_RESULT_GAP).max(1.0);
    let name = Type::Body.galley(&col, &c.name, p.t1, name_w, 1);
    col.painter().galley(
        pos2(line.left(), line.center().y - name.size().y / 2.0),
        name,
        p.t1,
    );
    // What to do about it.
    if c.state != CheckState::Pass && !pending {
        // The explanation, ending with the lead-in to the command when there is one.
        let help = match (&c.detail, &c.fix_command) {
            (Some(d), Some(_)) if !d.trim_end().ends_with(':') => {
                Some(format!("{d} {}", s::TO_FIX()))
            }
            (Some(d), _) => Some(d.clone()),
            (None, Some(_)) => Some(s::TO_FIX().to_owned()),
            (None, None) => None,
        };
        if let Some(h) = help {
            let g = Type::Caption.galley(&col, &h, p.t3, text_w, usize::MAX);
            let (r, _) = col.allocate_exact_size(vec2(text_w, g.size().y), Sense::hover());
            col.painter().galley(r.min, g, p.t3);
        }
        if c.id == RUNTIME_CHECK && adopt.available {
            col.add_space(layout::CODE_GAP);
            if adopt.busy {
                col.horizontal(|ui| {
                    progress::spinner_widget(ui, p.t3);
                    ui.label(Type::Body.rich(s::ADOPTING()).color(p.t2));
                });
            } else if col
                .add(Button::new(s::USE_UMU_RUNTIME()).variant(Variant::Secondary))
                .clicked()
            {
                out.adopt = true;
            }
        } else if let Some(cmd) = &c.fix_command {
            col.add_space(layout::CODE_GAP);
            if code_line(&mut col, cmd) {
                out.copied = true;
            }
        }
    }
    let content_h = col.min_rect().height();
    let row_h = (content_h + pad * 2.0).max(control::SETTING_ROW);
    let row = Rect::from_min_size(top, vec2(w, row_h));
    let resp = ui.allocate_rect(row, Sense::hover());
    crate::a11y::labelled(
        ui.ctx(),
        resp.id,
        egui::Role::ListItem,
        &crate::strings::onboarding::check_name(c),
    );
    // The glyph, level with the name.
    let glyph = match c.state {
        CheckState::Pass => Some((icons::CHECK_CIRCLE, p.t3)),
        CheckState::Fail => Some((icons::ERROR, p.coral)),
        CheckState::Warn => Some((icons::WARNING, p.amber)),
        CheckState::Pending => None,
    };
    if let Some((icon, color)) = glyph {
        ui.painter().text(
            pos2(
                top.x + icon_size::INLINE / 2.0,
                top.y + pad + line_h / 2.0 + layout::CHECK_GLYPH_DROP,
            ),
            Align2::CENTER_CENTER,
            icon.text(false),
            icons::font(icon_size::INLINE),
            color,
        );
    }
}

/// A command to run: a faint plate, the command in mono (selectable), *Copy* at its right.
/// True when it was copied.
fn code_line(ui: &mut Ui, cmd: &str) -> bool {
    let p = theme::theme(ui.ctx()).palette.clone();
    let [pad_left, pad] = layout::CODE_PAD;
    let mut copied = false;
    egui::Frame::NONE
        .fill(p.o1)
        .corner_radius(CornerRadius::same(radius::ROW))
        .inner_margin(egui::Margin {
            left: pad_left as i8,
            right: pad as i8,
            top: pad as i8,
            bottom: pad as i8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let b = Button::ghost(s::COPY()).accessible_name(s::COPY_COMMAND());
                let bw = b.width(ui);
                let text_w = (ui.available_width() - bw - ui.spacing().item_spacing.x).max(1.0);
                ui.allocate_ui_with_layout(
                    vec2(text_w, control::BUTTON),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        egui::ScrollArea::horizontal()
                            .id_salt(("code", cmd))
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(Type::Mono.rich(cmd).color(p.t1))
                                        .selectable(true)
                                        .extend(),
                                );
                            });
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(b).clicked() {
                        ui.ctx().copy_text(cmd.to_owned());
                        copied = true;
                    }
                });
            });
        });
    copied
}

/// The group at the top of Settings → Advanced: the checks, then *Check again* and *Create a
/// report…*.
pub fn section(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    crate::widgets::rows::heading(ui, s::DOCTOR(), true);
    let host = env.bridge.host().clone();
    if host.checks.is_empty() {
        ui.label(Type::Body.rich(s::DOCTOR_PENDING()).color(p.t3));
    }
    let rt = env.bridge.runtimes().clone();
    if rt.adoptable.is_empty() {
        state.onboarding.adopting = false;
    }
    let out = check_rows(
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
    if out.copied {
        state.toast(
            crate::strings::feedback::COPIED(),
            ToastKind::Success,
            None,
            env.mono,
        );
    }
    ui.add_space(theme::tokens::space::S2);
    super::super::statistics::on_axis(ui, |ui| {
        if ui.add(Button::ghost(s::CHECK_AGAIN())).clicked() {
            env.bridge.send(Command::RefreshHostFacts);
            env.bridge.send(Command::RunDoctor);
        }
        if ui.add(Button::ghost(s::CREATE_REPORT())).clicked() {
            super::open_report(state, env, None, None);
        }
    });
}
