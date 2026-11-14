//! The application shell (SCREEN_SPECS §0): sidebar, content frame, banner slot, toast area,
//! overlay layers, drop zone, window title per route, global shortcuts.
//!
//! `Panel::left("sidebar")` then `CentralPanel` last [F-305]; overlays are `egui::Modal`
//! [F-306]; toasts an `Area` anchored bottom-centre; the drop zone a foreground `Area`
//! covering the window [F-321].

use egui::{CentralPanel, Frame, Id, Key, Modifiers, Panel, Ui, ViewportCommand};

use super::{
    Env, actions, add_game, details, dialogs, gallery, library, onboarding, quick_launch, settings,
    setup, statistics,
};
use crate::app::UiState;
use crate::bridge::{HostStatus, OpenOrigin, UiCommand};
use crate::input::dnd::{self, DropAction, Payload};
use crate::input::picker::PickPurpose;
use crate::input::shortcuts::{self, Action, DispatchCtx, Here};
use crate::route::{Overlay, Route, SettingsTab};
use crate::strings;
use crate::theme::tokens::{layout, space};
use crate::theme::{self, Dur, Type, icons};
use crate::vm::dialogs::AfterDiscard;
use crate::vm::toasts::ToastKind;
use crate::widgets::banner::{Severity, banner};
use crate::widgets::toast;

/// A persistent condition shown at the top of the content area (USER_EXPERIENCE §6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    /// Stable key (dismissal is remembered per key).
    pub key: &'static str,
    /// Severity; higher wins the slot.
    pub severity: Severity,
    /// Sentence.
    pub text: String,
    /// One action label.
    pub action: Option<String>,
    /// Whether it can be dismissed.
    pub dismissible: bool,
}

/// One banner at a time, by priority.
#[derive(Clone, Debug, Default)]
pub struct BannerQueue {
    items: Vec<Banner>,
    dismissed: Vec<&'static str>,
}

impl BannerQueue {
    /// Add or replace a banner by key.
    pub fn set(&mut self, b: Banner) {
        self.items.retain(|x| x.key != b.key);
        self.items.push(b);
    }

    /// Remove a banner by key.
    pub fn clear(&mut self, key: &str) {
        self.items.retain(|x| x.key != key);
    }

    /// Dismiss by key (until the app restarts).
    pub fn dismiss(&mut self, key: &'static str) {
        self.dismissed.push(key);
    }

    /// Whether the banner `key` was dismissed.
    pub fn is_dismissed(&self, key: &str) -> bool {
        self.dismissed.contains(&key)
    }

    /// The banner that owns the slot: highest severity, then most recent.
    pub fn top(&self) -> Option<&Banner> {
        // `max_by_key` keeps the last maximum: the most recent of equal severity.
        self.items
            .iter()
            .filter(|b| !self.dismissed.contains(&b.key))
            .max_by_key(|b| b.severity)
    }
}

/// Effective sidebar collapse: the user's choice, else automatic below 900 px.
pub fn sidebar_collapsed(state: &UiState, window_w: f32) -> bool {
    window_w < layout::COMPACT_BELOW
        || state
            .sidebar_pref
            .unwrap_or(window_w < layout::SIDEBAR_AUTO_COLLAPSE_BELOW)
}

/// The window title for the current route.
pub fn window_title(state: &UiState, env: &Env<'_>) -> String {
    if let Some(q) = &state.quick
        && !q.expanded
    {
        let name = q
            .game
            .and_then(|g| env.bridge.library().get(g).map(|s| s.name.to_string()));
        return match name {
            Some(n) => strings::title::window(&n),
            None => strings::APP_NAME.to_owned(),
        };
    }
    let route = state.nav.current();
    let game_name = match route {
        Route::Game(id) => env.bridge.library().get(id).map(|g| g.name.to_string()),
        _ => None,
    };
    strings::title::window(route.title(game_name.as_deref()))
}

/// Paint the whole window.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let ctx = ui.ctx().clone();
    let theme = theme::theme(&ctx);
    let p = theme.palette.clone();
    crate::widgets::modal::apply_focus_return(&ctx);
    crate::widgets::track_focus_source(&ctx);

    let title = window_title(state, env);
    if state.last_title.as_deref() != Some(title.as_str()) {
        ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
        state.last_title = Some(title);
    }

    // Quick-launch layout: the compact window, nothing else.
    if state.quick.as_ref().is_some_and(|q| !q.expanded) {
        CentralPanel::default()
            .frame(Frame::NONE.fill(p.bg_base))
            .show(ui, |ui| quick_launch::show(ui, state, env));
        show_overlays(&ctx, state, env);
        crate::a11y::Announcer::show(ui);
        return;
    }

    theme::set_controller_mode(&ctx, state.controller);
    restore_card_focus(state);
    handle_global_keys(ui, state, env);
    handle_pad(&ctx, state, env);
    host_banners(state, env);
    super::debug::banners(state, env);
    super::debug::shortcuts(&ctx, state, env);
    for (game, path) in std::mem::take(&mut state.shares) {
        details::permissions::share_folder(state, env, game, path);
    }

    let window_w = ctx.content_rect().width();
    let route = state.nav.current();
    if state.controller {
        pad_footer(ui, route);
    }
    if route.has_sidebar() {
        let collapsed = sidebar_collapsed(state, window_w);
        let full_w = if window_w >= layout::WIDE_FROM {
            layout::SIDEBAR_W_WIDE
        } else {
            layout::SIDEBAR_W
        };
        let t = ctx.animate_bool_with_time(
            Id::unique("sidebar.width"),
            collapsed,
            theme.motion.movement(Dur::Glide),
        );
        let w = egui::lerp(full_w..=layout::RAIL_W, t);
        Panel::left("sidebar")
            .resizable(false)
            .exact_size(w)
            .show_separator_line(false)
            .frame(Frame::NONE.fill(p.bg_surface))
            .show(ui, |ui| {
                library::sidebar::show(ui, state, env, t > 0.5);
            });
    }

    // Route change: the content cross-fades in (USER_EXPERIENCE §10).
    let route_in = theme::motion::appear(
        &ctx,
        Id::unique(("route", route)),
        theme.motion.fade(Dur::Base),
    );
    CentralPanel::default()
        .frame(Frame::NONE.fill(p.bg_base))
        .show(ui, |ui| {
            ui.multiply_opacity(route_in);
            // The library keeps its tools in the top band, level with the brand, and shows
            // the banner beneath them; everywhere else the banner leads the page.
            let recovering = env.bridge.diagnostics().recovery.is_some();
            let in_page = route == Route::Library && !recovering;
            if route != Route::Onboarding && !in_page {
                banner_slot(ui, state, space::S4, 0.0);
            }
            if super::debug::recovery::full_area(ui, state, env) {
                return;
            }
            match route {
                Route::Library => library::show(ui, state, env),
                Route::Gallery => gallery::show(ui, state, env),
                Route::Game(id) => details::show(ui, state, env, id),
                Route::Statistics => statistics::show(ui, state, env),
                Route::Settings(tab) => settings::show(ui, state, env, tab),
                Route::Onboarding => onboarding::show(ui, state, env),
            }
        });

    show_overlays(&ctx, state, env);
    drop_zone(&ctx, state, env);
    announce_toasts(&ctx, state);
    crate::a11y::Announcer::show(ui);
    let out = toast::show(&ctx, &mut state.toasts, env.mono, 0.0);
    for (_, key) in out.actions {
        if let Some(cmd) = state.undo.remove(&key) {
            env.bridge.send(cmd);
        }
    }
}

/// The banner that owns the slot, on the page's margins, with `top` and `bottom` space
/// around it. Paints nothing when there is no banner.
pub fn banner_slot(ui: &mut Ui, state: &mut UiState, top: f32, bottom: f32) {
    let Some(b) = state.banners.top().cloned() else {
        return;
    };
    let margin = library::margin(ui);
    Frame::NONE
        .inner_margin(egui::Margin {
            left: margin as i8,
            right: margin as i8,
            top: top as i8,
            bottom: bottom as i8,
        })
        .show(ui, |ui| {
            let r = banner(ui, b.severity, &b.text, b.action.as_deref(), b.dismissible);
            if r.dismissed {
                state.banners.dismiss(b.key);
            }
            if r.action {
                banner_action(state, b.key);
            }
        });
}

/// Each toast is announced once, politely, when it appears (ACCESSIBILITY §4.3).
fn announce_toasts(ctx: &egui::Context, state: &mut UiState) {
    for t in state.toasts.visible() {
        if state.announced_toast.is_none_or(|last| t.id > last) {
            crate::a11y::Announcer::announce(ctx, t.text.clone(), crate::a11y::Politeness::Polite);
            state.announced_toast = Some(t.id);
        }
    }
}

/// After a game page, focus is on the card that was opened (ACCESSIBILITY §2.1).
fn restore_card_focus(state: &mut UiState) {
    let route = state.nav.current();
    if let (Route::Library, Some(Route::Game(id))) = (route, state.prev_route) {
        state.library.selected = Some(id);
        state.pending_focus = Some(id);
    }
    state.prev_route = Some(route);
}

/// Controller intents that are application actions (USER_EXPERIENCE §5.3).
fn handle_pad(ctx: &egui::Context, state: &mut UiState, env: &mut Env<'_>) {
    use crate::input::gamepad::NavIntent;
    let route = state.nav.current();
    let free = state.nav.overlays().is_empty()
        && state.library.menu.is_none()
        && !egui::Popup::is_any_open(ctx);
    // Entering controller mode: focus on the cursor's card (UF-15 step 2).
    if std::mem::take(&mut state.pad_entered)
        && free
        && route == Route::Library
        && ctx.memory(|m| m.focused()).is_none()
    {
        let lib = env.bridge.library().clone();
        let v = state.library.visible(&lib);
        let ids: Vec<_> = v.iter().map(|i| lib.games[*i as usize].id).collect();
        let target = state
            .library
            .selected
            .filter(|g| ids.contains(g))
            .or_else(|| ids.first().copied());
        state.library.selected = target;
        state.pending_focus = target;
    }
    for intent in std::mem::take(&mut state.pad_actions) {
        if !free {
            continue;
        }
        match intent {
            NavIntent::Favorite => on_action(state, env, route, Action::Favorite),
            NavIntent::ContextMenu => on_action(state, env, route, Action::ContextMenu),
            NavIntent::FocusSearch if route == Route::Library => state.focus_search = true,
            NavIntent::PrevGroup | NavIntent::NextGroup => {
                let fwd = intent == NavIntent::NextGroup;
                if route == Route::Library {
                    cycle_collection(state, env, fwd);
                } else {
                    let a = if fwd {
                        Action::NextTab
                    } else {
                        Action::PrevTab
                    };
                    on_action(state, env, route, a);
                }
            }
            _ => {}
        }
    }
}

/// LB/RB in the library: the previous/next collection shown in the sidebar.
fn cycle_collection(state: &mut UiState, env: &mut Env<'_>, fwd: bool) {
    use crate::vm::library::{CollectionExt, FIXED, count};
    let lib = env.bridge.library().clone();
    let mut all: Vec<_> = FIXED
        .into_iter()
        .filter(|c| !c.conditional() || count(&lib, *c) > 0)
        .collect();
    all.extend(
        lib.tags
            .iter()
            .map(|t| easyplay_core::library::Collection::Tag(t.id)),
    );
    let n = all.len();
    let i = all
        .iter()
        .position(|c| *c == state.library.collection)
        .unwrap_or(0);
    let j = if fwd { (i + 1) % n } else { (i + n - 1) % n };
    state.library.set_collection(all[j]);
    // Focus moves to the first card of the new collection.
    state.library.selected = None;
    let v = state.library.visible(&lib);
    let first = v.first().map(|i| lib.games[*i as usize].id);
    state.library.selected = first;
    state.pending_focus = first;
}

/// Button hints at the bottom of the window in controller mode (UF-15).
fn pad_footer(ui: &mut Ui, route: Route) {
    let hints: &[(&str, &str)] = if route == Route::Library {
        &strings::a11y::PAD_HINTS
    } else {
        &strings::a11y::PAD_HINTS_PAGE
    };
    let p = theme::theme(ui.ctx()).palette.clone();
    let resp = Panel::bottom("controller_hints")
        .exact_size(layout::TOOLBAR_H - space::S3)
        .show_separator_line(true)
        .frame(
            Frame::NONE
                .fill(p.bg_surface)
                .inner_margin(egui::Margin::symmetric(layout::PAGE_MARGIN as i8, 0)),
        )
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::S2;
                for (button, what) in hints.iter().rev() {
                    ui.label(Type::Body.rich(*what).color(p.text_secondary));
                    crate::widgets::key_hint::key_hint(ui, button);
                    ui.add_space(space::S3);
                }
            });
        });
    crate::a11y::labelled(
        ui.ctx(),
        resp.response.id,
        egui::Role::ContentInfo,
        &strings::a11y::pad_hints_spoken(hints),
    );
}

fn banner_action(state: &mut UiState, key: &str) {
    if super::debug::banner_action(state, key) {
        return;
    }
    if key == "host" {
        state.onboarding.reduced = true;
        state.onboarding.step = crate::vm::screens::OnboardingStep::SystemCheck;
        state.nav.go_to(Route::Onboarding);
    }
}

/// Standing host conditions become banners.
fn host_banners(state: &mut UiState, env: &Env<'_>) {
    let host = env.bridge.host();
    if host.status == HostStatus::NeedsAttention && host.has_blocking_failure() {
        state.banners.set(Banner {
            key: "host",
            severity: Severity::Warning,
            text: strings::onboarding::BANNER.to_owned(),
            action: Some(strings::onboarding::CHECK_AGAIN.to_owned()),
            dismissible: true,
        });
    } else {
        state.banners.clear("host");
    }
}

fn here(route: Route) -> Here {
    match route {
        Route::Library => Here::Library,
        Route::Game(_) => Here::GamePage,
        Route::Settings(_) => Here::Settings,
        _ => Here::Other,
    }
}

fn handle_global_keys(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let ctx = ui.ctx().clone();
    if !state.nav.overlays().is_empty()
        || state.library.menu.is_some()
        || egui::Popup::is_any_open(&ctx)
    {
        return;
    }
    let route = state.nav.current();
    if route == Route::Onboarding {
        return;
    }
    let focused = ctx.memory(|m| m.focused());
    let text_focus = focused.is_some()
        && ctx
            .memory(|m| m.focused())
            .is_some_and(|id| egui::text_edit::TextEditState::load(&ctx, id).is_some());
    let d = DispatchCtx {
        here: here(route),
        text_focus,
        single_keys: env.bridge.prefs().integration.single_key_shortcuts,
    };
    // `Esc` in the library: search, then filters, then selection.
    if route == Route::Library
        && !text_focus
        && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
    {
        state.library.escape();
    }
    for action in shortcuts::dispatch(&ctx, d) {
        on_action(state, env, route, action);
    }
    if ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Extra1)) {
        state.navigate(AfterDiscard::Back);
    }
    #[cfg(debug_assertions)]
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F12)) {
        if state.nav.current() == Route::Gallery {
            state.nav.back();
        } else {
            state.nav.push(Route::Gallery);
        }
    }
}

fn on_action(state: &mut UiState, env: &mut Env<'_>, route: Route, action: Action) {
    let ctx = env.ctx.clone();
    // The game a per-game shortcut applies to.
    let game = match route {
        Route::Game(id) => Some(id),
        Route::Library if state.library.focused => state.library.selected,
        _ => None,
    };
    if let Some(g) = game
        && let Some(a) = library::shortcut_action(action)
        && !(route == Route::Library && action == Action::OpenFolder)
    {
        actions::apply(state, env, g, a);
        return;
    }
    match action {
        Action::FocusSearch => state.focus_search = true,
        Action::SearchAll => {
            state.library.search_all = true;
            state.focus_search = true;
        }
        Action::AddGame => state.picks.push(PickPurpose::AddGame),
        Action::GridView => state.library.view = crate::vm::library::ViewMode::Grid,
        Action::ListView => state.library.view = crate::vm::library::ViewMode::List,
        Action::ToggleSidebar => {
            let w = ctx.content_rect().width();
            let now = sidebar_collapsed(state, w);
            state.sidebar_pref = Some(!now);
        }
        Action::Settings => {
            state.navigate(AfterDiscard::Route(Route::Settings(SettingsTab::General)));
        }
        Action::Back => state.navigate(AfterDiscard::Back),
        Action::NextTab | Action::PrevTab => {
            let fwd = action == Action::NextTab;
            match route {
                Route::Game(_) => {
                    let tabs = crate::vm::details::DetailsTab::visible(
                        env.bridge.prefs().advanced.developer_mode,
                    );
                    let n = tabs.len();
                    let i = state.details.tab.index().min(n - 1);
                    let j = if fwd { (i + 1) % n } else { (i + n - 1) % n };
                    state.details.tab = tabs[j];
                }
                Route::Settings(t) => {
                    let all = settings::TABS;
                    let i = all.iter().position(|x| *x == t).unwrap_or(0);
                    let j = if fwd {
                        (i + 1) % all.len()
                    } else {
                        (i + all.len() - 1) % all.len()
                    };
                    state.nav.replace(Route::Settings(all[j]));
                }
                _ => {}
            }
        }
        Action::SelectAll => {
            let lib = env.bridge.library().clone();
            let v = state.library.visible(&lib);
            state.library.select_all(&lib, &v);
        }
        Action::Undo => {
            if let Some(cmd) = state.last_undo.take().and_then(|k| state.undo.remove(&k)) {
                env.bridge.send(cmd);
                state.toast(strings::feedback::UNDONE, ToastKind::Info, None, env.mono);
            }
        }
        Action::ContextMenu => {
            if let Some(id) = state.library.selected
                && state.library.focused
            {
                let at = state
                    .library
                    .cursor_rect
                    .map_or(egui::Pos2::ZERO, |r| r.center());
                library::open_menu(state, id, at);
            }
        }
        Action::Refresh => env.bridge.send(UiCommand::RescanMissing),
        Action::Fullscreen => {
            let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!full));
        }
        Action::ShortcutSheet => state.nav.open(Overlay::Shortcuts),
        Action::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
        Action::Rename => {
            if let Route::Game(id) = route {
                let name = env
                    .bridge
                    .library()
                    .get(id)
                    .map(|g| g.name.to_string())
                    .unwrap_or_default();
                state.details.rename = Some(name);
            }
        }
        _ => {}
    }
}

fn overlay_id(o: Overlay) -> Id {
    Id::unique(("overlay", o))
}

fn show_overlays(ctx: &egui::Context, state: &mut UiState, env: &mut Env<'_>) {
    let overlays = state.nav.overlays().to_vec();
    for o in overlays {
        let id = overlay_id(o);
        match o {
            Overlay::Dialog(d) => dialogs::show(ctx, id, state, env, d),
            Overlay::Setup => setup::show(ctx, id, state, env),
            Overlay::AddGame => add_game::show(ctx, id, state, env),
            Overlay::Shortcuts => dialogs::shortcuts::show(ctx, id, state),
            Overlay::Artwork(g) => dialogs::artwork::show(ctx, id, state, env, g),
            Overlay::Tags(g) => dialogs::tags::show(ctx, id, state, env, g),
            Overlay::FixPicker(g) => details::runtime::fix_picker(ctx, id, state, env, g),
            Overlay::History => statistics::history_sheet(ctx, id, state, env),
            Overlay::Inspector => super::debug::inspector::show(ctx, id, state, env),
            Overlay::Logs => super::debug::logs::sheet_view(ctx, id, state, env),
            Overlay::Report => super::debug::report::show(ctx, id, state, env),
        }
    }
}

/// The drop zone (USER_EXPERIENCE §5.4) and drop routing.
fn drop_zone(ctx: &egui::Context, state: &mut UiState, env: &mut Env<'_>) {
    let (hovered, dropped, pos) = ctx.input(|i| {
        (
            i.raw
                .hovered_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect::<Vec<_>>(),
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect::<Vec<_>>(),
            i.pointer.hover_pos(),
        )
    });
    let any_hover = ctx.input(|i| !i.raw.hovered_files.is_empty());
    if !dropped.is_empty() {
        let target = state.dnd.target_at(pos);
        match dnd::route(target, dropped) {
            DropAction::Open(paths) => env.bridge.send(UiCommand::OpenPaths {
                paths,
                origin: OpenOrigin::DragDrop,
            }),
            DropAction::ScanFolder(folder) => {
                let q = env.bridge.query_id();
                state.add_game = Some(crate::vm::screens::AddGameVm {
                    folder: folder.clone(),
                    query: Some(q),
                    ..Default::default()
                });
                state.nav.open(Overlay::AddGame);
                env.bridge.send(UiCommand::ScanFolder {
                    folder,
                    reply_to: q,
                });
            }
            DropAction::SetCover(id, path) => {
                env.bridge.send(UiCommand::ImportArtwork {
                    id,
                    slot: easyplay_core::model::ArtSlot::Cover,
                    source: crate::bridge::ArtworkSource::File(path),
                });
                state.toast(
                    strings::feedback::COVER_SET,
                    ToastKind::Success,
                    Some(UiCommand::ClearArtwork {
                        id,
                        slot: easyplay_core::model::ArtSlot::Cover,
                    }),
                    env.mono,
                );
            }
            DropAction::ShareFolders(game, paths) => {
                for path in paths {
                    details::permissions::share_folder(state, env, game, path);
                }
            }
            DropAction::Refuse => {
                state.toast(strings::dnd::REFUSED, ToastKind::Warning, None, env.mono);
            }
        }
        state.dnd.hovering = None;
        return;
    }
    if !any_hover {
        state.dnd.hovering = None;
        return;
    }
    let payload = dnd::classify(&hovered);
    state.dnd.hovering = Some(payload);
    let theme = theme::theme(ctx);
    let p = theme.palette.clone();
    let rect = ctx.content_rect();
    let refuse = payload == Payload::Unsupported;
    let shown = theme::motion::appear(
        ctx,
        Id::unique("easyplay.dropzone.in"),
        theme.motion.fade(Dur::Quick),
    );
    egui::Area::new(Id::unique("easyplay.dropzone"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .interactable(false)
        .show(ctx, |ui| {
            ui.multiply_opacity(shown);
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, theme::palette::fade(p.backdrop, 0.85));
            let inner = rect.shrink(space::S6);
            let color = if refuse { p.danger } else { p.accent };
            let pts = [
                inner.left_top(),
                inner.right_top(),
                inner.right_bottom(),
                inner.left_bottom(),
                inner.left_top(),
            ];
            painter.extend(egui::Shape::dashed_line(
                &pts,
                egui::Stroke::new(2.0, color),
                10.0,
                6.0,
            ));
            let c = inner.center();
            painter.text(
                c - egui::vec2(0.0, space::S10),
                egui::Align2::CENTER_CENTER,
                if refuse {
                    icons::ERROR
                } else {
                    icons::DOWNLOAD
                }
                .text(false),
                icons::font(theme::tokens::icon::EMPTY),
                color,
            );
            painter.text(
                c + egui::vec2(0.0, space::S4),
                egui::Align2::CENTER_CENTER,
                if refuse {
                    strings::dnd::REFUSED
                } else {
                    strings::dnd::DROP_TO_ADD
                },
                Type::Title.font_id(),
                p.text_primary,
            );
            if let Some(what) = strings::dnd::payload(payload) {
                painter.text(
                    c + egui::vec2(0.0, space::S10),
                    egui::Align2::CENTER_CENTER,
                    what,
                    Type::Body.font_id(),
                    p.text_secondary,
                );
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_priority() {
        let mut q = BannerQueue::default();
        q.set(Banner {
            key: "a",
            severity: Severity::Info,
            text: "a".into(),
            action: None,
            dismissible: true,
        });
        q.set(Banner {
            key: "b",
            severity: Severity::Warning,
            text: "b".into(),
            action: None,
            dismissible: true,
        });
        q.set(Banner {
            key: "c",
            severity: Severity::Info,
            text: "c".into(),
            action: None,
            dismissible: true,
        });
        assert_eq!(q.top().map(|b| b.key), Some("b"));
        q.dismiss("b");
        assert_eq!(q.top().map(|b| b.key), Some("c"));
        q.clear("c");
        assert_eq!(q.top().map(|b| b.key), Some("a"));
    }
}
