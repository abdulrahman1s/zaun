//! Sidebar (SCREEN_SPECS §0, USER_EXPERIENCE §2): brand row, collections with counts, tags,
//! Statistics, host status and Settings pinned at the bottom. Collapses to an icon rail.
//! Arrow keys move between items; `Enter` selects.

use egui::accesskit::Role;
use egui::{
    Align, Align2, CornerRadius, Id, Key, Layout, Rect, Response, Sense, Stroke, Ui, WidgetText,
    pos2, vec2,
};

use crate::a11y;
use crate::app::UiState;
use crate::bridge::HostStatus;
use crate::route::{Route, SettingsTab};
use crate::screens::Env;
use crate::strings;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Icon, Type, icons};
use crate::vm::library::{Collection, LibraryVm};
use crate::widgets::focus_ring;
use crate::widgets::icon_button::IconButton;

fn collection_icon(c: Collection) -> Icon {
    match c {
        Collection::All => icons::LIBRARY,
        Collection::Favorites => icons::STAR,
        Collection::Recent => icons::RECENT,
        Collection::NeverPlayed => icons::GAMEPAD,
        Collection::Running => icons::PLAY,
        Collection::Installed => icons::FOLDER,
        Collection::Missing => icons::LINK_BREAK,
        Collection::Installers => icons::DOWNLOAD,
        Collection::Hidden => icons::EYE_OFF,
        Collection::Tag(_) => icons::TAG,
    }
}

/// How an item's leading mark is drawn.
enum Lead {
    Icon(Icon),
    Dot(egui::Color32),
}

#[allow(clippy::too_many_arguments)]
fn item(
    ui: &mut Ui,
    id: Id,
    lead: Lead,
    label: &str,
    count: Option<usize>,
    selected: bool,
    collapsed: bool,
    accessible: &str,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, layout::SIDEBAR_ITEM), Sense::hover());
    let resp = ui.interact(rect, id, Sense::click());
    crate::widgets::own_arrow_keys(&resp);
    a11y::option(&resp, Role::Link, accessible, selected);
    if collapsed {
        let resp2 = resp.clone().on_hover_text(label);
        let _ = resp2;
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let corner = CornerRadius::same(radius::SM);
        let hovered = resp.hovered();
        if selected {
            painter.rect_filled(rect, corner, p.accent_subtle);
            let bar = Rect::from_min_size(
                pos2(rect.left(), rect.top() + space::S2),
                vec2(3.0, rect.height() - space::S4),
            );
            painter.rect_filled(bar, CornerRadius::same(radius::XS), p.accent);
        } else if hovered {
            painter.rect_filled(rect, corner, fade(p.bg_raised, 0.9));
        }
        let fg = if selected {
            p.text_primary
        } else {
            mix(
                p.text_secondary,
                p.text_primary,
                f32::from(u8::from(hovered)) * 0.6,
            )
        };
        let lead_c = if collapsed {
            rect.center().x
        } else {
            rect.left() + space::S3 + icon_size::BUTTON / 2.0
        };
        let cy = rect.center().y;
        match lead {
            Lead::Icon(icon) => {
                painter.text(
                    pos2(lead_c, cy),
                    Align2::CENTER_CENTER,
                    icon.text(selected),
                    icons::font(icon_size::BUTTON - 2.0),
                    if selected { p.accent } else { fg },
                );
            }
            Lead::Dot(c) => {
                painter.circle_filled(pos2(lead_c, cy), 4.0, c);
            }
        }
        if !collapsed {
            let x = rect.left() + space::S3 + icon_size::BUTTON + space::S3;
            let count_g = count.map(|n| {
                WidgetText::from(Type::Caption.numeric(n.to_string())).into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    Type::Caption.numeric_font_id(),
                )
            });
            let count_w = count_g.as_ref().map_or(0.0, |g| g.size().x + space::S3);
            let g = WidgetText::from(label).into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                rect.right() - x - count_w - space::S2,
                if selected {
                    Type::BodyStrong.font_id()
                } else {
                    Type::Body.font_id()
                },
            );
            painter.galley(pos2(x, cy - g.size().y / 2.0), g, fg);
            if let Some(cg) = count_g {
                painter.galley(
                    pos2(
                        rect.right() - space::S3 - cg.size().x,
                        cy - cg.size().y / 2.0,
                    ),
                    cg,
                    p.text_muted,
                );
            }
        }
        if resp.has_focus() {
            focus_ring(painter, rect.shrink(1.0), corner, &theme);
        }
    }
    resp
}

fn section_label(ui: &mut Ui, text: &str, collapsed: bool) {
    let theme = theme::theme(ui.ctx());
    ui.add_space(space::S4);
    if collapsed {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().hline(
            r.shrink2(vec2(space::S3, 0.0)).x_range(),
            r.center().y,
            Stroke::new(stroke::BORDER, theme.palette.border_subtle),
        );
        ui.add_space(space::S2);
    } else {
        ui.horizontal(|ui| {
            ui.add_space(space::S3);
            ui.label(Type::Micro.rich(text).color(theme.palette.text_muted));
        });
        ui.add_space(space::S1);
    }
}

/// Paint the sidebar. `collapsed` draws the icon rail.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, collapsed: bool) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let full = ui.max_rect();
    // Right border.
    ui.painter().vline(
        full.right() - 0.5,
        full.y_range(),
        Stroke::new(stroke::BORDER, p.border_subtle),
    );
    a11y::labelled(
        ui.ctx(),
        ui.scope_id(),
        Role::Navigation,
        strings::nav::LANDMARK,
    );

    let lib = env.bridge.library().clone();
    let loaded = lib.revision > 0;
    let route = state.nav.current();
    let mut ids: Vec<Id> = Vec::new();
    let base = Id::unique("sidebar.items");
    let pad = if collapsed { space::S2 } else { space::S3 };

    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(pad as i8, space::S4 as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = space::S0;
            // Brand row with the collapse toggle.
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), layout::SIDEBAR_ITEM),
                Layout::left_to_right(Align::Center),
                |ui| {
                    if !collapsed {
                        ui.add_space(space::S2);
                        ui.label(
                            icons::icon_filled(icons::SHIELD_OFFLINE, icon_size::NAV)
                                .color(p.accent),
                        );
                        ui.label(Type::Heading.rich(strings::APP_NAME).color(p.text_primary));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if collapsed {
                            strings::nav::EXPAND
                        } else {
                            strings::nav::COLLAPSE
                        };
                        if ui
                            .add(
                                IconButton::new(icons::SIDEBAR, label)
                                    .shortcut(strings::nav::SIDEBAR_SHORTCUT),
                            )
                            .clicked()
                        {
                            state.sidebar_pref = Some(!collapsed);
                        }
                    });
                },
            );

            let bottom_h = layout::SIDEBAR_ITEM * 3.0 + space::S4 + space::S1;
            egui::ScrollArea::vertical()
                .id_salt("sidebar.scroll")
                .max_height((ui.available_height() - bottom_h).max(layout::SIDEBAR_ITEM))
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = space::S0;
                    section_label(ui, strings::nav::LIBRARY, collapsed);
                    for c in Collection::FIXED {
                        let n = LibraryVm::count(&lib, c, env.now);
                        if c.conditional() && n == 0 {
                            continue;
                        }
                        let selected = route == Route::Library && state.library.collection == c;
                        let id = base.with(("c", c));
                        ids.push(id);
                        let count = loaded.then_some(n);
                        let r = item(
                            ui,
                            id,
                            Lead::Icon(collection_icon(c)),
                            c.label(),
                            count,
                            selected,
                            collapsed,
                            &strings::nav::item_with_count(c.label(), n),
                        );
                        if r.clicked() {
                            state.library.collection = c;
                            state.nav.go_to(Route::Library);
                        }
                    }

                    if !lib.tags.is_empty() {
                        section_label(ui, strings::nav::TAGS, collapsed);
                        for t in lib.tags.iter() {
                            let c = Collection::Tag(t.id);
                            let n = LibraryVm::count(&lib, c, env.now);
                            let selected = route == Route::Library && state.library.collection == c;
                            let id = base.with(("t", t.id));
                            ids.push(id);
                            let r = item(
                                ui,
                                id,
                                Lead::Dot(p.tag(t.color.index())),
                                &t.name,
                                loaded.then_some(n),
                                selected,
                                collapsed,
                                &strings::nav::item_with_count(&t.name, n),
                            );
                            if r.clicked() {
                                state.library.collection = c;
                                state.nav.go_to(Route::Library);
                            }
                        }
                    }
                });

            // Bottom group, pinned: Statistics, host status, Settings.
            let spare = ui.available_height() - bottom_h;
            if spare > 0.0 {
                ui.add_space(spare);
            }
            let id = base.with("stats");
            ids.push(id);
            if item(
                ui,
                id,
                Lead::Icon(icons::CHART),
                strings::nav::STATISTICS,
                None,
                route == Route::Statistics,
                collapsed,
                strings::nav::STATISTICS,
            )
            .clicked()
            {
                state.nav.go_to(Route::Statistics);
            }
            let (r, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), space::S4), Sense::hover());
            ui.painter().hline(
                r.shrink2(vec2(space::S2, 0.0)).x_range(),
                r.center().y,
                Stroke::new(stroke::BORDER, p.border_subtle),
            );
            let (dot, label) = match env.bridge.host_status() {
                HostStatus::Ready => (p.success, strings::host::READY),
                HostStatus::NeedsAttention => (p.warning, strings::host::NEEDS_ATTENTION),
                HostStatus::Busy => (p.info, strings::host::BUSY),
                HostStatus::Unknown => (p.text_muted, strings::host::CHECKING),
            };
            let id = base.with("host");
            ids.push(id);
            item(
                ui,
                id,
                Lead::Dot(dot),
                label,
                None,
                false,
                collapsed,
                &strings::host::status(label),
            );
            let id = base.with("settings");
            ids.push(id);
            if item(
                ui,
                id,
                Lead::Icon(icons::GEAR),
                strings::nav::SETTINGS,
                None,
                matches!(route, Route::Settings(_)),
                collapsed,
                strings::nav::SETTINGS,
            )
            .clicked()
            {
                state.nav.go_to(Route::Settings(SettingsTab::General));
            }
        });

    // Arrow keys move focus between items.
    let focused = ui.ctx().memory(|m| m.focused());
    if let Some(pos) = focused.and_then(|f| ids.iter().position(|i| *i == f)) {
        let (up, down) = ui.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown)));
        let target = if up && pos > 0 {
            Some(pos - 1)
        } else if down && pos + 1 < ids.len() {
            Some(pos + 1)
        } else {
            None
        };
        if let Some(t) = target {
            ui.ctx().memory_mut(|m| m.request_focus(ids[t]));
        }
    }
}
