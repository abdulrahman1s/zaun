//! Main library (SCREEN_SPECS §2): toolbar, heading with result count, grid or list, empty
//! and loading states.

pub mod grid;
pub mod list;
pub mod sidebar;

use egui::{Align, Frame, Layout, Margin, Panel, Ui};

use super::Env;
use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::strings;
use crate::theme::tokens::{layout, space};
use crate::theme::{self, Type, icons};
use crate::vm::library::{Collection, Sort, ViewMode};
use crate::vm::toasts::ToastKind;
use crate::widgets::button::Button;
use crate::widgets::empty_state::empty_state;
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

fn toolbar(ui: &mut Ui, state: &mut UiState) {
    let theme = theme::theme(ui.ctx());
    let narrow = ui.available_width() < 560.0;
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        let search_w = (ui.available_width() * 0.42).clamp(160.0, 320.0);
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
        ui.add_space(space::S2);
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
            let mut sort = Sort::ALL
                .iter()
                .position(|s| *s == state.library.sort)
                .unwrap_or(0);
            let labels: Vec<&str> = Sort::ALL.iter().map(|s| s.label()).collect();
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
                state.library.sort = Sort::ALL[sort];
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(Button::primary(strings::library::ADD_GAME).icon(icons::PLUS))
                .clicked()
            {
                state.nav.open(crate::route::Overlay::AddGame);
            }
        });
    });
    let _ = theme;
}

/// Show the library route.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let m = margin(ui);
    Panel::top("library_toolbar")
        .exact_size(layout::TOOLBAR_H)
        .show_separator_line(false)
        .frame(Frame::NONE.inner_margin(Margin {
            left: m as i8,
            right: m as i8,
            top: space::S2 as i8,
            bottom: 0,
        }))
        .show(ui, |ui| toolbar(ui, state));

    let lib = env.bridge.library().clone();
    let visible = state.library.visible(&lib, env.now);
    let total = state.library.base_total(&lib, env.now);
    let collection_label = match state.library.collection {
        Collection::Tag(t) => lib
            .tags
            .iter()
            .find(|x| x.id == t)
            .map_or_else(|| strings::nav::TAGS.to_owned(), |x| x.name.to_string()),
        c => c.label().to_owned(),
    };

    egui::CentralPanel::default()
        .frame(Frame::NONE.inner_margin(Margin {
            left: m as i8,
            right: m as i8,
            top: space::S2 as i8,
            bottom: 0,
        }))
        .show(ui, |ui| {
            // Heading: the route's level-1 heading plus the result count.
            ui.horizontal(|ui| {
                ui.label(Type::Title.rich(&collection_label).color(p.text_primary));
                if lib.revision > 0 {
                    ui.label(
                        Type::Body
                            .numeric(strings::library::result_count(visible.len(), total))
                            .color(p.text_muted),
                    );
                }
            });
            ui.add_space(space::S3);

            if lib.revision == 0 {
                grid::skeleton(ui, theme.density);
                return;
            }
            if lib.games.is_empty() {
                ui.add_space(space::S16);
                if empty_state(
                    ui,
                    icons::FILE_EXE,
                    strings::library::EMPTY_TITLE,
                    strings::library::EMPTY_BODY,
                    Some(Button::primary(strings::library::ADD_GAME).icon(icons::PLUS)),
                )
                .is_some_and(|r| r.clicked())
                {
                    state.nav.open(crate::route::Overlay::AddGame);
                }
                return;
            }
            if visible.is_empty() {
                ui.add_space(space::S16);
                if !state.library.query.trim().is_empty() {
                    let msg = strings::library::no_results(state.library.query.trim());
                    if empty_state(
                        ui,
                        icons::SEARCH,
                        &msg,
                        "",
                        Some(Button::new(strings::library::CLEAR_SEARCH)),
                    )
                    .is_some_and(|r| r.clicked())
                    {
                        state.library.query.clear();
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
                ViewMode::Grid => grid::show(ui, state, env, &lib, &visible),
                ViewMode::List => list::show(ui, state, env, &lib, &visible),
            };
            if let Some(a) = action {
                apply(state, env, a);
            }
        });
}

/// What a card or row asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemAction {
    /// Open the game page.
    Open(easyplay_core::ids::GameId),
    /// Play or stop.
    Play(easyplay_core::ids::GameId),
    /// Toggle favourite.
    Favorite(easyplay_core::ids::GameId),
}

fn apply(state: &mut UiState, env: &mut Env<'_>, a: ItemAction) {
    let lib = env.bridge.library().clone();
    match a {
        ItemAction::Open(id) => {
            state.library.selected = Some(id);
            state.nav.push(crate::route::Route::Game(id));
        }
        ItemAction::Play(id) => {
            state.library.selected = Some(id);
            if let Some(g) = lib.game(id) {
                let cmd = match crate::widgets::card::play_state(g) {
                    crate::widgets::button::PlayState::Running
                    | crate::widgets::button::PlayState::Stopping => UiCommand::Stop(id),
                    crate::widgets::button::PlayState::Unavailable => {
                        state.nav.push(crate::route::Route::Game(id));
                        return;
                    }
                    _ => UiCommand::Launch(id),
                };
                env.bridge.send(cmd);
            }
        }
        ItemAction::Favorite(id) => {
            if let Some(g) = lib.game(id) {
                let on = !g.favorite;
                env.bridge.send(UiCommand::SetFavorite(id, on));
                state.toasts.push(
                    strings::feedback::favorited(&g.name, on),
                    ToastKind::Success,
                    None,
                    env.mono,
                );
            }
        }
    }
}
