//! "Add a game" (script `addGameDialog`; SCREEN_SPECS §5, UI-025, UF-4): the design's one
//! dialog in three steps, whose height morphs between them.
//!
//! 1. Choose a folder: a dashed drop area ("Drop the game's folder here / or pick it
//!    yourself / Choose folder…"). Shown while [`crate::vm::screens::AddGameVm`] has no
//!    folder; open it with [`open`]. Dropping a folder anywhere in the window scans it.
//! 2. Pick the program: the programs found in the folder as pick rows (file name in mono,
//!    kind and size in quiet text), most likely first and pre-selected; *Add several*
//!    switches the rows to checkboxes; uninstallers and helpers wait behind *Show N
//!    uninstallers and helpers*; *Choose a file instead…* opens the picker. *Back* /
//!    *Continue*.
//! 3. Set up this game: [`super::setup`], drawn as the same dialog ([`dialog_id`]).
//!
//! *Continue* asks the service to set the chosen programs up; the dialog stays (Continue
//! unavailable) until the setup arrives or something else answers (a question, a
//! message), and gives way after a few seconds without an answer.

use egui::{
    Align, Context, Id, Key, Layout, Modifiers, Rect, Sense, Shape, Stroke, Ui, UiBuilder, pos2,
    vec2,
};

use std::path::PathBuf;

use super::Env;
use crate::app::UiState;
use crate::bridge::{OpenOrigin, UiBridge, UiCommand};
use crate::input::picker::PickPurpose;
use crate::route::Overlay;
use crate::strings::setup as s;
use crate::theme::tokens::{control, overlay, radius, stroke};
use crate::theme::{self, Type};
use crate::vm::screens::AddGameVm;
use crate::widgets::banner::Note;
use crate::widgets::button::{Button, Variant};
use crate::widgets::choice::{PickKind, checkbox, pick};
use crate::widgets::modal::{Spec, closed, fine_print, show as dialog};
use crate::widgets::progress::spinner_widget;

/// The id of the add-a-game dialog, shared by its three steps (this screen and
/// [`super::setup`]) so that a step change morphs the panel instead of opening another.
pub fn dialog_id() -> Id {
    Id::unique("easyplay.add_a_game")
}

/// Where the programs of step 2 are kept while step 3 shows, so that *Back* returns to them.
pub(crate) fn stash_id() -> Id {
    dialog_id().with("stash")
}

/// When *Continue* asked for the setup (absent while nothing is awaited). The third step
/// clears it when it arrives.
pub(crate) fn pending_id() -> Id {
    dialog_id().with("pending")
}

/// Open the dialog at its first step (choose a folder).
pub fn open(state: &mut UiState) {
    state.add_game = Some(AddGameVm::default());
    if state.nav.top_overlay() != Some(Overlay::AddGame) {
        state.nav.open(Overlay::AddGame);
    }
}

/// Scan `folder` for its programs and show them: the dialog's second step, in place of the
/// first when the dialog is open (a folder chosen with *Choose folder…*, or dropped on the
/// window).
pub fn scan(state: &mut UiState, bridge: &UiBridge, folder: PathBuf) {
    let q = bridge.query_id();
    state.add_game = Some(AddGameVm {
        folder: folder.clone(),
        query: Some(q),
        ..Default::default()
    });
    if state.nav.top_overlay() != Some(Overlay::AddGame) {
        state.nav.open(Overlay::AddGame);
    }
    bridge.send(UiCommand::ScanFolder {
        folder,
        reply_to: q,
    });
}

/// Programs chosen with *Choose a file instead…*: they are set up as *Continue* sets up the
/// chosen rows, and *Back* in the next step returns to the programs of the folder.
pub fn picked_programs(ctx: &Context, state: &UiState, bridge: &UiBridge, paths: Vec<PathBuf>) {
    let open = state.nav.top_overlay() == Some(Overlay::AddGame);
    send(ctx, bridge, state.add_game.as_ref().filter(|_| open), paths);
}

/// Ask the service to set `paths` up. With `from` (the dialog's second step) the dialog
/// stays, *Continue* unavailable, until the setup arrives (it then morphs into step 3) or
/// something else answers; `from` is kept for *Back*.
fn send(ctx: &Context, bridge: &UiBridge, from: Option<&AddGameVm>, paths: Vec<PathBuf>) {
    if let Some(a) = from {
        let now = ctx.input(|i| i.time);
        ctx.data_mut(|d| {
            d.insert_temp(stash_id(), a.clone());
            d.insert_temp(pending_id(), now);
        });
    }
    bridge.send(UiCommand::OpenPaths {
        paths,
        origin: OpenOrigin::Picker,
    });
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The dashed drop area of step 1. Returns whether *Choose folder…* was clicked.
fn drop_area(ui: &mut Ui) -> bool {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width();
    let [side, vpad] = overlay::DROP_PAD;
    let gap = overlay::DROP_GAP;
    let top = ui.cursor().min;
    let b = Type::BodyBold.galley(ui, s::DROP_HERE(), p.t1, w - side * 2.0, usize::MAX);
    let o = Type::Body.galley(ui, s::OR_PICK(), p.t2, w - side * 2.0, usize::MAX);
    let mut y = top.y + vpad;
    let cx = top.x + w / 2.0;
    let bh = b.size().y;
    ui.painter().galley(pos2(cx - b.size().x / 2.0, y), b, p.t1);
    y += bh + gap;
    let oh = o.size().y;
    ui.painter().galley(pos2(cx - o.size().x / 2.0, y), o, p.t2);
    y += oh + gap + overlay::DROP_BUTTON_GAP;
    let bw = Button::new(s::CHOOSE_FOLDER()).width(ui);
    let br = Rect::from_min_size(pos2(cx - bw / 2.0, y), vec2(bw, control::BUTTON));
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt("add.choose")
            .max_rect(br)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let chosen = child.add(Button::new(s::CHOOSE_FOLDER())).clicked();
    y += control::BUTTON + vpad;
    let rect = Rect::from_min_max(top, pos2(top.x + w, y));
    ui.allocate_rect(rect, Sense::hover());
    // The dashed outline, 1.5 pt `line2`, radius 10.
    let points = rounded_outline(
        rect.shrink(stroke::FIELD_FOCUS / 2.0),
        f32::from(radius::POP),
    );
    let [dash, gap] = overlay::DROP_DASH;
    ui.painter().extend(Shape::dashed_line(
        &points,
        Stroke::new(stroke::FIELD_FOCUS, p.line2),
        dash,
        gap,
    ));
    chosen
}

/// The outline of a rounded rectangle as a closed list of points.
fn rounded_outline(r: Rect, rad: f32) -> Vec<egui::Pos2> {
    let mut pts = Vec::new();
    let steps = 6;
    let corners = [
        (pos2(r.right() - rad, r.top() + rad), -90.0_f32),
        (pos2(r.right() - rad, r.bottom() - rad), 0.0),
        (pos2(r.left() + rad, r.bottom() - rad), 90.0),
        (pos2(r.left() + rad, r.top() + rad), 180.0),
    ];
    pts.push(pos2(r.left() + rad, r.top()));
    for (c, start) in corners {
        for i in 0..=steps {
            let a = (start + 90.0 * i as f32 / steps as f32).to_radians();
            pts.push(pos2(c.x + rad * a.cos(), c.y + rad * a.sin()));
        }
    }
    pts.push(pos2(r.left() + rad, r.top()));
    pts
}

/// The pick rows of step 2.
fn rows(ui: &mut Ui, a: &mut AddGameVm) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let list: Vec<_> = a.rows().into_iter().cloned().collect();
    let kind = if a.multi {
        PickKind::Check
    } else {
        PickKind::Radio
    };
    let mut clicked = None;
    for (i, prog) in list.iter().enumerate() {
        let file = file_name(&prog.path);
        let role = s::role_guess(prog.role, prog.filtered);
        let on = a.chosen.contains(&i);
        let spoken = s::program_row(&file, role, prog.size);
        let (r, ()) = pick(ui, kind, on, &spoken, |ui| {
            let w = ui.available_width();
            let h = Type::Body.line_height();
            let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
            let meta = Type::Body.numeric_galley(ui, &s::role_size(role, prog.size), p.t3);
            let mw = meta.size().x;
            let name = Type::Mono.galley(ui, &file, p.t1, (w - mw - control::PICK_GAP).max(1.0), 1);
            let cy = rect.center().y;
            ui.painter()
                .galley(pos2(rect.left(), cy - name.size().y / 2.0), name, p.t1);
            ui.painter().galley(
                pos2(rect.right() - mw, cy - meta.size().y / 2.0),
                meta,
                p.t3,
            );
        });
        if r.clicked() {
            clicked = Some(i);
        }
    }
    if let Some(i) = clicked {
        a.choose(i);
    }
}

/// Show the dialog's first two steps.
pub fn show(ctx: &Context, _id: Id, state: &mut UiState, env: &mut Env<'_>) {
    let id = dialog_id();
    let Some(mut a) = state.add_game.take() else {
        state.nav.close(Overlay::AddGame);
        ctx.data_mut(|d| d.remove_temp::<f64>(pending_id()));
        return;
    };
    // Something else answered Continue (a question, a message): this dialog is done.
    let now = ctx.input(|i| i.time);
    let pending: Option<f64> = ctx.data(|d| d.get_temp(pending_id()));
    let covered = state
        .nav
        .top_overlay()
        .is_some_and(|o| o != Overlay::AddGame);
    if let Some(since) = pending
        && (covered || now - since > f64::from(overlay::ADD_WAIT))
    {
        ctx.data_mut(|d| {
            d.remove_temp::<f64>(pending_id());
            d.remove_temp::<AddGameVm>(stash_id());
        });
        closed(ctx, id);
        state.nav.close(Overlay::AddGame);
        return;
    }
    let choosing = a.scan.is_none() && a.query.is_none() && a.folder.as_os_str().is_empty();
    let folder = file_name(&a.folder);
    let text = if choosing {
        s::CHOOSE_TEXT().to_owned()
    } else {
        s::found_text(&folder)
    };
    let scanning = !choosing && a.scan.as_ref().is_none_or(|x| x.scanning);
    let ready = !a.chosen_paths().is_empty();
    let busy = pending.is_some();
    let mut picks = Vec::new();
    let mut back = false;
    // A control inside the body that acted this frame (it took the Enter that pressed it).
    let mut acted = false;
    let r = dialog(
        ctx,
        id,
        Spec::new(s::ADD_TITLE())
            .text(&text)
            .wide()
            .close_button()
            .step(u64::from(!choosing))
            .with_actions(!choosing),
        |ui| {
            if choosing {
                if drop_area(ui) {
                    picks.push(PickPurpose::AddGameFolder);
                }
                return;
            }
            let p = theme::theme(ui.ctx()).palette.clone();
            if let Some(e) = a.scan.as_ref().and_then(|x| x.error.clone()) {
                Note::new(crate::widgets::banner::Severity::Danger, &e).show(ui);
                ui.add_space(overlay::PRE[0]);
            }
            let total = a.scan.as_ref().map_or(0, |x| x.programs.len());
            let filtered = a
                .scan
                .as_ref()
                .map_or(0, |x| x.programs.iter().filter(|p| p.filtered).count());
            if scanning && total == 0 {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = overlay::DIALOG_ACTION_GAP;
                    spinner_widget(ui, p.t2);
                    ui.label(Type::Body.rich(s::LOOKING()).color(p.t2));
                });
            } else if total == 0 {
                ui.label(Type::Body.rich(s::NONE_FOUND()).color(p.t2));
            } else {
                rows(ui, &mut a);
            }
            ui.add_space(overlay::PRE[0]);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                if total > 0 {
                    acted |= checkbox(ui, &mut a.multi, s::ADD_SEVERAL()).clicked();
                }
                if filtered > 0 {
                    let label = if a.show_filtered {
                        s::HIDE_FILTERED().to_owned()
                    } else {
                        s::show_filtered(filtered)
                    };
                    if ui.add(Button::ghost(&label)).clicked() {
                        a.show_filtered = !a.show_filtered;
                        acted = true;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(Button::ghost(s::CHOOSE_FILE())).clicked() {
                        picks.push(PickPurpose::AddGameFile);
                        acted = true;
                    }
                });
            });
            if scanning && total > 0 {
                ui.add_space(overlay::PRE[0]);
                fine_print(ui, s::LOOKING());
            }
            if a.scan.as_ref().is_some_and(|x| x.truncated) {
                ui.add_space(overlay::PRE[0]);
                fine_print(ui, s::TRUNCATED());
            }
        },
        |ui| {
            let go = ui
                .add(Button::primary(s::CONTINUE()).disabled(if busy {
                    Some(s::SETTING_UP())
                } else {
                    (!ready).then_some(s::NONE_FOUND())
                }))
                .clicked();
            back = ui
                .add(Button::new(s::BACK()).variant(Variant::Secondary))
                .clicked();
            go
        },
    );
    state.picks.extend(picks);
    // Enter continues from anywhere in the dialog, unless it pressed a control.
    let enter = !choosing
        && ready
        && !busy
        && !acted
        && !back
        && !r.close_requested
        && !r.footer
        && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
    if (r.footer || enter) && !busy && ready {
        send(ctx, env.bridge, Some(&a), a.chosen_paths());
    }
    if back {
        a = AddGameVm::default();
    }
    if r.close_requested {
        ctx.data_mut(|d| {
            d.remove_temp::<f64>(pending_id());
            d.remove_temp::<AddGameVm>(stash_id());
        });
        state.nav.close(Overlay::AddGame);
        return;
    }
    state.add_game = Some(a);
}
