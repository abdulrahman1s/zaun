//! The list column (chat.html section 8 LIST COLUMN; styles `.cl`, `.cr`, `.pn`, `.me`;
//! script sections 22 and 27).
//!
//! - At the top, the search field filling the row and a 30 pt "+" with a menu (Add a
//!   friend… · New group…). `Ctrl+F` searches.
//! - Then, scrolling: Requests (avatar 36 · name · "Wants to be friends" · Accept and
//!   Ignore, 26 high); Playing now (posters 64 wide, 14 apart, the friend's 22 pt avatar
//!   over the top corner, a live mark while they share; hover lifts 2; a click opens their
//!   chat); then the chats, newest first: 56 high, radius 10, padding 10, avatar 36 · 12 ·
//!   name over a one-line preview, and on the trailing side the time over one of: the
//!   unread count, a bell when muted, three live bars while a call is on. Section labels
//!   (12 pt medium `t3`, 18 above and 8 below) only when there is more than one section;
//!   "Results" while searching. Rows slide to their new places (300, out).
//! - Under the chats, "Blocked (n)"; pinned at the bottom, the "you" row (52 high): your
//!   avatar with your status, your name, status words or "Playing <game>", a caret; its
//!   menu sets the status and opens the profile and Chat settings.
//!
//! Keys: the chats are one tab stop; ↑ ↓ move, Enter opens. A right-to-left interface
//! mirrors every row.

use easyplay_core::model::{
    ChatGroup, ChatPreview, ChatRef, ChatSnapshot, Friend, Membership, MessageAuthor, MessageBody,
    MyStatus, Presence,
};
use easyplay_core::service::ChatCommand;
use egui::{
    Align2, CornerRadius, Id, Key, Modifiers, Popup, PopupCloseBehavior, Rect, RectAlign, Response,
    Sense, Ui, UiBuilder, pos2, vec2,
};

use super::Env;
use super::avatar::{self, Shape};
use crate::app::UiState;
use crate::bridge::Command;
use crate::route::{Route, SettingsTab};
use crate::strings::chat::list as s;
use crate::theme::palette::{self, fade, mix, over};
use crate::theme::tokens::icon as icon_size;
use crate::theme::{self, Dur, Type, icons};
use crate::vm::chat::ChatDialog;
use crate::vm::dialogs::{AfterDiscard, Dialog};
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::icon_button::IconButton;
use crate::widgets::text_field::TextField;
use crate::widgets::{context_menu, focus_ring, hover_t, own_arrow_keys, press_t, tooltip};

/// The column's measures (chat.html `.cl-top`, `.cl-scroll`, `.cl-h`, `.cr`, `.pn`, `.me`).
mod m {
    /// Above the search row.
    pub const TOP_PAD: f32 = 4.0;
    /// After the "+" (the column's trailing side).
    pub const TOP_END: f32 = 10.0;
    /// Between the search field and "+".
    pub const TOP_GAP: f32 = 6.0;
    /// The "+" button.
    pub const PLUS: f32 = 30.0;
    /// Rows start this far before the gutter.
    pub const ROW_OUT: f32 = 10.0;
    /// Rows end this far from the column's trailing side (8, and the scroll bar's room).
    pub const ROW_END: f32 = 18.0;
    /// Under the last row.
    pub const SCROLL_BOTTOM: f32 = 12.0;
    /// A section label: above the first, above the others, below, in from the rows.
    pub const HEAD: [f32; 4] = [14.0, 18.0, 8.0, 10.0];
    /// Above the chats when they have no label.
    pub const SPACER: f32 = 10.0;
    /// A chat row.
    pub const ROW_H: f32 = 56.0;
    /// Inside a row.
    pub const ROW_PAD: f32 = 10.0;
    /// A row's corner.
    pub const ROW_RADIUS: u8 = 10;
    /// A row's avatar.
    pub const AV: f32 = 36.0;
    /// Between the avatar and the text, and between the text and the trailing column.
    pub const GAP: f32 = 12.0;
    /// Between the name and the preview.
    pub const NAME_GAP: f32 = 1.0;
    /// Between the time and its mark.
    pub const META_GAP: f32 = 5.0;
    /// The bell of a muted row.
    pub const MARK_ICON: f32 = 12.0;
    /// A request row: padding above and below; above its buttons; between them.
    pub const REQ: [f32; 3] = [8.0, 8.0, 6.0];
    /// Playing now: padding above, at the sides, below.
    pub const PN: [f32; 3] = [8.0, 10.0, 4.0];
    /// A poster's width and the space between posters.
    pub const POSTER: [f32; 2] = [64.0, 14.0];
    /// The friend's avatar on a poster, how far it hangs over, its ring.
    pub const PN_AV: [f32; 3] = [22.0, 7.0, 2.0];
    /// How far a poster rises under the pointer.
    pub const LIFT: f32 = 2.0;
    /// The live mark: inset, side, corner, dot.
    pub const LIVE: [f32; 4] = [4.0, 14.0, 4.0, 6.0];
    /// The line under an empty list: margins and line height.
    pub const NONE: [f32; 3] = [22.0, 10.0, 19.0];
    /// Above "Blocked (n)".
    pub const BLOCKED_TOP: f32 = 10.0;
    /// The "you" row: height, margin below, avatar, gap.
    pub const ME: [f32; 4] = [52.0, 8.0, 28.0, 10.0];
}

/// Mirror of x positions inside a span, for a right-to-left interface.
#[derive(Clone, Copy)]
struct Flip {
    rtl: bool,
    l: f32,
    r: f32,
}

impl Flip {
    fn new(span: Rect) -> Self {
        Self {
            rtl: crate::i18n::dir() == crate::i18n::Dir::Rtl,
            l: span.left(),
            r: span.right(),
        }
    }

    /// `rect`, laid out left to right, in its place.
    fn rect(self, rect: Rect) -> Rect {
        if self.rtl {
            Rect::from_x_y_ranges(
                (self.l + self.r - rect.right())..=(self.l + self.r - rect.left()),
                rect.y_range(),
            )
        } else {
            rect
        }
    }
}

/// Where the search field's id is kept (`Esc` clears the search only from the field).
pub fn search_key() -> Id {
    Id::unique("chat.list.search.id")
}

/// A friend's first name ("Mira" of "Mira Okafor").
pub fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// The conversations the list shows: every one except a blocked friend's, newest first.
pub fn listed(snap: &ChatSnapshot) -> Vec<&ChatPreview> {
    let mut out: Vec<&ChatPreview> = snap
        .chats
        .iter()
        .filter(|c| match c.chat {
            ChatRef::Direct(id) => !snap.friend(id).is_some_and(|f| f.blocked),
            ChatRef::Group(_) => true,
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.last.as_ref().map(|m| m.at.as_millis())));
    out
}

/// A chat's name as the list has it.
fn chat_name(snap: &ChatSnapshot, c: &ChatPreview) -> String {
    snap.chat_name(c.chat)
        .filter(|n| !n.is_empty())
        .map_or_else(|| c.name.clone(), str::to_owned)
}

/// Markdown as plain words (the script's `plain`): code blocks say "code", spoilers
/// "spoiler", links keep their text, marks go, lines join.
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    // Code blocks.
    while let Some(i) = rest.find("```") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 3..];
        match after.find("```") {
            Some(j) => rest = &after[j + 3..],
            None => rest = "",
        }
        out.push_str("code ");
    }
    out.push_str(rest);
    // Spoilers.
    let mut t = String::with_capacity(out.len());
    let mut r = out.as_str();
    while let Some(i) = r.find("||") {
        let after = &r[i + 2..];
        match after.find("||") {
            Some(j) if j > 0 => {
                t.push_str(&r[..i]);
                t.push_str("spoiler");
                r = &after[j + 2..];
            }
            _ => {
                t.push_str(&r[..i + 2]);
                r = after;
            }
        }
    }
    t.push_str(r);
    // Links: [text](url) keeps the text.
    let mut u = String::with_capacity(t.len());
    let mut r = t.as_str();
    while let Some(i) = r.find('[') {
        let after = &r[i + 1..];
        if let Some(j) = after.find("](")
            && let Some(k) = after[j + 2..].find(')')
            && !after[..j].contains('\n')
        {
            u.push_str(&r[..i]);
            u.push_str(&after[..j]);
            r = &after[j + 2 + k + 1..];
        } else {
            u.push_str(&r[..=i]);
            r = after;
        }
    }
    u.push_str(r);
    let lines: Vec<&str> = u
        .lines()
        .map(|l| {
            l.strip_prefix("> ")
                .or_else(|| l.strip_prefix('>'))
                .unwrap_or(l)
        })
        .collect();
    let joined = lines.join(" ");
    let cleaned: String = joined
        .chars()
        .filter(|c| !matches!(c, '*' | '_' | '~' | '`'))
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What a row says under its name (the script's `previewOf`).
pub fn preview(snap: &ChatSnapshot, c: &ChatPreview) -> String {
    if let ChatRef::Group(g) = c.chat
        && snap
            .group(g)
            .is_some_and(|g| g.membership == Membership::Invited)
    {
        return s::INVITED().to_owned();
    }
    let Some(msg) = &c.last else {
        return s::NO_MESSAGES().to_owned();
    };
    let what = match &msg.body {
        MessageBody::Call(call) => {
            return match call.kind {
                easyplay_core::model::CallKind::Ended => s::CALL_ENDED(),
                easyplay_core::model::CallKind::Missed => s::MISSED_CALL(),
                easyplay_core::model::CallKind::Declined => s::CALL_DECLINED(),
            }
            .to_owned();
        }
        // Conversation agent, round 3c: the notice in words.
        MessageBody::Notice(n) => {
            let friend = match c.chat {
                ChatRef::Direct(_) => c.name.split_whitespace().next().unwrap_or(&c.name),
                ChatRef::Group(_) => "",
            };
            return crate::strings::chat::conversation::notice(n, friend);
        }
        MessageBody::Deleted => return s::DELETED().to_owned(),
        MessageBody::Text(t) => plain(t),
        MessageBody::Voice { .. } => s::VOICE().to_owned(),
        MessageBody::Gif(_) => s::GIF().to_owned(),
        // Conversation agent, round 3: "Photo", "Video", "File: <name>".
        MessageBody::Image(_) => crate::strings::chat::conversation::PHOTO().to_owned(),
        MessageBody::Video(_) => crate::strings::chat::conversation::VIDEO_WORD().to_owned(),
        MessageBody::File(m) => crate::strings::chat::conversation::file_preview(&m.name),
        MessageBody::Stream(st) if st.live => s::sharing(&st.title),
        MessageBody::Stream(st) => s::shared(&st.title),
    };
    match &msg.author {
        MessageAuthor::Me => s::you_said(&what),
        MessageAuthor::Member(n) if matches!(c.chat, ChatRef::Group(_)) => {
            s::member_said(first_name(n), &what)
        }
        _ => what,
    }
}

/// A friend's status words (the script's `statusOf`).
pub fn status_words(f: &Friend) -> String {
    if let Some(pl) = &f.playing
        && f.presence != Presence::Offline
    {
        return s::playing(&pl.title);
    }
    match f.presence {
        Presence::Online => s::ONLINE(),
        Presence::Away => s::AWAY(),
        Presence::Busy => s::BUSY(),
        Presence::Offline => s::OFFLINE(),
    }
    .to_owned()
}

/// The user's own status words.
pub fn my_status_words(status: MyStatus) -> &'static str {
    match status {
        MyStatus::Online => s::ONLINE(),
        MyStatus::Away => s::AWAY(),
        MyStatus::Busy => s::BUSY(),
        MyStatus::Invisible => s::INVISIBLE(),
    }
}

/// Add `widget` in `rect` without moving the layout's cursor (`Ui::put` moves it to the
/// rect's bottom).
fn place(ui: &mut Ui, rect: Rect, widget: impl egui::Widget) -> Response {
    ui.new_child(UiBuilder::new().max_rect(rect)).add(widget)
}

/// `text` without its emoji (for text the font lays out whole: tooltips, the empty line).
fn without_emoji(text: &str) -> String {
    super::emoji::runs(text)
        .into_iter()
        .filter_map(|r| match r {
            super::emoji::Run::Text(t) => Some(t),
            super::emoji::Run::Emoji(_) => None,
        })
        .collect()
}

/// What a click in a menu of the list asks for.
enum Act {
    Open(ChatRef),
    Accept(i64),
    Ignore(i64),
    Mute(ChatRef, bool),
    Dialog(ChatDialog),
    Unblock(i64),
    Status(MyStatus),
    ShowPlaying(bool),
    Settings,
}

fn apply(state: &mut UiState, env: &mut Env<'_>, act: Act) {
    let send = |env: &mut Env<'_>, c: ChatCommand| env.bridge.send(Command::Chat(c));
    match act {
        Act::Open(chat) => super::open_chat(state, env, chat),
        Act::Accept(id) => send(env, ChatCommand::Accept(id)),
        Act::Ignore(id) => send(env, ChatCommand::Reject(id)),
        Act::Mute(chat, on) => send(env, ChatCommand::SetMuted { chat, on }),
        Act::Dialog(d) => {
            match d {
                ChatDialog::AddFriend => {
                    state.chat.link.clear();
                    state.chat.invite_asked = false;
                }
                ChatDialog::NewGroup => state.chat.group_name.clear(),
                _ => {}
            }
            state.show_dialog(Dialog::Chat(d));
        }
        Act::Unblock(friend) => send(env, ChatCommand::SetBlocked { friend, on: false }),
        Act::Status(st) => send(env, ChatCommand::SetStatus(st)),
        Act::ShowPlaying(on) => {
            super::super::settings::set_bool(state, env, "chat.show_playing", on, |p, v| {
                p.chat.show_playing = v
            })
        }
        Act::Settings => state.navigate(AfterDiscard::Route(Route::Settings(SettingsTab::Chat))),
    }
}

/// The list column, filling `ui`'s rect.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, snap: &ChatSnapshot) {
    let rect = ui.max_rect();
    let gutter = super::super::shell::gutter(ui.ctx());
    let flip = Flip::new(rect);
    let mut acts = Vec::new();
    let search_now = ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F));

    // The search row.
    let top = rect.top() + m::TOP_PAD;
    let plus = flip.rect(Rect::from_min_size(
        pos2(rect.right() - m::TOP_END - m::PLUS, top),
        vec2(m::PLUS, m::PLUS),
    ));
    let field = flip.rect(Rect::from_min_max(
        pos2(rect.left() + gutter, top),
        pos2(
            rect.right() - m::TOP_END - m::PLUS - m::TOP_GAP,
            top + m::PLUS,
        ),
    ));
    let mut fui = ui.new_child(UiBuilder::new().id_salt("chat.list.top").max_rect(field));
    let fr = TextField::new(&mut state.chat.query, "chat.list.search")
        .search()
        .hint(s::SEARCH())
        .accessible_name(s::SEARCH())
        .width(field.width())
        .show(&mut fui);
    let search_focus = fr.response.has_focus();
    ui.ctx()
        .data_mut(|d| d.insert_temp(search_key(), search_focus));
    if search_now {
        fr.response.request_focus();
    }
    let pr = place(ui, plus, IconButton::new(icons::PLUS, s::NEW()));
    let popup = Popup::menu(&pr).align(if flip.rtl {
        RectAlign::BOTTOM_START
    } else {
        RectAlign::BOTTOM_END
    });
    if let Some(Some(a)) = context_menu::popup(popup, |ui| {
        if context_menu::icon_item(ui, icons::USER_PLUS, s::ADD_FRIEND(), None).clicked() {
            return Some(Act::Dialog(ChatDialog::AddFriend));
        }
        if context_menu::icon_item(ui, icons::USERS, s::NEW_GROUP(), None).clicked() {
            return Some(Act::Dialog(ChatDialog::NewGroup));
        }
        None
    }) {
        acts.push(a);
    }

    // The "you" row, pinned at the bottom.
    let me_rect = flip.rect(Rect::from_min_max(
        pos2(
            rect.left() + gutter - m::ROW_OUT,
            rect.bottom() - m::ME[1] - m::ME[0],
        ),
        pos2(
            rect.right() - m::ROW_END + m::ROW_OUT,
            rect.bottom() - m::ME[1],
        ),
    ));
    me_row(ui, env, snap, me_rect, &mut acts);

    // The scrolling part.
    let scroll = Rect::from_min_max(
        pos2(rect.left(), top + m::PLUS),
        pos2(rect.right(), me_rect.top()),
    );
    let mut sui = ui.new_child(
        UiBuilder::new()
            .id_salt("chat.list.scroll")
            .max_rect(scroll)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    sui.set_clip_rect(scroll.intersect(ui.clip_rect()));
    egui::ScrollArea::vertical()
        .id_salt("chat.list")
        .auto_shrink([false, false])
        .show(&mut sui, |ui| {
            let full = ui.max_rect();
            let (l, r) = (gutter - m::ROW_OUT, m::ROW_END);
            let (l, r) = if flip.rtl { (r, l) } else { (l, r) };
            let col = Rect::from_min_max(
                pos2(full.left() + l, full.top()),
                pos2(full.right() - r, full.bottom()),
            );
            let mut cui = ui.new_child(
                UiBuilder::new()
                    .max_rect(col)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            cui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            sections(&mut cui, state, env, snap, &mut acts);
            cui.add_space(m::SCROLL_BOTTOM);
            let h = cui.min_rect().height();
            ui.allocate_space(vec2(ui.available_width(), h));
        });
    for a in acts {
        apply(state, env, a);
    }
}

/// A section label (`.cl-h`).
fn label(ui: &mut Ui, text: &str, first: bool) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.add_space(if first { m::HEAD[0] } else { m::HEAD[1] });
    let w = ui.available_width();
    let g = Type::Label.galley(ui, text, p.t3, w - m::HEAD[3] * 2.0, 1);
    let (r, resp) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
    let at = Flip::new(r).rect(Rect::from_min_size(
        pos2(r.left() + m::HEAD[3], r.top()),
        g.size(),
    ));
    ui.painter().galley(at.min, g, p.t3);
    crate::a11y::item(&resp, egui::Role::Heading, text);
    ui.add_space(m::HEAD[2]);
}

fn sections(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    acts: &mut Vec<Act>,
) {
    let f = snap.features;
    let raw = state.chat.query.trim().to_owned();
    let q = raw.to_lowercase();
    let all = listed(snap);
    let rows: Vec<&ChatPreview> = if q.is_empty() {
        all
    } else {
        all.into_iter()
            .filter(|c| chat_name(snap, c).to_lowercase().contains(&q))
            .collect()
    };
    let reqs = if q.is_empty() {
        &snap.requests[..]
    } else {
        &[]
    };
    let playing: Vec<&Friend> = if q.is_empty() && f.presence {
        snap.friends
            .iter()
            .filter(|fr| fr.playing.is_some() && fr.presence != Presence::Offline && !fr.blocked)
            .collect()
    } else {
        Vec::new()
    };
    let blocked = snap.friends.iter().filter(|fr| fr.blocked).count();
    let heads = !playing.is_empty() || !reqs.is_empty();
    let mut first = true;
    if !reqs.is_empty() {
        label(ui, s::REQUESTS(), first);
        first = false;
        for rq in reqs {
            request_row(ui, rq.id, &rq.name, acts);
        }
    }
    if !playing.is_empty() {
        label(ui, s::PLAYING_NOW(), first);
        first = false;
        playing_now(ui, env, &playing, acts);
    }
    if rows.is_empty() {
        let p = theme::theme(ui.ctx()).palette.clone();
        ui.add_space(m::NONE[0]);
        let text = if q.is_empty() {
            s::EMPTY().to_owned()
        } else {
            s::none_called(&without_emoji(&raw))
        };
        let w = ui.available_width() - m::NONE[1] * 2.0;
        let mut job = Type::Body.job(&text, p.t3, w, usize::MAX);
        for sec in &mut job.sections {
            sec.format.line_height = Some(m::NONE[2]);
        }
        let g = ui.fonts_mut(|fo| fo.layout_job(job));
        let (r, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), g.size().y), Sense::hover());
        let at = Flip::new(r).rect(Rect::from_min_size(
            pos2(r.left() + m::NONE[1], r.top()),
            g.size(),
        ));
        ui.painter().galley(at.min, g, p.t3);
        crate::a11y::item(&resp, egui::Role::Label, &text);
        ui.add_space(m::NONE[0]);
    } else {
        if heads || !q.is_empty() {
            label(
                ui,
                if q.is_empty() {
                    s::CHATS()
                } else {
                    s::RESULTS()
                },
                first,
            );
        } else {
            ui.add_space(m::SPACER);
        }
        chat_rows(ui, state, env, snap, &rows, !q.is_empty(), acts);
    }
    if blocked > 0 && q.is_empty() && f.block {
        ui.add_space(m::BLOCKED_TOP);
        let text = s::blocked(blocked);
        let b = Button::new(&text).variant(Variant::Ghost);
        let w = b.width(ui);
        let (r, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), theme::tokens::control::BUTTON),
            Sense::hover(),
        );
        let at = Flip::new(r).rect(Rect::from_min_size(r.min, vec2(w, r.height())));
        if place(ui, at, b).clicked() {
            acts.push(Act::Dialog(ChatDialog::Blocked));
        }
    }
}

/// A request: avatar · name · "Wants to be friends", and Accept · Ignore under them.
fn request_row(ui: &mut Ui, id: i64, name: &str, acts: &mut Vec<Act>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width();
    let text_h = Type::Body.line_height() + m::NAME_GAP + Type::Caption.line_height();
    let btn = Size::Sm.height();
    let h = m::REQ[0] + text_h + m::REQ[1] + btn + m::REQ[0];
    let (r, resp) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    crate::a11y::item(
        &resp,
        egui::Role::Group,
        &crate::strings::chat::request_spoken(name),
    );
    let flip = Flip::new(r);
    let av = Rect::from_min_size(
        pos2(r.left() + m::ROW_PAD, r.top() + m::REQ[0]),
        vec2(m::AV, m::AV),
    );
    avatar::avatar(ui.painter(), flip.rect(av), name, None, Shape::Person);
    let x = av.right() + m::GAP;
    let tw = r.right() - m::ROW_PAD - x;
    let ng = super::emoji::Line::new(ui, Type::BodyStrong, name, p.t1, tw);
    let sg = Type::Caption.galley(ui, s::WANTS(), p.t3, tw, 1);
    let y = r.top() + m::REQ[0];
    let nr = flip.rect(Rect::from_min_size(pos2(x, y), ng.size()));
    let sh = ng.size().y;
    ng.paint(ui.painter(), nr.min, p.t1);
    let sr = flip.rect(Rect::from_min_size(
        pos2(x, y + sh + m::NAME_GAP),
        sg.size(),
    ));
    ui.painter().galley(sr.min, sg, p.t3);
    let by = y + text_h + m::REQ[1];
    let accept_name = s::accept_name(name);
    let ignore_name = s::ignore_name(name);
    let a = Button::primary(s::ACCEPT())
        .size(Size::Sm)
        .accessible_name(accept_name);
    let aw = a.width(ui);
    let ar = flip.rect(Rect::from_min_size(pos2(x, by), vec2(aw, btn)));
    let accepted = place(ui, ar, a).clicked();
    let i = Button::new(s::IGNORE())
        .variant(Variant::Secondary)
        .size(Size::Sm)
        .accessible_name(ignore_name);
    let iw = i.width(ui);
    let ir = flip.rect(Rect::from_min_size(
        pos2(x + aw + m::REQ[2], by),
        vec2(iw, btn),
    ));
    let ignored = place(ui, ir, i).clicked();
    if accepted {
        acts.push(Act::Accept(id));
    } else if ignored {
        acts.push(Act::Ignore(id));
    }
}

/// Playing now: the poster of each friend's game, their avatar over its corner.
fn playing_now(ui: &mut Ui, env: &Env<'_>, friends: &[&Friend], acts: &mut Vec<Act>) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let [pw, gap] = m::POSTER;
    let ph = pw * theme::tokens::layout::COVER_ASPECT;
    let [top, side, bottom] = m::PN;
    let h = top + ph + bottom;
    let w = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let mut hui = ui.new_child(UiBuilder::new().id_salt("chat.pn").max_rect(row));
    egui::ScrollArea::horizontal()
        .id_salt("chat.pn.scroll")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .auto_shrink([false, true])
        .show(&mut hui, |ui| {
            let n = friends.len() as f32;
            let inner_w =
                (side * 2.0 + n * pw + (n - 1.0).max(0.0) * gap).max(ui.available_width());
            let (span, _) = ui.allocate_exact_size(vec2(inner_w, h), Sense::hover());
            let flip = Flip::new(span);
            for (k, fr) in friends.iter().enumerate() {
                let Some(pl) = &fr.playing else { continue };
                let slot = flip.rect(Rect::from_min_size(
                    pos2(span.left() + side + k as f32 * (pw + gap), span.top() + top),
                    vec2(pw, ph),
                ));
                let id = Id::unique(("chat.pn.item", fr.id));
                let resp = ui.interact(slot, id, Sense::click());
                let lift =
                    hover_t(&resp, &theme) * m::LIFT * if theme.motion.reduced { 0.0 } else { 1.0 };
                let r = slot.translate(vec2(0.0, -lift));
                let shade = palette::Shade {
                    y: 6,
                    blur: 14,
                    color: palette::black(0.28),
                };
                crate::widgets::cover::paint_shadow(ui.painter(), r, shade, 1.0);
                avatar::small_poster(ui.painter(), r, &pl.title, &theme);
                if pl.live {
                    let [inset, sz, rad, dot] = m::LIVE;
                    let lr = flip_in(
                        r,
                        Rect::from_min_size(r.min + vec2(inset, inset), vec2(sz, sz)),
                    );
                    ui.painter().rect_filled(
                        lr,
                        CornerRadius::same(rad as u8),
                        palette::black(0.66),
                    );
                    let a = super::paint::pulse_alpha(ui.ctx());
                    ui.painter()
                        .circle_filled(lr.center(), dot / 2.0, fade(p.coral, a));
                }
                let [ad, out, ring] = m::PN_AV;
                let ar = flip_in(
                    r,
                    Rect::from_min_size(pos2(r.right() + out - ad, r.top() - out), vec2(ad, ad)),
                );
                ui.painter()
                    .circle_filled(ar.center(), ad / 2.0 + ring, p.bg);
                avatar::pictured(
                    ui.painter(),
                    ar,
                    &fr.name,
                    fr.paper,
                    Shape::Person,
                    fr.picture.as_ref(),
                );
                let time = crate::strings::fmt::play_time(u64::from(pl.minutes) * 60_000);
                crate::a11y::button(&resp, &s::playing_name(&fr.name, &pl.title));
                let resp = tooltip::attach(
                    resp,
                    &without_emoji(&s::playing_tip(
                        first_name(&fr.name),
                        &pl.title,
                        &time,
                        pl.live,
                    )),
                );
                if resp.has_focus() {
                    focus_ring(ui.painter(), r, crate::widgets::cover::corner(r), &theme);
                }
                if resp.clicked() {
                    acts.push(Act::Open(ChatRef::Direct(fr.id)));
                }
            }
            let _ = env;
        });
}

/// `inner`, placed inside `outer` left to right, mirrored in a right-to-left interface.
fn flip_in(outer: Rect, inner: Rect) -> Rect {
    Flip::new(outer).rect(inner)
}

/// The chats, one tab stop: the row with focus (or the open chat, or the first) is the
/// one Tab reaches; ↑ ↓ move, Enter opens.
fn chat_rows(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    rows: &[&ChatPreview],
    searching: bool,
    acts: &mut Vec<Act>,
) {
    let cursor_key = Id::unique("chat.list.cursor");
    let open = state.chat.open.as_ref().map(|o| o.chat);
    let mut cursor: Option<ChatRef> = ui
        .ctx()
        .data(|d| d.get_temp::<Option<ChatRef>>(cursor_key))
        .flatten();
    if let Some(f) = state.chat.focus_row {
        cursor = Some(f);
    }
    if !cursor.is_some_and(|c| rows.iter().any(|r| r.chat == c)) {
        cursor = open
            .filter(|o| rows.iter().any(|r| r.chat == *o))
            .or_else(|| rows.first().map(|r| r.chat));
    }
    let origin = ui.cursor().top();
    let mut ids = Vec::with_capacity(rows.len());
    let mut focused_at = None;
    for (k, c) in rows.iter().enumerate() {
        let id = Id::unique(("chat.row", c.chat));
        let resp = chat_row(
            ui,
            env,
            snap,
            c,
            id,
            open == Some(c.chat),
            cursor == Some(c.chat),
            origin,
            searching,
            acts,
        );
        own_arrow_keys(&resp);
        if state.chat.focus_row == Some(c.chat) {
            resp.request_focus();
            resp.scroll_to_me(None);
            state.chat.focus_row = None;
        }
        if resp.has_focus() {
            focused_at = Some(k);
            cursor = Some(c.chat);
        }
        if resp.clicked() {
            cursor = Some(c.chat);
            acts.push(Act::Open(c.chat));
        }
        ids.push(c.chat);
    }
    if let Some(i) = focused_at {
        let (up, down) = ui.input_mut(|inp| {
            (
                inp.consume_key(Modifiers::NONE, Key::ArrowUp),
                inp.consume_key(Modifiers::NONE, Key::ArrowDown),
            )
        });
        let to = if down {
            ids.get(i + 1)
        } else if up && i > 0 {
            ids.get(i - 1)
        } else {
            None
        };
        if let Some(chat) = to {
            state.chat.focus_row = Some(*chat);
            cursor = Some(*chat);
            ui.ctx().request_repaint();
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(cursor_key, cursor));
}

/// What sits under a row's time.
enum Mark {
    None,
    Count(u32),
    Muted,
    Talking,
}

#[allow(clippy::too_many_arguments)]
fn chat_row(
    ui: &mut Ui,
    env: &Env<'_>,
    snap: &ChatSnapshot,
    c: &ChatPreview,
    id: Id,
    current: bool,
    tab_stop: bool,
    origin: f32,
    searching: bool,
    acts: &mut Vec<Act>,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let f = snap.features;
    let name = chat_name(snap, c);
    let friend = match c.chat {
        ChatRef::Direct(id) => snap.friend(id),
        ChatRef::Group(_) => None,
    };
    let group: Option<&ChatGroup> = match c.chat {
        ChatRef::Group(g) => snap.group(g),
        ChatRef::Direct(_) => None,
    };
    let muted =
        f.mute && (c.muted || friend.is_some_and(|x| x.muted) || group.is_some_and(|g| g.muted));
    let talking = f.calls
        && (snap.live.call.as_ref().is_some_and(|k| k.chat == c.chat)
            || group.is_some_and(|g| !g.voice.is_empty()));
    let mark = if muted {
        Mark::Muted
    } else if c.unread > 0 {
        Mark::Count(c.unread)
    } else if talking {
        Mark::Talking
    } else {
        Mark::None
    };
    let unread = c.unread > 0;
    let pv = preview(snap, c);
    let when: std::sync::Arc<str> = c.last.as_ref().map_or_else(
        || std::sync::Arc::from(""),
        |msg| {
            crate::strings::chat::time::cached(
                ui.ctx(),
                crate::strings::chat::time::Kind::List,
                msg.at,
                env.now,
                env.utc_offset_s,
            )
        },
    );

    // The slot, and where the row is drawn while it slides to it.
    let w = ui.available_width();
    let (slot, _) = ui.allocate_exact_size(vec2(w, m::ROW_H), Sense::hover());
    let target = slot.top() - origin;
    let secs = if searching {
        0.0
    } else {
        theme.motion.movement(Dur::Move)
    };
    let y = theme::motion::glide(ui.ctx(), id.with("slide"), target, secs).round();
    let rect = slot.translate(vec2(0.0, y - target));
    let sense = if tab_stop {
        Sense::click()
    } else {
        Sense::CLICK
    };
    let resp = ui.interact(rect, id, sense);

    // Spoken: "Mira Okafor, Online, I'm in Lantern Keep…, 09:12, 2 new messages".
    let mut parts = vec![name.clone()];
    if let Some(fr) = friend
        && f.presence
    {
        parts.push(status_words(fr));
    }
    parts.push(pv.clone());
    if !when.is_empty() {
        parts.push(when.to_string());
    }
    match mark {
        Mark::Count(n) => parts.push(s::new_messages(n)),
        Mark::Muted => parts.push(s::MUTED().to_owned()),
        Mark::Talking => parts.push(s::TALKING().to_owned()),
        Mark::None => {}
    }
    let spoken = parts.join(s::SEP());
    crate::a11y::option(&resp, egui::Role::Button, &spoken, current);

    // The menu (right click).
    let menu_id = Popup::default_response_id(&resp);
    let menu_open = Popup::is_id_open(ui.ctx(), menu_id);
    let popup = Popup::context_menu(&resp).close_behavior(PopupCloseBehavior::CloseOnClickOutside);
    if let Some(Some(a)) =
        context_menu::popup(popup, |ui| row_menu(ui, snap, c, friend, group, muted))
    {
        acts.push(a);
    }

    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let hover = if menu_open {
        1.0
    } else {
        hover_t(&resp, &theme)
    };
    let pressed = press_t(&resp, &theme);
    let fill = if current {
        p.o2
    } else {
        mix(fade(p.o1, hover), p.o2, pressed)
    };
    let corner = CornerRadius::same(m::ROW_RADIUS);
    let painter = ui.painter();
    painter.rect_filled(rect, corner, fill);
    let ring = over(fill, p.bg);
    let flip = Flip::new(rect);

    // Avatar.
    let av = Rect::from_min_size(
        pos2(rect.left() + m::ROW_PAD, rect.center().y - m::AV / 2.0),
        vec2(m::AV, m::AV),
    );
    let av = flip.rect(av);
    match c.chat {
        ChatRef::Direct(_) => {
            avatar::pictured(
                painter,
                av,
                &name,
                friend.and_then(|x| x.paper),
                Shape::Person,
                friend.and_then(|x| x.picture.as_ref()),
            );
            if f.presence
                && let Some(fr) = friend
                && !fr.blocked
            {
                avatar::status(painter, av, fr.presence, ring, false);
            }
        }
        ChatRef::Group(_) => {
            let pic = group.and_then(|g| g.picture.as_ref());
            avatar::pictured(painter, av, &name, None, Shape::Group, pic);
        }
    }

    // The trailing column: the time over its mark.
    let time = super::paint::time_galley(ui, &when, Type::Micro, p.t3);
    let mark_size = match mark {
        Mark::Count(n) => avatar::pill_size(ui, n),
        Mark::Muted => vec2(m::MARK_ICON, m::MARK_ICON),
        Mark::Talking => crate::widgets::progress::live_size(),
        Mark::None => vec2(0.0, 0.0),
    };
    let has_mark = !matches!(mark, Mark::None);
    let col_w = time.size().x.max(mark_size.x);
    let col_h = time.size().y
        + if has_mark {
            m::META_GAP + mark_size.y
        } else {
            0.0
        };
    let right = rect.right() - m::ROW_PAD;
    let col_top = rect.center().y - col_h / 2.0;
    let tr = flip.rect(Rect::from_min_size(
        pos2(right - time.size().x, col_top),
        time.size(),
    ));
    painter.galley(tr.min, time, p.t3);
    let mr = flip.rect(Rect::from_min_size(
        pos2(
            right - mark_size.x,
            col_top + Type::Micro.line_height() + m::META_GAP,
        ),
        mark_size,
    ));
    match mark {
        Mark::Count(n) => avatar::paint_pill(painter, mr, n),
        Mark::Muted => {
            painter.text(
                mr.center(),
                Align2::CENTER_CENTER,
                icons::BELL_OFF.text(false),
                icons::font(m::MARK_ICON),
                p.t3,
            );
        }
        Mark::Talking => {
            painter.extend(crate::widgets::progress::live_shapes(
                ui.ctx(),
                mr,
                p.accent_text,
            ));
        }
        Mark::None => {}
    }

    // Name over preview.
    let x = rect.left() + m::ROW_PAD + m::AV + m::GAP;
    let tw = (right - col_w - m::GAP - x).max(1.0);
    let name_style = if unread {
        Type::BodyBold
    } else {
        Type::BodyStrong
    };
    let ng = super::emoji::Line::new(ui, name_style, &name, p.t1, tw);
    let pv_color = if unread { p.t1 } else { p.t3 };
    let pg = super::emoji::Line::new(ui, Type::Caption, &pv, pv_color, tw);
    let th = Type::Body.line_height() + m::NAME_GAP + Type::Caption.line_height();
    let ty = rect.center().y - th / 2.0;
    let nr = flip.rect(Rect::from_min_size(pos2(x, ty), ng.size()));
    ng.paint(painter, nr.min, p.t1);
    let pr = flip.rect(Rect::from_min_size(
        pos2(x, ty + Type::Body.line_height() + m::NAME_GAP),
        pg.size(),
    ));
    pg.paint(painter, pr.min, pv_color);
    if resp.has_focus() {
        focus_ring(painter, rect, corner, &theme);
    }
    resp
}

/// A row's menu: Mute notifications ✓ · Check the connection… · Block… · Remove friend…
/// (a group: Mute · Invite people… · Leave group…). Gated by what this Chat can do.
fn row_menu(
    ui: &mut Ui,
    snap: &ChatSnapshot,
    c: &ChatPreview,
    friend: Option<&Friend>,
    group: Option<&ChatGroup>,
    muted: bool,
) -> Option<Act> {
    let f = snap.features;
    let mut out = None;
    if f.mute
        && context_menu::icon_check_item(ui, icons::BELL_OFF, s::MUTE(), muted, false).clicked()
    {
        out = Some(Act::Mute(c.chat, !muted));
    }
    match (friend, group) {
        (Some(fr), _) => {
            if f.verify
                && context_menu::icon_item(ui, icons::SHIELD_CHECK, s::CHECK_CONNECTION(), None)
                    .clicked()
            {
                out = Some(Act::Dialog(ChatDialog::Verify(fr.id)));
            }
            if f.mute || f.verify {
                context_menu::separator(ui);
            }
            if f.block {
                if fr.blocked {
                    if context_menu::icon_item(
                        ui,
                        icons::PROHIBIT,
                        &s::unblock(first_name(&fr.name)),
                        None,
                    )
                    .clicked()
                    {
                        out = Some(Act::Unblock(fr.id));
                    }
                } else if context_menu::icon_danger_item(
                    ui,
                    icons::PROHIBIT,
                    &s::block(first_name(&fr.name)),
                    None,
                )
                .clicked()
                {
                    out = Some(Act::Dialog(ChatDialog::Block(fr.id)));
                }
            }
            if context_menu::icon_danger_item(ui, icons::USER_MINUS, s::REMOVE_FRIEND(), None)
                .clicked()
            {
                out = Some(Act::Dialog(ChatDialog::RemoveFriend(fr.id)));
            }
        }
        (None, Some(g)) => {
            if g.membership == Membership::Member
                && context_menu::icon_item(ui, icons::USER_PLUS, s::INVITE_PEOPLE(), None).clicked()
            {
                out = Some(Act::Dialog(ChatDialog::InviteToGroup(g.id)));
            }
            if f.mute || g.membership == Membership::Member {
                context_menu::separator(ui);
            }
            if context_menu::icon_danger_item(ui, icons::LEAVE, s::LEAVE_GROUP(), None).clicked() {
                out = Some(Act::Dialog(ChatDialog::LeaveGroup(g.id)));
            }
        }
        (None, None) => {}
    }
    out
}

/// The game the user is in, if it may be shown to friends ("Playing <game>").
fn my_game(env: &Env<'_>) -> Option<String> {
    env.bridge
        .library()
        .games
        .iter()
        .find(|g| !g.hidden && matches!(g.activity, easyplay_core::model::Activity::Running { .. }))
        .map(|g| g.name.to_string())
}

/// The "you" row: avatar with status · name · status words · caret, and its menu.
fn me_row(ui: &mut Ui, env: &Env<'_>, snap: &ChatSnapshot, rect: Rect, acts: &mut Vec<Act>) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let f = snap.features;
    let Some(me) = snap.profile.as_ref() else {
        return;
    };
    let resp = ui.interact(rect, Id::unique("chat.me"), Sense::click());
    crate::a11y::button(&resp, s::ME_NAME());
    let menu_id = Popup::default_response_id(&resp);
    let open = Popup::is_id_open(ui.ctx(), menu_id);
    let hover = if open { 1.0 } else { hover_t(&resp, &theme) };
    let fill = mix(fade(p.o1, hover), p.o2, press_t(&resp, &theme));
    let corner = CornerRadius::same(m::ROW_RADIUS);
    ui.painter().rect_filled(rect, corner, fill);
    let ring = over(fill, p.bg);
    let flip = Flip::new(rect);
    let [_, _, ad, gap] = m::ME;
    let av = flip.rect(Rect::from_min_size(
        pos2(rect.left() + m::ROW_PAD, rect.center().y - ad / 2.0),
        vec2(ad, ad),
    ));
    let pic = me.picture.as_ref();
    avatar::pictured(ui.painter(), av, &me.name, me.paper, Shape::Person, pic);
    if f.presence {
        avatar::status(
            ui.painter(),
            av,
            avatar::my_presence(me.status),
            ring,
            false,
        );
    }
    let caret = icon_size::SMALL;
    let cr = flip.rect(Rect::from_center_size(
        pos2(rect.right() - m::ROW_PAD - caret / 2.0, rect.center().y),
        vec2(caret, caret),
    ));
    ui.painter().text(
        cr.center(),
        Align2::CENTER_CENTER,
        icons::CARET_DOWN.text(false),
        icons::font(caret),
        p.t3,
    );
    let x = rect.left() + m::ROW_PAD + ad + gap;
    let tw = (rect.right() - m::ROW_PAD - caret - gap - x).max(1.0);
    let show_playing = env.bridge.prefs().chat.show_playing;
    let words = if f.presence {
        match my_game(env) {
            Some(g) if me.status != MyStatus::Invisible && show_playing => s::playing(&g),
            _ => my_status_words(me.status).to_owned(),
        }
    } else {
        String::new()
    };
    let ng = super::emoji::Line::new(ui, Type::BodyStrong, &me.name, p.t1, tw);
    let wg = super::emoji::Line::new(ui, Type::Caption, &words, p.t3, tw);
    let th = ng.size().y
        + if words.is_empty() {
            0.0
        } else {
            m::NAME_GAP + wg.size().y
        };
    let ty = rect.center().y - th / 2.0;
    let nh = ng.size().y;
    let nr = flip.rect(Rect::from_min_size(pos2(x, ty), ng.size()));
    ng.paint(ui.painter(), nr.min, p.t1);
    if !words.is_empty() {
        let wr = flip.rect(Rect::from_min_size(
            pos2(x, ty + nh + m::NAME_GAP),
            wg.size(),
        ));
        wg.paint(ui.painter(), wr.min, p.t3);
    }
    if resp.has_focus() {
        focus_ring(ui.painter(), rect, corner, &theme);
    }
    let popup = Popup::menu(&resp)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(if flip.rtl {
            RectAlign::TOP_END
        } else {
            RectAlign::TOP_START
        });
    if let Some(Some(a)) = context_menu::popup(popup, |ui| {
        let mut out = None;
        if f.presence {
            for (st, label) in [
                (MyStatus::Online, s::ONLINE()),
                (MyStatus::Away, s::AWAY()),
                (MyStatus::Busy, s::BUSY()),
                (MyStatus::Invisible, s::INVISIBLE()),
            ] {
                let dot = avatar::my_presence(st);
                if context_menu::dot_check_item(ui, label, me.status == st, false, |p, r, ring| {
                    avatar::status_dot(p, r.center(), dot, ring, false);
                })
                .clicked()
                {
                    out = Some(Act::Status(st));
                }
            }
            context_menu::separator(ui);
            if context_menu::icon_check_item(
                ui,
                icons::GAMEPAD,
                s::SHOW_PLAYING(),
                show_playing,
                false,
            )
            .clicked()
            {
                out = Some(Act::ShowPlaying(!show_playing));
            }
            context_menu::separator(ui);
        }
        if context_menu::icon_item(ui, icons::PENCIL, s::EDIT_PROFILE(), None).clicked() {
            out = Some(Act::Dialog(ChatDialog::Profile));
            ui.close();
        }
        if context_menu::icon_item(ui, icons::GEAR, s::CHAT_SETTINGS(), None).clicked() {
            out = Some(Act::Settings);
            ui.close();
        }
        out
    }) {
        acts.push(a);
    }
}
