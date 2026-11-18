//! The Chat page (ADR-0033). Copy rules: USER_EXPERIENCE §9; the chat engine is never
//! named (no product names), relays are named by their host names only.

pub mod calls;
pub mod composer;
pub mod conversation;
pub mod details;
pub mod list;
pub mod picture;
pub mod services;
pub mod settings;
pub mod time;

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
    /// The "show the text" switch.
    NOTIFY_PREVIEW = "chat.notify_preview";
    /// Under it.
    NOTIFY_PREVIEW_HELP = "chat.notify_preview_help";
}

// ---- The Chat page ----------------------------------------------------------------------

texts! {
    /// The page's headline while Chat is off.
    OFF_TITLE = "chat.off_title";
    /// Under it: what Chat is, where messages travel, and that nothing is sent while off.
    OFF_BODY = "chat.off_body";
    /// The name field's placeholder.
    NAME_HINT = "chat.name_hint";
    /// Under the name field.
    NAME_RULE = "chat.name_rule";
    /// The switch that turns Chat on.
    TURN_ON = "chat.turn_on";
    /// Under it.
    TURN_ON_HELP = "chat.turn_on_help";
    /// This build has no chat engine.
    UNAVAILABLE_TITLE = "chat.unavailable_title";
    /// Under it.
    UNAVAILABLE_BODY = "chat.unavailable_body";
    /// While the engine starts.
    STARTING = "chat.starting";
    /// The button after a failure.
    TRY_AGAIN = "chat.try_again";
    /// Under the user's own name in the list.
    PROFILE_NOTE = "chat.profile_note";
    /// The door to the Add-a-friend dialog, and its title.
    ADD_FRIEND = "chat.add_friend";
    /// The door to the New-group dialog, and its title.
    NEW_GROUP = "chat.new_group";
    /// Heading over requests through the address.
    REQUESTS = "chat.requests";
    /// Under a request's name.
    REQUEST_NOTE = "chat.request_note";
    /// Accept a request.
    ACCEPT = "chat.accept";
    /// Decline a request or an invitation.
    DECLINE = "chat.decline";
    /// Heading over the conversations.
    CONVERSATIONS = "chat.conversations";
    /// The list has no conversation.
    EMPTY = "chat.empty";
    /// The preview of a conversation without messages.
    NO_MESSAGES = "chat.no_messages";
    /// The preview of a group the user is invited to.
    INVITATION = "chat.invitation";
    /// "You" as an author.
    YOU = "chat.you";
    /// The back word of a conversation in a narrow window.
    ALL_CONVERSATIONS = "chat.all_conversations";
    /// The conversation's menu.
    MORE = "chat.more";
    /// Menu item of a direct conversation.
    REMOVE_FRIEND = "chat.remove_friend";
    /// Menu item of a group.
    ADD_MEMBER = "chat.add_member";
    /// Menu item of a group.
    LEAVE_GROUP = "chat.leave_group";
    /// "Add member" when no friend is ready.
    NO_FRIENDS = "chat.no_friends";
    /// Why.
    NO_FRIENDS_WHY = "chat.no_friends_why";
    /// Subtitle of a friend whose connection is not ready.
    CONNECTING = "chat.connecting";
    /// At the top of the transcript while older messages load.
    LOADING_OLDER = "chat.loading_older";
    /// The transcript could not be loaded.
    LOAD_FAILED = "chat.load_failed";
    /// Delivery: not on a relay yet.
    SENDING = "chat.sending";
    /// Delivery: on the relay.
    SENT = "chat.sent";
    /// Delivery: on the friend's device.
    DELIVERED = "chat.delivered";
    /// Delivery: failed.
    NOT_SENT = "chat.not_sent";
    /// The composer's placeholder.
    MESSAGE_HINT = "chat.message_hint";
    /// The send button.
    SEND = "chat.send";
    /// Under the invitation of a group.
    INVITED_BODY = "chat.invited_body";
    /// Join a group.
    JOIN = "chat.join";
    /// The composer of a group being joined.
    JOINING = "chat.joining";
    /// The composer of a group the user left.
    LEFT = "chat.left";
    /// Copy a link.
    COPY_LINK = "chat.copy_link";
    /// While the invite is made.
    MAKING_INVITE = "chat.making_invite";
    /// Toast after copying a link.
    LINK_COPIED = "chat.link_copied";
    /// Accessible name of the invite's QR code.
    QR_INVITE = "chat.qr_invite";
    /// Accessible name of the address's QR code.
    QR_ADDRESS = "chat.qr_address";
    /// Use the link.
    CONNECT = "chat.connect";
    /// The reusable address switch.
    ADDRESS = "chat.address";
    /// Under it.
    ADDRESS_HELP = "chat.address_help";
    /// Close a dialog.
    DONE = "chat.done";
    /// Cancel a dialog.
    CANCEL = "chat.cancel";
    /// The New-group dialog's field.
    GROUP_NAME = "chat.group_name";
    /// Save the name.
    SAVE = "chat.save";
    /// Under the Remove-friend title.
    REMOVE_BODY = "chat.remove_body";
    /// Remove the friend.
    REMOVE = "chat.remove";
    /// Under the Leave-group title.
    LEAVE_BODY = "chat.leave_body";
    /// Leave the group.
    LEAVE = "chat.leave";
}

/// "You: see you at nine" (the preview of the user's own message).
pub fn you_said(text: &str) -> String {
    rust_i18n::t!("chat.you_said", text = text).into_owned()
}

/// "2 unread".
pub fn unread(n: u32) -> String {
    crate::i18n::plural::counted("chat.unread", u64::from(n))
}

/// "3 unread messages" (the top bar's dot).
pub fn unread_messages(n: u32) -> String {
    crate::i18n::plural::counted("chat.unread_messages", u64::from(n))
}

/// "3 members".
pub fn members(n: u32) -> String {
    crate::i18n::plural::counted("chat.members", u64::from(n))
}

/// A conversation row as a screen reader says it: "Kai, You: see you, 21:04".
pub fn row_spoken(name: &str, preview: &str, when: &str) -> String {
    rust_i18n::t!(
        "chat.row_spoken",
        name = name,
        preview = preview,
        when = when
    )
    .into_owned()
}

/// [`row_spoken`] with the unread count: "Kai, ready?, 21:04, 2 unread".
pub fn row_spoken_unread(name: &str, preview: &str, when: &str, unread: &str) -> String {
    rust_i18n::t!(
        "chat.row_spoken_unread",
        name = name,
        preview = preview,
        when = when,
        unread = unread
    )
    .into_owned()
}

/// The profile row: "Your name, Sam. Change it".
pub fn profile_spoken(name: &str) -> String {
    rust_i18n::t!("chat.profile_spoken", name = name).into_owned()
}

/// A request row: "Mo wants to be your friend".
pub fn request_spoken(name: &str) -> String {
    rust_i18n::t!("chat.request_spoken", name = name).into_owned()
}

/// The transcript's name: "Messages with Kai".
pub fn transcript(name: &str) -> String {
    rust_i18n::t!("chat.transcript", name = name).into_owned()
}

/// A message as a screen reader says it: "Kai, 21:04: ready?".
pub fn message_spoken(author: &str, when: &str, text: &str) -> String {
    rust_i18n::t!(
        "chat.message_spoken",
        author = author,
        when = when,
        text = text
    )
    .into_owned()
}

/// The user's own message: "You, 21:04, Delivered: on my way".
pub fn own_spoken(when: &str, delivery: &str, text: &str) -> String {
    rust_i18n::t!(
        "chat.own_spoken",
        when = when,
        delivery = delivery,
        text = text
    )
    .into_owned()
}

/// "21:04 · Delivered" under the user's own message.
pub fn time_delivery(when: &str, delivery: &str) -> String {
    rust_i18n::t!("chat.time_delivery", when = when, delivery = delivery).into_owned()
}

/// The composer's accessible name: "Message to Kai".
pub fn composer(name: &str) -> String {
    rust_i18n::t!("chat.composer.label", name = name).into_owned()
}

/// The composer while the friend's connection is not ready.
pub fn waiting(name: &str) -> String {
    rust_i18n::t!("chat.waiting", name = name).into_owned()
}

/// "You're invited to LAN night".
pub fn invited_title(name: &str) -> String {
    rust_i18n::t!("chat.invited_title", name = name).into_owned()
}

/// "Remove Kai?".
pub fn remove_title(name: &str) -> String {
    rust_i18n::t!("chat.remove_title", name = name).into_owned()
}

/// "Leave Squad?".
pub fn leave_title(name: &str) -> String {
    rust_i18n::t!("chat.leave_title", name = name).into_owned()
}

/// "Add Ana to Squad" (a friend in the Add-member submenu, spoken).
pub fn add_to(friend: &str, group: &str) -> String {
    rust_i18n::t!("chat.add_to", friend = friend, group = group).into_owned()
}

texts! {
    /// Beside the list while no conversation is open.
    PICK = "chat.pick";
}

/// "Kai: ready?" (the preview of a group member's message).
pub fn member_said(name: &str, text: &str) -> String {
    rust_i18n::t!("chat.member_said", name = name, text = text).into_owned()
}

/// "Accept Mo".
pub fn accept_name(name: &str) -> String {
    rust_i18n::t!("chat.accept_name", name = name).into_owned()
}

/// "Decline Mo".
pub fn decline_name(name: &str) -> String {
    rust_i18n::t!("chat.decline_name", name = name).into_owned()
}

/// "You:" before the preview of the user's own message.
pub fn you_prefix() -> String {
    crate::i18n::text("chat.you_prefix").to_owned()
}

/// "Kai:" before the preview of a group member's message.
pub fn author_prefix(name: &str) -> String {
    rust_i18n::t!("chat.author_prefix", name = name).into_owned()
}

/// A conversation's header as a screen reader says it: "Squad, 3 members".
pub fn header_spoken(name: &str, note: &str) -> String {
    rust_i18n::t!("chat.header_spoken", name = name, note = note).into_owned()
}

/// Parts of an accessible label read as one, joined with the language's list comma.
pub fn spoken(parts: &[&str]) -> String {
    let sep = rust_i18n::t!("chat.spoken_sep");
    parts
        .iter()
        .filter(|p| !p.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(&sep)
}

/// The start of a conversation, read as one: its name, then the sentence under it.
pub fn start_spoken(name: &str, body: &str) -> String {
    rust_i18n::t!("chat.start_spoken", name = name, body = body).into_owned()
}
