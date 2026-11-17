//! Chat strings: conversation (ADR-0033). Keys live under `chat.conversation.` in
//! locales/en.yml and ar.yml, between that file's `chat.conversation` markers. The header,
//! its menu, the transcript's rows, the message menu and what a screen reader says.

use crate::i18n::plural::{counted, counted_args};

texts! {
    /// The narrow window's back button.
    BACK = "chat.conversation.back";
    /// The header's details button (a friend).
    PROFILE = "chat.conversation.profile";
    /// The header's details button (a group).
    GROUP_DETAILS = "chat.conversation.group_details";
    /// The header's menu button.
    MORE = "chat.conversation.more";
    /// Call a friend.
    CALL = "chat.conversation.call";
    /// Join a group's voice.
    JOIN_VOICE = "chat.conversation.join_voice";
    /// Start a group's voice.
    START_VOICE = "chat.conversation.start_voice";
    /// The call button while in this call.
    IN_CALL = "chat.conversation.in_call";
    /// Share your game.
    SHARE = "chat.conversation.share";
    /// Stop sharing it.
    STOP_SHARE = "chat.conversation.stop_share";
    /// The live mark.
    LIVE = "chat.conversation.live";
    /// Status words.
    ONLINE = "chat.conversation.online";
    /// Status words.
    AWAY = "chat.conversation.away";
    /// Status words.
    BUSY = "chat.conversation.busy";
    /// Status words.
    OFFLINE = "chat.conversation.offline";
    /// Status words of someone blocked.
    BLOCKED = "chat.conversation.blocked";
    /// Still connecting.
    CONNECTING = "chat.conversation.connecting";
    /// "Last seen yesterday".
    SEEN_YESTERDAY = "chat.conversation.seen_yesterday";
    /// "Last seen just now".
    SEEN_NOW = "chat.conversation.seen_now";
    /// Menu: mute (a tick when on).
    MUTE = "chat.conversation.mute";
    /// Menu: compare security codes.
    CHECK = "chat.conversation.check";
    /// Menu: invite friends into a group (a submenu of friends).
    INVITE = "chat.conversation.invite";
    /// Nobody to invite.
    NO_ONE = "chat.conversation.no_one";
    /// Why.
    NO_ONE_WHY = "chat.conversation.no_one_why";
    /// Menu: remove a friend.
    REMOVE_FRIEND = "chat.conversation.remove_friend";
    /// Menu: leave a group.
    LEAVE_GROUP = "chat.conversation.leave_group";
    /// The start of a group.
    START_GROUP = "chat.conversation.start_group";
    /// After it in a direct chat.
    ENCRYPTED_DM = "chat.conversation.encrypted_dm";
    /// After it in a group.
    ENCRYPTED_GROUP = "chat.conversation.encrypted_group";
    /// Above the first unread message.
    NEW = "chat.conversation.new";
    /// The user, as an author.
    YOU = "chat.conversation.you";
    /// After an edited message.
    EDITED = "chat.conversation.edited";
    /// Under a message waiting for the network.
    WAITING_NET = "chat.conversation.waiting_net";
    /// Under a message waiting in a group.
    WAITING_THEY = "chat.conversation.waiting_they";
    /// Under a message that could not be sent.
    NOT_SENT = "chat.conversation.not_sent";
    /// Message menu.
    REPLY = "chat.conversation.reply";
    /// Message menu (opens the emoji picker).
    ADD_REACTION_MENU = "chat.conversation.add_reaction_menu";
    /// The tools' button.
    ADD_REACTION = "chat.conversation.add_reaction";
    /// Message menu.
    COPY = "chat.conversation.copy";
    /// Message menu.
    SAVE_GIF = "chat.conversation.save_gif";
    /// Message menu.
    UNSAVE_GIF = "chat.conversation.unsave_gif";
    /// The star of a saved GIF.
    GIF_SAVED_TIP = "chat.conversation.gif_saved_tip";
    /// Message menu.
    EDIT = "chat.conversation.edit";
    /// Message menu.
    DELETE = "chat.conversation.delete";
    /// The toast after deleting (with Undo).
    DELETED_TOAST = "chat.conversation.deleted_toast";
    /// Toast.
    COPIED = "chat.conversation.copied";
    /// Toast.
    GIF_SAVED = "chat.conversation.gif_saved";
    /// Toast.
    GIF_REMOVED = "chat.conversation.gif_removed";
    /// A voice message's play button.
    PLAY = "chat.conversation.play";
    /// Its pause button.
    PAUSE = "chat.conversation.pause";
    /// Its speed button.
    SPEED = "chat.conversation.speed";
    /// A voice message, spoken.
    VOICE = "chat.conversation.voice";
    /// The tag on a GIF.
    GIF = "chat.conversation.gif";
    /// A call that was declined.
    CALL_DECLINED = "chat.conversation.call_declined";
    /// Beside a missed call.
    CALL_BACK = "chat.conversation.call_back";
    /// Nobody watches a shared game yet.
    NOBODY_WATCHING = "chat.conversation.nobody_watching";
    /// Watch a shared game.
    WATCH = "chat.conversation.watch";
    /// Stop watching.
    STOP_WATCHING = "chat.conversation.stop_watching";
    /// Stop your own sharing.
    STOP = "chat.conversation.stop";
    /// A collapsed message of someone blocked.
    HIDDEN = "chat.conversation.hidden";
    /// Shows it.
    SHOW = "chat.conversation.show";
    /// A message deleted for everyone.
    DELETED = "chat.conversation.deleted";
    /// A spoiler, spoken.
    SPOILER = "chat.conversation.spoiler";
    /// A spoiler's tooltip.
    SPOILER_TIP = "chat.conversation.spoiler_tip";
    /// A secret in a quoted line.
    SPOILER_WORD = "chat.conversation.spoiler_word";
    /// Under the messages' list while older ones load.
    LOADING_OLDER = "chat.conversation.loading_older";
    /// Loading failed.
    LOAD_FAILED = "chat.conversation.load_failed";
    /// A picture (the list's preview, what a screen reader says).
    PHOTO = "chat.conversation.photo";
    /// A video (the list's preview, what a screen reader says).
    VIDEO_WORD = "chat.conversation.video_word";
    /// Fetch an offered picture, video or file.
    DOWNLOAD = "chat.conversation.download";
    /// Stop fetching or sending it.
    CANCEL = "chat.conversation.cancel";
    /// Open a received file with the system's app.
    OPEN = "chat.conversation.open";
    /// Close the picture viewer.
    CLOSE = "chat.conversation.close";
    /// Save a copy of a message's file where the user chooses.
    SAVE_AS = "chat.conversation.save_as";
    /// The viewer's previous picture.
    PREVIOUS = "chat.conversation.previous";
    /// The viewer's next picture.
    NEXT = "chat.conversation.next";
    /// Under the user's own media that could not be sent.
    MEDIA_NOT_SENT = "chat.conversation.media_not_sent";
    /// Under someone's media that could not be fetched.
    MEDIA_NOT_RECEIVED = "chat.conversation.media_not_received";
    /// A file whose fetching or sending was stopped.
    CANCELLED = "chat.conversation.cancelled";
}

fn t(key: &str, args: &[(&str, &str)]) -> String {
    let mut s = crate::i18n::text(key).to_owned();
    for (k, v) in args {
        s = s.replace(&format!("%{{{k}}}"), v);
    }
    s
}

/// "Mira is offline".
pub fn offline(name: &str) -> String {
    t("chat.conversation.offline_tip", &[("name", name)])
}

/// The live pill: "Watch Jonas".
pub fn watch(name: &str) -> String {
    t("chat.conversation.watch_name", &[("name", name)])
}

/// Its tooltip: "Watch Jonas play Ninefold Arena".
pub fn watch_tip(name: &str, game: &str) -> String {
    t(
        "chat.conversation.watch_tip",
        &[("name", name), ("game", game)],
    )
}

/// "Playing Lantern Keep, 42 min".
pub fn playing(game: &str, time: &str) -> String {
    t(
        "chat.conversation.playing",
        &[("game", game), ("time", time)],
    )
}

/// "Away for 20 min".
pub fn away_for(time: &str) -> String {
    t("chat.conversation.away_for", &[("time", time)])
}

/// "Last seen 12 min ago".
pub fn seen_minutes(n: u64) -> String {
    counted("chat.conversation.seen_minutes", n)
}

/// "Last seen 3 hours ago".
pub fn seen_hours(n: u64) -> String {
    counted("chat.conversation.seen_hours", n)
}

/// "Last seen 4 days ago".
pub fn seen_days(n: u64) -> String {
    counted("chat.conversation.seen_days", n)
}

/// "5 members, 5 online".
pub fn group_status(members: u32, online: u32) -> String {
    counted_args(
        "chat.conversation.group_status",
        u64::from(online),
        &[("members", &super::members(members))],
    )
}

/// "Block Mira…".
pub fn block(name: &str) -> String {
    t("chat.conversation.block", &[("name", name)])
}

/// "Unblock Mira".
pub fn unblock(name: &str) -> String {
    t("chat.conversation.unblock", &[("name", name)])
}

/// The toast after blocking a group member who is not a friend.
pub fn member_blocked(name: &str) -> String {
    t("chat.conversation.member_blocked", &[("name", name)])
}

/// "Invite Ana" (a friend in the Invite submenu), spoken with the group.
pub fn invite_to(friend: &str, group: &str) -> String {
    t(
        "chat.conversation.invite_to",
        &[("friend", friend), ("group", group)],
    )
}

/// "Jonas invited you to LAN night".
pub fn invited_by(who: &str, group: &str) -> String {
    t(
        "chat.conversation.invited_by",
        &[("who", who), ("group", group)],
    )
}

/// A notice of the conversation, in words. `friend` names the other side of a direct
/// chat ("You and Mira are now friends").
pub fn notice(n: &easyplay_core::model::ChatNotice, friend: &str) -> String {
    use easyplay_core::model::ChatNotice as N;
    let k = |key: &str| format!("chat.conversation.notice.{key}");
    match n {
        N::Connected => t(&k("connected"), &[("name", friend)]),
        N::GroupCreated => t(&k("group_created"), &[]),
        N::YouJoined => t(&k("you_joined"), &[]),
        N::MemberJoined(name) => t(&k("member_joined"), &[("name", name)]),
        N::MemberAdded { by: Some(by), who } => t(&k("member_added"), &[("by", by), ("who", who)]),
        N::MemberAdded { by: None, who } => t(&k("you_added"), &[("who", who)]),
        N::MemberLeft(name) => t(&k("member_left"), &[("name", name)]),
        N::MemberRemoved { by: Some(by), who } => {
            t(&k("member_removed"), &[("by", by), ("who", who)])
        }
        N::MemberRemoved { by: None, who } => t(&k("you_removed"), &[("who", who)]),
        N::YouLeft => t(&k("you_left"), &[]),
        N::YouRemoved(by) => t(&k("removed_you"), &[("by", by)]),
        N::GroupDeleted => t(&k("group_deleted"), &[]),
        N::GroupNameChanged(name) => t(&k("group_renamed"), &[("name", name)]),
        N::NameChanged { from, to } => t(&k("name_changed"), &[("from", from), ("to", to)]),
        N::FriendDeleted => t(&k("friend_deleted"), &[("name", friend)]),
        N::GroupInvitation { group, sent: false } => {
            t(&k("invited_you"), &[("name", friend), ("group", group)])
        }
        N::GroupInvitation { group, sent: true } => {
            t(&k("you_invited"), &[("name", friend), ("group", group)])
        }
        N::CodeChanged => t(&k("code_changed"), &[]),
    }
}

/// "File: shaders.zip" (the list's preview).
pub fn file_preview(name: &str) -> String {
    t("chat.conversation.file_preview", &[("name", name)])
}

/// A file's size: "640 KB", "2.4 MB", "1.2 GB".
pub fn size(n: u64) -> String {
    const K: f64 = 1000.0;
    let f = n as f64;
    if f < K {
        format!("{n} B")
    } else if f < K * K {
        format!("{:.0} KB", f / K)
    } else if f < K * K * K {
        format!("{:.1} MB", f / (K * K))
    } else {
        format!("{:.1} GB", f / (K * K * K))
    }
}

/// "Downloading, 45%".
pub fn receiving(percent: u8) -> String {
    t(
        "chat.conversation.receiving",
        &[("percent", &percent.to_string())],
    )
}

/// "Sending, 45%".
pub fn sending(percent: u8) -> String {
    t(
        "chat.conversation.sending_percent",
        &[("percent", &percent.to_string())],
    )
}

/// A picture, spoken: "Photo, 2.4 MB".
pub fn photo_spoken(size: &str) -> String {
    t("chat.conversation.photo_spoken", &[("size", size)])
}

/// A video, spoken: "Video, 0:42, 18.0 MB".
pub fn video_spoken(len: &str, size: &str) -> String {
    t(
        "chat.conversation.video_spoken",
        &[("len", len), ("size", size)],
    )
}

/// A file, spoken: "File shaders.zip, 3.1 MB".
pub fn file_spoken(name: &str, size: &str) -> String {
    t(
        "chat.conversation.file_spoken",
        &[("name", name), ("size", size)],
    )
}

/// "This is the start of your chat with Mira."
pub fn start_dm(name: &str) -> String {
    t("chat.conversation.start_dm", &[("name", name)])
}

/// "Waiting to be delivered. It arrives when Tomasz is back online."
pub fn waiting_for(name: &str) -> String {
    t("chat.conversation.waiting_for", &[("name", name)])
}

/// "React with thumbs up".
pub fn react_with(name: &str) -> String {
    t("chat.conversation.react_with", &[("name", name)])
}

/// A reaction pill, spoken: "👍 2, you reacted".
pub fn reaction_spoken(emoji: &str, n: u32, mine: bool) -> String {
    let key = if mine {
        "chat.conversation.reaction_mine"
    } else {
        "chat.conversation.reaction"
    };
    counted_args(key, u64::from(n), &[("emoji", emoji)])
}

/// A voice message's length, spoken: "Voice message, 0:14".
pub fn voice_spoken(len: &str) -> String {
    t("chat.conversation.voice_spoken", &[("len", len)])
}

/// "GIF: let's go".
pub fn gif_spoken(word: &str) -> String {
    t("chat.conversation.gif_spoken", &[("word", word)])
}

/// The toast after a file from a chat was saved: "Saved to Downloads" (core agent).
pub fn saved_to(folder: &str) -> String {
    t("chat.conversation.saved_to", &[("folder", folder)])
}

/// "Call ended, 38 min".
pub fn call_ended(time: &str) -> String {
    t("chat.conversation.call_ended", &[("time", time)])
}

/// "Missed call from Aiko".
pub fn missed_from(name: &str) -> String {
    t("chat.conversation.missed_from", &[("name", name)])
}

/// "Jonas is sharing Ninefold Arena".
pub fn sharing(name: &str, game: &str) -> String {
    t(
        "chat.conversation.sharing",
        &[("name", name), ("game", game)],
    )
}

/// "You are sharing Ninefold Arena".
pub fn you_sharing(game: &str) -> String {
    t("chat.conversation.you_sharing", &[("game", game)])
}

/// "Jonas shared Ninefold Arena".
pub fn shared(name: &str, game: &str) -> String {
    t(
        "chat.conversation.shared",
        &[("name", name), ("game", game)],
    )
}

/// "You shared Ninefold Arena".
pub fn you_shared(game: &str) -> String {
    t("chat.conversation.you_shared", &[("game", game)])
}

/// "2 people watching".
pub fn watching(n: u32) -> String {
    counted("chat.conversation.watching", u64::from(n))
}

/// "For 54 min".
pub fn for_time(time: &str) -> String {
    t("chat.conversation.for_time", &[("time", time)])
}

/// "Opens ninefold.example in your browser".
pub fn opens(host: &str) -> String {
    t("chat.conversation.opens", &[("host", host)])
}

/// A reply line, spoken: "Replying to Mira: so? did it work".
pub fn reply_spoken(name: &str, text: &str) -> String {
    t(
        "chat.conversation.reply_spoken",
        &[("name", name), ("text", text)],
    )
}

/// A message, spoken: "Mira, 08:42: so? did it work".
pub fn message_spoken(author: &str, when: &str, text: &str) -> String {
    t(
        "chat.conversation.message_spoken",
        &[("author", author), ("when", when), ("text", text)],
    )
}

/// The transcript's name: "Messages with Mira".
pub fn transcript(name: &str) -> String {
    t("chat.conversation.transcript", &[("name", name)])
}

#[cfg(test)]
mod tests {
    use easyplay_core::model::ChatNotice as N;

    use super::*;

    #[test]
    fn notices_read_as_sentences() {
        crate::i18n::set(crate::i18n::Language::English);
        assert_eq!(
            notice(&N::Connected, "Alaa"),
            "You and Alaa are now friends"
        );
        assert_eq!(
            notice(
                &N::MemberAdded {
                    by: Some("Jonas".into()),
                    who: "Aiko".into()
                },
                ""
            ),
            "Jonas added Aiko"
        );
        assert_eq!(
            notice(
                &N::MemberRemoved {
                    by: None,
                    who: "Rex".into()
                },
                ""
            ),
            "You removed Rex"
        );
        assert_eq!(
            notice(
                &N::GroupInvitation {
                    group: "LAN night".into(),
                    sent: true
                },
                "Mira"
            ),
            "You invited Mira to LAN night"
        );
        assert_eq!(size(2_400_000), "2.4 MB");
        assert_eq!(size(640_000), "640 KB");
    }
}
