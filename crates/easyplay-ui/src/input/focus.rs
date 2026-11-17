//! Grid and region focus (ACCESSIBILITY §2.1, task UX-002).
//!
//! The library grid and list are one composite widget each with **roving focus**: only the
//! cursor's card (or the first one on screen) is in the `Tab` order, so `Tab` enters the grid
//! at the cursor and the next `Tab` leaves it for the next region (top bar → filter
//! line → content). Inside, arrow keys move spatially ([`target`]); the cursor follows its game by
//! ID through re-sorting and filtering and, when the game disappears, moves to the nearest
//! remaining item ([`follow`]).

use easyplay_core::ids::GameId;
use egui::{InputState, Key, Modifiers};

/// A movement inside a grid or list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GridMove {
    /// One cell left (never wraps to the previous row).
    Left,
    /// One cell right (never wraps to the next row).
    Right,
    /// One row up, same column (clamped to a shorter row).
    Up,
    /// One row down.
    Down,
    /// `Home`: first in the row.
    RowStart,
    /// `End`: last in the row.
    RowEnd,
    /// `Ctrl+Home`: first overall.
    First,
    /// `Ctrl+End`: last overall.
    Last,
    /// `PgUp`: one viewport up.
    PageUp,
    /// `PgDn`: one viewport down.
    PageDown,
}

/// Consume this frame's movement key, if any. Returns the move and whether `Shift` was held
/// (range selection). `Ctrl` is only meaningful with `Home`/`End`.
pub fn take_move(i: &mut InputState) -> Option<(GridMove, bool)> {
    let shift = i.modifiers.shift;
    let plain = if shift {
        Modifiers::SHIFT
    } else {
        Modifiers::NONE
    };
    let ctrl = if shift {
        Modifiers::COMMAND | Modifiers::SHIFT
    } else {
        Modifiers::COMMAND
    };
    let table = [
        (ctrl, Key::Home, GridMove::First),
        (ctrl, Key::End, GridMove::Last),
        (plain, Key::ArrowLeft, GridMove::Left),
        (plain, Key::ArrowRight, GridMove::Right),
        (plain, Key::ArrowUp, GridMove::Up),
        (plain, Key::ArrowDown, GridMove::Down),
        (plain, Key::Home, GridMove::RowStart),
        (plain, Key::End, GridMove::RowEnd),
        (Modifiers::NONE, Key::PageUp, GridMove::PageUp),
        (Modifiers::NONE, Key::PageDown, GridMove::PageDown),
    ];
    table
        .into_iter()
        .find(|(m, k, _)| i.consume_key(*m, *k))
        .map(|(_, _, mv)| (mv, shift))
}

/// Where `mv` takes the cursor at visual position `pos`. `rows` are the card rows as
/// `(first position, length)` in display order (group headers excluded); `page` is the number
/// of rows in one viewport. `None` when the move is blocked (an edge).
pub fn target(rows: &[(usize, usize)], pos: usize, mv: GridMove, page: usize) -> Option<usize> {
    let row = rows.iter().position(|(s, l)| pos >= *s && pos < s + l)?;
    let (start, len) = rows[row];
    let col = pos - start;
    let in_row = |r: usize| {
        let (s, l) = rows[r];
        s + col.min(l.saturating_sub(1))
    };
    let last_row = rows.len() - 1;
    let t = match mv {
        GridMove::Left => (col > 0).then(|| pos - 1)?,
        GridMove::Right => (col + 1 < len).then_some(pos + 1)?,
        GridMove::Up => (row > 0).then(|| in_row(row - 1))?,
        GridMove::Down => (row < last_row).then(|| in_row(row + 1))?,
        GridMove::RowStart => start,
        GridMove::RowEnd => start + len - 1,
        GridMove::First => rows[0].0,
        GridMove::Last => rows[last_row].0 + rows[last_row].1 - 1,
        GridMove::PageUp => in_row(row.saturating_sub(page.max(1))),
        GridMove::PageDown => in_row((row + page.max(1)).min(last_row)),
    };
    (t != pos).then_some(t)
}

/// The rows of a one-column list of `n` items.
pub fn list_rows(n: usize) -> Vec<(usize, usize)> {
    (0..n).map(|i| (i, 1)).collect()
}

/// In a list, `Home`/`End` go to the first/last item (a row holds one item).
pub fn list_move(mv: GridMove) -> GridMove {
    match mv {
        GridMove::RowStart => GridMove::First,
        GridMove::RowEnd => GridMove::Last,
        other => other,
    }
}

/// Focus following by game ID: where the cursor `cursor` goes when the visible order changes
/// from `previous` to `visible`. The cursor stays on its game when it is still shown;
/// otherwise it moves to the nearest game of the previous order that is still shown
/// (preferring the following one, as a list does after a removal); `None` when nothing is
/// shown or there was no cursor.
pub fn follow(cursor: Option<GameId>, previous: &[GameId], visible: &[GameId]) -> Option<GameId> {
    let c = cursor?;
    if visible.contains(&c) {
        return Some(c);
    }
    let at = previous.iter().position(|g| *g == c);
    if let Some(at) = at {
        for d in 1..previous.len() {
            for i in [at.checked_add(d), at.checked_sub(d)].into_iter().flatten() {
                if let Some(g) = previous.get(i)
                    && visible.contains(g)
                {
                    return Some(*g);
                }
            }
        }
    }
    visible.first().copied()
}

/// The cell that is in the `Tab` order: the cursor when it is among the cells laid out this
/// frame, else the first laid-out cell.
pub fn rover(cursor_pos: Option<usize>, laid_out: std::ops::Range<usize>) -> Option<usize> {
    match cursor_pos {
        Some(p) if laid_out.contains(&p) => Some(p),
        _ => (!laid_out.is_empty()).then_some(laid_out.start),
    }
}

/// Whether assistive technology asked to focus the widget `id` this frame. Cells outside the
/// `Tab` order are not focusable for egui, so the grid honours the request itself by making
/// the cell the rover.
pub fn accesskit_focus_requested(ctx: &egui::Context, id: egui::Id) -> bool {
    let node = id.accesskit_id();
    ctx.input(|i| {
        i.events.iter().any(|e| {
            matches!(e, egui::Event::AccessKitActionRequest(r)
                if r.action == egui::accesskit::Action::Focus && r.target_node == node)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two rows of 5 and a last row of 2 (12 items), plus a group break after 7:
    /// rows (0,5) (5,2) (7,5).
    fn rows() -> Vec<(usize, usize)> {
        vec![(0, 5), (5, 2), (7, 5)]
    }

    #[test]
    fn arrows_move_one_cell_and_do_not_wrap() {
        let r = rows();
        assert_eq!(target(&r, 0, GridMove::Right, 2), Some(1));
        assert_eq!(target(&r, 4, GridMove::Right, 2), None, "row end");
        assert_eq!(target(&r, 5, GridMove::Left, 2), None, "row start");
        assert_eq!(
            target(&r, 3, GridMove::Down, 2),
            Some(6),
            "clamped to the short row"
        );
        assert_eq!(target(&r, 6, GridMove::Down, 2), Some(8));
        assert_eq!(target(&r, 9, GridMove::Up, 2), Some(6));
        assert_eq!(target(&r, 2, GridMove::Up, 2), None);
        assert_eq!(target(&r, 10, GridMove::Down, 2), None);
    }

    #[test]
    fn home_end_and_ctrl_variants() {
        let r = rows();
        assert_eq!(target(&r, 9, GridMove::RowStart, 2), Some(7));
        assert_eq!(target(&r, 7, GridMove::RowEnd, 2), Some(11));
        assert_eq!(target(&r, 9, GridMove::First, 2), Some(0));
        assert_eq!(target(&r, 1, GridMove::Last, 2), Some(11));
        assert_eq!(target(&r, 0, GridMove::First, 2), None, "already there");
    }

    #[test]
    fn pages_move_by_a_viewport_and_clamp() {
        let r = rows();
        assert_eq!(target(&r, 1, GridMove::PageDown, 2), Some(8));
        assert_eq!(target(&r, 8, GridMove::PageUp, 1), Some(6));
        assert_eq!(target(&r, 8, GridMove::PageUp, 9), Some(1));
        assert_eq!(target(&r, 3, GridMove::PageDown, 9), Some(10));
    }

    #[test]
    fn lists_treat_home_and_end_as_first_and_last() {
        let r = list_rows(4);
        assert_eq!(target(&r, 2, list_move(GridMove::RowStart), 1), Some(0));
        assert_eq!(target(&r, 0, list_move(GridMove::RowEnd), 1), Some(3));
        assert_eq!(target(&r, 1, GridMove::Right, 1), None);
    }

    fn ids(n: u128) -> Vec<GameId> {
        (1..=n)
            .map(|i| GameId::from_uuid(uuid::Uuid::from_u128(i)))
            .collect()
    }

    #[test]
    fn focus_follows_the_game_through_resorting() {
        let all = ids(5);
        let mut resorted = all.clone();
        resorted.reverse();
        assert_eq!(follow(Some(all[1]), &all, &resorted), Some(all[1]));
    }

    #[test]
    fn a_filtered_out_game_hands_focus_to_its_nearest_neighbour() {
        let all = ids(6);
        // Game 3 (index 2) and its successor are filtered out: the predecessor wins over a
        // farther successor.
        let shown = vec![all[0], all[1], all[4], all[5]];
        assert_eq!(follow(Some(all[2]), &all, &shown), Some(all[1]));
        // Removal: the following game takes the cursor.
        let shown = vec![all[0], all[1], all[3], all[4], all[5]];
        assert_eq!(follow(Some(all[2]), &all, &shown), Some(all[3]));
        assert_eq!(follow(Some(all[2]), &all, &[]), None);
        assert_eq!(follow(None, &all, &shown), None);
    }

    #[test]
    fn the_rover_is_the_cursor_when_laid_out() {
        assert_eq!(rover(Some(7), 5..10), Some(7));
        assert_eq!(rover(Some(20), 5..10), Some(5));
        assert_eq!(rover(None, 5..10), Some(5));
        assert_eq!(rover(None, 0..0), None);
    }
}
