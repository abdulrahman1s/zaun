//! Sidebar (SCREEN_SPECS §0, USER_EXPERIENCE §2): the brand in the top band, collections with
//! counts, tags, then Statistics and Settings pinned at the bottom above the host status.
//! Collapses to an icon rail. Arrow keys move between items; `Enter` selects.
//!
//! The selected item sits on a plate that glides to the next selection; the pointer gets a
//! lighter plate of its own.

use egui::accesskit::Role;
use egui::layers::ShapeIdx;
use egui::{
    Align, Align2, CornerRadius, Id, Key, Layout, Rect, Response, Sense, Shape, Stroke, Ui, pos2,
    vec2,
};

use crate::a11y;
use crate::app::UiState;
use crate::bridge::HostStatus;
use crate::route::{Route, SettingsTab};
use crate::screens::Env;
use crate::strings;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{control, icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Dur, Icon, Theme, Type, icons, motion};
use crate::vm::dialogs::AfterDiscard;
use crate::vm::library::{Collection, CollectionExt, FIXED, count};
use crate::widgets::icon_button::IconButton;
use crate::widgets::{brand, focus_ring, hover_t, tooltip};

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

/// One list of items sharing a selection plate: the scrolling collections, or the pinned
/// group at the bottom.
struct Plate {
    /// Where the plate is painted: reserved before the items so it lies under their text.
    slot: ShapeIdx,
    /// The selected item's rectangle, once laid out.
    selected: Option<Rect>,
    /// The top the plate's travel is measured from (moves with the list when it scrolls).
    origin: f32,
    id: Id,
}

impl Plate {
    fn new(ui: &Ui, id: Id) -> Self {
        Self {
            slot: ui.painter().add(Shape::Noop),
            selected: None,
            origin: ui.cursor().top(),
            id,
        }
    }

    /// Paint the plate under the selected item, gliding from the previous selection.
    fn paint(self, ui: &Ui, theme: &Theme) {
        let Some(rect) = self.selected else { return };
        let p = &theme.palette;
        let secs = theme.motion.movement(Dur::Glide);
        let y = motion::glide(ui.ctx(), self.id, rect.top() - self.origin, secs);
        let shown = motion::appear(ui.ctx(), self.id.with("in"), theme.motion.fade(Dur::Fast));
        let plate = Rect::from_min_size(pos2(rect.left(), self.origin + y), rect.size());
        let corner = CornerRadius::same(radius::ROW);
        let mut shapes = vec![Shape::rect_filled(
            plate,
            corner,
            fade(p.fill_selected, shown),
        )];
        if !p.allows_translucency() {
            shapes.push(Shape::rect_stroke(
                plate,
                corner,
                Stroke::new(stroke::BORDER, p.accent),
                egui::StrokeKind::Inside,
            ));
        }
        ui.painter().set(self.slot, Shape::Vec(shapes));
    }
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
    plate: &mut Plate,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, layout::SIDEBAR_ITEM), Sense::hover());
    let resp = ui.interact(rect, id, Sense::click());
    crate::widgets::own_arrow_keys(&resp);
    a11y::option(&resp, Role::Link, accessible, selected);
    if selected {
        plate.selected = Some(rect);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let corner = CornerRadius::same(radius::ROW);
        let hover = hover_t(&resp, &theme);
        if !selected && hover > 0.0 {
            painter.rect_filled(rect, corner, fade(p.fill_hover, hover));
        }
        let fg = if selected {
            p.text_primary
        } else {
            mix(p.text_secondary, p.text_primary, hover)
        };
        let lead_c = if collapsed {
            rect.center().x
        } else {
            rect.left() + space::S2 + 2.0 + icon_size::ROW / 2.0
        };
        let cy = rect.center().y;
        match lead {
            Lead::Icon(icon) => {
                painter.text(
                    pos2(lead_c, cy),
                    Align2::CENTER_CENTER,
                    icon.text(selected),
                    icons::font(icon_size::ROW),
                    if selected {
                        p.accent
                    } else {
                        mix(p.text_muted, p.text_primary, hover)
                    },
                );
            }
            Lead::Dot(c) => {
                painter.circle_filled(pos2(lead_c, cy), 4.0, c);
            }
        }
        if !collapsed {
            let x = lead_c + icon_size::ROW / 2.0 + space::S2 + 2.0;
            let count_g = count.map(|n| {
                Type::Caption.numeric_galley(
                    ui,
                    &n.to_string(),
                    if selected {
                        p.text_secondary
                    } else {
                        p.text_muted
                    },
                )
            });
            let count_w = count_g.as_ref().map_or(0.0, |g| g.size().x + space::S2);
            let label_type = if selected {
                Type::BodyStrong
            } else {
                Type::Body
            };
            let g = label_type.galley(
                ui,
                label,
                fg,
                rect.right() - space::S2 - 2.0 - x - count_w,
                1,
            );
            painter.galley(pos2(x, cy - g.size().y / 2.0), g, fg);
            if let Some(cg) = count_g {
                painter.galley(
                    pos2(
                        rect.right() - space::S2 - 2.0 - cg.size().x,
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
    if collapsed {
        // The rail has no labels: the tooltip carries them.
        tooltip::attach(resp.clone(), label);
    }
    resp
}

fn section_label(ui: &mut Ui, text: &str, collapsed: bool) {
    let theme = theme::theme(ui.ctx());
    if collapsed {
        ui.add_space(space::S2);
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().hline(
            r.shrink2(vec2(space::S2, 0.0)).x_range(),
            r.center().y,
            Stroke::new(stroke::BORDER, theme.palette.hairline),
        );
        ui.add_space(space::S2);
    } else {
        ui.add_space(space::S4);
        let (r, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), Type::Label.line_height() + space::S2),
            Sense::hover(),
        );
        ui.painter().text(
            pos2(r.left() + space::S2 + 2.0, r.center().y),
            Align2::LEFT_CENTER,
            text,
            Type::Label.font_id(),
            theme.palette.text_muted,
        );
    }
}

/// The brand in the top band: the mark and the wordmark, with the collapse toggle.
fn brand_row(ui: &mut Ui, state: &mut UiState, collapsed: bool) {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), layout::TOOLBAR_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            if !collapsed {
                ui.add_space(space::S2 + 2.0);
                let (r, _) = ui.allocate_exact_size(
                    vec2(layout::BRAND_MARK, layout::BRAND_MARK),
                    Sense::hover(),
                );
                brand::paint_mark(ui.painter(), r, p.accent, p.text_on_accent);
                ui.add_space(space::S0);
                ui.label(Type::Heading.rich(strings::APP_NAME).color(p.text_primary));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if collapsed {
                    // Centre the toggle in the rail.
                    ui.add_space(((ui.available_width() - control::MIN_TARGET) / 2.0).max(0.0));
                }
                let label = if collapsed {
                    strings::nav::EXPAND
                } else {
                    strings::nav::COLLAPSE
                };
                if ui
                    .add(
                        IconButton::new(icons::SIDEBAR, label)
                            .icon_size(icon_size::ROW)
                            .shortcut(strings::nav::SIDEBAR_SHORTCUT),
                    )
                    .clicked()
                {
                    state.sidebar_pref = Some(!collapsed);
                }
            });
        },
    );
}

/// The host status: a dot and a word at the foot of the sidebar.
fn host_status(ui: &mut Ui, env: &Env<'_>, collapsed: bool) {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let (dot, label) = match env.bridge.host_status() {
        HostStatus::Ready => (p.success, strings::host::READY),
        HostStatus::NeedsAttention => (p.warning, strings::host::NEEDS_ATTENTION),
        HostStatus::Busy => (p.info, strings::host::BUSY),
        HostStatus::Unknown => (p.text_muted, strings::host::CHECKING),
    };
    let (rect, resp) = ui.allocate_exact_size(
        vec2(ui.available_width(), layout::SIDEBAR_ITEM),
        Sense::hover(),
    );
    let spoken = strings::host::status(label);
    a11y::labelled(ui.ctx(), resp.id, Role::Status, &spoken);
    let cy = rect.center().y;
    let lead_c = if collapsed {
        rect.center().x
    } else {
        rect.left() + space::S2 + 2.0 + icon_size::ROW / 2.0
    };
    ui.painter().circle_filled(pos2(lead_c, cy), 3.5, dot);
    if collapsed {
        tooltip::attach(resp, &spoken);
    } else {
        let x = lead_c + icon_size::ROW / 2.0 + space::S2 + 2.0;
        let g = Type::Caption.galley(ui, label, p.text_muted, rect.right() - x, 1);
        ui.painter()
            .galley(pos2(x, cy - g.size().y / 2.0), g, p.text_muted);
    }
}

/// Paint the sidebar. `collapsed` draws the icon rail.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, collapsed: bool) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let full = ui.max_rect();
    // Right edge.
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
    let pad = if collapsed {
        space::S2
    } else {
        layout::SIDEBAR_PAD
    };

    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: pad as i8,
            right: pad as i8,
            top: 0,
            bottom: space::S2 as i8,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = space::S0;
            brand_row(ui, state, collapsed);

            let bottom_h = layout::SIDEBAR_ITEM * 3.0 + space::S0 * 3.0 + space::S3;
            egui::ScrollArea::vertical()
                .id_salt("sidebar.scroll")
                .max_height((ui.available_height() - bottom_h).max(layout::SIDEBAR_ITEM))
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = space::S0;
                    let mut plate = Plate::new(ui, base.with("plate.list"));
                    for c in FIXED {
                        let n = count(&lib, c);
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
                            &mut plate,
                        );
                        if r.clicked() {
                            state.library.set_collection(c);
                            state.navigate(AfterDiscard::Route(Route::Library));
                        }
                    }

                    if !lib.tags.is_empty() {
                        section_label(ui, strings::nav::TAGS, collapsed);
                        for t in lib.tags.iter() {
                            let c = Collection::Tag(t.id);
                            let n = count(&lib, c);
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
                                &mut plate,
                            );
                            if r.clicked() {
                                state.library.set_collection(c);
                                state.navigate(AfterDiscard::Route(Route::Library));
                            }
                        }
                    }
                    plate.paint(ui, &theme);
                });

            // Bottom group, pinned: Statistics and Settings, then the host status.
            let spare = ui.available_height() - bottom_h;
            if spare > 0.0 {
                ui.add_space(spare);
            }
            let mut plate = Plate::new(ui, base.with("plate.pinned"));
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
                &mut plate,
            )
            .clicked()
            {
                state.navigate(AfterDiscard::Route(Route::Statistics));
            }
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
                &mut plate,
            )
            .clicked()
            {
                state.navigate(AfterDiscard::Route(Route::Settings(SettingsTab::General)));
            }
            plate.paint(ui, &theme);
            let (r, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), space::S3), Sense::hover());
            ui.painter().hline(
                r.shrink2(vec2(space::S2, 0.0)).x_range(),
                r.center().y,
                Stroke::new(stroke::BORDER, p.border_subtle),
            );
            host_status(ui, env, collapsed);
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
