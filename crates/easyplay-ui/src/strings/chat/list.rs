//! Chat strings: list (ADR-0033). Keys live under `chat.list.` in locales/en.yml and ar.yml,
//! between that file's `chat.list` markers. The page's frame (the invitation, the Off page's
//! headline), the list column, its menus and the "you" row.

use easyplay_core::clock::Timestamp;

texts! {
    /// The search field's placeholder and name.
    SEARCH = "chat.list.search";
    /// The "+" button: tooltip and name.
    NEW = "chat.list.new";
    /// Menu item of "+".
    ADD_FRIEND = "chat.list.add_friend";
    /// Menu item of "+".
    NEW_GROUP = "chat.list.new_group";
    /// Section label.
    REQUESTS = "chat.list.requests";
    /// Section label.
    PLAYING_NOW = "chat.list.playing_now";
    /// Section label.
    CHATS = "chat.list.chats";
    /// Section label while searching.
    RESULTS = "chat.list.results";
    /// Under a request's name.
    WANTS = "chat.list.wants";
    /// Accept a request.
    ACCEPT = "chat.list.accept";
    /// Ignore a request.
    IGNORE = "chat.list.ignore";
    /// The list has no chat.
    EMPTY = "chat.list.empty";
    /// The preview of a chat without messages.
    NO_MESSAGES = "chat.list.no_messages";
    /// The preview of a voice message.
    VOICE = "chat.list.voice";
    /// The preview of a GIF.
    GIF = "chat.list.gif";
    /// The preview of a call that happened.
    CALL_ENDED = "chat.list.call_ended";
    /// The preview of a missed call.
    MISSED_CALL = "chat.list.missed_call";
    /// The preview of a declined call.
    CALL_DECLINED = "chat.list.call_declined";
    /// The preview of a deleted message.
    DELETED = "chat.list.deleted";
    /// The preview of a group the user is invited to.
    INVITED = "chat.list.invited";
    /// Spoken for the live bars of a row.
    TALKING = "chat.list.talking";
    /// Spoken for the bell of a muted row.
    MUTED = "chat.list.muted";
    /// Row menu: a check item.
    MUTE = "chat.list.mute";
    /// Row menu.
    CHECK_CONNECTION = "chat.list.check_connection";
    /// Row menu.
    REMOVE_FRIEND = "chat.list.remove_friend";
    /// Row menu of a group.
    INVITE_PEOPLE = "chat.list.invite_people";
    /// Row menu of a group.
    LEAVE_GROUP = "chat.list.leave_group";
    /// Status words.
    ONLINE = "chat.list.online";
    /// Status words.
    AWAY = "chat.list.away";
    /// Status words.
    BUSY = "chat.list.busy";
    /// Status words.
    INVISIBLE = "chat.list.invisible";
    /// Status words.
    OFFLINE = "chat.list.offline";
    /// "You" menu: a check item.
    SHOW_PLAYING = "chat.list.show_playing";
    /// "You" menu.
    EDIT_PROFILE = "chat.list.edit_profile";
    /// "You" menu.
    CHAT_SETTINGS = "chat.list.chat_settings";
    /// The "you" row's name.
    ME_NAME = "chat.list.me_name";
    /// The rows' list, as a screen reader names it.
    LIST_NAME = "chat.list.list_name";
    /// The invitation's headline (no friends yet; also the Off page's).
    INVITE_TITLE = "chat.list.invite_title";
    /// Under it.
    INVITE_BODY = "chat.list.invite_body";
    /// Fine print under the invitation's button.
    INVITE_FINE = "chat.list.invite_fine";
    /// The invitation's button.
    INVITE_ADD = "chat.list.invite_add";
    /// Between the parts of a spoken row.
    SEP = "chat.list.sep";
    /// The Off page's sentence: what Chat is, and the servers it uses (named before first
    /// use: messages and pictures pass through them).
    OFF_BODY = "chat.list.off_body";
}

/// "You: see you" (the preview of the user's own message).
pub fn you_said(text: &str) -> String {
    rust_i18n::t!("chat.list.you_said", text = text).into_owned()
}

/// "Jonas: see you" (the preview of a group member's message).
pub fn member_said(name: &str, text: &str) -> String {
    rust_i18n::t!("chat.list.member_said", name = name, text = text).into_owned()
}

/// "Sharing Lantern Keep".
pub fn sharing(game: &str) -> String {
    rust_i18n::t!("chat.list.sharing", game = game).into_owned()
}

/// "Shared Lantern Keep".
pub fn shared(game: &str) -> String {
    rust_i18n::t!("chat.list.shared", game = game).into_owned()
}

/// "Playing Lantern Keep" (status words).
pub fn playing(game: &str) -> String {
    rust_i18n::t!("chat.list.playing", game = game).into_owned()
}

/// "Accept Kit".
pub fn accept_name(name: &str) -> String {
    rust_i18n::t!("chat.list.accept_name", name = name).into_owned()
}

/// "Ignore Kit".
pub fn ignore_name(name: &str) -> String {
    rust_i18n::t!("chat.list.ignore_name", name = name).into_owned()
}

/// "No one called “zed”."
pub fn none_called(q: &str) -> String {
    rust_i18n::t!("chat.list.none_called", q = q).into_owned()
}

/// "Blocked (2)".
pub fn blocked(n: usize) -> String {
    rust_i18n::t!("chat.list.blocked", n = n).into_owned()
}

/// "Block Mira…".
pub fn block(name: &str) -> String {
    rust_i18n::t!("chat.list.block", name = name).into_owned()
}

/// "Unblock Mira".
pub fn unblock(name: &str) -> String {
    rust_i18n::t!("chat.list.unblock", name = name).into_owned()
}

/// "Mira is playing Lantern Keep, 42 min" (`live`: ", and sharing it live").
pub fn playing_tip(name: &str, game: &str, time: &str, live: bool) -> String {
    if live {
        rust_i18n::t!(
            "chat.list.playing_tip_live",
            name = name,
            game = game,
            time = time
        )
        .into_owned()
    } else {
        rust_i18n::t!(
            "chat.list.playing_tip",
            name = name,
            game = game,
            time = time
        )
        .into_owned()
    }
}

/// "Mira Okafor playing Lantern Keep" (a poster of Playing now, spoken; no comma, so
/// that it never reads as the start of a chat row's name).
pub fn playing_name(name: &str, game: &str) -> String {
    rust_i18n::t!("chat.list.playing_name", name = name, game = game).into_owned()
}

/// "2 new messages".
pub fn new_messages(n: u32) -> String {
    crate::i18n::plural::counted("chat.list.new_messages", u64::from(n))
}

/// "3 unread messages" (the top bar's count, spoken).
pub fn unread_total(n: u32) -> String {
    crate::i18n::plural::counted("chat.list.unread_total", u64::from(n))
}

/// A chat's time in the list: "09:12" today, "Yesterday", the weekday within a week, then
/// "3 Nov" (with the year in another year). `offset_s` is the local UTC offset.
pub fn when_short(t: Timestamp, now: Timestamp, offset_s: i32) -> String {
    let local = |t: Timestamp| (t.as_millis() + i64::from(offset_s) * 1000).div_euclid(86_400_000);
    let (d, today) = (local(t), local(now));
    match today - d {
        i64::MIN..=0 => super::super::stats::clock_time(t, offset_s),
        1 => super::super::date::yesterday().to_owned(),
        2..=6 => super::super::stats::WEEKDAY_SHORT()[(d + 3).rem_euclid(7) as usize].to_owned(),
        _ => super::short_time(t, now, offset_s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_today_yesterday_weekday_then_date() {
        crate::i18n::set(crate::i18n::Language::English);
        // 2026-11-17 is a Tuesday.
        let now = Timestamp::from_millis(1_794_873_600_000 + 10 * 3_600_000);
        let ago = |h: i64| Timestamp::from_millis(now.as_millis() - h * 3_600_000);
        assert_eq!(when_short(ago(1), now, 0), "09:00");
        assert_eq!(
            when_short(ago(24), now, 0),
            crate::strings::date::yesterday()
        );
        assert_eq!(when_short(ago(24 * 4), now, 0), "Fri");
        assert_eq!(when_short(ago(24 * 9), now, 0), "8 Nov");
    }
}
