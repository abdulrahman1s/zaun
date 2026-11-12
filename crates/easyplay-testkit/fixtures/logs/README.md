# Log corpus

Captured output of umu-launcher 1.4.4, pressure-vessel (SLR 3 "sniper"
3.0.20261110) and UMU-Proton-10.0-4 on the NixOS-style development host during the M5
manual launch (TECHNICAL_RESEARCH §6 E8). Paths are rewritten (`/home/sam`, `/mnt/e2e`,
`/tmp/scratch`), the user name removed, and the 8 224 hard-link warning groups of each
Isolated-mode run cut to three.

Format: `# failure: <code>|none` (the code `LogClassifier::failure` must report),
`# sandbox_started: yes|no` (whether `bwrap` had reported its child, so `bwrap:` lines are
the inner container's), then one `tag<TAB>line` per captured line. Tags: `phase:<name>`,
`fix:<level>`, `wine`, `umu_log`, `tb_start`, `tb_line`, `umu_error`, `container_error`,
`inner_bwrap`, `outer_bwrap`, `noise:hard_link`, `noise:locale_gen`, `other`.

| File | Captured from |
|---|---|
| `offline_game_isolated.txt` | Successful Offline Game session (cmd.exe), Isolated runtime, read-only Proton, fresh prefix |
| `container_execvp_failure.txt` | Unwrapped umu-run on NixOS: pressure-vessel finds no FHS `true` |
| `inner_bwrap_failure.txt` | nixpkgs FHS umu-run: the inner bwrap cannot re-bind a `--ro-bind-data` `/etc/machine-id` |
| `proton_traceback.txt` | Proton script fails in the container (`libffi.so.7`), experiment with a substituted `/usr` |
| `umu_invalid_protonpath.txt` | umu-run outside a sandbox with `PROTONPATH` at a folder without `toolmanifest.vdf` |
| `umu_no_protonpath.txt` | umu-run offline without `PROTONPATH` and no local build |

Used by `crates/easyplay-core/tests/umu_log_corpus.rs`.
