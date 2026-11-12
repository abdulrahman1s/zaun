//! Artwork sheet (SCREEN_SPECS_2 §14, UI-028, ADR-0018 UI parts): three slots (cover, icon,
//! hero) with preview, *Choose image…*, *Use program icon*, *Remove*, a drop target per slot,
//! a spinner while an import runs and inline errors that keep the old art. The Steam section
//! fetches official artwork by app ID (typed, or a store URL) when online artwork is on.

use easyplay_core::config::schema::ArtworkOnline;
use easyplay_core::ids::GameId;
use easyplay_core::model::ArtSlot;
use egui::{Context, CornerRadius, Id, Sense, Ui, vec2};

use crate::app::UiState;
use crate::bridge::{ArtworkSource, UiCommand};
use crate::input::dnd::DropTarget;
use crate::input::picker::PickPurpose;
use crate::route::Overlay;
use crate::screens::Env;
use crate::strings;
use crate::strings::artwork as s;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::tokens::{radius, space};
use crate::theme::{self, Type, icons};
use crate::widgets::button::{Button, Size};
use crate::widgets::cover::{CoverSource, paint_cover};
use crate::widgets::progress::spinner;
use crate::widgets::sheet::sheet;
use crate::widgets::text_field::TextField;

fn slot_label(slot: ArtSlot) -> &'static str {
    match slot {
        ArtSlot::Cover => s::COVER,
        ArtSlot::Icon => s::ICON,
        ArtSlot::Hero => s::HERO,
    }
}

#[allow(clippy::too_many_arguments)]
fn slot(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    game: GameId,
    name: &str,
    slot: ArtSlot,
    art: Option<easyplay_core::model::ArtKey>,
    size: egui::Vec2,
) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    ui.vertical(|ui| {
        ui.set_width(size.x.max(140.0));
        ui.label(
            Type::BodyStrong
                .rich(slot_label(slot))
                .color(p.text_primary),
        );
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let busy = state
            .artwork
            .as_ref()
            .is_some_and(|a| a.busy.contains(&slot));
        let src = match art {
            Some(a) => match env.textures.get(
                TexKey {
                    art: a,
                    size: ThumbSize::cover_for(size.x * ui.ctx().pixels_per_point()),
                },
                true,
                env.art,
            ) {
                TexState::Ready(t) => CoverSource::Texture(t, 1.0),
                TexState::Pending(c) => c.map_or(CoverSource::Placeholder, CoverSource::Pending),
            },
            None => CoverSource::Placeholder,
        };
        paint_cover(
            ui.painter(),
            rect,
            CornerRadius::same(radius::MD),
            src,
            name,
            &theme,
        );
        if busy {
            spinner(ui, rect.center(), space::S4, p.on_art);
        }
        crate::a11y::button(&resp, &s::slot_name(slot_label(slot), art.is_some()));
        state.dnd.target(rect, DropTarget::Cover(game));
        let choose = ui.add(Button::new(s::CHOOSE).size(Size::Sm).icon(icons::FOLDER));
        if choose.clicked() || resp.clicked() {
            state.picks.push(PickPurpose::Artwork(game, slot));
        }
        ui.label(Type::Caption.rich(s::FORMATS).color(p.text_muted));
        ui.horizontal_wrapped(|ui| {
            if slot != ArtSlot::Hero && ui.add(Button::ghost(s::USE_ICON).size(Size::Sm)).clicked()
            {
                if let Some(a) = &mut state.artwork {
                    a.busy.push(slot);
                }
                env.bridge.send(UiCommand::ImportArtwork {
                    id: game,
                    slot,
                    source: ArtworkSource::ProgramIcon,
                });
            }
            if art.is_some() && ui.add(Button::ghost(s::REMOVE).size(Size::Sm)).clicked() {
                env.bridge.send(UiCommand::ClearArtwork { id: game, slot });
            }
        });
        if let Some(err) = state
            .artwork
            .as_ref()
            .and_then(|a| a.errors.iter().find(|(x, _)| *x == slot))
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(icons::icon(icons::ERROR, theme::tokens::icon::INLINE).color(p.danger));
                ui.label(Type::Caption.rich(&err.1).color(p.danger));
            });
        }
    });
}

/// Show the sheet for `game`.
pub fn show(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>, game: GameId) {
    let lib = env.bridge.library().clone();
    let Some(g) = lib.get(game).cloned() else {
        state.nav.close(Overlay::Artwork(game));
        return;
    };
    if state.artwork.as_ref().is_none_or(|a| a.game != game) {
        state.artwork = Some(crate::vm::screens::ArtworkVm::new(game, None));
    }
    let online = env.bridge.prefs().artwork.online == ArtworkOnline::Steam;
    let title = s::title(&g.name);
    let r = sheet(
        ctx,
        id,
        &title,
        false,
        |ui| {
            let p = theme::theme(ui.ctx()).palette.clone();
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = space::S6;
                slot(
                    ui,
                    state,
                    env,
                    game,
                    &g.name,
                    ArtSlot::Cover,
                    g.cover,
                    vec2(140.0, 210.0),
                );
                slot(
                    ui,
                    state,
                    env,
                    game,
                    &g.name,
                    ArtSlot::Icon,
                    g.icon,
                    vec2(96.0, 96.0),
                );
                if ui.available_width() > 200.0 {
                    slot(
                        ui,
                        state,
                        env,
                        game,
                        &g.name,
                        ArtSlot::Hero,
                        None,
                        vec2(ui.available_width().min(260.0), 110.0),
                    );
                }
            });
            ui.add_space(space::S5);
            ui.label(Type::Heading.rich(s::STEAM).color(p.text_primary));
            if online {
                ui.label(Type::Caption.rich(s::STEAM_BODY).color(p.text_secondary));
                let Some(a) = state.artwork.as_mut() else {
                    return;
                };
                let err = a.app_id_error.then_some(s::APP_ID_ERROR);
                ui.horizontal(|ui| {
                    TextField::new(&mut a.app_id, "artwork.appid")
                        .label(s::APP_ID)
                        .hint(s::APP_ID_HINT)
                        .error(err)
                        .width(260.0)
                        .show(ui);
                });
                if ui
                    .add(Button::new(s::FETCH).icon(icons::DOWNLOAD))
                    .clicked()
                {
                    let parsed = a.parsed_app_id();
                    a.app_id_error = parsed.is_none() && !a.app_id.trim().is_empty();
                    if !a.app_id_error {
                        a.busy.push(ArtSlot::Cover);
                        env.bridge.send(UiCommand::FetchSteamArtwork {
                            id: game,
                            app_id: parsed,
                        });
                    }
                }
            } else {
                ui.label(Type::Caption.rich(s::STEAM_OFF).color(p.text_muted));
            }
        },
        |ui| ui.add(Button::primary(strings::feedback::CLOSE)).clicked(),
    );
    if r.close_requested || r.footer {
        crate::widgets::modal::closed(ctx, id);
        state.artwork = None;
        state.nav.close(Overlay::Artwork(game));
    }
}
