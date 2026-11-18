//! Placing things from the start of a row: the panel's own layout mirrors in Arabic (the
//! screen corners do not, `closed`).
use egui::{Rect, pos2, vec2};

#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub rect: Rect,
    pub rtl: bool,
}

impl Row {
    pub fn new(rect: Rect, rtl: bool) -> Self {
        Self { rect, rtl }
    }

    /// The rect `from_start` in from the row's start, `w` wide, at `top` (absolute), `h` high.
    pub fn span(&self, from_start: f32, w: f32, top: f32, h: f32) -> Rect {
        let x = if self.rtl {
            self.rect.right() - from_start - w
        } else {
            self.rect.left() + from_start
        };
        Rect::from_min_size(pos2(x, top), vec2(w, h))
    }

    /// The same, measured from the row's end.
    pub fn end_span(&self, from_end: f32, w: f32, top: f32, h: f32) -> Rect {
        self.span(self.rect.width() - from_end - w, w, top, h)
    }

    /// The x of a point `from_start` in.
    pub fn x(&self, from_start: f32) -> f32 {
        if self.rtl {
            self.rect.right() - from_start
        } else {
            self.rect.left() + from_start
        }
    }

    /// The anchor of text that starts at a point.
    pub fn start_anchor(&self, v: egui::Align) -> egui::Align2 {
        let h = if self.rtl {
            egui::Align::RIGHT
        } else {
            egui::Align::LEFT
        };
        egui::Align2([h, v])
    }

    pub fn end_anchor(&self, v: egui::Align) -> egui::Align2 {
        let h = if self.rtl {
            egui::Align::LEFT
        } else {
            egui::Align::RIGHT
        };
        egui::Align2([h, v])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_mirrors_in_right_to_left_rows() {
        let r = Rect::from_min_size(pos2(100.0, 0.0), vec2(200.0, 50.0));
        assert_eq!(Row::new(r, false).span(10.0, 30.0, 0.0, 5.0).left(), 110.0);
        assert_eq!(Row::new(r, true).span(10.0, 30.0, 0.0, 5.0).right(), 290.0);
        assert_eq!(
            Row::new(r, true).end_span(10.0, 30.0, 0.0, 5.0).left(),
            110.0
        );
    }
}
