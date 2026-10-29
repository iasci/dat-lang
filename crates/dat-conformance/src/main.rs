//! The conformance runner.
//!
//! - `examples`: runs every program under `examples/` with the commands its
//!   `.golden` file lists, on the interpreter and compiled, and compares
//!   stdout, stderr and exit status with the golden and across modes.
//! - `spec`: extracts every code block in `reference/`, classifies it, turns
//!   it into cases and runs the cases the current milestone supports.
//! - `self-test`: proves the mode diff works by injecting a fault into the
//!   interpreter (`DAT_FAULT=interp-plus-one`) and expecting a difference.
//! - `structure`: neither backend depends on the parser or checker, and
//!   generated Rust forbids `unsafe` and uses no `RefCell` or reflection.
//! - `concurrency`: two compiled runs of one file at once, with the file
//!   changed in between, each run their own version.
//! - `perf`: the latency budgets and speed gates. Not part of `all`, since
//!   it needs a quiet machine. `--shared-runner` relaxes it for CI.
//!
//! `--bless` rewrites goldens from the interpreter's results and the spec
//! ratchet from the current pass list.

#![forbid(unsafe_code)]

mod examples;
mod perf;
mod run;
mod spec;

use std::path::PathBuf;
use std::process::ExitCode;

pub struct Opts {
    pub root: PathBuf,
    pub bless: bool,
    pub verbose: bool,
    /// Skip the compiled mode (faster local runs). CI never sets it.
    pub interp_only: bool,
    /// `perf` on a shared CI runner: fail only at twice a budget or gate.
    pub shared_runner: bool,
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |args: &mut Vec<String>, f: &str| {
        let had = args.iter().any(|a| a == f);
        args.retain(|a| a != f);
        had
    };
    let opts = Opts {
        root: workspace_root(),
        bless: flag(&mut args, "--bless"),
        verbose: flag(&mut args, "--verbose"),
        interp_only: flag(&mut args, "--interp-only"),
        shared_runner: flag(&mut args, "--shared-runner"),
    };
    let what = args.first().map(String::as_str).unwrap_or("all");
    let ok = match what {
        "examples" => examples::run(&opts),
        "spec" => spec::run(&opts),
        "self-test" => examples::self_test(&opts),
        "structure" => examples::structure(&opts),
        "concurrency" => examples::concurrent_runs(&opts),
        "perf" => perf::run(&opts),
        "all" => {
            let a = examples::run(&opts);
            let b = examples::self_test(&opts);
            let c = spec::run(&opts);
            let d = examples::structure(&opts);
            let e = examples::concurrent_runs(&opts);
            a && b && c && d && e
        }
        other => {
            eprintln!(
                "usage: dat-conformance [examples|spec|self-test|structure|concurrency|perf|all] [--bless] [--verbose] [--interp-only] [--shared-runner]\nunknown: {other}"
            );
            return ExitCode::from(2);
        }
    };
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn workspace_root() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    p.canonicalize().unwrap_or(p)
}
