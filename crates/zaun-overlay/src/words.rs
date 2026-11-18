//! What the overlay says, in the user's language: the app's own catalogue
//! (`zaun_ui::i18n`, `locales/*.yml`), so the overlay and the window never disagree and
//! a translation is made once (ADR-0037 §6). The language follows `general.language` and
//! the app's locale, resolved as the window resolves them.
use zaun_core::chat::overlay::snapshot::{BodyKind, CallLine, OverlayMessage, Status};
use zaun_core::chat::overlay::{Corner, OverlayLook};
use zaun_core::clock::Timestamp;
use zaun_ui::i18n::{self, Dir, Language, LanguageChoice};
use zaun_ui::strings::chat as s;

/// Make the snapshot's language the overlay's.
pub fn set_language(look: &OverlayLook) {
    let choice = LanguageChoice::parse(&look.language);
    let language = i18n::resolve(choice, Some(look.locale.as_str()).filter(|l| !l.is_empty()));
    if language != i18n::language() {
        i18n::set(language);
    }
}

pub fn language() -> Language {
    i18n::language()
}

pub fn rtl() -> bool {
    i18n::dir() == Dir::Rtl
}

pub fn text(key: &str) -> &'static str {
    i18n::text(key)
}

/// A sentence with a key cap in it ("%{key} to reply"): the parts before and after the
/// key, in reading order.
pub fn around_key(key: &str) -> (String, String) {
    let t = text(key);
    match t.split_once("%{key}") {
        Some((a, b)) => (a.to_owned(), b.to_owned()),
        None => (t.to_owned(), String::new()),
    }
}

pub fn in_group(group: &str) -> String {
    text("overlay.in_group").replace("%{group}", group)
}

pub fn more(n: u32) -> String {
    text("overlay.more").replace("%{n}", &n.to_string())
}

pub fn voice_in(group: &str) -> String {
    text("overlay.voice_in").replace("%{group}", group)
}

pub fn members(n: u32, online: u32) -> String {
    s::conversation::group_status(n, online)
}

pub fn composer(name: &str) -> String {
    s::composer(name)
}

pub fn status(st: Status) -> &'static str {
    text(match st {
        Status::Online => "chat.list.online",
        Status::Away => "chat.list.away",
        Status::Busy => "chat.list.busy",
        Status::Offline => "chat.list.offline",
    })
}

pub fn corner(c: Corner) -> &'static str {
    text(match c {
        Corner::TopLeft => "chat.settings.corner_tl",
        Corner::TopRight => "chat.settings.corner_tr",
        Corner::BottomLeft => "chat.settings.corner_bl",
        Corner::BottomRight => "chat.settings.corner_br",
    })
}

/// A toast's line: the text, or what kind of message it is.
pub fn kind_text(kind: BodyKind, text: &str) -> String {
    body(
        &OverlayMessage {
            id: 0,
            author: zaun_core::chat::overlay::snapshot::Person::default(),
            me: false,
            kind,
            text: text.to_owned(),
            notice: None,
            call: None,
            at: 0,
            unread: false,
            mentions: Vec::new(),
        },
        "",
    )
}

/// What a message says when it is not plain text, or its text.
pub fn body(m: &OverlayMessage, friend: &str) -> String {
    match m.kind {
        BodyKind::Text => m.text.clone(),
        BodyKind::Notice => m
            .notice
            .clone()
            .map(|n| s::conversation::notice(&n.into(), friend))
            .unwrap_or_default(),
        BodyKind::Call => call_line(m.call),
        BodyKind::Voice => text("chat.list.voice").to_owned(),
        BodyKind::Gif => text("chat.list.gif").to_owned(),
        BodyKind::Deleted => text("chat.list.deleted").to_owned(),
        BodyKind::Stream => text("chat.list.shared").replace("%{game}", &m.text),
        BodyKind::Picture | BodyKind::Video | BodyKind::File => {
            if m.text.is_empty() {
                s::conversation::file_preview("")
            } else if m.kind == BodyKind::File {
                s::conversation::file_preview(&m.text)
            } else {
                m.text.clone()
            }
        }
    }
}

fn call_line(c: Option<CallLine>) -> String {
    match c {
        Some(CallLine::Ended { minutes }) => s::conversation::call_ended(
            &zaun_ui::strings::fmt::play_time(u64::from(minutes) * 60_000),
        ),
        Some(CallLine::Missed) => text("chat.list.missed_call").to_owned(),
        Some(CallLine::Declined) => text("chat.conversation.call_declined").to_owned(),
        None => text("chat.list.call_ended").to_owned(),
    }
}

/// The list's preview: "You: …", "Dev: …" in groups, the text otherwise.
pub fn preview(m: &OverlayMessage, group: bool, friend: &str) -> String {
    let b = body(m, friend);
    if m.me && m.kind != BodyKind::Notice {
        s::you_said(&b)
    } else if group && m.kind != BodyKind::Notice {
        s::member_said(first_name(&m.author.name), &b)
    } else {
        b
    }
}

/// "Dev Patel" → "Dev" (the widget's and the toasts' names).
pub fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

pub fn list_time(at: i64, now: Timestamp, offset_s: i32) -> String {
    s::time::list_time(language(), Timestamp::from_millis(at), now, offset_s)
}

pub fn head_time(at: i64, now: Timestamp, offset_s: i32) -> String {
    s::time::head_time(language(), Timestamp::from_millis(at), now, offset_s)
}

pub fn clock(at: i64, offset_s: i32) -> String {
    s::time::clock(Timestamp::from_millis(at), offset_s)
}

pub fn day(at: i64, offset_s: i32) -> String {
    s::time::day_title(language(), Timestamp::from_millis(at), offset_s)
}
