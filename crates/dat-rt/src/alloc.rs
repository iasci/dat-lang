//! Running out of memory. The allocator is the system's; when it fails,
//! the program prints one message and aborts, the same way in both run
//! modes. Rust's own message names the size that failed, which differs
//! between them: an interpreted value is larger than a compiled one.
//!
//! A list or string longer than [`crate::MAX_LEN`] is a catchable
//! `RangeError` instead, decided before anything is allocated, so it's the
//! same on every machine and in both modes.
//!
//! It also counts, for each thread, the blocks it allocated and hasn't
//! freed, while the debug checks ask it to (see `checks.rs`).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

static COUNTING: AtomicBool = AtomicBool::new(false);

thread_local! {
    // `const` and without a destructor, so using it never allocates.
    static LIVE: Cell<isize> = const { Cell::new(0) };
}

/// Counts every allocation and free from now on.
pub(crate) fn count_blocks() {
    COUNTING.store(true, Ordering::Relaxed);
}

/// The blocks this thread allocated and hasn't freed since counting started.
pub(crate) fn live_blocks() -> isize {
    LIVE.with(Cell::get)
}

#[inline(always)]
fn counted(change: isize) {
    if COUNTING.load(Ordering::Relaxed) {
        let _ = LIVE.try_with(|l| l.set(l.get() + change));
    }
}

struct Dat;

#[allow(unsafe_code)]
// SAFETY: every method passes its arguments to `System` unchanged, so it
// keeps `System`'s guarantees; a failed allocation never returns. Counting
// touches an atomic and a thread-local that never allocates.
unsafe impl GlobalAlloc for Dat {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if p.is_null() {
            out_of_memory();
        }
        counted(1);
        p
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if p.is_null() {
            out_of_memory();
        }
        counted(1);
        p
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        counted(-1);
        unsafe { System.dealloc(ptr, layout) }
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if p.is_null() {
            out_of_memory();
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: Dat = Dat;

/// Writes the message without allocating, then aborts: there's no memory
/// to unwind with, so it can't be a catchable error.
#[cold]
#[inline(never)]
fn out_of_memory() -> ! {
    let _ = std::io::stderr().write_all(b"error: out of memory\n");
    std::process::abort()
}
