//! Running the `dat` binary and normalizing what it prints.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The `dat` binary built next to this one.
pub fn dat_binary() -> PathBuf {
    let me = std::env::current_exe().expect("current exe");
    let p = me.with_file_name(if cfg!(windows) { "dat.exe" } else { "dat" });
    if !p.exists() {
        eprintln!("dat-conformance: {} is missing; build it first with `cargo build -p dat` (same profile)", p.display());
        std::process::exit(2);
    }
    p
}

fn dat_command(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Command {
    let mut cmd = Command::new(dat_binary());
    cmd.args(args).current_dir(dir);
    cmd.env_remove("DAT_FAULT");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd
}

/// Runs `dat <args>` in `dir`.
pub fn dat(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Outcome {
    outcome(dat_command(dir, args, env).output().expect("failed to run dat"))
}

/// Starts `dat <args>` in `dir` without waiting; `outcome` reads the result.
pub fn dat_start(dir: &Path, args: &[&str]) -> Child {
    dat_command(dir, args, &[]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("failed to run dat")
}

pub fn outcome(out: Output) -> Outcome {
    Outcome {
        status: out.status.code().unwrap_or(-1),
        stdout: normalize(&String::from_utf8_lossy(&out.stdout)),
        stderr: normalize(&String::from_utf8_lossy(&out.stderr)),
    }
}

/// Replaces what may differ between runs: the time in a test summary.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.split_inclusive('\n') {
        match (line.rfind(" ("), line.trim_end().strip_suffix(" ms)")) {
            (Some(i), Some(_))
                if line.contains("passed (") && line[i + 2..].trim_end().trim_end_matches(" ms)").chars().all(|c| c.is_ascii_digit()) =>
            {
                out.push_str(&line[..i]);
                out.push_str(" (<ms>)");
                if line.ends_with('\n') {
                    out.push('\n');
                }
            }
            _ => out.push_str(line),
        }
    }
    out
}

/// A short unified view of two texts, for failure reports.
pub fn diff(expected: &str, actual: &str) -> String {
    let e: Vec<&str> = expected.lines().collect();
    let a: Vec<&str> = actual.lines().collect();
    let mut out = String::new();
    let n = e.len().max(a.len());
    for i in 0..n {
        match (e.get(i), a.get(i)) {
            (Some(x), Some(y)) if x == y => out.push_str(&format!("   {x}\n")),
            (x, y) => {
                if let Some(x) = x {
                    out.push_str(&format!(" - {x}\n"));
                }
                if let Some(y) = y {
                    out.push_str(&format!(" + {y}\n"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn normalizes_test_time() {
        assert_eq!(super::normalize("1 failed, 0 passed (12 ms)\n"), "1 failed, 0 passed (<ms>)\n");
        assert_eq!(super::normalize("took (12 ms)\n"), "took (12 ms)\n");
    }
}
