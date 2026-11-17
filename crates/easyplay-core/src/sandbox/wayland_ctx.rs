//! The session's restricted Wayland socket (ADR-0029, `wp_security_context_v1`).
//!
//! When the compositor offers security contexts (`WaylandSocket::security_context`, asked
//! by the host probe), the planner binds a socket of the session's own at the
//! `WAYLAND_DISPLAY` name instead of the compositor's socket, and adds
//! [`HostAction::CreateWaylandContext`] so the supervisor registers it with the compositor
//! before the sandbox starts (`host::wayland`). Clients connecting through it are marked as
//! sandboxed and the compositor decides which protocols to withhold from them. Otherwise the
//! compositor's socket is bound as before and the plan notes it.
//!
//! Only the Wayland connection is restricted: X11 clients reach the X server (Xwayland) as
//! before (THREAT_MODEL T-15).

use std::path::PathBuf;

use crate::host::HostFacts;
use crate::sandbox::context::SandboxContext;
use crate::sandbox::plan::{HostAction, SOCKET_PATH_MAX};

/// File name of the restricted socket in the session's runtime directory.
pub const CONTEXT_SOCKET: &str = "wayland";

/// The session's restricted socket and the action that creates it: `None` when the
/// compositor does not offer security contexts or the socket's path is too long for a
/// socket address.
pub fn context_action(ctx: &SandboxContext<'_>, host: &HostFacts) -> Option<(PathBuf, HostAction)> {
    let w = host
        .display
        .wayland
        .as_ref()
        .filter(|w| w.security_context)?;
    let socket: PathBuf = ctx.session.run_dir.join(CONTEXT_SOCKET);
    if socket.as_os_str().len() > SOCKET_PATH_MAX {
        return None;
    }
    let app_id = ctx
        .game
        .map_or_else(|| ctx.kind.as_str().to_owned(), |g| g.id.to_string());
    let action = HostAction::CreateWaylandContext {
        display: w.path.clone(),
        socket: socket.clone(),
        app_id,
        instance_id: ctx.session.id.to_string(),
    };
    Some((socket, action))
}

/// The restricted sockets the plan's actions create.
pub fn context_sockets(actions: &[HostAction]) -> impl Iterator<Item = &PathBuf> {
    actions.iter().filter_map(|a| match a {
        HostAction::CreateWaylandContext { socket, .. } => Some(socket),
        _ => None,
    })
}
