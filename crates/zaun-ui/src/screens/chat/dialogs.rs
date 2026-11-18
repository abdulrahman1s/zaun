//! The Chat page's dialogs, queued with the others (`Dialog::Chat`; chat.html section 8
//! DIALOGS, script sections 25–26): *Add a friend* (two tabs: your link as a QR code with
//! *Copy link*, never printed; a field for theirs), *New group*, *Your profile*, *Check your
//! connection*, *Share <game> with <friend>?*, *Start a game to share it*, *Block <name>?*,
//! the blocked people, the confirmations to remove a friend or leave a group, and the
//! warning before opening a harmful site. Every dialog is 440 wide.
//!
//! [`share_flow`] and [`call_flow`] are the one place the header, the trays and the details
//! decide what Share and Call do.

use egui::{Align2, Context, CornerRadius, FontFamily, FontId, Id, Rect, Sense, Ui, pos2, vec2};
use zaun_core::config::schema::{PreferencePatch, ShareFps};
use zaun_core::model::{
    Activity, Availability, ChatProfile, ChatRef, ChatSnapshot, EntryKind, GameSummary,
};
use zaun_core::service::ChatCommand;

use super::avatar::{self, Shape};
use super::share_quality as sq;
use super::{Env, paint};
use crate::app::UiState;
use crate::bridge::{Command, UiCommand};
use crate::strings::chat as c;
use crate::strings::chat::calls as cs;
use crate::strings::chat::details as s;
use crate::theme::tokens::{layout, radius, space};
use crate::theme::{self, Type, icons};
use crate::vm::chat::{
    ChatDialog, PendingGroup, PendingShare, ProfileDraft, ShareChoice, valid_name,
};
use crate::vm::chat_call::ShareStep;
use crate::vm::chat_picture::{PictureChange, PictureEdit};
use crate::vm::dialogs::Dialog;
use crate::vm::toasts::ToastKind;
use crate::widgets::button::{Button, Variant};
use crate::widgets::choice::{PickKind, pick};
use crate::widgets::modal::{self, Action, DialogOutcome, Spec};
use crate::widgets::rows::SettingRow;
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::select::select;
use crate::widgets::text_field::TextField;
use crate::widgets::toggle::switch;

/// The dialogs' measures (`.qr`, `.invite`, `.code6`, `.promise`, `.share-g`).
mod m {
    /// The QR code's side (chat.html has 132; 168 gives the engine's short links whole
    /// modules of 3 px at 1×)…
    pub const QR: f32 = 168.0;
    /// …grown so that each module is at least this many pixels at 1× (a longer link
    /// makes a denser code; 3 px scans from a phone held at arm's length).
    pub const QR_MODULE: f32 = 3.0;
    /// Between the QR code and the text beside it.
    pub const INVITE_GAP: f32 = 16.0;
    /// Under the tabs.
    pub const TABS_BELOW: f32 = 16.0;
    /// Between the lines beside the code.
    pub const LINE_GAP: f32 = 10.0;
    /// Above a heading in a dialog's body, and under it.
    pub const HEADING: [f32; 2] = [18.0, 8.0];
    /// The code's digits: size, line height, gaps between rows and columns.
    pub const CODE: [f32; 4] = [15.0, 24.0, 6.0, 18.0];
    /// The code: groups of five, twelve of them, four to a row.
    pub const CODE_GROUPS: [usize; 3] = [5, 12, 4];
    /// The share dialog's poster and the gap beside it.
    pub const SHARE_POSTER: [f32; 2] = [76.0, 16.0];
    /// A promise: its check column, gap, line height, between promises.
    pub const PROMISE: [f32; 4] = [16.0, 10.0, 19.0, 8.0];
    /// The member avatar in New group.
    pub const PICK_AVATAR: f32 = 24.0;
    /// A blocked person's row: avatar, height.
    pub const BLOCKED_ROW: [f32; 2] = [28.0, 44.0];
    /// Games offered by Start a game.
    pub const START_GAMES: usize = 4;
    /// Their covers.
    pub const START_COVER: f32 = 24.0;
}

/// Show the Chat dialog `d` as the dialog `id`. Returns `true` once it is done.
pub fn dialog(
    ctx: &Context,
    id: Id,
    state: &mut UiState,
    env: &mut Env<'_>,
    d: ChatDialog,
) -> bool {
    match d {
        ChatDialog::AddFriend => add_friend(ctx, id, state, env),
        ChatDialog::NewGroup => new_group(ctx, id, state, env),
        ChatDialog::Profile => profile(ctx, id, state, env),
        ChatDialog::Verify(f) => verify(ctx, id, state, env, f),
        ChatDialog::Share(chat) => share(ctx, id, state, env, chat),
        ChatDialog::StartGame(chat) => start_game(ctx, id, state, env, chat),
        ChatDialog::Block(f) => block(ctx, id, state, env, f),
        ChatDialog::Blocked => blocked(ctx, id, state, env),
        ChatDialog::UnsafeLink => unsafe_link(ctx, id, state, env),
        ChatDialog::InviteToGroup(g) => invite_to_group(ctx, id, state, env, g),
        ChatDialog::GroupPicture(g) => super::picture_edit::group_dialog(ctx, id, state, env, g),
        ChatDialog::RemoveFriend(f) => {
            let name = super::name_of(env.bridge.chat(), ChatRef::Direct(f));
            confirm(
                ctx,
                id,
                &c::remove_title(&name),
                c::REMOVE_BODY(),
                c::REMOVE(),
                || ChatCommand::RemoveFriend(f),
                state,
                env,
            )
        }
        ChatDialog::LeaveGroup(g) => {
            let name = super::name_of(env.bridge.chat(), ChatRef::Group(g));
            confirm(
                ctx,
                id,
                &c::leave_title(&name),
                c::LEAVE_BODY(),
                c::LEAVE(),
                || ChatCommand::LeaveGroup(g),
                state,
                env,
            )
        }
    }
}

// ── What Share and Call do ─────────────────────────────────────────────────────────────

/// The game running now, if any (a game of the library, not an installer or tool).
fn running_game(env: &Env<'_>) -> Option<GameSummary> {
    env.bridge
        .library()
        .games
        .iter()
        .find(|g| matches!(g.activity, Activity::Running { .. }))
        .cloned()
}

/// Share your game in `chat`: stop when you share there; say so when you share elsewhere;
/// otherwise ask (*Share <game>…?*), or offer games to start when none is running.
pub(super) fn share_flow(
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
) {
    if !snap.features.share {
        return;
    }
    if let Some(sh) = &snap.live.share {
        if sh.chat == chat {
            env.bridge.send(Command::Chat(ChatCommand::StopShare));
            state.toast(s::STOPPED_SHARING(), ToastKind::Info, None, env.mono);
        } else {
            let to = paint::short_name(snap, sh.chat);
            state.toast(s::already_sharing(&to), ToastKind::Info, None, env.mono);
        }
        return;
    }
    // Streams are one to one and never in a call with the same friend: the engine holds one
    // call per friend, so a stream would end the call (ADR-0036).
    let in_call = snap.live.call.as_ref().is_some_and(|c| c.chat == chat)
        || snap.live.ringing.as_ref().is_some_and(|r| r.chat == chat);
    let why = match chat {
        ChatRef::Group(_) => Some(cs::SHARE_GROUP()),
        ChatRef::Direct(_) if in_call => Some(cs::SHARE_IN_CALL()),
        ChatRef::Direct(_) => None,
    };
    if let Some(why) = why {
        state.toast(why, ToastKind::Info, None, env.mono);
        return;
    }
    state.chat.call.share_step = ShareStep::Ask;
    let d = if running_game(env).is_some() {
        ChatDialog::Share(chat)
    } else {
        ChatDialog::StartGame(chat)
    };
    state.show_dialog(Dialog::Chat(d));
}

/// Call `chat` (or start or join its group's voice); one call at a time.
pub(super) fn call_flow(
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
) {
    if !snap.features.calls {
        return;
    }
    if let Some(call) = &snap.live.call {
        if call.chat != chat {
            state.toast(s::LEAVE_CALL_FIRST(), ToastKind::Info, None, env.mono);
        }
        return;
    }
    env.bridge.send(Command::Chat(ChatCommand::Call(chat)));
}

/// Block `friend` (they are not told): the toast offers Undo; their open chat closes.
pub(super) fn set_blocked(state: &mut UiState, env: &mut Env<'_>, friend: i64, on: bool) {
    let snap = env.bridge.chat().clone();
    let name = snap
        .friend(friend)
        .map(|f| paint::first_name(&f.name).to_owned())
        .unwrap_or_default();
    env.bridge
        .send(Command::Chat(ChatCommand::SetBlocked { friend, on }));
    if on {
        if state
            .chat
            .open
            .as_ref()
            .is_some_and(|o| o.chat == ChatRef::Direct(friend))
        {
            state.chat.close_chat();
            state.chat.focus_row = None;
        }
        let undo = UiCommand::Core(Command::Chat(ChatCommand::SetBlocked { friend, on: false }));
        state.toast(
            s::blocked_toast(&name),
            ToastKind::Success,
            Some(undo),
            env.mono,
        );
    } else {
        state.toast(
            s::unblocked_toast(&name),
            ToastKind::Success,
            None,
            env.mono,
        );
        super::open_chat(state, env, ChatRef::Direct(friend));
    }
}

// ── Parts ──────────────────────────────────────────────────────────────────────────────

/// A confirmation whose destructive action sends `cmd`; `Cancel` has focus.
#[allow(clippy::too_many_arguments)]
fn confirm(
    ctx: &Context,
    id: Id,
    title: &str,
    text: &str,
    verb: &str,
    cmd: impl FnOnce() -> ChatCommand,
    state: &mut UiState,
    env: &mut Env<'_>,
) -> bool {
    let actions = [
        Action {
            label: c::CANCEL(),
            variant: Variant::Secondary,
        },
        Action {
            label: verb,
            variant: Variant::Danger,
        },
    ];
    match modal::choose(ctx, id, Spec::new(title).text(text), &actions, 0, |_| {}) {
        DialogOutcome::Open => false,
        DialogOutcome::Chose(1) => {
            env.bridge.send(Command::Chat(cmd()));
            state.chat.close_chat();
            state.chat.focus_row = None;
            true
        }
        _ => true,
    }
}

/// Put focus in a dialog's field until it has taken it (the first passes of a dialog may
/// be sizing passes that cannot).
fn focus_once(ui: &Ui, dialog: Id, field: &egui::Response) {
    let key = dialog.with("chat.focus_pending");
    if modal::opening(ui.ctx(), dialog.with("chat.field")) {
        ui.ctx().data_mut(|d| d.insert_temp(key, true));
    }
    if ui.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(false) {
        if field.has_focus() {
            ui.ctx().data_mut(|d| d.remove_temp::<bool>(key));
        } else {
            field.request_focus();
        }
    }
}

/// A heading inside a dialog's body (`.h`): 18 above, 8 below.
fn heading(ui: &mut Ui, text: &str, first: bool) {
    let p = theme::theme(ui.ctx()).palette.clone();
    if !first {
        ui.add_space(m::HEADING[0]);
    }
    let g = Type::Heading.line(ui, text, p.t1);
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), g.size().y), Sense::hover());
    let x = if crate::i18n::dir() == crate::i18n::Dir::Rtl {
        r.right() - g.size().x
    } else {
        r.left()
    };
    ui.painter().galley(pos2(x, r.top()), g, p.t1);
    crate::a11y::labelled(ui.ctx(), resp.id, egui::Role::Heading, text);
    ui.add_space(m::HEADING[1]);
}

/// A paragraph in `style` and `color`, at the column's start.
fn para(ui: &mut Ui, text: &str, style: Type, color: egui::Color32) {
    style.paragraph(ui, text, color);
}

/// The three promises of sharing, each after an accent check.
fn promises(ui: &mut Ui, to: &str, group: bool) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let e2e = if group {
        s::PROMISE_E2E_GROUP().to_owned()
    } else {
        s::promise_e2e(to)
    };
    let [col, gap, line, between] = m::PROMISE;
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    for (i, text) in [s::PROMISE_PICTURE(), s::PROMISE_DESKTOP(), e2e.as_str()]
        .into_iter()
        .enumerate()
    {
        if i > 0 {
            ui.add_space(between);
        }
        let w = ui.available_width();
        let tw = (w - col - gap).max(1.0);
        let mut job = Type::Body.job(text, p.t2, tw, usize::MAX);
        for sct in &mut job.sections {
            sct.format.line_height = Some(line);
        }
        let g = ui.fonts_mut(|f| f.layout_job(job));
        let (r, resp) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
        crate::a11y::item(&resp, egui::Role::ListItem, text);
        let (check_x, text_x) = if rtl {
            (r.right() - col / 2.0, r.right() - col - gap - g.size().x)
        } else {
            (r.left() + col / 2.0, r.left() + col + gap)
        };
        ui.painter().text(
            pos2(check_x, r.top() + 2.0 + 7.0),
            Align2::CENTER_CENTER,
            icons::CHECK.text(false),
            icons::font(14.0),
            p.accent_text,
        );
        ui.painter().galley(pos2(text_x, r.top()), g, p.t2);
    }
}

/// A poster of `title` `w` wide in `r`'s corner.
fn poster(ui: &Ui, r: Rect, title: &str) {
    let th = theme::theme(ui.ctx());
    crate::widgets::cover::paint(
        ui.painter(),
        r,
        crate::widgets::cover::CoverSource::Placeholder,
        title,
        crate::widgets::cover::CoverLook::default(),
        &th,
    );
}

// ── Add a friend ──────────────────────────────────────────────────────────────────────

fn add_friend(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) -> bool {
    let snap = env.bridge.chat().clone();
    let p = theme::theme(ctx).palette.clone();
    // A one-time invite is made when the dialog opens without one.
    if snap.invite.is_none() && !state.chat.invite_asked {
        state.chat.invite_asked = true;
        env.bridge.send(Command::Chat(ChatCommand::CreateInvite));
    }
    let mut connect_enter = false;
    let mut address = None;
    let mut copy = None;
    let typed = state.chat.link.trim().to_owned();
    let unsaved = !typed.is_empty();
    let tab = state.chat.invite_tab.min(1);
    let shown = modal::show(
        ctx,
        id,
        Spec::new(c::ADD_FRIEND())
            .text(s::ADD_FRIEND_TEXT())
            .close_button()
            .step(tab as u64)
            .unsaved(unsaved),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut t = tab;
            ui.horizontal(|ui| {
                segmented(
                    ui,
                    s::TABS(),
                    &mut t,
                    &[
                        Segment {
                            icon: None,
                            label: s::TAB_SHARE(),
                        },
                        Segment {
                            icon: None,
                            label: s::TAB_HAVE(),
                        },
                    ],
                    false,
                );
            });
            state.chat.invite_tab = t;
            ui.add_space(m::TABS_BELOW);
            if tab == 0 {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = m::INVITE_GAP;
                    match snap.invite.as_deref() {
                        Some(l) => {
                            let side = paint::qr_side(ui.ctx(), l, m::QR, m::QR_MODULE);
                            paint::qr(ui, l, side, c::QR_INVITE());
                        }
                        None => {
                            let (r, _) = ui.allocate_exact_size(vec2(m::QR, m::QR), Sense::hover());
                            crate::widgets::skeleton::block(
                                ui,
                                r,
                                Some(CornerRadius::same(radius::POP)),
                            );
                        }
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        match snap.invite.as_deref() {
                            Some(l) => {
                                para(ui, s::SHARE_LINK_BODY(), Type::Body, p.t3);
                                ui.add_space(m::LINE_GAP);
                                let b = ui.add(Button::new(c::COPY_LINK()).icon(icons::COPY));
                                if b.clicked() {
                                    copy = Some(l.to_owned());
                                }
                                ui.add_space(m::LINE_GAP);
                                para(ui, s::SHARE_LINK_FINE(), Type::Caption, p.t3);
                            }
                            None => para(ui, c::MAKING_INVITE(), Type::Caption, p.t3),
                        }
                    });
                });
                // The reusable address: requests come through it (a switch, as before).
                ui.add_space(space::S4);
                let mut on = snap.address.is_some();
                let changed = SettingRow::new(c::ADDRESS())
                    .help(c::ADDRESS_HELP())
                    .show(ui, |ui| switch(ui, &mut on, c::ADDRESS()).changed());
                if changed {
                    address = Some(on);
                }
                if let Some(a) = snap.address.as_deref() {
                    ui.add_space(space::S2);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = m::INVITE_GAP;
                        let side = paint::qr_side(ui.ctx(), a, m::QR, m::QR_MODULE);
                        paint::qr(ui, a, side, c::QR_ADDRESS());
                        let b = ui.push_id("chat.copy.address", |ui| {
                            ui.add(Button::new(c::COPY_LINK()).icon(icons::COPY))
                        });
                        if b.inner.clicked() {
                            copy = Some(a.to_owned());
                        }
                    });
                }
            } else {
                para(ui, s::PASTE_BODY(), Type::Body, p.t3);
                ui.add_space(m::LINE_GAP);
                let r = TextField::new(&mut state.chat.link, "chat.link")
                    .hint(s::PASTE_HINT())
                    .accessible_name(s::LINK_FIELD())
                    .mono()
                    .show(ui)
                    .response;
                focus_once(ui, id.with(tab), &r);
                connect_enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            }
        },
        |ui| {
            if tab == 0 {
                (ui.add(Button::primary(c::DONE())).clicked(), false)
            } else {
                let go = ui
                    .add_enabled(!typed.is_empty(), Button::primary(c::CONNECT()))
                    .clicked();
                (false, go)
            }
        },
    );
    if let Some(link) = copy {
        env.ctx.copy_text(link);
        state.toast(c::LINK_COPIED(), ToastKind::Success, None, env.mono);
    }
    match address {
        Some(true) => env.bridge.send(Command::Chat(ChatCommand::CreateAddress)),
        Some(false) => env.bridge.send(Command::Chat(ChatCommand::DeleteAddress)),
        None => {}
    }
    let (done, connect) = shown.footer;
    let reset = |state: &mut UiState| {
        state.chat.link.clear();
        state.chat.invite_asked = false;
        state.chat.invite_tab = 0;
    };
    if (connect || connect_enter) && !typed.is_empty() {
        env.bridge.send(Command::Chat(ChatCommand::Connect(typed)));
        state.toast(s::REQUEST_SENT(), ToastKind::Success, None, env.mono);
        modal::closed(ctx, id);
        reset(state);
        return true;
    }
    if done {
        modal::closed(ctx, id);
    }
    if done || shown.close_requested {
        reset(state);
        return true;
    }
    false
}

// ── New group ─────────────────────────────────────────────────────────────────────────

fn new_group(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) -> bool {
    let snap = env.bridge.chat().clone();
    let p = theme::theme(ctx).palette.clone();
    let friends: Vec<_> = snap
        .friends
        .iter()
        .filter(|f| !f.blocked && f.ready)
        .cloned()
        .collect();
    let ticked = !state.chat.group_pick.is_empty();
    let unsaved = ticked || !state.chat.group_name.trim().is_empty();
    let shown = modal::show(
        ctx,
        id,
        Spec::new(c::NEW_GROUP()).unsaved(unsaved),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let r = TextField::new(&mut state.chat.group_name, "chat.group.name")
                .hint(c::GROUP_NAME())
                .accessible_name(c::GROUP_NAME())
                .show(ui)
                .response;
            focus_once(ui, id, &r);
            heading(ui, s::ADD_FRIENDS(), false);
            if friends.is_empty() {
                para(ui, s::NO_FRIENDS(), Type::Body, p.t3);
            }
            for f in &friends {
                let on = state.chat.group_pick.contains(&f.id);
                let (resp, ()) = pick(ui, PickKind::Check, on, &f.name, |ui| {
                    let w = ui.available_width();
                    let (r, _) = ui.allocate_exact_size(vec2(w, m::PICK_AVATAR), Sense::hover());
                    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
                    let g = Type::Body.galley(ui, &f.name, p.t1, w - m::PICK_AVATAR - space::S2, 1);
                    let (tx, ax) = if rtl {
                        (r.right() - g.size().x, r.left())
                    } else {
                        (r.left(), r.right() - m::PICK_AVATAR)
                    };
                    ui.painter()
                        .galley(pos2(tx, r.center().y - g.size().y / 2.0), g, p.t1);
                    avatar::pictured(
                        ui.painter(),
                        Rect::from_min_size(
                            pos2(ax, r.top()),
                            vec2(m::PICK_AVATAR, m::PICK_AVATAR),
                        ),
                        &f.name,
                        f.paper,
                        Shape::Person,
                        f.picture.as_ref(),
                    );
                });
                if resp.clicked() {
                    if on {
                        state.chat.group_pick.retain(|x| *x != f.id);
                    } else {
                        state.chat.group_pick.push(f.id);
                    }
                }
            }
        },
        |ui| {
            let go = ui
                .add_enabled(ticked, Button::primary(s::CREATE_GROUP()))
                .clicked();
            let cancel = ui.add(Button::new(c::CANCEL())).clicked();
            (go, cancel)
        },
    );
    let (go, cancel) = shown.footer;
    if go && ticked {
        let picked = std::mem::take(&mut state.chat.group_pick);
        let name = valid_name(&state.chat.group_name).unwrap_or_else(|| {
            let firsts: Vec<&str> = picked
                .iter()
                .take(3)
                .filter_map(|id| snap.friend(*id))
                .map(|f| paint::first_name(&f.name))
                .collect();
            s::and_list(&firsts)
        });
        env.bridge
            .send(Command::Chat(ChatCommand::NewGroup(name.clone())));
        state.chat.pending_group = Some(PendingGroup {
            name,
            friends: picked,
            before: snap.groups.iter().map(|g| g.id).collect(),
            asked: env.mono,
        });
        state.chat.group_name.clear();
        modal::closed(ctx, id);
        return true;
    }
    if cancel {
        modal::closed(ctx, id);
    }
    if cancel || shown.close_requested {
        state.chat.group_name.clear();
        state.chat.group_pick.clear();
        return true;
    }
    false
}

/// Keep `[chat] name` in step with the profile.
fn save_name(env: &mut Env<'_>, name: &str) {
    let mut next = (**env.bridge.prefs()).clone();
    next.chat.name = name.to_owned();
    env.bridge.set_prefs_preview(next);
}

// ── Invite to a group ─────────────────────────────────────────────────────────────────

/// Friends to show in Invite to a group before the list gets a search field.
const INVITE_SEARCH_FROM: usize = 8;

fn invite_to_group(
    ctx: &Context,
    id: Id,
    state: &mut UiState,
    env: &mut Env<'_>,
    group: i64,
) -> bool {
    let snap = env.bridge.chat().clone();
    let Some(g) = snap.group(group).cloned() else {
        return true;
    };
    let p = theme::theme(ctx).palette.clone();
    // Each time it opens it starts on Friends with nobody ticked.
    if modal::opening(ctx, id.with("chat.invite.fresh")) {
        state.chat.group_invite = Default::default();
    }
    // Friends who are members already (or invited): ticked and off.
    let members: Vec<i64> = g.member_list.iter().filter_map(|m| m.friend).collect();
    let friends: Vec<_> = snap
        .friends
        .iter()
        .filter(|f| !f.blocked && f.ready)
        .cloned()
        .collect();
    let open_count = friends.iter().filter(|f| !members.contains(&f.id)).count();
    let vm = &mut state.chat.group_invite;
    vm.pick.retain(|f| !members.contains(f));
    let ticked = !vm.pick.is_empty();
    if g.link.is_some() {
        vm.link_asked = false;
    }
    let tab = vm.tab.min(1);
    let mut link_act = None;
    let mut copy = None;
    let title = s::invite_title(&g.name);
    let shown = modal::show(
        ctx,
        id,
        Spec::new(&title)
            .close_button()
            .step(tab as u64)
            .unsaved(ticked),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut t = tab;
            ui.horizontal(|ui| {
                segmented(
                    ui,
                    s::INVITE_TABS(),
                    &mut t,
                    &[
                        Segment {
                            icon: None,
                            label: s::INVITE_TAB_FRIENDS(),
                        },
                        Segment {
                            icon: None,
                            label: s::INVITE_TAB_LINK(),
                        },
                    ],
                    false,
                );
            });
            vm.tab = t;
            ui.add_space(m::TABS_BELOW);
            if tab == 1 {
                link_act = group_link(ui, &g, vm.link_asked, &mut copy, &p);
                return;
            }
            if friends.len() > INVITE_SEARCH_FROM {
                let r = TextField::new(&mut vm.query, "chat.invite.search")
                    .search()
                    .hint(s::INVITE_SEARCH())
                    .accessible_name(s::INVITE_SEARCH())
                    .show(ui)
                    .response;
                focus_once(ui, id, &r);
                ui.add_space(space::S2);
            }
            if open_count == 0 {
                para(ui, s::INVITE_NOBODY(), Type::Body, p.t3);
                ui.add_space(space::S2);
            }
            let q = vm.query.trim().to_lowercase();
            let shown: Vec<_> = friends
                .iter()
                .filter(|f| q.is_empty() || f.name.to_lowercase().contains(&q))
                .collect();
            if shown.is_empty() {
                para(ui, s::INVITE_NO_MATCH(), Type::Body, p.t3);
            }
            for f in shown {
                let member = members.contains(&f.id);
                let on = member || vm.pick.contains(&f.id);
                let clicked = ui
                    .add_enabled_ui(!member, |ui| {
                        friend_pick(ui, f, on, member.then(s::IN_THE_GROUP), &p)
                    })
                    .inner;
                if clicked && !member {
                    if on {
                        vm.pick.retain(|x| *x != f.id);
                    } else {
                        vm.pick.push(f.id);
                    }
                }
            }
        },
        |ui| {
            if tab == 1 {
                return (false, ui.add(Button::primary(c::DONE())).clicked());
            }
            let go = ui
                .add_enabled(ticked, Button::primary(s::INVITE_SEND()))
                .clicked();
            let cancel = ui.add(Button::new(c::CANCEL())).clicked();
            (go, cancel)
        },
    );
    if let Some(link) = copy {
        env.ctx.copy_text(link);
        state.toast(c::LINK_COPIED(), ToastKind::Success, None, env.mono);
    }
    match link_act {
        Some(true) => {
            state.chat.group_invite.link_asked = true;
            env.bridge
                .send(Command::Chat(ChatCommand::CreateGroupLink(group)));
        }
        Some(false) => {
            state.chat.group_invite.link_asked = false;
            env.bridge
                .send(Command::Chat(ChatCommand::DeleteGroupLink(group)));
        }
        None => {}
    }
    let (go, cancel) = shown.footer;
    if go && ticked {
        let picked = std::mem::take(&mut state.chat.group_invite.pick);
        for &friend in &picked {
            env.bridge
                .send(Command::Chat(ChatCommand::AddMember { group, friend }));
        }
        state.toast(
            s::invited(picked.len(), &g.name),
            ToastKind::Success,
            None,
            env.mono,
        );
    }
    if go || cancel {
        modal::closed(ctx, id);
    }
    let done = go || cancel || shown.close_requested;
    if done {
        state.chat.group_invite = Default::default();
    }
    done
}

/// The group's own link: "Make a link" while there is none; then its QR code, Copy link,
/// what it does, and Delete link. The link itself is never printed. Returns
/// `Some(true)` to make it, `Some(false)` to delete it; a copy goes into `copy`.
fn group_link(
    ui: &mut Ui,
    g: &zaun_core::model::ChatGroup,
    asked: bool,
    copy: &mut Option<String>,
    p: &theme::palette::Palette,
) -> Option<bool> {
    let mut act = None;
    match g.link.as_deref() {
        None => {
            para(ui, s::GROUP_LINK_NONE(), Type::Body, p.t3);
            ui.add_space(m::LINE_GAP);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!asked, Button::new(s::MAKE_LINK()).busy(asked))
                    .clicked()
                {
                    act = Some(true);
                }
            });
        }
        Some(link) => {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = m::INVITE_GAP;
                let side = paint::qr_side(ui.ctx(), link, m::QR, m::QR_MODULE);
                paint::qr(ui, link, side, s::QR_GROUP());
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    para(ui, &s::group_link_body(&g.name), Type::Body, p.t3);
                    ui.add_space(m::LINE_GAP);
                    if ui
                        .add(Button::new(c::COPY_LINK()).icon(icons::COPY))
                        .clicked()
                    {
                        *copy = Some(link.to_owned());
                    }
                });
            });
            ui.add_space(space::S4);
            let w = ui.available_width();
            let (row, _) =
                ui.allocate_exact_size(vec2(w, theme::tokens::control::BUTTON), Sense::hover());
            let label = s::DELETE_LINK();
            let bw = Type::BodyStrong.line(ui, label, p.t1).size().x
                + theme::tokens::control::BUTTON_PAD * 2.0;
            // The coral text button hangs out by its padding, so its words stand on the
            // column's edge.
            let x = if crate::i18n::dir() == crate::i18n::Dir::Rtl {
                row.right() + theme::tokens::control::BUTTON_PAD - bw
            } else {
                row.left() - theme::tokens::control::BUTTON_PAD
            };
            let r =
                Rect::from_min_size(pos2(x, row.top()), vec2(bw, theme::tokens::control::BUTTON));
            if paint::place(ui, r, Button::new(label).variant(Variant::Warn)).clicked() {
                act = Some(false);
            }
        }
    }
    act
}

/// A friend to tick (`.pick` with a checkbox): the name, and at the end `note` (quiet) or
/// the avatar. Returns whether it was clicked.
fn friend_pick(
    ui: &mut Ui,
    f: &zaun_core::model::Friend,
    on: bool,
    note: Option<&str>,
    p: &theme::palette::Palette,
) -> bool {
    let (resp, ()) = pick(ui, PickKind::Check, on, &f.name, |ui| {
        let w = ui.available_width();
        let (r, _) = ui.allocate_exact_size(vec2(w, m::PICK_AVATAR), Sense::hover());
        let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
        let note_g = note.map(|n| Type::Caption.line(ui, n, p.t3));
        let end_w = note_g.as_ref().map_or(0.0, |g| g.size().x + space::S2) + m::PICK_AVATAR;
        let name = super::emoji::Line::new(
            ui,
            Type::Body,
            &f.name,
            p.t1,
            (w - end_w - space::S2).max(1.0),
        );
        let (tx, ax) = if rtl {
            (r.right() - name.size().x, r.left())
        } else {
            (r.left(), r.right() - m::PICK_AVATAR)
        };
        let ny = r.center().y - name.size().y / 2.0;
        name.paint(ui.painter(), pos2(tx, ny), p.t1);
        avatar::pictured(
            ui.painter(),
            Rect::from_min_size(pos2(ax, r.top()), vec2(m::PICK_AVATAR, m::PICK_AVATAR)),
            &f.name,
            f.paper,
            Shape::Person,
            f.picture.as_ref(),
        );
        if let Some(g) = note_g {
            let nx = if rtl {
                ax + m::PICK_AVATAR + space::S2
            } else {
                ax - space::S2 - g.size().x
            };
            ui.painter()
                .galley(pos2(nx, r.center().y - g.size().y / 2.0), g, p.t3);
        }
    });
    if let Some(n) = note {
        crate::a11y::describe(&resp, n);
    }
    resp.clicked()
}

// ── Your profile ──────────────────────────────────────────────────────────────────────

fn profile(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) -> bool {
    let snap = env.bridge.chat().clone();
    let current = snap.profile.clone().unwrap_or_default();
    // The pair the avatar shows now: the one chosen, or the one the name gives.
    let shown_pair = current
        .paper
        .unwrap_or_else(|| avatar_pair_of(&current.name));
    if state.chat.profile.is_none() || state.chat.picture.whose != Some(None) {
        state.chat.picture = PictureEdit::new(None);
    }
    // The picture's crop step is a step of this dialog (pictures, ADR-0038).
    let draft = state.chat.profile.get_or_insert_with(|| ProfileDraft {
        name: current.name.clone(),
        about: current.about.clone(),
        paper: shown_pair,
    });
    if super::picture_edit::crop_step(ctx, id, &mut state.chat.picture) {
        return false;
    }
    let valid = valid_name(&draft.name);
    let changed =
        draft.name != current.name || draft.about != current.about || draft.paper != shown_pair;
    let picture = &mut state.chat.picture;
    let unsaved = changed || picture.change != PictureChange::Keep;
    let busy = picture.loading.is_some();
    let shown = modal::show(
        ctx,
        id,
        Spec::new(s::PROFILE_TITLE())
            .text(s::PROFILE_TEXT())
            .step(0)
            .unsaved(unsaved),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let colour = super::picture_edit::row(
                ui,
                &mut state.picks,
                picture,
                &draft.name,
                Some(draft.paper),
                Shape::Person,
                current.picture.as_ref(),
                true,
            );
            if colour {
                draft.paper = (draft.paper + 1) % PAIRS;
            }
            heading(ui, s::NAME(), false);
            let r = TextField::new(&mut draft.name, "chat.profile.name")
                .accessible_name(s::NAME())
                .help(c::NAME_RULE())
                .show(ui)
                .response;
            focus_once(ui, id, &r);
            heading(ui, s::ABOUT_YOU(), false);
            TextField::new(&mut draft.about, "chat.profile.about")
                .accessible_name(s::ABOUT_YOU())
                .multiline()
                .show(ui);
        },
        |ui| {
            let save = ui
                .add_enabled(valid.is_some() && !busy, Button::primary(c::SAVE()))
                .clicked();
            let cancel = ui.add(Button::new(c::CANCEL())).clicked();
            (save, cancel)
        },
    );
    let (save, cancel) = shown.footer;
    if save && let Some(name) = valid {
        let d = state.chat.profile.take().unwrap_or_else(|| ProfileDraft {
            name: name.clone(),
            about: String::new(),
            paper: 0,
        });
        let about: String = d.about.trim().chars().take(ABOUT_MAX).collect();
        // The engine renders a picture's `/_profile` from the profile it last confirmed, so a
        // removal goes before a rename. A new picture goes after it: its file is read and
        // encoded first, and the preview's `SetProfile` replaces the whole profile.
        let picture = std::mem::take(&mut state.chat.picture);
        let removed = picture.change == PictureChange::Remove;
        let cmd = picture.commands(None, super::crop::CIRCLE);
        let (before, after) = if removed { (cmd, None) } else { (None, cmd) };
        if let Some(cmd) = before {
            env.bridge.send(Command::Chat(cmd));
        }
        if changed {
            save_name(env, &name);
            env.bridge
                .send(Command::Chat(ChatCommand::SetProfile(ChatProfile {
                    name,
                    about,
                    // A colour that was never chosen stays the name's own.
                    paper: (current.paper.is_some() || d.paper != shown_pair).then_some(d.paper),
                    status: current.status,
                    // The picture has its own commands (ADR-0038); this keeps it.
                    picture: current.picture.clone().filter(|_| !removed),
                })));
        }
        if let Some(cmd) = after {
            env.bridge.send(Command::Chat(cmd));
        }
        modal::closed(ctx, id);
        return true;
    }
    if cancel {
        modal::closed(ctx, id);
    }
    if cancel || shown.close_requested {
        state.chat.profile = None;
        state.chat.picture = PictureEdit::default();
        return true;
    }
    false
}

/// The number of paper-and-ink pairs.
const PAIRS: u8 = 10;
/// The longest line about yourself, in characters.
const ABOUT_MAX: usize = 160;

/// The pair a name gets when none was chosen (the same as the avatar's own choice).
fn avatar_pair_of(name: &str) -> u8 {
    let target = avatar::paper_of(name, None);
    (0..PAIRS)
        .find(|i| avatar::paper_of(name, Some(*i)) == target)
        .unwrap_or(0)
}

// ── Check your connection ─────────────────────────────────────────────────────────────

fn verify(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>, friend: i64) -> bool {
    let snap = env.bridge.chat().clone();
    if !snap.features.verify {
        return true;
    }
    if state.chat.code_asked != Some(friend) {
        state.chat.code_asked = Some(friend);
        env.bridge
            .send(Command::Chat(ChatCommand::ShowCode(friend)));
    }
    let f = snap.friend(friend);
    let first = f
        .map(|f| paint::first_name(&f.name).to_owned())
        .unwrap_or_default();
    let verified = f.is_some_and(|f| f.verified);
    let code = snap
        .code
        .as_ref()
        .filter(|(who, _)| *who == friend)
        .map(|(_, code)| code.clone());
    let p = theme::theme(ctx).palette.clone();
    let groups = code.as_deref().map(code_groups);
    let title = s::verify_title(&first);
    let text = s::verify_text(&first);
    let shown = modal::show(
        ctx,
        id,
        Spec::new(&title).text(&text),
        |ui| match &groups {
            Some(g) => code_grid(ui, g, p.t1),
            None => para(ui, s::CODE_LOADING(), Type::Body, p.t3),
        },
        |ui| {
            if verified {
                (false, ui.add(Button::new(s::CLOSE())).clicked())
            } else {
                let ok = ui
                    .add_enabled(code.is_some(), Button::primary(s::THEY_MATCH()))
                    .clicked();
                let not_now = ui.add(Button::new(s::NOT_NOW())).clicked();
                (ok, not_now)
            }
        },
    );
    let (ok, close) = shown.footer;
    if ok && let Some(code) = code {
        env.bridge
            .send(Command::Chat(ChatCommand::Verify { friend, code }));
        state.toast(
            s::verified_toast(&first),
            ToastKind::Success,
            None,
            env.mono,
        );
    }
    if ok || close {
        modal::closed(ctx, id);
    }
    if ok || close || shown.close_requested {
        state.chat.code_asked = None;
        return true;
    }
    false
}

/// The code's digits in twelve groups of five.
fn code_groups(code: &str) -> Vec<String> {
    let [size, count, _] = m::CODE_GROUPS;
    let digits: Vec<char> = code.chars().filter(char::is_ascii_digit).collect();
    digits
        .chunks(size)
        .take(count)
        .map(|c| c.iter().collect())
        .collect()
}

/// The groups four to a row in mono 15 / 24, 6 between rows and 18 between columns.
fn code_grid(ui: &mut Ui, groups: &[String], color: egui::Color32) {
    let [size, line, row_gap, col_gap] = m::CODE;
    let per_row = m::CODE_GROUPS[2];
    let font = FontId::new(size, FontFamily::Monospace);
    let galleys: Vec<_> = groups
        .iter()
        .map(|g| ui.painter().layout_no_wrap(g.clone(), font.clone(), color))
        .collect();
    let cell = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max);
    let rows = galleys.len().div_ceil(per_row);
    let h = rows as f32 * line + rows.saturating_sub(1) as f32 * row_gap;
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    crate::a11y::item(
        &resp,
        egui::Role::Label,
        &format!("{}: {}", s::CODE_SPOKEN(), groups.join(" ")),
    );
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    for (i, g) in galleys.into_iter().enumerate() {
        let (row, col) = (i / per_row, i % per_row);
        // Digits read left to right in every language; the grid starts at the start edge.
        let from = col as f32 * (cell + col_gap);
        let x = if rtl {
            r.right() - (per_row as f32) * (cell + col_gap) + col_gap + from
        } else {
            r.left() + from
        };
        let y = r.top() + row as f32 * (line + row_gap) + (line - g.size().y) / 2.0;
        ui.painter().galley(pos2(x, y), g, color);
    }
}

// ── Share ─────────────────────────────────────────────────────────────────────────────

/// Who a share goes to: a friend's first name or a group's name, and whether it is a group.
fn share_to(snap: &ChatSnapshot, chat: ChatRef) -> (String, bool) {
    (
        paint::short_name(snap, chat),
        matches!(chat, ChatRef::Group(_)),
    )
}

fn share(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>, chat: ChatRef) -> bool {
    let snap = env.bridge.chat().clone();
    let step = state.chat.call.share_step;
    // The window was picked (the share runs), or the picker was closed or failed (a toast
    // or the error says so): the dialog's part is over.
    let picked = snap.live.share.as_ref().is_some_and(|sh| sh.chat == chat);
    if step == ShareStep::Over || (step == ShareStep::Waiting && picked) {
        state.chat.call.share_step = ShareStep::Ask;
        state.chat.share_choice = None;
        modal::closed(ctx, id);
        return true;
    }
    let Some(game) = running_game(env) else {
        // The game ended while the dialog was queued: offer one to start instead.
        state.chat.call.share_step = ShareStep::Ask;
        state.show_dialog(Dialog::Chat(ChatDialog::StartGame(chat)));
        return true;
    };
    if step != ShareStep::Ask {
        return share_choose(ctx, id, state, env, chat, &game.name);
    }
    let (to, group) = share_to(&snap, chat);
    let prefs = env.bridge.prefs().clone();
    let saved = ShareChoice {
        quality: sq::HEIGHTS
            .iter()
            .position(|q| *q == prefs.chat.share_quality)
            .unwrap_or(1),
        fps: usize::from(prefs.chat.share_fps == ShareFps::F60),
        sound: prefs.chat.share_sound,
    };
    let choice = state.chat.share_choice.get_or_insert(saved);
    let title = s::share_title(&game.name, &to);
    let shown = modal::show(
        ctx,
        id,
        Spec::new(&title),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let [pw, gap] = m::SHARE_POSTER;
            let w = ui.available_width();
            let top = ui.cursor().min;
            let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
            let px = if rtl { top.x + w - pw } else { top.x };
            let pr = Rect::from_min_size(pos2(px, top.y), vec2(pw, pw * layout::COVER_ASPECT));
            poster(ui, pr, &game.name);
            let tx = if rtl { top.x } else { top.x + pw + gap };
            let mut col = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_size(
                        pos2(tx, top.y + 4.0),
                        vec2(w - pw - gap, f32::INFINITY),
                    ))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            col.spacing_mut().item_spacing.y = 0.0;
            promises(&mut col, &to, group);
            let h = col.min_rect().height().max(pr.height());
            ui.allocate_space(vec2(w, h));
            ui.add_space(space::S4);
            // Picture and Frame rate are selects (notes J): choices, said once each.
            let sizes: Vec<String> = sq::HEIGHTS.into_iter().map(sq::height_label).collect();
            let sizes: Vec<&str> = sizes.iter().map(String::as_str).collect();
            SettingRow::new(cs::PICTURE())
                .help(cs::PICTURE_HELP())
                .show(ui, |ui| {
                    select(
                        ui,
                        cs::PICTURE(),
                        None,
                        None,
                        &mut choice.quality,
                        &sizes,
                        false,
                    );
                });
            let rates: Vec<String> = sq::RATES.into_iter().map(cs::fps).collect();
            let rates: Vec<&str> = rates.iter().map(String::as_str).collect();
            SettingRow::new(cs::FRAME_RATE()).show(ui, |ui| {
                select(
                    ui,
                    cs::FRAME_RATE(),
                    None,
                    None,
                    &mut choice.fps,
                    &rates,
                    false,
                );
            });
            // Streams carry no sound yet: the switch stays off and says why.
            SettingRow::new(s::INCLUDE_SOUND())
                .enabled(false)
                .show(ui, |ui| {
                    let mut off = false;
                    let r = switch(ui, &mut off, s::INCLUDE_SOUND());
                    crate::widgets::tooltip::disabled(r, cs::NO_SOUND());
                });
        },
        |ui| {
            let go = ui.add(Button::primary(s::SHARE_GAME())).clicked();
            let cancel = ui.add(Button::new(c::CANCEL())).clicked();
            (go, cancel)
        },
    );
    let (go, cancel) = shown.footer;
    if go {
        // The choice is kept for next time and is where the share starts
        // (`[chat] share_quality`, `share_fps`, `share_sound`).
        let chosen = state.chat.share_choice.unwrap_or(saved);
        if chosen != saved {
            let quality = sq::HEIGHTS.get(chosen.quality).copied().unwrap_or_default();
            let fps = if chosen.fps == 0 {
                ShareFps::F30
            } else {
                ShareFps::F60
            };
            let mut next = (*prefs).clone();
            next.chat.share_quality = quality;
            next.chat.share_fps = fps;
            next.chat.share_sound = chosen.sound;
            let patch = PreferencePatch::new()
                .set("chat.share_quality", quality.as_str())
                .set("chat.share_fps", fps.as_str())
                .set("chat.share_sound", chosen.sound);
            env.bridge.send(UiCommand::SetPreference {
                patch,
                preview: Box::new(next.clone()),
            });
            env.bridge.set_prefs_preview(next);
        }
        // Next: say which window to pick before the desktop asks.
        state.chat.call.share_step = ShareStep::Choose;
        return false;
    }
    if cancel {
        modal::closed(ctx, id);
    }
    let done = cancel || shown.close_requested;
    if done {
        state.chat.share_choice = None;
    }
    done
}

/// The share dialog's second step: "Choose <game> in the next window", then Continue opens
/// the desktop's window picker and waits for it ("Waiting for your desktop…").
fn share_choose(
    ctx: &Context,
    id: Id,
    state: &mut UiState,
    env: &mut Env<'_>,
    chat: ChatRef,
    game: &str,
) -> bool {
    let p = theme::theme(ctx).palette.clone();
    let waiting = state.chat.call.share_step == ShareStep::Waiting;
    let title = cs::choose_title(game);
    let text = cs::choose_text(game);
    let shown = modal::show(
        ctx,
        id,
        Spec::new(&title).step(1),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let [pw, gap] = m::SHARE_POSTER;
            let w = ui.available_width();
            let top = ui.cursor().min;
            let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
            let px = if rtl { top.x + w - pw } else { top.x };
            let pr = Rect::from_min_size(pos2(px, top.y), vec2(pw, pw * layout::COVER_ASPECT));
            poster(ui, pr, game);
            let tx = if rtl { top.x } else { top.x + pw + gap };
            let tw = w - pw - gap;
            let g = Type::Prose.galley(ui, &text, p.t2, tw, usize::MAX);
            let gx = if rtl { tx + tw - g.size().x } else { tx };
            let h = g.size().y.max(pr.height());
            ui.painter().galley(pos2(gx, top.y), g, p.t2);
            let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
            crate::a11y::item(
                &ui.interact(r, id.with("text"), Sense::hover()),
                egui::Role::Label,
                &text,
            );
        },
        |ui| {
            let label = if waiting {
                cs::WAITING()
            } else {
                cs::CONTINUE()
            };
            let go = ui.add(Button::primary(label).busy(waiting)).clicked() && !waiting;
            let cancel = ui.add(Button::new(c::CANCEL())).clicked();
            (go, cancel)
        },
    );
    let (go, cancel) = shown.footer;
    if go {
        // The desktop's picker opens now; nothing is sent until a window is picked.
        env.bridge
            .send(Command::Chat(ChatCommand::StartShare(chat)));
        state.chat.call.share_step = ShareStep::Waiting;
    }
    if cancel || shown.close_requested {
        if waiting {
            env.bridge.send(Command::Chat(ChatCommand::StopShare));
        }
        state.chat.call.share_step = ShareStep::Ask;
        state.chat.share_choice = None;
        if cancel {
            modal::closed(ctx, id);
        }
        return true;
    }
    false
}

/// A failure while the desktop's window picker was open ends the wait: a closed picker says
/// so in a toast (returns `true`: handled), anything else shows as an error.
pub fn share_failed(
    state: &mut UiState,
    code: zaun_core::error::ErrorCode,
    mono: zaun_core::clock::MonoInstant,
) -> bool {
    use zaun_core::error::ErrorCode as E;
    if state.chat.call.share_step != ShareStep::Waiting {
        return false;
    }
    match code {
        E::ChtShareRefused => {
            state.chat.call.share_step = ShareStep::Over;
            state.toast(cs::NOT_STARTED(), ToastKind::Info, None, mono);
            true
        }
        E::ChtCaptureFailed | E::ChtStreamFailed | E::ChtNoGameRunning => {
            state.chat.call.share_step = ShareStep::Over;
            false
        }
        _ => false,
    }
}

fn start_game(
    ctx: &Context,
    id: Id,
    state: &mut UiState,
    env: &mut Env<'_>,
    chat: ChatRef,
) -> bool {
    let snap = env.bridge.chat().clone();
    let p = theme::theme(ctx).palette.clone();
    let (to, group) = share_to(&snap, chat);
    let mut games: Vec<GameSummary> = env
        .bridge
        .library()
        .games
        .iter()
        .filter(|g| {
            g.kind == EntryKind::Game
                && !g.hidden
                && g.availability == Availability::Present
                && g.last_played_at.is_some()
                && matches!(g.activity, Activity::Idle)
        })
        .cloned()
        .collect();
    // Most recently played first.
    games.sort_by_key(|g| std::cmp::Reverse(g.last_played_at));
    games.truncate(m::START_GAMES);
    let text = if games.is_empty() {
        s::START_TEXT_EMPTY().to_owned()
    } else {
        s::start_text(&to)
    };
    let mut chosen = None;
    let (now, offset) = (env.now, env.utc_offset_s);
    let shown = modal::show(
        ctx,
        id,
        Spec::new(s::START_TITLE())
            .text(&text)
            .close_button()
            .no_actions(),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for g in &games {
                if start_row(ui, g, now, offset, &p).clicked() {
                    chosen = Some(g.clone());
                }
            }
            ui.add_space(space::S4);
            promises(ui, &to, group);
        },
        |_| (),
    );
    if let Some(g) = chosen {
        modal::closed(ctx, id);
        state.chat.pending_share = Some(PendingShare {
            chat,
            game: g.id,
            started: false,
            asked: env.mono,
        });
        crate::screens::actions::play(state, env, &g);
        return true;
    }
    shown.close_requested
}

/// A game to start (`.pick`): its cover, title and when it was played, and "Play and share".
fn start_row(
    ui: &mut Ui,
    g: &GameSummary,
    now: zaun_core::clock::Timestamp,
    offset: i32,
    p: &theme::palette::Palette,
) -> egui::Response {
    let th = theme::theme(ui.ctx());
    let w = ui.available_width();
    let h = 46.0;
    let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let resp = ui.interact(r, Id::unique(("chat.start", g.id)), Sense::click());
    crate::a11y::button(&resp, &s::play_and_share_game(&g.name));
    let hover = crate::widgets::hover_t(&resp, &th);
    let corner = CornerRadius::same(radius::ROW);
    ui.painter()
        .rect_filled(r, corner, theme::palette::fade(p.o1, hover));
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let inner = r.shrink2(vec2(10.0, 5.0));
    let cw = m::START_COVER;
    let ch = cw * layout::COVER_ASPECT;
    let cx = if rtl {
        inner.right() - cw
    } else {
        inner.left()
    };
    poster(
        ui,
        Rect::from_min_size(pos2(cx, inner.center().y - ch / 2.0), vec2(cw, ch)),
        &g.name,
    );
    let act = Type::Body.line(ui, s::PLAY_AND_SHARE(), p.t3);
    let tw = inner.width() - cw - 10.0 - act.size().x - 10.0;
    let title = Type::Body.galley(ui, &g.name, p.t1, tw.max(1.0), 1);
    let when = g
        .last_played_at
        .map(|t| crate::strings::details::day_label(t, now, offset))
        .unwrap_or_default();
    let sub = Type::Caption.galley(ui, &when, p.t3, tw.max(1.0), 1);
    let th_h = title.size().y + sub.size().y;
    let ty = inner.center().y - th_h / 2.0;
    let x = |gw: f32| {
        if rtl {
            inner.right() - cw - 10.0 - gw
        } else {
            inner.left() + cw + 10.0
        }
    };
    ui.painter()
        .galley(pos2(x(title.size().x), ty), title.clone(), p.t1);
    ui.painter()
        .galley(pos2(x(sub.size().x), ty + title.size().y), sub, p.t3);
    let ax = if rtl {
        inner.left()
    } else {
        inner.right() - act.size().x
    };
    ui.painter()
        .galley(pos2(ax, inner.center().y - act.size().y / 2.0), act, p.t3);
    if resp.has_focus() {
        crate::widgets::focus_ring(ui.painter(), r, corner, &th);
    }
    resp
}

// ── Blocking ──────────────────────────────────────────────────────────────────────────

fn block(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>, friend: i64) -> bool {
    let snap = env.bridge.chat().clone();
    if !snap.features.block {
        return true;
    }
    let first = snap
        .friend(friend)
        .map(|f| paint::first_name(&f.name).to_owned())
        .unwrap_or_default();
    let actions = [
        Action {
            label: c::CANCEL(),
            variant: Variant::Secondary,
        },
        Action {
            label: s::BLOCK(),
            variant: Variant::Danger,
        },
    ];
    let title = s::block_title(&first);
    let text = s::block_text(&first);
    match modal::choose(ctx, id, Spec::new(&title).text(&text), &actions, 0, |_| {}) {
        DialogOutcome::Open => false,
        DialogOutcome::Chose(1) => {
            set_blocked(state, env, friend, true);
            true
        }
        _ => true,
    }
}

fn blocked(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) -> bool {
    let snap = env.bridge.chat().clone();
    let p = theme::theme(ctx).palette.clone();
    let people: Vec<_> = snap.friends.iter().filter(|f| f.blocked).cloned().collect();
    let mut unblock = None;
    let shown = modal::show(
        ctx,
        id,
        Spec::new(s::BLOCKED_TITLE())
            .text(s::BLOCKED_TEXT())
            .close_button()
            .no_actions(),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if people.is_empty() {
                para(ui, s::BLOCKED_NONE(), Type::Body, p.t3);
            }
            let [av, h] = m::BLOCKED_ROW;
            let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
            for f in &people {
                let w = ui.available_width();
                let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
                let label = s::UNBLOCK();
                let bw = Type::BodyStrong.line(ui, label, p.t2).size().x
                    + theme::tokens::control::BUTTON_PAD * 2.0;
                let (ax, bx) = if rtl {
                    (r.right() - av, r.left() - 10.0)
                } else {
                    (r.left(), r.right() - bw + 10.0)
                };
                avatar::pictured(
                    ui.painter(),
                    Rect::from_min_size(pos2(ax, r.center().y - av / 2.0), vec2(av, av)),
                    &f.name,
                    f.paper,
                    Shape::Person,
                    f.picture.as_ref(),
                );
                let g = Type::Body.galley(ui, &f.name, p.t1, w - av - bw - 20.0, 1);
                let tx = if rtl {
                    r.right() - av - 12.0 - g.size().x
                } else {
                    r.left() + av + 12.0
                };
                ui.painter()
                    .galley(pos2(tx, r.center().y - g.size().y / 2.0), g, p.t1);
                let br = Rect::from_min_size(
                    pos2(bx, r.center().y - theme::tokens::control::BUTTON / 2.0),
                    vec2(bw, theme::tokens::control::BUTTON),
                );
                let b = ui.push_id(("chat.unblock", f.id), |ui| {
                    paint::place(
                        ui,
                        br,
                        Button::ghost(label).accessible_name(s::unblock_name(&f.name)),
                    )
                });
                if b.inner.clicked() {
                    unblock = Some(f.id);
                }
            }
        },
        |_| (),
    );
    if let Some(f) = unblock {
        modal::closed(ctx, id);
        set_blocked(state, env, f, false);
        return true;
    }
    shown.close_requested
}

// ── A link that may be harmful (the services agent's copy) ────────────────────────────────────────────────────────

fn unsafe_link(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) -> bool {
    use crate::strings::chat::services as sv;
    let Some(link) = state.chat.link_to_open.clone() else {
        return true;
    };
    let host = zaun_core::linkcheck::WebLink::parse(&link)
        .map(|l| l.host().to_owned())
        .unwrap_or_else(|| link.clone());
    let unchecked = state.chat.link_unchecked;
    let (title, text) = if unchecked {
        (sv::UNCHECKED_TITLE(), sv::unchecked_text(&host))
    } else {
        (sv::UNSAFE_TITLE(), sv::unsafe_text(&host))
    };
    let actions = [
        Action {
            label: sv::CANCEL(),
            variant: Variant::Secondary,
        },
        Action {
            label: sv::OPEN_ANYWAY(),
            variant: Variant::Danger,
        },
    ];
    match modal::choose(ctx, id, Spec::new(title).text(&text), &actions, 0, |_| {}) {
        DialogOutcome::Open => false,
        DialogOutcome::Chose(1) => {
            state.chat.link_to_open = None;
            env.bridge
                .send(Command::Chat(ChatCommand::OpenLinkAnyway(link)));
            true
        }
        _ => {
            state.chat.link_to_open = None;
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_twelve_groups_of_five() {
        let code = "1234512345 12345123451234512345123451234512345123451234512345123451234599";
        let g = code_groups(code);
        assert_eq!(g.len(), 12);
        assert!(g.iter().all(|x| x.len() == 5));
    }
}
