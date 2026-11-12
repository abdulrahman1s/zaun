//! List view (SCREEN_SPECS §2): `icon · Name · ◆ Protection · Last played · Playtime · ▶`,
//! rows through `ScrollArea::show_rows`. Foundation version; sortable headers, the column
//! chooser and persisted widths arrive with UI-010.

use std::sync::Arc;

use egui::accesskit::Role;
use egui::{Align, CornerRadius, Layout, ScrollArea, Sense, Stroke, Ui, vec2};

use super::ItemAction;
use crate::a11y;
use crate::app::UiState;
use crate::bridge::LibraryView;
use crate::screens::Env;
use crate::strings;
use crate::theme::tokens::{icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Type};
use crate::widgets::button::{PillSize, play_pill};
use crate::widgets::card::play_state;
use crate::widgets::cover::{CoverSource, paint_cover};
use crate::widgets::shield;

/// Show the list; returns what the user asked for.
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    lib: &Arc<LibraryView>,
    visible: &Arc<[u32]>,
) -> Option<ItemAction> {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let mut action = None;
    let wide = ui.available_width() > 720.0;
    ScrollArea::vertical()
        .id_salt(("library.list", state.library.collection))
        .auto_shrink([false, false])
        .show_rows(ui, layout::ROW_H, visible.len(), |ui, range| {
            a11y::labelled(ui.ctx(), ui.scope_id(), Role::Table, strings::library::GRID);
            for pos in range {
                let g = &lib.games[visible[pos] as usize];
                let (rect, _) = ui
                    .allocate_exact_size(vec2(ui.available_width(), layout::ROW_H), Sense::hover());
                let id = egui::Id::unique(("row", g.id));
                let resp = ui.interact(rect, id, Sense::click());
                let state_text = strings::card::state(g.activity, g.availability, env.now);
                let name = strings::card::accessible_name(
                    &g.name,
                    g.preset,
                    state_text.as_deref(),
                    g.total_playtime_ms,
                    g.last_played_at,
                    env.now,
                    env.utc_offset_s,
                );
                a11y::option(
                    &resp,
                    Role::Row,
                    &name,
                    state.library.selected == Some(g.id),
                );
                if resp.hovered() || state.library.selected == Some(g.id) {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(radius::SM), p.bg_surface);
                }
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() - 0.5,
                    Stroke::new(stroke::BORDER, p.border_subtle),
                );
                let inner = rect.shrink2(vec2(space::S2, 0.0));
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(inner)
                        .layout(Layout::left_to_right(Align::Center)),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = space::S3;
                        let (ir, _) = ui.allocate_exact_size(
                            vec2(icon_size::NAV + 4.0, icon_size::NAV + 4.0),
                            Sense::hover(),
                        );
                        paint_cover(
                            ui.painter(),
                            ir,
                            CornerRadius::same(radius::XS),
                            CoverSource::Placeholder,
                            &g.name,
                            &theme,
                        );
                        ui.add_sized(
                            vec2((ui.available_width() * 0.36).max(120.0), layout::ROW_H),
                            egui::Label::new(Type::BodyStrong.rich(&*g.name).color(p.text_primary))
                                .truncate(),
                        );
                        shield::badge(
                            ui,
                            g.preset,
                            g.flags.needs_attention || g.flags.suspected_tamper,
                            icon_size::INLINE,
                        );
                        if wide {
                            ui.add_sized(
                                vec2(80.0, layout::ROW_H),
                                egui::Label::new(
                                    Type::Body
                                        .rich(strings::shield::preset_name(g.preset))
                                        .color(p.text_secondary),
                                ),
                            );
                        }
                        let meta = state_text.clone().unwrap_or_else(|| {
                            strings::card::played(
                                g.total_playtime_ms,
                                g.last_played_at,
                                env.now,
                                env.utc_offset_s,
                            )
                        });
                        ui.label(Type::Body.numeric(meta).color(p.text_muted));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if play_pill(ui, &play_state(g), &g.name, PillSize::Row, 1.0).clicked()
                            {
                                action = Some(ItemAction::Play(g.id));
                            }
                        });
                    },
                );
                if action.is_none() && resp.clicked() {
                    action = Some(ItemAction::Open(g.id));
                }
            }
        });
    action
}
