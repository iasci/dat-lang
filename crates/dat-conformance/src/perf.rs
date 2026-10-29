//! Latency budgets and speed gates (M0 exit criteria 9 and 13–16).
//!
//! Every measurement is wall time including process start: the median of
//! `RUNS` runs after `WARMUP` unmeasured ones. Latency budgets hard-fail at
//! twice the budget, since shared runners are noisy; speed gates compare two
//! programs measured in the same job, so they fail at the gate itself.

use crate::Opts;
use crate::run::dat_binary;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const WARMUP: usize = 3;
const RUNS: usize = 20;

fn time(cmd: &mut Command, runs: usize) -> Result<Duration, String> {
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    let mut samples = Vec::with_capacity(runs);
    for i in 0..WARMUP + runs {
        let start = Instant::now();
        let status = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
        let t = start.elapsed();
        if !status.success() && status.code() != Some(1) {
            return Err(format!("{cmd:?} exited with {status}"));
        }
        if i >= WARMUP {
            samples.push(t);
        }
    }
    samples.sort();
    Ok(samples[samples.len() / 2])
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

struct Gates {
    failed: Vec<String>,
    soft: Vec<String>,
}

impl Gates {
    fn budget(&mut self, what: &str, took: Duration, budget_ms: f64) {
        let t = ms(took);
        let verdict = if t <= budget_ms {
            "ok"
        } else if t <= 2.0 * budget_ms {
            self.soft.push(what.to_string());
            "over budget"
        } else {
            self.failed.push(what.to_string());
            "FAIL (over 2x budget)"
        };
        println!("  {what:<44} {t:>9.1} ms   budget {budget_ms:>6.0} ms   {verdict}");
    }

    fn ratio(&mut self, what: &str, ours: Duration, base: Duration, base_name: &str, gate: f64) {
        let r = ms(ours) / ms(base);
        let verdict = if r <= gate {
            "ok"
        } else {
            self.failed.push(what.to_string());
            "FAIL"
        };
        println!("  {what:<44} {:>9.1} ms   {base_name} {:.1} ms: {r:.2}x   gate {gate}x   {verdict}", ms(ours), ms(base));
    }
}

pub fn run(opts: &Opts) -> bool {
    let dat = dat_binary();
    let root = &opts.root;
    let fib_dir = root.join("examples/fib");
    let bench = root.join("bench");
    let out = root.join("target/bench");
    std::fs::create_dir_all(&out).unwrap();
    let mut g = Gates { failed: Vec::new(), soft: Vec::new() };
    let run_in = |dir: &Path, args: &[&str]| {
        let mut c = Command::new(&dat);
        c.args(args).current_dir(dir);
        c
    };
    println!("perf: median of {RUNS} runs after {WARMUP} warmups, wall time including process start");

    println!("interpreter path (criterion 13)");
    let measure = |c: &mut Command| time(c, RUNS).unwrap_or_else(|e| panic!("{e}"));
    g.budget("dat check fib.dat", measure(&mut run_in(&fib_dir, &["check", "fib.dat"])), 10.0);
    g.budget("dat test fib.dat (4 tests)", measure(&mut run_in(&fib_dir, &["test", "fib.dat"])), 20.0);
    g.budget("dat run hello.dat", measure(&mut run_in(&fib_dir, &["run", "hello.dat"])), 5.0);

    println!("compiled path, warm, dat-rt prebuilt (criterion 14)");
    // The first run builds; the measured runs are warm.
    let _ = run_in(&fib_dir, &["run", "--compiled", "fib.dat"]).output();
    let _ = run_in(&fib_dir, &["test", "--compiled", "fib.dat"]).output();
    g.budget("dat run --compiled fib.dat", time(&mut run_in(&fib_dir, &["run", "--compiled", "fib.dat"]), 5).unwrap(), 1000.0);
    g.budget("dat test --compiled fib.dat", time(&mut run_in(&fib_dir, &["test", "--compiled", "fib.dat"]), 5).unwrap(), 2000.0);

    println!("compiled speed (criteria 9 and 15)");
    let hand = out.join("fib35_hand");
    let status = Command::new("rustc")
        .env("RUSTUP_TOOLCHAIN", dat_codegen_toolchain())
        .args(["-C", "opt-level=3", "-C", "overflow-checks=on", "-o"])
        .arg(&hand)
        .arg(bench.join("fib35.rs"))
        .status();
    if !status.is_ok_and(|s| s.success()) {
        println!("  can't build bench/fib35.rs with rustc; skipping the compiled gates");
        g.failed.push("build bench/fib35.rs".into());
    } else {
        let gen_exe = out.join("fib35_dat");
        let built = run_in(&bench, &["build", "fib35.dat", "-o"]).arg(&gen_exe).status();
        let no_guard_exe = out.join("fib35_dat_no_guard");
        let built_ng = run_in(&bench, &["build", "fib35.dat", "-o"]).arg(&no_guard_exe).env("DAT_NO_STACK_GUARD", "1").status();
        if built.is_ok_and(|s| s.success()) && built_ng.is_ok_and(|s| s.success()) {
            let h = time(&mut Command::new(&hand), RUNS).unwrap();
            let d = time(&mut Command::new(&gen_exe), RUNS).unwrap();
            let n = time(&mut Command::new(&no_guard_exe), RUNS).unwrap();
            g.ratio("generated fib(35) vs hand-written Rust", d, h, "hand-written", 1.2);
            g.ratio("stack guard: fib(35) with vs without", d, n, "without", 1.05);
        } else {
            g.failed.push("dat build bench/fib35.dat".into());
        }
    }

    println!("interpreter speed (criterion 16)");
    let py = Command::new("python3").arg("--version").output();
    if py.is_ok_and(|o| o.status.success()) {
        let ours = time(&mut run_in(&bench, &["run", "fib30.dat"]), 10).unwrap();
        let cpython = time(Command::new("python3").arg(bench.join("fib.py")).arg("30"), 10).unwrap();
        g.ratio("interpreted fib(30) vs CPython", ours, cpython, "CPython", 2.0);
    } else {
        println!("  python3 not found; skipping the CPython gate");
    }

    if !g.soft.is_empty() {
        println!("perf: over budget but within 2x: {}", g.soft.join(", "));
    }
    if g.failed.is_empty() {
        println!("perf: all gates pass");
        true
    } else {
        println!("perf: FAILED: {}", g.failed.join(", "));
        false
    }
}

fn dat_codegen_toolchain() -> &'static str {
    // Same pin as dat-codegen; kept in sync by its unit test.
    "1.99.0"
}
