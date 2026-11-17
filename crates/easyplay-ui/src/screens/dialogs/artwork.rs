//! The artwork dialog (script `artworkDialog`; SCREEN_SPECS_2 §14, UI-028, ADR-0018,
//! ADR-0024): a wide dialog with the cover preview (112) at the left and three rows beside
//! it — Cover, Icon, Banner — with boxed buttons (*Choose…*, *Use program icon*, and
//! *Remove* while an image is set), the fine print about dropping a picture, then *Official
//! artwork from Steam*: which Steam app the game is (what this computer says, else a store
//! search for its name, closest first) as pick rows that download that app's artwork; one
//! field takes words, an app ID or a store link. A spinner shows while an import runs, and
//! an import that fails says so under its row and keeps the old art.

use easyplay_core::config::PreferencePatch;
use easyplay_core::config::schema::ArtworkOnline;
use easyplay_core::ids::GameId;
use easyplay_core::model::ArtSlot;
use egui::{Context, Id, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use super::name_and_size;
use crate::app::UiState;
use crate::bridge::{ArtworkSource, UiCommand};
use crate::input::dnd::DropTarget;
use crate::input::picker::PickPurpose;
use crate::route::Overlay;
use crate::screens::Env;
use crate::strings::artwork as s;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::tokens::{layout, overlay, radius};
use crate::theme::{self, Type};
use crate::vm::AsyncValue;
use crate::vm::screens::ArtworkVm;
use crate::widgets::button::Button;
use crate::widgets::choice::{PickKind, pick};
use crate::widgets::cover::{self, CoverLook, CoverSource};
use crate::widgets::modal::{Spec, fine_print, show as dialog};
use crate::widgets::progress::spinner_widget;
use crate::widgets::rows::{SettingRow, heading};
use crate::widgets::text_field::TextField;

fn slot_label(slot: ArtSlot) -> &'static str {
    match slot {
        ArtSlot::Cover => s::COVER(),
        ArtSlot::Icon => s::ICON(),
        ArtSlot::Hero => s::HERO(),
    }
}

/// One row: the slot's name (with a word of help), and its buttons at the right.
#[allow(clippy::too_many_arguments)]
fn slot_row(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    game: GameId,
    slot: ArtSlot,
    art: bool,
) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let busy = state
        .artwork
        .as_ref()
        .is_some_and(|a| a.busy.contains(&slot));
    let help = match (slot, art) {
        (ArtSlot::Cover, false) => s::MADE_FROM_TITLE(),
        _ => "",
    };
    let mut choose = false;
    let mut program_icon = false;
    let mut remove = false;
    SettingRow::new(slot_label(slot))
        .help(help)
        .id_salt(("artwork.slot", slot_label(slot)))
        .show(ui, |ui| {
            if busy {
                spinner_widget(ui, p.t2);
            }
            if art
                && ui
                    .add(Button::new(s::REMOVE()).accessible_name(format!(
                        "{} {}",
                        s::REMOVE(),
                        slot_label(slot)
                    )))
                    .clicked()
            {
                remove = true;
            }
            if slot == ArtSlot::Icon && ui.add(Button::new(s::USE_ICON())).clicked() {
                program_icon = true;
            }
            choose |= ui
                .add(Button::new(s::CHOOSE()).accessible_name(s::slot_name(slot_label(slot), art)))
                .clicked();
        });
    if choose {
        state.picks.push(PickPurpose::Artwork(game, slot));
    }
    if program_icon {
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
    if remove {
        env.bridge.send(UiCommand::ClearArtwork { id: game, slot });
    }
    if let Some(err) = state
        .artwork
        .as_ref()
        .and_then(|a| a.errors.iter().find(|(x, _)| *x == slot))
    {
        Type::Caption.paragraph(ui, &err.1, p.coral);
    }
}

/// One result of the Steam list.
struct Found {
    app_id: u32,
    title: String,
    detail: String,
    in_use: bool,
}

fn found_rows(a: &ArtworkVm, stored: Option<u32>) -> Vec<Found> {
    let mut rows = Vec::new();
    let mut push = |app_id: u32, title: String, detail: String| {
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
        push(id, s::app(id), String::new());
        return rows;
    }
    if let Some(found) = a.apps.ready() {
        if let Some((id, source)) = found.detected {
            push(id, s::app(id), s::found_in(source).to_owned());
        }
        for c in &found.candidates {
            push(c.app_id, c.name.clone(), s::app(c.app_id));
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

fn steam_section(
    ui: &mut Ui,
    a: &mut ArtworkVm,
    env: &mut Env<'_>,
    stored: Option<u32>,
    offline: bool,
) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let prefs = env.bridge.prefs().clone();
    heading(ui, s::STEAM(), false);
    let quiet = |ui: &mut Ui, text: &str| {
        Type::Body.paragraph(ui, text, p.t3);
    };
    if prefs.artwork.online != ArtworkOnline::Steam {
        quiet(ui, s::STEAM_OFF());
        ui.add_space(overlay::BLANK_TEXT_GAP);
        if ui.add(Button::new(s::TURN_ON())).clicked() {
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
    quiet(ui, s::steam_body(offline));
    if matches!(a.apps, AsyncValue::Idle) {
        ask(a, env, None);
    }
    ui.add_space(overlay::BLANK_TEXT_GAP);
    let mut search = false;
    let w = ui.available_width();
    let bw = Button::new(s::SEARCH()).width(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = overlay::DIALOG_ACTION_GAP;
        let f = TextField::new(&mut a.term, "artwork.search")
            .accessible_name(s::SEARCH_LABEL())
            .hint(s::SEARCH_HINT())
            .width(w - bw - overlay::DIALOG_ACTION_GAP)
            .show(ui);
        search |= f.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        search |= ui.add(Button::new(s::SEARCH())).clicked();
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
            AsyncValue::Loading { .. } => Some((s::SEARCHING(), false)),
            AsyncValue::Failed(m) if m.is_empty() => Some((s::LOOKUP_FAILED(), true)),
            AsyncValue::Failed(m) => Some((m.as_str(), true)),
            AsyncValue::Ready(_) if rows.is_empty() && !prefs.artwork.name_search => {
                Some((s::NAME_SEARCH_OFF(), false))
            }
            AsyncValue::Ready(_) if rows.is_empty() => Some((s::NOTHING_FOUND(), false)),
            _ => None,
        }
    };
    if let Some((text, bad)) = status {
        ui.add_space(overlay::PRE[0]);
        let colour = if bad { p.coral } else { p.t3 };
        Type::Caption.paragraph(ui, text, colour);
    }
    if !rows.is_empty() {
        ui.add_space(overlay::BLANK_TEXT_GAP - overlay::PRE[1] / 2.0 - 1.0);
    }
    let mut chosen = None;
    for r in &rows {
        let mark = if a.fetching == Some(r.app_id) {
            s::DOWNLOADING()
        } else if r.in_use {
            s::IN_USE()
        } else {
            ""
        };
        let (resp, ()) = pick(
            ui,
            PickKind::Radio,
            r.in_use,
            &s::row_name(&r.title, r.app_id),
            |ui| {
                name_and_size(ui, &r.title, mark);
                if !r.detail.is_empty() {
                    ui.label(Type::Caption.rich(&r.detail).color(p.t3));
                }
            },
        );
        if resp.clicked() {
            chosen = Some(r.app_id);
        }
    }
    if let Some(app_id) = chosen
        && a.fetching.is_none()
    {
        a.fetching = Some(app_id);
        a.fetch_error = None;
        a.busy.extend([ArtSlot::Cover, ArtSlot::Hero]);
        env.bridge.send(UiCommand::FetchSteamArtwork {
            id: a.game,
            app_id: Some(app_id),
        });
    }
    if let Some(e) = &a.fetch_error {
        Type::Caption.paragraph(ui, e, p.coral);
    }
}

/// Show the dialog for `game`.
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
    let offline = g.preset.base() == easyplay_core::model::BasePreset::Offline;
    let title = s::title(&g.name);
    let r = dialog(
        ctx,
        id,
        Spec::new(&title).wide().close_button(),
        |ui| {
            let theme = theme::theme(ui.ctx());
            let w = ui.available_width();
            let top = ui.cursor().min;
            // The cover preview at the left: the artwork, or the poster.
            let cw = layout::DIALOG_COVER;
            let rect = Rect::from_min_size(top, vec2(cw, cw * layout::COVER_ASPECT));
            let src = match g.cover {
                Some(art) => match env.textures.get(
                    TexKey {
                        art,
                        size: ThumbSize::cover_for(cw * ui.ctx().pixels_per_point()),
                    },
                    true,
                    env.art,
                ) {
                    TexState::Ready(t) => CoverSource::Texture(t, 1.0),
                    TexState::Pending(c) => {
                        c.map_or(CoverSource::Placeholder, CoverSource::Pending)
                    }
                },
                None => CoverSource::Placeholder,
            };
            cover::paint(
                ui.painter(),
                rect,
                src,
                &g.name,
                CoverLook::default(),
                &theme,
            );
            let resp = ui.interact(rect, Id::unique(("artwork.cover", game)), Sense::click());
            crate::a11y::button(&resp, s::COVER_PREVIEW());
            crate::a11y::describe(&resp, s::PREVIEW_HINT());
            if resp.clicked() {
                state.picks.push(PickPurpose::Artwork(game, ArtSlot::Cover));
            }
            state.dnd.target(rect, DropTarget::Cover(game));
            // The rows beside it.
            let x = top.x + cw + overlay::DIALOG_COVER_GAP;
            let mut col = ui.new_child(
                UiBuilder::new()
                    .id_salt("artwork.rows")
                    .max_rect(Rect::from_min_size(
                        pos2(x, top.y),
                        vec2(w - cw - overlay::DIALOG_COVER_GAP, f32::INFINITY),
                    ))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            col.spacing_mut().item_spacing.y = 0.0;
            slot_row(
                &mut col,
                state,
                env,
                game,
                ArtSlot::Cover,
                g.cover.is_some(),
            );
            slot_row(&mut col, state, env, game, ArtSlot::Icon, g.icon.is_some());
            slot_row(&mut col, state, env, game, ArtSlot::Hero, g.hero.is_some());
            col.add_space(overlay::NOTE_CODE_GAP);
            fine_print(&mut col, s::HOW_TO_CHANGE());
            let bottom = col.min_rect().bottom().max(rect.bottom());
            ui.allocate_rect(
                Rect::from_min_max(top, pos2(top.x + w, bottom)),
                Sense::hover(),
            );
            if let Some(a) = state.artwork.as_mut() {
                steam_section(ui, a, env, stored, offline);
            }
        },
        |ui| {
            let done = ui.add(Button::primary(s::DONE()));
            if crate::widgets::modal::opening(ui.ctx(), id) {
                done.request_focus();
            }
            if done.has_focus() {
                // `Enter` closes the dialog: always marked.
                crate::widgets::default_ring(
                    ui.painter(),
                    done.rect,
                    egui::CornerRadius::same(radius::MD),
                    &theme::theme(ui.ctx()),
                );
            }
            done.clicked()
        },
    );
    if r.close_requested || r.footer {
        if r.footer {
            crate::widgets::modal::closed(ctx, id);
        }
        state.artwork = None;
        state.nav.close(Overlay::Artwork(game));
    }
}
