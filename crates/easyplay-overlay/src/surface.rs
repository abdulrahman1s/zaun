//! A wgpu surface on the layer surface (ADR-0037 §2). The one place with `unsafe` in this
//! crate: wgpu takes libwayland's raw display and surface pointers, and no safe constructor
//! exists for a surface that is not a winit window (ARCHITECTURE §3 rule 5).
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::NonNull;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, RawDisplayHandle, RawWindowHandle,
    WaylandDisplayHandle, WaylandWindowHandle,
};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Proxy};

/// libwayland's `wl_display` of our connection, for wgpu's GL backend (EGL needs it when the
/// instance is made).
#[derive(Debug)]
pub struct Display(NonNull<c_void>);

// SAFETY: a `wl_display` pointer may be used from any thread (libwayland locks internally);
// wgpu only reads it, and `App` keeps the connection alive longer than the instance.
unsafe impl Send for Display {}
// SAFETY: as above; nothing is written through it.
unsafe impl Sync for Display {}

impl Display {
    /// `None` when the connection is not backed by libwayland (`client_system` is off).
    pub fn of(conn: &Connection) -> Option<Self> {
        NonNull::new(conn.backend().display_ptr().cast::<c_void>()).map(Self)
    }

    fn raw(&self) -> RawDisplayHandle {
        RawDisplayHandle::Wayland(WaylandDisplayHandle::new(self.0))
    }
}

impl HasDisplayHandle for Display {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: the pointer is libwayland's live display for as long as `self` exists (the
        // connection outlives it: `App` drops the GPU first).
        Ok(unsafe { DisplayHandle::borrow_raw(self.raw()) })
    }
}

/// A wgpu surface presenting into `surface`.
///
/// The caller must drop the returned surface before `surface` and the connection go away;
/// `App` keeps the GPU in a field declared (and so dropped) before the Wayland objects.
pub fn create(
    instance: &wgpu::Instance,
    display: &Display,
    surface: &WlSurface,
) -> Result<wgpu::Surface<'static>, String> {
    let ptr = NonNull::new(surface.id().as_ptr().cast::<c_void>())
        .ok_or("the layer surface has no libwayland proxy")?;
    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(display.raw()),
        raw_window_handle: RawWindowHandle::Wayland(WaylandWindowHandle::new(ptr)),
    };
    // SAFETY: both pointers are live libwayland objects of this connection, and the surface
    // made from them is dropped before either (see above).
    unsafe { instance.create_surface_unsafe(target) }.map_err(|e| e.to_string())
}
