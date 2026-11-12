//! `easyplay sandbox print <GAME> [--format args|json|explain|shell] [--kind game|prepare]`
//! (LAUNCH-006, CONFIGURATION §6, DEVELOPER_MODE §6).
//!
//! A dry run: nothing is created, no prefix is marked, no session row is written. `<GAME>` is
//! a temporary game source ([`crate::game_source`]). When the game's compatibility layer
//! still has to be installed, the plan of the Provision step is printed with a note, as a
//! dry run in the GUI would.

use std::sync::Arc;

use easyplay_core::error::{Error, ErrorCode};
use easyplay_core::ids::SessionId;
use easyplay_core::launch::preflight::STAT_DEADLINE;
use easyplay_core::launch::{self, LaunchTarget, PlanRequest, PlannedLaunch, PreflightInput};
use easyplay_core::model::{ResolvedRuntime, SessionKind};
use easyplay_core::runtime::{Resolution, RuntimeService};
use easyplay_core::sandbox::{self, ValidationInputs};
use easyplay_core::session::FlockLocks;

use crate::cli::{PlanKind, SandboxFormat};
use crate::game_source::{HostContext, load_game, print_issues};

/// Run the verb.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub fn run(game_arg: &str, format: SandboxFormat, kind: PlanKind) -> Result<(), Error> {
    let hc = HostContext::probe()?;
    let (fs, clock, host, mounts, paths) = (&hc.fs, &hc.clock, &hc.host, &hc.mounts, &hc.paths);
    let game = load_game(&**fs, mounts, host, &**clock, game_arg)?;
    let service = hc.runtime_service();
    let resolution = service.resolve(&game.runtime, Some(&game));
    let mut notes: Vec<String> = Vec::new();
    let (target, runtime, codename): (LaunchTarget<'_>, Option<ResolvedRuntime>, Option<String>) =
        match (resolution, kind) {
            (Resolution::Ready(rt), PlanKind::Game) => (
                LaunchTarget::Game {
                    exe: &game.executables[0],
                    extra_args: &[],
                },
                Some(rt),
                None,
            ),
            (Resolution::Ready(rt), PlanKind::Prepare) => (LaunchTarget::Prepare, Some(rt), None),
            (Resolution::NeedsProvision(need), _) => {
                notes.push(format!(
                    "The compatibility layer is not installed yet ({}); this is the plan of \
                     the download step that would run first.",
                    need.protonpath()
                ));
                (LaunchTarget::Provision, None, Some(need.protonpath()))
            }
            (Resolution::Unavailable(r), _) => {
                return Err(Error::new(r.code()).with("reason", format!("{r:?}")));
            }
        };

    // A dry run does not open the library: the stand-in machine ID uses zeros.
    let host_env_ref = hc.host_env("00000000000000000000000000000000");
    let session_id = SessionId::generate(&**clock);
    let req = PlanRequest {
        session_id,
        target,
        game: Some(&game),
        runtime: runtime.as_ref(),
        proton_codename: codename.as_deref(),
        network_override: None,
        host,
        env: &host_env_ref,
    };

    // Preflight for game launches: a dry run reports its failures the same way.
    if target.kind() == SessionKind::Game {
        let prefix_lock = paths.prefix_lock(game.id);
        let checked = launch::with_context(&req, |ctx, _, _| {
            launch::preflight(&PreflightInput {
                game: &game,
                exe: &game.executables[0],
                kind: SessionKind::Game,
                host,
                validation: ValidationInputs::from_context(ctx, host),
                fs: Arc::clone(fs),
                mounts,
                init_binary: &hc.init,
                prefix_lock: &prefix_lock,
                locks: &FlockLocks,
                data_dir: &paths.data_dir,
                isolation: runtime.as_ref().map(|r| (r.isolation, host.fs.reflink_ok)),
                deadline: STAT_DEADLINE,
            })
        })
        .map_err(Error::from)?;
        match checked {
            Ok(report) => {
                if let Some((avail, wanted)) = report.low_space {
                    notes.push(format!(
                        "Low disk space: {} MiB free, {} MiB recommended (EP-FS-002).",
                        avail >> 20,
                        wanted >> 20
                    ));
                }
            }
            Err(f) => {
                if let launch::PreflightFailure::PolicyInvalid(issues) = &f {
                    print_issues(issues);
                }
                return Err(f.into());
            }
        }
    }

    let planned: PlannedLaunch = match launch::plan_launch(&req) {
        Ok(p) => p,
        Err(e) => {
            print_issues(e.issues());
            return Err(e.into());
        }
    };
    for n in &notes {
        eprintln!("note: {n}");
    }
    for n in &planned.sandbox.notes {
        eprintln!("note: {}", n.text);
    }
    print_issues(&planned.sandbox.issues);

    match format {
        SandboxFormat::Args => {
            println!("{}", launch::shell::quote(planned.plan.bwrap.as_os_str()));
            for g in &planned.rendered.groups {
                let line: Vec<String> = g.iter().map(|a| launch::shell::quote(a)).collect();
                println!("{}", line.join(" "));
            }
            let cmd: Vec<String> = planned
                .rendered
                .command
                .iter()
                .map(|a| launch::shell::quote(a))
                .collect();
            println!("-- {}", cmd.join(" "));
        }
        SandboxFormat::Json => {
            #[derive(serde::Serialize)]
            struct Out<'a> {
                plan: &'a launch::LaunchPlan,
                sandbox: &'a sandbox::SandboxPlan,
                umu: &'a easyplay_core::umu::UmuInvocation,
                notes: &'a [String],
            }
            let out = Out {
                plan: &planned.plan,
                sandbox: &planned.sandbox,
                umu: &planned.umu,
                notes: &notes,
            };
            let text = serde_json::to_string_pretty(&out)
                .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with_source(e))?;
            println!("{text}");
        }
        SandboxFormat::Explain => {
            for s in sandbox::explain(&planned.sandbox, &game.policy) {
                println!("[{:?}] {}", s.severity, s.text);
                if let Some(d) = &s.detail {
                    println!("    {d}");
                }
            }
        }
        SandboxFormat::Shell => {
            print!(
                "{}",
                launch::render_shell(&planned.plan, &clock.now().to_rfc3339())
            );
        }
    }
    Ok(())
}
