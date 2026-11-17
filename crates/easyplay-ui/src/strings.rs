//! Every user-visible string of the interface, grouped by screen: a function per text, read
//! from the language catalogues (`locales/*.yml`, ADR-0032). See MODULES §8 and task UI-029.
//!
//! Screens never concatenate sentences: anything with a variable part is a function here.
//! Number and date formatting follows STATISTICS §8.

use easyplay_core::clock::Timestamp;
use easyplay_core::model::{Activity, Availability, BasePreset, LaunchStateTag, Preset};

/// Declares catalogue texts: each becomes a function, named like the constant it replaces,
/// that returns the text of its key in the current language (ADR-0032).
macro_rules! texts {
    ($( $(#[$m:meta])* $name:ident = $key:literal; )*) => {
        $(
            $(#[$m])*
            #[allow(non_snake_case)]
            pub fn $name() -> &'static str {
                crate::i18n::text($key)
            }
        )*
    };
}

pub mod a11y;
pub mod artwork;
pub mod debug;
pub mod details;
pub mod dialogs;
pub mod dnd;
pub mod errors;
pub mod launch;
pub mod menu;
pub mod onboarding;
pub mod picker;
pub mod service;
pub mod settings;
pub mod setup;
pub mod shortcuts;
pub mod stats;
pub mod steam;
pub mod tags;

/// The application name.
pub const APP_NAME: &str = "EasyPlay";

/// Window titles.
pub mod title {
    /// "<Route> — EasyPlay".
    pub fn window(route: &str) -> String {
        rust_i18n::t!("title.window", route = route, app = super::APP_NAME).into_owned()
    }
}

/// The top bar's destinations and routes.
pub mod nav {
    texts! {
        /// Section label.
        LIBRARY = "nav.library";
        /// Route.
        STATISTICS = "nav.statistics";
        /// Route.
        SETTINGS = "nav.settings";
        /// Route.
        ONBOARDING = "nav.onboarding";
        /// Debug route.
        GALLERY = "nav.gallery";
        /// Accessible name of the top bar's destinations.
        LANDMARK = "nav.landmark";
        /// The running game in the top bar, while it starts.
        STARTING = "nav.starting";
        /// Tooltip of the running game's title in the top bar.
        OPEN_PAGE = "nav.open_page";
    }

    /// "Open the page of Lantern Keep".
    pub fn open_page_of(name: &str) -> String {
        rust_i18n::t!("nav.open_page_of", name = name).into_owned()
    }
}

/// Collections (GAME_LIBRARY §6).
pub mod collection {
    texts! {
        /// All games.
        ALL = "collection.all";
        /// Favourites.
        FAVORITES = "collection.favorites";
        /// Played within two weeks.
        RECENT = "collection.recent";
        /// Never played.
        NEVER_PLAYED = "collection.never_played";
        /// Running.
        RUNNING = "collection.running";
        /// Installed.
        INSTALLED = "collection.installed";
        /// Missing.
        MISSING = "collection.missing";
        /// Installers.
        INSTALLERS = "collection.installers";
        /// Hidden.
        HIDDEN = "collection.hidden";
    }
}

/// Host status row (SCREEN_SPECS §0).
pub mod host {
    texts! {
        /// Something needs the user.
        NEEDS_ATTENTION = "host.needs_attention";
        /// Background jobs are running.
        BUSY = "host.busy";
    }
    /// Accessible name prefix.
    pub fn status(label: &str) -> String {
        rust_i18n::t!("host.status", label = label).into_owned()
    }
}

/// Library screen (SCREEN_SPECS §2).
pub mod library {
    texts! {
        /// Screen title.
        TITLE = "library.title";
        /// Search hint.
        SEARCH_HINT = "library.search_hint";
        /// Search field accessible name.
        SEARCH = "library.search";
        /// Clear search.
        CLEAR_SEARCH = "library.clear_search";
        /// View switch.
        VIEW_GRID = "library.view_grid";
        /// View switch.
        VIEW_LIST = "library.view_list";
        /// View switch group name.
        VIEW = "library.view";
    }
    /// The search field's key hint.
    pub const KEY_SEARCH: &str = "/";
    texts! {
        /// "Add game" in the top bar: its tooltip.
        ADD_GAME_TIP = "library.add_game_tip";
        /// "Add game": its shortcut.
        KEY_ADD = "library.key_add";
        /// The sort word's tooltip.
        SORT_TIP = "library.sort_tip";
        /// The filter line's accessible name.
        FILTER_LINE = "library.filter_line";
        /// Clearing the selection: its shortcut.
        KEY_ESC = "library.key_esc";
        /// Selection actions.
        SEL_TAGS = "library.sel_tags";
        /// Selection actions.
        SEL_FAVOURITE = "library.sel_favourite";
        /// Selection actions.
        SEL_HIDE = "library.sel_hide";
        /// Selection actions.
        SEL_PROTECTION = "library.sel_protection";
        /// Sort menu: orders.
        ORDER_RECENT_FIRST = "library.order_recent_first";
        /// Sort menu: orders.
        ORDER_OLDEST_FIRST = "library.order_oldest_first";
        /// Sort menu: orders.
        ORDER_Z_A = "library.order_z_a";
        /// Sort menu: orders.
        ORDER_A_Z = "library.order_a_z";
        /// Sort menu: orders.
        ORDER_MOST_PLAYED = "library.order_most_played";
        /// Sort menu: orders.
        ORDER_LEAST_PLAYED = "library.order_least_played";
        /// Sort menu: orders.
        ORDER_NEWEST_FIRST = "library.order_newest_first";
        /// Sort menu: orders.
        ORDER_MOST_LAUNCHED = "library.order_most_launched";
        /// Sort menu: orders.
        ORDER_LEAST_LAUNCHED = "library.order_least_launched";
        /// Sort menu: orders (protection, Custom last when ascending).
        ORDER_OPEN_FIRST = "library.order_open_first";
        /// Sort menu: orders.
        ORDER_CLOSED_FIRST = "library.order_closed_first";
        /// Grouping.
        GROUP_PLAYED = "library.group_played";
        /// Group heading.
        GROUP_NOT_PLAYED = "library.group_not_played";
        /// Group heading.
        GROUP_HAS_PLAYED = "library.group_has_played";
        /// Group heading of games without a tag.
        NO_TAG = "library.no_tag";
        /// Empty states: the first game.
        FIRST_TITLE = "library.first_title";
        /// Empty states.
        FIRST_TEXT = "library.first_text";
        /// Empty states.
        FIRST_FINE = "library.first_fine";
        /// Empty states.
        TRY_AGAIN = "library.try_again";
        /// Empty states: nothing matches.
        NO_MATCH_FILTERS = "library.no_match_filters";
        /// Empty states.
        NO_MATCH_TEXT = "library.no_match_text";
        /// Empty states.
        CLEAR = "library.clear";
        /// Empty states.
        FAV_TITLE = "library.fav_title";
        /// Empty states.
        FAV_TEXT = "library.fav_text";
        /// Empty states.
        RECENT_TITLE = "library.recent_title";
        /// Empty states.
        RECENT_TEXT = "library.recent_text";
        /// Empty states.
        NEVER_TITLE = "library.never_title";
        /// Empty states.
        NEVER_TEXT = "library.never_text";
        /// Empty states.
        OTHER_TITLE = "library.other_title";
        /// Empty states.
        OTHER_TEXT = "library.other_text";
        /// Empty states.
        SHOW_ALL = "library.show_all";
        /// The list's accessible name.
        LIST = "library.list";
    }

    /// "Nothing matches “lantern”".
    pub fn no_match(query: &str) -> String {
        rust_i18n::t!("library.no_match", query = query).into_owned()
    }

    /// "13 of 17".
    pub fn of_count(shown: usize, total: usize) -> String {
        rust_i18n::t!("library.of_count", shown = shown, total = total).into_owned()
    }
    texts! {
        /// Sort select label.
        SORT = "library.sort";
        /// Filter button.
        FILTER = "library.filter";
        /// Primary action.
        ADD_GAME = "library.add_game";
        /// Grid accessible name prefix.
        GRID = "library.grid";
        /// Sort keys.
        SORT_NAME = "library.sort_name";
        /// Sort keys.
        SORT_RECENT = "library.sort_recent";
        /// Sort keys.
        SORT_PLAYTIME = "library.sort_playtime";
        /// Sort keys.
        SORT_ADDED = "library.sort_added";
        /// Sort keys.
        SORT_LAUNCHES = "library.sort_launches";
        /// Sort keys.
        SORT_PROTECTION = "library.sort_protection";
        /// Sort keys.
        SORT_LAYER = "library.sort_layer";
        /// Sort menu: section label.
        SORT_BY = "library.sort_by";
        /// Sort menu: section label.
        GROUP_BY = "library.group_by";
        /// Groupings.
        GROUP_NONE = "library.group_none";
        /// Groupings.
        GROUP_LETTER = "library.group_letter";
        /// Groupings.
        GROUP_TAG = "library.group_tag";
        /// Groupings.
        GROUP_PROTECTION = "library.group_protection";
        /// List columns.
        COL_NAME = "library.col_name";
        /// List columns.
        COL_PROTECTION = "library.col_protection";
        /// List columns.
        COL_LAST_PLAYED = "library.col_last_played";
        /// List columns.
        COL_PLAYTIME = "library.col_playtime";
        /// Filter groups.
        FILTER_TAGS = "library.filter_tags";
        /// Filter groups.
        FILTER_PROTECTION = "library.filter_protection";
        /// Filter groups.
        FILTER_INTERNET = "library.filter_internet";
        /// Filter groups.
        FILTER_AVAILABILITY = "library.filter_availability";
        /// Filter groups.
        FILTER_LAYER = "library.filter_layer";
        /// Filter groups.
        FILTER_TYPE = "library.filter_type";
        /// Selection bar.
        CLEAR_SELECTION = "library.clear_selection";
        /// Selection bar.
        REMOVE_TAG = "library.remove_tag";
        /// Selection bar.
        REMOVE_SELECTED = "library.remove_selected";
    }

    /// The sort button's accessible name: "Sort: Name, ascending".
    pub fn sort_button(label: &str, descending: bool) -> String {
        let key = if descending {
            "library.sort_button.descending"
        } else {
            "library.sort_button.ascending"
        };
        rust_i18n::t!(key, sort = SORT(), label = label).into_owned()
    }

    /// The sort button's tooltip: "Sorted by Name, ascending" and, while grouping is on,
    /// ", grouped by Tag".
    pub fn arranged_by(sort: &str, descending: bool, group: Option<&str>) -> String {
        let by = sorted_by(sort, descending);
        match group {
            Some(g) => rust_i18n::t!("library.arranged_by_group", by = by, group = g).into_owned(),
            None => by,
        }
    }

    /// Preset filter labels.
    pub fn preset_filter(p: easyplay_core::library::PresetFilter) -> &'static str {
        use easyplay_core::library::PresetFilter as P;
        match p {
            P::Offline => crate::i18n::text("library.preset_filter.offline"),
            P::Online => crate::i18n::text("library.preset_filter.online"),
            P::Standard => crate::i18n::text("library.preset_filter.standard"),
            P::Custom => crate::i18n::text("library.preset_filter.custom"),
        }
    }

    /// Internet filter labels.
    pub fn network_filter(n: easyplay_core::model::NetworkAccess) -> &'static str {
        match n {
            easyplay_core::model::NetworkAccess::None => {
                crate::i18n::text("library.network_filter.no_internet")
            }
            easyplay_core::model::NetworkAccess::Full => {
                crate::i18n::text("library.network_filter.internet_allowed")
            }
        }
    }

    /// Availability filter labels.
    pub fn availability_filter(a: super::Availability) -> &'static str {
        match a {
            super::Availability::Present => {
                crate::i18n::text("library.availability_filter.installed")
            }
            super::Availability::Missing => {
                crate::i18n::text("library.availability_filter.not_found")
            }
            super::Availability::DriveDisconnected => {
                crate::i18n::text("library.availability_filter.drive_not_connected")
            }
            super::Availability::Unreadable => {
                crate::i18n::text("library.availability_filter.can_t_be_read")
            }
            super::Availability::Unknown => {
                crate::i18n::text("library.availability_filter.not_checked_yet")
            }
        }
    }

    /// Type filter labels.
    pub fn kind_filter(k: easyplay_core::model::EntryKind) -> &'static str {
        match k {
            easyplay_core::model::EntryKind::Game => crate::i18n::text("library.kind_filter.games"),
            easyplay_core::model::EntryKind::Installer => {
                crate::i18n::text("library.kind_filter.installers")
            }
            easyplay_core::model::EntryKind::Tool => crate::i18n::text("library.kind_filter.tools"),
        }
    }

    /// "3 selected".
    pub fn selected_count(n: usize) -> String {
        rust_i18n::t!("library.selected_count", n = n).into_owned()
    }

    /// "Sorted by Playtime, descending".
    pub fn sorted_by(label: &str, descending: bool) -> String {
        let key = if descending {
            "library.sorted_by.descending"
        } else {
            "library.sorted_by.ascending"
        };
        rust_i18n::t!(key, label = label).into_owned()
    }

    /// "Changed 3 games".
    pub fn bulk_changed(n: usize) -> String {
        crate::i18n::plural::counted("library.bulk_changed", n as u64)
    }

    /// "3 changed, 1 skipped (running)".
    pub fn bulk_summary(changed: usize, skipped: usize) -> String {
        if skipped == 0 {
            bulk_changed(changed)
        } else {
            rust_i18n::t!("library.bulk_summary", changed = changed, skipped = skipped).into_owned()
        }
    }

    /// "42 games" or "3 of 42".
    pub fn result_count(shown: usize, total: usize) -> String {
        if shown == total {
            crate::i18n::plural::counted("library.result_count", total as u64)
        } else {
            of_count(shown, total)
        }
    }

    /// "Remove rpg" for a removable chip.
    pub fn remove_chip(label: &str) -> String {
        rust_i18n::t!("library.remove_chip", label = label).into_owned()
    }

    /// "Games, 42 items, 6 columns".
    pub fn grid_name(items: usize, columns: usize) -> String {
        rust_i18n::t!(
            "library.grid_name",
            grid = GRID(),
            items = crate::i18n::plural::counted("library.grid_items", items as u64),
            columns = crate::i18n::plural::counted("library.grid_columns", columns as u64)
        )
        .into_owned()
    }
}

/// Game cards and rows (SCREEN_SPECS §3, GAME_LIBRARY §2).
pub mod card {
    use super::*;

    texts! {
        /// Favourite toggle.
        FAVORITE = "card.favorite";
        /// Status: no sessions yet.
        NOT_PLAYED = "card.not_played";
        /// Status: played, with no date of last play (a Steam game Steam gives no date for,
        /// ADR-0031) and its time not shown under covers.
        PLAYED = "card.played";
        /// Status.
        NOT_FOUND = "card.not_found";
        /// Status.
        DRIVE_DISCONNECTED = "card.drive_disconnected";
        /// Status.
        UNREADABLE = "card.unreadable";
        /// Status.
        INSTALLER = "card.installer";
        /// Status.
        NEEDS_ATTENTION = "card.needs_attention";
        /// Status.
        STILL_RUNNING = "card.still_running";
        /// Status.
        STOPPING = "card.stopping";
        /// Status: a session is running (the elapsed time follows).
        RUNNING = "card.running";
        /// Caption: the game is starting.
        STARTING = "card.starting";
    }

    // Time played, last played and a session's clock are written by `strings::fmt`
    // (`fmt::play_time`, `fmt::last_played`, `fmt::session_clock`).

    /// The line under a cover: what the game is doing, or how much it has been played
    /// ("12 h, yesterday", "Not played yet", "Not found", "Installer", "Running 0:42",
    /// "Starting…"). Without playtime only when it was last played.
    #[allow(clippy::too_many_arguments)]
    pub fn caption_meta(
        activity: Activity,
        availability: Availability,
        installer: bool,
        total_ms: u64,
        last: Option<Timestamp>,
        show_playtime: bool,
        now: Timestamp,
        offset_s: i32,
    ) -> String {
        match activity {
            Activity::Running { since } => {
                let ms = now.since(since).map_or(0, |d| d.as_millis() as u64);
                return rust_i18n::t!(
                    "card.caption_running",
                    state = RUNNING(),
                    clock = fmt::session_clock(ms)
                )
                .into_owned();
            }
            Activity::Launching(_) => return STARTING().to_owned(),
            Activity::Stopping => return STOPPING().to_owned(),
            Activity::Lingering => return STILL_RUNNING().to_owned(),
            Activity::Idle => {}
        }
        match availability {
            Availability::Missing => return NOT_FOUND().to_owned(),
            Availability::DriveDisconnected => return DRIVE_DISCONNECTED().to_owned(),
            Availability::Unreadable => return UNREADABLE().to_owned(),
            Availability::Present | Availability::Unknown => {}
        }
        if installer {
            return INSTALLER().to_owned();
        }
        if last.is_none() {
            // Time without a date: a Steam game Steam gives no date for (ADR-0031).
            return match (total_ms, show_playtime) {
                (0, _) => NOT_PLAYED().to_owned(),
                (_, true) => fmt::play_time(total_ms),
                (_, false) => PLAYED().to_owned(),
            };
        }
        let when = fmt::last_played(last, now, offset_s);
        if show_playtime {
            // Lower-casing is a no-op in scripts without case.
            rust_i18n::t!(
                "card.caption_time_last",
                time = fmt::play_time(total_ms),
                when = when.to_lowercase()
            )
            .into_owned()
        } else {
            when
        }
    }

    /// "Running · 42:10" (the session's clock, as the caption writes it).
    pub fn running(elapsed_ms: u64) -> String {
        rust_i18n::t!(
            "card.running_clock",
            state = RUNNING(),
            clock = fmt::session_clock(elapsed_ms)
        )
        .into_owned()
    }

    /// The short phase text for a launch state.
    pub fn phase(tag: LaunchStateTag) -> &'static str {
        match tag {
            LaunchStateTag::Validating
            | LaunchStateTag::ResolvingRuntime
            | LaunchStateTag::AuditingRuntime => crate::i18n::text("card.phase.checking"),
            LaunchStateTag::Provisioning => crate::i18n::text("card.phase.downloading"),
            LaunchStateTag::Preparing | LaunchStateTag::Planning => {
                crate::i18n::text("card.phase.preparing")
            }
            LaunchStateTag::Spawning | LaunchStateTag::Starting => {
                crate::i18n::text("card.phase.starting")
            }
        }
    }

    /// State text when the game is not idle or not present; `None` for the normal case.
    pub fn state(activity: Activity, availability: Availability, now: Timestamp) -> Option<String> {
        match activity {
            Activity::Running { since } => {
                let ms = now.since(since).map_or(0, |d| d.as_millis() as u64);
                Some(running(ms))
            }
            Activity::Lingering => Some(STILL_RUNNING().to_owned()),
            Activity::Launching(tag) => Some(phase(tag).to_owned()),
            Activity::Stopping => Some(STOPPING().to_owned()),
            Activity::Idle => match availability {
                Availability::Missing => Some(NOT_FOUND().to_owned()),
                Availability::DriveDisconnected => Some(DRIVE_DISCONNECTED().to_owned()),
                Availability::Unreadable => Some(UNREADABLE().to_owned()),
                Availability::Present | Availability::Unknown => None,
            },
        }
    }

    /// "Hollow Knight. Offline. Played 12 hours 5 minutes, last yesterday." `protection`
    /// is the level's name, or "Run by Steam" for a Steam game (ADR-0031).
    pub fn accessible_name(
        name: &str,
        protection: &str,
        state: Option<&str>,
        total_ms: u64,
        last: Option<Timestamp>,
        now: Timestamp,
        offset_s: i32,
    ) -> String {
        let mut parts = vec![
            rust_i18n::t!("card.a11y.head", name = name, protection = protection).into_owned(),
        ];
        if let Some(state) = state {
            parts.push(
                rust_i18n::t!("card.a11y.state", state = state.trim_end_matches('…')).into_owned(),
            );
        }
        parts.push(match last {
            Some(t) if total_ms > 0 => rust_i18n::t!(
                "card.a11y.played_last",
                time = fmt::duration_spoken(total_ms),
                last = fmt::relative_inline(t, now, offset_s)
            )
            .into_owned(),
            // Time without a date (a Steam game, ADR-0031).
            None if total_ms > 0 => {
                rust_i18n::t!("card.a11y.played", time = fmt::duration_spoken(total_ms))
                    .into_owned()
            }
            _ => rust_i18n::t!("card.a11y.not_played", state = NOT_PLAYED()).into_owned(),
        });
        parts.join(" ")
    }
}

/// The Play pill (UI_DESIGN_SYSTEM §7.1).
pub mod play {
    texts! {
        /// Ready.
        PLAY = "play.play";
        /// Running.
        STOP = "play.stop";
        /// Stopping.
        STOPPING = "play.stopping";
        /// Unavailable.
        LOCATE = "play.locate";
        /// Needs runtime.
        DOWNLOAD_AND_PLAY = "play.download_and_play";
        /// Installer.
        RUN_INSTALLER = "play.run_installer";
    }

    /// "Play Hollow Knight".
    pub fn play_name(name: &str) -> String {
        rust_i18n::t!("play.play_name", name = name).into_owned()
    }
    /// "Stop Hollow Knight".
    pub fn stop_name(name: &str) -> String {
        rust_i18n::t!("play.stop_name", name = name).into_owned()
    }
    /// "Starting Hollow Knight, preparing".
    pub fn busy_name(name: &str, phase: &str) -> String {
        rust_i18n::t!("play.busy_name", name = name, phase = phase).into_owned()
    }
    /// "Locate Hollow Knight".
    pub fn locate_name(name: &str) -> String {
        rust_i18n::t!("play.locate_name", name = name).into_owned()
    }
}

/// Protection presets and the shield (UI_DESIGN_SYSTEM §7.3).
pub mod shield {
    use super::*;

    /// "Offline", "Custom".
    pub fn preset_name(p: Preset) -> &'static str {
        match p {
            Preset::Offline => crate::i18n::text("shield.preset_name.offline"),
            Preset::Online => crate::i18n::text("shield.preset_name.online"),
            Preset::Standard => crate::i18n::text("shield.preset_name.standard"),
            Preset::Custom { .. } => crate::i18n::text("shield.preset_name.custom"),
        }
    }

    /// The preset's one-line description.
    pub fn base_description(b: BasePreset) -> &'static str {
        match b {
            BasePreset::Offline => crate::i18n::text("shield.base_description.no_internet_can_use"),
            BasePreset::Online => {
                crate::i18n::text("shield.base_description.internet_allowed_can_use")
            }
            BasePreset::Standard => {
                crate::i18n::text("shield.base_description.internet_allowed_fewer_restrictions")
            }
        }
    }

    /// The description for any preset.
    pub fn description(p: Preset) -> String {
        match p {
            Preset::Offline => base_description(BasePreset::Offline).to_owned(),
            Preset::Online => base_description(BasePreset::Online).to_owned(),
            Preset::Standard => base_description(BasePreset::Standard).to_owned(),
            Preset::Custom { base } => rust_i18n::t!(
                "shield.changed_from",
                preset = preset_name(match base {
                    BasePreset::Offline => Preset::Offline,
                    BasePreset::Online => Preset::Online,
                    BasePreset::Standard => Preset::Standard,
                }),
                description = base_description(base)
            )
            .into_owned(),
        }
    }

    /// "Protection: Offline. No internet. Can use only its own folder."
    pub fn accessible_name(p: Preset) -> String {
        rust_i18n::t!(
            "shield.accessible_name",
            preset = preset_name(p),
            description = description(p)
        )
        .into_owned()
    }

    texts! {
        /// Shown instead when attention is needed.
        ATTENTION = "shield.attention";
    }
}

/// Access statements (SANDBOX `explain` sentences arrive with SANDBOX-019).
pub mod access {}

/// Toasts and banners.
pub mod feedback {
    texts! {
        /// Dismiss button.
        DISMISS = "feedback.dismiss";
        /// Undo action.
        UNDO = "feedback.undo";
        /// Close button.
        CLOSE = "feedback.close";
        /// Toast region name.
        NOTIFICATIONS = "feedback.notifications";
        /// Demo banner shown while the UI runs without a backend.
        PREVIEW_BANNER = "feedback.preview_banner";
        /// Clipboard.
        COPIED = "feedback.copied";
        /// `Ctrl+Z`.
        UNDONE = "feedback.undone";
        /// Image dropped on a card.
        COVER_SET = "feedback.cover_set";
    }

    /// "Removed Hollow Knight".
    pub fn removed(name: &str) -> String {
        rust_i18n::t!("feedback.removed", name = name).into_owned()
    }
    /// "Stopped Hollow Knight".
    pub fn stopped(name: &str) -> String {
        rust_i18n::t!("feedback.stopped", name = name).into_owned()
    }
    /// "Added Hollow Knight".
    pub fn added(name: &str) -> String {
        rust_i18n::t!("feedback.added", name = name).into_owned()
    }
    /// "Hid 3 games".
    pub fn hid_many(n: usize) -> String {
        crate::i18n::plural::counted("feedback.hid_many", n as u64)
    }
    /// "3 games set to Online".
    pub fn set_to(n: usize, level: &str) -> String {
        crate::i18n::plural::counted_args("feedback.set_to", n as u64, &[("level", level)])
    }
    /// "Added 3 games to Favourites", "Removed 3 games from Favourites".
    pub fn favorited_many(n: usize, on: bool) -> String {
        let games = crate::i18n::plural::counted("feedback.games", n as u64);
        let key = if on {
            "feedback.favorites_added_many"
        } else {
            "feedback.favorites_removed_many"
        };
        rust_i18n::t!(key, games = games).into_owned()
    }
    /// "Reset private Windows storage for Hollow Knight".
    pub fn prefix_reset(name: &str) -> String {
        rust_i18n::t!("feedback.prefix_reset", name = name).into_owned()
    }
    /// Menu shortcut toast.
    pub fn shortcut(name: &str, on: bool) -> String {
        let key = if on {
            "feedback.shortcut_added"
        } else {
            "feedback.shortcut_removed"
        };
        rust_i18n::t!(key, name = name).into_owned()
    }
    /// Hide toast.
    pub fn hidden(name: &str, on: bool) -> String {
        let key = if on {
            "feedback.hidden_on"
        } else {
            "feedback.hidden_off"
        };
        rust_i18n::t!(key, name = name).into_owned()
    }
    /// "Renamed to Hollow Knight".
    pub fn renamed(name: &str) -> String {
        rust_i18n::t!("feedback.renamed", name = name).into_owned()
    }
    texts! {
        /// Saved tick.
        SAVED = "feedback.saved";
    }

    /// "Added to Favourites", "Removed from Favourites" (the game is the one in front of
    /// you; `_name` is kept for callers).
    pub fn favorited(_name: &str, on: bool) -> String {
        let key = if on {
            "feedback.favorites_added"
        } else {
            "feedback.favorites_removed"
        };
        crate::i18n::text(key).to_owned()
    }
}

/// Placeholder screens that later tasks fill in.
pub mod placeholder {}

/// The widget gallery (debug-only route, UI-004).
pub mod gallery {
    texts! {
        /// Section.
        APPEARANCE = "gallery.appearance";
        /// Section.
        TYPE = "gallery.type";
        /// Section.
        ICONS = "gallery.icons";
        /// Section.
        BUTTONS = "gallery.buttons";
        /// Section.
        PLAY = "gallery.play";
        /// Section.
        WORDS = "gallery.words";
        /// Section.
        INPUTS = "gallery.inputs";
        /// Section.
        ROWS = "gallery.rows";
        /// Section.
        NOTES = "gallery.notes";
        /// Section.
        LOADING = "gallery.loading";
        /// Section.
        BLANK = "gallery.blank";
        /// Section.
        COVERS = "gallery.covers";
        /// Section.
        OVERLAYS = "gallery.overlays";
        /// Section.
        MARKS = "gallery.marks";
        /// Chart sample: its name.
        CHART_NAME = "gallery.chart_name";
    }
    /// Chart sample: the days.
    #[allow(non_snake_case)]
    pub fn CHART_DAYS() -> [&'static str; 7] {
        [
            crate::i18n::text("gallery.chart_days.10_nov"),
            crate::i18n::text("gallery.chart_days.11_nov"),
            crate::i18n::text("gallery.chart_days.12_nov"),
            crate::i18n::text("gallery.chart_days.13_nov"),
            crate::i18n::text("gallery.chart_days.14_nov"),
            crate::i18n::text("gallery.chart_days.15_nov"),
            crate::i18n::text("gallery.chart_days.16_nov"),
        ]
    }
    /// Chart sample: the values.
    #[allow(non_snake_case)]
    pub fn CHART_VALUES() -> [&'static str; 7] {
        [
            crate::i18n::text("gallery.chart_values.0_min"),
            crate::i18n::text("gallery.chart_values.2_h_10_min"),
            crate::i18n::text("gallery.chart_values.0_min_2"),
            crate::i18n::text("gallery.chart_values.45_min"),
            crate::i18n::text("gallery.chart_values.0_min_3"),
            crate::i18n::text("gallery.chart_values.0_min_4"),
            crate::i18n::text("gallery.chart_values.45_min_2"),
        ]
    }
    texts! {
        /// Theme switch label.
        THEME = "gallery.theme";
        /// Accent switch label.
        ACCENT = "gallery.accent";
        /// Reduced motion switch.
        REDUCE_MOTION = "gallery.reduce_motion";
        /// Reduced motion description.
        REDUCE_MOTION_HINT = "gallery.reduce_motion_hint";
        /// Sample labels.
        PRIMARY = "gallery.primary";
        /// Sample labels.
        SECONDARY = "gallery.secondary";
        /// Sample labels.
        GHOST = "gallery.ghost";
        /// Sample labels.
        DANGER = "gallery.danger";
        /// Sample labels.
        WARN = "gallery.warn";
        /// Sample labels.
        LINK = "gallery.link";
        /// Sample labels.
        BUSY = "gallery.busy";
        /// Disabled reason.
        DISABLED_REASON = "gallery.disabled_reason";
        /// Sample labels.
        DISABLED = "gallery.disabled";
        /// Sample labels: a button that opens a menu.
        MENU = "gallery.menu";
        /// Sample labels: a button with a count.
        COUNTED = "gallery.counted";
        /// Sample field.
        FIELD_LABEL = "gallery.field_label";
        /// Sample field.
        FIELD_HELP = "gallery.field_help";
        /// Sample field.
        FIELD_ERROR = "gallery.field_error";
        /// Sample field: mono.
        FIELD_MONO = "gallery.field_mono";
        /// Sample switch row.
        TOGGLE_LABEL = "gallery.toggle_label";
        /// Sample switch row.
        TOGGLE_HINT = "gallery.toggle_hint";
        /// Sample checkbox.
        CHECK_LABEL = "gallery.check_label";
        /// Sample radio rows.
        PICK_A = "gallery.pick_a";
        /// Sample radio rows.
        PICK_A_NOTE = "gallery.pick_a_note";
        /// Sample radio rows.
        PICK_B = "gallery.pick_b";
        /// Sample slider.
        SLIDER = "gallery.slider";
        /// Sample slider value.
        SLIDER_VALUE = "gallery.slider_value";
        /// Sample segmented group.
        SEG_ACCESS = "gallery.seg_access";
        /// Sample select.
        SELECT_LABEL = "gallery.select_label";
    }
    /// Sample select options.
    #[allow(non_snake_case)]
    pub fn SELECT_OPTIONS() -> [&'static str; 3] {
        [
            crate::i18n::text("gallery.select_options.keep_running"),
            crate::i18n::text("gallery.select_options.close_it"),
            crate::i18n::text("gallery.select_options.ask_every_time"),
        ]
    }
    texts! {
        /// Sample inline select.
        INLINE_LABEL = "gallery.inline_label";
    }
    /// Sample inline select options.
    #[allow(non_snake_case)]
    pub fn INLINE_OPTIONS() -> [&'static str; 2] {
        [
            crate::i18n::text("gallery.inline_options.all_controllers"),
            crate::i18n::text("gallery.inline_options.the_first_controller"),
        ]
    }
    texts! {
        /// Swatch names.
        SWATCH = "gallery.swatch";
        /// Tab sample.
        TAB_OVERVIEW = "gallery.tab_overview";
        /// Tab sample.
        TAB_PROTECTION = "gallery.tab_protection";
        /// Tab sample.
        TAB_COMPAT = "gallery.tab_compat";
        /// Tab sample.
        TABS = "gallery.tabs";
        /// Key hint sample.
        SHORTCUT = "gallery.shortcut";
        /// Key hint sample.
        SHORTCUT_PLAY = "gallery.shortcut_play";
        /// Sample game name.
        SAMPLE_GAME = "gallery.sample_game";
        /// Why Play cannot start.
        BLOCKED = "gallery.blocked";
        /// Sample words: a tag.
        TAG = "gallery.tag";
        /// Sample words: a filter's remove name.
        REMOVE_TAG = "gallery.remove_tag";
        /// Sample words: the result count.
        COUNT = "gallery.count";
        /// Sample words: the selection.
        SELECTED = "gallery.selected";
        /// Sample words: the clear-selection word.
        CLEAR = "gallery.clear";
        /// Sample words: an action that destroys.
        REMOVE = "gallery.remove";
        /// Sample words: the sort word.
        SORT = "gallery.sort";
        /// Sample menu: section.
        MENU_SECTION = "gallery.menu_section";
    }
    /// Sample menu: options.
    #[allow(non_snake_case)]
    pub fn MENU_OPTIONS() -> [&'static str; 3] {
        [
            crate::i18n::text("gallery.menu_options.recently_played"),
            crate::i18n::text("gallery.menu_options.name"),
            crate::i18n::text("gallery.menu_options.time_played"),
        ]
    }
    /// Sample menu: a checklist with counts.
    #[allow(non_snake_case)]
    pub fn MENU_CHECKS() -> [&'static str; 2] {
        [
            crate::i18n::text("gallery.menu_checks.offline"),
            crate::i18n::text("gallery.menu_checks.online"),
        ]
    }
    texts! {
        /// Sample menu: a submenu.
        MENU_SUB = "gallery.menu_sub";
        /// Sample menu: an item that cannot be chosen, and why.
        MENU_OFF = "gallery.menu_off";
        /// Sample menu: why.
        MENU_OFF_WHY = "gallery.menu_off_why";
        /// Heading sample.
        HEADING = "gallery.heading";
        /// Heading sample with a note.
        HEADING_NOTED = "gallery.heading_noted";
        /// Heading sample: its note.
        HEADING_NOTE = "gallery.heading_note";
        /// Setting row sample.
        ROW_LABEL = "gallery.row_label";
        /// Setting row sample help.
        ROW_HELP = "gallery.row_help";
        /// Setting row sample, unmet.
        ROW_OFF = "gallery.row_off";
        /// Setting row sample, mono.
        ROW_MONO = "gallery.row_mono";
        /// Setting row sample, mono: its action.
        ROW_RUN = "gallery.row_run";
        /// Fact sample.
        FACT_LABEL = "gallery.fact_label";
        /// Fact sample.
        FACT_VALUE = "gallery.fact_value";
        /// Door sample.
        DOOR_LABEL = "gallery.door_label";
        /// Door sample.
        DOOR_VALUE = "gallery.door_value";
        /// Door sample, spoken.
        DOOR_NAME = "gallery.door_name";
        /// Door sample leading outside.
        DOOR_FOLDER = "gallery.door_folder";
        /// Door sample leading outside.
        DOOR_PATH = "gallery.door_path";
        /// Door sample leading outside, spoken.
        DOOR_FOLDER_NAME = "gallery.door_folder_name";
        /// Reach sample.
        CAN = "gallery.can";
        /// Reach sample.
        CAN_LIST = "gallery.can_list";
        /// Reach sample.
        CANT = "gallery.cant";
        /// Reach sample.
        CANT_LIST = "gallery.cant_list";
    }
    /// Table sample: header.
    #[allow(non_snake_case)]
    pub fn TABLE_HEAD() -> [&'static str; 3] {
        [
            crate::i18n::text("gallery.table_head.day"),
            crate::i18n::text("gallery.table_head.sessions"),
            crate::i18n::text("gallery.table_head.time_played"),
        ]
    }
    /// Table sample: rows.
    #[allow(non_snake_case)]
    pub fn TABLE_ROWS() -> [[&'static str; 3]; 2] {
        [
            [
                crate::i18n::text("gallery.table_rows.monday"),
                "2",
                crate::i18n::text("gallery.table_rows.1_h_10_min"),
            ],
            [
                crate::i18n::text("gallery.table_rows.tuesday"),
                "1",
                crate::i18n::text("gallery.table_rows.45_min"),
            ],
        ]
    }
    texts! {
        /// Sample banner.
        BANNER_INFO = "gallery.banner_info";
        /// Sample banner.
        BANNER_WARNING = "gallery.banner_warning";
        /// Sample banner.
        BANNER_ACTION = "gallery.banner_action";
        /// Sample note: a problem.
        NOTE_PROBLEM = "gallery.note_problem";
        /// Sample note: a problem's text.
        NOTE_PROBLEM_TEXT = "gallery.note_problem_text";
        /// Sample note: its code.
        NOTE_CODE = "gallery.note_code";
    }
    /// Sample note: its fixes.
    #[allow(non_snake_case)]
    pub fn NOTE_ACTIONS() -> [&'static str; 2] {
        [
            crate::i18n::text("gallery.note_actions.try_again"),
            crate::i18n::text("gallery.note_actions.create_a_report"),
        ]
    }
    texts! {
        /// Sample note: a missing game.
        NOTE_MISSING = "gallery.note_missing";
        /// Sample note: a missing game's text.
        NOTE_MISSING_TEXT = "gallery.note_missing_text";
        /// Sample note: something left running.
        NOTE_LINGERING = "gallery.note_lingering";
        /// Sample note: its text.
        NOTE_LINGERING_TEXT = "gallery.note_lingering_text";
        /// Progress.
        PROGRESS = "gallery.progress";
        /// Blank sample.
        BLANK_TITLE = "gallery.blank_title";
        /// Blank sample.
        BLANK_TEXT = "gallery.blank_text";
        /// Blank sample.
        BLANK_FINE = "gallery.blank_fine";
        /// Sample sheet.
        OPEN_SHEET = "gallery.open_sheet";
        /// Sample dialog.
        OPEN_DIALOG = "gallery.open_dialog";
        /// Sample toast.
        SHOW_TOAST = "gallery.show_toast";
        /// Sample toast.
        TOAST_TEXT = "gallery.toast_text";
        /// Sample clock.
        CLOCK_SAMPLE = "gallery.clock_sample";
        /// Shortest cover name.
        SHORT_NAME = "gallery.short_name";
        /// Longest cover name.
        LONG_NAME = "gallery.long_name";
    }
}

/// Theme names for pickers.
pub mod appearance {
    use crate::theme::{Accent, ThemeChoice};

    /// Theme name.
    pub fn theme(c: ThemeChoice) -> &'static str {
        match c {
            ThemeChoice::System => crate::i18n::text("appearance.theme.system"),
            ThemeChoice::Dark => crate::i18n::text("appearance.theme.dark"),
            ThemeChoice::Light => crate::i18n::text("appearance.theme.light"),
            ThemeChoice::HighContrast => crate::i18n::text("appearance.theme.high_contrast"),
        }
    }

    /// Accent name.
    pub fn accent(a: Accent) -> &'static str {
        match a {
            Accent::Tide => crate::i18n::text("appearance.accent.tide"),
            Accent::Ember => crate::i18n::text("appearance.accent.ember"),
            Accent::Iris => crate::i18n::text("appearance.accent.iris"),
            Accent::Moss => crate::i18n::text("appearance.accent.moss"),
        }
    }
}

/// Names of months and weekdays, and how a date is written (ADR-0032).
pub mod date {
    const MONTH_SHORT: [&str; 12] = [
        "date.month_short.1",
        "date.month_short.2",
        "date.month_short.3",
        "date.month_short.4",
        "date.month_short.5",
        "date.month_short.6",
        "date.month_short.7",
        "date.month_short.8",
        "date.month_short.9",
        "date.month_short.10",
        "date.month_short.11",
        "date.month_short.12",
    ];
    const MONTH_ABBR: [&str; 12] = [
        "date.month_abbr.1",
        "date.month_abbr.2",
        "date.month_abbr.3",
        "date.month_abbr.4",
        "date.month_abbr.5",
        "date.month_abbr.6",
        "date.month_abbr.7",
        "date.month_abbr.8",
        "date.month_abbr.9",
        "date.month_abbr.10",
        "date.month_abbr.11",
        "date.month_abbr.12",
    ];
    const MONTH_FULL: [&str; 12] = [
        "date.month_full.1",
        "date.month_full.2",
        "date.month_full.3",
        "date.month_full.4",
        "date.month_full.5",
        "date.month_full.6",
        "date.month_full.7",
        "date.month_full.8",
        "date.month_full.9",
        "date.month_full.10",
        "date.month_full.11",
        "date.month_full.12",
    ];
    const WEEKDAY_FULL: [&str; 7] = [
        "date.weekday_full.1",
        "date.weekday_full.2",
        "date.weekday_full.3",
        "date.weekday_full.4",
        "date.weekday_full.5",
        "date.weekday_full.6",
        "date.weekday_full.7",
    ];

    /// The short month ("Sept"), `i` counted from 0.
    pub fn month_short(i: usize) -> &'static str {
        crate::i18n::text(MONTH_SHORT[i % 12])
    }

    /// The abbreviated month ("Sep"), `i` counted from 0.
    pub fn month_abbr(i: usize) -> &'static str {
        crate::i18n::text(MONTH_ABBR[i % 12])
    }

    /// The full month name, `i` counted from 0.
    pub fn month_full(i: usize) -> &'static str {
        crate::i18n::text(MONTH_FULL[i % 12])
    }

    /// The weekday, Monday first, `i` counted from 0.
    pub fn weekday_full(i: usize) -> &'static str {
        crate::i18n::text(WEEKDAY_FULL[i % 7])
    }

    /// "Today".
    pub fn today() -> &'static str {
        crate::i18n::text("date.today")
    }

    /// "Yesterday".
    pub fn yesterday() -> &'static str {
        crate::i18n::text("date.yesterday")
    }

    /// "16 Nov" from a day and a month name.
    pub fn day_month(d: u32, month: &str) -> String {
        rust_i18n::t!("date.day_month", d = d, month = month).into_owned()
    }

    /// "16 Nov 2025".
    pub fn day_month_year(d: u32, month: &str, y: impl std::fmt::Display) -> String {
        rust_i18n::t!("date.day_month_year", d = d, month = month, y = y).into_owned()
    }

    /// "November 2025".
    pub fn month_year(month: &str, y: impl std::fmt::Display) -> String {
        rust_i18n::t!("date.month_year", month = month, y = y).into_owned()
    }
}

/// Number, duration and date formatting (STATISTICS §8).
pub mod fmt {
    use super::Timestamp;

    const MS_PER_MIN: u64 = 60_000;
    const MS_PER_HOUR: u64 = 3_600_000;

    /// Full units for screen readers: "12 hours 5 minutes".
    pub fn duration_spoken(ms: u64) -> String {
        let h = ms / MS_PER_HOUR;
        let m = (ms % MS_PER_HOUR) / MS_PER_MIN;
        let hours = crate::i18n::plural::counted("fmt.hours_spoken", h);
        let minutes = crate::i18n::plural::counted("fmt.minutes_spoken", m);
        match (h, m) {
            (0, 0) => crate::i18n::text("fmt.under_a_minute").to_owned(),
            (0, _) => minutes,
            (_, 0) => hours,
            _ if h >= 100 => hours,
            _ => rust_i18n::t!(
                "fmt.hours_and_minutes_spoken",
                hours = hours,
                minutes = minutes
            )
            .into_owned(),
        }
    }

    /// A game's or a period's time played (the design's `fmtTime`): "0 min", "45 min",
    /// "3 h 30 min", "3 h", then whole hours from ten: "21 h", "1,240 h".
    pub fn play_time(ms: u64) -> String {
        let min = ms / MS_PER_MIN;
        if min < 60 {
            return rust_i18n::t!("fmt.minutes_short", n = min).into_owned();
        }
        let (h, m) = (min / 60, min % 60);
        if h >= 10 || m == 0 {
            rust_i18n::t!("fmt.hours_short", n = thousands(h)).into_owned()
        } else {
            rust_i18n::t!("fmt.hours_minutes_short", h = h, m = m).into_owned()
        }
    }

    /// When a game was last played, by whole local days (the design's `fmtLast`): "Never",
    /// "Today", "Yesterday", "3 days ago", "Last week", "2 weeks ago", "Last month",
    /// "5 months ago", "Over a year ago". `offset_s` is the local UTC offset.
    pub fn last_played(last: Option<Timestamp>, now: Timestamp, offset_s: i32) -> String {
        let Some(t) = last else {
            return crate::i18n::text("fmt.never").to_owned();
        };
        match (local_day(now, offset_s) - local_day(t, offset_s)).max(0) {
            0 => crate::i18n::text("date.today").to_owned(),
            1 => crate::i18n::text("date.yesterday").to_owned(),
            d @ 2..=6 => crate::i18n::plural::counted("fmt.days_ago", d as u64),
            7..=13 => crate::i18n::text("fmt.last_week").to_owned(),
            d @ 14..=30 => crate::i18n::plural::counted("fmt.weeks_ago", (d / 7) as u64),
            31..=60 => crate::i18n::text("fmt.last_month").to_owned(),
            d @ 61..=364 => crate::i18n::plural::counted("fmt.months_ago", (d / 30) as u64),
            _ => crate::i18n::text("fmt.over_a_year_ago").to_owned(),
        }
    }

    /// A session's elapsed time as minutes and seconds (the design's `clock`): "0:42",
    /// "75:03".
    pub fn session_clock(ms: u64) -> String {
        let s = ms / 1000;
        format!("{}:{:02}", s / 60, s % 60)
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

    /// [`relative_date`] inside a sentence: "today", "yesterday", "Tuesday", "3 Nov".
    pub fn relative_inline(t: Timestamp, now: Timestamp, offset_s: i32) -> String {
        let (day, today) = (local_day(t, offset_s), local_day(now, offset_s));
        match today - day {
            // Lower-casing is a no-op in scripts without case.
            0 => crate::i18n::text("date.today").to_lowercase(),
            1 => crate::i18n::text("date.yesterday").to_lowercase(),
            _ => relative_date(t, now, offset_s),
        }
    }

    /// "Today", "Yesterday", a weekday within six days, then "3 Nov" (or "3 Nov 2025" in
    /// another year). `offset_s` is the local UTC offset.
    pub fn relative_date(t: Timestamp, now: Timestamp, offset_s: i32) -> String {
        let (day, today) = (local_day(t, offset_s), local_day(now, offset_s));
        match today - day {
            0 => super::date::today().to_owned(),
            1 => super::date::yesterday().to_owned(),
            2..=6 => super::date::weekday_full((day + 3).rem_euclid(7) as usize).to_owned(),
            _ => {
                let (y, m, d) = civil(day);
                let (ty, _, _) = civil(today);
                let month = super::date::month_abbr((m - 1) as usize);
                if y == ty {
                    super::date::day_month(d, month)
                } else {
                    super::date::day_month_year(d, month, y)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_durations_and_thousands() {
        assert_eq!(fmt::duration_spoken(30_000), "under a minute");
        assert_eq!(
            fmt::duration_spoken(12 * 3_600_000 + 5 * 60_000),
            "12 hours 5 minutes"
        );
        assert_eq!(fmt::duration_spoken(60_000), "1 minute");
        assert_eq!(fmt::thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn play_time_last_played_and_the_clock_read_as_the_design_writes_them() {
        let m = 60_000;
        for (ms, said) in [
            (0, "0 min"),
            (59_000, "0 min"),
            (45 * m, "45 min"),
            (64 * m, "1 h 4 min"),
            (210 * m, "3 h 30 min"),
            (180 * m, "3 h"),
            (588 * m, "9 h 48 min"),
            (21 * 60 * m + 7 * m, "21 h"),
            (1240 * 60 * m, "1,240 h"),
        ] {
            assert_eq!(fmt::play_time(ms), said);
        }
        let now = Timestamp::from_millis(1_794_441_600_000 + 15 * 3_600_000);
        let ago = |n: i64| Some(Timestamp::from_millis(now.as_millis() - n * 86_400_000));
        for (d, said) in [
            (0, "Today"),
            (1, "Yesterday"),
            (3, "3 days ago"),
            (7, "Last week"),
            (14, "2 weeks ago"),
            (31, "Last month"),
            (150, "5 months ago"),
            (400, "Over a year ago"),
        ] {
            assert_eq!(fmt::last_played(ago(d), now, 0), said);
        }
        assert_eq!(fmt::last_played(None, now, 0), "Never");
        assert_eq!(fmt::session_clock(42_000), "0:42");
        assert_eq!(fmt::session_clock(3_730_000), "62:10");
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
    fn a_running_game_says_its_session_clock_and_sorting_is_said() {
        let now = Timestamp::from_millis(1_794_441_600_000 + 15 * 3_600_000);
        let since = Timestamp::from_millis(now.as_millis() - 42 * 60_000 - 10_000);
        assert_eq!(
            card::state(Activity::Running { since }, Availability::Present, now).as_deref(),
            Some("Running · 42:10")
        );
        assert_eq!(
            card::state(Activity::Idle, Availability::Missing, now).as_deref(),
            Some("Not found")
        );
        assert_eq!(
            card::state(Activity::Idle, Availability::Present, now),
            None
        );
        assert_eq!(library::sort_button("Name", false), "Sort: Name, ascending");
        assert_eq!(
            library::arranged_by("Name", false, Some("Tag")),
            "Sorted by Name, ascending, grouped by Tag"
        );
        assert_eq!(
            library::arranged_by("Playtime", true, None),
            "Sorted by Playtime, descending"
        );
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

    /// UI-029: screens contain no sentence literals (two or more words in quotes) and no
    /// `format!` concatenation of user-facing text; everything comes from this module.
    #[test]
    fn screens_have_no_sentence_literals() {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/screens");
        let mut files = Vec::new();
        walk(&root, &mut files);
        assert!(files.len() > 10);
        let mut bad = Vec::new();
        for f in files {
            if f.ends_with("gallery.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&f).unwrap_or_default();
            let body = &text[..text.find("#[cfg(test)]").unwrap_or(text.len())];
            for (n, line) in body.lines().enumerate() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                for (i, lit) in code.split('"').enumerate() {
                    let sentence = i % 2 == 1
                        && lit
                            .split(' ')
                            .filter(|w| w.chars().any(char::is_alphabetic))
                            .count()
                            >= 2;
                    if sentence {
                        bad.push(format!("{}:{}: {lit}", f.display(), n + 1));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "sentence literals in screens: {bad:#?}");
    }

    /// SECURITY_MODEL §10 / USER_EXPERIENCE §9: no security theatre in any string.
    #[test]
    fn no_forbidden_claims() {
        // The English catalogue holds every user-visible string (ADR-0032).
        let files = [include_str!("../locales/en.yml")];
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
