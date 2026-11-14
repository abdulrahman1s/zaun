# Contributing to EasyPlay

Thank you for helping. EasyPlay is built from a written specification: read
[docs/README.md](docs/README.md) first (reading order, the design in one page, open
questions). If the code and the documents disagree, the documents win until changed by a
decision record in [docs/adr](docs/adr/README.md). Work is organised as tasks in
[docs/tasks](docs/tasks/README.md), with progress in
[docs/tasks/STATUS.md](docs/tasks/STATUS.md).

## Development environment

The toolchain (Rust from `rust-toolchain.toml`, with the musl target), `bubblewrap`,
`umu-launcher`, `cargo-deny`, `cargo-nextest`, `cargo-insta`, `desktop-file-validate` and
`appstreamcli` come from the Nix dev shell:

```sh
nix develop                     # or `direnv allow` with the provided .envrc
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check
```

Without entering the shell, prefix any command: `nix develop -c cargo test --workspace`.
Without Nix: rustup (the toolchain file selects the version and targets), a C compiler,
`pkg-config`, the Wayland/X11/xkbcommon/udev development headers, `musl-tools` on Debian.

| Task | Command |
|---|---|
| End-to-end tests that need real `bwrap` and user namespaces | `EASYPLAY_E2E=1 cargo test --workspace` |
| Static init shim, as released | `cargo build --profile init-release -p easyplay-init --target x86_64-unknown-linux-musl` |
| Compiled-in shim location | set `EASYPLAY_LIBEXEC_DIR` at build time (default `/usr/libexec/easyplay`) |
| Update snapshots | `cargo insta review` |
| Release build, checks and staged install tree | `scripts/build-release.sh` (`--tarball` for the release tarball) |

## Checking a window on a real desktop

Snapshot tests render the layout at a size the test chooses; they cannot show what size the
compositor gives the window. A change to how a window opens (quick launch, onboarding, the
library) is checked by running the binary on a real compositor, preferably a tiling one
(ADR-0021):

```sh
R=/run/user/$(id -u)/ep-test         # short: the instance socket path must fit in 108 bytes
S=$(mktemp -d)                        # throwaway library, settings and logs
mkdir -p -m 700 "$R" "$S"/{cfg,data,cache,state}
ln -s "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" "$R/"
XDG_RUNTIME_DIR=$R EASYPLAY_CONFIG_DIR=$S/cfg EASYPLAY_DATA_DIR=$S/data \
  EASYPLAY_CACHE_DIR=$S/cache EASYPLAY_STATE_DIR=$S/state \
  target/debug/easyplay [open path/to/Game.exe] &
niri msg --json windows               # is_floating and layout.window_size for that pid
```

- The private `XDG_RUNTIME_DIR` keeps the instance from handing over to an EasyPlay that is
  already running; empty data folders start in onboarding. A runtime path that is too long
  fails with a misleading `EP-FS-003` before any window opens.
- Read the size from the compositor (`niri msg`, `swaymsg -t get_tree`, `hyprctl
  clients`), not from a screenshot. `WAYLAND_DEBUG=1` shows the `configure` events and the
  `set_min_size` / `set_max_size` requests, which is how ADR-0021 was found.
- After closing a window, check `$S/state/crash` is empty: a crash during shutdown only
  shows as a banner on the next start.
- Sound checks fail in such an instance (no PipeWire socket in the private runtime
  directory); that is expected.

## Rules enforced by the build

- No `unsafe` outside `easyplay-init`, `core::session::sys` and `core::sandbox::fd`.
- Time only through `easyplay_core::clock::Clock`; files in core only through
  `easyplay_core::fsx::Fs`; processes only through `host::runner` and `session::spawn`
  (`clippy.toml`).
- Every user-visible string follows the copy rules of USER_EXPERIENCE and SECURITY_MODEL §10
  (no jargon in normal mode; no claims of perfect safety).

## Commits and releases

Commits follow [Conventional Commits](https://www.conventionalcommits.org/): `feat:`,
`fix:`, `perf:`, `docs:`, `test:`, `build:`, `ci:`, `refactor:`, `chore:`, with an optional
scope (`fix(sandbox): …`). The release workflow (`.github/workflows/build-release.yml`)
derives the next version from them on the default branch: `feat` → minor, `fix`/`perf` →
patch, `!` or `BREAKING CHANGE:` → major (minor while `0.x`). `[skip release]` in the
commit message skips a release. Add user-visible changes to [CHANGELOG.md](CHANGELOG.md)
under *Unreleased*.

## Packaging

| Format | Files | Notes |
|---|---|---|
| All | `scripts/build-release.sh` | Builds, checks and stages; every package starts from it. |
| Tarball | `packaging/tarball/install.sh`, `uninstall.sh` | Relocatable; `install.sh` records what it adds in `PREFIX/libexec/easyplay/install-manifest`. |
| Arch | `packaging/arch/PKGBUILD`, `packaging/arch/easyplay-bin/PKGBUILD` | `/usr/lib/easyplay` for the shim. |
| Fedora | `packaging/rpm/easyplay.spec` | Source build with vendored crates (COPR), or `--with prebuilt` from the tarball. |
| Debian/Ubuntu | `packaging/deb/build-deb.sh` | Builds the `.deb` from a staged tree or the tarball; ships the AppArmor profile. |
| AppArmor | `packaging/apparmor/easyplay` | Check syntax with `apparmor_parser -Q -K`; real validation needs Ubuntu (below). |
| Nix | `flake.nix`, `nix/` | `nix build`, `nix flake check`. |

What CI cannot check and must be done by hand on real systems before a release is listed in
[docs/RELEASE_STRATEGY.md §6](docs/RELEASE_STRATEGY.md). For the AppArmor profile (V-004),
on each supported Ubuntu release with `kernel.apparmor_restrict_unprivileged_userns=1`:
install the `.deb`, confirm `aa-status` lists `easyplay`, `easyplay//bwrap` and
`easyplay//srt-bwrap`, run `easyplay doctor` (the nested user-namespace check must pass), and
launch a real game in each protection preset; then repeat with the profile removed to
confirm the Doctor shows `EP-HOST-004` with the AppArmor guidance.

## Reporting problems

Bugs and ideas: [issues](https://github.com/easyplay/easyplay/issues). Security problems:
privately, see [SECURITY.md](SECURITY.md).
