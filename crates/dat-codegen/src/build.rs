//! Builds generated Rust with Cargo and runs it.
//!
//! Each entry gets a small crate in the cache, keyed by its source files and
//! mode, so an unchanged program is a Cargo no-op and an edited one rebuilds
//! incrementally. All crates share one target directory, so dat-rt is
//! compiled once. The toolchain is pinned to the one dat itself uses.

use crate::{Entry, emit_crate};
use dat_ir::Program;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The toolchain generated crates build with: the one in dat's own
/// `rust-toolchain.toml`, so both run modes use the same std.
pub const TOOLCHAIN: &str = "1.99.0";

pub enum Mode {
    Run,
    Test { filter: Option<String>, verbose: bool },
}

pub fn cache_dir() -> PathBuf {
    if let Ok(d) = std::env::var("DAT_CACHE_DIR") {
        return PathBuf::from(d);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/dat-cache")
}

fn runtime_dir() -> PathBuf {
    let p = std::env::var("DAT_RT_PATH").map(PathBuf::from).unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../dat-rt"));
    p.canonicalize().unwrap_or(p)
}

/// FNV-1a, for cache keys.
fn hash(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.bytes().chain([0xff]) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

/// Writes `contents` unless the file already holds it, so Cargo's
/// modification-time fingerprints stay valid.
fn write_if_changed(path: &Path, contents: &str) -> Result<(), String> {
    if std::fs::read_to_string(path).is_ok_and(|old| old == contents) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    }
    std::fs::write(path, contents).map_err(|e| format!("can't write {}: {e}", path.display()))
}

struct Crate {
    dir: PathBuf,
    name: String,
}

fn prepare(programs: &[&Program], entry: &Entry, kind: &str) -> Result<Crate, String> {
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
    let guard = if crate::no_stack_guard() { "no-guard" } else { "guard" };
    let mut key: Vec<&str> = vec![kind, &cwd, guard];
    key.extend(programs.iter().map(|p| p.file.as_str()));
    let name = format!("dat_{:016x}", hash(&key));
    let dir = cache_dir().join("crates").join(&name);
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
         [dependencies]\ndat-rt = {{ path = {:?} }}\n\n\
         [profile.release]\nopt-level = 3\ndebug = \"line-tables-only\"\npanic = \"unwind\"\nincremental = true\noverflow-checks = false\n\n\
         [profile.dat-dist]\ninherits = \"release\"\nincremental = false\ndebug = false\nlto = false\n\n\
         [workspace]\n",
        runtime_dir().display().to_string()
    );
    write_if_changed(&dir.join("Cargo.toml"), &manifest)?;
    write_if_changed(&dir.join("rust-toolchain.toml"), &format!("[toolchain]\nchannel = \"{TOOLCHAIN}\"\n"))?;
    write_if_changed(&dir.join("src/main.rs"), &emit_crate(programs, entry))?;
    Ok(Crate { dir, name })
}

fn cargo_build(c: &Crate, profile: &str) -> Result<PathBuf, String> {
    let target = cache_dir().join("target");
    let mut cmd = Command::new("cargo");
    cmd.arg("build").arg("--quiet").arg("--profile").arg(profile).arg("--manifest-path").arg(c.dir.join("Cargo.toml"));
    for var in
        ["RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR", "CARGO_ENCODED_RUSTFLAGS"]
    {
        cmd.env_remove(var);
    }
    // rustc recurses over deeply nested expressions too; give it room.
    cmd.env("CARGO_TARGET_DIR", &target).env("RUSTUP_TOOLCHAIN", TOOLCHAIN).env("RUST_MIN_STACK", (256u64 << 20).to_string());
    let out = cmd.stdin(Stdio::null()).output().map_err(|e| format!("can't run cargo: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "internal error: the generated Rust in {} didn't compile. This is a bug in dat.\n{}",
            c.dir.display(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let dir = if profile == "release" { "release" } else { profile };
    Ok(target.join(dir).join(&c.name))
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return 128 + sig;
        }
    }
    status.code().unwrap_or(1)
}

/// Compiles the programs and runs the result with stdio inherited. Returns
/// the program's exit status.
pub fn run_compiled(programs: &[&Program], mode: Mode) -> Result<i32, String> {
    let (entry, kind) = match &mode {
        Mode::Run => (Entry::Run, "run"),
        Mode::Test { .. } => (Entry::Test, "test"),
    };
    let c = prepare(programs, &entry, kind)?;
    let exe = cargo_build(&c, "release")?;
    let mut cmd = Command::new(&exe);
    if let Mode::Test { filter, verbose } = &mode {
        if let Some(f) = filter {
            cmd.arg("--filter").arg(f);
        }
        if *verbose {
            cmd.arg("--verbose");
        }
    }
    let status = cmd.status().map_err(|e| format!("can't run {}: {e}", exe.display()))?;
    Ok(exit_code(status))
}

/// `dat build`: compiles `program` with the distribution profile and copies
/// the executable to `out`.
pub fn build_executable(program: &Program, out: &Path) -> Result<(), String> {
    let c = prepare(&[program], &Entry::Run, "build")?;
    let exe = cargo_build(&c, "dat-dist")?;
    std::fs::copy(&exe, out).map_err(|e| format!("can't write {}: {e}", out.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn toolchain_matches_the_repository_pin() {
        let toml = include_str!("../../../rust-toolchain.toml");
        assert!(toml.contains(&format!("channel = \"{}\"", super::TOOLCHAIN)));
    }
}
