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
//! judged. More programs in `bench/` are timed and shown too, so a change
//! tuned to `fib` shows what it does elsewhere: two other recursive shapes,
//! and everyday code, a loop (`primes`) and string building (`rows`),
//! compiled against hand-written Rust and interpreted against CPython.
//! Float code (`floats`, M1) is judged: within about 4% of hand-written
//! Rust. Last, the size of the generated Rust per line of dat, which build
//! budgets can be stated in.

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

/// §02's patterns that never copy, each run `n` times: quadratic time
/// would mean a copy per step.
const COMPLEXITY: &[(&str, &str)] = &[
    ("append", "let items: list<int> = [];\n  for (const i of range(N)) {\n    items = append(items, i);\n  }\n  print(items.length);"),
    ("string", "let text = \"\";\n  for (const i of range(N)) {\n    text += \"x\";\n  }\n  print(text.length);"),
    (
        "map-take",
        "let byKey: map<int, list<int>> = {};\n  for (const i of range(N)) {\n    const k = i % 7;\n    let xs = byKey.get(k, []);\n    xs.push(i);\n    byKey[k] = xs;\n  }\n  print(byKey.size);",
    ),
    (
        "list-take",
        "let rows: list<list<int>> = [[], [], []];\n  for (const i of range(N)) {\n    const k = i % 3;\n    let row = rows[k];\n    row.push(i);\n    rows[k] = row;\n  }\n  print(rows[0].length);",
    ),
    (
        "shift",
        "let queue = [0];\n  let seen = 0;\n  while (!queue.isEmpty()) {\n    const v = queue.shift();\n    seen++;\n    if (v < N) {\n      queue.push(v + 1);\n    }\n  }\n  print(seen);",
    ),
];

fn complexity_program(body: &str, n: usize) -> String {
    format!(
        "function append(xs: list<int>, x: int): list<int> {{\n  let r = xs;\n  r.push(x);\n  return r;\n}}\n\nfunction main(): void {{\n  {}\n}}\n",
        body.replace('N', &n.to_string())
    )
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

    println!("compiled float code (M1)");
    let (hand, ours) = (out.join("floats_hand"), out.join("floats_dat"));
    if rustc(&bench.join("floats.rs"), &hand)
        && run_in(&bench, &["build", "floats.dat", "-o"]).arg(&ours).status().is_ok_and(|s| s.success())
    {
        let t = time_interleaved(&mut [&mut Command::new(&ours), &mut Command::new(&hand)], 2 * RUNS).unwrap();
        g.ratio("generated floats vs hand-written Rust", t[0], t[1], "hand-written", 1.04);
    } else {
        println!("  can't build bench/floats.dat or bench/floats.rs");
        g.failed.push("build bench/floats".into());
    }

    println!("other programs, shown but not judged");
    let others = [
        ("ack", "generated ack(3, 10) vs hand-written Rust"),
        ("mutual", "generated mutual recursion vs hand-written"),
        ("primes", "generated primes loop vs hand-written"),
        ("rows", "generated string rows vs hand-written"),
        ("wordcount", "generated word count (map) vs hand-written"),
        ("groups", "generated grouping (take and restore) vs hand"),
        ("append", "generated append-built list vs hand-written"),
        ("bfs", "generated BFS with shift vs hand-written"),
        ("sortcmp", "generated sort with comparator vs hand-written"),
        ("chains", "generated map/filter/reduce vs hand-written"),
        ("search", "generated search with a callback vs hand"),
    ];
    for (name, what) in others {
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

    println!("complexity: n and 4n, linear is about 4x (M2 criterion 8)");
    let dir = out.join("complexity");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, body) in COMPLEXITY {
        for (mode, n) in [("interpreted", 100_000), ("compiled", 1_000_000)] {
            let mut times = Vec::new();
            for size in [n, 4 * n] {
                let file = format!("{name}_{size}.dat");
                std::fs::write(dir.join(&file), complexity_program(body, size)).unwrap();
                let mut cmd = if mode == "compiled" {
                    let exe = dir.join(format!("{name}_{size}"));
                    if !run_in(&dir, &["build", &file, "-o"]).arg(&exe).status().is_ok_and(|s| s.success()) {
                        g.failed.push(format!("build complexity {name}"));
                        break;
                    }
                    Command::new(exe)
                } else {
                    run_in(&dir, &["run", &file])
                };
                times.push(time(&mut cmd, 5).unwrap());
            }
            if let [small, large] = times[..] {
                let what = format!("{mode} {name}");
                let r = large.as_secs_f64() / small.as_secs_f64().max(1e-9);
                let verdict = g.judge(&what, r, 8.0);
                println!("  {what:<44} {:>9.1} ms   4n {:.1} ms: {r:.1}x   gate 8x   {verdict}", ms(small), ms(large));
            }
        }
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
        for (name, what) in [
            ("primes", "interpreted primes loop vs CPython"),
            ("rows", "interpreted string rows vs CPython"),
            ("floats", "interpreted floats vs CPython"),
            ("wordcount", "interpreted word count vs CPython"),
            ("groups", "interpreted grouping vs CPython"),
            ("append", "interpreted append-built list vs CPython"),
            ("bfs", "interpreted BFS vs CPython"),
            ("sortcmp", "interpreted sort with comparator vs CPython"),
            ("chains", "interpreted map/filter/reduce vs CPython"),
            ("search", "interpreted search with a callback vs CPython"),
        ] {
            let mut ours = run_in(&bench, &["run", &format!("{name}.dat")]);
            let mut cpython = Command::new("python3");
            cpython.arg(bench.join(format!("{name}.py")));
            let t = time_interleaved(&mut [&mut ours, &mut cpython], 10).unwrap();
            shown(what, t[0], t[1], "CPython");
        }
    } else {
        println!("  python3 not found; skipping the CPython gate");
    }

    println!("generated Rust per line of dat, shown but not judged");
    let (mut dat_total, mut rust_total) = (0, 0);
    for name in
        ["fib35", "ack", "mutual", "primes", "rows", "floats", "wordcount", "groups", "append", "bfs", "sortcmp", "chains", "search"]
    {
        let src = std::fs::read_to_string(bench.join(format!("{name}.dat"))).unwrap_or_default();
        let rust = run_in(&bench, &["rust", &format!("{name}.dat")]).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string());
        let (d, r) = (code_lines(&src), code_lines(&rust.unwrap_or_default()));
        dat_total += d;
        rust_total += r;
        println!("  {name:<44} {d:>5} dat lines {r:>6} Rust lines   {:.1} per line", r as f64 / d.max(1) as f64);
    }
    println!(
        "  {:<44} {dat_total:>5} dat lines {rust_total:>6} Rust lines   {:.1} per line",
        "all",
        rust_total as f64 / dat_total.max(1) as f64
    );

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

/// Lines that hold code: not blank, and not only a comment.
fn code_lines(text: &str) -> usize {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")).count()
}

fn dat_codegen_toolchain() -> &'static str {
    // Same pin as dat-codegen; kept in sync by its unit test.
    "1.99.0"
}
