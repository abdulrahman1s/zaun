//! The picture of *Your profile* and of a group (ADR-0038; chat-calls.html C "YOUR PROFILE"):
//! the 64 row (the avatar · 16 · *Choose a picture…* and *Remove picture*, or *Change colour*
//! without a picture, over a status line), the crop step as a step of the same dialog, and
//! the group's own dialog for its owner. Nothing is sent until Save.

use egui::epaint::{Brush, RectShape};
use egui::{Color32, Context, Id, Rect, Response, Sense, Ui, pos2, vec2};
use zaun_core::model::Picture;

use super::avatar::{self, Shape};
use super::{Env, crop, paint};
use crate::app::UiState;
use crate::bridge::Command;
use crate::input::picker::PickPurpose;
use crate::strings::chat as c;
use crate::strings::chat::picture as s;
use crate::theme::{self, Type};
use crate::vm::chat_picture::{PictureChange, PictureEdit, Placed};
use crate::widgets::button::{Button, Variant};
use crate::widgets::modal::{self, Spec};

mod m {
    pub const AVATAR: f32 = 64.0;
    pub const GAP: f32 = 16.0;
    /// The buttons: from the row's top, between them.
    pub const BUTTONS: [f32; 2] = [6.0, 6.0];
    /// The status line: under the buttons, its height, its spinner, the gap after that.
    pub const STATUS: [f32; 4] = [6.0, 16.0, 12.0, 6.0];
    /// The white spinner over the avatar while preparing, and the avatar's dimming.
    pub const BUSY: [f32; 2] = [16.0, 0.5];
    /// Preparing shows only once it has taken this long (s).
    pub const PREPARING_AFTER: f64 = 0.150;
}

fn rtl() -> bool {
    crate::i18n::dir() == crate::i18n::Dir::Rtl
}

/// The row. `colour`: offer *Change colour* when there is no picture (Your profile; a
/// group's colour comes from its name). Returns whether *Change colour* was clicked.
#[allow(clippy::too_many_arguments)]
pub(super) fn row(
    ui: &mut Ui,
    picks: &mut Vec<PickPurpose>,
    edit: &mut PictureEdit,
    name: &str,
    paper: Option<u8>,
    shape: Shape,
    current: Option<&Picture>,
    colour: bool,
) -> bool {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width();
    let (row, resp) = ui.allocate_exact_size(vec2(w, m::AVATAR), Sense::hover());
    let at = |x: f32, width: f32| {
        if rtl() {
            row.right() - x - width
        } else {
            row.left() + x
        }
    };
    let av = Rect::from_min_size(
        pos2(at(0.0, m::AVATAR), row.top()),
        vec2(m::AVATAR, m::AVATAR),
    );
    let spoken = match shape {
        Shape::Person => c::details::YOUR_AVATAR(),
        Shape::Group => s::GROUP_TITLE(),
    };
    crate::a11y::item(&resp, egui::Role::Image, spoken);
    let now = ui.input(|i| i.time);
    let busy = edit.loading.is_some();
    let preparing = edit
        .loading
        .is_some_and(|(_, since)| now - since >= m::PREPARING_AFTER);
    if busy && !preparing {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(m::PREPARING_AFTER));
    }
    face(ui, av, edit, name, paper, shape, current, preparing);
    let has_picture = match &edit.change {
        PictureChange::Set(_) => true,
        PictureChange::Remove => false,
        PictureChange::Keep => current.is_some(),
    };
    let choose = Button::new(s::CHOOSE()).disabled(busy.then_some(s::PREPARING()));
    let second = if has_picture {
        Some(Button::new(s::REMOVE()))
    } else {
        colour.then(|| Button::new(c::details::CHANGE_COLOUR()))
    };
    let second = second.map(|b| {
        b.variant(Variant::Ghost)
            .disabled(busy.then_some(s::PREPARING()))
    });
    let top = row.top() + m::BUTTONS[0];
    let h = theme::tokens::control::BUTTON;
    let mut x = m::AVATAR + m::GAP;
    let cw = choose.width(ui);
    let chose = place(ui, at(x, cw), top, cw, h, choose);
    crate::widgets::tooltip::attach(chose.clone(), s::LIMITS());
    if chose.clicked() {
        edit.error = None;
        picks.push(PickPurpose::ChatAvatar(edit.whose.flatten()));
    }
    x += cw + m::BUTTONS[1];
    let mut change_colour = false;
    if let Some(b) = second {
        let bw = b.width(ui);
        if place(ui, at(x, bw), top, bw, h, b).clicked() {
            if has_picture {
                edit.change = PictureChange::Remove;
            } else {
                change_colour = true;
            }
        }
    }
    status(
        ui,
        &p,
        top + h + m::STATUS[0],
        m::AVATAR + m::GAP,
        row,
        edit,
        preparing,
    );
    change_colour
}

fn place(ui: &mut Ui, x: f32, top: f32, w: f32, h: f32, b: Button<'_>) -> Response {
    paint::place(ui, Rect::from_min_size(pos2(x, top), vec2(w, h)), b)
}

/// The status line: "Preparing your picture…" with a spinner, or why the file could not be
/// used (the error catalogue's title; its message is the tooltip).
fn status(
    ui: &mut Ui,
    p: &theme::palette::Palette,
    y: f32,
    from: f32,
    row: Rect,
    edit: &PictureEdit,
    preparing: bool,
) {
    let [_, line, spin, gap] = m::STATUS;
    let width = row.width() - from;
    let line_rect = |x: f32, w: f32| {
        let left = if rtl() {
            row.right() - x - w
        } else {
            row.left() + x
        };
        Rect::from_min_size(pos2(left, y), vec2(w, line))
    };
    if preparing {
        let r = line_rect(from, spin);
        crate::widgets::progress::spinner(ui, r.center(), spin / 2.0, p.t3);
        let g = Type::Caption.galley(ui, s::PREPARING(), p.t3, width - spin - gap, 1);
        let r = line_rect(from + spin + gap, g.size().x);
        ui.painter()
            .galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, p.t3);
    } else if let Some(e) = &edit.error {
        let g = Type::Caption.galley(ui, &e.title, p.coral, width, 1);
        let r = line_rect(from, g.size().x);
        let resp = ui.interact(r, Id::unique("chat.picture.error"), Sense::hover());
        ui.painter()
            .galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, p.coral);
        crate::a11y::item(&resp, egui::Role::Label, &e.title);
        crate::widgets::tooltip::attach(resp, &e.message);
    }
}

/// The avatar as Save would leave it: the new crop, the initials, or the picture now.
#[allow(clippy::too_many_arguments)]
fn face(
    ui: &Ui,
    r: Rect,
    edit: &PictureEdit,
    name: &str,
    paper: Option<u8>,
    shape: Shape,
    current: Option<&Picture>,
    preparing: bool,
) {
    let painter = ui.painter();
    match &edit.change {
        PictureChange::Set(placed) => {
            avatar::avatar(painter, r, name, paper, shape);
            if let Some(tex) = super::pictures::whole(ui.ctx(), &placed.source.preview) {
                let mut fill = RectShape::filled(r, avatar::corner(r, shape), Color32::WHITE);
                fill.brush = Some(std::sync::Arc::new(Brush {
                    fill_texture_id: tex,
                    uv: square_uv(placed),
                }));
                painter.add(fill);
            }
        }
        PictureChange::Remove => avatar::avatar(painter, r, name, paper, shape),
        PictureChange::Keep => avatar::pictured(painter, r, name, paper, shape, current),
    }
    if preparing {
        let [spin, dim] = m::BUSY;
        let bg = theme::theme(ui.ctx()).palette.raised;
        painter.rect_filled(r, avatar::corner(r, shape), bg.gamma_multiply(dim));
        crate::widgets::progress::spinner(ui, r.center(), spin / 2.0, Color32::WHITE);
    }
}

/// The crop's square as texture coordinates of the picture.
fn square_uv(placed: &Placed) -> Rect {
    let size = placed.size();
    let [x, y, side] = placed.view.square(size, placed.area, crop::CIRCLE);
    Rect::from_min_size(
        pos2(x / size[0], y / size[1]),
        vec2(side / size[0], side / size[1]),
    )
}

/// The crop step over the dialog `id`, while one is open: `Use picture` keeps the crop for
/// Save, Cancel (or Esc) goes back. Returns `false` when no crop step is open.
pub(super) fn crop_step(ctx: &Context, id: Id, edit: &mut PictureEdit) -> bool {
    let Some(placed) = &mut edit.crop else {
        return false;
    };
    // Esc goes back a step. Taken before the dialog sees it, which would close it whole.
    let layer = egui::LayerId::new(egui::Order::Foreground, id);
    let back = ctx.memory(|m| m.top_modal_layer()) == Some(layer)
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let shown = modal::show(
        ctx,
        id,
        Spec::new(s::CROP_TITLE())
            .text(s::CROP_TEXT())
            .step(1)
            .unsaved(true),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            crop::show(ui, placed);
        },
        |ui| {
            let used = ui.add(Button::primary(s::USE_PICTURE())).clicked();
            let cancel = ui.add(Button::new(c::CANCEL())).clicked();
            (used, cancel)
        },
    );
    let (used, cancel) = shown.footer;
    if used {
        edit.change = PictureChange::Set(placed.clone());
    }
    if used || cancel || back || shown.close_requested {
        edit.crop = None;
    }
    true
}

/// *Group picture* (`ChatDialog::GroupPicture`): the same row and crop step, for the group's
/// owner. Returns `true` once it is done.
pub(super) fn group_dialog(
    ctx: &Context,
    id: Id,
    state: &mut UiState,
    env: &mut Env<'_>,
    group: i64,
) -> bool {
    let snap = env.bridge.chat().clone();
    let Some(g) = snap.group(group).filter(|g| g.owner) else {
        modal::closed(ctx, id);
        state.chat.picture = PictureEdit::default();
        return true;
    };
    if state.chat.picture.whose != Some(Some(group)) {
        state.chat.picture = PictureEdit::new(Some(group));
    }
    if crop_step(ctx, id, &mut state.chat.picture) {
        return false;
    }
    let unsaved = state.chat.picture.change != PictureChange::Keep;
    let busy = state.chat.picture.loading.is_some();
    let chat = &mut state.chat;
    let shown = modal::show(
        ctx,
        id,
        Spec::new(s::GROUP_TITLE())
            .text(s::GROUP_TEXT())
            .step(0)
            .unsaved(unsaved),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            row(
                ui,
                &mut state.picks,
                &mut chat.picture,
                &g.name,
                None,
                Shape::Group,
                g.picture.as_ref(),
                false,
            );
        },
        |ui| {
            let save = ui.add_enabled(!busy, Button::primary(c::SAVE())).clicked();
            let cancel = ui.add(Button::new(c::CANCEL())).clicked();
            (save, cancel)
        },
    );
    let (save, cancel) = shown.footer;
    if save && let Some(cmd) = state.chat.picture.commands(Some(group), crop::CIRCLE) {
        env.bridge.send(Command::Chat(cmd));
    }
    if save || cancel {
        modal::closed(ctx, id);
    }
    let done = save || cancel || shown.close_requested;
    if done {
        state.chat.picture = PictureEdit::default();
    }
    done
}
