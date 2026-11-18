//! The call stage, its device menu, sharing's window step and the watch stage
//! (`chat-calls.html` sections D–F, table S).

use crate::i18n::plural;

texts! {
    /// The stage's status while the sound is set up.
    CONNECTING = "chat.call.connecting";
    /// The stage's status while the path is lost.
    RECONNECTING = "chat.call.reconnecting";
    /// The mic's caret (opens the device menu).
    DEVICES = "chat.call.devices";
    /// Opens Settings › Chat at "Voice and video".
    SETTINGS = "chat.call.settings";
    /// The stage becomes the tray.
    SMALLER = "chat.call.smaller";
    /// The tray becomes the stage.
    LARGER = "chat.call.larger";
    /// The grip under the stage.
    RESIZE = "chat.call.resize";
    /// Back to the grid from a focused tile.
    SHOW_EVERYONE = "chat.call.show_everyone";
    /// The device menu's rows.
    MICROPHONE = "chat.call.microphone";
    /// See [`MICROPHONE`].
    SPEAKER = "chat.call.speaker";
    /// A device row's value when none was chosen.
    SYSTEM_DEFAULT = "chat.call.system_default";
    /// The volume slider's label.
    MIC_VOLUME = "chat.call.mic_volume";
    /// The noise switch.
    NOISE = "chat.call.noise";
    /// Why Share is off during a call.
    SHARE_IN_CALL = "chat.call.share_in_call";
    /// Why a group cannot be shared with.
    SHARE_GROUP = "chat.call.share_group";
    /// A tile's spoken state.
    TILE_SPEAKING = "chat.call.tile_speaking";
    /// See [`TILE_SPEAKING`].
    TILE_MUTED = "chat.call.tile_muted";
    /// See [`TILE_SPEAKING`].
    TILE_DEAFENED = "chat.call.tile_deafened";
    /// The share dialog's second step: its buttons and the busy state.
    CONTINUE = "chat.share.continue";
    /// See [`CONTINUE`].
    WAITING = "chat.share.waiting";
    /// The portal was closed without a window.
    NOT_STARTED = "chat.share.not_started";
    /// The sharing tray while the game is hidden.
    PAUSED = "chat.share.paused";
    /// Why sound controls are off.
    NO_SOUND = "chat.share.no_sound";
    /// The watch stage until the first picture.
    WATCH_WAITING = "chat.watch.waiting";
    /// Closes the ended stage.
    CLOSE = "chat.watch.close";
    /// Full screen and back.
    FULL = "chat.watch.full";
    /// See [`FULL`].
    EXIT_FULL = "chat.watch.exit_full";
    /// The stream's sound control.
    VOLUME = "chat.watch.volume";
}

fn t(key: &str, args: &[(&str, &str)]) -> String {
    let mut s = crate::i18n::text(key).to_owned();
    for (k, v) in args {
        s = s.replace(&format!("%{{{k}}}"), v);
    }
    s
}

/// "Connection good, 42 ms", "… fair …" or "Connection poor, 310 ms, 6% lost".
pub fn quality(grade: Grade, ms: u16, loss: u8) -> String {
    let ms = ms.to_string();
    match grade {
        Grade::Good => t("chat.call.quality_good", &[("ms", &ms)]),
        Grade::Fair => t("chat.call.quality_fair", &[("ms", &ms)]),
        Grade::Poor => t(
            "chat.call.quality_poor",
            &[("ms", &ms), ("loss", &loss.to_string())],
        ),
    }
}

/// How well a call's path carries sound, as the bars show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grade {
    Good,
    Fair,
    Poor,
}

/// "80%".
pub fn percent(n: u8) -> String {
    t("chat.call.percent", &[("n", &n.to_string())])
}

/// "Choose Lantern Keep in the next window".
pub fn choose_title(game: &str) -> String {
    t("chat.share.choose_title", &[("game", game)])
}

/// "Your desktop asks which window to share. Pick the one called …".
pub fn choose_text(game: &str) -> String {
    t("chat.share.choose_text", &[("game", game)])
}

/// "2 watching".
pub fn watching(n: u32) -> String {
    plural::counted("chat.share.watching", u64::from(n))
}

/// "Jonas stopped sharing".
pub fn ended(name: &str) -> String {
    t("chat.watch.ended", &[("name", name)])
}

/// "1080p": the picture's height.
pub fn resolution(height: u32) -> String {
    t("chat.watch.resolution", &[("h", &height.to_string())])
}

/// "Mira didn't answer".
pub fn no_answer(name: &str) -> String {
    t("chat.conversation.no_answer", &[("name", name)])
}

texts! {
    /// "Call again" after an unanswered call.
    CALL_AGAIN = "chat.conversation.call_again";
}

// ---- Stream quality (notes J) ---------------------------------------------------------------

texts! {
    /// The share dialog's and the live menu's size choice.
    PICTURE = "chat.quality.picture";
    /// Under it in the share dialog.
    PICTURE_HELP = "chat.quality.picture_help";
    /// The frame rate choice.
    FRAME_RATE = "chat.quality.frame_rate";
    /// The size of the game's own window.
    SOURCE = "chat.quality.source";
    /// After "30 fps".
    FPS30_HINT = "chat.quality.fps30_hint";
    /// A size larger than the game's window.
    TOO_BIG = "chat.quality.too_big";
    /// The live chip's name for a screen reader.
    CHANGE = "chat.quality.change";
    /// The live menu's foot line.
    LIVE_NOTE = "chat.quality.live_note";
}

/// "60 fps".
pub fn fps(n: u32) -> String {
    t("chat.quality.fps", &[("n", &n.to_string())])
}

/// "1080p · 60".
pub fn chip(res: &str, fps: u32) -> String {
    t(
        "chat.quality.chip",
        &[("res", res), ("fps", &fps.to_string())],
    )
}

/// "Sending 1080p at 60 frames a second, hardware encoding".
pub fn sending(res: &str, fps: u32, hardware: bool) -> String {
    let enc = crate::i18n::text(if hardware {
        "chat.quality.hw"
    } else {
        "chat.quality.sw"
    });
    t(
        "chat.quality.sending",
        &[("res", res), ("fps", &fps.to_string()), ("enc", enc)],
    )
}

/// "1080p, 60 frames a second" (what a watcher receives).
pub fn received(res: &str, fps: u32) -> String {
    t(
        "chat.quality.received",
        &[("res", res), ("fps", &fps.to_string())],
    )
}
