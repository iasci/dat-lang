//! Latency budgets and speed gates (M0 exit criteria 9 and 13–16).
//!
//! Every measurement is wall time including process start: the median of
//! `RUNS` runs after `WARMUP` unmeasured ones. The programs a gate compares
//! run interleaved, one run of each in turn, so noise that comes and goes
//! hits both sides alike. The numbers that count come
//! from the reference machine, where every budget and gate fails at its
//! limit. Shared CI runners are too noisy for that, so with
//! `--shared-runner` a result fails only at twice its limit.
//!
//! `fib(35)` is the worst case for compiled calls: it does nothing per call
//! but call itself, so the stack check's own cost there is shown, not
//! judged. Two more recursive programs in `bench/` are timed against
//! hand-written Rust and shown too, so a change tuned to `fib` shows what
//! it does to other shapes.

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

/// The median time of each command, running them in turn `runs` times.
fn time_interleaved(cmds: &mut [&mut Command], runs: usize) -> Result<Vec<Duration>, String> {
    let mut samples = vec![Vec::with_capacity(runs); cmds.len()];
    for i in 0..WARMUP + runs {
        for (cmd, s) in cmds.iter_mut().zip(&mut samples) {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
            let start = Instant::now();
            let status = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
            let t = start.elapsed();
            if !status.success() && status.code() != Some(1) {
                return Err(format!("{cmd:?} exited with {status}"));
            }
            if i >= WARMUP {
                s.push(t);
            }
        }
    }
    Ok(samples
        .into_iter()
        .map(|mut s| {
            s.sort();
            s[s.len() / 2]
        })
        .collect())
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

struct Gates {
    failed: Vec<String>,
    soft: Vec<String>,
    shared_runner: bool,
}

impl Gates {
    /// The verdict for `value` against `limit`, recording a failure.
    fn judge(&mut self, what: &str, value: f64, limit: f64) -> &'static str {
        if value <= limit {
            "ok"
        } else if self.shared_runner && value <= 2.0 * limit {
            self.soft.push(what.to_string());
            "over, within 2x on a shared runner"
        } else {
            self.failed.push(what.to_string());
            "FAIL"
        }
    }

    fn budget(&mut self, what: &str, took: Duration, budget_ms: f64) {
        let t = ms(took);
        let verdict = self.judge(what, t, budget_ms);
        println!("  {what:<44} {t:>9.1} ms   budget {budget_ms:>6.0} ms   {verdict}");
    }

    fn ratio(&mut self, what: &str, ours: Duration, base: Duration, base_name: &str, gate: f64) {
        let r = ms(ours) / ms(base);
        let verdict = self.judge(what, r, gate);
        println!("  {what:<44} {:>9.1} ms   {base_name} {:.1} ms: {r:.2}x   gate {gate}x   {verdict}", ms(ours), ms(base));
    }
}

fn shown(what: &str, ours: Duration, base: Duration, base_name: &str) {
    let r = ms(ours) / ms(base);
    println!("  {what:<44} {:>9.1} ms   {base_name} {:.1} ms: {r:.2}x   not judged", ms(ours), ms(base));
}

/// Builds a hand-written baseline in `bench/` as bench/fib35.rs says.
fn rustc(src: &Path, exe: &Path) -> bool {
    Command::new("rustc")
        .env("RUSTUP_TOOLCHAIN", dat_codegen_toolchain())
        .args(["-C", "opt-level=3", "-C", "overflow-checks=on", "-C", "llvm-args=-align-all-functions=6", "-o"])
        .arg(exe)
        .arg(src)
        .status()
        .is_ok_and(|s| s.success())
}

pub fn run(opts: &Opts) -> bool {
    let dat = dat_binary();
    let root = &opts.root;
    let fib_dir = root.join("examples/fib");
    let bench = root.join("bench");
    let out = root.join("target/bench");
    std::fs::create_dir_all(&out).unwrap();
    let mut g = Gates { failed: Vec::new(), soft: Vec::new(), shared_runner: opts.shared_runner };
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
    if !rustc(&bench.join("fib35.rs"), &hand) {
        println!("  can't build bench/fib35.rs with rustc; skipping the compiled gates");
        g.failed.push("build bench/fib35.rs".into());
    } else {
        let gen_exe = out.join("fib35_dat");
        let built = run_in(&bench, &["build", "fib35.dat", "-o"]).arg(&gen_exe).status();
        let no_guard_exe = out.join("fib35_dat_no_guard");
        let built_ng = run_in(&bench, &["build", "fib35.dat", "-o"]).arg(&no_guard_exe).env("DAT_NO_STACK_GUARD", "1").status();
        if built.is_ok_and(|s| s.success()) && built_ng.is_ok_and(|s| s.success()) {
            let (mut h, mut d, mut n) = (Command::new(&hand), Command::new(&gen_exe), Command::new(&no_guard_exe));
            let t = time_interleaved(&mut [&mut h, &mut d, &mut n], 2 * RUNS).unwrap();
            let (h, d, n) = (t[0], t[1], t[2]);
            g.ratio("generated fib(35) vs hand-written Rust", d, h, "hand-written", 1.2);
            shown("stack guard: fib(35) with vs without", d, n, "without");
        } else {
            g.failed.push("dat build bench/fib35.dat".into());
        }
    }

    println!("other recursion, shown but not judged");
    for (name, what) in [("ack", "generated ack(3, 10) vs hand-written Rust"), ("mutual", "generated mutual recursion vs hand-written")] {
        let (hand, ours) = (out.join(format!("{name}_hand")), out.join(format!("{name}_dat")));
        let built = rustc(&bench.join(format!("{name}.rs")), &hand)
            && run_in(&bench, &["build", &format!("{name}.dat"), "-o"]).arg(&ours).status().is_ok_and(|s| s.success());
        if !built {
            println!("  can't build bench/{name}.dat or bench/{name}.rs");
            g.failed.push(format!("build bench/{name}"));
            continue;
        }
        let t = time_interleaved(&mut [&mut Command::new(&ours), &mut Command::new(&hand)], RUNS).unwrap();
        shown(what, t[0], t[1], "hand-written");
    }

    println!("interpreter speed (criterion 16)");
    let py = Command::new("python3").arg("--version").output();
    if py.is_ok_and(|o| o.status.success()) {
        let mut ours = run_in(&bench, &["run", "fib30.dat"]);
        let mut cpython = Command::new("python3");
        cpython.arg(bench.join("fib.py")).arg("30");
        let t = time_interleaved(&mut [&mut ours, &mut cpython], 10).unwrap();
        let (ours, cpython) = (t[0], t[1]);
        g.ratio("interpreted fib(30) vs CPython", ours, cpython, "CPython", 2.0);
    } else {
        println!("  python3 not found; skipping the CPython gate");
    }

    if !g.soft.is_empty() {
        println!("perf: over the limit but within 2x on a shared runner: {}", g.soft.join(", "));
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
