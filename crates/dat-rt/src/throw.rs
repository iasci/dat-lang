//! Throwing and catching in generated code.
//!
//! A runtime error unwinds with `resume_unwind(Box<DatError>)`, which skips the
//! panic hook and costs nothing until something is thrown. `try`/`catch`
//! becomes [`catch`] around a closure. The throw walks the [`Frame`] chain to
//! record a dat-only trace.

use crate::{DatError, ErrorKind, Frame, Loc, Str, Trace};
use std::any::Any;
use std::panic::{self, AssertUnwindSafe};

/// Throws `err`, thrown at `loc` in the function that got `frame`.
#[cold]
#[inline(never)]
pub fn throw(mut err: Box<DatError>, loc: &Loc, frame: &Frame<'_>) -> ! {
    err.trace = Trace::walk(loc, frame);
    panic::resume_unwind(err)
}

/// `throw e;` for an error value built by the program.
#[cold]
#[inline(never)]
pub fn throw_new(err: DatError, loc: &'static Loc, frame: &Frame<'_>) -> ! {
    throw(Box::new(err), loc, frame)
}

/// `fail(message)`.
#[cold]
#[inline(never)]
pub fn fail(message: Str, loc: &'static Loc, frame: &Frame<'_>) -> ! {
    throw(Box::new(DatError::new(ErrorKind::AssertionError, message.as_str())), loc, frame)
}

/// Rethrows an error a `catch` didn't handle, keeping its trace.
#[cold]
#[inline(never)]
pub fn rethrow(err: Box<DatError>) -> ! {
    panic::resume_unwind(err)
}

/// Runs a `try` block. A dat error comes back as `Err`; a Rust panic, which
/// would be a bug in dat itself, keeps unwinding.
#[inline]
pub fn catch<T>(f: impl FnOnce() -> T) -> Result<T, Box<DatError>> {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => Ok(v),
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
    frame: &Frame<'_>,
) -> ! {
    throw(DatError::assertion(src, cond_src, sides, message.as_ref().map(Str::as_str)), loc, frame)
}
