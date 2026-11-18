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
    /// Under the two sharing switches the media cannot honour yet (ADR-0036).
    SHARE_SOUND_LATER = "chat.settings.share_sound_later";
    /// See [`SHARE_SOUND_LATER`].
    SHARE_PAUSE_AUTO = "chat.settings.share_pause_auto";
    /// Group: notifications.
    NOTIFICATIONS = "chat.settings.notifications";
    /// New messages.
    NOTIFY_MESSAGES = "chat.settings.notify_messages";
    /// Incoming calls.
    NOTIFY_CALLS = "chat.settings.notify_calls";
    /// Microphone volume (`chat.mic_volume`).
    MIC_VOLUME = "chat.settings.mic_volume";
    /// Under "Reduce background noise".
    NOISE_HELP = "chat.settings.noise_help";
    /// The disclosure over the relay servers.
    ADVANCED = "chat.settings.advanced";
    /// The relay servers row (`chat.ice_servers`).
    RELAY_SERVERS = "chat.settings.relay_servers";
    /// Under it.
    RELAY_SERVERS_HELP = "chat.settings.relay_servers_help";
    /// The relay servers field when empty.
    RELAY_SERVERS_EMPTY = "chat.settings.relay_servers_empty";
    /// Group: the in-game overlay.
    OVERLAY = "chat.settings.overlay";
    /// The desktop has no way to show it (one line under the heading).
    OVERLAY_NONE = "chat.settings.overlay_none";
    /// The overlay's switch (`chat.overlay`).
    OVERLAY_ON = "chat.settings.overlay_on";
    /// Under it.
    OVERLAY_HELP = "chat.settings.overlay_help";
    /// The key row (`chat.overlay_key`).
    KEY = "chat.settings.key";
    /// The key cap's accessible name.
    KEY_CHANGE = "chat.settings.key_change";
    /// The key cap while recording.
    KEY_PRESS = "chat.settings.key_press";
    /// The key cap with no key.
    KEY_NONE = "chat.settings.key_none";
    /// Copy the command.
    COPY = "chat.settings.copy";
    /// The command was copied.
    COPIED = "chat.settings.copied";
    /// Toasts over the game (`chat.overlay_toasts`).
    TOASTS = "chat.settings.toasts";
    /// Under it.
    TOASTS_HELP = "chat.settings.toasts_help";
    /// The call's widget (`chat.overlay_voice_widget`).
    WIDGET = "chat.settings.widget";
    /// Under it.
    WIDGET_HELP = "chat.settings.widget_help";
    /// Where the widget sits (`chat.overlay_corner`).
    CORNER = "chat.settings.corner";
    /// Corner: top left.
    CORNER_TL = "chat.settings.corner_tl";
    /// Corner: top right.
    CORNER_TR = "chat.settings.corner_tr";
    /// Corner: bottom left.
    CORNER_BL = "chat.settings.corner_bl";
    /// Corner: bottom right.
    CORNER_BR = "chat.settings.corner_br";
    /// Picture: 1440p.
    PICTURE_1440 = "chat.settings.picture_1440";
    /// Under "Picture".
    PICTURE_HELP = "chat.settings.picture_help";
    /// The frame rate row (`chat.share_fps`).
    FRAME_RATE = "chat.settings.frame_rate";
    /// Frame rate: 30.
    FPS_30 = "chat.settings.fps_30";
    /// Frame rate: 60.
    FPS_60 = "chat.settings.fps_60";
    /// The input level row.
    LEVEL = "chat.settings.level";
    /// Under it, before the check.
    LEVEL_HELP = "chat.settings.level_help";
    /// Under it, while the check runs.
    CHECKING = "chat.settings.checking";
    /// Start the microphone check.
    CHECK = "chat.settings.check";
    /// Stop it.
    STOP_CHECK = "chat.settings.stop_check";
    /// Hardware encoding (`chat.share_hw_encode`).
    HW_ENCODE = "chat.settings.hw_encode";
    /// Under it.
    HW_ENCODE_HELP = "chat.settings.hw_encode_help";
}

/// A stored device that is not connected now: "USB headset (not connected)".
pub fn not_connected(name: &str) -> String {
    rust_i18n::t!("chat.settings.not_connected", name = name).into_owned()
}

/// "Bind a key in your desktop to run" before the command chip: the help of the key row is
/// the sentence around `%{cmd}`, split there (the chip is drawn in its place).
pub fn key_help() -> (String, String) {
    let s = rust_i18n::t!("chat.settings.key_help", cmd = "\u{1}").into_owned();
    match s.split_once('\u{1}') {
        Some((a, b)) => (a.trim_end().to_owned(), b.trim_start().to_owned()),
        None => (s, String::new()),
    }
}

/// The command a desktop key runs to open the overlay.
pub const OVERLAY_COMMAND: &str = "easyplay overlay";

/// Sample devices the preview offers (chat.html's), as `(microphones, speakers)`.
pub fn preview_devices() -> (&'static [&'static str], &'static [&'static str]) {
    (
        &["USB headset", "Webcam microphone"],
        &["USB headset", "Monitor speakers"],
    )
}
