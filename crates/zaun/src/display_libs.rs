//! Choosing winit's windowing backend before its event loop exists.
//!
//! winit creates its event loop once per process: when the Wayland client library cannot be
//! loaded (a binary started outside its package's `LD_LIBRARY_PATH`, e.g. on NixOS), the
//! first attempt fails and any retry only reports "EventLoop can't be recreated". So the
//! libraries are probed first (`dlopen` through the same crates winit uses): Wayland when it
//! is the session and loadable, X11 (Xwayland) when Wayland is not loadable but X11 is, and
//! otherwise a clear `ZN-HOST-020` naming the missing libraries.

use zaun_core::paths::PathEnv;
use zaun_core::{Error, ErrorCode};
use zaun_ui::DisplayBackend;

/// What could be loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loadable {
    /// `libwayland-client.so.0`.
    pub wayland: bool,
    /// `libX11.so.6` and `libX11-xcb.so.1`.
    pub x11: bool,
    /// `libxkbcommon.so.0` (both backends need it for the keyboard).
    pub xkb: bool,
}

impl Loadable {
    /// Probe this process (each library is opened once and stays loaded for winit).
    pub fn probe() -> Self {
        Self {
            wayland: wayland_sys::client::is_lib_available(),
            x11: x11_dl::xlib::Xlib::open().is_ok() && x11_dl::xlib_xcb::Xlib_xcb::open().is_ok(),
            xkb: xkbcommon_dl::xkbcommon_option().is_some(),
        }
    }
}

fn set(env: &PathEnv, key: &str) -> bool {
    env.get(key).is_some_and(|v| !v.is_empty())
}

/// The backend to use, or why there is none.
pub fn choose(env: &PathEnv, libs: Loadable) -> Result<DisplayBackend, Error> {
    let wayland_session = set(env, "WAYLAND_DISPLAY") || set(env, "WAYLAND_SOCKET");
    let x11_session = set(env, "DISPLAY");
    if !wayland_session && !x11_session {
        return Err(Error::new(ErrorCode::HostNoDisplay)
            .with("detail", "neither WAYLAND_DISPLAY nor DISPLAY is set"));
    }
    if libs.xkb {
        if wayland_session && libs.wayland {
            return Ok(DisplayBackend::Auto);
        }
        if x11_session && libs.x11 {
            if wayland_session {
                tracing::warn!("libwayland-client could not be loaded; using X11 (Xwayland)");
                return Ok(DisplayBackend::X11);
            }
            return Ok(DisplayBackend::Auto);
        }
    }
    let mut missing = Vec::new();
    if wayland_session && !libs.wayland {
        missing.push("libwayland-client.so.0");
    }
    if x11_session && !libs.x11 {
        missing.push("libX11.so.6");
    }
    if !libs.xkb {
        missing.push("libxkbcommon.so.0");
    }
    if !x11_session && wayland_session && libs.wayland {
        // Only xkbcommon is missing.
    } else if !x11_session {
        missing.push("(no DISPLAY for an X11 fallback)");
    }
    Err(Error::new(ErrorCode::HostNoDisplay).with(
        "detail",
        format!(
            "the display client libraries could not be loaded: {}; install them, or start \
             the packaged zaun (its wrapper sets LD_LIBRARY_PATH)",
            missing.join(", ")
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Loadable = Loadable {
        wayland: true,
        x11: true,
        xkb: true,
    };

    fn env(w: bool, x: bool) -> PathEnv {
        let mut e = PathEnv::new(1000);
        if w {
            e.set("WAYLAND_DISPLAY", "wayland-0");
        }
        if x {
            e.set("DISPLAY", ":0");
        }
        e
    }

    #[test]
    fn backends() {
        assert_eq!(choose(&env(true, true), ALL).unwrap(), DisplayBackend::Auto);
        assert_eq!(
            choose(&env(false, true), ALL).unwrap(),
            DisplayBackend::Auto
        );
        let no_wl = Loadable {
            wayland: false,
            ..ALL
        };
        assert_eq!(
            choose(&env(true, true), no_wl).unwrap(),
            DisplayBackend::X11
        );
        let e = choose(&env(true, false), no_wl).unwrap_err();
        assert_eq!(e.code, ErrorCode::HostNoDisplay);
        assert!(
            e.context_value("detail")
                .unwrap()
                .contains("libwayland-client")
        );
        let none = Loadable {
            wayland: false,
            x11: false,
            xkb: false,
        };
        let d = choose(&env(true, true), none).unwrap_err();
        let d = d.context_value("detail").unwrap();
        assert!(d.contains("libX11") && d.contains("libxkbcommon"), "{d}");
        assert!(choose(&env(false, false), ALL).is_err());
    }
}
