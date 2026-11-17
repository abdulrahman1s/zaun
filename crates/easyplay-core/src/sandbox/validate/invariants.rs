//! Plan invariants (SECURITY_MODEL §7.4 items 1–10, BUBBLEWRAP_SANDBOX §7). Any failure is a
//! planner bug: the launch aborts with `EP-SBX-900`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::deny::DenyList;
use super::env_rules::{UserVarRule, classify_user_var, is_never_set};
use super::issue::{IssueCode, PolicyIssue};
use crate::host::HostFacts;
use crate::model::{NetworkAccess, SandboxPolicy};
use crate::paths::normalize;
use crate::sandbox::context::SandboxContext;
use crate::sandbox::plan::{
    EnvOrigin, FD_INIT_STATUS, HostAction, INIT_PATH, Mount, MountKind, MountMode, PolicyField,
    SandboxPlan, Section, sound_socket,
};
use crate::sandbox::sections::tmp_run::XAUTH_NAME;
use crate::sandbox::sections::{Effective, SandboxDirs};

fn fail(code: IssueCode, path: Option<&Path>, msg: impl Into<String>) -> PolicyIssue {
    PolicyIssue::fatal(
        code,
        path.map(Path::to_path_buf),
        format!("sec.plan.invariant|{}", msg.into()),
    )
}

/// Check a finished plan. Returns one `Fatal` issue per violation.
pub fn plan_invariants(
    plan: &SandboxPlan,
    policy: &SandboxPolicy,
    ctx: &SandboxContext<'_>,
    host: &HostFacts,
) -> Vec<PolicyIssue> {
    let eff = Effective::compute(policy, ctx, host);
    let mut out = Vec::new();
    namespaces(plan, &eff, &mut out);
    environment(plan, ctx, &mut out);
    denied_sources(plan, ctx, &mut out);
    runtime_and_games(plan, ctx, &mut out);
    sockets(plan, ctx, host, &mut out);
    destinations(plan, &mut out);
    command(plan, ctx, &mut out);
    out
}

/// Invariants 1 and 2.
fn namespaces(plan: &SandboxPlan, eff: &Effective, out: &mut Vec<PolicyIssue>) {
    let ns = &plan.namespaces;
    if !(ns.user && ns.pid && ns.uts && ns.cgroup_try) {
        out.push(fail(
            IssueCode::InvNamespaces,
            None,
            "user, PID and UTS namespaces are required",
        ));
    }
    let x11_bound = plan
        .mounts
        .iter()
        .any(|m| m.origin.policy_field == Some(PolicyField::X11) && m.kind.is_host_bind());
    if !ns.ipc && !x11_bound {
        out.push(fail(
            IssueCode::InvNamespaces,
            None,
            "the IPC namespace may be shared only when the X11 socket is bound",
        ));
    }
    if ns.net != (eff.network == NetworkAccess::None) {
        out.push(fail(
            IssueCode::InvNamespaces,
            None,
            "the network namespace must be unshared exactly when the network is off",
        ));
    }
    if !(ns.die_with_parent && ns.new_session) {
        out.push(fail(
            IssueCode::InvLifecycle,
            None,
            "--die-with-parent and --new-session are required",
        ));
    }
    if ns.json_status_fd.is_none() || ns.sync_fd.is_none() {
        out.push(fail(
            IssueCode::InvLifecycle,
            None,
            "status and sync descriptors are required",
        ));
    }
}

/// Invariant 3.
fn environment(plan: &SandboxPlan, ctx: &SandboxContext<'_>, out: &mut Vec<PolicyIssue>) {
    if !plan.env.clear {
        out.push(fail(
            IssueCode::InvEnv,
            None,
            "the environment must be cleared",
        ));
    }
    for (name, entry) in &plan.env.set {
        if is_never_set(name) {
            out.push(fail(
                IssueCode::InvEnv,
                None,
                format!("{name} must never be set"),
            ));
        }
        let user = matches!(
            entry.origin,
            EnvOrigin::Global | EnvOrigin::Game | EnvOrigin::Exception
        );
        if user {
            match classify_user_var(name) {
                UserVarRule::Allowed => {}
                UserVarRule::Dangerous if ctx.developer_mode => {}
                _ => out.push(fail(
                    IssueCode::InvEnv,
                    None,
                    format!("{name} cannot come from user input"),
                )),
            }
        }
        if name.contains('=') || name.contains('\0') || entry.value.contains('\0') {
            out.push(fail(
                IssueCode::InvEnv,
                None,
                format!("{name} is malformed"),
            ));
        }
    }
}

/// Invariant 4.
fn denied_sources(plan: &SandboxPlan, ctx: &SandboxContext<'_>, out: &mut Vec<PolicyIssue>) {
    let deny = DenyList::new(
        ctx.home,
        ctx.uid,
        ctx.user_name,
        ctx.paths,
        Some(ctx.mounts),
    );
    for m in &plan.mounts {
        let (Some(field), Some(src)) = (&m.origin.policy_field, m.kind.src()) else {
            continue;
        };
        let exception = matches!(field, PolicyField::ExtraReadOnlyPath(_));
        let user_path = exception
            || matches!(
                field,
                PolicyField::GameDir | PolicyField::Folder(_) | PolicyField::SaveData
            );
        if !user_path {
            continue;
        }
        for hit in deny.check(src) {
            let accepted = exception && ctx.developer_mode && hit.group.bypassable();
            if !accepted {
                out.push(fail(
                    IssueCode::InvDenied,
                    Some(src),
                    format!("source is on the deny-list ({:?})", hit.group),
                ));
            }
        }
    }
}

/// Invariants 5, 6 and 7.
fn runtime_and_games(plan: &SandboxPlan, ctx: &SandboxContext<'_>, out: &mut Vec<PolicyIssue>) {
    let paths = ctx.paths;
    let untrusted = ctx.untrusted();
    let own_game = ctx.game.map(|g| paths.game_dir(g.id));
    for m in &plan.mounts {
        let writable: Vec<&Path> = match &m.kind {
            MountKind::Bind {
                src,
                mode: MountMode::Rw,
                ..
            } => vec![src],
            MountKind::OverlayRw { upper, work, .. } => vec![upper, work],
            _ => continue,
        };
        for src in writable {
            if untrusted && src.starts_with(&paths.runtimes_dir) {
                let lock = ctx
                    .runtime
                    .is_some_and(|rt| src.starts_with(&rt.slr_dir) && src.ends_with("files/.ref"));
                let var = src == ctx.session.var_dir;
                let shared = plan.audit_after.is_some() && src == paths.umu_store_dir;
                if !(lock || var || shared) {
                    out.push(fail(
                        IssueCode::InvRuntimeWritable,
                        Some(src),
                        "the runtime store is writable in an untrusted session",
                    ));
                }
            }
            if src.starts_with(&paths.games_dir)
                && !own_game.as_ref().is_some_and(|g| src.starts_with(g))
            {
                out.push(fail(
                    IssueCode::InvOtherGame,
                    Some(src),
                    "writable bind into another game's storage",
                ));
            }
        }
    }
    if let Some(rt) = ctx.runtime
        && !ctx.trusted_network()
    {
        let proton: Vec<&Mount> = plan
            .mounts
            .iter()
            .filter(|m| m.kind.src() == Some(rt.proton_path_host.as_path()))
            .collect();
        if proton.is_empty() {
            out.push(fail(
                IssueCode::InvProtonWritable,
                Some(&rt.proton_path_host),
                "the selected compatibility layer is not mounted",
            ));
        }
        for m in proton {
            if m.kind.writes_host() {
                out.push(fail(
                    IssueCode::InvProtonWritable,
                    Some(&rt.proton_path_host),
                    "the compatibility layer must be read-only",
                ));
            }
        }
    }
}

/// Invariant 8.
fn sockets(
    plan: &SandboxPlan,
    ctx: &SandboxContext<'_>,
    host: &HostFacts,
    out: &mut Vec<PolicyIssue>,
) {
    let dirs = SandboxDirs::new(ctx.home, ctx.uid);
    let xrd = &dirs.xrd;
    let mut allowed_dst: Vec<PathBuf> = vec![
        xrd.join("pulse/native"),
        xrd.join("pipewire-0"),
        xrd.join(XAUTH_NAME),
    ];
    if let Some(w) = &host.display.wayland {
        allowed_dst.push(xrd.join(&w.name));
    }
    let x11_dst = host
        .display
        .x11_bindable()
        .map(|x| Path::new("/tmp/.X11-unix").join(format!("X{}", x.number)));
    let host_xrd = host.display.xdg_runtime_dir.clone();
    // The sockets of the session's own sound server, which the plan starts (ADR-0026), and
    // of its restricted Wayland connection, which the plan creates (ADR-0029).
    let sound: Vec<PathBuf> = plan
        .host_actions
        .iter()
        .filter_map(|a| match a {
            HostAction::StartSoundServer { dir, .. } => Some(sound_socket(dir)),
            _ => None,
        })
        .chain(crate::sandbox::wayland_ctx::context_sockets(&plan.host_actions).cloned())
        .collect();
    let restricted_wayland = crate::sandbox::wayland_ctx::context_sockets(&plan.host_actions)
        .next()
        .is_some();
    for m in &plan.mounts {
        let dst = m.kind.dst();
        if m.kind.is_host_bind() && dst.starts_with(xrd) && !allowed_dst.iter().any(|a| a == dst) {
            out.push(fail(
                IssueCode::InvSocket,
                Some(dst),
                "unexpected entry in the runtime directory",
            ));
        }
        if m.kind.is_host_bind()
            && dst.starts_with("/tmp/.X11-unix")
            && x11_dst.as_deref() != Some(dst)
        {
            out.push(fail(
                IssueCode::InvSocket,
                Some(dst),
                "unexpected X11 socket",
            ));
        }
        if let (Some(src), Some(hx)) = (m.kind.src(), &host_xrd)
            && src.starts_with(hx)
        {
            let known = host.display.wayland.as_ref().is_some_and(|w| w.path == src)
                || host.audio.pulse_socket.as_deref() == Some(src)
                || host.audio.pipewire_socket.as_deref() == Some(src)
                || sound.iter().any(|s| s == src)
                || host
                    .display
                    .x11
                    .as_ref()
                    .and_then(|x| x.authority.as_deref())
                    == Some(src);
            if !known {
                out.push(fail(
                    IssueCode::InvSocket,
                    Some(src),
                    "only the documented sockets may come from the runtime directory",
                ));
            }
        }
        // A restricted Wayland socket replaces the compositor's own; never both (ADR-0029).
        if restricted_wayland
            && let Some(src) = m.kind.src()
            && host.display.wayland.as_ref().is_some_and(|w| w.path == src)
        {
            out.push(fail(
                IssueCode::InvSocket,
                Some(src),
                "the compositor's own Wayland socket is bound next to the restricted one",
            ));
        }
    }
}

/// Pairs (earlier section, later section) where a later host bind may lie inside an earlier
/// host bind (BUBBLEWRAP_SANDBOX §7 rule 2–3).
fn nested_allowed(earlier: &Mount, later: &Mount) -> bool {
    use Section as S;
    let (a, b) = (earlier.origin.section, later.origin.section);
    let identity_ro = |m: &Mount| matches!(&m.kind, MountKind::Bind { src, dst, mode: MountMode::Ro, .. } if src == dst);
    a == S::Home
        || matches!(
            (a, b),
            (S::System, S::System)
                | (S::Sysfs, S::Sysfs)
                | (S::RuntimeStore, S::RuntimeStore)
                | (S::Prefix, S::Prefix)
                | (S::Game, S::Grants)
                | (S::Grants, S::Grants)
        )
        || (identity_ro(earlier) && identity_ro(later))
}

/// Invariant 9.
fn destinations(plan: &SandboxPlan, out: &mut Vec<PolicyIssue>) {
    for (j, later) in plan.mounts.iter().enumerate() {
        let dst = later.kind.dst();
        if !dst.is_absolute() || normalize(dst) != dst {
            out.push(fail(
                IssueCode::InvDestination,
                Some(dst),
                "destination is not absolute and normalised",
            ));
            continue;
        }
        if let Some(src) = later.kind.src()
            && (!src.is_absolute() || normalize(src) != src)
        {
            out.push(fail(
                IssueCode::InvDestination,
                Some(src),
                "source is not absolute and normalised",
            ));
        }
        for earlier in &plan.mounts[..j] {
            let e = earlier.kind.dst();
            if later.kind.covers() && e.starts_with(dst) {
                out.push(fail(
                    IssueCode::InvDestination,
                    Some(dst),
                    format!(
                        "{} hides the earlier mount at {}",
                        dst.display(),
                        e.display()
                    ),
                ));
            } else if dst.starts_with(e)
                && dst != e
                && earlier.kind.is_host_bind()
                && later.kind.is_host_bind()
                && !nested_allowed(earlier, later)
            {
                out.push(fail(
                    IssueCode::InvDestination,
                    Some(dst),
                    format!(
                        "{} overlaps the earlier bind at {}",
                        dst.display(),
                        e.display()
                    ),
                ));
            }
        }
    }
}

/// Invariant 10.
fn command(plan: &SandboxPlan, ctx: &SandboxContext<'_>, out: &mut Vec<PolicyIssue>) {
    let expected: [OsString; 5] = [
        INIT_PATH.into(),
        "--status-fd".into(),
        FD_INIT_STATUS.to_string().into(),
        "--".into(),
        ctx.umu.path.clone().into_os_string(),
    ];
    if plan.command.len() < expected.len() || plan.command[..expected.len()] != expected {
        out.push(fail(
            IssueCode::InvCommand,
            None,
            "the command must be init --status-fd 6 -- umu-run …",
        ));
    }
    let init_bound = plan.mounts.iter().any(|m| {
        matches!(&m.kind, MountKind::Bind { src, dst, mode: MountMode::Ro, try_: false }
            if src == ctx.init_binary && dst == Path::new(INIT_PATH))
    });
    if !init_bound {
        out.push(fail(
            IssueCode::InvCommand,
            None,
            "the init shim is not mounted",
        ));
    }
    if !plan.chdir.is_absolute() {
        out.push(fail(
            IssueCode::InvCommand,
            Some(&plan.chdir),
            "working directory is not absolute",
        ));
    }
}
