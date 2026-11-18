//! Quick-launch layout (SCREEN_SPECS §7, UI-026, UF-3): the compact window shown when a known
//! game is opened from the file manager. Name and phase from the first frame; closes itself
//! 600 ms after the game runs; closing the window never cancels the launch, *Cancel* does.
//! *Open Zaun* grows the same window into the full layout. An unknown program opens the
//! setup sheet, and the window grows to fit it.
//!
//! The design does not draw it; it is set with the design's parts: the game's poster (or its
//! artwork) at the left, its name in the dialog title's face, what is happening in secondary
//! text (coral for a failure) over the progress bar, and the actions at the bottom right as a
//! dialog's are. No box.

use std::path::Path;
use std::time::Duration;

use egui::{
    Align, Context, Key, Layout, Modifiers, Rect, Sense, Ui, UiBuilder, ViewportCommand, pos2, vec2,
};
use zaun_core::library::LibrarySnapshot;
use zaun_core::model::GameSummary;

use super::Env;
use crate::app::UiState;
use crate::bridge::{Command, UiCommand};
use crate::route::{Overlay, Route};
use crate::strings;
use crate::strings::setup as s;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::tokens::{control, debug as dbg, layout, overlay};
use crate::theme::{self, Type};
use crate::vm::dialogs::Dialog;
use crate::vm::launch::present;
use crate::widgets::button::Button;
use crate::widgets::cover::{self, CoverLook, CoverSource};
use crate::widgets::progress;

/// The compact window size.
pub const SIZE: [f32; 2] = [420.0, 150.0];
/// "Already running": the window closes this long after saying so.
pub const ALREADY_CLOSE: Duration = Duration::from_secs(4);
/// The size while the setup sheet is shown.
pub const SETUP_SIZE: [f32; 2] = [720.0, 620.0];

/// The size to open the window at for `path`: compact for a game in `library`, room for the
/// setup sheet otherwise. A compositor that marks windows tiled (niri with `prefer-no-csd`)
/// ignores a later resize, so the first size has to be right.
pub fn window_size(library: &LibrarySnapshot, path: &Path) -> [f32; 2] {
    if library.games.iter().any(|g| *g.primary_exe_path == *path) {
        SIZE
    } else {
        SETUP_SIZE
    }
}

/// Largest window the compact window may grow to; effectively unbounded.
const MAX_SIZE: [f32; 2] = [16384.0, 16384.0];

/// Lift the compact window's fixed size. Its minimum and maximum are both [`SIZE`], and on
/// Wayland `Resizable(true)` alone leaves that maximum in place, so niri keeps the window
/// at 420 × 150; the maximum is raised explicitly.
fn unfix(env: &Env<'_>) {
    env.ctx.send_viewport_cmd(ViewportCommand::Resizable(true));
    env.ctx
        .send_viewport_cmd(ViewportCommand::MaxInnerSize(MAX_SIZE.into()));
    env.ctx
        .send_viewport_cmd(ViewportCommand::MinInnerSize(layout::MIN_WINDOW.into()));
}

/// Switch to the full layout (same window, saved geometry).
pub fn expand(state: &mut UiState, env: &Env<'_>) {
    if let Some(q) = &mut state.quick {
        q.expanded = true;
    }
    unfix(env);
    env.ctx
        .send_viewport_cmd(ViewportCommand::InnerSize(layout::DEFAULT_WINDOW.into()));
    state.nav.go_to(Route::Library);
}

/// Show the compact layout.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let Some(q) = state.quick.clone() else { return };
    // The setup sheet for an unknown program needs room; grow once, not every frame.
    if state.nav.top_overlay() == Some(Overlay::Setup)
        && !q.grown
        && ui.ctx().content_rect().width() < SETUP_SIZE[0]
    {
        if let Some(q) = &mut state.quick {
            q.grown = true;
        }
        unfix(env);
        env.ctx
            .send_viewport_cmd(ViewportCommand::InnerSize(SETUP_SIZE.into()));
    }
    let lib = env.bridge.library().clone();
    let game = q.game.and_then(|g| lib.get(g).cloned());
    let name = game.as_ref().map_or_else(
        || {
            q.path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        },
        |g| g.name.to_string(),
    );
    let launch = q.game.and_then(|g| state.launches.games.get(&g).cloned());
    let failure = q
        .game
        .and_then(|g| state.launches.failures.get(&g).cloned());
    // Auto-close 600 ms after the game runs.
    if let Some(t) = q.running_at {
        let since = env.mono.saturating_duration_since(t);
        let due = Duration::from_millis(600);
        if since >= due {
            env.ctx.send_viewport_cmd(ViewportCommand::Close);
        } else {
            ui.ctx().request_repaint_after(due - since);
        }
    }
    // Already running: say so, then close after 4 s (SCREEN_SPECS §7).
    if let Some(t) = q.already_at {
        let since = env.mono.saturating_duration_since(t);
        if since >= ALREADY_CLOSE {
            env.ctx.send_viewport_cmd(ViewportCommand::Close);
        } else {
            ui.ctx().request_repaint_after(ALREADY_CLOSE - since);
        }
    }
    let already = q.already_at.is_some();
    let failure = if already { None } else { failure };
    let cancellable = launch
        .as_ref()
        .is_some_and(|l| l.state.kind().cancellable());
    // Cancelled or ended: nothing is in progress any more.
    let over = launch
        .as_ref()
        .is_some_and(|l| l.state.kind().is_terminal());
    if state.nav.overlays().is_empty()
        && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
    {
        if cancellable && let Some(g) = q.game {
            env.bridge.send(Command::CancelLaunch { id: g });
        } else {
            env.ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
    let phase = match (&failure, &launch) {
        _ if already => strings::launch::already_running(&name),
        (Some(e), _) => e.title.clone(),
        (None, Some(l)) => {
            present(
                &l.state,
                env.mono.saturating_duration_since(l.since).as_secs_f32(),
            )
            .phase
        }
        (None, None) => strings::launch::STARTING().to_owned(),
    };
    let busy = failure.is_none() && q.running_at.is_none() && !already && !over;
    let page = ui.max_rect().shrink(dbg::QUICK_PAD);
    // The poster (or the game's artwork) at the left, 2:3.
    let cover = Rect::from_min_size(page.min, vec2(dbg::QUICK_COVER_W, dbg::QUICK_COVER_W * 1.5));
    let source = game.as_ref().map_or(CoverSource::Placeholder, |g| {
        cover_source(ui.ctx(), env, g, cover)
    });
    cover::paint(
        ui.painter(),
        cover,
        source,
        &name,
        CoverLook::default(),
        &theme,
    );
    // Beside it: the name in the dialog's title face, what is happening, and the bar.
    let text_x = cover.right() + dbg::QUICK_COVER_GAP;
    let text_w = (page.right() - text_x).max(1.0);
    let mut col = ui.new_child(
        UiBuilder::new()
            .id_salt("quick.text")
            .max_rect(Rect::from_min_size(
                pos2(text_x, page.top()),
                vec2(text_w, cover.height()),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    col.spacing_mut().item_spacing.y = dbg::QUICK_LINE_GAP;
    let title = Type::Title.galley(&col, &name, p.t1, text_w, 1);
    let (r, title_resp) = col.allocate_exact_size(title.size(), Sense::hover());
    col.painter().galley(r.min, title, p.t1);
    crate::a11y::labelled(col.ctx(), title_resp.id, egui::Role::Heading, &name);
    let ph = Type::Body.paragraph(
        &mut col,
        &phase,
        if failure.is_some() { p.coral } else { p.t2 },
    );
    crate::a11y::labelled(col.ctx(), ph.id, egui::Role::Status, &phase);
    if busy {
        progress::linear(&mut col, None, strings::launch::PROGRESS());
    }
    // The actions at the bottom right: the main one last, as in a dialog.
    let actions = Rect::from_min_max(pos2(page.left(), page.bottom() - control::BUTTON), page.max);
    let mut row = ui.new_child(
        UiBuilder::new()
            .id_salt("quick.actions")
            .max_rect(actions)
            .layout(Layout::right_to_left(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = overlay::DIALOG_ACTION_GAP;
    if let Some(e) = &failure {
        if row
            .add(Button::primary(strings::launch::TRY_AGAIN()))
            .clicked()
            && let Some(g) = q.game
        {
            state.launches.dismiss(g);
            env.bridge.send(UiCommand::Launch { id: g, exe: None });
        }
        if row.add(Button::new(s::DETAILS())).clicked() {
            state.show_dialog(Dialog::Error(e.clone()));
        }
    } else if cancellable
        && !already
        && row.add(Button::new(strings::launch::CANCEL())).clicked()
        && let Some(g) = q.game
    {
        env.bridge.send(Command::CancelLaunch { id: g });
    }
    if row.add(Button::ghost(s::OPEN_ZAUN())).clicked() {
        expand(state, env);
    }
}

/// The game's artwork for the window's cover, when it has some and it is loaded.
fn cover_source(ctx: &Context, env: &mut Env<'_>, game: &GameSummary, rect: Rect) -> CoverSource {
    let Some(art) = game.cover else {
        return CoverSource::Placeholder;
    };
    let key = TexKey {
        art,
        size: ThumbSize::cover_for(rect.width() * ctx.pixels_per_point()),
    };
    match env.textures.get(key, true, env.art) {
        TexState::Ready(t) => CoverSource::Texture(t, 1.0),
        TexState::Pending(c) => c.map_or(CoverSource::Placeholder, CoverSource::Pending),
    }
}
