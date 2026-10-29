//! Stack traces of dat frames only.
//!
//! Generated code keeps no call-chain bookkeeping, so a call costs what a
//! Rust call costs. When an error that no enclosing `try` will catch is
//! thrown, dat-rt records the return address of every native frame
//! ([`Trace::native`]), without symbolizing anything. The trace is worked
//! out only when it's printed: each address is symbolized once, however many
//! frames share it, and each location it yields, inlined ones included, that
//! points into the generated `main.rs` is looked up in the [`Site`] table the
//! generator wrote. That table maps the line and column of every call and
//! throw in the generated Rust to its dat location. Frames of dat-rt, std and
//! the harness aren't in the table and drop out.
//!
//! The interpreter builds the same trace while an error propagates: the throw
//! records its location as `pending`, and each frame the error leaves adds
//! itself with the pending location, or with the call it was making.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
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
    /// The return address of each native frame at the throw, innermost first.
    ips: Vec<usize>,
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
        let mut ips = Vec::new();
        backtrace::trace(|frame| {
            ips.push(frame.ip() as usize);
            true
        });
        let native = Native { ips, at, resolved: OnceLock::new() };
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
    fn resolve(&self) -> Trace {
        let sites = SITES.get().copied().unwrap_or(&[]);
        build(&self.ips, self.at, |ip| {
            let mut locs = Vec::new();
            backtrace::resolve(ip as *mut std::ffi::c_void, |symbol| {
                if let (Some(file), Some(line), Some(col)) = (symbol.filename(), symbol.lineno(), symbol.colno())
                    && let Some(loc) = site(sites, file, line, col)
                {
                    locs.push(loc);
                }
            });
            locs
        })
    }
}

/// The dat location of a call or throw at `line:col` of `file`; only
/// `main.rs` can be generated code.
fn site(sites: &[Site], file: &Path, line: u32, col: u32) -> Option<&'static Loc> {
    if !file.ends_with("src/main.rs") {
        return None;
    }
    let i = sites.binary_search_by(|s| (s.line, s.col).cmp(&(line, col))).ok()?;
    Some(sites[i].loc)
}

/// The trace of the native frames `ips`, innermost first. `locs_of` gives
/// the dat locations an address stands for, innermost first: several when
/// calls were inlined into its frame, none outside generated code. Deep
/// recursion repeats a few addresses, so each is resolved once.
fn build(ips: &[usize], mut at: Option<Loc>, mut locs_of: impl FnMut(usize) -> Vec<&'static Loc>) -> Trace {
    let mut known: HashMap<usize, Vec<&'static Loc>> = HashMap::new();
    let mut trace = Trace::default();
    for &ip in ips {
        for &loc in known.entry(ip).or_insert_with(|| locs_of(ip)).iter() {
            // The throw's own frame comes first, once.
            if let Some(a) = at.take()
                && a != *loc
            {
                trace.push(TraceFrame { func: a.func, loc: a });
            }
            trace.push(TraceFrame { func: loc.func, loc: *loc });
        }
    }
    if let Some(a) = at {
        // The throw's own frame wasn't found; it still comes first.
        trace.push(TraceFrame { func: a.func, loc: a });
    }
    trace
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
    fn build_maps_generated_frames_and_resolves_each_address_once() {
        static IN_F: Loc = Loc { file: "a.dat", func: "f", line: 3, col: 5 };
        static IN_G: Loc = Loc { file: "a.dat", func: "g", line: 6, col: 2 };
        static IN_MAIN: Loc = Loc { file: "a.dat", func: "main", line: 9, col: 3 };
        let mut resolved = Vec::new();
        // 1 is dat-rt, 2 is `f` inlined into `g`, 3 is `g` calling itself,
        // 4 is `main`, 5 is std.
        let ips = [1, 2, 3, 3, 3, 4, 5];
        let trace = build(&ips, Some(IN_F), |ip| {
            resolved.push(ip);
            match ip {
                2 => vec![&IN_F, &IN_G],
                3 => vec![&IN_G],
                4 => vec![&IN_MAIN],
                _ => vec![],
            }
        });
        let (frames, omitted) = trace.frames();
        let lines: Vec<_> = frames.iter().map(|f| (f.func, f.loc.line)).collect();
        assert_eq!(lines, [("f", 3), ("g", 6), ("g", 6), ("g", 6), ("g", 6), ("main", 9)]);
        assert_eq!(omitted, 0);
        assert_eq!(resolved, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn site_needs_the_generated_file() {
        static IN_F: Loc = Loc { file: "a.dat", func: "f", line: 3, col: 5 };
        let sites = [Site { line: 10, col: 20, loc: &IN_F }];
        assert_eq!(site(&sites, Path::new("/c/dat_1/src/main.rs"), 10, 20), Some(&IN_F));
        assert_eq!(site(&sites, Path::new("/x/dat-rt/src/throw.rs"), 10, 20), None);
        assert_eq!(site(&sites, Path::new("/c/dat_1/src/main.rs"), 10, 21), None);
    }
}
