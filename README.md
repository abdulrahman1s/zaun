# EasyPlay

**Double-click a Windows game on Linux and play it, each game in its own sandbox.**

EasyPlay is a Linux desktop application that runs Windows games (and other Windows `.exe`
programs) through [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher) and
Proton. Open an `.exe` from your file manager, give the game a name, answer one question
about what it may access, and play. Every game then runs inside its own
[bubblewrap](https://github.com/containers/bubblewrap) sandbox with its own private Windows
folder, and sees only its own files unless you allow more.

- **One question, plain words.** *Offline* (no internet), *Online* (internet, nothing else
  of yours), or *Standard*. Everything else has a safe default and sits behind "More options".
- **A library, not a launcher maze.** Search, sort, tags, artwork, and play time per game.
- **Compatibility layers kept up to date** for you, with game fixes from the umu database.
- **Nothing leaves your computer.** No telemetry, no accounts, no update checks of its own.

Sandboxing Windows software reduces risk; it is not a guarantee. Only run games you would
run anyway; EasyPlay makes it harder for one of them to reach the rest of your files.

> **Status:** pre-1.0. Everything works end to end, but expect rough edges and changes
> between `0.x` releases. Your library is migrated forward automatically; downgrading is not
> supported.

## Requirements

| | |
|---|---|
| System | 64-bit (x86_64) Linux with glibc: Arch, Fedora, Debian, Ubuntu, NixOS, SteamOS desktop mode (best effort) |
| Kernel | Unprivileged user namespaces enabled (the default almost everywhere) |
| bubblewrap | 0.8 or newer (`bwrap`) |
| umu-launcher | 1.4 or newer (`umu-run`) |
| Desktop | Wayland (with Xwayland) or X11 |
| Graphics | Vulkan-capable drivers, including the 32-bit ones, for most games |

EasyPlay's **Doctor** (Settings → Advanced → *Check my system*, or `easyplay doctor` in a
terminal) checks all of this and says exactly what to do when something is missing.

## Install

Download links are on the [releases page](https://github.com/easyplay/easyplay/releases).
Every release has a `SHA256SUMS` file.

### Arch Linux and derivatives

From the AUR, `easyplay` (builds from source) or `easyplay-bin` (the release binary):

```sh
paru -S easyplay-bin        # or: yay -S easyplay-bin
```

The `easyplay-bin-*.pkg.tar.zst` from a release also installs directly with
`sudo pacman -U easyplay-bin-*.pkg.tar.zst`.

### Fedora

```sh
sudo dnf install ./easyplay-*.x86_64.rpm
```

A COPR repository is planned; the spec (`packaging/rpm/easyplay.spec`) builds there from
source. `umu-launcher` must be installable from your configured repositories.

### Debian and Ubuntu

```sh
sudo apt install ./easyplay_*_amd64.deb
```

`umu-launcher` is recommended rather than required, because not every release packages it.
Without it, EasyPlay opens and the Doctor explains how to install it.

On Ubuntu 23.10 and newer, the package installs an AppArmor profile
(`/etc/apparmor.d/easyplay`) that allows EasyPlay's sandboxes to work under Ubuntu's
restriction of user namespaces. See [Troubleshooting](#troubleshooting) if games still
fail to start.

### NixOS and Nix

The flake provides the package, a NixOS module, a home-manager module and an overlay. The
package already brings `bubblewrap` and `umu-launcher`.

```nix
# flake.nix
{
  inputs.easyplay.url = "github:easyplay/easyplay";
  # Optional: build against your own nixpkgs.
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
| `programs.easyplay.allowUnprivilegedUsernsClone` | `false` | Sets `kernel.unprivileged_userns_clone = 1`. Only for hardened kernels that disable unprivileged user namespaces. |

Evaluation fails if `security.allowUserNamespaces = false` or `user.max_user_namespaces` is
0: bubblewrap cannot work without user namespaces.

home-manager (any distribution with Nix; on NixOS prefer the system module, which also
enables 32-bit graphics):

```nix
{
  imports = [ easyplay.homeManagerModules.default ];
  programs.easyplay = {
    enable = true;
    # Optional, rendered to ~/.config/easyplay/config.toml. Leave unset to let EasyPlay
    # manage the file itself.
    settings.appearance.theme = "dark";
  };
}
```

Without modules: `nix run github:easyplay/easyplay`, or
`nix profile install github:easyplay/easyplay`.

### Any distribution: the tarball

```sh
tar --zstd -xf easyplay-*-x86_64-linux.tar.zst
cd easyplay-*-x86_64-linux
./install.sh              # into ~/.local; as root (sudo ./install.sh) into /usr/local
```

`install.sh` copies the files, adds EasyPlay to your applications menu and refreshes the
desktop and icon caches. It never changes which program opens `.exe` files by default.
Install `bubblewrap` and `umu-launcher` with your package manager. To remove EasyPlay:

```sh
~/.local/libexec/easyplay/uninstall.sh      # or /usr/local/libexec/easyplay/uninstall.sh
```

Uninstalling removes exactly the files the installer added and keeps your games, settings
and downloaded runtimes.

## First run

1. Start **EasyPlay** from your applications menu, or open any `.exe` with it ("Open
   With → EasyPlay" in your file manager).
2. The first time, EasyPlay checks your system and downloads a compatibility layer (Proton)
   and the Steam Linux Runtime through umu. This is several hundred megabytes, once.
3. Opening a game it has not seen before shows a short setup: the name (with artwork if
   found), and the protection question. Choose and press **Play**.
4. Next time, the game starts straight away, from the library or by opening its `.exe`
   again. Each game's page has its settings; the **Protection** tab shows what the game can
   access, in plain language, and lets you change it.

Installers (`setup.exe`) work the same way: EasyPlay runs them in the game's sandbox and
then asks which of the installed programs is the game.

From a terminal, `easyplay --help` lists everything: `easyplay open Game.exe`,
`easyplay list`, `easyplay launch <game>`, `easyplay status`, `easyplay doctor`, and more
(`man easyplay`).

## Privacy

EasyPlay has no telemetry and no accounts. Network access happens only through umu, to
download compatibility layers and runtimes from GitHub and Valve, and, only if you turn on
online artwork, to Steam's image servers. Details:
[docs/PRIVACY.md](docs/PRIVACY.md).

## Troubleshooting

Start with the **Doctor** (Settings → Advanced → *Check my system*, or `easyplay doctor`). It checks bubblewrap,
umu, user namespaces, graphics drivers and the runtime, and suggests the fix for each
problem.

| Symptom | What to do |
|---|---|
| "This system does not allow sandboxes" (`EP-HOST-004`) on Ubuntu | Ubuntu restricts user namespaces through AppArmor. The `.deb` installs a profile for this; if you installed the tarball, or the profile is not loaded, run `sudo apparmor_parser -r /etc/apparmor.d/easyplay` (with the profile from `packaging/apparmor/easyplay`) or, less strictly, `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`, which relaxes the restriction for every program. |
| "This system does not allow sandboxes" elsewhere | Your kernel disables unprivileged user namespaces (common on hardened kernels): enable `kernel.unprivileged_userns_clone=1`, or on NixOS `programs.easyplay.allowUnprivilegedUsernsClone = true`. |
| `umu-run` not found | Install `umu-launcher` from your distribution, or follow the Doctor's link to its releases. |
| The game starts but shows a black window or no 3D | Install your driver's 32-bit Vulkan libraries (`lib32-vulkan-radeon`/`lib32-nvidia-utils` on Arch, `mesa-vulkan-drivers:i386` on Debian/Ubuntu). |
| A game needs a file outside its folder | The game's page → **Protection** → *Files and folders* → *Add folder…*. |
| Something else | Settings → Advanced → *Create a report…*, or run `easyplay -v` from a terminal, and attach the result to an [issue](https://github.com/easyplay/easyplay/issues). |

Security problems: please report them privately, see [SECURITY.md](SECURITY.md).

## Building from source

You need Rust 1.95 or newer with the `x86_64-unknown-linux-musl` target (for the small
static helper that runs inside each sandbox), a C compiler, and `pkg-config`.

```sh
git clone https://github.com/easyplay/easyplay && cd easyplay
scripts/build-release.sh --prefix /usr/local --destdir staging   # build, check, stage
sudo cp -a staging/usr/local/. /usr/local/
```

`scripts/build-release.sh --help` lists its options (`--tarball` builds the release
tarball). With Nix, `nix build` produces the same layout in `./result`, and `nix develop`
gives a shell with every tool. Contributing: [CONTRIBUTING.md](CONTRIBUTING.md). The design
and specification: [docs/README.md](docs/README.md).

## Licence

EasyPlay is free software under the GNU General Public License, version 3 or later
([LICENSE](LICENSE)). It bundles the Inter and JetBrains Mono fonts (SIL Open Font
Licence 1.1) and Phosphor icons (MIT). bubblewrap, umu-launcher, Proton and the Steam Linux
Runtime are separate programs that EasyPlay runs; they are not part of it.
