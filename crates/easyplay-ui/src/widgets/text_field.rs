//! Text field: label above (`caption`, `text.secondary`), field height `md`, helper or error
//! text below; error is a `danger` border and a message with an icon. Search fields get a
//! leading icon, a clear button when non-empty, and `Esc` clears (§7.4, ACCESSIBILITY §2).

use egui::{
    Align, Align2, CornerRadius, Frame, Key, Layout, Rect, Response, Sense, Stroke, StrokeKind,
    TextEdit, Ui, UiBuilder, pos2, vec2,
};

use super::focus_ring;
use crate::a11y;
use crate::strings;
use crate::theme::palette::mix;
use crate::theme::tokens::{control, icon as icon_size, radius, space, stroke};
use crate::theme::{self, Type, icons};

/// A text field builder.
#[must_use = "call `.show(ui)`"]
pub struct TextField<'a> {
    text: &'a mut String,
    id_salt: &'a str,
    label: Option<&'a str>,
    accessible_name: Option<&'a str>,
    hint: Option<&'a str>,
    help: Option<&'a str>,
    error: Option<&'a str>,
    search: bool,
    mono: bool,
    width: Option<f32>,
}

/// What happened this frame.
#[derive(Debug)]
pub struct TextFieldResponse {
    /// The text edit's response (focus, changes).
    pub response: Response,
    /// The clear button was used or `Esc` cleared a search field.
    pub cleared: bool,
}

impl<'a> TextField<'a> {
    /// A field editing `text`; `id_salt` keeps its state stable.
    pub fn new(text: &'a mut String, id_salt: &'a str) -> Self {
        Self {
            text,
            id_salt,
            label: None,
            accessible_name: None,
            hint: None,
            help: None,
            error: None,
            search: false,
            mono: false,
            width: None,
        }
    }

    /// Visible label above the field (also its accessible name).
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    /// Accessible name for fields without a visible label (search).
    pub fn accessible_name(mut self, name: &'a str) -> Self {
        self.accessible_name = Some(name);
        self
    }

    /// Placeholder.
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = Some(hint);
        self
    }

    /// Helper text below.
    pub fn help(mut self, help: &'a str) -> Self {
        self.help = Some(help);
        self
    }

    /// Error text below; replaces help and turns the border red.
    pub fn error(mut self, error: Option<&'a str>) -> Self {
        self.error = error;
        self
    }

    /// Search styling and behaviour.
    pub fn search(mut self) -> Self {
        self.search = true;
        self
    }

    /// Monospace text (paths, variables).
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// Fixed width (default: available width).
    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    /// Show the field.
    pub fn show(self, ui: &mut Ui) -> TextFieldResponse {
        let theme = theme::theme(ui.ctx());
        let p = theme.palette.clone();
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let mut cleared = false;
        let id = ui.make_persistent_id(self.id_salt);
        let caption_row = Type::Caption.line_height() + space::S1 + 2.0;
        let height = control::MD
            + if self.label.is_some() {
                caption_row
            } else {
                0.0
            }
            + if self.help.is_some() || self.error.is_some() {
                caption_row
            } else {
                0.0
            };
        let resp = ui
            .allocate_ui_with_layout(vec2(width, height), Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = space::S1 + 2.0;
                let label_resp = self
                    .label
                    .map(|l| ui.label(Type::Caption.rich(l).color(p.text_secondary)));
                let (rect, _) = ui.allocate_exact_size(vec2(width, control::MD), Sense::hover());
                let frame_idx = ui.painter().add(egui::Shape::Noop);
                let lead = if self.search {
                    icon_size::INLINE + space::S2
                } else {
                    0.0
                };
                let has_text = !self.text.is_empty();
                let trail = if self.search && has_text {
                    control::SM
                } else {
                    0.0
                };
                let inner = Rect::from_min_max(
                    pos2(rect.left() + space::S3 + lead, rect.top()),
                    pos2(rect.right() - space::S2 - trail, rect.bottom()),
                );
                let font = if self.mono {
                    Type::Mono.font_id()
                } else {
                    Type::Body.font_id()
                };
                let mut edit = TextEdit::singleline(self.text)
                    .id(id)
                    .frame(Frame::NONE)
                    .font(font)
                    .text_color(p.text_primary)
                    .desired_width(inner.width())
                    .align(Align2::LEFT_CENTER)
                    .min_size(vec2(inner.width(), inner.height()));
                if let Some(h) = self.hint {
                    edit = edit.hint_text(Type::Body.rich(h).color(p.text_muted));
                }
                let out = ui
                    .scope_builder(
                        UiBuilder::new()
                            .max_rect(inner)
                            .layout(Layout::left_to_right(Align::Center)),
                        |ui| edit.show(ui),
                    )
                    .inner;
                let mut r = out.response.response;
                if let Some(l) = &label_resp {
                    r = r.labelled_by(l.id);
                } else if let Some(n) = self.accessible_name {
                    r = r.accessible_name(n);
                }
                if self.search
                    && r.has_focus()
                    && ui.input(|i| i.key_pressed(Key::Escape))
                    && has_text
                {
                    self.text.clear();
                    cleared = true;
                }
                if self.search && has_text {
                    let cr = Rect::from_center_size(
                        pos2(
                            rect.right() - space::S1 - control::SM / 2.0,
                            rect.center().y,
                        ),
                        vec2(control::SM - space::S1, control::SM - space::S1),
                    );
                    let c = ui.interact(cr, id.with("clear"), Sense::click());
                    a11y::button(&c, strings::library::CLEAR_SEARCH);
                    let c = c.on_hover_text(strings::library::CLEAR_SEARCH);
                    let col = if c.hovered() {
                        p.text_primary
                    } else {
                        p.text_muted
                    };
                    if c.hovered() {
                        ui.painter()
                            .rect_filled(cr, CornerRadius::same(radius::SM), p.bg_overlay);
                    }
                    ui.painter().text(
                        cr.center(),
                        Align2::CENTER_CENTER,
                        icons::CLOSE.text(false),
                        icons::font(icon_size::INLINE - 2.0),
                        col,
                    );
                    if c.clicked() {
                        self.text.clear();
                        cleared = true;
                        r.request_focus();
                    }
                    if c.has_focus() {
                        focus_ring(ui.painter(), cr, CornerRadius::same(radius::SM), &theme);
                    }
                }
                let focused = r.has_focus();
                let hovered = r.hovered();
                let border = if self.error.is_some() {
                    p.danger
                } else if focused {
                    p.accent
                } else if hovered {
                    mix(p.border_strong, p.text_secondary, 0.35)
                } else {
                    p.border_strong
                };
                let corner = CornerRadius::same(radius::MD);
                let shapes = vec![
                    egui::Shape::rect_filled(rect, corner, p.bg_raised),
                    egui::Shape::rect_stroke(
                        rect,
                        corner,
                        Stroke::new(
                            if focused {
                                stroke::FOCUS
                            } else {
                                stroke::BORDER
                            },
                            border,
                        ),
                        StrokeKind::Inside,
                    ),
                ];
                ui.painter().set(frame_idx, egui::Shape::Vec(shapes));
                if self.search {
                    ui.painter().text(
                        pos2(rect.left() + space::S3, rect.center().y),
                        Align2::LEFT_CENTER,
                        icons::SEARCH.text(false),
                        icons::font(icon_size::INLINE),
                        if focused {
                            p.text_secondary
                        } else {
                            p.text_muted
                        },
                    );
                }
                if let Some(err) = self.error {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S1;
                        ui.label(icons::icon(icons::WARNING, 14.0).color(p.danger));
                        ui.label(Type::Caption.rich(err).color(p.danger));
                    });
                    a11y::describe(&r, err);
                } else if let Some(help) = self.help {
                    ui.label(Type::Caption.rich(help).color(p.text_muted));
                    a11y::describe(&r, help);
                }
                r
            })
            .inner;
        TextFieldResponse {
            response: resp,
            cleared,
        }
    }
}
