//! `explain_policy` (a policy explained without a launch plan) agrees with
//! `explain(plan(policy))` for the same policy, game and host (SANDBOX-019, the UI's `assess`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use proptest::prelude::*;
use zaun_core::model::{FolderAccess, FolderGrant, GrantOrigin, SandboxPolicy, SessionKind};
use zaun_core::sandbox::explain::{PolicyContext, explain_policy};
use zaun_core::sandbox::{AccessStatement, IssueCode, explain, plan};
use zaun_testkit::fixtures::hosts::{HOME, SandboxWorld};
use zaun_testkit::fixtures::policies::arb_case;

/// Everything but the mechanisms (the synthetic runtime is not listed, so mount indices move).
fn words(s: &[AccessStatement]) -> Vec<AccessStatement> {
    s.iter()
        .map(|s| AccessStatement {
            mounts: Vec::new(),
            ..s.clone()
        })
        .collect()
}

fn pctx(w: &SandboxWorld) -> PolicyContext<'_> {
    PolicyContext {
        game: w.game_id,
        root: w.root.as_deref(),
        exe: &w.exe,
        home: Path::new(HOME),
        uid: 1000,
        gid: 1000,
        user_name: "sam",
        group_name: "sam",
        paths: &w.paths,
        launch: &w.launch,
        global_env: &w.global_env,
        host_env: &w.host_env,
        user_dirs: &w.user_dirs,
        other_game_roots: &w.other_roots,
        mounts: &w.mounts,
        fs: &w.fs,
        developer_mode: w.developer_mode,
        seccomp_available: w.seccomp_available,
        isolation: w.runtime.isolation,
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 500, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn agrees_with_the_launch_plan(case in arb_case()) {
        let mut w = case.world();
        w.set_kind(SessionKind::Game, None);
        w.umu_args = vec![w.exe.clone().into_os_string()];
        let real = plan(&case.policy, &w.ctx(), &w.host);
        let got = explain_policy(&case.policy, &pctx(&w), &w.host);
        match real {
            Ok(p) => {
                prop_assert_eq!(words(&explain(&p, &case.policy)), words(&got.statements));
                let real_codes: Vec<IssueCode> = p.issues.iter().map(|i| i.code).collect();
                let got_codes: Vec<IssueCode> = got.issues.iter().map(|i| i.code).collect();
                prop_assert_eq!(real_codes, got_codes);
            }
            Err(issues) => {
                prop_assert!(got.issues.iter().any(|i| i.is_blocking()), "{issues:#?}");
            }
        }
    }
}

#[test]
fn missing_folder_is_reported_and_the_rest_explained() {
    let w = SandboxWorld::worked_example();
    let mut policy: SandboxPolicy = zaun_core::sandbox::offline();
    let gone = PathBuf::from("/home/sam/Gone");
    policy.folders.push(FolderGrant {
        path: gone.clone(),
        canonical: gone.clone(),
        access: FolderAccess::ReadOnly,
        origin: GrantOrigin::UserChosen,
        drive_letter: None,
        granted_at: zaun_core::clock::Timestamp::from_millis(0),
    });
    let got = explain_policy(&policy, &pctx(&w), &w.host);
    assert!(
        got.issues
            .iter()
            .any(|i| i.code == IssueCode::PathMissing && i.path.as_deref() == Some(gone.as_path()))
    );
    assert!(
        got.statements
            .iter()
            .any(|s| s.key == "access.network.none")
    );
    assert!(
        !got.statements
            .iter()
            .any(|s| s.args.get("path").map(String::as_str) == Some("/home/sam/Gone"))
    );
}

#[test]
fn no_breadth_scan_and_no_runtime_issue() {
    let w = SandboxWorld::worked_example();
    let policy = zaun_core::sandbox::offline();
    let got = explain_policy(&policy, &pctx(&w), &w.host);
    assert!(
        got.issues
            .iter()
            .all(|i| i.code != IssueCode::RuntimeLayout && i.code != IssueCode::BroadGrant),
        "{:#?}",
        got.issues
    );
    assert!(!got.statements.is_empty());
}
