//! Main library (SCREEN_SPECS §2).
//!
//! The top band holds the tools: search on the left, on the page's axis; sort, filter and
//! the view switch together on the right, then *Add game*. During multi-select the
//! selection bar takes its place. Below it one scroll area holds the page: the heading (the
//! collection's name, the live result count, the active filters as chips) scrolls away with
//! the grid or list beneath it, so the shelf gets the whole window once you move down it.
//! Empty and loading states, the context menu and type-to-search live here too.

pub mod grid;
pub mod list;
pub mod menu;
pub mod sidebar;

use easyplay_core::ids::GameId;
use easyplay_core::library::{Grouping, PresetFilter};
use easyplay_core::model::{Availability, BasePreset, EntryKind, NetworkAccess};
use egui::{Align, Frame, Id, Layout, Margin, Panel, Popup, PopupCloseBehavior, Stroke, Ui};

use super::Env;
use super::actions::{self, BulkAction, GameAction};
use crate::a11y::{Announcer, Politeness};
use crate::app::UiState;
use crate::bridge::LibraryView;
use crate::input::dnd::DropTarget;
use crate::input::picker::PickPurpose;
use crate::strings;
use crate::theme::palette::fade;
use crate::theme::tokens::{control, icon as icon_size, layout, radius, space, stroke, travel};
use crate::theme::{self, Dur, Type, icons, motion};
use crate::vm::library::{
    Collection, CollectionExt, FilterChip, GROUPINGS, SORTS, ViewMode, grouping_label, sort_label,
};
use crate::widgets::button::{Button, Variant};
use crate::widgets::chip::{ChipKind, chip};
use crate::widgets::context_menu::{self, check_item};
use crate::widgets::empty_state::empty_state;
use crate::widgets::icon_button::IconButton;
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::text_field::TextField;

/// Page margin for the current width.
pub fn margin(ui: &Ui) -> f32 {
    let w = ui.ctx().content_rect().width();
    if w < layout::COMPACT_BELOW {
        layout::PAGE_MARGIN_COMPACT
    } else if w >= layout::WIDE_FROM {
        layout::PAGE_MARGIN_WIDE
    } else {
        layout::PAGE_MARGIN
    }
}

/// Below this toolbar width the sort and filter buttons show their icons only.
const TOOLBAR_LABELS_FROM: f32 = 720.0;
/// Below this toolbar width *Add game* shows its icon only.
const TOOLBAR_ADD_LABEL_FROM: f32 = 520.0;

fn chip_label(lib: &LibraryView, c: &FilterChip) -> String {
    match c {
        FilterChip::Tag(t) => lib.tag(*t).map_or_else(String::new, |t| t.name.to_string()),
        FilterChip::Preset(p) => strings::library::preset_filter(*p).to_owned(),
        FilterChip::Network(n) => strings::library::network_filter(*n).to_owned(),
        FilterChip::Availability(a) => strings::library::availability_filter(*a).to_owned(),
        FilterChip::Layer(l) => l.to_string(),
        FilterChip::Kind(k) => strings::library::kind_filter(*k).to_owned(),
    }
}

/// Whether the menu of the toolbar button `key` was open last frame (the button keeps its
/// hovered look while its menu is open).
fn was_open(ui: &Ui, key: &'static str) -> bool {
    ui.ctx()
        .data(|d| d.get_temp::<bool>(Id::unique(key)))
        .unwrap_or(false)
}

fn remember_open(ui: &Ui, key: &'static str, resp: &egui::Response) {
    let open = Popup::is_id_open(ui.ctx(), Popup::default_response_id(resp));
    ui.ctx().data_mut(|d| d.insert_temp(Id::unique(key), open));
}

fn filter_button<'a>(label: &'a str, n: usize, name: String, icon_only: bool) -> Button<'a> {
    Button::new(label)
        .variant(if n > 0 {
            Variant::Tonal
        } else {
            Variant::Ghost
        })
        .icon(icons::FILTER)
        .badge(n)
        .icon_only(icon_only)
        .accessible_name(name)
}

fn filter_popover(ui: &mut Ui, state: &mut UiState, lib: &LibraryView, icon_only: bool) {
    let n = state.library.chips().len();
    let name = strings::library::filter_button(n);
    let resp = ui.add(
        filter_button(strings::library::FILTER, n, name, icon_only)
            .open(was_open(ui, "library.filter.open")),
    );
    remember_open(ui, "library.filter.open", &resp);
    context_menu::popup(
        Popup::menu(&resp)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .width(248.0),
        |ui| {
            egui::ScrollArea::vertical()
                .max_height(440.0)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let mut first = true;
                    let mut group = |ui: &mut Ui, title: &str, items: Vec<FilterChip>| {
                        if items.is_empty() {
                            return;
                        }
                        if !std::mem::take(&mut first) {
                            context_menu::separator(ui);
                        }
                        context_menu::section(ui, title);
                        for c in items {
                            let on = state.library.has_filter(&c);
                            if check_item(ui, &chip_label(lib, &c), on).clicked() {
                                state.library.toggle_filter(c);
                            }
                        }
                    };
                    group(
                        ui,
                        strings::library::FILTER_TAGS,
                        lib.tags.iter().map(|t| FilterChip::Tag(t.id)).collect(),
                    );
                    group(
                        ui,
                        strings::library::FILTER_PROTECTION,
                        [
                            PresetFilter::Offline,
                            PresetFilter::Online,
                            PresetFilter::Standard,
                            PresetFilter::Custom,
                        ]
                        .map(FilterChip::Preset)
                        .to_vec(),
                    );
                    group(
                        ui,
                        strings::library::FILTER_INTERNET,
                        [NetworkAccess::None, NetworkAccess::Full]
                            .map(FilterChip::Network)
                            .to_vec(),
                    );
                    group(
                        ui,
                        strings::library::FILTER_AVAILABILITY,
                        [
                            Availability::Present,
                            Availability::Missing,
                            Availability::DriveDisconnected,
                        ]
                        .map(FilterChip::Availability)
                        .to_vec(),
                    );
                    let mut layers: Vec<std::sync::Arc<str>> =
                        lib.games.iter().map(|g| g.runtime_label.clone()).collect();
                    layers.sort();
                    layers.dedup();
                    group(
                        ui,
                        strings::library::FILTER_LAYER,
                        layers.into_iter().map(FilterChip::Layer).collect(),
                    );
                    group(
                        ui,
                        strings::library::FILTER_TYPE,
                        [EntryKind::Game, EntryKind::Installer, EntryKind::Tool]
                            .map(FilterChip::Kind)
                            .to_vec(),
                    );
                });
        },
    );
}

/// The visible label and the accessible name of the sort button.
fn sort_labels(state: &UiState) -> (String, String) {
    let key = sort_label(state.library.sort);
    (
        key.to_owned(),
        strings::library::sort_button(key, state.library.descending),
    )
}

/// The sort button; tinted while the shelf is grouped, as the filter button is while
/// filters are on.
fn sort_button<'a>(label: &'a str, name: String, icon_only: bool, grouped: bool) -> Button<'a> {
    Button::new(label)
        .variant(if grouped {
            Variant::Tonal
        } else {
            Variant::Ghost
        })
        .icon(icons::SORT)
        .caret()
        .icon_only(icon_only)
        .accessible_name(name)
}

/// Sort key, direction and grouping in one menu: everything that arranges the shelf.
fn sort_menu(ui: &mut Ui, state: &mut UiState, icon_only: bool) {
    let (label, name) = sort_labels(state);
    let grouped = state.library.group != Grouping::None;
    let resp = ui
        .add(sort_button(&label, name, icon_only, grouped).open(was_open(ui, "library.sort.open")));
    remember_open(ui, "library.sort.open", &resp);
    if !icon_only {
        // Direction and grouping have no label of their own in the toolbar: say them on
        // hover.
        let group =
            (state.library.group != Grouping::None).then(|| grouping_label(state.library.group));
        crate::widgets::tooltip::attach(
            resp.clone(),
            &strings::library::arranged_by(
                sort_label(state.library.sort),
                state.library.descending,
                group,
            ),
        );
    }
    context_menu::popup(
        Popup::menu(&resp)
            .close_behavior(PopupCloseBehavior::CloseOnClick)
            .width(224.0),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            context_menu::section(ui, strings::library::SORT_BY);
            for s in SORTS {
                if context_menu::item(ui, None, sort_label(s), None, s == state.library.sort)
                    .clicked()
                {
                    state.library.set_sort(s, false);
                }
            }
            context_menu::separator(ui);
            for (text, descending) in [
                (strings::library::ORDER_ASCENDING, false),
                (strings::library::ORDER_DESCENDING, true),
            ] {
                let on = state.library.descending == descending;
                if context_menu::item(ui, None, text, None, on).clicked() {
                    state.library.descending = descending;
                }
            }
            context_menu::separator(ui);
            context_menu::section(ui, strings::library::GROUP_BY);
            for g in GROUPINGS {
                if context_menu::item(ui, None, grouping_label(g), None, g == state.library.group)
                    .clicked()
                {
                    state.library.group = g;
                }
            }
        },
    );
}

fn add_button<'a>(icon_only: bool) -> Button<'a> {
    Button::new(strings::library::ADD_GAME)
        .icon(icons::PLUS)
        .icon_only(icon_only)
        .shortcut("Ctrl+N")
}

fn toolbar(ui: &mut Ui, state: &mut UiState, lib: &LibraryView) {
    let w = ui.available_width();
    let icons_only = w < TOOLBAR_LABELS_FROM;
    let add_icon_only = w < TOOLBAR_ADD_LABEL_FROM;
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        // The tools on the right, measured, so that they end exactly on the page's right
        // edge; search takes what they leave, up to a comfortable width.
        let n = state.library.chips().len();
        let (sort_text, sort_name) = sort_labels(state);
        let view_switch = (control::MIN_TARGET + space::S1) * 2.0 + control::SEGMENT_INSET * 2.0;
        let grouped = state.library.group != Grouping::None;
        let tools = sort_button(&sort_text, sort_name, icons_only, grouped).width(ui)
            + space::S2
            + filter_button(strings::library::FILTER, n, String::new(), icons_only).width(ui)
            + space::S2
            + view_switch
            + space::S3
            + add_button(add_icon_only).width(ui);
        let search_w = (w - tools - space::S2 - space::S4).clamp(120.0, 340.0);
        let r = TextField::new(&mut state.library.query, "library.search")
            .search()
            .hint(strings::library::SEARCH_HINT)
            .accessible_name(strings::library::SEARCH)
            .shortcut_hint("/")
            .width(search_w)
            .show(ui);
        if state.focus_search {
            r.response.request_focus();
            state.focus_search = false;
        }
        // Whatever is left is the gap between finding and arranging.
        ui.add_space((ui.available_width() - tools).max(0.0));
        sort_menu(ui, state, icons_only);
        filter_popover(ui, state, lib, icons_only);
        let mut view = match state.library.view {
            ViewMode::Grid => 0,
            ViewMode::List => 1,
        };
        let opts = [
            Segment {
                icon: Some(icons::GRID),
                label: strings::library::VIEW_GRID,
            },
            Segment {
                icon: Some(icons::LIST),
                label: strings::library::VIEW_LIST,
            },
        ];
        if segmented(ui, strings::library::VIEW, &mut view, &opts, true).changed() {
            state.library.view = if view == 0 {
                ViewMode::Grid
            } else {
                ViewMode::List
            };
        }
        ui.add_space(space::S3 - space::S2);
        if ui.add(add_button(add_icon_only)).clicked() {
            state.picks.push(PickPurpose::AddGame);
        }
    });
}

fn selection_bar(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, lib: &LibraryView) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let mut action = None;
    // Labels give way to icons (with tooltips) when the bar is narrow.
    let icons_only = ui.available_width() < 860.0;
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = space::S1;
        if ui
            .add(
                IconButton::new(icons::CLOSE, strings::library::CLEAR_SELECTION)
                    .size(control::MD)
                    .shortcut("Esc"),
            )
            .clicked()
        {
            state.library.selection.clear();
        }
        ui.add_space(space::S1);
        ui.label(
            Type::BodyStrong
                .numeric(strings::library::selected_count(
                    state.library.selection.len(),
                ))
                .color(p.text_primary),
        );
        ui.add_space(space::S3);
        let tag_menu = |ui: &mut Ui, label: &str, add: bool, action: &mut Option<BulkAction>| {
            let r = ui.add(Button::ghost(label).icon(icons::TAG).icon_only(icons_only));
            context_menu::popup(Popup::menu(&r).width(200.0), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for t in &lib.tags {
                    if context_menu::item(ui, Some(icons::TAG), &t.name, None, false).clicked() {
                        *action = Some(if add {
                            BulkAction::AddTag(t.id)
                        } else {
                            BulkAction::RemoveTag(t.id)
                        });
                    }
                }
            });
        };
        tag_menu(ui, strings::library::ADD_TAG, true, &mut action);
        tag_menu(ui, strings::library::REMOVE_TAG, false, &mut action);
        if ui
            .add(
                Button::ghost(strings::menu::FAVORITE)
                    .icon(icons::STAR)
                    .icon_only(icons_only),
            )
            .clicked()
        {
            action = Some(BulkAction::Favorite);
        }
        if ui
            .add(
                Button::ghost(strings::menu::HIDE)
                    .icon(icons::EYE_OFF)
                    .icon_only(icons_only),
            )
            .clicked()
        {
            action = Some(BulkAction::Hide);
        }
        let r = ui.add(
            Button::ghost(strings::library::CHANGE_PROTECTION)
                .icon(icons::SHIELD_OFFLINE)
                .icon_only(icons_only),
        );
        context_menu::popup(Popup::menu(&r).width(200.0), |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for b in BasePreset::ALL {
                if context_menu::item(
                    ui,
                    Some(crate::widgets::shield::glyph(b.into(), false)),
                    strings::shield::preset_name(b.into()),
                    None,
                    false,
                )
                .clicked()
                {
                    action = Some(BulkAction::Preset(b));
                }
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(
                    Button::new(strings::library::REMOVE_SELECTED)
                        .variant(Variant::Danger)
                        .icon(icons::TRASH),
                )
                .clicked()
            {
                action = Some(BulkAction::Remove);
            }
        });
    });
    if let Some(a) = action {
        actions::bulk(state, env, a);
    }
}

/// The active filters as removable chips, after the result count.
fn chips(ui: &mut Ui, state: &mut UiState, lib: &LibraryView) {
    let chips = state.library.chips();
    if chips.is_empty() {
        return;
    }
    ui.add_space(space::S1);
    for c in chips {
        let label = chip_label(lib, &c);
        let kind = match &c {
            FilterChip::Tag(t) => ChipKind::Tag {
                color: lib.tag(*t).map_or(0, |t| t.color.index()),
                removable: true,
            },
            _ => ChipKind::Removable,
        };
        if chip(ui, kind, &label, None, true).removed {
            state.library.toggle_filter(c);
        }
    }
    if ui
        .add(
            Button::new(strings::library::CLEAR_ALL)
                .variant(Variant::Link)
                .size(crate::widgets::button::Size::Sm),
        )
        .clicked()
    {
        state.library.filters = Default::default();
    }
}

/// The page heading: the collection's name, how many games are shown, which filters are on.
fn heading(ui: &mut Ui, state: &mut UiState, lib: &LibraryView, label: &str, count: Option<&str>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(space::S3);
    let h = ui.add(egui::Label::new(Type::Display.rich(label).color(p.text_primary)).truncate());
    crate::a11y::labelled(ui.ctx(), h.id, egui::Role::Heading, label);
    // One line under the title, as tall as a chip whether or not there are any, so that
    // turning a filter on does not move the shelf.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S1 + 2.0);
        ui.set_min_height(control::CHIP_SM);
        match count {
            Some(count) => {
                ui.label(Type::Body.numeric(count).color(p.text_muted));
            }
            None if lib.revision == 0 => {
                let (r, _) = ui.allocate_exact_size(
                    egui::vec2(space::S16, Type::Caption.size()),
                    egui::Sense::hover(),
                );
                crate::widgets::skeleton::line(ui, r.min, r.width(), r.height());
            }
            None => {}
        }
        chips(ui, state, lib);
    });
    ui.add_space(space::S4);
}

/// Type-to-search: printable text while nothing has focus goes to the search field.
fn type_to_search(ui: &Ui, state: &mut UiState) {
    if ui.ctx().memory(|m| m.focused()).is_some() || Popup::is_any_open(ui.ctx()) {
        return;
    }
    let typed: String = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Text(t) if !t.trim().is_empty() && t != "/" && t != "?" => {
                    Some(t.clone())
                }
                _ => None,
            })
            .collect()
    });
    if !typed.is_empty() {
        state.library.query.push_str(&typed);
        state.focus_search = true;
    }
}

/// The states with nothing to list: an empty library, no results, an empty collection.
/// Returns whether one was shown.
fn empty(ui: &mut Ui, state: &mut UiState, lib: &LibraryView, shown: usize, room_h: f32) -> bool {
    if !lib.games.is_empty() && shown > 0 {
        return false;
    }
    // A little above the middle of the room that is left, where the eye rests.
    ui.add_space((room_h * 0.16).clamp(space::S6, space::S16 * 2.0));
    if lib.games.is_empty() {
        if empty_state(
            ui,
            icons::FILE_EXE,
            strings::library::EMPTY_TITLE,
            strings::library::EMPTY_BODY,
            Some(Button::primary(strings::library::ADD_GAME).icon(icons::PLUS)),
        )
        .is_some_and(|r| r.clicked())
        {
            state.picks.push(PickPurpose::AddGame);
        }
        ui.add_space(space::S8);
        drop_hint(ui);
        return true;
    }
    let q = state.library.query.trim().to_owned();
    if !q.is_empty() || state.library.filters.is_active() {
        let msg = strings::library::no_results(&q);
        let (label, scoped) = if !q.is_empty() {
            (strings::library::CLEAR_SEARCH, true)
        } else {
            (strings::library::CLEAR_FILTERS, false)
        };
        if empty_state(ui, icons::SEARCH, &msg, "", Some(Button::new(label)))
            .is_some_and(|r| r.clicked())
        {
            if scoped {
                state.library.query.clear();
            } else {
                state.library.filters = Default::default();
            }
        }
        if !q.is_empty() && state.library.collection != Collection::All && !state.library.search_all
        {
            ui.add_space(space::S2);
            ui.vertical_centered(|ui| {
                if ui
                    .add(Button::ghost(strings::library::SEARCH_ALL))
                    .clicked()
                {
                    state.library.search_all = true;
                }
            });
        }
    } else {
        let sentence = strings::collection::empty_sentence(state.library.collection.label());
        empty_state(
            ui,
            icons::LIBRARY,
            strings::library::EMPTY_COLLECTION,
            sentence,
            None,
        );
    }
    true
}

/// Show the library route.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let m = margin(ui);
    let lib = env.bridge.library().clone();
    type_to_search(ui, state);
    let selecting = !state.library.selection.is_empty();
    Panel::top("library_toolbar")
        .exact_size(layout::TOOLBAR_H)
        .show_separator_line(false)
        .frame(Frame::NONE.inner_margin(Margin::symmetric(m as i8, 0)))
        .show(ui, |ui| {
            if selecting {
                selection_bar(ui, state, env, &lib);
            } else if lib.revision == 0 || !lib.games.is_empty() {
                toolbar(ui, state, &lib);
            }
            // An empty library has nothing to find or arrange: the page itself offers the
            // one thing to do.
        });

    if state.banners.top().is_some() {
        // Beneath the tools, so the top band stays level with the brand.
        Panel::top("library_banner")
            .show_separator_line(false)
            .frame(Frame::NONE)
            .show(ui, |ui| {
                super::shell::banner_slot(ui, state, 0.0, space::S2)
            });
    }

    // The cursor follows its game; if the grid had focus, so does keyboard focus.
    if let Some(moved) = state.library.follow(&lib)
        && state.library.focused
    {
        state.pending_focus = Some(moved);
    }
    let result = state.library.result(&lib);
    let visible: std::sync::Arc<[u32]> = result.items.clone().into();
    let total = state.library.base_total(&lib);
    let collection_label = match state.library.collection {
        Collection::Tag(t) => lib
            .tag(t)
            .map_or_else(|| strings::nav::TAGS.to_owned(), |x| x.name.to_string()),
        c => c.label().to_owned(),
    };
    let loaded = lib.revision > 0;
    let count = (loaded && !lib.games.is_empty())
        .then(|| strings::library::result_count(visible.len(), total));
    if let Some(count) = &count {
        let key = Id::unique("library.count");
        let last: Option<String> = ui.ctx().data(|d| d.get_temp(key));
        if last.as_deref() != Some(count.as_str()) {
            if last.is_some() {
                Announcer::announce_debounced(
                    ui.ctx(),
                    "search",
                    count.clone(),
                    Politeness::Polite,
                    crate::a11y::SEARCH_DEBOUNCE_S,
                );
            }
            ui.ctx().data_mut(|d| d.insert_temp(key, count.clone()));
        }
    }

    // A change of collection or view is a small arrival: the page fades and settles.
    let view = state.library.view;
    let arrive = motion::appear(
        ui.ctx(),
        Id::unique(("library.page", state.library.collection, view)),
        theme.motion.fade(Dur::Base),
    );
    let rise = if theme.motion.reduced {
        0.0
    } else {
        (1.0 - arrive) * travel::RISE
    };
    let central = egui::CentralPanel::default()
        .frame(Frame::NONE)
        .show(ui, |ui| {
            let viewport = ui.max_rect();
            let out = egui::ScrollArea::vertical()
                .id_salt(("library.page", state.library.collection, view))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.multiply_opacity(arrive);
                    Frame::NONE
                        .inner_margin(Margin {
                            left: m as i8,
                            right: m as i8,
                            top: rise as i8,
                            bottom: space::S8 as i8,
                        })
                        .show(ui, |ui| {
                            let top = ui.cursor().top();
                            heading(ui, state, &lib, &collection_label, count.as_deref());
                            let room_h = viewport.height() - (ui.cursor().top() - top);
                            if !loaded {
                                grid::skeleton(ui, theme.density);
                                return;
                            }
                            if empty(ui, state, &lib, visible.len(), room_h) {
                                return;
                            }
                            let action = match view {
                                ViewMode::Grid => grid::show(
                                    ui,
                                    state,
                                    env,
                                    &lib,
                                    &visible,
                                    &result.groups,
                                    total,
                                    room_h,
                                ),
                                ViewMode::List => {
                                    grid::skip_intro(ui.ctx());
                                    list::show(ui, state, env, &lib, &visible)
                                }
                            };
                            if let Some((id, a)) = action {
                                actions::apply(state, env, id, a);
                            }
                        });
                });
            // Once the page has moved under the top band, a hairline says so.
            let scrolled = ui.ctx().animate_bool_with_time(
                Id::unique("library.scrolled"),
                out.state.clamped_offset().y > 1.0,
                theme.motion.fade(Dur::Fast),
            );
            if scrolled > 0.0 {
                ui.painter().hline(
                    viewport.x_range(),
                    viewport.top() + 0.5,
                    Stroke::new(stroke::BORDER, fade(p.hairline, scrolled)),
                );
            }
        });
    state.dnd.target(central.response.rect, DropTarget::Library);

    // The context menu, independent of which cards are laid out this frame.
    if let Some((id, at)) = state.library.menu {
        let mut open = true;
        let facts = menu::MenuFacts {
            has_shortcut: env.bridge.host().shortcuts.iter().any(|s| s.game == id),
            developer: env.bridge.prefs().advanced.developer_mode,
        };
        let chosen = menu::show(&env.ctx, &lib, id, at, &mut open, facts);
        if !open {
            state.library.menu = None;
            state.pending_focus = Some(id);
        }
        if let Some(a) = chosen {
            actions::apply(state, env, id, a);
        }
    }
}

/// Whether the page moved further than its own height since the last frame (a scrollbar
/// drag, a fling, a jump to the cursor): what is on screen now will not stay, so the grid
/// and the list ask for no artwork this frame and ask again once the page rests. `top` is
/// the viewport's top in content coordinates.
pub(crate) fn page_jumped(ui: &Ui, top: f32, view_h: f32) -> bool {
    let key = Id::unique("library.page.top");
    let last = ui.ctx().data(|d| d.get_temp::<f32>(key));
    ui.ctx().data_mut(|d| d.insert_temp(key, top));
    let jumped = last.is_some_and(|l| (top - l).abs() > view_h.max(1.0));
    if jumped {
        // The frame that asks for what the page came to rest on.
        ui.ctx().request_repaint();
    }
    jumped
}

/// The drop target of an empty library: a dashed outline that says what it takes.
fn drop_hint(ui: &mut Ui) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.vertical_centered(|ui| {
        let w = 340.0_f32.min(ui.available_width());
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, space::S16), egui::Sense::hover());
        let r = f32::from(radius::POP);
        // A rounded rectangle as a closed path, dashed.
        let mut pts = Vec::new();
        let corners = [
            (rect.right() - r, rect.top() + r, -90.0_f32),
            (rect.right() - r, rect.bottom() - r, 0.0),
            (rect.left() + r, rect.bottom() - r, 90.0),
            (rect.left() + r, rect.top() + r, 180.0),
        ];
        for (cx, cy, start) in corners {
            for i in 0..=6 {
                let a = (start + 15.0 * i as f32).to_radians();
                pts.push(egui::pos2(cx + r * a.cos(), cy + r * a.sin()));
            }
        }
        if let Some(first) = pts.first().copied() {
            pts.push(first);
        }
        ui.painter().extend(egui::Shape::dashed_line(
            &pts,
            Stroke::new(stroke::BORDER, p.border_strong),
            5.0,
            5.0,
        ));
        let text = Type::Body.galley(ui, strings::library::DROP_HERE, p.text_muted, w, 1);
        let icon_w = icon_size::INLINE + space::S2;
        let x = rect.center().x - (text.size().x + icon_w) / 2.0;
        ui.painter().text(
            egui::pos2(x + icon_size::INLINE / 2.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            icons::DOWNLOAD.text(false),
            icons::font(icon_size::INLINE),
            p.text_muted,
        );
        ui.painter().galley(
            egui::pos2(x + icon_w, rect.center().y - text.size().y / 2.0),
            text,
            p.text_muted,
        );
    });
}

/// Open the context menu for `id` at `at` (right-click, `Shift+F10`, Menu key).
pub fn open_menu(state: &mut UiState, id: GameId, at: egui::Pos2) {
    state.library.selected = Some(id);
    state.library.menu = Some((id, at));
}

/// The per-game action of a single-key shortcut, for the focused game.
pub fn shortcut_action(a: crate::input::shortcuts::Action) -> Option<GameAction> {
    use crate::input::shortcuts::Action as A;
    Some(match a {
        A::Play => GameAction::Play,
        A::Stop => GameAction::Stop,
        A::Favorite => GameAction::Favorite,
        A::Tags => GameAction::EditTags,
        A::Hide => GameAction::Hide,
        A::Remove => GameAction::Remove,
        A::OpenFolder => GameAction::OpenFolder,
        _ => return None,
    })
}
