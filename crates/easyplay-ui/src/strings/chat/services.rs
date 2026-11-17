//! Chat strings: services (ADR-0033). Keys live under `chat.services.` in locales/en.yml and ar.yml,
//! between that file's `chat.services` markers. The GIF picker and GIF messages (GIFs from
//! Giphy), and the link check's dialog.

texts! {
    /// The search field's placeholder and accessible name.
    SEARCH_NAME = "chat.services.search_name";
    /// The search field while GIF search is off in Settings.
    SEARCH_OFF = "chat.services.search_off";
    /// Section label: GIFs the user starred.
    SAVED = "chat.services.saved";
    /// Section label: Giphy's trending GIFs.
    TRENDING = "chat.services.trending";
    /// Section label: search results.
    RESULTS = "chat.services.results";
    /// While a search runs.
    SEARCHING = "chat.services.searching";
    /// No saved GIFs while search is off or unavailable.
    NO_SAVED = "chat.services.no_saved";
    /// The footer: what leaves the computer.
    FOOTER = "chat.services.footer";
    /// The footer while search is off.
    FOOTER_OFF = "chat.services.footer_off";
    /// Beside the footer: Giphy's terms (5A) require this mark where the API is used.
    POWERED_BY = "chat.services.powered_by";
    /// `chat.gif_key` is empty: where to add one.
    NO_KEY = "chat.services.no_key";
    /// Giphy (or the relay) could not be reached.
    OFFLINE = "chat.services.offline";
    /// Giphy refused the key.
    BAD_KEY = "chat.services.bad_key";
    /// `chat.gif_proxy` is not an address.
    BAD_RELAY = "chat.services.bad_relay";
    /// Giphy refused (too many searches).
    REFUSED = "chat.services.refused";
    /// A still GIF: tooltip.
    PLAY = "chat.services.play";
    /// Toast: the chosen GIF has no rendition small enough to send.
    TOO_LARGE = "chat.services.too_large";
    /// Toast: the chosen GIF could not be downloaded.
    NOT_SENT = "chat.services.not_sent";
    /// The dialog's title when Google Safe Browsing flags the site.
    UNSAFE_TITLE = "chat.services.unsafe_title";
    /// The dialog's title when the check got no answer.
    UNCHECKED_TITLE = "chat.services.unchecked_title";
    /// The dialog's button that opens the link.
    OPEN_ANYWAY = "chat.services.open_anyway";
    /// The dialog's button that closes it.
    CANCEL = "chat.services.cancel";
}

/// "No GIFs for “{query}”."
pub fn none_for(query: &str) -> String {
    rust_i18n::t!("chat.services.none_for", query = query).into_owned()
}

/// The accessible name of a GIF: "GIF: {title}".
pub fn gif_named(title: &str) -> String {
    rust_i18n::t!("chat.services.gif_named", title = title).into_owned()
}

/// The unsafe-link dialog's text, naming the host.
pub fn unsafe_text(host: &str) -> String {
    rust_i18n::t!("chat.services.unsafe_text", host = host).into_owned()
}

/// The dialog's text when the check got no answer, naming the host.
pub fn unchecked_text(host: &str) -> String {
    rust_i18n::t!("chat.services.unchecked_text", host = host).into_owned()
}
