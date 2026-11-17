//! The compositor's security contexts (`wp_security_context_v1`, ADR-0029): whether the
//! compositor offers them, and creating one restricted listening socket for a session.
//!
//! A client that connects through a socket registered as a security context is marked as
//! sandboxed, and the compositor decides which of its protocols such a client may use (niri,
//! for example, withholds screen capture, clipboard managers, virtual input and layer
//! shell). The context stays registered after this process disconnects, until every write
//! end of the pipe whose read end was handed over as the close descriptor is closed; the
//! supervisor hands over the read end of the sandbox's `--sync-fd` pipe, which `bwrap` holds
//! open for as long as the sandbox exists.
//!
//! Every wait for the compositor is bounded by [`COMPOSITOR_TIMEOUT`], so a stalled
//! compositor costs the probe or the launch two seconds, not forever. The compositor is
//! always reached through the socket path found by the probe, never through the
//! environment of whoever runs this.
//!
//! Without the `wayland-security-context` feature nothing here talks to a compositor:
//! [`LiveCompositor`] reports no support and [`create_security_context`] fails.

use std::os::fd::BorrowedFd;
use std::path::Path;
use std::time::Duration;

use crate::error::{Error, ErrorCode};

/// The global a compositor offers security contexts through.
pub const SECURITY_CONTEXT_MANAGER: &str = "wp_security_context_manager_v1";

/// The sandbox engine name the compositor sees on every context.
pub const SANDBOX_ENGINE: &str = crate::consts::APP_ID;

/// The longest any single wait for the compositor may take.
pub const COMPOSITOR_TIMEOUT: Duration = Duration::from_secs(2);

/// Whether the compositor offers security contexts (the probe's seam).
pub trait CompositorProbe: Send + Sync {
    /// Whether the compositor listening at `display` offers
    /// [`SECURITY_CONTEXT_MANAGER`] to this process. Any failure is `false`.
    fn offers_security_context(&self, display: &Path) -> bool;
}

/// Asks the real compositor.
#[derive(Clone, Copy, Debug, Default)]
pub struct LiveCompositor;

impl CompositorProbe for LiveCompositor {
    fn offers_security_context(&self, display: &Path) -> bool {
        match globals(display) {
            Ok(g) => g.iter().any(|i| i == SECURITY_CONTEXT_MANAGER),
            Err(e) => {
                let socket = display.display().to_string();
                tracing::info!(socket, "cannot list the compositor's globals: {e}");
                false
            }
        }
    }
}

/// Never reaches a compositor and reports no support (tests, fixtures).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoCompositor;

impl CompositorProbe for NoCompositor {
    fn offers_security_context(&self, _display: &Path) -> bool {
        false
    }
}

/// One security context to create.
#[derive(Clone, Copy, Debug)]
pub struct ContextRequest<'a> {
    /// The compositor's socket.
    pub display: &'a Path,
    /// The restricted socket to create; it must not exist yet.
    pub socket: &'a Path,
    /// What the compositor sees as the application (the game's ID).
    pub app_id: &'a str,
    /// What the compositor sees as this instance (the session's ID).
    pub instance_id: &'a str,
}

fn wayland_err(detail: impl std::fmt::Display) -> Error {
    Error::new(ErrorCode::SbxSetupFailed)
        .with("step", "Wayland security context")
        .with("detail", detail)
}

/// The interface names of the globals the compositor at `display` offers to this process.
pub fn globals(display: &Path) -> Result<Vec<String>, Error> {
    #[cfg(feature = "wayland-security-context")]
    {
        client::globals(display).map_err(wayland_err)
    }
    #[cfg(not(feature = "wayland-security-context"))]
    {
        let _ = display;
        Err(Error::not_available("SANDBOX-wayland-security-context"))
    }
}

/// Create the restricted socket of `request` and register it with the compositor as a
/// security context that ends when `close_fd` hangs up (every write end of its pipe is
/// closed). Returns once the compositor has accepted it; the socket file stays.
pub fn create_security_context(
    request: &ContextRequest<'_>,
    close_fd: BorrowedFd<'_>,
) -> Result<(), Error> {
    #[cfg(feature = "wayland-security-context")]
    {
        client::create(request, close_fd).map_err(|e| {
            wayland_err(e)
                .with("display", request.display.display())
                .with("socket", request.socket.display())
        })
    }
    #[cfg(not(feature = "wayland-security-context"))]
    {
        let _ = (request, close_fd);
        Err(Error::not_available("SANDBOX-wayland-security-context"))
    }
}

#[cfg(feature = "wayland-security-context")]
mod client {
    use std::os::fd::{AsFd as _, BorrowedFd};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;

    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use rustix::io::Errno;
    use wayland_client::backend::WaylandError;
    use wayland_client::protocol::{wl_callback, wl_registry};
    use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
    use wayland_protocols::wp::security_context::v1::client::wp_security_context_manager_v1::WpSecurityContextManagerV1;
    use wayland_protocols::wp::security_context::v1::client::wp_security_context_v1::WpSecurityContextV1;

    use super::{COMPOSITOR_TIMEOUT, ContextRequest, SANDBOX_ENGINE, SECURITY_CONTEXT_MANAGER};

    /// Reads one round trip may take before giving up (each bounded by the timeout).
    const MAX_READS: usize = 64;

    #[derive(Default)]
    pub(super) struct State {
        /// `(name, interface)` of every global seen.
        globals: Vec<(u32, String)>,
        /// The last `wl_display.sync` has been answered.
        synced: bool,
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for State {
        fn event(
            state: &mut Self,
            _: &wl_registry::WlRegistry,
            event: wl_registry::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                wl_registry::Event::Global {
                    name, interface, ..
                } => state.globals.push((name, interface)),
                wl_registry::Event::GlobalRemove { name } => {
                    state.globals.retain(|(n, _)| *n != name);
                }
                _ => {}
            }
        }
    }

    impl Dispatch<wl_callback::WlCallback, ()> for State {
        fn event(
            state: &mut Self,
            _: &wl_callback::WlCallback,
            event: wl_callback::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let wl_callback::Event::Done { .. } = event {
                state.synced = true;
            }
        }
    }

    delegate_noop!(State: ignore WpSecurityContextManagerV1);
    delegate_noop!(State: ignore WpSecurityContextV1);

    /// Wait until the compositor has handled every request sent so far.
    fn roundtrip(
        conn: &Connection,
        queue: &mut EventQueue<State>,
        state: &mut State,
    ) -> Result<(), String> {
        state.synced = false;
        conn.display().sync(&queue.handle(), ());
        let timeout = Timespec {
            tv_sec: i64::try_from(COMPOSITOR_TIMEOUT.as_secs()).unwrap_or(2),
            tv_nsec: 0,
        };
        for _ in 0..MAX_READS {
            queue.dispatch_pending(state).map_err(|e| e.to_string())?;
            if state.synced {
                return Ok(());
            }
            match queue.flush() {
                Ok(()) => {}
                Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.to_string()),
            }
            // `None`: events are already queued; dispatch them first.
            let Some(guard) = queue.prepare_read() else {
                continue;
            };
            let ready = {
                let fd = guard.connection_fd();
                let mut fds = [PollFd::new(&fd, PollFlags::IN)];
                poll(&mut fds, Some(&timeout))
            };
            match ready {
                Ok(0) => return Err("the compositor did not answer in time".into()),
                Ok(_) => {}
                Err(Errno::INTR) => continue,
                Err(e) => return Err(format!("waiting for the compositor: {e}")),
            }
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("the compositor kept sending without answering".into())
    }

    /// A connection whose globals have been read.
    struct Client {
        conn: Connection,
        queue: EventQueue<State>,
        state: State,
        registry: wl_registry::WlRegistry,
    }

    /// Connect to `display` and read its globals.
    fn connect(display: &Path) -> Result<Client, String> {
        let stream = UnixStream::connect(display)
            .map_err(|e| format!("cannot connect to the compositor: {e}"))?;
        let conn = Connection::from_socket(stream).map_err(|e| e.to_string())?;
        let mut queue = conn.new_event_queue();
        let registry = conn.display().get_registry(&queue.handle(), ());
        let mut state = State::default();
        roundtrip(&conn, &mut queue, &mut state)?;
        Ok(Client {
            conn,
            queue,
            state,
            registry,
        })
    }

    pub(super) fn globals(display: &Path) -> Result<Vec<String>, String> {
        let client = connect(display)?;
        Ok(client.state.globals.into_iter().map(|(_, i)| i).collect())
    }

    pub(super) fn create(
        request: &ContextRequest<'_>,
        close_fd: BorrowedFd<'_>,
    ) -> Result<(), String> {
        let Client {
            conn,
            mut queue,
            mut state,
            registry,
        } = connect(request.display)?;
        let Some(name) = state
            .globals
            .iter()
            .find(|(_, i)| i == SECURITY_CONTEXT_MANAGER)
            .map(|(n, _)| *n)
        else {
            return Err("the compositor does not offer security contexts".into());
        };
        let qh = queue.handle();
        let manager: WpSecurityContextManagerV1 = registry.bind(name, 1, &qh, ());
        let listener = UnixListener::bind(request.socket)
            .map_err(|e| format!("cannot create {}: {e}", request.socket.display()))?;
        let context = manager.create_listener(listener.as_fd(), close_fd, &qh, ());
        context.set_sandbox_engine(SANDBOX_ENGINE.to_owned());
        context.set_app_id(request.app_id.to_owned());
        context.set_instance_id(request.instance_id.to_owned());
        context.commit();
        context.destroy();
        manager.destroy();
        // A refusal (an invalid descriptor, a nested context) arrives as a protocol error.
        roundtrip(&conn, &mut queue, &mut state)
            .map_err(|e| format!("the compositor did not accept the context: {e}"))?;
        // The compositor holds its own copy of the listening socket now.
        drop(listener);
        Ok(())
    }
}
