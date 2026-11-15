//! Permission rows (UI_DESIGN_SYSTEM §7.4): a list in which every row is one thing a game
//! may be allowed, with its switch (the Protection tab). A row is an icon, a title, lines
//! beneath that say what it means, a mark while it differs from what is saved, and a switch
//! at its end; the whole row flips it. A permission that comes in degrees shows the degree
//! before the switch while it is on, as a control of its own that opens the choices. A row
//! without a switch is its degree alone (the whole row opens the choices), leads to another
//! part of the page, does one thing, or states a fact. Rows keep their places: nothing
//! moves when a switch is flipped. They are parted by hairlines that step aside for the
//! plate of the row being pointed at.

use std::borrow::Cow;
use std::sync::Arc;

use egui::accesskit::Role;
use egui::{
    Align2, Color32, CornerRadius, Galley, Id, Key, Modifiers, Popup, PopupAnchor,
    PopupCloseBehavior, Pos2, Rect, RectAlign, Response, Sense, Stroke, Ui, WidgetInfo, pos2, vec2,
};

use super::link_row::Tone;
use super::toggle::paint_track;
use super::{context_menu, focus_ring, hover_t, press_t, tooltip};
use crate::a11y;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{control, icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Icon, Palette, Theme, Type, icons};

/// A line beneath a row's title.
#[derive(Clone, Debug)]
pub struct Line<'a> {
    /// The sentence.
    pub text: Cow<'a, str>,
    /// Its weight.
    pub tone: Tone,
}

impl<'a> Line<'a> {
    /// A line of the usual weight.
    pub fn new(text: impl Into<Cow<'a, str>>) -> Self {
        Self::toned(text, Tone::Normal)
    }

    /// A line of the given weight.
    pub fn toned(text: impl Into<Cow<'a, str>>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }
}

/// One of the degrees of a permission.
#[derive(Clone, Copy, Debug)]
pub struct Choice<'a> {
    /// What it allows ("Can read and change").
    pub label: &'a str,
    /// Why it cannot be chosen on this computer, said beneath it.
    pub unavailable: Option<&'a str>,
}

impl<'a> Choice<'a> {
    /// A choice that can be made.
    pub const fn new(label: &'a str) -> Self {
        Self {
            label,
            unavailable: None,
        }
    }
}

/// The degree of a permission: its choices and the one in force.
#[derive(Clone, Debug)]
pub struct Degree<'a> {
    /// The choices, in menu order.
    pub choices: &'a [Choice<'a>],
    /// The one in force (checked in the menu).
    pub chosen: usize,
    /// What the row shows for it ("Can read").
    pub value: Cow<'a, str>,
    /// Accessible name of the control ("Music: Can read").
    pub name: Cow<'a, str>,
}

/// What pressing a row that has neither a switch nor a degree does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Press {
    /// Nothing: the row states a fact.
    #[default]
    Nothing,
    /// Leads to another part of the page.
    Link,
    /// Does one thing (*Add folder…*).
    Action,
}

/// One row.
#[derive(Clone, Debug)]
pub struct PermissionRow<'a> {
    /// The row's identity.
    pub id: Id,
    /// Leading icon.
    pub icon: Icon,
    /// Weight of the icon.
    pub tone: Tone,
    /// Set back: the title is not emphasised and the icon is muted (a permission that is
    /// off).
    pub quiet: bool,
    /// What the row is about ("Music").
    pub title: Cow<'a, str>,
    /// What it means ("in Home").
    pub lines: Vec<Line<'a>>,
    /// Said before the controls while the row differs from what is saved ("changed").
    pub mark: Option<&'a str>,
    /// The switch and whether it is on. Pressing the row flips it.
    pub switch: Option<bool>,
    /// The degree. Beside a switch it is a control of its own; without one, pressing the
    /// row opens its choices.
    pub degree: Option<Degree<'a>>,
    /// What pressing the row does when it has neither.
    pub press: Press,
    /// Accessible name ("Music").
    pub name: Cow<'a, str>,
    /// Tooltip (a folder's full path).
    pub hint: Option<Cow<'a, str>>,
    /// Report `Delete` pressed on the row.
    pub delete: bool,
}

impl<'a> PermissionRow<'a> {
    /// A row stating a fact, named by its title.
    pub fn new(id: Id, icon: Icon, title: impl Into<Cow<'a, str>>) -> Self {
        let title = title.into();
        Self {
            id,
            icon,
            tone: Tone::Normal,
            quiet: false,
            name: title.clone(),
            title,
            lines: Vec::new(),
            mark: None,
            switch: None,
            degree: None,
            press: Press::Nothing,
            hint: None,
            delete: false,
        }
    }
}

/// What the user did to a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picked {
    /// Flipped its switch to this.
    Switched(bool),
    /// Chose this one of its degrees (it may be the one already in force).
    Degree(usize),
    /// Pressed a link or an action.
    Pressed,
    /// Pressed `Delete` on it.
    Delete,
}

struct Laid {
    rect: Rect,
    resp: Response,
    line: Rect,
    title: Arc<Galley>,
    lines: Vec<(f32, Arc<Galley>)>,
    mark: Option<(Pos2, Arc<Galley>)>,
    track: Option<Rect>,
    /// The degree: where it stands, its text, and its own response beside a switch.
    degree: Option<(Rect, Arc<Galley>, Option<Response>)>,
}

/// Space inside the degree control, on both sides of its text and caret.
const DEGREE_PAD: f32 = space::S2;
/// The caret after a degree.
const CARET: f32 = space::S3;

fn lay(ui: &mut Ui, row: &PermissionRow<'_>, p: &Palette) -> Laid {
    let w = ui.available_width();
    let text_h = Type::Body.line_height();
    let pad = (layout::PERMISSION_ROW_H - text_h) / 2.0;

    // The end of the row, from the right: the switch or the chevron, the degree, the mark.
    let mut trail = match (row.switch, row.press) {
        (Some(_), _) => control::TOGGLE_W,
        (None, Press::Link) if row.degree.is_none() => icon_size::INLINE,
        _ => 0.0,
    };
    let value = row.degree.as_ref().map(|d| {
        let room = (w * 0.5 - CARET - space::S1).max(1.0);
        Type::Body.galley(ui, &d.value, p.text_secondary, room, 1)
    });
    let degree_w = value.as_ref().map(|g| {
        let inner = g.size().x + space::S1 + CARET;
        // Beside a switch the degree is a control with a plate of its own.
        if row.switch.is_some() {
            inner + DEGREE_PAD * 2.0
        } else {
            inner
        }
    });
    let degree_at = degree_w.map(|dw| {
        if trail > 0.0 {
            trail += space::S2;
        }
        trail += dw;
        trail
    });
    let mark = row
        .mark
        .map(|m| Type::Caption.galley(ui, m, p.accent, f32::INFINITY, 1));
    let mark_at = mark.as_ref().map(|g| {
        if trail > 0.0 {
            trail += space::S3;
        }
        trail += g.size().x;
        trail
    });
    if trail > 0.0 {
        trail += space::S3;
    }

    let text_w = (w - layout::LINK_ROW_LEAD - trail).max(1.0);
    let style = if row.quiet {
        Type::Body
    } else {
        Type::BodyStrong
    };
    let title = style.galley(ui, &row.title, p.text_primary, text_w, 2);
    let mut h = pad + title.size().y.max(text_h);
    let mut lines = Vec::with_capacity(row.lines.len());
    for l in &row.lines {
        let c = l.tone.color(p.text_secondary, p);
        let g = Type::Caption.galley(ui, &l.text, c, text_w, 3);
        h += if lines.is_empty() { 0.0 } else { space::S0 };
        lines.push((h, g.clone()));
        h += g.size().y;
    }
    h += pad;

    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let acts = row.switch.is_some() || row.degree.is_some() || row.press != Press::Nothing;
    let resp = if acts {
        let target = rect.expand2(vec2(layout::LINK_ROW_OVERHANG, 0.0));
        ui.interact(target, row.id, Sense::click())
    } else {
        ui.interact(rect, row.id, Sense::hover())
    };
    // After the name: what the lines say, and that the row is not saved.
    let said: Vec<&str> = row.lines.iter().map(|l| &*l.text).collect();
    let mut described = said.join(". ");
    if let Some(mark) = row.mark {
        if !described.is_empty() {
            described.push_str(". ");
        }
        described.push_str(mark);
    }
    let enabled = resp.enabled();
    match (row.switch, &row.degree, row.press) {
        (Some(on), _, _) => {
            let name = row.name.to_string();
            resp.widget_info(move || WidgetInfo::selected(Role::Switch, enabled, on, &name));
        }
        (None, Some(d), _) => {
            let name = d.name.to_string();
            resp.widget_info(move || WidgetInfo::labeled(Role::ComboBox, enabled, &name));
        }
        (None, None, Press::Nothing) => a11y::item(&resp, Role::ListItem, &row.name),
        (None, None, _) => a11y::button(&resp, &row.name),
    }
    if !described.is_empty() {
        a11y::describe(&resp, &described);
    }

    let line = Rect::from_min_size(pos2(rect.left(), rect.top() + pad), vec2(w, text_h));
    let cy = line.center().y;
    let track = row.switch.map(|_| {
        Rect::from_center_size(
            pos2(rect.right() - control::TOGGLE_W / 2.0, cy),
            vec2(control::TOGGLE_W, control::TOGGLE_H),
        )
    });
    let degree = value.zip(degree_w).zip(degree_at).map(|((g, dw), at)| {
        let at = Rect::from_center_size(
            pos2(rect.right() - at + dw / 2.0, cy),
            vec2(dw, control::MIN_TARGET),
        );
        // After the row, so that it lies over it and takes the pointer.
        let own = row.switch.zip(row.degree.as_ref()).map(|(_, d)| {
            let r = ui.interact(at, row.id.with("degree"), Sense::click());
            let name = d.name.to_string();
            r.widget_info(move || WidgetInfo::labeled(Role::ComboBox, enabled, &name));
            r
        });
        (at, g, own)
    });
    let mark = mark
        .zip(mark_at)
        .map(|(g, at)| (pos2(rect.right() - at, cy - g.size().y / 2.0), g));
    Laid {
        rect,
        resp,
        line,
        title,
        lines,
        mark,
        track,
        degree,
    }
}

/// The menu of a permission's degrees, opened by `resp`. It hangs from `under`, or opens
/// where the row was pressed when `at_pointer`. Returns the degree pressed.
fn menu(
    ui: &mut Ui,
    resp: &Response,
    under: Rect,
    at_pointer: bool,
    degree: &Degree<'_>,
) -> Option<usize> {
    let at_id = resp.id.with("menu.at");
    if resp.clicked() {
        let at = resp.interact_pointer_pos();
        ui.ctx().data_mut(|d| d.insert_temp(at_id, at));
    }
    let at: Option<Pos2> = ui.ctx().data(|d| d.get_temp(at_id)).flatten();
    let (anchor, align) = match at {
        Some(at) if at_pointer => (PopupAnchor::Position(at), RectAlign::BOTTOM_START),
        _ => (PopupAnchor::ParentRect(under), RectAlign::BOTTOM_END),
    };
    let popup = Popup::default_response_id(resp);
    let was_open = Popup::is_id_open(ui.ctx(), popup);
    let mut picked = None;
    context_menu::popup(
        Popup::menu(resp)
            .anchor(anchor)
            .align(align)
            .close_behavior(PopupCloseBehavior::CloseOnClick)
            .width(layout::PERMISSION_MENU_W),
        |ui| {
            for (k, c) in degree.choices.iter().enumerate() {
                match c.unavailable {
                    Some(why) => context_menu::unavailable_item(ui, c.label, why),
                    None => {
                        let checked = k == degree.chosen;
                        if context_menu::item(ui, None, c.label, None, checked).clicked() {
                            picked = Some(k);
                        }
                    }
                }
            }
        },
    );
    // Opened from the keyboard, the control takes the focus back when its menu closes.
    if was_open && at.is_none() && !Popup::is_id_open(ui.ctx(), popup) {
        resp.request_focus();
    }
    picked
}

fn paint_degree(
    ui: &Ui,
    at: Rect,
    value: &Arc<Galley>,
    own: Option<&Response>,
    row_hot: f32,
    theme: &Theme,
) {
    let p = &theme.palette;
    let painter = ui.painter();
    let corner = CornerRadius::same(radius::MD);
    let (hot, inset) = match own {
        Some(r) => {
            let open = Popup::is_id_open(ui.ctx(), Popup::default_response_id(r));
            let hot = hover_t(r, theme).max(f32::from(u8::from(open)));
            painter.rect_filled(at, corner, fade(p.fill_selected, hot));
            if r.has_focus() {
                focus_ring(painter, at, corner, theme);
            }
            (hot, DEGREE_PAD)
        }
        None => (row_hot, 0.0),
    };
    painter.galley(
        pos2(at.left() + inset, at.center().y - value.size().y / 2.0),
        value.clone(),
        mix(p.text_secondary, p.text_primary, hot),
    );
    painter.text(
        pos2(at.right() - inset, at.center().y),
        Align2::RIGHT_CENTER,
        icons::CARET_DOWN.text(false),
        icons::font(CARET),
        mix(p.text_muted, p.text_primary, hot),
    );
}

/// Show `rows` as one list. Returns the row the user acted on and what they did.
pub fn permission_rows(ui: &mut Ui, rows: &[PermissionRow<'_>]) -> Option<(usize, Picked)> {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let spacing = std::mem::replace(&mut ui.spacing_mut().item_spacing.y, 0.0);
    let laid: Vec<Laid> = rows.iter().map(|row| lay(ui, row, p)).collect();
    ui.spacing_mut().item_spacing.y = spacing;
    let hot: Vec<f32> = laid
        .iter()
        .map(|l| {
            if l.resp.sense.senses_click() {
                hover_t(&l.resp, &theme)
            } else {
                0.0
            }
        })
        .collect();
    let corner = CornerRadius::same(radius::ROW);
    let mut picked = None;
    for (i, (row, l)) in rows.iter().zip(&laid).enumerate() {
        // The degree first: a press on it is not a press on the row beneath.
        let own = l.degree.as_ref().and_then(|(_, _, own)| own.as_ref());
        match (&row.degree, &l.degree) {
            (Some(d), Some((at, _, Some(own)))) => {
                if let Some(k) = menu(ui, own, *at, false, d) {
                    picked = Some((i, Picked::Degree(k)));
                }
            }
            (Some(d), Some((at, _, None))) => {
                if let Some(k) = menu(ui, &l.resp, *at, true, d) {
                    picked = Some((i, Picked::Degree(k)));
                }
            }
            _ => {}
        }
        if l.resp.clicked() {
            match (row.switch, row.press) {
                (Some(on), _) => picked = Some((i, Picked::Switched(!on))),
                (None, Press::Link | Press::Action) if row.degree.is_none() => {
                    picked = Some((i, Picked::Pressed));
                }
                _ => {}
            }
        }
        if row.delete
            && l.resp.has_focus()
            && ui.input_mut(|inp| inp.consume_key(Modifiers::NONE, Key::Delete))
        {
            picked = Some((i, Picked::Delete));
        }
        if !ui.is_rect_visible(l.resp.rect) {
            continue;
        }
        // The row's plate rests while the pointer is on its degree.
        let h = if own.is_some_and(Response::hovered) {
            0.0
        } else {
            hot[i]
        };
        let painter = ui.painter();
        if h > 0.0 {
            painter.rect_filled(l.resp.rect, corner, fade(p.fill_hover, h));
            let press = press_t(&l.resp, &theme);
            if press > 0.0 {
                painter.rect_filled(l.resp.rect, corner, fade(p.fill_hover, press));
            }
        }
        if i > 0 {
            let strength = 1.0 - hot[i].max(hot[i - 1]);
            if strength > 0.0 {
                painter.hline(
                    l.rect.left() + layout::LINK_ROW_LEAD..=l.rect.right(),
                    l.rect.top(),
                    Stroke::new(stroke::BORDER, fade(p.border_subtle, strength)),
                );
            }
        }
        let rest = if row.quiet {
            p.text_muted
        } else {
            p.text_secondary
        };
        let cy = l.line.center().y;
        painter.text(
            pos2(l.rect.left() + icon_size::BUTTON / 2.0, cy),
            Align2::CENTER_CENTER,
            row.icon.text(false),
            icons::font(icon_size::BUTTON),
            row.tone.color(mix(rest, p.text_primary, h), p),
        );
        let text_x = l.rect.left() + layout::LINK_ROW_LEAD;
        painter.galley(
            pos2(text_x, l.line.top()),
            l.title.clone(),
            Color32::PLACEHOLDER,
        );
        for (top, g) in &l.lines {
            painter.galley(
                pos2(text_x, l.rect.top() + top),
                g.clone(),
                Color32::PLACEHOLDER,
            );
        }
        if let Some((at, g)) = &l.mark {
            painter.galley(*at, g.clone(), Color32::PLACEHOLDER);
        }
        if let Some((at, value, own)) = &l.degree {
            paint_degree(ui, *at, value, own.as_ref(), h, &theme);
        }
        if let (Some(track), Some(on)) = (l.track, row.switch) {
            paint_track(ui, &l.resp, track, on);
        } else if row.press == Press::Link && row.degree.is_none() {
            painter.text(
                pos2(l.rect.right() - icon_size::INLINE / 2.0, cy),
                Align2::CENTER_CENTER,
                icons::CARET_RIGHT.text(false),
                icons::font(icon_size::INLINE),
                mix(p.text_muted, p.text_primary, h),
            );
        }
        if l.resp.has_focus() {
            focus_ring(painter, l.resp.rect.shrink(1.0), corner, &theme);
        }
    }
    for (row, l) in rows.iter().zip(laid) {
        if let Some(hint) = &row.hint {
            tooltip::attach(l.resp, hint);
        }
    }
    picked
}
