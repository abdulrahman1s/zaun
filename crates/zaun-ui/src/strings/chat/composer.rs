//! Chat strings: composer (ADR-0033). Keys live under `chat.composer.` in locales/en.yml and ar.yml,
//! between that file's `chat.composer` markers.

texts! {
    /// Shortcut caps (the same in every language).
    KEY_BOLD = "chat.composer.key_bold";
    /// See [`KEY_BOLD`].
    KEY_ITALIC = "chat.composer.key_italic";
    /// See [`KEY_BOLD`].
    KEY_CODE = "chat.composer.key_code";
    /// Send with Ctrl+Enter.
    KEY_SEND_CTRL = "chat.composer.key_send_ctrl";
    /// Send with Enter.
    KEY_SEND = "chat.composer.key_send";
    /// The + button's tooltip.
    ATTACH = "chat.composer.attach";
    /// The + menu: a picture.
    PICTURE = "chat.composer.picture";
    /// A picture in a message (the reply bar).
    PHOTO = "chat.composer.photo";
    /// A video in a message (the reply bar).
    VIDEO = "chat.composer.video";
    /// A pasted picture waiting to be sent.
    PASTED_PICTURE = "chat.composer.pasted_picture";
    /// The field's placeholder while attachments wait.
    ADD_CAPTION = "chat.composer.add_caption";
    /// Only folders were dropped.
    FOLDERS_REFUSED = "chat.composer.folders_refused";
    /// The picture-or-video chooser's filter.
    MEDIA_FILTER = "chat.composer.media_filter";
    /// The + menu: any file.
    FILE = "chat.composer.file";
    /// The Aa button.
    FORMATTING = "chat.composer.formatting";
    /// The GIF button's tooltip.
    GIF = "chat.composer.gif";
    /// The GIF button's accessible name.
    GIF_SPOKEN = "chat.composer.gif_spoken";
    /// The emoji button; the picker's name; the `:` suggestions' name.
    EMOJI = "chat.composer.emoji";
    /// The microphone.
    RECORD = "chat.composer.record";
    /// The Send arrow.
    SEND = "chat.composer.send";
    /// The reply bar, before the name.
    REPLYING = "chat.composer.replying";
    /// The reply bar's name when the message is the user's own.
    YOURSELF = "chat.composer.yourself";
    /// The edit bar.
    EDITING = "chat.composer.editing";
    /// The bar's ×.
    CANCEL = "chat.composer.cancel";
    /// Formatting: bold.
    BOLD = "chat.composer.bold";
    /// Formatting: italic.
    ITALIC = "chat.composer.italic";
    /// Formatting: strikethrough.
    STRIKE = "chat.composer.strike";
    /// Formatting: code.
    CODE = "chat.composer.code";
    /// Formatting: secret (`#text#`, hidden until clicked).
    SPOILER = "chat.composer.spoiler";
    /// Formatting: text in a colour (opens the six colours).
    COLOR = "chat.composer.color";
    /// A colour of `!1 text!`.
    COLOR_RED = "chat.composer.color_red";
    /// A colour of `!2 text!`.
    COLOR_GREEN = "chat.composer.color_green";
    /// A colour of `!3 text!`.
    COLOR_BLUE = "chat.composer.color_blue";
    /// A colour of `!4 text!`.
    COLOR_YELLOW = "chat.composer.color_yellow";
    /// A colour of `!5 text!`.
    COLOR_CYAN = "chat.composer.color_cyan";
    /// A colour of `!6 text!`.
    COLOR_MAGENTA = "chat.composer.color_magenta";
    /// At the end of the formatting row.
    MARKDOWN_HINT = "chat.composer.markdown_hint";
    /// While recording, before the buttons.
    REC_LIMIT = "chat.composer.rec_limit";
    /// Discard the recording (tooltip).
    DISCARD = "chat.composer.discard";
    /// Discard the recording (accessible name).
    DISCARD_SPOKEN = "chat.composer.discard_spoken";
    /// Send the recording (accessible name).
    SEND_VOICE = "chat.composer.send_voice";
    /// The recording's accessible name.
    RECORDING = "chat.composer.recording";
    /// Unblock, in a blocked conversation.
    UNBLOCK = "chat.composer.unblock";
    /// The amber line while offline.
    OFFLINE = "chat.composer.offline";
    /// The `@` suggestions' name.
    MEMBERS = "chat.composer.members";
    /// The picker's search field.
    SEARCH_EMOJI = "chat.composer.search_emoji";
    /// The picker's first section and its clock tab.
    FREQUENT = "chat.composer.frequent";
    /// The picker's section while searching.
    RESULTS = "chat.composer.results";
    /// The picker's footer while nothing is pointed at.
    SHIFT_HINT = "chat.composer.shift_hint";
    /// Emoji group.
    GROUP_SMILEYS = "chat.composer.group_smileys";
    /// Emoji group.
    GROUP_PEOPLE = "chat.composer.group_people";
    /// Emoji group.
    GROUP_NATURE = "chat.composer.group_nature";
    /// Emoji group.
    GROUP_FOOD = "chat.composer.group_food";
    /// Emoji group.
    GROUP_TRAVEL = "chat.composer.group_travel";
    /// Emoji group.
    GROUP_ACTIVITIES = "chat.composer.group_activities";
    /// Emoji group.
    GROUP_OBJECTS = "chat.composer.group_objects";
    /// Emoji group.
    GROUP_SYMBOLS = "chat.composer.group_symbols";
    /// Emoji group.
    GROUP_FLAGS = "chat.composer.group_flags";
}

/// The field's accessible name: "Message to Mira Okafor".
pub fn label(name: &str) -> String {
    rust_i18n::t!("chat.composer.label", name = name).into_owned()
}

/// The field's placeholder: "Message Mira".
pub fn placeholder(name: &str) -> String {
    rust_i18n::t!("chat.composer.placeholder", name = name).into_owned()
}

/// A blocked conversation, in place of the field.
pub fn blocked(name: &str) -> String {
    rust_i18n::t!("chat.composer.blocked", name = name).into_owned()
}

/// Over the conversation while files are dragged over the window.
pub fn drop_to(name: &str) -> String {
    rust_i18n::t!("chat.composer.drop_to", name = name).into_owned()
}

/// The picker found nothing.
pub fn no_emoji(query: &str) -> String {
    rust_i18n::t!("chat.composer.no_emoji", query = query).into_owned()
}

/// The + menu's heading: "Up to 1 GB each" (the engine's send limit, in GiB).
pub fn file_limit(gib: u64) -> String {
    rust_i18n::t!("chat.composer.file_limit", size = gib).into_owned()
}

/// An attachment's ×: "Remove boss.mp4".
pub fn remove_named(name: &str) -> String {
    rust_i18n::t!("chat.composer.remove_named", name = name).into_owned()
}

/// The attachment strip's name: "Attachments: 2".
pub fn attachments(count: usize) -> String {
    rust_i18n::t!("chat.composer.attachments", count = count).into_owned()
}
