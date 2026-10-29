//! The dat runtime. Every operation whose result a program can observe lives
//! here once: checked integer arithmetic, error values and their messages,
//! `print`, `toString`, `assert` rendering, the uncaught-error printer, the test
//! reporter and the stack guard. The interpreter calls it, and generated Rust
//! links it, so the two run modes can differ only in how they step through the
//! IR.
//!
//! Functions come in two forms where it matters. The interpreter uses the
//! `try_*` form, which returns `Result`. Generated code uses the plain form,
//! which throws by unwinding (see [`throw`]); it is free until something fails.

#![forbid(unsafe_code)]

mod error;
mod fmt;
pub mod int;
mod output;
mod run;
mod stack;
mod str;
pub mod testing;
mod throw;
mod trace;

pub use error::{AssertInfo, DatError, ErrorKind};
pub use fmt::{bool_to_str, debug_bool, debug_int, debug_str, int_to_str};
pub use output::{capture_begin, capture_end, flush_stdout, print, print_raw, print_str};
pub use run::{exit_with, report_uncaught, run_main, run_on_dat_thread};
pub use stack::{STACK_SIZE, stack_check, stack_is_low};
pub use str::Str;
pub use throw::{assert_failed, catch, fail, rethrow, throw, throw_new};
pub use trace::{Entry, Frame, Loc, Trace, TraceFrame};

/// Marks code the type checker proves unreachable.
#[cold]
#[inline(never)]
pub fn unreachable() -> ! {
    panic!("internal error: reached code the type checker proved unreachable")
}

/// The one reference-counted pointer dat-rt uses, so a later switch to a
/// thread-safe count stays inside this crate.
pub type Shared<T> = std::rc::Rc<T>;

/// How a `try` block finished, when it didn't throw. Generated code runs the
/// block in a closure, so `return`, `break` and `continue` leave it as values.
pub enum Flow<T> {
    Normal,
    Return(T),
    Break,
    Continue,
}
