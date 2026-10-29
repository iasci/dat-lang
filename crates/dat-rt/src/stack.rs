//! The stack guard that makes deep recursion a catchable `StackOverflowError`.
//!
//! dat code runs on threads dat-rt creates, with a known stack size.
//! Recursive functions compare the stack pointer with a limit on entry.
//! The red zone left below the limit covers the callee's own frame and the
//! dat-rt and std calls made between two checks.

use crate::{DatError, throw_at};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The stack of the thread that runs compiled dat code. Generated frames are
/// small (16 bytes for a simple recursive function), so 32 MiB holds two
/// million or more of them, and 128,000 or more of the largest measured, a
/// function with a `try` and strings. A larger stack goes deeper, but
/// catching an overflow then takes longer, since unwinding grows with depth.
pub const STACK_SIZE: usize = 32 << 20;

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

/// The stack pointer, read from its register. The address of a local would
/// do, but it costs every checked function a stack slot.
#[inline(always)]
#[allow(unsafe_code)]
fn stack_pointer() -> usize {
    let sp: usize;
    // SAFETY: copies the stack pointer into a register; reads and writes no
    // memory, leaves the stack and the flags alone.
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!("mov {}, rsp", out(reg) sp, options(pure, nomem, nostack, preserves_flags))
    };
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!("mov {}, sp", out(reg) sp, options(pure, nomem, nostack, preserves_flags))
    };
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let marker = 0u8;
        sp = &marker as *const u8 as usize;
    }
    sp
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
