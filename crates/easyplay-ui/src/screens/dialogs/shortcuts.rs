//! The keyboard shortcuts dialog (`?`, `F1`; script `shortcutsDialog`), generated from the
//! app's shortcut table: groups under 15 pt headings, rows 32 high over hairlines with the
//! keycaps at the right and "or" between alternatives. A 440 wide dialog with a close
//! button and no actions.

use egui::{Context, Id, Sense, Stroke, Ui, pos2, vec2};

use crate::app::UiState;
use crate::input::shortcuts::{Scope, TABLE};
use crate::route::Overlay;
use crate::strings;
use crate::theme::tokens::{overlay, stroke};
use crate::theme::{self, Type};
use crate::widgets::key_hint::{key_hint_shapes, key_hint_size};
use crate::widgets::modal::{Spec, show as dialog};
use crate::widgets::rows::heading;

fn scope_title(s: Scope) -> &'static str {
    match s {
        Scope::Global => strings::shortcuts::SCOPE_GLOBAL(),
        Scope::Library => strings::shortcuts::SCOPE_LIBRARY(),
        Scope::GamePage => strings::shortcuts::SCOPE_GAME(),
        Scope::Pages => strings::shortcuts::SCOPE_PAGES(),
        Scope::Games => strings::shortcuts::SCOPE_GAMES(),
    }
}

/// One row: the action at the left, its keys at the right, a hairline under it.
fn row(ui: &mut Ui, label: &str, keys: &[String]) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, overlay::KEYS_ROW), Sense::hover());
    let gap = overlay::KEYS_GAP;
    let or = Type::Caption.line(ui, strings::shortcuts::OR(), p.t3);
    let total: f32 = keys.iter().map(|k| key_hint_size(ui, k).x).sum::<f32>()
        + keys.len().saturating_sub(1) as f32 * (or.size().x + gap * 2.0);
    let g = Type::Body.galley(ui, label, p.t1, (w - total - gap * 2.0).max(1.0), 1);
    let cy = rect.center().y;
    let painter = ui.painter();
    painter.galley(pos2(rect.left(), cy - g.size().y / 2.0), g, p.t1);
    let mut x = rect.right() - total;
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            x += gap;
            painter.galley(pos2(x, cy - or.size().y / 2.0), or.clone(), p.t3);
            x += or.size().x + gap;
        }
        painter.extend(key_hint_shapes(painter, pos2(x, cy), k, p.t3, &theme));
        x += key_hint_size(ui, k).x;
    }
    painter.hline(
        rect.x_range(),
        rect.bottom() - stroke::BORDER / 2.0,
        Stroke::new(stroke::BORDER, p.line),
    );
    let spoken = format!(
        "{label}: {}",
        keys.join(&format!(" {} ", strings::shortcuts::OR()))
    );
    crate::a11y::labelled(ui.ctx(), resp.id, egui::Role::Row, &spoken);
}

/// Show the dialog.
pub fn show(ctx: &Context, id: Id, state: &mut UiState) {
    let r = dialog(
        ctx,
        id,
        Spec::new(strings::shortcuts::TITLE())
            .close_button()
            .no_actions(),
        |ui| {
            for (i, scope) in [
                Scope::Global,
                Scope::Library,
                Scope::Games,
                Scope::GamePage,
                Scope::Pages,
            ]
            .into_iter()
            .enumerate()
            {
                let rows: Vec<_> = TABLE.iter().filter(|b| b.scope == scope).collect();
                if rows.is_empty() {
                    continue;
                }
                heading(ui, scope_title(scope), i == 0);
                for b in rows {
                    let keys: Vec<String> = b.keys.iter().map(|k| k.label()).collect();
                    row(ui, (b.label)(), &keys);
                }
            }
        },
        |_| (),
    );
    if r.close_requested {
        state.nav.close(Overlay::Shortcuts);
    }
}
