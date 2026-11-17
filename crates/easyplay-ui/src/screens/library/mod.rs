//! The library (the design's §4 LIBRARY): the wall of covers, or the list, under one line of
//! words. The line holds the scopes with their counts, "Filter" and the active filters, the
//! result count while refined and the sort; while games are selected it carries their
//! actions instead. The library's controls in the top bar (search, covers / list, "Add
//! game") are [`top_bar`]. Empty and loading states and the game menu live here too.
//!
//! A library adapts by size: 0 games is one invitation; up to 3 are a small display in the
//! middle of the window with captions and Play always shown; up to 6 keep their covers at
//! 208 and end with an "Add game" tile; from 7 it is the wall.

pub mod grid;
pub mod list;
pub mod menu;

use easyplay_core::ids::GameId;
use easyplay_core::library::PresetFilter;
use easyplay_core::model::{Availability, BasePreset, EntryKind, NetworkAccess};
use egui::{
    Align, Id, Layout, Popup, PopupCloseBehavior, Pos2, Rect, RectAlign, Sense, Ui, UiBuilder,
    pos2, vec2,
};

use super::Env;
use super::actions::{self, BulkAction, GameAction};
use crate::a11y::{Announcer, Politeness};
use crate::app::UiState;
use crate::bridge::LibraryView;
use crate::input::dnd::DropTarget;
use crate::strings;
use crate::theme::tokens::{icon as icon_size, layout, overlay};
use crate::theme::{self, Dur, Type, icons, motion};
use crate::vm::library::{
    Collection, CollectionExt, DESIGN_SORTS, FilterChip, GROUPINGS, SORTS, Shelf, ViewMode,
    grouping_label, library_size, order_labels, sort_label,
};
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::context_menu::{
    self, check_item_with, option_item, section, separator, tag_item,
};
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::text_field::TextField;
use crate::widgets::tooltip;
use crate::widgets::word::{
    Word, word_on, word_on_width, word_rule, word_rule_width, word_strong, word_text,
};

/// The page gutter for the current window: 28, or 16 under 620.
pub fn margin(ui: &Ui) -> f32 {
    if ui.ctx().content_rect().width() < layout::NARROW_BELOW {
        layout::GUTTER_NARROW
    } else {
        layout::GUTTER
    }
}

/// Whether the menu opened from the word with key `key` was open last frame (the word keeps
/// its pointed-at look while its menu is open).
fn was_open(ui: &Ui, key: &'static str) -> bool {
    ui.ctx()
        .data(|d| d.get_temp::<bool>(Id::unique(key)))
        .unwrap_or(false)
}

fn remember_open(ui: &Ui, key: &'static str, resp: &egui::Response) {
    let open = Popup::is_id_open(ui.ctx(), Popup::default_response_id(resp));
    ui.ctx().data_mut(|d| d.insert_temp(Id::unique(key), open));
}

/// The library is loaded and can be refined: more than three games.
fn has_tools(lib: &LibraryView) -> bool {
    lib.revision > 0 && library_size(lib) > layout::BIG_UP_TO
}

// ── Top bar ─────────────────────────────────────────────────────────────────────────────────

/// The library's side of the top bar: search, the covers / list switch, "Add game". Called
/// by the shell inside a right-to-left, centred layout: controls are added from the right
/// edge inwards.
pub fn top_bar(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let theme = theme::theme(ui.ctx());
    let lib = env.bridge.library().clone();
    let ready = lib.revision > 0;
    let size = library_size(&lib);
    let damaged = env.bridge.diagnostics().recovery.is_some();
    let narrow = ui.ctx().content_rect().width() < layout::LABELS_FROM;

    // "Add game": hidden while the wall itself offers it.
    let wall_offers = ready
        && size <= layout::SPARSE_UP_TO
        && state.library.collection == Collection::All
        && !state.library.refined();
    if !damaged && !wall_offers {
        let resp = ui.add(
            Button::new(strings::library::ADD_GAME())
                .variant(Variant::Ghost)
                .icon(icons::PLUS)
                .icon_only(narrow)
                .accessible_name(strings::library::ADD_GAME())
                .shortcut(strings::library::KEY_ADD()),
        );
        let resp = if narrow {
            resp
        } else {
            tooltip::tooltip(
                resp,
                strings::library::ADD_GAME_TIP(),
                Some(strings::library::KEY_ADD()),
            )
        };
        if resp.clicked() {
            super::add_game::open(state);
        }
    }
    // Over a damaged library there is nothing to search or to show another way.
    if damaged || !has_tools(&lib) {
        state.focus_search = false;
        state.library.typing = false;
        return;
    }
    let mut view = match state.library.view {
        ViewMode::Grid => 0,
        ViewMode::List => 1,
    };
    let opts = [
        Segment {
            icon: Some(icons::GRID),
            label: strings::library::VIEW_GRID(),
        },
        Segment {
            icon: Some(icons::LIST),
            label: strings::library::VIEW_LIST(),
        },
    ];
    if segmented(ui, strings::library::VIEW(), &mut view, &opts, true).changed() {
        state.library.view = if view == 0 {
            ViewMode::Grid
        } else {
            ViewMode::List
        };
    }
    // The search field widens while focused.
    let focus_key = Id::unique("library.search.focused");
    let was_focused = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(focus_key))
        .unwrap_or(false);
    let [rest, wide] = if narrow {
        layout::SEARCH_W_NARROW
    } else {
        [layout::SEARCH_W, layout::SEARCH_W_FOCUSED]
    };
    let w = motion::glide(
        ui.ctx(),
        Id::unique("library.search.width"),
        if was_focused { wide } else { rest },
        theme.motion.movement(Dur::Base),
    );
    let r = TextField::new(&mut state.library.query, "library.search")
        .search()
        .hint(strings::library::SEARCH_HINT())
        .accessible_name(strings::library::SEARCH())
        .shortcut_hint(strings::library::KEY_SEARCH)
        .width(w)
        .show(ui);
    if state.focus_search {
        r.response.request_focus();
        state.focus_search = false;
    }
    let focused = r.response.has_focus();
    ui.ctx().data_mut(|d| d.insert_temp(focus_key, focused));
    state.library.typing = focused;
}

// ── The filter line ─────────────────────────────────────────────────────────────────────────

/// What a filter word says.
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

/// The width of a word reading `label` with a caret (the sort word), to keep it at the
/// line's right end.
fn caret_word_w(ui: &Ui, label: &str) -> f32 {
    let g = Type::BodyStrong.line(ui, label, egui::Color32::PLACEHOLDER);
    layout::WORD_PAD * 2.0 + g.size().x + layout::WORD_GAP + icon_size::SMALL
}

/// The width of [`word_text`] reading `text`.
fn text_word_w(ui: &Ui, text: &str) -> f32 {
    let g = Type::BodyStrong.numeric_galley(ui, text, egui::Color32::PLACEHOLDER);
    layout::WORD_PAD * 2.0 + g.size().x
}

/// One section of the filter menu: its heading, then each value with its count.
fn filter_section(
    ui: &mut Ui,
    state: &mut UiState,
    lib: &LibraryView,
    title: &str,
    values: Vec<FilterChip>,
    always: bool,
    first: &mut bool,
) {
    let shown: Vec<(FilterChip, usize)> = values
        .into_iter()
        .map(|c| {
            let n = state.library.filter_count(lib, &c);
            (c, n)
        })
        .filter(|(c, n)| always || *n > 0 || state.library.has_filter(c))
        .collect();
    if shown.is_empty() {
        return;
    }
    if !std::mem::take(first) {
        separator(ui);
    }
    section(ui, title);
    let p = theme::theme(ui.ctx()).palette.clone();
    for (c, n) in shown {
        let on = state.library.has_filter(&c);
        let label = chip_label(lib, &c);
        let count = n.to_string();
        let r = match &c {
            FilterChip::Tag(_) => {
                let r = tag_item(ui, &label, p.tag_dot(&label), on);
                // The count at the right, as the other items have it.
                let g = Type::Caption.numeric_galley(ui, &count, p.t3);
                ui.painter().galley(
                    pos2(
                        r.rect.right() - overlay::MENU_ITEM_PAD[1] - g.size().x,
                        r.rect.center().y - g.size().y / 2.0,
                    ),
                    g,
                    p.t3,
                );
                r
            }
            _ => check_item_with(ui, &label, on, Some(&count)),
        };
        if r.clicked() {
            state.library.toggle_filter(c);
        }
    }
}

/// The filter menu: Protection, Tags, then the app's other filters.
fn filter_menu(ui: &mut Ui, state: &mut UiState, lib: &LibraryView) {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.set_min_width(layout::MENU_W[0] - layout::MENU_PAD * 2.0);
    let mut first = true;
    filter_section(
        ui,
        state,
        lib,
        strings::library::FILTER_PROTECTION(),
        [
            PresetFilter::Offline,
            PresetFilter::Online,
            PresetFilter::Standard,
        ]
        .map(FilterChip::Preset)
        .to_vec(),
        true,
        &mut first,
    );
    filter_section(
        ui,
        state,
        lib,
        strings::library::FILTER_TAGS(),
        lib.tags.iter().map(|t| FilterChip::Tag(t.id)).collect(),
        false,
        &mut first,
    );
    filter_section(
        ui,
        state,
        lib,
        strings::library::FILTER_INTERNET(),
        [NetworkAccess::None, NetworkAccess::Full]
            .map(FilterChip::Network)
            .to_vec(),
        false,
        &mut first,
    );
    filter_section(
        ui,
        state,
        lib,
        strings::library::FILTER_AVAILABILITY(),
        [
            Availability::Present,
            Availability::Missing,
            Availability::DriveDisconnected,
        ]
        .map(FilterChip::Availability)
        .to_vec(),
        false,
        &mut first,
    );
    let mut layers: Vec<std::sync::Arc<str>> =
        lib.games.iter().map(|g| g.runtime_label.clone()).collect();
    layers.sort();
    layers.dedup();
    filter_section(
        ui,
        state,
        lib,
        strings::library::FILTER_LAYER(),
        layers.into_iter().map(FilterChip::Layer).collect(),
        false,
        &mut first,
    );
    filter_section(
        ui,
        state,
        lib,
        strings::library::FILTER_TYPE(),
        [EntryKind::Game, EntryKind::Installer, EntryKind::Tool]
            .map(FilterChip::Kind)
            .to_vec(),
        false,
        &mut first,
    );
}

/// The sort menu: sort by, the two orders of the current key, group by.
fn sort_menu(ui: &mut Ui, state: &mut UiState) {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.set_min_width(layout::MENU_W[0] - layout::MENU_PAD * 2.0);
    section(ui, strings::library::SORT_BY());
    for (i, s) in SORTS.into_iter().enumerate() {
        if i == DESIGN_SORTS {
            // The app's other sort keys, after the design's four.
            separator(ui);
        }
        if option_item(ui, sort_label(s), s == state.library.sort).clicked() {
            state.library.set_sort(s, false);
        }
    }
    separator(ui);
    let (desc, asc) = order_labels(state.library.sort);
    for (label, d) in [(desc, true), (asc, false)] {
        if option_item(ui, label, state.library.descending == d).clicked() {
            state.library.descending = d;
        }
    }
    separator(ui);
    section(ui, strings::library::GROUP_BY());
    for g in GROUPINGS {
        if option_item(ui, grouping_label(g), g == state.library.group).clicked() {
            state.library.group = g;
        }
    }
}

/// A word that opens a menu below it (`end`: right-aligned with it).
fn menu_word<R>(
    ui: &mut Ui,
    word: Word<'_>,
    key: &'static str,
    end: bool,
    content: impl FnOnce(&mut Ui) -> R,
) {
    let resp = ui.add(word.open(was_open(ui, key)));
    remember_open(ui, key, &resp);
    let popup = Popup::menu(&resp)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(if end {
            RectAlign::BOTTOM_END
        } else {
            RectAlign::BOTTOM_START
        });
    context_menu::popup(popup, content);
}

/// The line of words: scopes, filters, the count and the sort.
fn scopes_line(ui: &mut Ui, state: &mut UiState, lib: &LibraryView, shelf: &Shelf, right: f32) {
    let palette = theme::theme(ui.ctx()).palette.clone();
    let scopes: Vec<(Collection, usize)> = state
        .library
        .shown_scopes(lib)
        .into_iter()
        .map(|c| (c, state.library.scope_count(lib, c)))
        .collect();
    let chips: Vec<(FilterChip, String, Option<egui::Color32>)> = state
        .library
        .chips()
        .into_iter()
        .map(|c| {
            let dot = match &c {
                FilterChip::Tag(t) => lib.tag(*t).map(|t| palette.tag_dot(&t.name)),
                _ => None,
            };
            let label = chip_label(lib, &c);
            (c, label, dot)
        })
        .collect();
    let count = state
        .library
        .refined()
        .then(|| strings::library::of_count(shelf.items.len(), shelf.pool));
    let sort = sort_label(state.library.sort);
    let tail = caret_word_w(ui, sort)
        + count
            .as_deref()
            .map_or(0.0, |c| text_word_w(ui, c) + layout::WORD_SPACING);
    // The line clips what does not fit (CSS `overflow: hidden`). When the scopes would push
    // "Filter" and the sort out of a narrow window, they fold into one word that opens a
    // menu of them, so that nothing becomes unreachable.
    let gap = layout::WORD_SPACING;
    let scopes_w: f32 = scopes
        .iter()
        .map(|(c, n)| Word::new(c.label()).count(*n).width(ui) + gap)
        .sum();
    let rest_w = word_rule_width()
        + gap
        + Word::new(strings::library::FILTER()).width(ui)
        + gap
        + chips
            .iter()
            .map(|(_, l, d)| word_on_width(ui, l, d.is_some()) + gap)
            .sum::<f32>()
        + tail;
    let room = right - ui.cursor().left();
    if scopes_w + rest_w > room {
        let current = state.library.collection;
        let n = state.library.scope_count(lib, current);
        menu_word(
            ui,
            Word::new(current.label()).count(n).caret(),
            "library.scopes.open",
            false,
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.set_min_width(layout::MENU_W[0] - layout::MENU_PAD * 2.0);
                for (c, n) in &scopes {
                    let on = *c == current;
                    if context_menu::item(ui, None, c.label(), Some(&n.to_string()), on).clicked()
                        && !on
                    {
                        state.library.set_collection(*c);
                    }
                }
            },
        );
    } else {
        for (c, n) in &scopes {
            let on = state.library.collection == *c;
            if ui.add(Word::new(c.label()).count(*n).pressed(on)).clicked() && !on {
                state.library.set_collection(*c);
            }
        }
    }
    word_rule(ui);
    menu_word(
        ui,
        Word::new(strings::library::FILTER()),
        "library.filter.open",
        false,
        |ui| filter_menu(ui, state, lib),
    );
    for (c, label, dot) in chips {
        if word_on(ui, &label, dot, &strings::library::remove_chip(&label)).clicked() {
            state.library.toggle_filter(c);
        }
    }
    // The spacer, then the count and the sort at the right end.
    let spacer = (right - ui.cursor().left() - tail).max(0.0);
    ui.add_space(spacer);
    if let Some(c) = &count {
        word_text(ui, c);
    }
    menu_word(
        ui,
        Word::new(sort)
            .caret()
            .tip(strings::library::SORT_TIP(), None),
        "library.sort.open",
        true,
        |ui| sort_menu(ui, state),
    );
}

/// The line while games are selected: ×, the count, the actions, "Remove…" at the end. A
/// selection of Steam games only has no "Protection" and no "Remove…": neither applies to a
/// game Steam runs (ADR-0031); in a mixed selection they skip the Steam games.
fn selection_line(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    lib: &LibraryView,
    right: f32,
) {
    let mut action = None;
    if ui
        .add(
            Word::new(strings::library::CLEAR_SELECTION())
                .icon(icons::CLOSE)
                .icon_only()
                .tip(
                    strings::library::CLEAR_SELECTION(),
                    Some(strings::library::KEY_ESC()),
                ),
        )
        .clicked()
    {
        state.library.selection.clear();
        state.library.anchor = None;
    }
    word_strong(
        ui,
        &strings::library::selected_count(state.library.selection.len()),
    );
    let picked = state.library.selection.clone();
    let all_steam = picked
        .iter()
        .all(|i| lib.get(*i).is_some_and(|g| g.is_steam()));
    menu_word(
        ui,
        Word::new(strings::library::SEL_TAGS()),
        "library.sel.tags.open",
        false,
        |ui| crate::screens::dialogs::tags::tag_menu(ui, state, env, &picked),
    );
    if ui
        .add(Word::new(strings::library::SEL_FAVOURITE()))
        .clicked()
    {
        action = Some(BulkAction::Favorite);
    }
    if ui.add(Word::new(strings::library::SEL_HIDE())).clicked() {
        action = Some(BulkAction::Hide);
    }
    if !all_steam {
        menu_word(
            ui,
            Word::new(strings::library::SEL_PROTECTION()),
            "library.sel.prot.open",
            false,
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.set_min_width(layout::MENU_W[0] - layout::MENU_PAD * 2.0);
                for b in BasePreset::ALL {
                    if context_menu::item(
                        ui,
                        None,
                        strings::shield::preset_name(b.into()),
                        None,
                        false,
                    )
                    .clicked()
                    {
                        action = Some(BulkAction::Preset(b));
                    }
                }
            },
        );
    }
    if all_steam {
        if let Some(a) = action {
            actions::bulk(state, env, a);
        }
        return;
    }
    let remove = strings::library::REMOVE_SELECTED;
    let g = Type::BodyStrong.line(ui, remove(), egui::Color32::PLACEHOLDER);
    let w = layout::WORD_PAD * 2.0 + g.size().x;
    ui.add_space((right - ui.cursor().left() - w).max(0.0));
    // A quiet word like the others (the design's `.w` outranks its `.b--warn` colour).
    if ui.add(Word::new(remove())).clicked() {
        action = Some(BulkAction::Remove);
    }
    if let Some(a) = action {
        actions::bulk(state, env, a);
    }
}

/// The filter line: 34 high, 8 under it; it starts 8 left of the gutter so the first word's
/// text stands on the page's axis. Crossfades when the selection starts or ends.
fn filter_line(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    lib: &LibraryView,
    shelf: &Shelf,
) {
    let theme = theme::theme(ui.ctx());
    let m = margin(ui);
    let full = ui.available_rect_before_wrap();
    let line = Rect::from_min_size(full.min, vec2(full.width(), layout::FILTER_LINE));
    ui.allocate_rect(
        Rect::from_min_size(
            full.min,
            vec2(full.width(), layout::FILTER_LINE + layout::FILTER_LINE_GAP),
        ),
        Sense::hover(),
    );
    let inner = Rect::from_min_max(
        pos2(line.left() + m - layout::WORD_PAD, line.top()),
        pos2(line.right() - m + layout::WORD_PAD, line.bottom()),
    );
    let selecting = !state.library.selection.is_empty();
    let fade_in = motion::appear(
        ui.ctx(),
        Id::unique(("library.line", selecting)),
        theme.motion.fade(Dur::Fast),
    );
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.set_clip_rect(inner.intersect(ui.clip_rect()));
    row.multiply_opacity(fade_in);
    row.spacing_mut().item_spacing.x = layout::WORD_SPACING;
    crate::a11y::labelled(
        ui.ctx(),
        row.scope_id(),
        egui::Role::Toolbar,
        strings::library::FILTER_LINE(),
    );
    if selecting {
        selection_line(&mut row, state, env, lib, inner.right());
    } else {
        scopes_line(&mut row, state, lib, shelf, inner.right());
    }
}

// ── Empty states ────────────────────────────────────────────────────────────────────────────

/// What a blank offers.
enum BlankAct {
    AddGame,
    Clear,
    ShowAll,
}

/// An empty state (the overlays' `blank`): the headline, a sentence, the action, fine
/// print, in a column at most 440 wide in the middle of the page. It fades in.
fn blank(
    ui: &mut Ui,
    state: &mut UiState,
    title: &str,
    text: &str,
    act: BlankAct,
    fine: Option<&str>,
) {
    let theme = theme::theme(ui.ctx());
    let a = motion::appear(
        ui.ctx(),
        Id::unique(("library.blank", title)),
        theme.motion.fade(Dur::Base),
    );
    ui.multiply_opacity(a);
    let clicked = crate::widgets::empty_state::blank(
        ui,
        title,
        text,
        |acts| match act {
            // The accent pill the size of Play (`.b--primary.play`).
            BlankAct::AddGame => acts
                .add(
                    Button::primary(strings::library::ADD_GAME())
                        .size(Size::Lg)
                        .icon(icons::PLUS),
                )
                .clicked(),
            BlankAct::Clear => acts
                .add(Button::new(strings::library::CLEAR()).variant(Variant::Secondary))
                .clicked(),
            BlankAct::ShowAll => acts
                .add(Button::new(strings::library::SHOW_ALL()).variant(Variant::Secondary))
                .clicked(),
        },
        fine,
    );
    if clicked {
        match act {
            BlankAct::AddGame => super::add_game::open(state),
            BlankAct::Clear => state.library.clear_refinements(),
            BlankAct::ShowAll => state.library.set_collection(Collection::All),
        }
    }
}

/// The blank for a library with nothing to show here (the design's `BLANKS`).
fn nothing_here(ui: &mut Ui, state: &mut UiState, lib: &LibraryView) {
    use strings::library as l;
    if lib.games.is_empty() {
        blank(
            ui,
            state,
            l::FIRST_TITLE(),
            l::FIRST_TEXT(),
            BlankAct::AddGame,
            Some(l::FIRST_FINE()),
        );
        return;
    }
    if state.library.refined() {
        let q = state.library.query.trim().to_owned();
        let title = if q.is_empty() {
            l::NO_MATCH_FILTERS().to_owned()
        } else {
            l::no_match(&q)
        };
        blank(ui, state, &title, l::NO_MATCH_TEXT(), BlankAct::Clear, None);
        return;
    }
    let (title, text) = match state.library.collection {
        Collection::Favorites => (l::FAV_TITLE(), l::FAV_TEXT()),
        Collection::Recent => (l::RECENT_TITLE(), l::RECENT_TEXT()),
        Collection::NeverPlayed => (l::NEVER_TITLE(), l::NEVER_TEXT()),
        _ => (l::OTHER_TITLE(), l::OTHER_TEXT()),
    };
    blank(ui, state, title, text, BlankAct::ShowAll, None);
}

// ── The page ────────────────────────────────────────────────────────────────────────────────

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

/// Announce "13 of 17" politely when it changes.
fn announce_count(ui: &Ui, count: Option<String>) {
    let Some(count) = count else {
        return;
    };
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
        ui.ctx().data_mut(|d| d.insert_temp(key, count));
    }
}

/// Show the library route.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let theme = theme::theme(ui.ctx());
    let m = margin(ui);
    let lib = env.bridge.library().clone();
    let ready = lib.revision > 0;
    let size = library_size(&lib);
    ui.spacing_mut().item_spacing.y = 0.0;
    if has_tools(&lib) {
        type_to_search(ui, state);
    }

    // The cursor follows its game; if the wall had focus, so does keyboard focus.
    if let Some(moved) = state.library.follow(&lib)
        && state.library.focused
    {
        state.pending_focus = Some(moved);
    }
    let shelf = state.library.result(&lib);
    announce_count(
        ui,
        (ready && state.library.refined())
            .then(|| strings::library::of_count(shelf.items.len(), shelf.pool)),
    );

    let line = ready && !(size <= layout::BIG_UP_TO && state.library.collection == Collection::All);
    if line {
        filter_line(ui, state, env, &lib, &shelf);
    }
    // The shell shows the banner, if any, under the top bar of every page.

    // A new scope starts at the top of the page.
    let scope_key = Id::unique("library.scope");
    let last_scope: Option<Collection> = ui.ctx().data(|d| d.get_temp(scope_key));
    ui.ctx()
        .data_mut(|d| d.insert_temp(scope_key, state.library.collection));
    let to_top = last_scope.is_some_and(|s| s != state.library.collection);

    let viewport = ui.available_rect_before_wrap();
    let page_w = viewport.width();
    let view_h = viewport.height() - layout::PAGE_PAD[0] - layout::PAGE_PAD[1];
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt("library.page")
        .auto_shrink([false, false]);
    if to_top {
        scroll = scroll.vertical_scroll_offset(0.0);
    }
    let out = scroll.show(ui, |ui| {
        let origin = ui.max_rect().min;
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: m as i8,
                right: m as i8,
                top: layout::PAGE_PAD[0] as i8,
                bottom: layout::PAGE_PAD[1] as i8,
            })
            .show(ui, |ui| {
                if !ready {
                    grid::skeleton(ui, theme.density);
                    return;
                }
                if shelf.items.is_empty() {
                    nothing_here(ui, state, &lib);
                    return;
                }
                let action = match state.library.view {
                    ViewMode::Grid => {
                        grid::show(ui, state, env, &lib, &shelf, size, view_h, origin)
                    }
                    ViewMode::List => list::show(ui, state, env, &lib, &shelf, page_w, origin),
                };
                if let Some((id, a)) = action {
                    actions::apply(state, env, id, a);
                }
            });
    });
    let _ = out;
    state.dnd.target(viewport, DropTarget::Library);

    // `T`: the tag menu, hanging from the focused cover; it stays open while tags are ticked.
    if let Some(id) = state.library.tag_menu {
        let anchor = state
            .library
            .cursor_rect
            .unwrap_or_else(|| Rect::from_center_size(viewport.center(), vec2(0.0, 0.0)));
        let popup_id = Id::unique(("library.tag_menu", id));
        // Opened from the keyboard: focus enters the menu (ACCESSIBILITY §2).
        let seen = popup_id.with("seen");
        let first_frame = env.ctx.data(|d| d.get_temp::<bool>(seen)).is_none();
        env.ctx.data_mut(|d| d.insert_temp(seen, true));
        let mut open = true;
        let popup = Popup::new(
            popup_id,
            env.ctx.clone(),
            egui::PopupAnchor::ParentRect(anchor),
            egui::LayerId::new(egui::Order::Foreground, popup_id),
        )
        .kind(egui::PopupKind::Menu)
        .layout(Layout::top_down_justified(Align::Min))
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .accessible_name(strings::menu::TAGS())
        .align(RectAlign::BOTTOM_END)
        .open_bool(&mut open);
        context_menu::popup(popup, |ui| {
            crate::screens::dialogs::tags::tag_menu_opened(ui, state, env, &[id], first_frame);
        });
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            open = false;
        }
        if !open {
            env.ctx.data_mut(|d| d.remove_temp::<bool>(seen));
            state.library.tag_menu = None;
            state.pending_focus = Some(id);
        }
    }

    // The game menu, independent of which covers are laid out this frame.
    if let Some((id, at)) = state.library.menu {
        let mut open = true;
        let facts = menu::MenuFacts {
            has_shortcut: env.bridge.host().shortcuts.iter().any(|s| s.game == id),
            developer: env.bridge.prefs().advanced.developer_mode,
        };
        let anchor = state
            .library
            .menu_anchor
            .map_or(menu::Anchor::Pointer(at), menu::Anchor::Below);
        let mut sent = Vec::new();
        let tags = menu::TagEdit {
            new_tag: &mut state.tag_text,
            sent: &mut sent,
        };
        let chosen = menu::show_at(
            &env.ctx,
            &lib,
            id,
            anchor,
            &mut open,
            facts,
            true,
            Some(tags),
        );
        for c in sent {
            env.bridge.send(c);
        }
        if !open {
            state.library.menu = None;
            state.library.menu_anchor = None;
            state.pending_focus = Some(id);
        }
        if let Some(a) = chosen {
            actions::apply(state, env, id, a);
        }
    }
}

/// Whether the page moved further than its own height since the last frame (a scrollbar
/// drag, a fling, a jump to the cursor): what is on screen now will not stay, so the wall
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

/// Open the game menu for `id` (the Menu key, `Shift+F10`, the controller): it hangs from
/// the focused cover when `at` is that cover's centre, else it opens at `at`.
pub fn open_menu(state: &mut UiState, id: GameId, at: Pos2) {
    let below = state
        .library
        .cursor_rect
        .filter(|r| r.center() == at && state.library.selected == Some(id));
    state.library.selected = Some(id);
    state.library.menu = Some((id, at));
    state.library.menu_anchor = below;
}

/// Open the game menu for `id` at the pointer (a right-click).
pub(crate) fn open_menu_at(state: &mut UiState, id: GameId, at: Pos2) {
    state.library.selected = Some(id);
    state.library.menu = Some((id, at));
    state.library.menu_anchor = None;
}

/// Whether `focused` is the focus of a game in the library: its cover, or its row in the
/// list (a focused cover or row is the selected game).
pub fn is_game_focus(state: &UiState, focused: Id) -> bool {
    state
        .library
        .selected
        .is_some_and(|g| grid::card_id(g) == focused || list::row_id(g) == focused)
}

/// The per-game action of a single-key shortcut, for the focused game.
pub fn shortcut_action(a: crate::input::shortcuts::Action) -> Option<GameAction> {
    use crate::input::shortcuts::Action as A;
    Some(match a {
        A::Play => GameAction::Play,
        A::Stop => GameAction::Stop,
        A::Favorite => GameAction::Favorite,
        A::Tags => GameAction::TagMenu,
        A::Hide => GameAction::Hide,
        A::Remove => GameAction::Delete,
        A::OpenFolder => GameAction::OpenFolder,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_removes_the_selection() {
        assert_eq!(
            shortcut_action(crate::input::shortcuts::Action::Remove),
            Some(GameAction::Delete)
        );
        assert_eq!(
            shortcut_action(crate::input::shortcuts::Action::Tags),
            Some(GameAction::TagMenu)
        );
    }
}
