//! Main library (SCREEN_SPECS §2): toolbar (or the selection bar during multi-select), filter
//! chips, heading with the live result count, grid or list, empty and loading states, the
//! context menu, type-to-search.

pub mod grid;
pub mod list;
pub mod menu;
pub mod sidebar;

use easyplay_core::ids::GameId;
use easyplay_core::library::PresetFilter;
use easyplay_core::model::{Availability, BasePreset, EntryKind, NetworkAccess};
use egui::{Align, Frame, Layout, Margin, Panel, Popup, PopupCloseBehavior, Ui};

use super::Env;
use super::actions::{self, BulkAction, GameAction};
use crate::a11y::{Announcer, Politeness};
use crate::app::UiState;
use crate::bridge::LibraryView;
use crate::input::dnd::DropTarget;
use crate::input::picker::PickPurpose;
use crate::strings;
use crate::theme::tokens::{layout, space};
use crate::theme::{self, Type, icons};
use crate::vm::library::{
    CollectionExt, FilterChip, GROUPINGS, SORTS, ViewMode, grouping_label, sort_label,
};
use crate::widgets::button::{Button, Size};
use crate::widgets::chip::{ChipKind, chip};
use crate::widgets::context_menu::{self, check_item};
use crate::widgets::empty_state::empty_state;
use crate::widgets::icon_button::IconButton;
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::select::select;
use crate::widgets::text_field::TextField;

/// Page margin for the current width.
pub fn margin(ui: &Ui) -> f32 {
    if ui.ctx().content_rect().width() < layout::COMPACT_BELOW {
        layout::PAGE_MARGIN_COMPACT
    } else {
        layout::PAGE_MARGIN
    }
}

fn chip_label(lib: &LibraryView, c: &FilterChip) -> String {
    match c {
        FilterChip::Tag(t) => lib
            .tag(*t)
            .map_or_else(String::new, |t| strings::library::tag_chip(&t.name)),
        FilterChip::Preset(p) => strings::library::preset_filter(*p).to_owned(),
        FilterChip::Network(n) => strings::library::network_filter(*n).to_owned(),
        FilterChip::Availability(a) => strings::library::availability_filter(*a).to_owned(),
        FilterChip::Layer(l) => l.to_string(),
        FilterChip::Kind(k) => strings::library::kind_filter(*k).to_owned(),
    }
}

fn filter_popover(ui: &mut Ui, state: &mut UiState, lib: &LibraryView) {
    let n = state.library.chips().len();
    let label = strings::library::filter_button(n);
    let resp = ui.add(Button::ghost(&label).icon(icons::FILTER).size(Size::Sm));
    Popup::menu(&resp)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .width(240.0)
        .show(|ui| {
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    let p = theme::theme(ui.ctx()).palette.clone();
                    let mut group = |ui: &mut Ui, title: &str, items: Vec<FilterChip>| {
                        if items.is_empty() {
                            return;
                        }
                        ui.label(Type::Micro.rich(title).color(p.text_muted));
                        for c in items {
                            let on = state.library.has_filter(&c);
                            if check_item(ui, &chip_label(lib, &c), on).clicked() {
                                state.library.toggle_filter(c);
                            }
                        }
                        ui.add_space(space::S1);
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
        });
}

fn sort_menu(ui: &mut Ui, state: &mut UiState) {
    let mut sort = SORTS
        .iter()
        .position(|s| *s == state.library.sort)
        .unwrap_or(0);
    let labels: Vec<&str> = SORTS.iter().map(|s| sort_label(*s)).collect();
    if select(
        ui,
        strings::library::SORT,
        Some(strings::library::SORT),
        Some(icons::SORT),
        &mut sort,
        &labels,
        true,
    )
    .changed()
    {
        state.library.set_sort(SORTS[sort], false);
    }
    let dir = if state.library.descending {
        strings::library::DESCENDING
    } else {
        strings::library::ASCENDING
    };
    let arrow = if state.library.descending {
        "↓"
    } else {
        "↑"
    };
    if ui
        .add(Button::ghost(arrow).size(Size::Sm).accessible_name(dir))
        .on_hover_text(dir)
        .clicked()
    {
        state.library.descending = !state.library.descending;
    }
}

fn group_menu(ui: &mut Ui, state: &mut UiState) {
    let mut g = GROUPINGS
        .iter()
        .position(|x| *x == state.library.group)
        .unwrap_or(0);
    let labels: Vec<&str> = GROUPINGS.iter().map(|x| grouping_label(*x)).collect();
    if select(
        ui,
        strings::library::GROUP,
        Some(strings::library::GROUP),
        None,
        &mut g,
        &labels,
        true,
    )
    .changed()
    {
        state.library.group = GROUPINGS[g];
    }
}

fn toolbar(ui: &mut Ui, state: &mut UiState, lib: &LibraryView) {
    let narrow = ui.available_width() < 640.0;
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        let search_w = (ui.available_width() * 0.36).clamp(150.0, 320.0);
        let r = TextField::new(&mut state.library.query, "library.search")
            .search()
            .hint(strings::library::SEARCH_HINT)
            .accessible_name(strings::library::SEARCH)
            .width(search_w)
            .show(ui);
        if state.focus_search {
            r.response.request_focus();
            state.focus_search = false;
        }
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
        if !narrow {
            sort_menu(ui, state);
        }
        filter_popover(ui, state, lib);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let add = if narrow {
                ui.add(IconButton::new(icons::PLUS, strings::library::ADD_GAME).shortcut("Ctrl+N"))
            } else {
                ui.add(Button::primary(strings::library::ADD_GAME).icon(icons::PLUS))
            };
            if add.clicked() {
                state.picks.push(PickPurpose::AddGame);
            }
            if !narrow && ui.available_width() > 200.0 {
                group_menu(ui, state);
            }
        });
    });
}

fn selection_bar(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, lib: &LibraryView) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let mut action = None;
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        if ui
            .add(IconButton::new(icons::CLOSE, strings::library::CLEAR_SELECTION).shortcut("Esc"))
            .clicked()
        {
            state.library.selection.clear();
        }
        ui.label(
            Type::BodyStrong
                .numeric(strings::library::selected_count(
                    state.library.selection.len(),
                ))
                .color(p.text_primary),
        );
        ui.add_space(space::S2);
        let tag_menu = |ui: &mut Ui, label: &str, add: bool, action: &mut Option<BulkAction>| {
            let r = ui.add(Button::ghost(label).icon(icons::TAG).size(Size::Sm));
            Popup::menu(&r).show(|ui| {
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
                    .size(Size::Sm),
            )
            .clicked()
        {
            action = Some(BulkAction::Favorite);
        }
        if ui
            .add(
                Button::ghost(strings::menu::HIDE)
                    .icon(icons::EYE_OFF)
                    .size(Size::Sm),
            )
            .clicked()
        {
            action = Some(BulkAction::Hide);
        }
        let r = ui.add(
            Button::ghost(strings::library::CHANGE_PROTECTION)
                .icon(icons::SHIELD_OFFLINE)
                .size(Size::Sm),
        );
        Popup::menu(&r).show(|ui| {
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
                        .variant(crate::widgets::button::Variant::Danger)
                        .icon(icons::TRASH)
                        .size(Size::Sm),
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

fn chips_row(ui: &mut Ui, state: &mut UiState, lib: &LibraryView) {
    let chips = state.library.chips();
    if chips.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
        for c in chips {
            let label = chip_label(lib, &c);
            let color = match &c {
                FilterChip::Tag(t) => lib.tag(*t).map_or(0, |t| t.color.index()),
                _ => 0,
            };
            let r = chip(
                ui,
                ChipKind::Tag {
                    color,
                    removable: true,
                },
                &label,
                None,
                true,
            );
            if r.removed {
                state.library.toggle_filter(c);
            }
        }
        if ui
            .add(Button::ghost(strings::library::CLEAR_ALL).size(Size::Sm))
            .clicked()
        {
            state.library.filters = Default::default();
        }
    });
    ui.add_space(space::S2);
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
        .frame(Frame::NONE.inner_margin(Margin {
            left: m as i8,
            right: m as i8,
            top: space::S2 as i8,
            bottom: 0,
        }))
        .show(ui, |ui| {
            if selecting {
                selection_bar(ui, state, env, &lib);
            } else {
                toolbar(ui, state, &lib);
            }
        });

    let result = state.library.result(&lib);
    let visible: std::sync::Arc<[u32]> = result.items.clone().into();
    let total = state.library.base_total(&lib);
    let collection_label = match state.library.collection {
        easyplay_core::library::Collection::Tag(t) => lib
            .tag(t)
            .map_or_else(|| strings::nav::TAGS.to_owned(), |x| x.name.to_string()),
        c => c.label().to_owned(),
    };

    let central = egui::CentralPanel::default()
        .frame(Frame::NONE.inner_margin(Margin {
            left: m as i8,
            right: m as i8,
            top: space::S2 as i8,
            bottom: 0,
        }))
        .show(ui, |ui| {
            chips_row(ui, state, &lib);
            ui.horizontal(|ui| {
                ui.label(Type::Title.rich(&collection_label).color(p.text_primary));
                if lib.revision > 0 {
                    let count = strings::library::result_count(visible.len(), total);
                    ui.label(Type::Body.numeric(&count).color(p.text_muted));
                    let key = egui::Id::unique("library.count");
                    let last: Option<String> = ui.ctx().data(|d| d.get_temp(key));
                    if last.as_deref() != Some(count.as_str()) {
                        if last.is_some() {
                            Announcer::announce(ui.ctx(), count.clone(), Politeness::Polite);
                        }
                        ui.ctx().data_mut(|d| d.insert_temp(key, count));
                    }
                }
            });
            ui.add_space(space::S3);

            if lib.revision == 0 {
                grid::skeleton(ui, theme.density);
                return;
            }
            if lib.games.is_empty() {
                ui.add_space(space::S12);
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
                ui.add_space(space::S6);
                drop_hint(ui);
                return;
            }
            if visible.is_empty() {
                ui.add_space(space::S16);
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
                    if !q.is_empty()
                        && state.library.collection != easyplay_core::library::Collection::All
                        && !state.library.search_all
                    {
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
                    let sentence =
                        strings::collection::empty_sentence(state.library.collection.label());
                    empty_state(
                        ui,
                        icons::LIBRARY,
                        strings::library::EMPTY_COLLECTION,
                        sentence,
                        None,
                    );
                }
                return;
            }
            let action = match state.library.view {
                ViewMode::Grid => grid::show(ui, state, env, &lib, &visible, &result.groups),
                ViewMode::List => list::show(ui, state, env, &lib, &visible),
            };
            if let Some((id, a)) = action {
                actions::apply(state, env, id, a);
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

fn drop_hint(ui: &mut Ui) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.vertical_centered(|ui| {
        let w = 320.0_f32.min(ui.available_width());
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, space::S16), egui::Sense::hover());
        let stroke = egui::Stroke::new(1.0, p.border_strong);
        let shapes = egui::Shape::dashed_line(
            &[
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
                rect.left_top(),
            ],
            stroke,
            6.0,
            4.0,
        );
        ui.painter().extend(shapes);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            strings::library::DROP_HERE,
            Type::Body.font_id(),
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
