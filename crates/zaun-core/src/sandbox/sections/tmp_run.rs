//! §4.7 `/tmp`, `/run`, display and sound sockets.
//!
//! The X11 socket is bound *after* `--tmpfs /tmp` in this section (a bind before the tmpfs
//! would be hidden by it), and it is always the filesystem socket `/tmp/.X11-unix/X<n>`: the
//! abstract socket `@/tmp/.X11-unix/X<n>` belongs to the host network namespace and is
//! unreachable under `--unshare-net` (TECHNICAL_RESEARCH F-221). Nothing else from `$XRD`
//! is ever bound, except the sockets of the session's own sound server (ADR-0026) and of
//! its restricted Wayland connection (ADR-0029).

use std::path::{Path, PathBuf};

use super::{Effective, SandboxDirs, etc_files, mk};
use crate::host::HostFacts;
use crate::model::{AudioAccess, ControllerAccess};
use crate::sandbox::context::SandboxContext;
use crate::sandbox::plan::{
    EnvOrigin, HostAction, PROTON_LOG_PATH, PlanBuilder, PolicyField, RUN_ZAUN, SMALL_TMP_BYTES,
    SOCKET_PATH_MAX, sound_socket,
};
use crate::sandbox::validate::{IssueCode, PolicyIssue};
use crate::sandbox::wayland_ctx;

/// File name of the X authority copy inside the private runtime directory.
pub const XAUTH_NAME: &str = "zaun-xauth";

/// Bind `socket` as the PulseAudio socket of the private runtime directory.
fn pulse_socket(
    b: &mut PlanBuilder,
    xrd: &Path,
    socket: &Path,
    what: &'static str,
    field: PolicyField,
) {
    let dst = xrd.join("pulse/native");
    b.mount(
        mk::dir(xrd.join("pulse")),
        "PulseAudio directory",
        Some(PolicyField::Audio),
    );
    b.mount(mk::ro(socket, &dst), what, Some(field));
    b.env(
        "PULSE_SERVER",
        format!("unix:{}", dst.display()),
        EnvOrigin::Fixed,
    );
}

/// The session's own sound server, when this host can run one: the `pipewire` program, the
/// host's PipeWire socket to play through, and a socket path short enough to bind.
pub fn sound_server(ctx: &SandboxContext<'_>, host: &HostFacts) -> Option<HostAction> {
    let program = host.audio.pipewire_program.clone()?;
    let remote = host.audio.pipewire_socket.clone()?;
    let dir: PathBuf = ctx.session.run_dir.join("sound");
    if sound_socket(&dir).as_os_str().len() > SOCKET_PATH_MAX {
        return None;
    }
    Some(HostAction::StartSoundServer {
        program,
        dir,
        remote,
        log: ctx.session.log_dir.join("sound-server.log"),
    })
}

/// Sound without the microphone: the session's own sound server, which refuses to record,
/// and not the host's PipeWire socket, which records whatever its server is told to.
/// `Effective` has already turned sound off where there can be no such server.
fn playback_only(ctx: &SandboxContext<'_>, host: &HostFacts, xrd: &Path, b: &mut PlanBuilder) {
    let Some(action) = sound_server(ctx, host) else {
        return;
    };
    let socket = match &action {
        HostAction::StartSoundServer { dir, .. } => sound_socket(dir),
        _ => return,
    };
    b.action(action);
    pulse_socket(
        b,
        xrd,
        &socket,
        "sound server that refuses recording",
        PolicyField::Audio,
    );
}

/// §4.7.
pub fn emit(ctx: &SandboxContext<'_>, host: &HostFacts, eff: &Effective, b: &mut PlanBuilder) {
    let dirs = SandboxDirs::new(ctx.home, ctx.uid);
    let xrd = &dirs.xrd;
    let size = eff.trusted_network.then_some(SMALL_TMP_BYTES);
    b.mount(mk::tmpfs("/tmp", Some(0o1777), size), "private /tmp", None);
    b.mount(mk::dir("/run"), "empty /run", None);
    b.mount(
        mk::tmpfs(xrd, Some(0o700), None),
        "private runtime directory",
        None,
    );
    b.mount(mk::dir(RUN_ZAUN), "Zaun mounts", None);
    b.env(
        "XDG_RUNTIME_DIR",
        xrd.display().to_string(),
        EnvOrigin::Fixed,
    );

    if !eff.wayland && !eff.x11 {
        b.issue(PolicyIssue::error(
            IssueCode::NoDisplay,
            None,
            "sec.display.none|No display is available to open a window",
        ));
    }
    if eff.x11
        && let Some(x11) = host.display.x11_bindable()
    {
        let n = x11.number;
        let dst = Path::new("/tmp/.X11-unix").join(format!("X{n}"));
        b.mount(
            mk::dir("/tmp/.X11-unix"),
            "X11 socket directory",
            Some(PolicyField::X11),
        );
        b.mount(
            mk::ro(&x11.socket, &dst),
            "X11 display socket",
            Some(PolicyField::X11),
        );
        b.env("DISPLAY", format!(":{n}"), EnvOrigin::Fixed);
        if let Some(auth) = &x11.authority {
            let dst = xrd.join(XAUTH_NAME);
            b.mount(mk::ro(auth, &dst), "X11 authority", Some(PolicyField::X11));
            b.env("XAUTHORITY", dst.display().to_string(), EnvOrigin::Fixed);
        }
    }
    if eff.wayland
        && let Some(w) = &host.display.wayland
    {
        let dst = xrd.join(&w.name);
        match wayland_ctx::context_action(ctx, host) {
            Some((socket, action)) => {
                b.action(action);
                b.mount(
                    mk::ro(&socket, &dst),
                    "restricted Wayland socket (security context)",
                    Some(PolicyField::Wayland),
                );
            }
            None => {
                b.mount(
                    mk::ro(&w.path, &dst),
                    "Wayland display socket",
                    Some(PolicyField::Wayland),
                );
                b.note(
                    "note.display.no_wayland_context",
                    "The compositor cannot restrict this Wayland connection: the game gets the compositor's own socket",
                );
            }
        }
        b.env("WAYLAND_DISPLAY", w.name.clone(), EnvOrigin::Fixed);
    }
    if eff.audio == AudioAccess::Playback {
        playback_only(ctx, host, xrd, b);
    }
    if eff.playback_unavailable {
        b.note(
            "note.audio.no_playback_only",
            "This computer cannot give sound without the microphone; the game has no sound",
        );
    }
    if eff.audio == AudioAccess::Full {
        // The host's sockets play and record alike.
        let field = Some(PolicyField::Microphone);
        match &host.audio.pulse_socket {
            Some(pulse) => {
                pulse_socket(b, xrd, pulse, "PulseAudio socket", PolicyField::Microphone)
            }
            None => b.note(
                "note.audio.no_pulse",
                "PulseAudio socket not found; sound through PipeWire only",
            ),
        }
        match &host.audio.pipewire_socket {
            Some(pw) => b.mount(mk::ro(pw, xrd.join("pipewire-0")), "PipeWire socket", field),
            None => b.note(
                "note.audio.no_pipewire",
                "PipeWire socket not found; sound through PulseAudio only",
            ),
        }
    }
    if eff.controllers != ControllerAccess::None && host.input.udev_db {
        b.mount(
            mk::ro_same("/run/udev"),
            "controller metadata (udev database)",
            Some(PolicyField::Controllers),
        );
    }
    if let Some(dir) = etc_files::resolver_dir(host, eff) {
        b.mount(
            mk::ro_same(&dir),
            "name resolver directory",
            Some(PolicyField::Network),
        );
    }
    if ctx.launch.compat.proton_log && ctx.game.is_some() {
        let src = ctx.session.log_dir.join("proton");
        b.action(HostAction::CreateDir {
            path: src.clone(),
            mode: 0o700,
        });
        b.mount(mk::rw(&src, PROTON_LOG_PATH), "Proton log directory", None);
        b.env("PROTON_LOG", "1", EnvOrigin::Option("proton_log".into()));
        b.env(
            "PROTON_LOG_DIR",
            PROTON_LOG_PATH,
            EnvOrigin::Option("proton_log".into()),
        );
    }
}
