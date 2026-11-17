//! The empty state (CSS `.blank`): a column at most 440 wide in the middle of the page, 40
//! below its top: a headline in the display face (34 / 36), a sentence (15 / 22, `t2`) 10
//! below it, the actions 24 below that (8 apart), and fine print (12 / 17, `t3`) 14 below
//! them. The text is left-aligned; a column whose lines are all shorter than 440 is as
//! wide as its widest line and centred. No icon.

use egui::{Align, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use crate::a11y;
use crate::theme::tokens::{control, layout, overlay};
use crate::theme::{self, Type};

/// The fine print under an empty state's actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fine<'a> {
    /// Text.
    Text(&'a str),
    /// A technical detail, in mono ("Permission denied (os error 13)").
    Mono(&'a str),
}

/// An empty state: `title`, `text`, the buttons `actions` adds (left to right), and the
/// optional fine print. Returns what `actions` returned.
pub fn blank<R>(
    ui: &mut Ui,
    title: &str,
    text: &str,
    actions: impl FnOnce(&mut Ui) -> R,
    fine: Option<&str>,
) -> R {
    blank_with(ui, title, text, actions, fine.map(Fine::Text))
}

/// [`blank`] whose fine print may be a technical detail in mono.
pub fn blank_with<R>(
    ui: &mut Ui,
    title: &str,
    text: &str,
    actions: impl FnOnce(&mut Ui) -> R,
    fine: Option<Fine<'_>>,
) -> R {
    let p = theme::theme(ui.ctx()).palette.clone();
    let avail = ui.available_rect_before_wrap();
    let col_w = layout::BLANK_W.min(avail.width());
    let id = ui.make_persistent_id(("blank", title));
    let acts_w: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
    // The column is as wide as its widest line, at most 440.
    let natural = |t: Type, s: &str| Type::line(t, ui, s, p.t1).size().x;
    let fine_w = match fine {
        Some(Fine::Text(s)) => natural(Type::Caption, s),
        Some(Fine::Mono(s)) => natural(Type::Mono, s),
        None => 0.0,
    };
    let w = natural(Type::Blank, title)
        .max(natural(Type::Lead, text))
        .max(fine_w)
        .max(acts_w)
        .min(col_w)
        .ceil();
    let left = avail.center().x - w / 2.0;
    let mut y = avail.top() + overlay::BLANK_PAD;
    let g = super::modal::balanced(ui, Type::Blank, title, p.t1, w);
    let h = g.size().y;
    let r = Rect::from_min_size(pos2(left, y), g.size());
    ui.painter().galley(r.min, g, p.t1);
    let node = ui.interact(r, id.with("title"), Sense::hover());
    a11y::item(&node, egui::Role::Heading, title);
    y += h;
    if !text.is_empty() {
        y += overlay::BLANK_TEXT_GAP;
        let g = super::modal::pretty(ui, Type::Lead, text, p.t2, w);
        let h = g.size().y;
        let r = Rect::from_min_size(pos2(left, y), g.size());
        ui.painter().galley(r.min, g, p.t2);
        let node = ui.interact(r, id.with("text"), Sense::hover());
        a11y::item(&node, egui::Role::Label, text);
        y += h;
    }
    y += overlay::BLANK_ACTIONS_GAP;
    let mut row = ui.new_child(
        UiBuilder::new()
            .id_salt(id.with("actions"))
            .max_rect(Rect::from_min_size(
                pos2(left, y),
                vec2(col_w, control::BUTTON),
            ))
            .layout(Layout::left_to_right(Align::Center).with_main_wrap(true)),
    );
    row.spacing_mut().item_spacing = vec2(overlay::DIALOG_ACTION_GAP, overlay::DIALOG_ACTION_GAP);
    let out = actions(&mut row);
    let r = row.min_rect();
    if (r.width() - acts_w).abs() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(id, r.width()));
        ui.ctx().request_repaint();
    }
    if r.height() > 0.0 {
        y = r.bottom();
    } else {
        y -= overlay::BLANK_ACTIONS_GAP;
    }
    if let Some(f) = fine {
        y += overlay::BLANK_FINE_GAP;
        let (t, s) = match f {
            Fine::Text(s) => (Type::Caption, s),
            Fine::Mono(s) => (Type::Mono, s),
        };
        let g = if crate::i18n::bidi::has_rtl(s) {
            // Right-to-left text is broken and ordered by `Type::galley`.
            t.galley(ui, s, p.t3, w, usize::MAX)
        } else {
            let mut job = t.job(s, p.t3, w, usize::MAX);
            if t == Type::Caption {
                for sec in &mut job.sections {
                    sec.format.line_height = Some(overlay::FINE_LINE);
                }
            }
            ui.fonts_mut(|fo| fo.layout_job(job))
        };
        let h = g.size().y;
        ui.painter().galley(pos2(left, y), g, p.t3);
        y += h;
    }
    y += overlay::BLANK_PAD;
    ui.allocate_rect(
        Rect::from_min_max(avail.left_top(), pos2(avail.right(), y)),
        Sense::hover(),
    );
    out
}
