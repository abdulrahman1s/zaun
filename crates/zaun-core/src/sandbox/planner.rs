//! `plan(policy, context, host)`: run the sections in the fixed order, then the invariants
//! (BUBBLEWRAP_SANDBOX §1, §7; SECURITY_MODEL §7.4). Pure: reads through `Fs` only, writes
//! nothing, spawns nothing, never consults the clock.

use std::ffi::OsString;
use std::path::PathBuf;

use super::context::SandboxContext;
use super::plan::{
    FD_ARGS, FD_INIT_STATUS, FD_SECCOMP, FdContent, FdPayload, HostAction, INIT_PATH, PlanBuilder,
    SANDBOX_PLAN_FORMAT, SandboxPlan, Section,
};
use super::sections::{
    Effective, devices, env, etc_files, game, grants, home, namespaces, prefix, runtime_store,
    sysfs, system, tmp_run,
};
use super::validate::{
    IssueCode, PolicyIssue, ValidationInputs, any_blocking, plan_invariants, validate_policy,
};
use crate::host::HostFacts;
use crate::model::{GameDirAccess, RuntimeIsolationMode, SandboxPolicy, SessionKind, WorkingDir};
use crate::paths::normalize;

/// Build and validate the plan of one sandbox.
///
/// `Err` carries every issue found (at least one blocking); `Ok` plans carry their
/// non-blocking issues in [`SandboxPlan::issues`].
pub fn plan(
    policy: &SandboxPolicy,
    ctx: &SandboxContext<'_>,
    host: &HostFacts,
) -> Result<SandboxPlan, Vec<PolicyIssue>> {
    plan_with(policy, ctx, host, |_| {})
}

/// [`plan`] with a hook that may alter the finished plan before the invariants run. Exists
/// so tests can prove that a broken section is caught (SANDBOX-017); never used otherwise.
#[doc(hidden)]
pub fn plan_with(
    policy: &SandboxPolicy,
    ctx: &SandboxContext<'_>,
    host: &HostFacts,
    tamper: impl FnOnce(&mut SandboxPlan),
) -> Result<SandboxPlan, Vec<PolicyIssue>> {
    if let Some(issue) = check_context(ctx) {
        return Err(vec![issue]);
    }
    let mut issues = validate_policy(policy, &ValidationInputs::from_context(ctx, host));
    if any_blocking(&issues) {
        return Err(issues);
    }
    let eff = Effective::compute(policy, ctx, host);
    let mut b = PlanBuilder::new();

    let step = |b: &mut PlanBuilder, s: Section| {
        // Order is fixed below; a failure here is recorded as builder misuse.
        let _ = b.begin(s);
    };
    step(&mut b, Section::Namespaces);
    let (namespaces, hostname) = namespaces::build(&eff, &mut b);
    step(&mut b, Section::System);
    system::emit(ctx, host, &mut b);
    step(&mut b, Section::Etc);
    etc_files::emit(ctx, host, &eff, &mut b);
    step(&mut b, Section::Devices);
    devices::emit(policy, host, &eff, ctx.developer_mode, &mut b);
    step(&mut b, Section::Sysfs);
    sysfs::emit(host, &eff, &mut b);
    step(&mut b, Section::TmpRun);
    tmp_run::emit(ctx, host, &eff, &mut b);
    step(&mut b, Section::Home);
    home::emit(ctx, &mut b);
    step(&mut b, Section::RuntimeStore);
    runtime_store::emit(ctx, &eff, &mut b);
    step(&mut b, Section::Prefix);
    prefix::emit(policy, ctx, &mut b);
    step(&mut b, Section::Game);
    game::emit(policy, ctx, host, &mut b);
    step(&mut b, Section::Grants);
    grants::emit(policy, ctx, &mut b);
    step(&mut b, Section::Umu);
    system::emit_umu(ctx, host, &mut b);
    step(&mut b, Section::Env);
    env::emit(policy, ctx, host, &eff, &mut b);
    step(&mut b, Section::Command);
    let chdir = working_dir(policy, ctx, &mut b);
    let mut command: Vec<OsString> = vec![
        INIT_PATH.into(),
        "--status-fd".into(),
        FD_INIT_STATUS.to_string().into(),
        "--".into(),
        ctx.umu.path.clone().into_os_string(),
    ];
    command.extend(ctx.launch.umu_args.iter().cloned());

    let parts = b.finish();
    issues.extend(parts.issues);
    if let Some(e) = parts.error {
        issues.push(PolicyIssue::fatal(
            IssueCode::InvBuilder,
            None,
            format!("sec.plan.builder|{e}"),
        ));
    }
    let mut fds = parts.fds;
    if eff.seccomp {
        fds.push(FdPayload {
            fd: FD_SECCOMP,
            target: None,
            content: FdContent::SeccompProgram,
        });
    }
    fds.sort_by_key(|f| f.fd);
    let mut host_actions = parts.actions;
    host_actions.push(HostAction::WriteMemfd { fd: FD_ARGS });
    host_actions.extend(fds.iter().map(|f| HostAction::WriteMemfd { fd: f.fd }));
    if ctx.untrusted()
        && let Some(g) = ctx.game
    {
        host_actions.push(HostAction::MarkTainted { game: g.id });
    }
    let audit_after = match (eff.isolation, ctx.runtime) {
        (Some(RuntimeIsolationMode::Shared), Some(rt)) => Some(rt.slr_variant),
        _ => None,
    };
    let mut plan = SandboxPlan {
        format: SANDBOX_PLAN_FORMAT,
        kind: ctx.kind,
        namespaces,
        hostname,
        mounts: parts.mounts,
        env: super::plan::EnvPlan {
            clear: true,
            set: parts.env,
            unset: Vec::new(),
        },
        fds,
        chdir,
        command,
        host_actions,
        notes: parts.notes,
        issues: Vec::new(),
        audit_after,
    };
    tamper(&mut plan);
    issues.extend(plan_invariants(&plan, policy, ctx, host));
    dedupe(&mut issues);
    if any_blocking(&issues) {
        return Err(issues);
    }
    plan.issues = issues;
    Ok(plan)
}

fn dedupe(issues: &mut Vec<PolicyIssue>) {
    let mut seen: Vec<PolicyIssue> = Vec::with_capacity(issues.len());
    for i in issues.drain(..) {
        if !seen.contains(&i) {
            seen.push(i);
        }
    }
    *issues = seen;
}

fn check_context(ctx: &SandboxContext<'_>) -> Option<PolicyIssue> {
    let bad = |m: &'static str| Some(PolicyIssue::error(IssueCode::ContextInvalid, None, m));
    if ctx.kind != SessionKind::Provision && ctx.game.is_none() {
        return bad("sec.context.no_game|This kind of session needs a game");
    }
    if ctx.runtime.is_none() && !(ctx.trusted_network() && ctx.proton_codename.is_some()) {
        return bad("sec.context.no_runtime|No compatibility layer was resolved");
    }
    if !ctx.home.is_absolute() || !ctx.init_binary.is_absolute() || !ctx.umu.path.is_absolute() {
        return bad("sec.context.relative|Context paths must be absolute");
    }
    None
}

/// §4.16 working directory.
fn working_dir(policy: &SandboxPolicy, ctx: &SandboxContext<'_>, b: &mut PlanBuilder) -> PathBuf {
    let home = ctx.home.to_path_buf();
    let Some(game) = ctx.game.filter(|_| ctx.untrusted()) else {
        return home;
    };
    let folder = match (policy.game_dir, game.root) {
        (GameDirAccess::FileOnly, _) | (_, None) => None,
        (_, Some(r)) => Some(r),
    };
    match ctx.launch.working_dir {
        WorkingDir::ExeDir => match folder {
            Some(_) => game.exe.parent().map_or(home, |p| p.to_path_buf()),
            None => home,
        },
        WorkingDir::GameRoot => folder.map_or(home, |r| r.to_path_buf()),
        WorkingDir::Custom(p) => {
            let p = normalize(p);
            let mut mounted: Vec<PathBuf> = vec![home.clone(), game.prefix.to_path_buf()];
            mounted.extend(folder.map(|r| r.to_path_buf()));
            mounted.extend(policy.folders.iter().map(|g| g.canonical.clone()));
            if p.is_absolute() && mounted.iter().any(|m| p.starts_with(m)) {
                p
            } else {
                b.issue(PolicyIssue::error(
                    IssueCode::WorkingDirOutside,
                    Some(p),
                    "sec.path.working_dir|The working folder must be inside a folder the game can use",
                ));
                home
            }
        }
    }
}
