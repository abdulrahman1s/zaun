//! §4.7 `/tmp`, `/run`, display and sound sockets.
//!
//! The X11 socket is bound *after* `--tmpfs /tmp` in this section (a bind before the tmpfs
//! would be hidden by it), and it is always the filesystem socket `/tmp/.X11-unix/X<n>`: the
//! abstract socket `@/tmp/.X11-unix/X<n>` belongs to the host network namespace and is
//! unreachable under `--unshare-net` (TECHNICAL_RESEARCH F-221). Nothing else from `$XRD`
//! is ever bound.

use std::path::Path;

use super::{Effective, SandboxDirs, etc_files, mk};
use crate::host::HostFacts;
use crate::model::{AudioAccess, ControllerAccess};
use crate::sandbox::context::SandboxContext;
use crate::sandbox::plan::{
    EnvOrigin, HostAction, PROTON_LOG_PATH, PlanBuilder, PolicyField, RUN_EASYPLAY, SMALL_TMP_BYTES,
};
use crate::sandbox::validate::{IssueCode, PolicyIssue};

/// File name of the X authority copy inside the private runtime directory.
pub const XAUTH_NAME: &str = "easyplay-xauth";

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
    b.mount(mk::dir(RUN_EASYPLAY), "EasyPlay mounts", None);
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
        b.mount(
            mk::ro(&w.path, xrd.join(&w.name)),
            "Wayland display socket",
            Some(PolicyField::Wayland),
        );
        b.env("WAYLAND_DISPLAY", w.name.clone(), EnvOrigin::Fixed);
    }
    if eff.audio == AudioAccess::Full {
        let field = Some(PolicyField::Audio);
        match &host.audio.pulse_socket {
            Some(pulse) => {
                let dst = xrd.join("pulse/native");
                b.mount(
                    mk::dir(xrd.join("pulse")),
                    "PulseAudio directory",
                    field.clone(),
                );
                b.mount(mk::ro(pulse, &dst), "PulseAudio socket", field.clone());
                b.env(
                    "PULSE_SERVER",
                    format!("unix:{}", dst.display()),
                    EnvOrigin::Fixed,
                );
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
