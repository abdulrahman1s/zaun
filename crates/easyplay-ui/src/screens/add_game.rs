//! Add-game sheet (SCREEN_SPECS §5, UI-025, UF-4): programs found in a dropped or picked
//! folder, most likely first and pre-selected; radio selection, or checkboxes with *Add
//! several*; filtered files under a disclosure; *Choose a file…* opens the picker on its own
//! thread. Continue is available as soon as one candidate exists.

use egui::{Context, CornerRadius, Id, Key, Modifiers, Sense, Ui, vec2};

use super::Env;
use crate::app::UiState;
use crate::bridge::{OpenOrigin, UiCommand};
use crate::input::picker::PickPurpose;
use crate::route::Overlay;
use crate::strings;
use crate::strings::setup as s;
use crate::theme::tokens::{icon as icon_size, layout, radius, space};
use crate::theme::{self, Type, icons};
use crate::widgets::button::{Button, Size};
use crate::widgets::progress;
use crate::widgets::sheet::sheet;

fn rows(ui: &mut Ui, state: &mut UiState) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let Some(a) = state.add_game.as_mut() else {
        return;
    };
    let list: Vec<_> = a.rows().into_iter().cloned().collect();
    let multi = a.multi;
    let mut clicked = None;
    let mut focus_target = None;
    let group = egui::ScrollArea::vertical()
        .max_height(320.0)
        .show(ui, |ui| {
            for (i, prog) in list.iter().enumerate() {
                let file = prog
                    .path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                let role = s::role_guess(prog.role, prog.filtered);
                let on = a.chosen.contains(&i);
                let (rect, resp) = ui
                    .allocate_exact_size(vec2(ui.available_width(), layout::ROW_H), Sense::click());
                let id = resp.id;
                if resp.hovered() || resp.has_focus() || on {
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(radius::SM),
                        if on { p.accent_subtle } else { p.bg_raised },
                    );
                }
                if resp.has_focus() {
                    crate::widgets::focus_ring(
                        ui.painter(),
                        rect,
                        CornerRadius::same(radius::SM),
                        &theme,
                    );
                }
                let x = rect.left() + space::S3;
                let glyph = match (multi, on) {
                    (true, true) => icons::CHECK,
                    (true, false) => icons::CLOSE,
                    (false, _) => icons::FILE_EXE,
                };
                if !(multi && !on) {
                    ui.painter().text(
                        egui::pos2(x + 8.0, rect.center().y),
                        egui::Align2::CENTER_CENTER,
                        glyph.text(false),
                        icons::font(icon_size::INLINE),
                        if on { p.accent } else { p.text_secondary },
                    );
                } else {
                    let b = egui::Rect::from_center_size(
                        egui::pos2(x + 8.0, rect.center().y),
                        vec2(14.0, 14.0),
                    );
                    ui.painter().rect_stroke(
                        b,
                        CornerRadius::same(radius::XS),
                        egui::Stroke::new(1.0, p.border_strong),
                        egui::StrokeKind::Inside,
                    );
                }
                ui.painter().text(
                    egui::pos2(x + space::S8, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &file,
                    Type::BodyStrong.font_id(),
                    if prog.filtered {
                        p.text_secondary
                    } else {
                        p.text_primary
                    },
                );
                ui.painter().text(
                    egui::pos2(rect.right() - space::S3, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    s::role_size(role, prog.size),
                    Type::Body.numeric_font_id(),
                    p.text_muted,
                );
                let name = s::program_row(&file, role, prog.size);
                let role_kind = if multi {
                    egui::Role::CheckBox
                } else {
                    egui::Role::RadioButton
                };
                crate::a11y::option(&resp, role_kind, &name, on);
                if resp.clicked() || (resp.has_focus() && ui.input(|i| i.key_pressed(Key::Space))) {
                    clicked = Some(i);
                }
                if resp.has_focus() {
                    crate::widgets::own_arrow_keys(&resp);
                    let (up, down) = ui.input_mut(|i| {
                        (
                            i.consume_key(Modifiers::NONE, Key::ArrowUp),
                            i.consume_key(Modifiers::NONE, Key::ArrowDown),
                        )
                    });
                    if up && i > 0 {
                        focus_target = Some(i - 1);
                    }
                    if down && i + 1 < list.len() {
                        focus_target = Some(i + 1);
                    }
                }
                ui.data_mut(|d| d.insert_temp(Id::unique(("add.row", i)), id));
                // Initial focus: the pre-selected row.
                if i == 0
                    && ui.memory(|m| m.focused().is_none())
                    && !ui.data(|d| {
                        d.get_temp::<bool>(Id::unique("add.focused"))
                            .unwrap_or(false)
                    })
                {
                    resp.request_focus();
                    ui.data_mut(|d| d.insert_temp(Id::unique("add.focused"), true));
                }
            }
        });
    let gname = s::PROGRAMS;
    crate::a11y::labelled(
        ui.ctx(),
        group.id,
        if multi {
            egui::Role::List
        } else {
            egui::Role::RadioGroup
        },
        gname,
    );
    if let Some(t) = focus_target
        && let Some(id) = ui.data(|d| d.get_temp::<Id>(Id::unique(("add.row", t))))
    {
        ui.memory_mut(|m| m.request_focus(id));
        if !multi {
            a.choose(t);
        }
    }
    if let Some(i) = clicked {
        a.choose(i);
    }
}

/// Show the sheet.
pub fn show(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) {
    if state.add_game.is_none() {
        state.nav.close(Overlay::AddGame);
        return;
    }
    let mut cont = false;
    let mut cancel = false;
    let ready = state
        .add_game
        .as_ref()
        .is_some_and(|a| !a.chosen_paths().is_empty());
    let r = sheet(
        ctx,
        id,
        s::ADD_TITLE,
        false,
        |ui| {
            let p = theme::theme(ui.ctx()).palette.clone();
            let Some(a) = state.add_game.as_mut() else {
                return;
            };
            let folder = a.folder.file_name().map_or_else(
                || a.folder.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let scanning = a.scan.as_ref().is_none_or(|x| x.scanning);
            ui.horizontal(|ui| {
                ui.label(
                    Type::Heading
                        .rich(s::found_in(&folder))
                        .color(p.text_primary),
                );
                if scanning {
                    let (rr, _) = ui.allocate_exact_size(
                        vec2(icon_size::INLINE, icon_size::INLINE),
                        Sense::hover(),
                    );
                    progress::spinner(ui, rr.center(), icon_size::INLINE / 2.0, p.accent);
                    ui.label(Type::Caption.rich(s::LOOKING).color(p.text_secondary));
                }
            });
            if let Some(e) = a.scan.as_ref().and_then(|x| x.error.clone()) {
                super::details::permissions::issue_line(ui, &e, true);
            }
            let total = a.scan.as_ref().map_or(0, |x| x.programs.len());
            let filtered = a
                .scan
                .as_ref()
                .map_or(0, |x| x.programs.iter().filter(|p| p.filtered).count());
            if !scanning && total == 0 {
                ui.label(Type::Body.rich(s::NONE_FOUND).color(p.text_secondary));
            } else {
                ui.checkbox(&mut a.multi, s::ADD_SEVERAL);
                rows(ui, state);
                let Some(a) = state.add_game.as_mut() else {
                    return;
                };
                if filtered > 0
                    && ui
                        .add(
                            Button::ghost(if a.show_filtered {
                                s::HIDE_FILTERED
                            } else {
                                s::SHOW_FILTERED
                            })
                            .size(Size::Sm)
                            .icon(if a.show_filtered {
                                icons::CARET_DOWN
                            } else {
                                icons::CARET_RIGHT
                            }),
                        )
                        .clicked()
                {
                    a.show_filtered = !a.show_filtered;
                }
                if a.scan.as_ref().is_some_and(|x| x.truncated) {
                    ui.label(Type::Caption.rich(s::TRUNCATED).color(p.text_muted));
                }
            }
            ui.horizontal(|ui| {
                ui.label(Type::Body.rich(s::NOT_HERE).color(p.text_secondary));
                if ui
                    .add(
                        Button::new(s::CHOOSE_FILE)
                            .size(Size::Sm)
                            .icon(icons::FILE_EXE),
                    )
                    .clicked()
                {
                    state.picks.push(PickPurpose::AddGameFile);
                }
            });
        },
        |ui| {
            if ui
                .add(Button::primary(s::CONTINUE).disabled((!ready).then_some(s::NONE_FOUND)))
                .clicked()
            {
                cont = true;
            }
            if ui.add(Button::new(strings::dialogs::CANCEL)).clicked() {
                cancel = true;
            }
        },
    );
    // Enter continues from anywhere in the sheet.
    if !cont
        && ctx.input(|i| i.key_pressed(Key::Enter))
        && state
            .add_game
            .as_ref()
            .is_some_and(|a| !a.chosen_paths().is_empty())
    {
        cont = true;
    }
    if cont {
        let paths = state
            .add_game
            .as_ref()
            .map(|a| a.chosen_paths())
            .unwrap_or_default();
        env.bridge.send(UiCommand::OpenPaths {
            paths,
            origin: OpenOrigin::Picker,
        });
    }
    if cont || cancel || r.close_requested {
        crate::widgets::modal::closed(ctx, id);
        ctx.data_mut(|d| d.remove_temp::<bool>(Id::unique("add.focused")));
        state.add_game = None;
        state.nav.close(Overlay::AddGame);
    }
}
