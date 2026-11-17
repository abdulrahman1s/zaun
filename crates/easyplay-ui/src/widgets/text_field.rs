//! Text fields.
//!
//! - A field is 30 high on the `well`, radius 8, 10 padding, with a 1 pt `line` inside its
//!   edge; focused, a 1.5 pt accent line instead. The placeholder is quiet text.
//! - A multi-line field is at least 76 high, 8 / 10 padding, 19 pt lines.
//! - The search field is 30 high on `o1` with a quiet 16 pt magnifier; focused, the `well`
//!   with the accent line. While it is empty and not focused, its shortcut stands at the
//!   right as a keycap; while it has text, a small clear button stands there. `Esc` clears
//!   it. Its width (and the animation of its width) is the
//!   caller's.
//!
//! An optional label stands above and help or an error below (12 pt).

use std::sync::Arc;

use egui::{
    Align, Align2, Color32, CornerRadius, Frame, Galley, Key, Layout, Rect, Response, Sense, Shape,
    Stroke, StrokeKind, TextBuffer, TextEdit, Ui, UiBuilder, pos2, vec2,
};

use super::key_hint::{key_hint_shapes, key_hint_size};
use super::{contrast_outline, focus_ring, glyph_shape, pointer_t, tooltip};
use crate::a11y;
use crate::strings;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{control, icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Dur, Type, icons};

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
    shortcut: Option<&'a str>,
    mono: bool,
    width: Option<f32>,
    multiline: bool,
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
            shortcut: None,
            mono: false,
            width: None,
            multiline: false,
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

    /// Help text below.
    pub fn help(mut self, help: &'a str) -> Self {
        self.help = Some(help);
        self
    }

    /// Error text below; replaces help and turns the field's line coral.
    pub fn error(mut self, error: Option<&'a str>) -> Self {
        self.error = error;
        self
    }

    /// The search field's look and behaviour.
    pub fn search(mut self) -> Self {
        self.search = true;
        self
    }

    /// The key that focuses a search field, shown as a keycap while it is empty and idle.
    pub fn shortcut_hint(mut self, key: &'a str) -> Self {
        self.shortcut = Some(key);
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

    /// Several lines (notes, a command line): at least 76 high, growing with its text.
    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    /// Show the field.
    pub fn show(self, ui: &mut Ui) -> TextFieldResponse {
        let theme = theme::theme(ui.ctx());
        let p = theme.palette.clone();
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let id = ui.make_persistent_id(self.id_salt);
        let mut cleared = false;
        let resp = ui
            .allocate_ui_with_layout(
                vec2(width, control::BUTTON),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing.y = space::S1;
                    let label_resp = self
                        .label
                        .map(|l| ui.label(Type::Caption.rich(l).color(p.t3)));
                    let top = ui.cursor().min;
                    let frame_idx = ui.painter().add(Shape::Noop);
                    let has_text = !self.text.is_empty();
                    let [pad_l, pad_r] = if self.search {
                        [
                            control::SEARCH_PAD[0] + icon_size::INLINE + control::BUTTON_GAP,
                            // Room for the clear button while there is text.
                            control::SEARCH_PAD[1]
                                + if has_text {
                                    control::WORD_X + space::S1
                                } else {
                                    0.0
                                },
                        ]
                    } else {
                        [control::TEXTAREA_PAD[0], control::TEXTAREA_PAD[0]]
                    };
                    let font_type = if self.mono { Type::Mono } else { Type::Body };
                    let line_h = if self.multiline {
                        control::TEXTAREA_LINE
                    } else {
                        font_type.line_height()
                    };
                    let mut layouter = move |ui: &Ui,
                                             buf: &dyn TextBuffer,
                                             wrap: f32|
                          -> Arc<Galley> {
                        let mut format = font_type.format(Color32::PLACEHOLDER, false);
                        format.line_height = Some(line_h);
                        format.color = theme::theme(ui.ctx()).palette.t1;
                        let mut job =
                            egui::text::LayoutJob::single_section(buf.as_str().to_owned(), format);
                        job.wrap.max_width = wrap;
                        ui.fonts_mut(|f| f.layout_job(job))
                    };
                    let hint = self.hint.map(|h| font_type.rich(h).color(p.t3));
                    let (rect, r) = if self.multiline {
                        let pad_y = control::TEXTAREA_PAD[1];
                        let inner = Rect::from_min_size(
                            pos2(top.x + pad_l, top.y + pad_y),
                            vec2(width - pad_l - pad_r, f32::INFINITY),
                        );
                        let min_h = layout::TEXTAREA_MIN_H - pad_y * 2.0;
                        let mut edit = TextEdit::multiline(self.text)
                            .id(id)
                            .frame(Frame::NONE)
                            .margin(egui::Margin::ZERO)
                            .layouter(&mut layouter)
                            .desired_width(inner.width())
                            .min_size(vec2(inner.width(), min_h));
                        if let Some(h) = hint {
                            edit = edit.hint_text(h);
                        }
                        let out = ui
                            .scope_builder(
                                UiBuilder::new()
                                    .max_rect(inner)
                                    .layout(Layout::top_down(Align::Min)),
                                |ui| edit.show(ui),
                            )
                            .inner;
                        let h = out.response.response.rect.height().max(min_h) + pad_y * 2.0;
                        let rect = Rect::from_min_size(top, vec2(width, h));
                        ui.allocate_rect(rect, Sense::hover());
                        (rect, out.response.response)
                    } else {
                        let (rect, _) =
                            ui.allocate_exact_size(vec2(width, control::BUTTON), Sense::hover());
                        let inner = Rect::from_min_max(
                            pos2(rect.left() + pad_l, rect.top()),
                            pos2(rect.right() - pad_r, rect.bottom()),
                        );
                        let mut edit = TextEdit::singleline(self.text)
                            .id(id)
                            .frame(Frame::NONE)
                            .margin(egui::Margin::ZERO)
                            .layouter(&mut layouter)
                            .desired_width(inner.width())
                            .align(Align2::LEFT_CENTER)
                            .min_size(inner.size());
                        if let Some(h) = hint {
                            edit = edit.hint_text(h);
                        }
                        let out = ui
                            .scope_builder(
                                UiBuilder::new()
                                    .max_rect(inner)
                                    .layout(Layout::left_to_right(Align::Center)),
                                |ui| edit.show(ui),
                            )
                            .inner;
                        (rect, out.response.response)
                    };
                    let mut r = r;
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
                    let focused = r.has_focus();
                    let focus_t = ui.ctx().animate_bool_with_time(
                        id.with("focus"),
                        focused,
                        theme.motion.fade(Dur::Fast),
                    );
                    let corner = CornerRadius::same(radius::MD);
                    let mut shapes = Vec::new();
                    let accent = if self.error.is_some() {
                        p.coral
                    } else {
                        p.accent_mark
                    };
                    if self.search {
                        shapes.push(Shape::rect_filled(rect, corner, mix(p.o1, p.well, focus_t)));
                        if let Some(o) = contrast_outline(rect, corner, &theme) {
                            shapes.push(o);
                        }
                    } else {
                        shapes.push(Shape::rect_filled(rect, corner, p.well));
                        shapes.push(Shape::rect_stroke(
                            rect,
                            corner,
                            Stroke::new(stroke::BORDER, fade(p.line, 1.0 - focus_t)),
                            StrokeKind::Inside,
                        ));
                    }
                    let line = if self.error.is_some() { 1.0 } else { focus_t };
                    if line > 0.0 {
                        shapes.push(Shape::rect_stroke(
                            rect,
                            corner,
                            Stroke::new(stroke::FIELD_FOCUS, fade(accent, line)),
                            StrokeKind::Inside,
                        ));
                    }
                    if self.search {
                        let painter = ui.painter();
                        shapes.push(glyph_shape(
                            painter,
                            pos2(
                                rect.left() + control::SEARCH_PAD[0] + icon_size::INLINE / 2.0,
                                rect.center().y,
                            ),
                            icons::SEARCH,
                            false,
                            icon_size::INLINE,
                            p.t3,
                        ));
                        // The shortcut's keycap, while nothing is typed and it is not focused.
                        if let Some(key) = self.shortcut
                            && !has_text
                            && !focused
                        {
                            let size = key_hint_size(ui, key);
                            let at = pos2(
                                rect.right() - control::SEARCH_PAD[1] - size.x,
                                rect.center().y,
                            );
                            shapes.extend(key_hint_shapes(painter, at, key, p.t3, &theme));
                        }
                    }
                    ui.painter().set(frame_idx, Shape::Vec(shapes));
                    // The design hides the search field's own clear button; EasyPlay keeps
                    // one, quiet, in the keycap's place while there is text.
                    if self.search && has_text {
                        let c = pos2(
                            rect.right() - control::SEARCH_PAD[1] - control::WORD_X / 2.0,
                            rect.center().y,
                        );
                        let hit = Rect::from_center_size(c, egui::Vec2::splat(control::MIN_TARGET));
                        let x = ui.interact(hit, id.with("clear"), Sense::click());
                        a11y::button(&x, strings::library::CLEAR_SEARCH());
                        let hover = pointer_t(&x, &theme);
                        let painter = ui.painter();
                        painter.circle_filled(c, control::WORD_X / 2.0, fade(p.o2, hover));
                        painter.add(glyph_shape(
                            painter,
                            c,
                            icons::CLOSE,
                            false,
                            icon_size::SMALL,
                            mix(p.t3, p.t1, hover),
                        ));
                        if x.has_focus() {
                            focus_ring(
                                painter,
                                Rect::from_center_size(c, egui::Vec2::splat(control::WORD_X)),
                                CornerRadius::same(radius::PILL),
                                &theme,
                            );
                        }
                        if tooltip::attach(x, strings::library::CLEAR_SEARCH()).clicked() {
                            self.text.clear();
                            cleared = true;
                            r.request_focus();
                        }
                    }
                    if let Some(err) = self.error {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S1;
                            ui.label(icons::icon(icons::WARNING, icon_size::SMALL).color(p.coral));
                            ui.label(Type::Caption.rich(err).color(p.coral));
                        });
                        a11y::describe(&r, err);
                    } else if let Some(help) = self.help {
                        ui.label(Type::Caption.rich(help).color(p.t3));
                        a11y::describe(&r, help);
                    }
                    r
                },
            )
            .inner;
        TextFieldResponse {
            response: resp,
            cleared,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_is_30_high_and_a_multiline_one_at_least_76() {
        let (a, b) = crate::widgets::test_frame(&Default::default(), |ui| {
            let mut s = String::new();
            let mut t = String::new();
            let a = TextField::new(&mut s, "a").search().show(ui).response;
            let top = ui.cursor().min.y;
            let _ = TextField::new(&mut t, "b").multiline().show(ui);
            (a.rect, ui.cursor().min.y - top)
        });
        assert!(a.height() <= control::BUTTON);
        assert!(b >= layout::TEXTAREA_MIN_H);
    }
}
