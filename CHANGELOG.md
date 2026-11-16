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
- Optional background service (`systemctl --user enable --now easyplay.service`, or
  `programs.easyplay.backgroundService.enable` on NixOS and home-manager): EasyPlay stays
  loaded without a window, so a game opened from the file manager starts in about 20 ms
  and windows open without a cold start. Running games are not affected when it stops.
- The microphone is a permission of its own, off by default: games get sound through a
  sound server of their own that refuses to record. Games added before keep the
  microphone until it is switched off. Needs PipeWire; on a computer without it, sound
  without the microphone is silence, and the Protection tab says so.
- Launch options suggest Proton's options as a variable's name is typed, each with what it
  does: frame-rate caps, WoW64 mode, older Direct3D, upscaler upgrades on GE-Proton and
  CachyOS builds, and more.
- A game's Overview starts with what the game is: the engine it is made with (Unity,
  Unreal, Godot, GameMaker, Ren'Py, RPG Maker and more), what kind of program it is and its
  version, who made it, and whether it has anti-cheat or Steam DRM, which often stop games
  under Proton.
- Command line: `easyplay open|launch|stop|status|list|add|edit|remove|show|sandbox|export|import|doctor|runtimes|service`,
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
- `share/systemd/user/easyplay.service` is installed but must not be enabled by the
  package; its `ExecStart` names `/usr/bin/easyplay` (the tarball installer and the Nix
  package rewrite it).
- Packaging files: `packaging/arch` (`easyplay`, `easyplay-bin`), `packaging/rpm`,
  `packaging/deb`, `packaging/apparmor`, `packaging/tarball`, and the Nix flake.

[Unreleased]: https://github.com/easyplay/easyplay/commits/master
