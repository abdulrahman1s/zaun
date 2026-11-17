# Changelog

All notable changes for users and for packagers. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[semantic versioning](https://semver.org/) (`0.x`: anything may change between minor
versions, RELEASE_STRATEGY §1). Release versions are derived from Conventional Commits by
the release workflow; this file is written by hand for each release (RELEASE_STRATEGY §6,
"Publish").

## [Unreleased]

### For users

- The interface is available in Arabic (Settings → General → Language: *Same as system*, *English*, *العربية*). Text is set right to left; the layout of columns and bars is not mirrored yet. Texts that come from outside the interface (some error messages and system-check results) stay in English.
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
- On Wayland desktops whose compositor supports security contexts, a game gets a
  restricted connection to the desktop, and the compositor decides what to keep from it; on
  niri (the only one tried so far) that is screen capture, clipboard managers, fake keyboard
  and mouse input and windows drawn over everything. `easyplay doctor` says whether this
  desktop does. Games drawing through X11 (Proton's default) are still exposed to other X11
  windows. A game that uses Proton's Wayland driver and misbehaves this way can be switched
  to X11 in its options. *Copy as shell command* refuses to run for such games, since a
  script cannot recreate the restricted connection.
- A new look. A game without artwork gets a poster: its title set large in a condensed
  face on one of ten colour pairs, the same for as long as the game keeps its name. The
  library is a wall of covers under one top bar (no sidebar), with scopes, filters and the
  sort on one line of words; a small library shows its few games larger, in the middle of
  the window. A game's page keeps its cover on the left with thirty days of play under it.
  New dark, light and high-contrast themes; the accent colour marks only Play, selection
  and focus.
- Movement explains what changed: covers slide to their new places when the library is
  filtered or sorted, and the cover you open flies to the game's page. With *Reduce motion*
  on, everything is a short fade.
- Command line: `easyplay open|launch|stop|status|list|add|edit|remove|show|sandbox|export|import|doctor|runtimes|service`,
  with `--json` output, a man page and bash/zsh/fish completions.
- Optional "Show my Steam games" (Settings → General): games installed with Steam appear
  in the library with their play time and update state from Steam; Play and "Update in
  Steam" hand over to Steam; programs such as mod installers can be run in a Steam game's
  Windows storage inside EasyPlay's protection, without internet. Off by default; nothing
  leaves the computer. New command `easyplay steam scan|on|off|update|open`.

### For packagers

- Build with `scripts/build-release.sh` (`--help` for options). It builds with `--locked`,
  refuses the `dev-inspection` feature, checks the size budgets (`easyplay` <= 35 MB,
  `easyplay-init` <= 1 MB) and that the shim is static, and stages DESKTOP_INTEGRATION §10.
- `EASYPLAY_LIBEXEC_DIR` sets the compiled-in location of `easyplay-init` (default
  `/usr/libexec/easyplay`); the shim is also found next to the running binary.
- The `wayland-security-context` feature gives games a restricted Wayland socket
  (ADR-0029). It is a default feature of `easyplay-core`, like `seccomp`, and needs no new
  dependency (`wayland-client` and `wayland-protocols` were already built for the window).
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
- Library schema version 2 (migration `0002_steam`, adds two tables, changes no data).

[Unreleased]: https://github.com/easyplay/easyplay/commits/master
