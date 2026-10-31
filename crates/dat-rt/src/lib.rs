//! The dat runtime. Every operation whose result a program can observe lives
//! here once: checked integer arithmetic, the methods of numbers and strings,
//! lists, sets and maps, the value traits for printing, equality, hashing and
//! order, number printing, error values and their messages,
//! `print`, `toString`, `assert` rendering, the uncaught-error printer, the test
//! reporter and the stack guard. The interpreter calls it, and generated Rust
//! links it, so the two run modes can differ only in how they step through the
//! IR.
//!
//! Functions come in two forms where it matters. The interpreter uses the
//! `try_*` form, which returns `Result`. Generated code uses the plain form,
//! which throws by unwinding (see [`throw`]); it is free until something fails.

// Two places opt out: reading the stack pointer (`stack.rs`) and the
// allocator wrapper that reports running out of memory (`alloc.rs`).
#![deny(unsafe_code)]

mod alloc;
pub mod checks;
pub mod copies;
mod error;
mod fmt;
mod func;
pub mod int;
pub mod list;
pub mod map;
pub mod num;
mod output;
mod run;
pub mod set;
mod shared;
mod stack;
mod str;
pub mod testing;
pub mod text;
mod throw;
mod trace;
mod value;

pub use error::{AssertInfo, DatError, ErrorKind};
pub use fmt::{bool_to_str, debug_bool, debug_float, debug_int, debug_str, float_to_str, int_to_str};
pub use func::{
    FnRef0, FnRef1, FnRef2, FnRef3, FnRef4, FnRef5, FnRef6, FnRef7, FnRef8, FnRef9, FnRef10, FnRef11, FnRef12, Func0, Func1, Func2, Func3,
    Func4, Func5, Func6, Func7, Func8, Func9, Func10, Func11, Func12,
};
pub use list::{List, ListMut, ListSlice};
pub use map::Map;
pub use output::{capture_begin, capture_end, flush_stdout, print, print_raw, print_str};
pub use run::{exit_with, report_uncaught, run_main, run_on_dat_thread};
pub use set::Set;
pub use shared::Shared;
pub use stack::{STACK_SIZE, StackLimit, stack_check, stack_is_low};
pub use str::{Join, Str};
pub use throw::{ALL_ERRORS, assert_failed, catch, fail, rethrow, throw, throw_at, throw_error, throw_new};
pub use trace::{Entry, Loc, Site, Trace, TraceFrame};
pub use value::{DatHash, Equal, Order, Placeholder, Show, debug, show, text};

/// The result of an operation that can throw, in the interpreter's form.
pub type R<T> = Result<T, Box<DatError>>;

/// The longest list a program can ask for at once, in elements, and the
/// longest string, in bytes: 2^48. A longer `range()` or `repeat()` is a
/// `RangeError`, the same on every machine and in both run modes. It's
/// beyond any machine's memory, so a shorter one that can't be allocated
/// is out of memory instead (see `alloc.rs`).
pub const MAX_LEN: usize = 1 << 48;

/// Unwraps the result of a method that takes callbacks. Generated code passes
/// callbacks that always return `Ok` (they throw by unwinding), so the `Err`
/// arm is there for the method's own errors, thrown at the call in the caller.
#[inline(always)]
pub fn done<T>(r: R<T>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => throw_at(e, None),
    }
}

/// Takes a variable's value at its last use, leaving a placeholder. A
/// `try` block runs as a closure, so a move there takes the value out of
/// the variable instead of moving the variable into the closure.
#[inline(always)]
pub fn take<T: Placeholder>(x: &mut T) -> T {
    std::mem::replace(x, T::placeholder())
}

/// Marks code the type checker proves unreachable.
#[cold]
#[inline(never)]
pub fn unreachable() -> ! {
    panic!("internal error: reached code the type checker proved unreachable")
}

/// Placed before a return that could hand back a dat call's result
/// directly. Such a call could otherwise become a tail call, which replaces
/// the caller's native frame and would drop the caller from traces. It
/// compiles to no instructions.
#[inline(always)]
pub fn no_tail_call() {
    std::hint::black_box(());
}

/// How a `try` block finished, when it didn't throw. Generated code runs the
/// block in a closure, so `return`, `break` and `continue` leave it as values.
pub enum Flow<T> {
    Normal,
    Return(T),
    Break,
    Continue,
}
