//! Records the commit `dat --version` reports.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let commit = git(&["describe", "--always", "--dirty", "--abbrev=12"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=DAT_COMMIT={commit}");
    // Describe again after a commit or checkout, or a change to the sources
    // dat is built from, which may make the tree dirty. In a worktree,
    // `.git` is a file, so the git directories come from git itself. A path
    // that doesn't exist would rerun this on every build.
    let mut watch: Vec<PathBuf> = vec!["../../crates".into(), "../../Cargo.toml".into(), "../../Cargo.lock".into()];
    let dirs = ["--absolute-git-dir", "--git-common-dir"].map(|d| git(&["rev-parse", "--path-format=absolute", d]));
    if let [Some(dir), Some(common)] = dirs {
        let (dir, common) = (Path::new(&dir), Path::new(&common));
        watch.extend([dir.join("HEAD"), dir.join("index"), common.join("refs/heads"), common.join("packed-refs")]);
    }
    for p in watch.iter().filter(|p| p.exists()) {
        println!("cargo:rerun-if-changed={}", p.display());
    }
}
