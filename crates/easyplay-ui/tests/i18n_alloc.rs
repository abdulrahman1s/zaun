//! A text lookup allocates nothing once its key has been asked for (AGENTS.md §6: no
//! allocation in per-frame UI code; every label calls `i18n::text` in every frame). A binary
//! of its own: it installs a global allocator that counts this thread's allocations.

#![allow(unsafe_code, clippy::unwrap_used)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use easyplay_ui::i18n::{self, Language};

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

/// The system allocator, counting the allocations of a thread that turned counting on.
struct Counting;

fn note() {
    // `try_with`: thread-local storage may already be gone while a thread exits.
    if COUNTING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
    }
}

// SAFETY: every call is passed unchanged to `System`, which upholds `GlobalAlloc`'s
// contract; `note` only touches const-initialised thread-locals without destructors, so it
// never allocates or re-enters the allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: the caller's guarantees for `layout` are passed on as they are.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: as in `alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        // SAFETY: `ptr` came from this allocator, that is from `System`, with `layout`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator, that is from `System`, with `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Allocations made on this thread while `f` runs.
fn allocations(f: impl FnOnce()) -> usize {
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|c| c.set(true));
    f();
    COUNTING.with(|c| c.set(false));
    ALLOCATIONS.with(Cell::get)
}

#[test]
fn a_text_asked_for_before_allocates_nothing() {
    const KEYS: [&str; 3] = ["language.name", "sample.greeting", "no.such.key"];
    for key in KEYS {
        i18n::text(key);
        i18n::text_in(Language::Arabic, key);
    }
    let n = allocations(|| {
        for _ in 0..100 {
            for key in KEYS {
                assert!(!i18n::text(key).is_empty());
                assert!(!i18n::text_in(Language::Arabic, key).is_empty());
            }
        }
    });
    assert_eq!(n, 0, "allocations in 600 lookups of known keys");
}

#[test]
fn the_counter_sees_an_allocation() {
    assert!(allocations(|| drop(std::hint::black_box(vec![1u8; 64]))) >= 1);
}
