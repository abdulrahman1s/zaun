//! Chat strings: details (ADR-0033). Keys live under `chat.details.` in locales/en.yml and ar.yml,
//! between that file's `chat.details` markers. The incoming call card, the top bar's call
//! pill, the trays, the details panel and the page's dialogs.

use crate::i18n::plural;

texts! {
    // ---- incoming call, top bar
    /// Under the caller's name.
    CALLING_YOU = "chat.details.calling_you";
    /// The card's buttons.
    DECLINE = "chat.details.decline";
    /// See [`DECLINE`].
    ANSWER = "chat.details.answer";
    /// A call that has not been answered yet.
    RINGING = "chat.details.ringing";
    /// The live mark while the user shares.
    SHARING = "chat.details.sharing";
    /// Its tooltip.
    SHARING_TIP = "chat.details.sharing_tip";
    // ---- call buttons
    /// Mute's tooltip.
    MUTE = "chat.details.mute";
    /// Mute's tooltip while muted.
    UNMUTE = "chat.details.unmute";
    /// Mute's accessible name.
    MUTE_MIC = "chat.details.mute_mic";
    /// Stop hearing's tooltip and name.
    DEAFEN = "chat.details.deafen";
    /// Stop hearing's tooltip while on.
    UNDEAFEN = "chat.details.undeafen";
    /// The share button's tooltip.
    SHARE_TIP = "chat.details.share_tip";
    /// The share button's tooltip while sharing.
    STOP_SHARE_TIP = "chat.details.stop_share_tip";
    /// Leave.
    LEAVE_CALL = "chat.details.leave_call";
    /// Leave while it still rings.
    CANCEL_CALL = "chat.details.cancel_call";
    /// Stop sharing.
    STOP_SHARING = "chat.details.stop_sharing";
    // ---- trays
    /// A group's voice with only the user in it.
    JUST_YOU = "chat.details.just_you";
    /// Under "Jonas and Dev are talking".
    VOICE_HERE = "chat.details.voice_here";
    /// Join a group's voice.
    JOIN = "chat.details.join";
    /// Your own sharing: what is sent.
    SHARE_ONLY = "chat.details.share_only";
    /// Nobody watches yet.
    NOBODY_WATCHING = "chat.details.nobody_watching";
    /// The stage's chip.
    LIVE = "chat.details.live";
    /// The stage's sound button.
    SOUND_ON = "chat.details.sound_on";
    /// See [`SOUND_ON`].
    SOUND_OFF = "chat.details.sound_off";
    /// The stage's size button.
    LARGER = "chat.details.larger";
    /// See [`LARGER`].
    SMALLER = "chat.details.smaller";
    /// Stop watching.
    STOP_WATCHING = "chat.details.stop_watching";
    // ---- details
    /// The × at the panel's top.
    CLOSE_DETAILS = "chat.details.close_details";
    /// Call a friend.
    CALL = "chat.details.call";
    /// Share a game.
    SHARE_GAME = "chat.details.share_game";
    /// Call is off while in a call.
    IN_CALL = "chat.details.in_call";
    /// A heading.
    PLAYING_NOW = "chat.details.playing_now";
    /// The game is in the user's library.
    IN_LIBRARY = "chat.details.in_library";
    /// It isn't.
    NOT_IN_LIBRARY = "chat.details.not_in_library";
    /// The tooltip of the playing-now row.
    OPEN_IN_LIBRARY = "chat.details.open_in_library";
    /// Watch a friend's game.
    WATCH = "chat.details.watch";
    /// A heading.
    ALSO_PLAYS = "chat.details.also_plays";
    /// A heading.
    YOUR_CONNECTION = "chat.details.your_connection";
    /// A fact.
    FRIENDS_SINCE = "chat.details.friends_since";
    /// A door to the code check.
    SECURITY = "chat.details.security";
    /// Its value.
    VERIFIED = "chat.details.verified";
    /// Its value.
    NOT_VERIFIED = "chat.details.not_verified";
    /// The switch.
    MUTE_NOTIFICATIONS = "chat.details.mute_notifications";
    /// A coral text button.
    REMOVE_FRIEND = "chat.details.remove_friend";
    /// Join a group's voice.
    JOIN_VOICE = "chat.details.join_voice";
    /// Start a group's voice.
    START_VOICE = "chat.details.start_voice";
    /// Invite people to a group.
    INVITE = "chat.details.invite";
    /// A heading.
    MEMBERS = "chat.details.members";
    /// The user in the member list.
    YOU = "chat.details.you";
    /// The group's maker.
    OWNER = "chat.details.owner";
    /// A heading.
    THIS_GROUP = "chat.details.this_group";
    /// A coral text button.
    LEAVE_GROUP = "chat.details.leave_group";
    /// A member's menu.
    MESSAGE = "chat.details.message";
    /// A member's menu.
    PROFILE = "chat.details.profile";
    /// Status words.
    ONLINE = "chat.details.online";
    /// See [`ONLINE`].
    AWAY = "chat.details.away";
    /// See [`ONLINE`].
    BUSY = "chat.details.busy";
    /// See [`ONLINE`].
    OFFLINE = "chat.details.offline";
    /// See [`ONLINE`].
    INVISIBLE = "chat.details.invisible";
    // ---- Add a friend
    /// Under the title.
    ADD_FRIEND_TEXT = "chat.details.add_friend_text";
    /// A tab.
    TAB_SHARE = "chat.details.tab_share";
    /// A tab.
    TAB_HAVE = "chat.details.tab_have";
    /// The tabs' accessible name.
    TABS = "chat.details.tabs";
    /// Beside the QR code.
    SHARE_LINK_BODY = "chat.details.share_link_body";
    /// Fine print under Copy link.
    SHARE_LINK_FINE = "chat.details.share_link_fine";
    /// Above the field.
    PASTE_BODY = "chat.details.paste_body";
    /// The field's placeholder.
    PASTE_HINT = "chat.details.paste_hint";
    /// The field's accessible name.
    LINK_FIELD = "chat.details.link_field";
    /// After Connect.
    REQUEST_SENT = "chat.details.request_sent";
    // ---- New group
    /// A heading.
    ADD_FRIENDS = "chat.details.add_friends";
    /// The main action.
    CREATE_GROUP = "chat.details.create_group";
    /// With no friends to tick.
    NO_FRIENDS = "chat.details.no_friends";
    // ---- Your profile
    /// The title.
    PROFILE_TITLE = "chat.details.profile_title";
    /// Under it.
    PROFILE_TEXT = "chat.details.profile_text";
    /// Steps through the ten pairs.
    CHANGE_COLOUR = "chat.details.change_colour";
    /// A heading.
    NAME = "chat.details.name";
    /// A heading.
    ABOUT_YOU = "chat.details.about_you";
    /// Your avatar's accessible name.
    YOUR_AVATAR = "chat.details.your_avatar";
    // ---- Verify
    /// Verify's main action.
    THEY_MATCH = "chat.details.they_match";
    /// Verify's other action.
    NOT_NOW = "chat.details.not_now";
    /// Close a dialog.
    CLOSE = "chat.details.close";
    /// While the code is on its way.
    CODE_LOADING = "chat.details.code_loading";
    /// The digits' accessible name.
    CODE_SPOKEN = "chat.details.code_spoken";
    // ---- Share
    /// A promise.
    PROMISE_PICTURE = "chat.details.promise_picture";
    /// A promise.
    PROMISE_DESKTOP = "chat.details.promise_desktop";
    /// A promise (groups).
    PROMISE_E2E_GROUP = "chat.details.promise_e2e_group";
    /// A row.
    PICTURE = "chat.details.picture";
    /// Under it.
    PICTURE_HELP = "chat.details.picture_help";
    /// A picture choice.
    Q720 = "chat.details.q720";
    /// A picture choice.
    Q1080 = "chat.details.q1080";
    /// A picture choice.
    QSOURCE = "chat.details.qsource";
    /// A switch.
    INCLUDE_SOUND = "chat.details.include_sound";
    /// A toast.
    STOPPED_SHARING = "chat.details.stopped_sharing";
    /// A toast.
    LEAVE_CALL_FIRST = "chat.details.leave_call_first";
    // ---- Start a game
    /// The title.
    START_TITLE = "chat.details.start_title";
    /// With no game to start.
    START_TEXT_EMPTY = "chat.details.start_text_empty";
    /// Each row's action.
    PLAY_AND_SHARE = "chat.details.play_and_share";
    // ---- Block
    /// The coral action.
    BLOCK = "chat.details.block";
    /// The title of the blocked list.
    BLOCKED_TITLE = "chat.details.blocked_title";
    /// Under it.
    BLOCKED_TEXT = "chat.details.blocked_text";
    /// Each row's action.
    UNBLOCK = "chat.details.unblock";
    /// Nobody is blocked.
    BLOCKED_NONE = "chat.details.blocked_none";
}

fn t1(key: &str, name: &str) -> String {
    rust_i18n::t!(key, name = name).into_owned()
}

/// "Mira is calling" (the card's accessible name).
pub fn calling_spoken(name: &str) -> String {
    t1("chat.details.calling_spoken", name)
}

/// "with Mira" (after "● Sharing").
pub fn sharing_with(name: &str) -> String {
    t1("chat.details.sharing_with", name)
}

/// "Open Mira" (the pill's tooltip).
pub fn open_name(name: &str) -> String {
    t1("chat.details.open_name", name)
}

/// "Calling Mira…".
pub fn calling(names: &str) -> String {
    t1("chat.details.calling", names)
}

/// "Mira and you".
pub fn with_you(names: &str) -> String {
    t1("chat.details.with_you", names)
}

/// The call's second line: its clock, what you share, whether you are muted.
pub fn call_line(clock: &str, sharing: Option<&str>, muted: bool) -> String {
    match (sharing, muted) {
        (None, false) => clock.to_owned(),
        (Some(g), false) => {
            rust_i18n::t!("chat.details.call_line_sharing", clock = clock, game = g).into_owned()
        }
        (None, true) => rust_i18n::t!("chat.details.call_line_muted", clock = clock).into_owned(),
        (Some(g), true) => rust_i18n::t!(
            "chat.details.call_line_sharing_muted",
            clock = clock,
            game = g
        )
        .into_owned(),
    }
}

/// "Jonas and Dev are talking" (`n` people).
pub fn talking(names: &str, n: usize) -> String {
    plural::counted_args("chat.details.talking", n as u64, &[("names", names)])
}

/// "You are sharing Lantern Keep".
pub fn you_share(game: &str) -> String {
    rust_i18n::t!("chat.details.you_share", game = game).into_owned()
}

/// "2 people watching".
pub fn watching(n: u32) -> String {
    plural::counted("chat.details.watching", u64::from(n))
}

/// "Jonas, Ninefold Arena" (the stage's chip).
pub fn stage_who(name: &str, game: &str) -> String {
    rust_i18n::t!("chat.details.stage_who", name = name, game = game).into_owned()
}

/// "Mira is offline" (why Call is off).
pub fn offline_name(name: &str) -> String {
    t1("chat.details.offline_name", name)
}

/// "Playing Lantern Keep".
pub fn playing(game: &str) -> String {
    rust_i18n::t!("chat.details.playing", game = game).into_owned()
}

/// "42 min, sharing it live".
pub fn sharing_live(time: &str) -> String {
    rust_i18n::t!("chat.details.sharing_live", time = time).into_owned()
}

/// "5 members, 5 online".
pub fn group_line(members: u32, online: u32) -> String {
    rust_i18n::t!(
        "chat.details.group_line",
        members = super::super::chat::members(members),
        online = online.to_string()
    )
    .into_owned()
}

/// "Block Mira…".
pub fn block_name(name: &str) -> String {
    t1("chat.details.block_name", name)
}

/// "Unblock Mira".
pub fn unblock_name(name: &str) -> String {
    t1("chat.details.unblock_name", name)
}

/// "Mira and Jonas", "Mira, Jonas and Dev".
pub fn and_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => {
            let sep = crate::i18n::text("chat.details.list_sep");
            let head = rest.join(sep);
            rust_i18n::t!("chat.details.list_pair", a = head, b = *last).into_owned()
        }
    }
}

/// "Created Raid" (a toast).
pub fn created(name: &str) -> String {
    t1("chat.details.created", name)
}

/// "Check your connection with Mira".
pub fn verify_title(name: &str) -> String {
    t1("chat.details.verify_title", name)
}

/// Under it.
pub fn verify_text(name: &str) -> String {
    t1("chat.details.verify_text", name)
}

/// "Connection with Mira verified" (a toast).
pub fn verified_toast(name: &str) -> String {
    t1("chat.details.verified_toast", name)
}

/// "Share Lantern Keep with Mira?".
pub fn share_title(game: &str, name: &str) -> String {
    rust_i18n::t!("chat.details.share_title", game = game, name = name).into_owned()
}

/// "End-to-end encrypted, straight to Mira.".
pub fn promise_e2e(name: &str) -> String {
    t1("chat.details.promise_e2e", name)
}

/// "You are already sharing with Mira" (a toast).
pub fn already_sharing(name: &str) -> String {
    t1("chat.details.already_sharing", name)
}

/// Under "Start a game to share it".
pub fn start_text(name: &str) -> String {
    t1("chat.details.start_text", name)
}

/// "Play and share Lantern Keep" (a row's accessible name).
pub fn play_and_share_game(game: &str) -> String {
    rust_i18n::t!("chat.details.play_and_share_game", game = game).into_owned()
}

/// "Block Mira?".
pub fn block_title(name: &str) -> String {
    t1("chat.details.block_title", name)
}

/// What blocking does.
pub fn block_text(name: &str) -> String {
    t1("chat.details.block_text", name)
}

/// "Blocked Mira" (a toast).
pub fn blocked_toast(name: &str) -> String {
    t1("chat.details.blocked_toast", name)
}

/// "Unblocked Mira" (a toast).
pub fn unblocked_toast(name: &str) -> String {
    t1("chat.details.unblocked_toast", name)
}
