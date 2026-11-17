//! The page of a game Steam runs (ADR-0031): the game page's two columns with what applies
//! to it. The left column holds Steam's cover (not a button: the artwork is Steam's) and the
//! play record with Steam's total, said to come from Steam, without the thirty bars
//! (EasyPlay records no sessions for it). The right column reads down: back, the title (not
//! renamable), the meta row that starts with "Run by Steam" — without the shield, which marks
//! a protection level — then the tags; Play (no Stop: the quiet Play while Steam runs it),
//! favourite, more; the notes (an update waiting, paused or running; files Steam reports
//! missing; Steam not found; a library drive not connected); then the tabs *Overview* and,
//! for a game Steam runs through Proton, *Windows storage*.
//!
//! The page asks only `SteamAction::Details` (once on arrival, again when the library
//! changes): never `LoadGame`, `AboutGame` or the statistics, which do not apply.

use easyplay_core::ids::GameId;
use easyplay_core::model::{
    Activity, Availability, GameSummary, SteamCompat, SteamFacts, SteamInstallKind, SteamUpdate,
};
use egui::{Align, CornerRadius, Id, Layout, Rect, ScrollArea, Sense, Ui, UiBuilder, pos2, vec2};

use super::{back_link, fine, record_lines, risen, tag_words, title, title_type};
use crate::app::UiState;
use crate::bridge::{FolderTarget, SteamAction, SteamDetail, UiCommand};
use crate::input::picker::PickPurpose;
use crate::screens::Env;
use crate::screens::actions::{self, GameAction};
use crate::strings;
use crate::strings::details as s;
use crate::strings::steam as st;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::tokens::{control, game as gm, layout, space, travel};
use crate::theme::{self, Dur, Type, icons, motion};
use crate::vm::AsyncValue;
use crate::vm::details::{DetailsTab, DetailsVm, SteamTab};
use crate::widgets::banner::{Note, Severity};
use crate::widgets::button::{Button, PillSize, Variant, play_pill, play_pill_blocked};
use crate::widgets::card::{PlayView, play_view, tick_elapsed};
use crate::widgets::cover::{self, CoverLook, CoverSource};
use crate::widgets::icon_button::IconButton;
use crate::widgets::rows::{self, Leads, SettingRow};
use crate::widgets::tabs::Tabs;
use crate::widgets::tooltip;
use crate::widgets::word::Word;

/// Ask for what the page shows beyond the summary.
fn ask(state: &mut UiState, env: &mut Env<'_>, id: GameId, rev: u64) {
    let q = env.bridge.query_id();
    state.details.steam_q = Some(q);
    state.details.steam_rev = rev;
    if !matches!(state.details.steam, AsyncValue::Ready(_)) {
        state.details.steam = AsyncValue::Loading { since: env.mono };
    }
    env.bridge
        .send(UiCommand::Steam(SteamAction::Details { id, reply_to: q }));
}

/// Whether Steam runs the game through Proton (it then has a Windows storage tab).
fn is_proton(f: &SteamFacts) -> bool {
    matches!(f.compat, SteamCompat::Proton { .. })
}

/// The tabs of the page, in order, with their names.
fn tabs(f: &SteamFacts) -> Vec<(SteamTab, &'static str)> {
    let mut out = vec![(SteamTab::Overview, s::tab(DetailsTab::Overview))];
    if is_proton(f) {
        out.push((SteamTab::Storage, st::page::STORAGE_TAB()));
    }
    out
}

/// `Ctrl+Tab` / `Ctrl+Shift+Tab` on the page of the Steam game `g`: the next or previous of
/// its tabs.
pub fn cycle_tab(state: &mut UiState, g: &GameSummary, forward: bool) {
    let Some(f) = g.steam() else {
        return;
    };
    let all = tabs(f);
    let n = all.len();
    let i = all
        .iter()
        .position(|t| t.0 == state.details.steam_tab)
        .unwrap_or(0);
    let j = if forward {
        (i + 1) % n
    } else {
        (i + n - 1) % n
    };
    state.details.steam_tab = all[j].0;
}

/// Show the page of the Steam game `g`.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let id = g.id;
    let rev = env.bridge.library().revision;
    if state.details.game != Some(id) {
        state.details = DetailsVm {
            game: Some(id),
            ..DetailsVm::default()
        };
        ask(state, env, id, rev);
    } else if state.details.steam_q.is_none() && state.details.steam_rev != rev {
        // The summary changed (it runs, it was updated, a note was saved): ask again.
        ask(state, env, id, rev);
    }
    // A Steam game's name is Steam's (F2 renames other games).
    state.details.rename = None;
    let Some(facts) = g.steam() else {
        return;
    };
    if !is_proton(facts) {
        state.details.steam_tab = SteamTab::Overview;
    }
    let page = ui.available_rect_before_wrap();
    let f = super::frame(page);
    if let Some(c) = f.cover {
        cover(ui, state, env, g, c);
        let at = Rect::from_min_max(
            pos2(c.left(), c.bottom() + gm::RECORD_TOP),
            pos2(c.right(), page.bottom()),
        );
        record(ui, env, g, at);
    }
    let [pad_l, pad_r, pad_b] = f.pad;
    let content_w = (f.column.width() - pad_l - pad_r).max(0.0);
    ui.ctx()
        .data_mut(|d| d.insert_temp(Id::unique("details.content_w"), content_w));
    let mut col = ui.new_child(
        UiBuilder::new()
            .scope_id(Id::unique(("details.column", id)))
            .max_rect(f.column)
            .layout(Layout::top_down(Align::Min)),
    );
    ScrollArea::vertical()
        .id_salt(("details", id))
        .auto_shrink([false, false])
        .show(&mut col, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: pad_l as i8,
                    right: pad_r as i8,
                    top: 0,
                    bottom: pad_b as i8,
                })
                .show(ui, |ui| {
                    column_body(ui, state, env, g, facts, &f, content_w)
                });
        });
    ui.advance_cursor_after_rect(page);
    // The game menu ("More"): the Steam game's items.
    if let Some((mid, at)) = state.library.menu
        && mid == id
    {
        let lib = env.bridge.library().clone();
        let mut open = true;
        let facts = crate::screens::library::menu::MenuFacts {
            has_shortcut: false,
            developer: env.bridge.prefs().advanced.developer_mode,
        };
        let chosen = crate::screens::library::menu::show(&env.ctx, &lib, id, at, &mut open, facts);
        if !open {
            state.library.menu = None;
        }
        if let Some(a) = chosen {
            actions::apply(state, env, id, a);
        }
    }
}

// ---- The cover column ----------------------------------------------------------------------

/// Steam's cover, as the page draws every cover; not a button (the artwork is Steam's).
fn cover(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary, rect: Rect) {
    let theme = theme::theme(ui.ctx());
    state.flight.note_page(g.id, rect);
    if state.flight.hides(g.id) {
        return;
    }
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
    cover::paint_shadow(ui.painter(), rect, theme.palette.shade_cover, 1.0);
    cover::paint(
        ui.painter(),
        rect,
        src,
        &g.name,
        // EasyPlay's mark (a shield) is not put on a game Steam runs.
        CoverLook {
            mark: false,
            ..CoverLook::default()
        },
        &theme,
    );
}

/// What the play record says.
enum Said {
    /// Play handed it to Steam, which is starting it.
    Starting,
    /// Steam runs it: how long EasyPlay has seen it run.
    Playing(String),
    /// No time and no date from Steam.
    Never,
    /// Steam's total, and when last (Steam's date).
    Played { time: String, last: Option<String> },
}

fn said(env: &Env<'_>, g: &GameSummary) -> Said {
    match g.activity {
        Activity::Launching(_) => Said::Starting,
        Activity::Running { since } => {
            let ms = env.now.since(since).map_or(0, |d| d.as_millis() as u64);
            Said::Playing(strings::fmt::session_clock(ms))
        }
        _ if g.never_played() => Said::Never,
        _ => Said::Played {
            time: strings::fmt::play_time(g.total_playtime_ms),
            last: g
                .last_played_at
                .map(|t| strings::fmt::last_played(Some(t), env.now, env.utc_offset_s)),
        },
    }
}

/// The play record under the cover: Steam's total and when last, then a quiet "from Steam"
/// where an EasyPlay game shows its thirty days.
fn record(ui: &mut Ui, env: &Env<'_>, g: &GameSummary, at: Rect) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let painter = ui.painter().clone();
    tick_elapsed(ui.ctx(), g.activity, env.now);
    let live = Id::unique(("details.record.live", g.id));
    match said(env, g) {
        Said::Starting => {
            let bottom = record_lines(
                ui,
                &painter,
                at,
                at.top(),
                (s::STARTING(), p.accent_text),
                st::STARTING_IN_STEAM(),
            );
            let r = Rect::from_min_max(at.min, pos2(at.right(), bottom));
            let resp = ui.interact(r, live, Sense::hover());
            crate::a11y::labelled(
                ui.ctx(),
                resp.id,
                egui::Role::Status,
                st::STARTING_SENTENCE(),
            );
        }
        Said::Playing(clock) => {
            let bottom = record_lines(
                ui,
                &painter,
                at,
                at.top(),
                (&clock, p.accent_text),
                s::PLAYING_NOW(),
            );
            let r = Rect::from_min_max(at.min, pos2(at.right(), bottom));
            let resp = ui.interact(r, live, Sense::hover());
            crate::a11y::item(&resp, egui::Role::Label, &s::playing_sentence(&clock));
        }
        Said::Never => {
            let g1 = Type::Body.galley(ui, s::NOT_PLAYED_YET(), p.t2, at.width(), usize::MAX);
            let r = Rect::from_min_size(at.min, g1.size());
            painter.galley(r.min, g1, p.t2);
            let resp = ui.interact(
                r,
                Id::unique(("details.record.never", g.id)),
                Sense::hover(),
            );
            crate::a11y::item(&resp, egui::Role::Label, s::NOT_PLAYED_YET());
        }
        Said::Played { time, last } => {
            let line = last
                .as_deref()
                .map_or_else(|| st::PLAYED().to_owned(), s::record_line);
            let bottom = record_lines(ui, &painter, at, at.top(), (&time, p.t1), &line);
            let foot = bottom + gm::RECORD_FOOT_TOP;
            let l = Type::Micro.line(ui, st::FROM_STEAM(), p.t3);
            painter.galley(pos2(at.left(), foot), l, p.t3);
            let r = Rect::from_min_max(at.min, pos2(at.right(), foot + Type::Micro.line_height()));
            let resp = ui.interact(
                r,
                Id::unique(("details.record.steam", g.id)),
                Sense::hover(),
            );
            crate::a11y::item(
                &resp,
                egui::Role::Label,
                &st::played_sentence(&time, last.as_deref()),
            );
            crate::a11y::describe(&resp, st::RECORD_FROM_STEAM());
        }
    }
}

/// The record as one sentence, in a page too narrow for the cover column.
fn record_sentence(ui: &mut Ui, env: &Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    tick_elapsed(ui.ctx(), g.activity, env.now);
    let (text, c) = match said(env, g) {
        Said::Playing(clock) => (s::playing_sentence(&clock), p.accent_text),
        Said::Starting => (st::STARTING_SENTENCE().to_owned(), p.accent_text),
        Said::Never => (s::NOT_PLAYED_SENTENCE().to_owned(), p.t2),
        Said::Played { time, last } => (st::played_sentence(&time, last.as_deref()), p.t2),
    };
    ui.add_space(gm::SENTENCE_TOP);
    Type::Lead.paragraph(ui, &text, c);
}

// ---- The right column ----------------------------------------------------------------------

/// Under the title: "Run by Steam" (no shield: the shield marks a protection level, and this
/// game has none), then the tags and "+ Tag".
fn meta_row(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let top = ui.cursor().min;
    let w = ui.available_width() + layout::WORD_PAD;
    let mut row = ui.new_child(
        UiBuilder::new()
            .id_salt("details.meta")
            .max_rect(Rect::from_min_size(
                pos2(top.x - layout::WORD_PAD, top.y),
                vec2(w, control::SM),
            ))
            .layout(Layout::left_to_right(Align::Center).with_main_wrap(true)),
    );
    row.spacing_mut().item_spacing = vec2(layout::WORD_SPACING, layout::WORD_SPACING);
    // It explains itself; pressed, it shows the Overview, where *How it runs* says it.
    if row
        .add(
            Word::new(st::RUN_BY_STEAM())
                .strong()
                .tip(st::RUN_BY_STEAM_DETAIL(), None),
        )
        .clicked()
    {
        state.details.steam_tab = SteamTab::Overview;
    }
    tag_words(&mut row, state, env, g);
    let used = row.min_rect();
    ui.allocate_rect(
        Rect::from_min_max(top, pos2(top.x + ui.available_width(), used.bottom())),
        Sense::hover(),
    );
}

/// Play (handed to Steam; the quiet Play while Steam runs it), favourite, more.
fn actions_row(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gm::ACTIONS_GAP;
        let (pill, plays) = match play_view(g) {
            PlayView::State(st) => (play_pill(ui, &st, &g.name, PillSize::Page, 1.0), true),
            PlayView::Blocked(why) => (
                play_pill_blocked(ui, &g.name, why, PillSize::Page, 1.0),
                false,
            ),
        };
        if !state.details.play_focused {
            // The page's first focus: Play.
            state.details.play_focused = true;
            pill.request_focus();
        }
        if plays && pill.clicked() {
            actions::play(state, env, g);
        }
        let tip = if g.favorite {
            s::UNFAVOURITE
        } else {
            s::FAVOURITE
        };
        let fav = ui.add(
            IconButton::new(icons::STAR, tip())
                .round(true)
                .toggle(g.favorite)
                .selected(g.favorite)
                .shortcut("F"),
        );
        if fav.clicked() {
            actions::apply(state, env, g.id, GameAction::Favorite);
        }
        let more = ui.add(IconButton::new(icons::DOTS, s::MORE()).round(true));
        if more.clicked() {
            state.library.menu = Some((g.id, more.rect.left_bottom()));
        }
    });
}

/// A note above the tabs, 20 below what precedes it; true when its action was pressed.
fn note(ui: &mut Ui, n: Note<'_>) -> bool {
    ui.add_space(gm::NOTE_TOP);
    n.show(ui).clicked == Some(0)
}

/// The notes above the tabs, in order: the update, files Steam reports missing, Steam not
/// found, the game's folder out of reach.
fn notes(
    ui: &mut Ui,
    env: &mut Env<'_>,
    g: &GameSummary,
    facts: &SteamFacts,
    detail: Option<&SteamDetail>,
) {
    let id = g.id;
    let update: &[&str] = &[st::action::UPDATE_IN_STEAM()];
    let bytes = strings::fmt::bytes;
    let progress = facts
        .download
        .map(|d| (bytes(d.done), bytes(d.total), d.done));
    let text = match (facts.update, &progress) {
        (SteamUpdate::UpToDate, _) => None,
        (SteamUpdate::Updating, Some((done, total, _))) => {
            Some((st::note::UPDATING_TITLE(), st::note::progress(done, total)))
        }
        (SteamUpdate::Updating, None) => Some((
            st::note::UPDATING_TITLE(),
            st::note::UPDATING_TEXT().to_owned(),
        )),
        (SteamUpdate::Waiting, Some((_, total, 0))) => Some((
            st::note::WAITING_TITLE(),
            st::note::waiting_to_download(total),
        )),
        (SteamUpdate::Waiting, Some((done, total, _))) => Some((
            st::note::WAITING_TITLE(),
            st::note::waiting_progress(done, total),
        )),
        (SteamUpdate::Waiting, None) => Some((
            st::note::WAITING_TITLE(),
            st::note::WAITING_TEXT().to_owned(),
        )),
        (SteamUpdate::Paused, Some((done, total, d))) if *d > 0 => Some((
            st::note::PAUSED_TITLE(),
            st::note::paused_progress(done, total),
        )),
        (SteamUpdate::Paused, _) => {
            Some((st::note::PAUSED_TITLE(), st::note::PAUSED_TEXT().to_owned()))
        }
    };
    if let Some((title, text)) = text
        && note(
            ui,
            Note::new(Severity::Info, &text)
                .title(title)
                .actions(update),
        )
    {
        env.bridge.send(UiCommand::Steam(SteamAction::Update(id)));
    }
    if facts.files_missing
        && note(
            ui,
            Note::missing(st::note::FILES_TITLE(), st::note::FILES_TEXT())
                .actions(&[st::action::OPEN_IN_STEAM()]),
        )
    {
        env.bridge
            .send(UiCommand::Steam(SteamAction::OpenInSteam(id)));
    }
    if detail.is_some_and(|d| !d.client_found) {
        let _ = note(
            ui,
            Note::missing(st::note::NO_STEAM_TITLE(), st::note::NO_STEAM_TEXT()),
        );
    }
    let away = match g.availability {
        Availability::DriveDisconnected => Some((st::note::DRIVE_TITLE(), st::note::DRIVE_TEXT())),
        Availability::Missing | Availability::Unreadable => {
            Some((st::note::FOLDER_TITLE(), st::note::FOLDER_TEXT()))
        }
        Availability::Present | Availability::Unknown => None,
    };
    if let Some((title, text)) = away
        && note(ui, Note::missing(title, text).actions(&[s::CHECK_AGAIN()]))
    {
        // Checking for missing games looks at Steam's library folders again too.
        env.bridge.send(UiCommand::RescanMissing);
    }
}

/// The scrolling column: back link, title block, actions, notes, tabs and the tab's body.
fn column_body(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    g: &GameSummary,
    facts: &SteamFacts,
    f: &super::Frame,
    content_w: f32,
) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let rise = |k: usize| {
        motion::arrive(
            ui.ctx(),
            Id::unique(("details.rise", g.id, k)),
            theme.motion,
            travel::PAGE_RISE_AT[k],
            Dur::Page,
            travel::PAGE_RISE,
        )
    };
    let (r0, r1, r2) = (rise(0), rise(1), rise(2));
    let w = ui.available_width().min(layout::GAME_COL_MAX);
    ui.set_width(w);
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    back_link(ui, state);
    risen(ui, r0, |ui| {
        ui.add_space(gm::TITLE_TOP);
        title(ui, state, env, g, title_type(f));
        ui.add_space(gm::META_TOP);
        meta_row(ui, state, env, g);
        if f.narrow {
            record_sentence(ui, env, g);
        }
    });
    ui.add_space(gm::ACTIONS_TOP);
    let actions_bottom = risen(ui, r1, |ui| {
        actions_row(ui, state, env, g);
        ui.min_rect().bottom()
    });
    // The compact Play joins the tab bar once the actions have scrolled away.
    let stuck = actions_bottom < ui.clip_rect().top() + gm::PIN_SLACK;
    let compact = ui.ctx().animate_bool_with_time_and_easing(
        Id::unique(("details.compact_play", g.id)),
        stuck,
        theme.motion.fade(Dur::Fast),
        egui::emath::easing::linear,
    );
    let gap = if content_w < layout::TABS_TIGHT_BELOW {
        layout::TAB_GAP[1]
    } else {
        layout::TAB_GAP[0]
    };
    let tabs = tabs(facts);
    let detail = state.details.steam.ready().cloned();
    let mut picked = None;
    let (slot, pinned) = risen(ui, r2, |ui| {
        notes(ui, env, g, facts, detail.as_deref());
        ui.add_space(gm::TABS_TOP);
        let (slot, _) = ui.allocate_exact_size(vec2(w, layout::TAB_H), Sense::hover());
        let pinned = slot.top() < ui.clip_rect().top();
        if !pinned {
            let mut bar_ui = ui.new_child(
                UiBuilder::new()
                    .id_salt("details.tabs")
                    .max_rect(slot)
                    .layout(Layout::top_down(Align::Min)),
            );
            picked = Some(tab_bar(&mut bar_ui, state, g, &tabs, gap, compact));
        }
        ui.add_space(gm::PANEL_TOP);
        let body_in = motion::since_change(
            ui.ctx(),
            Id::unique(("details.body", g.id)),
            state.details.steam_tab,
            theme
                .motion
                .fade(Dur::Base)
                .max(theme.motion.movement(Dur::Base)),
            false,
        )
        .map_or((1.0, 0.0), |t| {
            let fade = theme.motion.fade(Dur::Base);
            let mv = theme.motion.movement(Dur::Base);
            let a = if fade > 0.0 { (t / fade).min(1.0) } else { 1.0 };
            let o = if mv > 0.0 {
                travel::RISE * (1.0 - motion::ease_out(t / mv))
            } else {
                0.0
            };
            (a, o)
        });
        risen(ui, body_in, |ui| match state.details.steam_tab {
            SteamTab::Overview => overview(ui, state, env, g, facts, detail.as_deref()),
            SteamTab::Storage => storage(ui, state, env, g, facts, detail.as_deref()),
        });
        (slot, pinned)
    });
    if pinned {
        let bar = Rect::from_min_size(
            pos2(slot.left(), ui.clip_rect().top()),
            vec2(w, layout::TAB_H),
        );
        let mut bar_ui = ui.new_child(
            UiBuilder::new()
                .id_salt("details.tabs")
                .max_rect(bar)
                .layout(Layout::top_down(Align::Min)),
        );
        bar_ui.painter().rect_filled(bar, CornerRadius::ZERO, p.bg);
        // Clicks on the bar's empty part must not reach the body under it.
        let _ = bar_ui.interact(bar, Id::unique("details.tabs.plate"), Sense::click());
        picked = Some(risen(&mut bar_ui, (r2.0, 0.0), |ui| {
            tab_bar(ui, state, g, &tabs, gap, compact)
        }));
    }
    if let Some((sel, pressed)) = picked {
        if pressed {
            actions::play(state, env, g);
        }
        if let Some(sel) = sel {
            state.details.steam_tab = sel;
        }
    }
}

/// The tab bar, with the compact Play at its right end while `compact` (its opacity) is
/// above 0. Returns the tab chosen, and whether Play was pressed.
fn tab_bar(
    ui: &mut Ui,
    state: &UiState,
    g: &GameSummary,
    tabs: &[(SteamTab, &'static str)],
    gap: f32,
    compact: f32,
) -> (Option<SteamTab>, bool) {
    let names: Vec<&str> = tabs.iter().map(|t| t.1).collect();
    let mut sel = tabs
        .iter()
        .position(|t| t.0 == state.details.steam_tab)
        .unwrap_or(0);
    let view = play_view(g);
    let (list, pressed) = Tabs::new(s::TABS(), &names)
        .gap(gap)
        .show_with(ui, &mut sel, |ui| {
            compact > 0.0
                && match &view {
                    PlayView::State(st) => {
                        play_pill(ui, st, &g.name, PillSize::Compact, compact).clicked()
                    }
                    PlayView::Blocked(why) => {
                        let _ = play_pill_blocked(ui, &g.name, why, PillSize::Compact, compact);
                        false
                    }
                }
        });
    (
        list.changed().then(|| tabs.get(sel).map(|t| t.0)).flatten(),
        pressed,
    )
}

// ---- Overview ------------------------------------------------------------------------------

/// The Overview: *How it runs*, *Where it lives*, *Notes*, *What it is*.
fn overview(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    g: &GameSummary,
    facts: &SteamFacts,
    detail: Option<&SteamDetail>,
) {
    let p = theme::theme(ui.ctx()).palette.clone();
    // *How it runs*: run by Steam, said plainly; then Proton.
    rows::heading(ui, s::RUNS(), true);
    if matches!(state.details.steam, AsyncValue::Failed(_)) {
        let mut retry = false;
        ui.horizontal(|ui| {
            ui.label(Type::Body.rich(s::COULDNT_LOAD()).color(p.t2));
            retry = ui.add(Button::ghost(s::RETRY())).clicked();
        });
        if retry {
            let rev = env.bridge.library().revision;
            ask(state, env, g.id, rev);
        }
    }
    rows::fact(ui, s::PROTECTION(), |ui| {
        let r = ui.add(egui::Label::new(
            Type::BodyStrong.rich(st::RUN_BY_STEAM()).color(p.t1),
        ));
        crate::a11y::item(
            &r,
            egui::Role::Label,
            &s::fact_name(s::PROTECTION(), st::RUN_BY_STEAM()),
        );
        ui.add_space(gm::REACH_TOP);
        Type::Body.paragraph(ui, st::RUN_BY_STEAM_DETAIL(), p.t3);
    });
    match &facts.compat {
        SteamCompat::Proton { tool, version, .. } => {
            let choice = tool.as_deref().map_or_else(
                || st::page::PROTON_DEFAULT().to_owned(),
                st::page::proton_chosen,
            );
            let last = version.as_deref().map(st::page::last_run_with);
            let spoken = s::row_name(st::page::PROTON(), &choice, last.as_deref());
            let door = rows::door(ui, st::page::PROTON(), &spoken, Leads::Outside, |ui| {
                Type::Body.paragraph(ui, &choice, p.t1);
                if let Some(l) = &last {
                    Type::Body.paragraph(ui, l, p.t3);
                }
            });
            if tooltip::tooltip(door, st::action::OPEN_IN_STEAM_HELP(), None).clicked() {
                env.bridge
                    .send(UiCommand::Steam(SteamAction::OpenInSteam(g.id)));
            }
        }
        SteamCompat::NotProton => {
            let value = if g.never_played() {
                st::page::NOT_RUN_YET()
            } else {
                st::page::NATIVE()
            };
            rows::fact_text(ui, st::page::PROTON(), value);
        }
    }

    // *Where it lives*: the install folder, and the Windows storage of a Proton game.
    rows::heading(ui, s::LIVES(), false);
    let (folder, parent) = s::location(g.root.as_deref(), &g.primary_exe_path);
    let spoken = s::row_name(s::GAME_FOLDER(), &folder, parent.as_deref());
    let door = rows::door(ui, s::GAME_FOLDER(), &spoken, Leads::Outside, |ui| {
        // As the Overview of every game says it: the folder, then where it is, quiet.
        let after = parent.as_ref().map(|x| format!(" {x}"));
        let mut parts = vec![(folder.as_str(), Type::Body, p.t1)];
        if let Some(a) = &after {
            parts.push((a.as_str(), Type::Body, p.t3));
        }
        super::overview::runs(ui, &parts);
    });
    if tooltip::tooltip(door, s::OPEN_IN_FILE_MANAGER(), None).clicked() {
        env.bridge
            .send(UiCommand::OpenFolder(FolderTarget::GameFolder(g.id)));
    }
    if facts.prefix().is_some() {
        let spoken = s::row_name(st::page::STORAGE(), st::page::STORAGE_IN_STEAM(), None);
        let door = rows::door(ui, st::page::STORAGE(), &spoken, Leads::Outside, |ui| {
            Type::Body.paragraph(ui, st::page::STORAGE_IN_STEAM(), p.t1);
        });
        if tooltip::tooltip(door, st::page::OPEN_STORAGE_HELP(), None).clicked() {
            env.bridge
                .send(UiCommand::Steam(SteamAction::OpenStorageFolder(g.id)));
        }
    }

    // *Notes*: kept by EasyPlay; they arrive with the page's detail.
    super::overview::notes_field(ui, state, env, g.id, detail.is_some());

    // *What it is*: what Steam's files say; a fact with nothing to say is left out.
    rows::heading(ui, s::WHAT_IT_IS(), false);
    let fact = |ui: &mut Ui, label: &str, value: String| {
        rows::fact(ui, label, |ui| {
            let r = Type::Body.paragraph(ui, &value, p.t1);
            crate::a11y::item(&r, egui::Role::Label, &s::fact_name(label, &value));
        });
    };
    if facts.size_on_disk > 0 {
        fact(
            ui,
            st::page::SIZE(),
            strings::fmt::bytes(facts.size_on_disk),
        );
    }
    if let Some(b) = facts.build_id {
        fact(ui, st::page::BUILD(), b.to_string());
    }
    if let Some(t) = facts.last_updated {
        fact(
            ui,
            st::page::UPDATED(),
            s::date_label(t, env.now, env.utc_offset_s),
        );
    }
    fact(ui, st::page::APP_ID(), facts.app_id.to_string());
    let with = match facts.install {
        SteamInstallKind::Native => st::page::STEAM_NATIVE(),
        SteamInstallKind::Flatpak => st::page::STEAM_FLATPAK(),
        SteamInstallKind::Snap => st::page::STEAM_SNAP(),
    };
    fact(ui, st::page::INSTALLED_WITH(), with.to_owned());
    fact(
        ui,
        s::ADDED(),
        s::date_label(g.added_at, env.now, env.utc_offset_s),
    );
}

// ---- Windows storage -----------------------------------------------------------------------

/// The Windows storage of a game Steam runs through Proton: what the two programs do, said
/// once, then three rows: open the folder, Windows settings, run another program. The two
/// programs run only while the game is closed and its storage and compatibility layer are
/// there; a row that cannot act says why under its label.
fn storage(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    g: &GameSummary,
    facts: &SteamFacts,
    detail: Option<&SteamDetail>,
) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let id = g.id;
    fine(ui, st::action::STORAGE_HELP(), p.t3);
    ui.add_space(space::S2);
    let has_storage = facts.prefix().is_some();
    let running = detail.is_some_and(|d| d.running) || crate::widgets::card::steam_runs(g);
    let tools = detail.is_some_and(|d| d.prefix_tools) && !running;
    // Why the two programs cannot run now, when they cannot.
    let why = if !has_storage {
        st::action::NO_STORAGE()
    } else if running {
        st::action::STORAGE_BUSY()
    } else {
        st::page::NO_PROTON_BUILD()
    };
    let known = detail.is_some();
    let help = |own: &'static str| if tools || !known { own } else { why };
    let button = |ui: &mut Ui, label: &str| {
        crate::screens::settings::band(ui, control::BUTTON, |ui| {
            ui.add(Button::new(label).variant(Variant::Secondary))
                .clicked()
        })
    };
    let open = SettingRow::new(st::page::OPEN_FOLDER())
        .help(if has_storage {
            st::page::OPEN_FOLDER_HELP()
        } else {
            st::action::NO_STORAGE()
        })
        .enabled(has_storage)
        .show(ui, |ui| button(ui, st::page::OPEN()));
    if open {
        env.bridge
            .send(UiCommand::Steam(SteamAction::OpenStorageFolder(id)));
    }
    let settings = SettingRow::new(st::action::WINDOWS_SETTINGS())
        .help(help(st::page::WINDOWS_SETTINGS_HELP()))
        .enabled(tools)
        .show(ui, |ui| button(ui, st::page::OPEN()));
    if settings {
        env.bridge
            .send(UiCommand::Steam(SteamAction::WindowsSettings(id)));
    }
    let run = SettingRow::new(st::page::RUN_PROGRAM())
        .help(help(st::page::RUN_PROGRAM_HELP()))
        .enabled(tools)
        .show(ui, |ui| button(ui, st::page::CHOOSE()));
    if run {
        // The picked program runs in this game's Windows storage (`app.rs`).
        state.picks.push(PickPurpose::RunProgram(id));
    }
}
