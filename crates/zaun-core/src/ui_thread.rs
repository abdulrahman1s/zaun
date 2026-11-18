//! The UI-thread guard of PERFORMANCE §2 (task PERF-002).
//!
//! The UI crate marks the thread that runs a frame with [`enter`]; the real implementations
//! of the side-effect seams (`fsx::RealFs`, `storage`'s connection, `host::RealRunner`,
//! `session::RealSpawner`) call [`assert_not_ui_thread`] before blocking work. In debug
//! builds a call from a marked thread panics, so a UI test that reaches real I/O from a
//! screen fails; release builds compile the check away.
//!
//! The mark is scoped (an RAII [`UiThreadScope`]) rather than permanent so that tests which
//! drive a harness on their own thread may still prepare fixtures between frames.
//!
//! Policy validation for the editor (`sandbox::explain`) is the one documented exception:
//! it may `stat`, `lstat`, `readlink` and `realpath` the policy's own paths on the UI thread
//! (a handful of system calls per edit, PERFORMANCE §2). `RealFs` therefore does not assert
//! on those four operations.

use std::cell::Cell;

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// While alive, the current thread counts as the UI thread. Scopes nest.
#[must_use = "the thread is marked only while the scope is alive"]
#[derive(Debug)]
pub struct UiThreadScope {
    _not_send: std::marker::PhantomData<*const ()>,
}

/// Mark the current thread as the UI thread until the returned scope is dropped.
pub fn enter() -> UiThreadScope {
    DEPTH.with(|d| d.set(d.get().saturating_add(1)));
    UiThreadScope {
        _not_send: std::marker::PhantomData,
    }
}

impl Drop for UiThreadScope {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Whether the current thread is inside a UI-thread scope.
pub fn is_ui_thread() -> bool {
    DEPTH.with(|d| d.get() > 0)
}

/// Debug builds: panic when called on the UI thread. `what` names the blocking operation.
#[track_caller]
#[inline]
pub fn assert_not_ui_thread(what: &str) {
    if cfg!(debug_assertions) && is_ui_thread() {
        panic!("blocking call on the UI thread: {what} (PERFORMANCE §2)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_nest_and_end() {
        assert!(!is_ui_thread());
        let a = enter();
        let b = enter();
        drop(b);
        assert!(is_ui_thread());
        drop(a);
        assert!(!is_ui_thread());
        assert_not_ui_thread("nothing");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "blocking call on the UI thread: read")]
    fn asserts_inside_a_scope() {
        let _ui = enter();
        assert_not_ui_thread("read");
    }

    #[test]
    fn other_threads_are_not_marked() {
        let _ui = enter();
        std::thread::spawn(|| assert!(!is_ui_thread()))
            .join()
            .unwrap();
    }
}
