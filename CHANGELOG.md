# Changelog

All notable changes for users and for packagers. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[semantic versioning](https://semver.org/) (`0.x`: anything may change between minor
versions, RELEASE_STRATEGY §1). Release versions are derived from Conventional Commits by
the release workflow; this file is written by hand for each release (RELEASE_STRATEGY §6,
"Publish").

## [Unreleased]

### For users

- Chat: a call opens as a stage under the conversation's header: a poster for each person
  on their own colour, a ring that grows with the voice that speaks, mute and stop-hearing
  marks, and a click shows one person big over the others. The bar says Calling…,
  Connecting… or Reconnecting… and, once the sound flows, how good the connection is (three
  bars, quiet while it is good). "Make smaller" turns it back into the slim tray, and the
  grip under it resizes it. The microphone's menu lists the microphones and speakers, the
  microphone volume with a live level, and background noise (the same settings as
  Settings › Chat; they apply from the next call). Sharing now says which
  window to pick before the desktop asks, and a friend's game can fill the screen (F). Your
  own unanswered call reads "Mira didn't answer" with Call again. Ctrl+Shift+M mutes and
  Ctrl+Shift+D stops hearing on any page during a call. Sharing chooses the picture
  (720p to 1440p, or as in the game) and 30 or 60 frames a second; while you share, a chip
  says what is sent ("1080p · 60") and changes it at once, and a friend watching sees what
  arrives.
- Chat: message times read like Discord's. A group's first message says "Today at 13:59",
  "Yesterday at 13:59" or "12/11/2026 21:40"; day dividers give the full date ("Tuesday,
  17 November 2026"); the chat list says the time today, "Yesterday", the weekday within six
  days, then the date. Pointing at any time shows the full date and time after half a
  second. "New" joins the day's divider instead of adding a second line, and events (calls,
  someone joining) now sit evenly between messages: 14 points between groups, none inside
  one. In Arabic, dates and clocks read as one left-to-right run, and left-to-right messages
  line up on the avatar's side.
- Settings › Chat: "Calls" is now "Voice and video", with the microphone volume
  (`chat.mic_volume`) and, under Advanced, your own relay servers (`chat.ice_servers`). A
  new Overlay group turns the in-game overlay on, records the key its hints show
  (`chat.overlay_key`; the desktop owns the binding) and places its call widget; on desktops
  that cannot show it, the group says so and its rows are dimmed.
- Chat: share the game you are playing with a friend, or watch theirs (ADR-0036), one to one:
  the game's window only (never the screen), chosen in the desktop's own window picker, sent
  as H.264 through the same end-to-end encrypted, relay-only connection as calls, and only
  while the friend watches. A friend's stream is ready to watch at once; it pauses while the
  game's window is hidden and ends with the game. No sound yet, not during a call with the
  same friend, not in groups. It needs a build with window capture (the `portal` feature);
  other builds hide it. The preview (`easyplay --preview`) shows a test picture on the watch
  stage. New messages `EP-CHT-014` to `EP-CHT-017` say why a share could not start.
- Shared games are encoded on the graphics card when it can (NVIDIA NVENC, or VA-API on AMD
  and Intel), lighter on the game; software encoding takes over if it cannot, even in the
  middle of a share (`chat.share_hw_encode`, on by default). The picture can be 720p, 1080p,
  1440p or as in the game, at 30 or 60 frames a second (`chat.share_quality`,
  `chat.share_fps`, default 60), and changed while sharing without stopping.
- When something was removed in the meantime (from another window or the command line), the
  message now says so: "This game is no longer in the library", "This tag
  no longer exists" or "This session is no longer there" (`EP-LIB-040`, `EP-LIB-041`,
  `EP-SES-040`). Before, it was a generic internal error or "This value cannot be used". The
  command line prints the same.
- Less work in every frame: looking up interface text no longer allocates (about 1.3 million
  allocations fewer in a two-minute session). The Statistics page keeps one result per time
  range instead of one per library change.
- Chat, a new page between Library and Statistics, off by default (Settings → Chat, or the
  switch on the page). Add a friend with a one-time link, shown as a QR code or copied with
  one button. Talk one to one or in groups, with replies, emoji reactions, edits, deletes,
  `@mentions` in groups, simple formatting, emoji pictures (Twemoji, built in) with `:`
  autocomplete, pictures and GIFs; ↑ in an empty message box edits your last message. Mute
  a conversation, block someone (they are not told), or compare security codes with a
  friend. The top bar shows how many messages are unread. Messages are end-to-end
  encrypted, with no account and no EasyPlay server: they travel through relays (by default
  the public servers at simplex.im and simplexonflux.com, or ones you list), which see your
  IP address and timing but never the content. The page says this before you turn it on.
  GIF search goes to GIPHY through Signal's content proxy, so GIPHY never sees your address;
  only your search words are sent, and friends receive the GIF as a file. Opening a link can
  first ask Google Safe Browsing about the site (*Check links before opening*, off by
  default; only the site's name is sent). Notifications show the sender's name only, unless
  you choose to show the text. Everything lives in an encrypted database in EasyPlay's data
  folder that games cannot reach. Voice calls, watching a friend play and an in-game overlay
  are planned and appear only in the preview (`easyplay --preview`) for now. Chat shows "not
  included in this build" when the package leaves it out.
- Chat: profile pictures. Friends', groups' and group members' pictures are read from their
  profiles (a bad or oversized one is skipped, never the friend). Your own picture, and a
  group's for its owner, is made from any picture file the way the official SimpleX apps make
  it: a square you choose, shrunk to fit the profile's limit (12 500 characters), with none
  of the file's metadata kept. It reaches friends end-to-end encrypted inside your profile.
  Pictures replace the initials wherever an avatar is drawn and fade in once decoded.
  *Your profile* has *Choose a picture…*, a crop step (drag, zoom 100–400 %, arrow keys) and
  *Remove picture*; nothing is sent until Save. A group's owner changes the group's picture
  from its details.
- Chat: voice calls with a friend (one to one, one call at a time). Calls ring for 20 seconds
  and then show as a missed call; declining stops the caller's ringing at once and both
  sides read "Call declined" (not counted as unread); leaving writes
  "Call ended, N min". The sound goes through a relay by default (*Relay calls*), so your
  friend never learns your IP address: the SimpleX apps' own public call relay
  (`turn.simplex.im`), or the servers you list in `chat.ice_servers`; it is encrypted end to
  end and the relay cannot hear it. Mute, deafen, noise suppression, and your choice of
  microphone and speakers (PulseAudio or PipeWire). Blocking a friend ends a call with them.
  A desktop notification announces an incoming call (`chat.notify_calls`). A call from the
  official SimpleX apps cannot be answered and shows as missed. New errors `EP-CHT-010`
  (no microphone), `EP-CHT-011` (the call's sound stopped), `EP-CHT-012` (the call could not
  connect). Group voice and watching a friend play are still to come.
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
- A game's Protection tab is one list read top to bottom: each thing a game can use has an
  icon that lights up while it is allowed, a line saying what it is, and a switch; folders
  show their full path. Levels are no longer shown on the game's page: under its title it
  says *Offline* or *Online*, and the Overview lists what is *Allowed* and *Blocked*.
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
- A Steam game's page has a *Launch options* tab: the same setting as Steam › Properties ›
  Launch options, changed in Steam's own settings file while Steam is closed (the tab
  offers to close Steam, and keeps a backup `localconfig.vdf.easyplay-bak`), with a
  *Performance overlay (MangoHud)* switch that adds or removes `mangohud %command%`.
  `easyplay steam launch-options <GAME> [--set TEXT] [--mangohud on|off]` does the same.
- Steam games carry a small Steam mark on their cover. The library's Filter menu lists
  only EasyPlay's own compatibility layers and gains a *Source* group (EasyPlay, Steam).
- The groundwork of Chat's in-game overlay (ADR-0037), off by default (`chat.overlay` in
  the settings file) until its design arrives: while a game runs, new messages appear over
  it and a call shows a small widget; a key bound in the compositor to
  `easyplay overlay toggle` (for example Shift+F1) opens a chat panel above the game and
  gives the keyboard back when closed. Works on niri, KDE, Sway and Hyprland; GNOME keeps the
  notifications. The overlay never runs inside a game's sandbox and games cannot reach it.
- The in-game overlay now looks like the design: people in the call as pills with speaking
  rings and message and call toasts in a corner of your choice, and,
  on the key, a chat panel over the dimmed game with your recent chats, the conversation,
  the call's buttons, a "Show while playing" menu and an answer bar for a call that rings
  while you play. It speaks your language (Arabic included) and uses your accent. While it
  runs, the messages and calls it shows are not announced again as desktop notifications.
  `easyplay overlay` now says why the overlay can't open (for example on GNOME).

### For packagers

- Game streams (ADR-0036) compile Cisco's OpenH264 from source into `easyplay-media`
  (the `openh264` crate, BSD-2-Clause; a C++ compiler is needed, no system library). Cisco's
  H.264 patent licence covers only Cisco's own binaries, not a build from source: check
  what your distribution allows. The window capture is behind the cargo feature `portal`
  (`easyplay` and `easyplay-media`; on by default in `easyplay` and the Nix package): it links `libpipewire-0.3` and needs
  libclang to build (bindgen); without it sharing fails with `EP-CHT-016` and watching
  still works.
- Hardware encoding of shared games (ADR-0036, cargo feature `hwenc`, on by default in
  `easyplay`) links FFmpeg 8's `libavcodec` and `libavutil` (nixpkgs `ffmpeg_8-headless`, a
  GPL-3 build with NVENC and VA-API) through the MIT `rsmpeg` binding; bindgen needs
  libclang. NVENC and VA-API drivers are loaded at run time; without them software encoding
  is used.
- The in-game overlay (ADR-0037) adds the binary crate `easyplay-overlay`
  (GPL-3.0-or-later; egui-wgpu, wgpu, smithay-client-toolkit, all already in the tree, and
  the `xkbcommon` crate, which links `libxkbcommon`). Install it to `libexec/easyplay/`; it
  dlopens `libwayland-client` and the Vulkan or GL loader, so give it the same run path as
  `easyplay`. Without it, or without a Wayland session, there is simply no overlay.

- New build dependency `strum` (MIT, derive macros only). `anyhow` is no longer used.
- Chat (ADR-0033) adds the crate `easyplay-chat`, licensed **AGPL-3.0-only** (the rest is
  GPL-3.0-or-later; GPLv3 §13 permits the combination), the only code that links the SimpleX
  chat engine. Install the binary to `libexec/easyplay/`, and `libsimplex.so` with the
  libraries it needs to `libexec/easyplay/simplex/` (about 237 MiB; chat core 7.0.0.12 was
  the one tested). Elsewhere: `EASYPLAY_SIMPLEX_LIB` names `libsimplex.so`;
  `EASYPLAY_SIMPLEX_LIBRARY_PATH` becomes the helper's `LD_LIBRARY_PATH` (needed on NixOS).
  Without the helper or the library the Chat page says "not included in this build" and
  nothing else changes; the AGPL source of the helper must be offered with the package.
- Chat ships Twemoji's PNG emoji (`twemoji-assets`, CC-BY 4.0; credited in Settings → About →
  Licences, text in `assets/licenses/CC-BY-4.0-Twemoji.txt`). It adds about 5.6 MB to the
  binary. The `image` crate gains its `gif` feature.
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
