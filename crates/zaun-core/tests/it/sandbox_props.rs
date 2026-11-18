//! Invariant and metamorphic properties of the sandbox planner (SANDBOX-031,
//! TESTING_STRATEGY §4.2). 1 000 cases per property.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use zaun_core::model::{NetworkAccess, SandboxPolicy};
use zaun_core::paths::normalize;
use zaun_core::sandbox::plan::{Mount, MountKind, PolicyField, Section};
use zaun_core::sandbox::sections::Effective;
use zaun_core::sandbox::sections::namespaces::FORBIDDEN_OPTIONS;
use zaun_core::sandbox::validate::{DenyList, IssueSeverity};
use zaun_core::sandbox::{SandboxPlan, explain, plan, plan_invariants, render};
use zaun_testkit::fixtures::hosts::SandboxWorld;
use zaun_testkit::fixtures::policies::{Case, arb_case};

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 1000,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn plan_ok(w: &SandboxWorld, policy: &SandboxPolicy) -> SandboxPlan {
    match plan(policy, &w.ctx(), &w.host) {
        Ok(p) => p,
        Err(issues) => panic!("valid case failed to plan: {issues:#?}"),
    }
}

fn kinds(p: &SandboxPlan) -> Vec<MountKind> {
    p.mounts.iter().map(|m| m.kind.clone()).collect()
}

proptest! {
    #![proptest_config(config())]

    /// Invariants pass; rendering is deterministic; network flag; forbidden options; bind
    /// sources; runtime store; Proton; runtime directory; parents before children.
    #[test]
    fn plan_invariant_properties(case in arb_case()) {
        let w = case.world();
        let ctx = w.ctx();
        let p = plan_ok(&w, &case.policy);
        let fatal: Vec<_> = plan_invariants(&p, &case.policy, &ctx, &w.host);
        prop_assert!(fatal.is_empty(), "{fatal:#?}");
        prop_assert!(p.issues.iter().all(|i| i.severity < IssueSeverity::Error));

        // Deterministic.
        let again = plan_ok(&w, &case.policy);
        prop_assert_eq!(&again, &p);
        let r = render(&p).unwrap();
        prop_assert_eq!(&render(&again).unwrap(), &r);

        // --unshare-net ⇔ effective network None.
        let eff = Effective::compute(&case.policy, &ctx, &w.host);
        let has_net = r.args.iter().any(|a| a == "--unshare-net");
        prop_assert_eq!(has_net, eff.network == NetworkAccess::None);

        // Forbidden options.
        for a in &r.args {
            prop_assert!(!FORBIDDEN_OPTIONS.iter().any(|f| a == *f), "forbidden {a:?}");
        }

        // Bind sources absolute and normalised; user sources not denied.
        let deny = DenyList::new(ctx.home, ctx.uid, ctx.user_name, ctx.paths, Some(ctx.mounts));
        for m in &p.mounts {
            if let Some(src) = m.kind.src() {
                prop_assert!(src.is_absolute() && normalize(src) == src, "{src:?}");
                if matches!(m.origin.policy_field, Some(PolicyField::GameDir | PolicyField::Folder(_) | PolicyField::SaveData)) {
                    prop_assert!(!deny.is_denied(src), "denied source {src:?}");
                }
            }
        }

        // Runtime store and Proton in untrusted kinds.
        if ctx.untrusted() {
            let rt = &w.runtime;
            for m in &p.mounts {
                if let Some(src) = m.kind.src() && m.kind.writes_host() && src.starts_with(&w.paths.runtimes_dir) {
                    let ok = src == w.session.var_dir
                        || (src.starts_with(&rt.slr_dir) && src.ends_with("files/.ref"))
                        || (p.audit_after.is_some() && src == w.paths.umu_store_dir);
                    prop_assert!(ok, "writable store source {src:?}");
                }
                if m.kind.src() == Some(rt.proton_path_host.as_path()) {
                    prop_assert!(!m.kind.writes_host(), "Proton writable");
                }
            }
        }

        // Nothing under /run/user/<uid> except the documented sockets.
        let xrd = Path::new("/run/user/1000");
        let allowed: BTreeSet<PathBuf> = ["wayland-0", "pulse", "pulse/native", "pipewire-0", "zaun-xauth"]
            .iter().map(|n| xrd.join(n)).collect();
        for m in &p.mounts {
            let d = m.kind.dst();
            if d.starts_with(xrd) && d != xrd {
                prop_assert!(allowed.contains(d), "unexpected {d:?}");
            }
        }

        // Parent destinations precede child destinations (no later mount hides an earlier).
        for (j, later) in p.mounts.iter().enumerate() {
            if !later.kind.covers() { continue; }
            for earlier in &p.mounts[..j] {
                prop_assert!(!earlier.kind.dst().starts_with(later.kind.dst()),
                    "{:?} hides {:?}", later.kind.dst(), earlier.kind.dst());
            }
        }

        // The args payload round-trips through a NUL split.
        let payload = r.args_payload();
        let split: Vec<OsString> = payload
            .split(|b| *b == 0)
            .take(r.args.len())
            .map(<std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes)
            .map(std::ffi::OsStr::to_owned)
            .collect();
        prop_assert_eq!(&split, &r.args);
        prop_assert_eq!(payload.last(), Some(&0u8));
    }

    /// Every mount with a policy_field is covered by an access statement, and every statement
    /// has a mechanism.
    #[test]
    fn explain_covers_every_policy_mount(case in arb_case()) {
        let w = case.world();
        let p = plan_ok(&w, &case.policy);
        let st = explain(&p, &case.policy);
        let covered: BTreeSet<usize> = st.iter().flat_map(|s| s.mounts.iter().copied()).collect();
        for (i, m) in p.mounts.iter().enumerate() {
            if m.origin.policy_field.is_some() {
                prop_assert!(covered.contains(&i), "mount {i} {:?} not explained", m.kind);
            }
        }
        for s in &st {
            prop_assert!(s.has_mechanism(), "{} has no mechanism", s.key);
            for i in &s.mounts { prop_assert!(*i < p.mounts.len()); }
        }
        let cats: Vec<_> = st.iter().map(|s| s.category).collect();
        let mut sorted = cats.clone();
        sorted.sort();
        prop_assert_eq!(cats, sorted);
    }

    /// Removing a folder grant removes exactly its mounts.
    #[test]
    fn removing_a_grant_removes_exactly_its_mounts(case in arb_case(), pick in any::<prop::sample::Index>()) {
        prop_assume!(!case.policy.folders.is_empty());
        let w = case.world();
        let with = plan_ok(&w, &case.policy);
        let mut reduced = case.policy.clone();
        let removed = reduced.folders.remove(pick.index(reduced.folders.len()));
        let without = plan_ok(&w, &reduced);
        let expected: Vec<MountKind> = with
            .mounts
            .iter()
            .filter(|m| m.origin.policy_field != Some(PolicyField::Folder(removed.canonical.clone())))
            .map(|m| m.kind.clone())
            .collect();
        prop_assert_eq!(kinds(&without), expected);
        prop_assert_eq!(&without.env, &with.env);
        prop_assert_eq!(&without.namespaces, &with.namespaces);
    }

    /// Switching only `network` changes only the network flag, the network `/etc` entries
    /// (and resolver directory) and the sysfs masks.
    #[test]
    fn switching_network_changes_only_network_parts(case in arb_case()) {
        let w = case.world();
        let a = plan_ok(&w, &case.policy);
        let mut flipped = case.policy.clone();
        flipped.network = match flipped.network {
            NetworkAccess::None => NetworkAccess::Full,
            NetworkAccess::Full => NetworkAccess::None,
        };
        let b = plan_ok(&w, &flipped);
        let rest = |p: &SandboxPlan| -> Vec<MountKind> {
            p.mounts
                .iter()
                .filter(|m: &&Mount| {
                    m.origin.policy_field != Some(PolicyField::Network)
                        && !(m.origin.section == Section::Sysfs && matches!(m.kind, MountKind::Tmpfs { .. }))
                })
                .map(|m| m.kind.clone())
                .collect()
        };
        prop_assert_eq!(rest(&a), rest(&b));
        prop_assert_eq!(&a.env, &b.env);
        let (mut na, mut nb) = (a.namespaces.clone(), b.namespaces.clone());
        na.net = false;
        nb.net = false;
        prop_assert_eq!(na, nb);
        prop_assert_eq!(&a.command, &b.command);
        prop_assert_eq!(&a.chdir, &b.chdir);
        let _ = Case::world;
    }

    /// The environment never contains `container`, `UMU_ZENITY`, `DBUS_SESSION_BUS_ADDRESS`
    /// or a host secret.
    #[test]
    fn environment_never_contains_forbidden_names(case in arb_case()) {
        let w = case.world();
        let p = plan_ok(&w, &case.policy);
        for n in ["container", "UMU_ZENITY", "DBUS_SESSION_BUS_ADDRESS", "AWS_SECRET_ACCESS_KEY", "SSH_AUTH_SOCK", "DXVK_CONFIG_FILE"] {
            prop_assert!(p.env.get(n).is_none(), "{n} present");
        }
        prop_assert!(p.env.clear);
        prop_assert_eq!(p.env.get("UMU_RUNTIME_UPDATE").is_some(), case.kind != zaun_core::model::SessionKind::Provision);
    }
}
