//! The `dat test` report, shared by both run modes (`reference/20-testing.md`).

use crate::{DatError, Entry, Loc, Site, StackLimit, capture_begin, capture_end, flush_stdout};
use std::fmt::Write as _;
use std::time::Duration;

/// A test, as both modes describe it to the reporter.
pub struct TestInfo {
    pub name: String,
    /// The name its frame has in traces: `test "name"`.
    pub frame: &'static str,
    pub loc: Loc,
}

pub struct Outcome {
    pub result: Result<(), Box<DatError>>,
    /// What the test printed.
    pub output: String,
}

/// Runs one test with its output captured.
pub fn run_one(f: impl FnOnce() -> Result<(), Box<DatError>>) -> Outcome {
    capture_begin();
    let result = f();
    let output = capture_end();
    Outcome { result, output }
}

/// Prints the report to stdout and returns the exit status: 0 if every test
/// passed, 1 if any failed.
pub fn report(tests: &[TestInfo], outcomes: &[Outcome], elapsed: Duration, verbose: bool) -> i32 {
    let mut out = String::new();
    let mut failed = 0;
    for (t, o) in tests.iter().zip(outcomes) {
        match &o.result {
            Ok(()) => {
                if verbose {
                    let _ = writeln!(out, "PASS {}:{}  {}", t.loc.file, t.loc.line, t.name);
                }
            }
            Err(e) => {
                failed += 1;
                out.push_str(&render_failure(t, e, &o.output));
                out.push('\n');
            }
        }
    }
    let passed = outcomes.len() - failed;
    let _ = writeln!(out, "{failed} failed, {passed} passed ({} ms)", elapsed.as_millis());
    crate::print_raw(&out);
    flush_stdout();
    if failed > 0 { 1 } else { 0 }
}

fn render_failure(t: &TestInfo, e: &DatError, output: &str) -> String {
    let (frames, _) = e.trace.frames();
    // Report the failure where the test's own body was when it happened.
    let at = frames.iter().position(|f| f.func == t.frame);
    let line = at.map_or(t.loc, |i| frames[i].loc);
    let mut out = String::new();
    let _ = writeln!(out, "FAIL {}:{}  {}", line.file, line.line, t.name);
    match &e.assert {
        Some(a) => {
            let _ = writeln!(out, "  {}", a.src);
            if let Some((l, r)) = &a.sides {
                let _ = writeln!(out, "    left:  {l}\n    right: {r}");
            }
            if let Some(m) = &a.message {
                let _ = writeln!(out, "    message: {m}");
            }
        }
        None => {
            let _ = writeln!(out, "  {}", e.to_display());
        }
    }
    for f in &frames[..at.unwrap_or(frames.len())] {
        let _ = writeln!(out, "    at {} ({}:{}:{})", f.func, f.loc.file, f.loc.line, f.loc.col);
    }
    if !output.is_empty() {
        out.push_str("  output:\n");
        for l in output.lines() {
            let _ = writeln!(out, "    {l}");
        }
    }
    out
}

/// A test in a compiled test binary.
pub struct CompiledTest {
    pub name: &'static str,
    pub frame: &'static str,
    pub loc: Loc,
    pub run: Entry,
}

/// The entry point of a compiled test binary. Takes `--filter <text>` and
/// `--verbose`. `sites` and `limit` are as for [`crate::run_main`].
pub fn main(tests: &[CompiledTest], sites: &'static [Site], limit: &'static StackLimit) -> ! {
    crate::trace::set_sites(sites);
    crate::stack::set_compiled_limit(limit);
    let mut filter = None;
    let mut verbose = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--filter" => filter = args.next(),
            "--verbose" => verbose = true,
            _ => {}
        }
    }
    let selected: Vec<&CompiledTest> = tests.iter().filter(|t| filter.as_deref().is_none_or(|f| t.name.contains(f))).collect();
    if selected.is_empty() {
        crate::exit_with(2);
    }
    let code = crate::run_on_dat_thread(crate::STACK_SIZE, || {
        let infos: Vec<TestInfo> = selected.iter().map(|t| TestInfo { name: t.name.to_string(), frame: t.frame, loc: t.loc }).collect();
        let start = std::time::Instant::now();
        let outcomes: Vec<Outcome> = selected.iter().map(|t| run_one(|| crate::throw::run_entry(t.run))).collect();
        report(&infos, &outcomes, start.elapsed(), verbose)
    });
    crate::exit_with(code)
}
