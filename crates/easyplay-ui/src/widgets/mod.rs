//! Reusable controls with built-in states, keyboard behaviour and accessible names. Widgets never
//! send commands, read snapshots or allocate textures. See MODULES §8 and UI_DESIGN_SYSTEM §7.
//!
//! Every widget reads its colours and sizes from [`crate::theme::theme`]; none contains a
//! literal colour, size or duration.

pub mod banner;
pub mod bar_chart;
pub mod brand;
pub mod button;
pub mod card;
pub mod chip;
pub mod context_menu;
pub mod cover;
pub mod empty_state;
pub mod icon_button;
pub mod key_hint;
pub mod link_row;
pub mod modal;
pub mod path_row;
pub mod progress;
pub mod segmented;
pub mod select;
pub mod sheet;
pub mod shield;
pub mod skeleton;
pub mod slider;
pub mod stat_tile;
pub mod tabs;
pub mod text_field;
pub mod toast;
pub mod toggle;
pub mod tooltip;

use egui::{Context, CornerRadius, Id, Painter, Rect, Response, Stroke, StrokeKind};

use crate::theme::{Theme, tokens::stroke};

fn focus_source_id() -> Id {
    Id::unique("easyplay.focus_from_keyboard")
}

/// Remember whether focus was last moved with the keyboard (or a controller) or with the
/// pointer. Call once at the start of every frame (the shell does).
pub fn track_focus_source(ctx: &Context) {
    let (key, pointer) = ctx.input(|i| {
        (
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Key { pressed: true, .. })),
            i.pointer.any_pressed(),
        )
    });
    ctx.data_mut(|d| {
        // Until there is input, focus was placed by the application: no ring.
        let from_keyboard = d.get_temp_mut_or_insert_with(focus_source_id(), || false);
        if pointer || key {
            *from_keyboard = key && !pointer;
        }
    });
}

/// Whether focus rings are shown: after keyboard or controller input, not after a click.
/// A control that takes text shows its own focus state regardless. Without a shell (a widget
/// on its own) rings always show.
pub fn focus_visible(ctx: &Context) -> bool {
    crate::theme::controller_mode(ctx)
        || ctx
            .data(|d| d.get_temp::<bool>(focus_source_id()))
            .unwrap_or(true)
}

/// Paint the focus ring: `focus` colour, outside `rect` with a gap (ACCESSIBILITY §2). Only
/// while focus is being moved with the keyboard ([`focus_visible`]).
pub fn focus_ring(painter: &Painter, rect: Rect, corner: CornerRadius, theme: &Theme) {
    if focus_visible(painter.ctx()) {
        default_ring(painter, rect, corner, theme);
    }
}

/// The ring of a default action (the button `Enter` activates in a dialog): painted however
/// focus arrived, so the safe choice is always marked.
pub fn default_ring(painter: &Painter, rect: Rect, corner: CornerRadius, theme: &Theme) {
    // Controller mode: a larger ring, visible from the couch (USER_EXPERIENCE §5.3).
    let w = theme.focus_width()
        + if crate::theme::controller_mode(painter.ctx()) {
            1.0
        } else {
            0.0
        };
    let grow = stroke::FOCUS_GAP + w / 2.0;
    let corner = CornerRadius {
        nw: corner.nw.saturating_add(grow as u8),
        ne: corner.ne.saturating_add(grow as u8),
        sw: corner.sw.saturating_add(grow as u8),
        se: corner.se.saturating_add(grow as u8),
    };
    painter.rect_stroke(
        rect.expand(grow),
        corner,
        Stroke::new(w, theme.palette.focus),
        StrokeKind::Middle,
    );
}

/// Whether a widget shows its keyboard focus ring this frame.
pub fn shows_focus(resp: &Response) -> bool {
    resp.has_focus()
}

/// Opacity of a hover effect, animated with `dur.fast`.
pub fn hover_t(resp: &Response, theme: &Theme) -> f32 {
    resp.ctx.animate_bool_with_time(
        resp.id.with("hover"),
        resp.hovered() || resp.has_focus(),
        theme.motion.fade(crate::theme::Dur::Fast),
    )
}

/// How far a control is pressed (0–1), animated with `dur.fast`: the press deepens a fill or
/// shrinks a cover, and releases as smoothly.
pub fn press_t(resp: &Response, theme: &Theme) -> f32 {
    resp.ctx.animate_bool_with_time(
        resp.id.with("press"),
        resp.is_pointer_button_down_on(),
        theme.motion.fade(crate::theme::Dur::Fast),
    )
}

/// Keep egui's own arrow-key focus movement away from a focused composite widget (grid
/// cells, sidebar items, segmented options, tabs) that moves focus itself.
pub fn own_arrow_keys(resp: &Response) {
    if resp.has_focus() {
        resp.ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                resp.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            );
        });
    }
}
