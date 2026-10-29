//! Stack traces of dat frames only.
//!
//! In generated code each function builds a [`Frame`]: a two-word record
//! that points at its caller's record and says where the caller made the
//! call. It passes the record to the functions it calls, and a throw walks
//! the chain, so the trace costs two stores per call until something is
//! thrown, and no function needs a landing pad.
//!
//! The interpreter builds the same trace while an error propagates: the throw
//! records its location as `pending`, and each frame the error leaves adds
//! itself with the pending location, or with the call it was making.

use std::fmt::Write as _;

/// A source location, as generated code stores it in a `static`. `func` is
/// the dat function the location is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loc {
    pub file: &'static str,
    pub func: &'static str,
    pub line: u32,
    pub col: u32,
}

/// The call-chain record of one running dat function.
pub struct Frame<'a> {
    /// The caller's record.
    pub parent: Option<&'a Frame<'a>>,
    /// Where the caller made this call. `None` for the outermost function.
    pub at: Option<&'static Loc>,
}

impl Frame<'static> {
    /// The record above `main` and each test. Make it a local on the dat
    /// thread, because the stack guard compares records' addresses.
    pub const ROOT: Frame<'static> = Frame { parent: None, at: None };
}

/// The signature of `main` and of each test in generated code.
pub type Entry = fn(&Frame<'_>, Option<&'static Loc>);

/// One line of a trace: a function, and where it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceFrame {
    pub func: &'static str,
    pub loc: Loc,
}

/// Frames kept at each end of a deep trace. The middle is counted, not stored.
const KEEP: usize = 32;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trace {
    /// Where the error was thrown, until the frame that threw it is recorded.
    pub pending: Option<Loc>,
    /// The innermost frames, innermost first.
    head: Vec<TraceFrame>,
    /// The outermost frames seen so far, as a ring of at most `KEEP`.
    tail: Vec<TraceFrame>,
    tail_start: usize,
    omitted: u64,
}

impl Trace {
    pub fn at(loc: Loc) -> Trace {
        Trace { pending: Some(loc), ..Trace::default() }
    }

    /// The trace of an error thrown at `loc` in the function that got `frame`.
    pub fn walk(loc: &Loc, frame: &Frame<'_>) -> Trace {
        let mut t = Trace::default();
        t.push(TraceFrame { func: loc.func, loc: *loc });
        let mut cur = frame;
        while let Some(at) = cur.at {
            t.push(TraceFrame { func: at.func, loc: *at });
            match cur.parent {
                Some(p) => cur = p,
                None => break,
            }
        }
        t
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

#[cfg(test)]
mod tests {
    use super::*;

    const L: Loc = Loc { file: "a.dat", func: "f", line: 1, col: 1 };

    #[test]
    fn walk_follows_the_chain() {
        static IN_MAIN: Loc = Loc { file: "a.dat", func: "main", line: 9, col: 3 };
        let root = Frame::ROOT;
        let callee = Frame { parent: Some(&root), at: Some(&IN_MAIN) };
        let t = Trace::walk(&L, &callee);
        let (frames, _) = t.frames();
        assert_eq!(frames.iter().map(|f| f.func).collect::<Vec<_>>(), ["f", "main"]);
        assert_eq!(frames[1].loc.line, 9);
    }

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
}
