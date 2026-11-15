//! Artwork sheet (SCREEN_SPECS_2 §14, UI-028, ADR-0018, ADR-0024): three slots (cover, icon,
//! banner) with preview; a slot is clicked or has an image dropped on it to change it,
//! *Use program icon* and *Remove* beneath; a spinner while an import runs and inline errors
//! that keep the old art. *Official artwork from Steam* says which Steam app the game is (what
//! this computer says, else a store search for its name, closest first) as rows that
//! download that app's artwork; one field takes words, an app ID or a store link.

use easyplay_core::config::PreferencePatch;
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
use crate::vm::AsyncValue;
use crate::vm::screens::ArtworkVm;
use crate::widgets::button::{Button, Size};
use crate::widgets::cover::{CoverSource, paint_cover};
use crate::widgets::permission_row::{Line, PermissionRow, Picked, Press, permission_rows};
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
        if resp.clicked() {
            state.picks.push(PickPurpose::Artwork(game, slot));
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = space::S1;
            if ui
                .add(Button::ghost(s::CHOOSE).size(Size::Sm).icon(icons::FOLDER))
                .clicked()
            {
                state.picks.push(PickPurpose::Artwork(game, slot));
            }
            if slot == ArtSlot::Icon && ui.add(Button::ghost(s::USE_ICON).size(Size::Sm)).clicked()
            {
                if let Some(a) = &mut state.artwork {
                    a.busy.push(slot);
                    a.errors.retain(|(x, _)| *x != slot);
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

/// One row of the Steam list.
struct Found {
    app_id: u32,
    title: String,
    detail: Option<String>,
    in_use: bool,
}

fn found_rows(a: &ArtworkVm, stored: Option<u32>) -> Vec<Found> {
    let mut rows = Vec::new();
    let mut push = |app_id: u32, title: String, detail: Option<String>| {
        if rows.iter().all(|r: &Found| r.app_id != app_id) {
            rows.push(Found {
                app_id,
                title,
                detail,
                in_use: stored == Some(app_id),
            });
        }
    };
    if let Some(id) = a.typed_app_id() {
        push(id, s::app(id), None);
        return rows;
    }
    if let Some(found) = a.apps.ready() {
        if let Some((id, source)) = found.detected {
            push(id, s::app(id), Some(s::found_in(source).to_owned()));
        }
        for c in &found.candidates {
            push(c.app_id, c.name.clone(), Some(s::app(c.app_id)));
        }
    }
    rows
}

fn ask(a: &mut ArtworkVm, env: &mut Env<'_>, term: Option<String>) {
    let q = env.bridge.query_id();
    a.apps_q = Some(q);
    a.apps = AsyncValue::Loading { since: env.mono };
    a.fetch_error = None;
    env.bridge.send(UiCommand::FindSteamApp {
        id: a.game,
        term,
        reply_to: q,
    });
}

fn steam_section(ui: &mut Ui, a: &mut ArtworkVm, env: &mut Env<'_>, stored: Option<u32>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let prefs = env.bridge.prefs().clone();
    ui.label(Type::Heading.rich(s::STEAM).color(p.text_primary));
    if prefs.artwork.online != ArtworkOnline::Steam {
        ui.label(Type::Caption.rich(s::STEAM_OFF).color(p.text_secondary));
        if ui
            .add(Button::new(s::TURN_ON).icon(icons::DOWNLOAD))
            .clicked()
        {
            let mut next = (*prefs).clone();
            next.artwork.online = ArtworkOnline::Steam;
            env.bridge.send(UiCommand::SetPreference {
                patch: PreferencePatch::new().set("artwork.online", "steam"),
                preview: Box::new(next.clone()),
            });
            env.bridge.set_prefs_preview(next);
            a.apps = AsyncValue::Idle;
        }
        return;
    }
    ui.label(Type::Caption.rich(s::STEAM_BODY).color(p.text_secondary));
    if matches!(a.apps, AsyncValue::Idle) {
        ask(a, env, None);
    }
    ui.add_space(space::S2);
    let mut search = false;
    ui.horizontal(|ui| {
        let f = TextField::new(&mut a.term, "artwork.search")
            .label(s::SEARCH_LABEL)
            .hint(s::SEARCH_HINT)
            .width(320.0)
            .show(ui);
        search |= f.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        search |= ui.add(Button::new(s::SEARCH).icon(icons::SEARCH)).clicked();
    });
    if search && a.typed_app_id().is_none() && !a.term.trim().is_empty() {
        let term = a.term.trim().to_owned();
        ask(a, env, Some(term));
    }
    let rows = found_rows(a, stored);
    let status = if a.typed_app_id().is_some() {
        None
    } else {
        match &a.apps {
            AsyncValue::Loading { .. } => Some((s::SEARCHING, false)),
            AsyncValue::Failed(m) if m.is_empty() => Some((s::LOOKUP_FAILED, true)),
            AsyncValue::Failed(m) => Some((m.as_str(), true)),
            AsyncValue::Ready(_) if rows.is_empty() && !prefs.artwork.name_search => {
                Some((s::NAME_SEARCH_OFF, false))
            }
            AsyncValue::Ready(_) if rows.is_empty() => Some((s::NOTHING_FOUND, false)),
            _ => None,
        }
    };
    ui.add_space(space::S2);
    if let Some((text, bad)) = status {
        let colour = if bad { p.danger } else { p.text_muted };
        ui.label(Type::Caption.rich(text).color(colour));
    }
    if rows.is_empty() {
        return;
    }
    let list: Vec<PermissionRow<'_>> = rows
        .iter()
        .map(|r| {
            let mut row = PermissionRow::new(
                Id::unique("artwork.steam").with(r.app_id),
                icons::DOWNLOAD,
                r.title.as_str(),
            );
            row.lines = r.detail.iter().map(|d| Line::new(d.as_str())).collect();
            row.mark = if a.fetching == Some(r.app_id) {
                Some(s::DOWNLOADING)
            } else if r.in_use {
                Some(s::IN_USE)
            } else {
                None
            };
            row.press = Press::Action;
            row.name = s::row_name(&r.title, r.app_id).into();
            row.hint = Some(s::DOWNLOAD.into());
            row
        })
        .collect();
    if let Some((i, Picked::Pressed)) = permission_rows(ui, &list)
        && a.fetching.is_none()
    {
        let app_id = rows[i].app_id;
        a.fetching = Some(app_id);
        a.fetch_error = None;
        a.busy.extend([ArtSlot::Cover, ArtSlot::Hero]);
        env.bridge.send(UiCommand::FetchSteamArtwork {
            id: a.game,
            app_id: Some(app_id),
        });
    }
    if let Some(e) = &a.fetch_error {
        ui.horizontal_wrapped(|ui| {
            ui.label(icons::icon(icons::ERROR, theme::tokens::icon::INLINE).color(p.danger));
            ui.label(Type::Caption.rich(e).color(p.danger));
        });
    }
}

/// Show the sheet for `game`.
pub fn show(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>, game: GameId) {
    let lib = env.bridge.library().clone();
    let Some(g) = lib.get(game).cloned() else {
        state.nav.close(Overlay::Artwork(game));
        return;
    };
    if state.artwork.as_ref().is_none_or(|a| a.game != game) {
        state.artwork = Some(ArtworkVm::new(game, None));
    }
    if let Some(a) = &mut state.artwork
        && a.term.is_empty()
        && matches!(a.apps, AsyncValue::Idle)
    {
        a.term = g.name.to_string();
    }
    let stored = g.steam_app_id;
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
                    vec2(120.0, 180.0),
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
                        g.hero,
                        vec2(ui.available_width().min(260.0), 100.0),
                    );
                }
            });
            ui.add_space(space::S2);
            ui.label(Type::Caption.rich(s::HOW_TO_CHANGE).color(p.text_muted));
            ui.add_space(space::S5);
            if let Some(a) = state.artwork.as_mut() {
                steam_section(ui, a, env, stored);
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
