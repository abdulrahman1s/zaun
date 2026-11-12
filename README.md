# EasyPlay — specification repository

EasyPlay is a Linux desktop application, written in Rust, that lets a person double-click a
Windows `.exe` (usually a game), answer a few plain-language questions once, and play.
Every Windows program runs through [`umu-run`](https://github.com/Open-Wine-Components/umu-launcher)
inside a [Bubblewrap](https://github.com/containers/bubblewrap) sandbox that EasyPlay builds
for that one game. The interface is built with [`egui`](https://github.com/emilk/egui).

This repository holds the authoritative blueprint (`docs/`) and the implementation that
follows it file by file (`crates/`, in progress: see [docs/tasks/STATUS.md](docs/tasks/STATUS.md)).
If the code and these documents ever disagree, the documents win until they are changed by
an explicit decision record.

| | |
|---|---|
| Product name | EasyPlay (working title) |
| Binary | `easyplay` (+ helper `easyplay-init`) |
| Application ID | `io.github.easyplay.EasyPlay` (placeholder, see [Open questions](#open-questions)) |
| Language | Rust, edition 2024, MSRV 1.95 |
| UI toolkit | `egui` / `eframe` 0.37 |
| Launch engine | `umu-launcher` ≥ 1.4 (verified against 1.4.4) |
| Sandbox | `bubblewrap` ≥ 0.8 (verified against 0.12.0), unprivileged user namespaces required |
| Baseline date | 2026-11-12 |

## How to read this repository

Read in this order the first time. Every later document assumes the ones above it.

| # | Document | Answers |
|---|---|---|
| 1 | [docs/PRODUCT_VISION.md](docs/PRODUCT_VISION.md) | What are we building, for whom, and what is out of scope? |
| 2 | [docs/TECHNICAL_RESEARCH.md](docs/TECHNICAL_RESEARCH.md) | What is *verified* about egui, umu and bwrap, what is assumed, what must be tested? |
| 3 | [docs/GLOSSARY.md](docs/GLOSSARY.md) | What does each term mean in this project? |
| 4 | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Processes, threads, crates, data flow. |
| 5 | [docs/MODULES.md](docs/MODULES.md) | Every crate, module and file: responsibility, ownership, interfaces, non-responsibilities. |
| 6 | [docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md) | Trust boundaries, permission model, presets, invariants. |
| 7 | [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) | A malicious `.exe`: attack by attack, with mitigations and residual risk. |
| 8 | [docs/BUBBLEWRAP_SANDBOX.md](docs/BUBBLEWRAP_SANDBOX.md) | Exactly how the `bwrap` command line is constructed. |
| 9 | [docs/UMU_INTEGRATION.md](docs/UMU_INTEGRATION.md) | Exactly how `umu-run` is invoked and what it does. |
| 10 | [docs/RUNTIME_MANAGEMENT.md](docs/RUNTIME_MANAGEMENT.md) | Proton and Steam Linux Runtime: store, download, integrity, updates. |
| 11 | [docs/PROCESS_LIFECYCLE.md](docs/PROCESS_LIFECYCLE.md) | Supervisor, init shim, process tracking, stopping, crash recovery. |
| 12 | [docs/LOGIC_SPECS.md](docs/LOGIC_SPECS.md) | State machines for every major feature. |
| 13 | [docs/GAME_IDENTITY.md](docs/GAME_IDENTITY.md) | How an `.exe` is recognised, matched, re-linked. |
| 14 | [docs/GAME_LIBRARY.md](docs/GAME_LIBRARY.md) | Library behaviour: search, sort, filter, views, tags. |
| 15 | [docs/DATA_MODEL.md](docs/DATA_MODEL.md) | Rust domain types and the SQLite schema. |
| 16 | [docs/PERSISTENCE.md](docs/PERSISTENCE.md) | What is stored where and why; migrations; backups. |
| 17 | [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | Global preferences, per-game settings, environment variables, CLI. |
| 18 | [docs/STATISTICS.md](docs/STATISTICS.md) | Sessions, playtime, aggregates, edge cases. |
| 19 | [docs/USER_EXPERIENCE.md](docs/USER_EXPERIENCE.md) | UX principles, navigation, states, feedback, input. |
| 20 | [docs/UI_DESIGN_SYSTEM.md](docs/UI_DESIGN_SYSTEM.md) | Visual identity: tokens, type, spacing, motion, components. |
| 21 | [docs/USER_FLOWS.md](docs/USER_FLOWS.md) | Step-by-step flows, including every branch. |
| 22 | [docs/SCREEN_SPECS.md](docs/SCREEN_SPECS.md), [docs/SCREEN_SPECS_2.md](docs/SCREEN_SPECS_2.md) | Every screen: layout, states, keyboard, accessibility, egui mapping. |
| 23 | [docs/DESKTOP_INTEGRATION.md](docs/DESKTOP_INTEGRATION.md) | `.desktop`, MIME, single instance, CLI, shortcuts. |
| 24 | [docs/ERROR_HANDLING.md](docs/ERROR_HANDLING.md) | Error taxonomy, codes, messages, recovery. |
| 25 | [docs/LOGGING.md](docs/LOGGING.md) | What is logged, where, how long, redaction. |
| 26 | [docs/DEVELOPER_MODE.md](docs/DEVELOPER_MODE.md) | The advanced inspector for launches and sandboxes. |
| 27 | [docs/PRIVACY.md](docs/PRIVACY.md) | Data collected (none leaves the machine), network endpoints. |
| 28 | [docs/ACCESSIBILITY.md](docs/ACCESSIBILITY.md) | Keyboard, screen reader, contrast, motion, controller. |
| 29 | [docs/PERFORMANCE.md](docs/PERFORMANCE.md) | Budgets, threading rules, caching, virtualization. |
| 30 | [docs/TESTING_STRATEGY.md](docs/TESTING_STRATEGY.md) | Test layers, fixtures, fakes, security tests. |
| 31 | [docs/RELEASE_STRATEGY.md](docs/RELEASE_STRATEGY.md) | Packaging, versioning, channels, compatibility policy. |
| 32 | [docs/ROADMAP.md](docs/ROADMAP.md) | What ships in 1.0 and what is deliberately later. |
| 33 | [docs/IMPLEMENTATION_ORDER.md](docs/IMPLEMENTATION_ORDER.md) | Milestones in dependency order with acceptance criteria. |
| 34 | [docs/tasks/README.md](docs/tasks/README.md) | The granular task list the coding agent executes. |
| 35 | [docs/adr/README.md](docs/adr/README.md) | Decision records for every major choice. |

## The design in one page

**What happens when a person opens `Game.exe`:**

1. The desktop hands the path to `easyplay open <path>` (MIME association, drag-and-drop,
   file picker or CLI all converge on the same entry point).
2. EasyPlay fingerprints the file and looks it up ([GAME_IDENTITY](docs/GAME_IDENTITY.md)).
   Known game → launch with its saved profile. Unknown → show the first-run setup dialog.
3. Setup asks for a name, shows detected artwork, and asks one security question in plain
   language: *Offline*, *Online*, or *Standard*. Everything else has a safe default and sits
   behind "More options".
4. The launch pipeline resolves a Proton build, prepares the prefix, computes a **sandbox
   plan** (a pure data structure) from the game's **sandbox policy**, and hands a complete
   **launch plan** to a detached **supervisor** process.
5. The supervisor runs `bwrap … -- easyplay-init -- umu-run Game.exe`. `umu-run` starts the
   Steam Linux Runtime container and Proton inside our sandbox.
6. The supervisor records the session in SQLite, writes a heartbeat, captures logs, and
   closes the session when the sandbox is empty. The GUI may crash or be closed; the game and
   its statistics are unaffected.

**The three load-bearing ideas:**

- **The outer `bwrap` is the only security boundary.** `umu-run`, pressure-vessel, Proton and
  the game all sit inside it. Nothing inside is trusted to restrict anything else
  ([SECURITY_MODEL §2](docs/SECURITY_MODEL.md)).
- **Policy → Plan → Arguments.** A user-editable `SandboxPolicy` is resolved against the host
  into an explicit `SandboxPlan` (every mount, variable and namespace), which is rendered to
  `bwrap` arguments by a pure function. The same plan generates the "What can this game
  access?" text shown to the user, so the explanation cannot drift from reality.
- **One sandbox instance is one session.** Statistics count sandbox lifetimes, never
  processes, so launchers, patchers and helper processes can never inflate play counts
  ([STATISTICS](docs/STATISTICS.md), [PROCESS_LIFECYCLE](docs/PROCESS_LIFECYCLE.md)).

## NixOS

The flake provides the package, a NixOS module, a home-manager module and an overlay
([RELEASE_STRATEGY §2.4](docs/RELEASE_STRATEGY.md)). The package already puts `bubblewrap`
and `umu-launcher` on EasyPlay's `PATH`; nothing else needs installing.

```nix
# flake.nix
{
  inputs.easyplay.url = "github:easyplay/easyplay";
  # Optional: build against your own nixpkgs (the Rust toolchain still comes from
  # rust-toolchain.toml through rust-overlay).
  # inputs.easyplay.inputs.nixpkgs.follows = "nixpkgs";

  outputs = { nixpkgs, easyplay, ... }: {
    nixosConfigurations.my-host = nixpkgs.lib.nixosSystem {
      modules = [ ./configuration.nix easyplay.nixosModules.default ];
    };
  };
}
```

```nix
# configuration.nix
{
  programs.easyplay.enable = true;
}
```

| Option | Default | Effect |
|---|---|---|
| `programs.easyplay.enable` | `false` | Installs EasyPlay system-wide. |
| `programs.easyplay.package` | this flake's package | The package to install (`pkgs.easyplay` with `easyplay.overlays.default`). |
| `programs.easyplay.enable32BitGraphics` | `true` | `hardware.graphics.enable` and `enable32Bit` (as `mkDefault`); most games need 32-bit drivers. |
| `programs.easyplay.associateWindowsExecutables` | `true` | Adds EasyPlay to "Open With" for `.exe`/`.msi` (`xdg.mime.addedAssociations`). Never sets it as the default. |
| `programs.easyplay.allowUnprivilegedUsernsClone` | `false` | Sets `kernel.unprivileged_userns_clone = 1`. Only for hardened kernels that disable unprivileged user namespaces; stock kernels do not need it. |

Evaluation fails if `security.allowUserNamespaces = false` or `user.max_user_namespaces` is 0:
bubblewrap cannot work without user namespaces.

home-manager (any distribution with Nix; on NixOS prefer the system module, which also
enables 32-bit graphics):

```nix
{
  imports = [ easyplay.homeManagerModules.default ];
  programs.easyplay = {
    enable = true;
    # Optional, rendered to ~/.config/easyplay/config.toml (docs/CONFIGURATION.md §2).
    # Leave unset to let EasyPlay manage the file itself.
    settings.appearance.theme = "dark";
  };
}
```

Without modules: `nix run github:easyplay/easyplay`, or `nix profile install
github:easyplay/easyplay`.

## Status of claims in these documents

Every technical claim about a third-party project is tagged:

| Tag | Meaning |
|---|---|
| **[V]** Verified | Read in the project's source or documentation at the stated version, or reproduced on a test host. The evidence is in [TECHNICAL_RESEARCH](docs/TECHNICAL_RESEARCH.md). |
| **[R]** Recommendation | An architectural decision made here. Recorded in an ADR. |
| **[A]** Assumption | Believed true but not confirmed. Each has an ID `V-nnn` and a validation task that must pass before the dependent feature is considered done. |

Sandboxing Windows software under Wine is **risk reduction, not a guarantee**. The documents
say so wherever a user could be misled, and the UI must say so too
([SECURITY_MODEL §9](docs/SECURITY_MODEL.md)).

## Open questions

These need a human decision. None blocks implementation; each has a default.

| ID | Question | Default used by the spec |
|---|---|---|
| OQ-1 | Final product name and reverse-DNS application ID. | `EasyPlay`, `io.github.easyplay.EasyPlay`. Both live in one constants module. |
| OQ-2 | Project licence. `umu-launcher` is GPL-3.0; EasyPlay only executes it. | `GPL-3.0-or-later`. |
| OQ-3 | Should release builds bundle `umu-launcher` or require the distribution package? | Require it; detect and explain when missing. |
| OQ-4 | Online artwork lookup (e.g. SteamGridDB) needs an API key and sends game names to a third party. | Not in 1.0. Local files and embedded icons only. |
| OQ-5 | Ship an AppArmor profile for distributions that restrict unprivileged user namespaces? | Yes, in the `.deb`; validated by task `PKG-006`. |

## Repository layout (target, once implementation starts)

```text
easyplay/
├── README.md
├── Cargo.toml                  # workspace
├── rust-toolchain.toml         # pinned stable toolchain ≥ 1.95
├── deny.toml                   # cargo-deny: licences, advisories, duplicate versions
├── kittest.toml                # egui_kittest snapshot settings
├── crates/
│   ├── easyplay-core/          # everything that is not UI: domain, storage, sandbox, launch
│   ├── easyplay-ui/            # egui views, widgets, theme, view-models
│   ├── easyplay/               # the binary: CLI, single instance, supervisor entry, wiring
│   ├── easyplay-init/          # tiny in-sandbox init shim (static binary)
│   └── easyplay-testkit/       # dev-only fakes, fixtures, fake umu-run and bwrap
├── assets/                     # fonts, icon font, app icon, .desktop, metainfo
├── packaging/                  # arch, deb, rpm, nix, apparmor, tarball
├── spikes/                     # M0 validation experiments (scripts and results; not built)
├── fuzz/                       # cargo-fuzz targets for parsers of untrusted input
├── docs/                       # this specification
└── tests/                      # workspace-level end-to-end tests
```

The full tree with one line per file is in [docs/MODULES.md](docs/MODULES.md).

## Contributing

The toolchain (Rust 1.99.1 from `rust-toolchain.toml`, with the musl target), `bubblewrap`,
`umu-launcher`, `cargo-deny`, `cargo-nextest` and `cargo-insta` come from the Nix dev shell:

```sh
nix develop                     # or `direnv allow` with the provided .envrc
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check
```

Without entering the shell, prefix any command: `nix develop -c cargo test --workspace`.

| Task | Command |
|---|---|
| End-to-end tests that need real `bwrap` and user namespaces | `EASYPLAY_E2E=1 cargo test --workspace` |
| Static init shim, as released | `cargo build --release -p easyplay-init --target x86_64-unknown-linux-musl` (fully specified profile: `--profile init-release`) |
| Compiled-in shim location | set `EASYPLAY_LIBEXEC_DIR` at build time (default `/usr/libexec/easyplay`) |
| Update snapshots | `cargo insta review` |

Rules enforced by the build: no `unsafe` outside `easyplay-init`, `core::session::sys` and
`core::sandbox::fd`; time only through `easyplay_core::clock::Clock`; files in core only
through `easyplay_core::fsx::Fs`; processes only through `host::runner` and `session::spawn`
(`clippy.toml`). Work through the tasks in [docs/tasks](docs/tasks/README.md) and tick them in
[docs/tasks/STATUS.md](docs/tasks/STATUS.md); commits follow Conventional Commits (the release
workflow derives versions from them).
