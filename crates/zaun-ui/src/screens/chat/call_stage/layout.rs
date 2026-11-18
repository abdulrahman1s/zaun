//! Where the call box's parts go (`chat-calls.html` notes D): the stage's height, the grid
//! of 2–8 tiles, and a focused tile over its strip. Pure: rectangles in, rectangles out.

use egui::{Rect, Vec2, pos2, vec2};

/// The measures (notes D).
pub mod m {
    use egui::Vec2;

    /// The tray's height (the box made smaller).
    pub const TRAY: f32 = 60.0;
    /// The stage: share of the window's height and its least and most height.
    pub const STAGE: (f32, f32, f32) = (0.44, 240.0, 560.0);
    /// The same while a shared game has the big tile.
    pub const STAGE_STREAM: (f32, f32, f32) = (0.60, 320.0, 720.0);
    /// The grip's range: least height, most share of the conversation's height.
    pub const GRIP_RANGE: (f32, f32) = (200.0, 0.70);
    /// Let go under this height, the grip makes the box the tray.
    pub const GRIP_TRAY: f32 = 160.0;
    /// The grip's keyboard step.
    pub const GRIP_STEP: f32 = 24.0;
    /// Padding of the tiles' area (top and sides), and between tiles.
    pub const PAD: f32 = 12.0;
    /// See [`PAD`].
    pub const GAP: f32 = 8.0;
    /// The control bar's height.
    pub const BAR: f32 = 64.0;
    /// A column is never narrower than this; the grid uses one column fewer instead.
    pub const MIN_COL: f32 = 150.0;
    /// Four tiles go in one row when the area is wider than this ratio.
    pub const FOUR_IN_ROW: f32 = 3.2;
    /// The avatar: share of the tile's shorter side, least and most.
    pub const AVATAR: (f32, f32, f32) = (0.4, 32.0, 96.0);
    /// A strip tile and the strip's avatar.
    pub const STRIP: Vec2 = Vec2::new(112.0, 64.0);
    /// See [`STRIP`].
    pub const STRIP_AVATAR: f32 = 32.0;
}

/// The stage's height in a window `window_h` high; taller while a shared game has the big
/// tile. 396 at 900, 440 at 1000.
pub fn stage_height(window_h: f32, stream: bool) -> f32 {
    let (share, lo, hi) = if stream { m::STAGE_STREAM } else { m::STAGE };
    (window_h * share).clamp(lo, hi)
}

/// The grip's height clamped to its range in a conversation `convo_h` high.
pub fn grip_clamp(h: f32, convo_h: f32) -> f32 {
    let (lo, share) = m::GRIP_RANGE;
    h.clamp(lo, (convo_h * share).max(lo))
}

/// Columns for `n` tiles in `area`.
pub fn columns(n: usize, area: Vec2) -> usize {
    let mut cols: usize = match n {
        0..=2 => n.max(1),
        3 => 3,
        4 if area.x > area.y * m::FOUR_IN_ROW => 4,
        4 => 2,
        5 | 6 => 3,
        _ => 4,
    };
    while cols > 1 && col_width(area.x, cols) < m::MIN_COL {
        cols -= 1;
    }
    cols
}

fn col_width(w: f32, cols: usize) -> f32 {
    (w - m::GAP * (cols as f32 - 1.0)) / cols as f32
}

/// The rectangles of `n` tiles in `area`: exactly `cols` to a row, at most 16:9, the last
/// row and the whole group centred.
pub fn grid(n: usize, area: Rect) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    let cols = columns(n, area.size());
    let rows = n.div_ceil(cols);
    let h = ((area.height() - m::GAP * (rows as f32 - 1.0)) / rows as f32).max(1.0);
    let w = col_width(area.width(), cols).min(h * 16.0 / 9.0).max(1.0);
    let group_h = h * rows as f32 + m::GAP * (rows as f32 - 1.0);
    let top = area.center().y - group_h / 2.0;
    (0..n)
        .map(|i| {
            let (row, col) = (i / cols, i % cols);
            let in_row = (n - row * cols).min(cols);
            let row_w = w * in_row as f32 + m::GAP * (in_row as f32 - 1.0);
            let left = area.center().x - row_w / 2.0;
            Rect::from_min_size(
                pos2(
                    left + col as f32 * (w + m::GAP),
                    top + row as f32 * (h + m::GAP),
                ),
                vec2(w, h),
            )
        })
        .collect()
}

/// A focused tile, 16:9 as large as fits above the strip, and the strip's `n` tiles under
/// it, centred.
pub fn focused(n: usize, area: Rect) -> (Rect, Vec<Rect>) {
    let strip_h = if n == 0 { 0.0 } else { m::STRIP.y + m::GAP };
    let room = vec2(area.width(), (area.height() - strip_h).max(1.0));
    let w = room.x.min(room.y * 16.0 / 9.0);
    let big = Rect::from_center_size(
        pos2(area.center().x, area.top() + room.y / 2.0),
        vec2(w, w * 9.0 / 16.0),
    );
    let strip_w = m::STRIP.x * n as f32 + m::GAP * (n as f32 - 1.0).max(0.0);
    let left = area.center().x - strip_w / 2.0;
    let y = area.bottom() - m::STRIP.y;
    let strip = (0..n)
        .map(|i| Rect::from_min_size(pos2(left + i as f32 * (m::STRIP.x + m::GAP), y), m::STRIP))
        .collect();
    (big, strip)
}

/// The avatar's diameter in a tile of `size`.
pub fn avatar(size: Vec2) -> f32 {
    let (share, lo, hi) = m::AVATAR;
    (size.x.min(size.y) * share).clamp(lo, hi)
}

/// Mirror `r` inside `within` for a right-to-left interface.
pub fn mirror(r: Rect, within: Rect, rtl: bool) -> Rect {
    if !rtl {
        return r;
    }
    let x = within.left() + within.right() - r.right();
    Rect::from_min_size(pos2(x, r.top()), r.size())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tiles' area of a 1440 × 900 window with the list and no details: the box is
    /// 1091 wide and 396 high, less the padding and the bar.
    fn area_1440(box_h: f32) -> Rect {
        Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(1091.0 - m::PAD * 2.0, box_h - m::PAD - m::BAR),
        )
    }

    #[test]
    fn the_stage_takes_44_percent_clamped_and_60_with_a_stream() {
        assert_eq!(stage_height(900.0, false), 396.0);
        assert_eq!(stage_height(1000.0, false), 440.0);
        assert_eq!(stage_height(400.0, false), 240.0);
        assert_eq!(stage_height(2000.0, false), 560.0);
        assert_eq!(stage_height(900.0, true), 540.0);
        assert_eq!(stage_height(1400.0, true), 720.0);
    }

    #[test]
    fn the_grip_stays_between_200_and_70_percent() {
        assert_eq!(grip_clamp(120.0, 800.0), 200.0);
        assert_eq!(grip_clamp(700.0, 800.0), 560.0);
        assert_eq!(grip_clamp(300.0, 800.0), 300.0);
    }

    #[test]
    fn columns_follow_the_count_and_never_go_under_150() {
        let wide = vec2(1067.0, 320.0);
        assert_eq!(columns(2, wide), 2);
        assert_eq!(columns(3, wide), 3);
        assert_eq!(columns(4, wide), 4, "wider than 3.2 : 1");
        assert_eq!(columns(4, vec2(900.0, 320.0)), 2);
        assert_eq!(columns(6, wide), 3);
        assert_eq!(columns(8, wide), 4);
        assert_eq!(columns(8, vec2(500.0, 320.0)), 3, "four would be under 150");
    }

    #[test]
    fn two_tiles_and_eight_tiles_fill_the_stage_with_the_avatar_sizes_of_the_notes() {
        let area = area_1440(396.0);
        let two = grid(2, area);
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].height(), 320.0);
        // 0.4 × 320 = 128, held at the most (the notes' 88 is the face inside its ring).
        assert_eq!(avatar(two[0].size()), 96.0);
        let eight = grid(8, area);
        assert_eq!(eight[0].height(), 156.0);
        assert!((avatar(eight[0].size()) - 62.4).abs() < 0.01);
    }

    #[test]
    fn rows_hold_exactly_cols_tiles_and_the_last_row_is_centred() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1800.0, 400.0));
        let five = grid(5, area);
        // Three on top, two under them, centred under the three.
        assert_eq!(five[0].top(), five[2].top());
        assert!(five[3].top() > five[0].top());
        let top_mid = (five[0].left() + five[2].right()) / 2.0;
        let low_mid = (five[3].left() + five[4].right()) / 2.0;
        assert!((top_mid - low_mid).abs() < 0.01);
        assert!((top_mid - area.center().x).abs() < 0.01);
        // Tiles are at most 16:9.
        assert!(five[0].width() <= five[0].height() * 16.0 / 9.0 + 0.01);
    }

    #[test]
    fn a_focused_tile_is_16_9_over_its_centred_strip() {
        let area = area_1440(540.0);
        let (big, strip) = focused(3, area);
        assert!((big.width() / big.height() - 16.0 / 9.0).abs() < 0.01);
        assert!(big.bottom() <= strip[0].top() - m::GAP + 0.01);
        let mid = (strip[0].left() + strip[2].right()) / 2.0;
        assert!((mid - area.center().x).abs() < 0.01);
    }
}
