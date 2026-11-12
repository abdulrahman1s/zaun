//! Routes whose screens arrive with later tasks: the game page (UI-014), statistics (UI-022),
//! settings (UI-023) and onboarding (UI-024). Each shows what exists and the way back; the
//! game page already shows the hero title, the page-size Play pill and the access summary.

use easyplay_core::ids::GameId;
use egui::{ScrollArea, Ui};

use super::Env;
use super::gallery::appearance_panel;
use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::route::Route;
use crate::strings;
use crate::theme::tokens::{control, icon as icon_size, layout, space};
use crate::theme::{self, Type, icons};
use crate::widgets::button::{Button, PillSize, PlayState, play_pill};
use crate::widgets::card::play_state;
use crate::widgets::empty_state::empty_state;
use crate::widgets::icon_button::IconButton;
use crate::widgets::shield;

fn page<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let m = super::library::margin(ui);
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(m as i8, space::S6 as i8))
        .show(ui, add)
        .inner
}

/// Statistics, settings, onboarding.
pub fn show(ui: &mut Ui, state: &mut UiState, route: Route) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    page(ui, |ui| {
        let title = route.title(None);
        ui.label(Type::Title.rich(title).color(p.text_primary));
        ui.add_space(space::S6);
        if let Route::Settings(_) = route {
            ui.label(
                Type::Heading
                    .rich(strings::gallery::THEME)
                    .color(p.text_primary),
            );
            ui.add_space(space::S2);
            ui.scope(|ui| {
                ui.set_max_width(layout::SHEET_MAX_W);
                appearance_panel(ui, &mut state.gallery);
            });
            return;
        }
        ui.add_space(space::S16);
        if empty_state(
            ui,
            if route == Route::Statistics {
                icons::CHART
            } else {
                icons::WRENCH
            },
            strings::placeholder::COMING_TITLE,
            strings::placeholder::COMING_BODY,
            Some(Button::new(strings::placeholder::BACK_TO_LIBRARY).icon(icons::BACK)),
        )
        .is_some_and(|r| r.clicked())
        {
            state.nav.go_to(Route::Library);
        }
    });
}

/// The game page frame: back, title, Play pill, access summary.
pub fn game(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, id: GameId) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let lib = env.bridge.library().clone();
    let Some(g) = lib.game(id) else {
        state.nav.back();
        return;
    };
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            page(ui, |ui| {
                if ui
                    .add(
                        IconButton::new(icons::BACK, strings::nav::BACK)
                            .shortcut(strings::nav::BACK_SHORTCUT),
                    )
                    .clicked()
                {
                    state.nav.back();
                }
                ui.add_space(space::S4);
                ui.label(Type::Display.rich(&*g.name).color(p.text_primary));
                let meta = strings::card::played(
                    g.total_playtime_ms,
                    g.last_played_at,
                    env.now,
                    env.utc_offset_s,
                );
                ui.label(Type::Body.numeric(meta).color(p.text_secondary));
                ui.add_space(space::S5);
                ui.horizontal(|ui| {
                    let st = play_state(g);
                    if play_pill(ui, &st, &g.name, PillSize::Page, 1.0).clicked() {
                        let cmd = match st {
                            PlayState::Running | PlayState::Stopping => Some(UiCommand::Stop(id)),
                            PlayState::Unavailable => None,
                            _ => Some(UiCommand::Launch(id)),
                        };
                        if let Some(c) = cmd {
                            env.bridge.send(c);
                        }
                    }
                    ui.add_space(space::S2);
                    let fav_label = strings::card::FAVORITE;
                    let icon = if g.favorite {
                        icons::STAR_FILL
                    } else {
                        icons::STAR
                    };
                    if ui
                        .add(
                            IconButton::new(icon, fav_label)
                                .toggle(g.favorite)
                                .size(control::LG)
                                .icon_size(icon_size::BUTTON),
                        )
                        .clicked()
                    {
                        env.bridge.send(UiCommand::SetFavorite(id, !g.favorite));
                    }
                });
                ui.add_space(space::S8);
                ui.scope(|ui| {
                    ui.set_max_width(420.0);
                    let statements: Vec<(theme::Icon, &str)> = match g.network {
                        easyplay_core::model::NetworkAccess::None => {
                            vec![(icons::WIFI_OFF, strings::access::NO_INTERNET)]
                        }
                        easyplay_core::model::NetworkAccess::Full => {
                            vec![(icons::GLOBE, strings::access::INTERNET)]
                        }
                    };
                    let mut all = statements;
                    all.push((icons::FOLDER, strings::access::OWN_FOLDER));
                    all.push((icons::GAMEPAD, strings::access::CONTROLLERS));
                    shield::summary_card(ui, g.preset, &all, strings::access::SEE_ALL);
                });
                ui.add_space(space::S8);
                ui.label(
                    Type::Caption
                        .rich(strings::placeholder::COMING_BODY)
                        .color(p.text_muted),
                );
            });
        });
}
