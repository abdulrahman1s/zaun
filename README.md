<p align="center">
  <img src="assets/icons/app/zaun-128.png" width="96" height="96" alt="">
</p>

<h1 align="center">Zaun</h1>

<p align="center"><b>Your Windows games and your friends, together on Linux.</b></p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#games">Games</a> ·
  <a href="#friends">Friends</a> ·
  <a href="#how-chat-stays-private">Security</a> ·
  <a href="#building-from-source">Build</a>
</p>

---

Zaun is a game library where every Windows game runs in its own sandbox, with private chat,
voice calls, game sharing and an in-game overlay built in. No accounts, no tracking.

> **Status:** pre-1.0. It works end to end; expect rough edges between `0.x` releases.

## Games

**Every game in its own box.** Each game runs through
[umu-launcher](https://github.com/Open-Wine-Components/umu-launcher) and Proton inside its
own [bubblewrap](https://github.com/containers/bubblewrap) sandbox, with a private Windows
folder of its own. It sees its own files and nothing else of yours unless you say so.
Internet is off unless you choose *Online*.

**Protection you can read.** A game's **Protection** tab is one list, top to bottom: the
internet, microphone, camera, controllers, your Pictures or Downloads folder, a folder you
share. Each has an icon that lights up while it's allowed, one plain line saying what it
is, and a switch. Folders show their full path.

**A library that looks like a shelf.** A wall of covers. A game without artwork gets a
poster of its own: its title set large on one of ten colour pairs. Search, filters, tags,
favourites and sorting sit on one line under the top bar.

**A page for every game.** Its cover with thirty days of play under it, and an **Overview**
that says what the game is: the engine it was made with (Unity, Unreal, Godot, Ren'Py and
more), who made it, and whether it uses anti-cheat or Steam DRM, which often stop games
under Proton. Then **Protection**, **Compatibility**, **Launch options** (with suggestions
for Proton's options as you type), **Activity** and **Logs**.

**Your Steam games too, if you want them.** Turn on *Show my Steam games* and they appear
in the library with their play time and update state. Play hands over to Steam, and a Steam
game's **Launch options** tab edits Steam's own setting, with a switch for MangoHud.

**Installers, statistics, the terminal.** A `setup.exe` runs inside the game's sandbox, then
Zaun asks which installed program is the game. Proton and the Steam Linux Runtime are
downloaded and kept up to date through umu. Play time by day, month and year is computed on
your computer. Everything also works from a terminal: `zaun open`, `launch`, `stop`,
`status`, `list`, `add`, `doctor` and more, with `--json`, a man page and shell completions.

**Fast.** An optional background service keeps Zaun loaded, so a game opened from your file
manager starts in about 20 ms. Running games don't care whether Zaun's window is open.

Sandboxing Windows software reduces risk; it isn't a guarantee. Only run games you would run
anyway. Zaun makes it much harder for one of them to reach the rest of your files.

## Friends

Chat is **off until you turn it on** (the Chat page, or Settings → Chat).

**Add friends without an account.** No username, no phone number, no email. You become
friends by passing a one-time link, shown as a QR code or copied with one button. Your
friends can use Zaun, or the SimpleX Chat app on their phone or computer.

**Talk.** One-to-one and group conversations with replies, reactions, edits, `@mentions`,
simple formatting (bold, italic, code, quotes, lists, spoilers), emoji with `:` autocomplete,
files, pictures, GIFs and profile pictures. Times read like they should ("Today at 13:59"),
with the full date when you point at one. Press ↑ in an empty box to edit your last message.

**See who's playing what.** Your friends list shows what each friend is playing right now.

**Voice calls.** Call a friend with one click. Mute, deafen, noise suppression, and your
choice of microphone and speakers, with a *Let's check* test that plays your voice back.
Calls go through a relay by default, so a friend never learns your IP address.

**Share your game.** Stream one game's window to a friend: never your whole screen, only
the window you pick. It's encoded on your graphics card when it can be (NVIDIA NVENC, AMD
and Intel VA-API) at 720p, 1080p, 1440p or the game's own size, 30 or 60 frames a second,
changeable while you share. Game sound in streams is still to come.

### The in-game overlay

Press a key in a fullscreen game and your chats come up over it: recent conversations, the
open conversation, a reply box and the call's buttons. Close it and it's out of the way:
new messages slide in as small notes in the corner you choose, and people in a call show as
a column of pills that light up when they speak. The overlay is a separate window drawn on
top by your desktop: nothing is injected into the game, and it takes no input while it's
closed.

It needs a desktop with layer-shell surfaces: niri, KDE Plasma, Sway or Hyprland. Wayland
gives programs no global hotkeys, so you bind the key in your desktop:

| Desktop | Binding |
|---|---|
| niri | `binds { Shift+F1 { spawn "zaun" "overlay" "toggle"; } }` |
| Sway | `bindsym Shift+F1 exec zaun overlay toggle` |
| Hyprland | `bind = SHIFT, F1, exec, zaun overlay toggle` |
| KDE Plasma | System Settings → Keyboard → Shortcuts → Add Command: `zaun overlay toggle` |

GNOME can't show overlays; there, messages and calls arrive as notifications instead.

## How Chat stays private

Zaun's chat runs on the [SimpleX](https://simplex.chat) messaging protocol and its open-source
engine, the same one the SimpleX Chat apps use. What that means for you:

- **No identifiers at all.** SimpleX has no user IDs: not a phone number, not a username,
  not even a random number. Each conversation uses its own one-way message queues on the
  relays, with different addresses for sending and receiving, so there is nothing that ties
  your conversations together or to you.
- **End-to-end encryption, with a post-quantum layer.** Messages use the double-ratchet
  algorithm (forward secrecy and recovery after a compromise), with a post-quantum key
  exchange in one-to-one conversations. Only you and your friends can read them.
- **Relays that know very little.** Messages pass through relays (by default the public
  `simplex.im` and `simplexonflux.com` servers, or ones you run yourself). A relay sees an IP
  address and the timing of encrypted packets. It can't read them, can't see who they're
  for, and keeps them only until they're delivered. An extra encryption layer between the
  relay and the recipient means traffic going in and out of a relay can't be matched up.
- **Calls that hide your address.** Voice and game streams are WebRTC with DTLS-SRTP,
  end to end, with keys checked through the encrypted chat. When both apps support it, each
  voice frame is also sealed with AES-256-GCM under a key the two sides agree on privately.
  Calls go through a TURN relay by default, so your friend sees the relay's address, never
  yours.
- **Verify, don't trust.** Compare a security code with a friend, in person or over another
  channel, to make sure nobody is sitting in between.

And on your computer:

- **The chat engine has a sandbox of its own.** It runs as a separate process inside
  bubblewrap, with exactly one writable folder: its own. No home folder, no library, no game
  prefixes. Zaun talks to it over a pipe, not a network port another program could find.
- **Games can't reach your chats.** The chat database is encrypted, and its folder (and every
  folder above it) can never be shared with a game, even in Developer Mode.
- **Links stay links.** Zaun never fetches a link you receive. If you turn on *Check links
  before opening*, it asks Google Safe Browsing about the site's name only, never the full
  link.
- **GIFs without being watched.** GIF search goes to [GIPHY](https://giphy.com) through
  Signal's content proxy, so GIPHY never sees your address. Only your search words are sent,
  and you send the GIF itself as a file, so your friends' apps never contact anyone.
- **Notifications show who wrote**, and the message text only if you choose to. Mute a
  conversation, or block someone; they aren't told.

## Privacy

Zaun has no telemetry and no accounts, and makes no network request you haven't asked for:

| When | Who it contacts | What it sends |
|---|---|---|
| Downloading and updating compatibility layers | GitHub and Valve, through umu | What any download sends |
| Online artwork (off by default) | Steam's image servers | The game's Steam app number |
| Chat (off by default) | The relays you use | Encrypted messages; relays see your IP address and timing |
| Calls and game streams (with Chat) | The call relay, by default `turn.simplex.im` | Encrypted media; the relay sees both ends' relay addresses and the amount of traffic |
| GIF search in Chat | GIPHY, through Signal's content proxy | The words you search for; the proxy sees only the host name |
| *Check links before opening* (off by default) | Google Safe Browsing | The name of the site you are opening |

Games are a different matter: a game set to *Online* talks to whatever it wants, as it
would anywhere.

## Requirements

| | |
|---|---|
| System | 64-bit (x86_64) Linux with glibc: Arch, Fedora, Debian, Ubuntu, NixOS, SteamOS desktop mode (best effort) |
| Kernel | Unprivileged user namespaces enabled (the default almost everywhere) |
| bubblewrap | 0.8 or newer (`bwrap`) |
| umu-launcher | 1.4 or newer (`umu-run`) |
| Desktop | Wayland (with Xwayland) or X11 |
| Graphics | Vulkan-capable drivers, including the 32-bit ones, for most games |
| Sound | PipeWire, for games to have sound without access to your microphone |

Zaun's **Doctor** checks all of this and says exactly what to do when something is
missing. Open it from Settings → Advanced → *Check my system*, or run `zaun doctor` in a
terminal.

## Install

Download links are on the [releases page](https://github.com/abdulrahman1s/zaun/releases).
Every release has a `SHA256SUMS` file.

### Arch Linux and derivatives

From the AUR, install `zaun` (builds from source) or `zaun-bin` (the release binary):

```sh
paru -S zaun-bin        # or: yay -S zaun-bin
```

The `zaun-bin-*.pkg.tar.zst` from a release also installs directly with
`sudo pacman -U zaun-bin-*.pkg.tar.zst`.

### Fedora

```sh
sudo dnf install ./zaun-*.x86_64.rpm
```

A COPR repository is planned; the spec (`packaging/rpm/zaun.spec`) builds there from
source. `umu-launcher` must be installable from your configured repositories.

### Debian and Ubuntu

```sh
sudo apt install ./zaun_*_amd64.deb
```

`umu-launcher` is recommended rather than required, because not every release packages it.
Without it, Zaun still opens and the Doctor explains how to install it.

On Ubuntu 23.10 and newer, the package installs an AppArmor profile
(`/etc/apparmor.d/zaun`). It lets Zaun's sandboxes work under Ubuntu's restriction of
user namespaces. See [Troubleshooting](#troubleshooting) if games still fail to start.

### NixOS and Nix

The flake provides the package, a NixOS module, a home-manager module and an overlay. The
package already brings `bubblewrap` and `umu-launcher`.

```nix
# flake.nix
{
  inputs.zaun.url = "github:abdulrahman1s/zaun";
  # Optional: build against your own nixpkgs.
  # inputs.zaun.inputs.nixpkgs.follows = "nixpkgs";

  outputs = { nixpkgs, zaun, ... }: {
    nixosConfigurations.my-host = nixpkgs.lib.nixosSystem {
      modules = [ ./configuration.nix zaun.nixosModules.default ];
    };
  };
}
```

```nix
# configuration.nix
{
  programs.zaun.enable = true;
}
```

| Option | Default | Effect |
|---|---|---|
| `programs.zaun.enable` | `false` | Installs Zaun system-wide. |
| `programs.zaun.package` | this flake's package | The package to install (`pkgs.zaun` with `zaun.overlays.default`). |
| `programs.zaun.enable32BitGraphics` | `true` | `hardware.graphics.enable` and `enable32Bit` (as `mkDefault`); most games need 32-bit drivers. |
| `programs.zaun.associateWindowsExecutables` | `true` | Adds Zaun to "Open With" for `.exe`/`.msi` (`xdg.mime.addedAssociations`). Never sets it as the default. |
| `programs.zaun.allowUnprivilegedUsernsClone` | `false` | Sets `kernel.unprivileged_userns_clone = 1`. Only for hardened kernels that disable unprivileged user namespaces. |
| `programs.zaun.backgroundService.enable` | `false` | Keeps Zaun loaded in the background, so games and windows open instantly. |

Evaluation fails if `security.allowUserNamespaces = false` or `user.max_user_namespaces` is
0, because bubblewrap cannot work without user namespaces.

home-manager works on any distribution with Nix. On NixOS, prefer the system module, which
also enables 32-bit graphics.

```nix
{
  imports = [ zaun.homeManagerModules.default ];
  programs.zaun = {
    enable = true;
    # Optional, rendered to ~/.config/zaun/config.toml. Leave unset to let Zaun
    # manage the file itself.
    settings.appearance.theme = "dark";
  };
}
```

Without modules: `nix run github:abdulrahman1s/zaun`, or
`nix profile install github:abdulrahman1s/zaun`.

### Any distribution: the tarball

```sh
tar --zstd -xf zaun-*-x86_64-linux.tar.zst
cd zaun-*-x86_64-linux
./install.sh              # into ~/.local; as root (sudo ./install.sh) into /usr/local
```

`install.sh` copies the files, adds Zaun to your applications menu and refreshes the
desktop and icon caches. It never changes which program opens `.exe` files by default.
Install `bubblewrap` and `umu-launcher` with your package manager. To remove Zaun:

```sh
~/.local/libexec/zaun/uninstall.sh      # or /usr/local/libexec/zaun/uninstall.sh
```

Uninstalling removes exactly the files the installer added and keeps your games, settings
and downloaded runtimes.

### The background service (optional)

```sh
systemctl --user enable --now zaun.service
```

Zaun then stays loaded without a window. A game opened from the file manager starts in
about 20 ms, and windows open without a cold start. Packages install the unit but never
enable it for you.

## First run

1. Start **Zaun** from your applications menu, or open any `.exe` with it ("Open
   With → Zaun" in your file manager).
2. The first time, Zaun checks your system and downloads a compatibility layer (Proton)
   and the Steam Linux Runtime through umu. This is several hundred megabytes, once.
3. Opening a game it has not seen before shows a short setup. You give the name (with
   artwork if found) and choose *Offline*, *Online* or *Standard*. Press **Play**.
4. Next time, the game starts straight away, from the library or by opening its `.exe`
   again. To change what a game can use, open its page → **Protection** and flip the
   switches.

The top bar has four destinations: **Library**, **Chat**, **Statistics** and **Settings**.
While a game runs, the top bar shows it with its play time and a stop button, on every page.

From a terminal, `zaun --help` lists everything, for example `zaun open Game.exe`,
`zaun list`, `zaun launch <game>`, `zaun status`, `zaun doctor` and
`zaun steam scan`. See also `man zaun`.

## Troubleshooting

Start with the **Doctor** (Settings → Advanced → *Check my system*, or `zaun doctor`).
It checks bubblewrap, umu, user namespaces, graphics drivers and the runtime, and suggests
the fix for each problem.

| Symptom | What to do |
|---|---|
| "This system does not allow sandboxes" (`ZN-HOST-004`) on Ubuntu | Ubuntu restricts user namespaces through AppArmor. The `.deb` installs a profile for this. If you installed the tarball, or the profile is not loaded, run `sudo apparmor_parser -r /etc/apparmor.d/zaun` (with the profile from `packaging/apparmor/zaun`). Less strictly, `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0` relaxes the restriction for every program. |
| "This system does not allow sandboxes" elsewhere | Your kernel disables unprivileged user namespaces (common on hardened kernels). Enable `kernel.unprivileged_userns_clone=1`, or on NixOS set `programs.zaun.allowUnprivilegedUsernsClone = true`. |
| `umu-run` not found | Install `umu-launcher` from your distribution, or follow the Doctor's link to its releases. |
| The game starts but shows a black window or no 3D | Install your driver's 32-bit Vulkan libraries (`lib32-vulkan-radeon`/`lib32-nvidia-utils` on Arch, `mesa-vulkan-drivers:i386` on Debian/Ubuntu). |
| A game has no sound | Make sure PipeWire is running; games get sound through PipeWire only. |
| A game needs a file outside its folder | The game's page → **Protection** → *Share another folder…*. |
| A game needs the microphone | The game's page → **Protection** → turn on *Microphone*. It is off for every game until you allow it. |
| The in-game overlay does not open | Bind a key in your compositor to `zaun overlay toggle` (see [the overlay](#the-in-game-overlay)), and turn it on in Settings → Chat → Overlay. GNOME cannot show it; you still get notifications. |
| A call connects but nobody hears anything | Check the microphone and speakers in Settings → Chat → Voice and video (*Let's check* plays your voice back). If calls never connect, add your own TURN relay under *Advanced*, or turn off *Send calls through a relay*. |
| The Chat page says "not included in this build" | Your package was built without the chat engine. Everything else works as usual. |
| Something else | Settings → Advanced → *Create a report…*, or run `zaun -v` from a terminal, and attach the result to an [issue](https://github.com/abdulrahman1s/zaun/issues). |

Security problems: please report them privately, as described in [SECURITY.md](SECURITY.md).

## Building from source

You need Rust 1.95 or newer with the `x86_64-unknown-linux-musl` target (for the small
static helper that runs inside each sandbox), a C compiler, and `pkg-config`.

```sh
git clone https://github.com/abdulrahman1s/zaun && cd zaun
scripts/build-release.sh --prefix /usr/local --destdir staging   # build, check, stage
sudo cp -a staging/usr/local/. /usr/local/
```

`scripts/build-release.sh --help` lists its options (`--tarball` builds the release
tarball). With Nix, `nix build` produces the same layout in `./result`, and `nix develop`
gives a shell with every tool.

Chat needs the SimpleX chat engine (`libsimplex.so`). The Nix package includes it. For other
builds, see the packager notes in [CHANGELOG.md](CHANGELOG.md). Without it, Zaun builds
and runs normally, and the Chat page says it is not included.

To try the interface with sample games, friends, calls and streams, without touching your
own library or contacting anyone, run `zaun --preview`.

### Where to read more

| | |
|---|---|
| [CONTRIBUTING.md](CONTRIBUTING.md) | Setting up, tests, snapshots, conventions |
| [SECURITY.md](SECURITY.md) | Reporting a vulnerability |
| [CHANGELOG.md](CHANGELOG.md) | What changed in each version |

## Licence

Zaun is free software under the GNU General Public License, version 3 or later
([LICENSE](LICENSE)). The chat helper `zaun-chat`, the only part that links the SimpleX
chat engine, is under the GNU Affero General Public License, version 3.

Zaun bundles these:

- the Inter, Instrument Sans, JetBrains Mono and Noto Sans Arabic fonts (SIL Open Font
  Licence 1.1);
- Phosphor icons (MIT);
- Twemoji graphics (CC-BY 4.0).

bubblewrap, umu-launcher, Proton and the Steam Linux Runtime are separate programs that
Zaun runs; they are not part of it.
