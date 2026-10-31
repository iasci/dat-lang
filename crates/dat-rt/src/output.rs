//! `print` and per-test output capture.
//!
//! stdout is buffered and flushed before anything goes to stderr, so a
//! terminal shows both in the order they happened.

use crate::Str;
use std::cell::RefCell;
use std::io::Write;

struct Output {
    buf: Vec<u8>,
    /// Set while a test runs: its output is kept, and shown only if it fails.
    capture: Option<String>,
}

thread_local! {
    static OUT: RefCell<Output> = const { RefCell::new(Output { buf: Vec::new(), capture: None }) };
}

const FLUSH_AT: usize = 1 << 16;

/// `print`, with its arguments already converted and joined.
pub fn print(s: Str) {
    print_str(s.as_str());
}

pub fn print_str(s: &str) {
    OUT.with_borrow_mut(|o| match &mut o.capture {
        Some(c) => {
            c.push_str(s);
            c.push('\n');
        }
        None => {
            o.buf.extend_from_slice(s.as_bytes());
            o.buf.push(b'\n');
            if o.buf.len() >= FLUSH_AT {
                write_out(&mut o.buf);
            }
        }
    });
}

fn write_out(buf: &mut Vec<u8>) {
    let mut out = std::io::stdout().lock();
    // A closed pipe isn't an error the program can act on.
    let _ = out.write_all(buf);
    let _ = out.flush();
    buf.clear();
}

/// Makes the output buffer now, so the debug checks don't count it.
pub(crate) fn reserve() {
    OUT.with_borrow_mut(|o| o.buf.reserve(FLUSH_AT));
}

pub fn flush_stdout() {
    OUT.with_borrow_mut(|o| write_out(&mut o.buf));
}

pub fn capture_begin() {
    OUT.with_borrow_mut(|o| o.capture = Some(String::new()));
}

pub fn capture_end() -> String {
    OUT.with_borrow_mut(|o| o.capture.take().unwrap_or_default())
}

/// Writes to stdout, bypassing any capture. The test report uses it.
pub fn print_raw(s: &str) {
    OUT.with_borrow_mut(|o| o.buf.extend_from_slice(s.as_bytes()));
}
