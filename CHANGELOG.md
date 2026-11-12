# Changelog

All notable changes for users and for packagers. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[semantic versioning](https://semver.org/) (`0.x`: anything may change between minor
versions, RELEASE_STRATEGY §1). Release versions are derived from Conventional Commits by
the release workflow; this file is written by hand for each release (RELEASE_STRATEGY §6,
"Publish").

## [Unreleased]

### For users

- First public release. Open a Windows `.exe` from the file manager, choose *Offline*,
  *Online* or *Standard* protection, and play; every game runs through umu-launcher inside
  its own bubblewrap sandbox with a private Windows prefix.
- Game library with search, sorting, filters, tags, artwork (local, or opt-in from Steam's
  image servers) and play-time statistics that stay on the computer.
- Compatibility layers (Proton) and the Steam Linux Runtime are installed and kept up to
  date through umu, with integrity checks and recovery.
- Installer flow for `setup.exe`-style installers; per-game folder permissions, devices and
  launch options; Doctor (`easyplay doctor`) for host problems.
- Command line: `easyplay open|launch|stop|status|list|add|edit|remove|show|sandbox|export|import|doctor|runtimes`,
  with `--json` output, a man page and bash/zsh/fish completions.

### For packagers

- Build with `scripts/build-release.sh` (`--help` for options). It builds with `--locked`,
  refuses the `dev-inspection` feature, checks the size budgets (`easyplay` <= 35 MB,
  `easyplay-init` <= 1 MB) and that the shim is static, and stages DESKTOP_INTEGRATION §10.
- `EASYPLAY_LIBEXEC_DIR` sets the compiled-in location of `easyplay-init` (default
  `/usr/libexec/easyplay`); the shim is also found next to the running binary.
- Run-time dependencies: `bubblewrap` (>= 0.8), `umu-launcher` (>= 1.4); through `dlopen`:
  Wayland and/or X11 client libraries, `libxkbcommon`, Vulkan loader or OpenGL, `libudev`.
  SQLite is compiled in.
- The Debian/Ubuntu package installs `/etc/apparmor.d/easyplay` (user namespaces for
  EasyPlay, bwrap and pressure-vessel's srt-bwrap on systems with
  `kernel.apparmor_restrict_unprivileged_userns=1`).
- Packaging files: `packaging/arch` (`easyplay`, `easyplay-bin`), `packaging/rpm`,
  `packaging/deb`, `packaging/apparmor`, `packaging/tarball`, and the Nix flake.

[Unreleased]: https://github.com/easyplay/easyplay/commits/master
