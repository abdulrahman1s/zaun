//! Tabs: plain words 36 high, 13 / 500, quiet text (secondary under the pointer, primary
//! when selected), 22 apart (14 in a tight column), over a 1 pt `line` rule as wide as the
//! bar. A 2 pt accent underline slides and resizes to the selected word (220 ms,
//! `ease_out`). Something can stand at the bar's right end (the game page pins a compact
//! Play there). Arrow keys move one tab, Home and End to the ends; the words scroll sideways
//! when they do not fit.

use egui::accesskit::Role;
use egui::{
    Align, Color32, CornerRadius, Key, Layout, Modifiers, Rect, Response, ScrollArea, Sense, Shape,
    Ui, UiBuilder, pos2, vec2,
};

use super::{focus_ring, pointer_t};
use crate::a11y;
use crate::theme::palette::mix;
use crate::theme::tokens::{control, layout, radius, stroke};
use crate::theme::{self, Dur, Type, motion};

/// A tab bar labelled `label` (for assistive technology) over `names`.
#[must_use = "call `show` or `show_with`"]
pub struct Tabs<'a> {
    label: &'a str,
    names: &'a [&'a str],
    gap: f32,
    indent: f32,
}

impl<'a> Tabs<'a> {
    /// Tabs 22 apart.
    pub fn new(label: &'a str, names: &'a [&'a str]) -> Self {
        Self {
            label,
            names,
            gap: layout::TAB_GAP[0],
            indent: 0.0,
        }
    }

    /// Start the words `indent` in from the bar's left edge: a page whose column stands in
    /// the middle of a wide window keeps its tabs on the column while the rule spans the page.
    pub fn indent(mut self, indent: f32) -> Self {
        self.indent = indent.max(0.0);
        self
    }

    /// The gap between tabs ([`layout::TAB_GAP`]: 22, or 14 in a tight column).
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Show the bar; `selected` changes on click or arrow keys. Returns the list's response
    /// (`changed()` when the selection moved).
    pub fn show(self, ui: &mut Ui, selected: &mut usize) -> Response {
        self.show_with(ui, selected, |_| {}).0
    }

    /// Show the bar with `trailing` laid out right to left at its right end, centred with a
    /// 3 margin under it (CSS `align-self: center; margin-bottom: 3px`): its middle 1.5
    /// above the bar's.
    pub fn show_with<R>(
        self,
        ui: &mut Ui,
        selected: &mut usize,
        trailing: impl FnOnce(&mut Ui) -> R,
    ) -> (Response, R) {
        let theme = theme::theme(ui.ctx());
        let p = theme.palette.clone();
        let id = ui.make_persistent_id(("tabs", self.label));
        let full_w = ui.available_width();
        let (bar, _) = ui.allocate_exact_size(vec2(full_w, layout::TAB_H), Sense::hover());
        // The rule under the whole bar (an inner shadow in the design: inside its bottom).
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(bar.left(), bar.bottom() - stroke::BORDER), bar.max),
            CornerRadius::ZERO,
            p.line,
        );
        let mut end = ui.new_child(
            UiBuilder::new()
                .id_salt(id.with("end"))
                .max_rect(Rect::from_min_max(
                    bar.min,
                    pos2(bar.right(), bar.bottom() - control::TAB_TRAIL_LIFT),
                ))
                .layout(Layout::right_to_left(Align::Center)),
        );
        let out = trailing(&mut end);
        let end_w = if end.min_rect().width() > 0.0 {
            end.min_rect().width() + self.gap
        } else {
            0.0
        };
        let words = Rect::from_min_max(
            pos2(bar.left() + self.indent, bar.top()),
            pos2(bar.right() - end_w, bar.bottom()),
        );
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt(id.with("words"))
                .max_rect(words)
                .layout(Layout::left_to_right(Align::Min)),
        );
        let before = *selected;
        let names = self.names;
        let gap = self.gap;
        let scroll = ScrollArea::horizontal()
            .id_salt(id)
            .max_width(words.width())
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.horizontal(|ui| {
                    let origin = ui.cursor().min.x;
                    let mut ink: Option<(f32, f32, f32)> = None;
                    for (i, name) in names.iter().enumerate() {
                        let g = Type::BodyStrong.line(ui, name, Color32::PLACEHOLDER);
                        let (rect, _) =
                            ui.allocate_exact_size(vec2(g.size().x, layout::TAB_H), Sense::hover());
                        let resp = ui.interact(rect, id.with(i), Sense::click());
                        super::own_arrow_keys(&resp);
                        a11y::option(&resp, Role::Tab, name, *selected == i);
                        if resp.clicked() {
                            *selected = i;
                        }
                        // A word that does not fit scrolls into view when it is reached.
                        if resp.gained_focus() || (resp.clicked() && i != before) {
                            resp.scroll_to_me(None);
                        }
                        if resp.has_focus() {
                            // Consumed, so the tab that takes focus this frame does not
                            // read the same press again: one press moves one tab.
                            let (l, r, home, end) = ui.input_mut(|inp| {
                                (
                                    inp.consume_key(Modifiers::NONE, Key::ArrowLeft),
                                    inp.consume_key(Modifiers::NONE, Key::ArrowRight),
                                    inp.consume_key(Modifiers::NONE, Key::Home),
                                    inp.consume_key(Modifiers::NONE, Key::End),
                                )
                            });
                            let last = names.len().saturating_sub(1);
                            let target = if home && i != 0 {
                                Some(0)
                            } else if end && i != last {
                                Some(last)
                            } else if l && i > 0 {
                                Some(i - 1)
                            } else if r && i < last {
                                Some(i + 1)
                            } else {
                                None
                            };
                            if let Some(t) = target {
                                *selected = t;
                                ui.memory_mut(|m| m.request_focus(id.with(t)));
                            }
                        }
                        let is_sel = *selected == i;
                        let hover = if ui.is_enabled() {
                            pointer_t(&resp, &theme)
                        } else {
                            0.0
                        };
                        let fg = if is_sel { p.t1 } else { mix(p.t3, p.t2, hover) };
                        ui.painter().add(Shape::galley(
                            pos2(rect.left(), rect.center().y - g.size().y / 2.0),
                            g,
                            fg,
                        ));
                        if is_sel {
                            ink = Some((rect.left() - origin, rect.width(), rect.bottom()));
                        }
                        if resp.has_focus() {
                            // Inside the tab (CSS `outline-offset: -3px`).
                            let inset = control::TAB_RING_INSET + stroke::FOCUS_GAP;
                            focus_ring(
                                ui.painter(),
                                rect.shrink(inset),
                                CornerRadius::same(radius::XS),
                                &theme,
                            );
                        }
                    }
                    // The underline slides to the selected word, relative to the bar so a
                    // scrolled page does not make it trail.
                    if let Some((x, w, bottom)) = ink {
                        let secs = theme.motion.movement(Dur::Base);
                        let x = motion::glide(ui.ctx(), id.with("ink.x"), x, secs);
                        let w = motion::glide(ui.ctx(), id.with("ink.w"), w, secs);
                        ui.painter().rect_filled(
                            Rect::from_min_max(
                                pos2(origin + x, bottom - stroke::FOCUS),
                                pos2(origin + x + w, bottom),
                            ),
                            CornerRadius::ZERO,
                            p.accent_mark,
                        );
                    }
                })
            });
        let mut list = scroll.inner.response;
        a11y::labelled(ui.ctx(), list.id, Role::TabList, self.label);
        if *selected != before {
            list.mark_changed();
        }
        (list, out)
    }
}

/// Show tabs 22 apart; `selected` changes on click or arrow keys. Returns the list's
/// response (`changed()` when the selection moved).
pub fn tabs(ui: &mut Ui, label: &str, selected: &mut usize, names: &[&str]) -> Response {
    Tabs::new(label, names).show(ui, selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_is_36_high() {
        let r = crate::widgets::test_frame(&Default::default(), |ui| {
            let mut i = 0;
            let top = ui.cursor().min.y;
            let _ = tabs(ui, "Game", &mut i, &["Overview", "Protection"]);
            ui.cursor().min.y - top
        });
        assert_eq!(r, layout::TAB_H);
    }
}
