//! The stack guard that makes deep recursion a catchable `StackOverflowError`.
//!
//! dat code runs on threads dat-rt creates, with a known stack size.
//! Recursive functions compare the address of a local with a limit on entry.
//! The red zone left below the limit covers the callee's own frame and the
//! dat-rt and std calls made between two checks.

use crate::{DatError, throw_at};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The stack of the thread that runs compiled dat code. Generated frames are
/// small (16 bytes for a simple recursive function), so 16 MiB holds about a
/// million of them, and 20,000 or more of the largest, a function with a
/// `try` and strings. It isn't larger because an error that escapes is
/// traced by reading every native frame through std, which costs about 3 µs
/// and 500 bytes per frame.
pub const STACK_SIZE: usize = 16 << 20;

const RED_ZONE: usize = 256 << 10;

/// The lowest stack address dat code may reach. One dat thread runs at a time.
pub struct StackLimit(AtomicUsize);

impl StackLimit {
    #[allow(clippy::new_without_default)]
    pub const fn new() -> StackLimit {
        StackLimit(AtomicUsize::new(0))
    }
}

/// The interpreter's limit. Generated code declares its own, so the check
/// reads a static of its own crate instead of going through the GOT.
static LIMIT: StackLimit = StackLimit::new();

/// The generated program's limit, set along with [`LIMIT`].
static COMPILED: OnceLock<&'static StackLimit> = OnceLock::new();

pub(crate) fn set_compiled_limit(limit: &'static StackLimit) {
    let _ = COMPILED.set(limit);
}

#[inline(always)]
fn stack_pointer() -> usize {
    let marker = 0u8;
    &marker as *const u8 as usize
}

/// Called on entry to every recursive function in generated code, with the
/// program's own limit.
#[inline(always)]
pub fn stack_check(limit: &StackLimit) {
    if stack_pointer() < limit.0.load(Ordering::Relaxed) {
        overflow()
    }
}

/// The interpreter's form of [`stack_check`].
#[inline(always)]
pub fn stack_is_low() -> bool {
    stack_pointer() < LIMIT.0.load(Ordering::Relaxed)
}

/// The call that overflowed is reported in the caller, at the call: the
/// trace starts at the first frame below this one.
#[cold]
#[inline(never)]
fn overflow() -> ! {
    throw_at(DatError::stack_overflow(), None)
}

/// Sets the limit for the current thread, whose stack is `size` bytes.
pub(crate) fn set_limit(size: usize) {
    let limit = stack_pointer().saturating_sub(size.saturating_sub(RED_ZONE));
    LIMIT.0.store(limit, Ordering::Relaxed);
    if let Some(l) = COMPILED.get() {
        l.0.store(limit, Ordering::Relaxed);
    }
}
