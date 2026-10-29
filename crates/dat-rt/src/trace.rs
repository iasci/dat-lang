//! Stack traces of dat frames only.
//!
//! Generated code keeps no call-chain bookkeeping, so a call costs what a
//! Rust call costs. When an error that no enclosing `try` will catch is
//! thrown, dat-rt captures the native stack ([`Trace::native`]). The trace is
//! worked out only when it's printed: each native frame, inlined ones
//! included, that points into the generated `main.rs` is looked up in the
//! [`Site`] table the generator wrote, which maps the line and column of every
//! call and throw in the generated Rust to its dat location. Frames of dat-rt,
//! std and the harness aren't in the table and drop out.
//!
//! The interpreter builds the same trace while an error propagates: the throw
//! records its location as `pending`, and each frame the error leaves adds
//! itself with the pending location, or with the call it was making.

use std::backtrace::Backtrace;
use std::fmt::{self, Write as _};
use std::sync::{Arc, OnceLock};

/// A source location, as generated code stores it in a `static`. `func` is
/// the dat function the location is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loc {
    pub file: &'static str,
    pub func: &'static str,
    pub line: u32,
    pub col: u32,
}

/// The signature of `main` and of each test in generated code.
pub type Entry = fn();

/// A call or throw in the generated Rust: where it is in `main.rs`, as debug
/// info reports it (1-based line, 1-based byte column), and its dat location.
pub struct Site {
    pub line: u32,
    pub col: u32,
    pub loc: &'static Loc,
}

/// The generated program's sites, sorted by line and column.
static SITES: OnceLock<&'static [Site]> = OnceLock::new();

/// Called once by the generated `main` before any dat code runs.
pub(crate) fn set_sites(sites: &'static [Site]) {
    let _ = SITES.set(sites);
}

/// One line of a trace: a function, and where it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceFrame {
    pub func: &'static str,
    pub loc: Loc,
}

/// Frames kept at each end of a deep trace. The middle is counted, not stored.
const KEEP: usize = 32;

#[derive(Clone, Debug, Default)]
pub struct Trace {
    /// Where the error was thrown, until the frame that threw it is recorded.
    pub pending: Option<Loc>,
    /// The innermost frames, innermost first.
    head: Vec<TraceFrame>,
    /// The outermost frames seen so far, as a ring of at most `KEEP`.
    tail: Vec<TraceFrame>,
    tail_start: usize,
    omitted: u64,
    /// Compiled code: the native stack at the throw, read when printed.
    native: Option<Arc<Native>>,
}

#[derive(Debug)]
struct Native {
    stack: Backtrace,
    /// The throw's own location; `None` for a stack overflow, which is
    /// reported at the call in the caller.
    at: Option<Loc>,
    resolved: OnceLock<Trace>,
}

impl Trace {
    pub fn at(loc: Loc) -> Trace {
        Trace { pending: Some(loc), ..Trace::default() }
    }

    /// The trace of an error thrown here, in compiled code, at `at`.
    #[cold]
    #[inline(never)]
    pub fn native(at: Option<Loc>) -> Trace {
        let native = Native { stack: Backtrace::force_capture(), at, resolved: OnceLock::new() };
        Trace { native: Some(Arc::new(native)), ..Trace::default() }
    }

    /// Records that the error left the frame of `func`, which was at `at` if
    /// the error didn't start in it.
    pub fn leave(&mut self, func: &'static str, at: Loc) {
        let loc = self.pending.take().unwrap_or(at);
        self.push(TraceFrame { func, loc });
    }

    fn push(&mut self, frame: TraceFrame) {
        if self.head.len() < KEEP {
            self.head.push(frame);
        } else if self.tail.len() < KEEP {
            self.tail.push(frame);
        } else {
            self.tail[self.tail_start] = frame;
            self.tail_start = (self.tail_start + 1) % KEEP;
            self.omitted += 1;
        }
    }

    /// The recorded frames, innermost first, and how many were left out in the middle.
    pub fn frames(&self) -> (Vec<TraceFrame>, u64) {
        if let Some(n) = &self.native {
            return n.resolved.get_or_init(|| n.resolve()).frames();
        }
        let mut all = self.head.clone();
        all.extend_from_slice(&self.tail[self.tail_start..]);
        all.extend_from_slice(&self.tail[..self.tail_start]);
        (all, self.omitted)
    }

    pub fn render(&self, indent: &str) -> String {
        let (frames, omitted) = self.frames();
        let mut out = String::new();
        for (i, f) in frames.iter().enumerate() {
            if i == KEEP && omitted > 0 {
                let _ = writeln!(out, "{indent}... {omitted} more calls");
            }
            let _ = writeln!(out, "{indent}at {} ({}:{}:{})", f.func, f.loc.file, f.loc.line, f.loc.col);
        }
        out
    }
}

impl Native {
    /// Reads the captured stack. std offers it only as text, so the text is
    /// parsed as it's written, one line at a time, and never held whole.
    fn resolve(&self) -> Trace {
        let mut r = Resolver { sites: SITES.get().copied().unwrap_or(&[]), line: String::new(), trace: Trace::default(), at: self.at };
        let _ = write!(r, "{}", self.stack);
        if let Some(at) = r.at.take() {
            // The throw's own frame wasn't found; it still comes first.
            r.trace.push(TraceFrame { func: at.func, loc: at });
        }
        r.trace
    }
}

struct Resolver {
    sites: &'static [Site],
    line: String,
    trace: Trace,
    /// The throw's location, until the first dat frame is seen.
    at: Option<Loc>,
}

impl Resolver {
    /// A location line reads `at <path>:<line>:<col>`; only `main.rs` can be
    /// generated code.
    fn site(&self, text: &str) -> Option<&'static Loc> {
        let text = text.trim().strip_prefix("at ")?;
        let mut parts = text.rsplitn(3, ':');
        let col: u32 = parts.next()?.parse().ok()?;
        let line: u32 = parts.next()?.parse().ok()?;
        let path = parts.next()?;
        if !(path.ends_with("/src/main.rs") || path.ends_with("\\src\\main.rs") || path == "src/main.rs") {
            return None;
        }
        let i = self.sites.binary_search_by(|s| (s.line, s.col).cmp(&(line, col))).ok()?;
        Some(self.sites[i].loc)
    }

    fn frame(&mut self, loc: &'static Loc) {
        if let Some(at) = self.at.take()
            && at != *loc
        {
            self.trace.push(TraceFrame { func: at.func, loc: at });
        }
        self.trace.push(TraceFrame { func: loc.func, loc: *loc });
    }
}

impl fmt::Write for Resolver {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for piece in s.split_inclusive('\n') {
            self.line.push_str(piece);
            if piece.ends_with('\n') {
                if let Some(loc) = self.site(&self.line) {
                    self.frame(loc);
                }
                self.line.clear();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: Loc = Loc { file: "a.dat", func: "f", line: 1, col: 1 };

    #[test]
    fn deep_trace_counts() {
        let mut t = Trace::at(L);
        for _ in 0..100 {
            t.leave("f", L);
        }
        t.leave("main", Loc { line: 9, ..L });
        let (frames, omitted) = t.frames();
        assert_eq!(frames.len(), 64);
        assert_eq!(omitted, 37);
        assert_eq!(frames.last().unwrap().func, "main");
    }

    #[test]
    fn resolver_maps_generated_lines_only() {
        static IN_F: Loc = Loc { file: "a.dat", func: "f", line: 3, col: 5 };
        static IN_MAIN: Loc = Loc { file: "a.dat", func: "main", line: 9, col: 3 };
        static SITES: [Site; 2] = [Site { line: 10, col: 20, loc: &IN_F }, Site { line: 14, col: 9, loc: &IN_MAIN }];
        let mut r = Resolver { sites: &SITES, line: String::new(), trace: Trace::default(), at: Some(IN_F) };
        let text = "   0:     0x1 - dat_rt::throw\n            at /x/dat-rt/src/throw.rs:20:5\n   1:     0x2 - f2_f\n             \
                    at /c/dat_1/src/main.rs:10:20\n   2:     0x3 - f1_main\n             at /c/dat_1/src/main.rs:14:9\n   3: std::rt\n";
        r.write_str(text).unwrap();
        let (frames, _) = r.trace.frames();
        assert_eq!(frames.iter().map(|f| (f.func, f.loc.line)).collect::<Vec<_>>(), [("f", 3), ("main", 9)]);
    }
}
