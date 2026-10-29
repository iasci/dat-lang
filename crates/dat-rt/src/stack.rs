//! The stack guard that makes deep recursion a catchable `StackOverflowError`.
//!
//! dat code runs on threads dat-rt creates, with a known stack size. Recursive
//! functions compare the address of their caller's [`Frame`] record, which
//! lives in the caller's stack frame and arrives in a register, with a limit
//! on entry. Reading the stack pointer itself measured slower. The red zone
//! left below the limit covers the callee's own frame and the dat-rt and std
//! calls made between two checks.

use crate::{DatError, Frame, Loc};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The stack of the thread that runs compiled dat code.
pub const STACK_SIZE: usize = 64 << 20;

const RED_ZONE: usize = 256 << 10;

/// The lowest stack address dat code may reach. One dat thread runs at a time.
static LIMIT: AtomicUsize = AtomicUsize::new(0);

#[inline(always)]
fn stack_pointer() -> usize {
    let marker = 0u8;
    &marker as *const u8 as usize
}

/// Called on entry to every recursive function in generated code, with the
/// caller's record and the call's location. Records live on the dat thread's
/// stack, so the caller's record marks how deep the stack is.
#[inline(always)]
pub fn stack_check(up: &Frame<'_>, at: Option<&'static Loc>) {
    if (up as *const Frame<'_> as usize) < LIMIT.load(Ordering::Relaxed) {
        overflow(up, at)
    }
}

/// The interpreter's form of [`stack_check`].
#[inline(always)]
pub fn stack_is_low() -> bool {
    stack_pointer() < LIMIT.load(Ordering::Relaxed)
}

/// The call that overflowed is reported in the caller, at the call.
#[cold]
#[inline(never)]
fn overflow(up: &Frame<'_>, at: Option<&'static Loc>) -> ! {
    match at {
        Some(at) => crate::throw(DatError::stack_overflow(), at, up),
        None => std::panic::resume_unwind(DatError::stack_overflow()),
    }
}

/// Sets the limit for the current thread, whose stack is `size` bytes.
pub(crate) fn set_limit(size: usize) {
    let base = stack_pointer();
    LIMIT.store(base.saturating_sub(size.saturating_sub(RED_ZONE)), Ordering::Relaxed);
}
