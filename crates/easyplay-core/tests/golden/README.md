# Sandbox golden matrix

`tests/sandbox_golden.rs` snapshots the complete planner output (bwrap options, in-sandbox
command, descriptor payloads, host actions, notes, issues) for a curated set of cells of the
matrix in TESTING_STRATEGY §4.1. Snapshots live in `tests/snapshots/`; paths are normalised to
`D` (EasyPlay data directory), `G` (game ID) and `S` (session ID), as in BUBBLEWRAP_SANDBOX §6.

Snapshot 01 is the worked example of BUBBLEWRAP_SANDBOX §6. The test also compares it line by
line with the listing in that document, so the specification and the planner cannot drift.

Reviewing a changed snapshot line by line is a required step of code review for `sandbox/`.

| Cells | Host | What the group covers |
|---|---|---|
| 02–11 | `arch_mesa` | Every preset × every game-folder mode (`ReadOnly`, `ReadWrite`, `Protected`, `FileOnly`) and both isolation modes; preset × network /etc entries, sysfs masks, machine ID, `AllInput`. |
| 12–17 | `arch_mesa` | Every session kind: Installer (loose file), Tool, Prepare with network (trusted, store read-write, umu cache, small `/tmp`), Prepare under policy (Isolated and Shared), Provision (tmpfs home and prefix, codename `PROTONPATH`, install-ID machine ID). |
| 18–24 | `fedora_nvidia` | NVIDIA device nodes; `pki`, `nvidia`, `OpenCL` /etc entries; two interface masks; trusted kinds on a second layout. |
| 25–31 | `ubuntu_lts_apparmor` | Debian-family layout (`lib32`, `libx32`, `alternatives`, `timezone`); non-reflink host (Shared under Auto) × presets; Isolated forced; Tool with a loose file. |
| 32–37 | `debian_stable_old_bwrap` | Real `/bin`, `/lib*` directories; extra library root; regular-file `resolv.conf`; no overlay support → `Protected` fails with `SEC-OVERLAY-001`. |
| 38–45 | `nixos` | `/nix`, `/run/current-system`, `/etc/static`; `/etc` links through `/etc/static` bound by resolved target (`zoneinfo`, `localtime`); layout `PATH`; all session kinds. |
| 46–50 | `x11_only` | X11 session: no Wayland socket, authority file in the real home, display 1, `SEC-DISPLAY-001`, shared IPC namespace. |
| 51–54 | `no_pipewire` | PulseAudio only (note), with Shared and Protected. |
| 55–59 | `input_group_user` | Keyboards readable: Standard falls back to detected controllers; Offline/Online unchanged; Provision. |
| 60 | `no_userns` | Planning is independent of the user-namespace probe. |
| zz | `input_group_user` | `AllInput` requested explicitly: downgraded with `SEC-INPUT-001`. |

Pairs that interact and where they are covered: network × masking (05–11, 18–19); preset ×
controllers × host input (09, 55–57, zz); isolation × kind (04, 14–15, 25, 28); game folder ×
overlay support (03, 21, 33, 41); kind × home/prefix/runtime (12–17, 22, 29, 42–44); layout ×
`/etc` symlink rule (01, 25, 32, 38); display session × IPC sharing (01, 46).
