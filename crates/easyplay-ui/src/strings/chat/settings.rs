//! Chat strings: settings (ADR-0033). Keys live under `chat.settings.` in locales/en.yml and ar.yml,
//! between that file's `chat.settings` markers. Settings › Chat (chat.html SETTINGS `chat`).

texts! {
    /// The tab's name.
    TAB = "chat.settings.tab";
    /// Group: what friends see.
    FRIENDS_SEE = "chat.settings.friends_see";
    /// "Show what I'm playing".
    SHOW_PLAYING = "chat.settings.show_playing";
    /// Under it.
    SHOW_PLAYING_HELP = "chat.settings.show_playing_help";
    /// Group: messages.
    MESSAGES = "chat.settings.messages";
    /// The emoji style row.
    EMOJI = "chat.settings.emoji";
    /// Under it.
    EMOJI_HELP = "chat.settings.emoji_help";
    /// Segment: Twemoji.
    EMOJI_TWEMOJI = "chat.settings.emoji_twemoji";
    /// Segment: the system's emoji.
    EMOJI_SYSTEM = "chat.settings.emoji_system";
    /// The send key row.
    SEND_WITH = "chat.settings.send_with";
    /// Segment: Enter.
    SEND_ENTER = "chat.settings.send_enter";
    /// Segment: Ctrl Enter.
    SEND_CTRL_ENTER = "chat.settings.send_ctrl_enter";
    /// GIFs play by themselves.
    GIF_AUTOPLAY = "chat.settings.gif_autoplay";
    /// GIF search.
    GIF_SEARCH = "chat.settings.gif_search";
    /// Under it.
    GIF_SEARCH_HELP = "chat.settings.gif_search_help";
    /// The Giphy key (`chat.gif_key`).
    GIF_KEY = "chat.settings.gif_key";
    /// Under it.
    GIF_KEY_HELP = "chat.settings.gif_key_help";
    /// The relay GIF search goes through (`chat.gif_proxy`).
    GIF_PROXY = "chat.settings.gif_proxy";
    /// Under it.
    GIF_PROXY_HELP = "chat.settings.gif_proxy_help";
    /// The key and relay fields when empty.
    GIF_NOT_SET = "chat.settings.gif_not_set";
    /// Link previews.
    LINK_PREVIEWS = "chat.settings.link_previews";
    /// Under it.
    LINK_PREVIEWS_HELP = "chat.settings.link_previews_help";
    /// The link check.
    CHECK_LINKS = "chat.settings.check_links";
    /// Under it.
    CHECK_LINKS_HELP = "chat.settings.check_links_help";
    /// Group: calls.
    CALLS = "chat.settings.calls";
    /// The microphone row.
    MIC = "chat.settings.mic";
    /// The speakers row.
    SPEAKERS = "chat.settings.speakers";
    /// The default device.
    SYSTEM_DEFAULT = "chat.settings.system_default";
    /// Noise suppression.
    NOISE = "chat.settings.noise";
    /// Calls through a relay.
    RELAY_CALLS = "chat.settings.relay_calls";
    /// Under it.
    RELAY_CALLS_HELP = "chat.settings.relay_calls_help";
    /// Group: sharing a game.
    SHARING = "chat.settings.sharing";
    /// The picture row.
    PICTURE = "chat.settings.picture";
    /// Segment: 720p.
    PICTURE_720 = "chat.settings.picture_720";
    /// Segment: 1080p.
    PICTURE_1080 = "chat.settings.picture_1080";
    /// Segment: as in the game.
    PICTURE_SOURCE = "chat.settings.picture_source";
    /// Include the game's sound.
    SHARE_SOUND = "chat.settings.share_sound";
    /// Pause while the game is not in front.
    SHARE_PAUSE = "chat.settings.share_pause";
    /// Under it.
    SHARE_PAUSE_HELP = "chat.settings.share_pause_help";
    /// Group: notifications.
    NOTIFICATIONS = "chat.settings.notifications";
    /// New messages.
    NOTIFY_MESSAGES = "chat.settings.notify_messages";
    /// Incoming calls.
    NOTIFY_CALLS = "chat.settings.notify_calls";
}

/// Sample devices the preview offers (chat.html's), as `(microphones, speakers)`.
pub fn preview_devices() -> (&'static [&'static str], &'static [&'static str]) {
    (
        &["USB headset", "Webcam microphone"],
        &["USB headset", "Monitor speakers"],
    )
}
