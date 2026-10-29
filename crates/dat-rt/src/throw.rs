//! Throwing and catching in generated code.
//!
//! A runtime error unwinds with `resume_unwind(Box<DatError>)`, which skips the
//! panic hook and costs nothing until something is thrown. `try`/`catch`
//! becomes [`catch`] around a closure.
//!
//! A throw captures the native stack for the trace ([`Trace::native`]) only
//! when the error can escape: each running `try` counts the error types its
//! clauses handle, and an error that some running `try` handles will be caught
//! there, with its trace unread. A clause that doesn't match rethrows, so the
//! error still reaches the `try` that handles it. Catching a rethrown error
//! again, or printing it, needs the trace only if no `try` handles it, and
//! then it was captured.

use crate::{DatError, ErrorKind, Loc, Str, Trace};
use std::any::Any;
use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};

/// Every error type, for a catch-all clause.
pub const ALL_ERRORS: &[ErrorKind] = &ErrorKind::ALL;

thread_local! {
    /// For each error type, how many running `try` blocks handle it.
    static HANDLERS: [Cell<u32>; ErrorKind::ALL.len()] = const { [const { Cell::new(0) }; ErrorKind::ALL.len()] };
}

/// Whether a running `try` will catch an error of this type.
fn handled(kind: ErrorKind) -> bool {
    HANDLERS.with(|h| h[kind as usize].get() > 0)
}

fn count(kinds: &[ErrorKind], add: bool) {
    HANDLERS.with(|h| {
        for &k in kinds {
            let c = &h[k as usize];
            c.set(if add { c.get() + 1 } else { c.get() - 1 });
        }
    });
}

/// Throws `err`, thrown at `at`; `None` reports it at the call in the caller.
/// Each way to throw inlines it, so a throw leaves one dat-rt frame for the
/// unwinder to pass, which is most of what a caught error costs.
#[inline(always)]
fn raise(mut err: Box<DatError>, at: Option<&Loc>) -> ! {
    err.trace = if handled(err.kind) { Trace::default() } else { Trace::native(at.copied()) };
    panic::resume_unwind(err)
}

/// Throws `err`; `None` reports it at the call in the caller.
#[cold]
#[inline(never)]
pub fn throw_at(err: Box<DatError>, at: Option<&Loc>) -> ! {
    raise(err, at)
}

/// Throws `err`, thrown at `loc`.
#[cold]
#[inline(never)]
pub fn throw(err: Box<DatError>, loc: &Loc) -> ! {
    raise(err, Some(loc))
}

/// `throw e;` for an error value built by the program.
#[cold]
#[inline(never)]
pub fn throw_new(err: DatError, loc: &'static Loc) -> ! {
    raise(Box::new(err), Some(loc))
}

/// `throw RangeError { message: m };`. The error is built here, out of line,
/// so it takes no room in the frame of the function that throws, which
/// keeps deep recursion deep.
#[cold]
#[inline(never)]
pub fn throw_error(kind: ErrorKind, message: Str, loc: &'static Loc) -> ! {
    raise(Box::new(DatError::new(kind, message.as_str())), Some(loc))
}

/// `fail(message)`.
#[cold]
#[inline(never)]
pub fn fail(message: Str, loc: &'static Loc) -> ! {
    raise(Box::new(DatError::new(ErrorKind::AssertionError, message.as_str())), Some(loc))
}

/// Rethrows an error a `catch` didn't handle, keeping its trace.
#[cold]
#[inline(never)]
pub fn rethrow(err: Box<DatError>) -> ! {
    panic::resume_unwind(err)
}

/// Runs a `try` block whose clauses handle `kinds`. A dat error comes back
/// as `Err`; a Rust panic, which would be a bug in dat itself, keeps unwinding.
#[inline]
pub fn catch<T>(kinds: &[ErrorKind], f: impl FnOnce() -> T) -> Result<T, Box<DatError>> {
    count(kinds, true);
    let result = panic::catch_unwind(AssertUnwindSafe(f));
    count(kinds, false);
    match result {
        Ok(v) => Ok(v),
        Err(payload) => Err(caught(payload)),
    }
}

/// Runs `main` or a test: like [`catch`], but an error that reaches it is
/// reported, so it doesn't count as handled.
pub(crate) fn run_entry(f: crate::Entry) -> Result<(), Box<DatError>> {
    match panic::catch_unwind(f) {
        Ok(()) => Ok(()),
        Err(payload) => Err(caught(payload)),
    }
}

#[cold]
#[inline(never)]
fn caught(payload: Box<dyn Any + Send>) -> Box<DatError> {
    match payload.downcast::<DatError>() {
        Ok(err) => err,
        Err(other) => panic::resume_unwind(other),
    }
}

/// A failed `assert` in generated code. `sides` holds both operands of a
/// failed comparison, rendered.
#[cold]
#[inline(never)]
pub fn assert_failed(
    src: &'static str,
    cond_src: &'static str,
    sides: Option<(String, String)>,
    message: Option<Str>,
    loc: &'static Loc,
) -> ! {
    raise(DatError::assertion(src, cond_src, sides, message.as_ref().map(Str::as_str)), Some(loc))
}
