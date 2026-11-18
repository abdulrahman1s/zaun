//! The application shell (SCREEN_SPECS §0): the top bar, the page beneath it, the banner
//! slot, the toast area, overlay layers, the drop zone, the window title per route, global
//! shortcuts, and what moves between pages.
//!
//! There is no sidebar. One 52 pt top bar is the only chrome that is always there: the
//! mark, three destinations, the running game if there is one, and the page's own controls.
//! `Panel::top("top_bar")` then `CentralPanel` last [F-305]; overlays are `egui::Modal`
//! [F-306]; toasts an `Area` anchored bottom-centre; the drop zone a foreground `Area`
//! covering the window [F-321].
//!
//! A change of page is a short exchange: the old page fades out, the new one fades in, and
//! between the library and a game page the cover you opened is the cover that arrives
//! (`flight`).

use egui::{
    Align, Align2, CentralPanel, CornerRadius, Frame, Id, Key, Layout, Margin, Modifiers, Panel,
    Rect, Sense, Ui, ViewportCommand, pos2, vec2,
};
use zaun_core::ids::GameId;
use zaun_core::model::Activity;

use super::flight::Flying;
use super::{
    Env, actions, add_game, details, dialogs, gallery, library, onboarding, quick_launch, settings,
    setup, statistics,
};
use crate::app::UiState;
use crate::bridge::{Command, HostStatus, OpenOrigin, UiCommand};
use crate::input::dnd::{self, DropAction, Payload};
use crate::input::shortcuts::{self, Action, DispatchCtx, Here};
use crate::route::{Overlay, Route, SettingsTab};
use crate::strings;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{control, icon as icon_size, layout, radius, space, stroke, travel};
use crate::theme::{self, Dur, Theme, Type, icons, motion};
use crate::vm::dialogs::AfterDiscard;
use crate::vm::toasts::ToastKind;
use crate::widgets::banner::{Severity, banner};
use crate::widgets::cover::{self, CoverLook, CoverSource};
use crate::widgets::{brand, focus_ring, hover_t, press_t, progress, toast, tooltip};

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

/// A change of page in progress: the page being left, and when the change began.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageTurn {
    /// The page being left.
    pub from: Route,
    /// When the change began (egui time).
    pub started: f64,
}

/// The page a route belongs to: a change of page fades, a change of tab does not.
fn page_of(route: Route) -> Route {
    match route {
        Route::Settings(_) => Route::Settings(SettingsTab::General),
        r => r,
    }
}

/// The page gutter for the width of the page area.
pub fn gutter(ctx: &egui::Context) -> f32 {
    if ctx.content_rect().width() < layout::NARROW_BELOW {
        layout::GUTTER_NARROW
    } else {
        layout::GUTTER
    }
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
            .frame(Frame::NONE.fill(p.bg))
            .show(ui, |ui| quick_launch::show(ui, state, env));
        show_overlays(&ctx, state, env);
        crate::a11y::Announcer::show(ui);
        // What is leaving (a dialog, a menu) is painted last.
        crate::widgets::modal::paint_exits(&ctx);
        return;
    }

    theme::set_controller_mode(&ctx, state.controller);
    restore_card_focus(state);
    handle_global_keys(ui, state, env);
    settings::watch_mic_test(&ctx, env);
    handle_pad(&ctx, state, env);
    host_banners(state, env);
    super::debug::banners(state, env);
    super::debug::shortcuts(&ctx, state, env);
    for (game, path) in std::mem::take(&mut state.shares) {
        details::permissions::share_folder(state, env, game, path);
    }

    let route = state.nav.current();
    if let Route::Settings(tab) = route {
        state.settings_tab = tab;
    }
    if state.controller {
        pad_footer(ui, route);
    }
    if route.has_top_bar() {
        top_bar(ui, state, env, route);
    }

    // A change of page: the old one leaves first, the new one follows.
    let now = ctx.input(|i| i.time);
    let shown = state.shown_page.unwrap_or(route);
    if page_of(shown) != page_of(route) {
        state.turn = Some(PageTurn {
            from: shown,
            started: now,
        });
        begin_flight(&ctx, state, shown, route, now, &theme);
    }
    state.shown_page = Some(route);
    let (painted, opacity) = match state.turn {
        Some(turn) => {
            let dt = (now - turn.started) as f32;
            ctx.request_repaint();
            if dt < travel::PAGE_IN_AT {
                (turn.from, 1.0 - dt / Dur::Tip.secs())
            } else {
                let fade_in = theme.motion.fade(Dur::Base).max(f32::EPSILON);
                let a = ((dt - travel::PAGE_IN_AT) / fade_in).clamp(0.0, 1.0);
                if a >= 1.0 {
                    state.turn = None;
                }
                (route, a)
            }
        }
        None => (route, 1.0),
    };
    if painted == Route::Library {
        // The wall reports afresh where its covers are.
        state.flight.wall.clear();
    }

    CentralPanel::default()
        .frame(Frame::NONE.fill(p.bg))
        .show(ui, |ui| {
            ui.multiply_opacity(opacity);
            let recovering = env.bridge.diagnostics().recovery.is_some();
            if painted != Route::Onboarding {
                banner_slot(ui, state, 0.0, space::S2);
            }
            if super::debug::recovery::full_area(ui, state, env) {
                return;
            }
            let _ = recovering;
            match painted {
                Route::Library => library::show(ui, state, env),
                Route::Gallery => gallery::show(ui, state, env),
                Route::Game(id) => details::show(ui, state, env, id),
                Route::Statistics => statistics::show(ui, state, env),
                Route::Chat => super::chat::show(ui, state, env),
                Route::Settings(tab) => settings::show(ui, state, env, tab),
                Route::Onboarding => onboarding::show(ui, state, env),
            }
        });

    paint_flight(&ctx, state, env, now, &theme);
    // Someone calling, on every page; also where Chat's waiting work resolves.
    super::chat::incoming_card(&ctx, state, env);
    show_overlays(&ctx, state, env);
    drop_zone(&ctx, state, env);
    announce_toasts(&ctx, state);
    crate::a11y::Announcer::show(ui);
    let out = toast::show(&ctx, &mut state.toasts, env.mono, 0.0);
    for (_, key) in out.actions {
        for cmd in state.undo.remove(&key).unwrap_or_default() {
            env.bridge.send(cmd);
        }
    }
}

/// The page area: the window beneath the top bar.
fn page_area(ctx: &egui::Context) -> Rect {
    let r = ctx.content_rect();
    Rect::from_min_max(pos2(r.left(), r.top() + layout::TOP_BAR), r.max)
}

/// Between the library and a game page the cover travels: start its flight when both of its
/// places are on screen. Otherwise the pages just exchange.
fn begin_flight(
    ctx: &egui::Context,
    state: &mut UiState,
    from: Route,
    to: Route,
    now: f64,
    theme: &Theme,
) {
    state.flight.flying = None;
    if theme.motion.movement(Dur::Page) <= 0.0 {
        return;
    }
    let area = page_area(ctx);
    let on_wall = |state: &UiState, id: GameId| {
        state
            .flight
            .wall
            .get(&id)
            .copied()
            .filter(|r| area.intersects(*r))
    };
    let flying =
        match (from, to) {
            (Route::Library, Route::Game(id)) => on_wall(state, id)
                .zip(details::cover_slot(area))
                .map(|(from, to)| Flying {
                    id,
                    from,
                    to,
                    started: now,
                    to_page: true,
                }),
            (Route::Game(id), Route::Library) => state
                .flight
                .page
                .filter(|(g, _)| *g == id)
                .map(|(_, r)| r)
                .zip(on_wall(state, id))
                .map(|(from, to)| Flying {
                    id,
                    from,
                    to,
                    started: now,
                    to_page: false,
                }),
            _ => None,
        };
    state.flight.flying = flying;
}

/// The cover in flight, on a layer above the pages.
fn paint_flight(
    ctx: &egui::Context,
    state: &mut UiState,
    env: &mut Env<'_>,
    now: f64,
    theme: &Theme,
) {
    let Some(f) = state.flight.flying else {
        return;
    };
    let secs = theme.motion.movement(Dur::Page);
    let t = motion::progress(now, f.started, 0.0, secs);
    let lib = env.bridge.library().clone();
    let Some(game) = lib.get(f.id) else {
        state.flight.flying = None;
        return;
    };
    // The pass it lands in still paints it, at its place: both ends have already left it
    // out of this pass, and leaving it out here too blinks the cover for a frame.
    let landed = t >= 1.0;
    if landed {
        state.flight.flying = None;
    }
    let e = motion::ease_out(t.min(1.0));
    let rect = Rect::from_min_max(f.from.min.lerp(f.to.min, e), f.from.max.lerp(f.to.max, e));
    let source = match game.cover {
        Some(art) => {
            let px = f.from.width().max(f.to.width()) * ctx.pixels_per_point();
            let key = TexKey {
                art,
                size: ThumbSize::cover_for(px),
            };
            match env.textures.get_or_other_size(key, true, env.art) {
                TexState::Ready(t) => CoverSource::Texture(t, 1.0),
                TexState::Pending(c) => c.map_or(CoverSource::Placeholder, CoverSource::Pending),
            }
        }
        None => CoverSource::Placeholder,
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        Id::unique("zaun.flight"),
    ));
    // It arrives with the shadow of the place it is going to.
    let shade = if f.to_page { e } else { 1.0 - e };
    cover::paint_shadow(&painter, rect, theme.palette.shade_cover, shade);
    let look = CoverLook {
        mark: !game.is_steam(),
        ..CoverLook::default()
    };
    cover::paint(&painter, rect, source, &game.name, look, theme);
    // A Steam game keeps its badge on the way, as both of its places draw it.
    if game.is_steam() {
        cover::paint_steam_badge(&painter, rect, source, &game.name, look, theme);
    }
    ctx.request_repaint();
}

/// A destination of the top bar: 14 pt medium, quiet unless it is where you are.
fn nav_item(ui: &mut Ui, label: &str, current: bool, count: u32) -> egui::Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let text = Type::Nav.line(ui, label, p.t1);
    let extra = super::chat::nav_count_width(ui, count);
    let size = vec2(
        text.size().x + extra + layout::NAV_PAD * 2.0,
        control::BUTTON,
    );
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let name = label.to_owned();
    resp.widget_info(move || egui::WidgetInfo::selected(egui::Role::Button, true, current, &name));
    if ui.is_rect_visible(rect) {
        let hover = hover_t(&resp, &theme);
        let pressed = press_t(&resp, &theme);
        let corner = CornerRadius::same(radius::ROW);
        ui.painter()
            .rect_filled(rect, corner, mix(fade(p.o1, hover), p.o2, pressed));
        let color = if current {
            p.t1
        } else {
            mix(p.t3, p.t2, hover)
        };
        let text_w = text.size().x;
        ui.painter().galley_with_override_text_color(
            pos2(
                rect.left() + layout::NAV_PAD,
                rect.center().y - text.size().y / 2.0,
            ),
            text,
            color,
        );
        super::chat::nav_count(
            ui,
            &resp,
            rect.left() + layout::NAV_PAD + text_w,
            rect.center().y,
            count,
        );
        if resp.has_focus() {
            focus_ring(ui.painter(), rect, corner, &theme);
        }
    }
    resp
}

/// The game that is starting, running or stopping, if there is one: one Zaun runs
/// before one Steam runs (ADR-0031). The flag says it is a Steam game.
fn active_game(env: &Env<'_>) -> Option<(GameId, std::sync::Arc<str>, Activity, bool)> {
    let lib = env.bridge.library();
    let active = |steam: bool| {
        lib.games
            .iter()
            .find(|g| g.is_steam() == steam && !matches!(g.activity, Activity::Idle))
    };
    active(false)
        .or_else(|| active(true))
        .map(|g| (g.id, g.name.clone(), g.activity, g.is_steam()))
}

/// The running game in the top bar: the app's only status display. A 30 pt pill with the
/// live bars, the title (which opens its page), the session clock, and a round stop button.
/// A Steam game has no stop button: Steam runs it (ADR-0031). Laid out right to left, as its
/// caller is.
fn running_pill(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let Some((id, name, activity, steam)) = active_game(env) else {
        return;
    };
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let starting = matches!(activity, Activity::Launching(_));
    let clock = match activity {
        Activity::Running { since } => {
            let ms = env.now.since(since).map_or(0, |d| d.as_millis() as u64);
            // The clock changes once a second: ask for the frame that shows the next one.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(1_000 - ms % 1_000));
            strings::fmt::session_clock(ms)
        }
        Activity::Launching(_) => strings::nav::STARTING().to_owned(),
        Activity::Stopping => strings::play::STOPPING().to_owned(),
        Activity::Lingering | Activity::Idle => String::new(),
    };
    let time = Type::BodyStrong.numeric_galley(ui, &clock, p.accent_text);
    let (pad_l, pad_r, gap) = (space::S2 + space::S0, space::S1, space::S2);
    let lead_w = control::SPINNER.min(space::S2 + space::S0);
    // The pill is at most 300 wide and never wider than the room the bar has left: its
    // title is cut first, then left out, and a bar with no room at all shows no pill.
    let apart = space::S1 + space::S0;
    let room = (ui.available_width() - apart - layout::TOP_BAR_GAP).min(layout::RUN_PILL_MAX_W);
    let stop_w = if steam { 0.0 } else { gap + control::RUN_STOP };
    let bare = pad_l + lead_w + gap + time.size().x + stop_w + pad_r;
    if room < bare {
        return;
    }
    let title_room = room - bare - gap;
    let with_title = title_room >= space::S10;
    let title = Type::BodyStrong.galley(
        ui,
        if with_title { &name } else { "" },
        p.t1,
        title_room.max(1.0),
        1,
    );
    let w = bare
        + if with_title {
            title.size().x + gap
        } else {
            0.0
        };
    let shown = motion::appear(
        ui.ctx(),
        Id::unique("zaun.running_pill"),
        theme.motion.fade(Dur::Base),
    );
    // The pill stands a little apart from the page's controls.
    ui.add_space(apart);
    let (rect, _) = ui.allocate_exact_size(vec2(w, control::BUTTON), Sense::hover());
    let painter = ui.painter().clone();
    let f = |c: egui::Color32| fade(c, shown);
    painter.rect_filled(rect, CornerRadius::same(radius::PILL), f(p.o2));
    let cy = rect.center().y;
    let mut x = rect.left() + pad_l;
    if starting || matches!(activity, Activity::Stopping) {
        progress::spinner(ui, pos2(x + lead_w / 2.0, cy), lead_w / 2.0 - 1.0, f(p.t1));
    } else {
        let size = progress::live_size();
        let at = Rect::from_center_size(pos2(x + lead_w / 2.0, cy), size);
        painter.extend(progress::live_shapes(ui.ctx(), at, f(p.accent_text)));
    }
    x += lead_w + gap;
    // The title opens the game page (the live mark with it, so there is something to
    // press when the title has no room).
    let title_rect = Rect::from_min_max(rect.left_top(), pos2(x + title.size().x, rect.bottom()));
    let open = ui.interact(title_rect, Id::unique(("running.open", id)), Sense::click());
    crate::a11y::button(&open, &strings::nav::open_page_of(&name));
    let open = tooltip::attach(open, strings::nav::OPEN_PAGE());
    painter.galley_with_override_text_color(
        pos2(x, cy - title.size().y / 2.0),
        title.clone(),
        f(p.t1),
    );
    if open.has_focus() {
        focus_ring(&painter, title_rect, CornerRadius::same(radius::XS), &theme);
    }
    if with_title {
        x += title.size().x + gap;
    }
    painter.galley_with_override_text_color(
        pos2(x, cy - time.size().y / 2.0),
        time.clone(),
        f(p.accent_text),
    );
    if steam {
        if open.clicked() {
            actions::apply(state, env, id, actions::GameAction::Open);
        }
        return;
    }
    x += time.size().x + gap;
    // The stop button: a round quiet button.
    let stop_rect = Rect::from_center_size(
        pos2(x + control::RUN_STOP / 2.0, cy),
        vec2(control::RUN_STOP, control::RUN_STOP),
    );
    let can_stop = matches!(activity, Activity::Running { .. } | Activity::Lingering);
    let stop = ui.interact(
        stop_rect,
        Id::unique(("running.stop", id)),
        if can_stop {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    crate::a11y::button(&stop, &strings::play::stop_name(&name));
    let stop = tooltip::attach(stop, strings::play::STOP());
    let hover = if can_stop {
        hover_t(&stop, &theme)
    } else {
        0.0
    };
    let pressed = if can_stop {
        press_t(&stop, &theme)
    } else {
        0.0
    };
    painter.circle_filled(
        stop_rect.center(),
        control::RUN_STOP / 2.0,
        f(mix(fade(p.o2, hover), p.o3, pressed)),
    );
    painter.text(
        stop_rect.center(),
        Align2::CENTER_CENTER,
        icons::STOP.text(true),
        icons::font(icon_size::INLINE),
        f(if can_stop {
            mix(p.t2, p.t1, hover)
        } else {
            p.t4
        }),
    );
    if stop.has_focus() {
        focus_ring(
            &painter,
            stop_rect,
            CornerRadius::same(radius::PILL),
            &theme,
        );
    }
    if open.clicked() {
        actions::apply(state, env, id, actions::GameAction::Open);
    } else if stop.clicked() {
        actions::apply(state, env, id, actions::GameAction::Stop);
    }
}

/// The top bar: the mark, the three destinations, and on the right the running game and
/// the page's own controls. 52 high, no line under it.
fn top_bar(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, route: Route) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let left = gutter(ui.ctx());
    let panel = Panel::top("top_bar")
        .exact_size(layout::TOP_BAR)
        .show_separator_line(false)
        .frame(Frame::NONE.fill(p.bg).inner_margin(Margin {
            left: left as i8,
            right: layout::TOP_BAR_END as i8,
            top: 0,
            bottom: 0,
        }))
        .show(ui, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let (mark, _) =
                    ui.allocate_exact_size(vec2(layout::MARK, layout::MARK), Sense::hover());
                brand::paint_mark(ui.painter(), mark, p.fill, p.bg);
                ui.add_space(layout::MARK_GAP + layout::TOP_BAR_GAP);
                // Library · Chat · Statistics · Settings (chat.html's top bar).
                let here = match page_of(route) {
                    Route::Library | Route::Game(_) => Some(0),
                    Route::Chat => Some(1),
                    Route::Statistics => Some(2),
                    Route::Settings(_) => Some(3),
                    _ => None,
                };
                let items = [
                    (strings::nav::LIBRARY(), Route::Library),
                    (strings::nav::CHAT(), Route::Chat),
                    (strings::nav::STATISTICS(), Route::Statistics),
                    (
                        strings::nav::SETTINGS(),
                        Route::Settings(state.settings_tab),
                    ),
                ];
                let nav = ui
                    .scope(|ui| {
                        ui.spacing_mut().item_spacing.x = layout::NAV_GAP;
                        for (i, (label, to)) in items.into_iter().enumerate() {
                            // "Chat" carries the count of unread messages in unmuted
                            // chats (ADR-0033).
                            let count = if to == Route::Chat {
                                env.bridge.chat().unread()
                            } else {
                                0
                            };
                            let item = nav_item(ui, label, here == Some(i), count);
                            if item.clicked() && page_of(route) != page_of(to) {
                                state.navigate(AfterDiscard::Route(to));
                            }
                        }
                    })
                    .response;
                crate::a11y::labelled(
                    ui.ctx(),
                    nav.id,
                    egui::Role::Navigation,
                    strings::nav::LANDMARK(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = layout::TOP_BAR_GAP;
                    match route {
                        Route::Library => library::top_bar(ui, state, env),
                        Route::Statistics => statistics::top_bar(ui, state, env),
                        _ => {}
                    }
                    running_pill(ui, state, env);
                    // A call or a share: left of the running game (laid out right to left).
                    super::chat::top_bar_pill(ui, state, env);
                });
            });
        });
    let _ = (panel, stroke::BORDER);
}

/// The banner that owns the slot, on the page's margins, with `top` and `bottom` space
/// around it. Paints nothing when there is no banner.
pub fn banner_slot(ui: &mut Ui, state: &mut UiState, top: f32, bottom: f32) {
    let Some(b) = state.banners.top().cloned() else {
        return;
    };
    let margin = gutter(ui.ctx());
    Frame::NONE
        .inner_margin(Margin {
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

/// LB/RB in the library: the previous/next scope of the filter line.
fn cycle_collection(state: &mut UiState, env: &mut Env<'_>, fwd: bool) {
    let lib = env.bridge.library().clone();
    state.library.cycle_scope(&lib, fwd);
    // Focus moves to the first cover of the new scope.
    state.library.selected = None;
    let v = state.library.visible(&lib);
    let first = v.first().map(|i| lib.games[*i as usize].id);
    state.library.selected = first;
    state.pending_focus = first;
}

/// Button hints at the bottom of the window in controller mode (UF-15).
fn pad_footer(ui: &mut Ui, route: Route) {
    let hints: &[(&str, &str)] = if route == Route::Library {
        &strings::a11y::PAD_HINTS()
    } else {
        &strings::a11y::PAD_HINTS_PAGE()
    };
    let p = theme::theme(ui.ctx()).palette.clone();
    let resp = Panel::bottom("controller_hints")
        .exact_size(layout::TOP_BAR - space::S3)
        .show_separator_line(true)
        .frame(
            Frame::NONE
                .fill(p.bg)
                .inner_margin(Margin::symmetric(layout::GUTTER as i8, 0)),
        )
        .show(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::S2;
                for (button, what) in hints.iter().rev() {
                    ui.label(Type::Body.rich(*what).color(p.t2));
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
            text: strings::onboarding::BANNER().to_owned(),
            action: Some(strings::onboarding::CHECK_AGAIN().to_owned()),
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
    if settings::listening_for_key(&ctx)
        || !state.nav.overlays().is_empty()
        || state.library.menu.is_some()
        || state.library.tag_menu.is_some()
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
    // `Esc` on the Chat page, from anywhere on it: one step back at a time (chat.html
    // `chatKey`); with nothing left to undo it is Back, except from a field.
    if route == Route::Chat
        && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
        && !super::chat::escape(&ctx, state)
        && !text_focus
        && !super::chat::had_text_focus(&ctx)
    {
        state.navigate(AfterDiscard::Back);
    }
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
    if ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Extra1))
        && !super::chat::back(state)
    {
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
        Action::AddGame => add_game::open(state),
        Action::GridView => state.library.view = crate::vm::library::ViewMode::Grid,
        Action::ListView => state.library.view = crate::vm::library::ViewMode::List,
        Action::Settings => {
            state.navigate(AfterDiscard::Route(Route::Settings(SettingsTab::General)));
        }
        // In a narrow window, Back on the Chat page closes the open conversation first.
        Action::Back if super::chat::back(state) => {}
        Action::Back => state.navigate(AfterDiscard::Back),
        Action::NextTab | Action::PrevTab => {
            let fwd = action == Action::NextTab;
            match route {
                // A Steam game's page has tabs of its own (ADR-0031).
                Route::Game(id) if id.is_steam() => {
                    if let Some(g) = env.bridge.library().get(id).cloned() {
                        details::steam::cycle_tab(state, &g, fwd);
                    }
                }
                Route::Game(_) => {
                    let tabs = crate::vm::details::DetailsTab::visible();
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
            if let Some(cmds) = state.last_undo.take().and_then(|k| state.undo.remove(&k)) {
                for cmd in cmds {
                    env.bridge.send(cmd);
                }
                state.toast(strings::feedback::UNDONE(), ToastKind::Info, None, env.mono);
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
        Action::Refresh => env.bridge.send(Command::RescanMissing),
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
    // Order: the page draws before this runs. On the Chat page with a conversation that
    // carries files, the composer has already taken any dropped files (and draws its own
    // "Drop to send" overlay over the conversation); adding games must not see them, so the
    // drop zone stands aside for this pass. Everywhere else it works as before.
    if std::mem::take(&mut state.chat.takes_drops) {
        state.dnd.hovering = None;
        return;
    }
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
            DropAction::Open(paths) => env.bridge.send(Command::OpenPaths {
                paths,
                origin: OpenOrigin::DragDrop,
            }),
            DropAction::ScanFolder(folder) => {
                add_game::scan(state, env.bridge, folder);
            }
            DropAction::SetCover(id, path) => {
                env.bridge.send(UiCommand::ImportArtwork {
                    id,
                    slot: zaun_core::model::ArtSlot::Cover,
                    source: crate::bridge::ArtworkSource::File(path),
                });
                state.toast(
                    strings::feedback::COVER_SET(),
                    ToastKind::Success,
                    Some(UiCommand::ClearArtwork {
                        id,
                        slot: zaun_core::model::ArtSlot::Cover,
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
                state.toast(strings::dnd::REFUSED(), ToastKind::Warning, None, env.mono);
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
    let shown = motion::appear(
        ctx,
        Id::unique("zaun.dropzone.in"),
        theme.motion.fade(Dur::Quick),
    );
    egui::Area::new(Id::unique("zaun.dropzone"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .interactable(false)
        .show(ctx, |ui| {
            ui.multiply_opacity(shown);
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, p.scrim);
            let inner = rect.shrink(layout::DIALOG_MARGIN);
            let color = if refuse { p.coral } else { p.accent_mark };
            let pts = [
                inner.left_top(),
                inner.right_top(),
                inner.right_bottom(),
                inner.left_bottom(),
                inner.left_top(),
            ];
            painter.extend(egui::Shape::dashed_line(
                &pts,
                egui::Stroke::new(stroke::FIELD_FOCUS, color),
                space::S2,
                space::S1 + space::S0,
            ));
            let c = inner.center();
            let title = Type::Title.line(
                ui,
                if refuse {
                    strings::dnd::REFUSED()
                } else {
                    strings::dnd::DROP_TO_ADD()
                },
                p.t1,
            );
            let what = strings::dnd::payload(payload).map(|w| Type::Lead.line(ui, &w, p.t2));
            let h = title.size().y + what.as_ref().map_or(0.0, |w| space::S2 + w.size().y);
            let mut y = c.y - h / 2.0;
            painter.galley(pos2(c.x - title.size().x / 2.0, y), title.clone(), p.t1);
            y += title.size().y + space::S2;
            if let Some(what) = what {
                painter.galley(pos2(c.x - what.size().x / 2.0, y), what, p.t2);
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
