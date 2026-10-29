use crate::{Str, Trace};

/// The built-in error types of `reference/12-errors.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    Error,
    ValueError,
    RangeError,
    KeyError,
    TypeError,
    AssertionError,
    StackOverflowError,
}

impl ErrorKind {
    /// In declaration order, so `kind as usize` indexes it.
    pub const ALL: [ErrorKind; 7] = [
        ErrorKind::Error,
        ErrorKind::ValueError,
        ErrorKind::RangeError,
        ErrorKind::KeyError,
        ErrorKind::TypeError,
        ErrorKind::AssertionError,
        ErrorKind::StackOverflowError,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ErrorKind::Error => "Error",
            ErrorKind::ValueError => "ValueError",
            ErrorKind::RangeError => "RangeError",
            ErrorKind::KeyError => "KeyError",
            ErrorKind::TypeError => "TypeError",
            ErrorKind::AssertionError => "AssertionError",
            ErrorKind::StackOverflowError => "StackOverflowError",
        }
    }
}

/// What a failed `assert` shows besides its message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssertInfo {
    /// The call as written: `assert(fib(10) == 56)`.
    pub src: String,
    /// Both sides of a failed comparison, already rendered.
    pub sides: Option<(String, String)>,
    /// The message argument, when the call gave one.
    pub message: Option<String>,
}

/// A thrown error: its type, its message and where it came from.
///
/// It is the payload generated code unwinds with, so it must be `Send` and
/// holds no `Str`.
#[derive(Clone, Debug)]
pub struct DatError {
    pub kind: ErrorKind,
    pub message: String,
    pub assert: Option<Box<AssertInfo>>,
    pub trace: Trace,
}

impl DatError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> DatError {
        DatError { kind, message: message.into(), assert: None, trace: Trace::default() }
    }

    /// `int overflow: 9223372036854775807 + 1`
    #[cold]
    #[inline(never)]
    pub fn int_overflow(a: i64, op: &str, b: i64) -> Box<DatError> {
        Box::new(DatError::new(ErrorKind::RangeError, format!("int overflow: {a} {op} {b}")))
    }

    /// `int overflow: -(-9223372036854775808)`
    #[cold]
    #[inline(never)]
    pub fn int_neg_overflow(a: i64) -> Box<DatError> {
        Box::new(DatError::new(ErrorKind::RangeError, format!("int overflow: -({a})")))
    }

    /// `int remainder by zero: 7 % 0`
    #[cold]
    #[inline(never)]
    pub fn rem_by_zero(a: i64) -> Box<DatError> {
        Box::new(DatError::new(ErrorKind::RangeError, format!("int remainder by zero: {a} % 0")))
    }

    #[cold]
    #[inline(never)]
    pub fn stack_overflow() -> Box<DatError> {
        Box::new(DatError::new(ErrorKind::StackOverflowError, "too many nested calls"))
    }

    /// A failed `assert`. `cond_src` is the condition as written.
    #[cold]
    #[inline(never)]
    pub fn assertion(src: &str, cond_src: &str, sides: Option<(String, String)>, message: Option<&str>) -> Box<DatError> {
        let given = message.map(str::to_string);
        let message = given.clone().unwrap_or_else(|| format!("assertion failed: {cond_src}"));
        let mut e = DatError::new(ErrorKind::AssertionError, message);
        e.assert = Some(Box::new(AssertInfo { src: src.to_string(), sides, message: given }));
        Box::new(e)
    }

    /// `toString()`: the type name and the message, `RangeError: index 5 out of range`.
    pub fn to_display(&self) -> String {
        format!("{}: {}", self.kind.name(), self.message)
    }

    pub fn to_str(&self) -> Str {
        Str::from_string(self.to_display())
    }

    pub fn message_str(&self) -> Str {
        Str::new(&self.message)
    }

    /// The conversion `catch (e: Error)` applies: the message becomes the
    /// original error's `toString()`.
    pub fn into_error(self) -> DatError {
        if self.kind == ErrorKind::Error {
            return self;
        }
        DatError { kind: ErrorKind::Error, message: self.to_display(), assert: None, trace: self.trace }
    }

    /// The text written to stderr when the error escapes `main`: the error,
    /// both sides of a failed comparison, then the trace.
    pub fn render_uncaught(&self) -> String {
        let mut out = self.to_display();
        out.push('\n');
        if let Some((l, r)) = self.assert.as_ref().and_then(|a| a.sides.as_ref()) {
            out.push_str(&format!("  left:  {l}\n  right: {r}\n"));
        }
        out.push_str(&self.trace.render("  "));
        out
    }
}
