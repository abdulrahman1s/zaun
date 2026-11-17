//! The Chat page (ADR-0033). Copy rules: USER_EXPERIENCE §9; the chat engine is never
//! named (no product names), relays are named by their host names only.

texts! {
    /// The Settings group's heading.
    GROUP = "chat.group";
    /// The Settings switch.
    ENABLE = "chat.enable";
    /// Under the switch: what turning it on does and where messages travel.
    ENABLE_HELP = "chat.enable_help";
    /// The name row.
    NAME = "chat.name";
    /// Under the name row.
    NAME_HELP = "chat.name_help";
    /// The relays row.
    RELAYS = "chat.relays";
    /// Under the relays row.
    RELAYS_HELP = "chat.relays_help";
    /// The relays field when empty.
    RELAYS_PRESET = "chat.relays_preset";
    /// The notifications switch.
    NOTIFY = "chat.notify";
    /// Under it.
    NOTIFY_HELP = "chat.notify_help";
    /// The "show the text" switch.
    NOTIFY_PREVIEW = "chat.notify_preview";
    /// Under it.
    NOTIFY_PREVIEW_HELP = "chat.notify_preview_help";
}
