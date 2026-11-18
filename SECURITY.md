# Security policy

Zaun's purpose is to limit what a Windows program can reach, so a way out of a game's
sandbox, or a way for one game to read another's data or yours, is a security problem. So is
anything that loses or corrupts a user's library or saves.

## Reporting

Please report privately, not in a public issue: use **Report a vulnerability** on the
repository's [Security tab](https://github.com/abdulrahman1s/zaun/security/advisories/new)
(GitHub private vulnerability reporting). Include the Zaun version (`zaun --version`),
the distribution, the game's protection settings, and the steps or a test program.

- We acknowledge a report within **three working days**.
- Sandbox-escape and data-loss fixes ship as patch releases on the latest minor version as
  soon as possible. The release notes state the affected versions, the impact in plain words,
  and whether stored per-game settings are affected (RELEASE_STRATEGY §5).
- We credit reporters in the release notes unless they prefer otherwise.

## Scope

In scope: the sandbox Zaun builds (the `bwrap` command line, mounts, environment,
devices, seccomp), the init shim, the supervisor, the library and its files, the
single-instance socket, and the packages in this repository (including the AppArmor profile).

Out of scope, but welcome as ordinary issues: bugs in bubblewrap, umu-launcher, Proton, Wine
or the Steam Linux Runtime themselves (please also report those upstream), and attacks that
need the user to widen a game's permissions first.

## Supported versions

Before 1.0, only the latest release receives fixes.
