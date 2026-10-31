//! `--report-copies`: how often a change copied a value that another copy
//! still shared, and where. Lists, sets, maps and strings copy their
//! contents only then (copy-on-write, §02), so each count is a place where
//! a value was shared longer than the program needed.
//!
//! Every copy goes through [`crate::Shared::make_mut`], which calls
//! [`copied`] on its cold path. Counting is off unless [`start`] ran, so a
//! normal run pays nothing. Both run modes say where each change happens
//! with [`at`], around exactly the same IR nodes, so the reports match.

use crate::Loc;
use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);

/// The copies at one location.
struct Count {
    loc: Loc,
    copies: u64,
    elements: u64,
    bytes: u64,
    /// Whether a string was among the copies, so its bytes are shown even
    /// when it was empty.
    strings: bool,
    lists: bool,
}

thread_local! {
    /// The change running now.
    static AT: Cell<Option<Loc>> = const { Cell::new(None) };
    static COUNTS: RefCell<Vec<Count>> = const { RefCell::new(Vec::new()) };
}

/// Starts counting, before any dat code runs.
pub fn start() {
    ON.store(true, Ordering::Relaxed);
}

/// Makes the table of counts now, so the debug checks don't count it.
pub(crate) fn reserve() {
    if on() {
        COUNTS.with_borrow_mut(|c| c.reserve(64));
    }
}

/// Whether [`start`] ran.
#[inline]
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Puts the change at `loc` in charge until the guard drops, which puts the
/// one before it back: a change can run others while it works out its
/// operands or calls a callback.
#[must_use]
pub fn at(loc: Loc) -> At {
    At(AT.replace(Some(loc)))
}

pub struct At(Option<Loc>);

impl Drop for At {
    fn drop(&mut self) {
        AT.set(self.0.take());
    }
}

/// How much a copied value holds: the elements of a list, set or map, or
/// the bytes of a string.
pub trait Size {
    const BYTES: bool = false;
    fn size(&self) -> u64;
}

/// Called by `make_mut` just before it copies a shared value.
#[cold]
#[inline(never)]
pub(crate) fn copied(size: u64, bytes: bool) {
    if !on() {
        return;
    }
    let Some(loc) = AT.get() else {
        // A copy outside every change the backends mark would be a bug in
        // dat; say so in the report instead of losing it.
        return record(Loc { file: "<unknown>", func: "<unknown>", line: 0, col: 0 }, size, bytes);
    };
    record(loc, size, bytes);
}

fn record(loc: Loc, size: u64, is_bytes: bool) {
    let (elements, bytes) = if is_bytes { (0, size) } else { (size, 0) };
    COUNTS.with_borrow_mut(|counts| match counts.iter_mut().find(|c| c.loc == loc) {
        Some(c) => {
            c.copies += 1;
            c.elements += elements;
            c.bytes += bytes;
            c.strings |= is_bytes;
            c.lists |= !is_bytes;
        }
        None => counts.push(Count { loc, copies: 1, elements, bytes, strings: is_bytes, lists: !is_bytes }),
    });
}

/// The report, sorted by location: a line per place that copied.
pub fn report() -> String {
    let mut counts = COUNTS.with_borrow_mut(std::mem::take);
    counts.sort_by(|a, b| (a.loc.file, a.loc.line, a.loc.col).cmp(&(b.loc.file, b.loc.line, b.loc.col)));
    let total: u64 = counts.iter().map(|c| c.copies).sum();
    if total == 0 {
        return "copies: none\n".to_string();
    }
    let places = if counts.len() == 1 { "1 place" } else { &format!("{} places", counts.len()) };
    let mut out = format!("copies: {total} at {places}\n");
    for c in &counts {
        let times = if c.copies == 1 { "1 copy".to_string() } else { format!("{} copies", c.copies) };
        let mut what = Vec::new();
        if c.lists {
            what.push(format!("{} element{}", c.elements, if c.elements == 1 { "" } else { "s" }));
        }
        if c.strings {
            what.push(format!("{} byte{}", c.bytes, if c.bytes == 1 { "" } else { "s" }));
        }
        let _ = writeln!(out, "  {}:{}:{} in {}: {times}, {}", c.loc.file, c.loc.line, c.loc.col, c.loc.func, what.join(" and "));
    }
    out
}

/// Prints the report to stderr, after the program's output, if counting is on.
pub fn finish() {
    if on() {
        crate::flush_stdout();
        eprint!("{}", report());
    }
}
