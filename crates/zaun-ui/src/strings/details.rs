//! The game page (SCREEN_SPECS §4, SCREEN_SPECS_2 §8–11).

use std::path::Path;

use zaun_core::clock::Timestamp;
use zaun_core::model::{Availability, BasePreset, Permissions, SandboxPolicy};

use crate::vm::details::DetailsTab;

texts! {
    /// Sample tool label in preview data.
    SAMPLE_TOOL_LABEL = "details.sample_tool_label";
    /// Sample note in preview data.
    SAMPLE_NOTE = "details.sample_note";
    /// Tab list name.
    TABS = "details.tabs";
}

/// Tab names.
pub fn tab(t: DetailsTab) -> &'static str {
    match t {
        DetailsTab::Overview => crate::i18n::text("details.tab.overview"),
        DetailsTab::Protection => crate::i18n::text("details.tab.protection"),
        DetailsTab::Compatibility => crate::i18n::text("details.tab.compatibility"),
        DetailsTab::LaunchOptions => crate::i18n::text("details.tab.launch_options"),
        DetailsTab::Activity => crate::i18n::text("details.tab.activity"),
        DetailsTab::Logs => crate::i18n::text("details.tab.logs"),
    }
}

texts! {
    /// Rename field.
    RENAME_FIELD = "details.rename_field";
    /// Rename field description.
    RENAME_HINT = "details.rename_hint";
    /// Stopping notice.
    STOPPING_BODY = "details.stopping_body";
    /// Availability action.
    CHECK_AGAIN = "details.check_again";
    /// Tamper notice.
    TAMPER = "details.tamper";
    /// Tamper notice.
    TAMPER_BODY = "details.tamper_body";
}

/// Availability notice title and body.
pub fn availability(a: Availability) -> (&'static str, &'static str) {
    match a {
        Availability::DriveDisconnected => (
            crate::i18n::text("details.availability.the_drive_with_this"),
            crate::i18n::text("details.availability.connect_the_drive_and"),
        ),
        Availability::Unreadable => (
            crate::i18n::text("details.availability.this_game_can_t"),
            crate::i18n::text("details.availability.zaun_isn_t_allowed"),
        ),
        _ => (
            crate::i18n::text("details.availability.this_game_s_program"),
            crate::i18n::text("details.availability.it_may_have_been"),
        ),
    }
}

// ---- Overview ----------------------------------------------------------------------------

/// The program and its arguments as one command line: "LanternKeep.exe -screen-fullscreen 1".
pub fn command_line(program: &str, args: &str) -> String {
    if args.is_empty() {
        program.to_owned()
    } else {
        format!("{program} {args}")
    }
}

/// "Last played: Yesterday".
pub fn fact_name(label: &str, value: &str) -> String {
    rust_i18n::t!("details.fact_name", label = label, value = value).into_owned()
}

texts! {
    /// Section: what the game's files tell (ADR-0028).
    WHAT_IT_IS = "details.what_it_is";
    /// Fact.
    ENGINE = "details.engine";
    /// Fact: the main program.
    PROGRAM = "details.program";
    /// Fact: the company in the program's version resource.
    MADE_BY = "details.made_by";
    /// Fact.
    ANTI_CHEAT = "details.anti_cheat";
    /// After the anti-cheat systems found.
    ANTI_CHEAT_RISK = "details.anti_cheat_risk";
    /// Fact: Steam and other stores' services.
    STORE_SERVICES = "details.store_services";
    /// A finding.
    USES_STEAM_API = "details.uses_steam_api";
    /// A finding.
    NEEDS_STEAM = "details.needs_steam";
    /// A finding.
    STORE_SDK = "details.store_sdk";
    /// Fact: Proton's per-game fixes.
    GAME_FIXES = "details.game_fixes";
    /// When the layer applies none.
    NO_FIXES_HERE = "details.no_fixes_here";
}

/// The game Proton's fixes are for (a title, or a umu-database ID).
pub fn fixes_for(known: &str) -> String {
    rust_i18n::t!("details.fixes_for", known = known).into_owned()
}

/// Findings joined on one line.
pub fn joined(parts: &[String]) -> String {
    parts.join(" · ")
}

/// Who made the game, and the product's name when it is not the game's.
pub fn made_by(company: &str, product: Option<&str>) -> String {
    match product {
        Some(p) => format!("{company} · {p}"),
        None => company.to_owned(),
    }
}

/// The anti-cheat systems found, and what they mean here.
pub fn anti_cheat(found: &[zaun_core::identity::AntiCheat]) -> String {
    use zaun_core::identity::AntiCheat as A;
    let names: Vec<&str> = found
        .iter()
        .map(|a| match a {
            A::EasyAntiCheat => "Easy Anti-Cheat",
            A::BattlEye => "BattlEye",
        })
        .collect();
    rust_i18n::t!(
        "details.anti_cheat_names",
        names = names.join(crate::i18n::text("list.separator")),
        risk = ANTI_CHEAT_RISK()
    )
    .into_owned()
}

/// Proton's fixes for the game, under a layer that applies none.
pub fn fixes_not_applied(known: &str) -> String {
    rust_i18n::t!(
        "details.fixes_not_applied",
        known = known,
        hint = NO_FIXES_HERE()
    )
    .into_owned()
}

/// What an engine is called, with its variant.
pub fn engine_name(g: zaun_core::identity::EngineGuess) -> String {
    use zaun_core::identity::{Engine as E, Variant as V};
    let name = match g.engine {
        E::Unity => "Unity",
        E::Unreal => "Unreal Engine",
        E::Godot => "Godot",
        E::GameMaker => "GameMaker",
        E::RenPy => "Ren'Py",
        E::RpgMaker => "RPG Maker",
        E::Source => "Source",
        E::Source2 => "Source 2",
        E::CryEngine => "CryEngine",
        E::Fna => "FNA",
        E::MonoGame => "MonoGame",
        E::Xna => "XNA",
        E::Love => "LÖVE",
        E::Cocos => "Cocos2d-x",
        E::Electron => "Electron",
        E::NwJs => "NW.js",
    };
    match g.variant {
        None => name.to_owned(),
        Some(V::Il2Cpp) => format!("{name} (IL2CPP)"),
        Some(V::Mono) => format!("{name} (Mono)"),
        Some(V::Xp) => format!("{name} XP"),
        Some(V::Vx) => format!("{name} VX"),
        Some(V::VxAce) => format!("{name} VX Ace"),
        Some(V::Mv) => format!("{name} MV"),
        Some(V::Mz) => format!("{name} MZ"),
    }
}

/// What kind of program the main program is, and its version.
pub fn program_kind(pe: &zaun_core::model::PeInfo) -> Option<String> {
    match (machine_kind(pe), pe.file_version.as_deref()) {
        (Some(k), Some(v)) => {
            Some(rust_i18n::t!("details.program_kind_version", kind = k, v = v).into_owned())
        }
        (Some(k), None) => Some(k.to_owned()),
        (None, Some(v)) => Some(rust_i18n::t!("details.program_version", v = v).into_owned()),
        (None, None) => None,
    }
}

/// What kind of program a PE file is, when its machine says.
fn machine_kind(pe: &zaun_core::model::PeInfo) -> Option<&'static str> {
    use zaun_core::model::PeMachine as M;
    if pe.is_dotnet {
        return Some(crate::i18n::text("details.machine.dotnet"));
    }
    match pe.machine {
        M::X86_64 => Some(crate::i18n::text("details.machine.x64")),
        M::X86 => Some(crate::i18n::text("details.machine.x86")),
        M::Arm64 => Some(crate::i18n::text("details.machine.arm64")),
        _ => None,
    }
}

texts! {
    /// Section: protection, compatibility, launch options.
    RUNS = "details.runs";
    /// Section: the folders.
    LIVES = "details.lives";
    /// Row.
    GAME_FOLDER = "details.game_folder";
    /// Row, for a game that is a single file.
    PROGRAM_FILE = "details.program_file";
    /// Row.
    SAVE_DATA = "details.save_data";
    /// Row.
    OTHER_PROGRAMS = "details.other_programs";
    /// Value.
    PRIVATE_STORAGE = "details.private_storage";
    /// Value.
    YOUR_FOLDER = "details.your_folder";
    /// Section.
    NOTES = "details.notes";
    /// Query failure.
    COULDNT_LOAD = "details.couldnt_load";
    /// Button.
    RETRY = "details.retry";
}

/// "31 sessions".
pub fn sessions(n: u32) -> String {
    crate::i18n::plural::counted("details.sessions", u64::from(n))
}

/// "Protection: Offline", "Game folder: Hollow Knight, in Games".
pub fn row_name(title: &str, value: &str, detail: Option<&str>) -> String {
    match detail {
        Some(d) => rust_i18n::t!(
            "details.row_name_detail",
            title = title,
            value = value,
            detail = d
        )
        .into_owned(),
        None => rust_i18n::t!("details.row_name", title = title, value = value).into_owned(),
    }
}

/// "2 more…".
pub fn more_programs(n: usize) -> String {
    rust_i18n::t!("details.more_programs", n = n).into_owned()
}

/// "1 variable".
pub fn variables(n: usize) -> String {
    crate::i18n::plural::counted("details.variables", n as u64)
}

/// ("Hollow Knight", Some("in Games")) for a game folder; the program's file otherwise.
pub fn location(root: Option<&Path>, exe: &Path) -> (String, Option<String>) {
    let p = root.unwrap_or(exe);
    let name = p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let parent = p
        .parent()
        .and_then(|x| x.file_name())
        .map(|n| rust_i18n::t!("details.in_folder", name = n.to_string_lossy()).into_owned());
    (name, parent)
}

// ---- Save bar ----------------------------------------------------------------------------

texts! {
    /// Save bar.
    UNSAVED = "details.unsaved";
    /// Save bar when access widens.
    UNSAVED_WIDENS = "details.unsaved_widens";
    /// Button.
    SAVE = "details.save";
    /// Button.
    DISCARD = "details.discard";
    /// Disabled reason.
    FIX_LAUNCH = "details.fix_launch";
}

/// "Fix 1 problem to save".
pub fn fix_problems(n: usize) -> String {
    crate::i18n::plural::counted("details.fix_problems", n as u64)
}

// ---- Protection ----------------------------------------------------------------------------

texts! {
    /// Overview row.
    PROTECTION = "details.protection";
    /// Heading and radio group of the Protection tab (the tab itself is called Protection).
    PROTECTION_LEVEL = "details.protection_level";
    /// Link.
    RESET_TO_PRESET = "details.reset_to_preset";
    /// Inline confirm button.
    REPLACE = "details.replace";
    /// Banner while running.
    APPLIES_NEXT_TIME = "details.applies_next_time";
    /// Policy of a newer version.
    POLICY_TOO_NEW = "details.policy_too_new";
    /// On a row while it differs from what is saved.
    CHANGED_MARK = "details.changed_mark";
    /// Permission.
    INTERNET_ON = "details.internet_on";
    /// List of permissions, and a section of the setup sheet.
    FILES = "details.files";
    /// Degree of access.
    CAN_READ = "details.can_read";
    /// Degree of access.
    CAN_READ_CHANGE = "details.can_read_change";
    /// Degree of access to the game folder.
    CHANGES_KEPT_APART = "details.changes_kept_apart";
    /// Access (setup sheet).
    CAN_CHANGE = "details.can_change";
    /// Row.
    ADD_FOLDER = "details.add_folder";
    /// Drop hint.
    DROP_FOLDER = "details.drop_folder";
    /// Permission.
    REMOVABLE = "details.removable";
    /// Note.
    WRITABLE_WITH_INTERNET = "details.writable_with_internet";
    /// Option.
    CHOOSE_FOLDER = "details.choose_folder";
    /// List of permissions.
    INTERNET_AND_DEVICES = "details.internet_and_devices";
    /// Section of the setup sheet.
    DEVICES = "details.devices";
    /// Permission.
    SOUND = "details.sound";
    /// Permission.
    MICROPHONE = "details.microphone";
    /// Under the microphone permission while it is off, on a computer that cannot keep it out
    /// of the sound server (ADR-0026).
    MICROPHONE_INSEPARABLE = "details.microphone_inseparable";
    /// Toggle (setup sheet).
    CONTROLLERS = "details.controllers";
    /// Permission.
    GAME_CONTROLLERS = "details.game_controllers";
    /// Degree of the controllers permission.
    CONNECTED_ONLY = "details.connected_only";
    /// Degree of the controllers permission.
    ALL_INPUT = "details.all_input";
    /// Disabled reason.
    ALL_INPUT_UNAVAILABLE = "details.all_input_unavailable";
    /// Permission.
    CAMERA = "details.camera";
    /// Row: where the game draws.
    SCREEN = "details.screen";
    /// X11 notice.
    X11_NOTICE = "details.x11_notice";
    /// Setup sheet.
    GAME_FOLDER_ACCESS = "details.game_folder_access";
    /// Option.
    READ_ONLY = "details.read_only";
    /// Option.
    PROTECTED = "details.protected";
    /// Loose file.
    FILE_ONLY_EXPLAINED = "details.file_only_explained";
    /// Advanced.
    IDENTITY = "details.identity";
    /// Toggle.
    MASK_HOSTNAME = "details.mask_hostname";
    /// What breaks.
    MASK_HOSTNAME_BREAKS = "details.mask_hostname_breaks";
    /// Toggle.
    MASK_MACHINE_ID = "details.mask_machine_id";
    /// What breaks.
    MASK_MACHINE_ID_BREAKS = "details.mask_machine_id_breaks";
    /// Toggle.
    MASK_HARDWARE = "details.mask_hardware";
    /// What breaks.
    MASK_HARDWARE_BREAKS = "details.mask_hardware_breaks";
    /// Toggle.
    SECCOMP = "details.seccomp";
    /// Hint.
    SECCOMP_HINT = "details.seccomp_hint";
    /// Advanced.
    LAYER_ISOLATION = "details.layer_isolation";
    /// Option.
    ISO_AUTO = "details.iso_auto";
    /// Option.
    ISO_PRIVATE = "details.iso_private";
    /// Option.
    ISO_SHARED = "details.iso_shared";
    /// Toggle.
    DIRECT_SOUND = "details.direct_sound";
    /// Hint.
    DIRECT_SOUND_HINT = "details.direct_sound_hint";
}

/// "Custom (based on Online)".
pub fn custom_based_on(b: BasePreset) -> String {
    rust_i18n::t!(
        "details.custom_based_on",
        preset = super::shield::preset_name(b.into())
    )
    .into_owned()
}

/// "Choosing Online replaces your changes."
pub fn replace_custom(b: BasePreset) -> String {
    rust_i18n::t!(
        "details.replace_custom",
        preset = super::shield::preset_name(b.into())
    )
    .into_owned()
}

/// "Share Music".
pub fn share_toggle(name: &str) -> String {
    rust_i18n::t!("details.share_toggle", name = name).into_owned()
}

/// "This folder can't be shared: …".
pub fn rejected(reason: &str) -> String {
    rust_i18n::t!("details.rejected", reason = reason).into_owned()
}

/// "in Home", "in Documents ▸ My Games".
pub fn folder_location(path: &Path, home: &Path) -> String {
    let parent = path.parent().unwrap_or(path);
    if parent == home {
        return crate::i18n::text("details.in_home").to_owned();
    }
    let name = match parent.strip_prefix(home) {
        Ok(rel) => rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(crate::i18n::text("setup.breadcrumb_separator")),
        Err(_) => parent.display().to_string(),
    };
    rust_i18n::t!("details.in_folder", name = name).into_owned()
}

/// A permission and its degree: "Music: Can read".
pub fn permission(title: &str, value: &str) -> String {
    rust_i18n::t!("details.permission", title = title, value = value).into_owned()
}

/// "Hollow Knight in Games".
pub fn place(name: &str, parent: Option<&str>) -> String {
    match parent {
        Some(parent) => rust_i18n::t!("details.place", name = name, parent = parent).into_owned(),
        None => name.to_owned(),
    }
}

/// "Advanced · 1 changed".
pub fn advanced(changed: usize) -> String {
    if changed == 0 {
        crate::i18n::text("details.advanced").to_owned()
    } else {
        rust_i18n::t!("details.advanced_changed", n = changed).into_owned()
    }
}

/// How many advanced fields differ: the switches under Advanced, including how far the home
/// folders shared in both and removable drives may go.
pub fn advanced_changes(a: &SandboxPolicy, b: &SandboxPolicy) -> usize {
    let advanced = Permissions::COMPUTER_NAME
        | Permissions::MACHINE_ID
        | Permissions::HARDWARE_IDS
        | Permissions::UNFILTERED_SYSCALLS
        | Permissions::DIRECT_SOUND;
    let mut flags = (a.permissions() ^ b.permissions()) & advanced;
    if a.permissions().contains(Permissions::REMOVABLE)
        && b.permissions().contains(Permissions::REMOVABLE)
    {
        flags |= (a.permissions() ^ b.permissions()) & Permissions::REMOVABLE_WRITE;
    }
    let folders = b
        .folders
        .iter()
        .filter(|f| matches!(f.origin, zaun_core::model::GrantOrigin::WellKnown(_)))
        .filter(|f| {
            a.folders
                .iter()
                .any(|x| x.canonical == f.canonical && x.access != f.access)
        })
        .count();
    flags.bits().count_ones() as usize
        + folders
        + usize::from(a.hardening.runtime_isolation != b.hardening.runtime_isolation)
}

// ---- Compatibility ------------------------------------------------------------------------

texts! {
    /// Section.
    LAYER = "details.layer";
    /// Checkbox.
    FOLLOW_UPDATES = "details.follow_updates";
    /// Missing layer.
    LAYER_MISSING = "details.layer_missing";
    /// Older layer.
    LAYER_OLDER = "details.layer_older";
    /// Button.
    USE_DEFAULT = "details.use_default";
    /// Empty catalogue.
    NONE_INSTALLED = "details.none_installed";
    /// Button.
    DOWNLOAD_NOW = "details.download_now";
    /// Section.
    FIXES = "details.fixes";
    /// Layer without fixes.
    NO_FIXES_IN_LAYER = "details.no_fixes_in_layer";
    /// No fix.
    NO_FIX = "details.no_fix";
    /// Button.
    CHANGE = "details.change";
    /// Toggle.
    TURN_OFF_FIXES = "details.turn_off_fixes";
    /// Section.
    OPTIONS = "details.options";
    /// Row.
    GRAPHICS = "details.graphics";
    /// Row.
    WINDOW_SYSTEM = "details.window_system";
    /// Option.
    AUTOMATIC = "details.automatic";
    /// Row.
    OVERLAY = "details.overlay";
    /// Row.
    LANGUAGE = "details.language";
    /// Option.
    LANG_SYSTEM = "details.lang_system";
    /// Row.
    SYNC = "details.sync";
    /// Option.
    NO_FSYNC = "details.no_fsync";
    /// Option.
    NO_ESYNC = "details.no_esync";
    /// Field.
    DLL_OVERRIDES = "details.dll_overrides";
    /// Field.
    DXVK_HUD = "details.dxvk_hud";
    /// Hint.
    PROTON_LOG_HINT = "details.proton_log_hint";
    /// Card.
    STORAGE = "details.storage";
    /// Size placeholder.
    CALCULATING = "details.calculating";
    /// The size could not be measured.
    SIZE_UNKNOWN = "details.size_unknown";
    /// Button.
    OPEN_FOLDER = "details.open_folder";
    /// Button.
    WINDOWS_SETTINGS = "details.windows_settings";
    /// Button.
    RESET = "details.reset";
    /// Sheet.
    FIX_PICKER = "details.fix_picker";
    /// Field.
    FIX_SEARCH = "details.fix_search";
    /// Empty.
    NO_FIX_FOUND = "details.no_fix_found";
    /// Button.
    NO_FIX_USE = "details.no_fix_use";
}

/// "Default — UMU-Proton 10.0-4".
pub fn default_layer(resolved: Option<&str>) -> String {
    match resolved {
        Some(r) => rust_i18n::t!("details.default_layer_named", name = r).into_owned(),
        None => NONE_INSTALLED().to_owned(),
    }
}

/// "UMU-Proton (latest)".
pub fn channel(name: &str) -> String {
    rust_i18n::t!("details.channel", name = name).into_owned()
}

/// "Hollow Knight (GOG) · umu-367520".
pub fn fix_row(title: &str, store: &str, id: &str) -> String {
    rust_i18n::t!("details.fix_row", title = title, store = store, id = id).into_owned()
}

// ---- Launch options ----------------------------------------------------------------------

texts! {
    /// Card.
    PROGRAMS_TITLE = "details.programs_title";
    /// Role.
    ROLE_PRIMARY = "details.role_primary";
    /// Role.
    ROLE_ALTERNATE = "details.role_alternate";
    /// Role.
    ROLE_LAUNCHER = "details.role_launcher";
    /// Role.
    ROLE_TOOL = "details.role_tool";
    /// Role.
    ROLE_INSTALLER = "details.role_installer";
    /// Menu.
    MAKE_MAIN = "details.make_main";
    /// Menu.
    REMOVE_PROGRAM = "details.remove_program";
    /// Button.
    ADD_PROGRAM = "details.add_program";
    /// Field.
    ARGUMENTS = "details.arguments";
    /// Help.
    ARGUMENTS_HELP = "details.arguments_help";
    /// Error.
    ARGS_UNBALANCED = "details.args_unbalanced";
    /// Section.
    START_IN = "details.start_in";
    /// Option.
    WD_PROGRAM = "details.wd_program";
    /// Option.
    WD_GAME = "details.wd_game";
    /// Option.
    WD_OTHER = "details.wd_other";
    /// Section.
    WHEN_STOPPING = "details.when_stopping";
    /// Section.
    ENV_VARS = "details.env_vars";
    /// Rule.
    DEV_ONLY_VAR = "details.dev_only_var";
    /// Rule.
    INVALID_VAR = "details.invalid_var";
}

/// "Actions for Config.exe".
pub fn program_menu(file: &str) -> String {
    rust_i18n::t!("details.program_menu", file = file).into_owned()
}

/// "Run Config.exe".
pub fn run_program(file: &str) -> String {
    rust_i18n::t!("details.run_program", file = file).into_owned()
}

/// "Mark as Tool".
pub fn mark_as(role: &str) -> String {
    rust_i18n::t!("details.mark_as", role = role).into_owned()
}

/// "3 arguments".
pub fn args_preview(n: usize) -> String {
    crate::i18n::plural::counted("details.argument_count", n as u64)
}

/// "10 s".
pub fn seconds(n: u8) -> String {
    rust_i18n::t!("details.seconds", n = n).into_owned()
}

/// "PATH is managed by Zaun".
pub fn reserved_var(name: &str) -> String {
    rust_i18n::t!("details.reserved_var", name = name).into_owned()
}

texts! {
    /// Placeholder of a variable's name field.
    FIND_PROTON_OPTION = "details.find_proton_option";
    /// Accessible name of the list of suggested options.
    PROTON_OPTIONS = "details.proton_options";
}

/// Under an option the game's compatibility layer does not read.
pub fn not_read_by(summary: &str, layer: &str) -> String {
    rust_i18n::t!("details.not_read_by", summary = summary, layer = layer).into_owned()
}

/// "Variable name, row 1".
pub fn var_name(row: usize) -> String {
    rust_i18n::t!("details.var_name", row = row).into_owned()
}

/// "Value, row 1".
pub fn var_value(row: usize) -> String {
    rust_i18n::t!("details.var_value", row = row).into_owned()
}

/// "Remove variable 1".
pub fn remove_var(row: usize) -> String {
    rust_i18n::t!("details.remove_var", row = row).into_owned()
}

// ---- Activity ----------------------------------------------------------------------------

texts! {
    /// Heading.
    HISTORY = "details.history";
    /// Button.
    LOAD_MORE = "details.load_more";
    /// Menu.
    REMOVE_SESSION = "details.remove_session";
    /// Link.
    RESET_STATS = "details.reset_stats";
    /// Folder row state when the folder is gone (SCREEN_SPECS_2 §9).
    FOLDER_NOT_FOUND = "details.folder_not_found";
    /// Button on a missing folder row.
    LOCATE_FOLDER = "details.locate_folder";
    /// Program menu item.
    RENAME_LABEL = "details.rename_label";
    /// Program menu item.
    PROGRAM_ARGS = "details.program_args";
    /// Inline editor field.
    LABEL_FIELD = "details.label_field";
    /// Inline editor help.
    LABEL_HELP = "details.label_help";
    /// Inline editor field.
    PROGRAM_ARGS_FIELD = "details.program_args_field";
    /// Inline editor help.
    PROGRAM_ARGS_HELP = "details.program_args_help";
    /// Inline editor button.
    SAVE_PROGRAM = "details.save_program";
    /// Inline editor button.
    CANCEL_PROGRAM = "details.cancel_program";
}

/// "Own arguments: -dx12".
pub fn own_args(args: &str) -> String {
    rust_i18n::t!("details.own_args", args = args).into_owned()
}

// ---- The game page as the redesign has it (zaun-redesign.html §9) -------------------

texts! {
    /// The back link above the title.
    BACK_LINK = "details.back_link";
    /// Tooltip of the back link.
    BACK_TIP = "details.back_tip";
    /// The cover's button and tooltip.
    CHANGE_ARTWORK = "details.change_artwork";
    /// Tooltip of the play record.
    OPEN_ACTIVITY = "details.open_activity";
    /// The play record of a game never played.
    NOT_PLAYED_YET = "details.not_played_yet";
    /// Under the session clock.
    PLAYING_NOW = "details.playing_now";
    /// The play record while the game starts.
    STARTING = "details.starting";
    /// Under "Starting…".
    GETTING_READY = "details.getting_ready";
    /// Under the record's bars, left.
    LAST_30_DAYS = "details.last_30_days";
    /// Tooltip of the protection level under the title.
    PROTECTION_TIP = "details.protection_tip";
    /// The protection level of a game whose permissions differ from its level's.
    CUSTOM = "details.custom";
    /// The word that adds a tag.
    TAG_WORD = "details.tag_word";
    /// Tooltip of a tag and of "+ Tag".
    TAGS_TIP = "details.tags_tip";
    /// Round button beside Play, and its tooltip while off.
    FAVOURITE = "details.favourite";
    /// Tooltip of the favourite button while on.
    UNFAVOURITE = "details.unfavourite";
    /// Round button beside Play.
    MORE = "details.more";
    /// Note over a game still running in the background.
    LINGERING_TITLE = "details.lingering_title";
    /// Note over a game still running in the background.
    LINGERING_BODY = "details.lingering_body";
    /// Note while a stop was asked for.
    STOPPING_TITLE = "details.stopping_title";
}

/// "played, most recently yesterday" (`last` as [`super::fmt::last_played`] writes it).
pub fn record_line(last: &str) -> String {
    // Lower-casing is a no-op in scripts without case.
    rust_i18n::t!("details.record_line", last = last.to_lowercase()).into_owned()
}

/// The record as one sentence, in a narrow window: "Played 12 h, most recently yesterday.".
pub fn played_sentence(time: &str, last: Option<&str>) -> String {
    match last {
        // Lower-casing is a no-op in scripts without case.
        Some(last) => rust_i18n::t!(
            "details.played_sentence_last",
            time = time,
            last = last.to_lowercase()
        )
        .into_owned(),
        None => rust_i18n::t!("details.played_sentence", time = time).into_owned(),
    }
}

/// The record as one sentence while the game is played: "Playing now, 0:42.".
pub fn playing_sentence(clock: &str) -> String {
    rust_i18n::t!("details.playing_sentence", clock = clock).into_owned()
}

texts! {
    /// The record as one sentence of a game never played.
    NOT_PLAYED_SENTENCE = "details.not_played_sentence";
}

/// "Last 30 days: 3 h 41 min" (the record's bars, spoken).
pub fn days_name(total: &str) -> String {
    rust_i18n::t!("details.days_name", label = LAST_30_DAYS(), total = total).into_owned()
}

/// The record's accessible name: "12 h played, most recently yesterday. Open Activity".
pub fn record_name(time: &str, line: &str) -> String {
    rust_i18n::t!(
        "details.record_name",
        time = time,
        line = line,
        open = OPEN_ACTIVITY()
    )
    .into_owned()
}

/// The note of a game whose program is missing, unreachable or unreadable: its title as a
/// sentence.
pub fn availability_title(a: Availability) -> String {
    rust_i18n::t!("details.availability_title", title = availability(a).0).into_owned()
}

/// Days since the epoch of `t`'s local date.
fn local_days(t: Timestamp, offset_s: i32) -> i64 {
    use zaun_core::stats::rollup::{key_days, local_day};
    key_days(local_day(t, (offset_s / 60) as i16))
}

/// How many local days lie between `t` and `now`.
pub fn days_ago(t: Timestamp, now: Timestamp, offset_s: i32) -> i64 {
    local_days(now, offset_s) - local_days(t, offset_s)
}

/// A date: "4 Sept", or "4 Sept 2025" in another year.
pub fn date_label(t: Timestamp, now: Timestamp, offset_s: i32) -> String {
    use zaun_core::stats::rollup::civil_from_days;
    let (y, m, d) = civil_from_days(local_days(t, offset_s));
    let (ty, _, _) = civil_from_days(local_days(now, offset_s));
    let month = super::date::month_short((m.clamp(1, 12) - 1) as usize);
    if y == ty {
        super::date::day_month(d, month)
    } else {
        super::date::day_month_year(d, month, y)
    }
}

/// A day in a list: "Today", "Yesterday", a weekday within the week, then a date.
pub fn day_label(t: Timestamp, now: Timestamp, offset_s: i32) -> String {
    let day = local_days(t, offset_s);
    match days_ago(t, now, offset_s) {
        0 => super::date::today().to_owned(),
        1 => super::date::yesterday().to_owned(),
        2..=6 => super::date::weekday_full((day + 3).rem_euclid(7) as usize).to_owned(),
        _ => date_label(t, now, offset_s),
    }
}

// ---- Overview, as designed -----------------------------------------------------------------

texts! {
    /// The reach lists' labels.
    CAN_USE = "details.can_use";
    /// The reach lists' labels.
    CANT_USE = "details.cant_use";
    /// An empty "Can't use" list.
    NOTHING_HELD_BACK = "details.nothing_held_back";
    /// A reach item.
    REACH_INTERNET = "details.reach_internet";
    /// A reach item.
    REACH_OWN_FOLDER = "details.reach_own_folder";
    /// A reach item, of a game that is a single program.
    REACH_OWN_PROGRAM = "details.reach_own_program";
    /// A reach item.
    REACH_PERSONAL = "details.reach_personal";
    /// A reach item, when some personal folder is shared.
    REACH_OTHER_PERSONAL = "details.reach_other_personal";
    /// A reach item.
    REACH_CONTROLLERS = "details.reach_controllers";
    /// A reach item.
    REACH_SOUND = "details.reach_sound";
    /// A reach item.
    REACH_MICROPHONE = "details.reach_microphone";
    /// A reach item.
    REACH_CAMERA = "details.reach_camera";
}

/// A comma list with its first letter capitalised.
pub fn comma_list(items: &[String]) -> String {
    let joined = items.join(", ");
    let mut chars = joined.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => joined,
    }
}

texts! {
    /// Tooltip of a door that opens the file manager.
    OPEN_IN_FILE_MANAGER = "details.open_in_file_manager";
    /// Placeholder of the notes.
    NOTES_HINT = "details.notes_hint";
    /// After the notes heading once they are saved.
    NOTES_SAVED = "details.notes_saved";
    /// Fact.
    ADDED = "details.added";
    /// Value of the Save data door.
    SAVE_PRIVATE = "details.save_private";
}

/// ", 1 variable" after the launch options' program.
pub fn variables_after(n: usize) -> String {
    rust_i18n::t!("details.variables_after", variables = variables(n)).into_owned()
}

/// "64-bit Windows program, version 1.4.2.0".
pub fn program_fact(pe: &zaun_core::model::PeInfo) -> Option<String> {
    match (machine_kind(pe), pe.file_version.as_deref()) {
        (Some(k), Some(v)) => {
            Some(rust_i18n::t!("details.program_fact_version", kind = k, v = v).into_owned())
        }
        (Some(k), None) => Some(k.to_owned()),
        (None, Some(v)) => Some(rust_i18n::t!("details.program_version", v = v).into_owned()),
        (None, None) => None,
    }
}

// ---- Protection, as designed ---------------------------------------------------------------

/// "Custom, based on Offline. ".
pub fn custom_line(b: BasePreset) -> String {
    rust_i18n::t!(
        "details.custom_line",
        preset = super::shield::preset_name(b.into())
    )
    .into_owned()
}

/// "Reset to Offline".
pub fn reset_to(b: BasePreset) -> String {
    rust_i18n::t!(
        "details.reset_to",
        preset = super::shield::preset_name(b.into())
    )
    .into_owned()
}

texts! {
    /// Degree of the game folder.
    CAN_ONLY_READ = "details.can_only_read";
}

// ---- The Protection tab as a grouped list (ADR-0030) ---------------------------------------

texts! {
    /// The sentence at the top of the Protection tab.
    PROT_LEDE = "details.prot.lede";
    /// Group heading.
    OWN_FILES = "details.prot.own_files";
    /// Group heading.
    YOUR_FILES = "details.prot.your_files";
    /// The count of *Your files* when nothing is shared.
    NONE_SHARED = "details.prot.none_shared";
    /// What a row is for.
    INTERNET_LINE = "details.prot.internet_line";
    /// What a row is for.
    SOUND_LINE = "details.prot.sound_line";
    /// What a row is for.
    MICROPHONE_LINE = "details.prot.microphone_line";
    /// What a row is for.
    CONTROLLERS_LINE = "details.prot.controllers_line";
    /// What a row is for.
    CAMERA_LINE = "details.prot.camera_line";
    /// What a row is for.
    SAVES_LINE = "details.prot.saves_line";
    /// What a row is for.
    REMOVABLE_LINE = "details.prot.removable_line";
    /// How a choice reads in its row.
    CONNECTED_AT_START = "details.prot.connected_at_start";
    /// How a choice reads in its row.
    ALL_DEVICES = "details.prot.all_devices";
    /// How a choice reads in its row.
    READ_AND_CHANGE = "details.prot.read_and_change";
    /// How a choice reads in its row.
    READ_ONLY_SHORT = "details.prot.read_only";
    /// How a choice reads in its row.
    KEPT_APART = "details.prot.kept_apart";
    /// How a choice reads in its row.
    PRIVATE_SHORT = "details.prot.private";
    /// The select of a folder the user shared.
    SHARED = "details.prot.shared";
    /// Menu item of a shared folder.
    STOP_SHARING = "details.prot.stop_sharing";
    /// A shared folder that can't be found.
    FOLDER_MISSING = "details.prot.missing";
    /// A folder the game may change.
    CAN_CHANGE_HERE = "details.prot.can_change";
    /// Text button under the list.
    SHARE_ANOTHER = "details.prot.share_another";
    /// Advanced: the degree of the home folders and removable drives.
    CHANGING_FILES = "details.prot.changing_files";
    /// Advanced: help of [`CHANGING_FILES`].
    CHANGING_FILES_HELP = "details.prot.changing_files_help";
    /// A reach item.
    REACH_REMOVABLE = "details.prot.reach_removable";
}

/// "Offline" or "Online": the one shorthand of protection outside the Protection tab.
pub fn net_word(online: bool) -> &'static str {
    use zaun_core::model::Preset;
    super::shield::preset_name(if online {
        Preset::Online
    } else {
        Preset::Offline
    })
}

/// "2 of 5 allowed".
pub fn devices_allowed(n: usize, of: usize) -> String {
    crate::i18n::plural::counted_args(
        "details.prot.devices_allowed",
        n as u64,
        &[("m", &of.to_string())],
    )
}

/// "None shared", "1 folder shared".
pub fn folders_shared(n: usize) -> String {
    if n == 0 {
        NONE_SHARED().to_owned()
    } else {
        crate::i18n::plural::counted("details.prot.folders_shared", n as u64)
    }
}

/// "Pictures folder": a folder is never a bare word.
pub fn folder_named(name: &str) -> String {
    rust_i18n::t!("details.prot.folder_named", name = name).into_owned()
}

/// "Mods folder options".
pub fn shared_options(name: &str) -> String {
    rust_i18n::t!("details.prot.shared_options", name = name).into_owned()
}

/// "Pictures folder: can change files".
pub fn can_change_name(name: &str) -> String {
    rust_i18n::t!("details.prot.can_change_name", name = name).into_owned()
}

/// The name of a home folder ("Pictures"), whatever the folder is called on disk.
pub fn user_dir(d: zaun_core::model::UserDir) -> &'static str {
    use zaun_core::model::UserDir as D;
    crate::i18n::text(match d {
        D::Downloads => "details.prot.downloads",
        D::Documents => "details.prot.documents",
        D::Pictures => "details.prot.pictures",
        D::Music => "details.prot.music",
        D::Videos => "details.prot.videos",
        D::Desktop => "details.prot.desktop",
    })
}

/// A reach item for the home folders allowed: "your Pictures folder", "your Pictures and
/// Music folders", from three "3 of your home folders".
pub fn reach_home(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [a] => rust_i18n::t!("details.prot.reach_home_one", name = a).into_owned(),
        [a, b] => rust_i18n::t!("details.prot.reach_home_two", a = a, b = b).into_owned(),
        many => crate::i18n::plural::counted("details.prot.reach_home_many", many.len() as u64),
    }
}

/// A reach item for a folder the user shared: "the Mods folder".
pub fn reach_shared(name: &str) -> String {
    rust_i18n::t!("details.prot.reach_shared", name = name).into_owned()
}

// ---- Compatibility, as designed --------------------------------------------------------------

/// "Default, UMU-Proton 10.0-4".
pub fn default_layer_value(resolved: Option<&str>) -> String {
    match resolved {
        Some(r) => rust_i18n::t!("details.default_layer_value_named", name = r).into_owned(),
        None => crate::i18n::text("details.default_layer_value").to_owned(),
    }
}

/// Help of the layer row: "Last used UMU-Proton 10.0-4. Storage is prepared with the same
/// version.".
pub fn layer_help(used: Option<&str>, prepared: Option<&str>, older: bool) -> String {
    let Some(used) = used else {
        return crate::i18n::text("details.layer_help.not_used").to_owned();
    };
    match prepared {
        Some(_) if older => {
            rust_i18n::t!("details.layer_help.newer_storage", used = used).into_owned()
        }
        Some(p)
            if used.replace(' ', "-").eq_ignore_ascii_case(p) || p.replace('-', " ") == used =>
        {
            rust_i18n::t!("details.layer_help.same_storage", used = used).into_owned()
        }
        Some(p) => {
            rust_i18n::t!("details.layer_help.storage_with", used = used, prepared = p).into_owned()
        }
        None => rust_i18n::t!("details.layer_help.last_used", used = used).into_owned(),
    }
}

texts! {
    /// Row.
    LAYER_ROW = "details.layer_row";
    /// Help of "Follow updates".
    FOLLOW_HELP = "details.follow_help";
    /// Help of the chosen fix.
    FIX_HELP = "details.fix_help";
    /// Help of the performance overlay without MangoHud.
    MANGOHUD_MISSING = "details.mangohud_missing";
    /// Row of Proton's own log.
    PROTON_LOG_ROW = "details.proton_log_row";
    /// Help of the private storage.
    STORAGE_HELP = "details.storage_help";
    /// The private storage before the game first starts.
    STORAGE_NOT_YET = "details.storage_not_yet";
    /// Button.
    OPEN = "details.open";
}

/// "2.3 GB, created 16 Oct".
pub fn storage_value(size: &str, created: Option<&str>) -> String {
    match created {
        Some(c) => {
            rust_i18n::t!("details.storage_value_created", size = size, created = c).into_owned()
        }
        None => size.to_owned(),
    }
}

// ---- Launch options, as designed ---------------------------------------------------------------

/// "Tool, shown as “Settings”".
pub fn shown_as(role: &str, label: Option<&str>) -> String {
    match label {
        Some(l) => rust_i18n::t!("details.shown_as", role = role, label = l).into_owned(),
        None => role.to_owned(),
    }
}

texts! {
    /// Button.
    RUN_ONCE = "details.run_once";
    /// Placeholder of the arguments.
    ARGS_NONE = "details.args_none";
    /// Heading.
    STARTING_AND_STOPPING = "details.starting_and_stopping";
    /// Help of the stop grace.
    STOP_WAIT_HELP = "details.stop_wait_help";
    /// Placeholder of a new variable's name.
    NAME_HINT = "details.name_hint";
    /// Placeholder of a new variable's value.
    VALUE_HINT = "details.value_hint";
    /// Accessible name of a new variable's name.
    NEW_VAR_NAME = "details.new_var_name";
    /// Accessible name of a new variable's value.
    NEW_VAR_VALUE = "details.new_var_value";
    /// Adds the variable.
    ADD = "details.add";
}

/// "Passed to the program when it starts. 2 arguments." / "… None set.".
pub fn args_note(n: usize) -> String {
    if n == 0 {
        rust_i18n::t!("details.args_note_none", help = ARGUMENTS_HELP()).into_owned()
    } else {
        rust_i18n::t!(
            "details.args_note",
            help = ARGUMENTS_HELP(),
            args = args_preview(n)
        )
        .into_owned()
    }
}

/// "1 set", after the heading of the environment variables.
pub fn vars_set(n: usize) -> String {
    rust_i18n::t!("details.vars_set", n = n).into_owned()
}

// ---- Activity, as designed -----------------------------------------------------------------

/// What the headline says of each range (7 days, 30 days, year, all time).
#[allow(non_snake_case)]
pub fn RANGE_SAYS() -> [&'static str; 4] {
    [
        crate::i18n::text("details.range_says.in_the_last_7"),
        crate::i18n::text("details.range_says.in_the_last_30"),
        crate::i18n::text("details.range_says.in_the_last_year"),
        crate::i18n::text("details.range_says.in_all"),
    ]
}
texts! {
    /// The sentence of a range without play.
    NO_SESSIONS_PERIOD = "details.no_sessions_period";
    /// Accessible name of the range control.
    PERIOD = "details.period";
    /// The Activity tab of a game never played.
    NEVER_BODY = "details.never_body";
}

/// "1 h 12 min in the last 7 days".
pub fn activity_headline(time: &str, range: usize) -> String {
    let says = RANGE_SAYS();
    rust_i18n::t!(
        "details.activity_headline",
        time = time,
        range = says.get(range).copied().unwrap_or(says[3])
    )
    .into_owned()
}

/// "1 session, 1 h 12 min on average. The longest ran 1 h 12 min. First played 4 Sept."
pub fn activity_sentence(
    count: u32,
    average: Option<&str>,
    longest: Option<&str>,
    first: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if count == 0 {
        parts.push(NO_SESSIONS_PERIOD().to_owned());
    } else {
        parts.push(match average {
            Some(a) => crate::i18n::plural::counted_args(
                "details.activity_count_average",
                u64::from(count),
                &[("average", a)],
            ),
            None => crate::i18n::plural::counted("details.activity_count", u64::from(count)),
        });
        if let Some(l) = longest {
            parts.push(rust_i18n::t!("details.activity_longest", time = l).into_owned());
        }
    }
    if let Some(f) = first {
        parts.push(rust_i18n::t!("details.activity_first", time = f).into_owned());
    }
    parts.join(" ")
}

texts! {
    /// History column.
    COL_DATE = "details.col_date";
    /// History column.
    COL_STARTED = "details.col_started";
    /// History column.
    COL_DURATION = "details.col_duration";
    /// History column.
    COL_ENDED = "details.col_ended";
    /// Ghost button over the chart.
    SHOW_TABLE = "details.show_table";
    /// Ghost button over the table.
    SHOW_CHART = "details.show_chart";
}

// ---- Logs, as designed ----------------------------------------------------------------------

/// The level control's options.
#[allow(non_snake_case)]
pub fn LOG_LEVELS() -> [&'static str; 4] {
    [
        crate::i18n::text("details.log_levels.errors"),
        crate::i18n::text("details.log_levels.warnings"),
        crate::i18n::text("details.log_levels.info"),
        crate::i18n::text("details.log_levels.debug"),
    ]
}
texts! {
    /// Accessible name of the level control.
    LOG_LEVEL = "details.log_level";
    /// Placeholder of the log filter.
    LOG_FILTER = "details.log_filter";
    /// Accessible name of the log filter.
    LOG_FILTER_NAME = "details.log_filter_name";
    /// Checkbox button.
    LOG_WRAP = "details.log_wrap";
    /// Ghost button: copy the visible lines.
    LOG_COPY = "details.log_copy";
    /// An empty log.
    LOG_NOTHING = "details.log_nothing";
    /// A filter that matches no line.
    LOG_NO_MATCH = "details.log_no_match";
    /// Accessible name of the log view.
    LOG_VIEW = "details.log_view";
    /// Before the link of the fine print.
    LOG_MORE_BEFORE = "details.log_more_before";
    /// The link of the fine print.
    LOG_MORE_LINK = "details.log_more_link";
    /// After the link of the fine print.
    LOG_MORE_AFTER = "details.log_more_after";
}

/// "3 of 3 lines shown, 0 errors, 0 warnings.".
pub fn log_counts(shown: usize, total: usize, errors: usize, warnings: usize) -> String {
    rust_i18n::t!(
        "details.log_counts",
        shown = shown,
        lines = crate::i18n::plural::counted("details.log_lines", total as u64),
        errors = crate::i18n::plural::counted("details.log_errors", errors as u64),
        warnings = crate::i18n::plural::counted("details.log_warnings", warnings as u64)
    )
    .into_owned()
}

#[cfg(test)]
mod redesign_tests {
    use super::*;

    #[test]
    fn sentences_read_as_designed() {
        assert_eq!(
            played_sentence("12 h", Some("Yesterday")),
            "Played 12 h, most recently yesterday."
        );
        assert_eq!(
            comma_list(&["its own folder".into(), "sound".into()]),
            "Its own folder, sound"
        );
        assert_eq!(
            activity_sentence(1, Some("1 h 12 min"), Some("1 h 12 min"), Some("4 Sept")),
            "1 session, 1 h 12 min on average. The longest ran 1 h 12 min. First played 4 Sept."
        );
        assert_eq!(activity_sentence(0, None, None, None), NO_SESSIONS_PERIOD());
        assert_eq!(
            args_note(2),
            "Passed to the program when it starts. 2 arguments."
        );
        assert_eq!(
            args_note(0),
            "Passed to the program when it starts. None set."
        );
        assert_eq!(
            log_counts(3, 3, 0, 0),
            "3 of 3 lines shown, 0 errors, 0 warnings."
        );
    }
}
