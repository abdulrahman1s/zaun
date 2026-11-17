//! *Copy as shell command* (DEVELOPER_MODE §5): a POSIX-shell script that reproduces a launch
//! by hand. EasyPlay never executes it.
//!
//! Rules: every argument single-quoted with `'\''` escaping; text payloads become
//! here-documents with unique delimiters, other payloads go through `base64 -d` into a
//! temporary file; the status and sync descriptors are dropped (and `--status-fd` from the
//! init shim's command line); host actions that create directories or drive links become
//! `mkdir`/`ln` lines, the others are comments.
//!
//! The system-call filter is reproduced, never dropped: the plan records only its descriptor,
//! so the script embeds the same cached program the spawner would load
//! ([`seccomp::program`] for the profile the options imply) as a binary payload. A build
//! that cannot produce the program writes a script that refuses to run instead of one that
//! would quietly start the game with less protection than EasyPlay gives it.
//!
//! The same goes for the restricted Wayland socket (ADR-0029): only a Wayland client can
//! register it with the compositor, so a plan that has one gives a script that refuses to
//! run rather than one that binds the compositor's own socket in its place.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::os::unix::ffi::OsStrExt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

use super::plan_file::{FdKind, LaunchPlan, group_args};
use crate::sandbox::{HostAction, seccomp, sound, sound_socket};

/// Quote one argument for a POSIX shell.
pub fn quote(a: &OsStr) -> String {
    let t = String::from_utf8_lossy(a.as_bytes());
    format!("'{}'", t.replace('\'', r"'\''"))
}

fn q(s: impl AsRef<OsStr>) -> String {
    quote(s.as_ref())
}

/// Render the script. `generated_at` is shown in the header only.
pub fn render_shell(plan: &LaunchPlan, generated_at: &str) -> String {
    let mut out = String::new();
    let s = &plan.session;
    let _ = writeln!(out, "#!/bin/sh");
    let _ = writeln!(
        out,
        "# EasyPlay launch of {:?} (session kind: {}), generated {generated_at}",
        s.game_name,
        s.kind.as_str()
    );
    let _ = writeln!(
        out,
        "# Descriptor payloads are recreated below. Paths are real; review before running."
    );
    let _ = writeln!(
        out,
        "# Running this bypasses EasyPlay's session tracking: no session row, no statistics,"
    );
    let _ = writeln!(out, "# no prefix lock.");
    let _ = writeln!(out, "set -e");

    for a in &plan.host_actions {
        match a {
            HostAction::CreateDir { path, mode } => {
                let _ = writeln!(out, "mkdir -p -m {mode:04o} {}", q(path));
            }
            HostAction::EnsureDosDevices { prefix, links } => {
                let dd = prefix.join("dosdevices");
                let _ = writeln!(out, "mkdir -p {}", q(&dd));
                for l in links {
                    let link = dd.join(format!("{}:", l.letter.to_ascii_lowercase()));
                    let _ = writeln!(out, "ln -sfn {} {}", q(&l.target), q(&link));
                }
            }
            HostAction::AcquireLock { path } => {
                let _ = writeln!(out, "# skipped: prefix lock {}", q(path));
            }
            HostAction::MarkTainted { game } => {
                let _ = writeln!(out, "# skipped: mark the prefix of game {game} as used");
            }
            HostAction::WriteMemfd { .. } => {}
            HostAction::StartSoundServer {
                program,
                dir,
                remote,
                log,
            } => {
                let conf = dir.join(sound::SOUND_CONFIG);
                let socket = sound_socket(dir);
                let _ = writeln!(out, "# the sound server that refuses recording (ADR-0026)");
                let _ = writeln!(out, "mkdir -p -m 0700 {}", q(dir));
                let _ = writeln!(
                    out,
                    "_ep_secret=$(od -An -tx1 -N{} /dev/urandom | tr -d ' \\n')",
                    sound::SECRET_BYTES
                );
                // The configuration holds no `$` but the secret's.
                let text = sound::config(&socket, "${_ep_secret}");
                let _ = write!(
                    out,
                    "cat > {} <<EOF_EASYPLAY_SOUND\n{text}EOF_EASYPLAY_SOUND\n",
                    q(&conf)
                );
                let _ = writeln!(
                    out,
                    "env -i XDG_RUNTIME_DIR=\"$XDG_RUNTIME_DIR\" PIPEWIRE_REMOTE={} PIPEWIRE_CONFIG_DIR={} {} -c {} >>{} 2>&1 &",
                    q(remote),
                    q(dir),
                    q(program),
                    q(&conf),
                    q(log)
                );
                let _ = writeln!(out, "while [ ! -S {} ]; do sleep 0.05; done", q(&socket));
                let _ = writeln!(out, "# stop it afterwards: kill $!");
            }
            HostAction::CreateWaylandContext { socket, .. } => {
                let _ = writeln!(
                    out,
                    "# the restricted Wayland socket {} (ADR-0029) is registered with the compositor by EasyPlay; a shell cannot",
                    q(socket)
                );
                let _ = writeln!(
                    out,
                    "echo 'This script cannot create the restricted Wayland socket; not starting with the socket of the compositor instead.' >&2"
                );
                let _ = writeln!(out, "exit 1");
            }
        }
    }

    for fd in &plan.sandbox.fds {
        let mut content = fd.content().unwrap_or_default();
        let mut b64: String = fd.content_b64.clone();
        if fd.kind == FdKind::Seccomp && content.is_empty() {
            match seccomp::program(seccomp::Profile::for_args(&plan.sandbox.args)) {
                Ok(p) => {
                    content = p.to_vec();
                    b64 = B64.encode(p);
                }
                Err(_) => {
                    let _ = writeln!(
                        out,
                        "echo 'This EasyPlay build cannot reproduce the system-call filter; not starting without it.' >&2"
                    );
                    let _ = writeln!(out, "exit 1");
                    continue;
                }
            }
        }
        let target = match (fd.kind, &fd.target) {
            (FdKind::Seccomp, _) => "system-call filter (seccomp program)".to_owned(),
            (_, Some(t)) => t.display().to_string(),
            (_, None) => String::new(),
        };
        let delim = format!("EOF_EASYPLAY_{}", fd.fd);
        let text = std::str::from_utf8(&content)
            .ok()
            .filter(|_| fd.kind == FdKind::Data)
            .filter(|t| t.ends_with('\n') && !t.lines().any(|l| l == delim) && !t.contains('\0'));
        match text {
            Some(t) => {
                let _ = writeln!(out, "# {target}");
                let _ = write!(out, "exec {}<<'{delim}'\n{t}{delim}\n", fd.fd);
            }
            None => {
                let _ = writeln!(out, "# {target} (binary or without final newline)");
                let _ = writeln!(out, "_ep_tmp=$(mktemp)");
                let _ = writeln!(out, "printf '%s' {} | base64 -d > \"$_ep_tmp\"", q(&b64));
                let _ = writeln!(out, "exec {}<\"$_ep_tmp\"", fd.fd);
                let _ = writeln!(out, "rm -f \"$_ep_tmp\"");
            }
        }
    }

    let _ = writeln!(out, "exec {} \\", q(&plan.bwrap));
    for g in group_args(&plan.sandbox.args) {
        let opt = g.first().and_then(|o| o.to_str()).unwrap_or("");
        if matches!(opt, "--json-status-fd" | "--sync-fd") {
            continue;
        }
        let line: Vec<String> = g.iter().map(|a| quote(a)).collect();
        let _ = writeln!(out, "  {} \\", line.join(" "));
    }
    let mut cmd = plan.sandbox.command.clone();
    if cmd.get(1).is_some_and(|a| a == "--status-fd") && cmd.len() > 2 {
        cmd.drain(1..3);
    }
    let cmd: Vec<String> = cmd.iter().map(|a| quote(a)).collect();
    let _ = writeln!(out, "  -- {}", cmd.join(" "));
    out
}
