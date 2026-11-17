//! Validation tests (SANDBOX-003/004, TESTING_STRATEGY §4.3): every deny-list group with
//! equal, inside and ancestor candidates, every example sentence of SECURITY_MODEL §7.2,
//! normalisation, pseudo-filesystems, breadth warnings and cross-field rules.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use easyplay_core::model::{
    ControllerAccess, EnvVar, FolderAccess, GameDirAccess, NetworkAccess, PolicyException,
    RemovableAccess, SandboxPolicy, SaveDataLocation, SocketAccess, WindowSystem,
};
use easyplay_core::sandbox::validate::{
    DenyGroup, DenyList, IssueCode, IssueSeverity, PolicyIssue, Relation, UserVarRule,
    ValidationInputs, classify_user_var, validate_policy,
};
use easyplay_core::sandbox::{offline, online, plan};
use easyplay_testkit::fixtures::hosts::{self, SandboxWorld};
use easyplay_testkit::fixtures::policies::grant;

fn deny(w: &SandboxWorld) -> DenyList {
    let ctx = w.ctx();
    DenyList::new(
        ctx.home,
        ctx.uid,
        ctx.user_name,
        ctx.paths,
        Some(ctx.mounts),
    )
}

fn groups_of(d: &DenyList, p: &str) -> Vec<(DenyGroup, Relation)> {
    d.check(Path::new(p))
        .into_iter()
        .map(|h| (h.group, h.relation))
        .collect()
}

fn entries(d: &DenyList, g: DenyGroup) -> Vec<PathBuf> {
    d.entries()
        .iter()
        .filter(|(x, _)| *x == g)
        .map(|(_, p)| p.clone())
        .collect()
}

fn validate(w: &SandboxWorld, policy: &SandboxPolicy) -> Vec<PolicyIssue> {
    let ctx = w.ctx();
    validate_policy(policy, &ValidationInputs::from_context(&ctx, &w.host))
}

fn codes(issues: &[PolicyIssue]) -> Vec<IssueCode> {
    issues.iter().map(|i| i.code).collect()
}

fn with_grant(path: &str, access: FolderAccess) -> SandboxPolicy {
    let mut p = offline();
    p.folders.push(grant(path, access, None));
    p
}

/// Assert every entry of `g` is denied for `rel` (candidate built by `make`).
fn assert_group(g: DenyGroup, rel: Relation, make: impl Fn(&Path) -> PathBuf, denied: bool) {
    let w = SandboxWorld::worked_example();
    let d = deny(&w);
    let list = entries(&d, g);
    assert!(!list.is_empty());
    for e in list {
        let c = make(&e);
        let hit = d
            .check(&c)
            .iter()
            .any(|h| h.group == g && h.relation == rel);
        assert_eq!(
            hit,
            denied,
            "{g:?} {rel:?} candidate {} (entry {})",
            c.display(),
            e.display()
        );
    }
}

fn inside(e: &Path) -> PathBuf {
    e.join("child")
}
fn parent(e: &Path) -> PathBuf {
    e.parent().unwrap_or(e).to_path_buf()
}

// ---- one test per deny-list rule -----------------------------------------------------

#[test]
fn deny_top_level_equal_is_rejected() {
    assert_group(
        DenyGroup::TopLevel,
        Relation::Equal,
        Path::to_path_buf,
        true,
    );
}
#[test]
fn deny_top_level_inside_is_allowed() {
    assert_group(DenyGroup::TopLevel, Relation::Inside, inside, false);
}
#[test]
fn deny_system_equal_is_rejected() {
    assert_group(DenyGroup::System, Relation::Equal, Path::to_path_buf, true);
}
#[test]
fn deny_system_inside_is_rejected() {
    assert_group(DenyGroup::System, Relation::Inside, inside, true);
}
#[test]
fn deny_system_ancestor_is_not_a_system_hit() {
    let w = SandboxWorld::worked_example();
    let d = deny(&w);
    assert!(!groups_of(&d, "/run/user").contains(&(DenyGroup::System, Relation::Ancestor)));
}
#[test]
fn deny_credentials_equal_inside_ancestor_are_rejected() {
    assert_group(
        DenyGroup::Credentials,
        Relation::Equal,
        Path::to_path_buf,
        true,
    );
    assert_group(DenyGroup::Credentials, Relation::Inside, inside, true);
}
#[test]
fn deny_credentials_ancestor_is_rejected() {
    let w = SandboxWorld::worked_example();
    let d = deny(&w);
    for e in entries(&d, DenyGroup::Credentials) {
        let p = parent(&e);
        if p == Path::new("/home/sam") {
            continue; // equal-group TopLevel instead
        }
        assert!(
            groups_of(&d, p.to_str().unwrap())
                .contains(&(DenyGroup::Credentials, Relation::Ancestor)),
            "{}",
            p.display()
        );
    }
    assert!(
        groups_of(&d, "/home/sam/.local/share")
            .contains(&(DenyGroup::Credentials, Relation::Ancestor))
    );
}
#[test]
fn deny_browsers_equal_inside_ancestor_are_rejected() {
    assert_group(
        DenyGroup::Browsers,
        Relation::Equal,
        Path::to_path_buf,
        true,
    );
    assert_group(DenyGroup::Browsers, Relation::Inside, inside, true);
    let w = SandboxWorld::worked_example();
    assert!(
        groups_of(&deny(&w), "/home/sam/.var").contains(&(DenyGroup::Browsers, Relation::Ancestor))
    );
}
#[test]
fn deny_shell_equal_inside_ancestor_are_rejected() {
    assert_group(DenyGroup::Shell, Relation::Equal, Path::to_path_buf, true);
    assert_group(DenyGroup::Shell, Relation::Inside, inside, true);
    let w = SandboxWorld::worked_example();
    assert!(
        groups_of(&deny(&w), "/home/sam/.local").contains(&(DenyGroup::Shell, Relation::Ancestor))
    );
}
#[test]
fn deny_easyplay_equal_inside_ancestor_are_rejected() {
    assert_group(
        DenyGroup::EasyPlay,
        Relation::Equal,
        Path::to_path_buf,
        true,
    );
    assert_group(DenyGroup::EasyPlay, Relation::Inside, inside, true);
    let w = SandboxWorld::worked_example();
    assert!(
        groups_of(&deny(&w), "/home/sam/.local/state")
            .contains(&(DenyGroup::EasyPlay, Relation::Ancestor))
    );
    // The current game's own prefix is EasyPlay storage too: never grantable.
    assert!(deny(&w).is_denied(&w.game_paths.prefix));
}
#[test]
fn deny_containers_equal_inside_ancestor_are_rejected() {
    assert_group(
        DenyGroup::Containers,
        Relation::Equal,
        Path::to_path_buf,
        true,
    );
    assert_group(DenyGroup::Containers, Relation::Inside, inside, true);
    let w = SandboxWorld::worked_example();
    assert!(
        groups_of(&deny(&w), "/var/lib").contains(&(DenyGroup::Containers, Relation::Ancestor))
    );
}
#[test]
fn deny_mount_root_ancestor_of_home_is_top_level() {
    let mut w = SandboxWorld::worked_example();
    w.fs.mount("/home", 30, "ext4");
    w.mounts = easyplay_core::fsx::MountTable::parse(w.fs.mountinfo().as_bytes());
    assert!(groups_of(&deny(&w), "/home").contains(&(DenyGroup::TopLevel, Relation::Equal)));
}
#[test]
fn deny_relations_are_component_wise_not_string_prefix() {
    let w = SandboxWorld::worked_example();
    let d = deny(&w);
    assert!(!d.is_denied(Path::new("/home/sam/.sshkeys")));
    assert!(!d.is_denied(Path::new("/usrlocal")));
    assert!(d.is_denied(Path::new("/home/sam/.ssh/id_ed25519")));
}

// ---- SECURITY_MODEL §7.2 example sentences -------------------------------------------

#[test]
fn documents_is_allowed() {
    let w = SandboxWorld::worked_example();
    let issues = validate(
        &w,
        &with_grant("/home/sam/Documents", FolderAccess::ReadWrite),
    );
    assert!(!issues.iter().any(PolicyIssue::is_blocking), "{issues:#?}");
}
#[test]
fn dot_config_is_rejected_as_ancestor() {
    let w = SandboxWorld::worked_example();
    w.fs.dir("/home/sam/.config");
    let d = deny(&w);
    assert!(
        groups_of(&d, "/home/sam/.config")
            .iter()
            .any(|(_, r)| *r == Relation::Ancestor)
    );
    assert!(
        codes(&validate(
            &w,
            &with_grant("/home/sam/.config", FolderAccess::ReadOnly)
        ))
        .contains(&IssueCode::PathDenied)
    );
}
#[test]
fn home_and_slash_home_are_rejected() {
    let w = SandboxWorld::worked_example();
    for p in ["/home", "/home/sam"] {
        assert!(
            codes(&validate(&w, &with_grant(p, FolderAccess::ReadOnly)))
                .contains(&IssueCode::PathDenied),
            "{p}"
        );
    }
}
#[test]
fn drive_root_is_allowed_with_breadth_warning() {
    let w = SandboxWorld::worked_example();
    let issues = validate(
        &w,
        &with_grant("/run/media/sam/USB", FolderAccess::ReadWrite),
    );
    assert!(!issues.iter().any(PolicyIssue::is_blocking), "{issues:#?}");
    assert!(codes(&issues).contains(&IssueCode::MountRootGrant));
}
#[test]
fn run_media_user_only_through_removable_permission() {
    let w = SandboxWorld::worked_example();
    assert!(
        codes(&validate(
            &w,
            &with_grant("/run/media/sam", FolderAccess::ReadOnly)
        ))
        .contains(&IssueCode::PathDenied)
    );
    let mut p = offline();
    p.removable = RemovableAccess::ReadOnly;
    let plan = plan(&p, &w.ctx(), &w.host).unwrap();
    assert!(plan.mount_at(Path::new("/run/media/sam")).is_some());
}

// ---- normalisation -------------------------------------------------------------------

#[test]
fn symlink_grant_into_denied_location_is_rejected() {
    let w = SandboxWorld::worked_example();
    let issues = validate(&w, &with_grant("/home/sam/sshlink", FolderAccess::ReadOnly));
    let hit = issues
        .iter()
        .find(|i| i.code == IssueCode::PathDenied)
        .expect("denied");
    assert_eq!(hit.path.as_deref(), Some(Path::new("/home/sam/.ssh")));
}
#[test]
fn changed_target_is_reported_and_both_targets_checked() {
    let w = SandboxWorld::worked_example();
    let mut p = offline();
    let mut g = grant("/home/sam/sshlink", FolderAccess::ReadOnly, None);
    g.canonical = "/home/sam/Music".into();
    p.folders.push(g);
    let c = codes(&validate(&w, &p));
    assert!(c.contains(&IssueCode::PathTargetChanged) && c.contains(&IssueCode::PathDenied));
}
#[test]
fn dot_dot_components_are_rejected_in_stored_paths() {
    let w = SandboxWorld::worked_example();
    let mut p = offline();
    let mut g = grant("/home/sam/Music/../.ssh", FolderAccess::ReadOnly, None);
    g.canonical = "/home/sam/Music/../.ssh".into();
    p.folders.push(g);
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::PathInvalid));
}
#[test]
fn dot_dot_in_chosen_path_resolves_before_the_deny_check() {
    let w = SandboxWorld::worked_example();
    let mut p = offline();
    let mut g = grant("/home/sam/Music/../.ssh", FolderAccess::ReadOnly, None);
    g.canonical = "/home/sam/.ssh".into();
    p.folders.push(g);
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::PathDenied));
}
#[test]
fn missing_and_relative_paths_are_rejected() {
    let w = SandboxWorld::worked_example();
    assert!(
        codes(&validate(
            &w,
            &with_grant("/nope/missing", FolderAccess::ReadOnly)
        ))
        .contains(&IssueCode::PathMissing)
    );
    assert!(
        codes(&validate(
            &w,
            &with_grant("relative/dir", FolderAccess::ReadOnly)
        ))
        .contains(&IssueCode::PathNotAbsolute)
    );
}
#[test]
fn file_grant_is_rejected() {
    let w = SandboxWorld::worked_example();
    let c = codes(&validate(
        &w,
        &with_grant(
            "/mnt/games/Hollow Knight/hollow_knight.exe",
            FolderAccess::ReadOnly,
        ),
    ));
    assert!(c.contains(&IssueCode::PathMissing));
}
#[test]
fn pseudo_filesystems_are_rejected() {
    let w = SandboxWorld::worked_example();
    w.fs.dir("/proc/1").dir("/sys/kernel");
    assert!(
        codes(&validate(
            &w,
            &with_grant("/proc/1", FolderAccess::ReadOnly)
        ))
        .contains(&IssueCode::PathPseudoFs)
    );
}
#[test]
fn nul_in_path_is_rejected() {
    use std::os::unix::ffi::OsStrExt;
    let w = SandboxWorld::worked_example();
    let mut p = offline();
    let mut g = grant("/tmp", FolderAccess::ReadOnly, None);
    g.path = PathBuf::from(std::ffi::OsStr::from_bytes(b"/mnt/a\0b"));
    p.folders.push(g);
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::PathInvalid));
}

// ---- breadth warnings and cross-field rules -----------------------------------------

#[test]
fn broad_grant_threshold() {
    let w = SandboxWorld::worked_example();
    for i in 0..10_000 {
        w.fs.file(format!("/mnt/big/f{i}"), b"");
    }
    let at = validate(&w, &with_grant("/mnt/big", FolderAccess::ReadWrite));
    assert!(
        !codes(&at).contains(&IssueCode::BroadGrant),
        "exactly 10 000 is not broad"
    );
    w.fs.file("/mnt/big/one-more", b"");
    let over = validate(&w, &with_grant("/mnt/big", FolderAccess::ReadWrite));
    let issue = over
        .iter()
        .find(|i| i.code == IssueCode::BroadGrant)
        .expect("broad");
    assert_eq!(
        issue.ack_flag(),
        Some(easyplay_core::model::GameFlags::BROAD_GRANT_ACK)
    );
    let ro = validate(&w, &with_grant("/mnt/big", FolderAccess::ReadOnly));
    assert!(!codes(&ro).contains(&IssueCode::BroadGrant));
}
#[test]
fn game_folder_user_dir_and_containing_game_warn() {
    let mut w = SandboxWorld::worked_example();
    w.root = Some("/home/sam/Downloads".into());
    w.exe = "/home/sam/Downloads/game.exe".into();
    w.fs.file(&w.exe, b"MZ");
    w.other_roots = vec!["/home/sam/Downloads/Other".into()];
    let c = codes(&validate(&w, &offline()));
    assert!(c.contains(&IssueCode::GameDirUserDir) && c.contains(&IssueCode::GameDirContainsGame));
}
#[test]
fn protected_requires_overlay_and_warns_on_bad_lower_fs() {
    let w = SandboxWorld::new(hosts::debian_stable_old_bwrap());
    let mut p = offline();
    p.game_dir = GameDirAccess::Protected;
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::OverlayUnsupported));
    let mut w = SandboxWorld::worked_example();
    w.root = Some("/run/media/sam/USB/Games".into());
    w.exe = "/run/media/sam/USB/Games/g.exe".into();
    w.fs.file(&w.exe, b"MZ");
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::OverlayLowerFs));
}
#[test]
fn network_with_writable_grant_warns() {
    let w = SandboxWorld::worked_example();
    let mut p = online();
    p.folders
        .push(grant("/mnt/data/shared", FolderAccess::ReadWrite, None));
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::NetworkWithWritableGrant));
    let mut q = offline();
    q.folders
        .push(grant("/mnt/data/shared", FolderAccess::ReadWrite, None));
    assert!(!codes(&validate(&w, &q)).contains(&IssueCode::NetworkWithWritableGrant));
}
#[test]
fn all_input_unsafe_warns_and_downgrades() {
    let w = SandboxWorld::new(hosts::input_group_user());
    let mut p = offline();
    p.devices.controllers = ControllerAccess::AllInput;
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::AllInputUnsafe));
    let plan = plan(&p, &w.ctx(), &w.host).unwrap();
    assert!(plan.mount_at(Path::new("/dev/input")).is_none());
    assert!(plan.mount_at(Path::new("/dev/input/event21")).is_some());
    // Keyboards are readable on this host but never bound.
    assert!(plan.mount_at(Path::new("/dev/input/event3")).is_none());
}
#[test]
fn x11_session_warns_with_ack_flag() {
    let w = SandboxWorld::new(hosts::x11_only());
    let i = validate(&w, &offline());
    let x = i
        .iter()
        .find(|i| i.code == IssueCode::X11Session)
        .expect("x11");
    assert_eq!(
        x.ack_flag(),
        Some(easyplay_core::model::GameFlags::X11_NOTICE_ACK)
    );
}
#[test]
fn reserved_values_and_schema_are_rejected() {
    let w = SandboxWorld::worked_example();
    let mut p = offline();
    p.display.isolation = easyplay_core::model::DisplayIsolation::Gamescope;
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::ReservedValue));
    let mut q = offline();
    q.schema_version = 99;
    assert_eq!(codes(&validate(&w, &q)), vec![IssueCode::SchemaTooNew]);
}
#[test]
fn both_displays_denied_and_window_system_rules() {
    let mut w = SandboxWorld::worked_example();
    let mut p = offline();
    p.display.wayland = SocketAccess::Deny;
    p.display.x11 = SocketAccess::Deny;
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::NoDisplay));
    let mut q = offline();
    q.display.wayland = SocketAccess::Deny;
    w.launch.compat.window_system = WindowSystem::Wayland;
    assert!(codes(&validate(&w, &q)).contains(&IssueCode::WindowSystemDenied));
}
#[test]
fn no_display_on_host_fails_planning_with_sbx_020() {
    let mut w = SandboxWorld::worked_example();
    w.host.display.wayland = None;
    w.host.display.x11 = None;
    let err = plan(&offline(), &w.ctx(), &w.host).unwrap_err();
    let i = err.iter().find(|i| i.code == IssueCode::NoDisplay).unwrap();
    assert_eq!(i.code.error_code().as_str(), "EP-SBX-020");
}
#[test]
fn file_only_iff_loose_executable() {
    let mut w = SandboxWorld::worked_example();
    let mut p = offline();
    p.game_dir = GameDirAccess::FileOnly;
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::GameDirKindMismatch));
    w.loose_exe();
    assert!(codes(&validate(&w, &offline())).contains(&IssueCode::GameDirKindMismatch));
    assert!(!validate(&w, &p).iter().any(PolicyIssue::is_blocking));
}
#[test]
fn exe_outside_root_is_rejected() {
    let mut w = SandboxWorld::worked_example();
    w.exe = "/mnt/games/Celeste/c.exe".into();
    assert!(codes(&validate(&w, &offline())).contains(&IssueCode::ExeOutsideRoot));
}
#[test]
fn save_dir_rules() {
    let w = SandboxWorld::worked_example();
    let mut p = offline();
    p.game_dir = GameDirAccess::ReadOnly;
    p.save_data = SaveDataLocation::External {
        dir: "/mnt/games/Hollow Knight/hollow_knight_Data".into(),
    };
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::SaveDirConflict));
    let mut q = offline();
    q.save_data = SaveDataLocation::External {
        dir: "/home/sam/.ssh".into(),
    };
    assert!(codes(&validate(&w, &q)).contains(&IssueCode::PathDenied));
}
#[test]
fn grant_containing_game_folder_is_rejected() {
    let w = SandboxWorld::worked_example();
    let c = codes(&validate(
        &w,
        &with_grant("/mnt/games", FolderAccess::ReadOnly),
    ));
    assert!(c.contains(&IssueCode::PathCoversGameDir));
    let mut w2 = SandboxWorld::worked_example();
    w2.root = Some("/run/media/sam/USB/Games".into());
    w2.exe = "/run/media/sam/USB/Games/g.exe".into();
    w2.fs.file(&w2.exe, b"MZ");
    let mut p = offline();
    p.removable = RemovableAccess::ReadOnly;
    assert!(codes(&validate(&w2, &p)).contains(&IssueCode::PathCoversGameDir));
}
#[test]
fn duplicate_grants_are_rejected() {
    let w = SandboxWorld::worked_example();
    let mut p = with_grant("/mnt/data/shared", FolderAccess::ReadOnly);
    p.folders
        .push(grant("/mnt/data/shared", FolderAccess::ReadWrite, None));
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::PathDuplicate));
}

// ---- exceptions and variables --------------------------------------------------------

fn exc(path: &str) -> PolicyException {
    PolicyException::ExtraReadOnlyPath {
        path: path.into(),
        canonical: path.into(),
        reason: "Mod manager reads its config".into(),
    }
}

#[test]
fn exceptions_need_developer_mode() {
    let w = SandboxWorld::worked_example();
    let mut p = offline();
    p.exceptions.push(exc("/mnt/data/shared"));
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::ExceptionNeedsDevMode));
}
#[test]
fn developer_mode_bypasses_credentials_but_never_easyplay_or_top_level() {
    let mut w = SandboxWorld::worked_example();
    w.developer_mode = true;
    let mut p = offline();
    p.exceptions.push(exc("/home/sam/.config/gh"));
    let i = validate(&w, &p);
    assert!(!i.iter().any(PolicyIssue::is_blocking), "{i:#?}");
    assert!(codes(&i).contains(&IssueCode::PathDenyBypassed));
    let pl = plan(&p, &w.ctx(), &w.host).unwrap();
    let st = easyplay_core::sandbox::explain(&pl, &p);
    assert!(
        st.iter()
            .any(|s| s.severity == easyplay_core::sandbox::StatementSeverity::Danger)
    );
    for denied in ["/home/sam", "/home/sam/.local/share/easyplay"] {
        w.fs.dir(denied);
        let mut q = offline();
        q.exceptions.push(exc(denied));
        assert!(
            codes(&validate(&w, &q)).contains(&IssueCode::PathDenied),
            "{denied}"
        );
    }
}
#[test]
fn exception_reason_must_be_ten_characters() {
    let mut w = SandboxWorld::worked_example();
    w.developer_mode = true;
    let mut p = offline();
    p.exceptions.push(PolicyException::ExtraReadOnlyPath {
        path: "/mnt/data/shared".into(),
        canonical: "/mnt/data/shared".into(),
        reason: "short".into(),
    });
    assert!(codes(&validate(&w, &p)).contains(&IssueCode::ExceptionReason));
}
#[test]
fn extra_device_rules() {
    let mut w = SandboxWorld::worked_example();
    w.developer_mode = true;
    let dev = |p: &str| {
        let mut pol = offline();
        pol.exceptions.push(PolicyException::ExtraDevice {
            path: p.into(),
            reason: "Racing wheel force feedback".into(),
        });
        pol
    };
    assert!(
        !validate(&w, &dev("/dev/hidraw7"))
            .iter()
            .any(PolicyIssue::is_blocking)
    );
    assert!(codes(&validate(&w, &dev("/dev/uinput"))).contains(&IssueCode::ExceptionDevice));
    assert!(codes(&validate(&w, &dev("/dev/missing"))).contains(&IssueCode::ExceptionDevice));
    let pl = plan(&dev("/dev/hidraw7"), &w.ctx(), &w.host).unwrap();
    assert!(pl.mount_at(Path::new("/dev/hidraw7")).is_some());
}
#[test]
fn user_variable_rules() {
    assert_eq!(classify_user_var("HOME"), UserVarRule::Reserved);
    assert_eq!(classify_user_var("UMU_ZENITY"), UserVarRule::Reserved);
    assert_eq!(
        classify_user_var("DBUS_SESSION_BUS_ADDRESS"),
        UserVarRule::Reserved
    );
    assert_eq!(classify_user_var("container"), UserVarRule::Reserved);
    assert_eq!(classify_user_var("LD_PRELOAD"), UserVarRule::Dangerous);
    assert_eq!(classify_user_var("GST_PLUGIN_PATH"), UserVarRule::Dangerous);
    assert_eq!(classify_user_var("DXVK_CONFIG_FILE"), UserVarRule::Allowed);
    assert_eq!(classify_user_var("1BAD"), UserVarRule::Invalid);
    let mut w = SandboxWorld::worked_example();
    w.launch.env.push(EnvVar {
        name: "PROTONPATH".into(),
        value: "/x".into(),
    });
    assert!(codes(&validate(&w, &offline())).contains(&IssueCode::EnvReserved));
}
#[test]
fn issue_codes_are_unique_and_stable() {
    let mut seen = std::collections::BTreeSet::new();
    for c in IssueCode::ALL {
        assert!(seen.insert(c.as_str()), "{}", c.as_str());
        assert!(c.as_str().starts_with("SEC-"));
    }
    let _ = (IssueSeverity::Info, NetworkAccess::None);
}

/// ADR-0033: the chat database and its key (`<data>/chat`) are never reachable from a
/// game: not the folder, nothing in it, and no folder above it (`~/.local/share`, the home
/// folder) can be granted, and no Developer Mode exception lifts it.
#[test]
fn deny_chat_folder_and_everything_above_it() {
    let w = SandboxWorld::worked_example();
    let d = deny(&w);
    let chat = w.ctx().paths.chat_dir.clone();
    let key = chat.join("key");
    for p in [&chat, &key] {
        let hits = d.check(p);
        assert!(
            hits.iter().any(|h| h.group == DenyGroup::EasyPlay),
            "{} is not denied: {hits:?}",
            p.display()
        );
        assert!(hits.iter().all(|h| !h.group.bypassable()));
    }
    let mut above = chat.parent();
    while let Some(p) = above {
        assert!(d.is_denied(p), "{} can be granted", p.display());
        above = p.parent().filter(|p| *p != Path::new("/"));
    }
}
