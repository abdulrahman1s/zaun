//! Steam games (ADR-0031): the option, the "run by Steam" state, play-time source, update
//! states, actions and their notices. Copy rules: USER_EXPERIENCE §9, SECURITY_MODEL §10 —
//! nothing here claims protection for a game Steam runs.

texts! {
    /// The Settings group's heading.
    GROUP = "steam.group";
    /// The Settings switch.
    SHOW_OPTION = "steam.show_option";
    /// Under the switch.
    SHOW_OPTION_HELP = "steam.show_option_help";
    /// "Check again" next to the switch.
    RESCAN = "steam.rescan";
    /// Under "Check Steam again".
    RESCAN_HELP = "steam.rescan_help";
    /// The button of "Check Steam again".
    CHECK_NOW = "steam.check_now";
    /// The scope word on the library's filter line.
    SCOPE = "steam.scope";
    /// The state shown where an EasyPlay game shows its protection.
    RUN_BY_STEAM = "steam.run_by_steam";
    /// Its explanation (game page, tooltip).
    RUN_BY_STEAM_DETAIL = "steam.run_by_steam_detail";
    /// The origin label on a card or row.
    ORIGIN = "steam.origin";
    /// Where a play time comes from.
    PLAYTIME_FROM_STEAM = "steam.playtime_from_steam";
    /// Where an EasyPlay game's play time comes from (shown only next to a Steam one).
    PLAYTIME_FROM_EASYPLAY = "steam.playtime_from_easyplay";
    /// Under the play record, where an EasyPlay game shows its last thirty days.
    FROM_STEAM = "steam.from_steam";
    /// The play record's description for a screen reader.
    RECORD_FROM_STEAM = "steam.record_from_steam";
    /// Under "Starting…" in the play record, after Play handed the game to Steam.
    STARTING_IN_STEAM = "steam.starting_in_steam";
    /// The play record as one sentence while Steam starts the game (a narrow page).
    STARTING_SENTENCE = "steam.starting_sentence";
    /// Under the time in the play record when Steam gives no date of last play.
    PLAYED = "steam.played";
    /// The list's Last played cell of a Steam game Steam gives no date for.
    LAST_UNKNOWN = "steam.last_unknown";
}

/// The play record as one sentence, in a page too narrow for the cover column: "Played
/// 74 h 10 min in Steam, most recently 3 days ago." (`last` as `fmt::last_played` writes it).
pub fn played_sentence(time: &str, last: Option<&str>) -> String {
    match last {
        // Lower-casing is a no-op in scripts without case.
        Some(last) => rust_i18n::t!(
            "steam.played_sentence_last",
            time = time,
            last = last.to_lowercase()
        )
        .into_owned(),
        None => rust_i18n::t!("steam.played_sentence", time = time).into_owned(),
    }
}

/// Update states.
pub mod update {
    texts! {
        /// Nothing waiting.
        UP_TO_DATE = "steam.update.up_to_date";
        /// Waiting.
        WAITING = "steam.update.waiting";
        /// Paused.
        PAUSED = "steam.update.paused";
        /// Running.
        UPDATING = "steam.update.updating";
        /// Files missing or damaged.
        FILES_MISSING = "steam.update.files_missing";
    }
}

/// Actions.
pub mod action {
    texts! {
        /// Show Steam's downloads.
        UPDATE_IN_STEAM = "steam.action.update_in_steam";
        /// Show the game in Steam (Properties: the Proton choice).
        OPEN_IN_STEAM = "steam.action.open_in_steam";
        /// Under "Open in Steam" on the game page.
        OPEN_IN_STEAM_HELP = "steam.action.open_in_steam_help";
        /// Show the prefix folder.
        OPEN_STORAGE = "steam.action.open_storage";
        /// winecfg.
        WINDOWS_SETTINGS = "steam.action.windows_settings";
        /// Run a program in the prefix.
        RUN_PROGRAM = "steam.action.run_program";
        /// Under the two prefix actions.
        STORAGE_HELP = "steam.action.storage_help";
        /// Why the prefix actions are not available while the game runs.
        STORAGE_BUSY = "steam.action.storage_busy";
        /// Why there are no prefix actions.
        NO_STORAGE = "steam.action.no_storage";
        /// Why there is no Stop.
        NO_STOP = "steam.action.no_stop";
    }
}

/// The game page of a Steam game.
pub mod page {
    texts! {
        /// The Windows storage tab.
        STORAGE_TAB = "steam.page.storage_tab";
        /// "Proton", the fact's label in How it runs.
        PROTON = "steam.page.proton";
    }
    /// The Proton choice when the game has its own.
    pub fn proton_chosen(tool: &str) -> String {
        rust_i18n::t!("steam.page.proton_chosen", tool = tool).into_owned()
    }
    texts! {
        /// The Proton choice when Steam's default applies.
        PROTON_DEFAULT = "steam.page.proton_default";
    }
    /// The build that last ran it, on the line under the choice.
    pub fn last_run_with(version: &str) -> String {
        rust_i18n::t!("steam.page.last_run_with", version = version).into_owned()
    }
    texts! {
        /// A game Steam runs without Proton, once it has been played.
        NATIVE = "steam.page.native";
        /// No Proton recorded for a game not played yet.
        NOT_RUN_YET = "steam.page.not_run_yet";
        /// The Windows storage door.
        STORAGE = "steam.page.storage";
        /// Its value: where it is.
        STORAGE_IN_STEAM = "steam.page.storage_in_steam";
        /// The Steam facts of What it is.
        SIZE = "steam.page.size";
        /// The installed build.
        BUILD = "steam.page.build";
        /// When Steam last updated it.
        UPDATED = "steam.page.updated";
        /// The app ID.
        APP_ID = "steam.page.app_id";
        /// Which Steam lists it.
        INSTALLED_WITH = "steam.page.installed_with";
        /// Steam installed from the distribution or Valve.
        STEAM_NATIVE = "steam.page.steam_native";
        /// Steam from Flathub.
        STEAM_FLATPAK = "steam.page.steam_flatpak";
        /// Steam from the Snap store.
        STEAM_SNAP = "steam.page.steam_snap";
        /// The Windows storage tab's rows.
        OPEN_FOLDER = "steam.page.open_folder";
        /// Under "Open folder".
        OPEN_FOLDER_HELP = "steam.page.open_folder_help";
        /// Its button.
        OPEN = "steam.page.open";
        /// Under "Windows settings".
        WINDOWS_SETTINGS_HELP = "steam.page.windows_settings_help";
        /// "Run another program…".
        RUN_PROGRAM = "steam.page.run_program";
        /// Under it.
        RUN_PROGRAM_HELP = "steam.page.run_program_help";
        /// Its button.
        CHOOSE = "steam.page.choose";
        /// Why the two programs are not offered while the game has Windows storage and is
        /// closed: the compatibility layer that last ran it is gone.
        NO_PROTON_BUILD = "steam.page.no_proton_build";
        /// The Windows storage door's description and the tab's accessible summary.
        OPEN_STORAGE_HELP = "steam.page.open_storage_help";
    }
}

/// The notes above a Steam game's tabs.
pub mod note {
    texts! {
        /// An update is waiting.
        WAITING_TITLE = "steam.note.waiting_title";
        /// Its text.
        WAITING_TEXT = "steam.note.waiting_text";
        /// An update is paused.
        PAUSED_TITLE = "steam.note.paused_title";
        /// Its text.
        PAUSED_TEXT = "steam.note.paused_text";
        /// Steam is updating the game.
        UPDATING_TITLE = "steam.note.updating_title";
        /// Its text when there is nothing to count.
        UPDATING_TEXT = "steam.note.updating_text";
    }
    /// Download progress: "1.2 GB of 1.4 GB downloaded.".
    pub fn progress(done: &str, total: &str) -> String {
        rust_i18n::t!("steam.note.progress", done = done, total = total).into_owned()
    }
    /// A waiting update of known size, nothing downloaded yet.
    pub fn waiting_to_download(total: &str) -> String {
        rust_i18n::t!("steam.note.waiting_to_download", total = total).into_owned()
    }
    /// A waiting update, partly downloaded.
    pub fn waiting_progress(done: &str, total: &str) -> String {
        rust_i18n::t!("steam.note.waiting_progress", done = done, total = total).into_owned()
    }
    /// A paused update, partly downloaded.
    pub fn paused_progress(done: &str, total: &str) -> String {
        rust_i18n::t!("steam.note.paused_progress", done = done, total = total).into_owned()
    }
    texts! {
        /// Files missing or damaged.
        FILES_TITLE = "steam.note.files_title";
        /// Its text.
        FILES_TEXT = "steam.note.files_text";
        /// Steam's program cannot be found.
        NO_STEAM_TITLE = "steam.note.no_steam_title";
        /// Its text.
        NO_STEAM_TEXT = "steam.note.no_steam_text";
        /// The library folder holding the game is on a drive that is not connected.
        DRIVE_TITLE = "steam.note.drive_title";
        /// Its text.
        DRIVE_TEXT = "steam.note.drive_text";
        /// The game's folder is gone or cannot be read.
        FOLDER_TITLE = "steam.note.folder_title";
        /// Its text.
        FOLDER_TEXT = "steam.note.folder_text";
    }
}

/// The Launch options tab of a Steam game (ADR-0031 §6a): Steam's own setting.
pub mod launch {
    texts! {
        /// The field's name.
        FIELD = "steam.launch.field";
        /// Under the field.
        HELP = "steam.launch.help";
        /// The field's button.
        SAVE = "steam.launch.save";
        /// The MangoHud switch.
        OVERLAY = "steam.launch.overlay";
        /// Under the switch, when MangoHud is installed.
        OVERLAY_HELP = "steam.launch.overlay_help";
        /// The note while Steam is open: its title.
        OPEN_TITLE = "steam.launch.open_title";
        /// Its text.
        OPEN_TEXT = "steam.launch.open_text";
        /// Its action.
        CLOSE_STEAM = "steam.launch.close_steam";
        /// The account's settings file cannot be found or read.
        UNREADABLE = "steam.launch.unreadable";
    }
    /// After the launch options were written.
    pub fn saved(name: &str) -> String {
        rust_i18n::t!("steam.launch.saved", name = name).into_owned()
    }
}

/// The library's filter group that tells Steam games from EasyPlay's own.
pub mod filter {
    texts! {
        /// The group's heading.
        SOURCE = "steam.filter.source";
        /// Games added to EasyPlay.
        EASYPLAY = "steam.filter.easyplay";
        /// Games installed with Steam.
        STEAM = "steam.filter.steam";
    }
}

/// After "Close Steam".
pub fn asked_to_close() -> &'static str {
    crate::i18n::text("steam.asked_to_close")
}

/// The selection's actions that skip Steam games.
pub mod selection {
    texts! {
        /// Removing a Steam game.
        NOT_REMOVED = "steam.selection.not_removed";
    }
    /// Some Steam games were left as they were by a change of protection or a removal.
    pub fn skipped(n: usize) -> String {
        crate::i18n::plural::counted("steam.selection.skipped", n as u64)
    }
}

/// The Statistics page's line about Steam games, with Steam's total for them.
pub fn statistics_line(total: &str) -> String {
    rust_i18n::t!("steam.statistics_line", total = total).into_owned()
}

/// "Asked Steam to start …".
pub fn asked_to_play(name: &str) -> String {
    rust_i18n::t!("steam.asked_to_play", name = name).into_owned()
}

/// After "Update in Steam".
pub fn opened_downloads(name: Option<&str>) -> String {
    match name {
        Some(n) => rust_i18n::t!("steam.opened_downloads_for", name = n).into_owned(),
        None => crate::i18n::text("steam.opened_downloads").to_owned(),
    }
}

/// After "Open in Steam".
pub fn opened_in_steam(name: &str) -> String {
    rust_i18n::t!("steam.opened_in_steam", name = name).into_owned()
}

/// After "Check Steam again".
pub fn found(games: usize) -> String {
    match games {
        0 => crate::i18n::text("steam.found_none").to_owned(),
        n => crate::i18n::plural::counted("steam.found", n as u64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_rules() {
        for s in [
            SHOW_OPTION().to_owned(),
            SHOW_OPTION_HELP().to_owned(),
            RESCAN_HELP().to_owned(),
            RUN_BY_STEAM().to_owned(),
            RUN_BY_STEAM_DETAIL().to_owned(),
            action::STORAGE_HELP().to_owned(),
            action::NO_STOP().to_owned(),
            page::WINDOWS_SETTINGS_HELP().to_owned(),
            page::RUN_PROGRAM_HELP().to_owned(),
            note::WAITING_TEXT().to_owned(),
            note::FILES_TEXT().to_owned(),
            note::NO_STEAM_TEXT().to_owned(),
            selection::NOT_REMOVED().to_owned(),
            STARTING_IN_STEAM().to_owned(),
            page::NO_PROTON_BUILD().to_owned(),
            page::OPEN_STORAGE_HELP().to_owned(),
            note::waiting_progress("1 GB", "2 GB"),
            launch::HELP().to_owned(),
            launch::OVERLAY_HELP().to_owned(),
            launch::OPEN_TEXT().to_owned(),
            launch::UNREADABLE().to_owned(),
            launch::saved("Brass Lighthouse"),
        ] {
            assert!(!s.contains('!'));
            let l = s.to_lowercase();
            for word in ["safe", "secure", "prefix", "wine", "sandbox"] {
                assert!(!l.contains(word), "{word} in {s}");
            }
        }
        assert_eq!(found(0), "No Steam games found");
        assert_eq!(found(2), "Found 2 Steam games");
        assert_eq!(
            selection::skipped(2),
            "2 Steam games were left as they are: Steam runs them."
        );
    }
}
