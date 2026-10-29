//! Example programs with golden results, run in both modes.
//!
//! `examples/**/X.dat` has its expected results in `X.golden`:
//!
//! ```text
//! === run
//! status: 0
//! --- stdout
//! fib(25) = 75025
//! --- stderr
//! ```
//!
//! Each `===` section names a `dat` command. `run` and `test` sections run on
//! the interpreter and with `--compiled`, and both must match. If `X.fixed`
//! exists, the safe fixes from `dat check --json` are applied mechanically
//! and must give exactly that text, as must `dat fix`; the fixed program must
//! check clean and give the `run (fixed)` section in both modes.

use crate::Opts;
use crate::run::{Outcome, dat, diff};
use dat_diag::Json;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
struct Section {
    command: String,
    expected: Option<Outcome>,
}

fn parse_golden(text: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut block: Option<&str> = None;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches('\n');
        if let Some(cmd) = bare.strip_prefix("=== ") {
            // The blank line before a section separates it; it isn't output.
            if let (Some(prev), Some(b)) = (out.last_mut(), block) {
                let e = prev.expected.as_mut().unwrap();
                let t = if b == "stdout" { &mut e.stdout } else { &mut e.stderr };
                if t.ends_with("\n\n") {
                    t.pop();
                }
            }
            out.push(Section {
                command: cmd.to_string(),
                expected: Some(Outcome { status: 0, stdout: String::new(), stderr: String::new() }),
            });
            block = None;
            continue;
        }
        let Some(cur) = out.last_mut() else { continue };
        let exp = cur.expected.as_mut().unwrap();
        if let Some(s) = bare.strip_prefix("status: ") {
            exp.status = s.trim().parse().unwrap_or(-1);
            block = None;
        } else if bare == "--- stdout" {
            block = Some("stdout");
        } else if bare == "--- stderr" {
            block = Some("stderr");
        } else {
            match block {
                Some("stdout") => exp.stdout.push_str(line),
                Some("stderr") => exp.stderr.push_str(line),
                _ => {}
            }
        }
    }
    out
}

fn write_golden(name: &str, sections: &[(String, Outcome)]) -> String {
    let mut out = format!(
        "# Golden results for {name}. `dat-conformance examples --bless` rewrites this file.\n\
         # `run` and `test` sections must match on the interpreter and compiled.\n"
    );
    for (cmd, o) in sections {
        out.push_str(&format!("\n=== {cmd}\nstatus: {}\n", o.status));
        for (label, text) in [("stdout", &o.stdout), ("stderr", &o.stderr)] {
            if !text.is_empty() {
                out.push_str(&format!("--- {label}\n{text}"));
                if !text.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
    }
    out
}

fn find_examples(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            find_examples(&p, out);
        } else if p.extension().is_some_and(|e| e == "dat") {
            out.push(p);
        }
    }
}

fn split_command(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().map(str::to_string).collect()
}

/// Runs a section's command; for `run` and `test`, in both modes.
fn run_section(dir: &Path, file: &str, cmd: &str, opts: &Opts) -> Vec<(&'static str, Outcome)> {
    let words = split_command(cmd);
    let mut args: Vec<&str> = words.iter().map(String::as_str).collect();
    args.push(file);
    let both = matches!(args[0], "run" | "test");
    // `ir` and `rust` print to stdout and are compared like the others.
    let mut out = vec![(if both { "interpreter" } else { "" }, dat(dir, &args, &[]))];
    if both && !opts.interp_only {
        let mut compiled = args.clone();
        compiled.insert(1, "--compiled");
        out.push(("compiled", dat(dir, &compiled, &[])));
    }
    out
}

fn default_commands(src: &str, has_fixed: bool) -> Vec<String> {
    let mut cmds = vec!["check".to_string()];
    let has_main = src.contains("function main(");
    let has_tests = src.lines().any(|l| l.starts_with("test \""));
    if has_main {
        cmds.push("run".into());
    }
    if has_tests {
        cmds.push("test".into());
    }
    if has_fixed {
        cmds.push("check --json".into());
        cmds.push("run (fixed)".into());
    }
    cmds
}

struct Report {
    ok: usize,
    failed: Vec<String>,
}

impl Report {
    fn check(&mut self, what: String, expected: &Outcome, actual: &Outcome, verbose: bool) {
        if expected == actual {
            self.ok += 1;
            if verbose {
                println!("ok    {what}");
            }
            return;
        }
        let mut msg = format!("FAIL  {what}\n");
        if expected.status != actual.status {
            msg.push_str(&format!("  status: expected {}, got {}\n", expected.status, actual.status));
        }
        if expected.stdout != actual.stdout {
            msg.push_str(&format!("  stdout:\n{}", diff(&expected.stdout, &actual.stdout)));
        }
        if expected.stderr != actual.stderr {
            msg.push_str(&format!("  stderr:\n{}", diff(&expected.stderr, &actual.stderr)));
        }
        println!("{msg}");
        self.failed.push(what);
    }
}

/// Applies the preferred safe fix of every diagnostic in a `--json` document to `text`.
fn apply_json_fixes(json: &str, text: &str) -> Result<String, String> {
    let doc = Json::parse(json)?;
    let mut edits: Vec<(usize, usize, String, String)> = Vec::new();
    for d in doc.get("diagnostics").map(Json::as_arr).unwrap_or(&[]) {
        let fixes = d.get("fixes").map(Json::as_arr).unwrap_or(&[]);
        let Some(fix) = fixes.iter().find(|f| f.get("applicability").and_then(Json::as_str) == Some("safe")) else { continue };
        for e in fix.get("edits").map(Json::as_arr).unwrap_or(&[]) {
            let n = |k: &str| e.get(k).and_then(Json::as_i64).unwrap_or(0) as usize;
            let s = |k: &str| e.get(k).and_then(Json::as_str).unwrap_or("").to_string();
            edits.push((n("byteStart"), n("byteEnd"), s("before"), s("after")));
        }
    }
    edits.sort_by_key(|e| e.0);
    let mut out = String::new();
    let mut pos = 0;
    for (start, end, before, after) in edits {
        if start < pos || text.get(start..end) != Some(before.as_str()) {
            return Err(format!("stale edit at bytes {start}..{end}: expected {before:?}"));
        }
        out.push_str(&text[pos..start]);
        out.push_str(&after);
        pos = end;
    }
    out.push_str(&text[pos..]);
    Ok(out)
}

/// Structural rules: both backends read only the IR (criterion 4), and
/// generated Rust forbids `unsafe` and uses no `RefCell` or reflection
/// (criterion 6).
pub fn structure(opts: &Opts) -> bool {
    let mut ok = true;
    let out = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps", "--offline"])
        .current_dir(&opts.root)
        .output()
        .expect("cargo metadata");
    let meta = Json::parse(&String::from_utf8_lossy(&out.stdout)).expect("cargo metadata JSON");
    let deps: std::collections::BTreeMap<String, Vec<String>> = meta
        .get("packages")
        .map(Json::as_arr)
        .unwrap_or(&[])
        .iter()
        .map(|p| {
            let name = p.get("name").and_then(Json::as_str).unwrap_or("").to_string();
            let d = p
                .get("dependencies")
                .map(Json::as_arr)
                .unwrap_or(&[])
                .iter()
                .filter(|d| d.get("kind").is_none_or(|k| *k == Json::Null))
                .filter_map(|d| d.get("name").and_then(Json::as_str).map(str::to_string))
                .collect();
            (name, d)
        })
        .collect();
    for backend in ["dat-interp", "dat-codegen"] {
        let mut seen = vec![backend.to_string()];
        let mut i = 0;
        while i < seen.len() {
            for d in deps.get(&seen[i]).cloned().unwrap_or_default() {
                if !seen.contains(&d) {
                    seen.push(d);
                }
            }
            i += 1;
        }
        let bad: Vec<&String> = seen.iter().filter(|d| *d == "dat-syntax" || *d == "dat-check").collect();
        if bad.is_empty() {
            println!("ok    {backend} depends only on {}", seen[1..].join(", "));
        } else {
            println!("FAIL  {backend} depends on {bad:?}; backends may read only the IR");
            ok = false;
        }
    }
    let mut files = Vec::new();
    find_examples(&opts.root.join("examples"), &mut files);
    for path in files {
        let dir = path.parent().unwrap();
        let file = path.file_name().unwrap().to_str().unwrap();
        let o = dat(dir, &["rust", file], &[]);
        if o.status != 0 {
            continue;
        }
        let code = &o.stdout;
        let problems: Vec<&str> = [
            (!code.contains("#![forbid(unsafe_code)]"), "no #![forbid(unsafe_code)]"),
            (code.contains("unsafe ") || code.contains("unsafe{"), "unsafe code"),
            (code.contains("RefCell"), "RefCell"),
            (code.contains("Any"), "dyn Any"),
            (has_macro_call(code), "macros"),
        ]
        .into_iter()
        .filter(|(bad, _)| *bad)
        .map(|(_, what)| what)
        .collect();
        if problems.is_empty() {
            if opts.verbose {
                println!("ok    generated Rust for {file}: forbid(unsafe_code), no RefCell, no reflection, no macros");
            }
        } else {
            println!("FAIL  generated Rust for {file}: {}", problems.join(", "));
            ok = false;
        }
    }
    println!("structure: {}", if ok { "ok" } else { "FAILED" });
    ok
}

/// `name!(`, `name![` or `name!{` outside string literals.
fn has_macro_call(code: &str) -> bool {
    code.lines().any(|line| {
        let code = line.split("//").next().unwrap_or("");
        let b = code.as_bytes();
        let mut in_str = false;
        for i in 1..b.len() {
            if b[i] == b'"' && b[i - 1] != b'\\' {
                in_str = !in_str;
            }
            if !in_str
                && b[i] == b'!'
                && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')
                && matches!(b.get(i + 1), Some(b'(' | b'[' | b'{'))
            {
                return true;
            }
        }
        false
    })
}

pub fn run(opts: &Opts) -> bool {
    let mut files = Vec::new();
    find_examples(&opts.root.join("examples"), &mut files);
    let mut report = Report { ok: 0, failed: Vec::new() };
    println!("examples: {} programs{}", files.len(), if opts.interp_only { " (interpreter only)" } else { "" });
    for path in &files {
        let dir = path.parent().unwrap();
        let file = path.file_name().unwrap().to_str().unwrap();
        let rel = path.strip_prefix(&opts.root).unwrap_or(path).display().to_string();
        let golden_path = path.with_extension("golden");
        let fixed_path = path.with_extension("fixed");
        let src = std::fs::read_to_string(path).unwrap_or_default();
        let sections = match std::fs::read_to_string(&golden_path) {
            Ok(text) => parse_golden(&text),
            Err(_) if opts.bless => {
                default_commands(&src, fixed_path.exists()).into_iter().map(|command| Section { command, expected: None }).collect()
            }
            Err(_) => {
                println!("FAIL  {rel}: no golden file {}", golden_path.display());
                report.failed.push(rel.clone());
                continue;
            }
        };
        let mut blessed = Vec::new();
        for s in &sections {
            if s.command == "run (fixed)" {
                let results = fixed_flow(path, &fixed_path, opts, &mut report, &rel);
                for (mode, actual) in results {
                    let expected = if opts.bless { None } else { s.expected.clone() };
                    let expected = expected.unwrap_or_else(|| actual.clone());
                    if blessed.iter().all(|(c, _): &(String, Outcome)| c != &s.command) {
                        blessed.push((s.command.clone(), actual.clone()));
                    }
                    report.check(format!("{rel}: {} ({mode})", s.command), &expected, &actual, opts.verbose);
                }
                continue;
            }
            let results = run_section(dir, file, &s.command, opts);
            let first = results[0].1.clone();
            for (mode, actual) in &results {
                let expected = if opts.bless { first.clone() } else { s.expected.clone().unwrap_or_else(|| first.clone()) };
                let label = if mode.is_empty() { format!("{rel}: {}", s.command) } else { format!("{rel}: {} ({mode})", s.command) };
                report.check(label, &expected, actual, opts.verbose);
            }
            blessed.push((s.command.clone(), first));
        }
        if opts.bless {
            let text = write_golden(file, &blessed);
            std::fs::write(&golden_path, text).expect("write golden");
        }
    }
    println!("examples: {} ok, {} failed", report.ok, report.failed.len());
    report.failed.is_empty()
}

/// The fix round trip: the JSON fixes and `dat fix` give `X.fixed`, which
/// checks clean. Returns the fixed program's `run` results in both modes.
fn fixed_flow(path: &Path, fixed_path: &Path, opts: &Opts, report: &mut Report, rel: &str) -> Vec<(&'static str, Outcome)> {
    let dir = path.parent().unwrap();
    let file = path.file_name().unwrap().to_str().unwrap();
    let src = std::fs::read_to_string(path).unwrap_or_default();
    let expected_fixed = std::fs::read_to_string(fixed_path).unwrap_or_default();
    let check = dat(dir, &["check", "--json", file], &[]);
    let tmp = opts.root.join("target/conformance-tmp").join(file.trim_end_matches(".dat"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let ok = |o: &str| Outcome { status: 0, stdout: o.to_string(), stderr: String::new() };
    match apply_json_fixes(&check.stderr, &src) {
        Ok(fixed) => {
            report.check(format!("{rel}: JSON fixes give {}", fixed_path.display()), &ok(&expected_fixed), &ok(&fixed), opts.verbose)
        }
        Err(e) => {
            println!("FAIL  {rel}: can't apply the JSON fixes: {e}");
            report.failed.push(rel.to_string());
        }
    }
    std::fs::write(tmp.join(file), &src).unwrap();
    let fix = dat(&tmp, &["fix", file], &[]);
    let after = std::fs::read_to_string(tmp.join(file)).unwrap_or_default();
    report.check(
        format!("{rel}: dat fix"),
        &ok(&expected_fixed),
        &Outcome { status: fix.status, stdout: after, stderr: fix.stderr },
        opts.verbose,
    );
    std::fs::write(tmp.join(file), &expected_fixed).unwrap();
    let clean = dat(&tmp, &["check", file], &[]);
    report.check(format!("{rel}: the fixed program checks clean"), &ok(""), &clean, opts.verbose);
    run_section(&tmp, file, "run", opts)
}

/// The differential harness must notice a wrong `+` in the interpreter.
pub fn self_test(opts: &Opts) -> bool {
    let dir = opts.root.join("examples/fib");
    let faulty = dat(&dir, &["run", "fib.dat"], &[("DAT_FAULT", "interp-plus-one")]);
    let reference = if opts.interp_only { dat(&dir, &["run", "fib.dat"], &[]) } else { dat(&dir, &["run", "--compiled", "fib.dat"], &[]) };
    let golden = parse_golden(&std::fs::read_to_string(dir.join("fib.golden")).unwrap_or_default());
    let expected = golden.iter().find(|s| s.command == "run").and_then(|s| s.expected.clone());
    let caught_by_diff = faulty != reference;
    let caught_by_golden = expected.as_ref().is_some_and(|e| *e != faulty) && expected.as_ref() == Some(&reference);
    if caught_by_diff && caught_by_golden {
        println!("self-test: ok, DAT_FAULT=interp-plus-one was reported as a difference:\n{}", diff(&reference.stdout, &faulty.stdout));
        true
    } else {
        println!("self-test: FAIL, the injected fault went unnoticed (mode diff: {caught_by_diff}, golden: {caught_by_golden})");
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_round_trip() {
        let o = Outcome { status: 1, stdout: "a\nb\n".into(), stderr: "e\n".into() };
        let text = write_golden(
            "x.dat",
            &[("run".into(), o.clone()), ("check".into(), Outcome { status: 0, stdout: String::new(), stderr: String::new() })],
        );
        let parsed = parse_golden(&text);
        assert_eq!(parsed[0].command, "run");
        assert_eq!(parsed[0].expected.as_ref().unwrap(), &o);
        assert_eq!(parsed[1].expected.as_ref().unwrap().stdout, "");
    }
}
