//! The game page (SCREEN_SPECS §4, UI-014): back and overflow, hero with ambient tint, cover
//! menu, inline rename, meta line with tags, the page-size Play pill, one state area (launch
//! progress, problem panel, still running, availability), tabs, and the Overview cards.
//! The page renders at once from the `GameSummary`; the full record and the playtime query
//! fill in (skeletons after 150 ms).

pub mod activity;
pub mod launch_options;
pub mod launch_panel;
pub mod overview;
pub mod permissions;
pub mod runtime;

use easyplay_core::ids::GameId;
use easyplay_core::model::{Activity, ArtSlot, Availability, GameSummary};
use easyplay_core::sandbox::{AccessCategory, AccessStatement, StatementSeverity};
use egui::{Align, CornerRadius, Key, Layout, Modifiers, Popup, ScrollArea, Sense, Ui, vec2};

use super::Env;
use super::actions::{self, GameAction};
use crate::app::UiState;
use crate::bridge::{GameEdit, UiCommand};
use crate::input::dnd::DropTarget;
use crate::input::picker::PickPurpose;
use crate::strings;
use crate::strings::details as s;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{control, icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Icon, Type, icons};
use crate::vm::AsyncValue;
use crate::vm::details::{DetailsTab, DetailsVm, range_for};
use crate::vm::toasts::ToastKind;
use crate::widgets::button::{Button, PillSize, Size, Variant, play_pill};
use crate::widgets::card::play_state;
use crate::widgets::chip::{ChipKind, chip};
use crate::widgets::context_menu::item;
use crate::widgets::cover::{CoverSource, paint_cover, placeholder_colors};
use crate::widgets::icon_button::IconButton;
use crate::widgets::shield;
use crate::widgets::tabs::tabs;

/// The icon of an access statement.
pub fn statement_icon(st: &AccessStatement) -> Icon {
    match st.category {
        AccessCategory::Network => {
            if st.key.ends_with("none") {
                icons::WIFI_OFF
            } else {
                icons::GLOBE
            }
        }
        AccessCategory::OwnFolder => {
            if st.key.contains("private") {
                icons::FLOPPY
            } else {
                icons::FOLDER
            }
        }
        AccessCategory::YourFolders => {
            if st.key.ends_with("none") {
                icons::FOLDER_LOCK
            } else if st.key.contains("removable") {
                icons::USB
            } else {
                icons::FOLDER
            }
        }
        AccessCategory::Devices => {
            if st.key.contains("sound") {
                icons::SPEAKER
            } else if st.key.contains("camera") {
                icons::CAMERA
            } else {
                icons::GAMEPAD
            }
        }
        AccessCategory::Display => icons::MONITOR,
        AccessCategory::Identity => icons::INFO,
        AccessCategory::HardeningChanges => icons::WARNING,
        AccessCategory::Exceptions => icons::WRENCH,
    }
}

/// A card: `bg.surface`, `r.md`, a level-2 heading.
pub fn card<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = theme::theme(ui.ctx()).palette.clone();
    egui::Frame::NONE
        .fill(p.bg_surface)
        .stroke(egui::Stroke::new(stroke::BORDER, p.border_subtle))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(space::S4)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = space::S2;
            let h = ui.label(Type::Heading.rich(title).color(p.text_primary));
            crate::a11y::labelled(ui.ctx(), h.id, egui::Role::Heading, title);
            add(ui)
        })
        .inner
}

/// One access statement row; `marker` is "added"/"removed" while editing.
pub fn statement_row(ui: &mut Ui, st: &AccessStatement, marker: Option<&str>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let color = match st.severity {
        StatementSeverity::Warning => p.warning,
        StatementSeverity::Danger => p.danger,
        _ => p.text_primary,
    };
    ui.horizontal(|ui| {
        ui.label(icons::icon(statement_icon(st), icon_size::INLINE).color(
            if matches!(
                st.severity,
                StatementSeverity::Warning | StatementSeverity::Danger
            ) {
                color
            } else {
                p.text_secondary
            },
        ));
        ui.add(egui::Label::new(Type::Body.rich(&st.text).color(color)).wrap());
        if let Some(m) = marker {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(Type::Caption.rich(m).color(p.accent));
            });
        }
    });
    if let Some(d) = &st.detail {
        ui.label(Type::Caption.rich(d).color(p.text_muted));
    }
}

/// Reset the page state for `id` and ask for its record and playtime.
fn enter(state: &mut UiState, env: &mut Env<'_>, id: GameId) {
    let tab = if state.details.game.is_none() {
        state.details.tab
    } else {
        DetailsTab::Overview
    };
    state.details = DetailsVm {
        game: Some(id),
        tab,
        ..DetailsVm::default()
    };
    load(state, env, id);
    overview::request(state, env, id);
}

fn load(state: &mut UiState, env: &mut Env<'_>, id: GameId) {
    let q = env.bridge.query_id();
    state.details.detail_q = Some(q);
    if !matches!(state.details.detail, AsyncValue::Ready(_)) {
        state.details.detail = AsyncValue::Loading { since: env.mono };
    }
    env.bridge.send(UiCommand::LoadGame { id, reply_to: q });
}

fn hero_cover(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    g: &GameSummary,
    rect: egui::Rect,
) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let resp = ui.interact(rect, egui::Id::unique(("hero_cover", g.id)), Sense::click());
    let src = match g.cover {
        Some(a) => match env.textures.get(
            TexKey {
                art: a,
                size: ThumbSize::cover_for(rect.width() * ui.ctx().pixels_per_point()),
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
        &g.name,
        &theme,
    );
    if resp.hovered() || resp.has_focus() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(radius::MD), fade(p.scrim, 0.35));
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::GRID.text(false),
            icons::font(icon_size::NAV),
            p.on_art,
        );
    }
    if resp.has_focus() {
        crate::widgets::focus_ring(ui.painter(), rect, CornerRadius::same(radius::MD), &theme);
    }
    crate::a11y::button(&resp, s::CHANGE_PICTURE);
    state.dnd.target(rect, DropTarget::Cover(g.id));
    if resp.clicked() {
        state.details.cover_menu = true;
    }
    if state.details.cover_menu {
        let mut open = true;
        let mut chosen = None;
        Popup::menu(&resp)
            .open_bool(&mut open)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                if item(
                    ui,
                    Some(icons::FOLDER),
                    strings::artwork::CHOOSE,
                    None,
                    false,
                )
                .clicked()
                {
                    chosen = Some(0);
                }
                if item(
                    ui,
                    Some(icons::FILE_EXE),
                    strings::artwork::USE_ICON,
                    None,
                    false,
                )
                .clicked()
                {
                    chosen = Some(1);
                }
                if g.cover.is_some()
                    && item(
                        ui,
                        Some(icons::TRASH),
                        strings::artwork::REMOVE,
                        None,
                        false,
                    )
                    .clicked()
                {
                    chosen = Some(2);
                }
                if item(ui, Some(icons::GRID), strings::menu::ARTWORK, None, false).clicked() {
                    chosen = Some(3);
                }
            });
        if !open || chosen.is_some() {
            state.details.cover_menu = false;
        }
        match chosen {
            Some(0) => state.picks.push(PickPurpose::Artwork(g.id, ArtSlot::Cover)),
            Some(1) => env.bridge.send(UiCommand::ImportArtwork {
                id: g.id,
                slot: ArtSlot::Cover,
                source: crate::bridge::ArtworkSource::ProgramIcon,
            }),
            Some(2) => env.bridge.send(UiCommand::ClearArtwork {
                id: g.id,
                slot: ArtSlot::Cover,
            }),
            Some(3) => actions::apply(state, env, g.id, GameAction::Artwork),
            _ => {}
        }
    }
}

fn title(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    if let Some(text) = &mut state.details.rename {
        let r = ui.add(
            egui::TextEdit::singleline(text)
                .font(Type::Title.font_id())
                .desired_width(ui.available_width().min(520.0))
                .char_limit(80),
        );
        crate::a11y::labelled(ui.ctx(), r.id, egui::Role::TextInput, s::RENAME_FIELD);
        crate::a11y::describe(&r, s::RENAME_HINT);
        if !r.has_focus() && !r.lost_focus() {
            r.request_focus();
        }
        let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        if esc {
            state.details.rename = None;
        } else if enter || r.lost_focus() {
            let name = text.trim().to_owned();
            state.details.rename = None;
            if !name.is_empty()
                && name != *g.name
                && easyplay_core::model::validate_name(&name).is_ok()
            {
                env.bridge.send(UiCommand::UpdateGame {
                    id: g.id,
                    edits: vec![GameEdit::Name(name.clone())],
                });
                state.toast(
                    strings::feedback::renamed(&name),
                    ToastKind::Success,
                    Some(UiCommand::UpdateGame {
                        id: g.id,
                        edits: vec![GameEdit::Name(g.name.to_string())],
                    }),
                    env.mono,
                );
            }
        }
    } else {
        let r = ui.add(
            egui::Label::new(Type::Display.rich(&*g.name).color(hero_text(&p).0))
                .sense(Sense::click())
                .truncate(),
        );
        crate::a11y::labelled(ui.ctx(), r.id, egui::Role::Heading, &g.name);
        if r.clicked() {
            state.details.rename = Some(g.name.to_string());
        }
        r.on_hover_text(s::RENAME_TOOLTIP);
    }
}

fn meta_line(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let lib = env.bridge.library().clone();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(space::S2, space::S1);
        shield::badge(ui, g.preset, g.flags.needs_attention, icon_size::INLINE);
        let (fg, fg2) = hero_text(&p);
        ui.label(
            Type::BodyStrong
                .rich(strings::shield::preset_name(g.preset))
                .color(fg),
        );
        ui.label(Type::Body.rich("·").color(fg2));
        ui.label(Type::Body.rich(&*g.runtime_label).color(fg2));
        for t in &g.tags {
            if let Some(t) = lib.tag(*t) {
                chip(
                    ui,
                    ChipKind::Tag {
                        color: t.color.index(),
                        removable: false,
                    },
                    &t.name,
                    None,
                    true,
                );
            }
        }
        if ui
            .add(IconButton::new(icons::PLUS, strings::menu::NEW_TAG).shortcut("T"))
            .clicked()
        {
            actions::apply(state, env, g.id, GameAction::EditTags);
        }
    });
}

fn hero(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let compact = ui.available_width() < layout::COMPACT_BELOW;
    let cover_w = if compact { 96.0 } else { 132.0 };
    let h = cover_w * layout::COVER_ASPECT + space::S6 * 2.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    // Ambient tint from the cover's dominant colour (snapshot or thumbnail header), else the
    // placeholder colours; fading into the page.
    let accent = g
        .accent
        .map(|[r, gr, b]| egui::Color32::from_rgb(r, gr, b))
        .or_else(|| g.cover.and_then(|c| env.textures.dominant(c)));
    let (a, b) = match accent {
        Some(c) => (c, mix(c, p.bg_base, 0.4)),
        None => placeholder_colors(&g.name, &theme),
    };
    let top = mix(p.bg_base, a, if p.kind.is_dark() { 0.55 } else { 0.35 });
    let bottom = mix(p.bg_base, b, if p.kind.is_dark() { 0.25 } else { 0.15 });
    crate::widgets::cover::paint_vertical_gradient(ui.painter(), rect, top, bottom);
    // The banner image, cropped to fill, under a scrim that keeps the text readable.
    if let Some(art) = g.hero
        && let TexState::Ready(tex) = env.textures.get(
            TexKey {
                art,
                size: ThumbSize::Hero,
            },
            true,
            env.art,
        )
    {
        let [tw, th] = ThumbSize::Hero.pixels();
        let uv = cover_uv(rect.aspect_ratio(), tw as f32 / th as f32);
        ui.painter().image(tex, rect, uv, egui::Color32::WHITE);
        let scrim = fade(p.bg_base, if p.kind.is_dark() { 0.55 } else { 0.65 });
        crate::widgets::cover::paint_vertical_gradient(
            ui.painter(),
            rect,
            fade(p.bg_base, 0.2),
            scrim,
        );
    }
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(radius::LG),
        fade(p.scrim, if p.kind.is_dark() { 0.35 } else { 0.0 }),
    );
    let inner = rect.shrink(space::S6);
    let cover = egui::Rect::from_min_size(inner.min, vec2(cover_w, cover_w * layout::COVER_ASPECT));
    hero_cover(ui, state, env, g, cover);
    let text = egui::Rect::from_min_max(
        egui::pos2(cover.right() + space::S6, inner.top()),
        inner.max,
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(text)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            ui.spacing_mut().item_spacing.y = space::S2;
            if !p.kind.is_dark() {
                // Light themes: the tint is light, so text uses the normal roles.
                let v = &mut ui.style_mut().visuals;
                v.override_text_color = None;
            }
            title(ui, state, env, g);
            meta_line(ui, state, env, g);
            ui.add_space(space::S2);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::S4;
                let st = play_state(g);
                let pill = play_pill(ui, &st, &g.name, PillSize::Page, 1.0);
                if !state.details.play_focused {
                    // Initial focus: the Play pill.
                    state.details.play_focused = true;
                    pill.request_focus();
                }
                if pill.clicked() {
                    actions::play(state, env, g);
                }
                crate::widgets::card::tick_elapsed(ui.ctx(), g.activity, env.now);
                let status = strings::card::state(g.activity, g.availability, env.now)
                    .unwrap_or_else(|| {
                        s::played_line(
                            g.total_playtime_ms,
                            g.last_played_at,
                            env.now,
                            env.utc_offset_s,
                        )
                    });
                ui.label(Type::Body.numeric(status).color(hero_text(&p).1));
            });
        },
    );
    ui.advance_cursor_after_rect(rect);
}

/// UV rectangle that crops an image of aspect `image` to fill a rectangle of aspect `rect`
/// (centred).
pub fn cover_uv(rect: f32, image: f32) -> egui::Rect {
    if !(rect > 0.0 && image > 0.0) {
        return egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    }
    if rect > image {
        // Wider than the image: crop top and bottom.
        let h = image / rect;
        let y = (1.0 - h) / 2.0;
        egui::Rect::from_min_max(egui::pos2(0.0, y), egui::pos2(1.0, y + h))
    } else {
        let w = rect / image;
        let x = (1.0 - w) / 2.0;
        egui::Rect::from_min_max(egui::pos2(x, 0.0), egui::pos2(x + w, 1.0))
    }
}

/// Text colours over the hero: on-art colours over the dark tint, normal roles in light
/// themes (the tint is light there).
pub fn hero_text(p: &theme::Palette) -> (egui::Color32, egui::Color32) {
    if p.kind.is_dark() {
        (p.on_art, p.on_art_secondary)
    } else {
        (p.text_primary, p.text_secondary)
    }
}

fn top_bar(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.horizontal(|ui| {
        if ui
            .add(
                Button::ghost(strings::library::TITLE)
                    .icon(icons::BACK)
                    .size(Size::Sm),
            )
            .on_hover_text(strings::nav::BACK_SHORTCUT)
            .clicked()
        {
            state.navigate(crate::vm::dialogs::AfterDiscard::Back);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let fav = ui.add(
                IconButton::new(
                    if g.favorite {
                        icons::STAR_FILL
                    } else {
                        icons::STAR
                    },
                    strings::card::FAVORITE,
                )
                .toggle(g.favorite)
                .shortcut("F"),
            );
            if fav.clicked() {
                actions::apply(state, env, g.id, GameAction::Favorite);
            }
            let more = ui.add(IconButton::new(icons::DOTS, strings::menu::NAME));
            if more.clicked() {
                state.library.menu = Some((g.id, more.rect.left_bottom()));
            }
            let _ = p;
        });
    });
}

/// Show the game page.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, id: GameId) {
    let lib = env.bridge.library().clone();
    let Some(g) = lib.get(id).cloned() else {
        return;
    };
    if state.details.game != Some(id) {
        enter(state, env, id);
    } else if state.details.detail_q.is_none()
        && state.details.loaded_rev != 0
        && state.details.loaded_rev != lib.revision
    {
        load(state, env, id);
    }
    // Page shortcuts not in the global table.
    if state.nav.overlays().is_empty()
        && ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S))
    {
        permissions::save(state, env);
    }
    let m = super::library::margin(ui);
    let dirty_bar = state.details.dirty();
    if dirty_bar {
        egui::Panel::bottom("details_save_bar")
            .frame(
                egui::Frame::NONE
                    .fill(theme::theme(ui.ctx()).palette.bg_surface)
                    .inner_margin(egui::Margin::symmetric(m as i8, space::S3 as i8)),
            )
            .show_separator_line(true)
            .show(ui, |ui| save_bar(ui, state, env));
    }
    ScrollArea::vertical()
        .id_salt(("details", id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(m as i8, space::S4 as i8))
                .show(ui, |ui| {
                    top_bar(ui, state, env, &g);
                    ui.add_space(space::S2);
                    hero(ui, state, env, &g);
                    ui.add_space(space::S4);
                    launch_panel::state_area(ui, state, env, &g);
                    let developer = env.bridge.prefs().advanced.developer_mode;
                    if !developer && state.details.tab == DetailsTab::Logs {
                        state.details.tab = DetailsTab::Overview;
                    }
                    let names: Vec<&str> = DetailsTab::visible(developer)
                        .iter()
                        .map(|t| s::tab(*t))
                        .collect();
                    let mut sel = state.details.tab.index();
                    if tabs(ui, s::TABS, &mut sel, &names).changed() {
                        state.navigate(crate::vm::dialogs::AfterDiscard::Tab(sel));
                    }
                    ui.add_space(space::S4);
                    if state.details.tab == DetailsTab::Overview {
                        overview::show(ui, state, env, &g);
                    } else {
                        let w = ui.available_width().min(880.0);
                        ui.allocate_ui_with_layout(
                            vec2(w, 0.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.set_width(w);
                                match state.details.tab {
                                    DetailsTab::Protection => permissions::show(ui, state, env, &g),
                                    DetailsTab::Compatibility => runtime::show(ui, state, env, &g),
                                    DetailsTab::LaunchOptions => {
                                        launch_options::show(ui, state, env, &g)
                                    }
                                    DetailsTab::Logs => {
                                        crate::screens::debug::logs::tab(ui, state, env, id)
                                    }
                                    _ => activity::show(ui, state, env, &g),
                                }
                            },
                        );
                    }
                    ui.add_space(space::S8);
                });
        });
    // The overflow menu (same items as the card's).
    if let Some((mid, at)) = state.library.menu
        && mid == id
    {
        let mut open = true;
        let facts = super::library::menu::MenuFacts {
            has_shortcut: env.bridge.host().shortcuts.iter().any(|x| x.game == id),
            developer: env.bridge.prefs().advanced.developer_mode,
        };
        let chosen = super::library::menu::show(&env.ctx, &lib, id, at, &mut open, facts);
        if !open {
            state.library.menu = None;
        }
        if let Some(a) = chosen {
            actions::apply(state, env, id, a);
        }
    }
    let _ = (
        control::MD,
        Variant::Ghost,
        Availability::Present,
        Activity::Idle,
        range_for,
    );
}

fn save_bar(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let (errors, widens) = match state.details.tab {
        DetailsTab::Protection => state
            .details
            .protection
            .as_ref()
            .map_or((0, false), |e| (e.errors(), !e.widenings().is_empty())),
        _ => (0, false),
    };
    let launch_error = state
        .details
        .launch
        .as_ref()
        .is_some_and(|l| l.resolved().is_err() || launch_options::env_errors(l) > 0);
    ui.horizontal(|ui| {
        ui.label(icons::icon(icons::INFO, icon_size::INLINE).color(p.text_secondary));
        let sentence = if widens {
            s::UNSAVED_WIDENS
        } else {
            s::UNSAVED
        };
        ui.add(egui::Label::new(Type::Body.rich(sentence).color(p.text_primary)).wrap());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let reason = if errors > 0 {
                Some(s::fix_problems(errors))
            } else if launch_error {
                Some(s::FIX_LAUNCH.to_owned())
            } else {
                None
            };
            let save = ui.add(Button::primary(s::SAVE).disabled(reason.as_deref()));
            if save.clicked() {
                permissions::save(state, env);
            }
            if let Some(r) = &reason {
                ui.label(Type::Caption.rich(r).color(p.danger));
            }
            if ui.add(Button::new(s::DISCARD)).clicked() {
                state.details.discard();
            }
        });
    });
}

/// Skeleton lines while a query is slow.
pub fn skeleton_lines(ui: &mut Ui, n: usize) {
    for _ in 0..n {
        let (r, _) =
            ui.allocate_exact_size(vec2(ui.available_width().min(320.0), 14.0), Sense::hover());
        crate::widgets::skeleton::block(ui, r, Some(CornerRadius::same(radius::XS)));
        ui.add_space(space::S1);
    }
}
