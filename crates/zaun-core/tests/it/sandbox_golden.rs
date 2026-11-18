//! Golden tests of the sandbox planner (SANDBOX-030, TESTING_STRATEGY §4.1).
//!
//! Snapshot 1 is the worked example of BUBBLEWRAP_SANDBOX §6, compared line by line with the
//! listing in the document itself. The curated matrix below covers every value of every
//! dimension and the interacting pairs listed in `tests/golden/README.md`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use zaun_core::model::{
    BasePreset, GameDirAccess, NetworkAccess, RuntimeIsolationMode, SandboxPolicy, SessionKind,
};
use zaun_core::sandbox::plan::FdContent;
use zaun_core::sandbox::presets::preset_policy;
use zaun_core::sandbox::{bwrap, offline, plan, render};
use zaun_testkit::fixtures::hosts::{self, SandboxWorld};
use zaun_testkit::golden::normalize_paths;

/// `insta::assert_snapshot!` against `tests/snapshots/sandbox_golden__<name>.snap`. This file
/// is a module of the `it` test binary, which would otherwise name the files after the
/// binary (`it__sandbox_golden__…`) and look for them in `tests/it/snapshots/`.
macro_rules! assert_golden {
    ($name:expr, $value:expr $(,)?) => {{
        let mut settings = zaun_testkit::insta::Settings::clone_current();
        settings.set_snapshot_path("../snapshots");
        settings.set_prepend_module_to_snapshot(false);
        let name = format!("sandbox_golden__{}", $name);
        settings.bind(|| zaun_testkit::insta::assert_snapshot!(name, $value));
    }};
}

fn normalized(w: &SandboxWorld, text: &str) -> String {
    let reps = w.replacements();
    let pairs: Vec<(&Path, &str)> = reps.iter().map(|(p, r)| (p.as_path(), *r)).collect();
    normalize_paths(text, &pairs)
}

/// The two `text` blocks of §6: the option listing and the command line; `None` in a
/// checkout without `docs/` (kept out of the public repository).
fn worked_example_from_doc() -> Option<(String, String)> {
    use zaun_core::fsx::{Fs, RealFs};
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/BUBBLEWRAP_SANDBOX.md");
    let doc = String::from_utf8(RealFs.read(&path, 1 << 20).ok()?).unwrap();
    let section = &doc[doc.find("## 6. Worked example").unwrap()..];
    let mut blocks = section
        .split("```text\n")
        .skip(1)
        .map(|b| b.split("```").next().unwrap().to_owned());
    Some((
        blocks.next().unwrap(),
        blocks.next().unwrap().trim().to_owned(),
    ))
}

#[test]
fn worked_example_matches_bubblewrap_sandbox_section_6() {
    let w = SandboxWorld::worked_example();
    let p = plan(&offline(), &w.ctx(), &w.host).expect("plan");
    let r = render(&p).unwrap();
    let Some((listing, command)) = worked_example_from_doc() else {
        eprintln!("skipped: docs/BUBBLEWRAP_SANDBOX.md is not in this checkout");
        return;
    };
    let ours = normalized(&w, &bwrap::display_lines(&r));
    if ours != listing {
        let a: Vec<&str> = ours.lines().collect();
        let b: Vec<&str> = listing.lines().collect();
        for i in 0..a.len().max(b.len()) {
            let (x, y) = (
                a.get(i).copied().unwrap_or("<none>"),
                b.get(i).copied().unwrap_or("<none>"),
            );
            if x != y {
                panic!(
                    "line {}: planner `{x}` vs document `{y}`\n\nfull output:\n{ours}",
                    i + 1
                );
            }
        }
    }
    assert_eq!(normalized(&w, &bwrap::display_command(&r)), command);
    assert_golden!("01_worked_example", snapshot(&w, &offline()));
}

/// Full snapshot text: options, command, descriptors, host actions, notes, issues.
fn snapshot(w: &SandboxWorld, policy: &SandboxPolicy) -> String {
    let mut out = String::new();
    match plan(policy, &w.ctx(), &w.host) {
        Ok(p) => {
            let r = render(&p).unwrap();
            out.push_str(&bwrap::display_lines(&r));
            out.push_str(&bwrap::display_command(&r));
            out.push_str("\n\n# descriptors\n");
            for fd in &r.fds {
                let target = fd
                    .target
                    .as_ref()
                    .map(|t| t.display().to_string())
                    .unwrap_or_default();
                let content = match &fd.content {
                    FdContent::Bytes(b) => String::from_utf8_lossy(b).escape_debug().to_string(),
                    FdContent::SeccompProgram => "<seccomp program>".into(),
                };
                out.push_str(&format!("{} {} {}\n", fd.fd, target, content));
            }
            out.push_str("\n# host actions\n");
            for a in &p.host_actions {
                out.push_str(&format!("{}\n", serde_json::to_string(a).unwrap()));
            }
            out.push_str("\n# notes\n");
            for n in &p.notes {
                out.push_str(&format!("{}: {}\n", n.key, n.text));
            }
            out.push_str("\n# issues\n");
            for i in &p.issues {
                out.push_str(&format!(
                    "{} {:?} {}\n",
                    i.code.as_str(),
                    i.severity,
                    i.message
                ));
            }
        }
        Err(issues) => {
            out.push_str("# planning failed\n");
            for i in &issues {
                out.push_str(&format!(
                    "{} {:?} {} {}\n",
                    i.code.as_str(),
                    i.severity,
                    i.path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    i.message
                ));
            }
        }
    }
    normalized(w, &out)
}

/// One matrix cell.
struct Cell {
    host: &'static str,
    preset: BasePreset,
    kind: Kind,
    isolation: RuntimeIsolationMode,
    game_dir: GameDirAccess,
}

#[derive(Clone, Copy)]
enum Kind {
    Game,
    Installer,
    Tool,
    PrepareNetwork,
    PreparePolicy,
    Provision,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Game => "game",
            Kind::Installer => "installer",
            Kind::Tool => "tool",
            Kind::PrepareNetwork => "prepare_network",
            Kind::PreparePolicy => "prepare_policy",
            Kind::Provision => "provision",
        }
    }
}

fn cell_name(c: &Cell) -> String {
    format!(
        "{}__{}__{}__{}__{}",
        c.host,
        c.preset.as_str(),
        c.kind.name(),
        match c.isolation {
            RuntimeIsolationMode::Isolated => "isolated",
            RuntimeIsolationMode::Shared => "shared",
        },
        match c.game_dir {
            GameDirAccess::FileOnly => "file_only",
            GameDirAccess::ReadOnly => "read_only",
            GameDirAccess::ReadWrite => "read_write",
            GameDirAccess::Protected => "protected",
        }
    )
}

fn run_cell(c: &Cell) -> String {
    let mut w = SandboxWorld::new(hosts::by_name(c.host));
    w.runtime.isolation = c.isolation;
    if c.game_dir == GameDirAccess::FileOnly {
        w.loose_exe();
    }
    let (kind, net) = match c.kind {
        Kind::Game => (SessionKind::Game, None),
        Kind::Installer => (SessionKind::Installer, None),
        Kind::Tool => (SessionKind::Tool, None),
        Kind::PrepareNetwork => (SessionKind::Prepare, Some(NetworkAccess::Full)),
        Kind::PreparePolicy => (SessionKind::Prepare, None),
        Kind::Provision => (SessionKind::Provision, None),
    };
    w.set_kind(kind, net);
    let mut policy = preset_policy(c.preset, &w.host);
    policy.game_dir = c.game_dir;
    snapshot(&w, &policy)
}

const fn cell(
    host: &'static str,
    preset: BasePreset,
    kind: Kind,
    isolation: RuntimeIsolationMode,
    game_dir: GameDirAccess,
) -> Cell {
    Cell {
        host,
        preset,
        kind,
        isolation,
        game_dir,
    }
}

/// The curated matrix. See `tests/golden/README.md` for what each group covers.
const MATRIX: &[Cell] = matrix::MATRIX;

mod matrix {
    use super::{Cell, Kind, cell};
    use Kind::{Game, Installer, PrepareNetwork, PreparePolicy, Provision, Tool};
    use zaun_core::model::BasePreset::{Offline, Online, Standard};
    use zaun_core::model::GameDirAccess::{FileOnly, Protected, ReadOnly, ReadWrite};
    use zaun_core::model::RuntimeIsolationMode::{Isolated, Shared};

    pub(super) const MATRIX: &[Cell] = &[
        // arch_mesa: every preset × game-folder mode, both isolation modes.
        cell("arch_mesa", Offline, Game, Isolated, ReadOnly),
        cell("arch_mesa", Offline, Game, Isolated, Protected),
        cell("arch_mesa", Offline, Game, Isolated, FileOnly),
        cell("arch_mesa", Offline, Game, Shared, ReadWrite),
        cell("arch_mesa", Online, Game, Isolated, ReadWrite),
        cell("arch_mesa", Online, Game, Shared, ReadOnly),
        cell("arch_mesa", Online, Game, Isolated, FileOnly),
        cell("arch_mesa", Standard, Game, Isolated, ReadWrite),
        cell("arch_mesa", Standard, Game, Shared, Protected),
        cell("arch_mesa", Standard, Game, Isolated, FileOnly),
        // Session kinds on arch_mesa.
        cell("arch_mesa", Offline, Installer, Isolated, FileOnly),
        cell("arch_mesa", Online, Tool, Isolated, ReadWrite),
        cell("arch_mesa", Offline, PrepareNetwork, Isolated, ReadWrite),
        cell("arch_mesa", Offline, PreparePolicy, Isolated, ReadWrite),
        cell("arch_mesa", Online, PreparePolicy, Shared, ReadWrite),
        cell("arch_mesa", Offline, Provision, Isolated, ReadWrite),
        // fedora_nvidia: NVIDIA nodes, pki, two interfaces.
        cell("fedora_nvidia", Offline, Game, Isolated, ReadWrite),
        cell("fedora_nvidia", Online, Game, Isolated, ReadWrite),
        cell("fedora_nvidia", Standard, Game, Shared, ReadWrite),
        cell("fedora_nvidia", Offline, Game, Isolated, Protected),
        cell(
            "fedora_nvidia",
            Offline,
            PrepareNetwork,
            Isolated,
            ReadWrite,
        ),
        cell("fedora_nvidia", Offline, Provision, Isolated, ReadWrite),
        cell("fedora_nvidia", Online, Installer, Isolated, FileOnly),
        // ubuntu_lts_apparmor: Debian-family layout, non-reflink (Shared under Auto).
        cell("ubuntu_lts_apparmor", Offline, Game, Shared, ReadWrite),
        cell("ubuntu_lts_apparmor", Online, Game, Shared, ReadOnly),
        cell("ubuntu_lts_apparmor", Standard, Game, Shared, ReadWrite),
        cell("ubuntu_lts_apparmor", Offline, Game, Isolated, ReadWrite),
        cell(
            "ubuntu_lts_apparmor",
            Offline,
            PreparePolicy,
            Shared,
            ReadWrite,
        ),
        cell("ubuntu_lts_apparmor", Offline, Provision, Shared, ReadWrite),
        cell("ubuntu_lts_apparmor", Standard, Tool, Shared, FileOnly),
        // debian_stable_old_bwrap: no overlay, real /bin, extra library root.
        cell("debian_stable_old_bwrap", Offline, Game, Shared, ReadWrite),
        cell("debian_stable_old_bwrap", Offline, Game, Shared, Protected),
        cell("debian_stable_old_bwrap", Online, Game, Shared, ReadOnly),
        cell(
            "debian_stable_old_bwrap",
            Standard,
            Game,
            Isolated,
            ReadWrite,
        ),
        cell(
            "debian_stable_old_bwrap",
            Offline,
            PrepareNetwork,
            Shared,
            ReadWrite,
        ),
        cell(
            "debian_stable_old_bwrap",
            Online,
            Installer,
            Shared,
            FileOnly,
        ),
        // nixos: Nix roots, /etc via /etc/static, layout pass-through variables.
        cell("nixos", Offline, Game, Isolated, ReadWrite),
        cell("nixos", Online, Game, Isolated, ReadWrite),
        cell("nixos", Standard, Game, Shared, ReadWrite),
        cell("nixos", Offline, Game, Isolated, Protected),
        cell("nixos", Offline, PrepareNetwork, Isolated, ReadWrite),
        cell("nixos", Offline, PreparePolicy, Isolated, ReadWrite),
        cell("nixos", Offline, Provision, Isolated, ReadWrite),
        cell("nixos", Online, Installer, Isolated, FileOnly),
        // x11_only: X11 session, authority in the real home, no Wayland.
        cell("x11_only", Offline, Game, Isolated, ReadWrite),
        cell("x11_only", Online, Game, Isolated, ReadWrite),
        cell("x11_only", Standard, Game, Shared, ReadOnly),
        cell("x11_only", Offline, Provision, Isolated, ReadWrite),
        cell("x11_only", Offline, Game, Isolated, FileOnly),
        // no_pipewire: PulseAudio only.
        cell("no_pipewire", Offline, Game, Isolated, ReadWrite),
        cell("no_pipewire", Online, Game, Shared, ReadWrite),
        cell("no_pipewire", Standard, Game, Isolated, Protected),
        cell("no_pipewire", Offline, PreparePolicy, Isolated, ReadWrite),
        // input_group_user: keyboards readable, AllInput downgraded.
        cell("input_group_user", Offline, Game, Isolated, ReadWrite),
        cell("input_group_user", Online, Game, Isolated, ReadWrite),
        cell("input_group_user", Standard, Game, Isolated, ReadWrite),
        cell("input_group_user", Standard, Game, Shared, FileOnly),
        cell("input_group_user", Offline, Provision, Isolated, ReadWrite),
        // no_userns: planning is unaffected (the probe blocks the launch).
        cell("no_userns", Offline, Game, Shared, ReadWrite),
    ];
}

#[test]
fn golden_matrix() {
    let mut names = std::collections::BTreeSet::new();
    for (i, c) in MATRIX.iter().enumerate() {
        let name = format!("{:02}_{}", i + 2, cell_name(c));
        assert!(names.insert(cell_name(c)), "duplicate cell {name}");
        assert_golden!(name, run_cell(c));
    }
}

#[test]
fn standard_input_group_downgrades_all_input_with_issue() {
    let w = SandboxWorld::new(hosts::input_group_user());
    let mut policy = preset_policy(BasePreset::Standard, &w.host);
    policy.devices.controllers = zaun_core::model::ControllerAccess::AllInput;
    let p = plan(&policy, &w.ctx(), &w.host).unwrap();
    assert!(p.issues.iter().any(|i| i.code.as_str() == "SEC-INPUT-001"));
    assert!(p.mount_at(Path::new("/dev/input")).is_none());
    assert_golden!("zz_input_group_all_input_requested", snapshot(&w, &policy));
}
