//! Every user-visible string as a function or constant, grouped by screen. The single place to
//! localise later. See MODULES §8, USER_EXPERIENCE §9 and task UI-029.
//!
//! Screens never concatenate sentences: anything with a variable part is a function here.
//! Number and date formatting follows STATISTICS §8.

use easyplay_core::clock::Timestamp;
use easyplay_core::model::{Activity, Availability, BasePreset, LaunchStateTag, Preset};

pub mod artwork;
pub mod details;
pub mod dialogs;
pub mod dnd;
pub mod errors;
pub mod launch;
pub mod menu;
pub mod onboarding;
pub mod picker;
pub mod settings;
pub mod setup;
pub mod shortcuts;
pub mod stats;
pub mod tags;

/// The application name.
pub const APP_NAME: &str = "EasyPlay";

/// Window titles.
pub mod title {
    /// "<Route> — EasyPlay".
    pub fn window(route: &str) -> String {
        format!("{route} — {}", super::APP_NAME)
    }
}

/// Plural helper: `one` when `n == 1`, otherwise `many`.
pub fn plural<'a>(n: u64, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

/// Sidebar and routes.
pub mod nav {
    /// Section label.
    pub const LIBRARY: &str = "Library";
    /// Section label.
    pub const TAGS: &str = "Tags";
    /// Route.
    pub const STATISTICS: &str = "Statistics";
    /// Route.
    pub const SETTINGS: &str = "Settings";
    /// Route.
    pub const ONBOARDING: &str = "Welcome";
    /// Debug route.
    pub const GALLERY: &str = "Widget gallery";
    /// Accessible name of the sidebar landmark.
    pub const LANDMARK: &str = "Library navigation";
    /// Toggle.
    pub const COLLAPSE: &str = "Collapse sidebar";
    /// Toggle.
    pub const EXPAND: &str = "Expand sidebar";
    /// Shortcut hint for the toggle.
    pub const SIDEBAR_SHORTCUT: &str = "Ctrl+B";
    /// Back button.
    pub const BACK: &str = "Back";
    /// Back shortcut.
    pub const BACK_SHORTCUT: &str = "Alt+←";

    /// "All games, 42".
    pub fn item_with_count(label: &str, count: usize) -> String {
        format!("{label}, {count}")
    }
}

/// Collections (GAME_LIBRARY §6).
pub mod collection {
    /// All games.
    pub const ALL: &str = "All games";
    /// Favourites.
    pub const FAVORITES: &str = "Favourites";
    /// Recently played.
    pub const RECENT: &str = "Recently played";
    /// Never played.
    pub const NEVER_PLAYED: &str = "Never played";
    /// Running.
    pub const RUNNING: &str = "Running";
    /// Installed.
    pub const INSTALLED: &str = "Installed";
    /// Missing.
    pub const MISSING: &str = "Missing";
    /// Installers.
    pub const INSTALLERS: &str = "Installers";
    /// Hidden.
    pub const HIDDEN: &str = "Hidden";

    /// The empty-collection sentence.
    pub fn empty_sentence(label: &str) -> &'static str {
        match label {
            FAVORITES => "Games you mark with a star appear here.",
            RECENT => "Games you played in the last 30 days appear here.",
            NEVER_PLAYED => "Every game in your library has been played.",
            HIDDEN => "Games you hide from the library appear here.",
            _ => "Games with this tag appear here.",
        }
    }
}

/// Host status row (SCREEN_SPECS §0).
pub mod host {
    /// All checks pass.
    pub const READY: &str = "Ready";
    /// Something needs the user.
    pub const NEEDS_ATTENTION: &str = "Needs attention";
    /// Background jobs are running.
    pub const BUSY: &str = "Working in the background";
    /// Before the first snapshot.
    pub const CHECKING: &str = "Checking…";
    /// Accessible name prefix.
    pub fn status(label: &str) -> String {
        format!("System status: {label}")
    }
}

/// Library screen (SCREEN_SPECS §2).
pub mod library {
    /// Screen title.
    pub const TITLE: &str = "Library";
    /// Search hint.
    pub const SEARCH_HINT: &str = "Search games…";
    /// Search field accessible name.
    pub const SEARCH: &str = "Search games";
    /// Clear search.
    pub const CLEAR_SEARCH: &str = "Clear search";
    /// View switch.
    pub const VIEW_GRID: &str = "Grid view";
    /// View switch.
    pub const VIEW_LIST: &str = "List view";
    /// View switch group name.
    pub const VIEW: &str = "View";
    /// Sort select label.
    pub const SORT: &str = "Sort";
    /// Filter button.
    pub const FILTER: &str = "Filter";
    /// Primary action.
    pub const ADD_GAME: &str = "Add game";
    /// Grid accessible name prefix.
    pub const GRID: &str = "Games";
    /// Empty library.
    pub const EMPTY_TITLE: &str = "Your library is empty";
    /// Empty library.
    pub const EMPTY_BODY: &str =
        "Double-click a Windows program in your file manager, or add one here.";
    /// Empty library drop target.
    pub const DROP_HERE: &str = "Drop a program here";
    /// No results action.
    pub const CLEAR_FILTERS: &str = "Clear filters";
    /// No results action.
    pub const SEARCH_ALL: &str = "Search all games";
    /// Empty collection title.
    pub const EMPTY_COLLECTION: &str = "Nothing here yet";

    /// Sort keys.
    pub const SORT_NAME: &str = "Name";
    /// Sort keys.
    pub const SORT_RECENT: &str = "Recently played";
    /// Sort keys.
    pub const SORT_PLAYTIME: &str = "Playtime";
    /// Sort keys.
    pub const SORT_ADDED: &str = "Date added";
    /// Sort keys.
    pub const SORT_LAUNCHES: &str = "Times launched";
    /// Sort keys.
    pub const SORT_PROTECTION: &str = "Protection";
    /// Sort keys.
    pub const SORT_LAYER: &str = "Compatibility layer";
    /// Sort direction.
    pub const ASCENDING: &str = "Ascending order";
    /// Sort direction.
    pub const DESCENDING: &str = "Descending order";
    /// Grouping select.
    pub const GROUP: &str = "Group";
    /// Groupings.
    pub const GROUP_NONE: &str = "None";
    /// Groupings.
    pub const GROUP_LETTER: &str = "First letter";
    /// Groupings.
    pub const GROUP_TAG: &str = "Tag";
    /// Groupings.
    pub const GROUP_PROTECTION: &str = "Protection";
    /// List columns.
    pub const COL_NAME: &str = "Name";
    /// List columns.
    pub const COL_PROTECTION: &str = "Protection";
    /// List columns.
    pub const COL_LAST_PLAYED: &str = "Last played";
    /// List columns.
    pub const COL_PLAYTIME: &str = "Playtime";
    /// List columns.
    pub const COL_TAGS: &str = "Tags";
    /// List columns.
    pub const COL_LAYER: &str = "Compatibility layer";
    /// List columns.
    pub const COL_ADDED: &str = "Added";
    /// Column chooser.
    pub const COLUMNS: &str = "Choose columns";
    /// Filter groups.
    pub const FILTER_TAGS: &str = "Tags";
    /// Filter groups.
    pub const FILTER_PROTECTION: &str = "Protection";
    /// Filter groups.
    pub const FILTER_INTERNET: &str = "Internet";
    /// Filter groups.
    pub const FILTER_AVAILABILITY: &str = "Availability";
    /// Filter groups.
    pub const FILTER_LAYER: &str = "Compatibility layer";
    /// Filter groups.
    pub const FILTER_TYPE: &str = "Type";
    /// Chip row.
    pub const CLEAR_ALL: &str = "Clear all";
    /// Selection bar.
    pub const CLEAR_SELECTION: &str = "Clear selection";
    /// Selection bar.
    pub const ADD_TAG: &str = "Add tag";
    /// Selection bar.
    pub const REMOVE_TAG: &str = "Remove tag";
    /// Selection bar.
    pub const CHANGE_PROTECTION: &str = "Change protection";
    /// Selection bar.
    pub const REMOVE_SELECTED: &str = "Remove…";

    /// "Filter" or "Filter · 2".
    pub fn filter_button(active: usize) -> String {
        if active == 0 {
            FILTER.to_owned()
        } else {
            format!("{FILTER} · {active}")
        }
    }

    /// "tag:rpg".
    pub fn tag_chip(name: &str) -> String {
        format!("tag:{name}")
    }

    /// Preset filter labels.
    pub fn preset_filter(p: easyplay_core::library::PresetFilter) -> &'static str {
        use easyplay_core::library::PresetFilter as P;
        match p {
            P::Offline => "Offline",
            P::Online => "Online",
            P::Standard => "Standard",
            P::Custom => "Custom",
        }
    }

    /// Internet filter labels.
    pub fn network_filter(n: easyplay_core::model::NetworkAccess) -> &'static str {
        match n {
            easyplay_core::model::NetworkAccess::None => "No internet",
            easyplay_core::model::NetworkAccess::Full => "Internet allowed",
        }
    }

    /// Availability filter labels.
    pub fn availability_filter(a: super::Availability) -> &'static str {
        match a {
            super::Availability::Present => "Installed",
            super::Availability::Missing => "Not found",
            super::Availability::DriveDisconnected => "Drive not connected",
            super::Availability::Unreadable => "Can’t be read",
            super::Availability::Unknown => "Not checked yet",
        }
    }

    /// Type filter labels.
    pub fn kind_filter(k: easyplay_core::model::EntryKind) -> &'static str {
        match k {
            easyplay_core::model::EntryKind::Game => "Games",
            easyplay_core::model::EntryKind::Installer => "Installers",
            easyplay_core::model::EntryKind::Tool => "Tools",
        }
    }

    /// "3 selected".
    pub fn selected_count(n: usize) -> String {
        format!("{n} selected")
    }

    /// "R · 4".
    pub fn group_header(label: &str, n: usize) -> String {
        format!("{label} · {n}")
    }

    /// Header text with the sort arrow.
    pub fn sorted_header(label: &str, descending: bool) -> String {
        format!("{label} {}", if descending { "↓" } else { "↑" })
    }

    /// "Sorted by Playtime, descending".
    pub fn sorted_by(label: &str, descending: bool) -> String {
        format!(
            "Sorted by {label}, {}",
            if descending {
                "descending"
            } else {
                "ascending"
            }
        )
    }

    /// "Changed 3 games".
    pub fn bulk_changed(n: usize) -> String {
        format!("Changed {n} {}", super::plural(n as u64, "game", "games"))
    }

    /// "3 changed, 1 skipped (running)".
    pub fn bulk_summary(changed: usize, skipped: usize) -> String {
        if skipped == 0 {
            bulk_changed(changed)
        } else {
            format!("{changed} changed, {skipped} skipped (running)")
        }
    }

    /// "42 games" or "3 of 42".
    pub fn result_count(shown: usize, total: usize) -> String {
        if shown == total {
            format!("{total} {}", super::plural(total as u64, "game", "games"))
        } else {
            format!("{shown} of {total}")
        }
    }

    /// "No games match “query”."
    pub fn no_results(query: &str) -> String {
        format!("No games match “{query}”.")
    }

    /// "Remove rpg" for a removable chip.
    pub fn remove_chip(label: &str) -> String {
        format!("Remove {label}")
    }

    /// "Games, 42 items, 6 columns".
    pub fn grid_name(items: usize, columns: usize) -> String {
        format!(
            "{GRID}, {items} {}, {columns} {}",
            super::plural(items as u64, "item", "items"),
            super::plural(columns as u64, "column", "columns")
        )
    }
}

/// Game cards and rows (SCREEN_SPECS §3, GAME_LIBRARY §2).
pub mod card {
    use super::*;

    /// Favourite toggle.
    pub const FAVORITE: &str = "Favourite";
    /// Status: no sessions yet.
    pub const NOT_PLAYED: &str = "Not played yet";
    /// Status.
    pub const NOT_FOUND: &str = "Not found";
    /// Status.
    pub const DRIVE_DISCONNECTED: &str = "Drive not connected";
    /// Status.
    pub const UNREADABLE: &str = "Can’t be read";
    /// Status.
    pub const INSTALLER: &str = "Installer";
    /// Status.
    pub const NEEDS_ATTENTION: &str = "Needs attention";
    /// Status.
    pub const STILL_RUNNING: &str = "Still running in the background";
    /// Status.
    pub const STOPPING: &str = "Stopping…";

    /// "Running · 0:42:10".
    pub fn running(elapsed_ms: u64) -> String {
        format!("Running · {}", fmt::clock(elapsed_ms))
    }

    /// The short phase text for a launch state.
    pub fn phase(tag: LaunchStateTag) -> &'static str {
        match tag {
            LaunchStateTag::Validating
            | LaunchStateTag::ResolvingRuntime
            | LaunchStateTag::AuditingRuntime => "Checking…",
            LaunchStateTag::Provisioning => "Downloading…",
            LaunchStateTag::Preparing | LaunchStateTag::Planning => "Preparing…",
            LaunchStateTag::Spawning | LaunchStateTag::Starting => "Starting…",
        }
    }

    /// The lower-case phase for spoken names ("preparing").
    pub fn phase_spoken(tag: LaunchStateTag) -> String {
        phase(tag).trim_end_matches('…').to_lowercase()
    }

    /// "12 h 05 min · Yesterday", "Not played yet".
    pub fn played(total_ms: u64, last: Option<Timestamp>, now: Timestamp, offset_s: i32) -> String {
        match last {
            Some(t) if total_ms > 0 => format!(
                "{} · {}",
                fmt::duration(total_ms),
                fmt::relative_date(t, now, offset_s)
            ),
            Some(t) => fmt::relative_date(t, now, offset_s),
            None => NOT_PLAYED.to_owned(),
        }
    }

    /// State text when the game is not idle or not present; `None` for the normal case.
    pub fn state(activity: Activity, availability: Availability, now: Timestamp) -> Option<String> {
        match activity {
            Activity::Running { since } => {
                let ms = now.since(since).map_or(0, |d| d.as_millis() as u64);
                Some(running(ms))
            }
            Activity::Lingering => Some(STILL_RUNNING.to_owned()),
            Activity::Launching(tag) => Some(phase(tag).to_owned()),
            Activity::Stopping => Some(STOPPING.to_owned()),
            Activity::Idle => match availability {
                Availability::Missing => Some(NOT_FOUND.to_owned()),
                Availability::DriveDisconnected => Some(DRIVE_DISCONNECTED.to_owned()),
                Availability::Unreadable => Some(UNREADABLE.to_owned()),
                Availability::Present | Availability::Unknown => None,
            },
        }
    }

    /// "Hollow Knight. Offline. Played 12 hours 5 minutes, last yesterday."
    pub fn accessible_name(
        name: &str,
        preset: Preset,
        state: Option<&str>,
        total_ms: u64,
        last: Option<Timestamp>,
        now: Timestamp,
        offset_s: i32,
    ) -> String {
        let mut s = format!("{name}. {}.", shield::preset_name(preset));
        if let Some(state) = state {
            s.push_str(&format!(" {}.", state.trim_end_matches('…')));
        }
        match last {
            Some(t) if total_ms > 0 => s.push_str(&format!(
                " Played {}, last {}.",
                fmt::duration_spoken(total_ms),
                fmt::relative_date(t, now, offset_s).to_lowercase()
            )),
            _ => s.push_str(&format!(" {NOT_PLAYED}.")),
        }
        s
    }
}

/// The Play pill (UI_DESIGN_SYSTEM §7.1).
pub mod play {
    /// Ready.
    pub const PLAY: &str = "Play";
    /// Running.
    pub const STOP: &str = "Stop";
    /// Stopping.
    pub const STOPPING: &str = "Stopping…";
    /// Unavailable.
    pub const LOCATE: &str = "Locate…";
    /// Needs runtime.
    pub const DOWNLOAD_AND_PLAY: &str = "Download and play";
    /// Installer.
    pub const RUN_INSTALLER: &str = "Run installer";

    /// "Play Hollow Knight".
    pub fn play_name(name: &str) -> String {
        format!("Play {name}")
    }
    /// "Stop Hollow Knight".
    pub fn stop_name(name: &str) -> String {
        format!("Stop {name}")
    }
    /// "Starting Hollow Knight, preparing".
    pub fn busy_name(name: &str, phase: &str) -> String {
        format!("Starting {name}, {phase}")
    }
    /// "Locate Hollow Knight".
    pub fn locate_name(name: &str) -> String {
        format!("Locate {name}")
    }
}

/// Protection presets and the shield (UI_DESIGN_SYSTEM §7.3).
pub mod shield {
    use super::*;

    /// "Offline", "Custom".
    pub fn preset_name(p: Preset) -> &'static str {
        match p {
            Preset::Offline => "Offline",
            Preset::Online => "Online",
            Preset::Standard => "Standard",
            Preset::Custom { .. } => "Custom",
        }
    }

    /// The preset's one-line description.
    pub fn base_description(b: BasePreset) -> &'static str {
        match b {
            BasePreset::Offline => "No internet. Can use only its own folder.",
            BasePreset::Online => "Internet allowed. Can use only its own folder.",
            BasePreset::Standard => "Internet allowed. Fewer restrictions for best compatibility.",
        }
    }

    /// The description for any preset.
    pub fn description(p: Preset) -> String {
        match p {
            Preset::Offline => base_description(BasePreset::Offline).to_owned(),
            Preset::Online => base_description(BasePreset::Online).to_owned(),
            Preset::Standard => base_description(BasePreset::Standard).to_owned(),
            Preset::Custom { base } => format!(
                "Changed from {}. {}",
                preset_name(match base {
                    BasePreset::Offline => Preset::Offline,
                    BasePreset::Online => Preset::Online,
                    BasePreset::Standard => Preset::Standard,
                }),
                base_description(base)
            ),
        }
    }

    /// "Protection: Offline. No internet. Can use only its own folder."
    pub fn accessible_name(p: Preset) -> String {
        format!("Protection: {}. {}", preset_name(p), description(p))
    }

    /// Shown instead when attention is needed.
    pub const ATTENTION: &str = "Protection needs attention";
}

/// Access statements (SANDBOX `explain` sentences arrive with SANDBOX-019).
pub mod access {
    /// Network off.
    pub const NO_INTERNET: &str = "Cannot use the internet";
    /// Network on.
    pub const INTERNET: &str = "Can use the internet";
    /// Own folder.
    pub const OWN_FOLDER: &str = "Can read and change files in its own folder";
    /// Controllers.
    pub const CONTROLLERS: &str = "Can use connected controllers";
    /// Link to the full list.
    pub const SEE_ALL: &str = "See everything this game can reach";
}

/// Toasts and banners.
pub mod feedback {
    /// Dismiss button.
    pub const DISMISS: &str = "Dismiss";
    /// Undo action.
    pub const UNDO: &str = "Undo";
    /// Details action.
    pub const DETAILS: &str = "Details";
    /// Close button.
    pub const CLOSE: &str = "Close";
    /// Toast region name.
    pub const NOTIFICATIONS: &str = "Notifications";
    /// Demo banner shown while the UI runs without a backend.
    pub const PREVIEW_BANNER: &str =
        "Preview mode: this library is sample data. Games cannot be started yet.";

    /// Clipboard.
    pub const COPIED: &str = "Copied";
    /// `Ctrl+Z`.
    pub const UNDONE: &str = "Undone";
    /// Image dropped on a card.
    pub const COVER_SET: &str = "Cover changed";

    /// "Removed Hollow Knight".
    pub fn removed(name: &str) -> String {
        format!("Removed {name}")
    }
    /// "Stopped Hollow Knight".
    pub fn stopped(name: &str) -> String {
        format!("Stopped {name}")
    }
    /// "Added Hollow Knight to the library".
    pub fn added(name: &str) -> String {
        format!("Added {name} to the library")
    }
    /// "Reset private Windows storage for Hollow Knight".
    pub fn prefix_reset(name: &str) -> String {
        format!("Reset private Windows storage for {name}")
    }
    /// "Changed tags of Hollow Knight".
    pub fn tags_changed(name: &str) -> String {
        format!("Changed tags of {name}")
    }
    /// Menu shortcut toast.
    pub fn shortcut(name: &str, on: bool) -> String {
        if on {
            format!("Added {name} to the applications menu")
        } else {
            format!("Removed {name} from the applications menu")
        }
    }
    /// Hide toast.
    pub fn hidden(name: &str, on: bool) -> String {
        if on {
            format!("Hid {name}")
        } else {
            format!("{name} is shown again")
        }
    }
    /// "Renamed to Hollow Knight".
    pub fn renamed(name: &str) -> String {
        format!("Renamed to {name}")
    }
    /// Saved tick.
    pub const SAVED: &str = "Saved";

    /// "Starting Hollow Knight is not available in preview mode."
    pub fn preview_action(action: &str, name: &str) -> String {
        format!("{action} {name} is not available in preview mode.")
    }
    /// "Added Hollow Knight to favourites."
    pub fn favorited(name: &str, on: bool) -> String {
        if on {
            format!("Added {name} to favourites")
        } else {
            format!("Removed {name} from favourites")
        }
    }
}

/// Placeholder screens that later tasks fill in.
pub mod placeholder {
    /// Title.
    pub const COMING_TITLE: &str = "Not built yet";
    /// Body.
    pub const COMING_BODY: &str =
        "This part of EasyPlay arrives in a later version of the preview.";
    /// Back to library.
    pub const BACK_TO_LIBRARY: &str = "Back to library";
}

/// The widget gallery (debug-only route, UI-004).
pub mod gallery {
    /// Section.
    pub const BUTTONS: &str = "Buttons";
    /// Section.
    pub const PLAY: &str = "Play pill";
    /// Section.
    pub const INPUTS: &str = "Inputs";
    /// Section.
    pub const DISPLAY: &str = "Display";
    /// Section.
    pub const CARDS: &str = "Game cards";
    /// Section.
    pub const OVERLAYS: &str = "Overlays";
    /// Section.
    pub const TYPE: &str = "Type scale";
    /// Section.
    pub const ICONS: &str = "Icons";
    /// Theme switch label.
    pub const THEME: &str = "Theme";
    /// Accent switch label.
    pub const ACCENT: &str = "Accent";
    /// Reduced motion toggle.
    pub const REDUCE_MOTION: &str = "Reduce motion";
    /// Reduced motion description.
    pub const REDUCE_MOTION_HINT: &str = "Turns off movement; short fades remain.";
    /// Sample labels.
    pub const PRIMARY: &str = "Primary";
    /// Sample labels.
    pub const SECONDARY: &str = "Secondary";
    /// Sample labels.
    pub const GHOST: &str = "Ghost";
    /// Sample labels.
    pub const DANGER: &str = "Remove game";
    /// Sample labels.
    pub const LINK: &str = "Open folder";
    /// Sample labels.
    pub const BUSY: &str = "Saving";
    /// Disabled reason.
    pub const DISABLED_REASON: &str = "Nothing to save yet.";
    /// Sample labels.
    pub const DISABLED: &str = "Save";
    /// Sample field.
    pub const FIELD_LABEL: &str = "Name";
    /// Sample field.
    pub const FIELD_HELP: &str = "Shown in the library and on the game page.";
    /// Sample field.
    pub const FIELD_ERROR: &str = "A name is required.";
    /// Sample toggle.
    pub const TOGGLE_LABEL: &str = "Show playtime on cards";
    /// Sample toggle.
    pub const TOGGLE_HINT: &str = "Playtime and last played under each title.";
    /// Sample chips.
    pub const CHIP_OFFLINE: &str = "Offline";
    /// Sample chips.
    pub const CHIP_RPG: &str = "rpg";
    /// Sample banner.
    pub const BANNER_INFO: &str = "A newer compatibility layer is available.";
    /// Sample banner.
    pub const BANNER_WARNING: &str = "Some games are on a drive that is not connected.";
    /// Sample banner.
    pub const BANNER_ACTION: &str = "Review";
    /// Sample sheet.
    pub const OPEN_SHEET: &str = "Open sheet";
    /// Sample dialog.
    pub const OPEN_DIALOG: &str = "Open dialog";
    /// Sample toast.
    pub const SHOW_TOAST: &str = "Show toast";
    /// Sample sheet.
    pub const SHEET_TITLE: &str = "Set up Hollow Knight";
    /// Sample sheet.
    pub const SHEET_BODY: &str = "Choose how much this game can reach. You can change this later.";
    /// Sample dialog.
    pub const DIALOG_TITLE: &str = "Remove Hollow Knight?";
    /// Sample dialog.
    pub const DIALOG_BODY: &str = "This removes the game from your library and deletes 2.1 GB of private storage. Your game files are kept.";
    /// Sample dialog.
    pub const KEEP: &str = "Keep game";
    /// Sample toast.
    pub const TOAST_TEXT: &str = "Saved";
    /// Stat tile.
    pub const STAT_LABEL: &str = "Total playtime";
    /// Stat tile.
    pub const STAT_CAPTION: &str = "across 24 games";
    /// Progress.
    pub const PROGRESS: &str = "Downloading";
    /// Segmented sample.
    pub const SEG_READ: &str = "Read only";
    /// Segmented sample.
    pub const SEG_WRITE: &str = "Read and change";
    /// Segmented sample group.
    pub const SEG_ACCESS: &str = "Access";
    /// Select sample.
    pub const SELECT_LABEL: &str = "Sort";
    /// Path row sample.
    pub const PATH_NAME: &str = "Music";
    /// Path row sample.
    pub const PATH_LOCATION: &str = "in Home";
    /// Path row sample.
    pub const REMOVE: &str = "Remove";
    /// Key hint sample.
    pub const SHORTCUT_PLAY: &str = "Play the selected game";
    /// Tab sample.
    pub const TAB_OVERVIEW: &str = "Overview";
    /// Tab sample.
    pub const TAB_PROTECTION: &str = "Protection";
    /// Tab sample.
    pub const TAB_COMPAT: &str = "Compatibility";
    /// Tab sample.
    pub const TABS: &str = "Game page sections";
    /// Sample game name.
    pub const SAMPLE_GAME: &str = "Lantern Keep";
    /// Sample tag.
    pub const CHIP_COOP: &str = "co-op";
    /// Sample path.
    pub const PATH_FULL: &str = "/home/user/Music";
    /// Shortest card name.
    pub const SHORT_NAME: &str = "A";
    /// Longest card name.
    pub const LONG_NAME: &str =
        "An Extraordinarily Long Name That Keeps Going: The Complete Collector’s Edition";
    /// Sample stat value.
    pub const STAT_VALUE: &str = "1,240 h";
    /// Sample stat value, spoken.
    pub const STAT_SPOKEN: &str = "1,240 hours";
    /// Sample clock.
    pub const CLOCK_SAMPLE: &str = "1:11:11 · 0:42:10";
}

/// Theme names for pickers.
pub mod appearance {
    use crate::theme::{Accent, ThemeChoice};

    /// Theme name.
    pub fn theme(c: ThemeChoice) -> &'static str {
        match c {
            ThemeChoice::System => "System",
            ThemeChoice::Dark => "Dark",
            ThemeChoice::Light => "Light",
            ThemeChoice::HighContrast => "High contrast",
        }
    }

    /// Accent name.
    pub fn accent(a: Accent) -> &'static str {
        match a {
            Accent::Tide => "Tide",
            Accent::Ember => "Ember",
            Accent::Iris => "Iris",
            Accent::Moss => "Moss",
        }
    }
}

/// Number, duration and date formatting (STATISTICS §8).
pub mod fmt {
    use super::Timestamp;

    const MS_PER_MIN: u64 = 60_000;
    const MS_PER_HOUR: u64 = 3_600_000;

    /// "under a minute", "42 min", "12 h 05 min", "1,240 h".
    pub fn duration(ms: u64) -> String {
        if ms < MS_PER_MIN {
            "under a minute".to_owned()
        } else if ms < MS_PER_HOUR {
            format!("{} min", ms / MS_PER_MIN)
        } else if ms < 100 * MS_PER_HOUR {
            format!(
                "{} h {:02} min",
                ms / MS_PER_HOUR,
                (ms % MS_PER_HOUR) / MS_PER_MIN
            )
        } else {
            format!("{} h", thousands(ms / MS_PER_HOUR))
        }
    }

    /// Full units for screen readers: "12 hours 5 minutes".
    pub fn duration_spoken(ms: u64) -> String {
        let h = ms / MS_PER_HOUR;
        let m = (ms % MS_PER_HOUR) / MS_PER_MIN;
        let hours = format!("{h} {}", super::plural(h, "hour", "hours"));
        let minutes = format!("{m} {}", super::plural(m, "minute", "minutes"));
        match (h, m) {
            (0, 0) => "under a minute".to_owned(),
            (0, _) => minutes,
            (_, 0) => hours,
            _ if h >= 100 => hours,
            _ => format!("{hours} {minutes}"),
        }
    }

    /// Elapsed time as a clock: "0:42:10".
    pub fn clock(ms: u64) -> String {
        let s = ms / 1000;
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    }

    /// "32 MB", "2.1 GB", "640 KB".
    pub fn bytes(n: u64) -> String {
        const K: f64 = 1000.0;
        let f = n as f64;
        if f < K {
            format!("{n} B")
        } else if f < K * K {
            format!("{:.0} KB", f / K)
        } else if f < K * K * K {
            format!("{:.0} MB", f / (K * K))
        } else {
            format!("{:.1} GB", f / (K * K * K))
        }
    }

    /// "1,240".
    pub fn thousands(n: u64) -> String {
        let digits = n.to_string();
        let mut out = String::with_capacity(digits.len() + digits.len() / 3);
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(ch);
        }
        out
    }

    const WEEKDAYS: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    /// Days since 1970-01-01 in local time.
    fn local_day(t: Timestamp, offset_s: i32) -> i64 {
        (t.as_millis() + i64::from(offset_s) * 1000).div_euclid(86_400_000)
    }

    /// (year, month 1–12, day) from days since the epoch (Howard Hinnant's algorithm).
    fn civil(days: i64) -> (i64, u32, u32) {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let y = yoe + era * 400 + i64::from(m <= 2);
        (y, m, d)
    }

    /// "Today", "Yesterday", a weekday within six days, then "3 Nov" (or "3 Nov 2025" in
    /// another year). `offset_s` is the local UTC offset.
    pub fn relative_date(t: Timestamp, now: Timestamp, offset_s: i32) -> String {
        let (day, today) = (local_day(t, offset_s), local_day(now, offset_s));
        match today - day {
            0 => "Today".to_owned(),
            1 => "Yesterday".to_owned(),
            2..=6 => WEEKDAYS[(day + 3).rem_euclid(7) as usize].to_owned(),
            _ => {
                let (y, m, d) = civil(day);
                let (ty, _, _) = civil(today);
                let month = MONTHS[(m - 1) as usize];
                if y == ty {
                    format!("{d} {month}")
                } else {
                    format!("{d} {month} {y}")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_follow_statistics_rules() {
        assert_eq!(fmt::duration(30_000), "under a minute");
        assert_eq!(fmt::duration(42 * 60_000), "42 min");
        assert_eq!(fmt::duration(12 * 3_600_000 + 5 * 60_000), "12 h 05 min");
        assert_eq!(fmt::duration(1240 * 3_600_000), "1,240 h");
        assert_eq!(
            fmt::duration_spoken(12 * 3_600_000 + 5 * 60_000),
            "12 hours 5 minutes"
        );
        assert_eq!(fmt::duration_spoken(60_000), "1 minute");
        assert_eq!(fmt::clock(42 * 60_000 + 10_000), "0:42:10");
        assert_eq!(fmt::thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn relative_dates() {
        // 2026-11-12T00:00:00Z is a Thursday.
        let now = Timestamp::from_millis(1_794_441_600_000 + 15 * 3_600_000);
        let day = |n: i64| Timestamp::from_millis(now.as_millis() - n * 86_400_000);
        assert_eq!(fmt::relative_date(now, now, 0), "Today");
        assert_eq!(fmt::relative_date(day(1), now, 0), "Yesterday");
        assert_eq!(fmt::relative_date(day(2), now, 0), "Tuesday");
        assert_eq!(fmt::relative_date(day(6), now, 0), "Friday");
        assert_eq!(fmt::relative_date(day(9), now, 0), "3 Nov");
        assert_eq!(fmt::relative_date(day(400), now, 0), "8 Oct 2025");
    }

    #[test]
    fn counts_and_names() {
        assert_eq!(library::result_count(1, 1), "1 game");
        assert_eq!(library::result_count(42, 42), "42 games");
        assert_eq!(library::result_count(3, 42), "3 of 42");
        assert_eq!(title::window("Library"), "Library — EasyPlay");
        assert_eq!(
            shield::accessible_name(Preset::Offline),
            "Protection: Offline. No internet. Can use only its own folder."
        );
    }

    /// SECURITY_MODEL §10 / USER_EXPERIENCE §9: no security theatre in any string.
    #[test]
    fn no_forbidden_claims() {
        let files = [
            include_str!("strings.rs"),
            include_str!("strings/artwork.rs"),
            include_str!("strings/details.rs"),
            include_str!("strings/dialogs.rs"),
            include_str!("strings/dnd.rs"),
            include_str!("strings/errors.rs"),
            include_str!("strings/launch.rs"),
            include_str!("strings/menu.rs"),
            include_str!("strings/onboarding.rs"),
            include_str!("strings/picker.rs"),
            include_str!("strings/settings.rs"),
            include_str!("strings/setup.rs"),
            include_str!("strings/shortcuts.rs"),
            include_str!("strings/stats.rs"),
            include_str!("strings/tags.rs"),
        ];
        for src in files {
            let body = &src[..src.find("#[cfg(test)]").unwrap_or(src.len())];
            let lower = body.to_lowercase();
            let words: Vec<&str> = lower
                .split(|c: char| !c.is_alphanumeric() && c != '%')
                .collect();
            for word in [
                "secure",
                "safe",
                "virus",
                "fatal",
                "100%",
                "guarantee",
                "guaranteed",
            ] {
                assert!(!words.contains(&word), "forbidden word {word:?}");
            }
        }
    }
}
