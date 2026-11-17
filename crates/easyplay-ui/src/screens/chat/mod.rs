//! The Chat page (ADR-0033; chat.html section 8 "8 · CHAT", styles section 13): friends,
//! groups and conversations.
//!
//! What it shows follows Chat's status. While off, the invitation's look: "Play together,
//! privately", what Chat is and which relays it uses (named before first use), the name
//! friends see and the switch that turns it on. A build without the engine says so; while it
//! starts, a quiet line over skeleton rows; after repeated failures, the catalogue's words
//! for the error and *Try again*.
//!
//! Running, three columns with a 1 pt `line` between them: the list (300), the
//! conversation, and the details (312, only while open; open by default from 1240 wide, the
//! user's choice then sticks). Up to 1080 wide the details lie over the conversation (the
//! page's fill, a shadow towards it); up to 760 the list and the conversation take turns.
//! With nobody to talk to the page is one invitation, like the empty library. A
//! right-to-left interface mirrors the columns.
//!
//! Keys: `Esc` steps back one thing at a time (`ChatVm::escape`, called from the shell
//! before any widget sees the key); typing while nothing has focus goes to the message
//! field; `Ctrl+F` searches the chats.

mod avatar;
mod composer;
mod conversation;
mod details;
mod dialogs;
pub(crate) mod emoji;
mod gif;
mod incoming;
mod list;
mod markdown;
mod media;
mod paint;
mod trays;

pub use dialogs::dialog;
pub use incoming::{card as incoming_card, top_bar_pill};

use easyplay_core::model::{ChatRef, ChatSnapshot, ChatStatus};
use easyplay_core::service::ChatCommand;
use egui::{Align, Id, Layout, Margin, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};

use super::Env;
use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::route::Route;
use crate::strings::chat as c;
use crate::strings::chat::list as ls;
use crate::theme::palette;
use crate::theme::tokens::{layout, overlay, space, stroke};
use crate::theme::{self, Dur, Type, icons, motion};
use crate::vm::chat::{ChatDialog, valid_name};
use crate::vm::dialogs::Dialog;
use crate::widgets::button::{Button, Size};
use crate::widgets::empty_state::blank;
use crate::widgets::rows::SettingRow;
use crate::widgets::skeleton;
use crate::widgets::text_field::TextField;
use crate::widgets::toggle::switch;

/// The page's measures (chat.html `.chat`, `.dt`, the narrow-window rules).
mod page {
    /// The list column.
    pub const LIST_W: f32 = 300.0;
    /// The details panel.
    pub const DETAILS_W: f32 = 312.0;
    /// Up to this width the details lie over the conversation.
    pub const OVERLAY_TO: f32 = 1080.0;
    /// Up to this width the list and the conversation take turns.
    pub const TURNS_TO: f32 = 760.0;
    /// From this width the details are open by default.
    pub const DETAILS_FROM: f32 = 1240.0;
    /// The shadow of details lying over the conversation: offset towards it, blur, alpha.
    pub const OVER_SHADOW: [f32; 3] = [18.0, 36.0, 0.28];
    /// The invitation stands in the middle of a box this high (or the page, if lower).
    pub const BLANK_BOX: f32 = 520.0;
}

/// Older measures still used by the other parts of the page (conversation, dialogs).
#[allow(dead_code)]
mod metrics {
    /// A conversation row.
    pub const ROW_H: f32 = 58.0;
    /// The avatar of a conversation row (radius).
    pub const AVATAR: f32 = 18.0;
    /// The avatar in a conversation's header (radius).
    pub const AVATAR_SMALL: f32 = 15.0;
    /// The conversation's side padding.
    pub const PANE_PAD: f32 = 24.0;
    /// The conversation's header.
    pub const HEADER_H: f32 = 60.0;
    /// The transcript's measure.
    pub const TRANSCRIPT_MAX: f32 = 760.0;
    /// A message is at most this share of the transcript's width…
    pub const BUBBLE_SHARE: f32 = 0.78;
    /// …and at most this wide.
    pub const BUBBLE_MAX: f32 = 520.0;
    /// Padding inside a message: sides, top and bottom.
    pub const BUBBLE_PAD: [f32; 2] = [12.0, 7.0];
    /// Radius of a message.
    pub const BUBBLE_RADIUS: u8 = 12;
    /// Between messages of one run (one author, minutes apart).
    pub const RUN_GAP: f32 = 3.0;
    /// Between runs.
    pub const MESSAGE_GAP: f32 = 12.0;
    /// Messages this close in time from one author form a run.
    pub const RUN_MS: i64 = 5 * 60_000;
    /// Older messages load when the transcript is scrolled this close to its top.
    pub const OLDER_AT: f32 = 32.0;
    /// A QR code's side.
    pub const QR_SIDE: f32 = 176.0;
    /// The unread count's pill: height and minimum width.
    pub const BADGE: [f32; 2] = [18.0, 18.0];
    /// Skeleton rows while starting.
    pub const SKELETON_ROWS: usize = 5;
}

/// The page.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    // The list says again this pass whether its search field has focus.
    ui.ctx()
        .data_mut(|d| d.insert_temp(list::search_key(), false));
    let snap = env.bridge.chat().clone();
    if snap.status != ChatStatus::Off {
        state.chat.turning_on = false;
    }
    if snap.status != ChatStatus::Ready {
        state.chat.narrow_open = false;
    }
    match &snap.status {
        ChatStatus::Off => off(ui, state, env),
        ChatStatus::Unavailable => padded(ui, |ui| {
            blank(
                ui,
                c::UNAVAILABLE_TITLE(),
                c::UNAVAILABLE_BODY(),
                |_| (),
                None,
            );
        }),
        ChatStatus::Starting => starting(ui),
        ChatStatus::Failed(code) => {
            let e = easyplay_core::error::catalog::describe(
                &easyplay_core::error::Error::new(*code),
                &easyplay_core::error::catalog::DescribeCtx::plain(),
            );
            let retry = padded(ui, |ui| {
                blank(
                    ui,
                    &e.title,
                    &e.message,
                    |ui| ui.add(Button::primary(c::TRY_AGAIN())).clicked(),
                    None,
                )
            });
            if retry {
                env.bridge.send(UiCommand::Chat(ChatCommand::SetEnabled {
                    on: true,
                    name: None,
                }));
            }
        }
        ChatStatus::Ready => ready(ui, state, env, &snap),
    }
    note_text_focus(ui.ctx());
}

/// Install the emoji glyphs in the style Settings asks for (Twemoji or the system's), once
/// a frame before anything draws people's text (the list, the top bar, the call card).
pub fn prepare(ctx: &egui::Context, prefs: &easyplay_core::config::schema::Preferences) {
    emoji::set_system(ctx, emoji::system_style(prefs));
}

/// Back on the Chat page (`Backspace`, `Alt+←`, the mouse's back button): in a narrow
/// window an open conversation closes first. Returns whether Back was used up.
pub fn back(state: &mut UiState) -> bool {
    if state.nav.current() == Route::Chat && state.chat.narrow_open && state.chat.open.is_some() {
        state.chat.close_chat();
        return true;
    }
    false
}

/// `Esc` on the Chat page, before any widget sees it (the shell calls this): one step of
/// `ChatVm::escape`. Returns whether a step was taken.
pub fn escape(ctx: &egui::Context, state: &mut UiState) -> bool {
    // egui drops focus when `Esc` is pressed: what had focus is the last pass's.
    let search = ctx
        .data(|d| d.get_temp::<bool>(list::search_key()))
        .unwrap_or(false);
    state.chat.escape(search)
}

/// Whether a text field of the Chat page had focus in the last pass (egui drops focus
/// when `Esc` is pressed, before the shell sees the key).
pub fn had_text_focus(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(text_focus_key()))
        .unwrap_or(false)
}

fn text_focus_key() -> Id {
    Id::unique("chat.text_focus")
}

/// Remember whether a text field has focus at the end of the page's pass.
fn note_text_focus(ctx: &egui::Context) {
    let on = ctx
        .memory(|m| m.focused())
        .is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some());
    ctx.data_mut(|d| d.insert_temp(text_focus_key(), on));
}

/// Whether the details panel shows at this width with `open` chat (the conversation's
/// header shows its button pressed while they do).
pub fn details_shown(state: &UiState) -> bool {
    state.chat.open.is_some() && state.chat.details_open()
}

/// The page's gutters around `add`.
fn padded<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let gutter = super::shell::gutter(ui.ctx());
    egui::Frame::NONE
        .inner_margin(Margin {
            left: gutter as i8,
            right: gutter as i8,
            top: 0,
            bottom: 0,
        })
        .show(ui, add)
        .inner
}

/// A scrolling page with a column at most `max` wide in its middle.
fn column_page(ui: &mut Ui, id: &str, max: f32, top: f32, add: impl FnOnce(&mut Ui)) {
    let gutter = super::shell::gutter(ui.ctx());
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(Margin {
                    left: gutter as i8,
                    right: gutter as i8,
                    top: top as i8,
                    bottom: layout::PAGE_PAD[1] as i8,
                })
                .show(ui, |ui| {
                    let w = ui.available_width().min(max);
                    let indent = super::centre_indent(ui.available_width(), max);
                    let at = ui.cursor().min + vec2(indent, 0.0);
                    let mut column = ui.new_child(
                        UiBuilder::new()
                            .max_rect(Rect::from_min_size(at, vec2(w, f32::INFINITY)))
                            .layout(Layout::top_down(Align::Min)),
                    );
                    column.set_width(w);
                    column.spacing_mut().item_spacing.y = 0.0;
                    add(&mut column);
                    let h = column.min_rect().height();
                    ui.allocate_space(vec2(ui.available_width(), h));
                });
        });
}

/// Chat is off: the invitation's look, with what Chat is and where messages travel (the
/// relays are named before first use), the name friends see, and the switch.
fn off(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let prefs = env.bridge.prefs().clone();
    column_page(
        ui,
        "chat.off",
        layout::BLANK_W,
        overlay::BLANK_PAD * 2.0,
        |ui| {
            let m = theme::theme(ui.ctx()).motion;
            let (alpha, below) = motion::arrive(
                ui.ctx(),
                Id::unique("chat.off.arrive"),
                m,
                0.0,
                Dur::Base,
                theme::tokens::travel::RISE,
            );
            ui.multiply_opacity(alpha);
            ui.add_space(below);
            let w = ui.available_width();
            let g = crate::widgets::modal::balanced(ui, Type::Blank, ls::INVITE_TITLE(), p.t1, w);
            let (r, resp) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
            let x = if crate::i18n::dir() == crate::i18n::Dir::Rtl {
                r.right() - g.size().x
            } else {
                r.left()
            };
            ui.painter().galley(pos2(x, r.top()), g, p.t1);
            crate::a11y::labelled(ui.ctx(), resp.id, egui::Role::Heading, ls::INVITE_TITLE());
            ui.add_space(overlay::BLANK_TEXT_GAP);
            Type::Lead.paragraph(ui, ls::OFF_BODY(), p.t2);
            ui.add_space(overlay::BLANK_ACTIONS_GAP);
            let name = state
                .chat
                .name
                .get_or_insert_with(|| prefs.chat.name.clone());
            TextField::new(name, "chat.off.name")
                .label(c::NAME())
                .hint(c::NAME_HINT())
                .help(c::NAME_RULE())
                .width(ui.available_width().min(layout::PATH_FIELD_W))
                .show(ui);
            let typed = valid_name(name);
            ui.add_space(space::S2);
            let mut on = state.chat.turning_on;
            let changed = SettingRow::new(c::TURN_ON())
                .help(c::TURN_ON_HELP())
                .enabled(typed.is_some() && !state.chat.turning_on)
                .show(ui, |ui| switch(ui, &mut on, c::TURN_ON()).changed());
            ui.add_space(overlay::BLANK_FINE_GAP);
            Type::Caption.paragraph_lh(
                ui,
                ls::INVITE_FINE(),
                p.t3,
                false,
                Some(overlay::FINE_LINE),
            );
            if changed
                && on
                && let Some(name) = typed
            {
                state.chat.turning_on = true;
                let mut next = (*prefs).clone();
                next.chat.enabled = true;
                next.chat.name.clone_from(&name);
                env.bridge.set_prefs_preview(next);
                env.bridge.send(UiCommand::Chat(ChatCommand::SetEnabled {
                    on: true,
                    name: Some(name),
                }));
            }
        },
    );
}

/// Chat is starting: a quiet line over skeleton rows.
fn starting(ui: &mut Ui) {
    let p = theme::theme(ui.ctx()).palette.clone();
    column_page(
        ui,
        "chat.starting",
        layout::COL_MAX,
        layout::SETTINGS_TOP,
        |ui| {
            let r = Type::Body.paragraph(ui, c::STARTING(), p.t3);
            crate::a11y::item(&r, egui::Role::Status, c::STARTING());
            ui.add_space(space::S4);
            for k in 0..metrics::SKELETON_ROWS {
                let (rect, _) = ui.allocate_exact_size(
                    vec2(ui.available_width(), metrics::ROW_H),
                    Sense::hover(),
                );
                let d = metrics::AVATAR * 2.0;
                let a =
                    Rect::from_min_size(pos2(rect.left(), rect.center().y - d / 2.0), vec2(d, d));
                skeleton::block_at(
                    ui,
                    a,
                    Some(egui::CornerRadius::same(theme::tokens::radius::PILL)),
                    k,
                );
                let x = a.right() + space::S3;
                let w = rect.right() - x;
                skeleton::line(ui, pos2(x, rect.center().y - 12.0), w * 0.35, 10.0);
                skeleton::line(ui, pos2(x, rect.center().y + 4.0), w * 0.7, 10.0);
            }
        },
    );
}

/// Nobody to talk to yet: one invitation, like the empty library.
fn invitation(ui: &mut Ui, state: &mut UiState) {
    // `.blank`: centred in a box min(520, the page) high (its height from the last pass).
    let key = Id::unique("chat.invitation.h");
    let last: f32 = ui.ctx().data(|d| d.get_temp(key)).unwrap_or(0.0);
    let boxed = page::BLANK_BOX.min(ui.available_height());
    if last > 0.0 {
        ui.add_space(((boxed - last) / 2.0).max(0.0));
    } else {
        ui.ctx().request_repaint();
    }
    let top = ui.cursor().top();
    let add = padded(ui, |ui| {
        blank(
            ui,
            ls::INVITE_TITLE(),
            ls::INVITE_BODY(),
            |ui| {
                ui.add(
                    Button::primary(ls::INVITE_ADD())
                        .size(Size::Lg)
                        .icon(icons::USER_PLUS),
                )
                .clicked()
            },
            Some(ls::INVITE_FINE()),
        )
    });
    let h = ui.cursor().top() - top;
    if (h - last).abs() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(key, h));
        ui.ctx().request_repaint();
    }
    if add {
        state.chat.link.clear();
        state.chat.invite_asked = false;
        state.show_dialog(Dialog::Chat(ChatDialog::AddFriend));
    }
}

/// Typing while nothing has focus goes to the open chat's message field: the text joins
/// its draft and the composer takes focus (`focus_composer`).
fn type_anywhere(ui: &Ui, state: &mut UiState) {
    let Some(chat) = state.chat.open.as_ref().map(|o| o.chat) else {
        return;
    };
    let ctx = ui.ctx();
    if ctx.memory(|m| m.focused()).is_some()
        || egui::Popup::is_any_open(ctx)
        || !state.nav.overlays().is_empty()
        || state.chat.picker.is_some()
    {
        return;
    }
    let typed = ui.input_mut(|i| {
        let mut out = String::new();
        if i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt {
            return out;
        }
        i.events.retain(|e| match e {
            egui::Event::Text(t) if t != " " && t != "?" && !t.chars().any(char::is_control) => {
                out.push_str(t);
                false
            }
            _ => true,
        });
        out
    });
    if !typed.is_empty() {
        state.chat.drafts.entry(chat).or_default().push_str(&typed);
        state.chat.focus_composer = true;
    }
}

/// Chat is running: the list, the open conversation, and the details.
fn ready(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, snap: &ChatSnapshot) {
    // A conversation that left the snapshot (removed, left) closes.
    if let Some(open) = state.chat.open.as_ref().map(|o| o.chat)
        && !snap.chats.iter().any(|p| p.chat == open)
    {
        state.chat.open = None;
    }
    if snap.friends.is_empty() && snap.groups.is_empty() && snap.requests.is_empty() {
        state.chat.narrow_open = false;
        invitation(ui, state);
        return;
    }
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let page = ui.available_rect_before_wrap();
    let w = page.width();
    let narrow = w <= page::TURNS_TO;
    let over = w <= page::OVERLAY_TO;
    state.chat.details_auto = w >= page::DETAILS_FROM;
    // A wide window always has a conversation open: the newest (the script's renderChat).
    if !narrow
        && state.chat.open.is_none()
        && let Some(first) = list::listed(snap).first().map(|c| c.chat)
    {
        open_chat(state, env, first);
    }
    type_anywhere(ui, state);
    let open = state.chat.open.as_ref().map(|o| o.chat);
    state.chat.narrow_open = narrow && open.is_some();
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let flip = |r: Rect| {
        if rtl {
            Rect::from_x_y_ranges(
                (page.left() + page.right() - r.right())..=(page.left() + page.right() - r.left()),
                r.y_range(),
            )
        } else {
            r
        }
    };
    let line = Stroke::new(stroke::BORDER, p.line);

    // Which columns show.
    let show_list = if narrow {
        open.is_none()
    } else {
        !state.chat.stage_large
    };
    let show_convo = open.is_some() && (!narrow || !show_list);
    let details_on = show_convo && state.chat.details_open();
    let dw = motion::glide(
        ui.ctx(),
        Id::unique("chat.details.w"),
        if details_on { page::DETAILS_W } else { 0.0 },
        theme.motion.movement(Dur::Move),
    )
    // Whole points while it slides, so the columns' children stay on the pixel grid.
    .round();
    let list_w = if show_list {
        if narrow { w } else { page::LIST_W }
    } else {
        0.0
    };
    let list_rect = flip(Rect::from_min_size(page.min, vec2(list_w, page.height())));
    let push = if over { 0.0 } else { dw };
    let convo_rect = flip(Rect::from_min_max(
        pos2(page.left() + list_w, page.top()),
        pos2(page.right() - push, page.bottom()),
    ));
    let details_rect = flip(Rect::from_min_max(
        pos2(page.right() - dw, page.top()),
        page.max,
    ));

    if show_list {
        let mut lui = ui.new_child(
            UiBuilder::new()
                .id_salt("chat.list.col")
                .max_rect(list_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        lui.set_clip_rect(list_rect.intersect(ui.clip_rect()));
        list::show(&mut lui, state, env, snap);
        if !narrow {
            let x = if rtl {
                list_rect.left() + stroke::BORDER / 2.0
            } else {
                list_rect.right() - stroke::BORDER / 2.0
            };
            ui.painter().vline(x, list_rect.y_range(), line);
        }
    }
    if show_convo && let Some(chat) = open {
        let mut cui = ui.new_child(
            UiBuilder::new()
                .id_salt("chat.convo.col")
                .max_rect(convo_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        cui.set_clip_rect(convo_rect.intersect(ui.clip_rect()));
        conversation::show(&mut cui, state, env, snap, chat, narrow);
    } else if !narrow {
        // Nothing to open: one quiet line where the conversation would be.
        let mut cui = ui.new_child(
            UiBuilder::new()
                .id_salt("chat.convo.none")
                .max_rect(convo_rect),
        );
        cui.add_space(convo_rect.height() / 2.0 - Type::Caption.line_height());
        padded(&mut cui, |ui| quiet_line(ui, ls::EMPTY(), p.t3, true));
    }
    if dw > 0.5
        && let Some(chat) = open
    {
        // The panel keeps its width while it slides: it is cut by its column.
        let full = flip(Rect::from_min_size(
            pos2(page.right() - dw, page.top()),
            vec2(page::DETAILS_W, page.height()),
        ));
        if over {
            let area = egui::Area::new(Id::unique("chat.details.over"))
                .order(egui::Order::Middle)
                .fixed_pos(details_rect.min)
                .constrain(false)
                .interactable(true);
            area.show(ui.ctx(), |aui| {
                // The panel's whole rect is the area's (and takes the pointer from what
                // lies under it).
                aui.allocate_rect(details_rect, Sense::click());
                let [dx, blur, a] = page::OVER_SHADOW;
                let shadow = egui::epaint::Shadow {
                    offset: [if rtl { dx as i8 } else { -dx as i8 }, 0],
                    blur: blur as u8,
                    spread: 0,
                    color: palette::black(a),
                };
                aui.painter()
                    .with_clip_rect(Rect::EVERYTHING)
                    .add(shadow.as_shape(details_rect, 0));
                aui.painter().rect_filled(details_rect, 0, p.bg);
                details_column(aui, state, env, snap, chat, full, details_rect, rtl, line);
            });
        } else {
            details_column(ui, state, env, snap, chat, full, details_rect, rtl, line);
        }
    }
    ui.allocate_rect(page, Sense::hover());
}

/// The details panel in `shown` (its column; `full` is the panel at its whole width).
#[allow(clippy::too_many_arguments)]
fn details_column(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
    full: Rect,
    shown: Rect,
    rtl: bool,
    line: Stroke,
) {
    let mut dui = ui.new_child(
        UiBuilder::new()
            .id_salt("chat.details.col")
            .max_rect(full)
            .layout(Layout::top_down(Align::Min)),
    );
    dui.set_clip_rect(shown.intersect(ui.clip_rect()));
    details::show(&mut dui, state, env, snap, chat);
    let x = if rtl {
        shown.right() - stroke::BORDER / 2.0
    } else {
        shown.left() + stroke::BORDER / 2.0
    };
    ui.painter().vline(x, shown.y_range(), line);
}

/// Open `chat`: ask for its latest page.
pub(crate) fn open_chat(state: &mut UiState, env: &mut Env<'_>, chat: ChatRef) {
    if state.chat.open_chat(chat) {
        let q = env.bridge.query_id();
        state.chat.asked(q, false);
        env.bridge.send(UiCommand::Chat(ChatCommand::LoadChat {
            chat,
            before: None,
            reply_to: q,
        }));
    }
}

/// The name of a conversation as the list has it.
fn name_of(snap: &ChatSnapshot, chat: ChatRef) -> String {
    snap.chat_name(chat)
        .map(str::to_owned)
        .or_else(|| {
            snap.chats
                .iter()
                .find(|p| p.chat == chat)
                .map(|p| p.name.clone())
        })
        .unwrap_or_default()
}

/// A short line in a quiet colour, in its own direction.
fn quiet_line(ui: &mut Ui, text: &str, color: egui::Color32, centred: bool) -> egui::Response {
    let w = ui.available_width();
    let para = paint::Para::new(ui, text, Type::Caption, color, w);
    let size = para.size();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, size.y), Sense::hover());
    let x = if centred {
        rect.center().x - size.x / 2.0
    } else {
        rect.left()
    };
    para.paint(
        ui.painter(),
        Rect::from_min_size(pos2(x, rect.top()), size),
        color,
    );
    crate::a11y::item(&resp, egui::Role::Label, text);
    resp
}

/// Whether the window has focus (messages are read only while it has).
#[allow(dead_code)]
fn window_focused(ui: &Ui) -> bool {
    ui.input(|i| i.focused)
}

/// The count pill by the top bar's "Chat": the unread messages of every unmuted chat.
/// Returns the width it takes after the label (0 without unread messages).
pub fn nav_count_width(ui: &Ui, unread: u32) -> f32 {
    if unread == 0 {
        return 0.0;
    }
    NAV_COUNT_GAP + avatar::pill_size(ui, unread).x
}

/// Between "Chat" and its count pill.
pub const NAV_COUNT_GAP: f32 = 6.0;

/// Paint the top bar's count pill with its leading edge at `left`, centred on `y`; says
/// the count to a screen reader through `item`.
pub fn nav_count(ui: &Ui, item: &egui::Response, left: f32, y: f32, unread: u32) {
    if unread == 0 {
        return;
    }
    let size = avatar::pill_size(ui, unread);
    let r = Rect::from_min_size(pos2(left + NAV_COUNT_GAP, y - size.y / 2.0), size);
    avatar::paint_pill(ui.painter(), r, unread);
    crate::a11y::describe(item, &ls::unread_total(unread));
}
