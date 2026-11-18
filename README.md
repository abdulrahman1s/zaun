<p align="center">
  <img src="assets/icons/app/zaun-128.png" width="96" height="96" alt="">
</p>

<h1 align="center">Zaun</h1>

<p align="center">
  <b>Double-click a Windows game on Linux and play it, each game in its own sandbox.</b>
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#first-run">First run</a> ·
  <a href="#chat">Chat</a> ·
  <a href="#privacy">Privacy</a> ·
  <a href="#troubleshooting">Troubleshooting</a> ·
  <a href="#building-from-source">Build</a> ·
  <a href="docs/README.md">Docs</a>
</p>

---

Zaun is a Linux desktop application for Windows games and other Windows `.exe`
programs. Open an `.exe` from your file manager, give the game a name, say whether it may go
online, and press **Play**. Zaun runs it through
[umu-launcher](https://github.com/Open-Wine-Components/umu-launcher) and Proton inside its
own [bubblewrap](https://github.com/containers/bubblewrap) sandbox, with a private Windows
folder of its own. The game sees its own files and nothing else of yours unless you allow it.

There is no account, no telemetry and no Zaun server. Your library, play time and
settings stay on your computer.

> **Status:** pre-1.0. Everything works end to end, but expect rough edges and changes
> between `0.x` releases. Your library is migrated forward automatically; downgrading is not
> supported.

## What it does

**Protection you can read.** Every game starts from the same short list: sound, game
controllers, its own folder and its saves. Internet is off unless you choose *Online*. Its
**Protection** tab is one list read from top to bottom. Each item (the internet, microphone,
camera, controllers, your Pictures or Downloads folder, a folder you share) has an icon that
lights up while it is allowed, one plain line saying what it is, and a switch. Folders show
their full path. Under the game's title it simply says *Offline* or *Online*.

**A library that looks like a shelf.** The library is a wall of covers. A game without
artwork gets a poster of its own: its title set large on one of ten colour pairs, the same
for as long as the game keeps its name. Search, filters, tags, favourites and sorting sit on
one line of words under the top bar. A small library shows its few games larger, in the
middle of the window.

**A page for every game.** Each game's page shows its cover with thirty days of play under
it. The **Overview** says what the game is: the engine it was made with (Unity, Unreal,
Godot, Ren'Py and more), who made it, and whether it uses anti-cheat or Steam DRM, which
often stop games under Proton. Then come **Protection**, **Compatibility**, **Launch
options** (with suggestions for Proton's options as you type), **Activity** and **Logs**.

**Compatibility layers kept up to date.** Proton and the Steam Linux Runtime are downloaded,
checked and updated through umu, with game fixes from the umu database.

**Installers work too.** A `setup.exe` runs inside the game's sandbox, then Zaun asks
which of the programs it installed is the game.

**Your Steam games, if you want them.** With *Show my Steam games* turned on, games installed
with Steam appear in the library with their play time and update state. Play hands over to
Steam. A Steam game's **Launch options** tab edits Steam's own setting (while Steam is
closed, with a backup) and has a switch for the MangoHud performance overlay.

**Statistics.** Play time by day, month and year, your most played games and recent
sessions. Everything is computed on your computer.

**Chat with friends.** Chat is optional and end-to-end encrypted, with no account. Turn it on
and add friends, talk one to one or in groups, and see who is playing what. See
[Chat](#chat).

**Fast, and stays out of the way.** An optional background service keeps Zaun loaded
without a window, so a game opened from the file manager starts in about 20 ms. Running
games are not affected when Zaun closes.

**Everything from a terminal as well.** `zaun open`, `launch`, `stop`, `status`,
`list`, `add`, `edit`, `remove`, `doctor` and more, with `--json` output, a man page and
shell completions.

**Made to be comfortable.** It has dark, light and high-contrast themes and four accent
colours. The interface can be scaled from 80 to 150 %, and *Reduce motion* turns every
movement into a short fade. It is fully keyboard-driven and works with screen readers. The
interface is available in English and Arabic.

Sandboxing Windows software reduces risk; it is not a guarantee. Only run games you would
run anyway. Zaun makes it harder for one of them to reach the rest of your files.

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

Download links are on the [releases page](https://github.com/zaun/zaun/releases).
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
  inputs.zaun.url = "github:zaun/zaun";
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

Without modules: `nix run github:zaun/zaun`, or
`nix profile install github:zaun/zaun`.

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

## Chat

Chat lets you talk with friends inside Zaun. It is **off until you turn it on**: open
the Chat page, or Settings → Chat.

- **No account, no username, no phone number.** You become friends by passing a one-time
  link, shown as a QR code or copied with one button. Everything known about your friends
  is stored on your computer.
- **End-to-end encrypted.** Messages pass through relays: by default the public servers at
  `simplex.im` and `simplexonflux.com`, or relays you list in Settings. The relays see your
  IP address and when messages pass, never what they say. The page tells you this before
  you turn Chat on.
- **Conversations and groups**, with replies, emoji reactions, edits, `@mentions` in
  groups, simple formatting (bold, italic, code, quotes, lists, spoilers) and emoji you can
  pick from a list or autocomplete by typing `:`. Press ↑ in an empty message box to edit
  your last message.
- **GIFs** come from [GIPHY](https://giphy.com), reached through Signal's content proxy
  (`contentproxy.signal.org`), so GIPHY never sees your address and the proxy sees only which
  site you are reaching. Only the words you type in the GIF search are sent. You send the GIF
  itself as a file, so your friends' apps never contact GIPHY or the proxy. The key and the
  proxy are Signal's public ones by default; you can change both in Settings → Chat.
- **Links stay links.** Zaun never fetches a link you receive. If you turn on *Check
  links before opening*, it first asks Google Safe Browsing whether the site is known to be
  harmful. Only the site's name is sent, never the full link.
- **Notifications** show who wrote to you when Zaun is in the background. They show the
  message text only if you choose to. You can mute a conversation, block someone (they are
  not told), and compare security codes with a friend to make sure nobody is in between.
- Chat's database is encrypted and kept in Zaun's data folder, which no game's sandbox
  can reach. The chat engine runs in a sandbox of its own and talks to Zaun over a pipe,
  not a network port that games could find.

Voice calls, watching a friend's game and an in-game overlay are planned but not yet
available.

## Privacy

Zaun has no telemetry and no accounts, and it makes no network requests you have not
asked for:

| When | Who it contacts | What it sends |
|---|---|---|
| Downloading and updating compatibility layers | GitHub and Valve, through umu | What any download sends |
| Online artwork (off by default) | Steam's image servers | The game's Steam app number |
| Chat (off by default) | The relays you use | Encrypted messages; relays see your IP address and timing |
| GIF search in Chat | GIPHY, through Signal's content proxy | The words you type in the GIF search; the proxy sees only the host name |
| *Check links before opening* (off by default) | Google Safe Browsing | The name of the site you are opening |

Games are a different matter: a game set to *Online* talks to whatever it wants, as it
would anywhere. All the details are in [docs/PRIVACY.md](docs/PRIVACY.md).

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
| The Chat page says "not included in this build" | Your package was built without the chat engine. Everything else works as usual. |
| Something else | Settings → Advanced → *Create a report…*, or run `zaun -v` from a terminal, and attach the result to an [issue](https://github.com/zaun/zaun/issues). |

Security problems: please report them privately, as described in [SECURITY.md](SECURITY.md).

## Building from source

You need Rust 1.95 or newer with the `x86_64-unknown-linux-musl` target (for the small
static helper that runs inside each sandbox), a C compiler, and `pkg-config`.

```sh
git clone https://github.com/zaun/zaun && cd zaun
scripts/build-release.sh --prefix /usr/local --destdir staging   # build, check, stage
sudo cp -a staging/usr/local/. /usr/local/
```

`scripts/build-release.sh --help` lists its options (`--tarball` builds the release
tarball). With Nix, `nix build` produces the same layout in `./result`, and `nix develop`
gives a shell with every tool.

Chat needs the SimpleX chat engine (`libsimplex.so`). The Nix package includes it. For other
builds, see the packager notes in [CHANGELOG.md](CHANGELOG.md). Without it, Zaun builds
and runs normally, and the Chat page says it is not included.

To try the interface with sample games and friends, without touching your own library, run
`zaun --preview`.

### Where to read more

| | |
|---|---|
| [CONTRIBUTING.md](CONTRIBUTING.md) | Setting up, tests, snapshots, conventions |
| [docs/README.md](docs/README.md) | The full design and specification, in reading order |
| [docs/adr/](docs/adr/README.md) | Every design decision and why it was made |
| [docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md), [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) | What the sandbox protects against, and what it doesn't |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | Every setting in `~/.config/zaun/config.toml` |

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
