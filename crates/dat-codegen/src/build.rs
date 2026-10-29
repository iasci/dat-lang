//! Builds generated Rust with Cargo and runs it.
//!
//! Each entry gets a small crate in the cache, keyed by its source files and
//! mode, so an unchanged program is a Cargo no-op and an edited one rebuilds
//! incrementally. All crates share one target directory, so dat-rt is
//! compiled once. The toolchain is pinned to the one dat itself uses.
//!
//! Two runs of one file share its crate, so each holds the crate's lock
//! from writing its source until its program has started.
//!
//! dat-rt's dependencies are pinned: each generated crate gets a
//! `Cargo.lock` holding exactly the versions in dat's own, and builds with
//! `--locked`. dat's own build has downloaded them, so a generated crate
//! builds without network.

use crate::{Entry, emit_crate};
use dat_ir::Program;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The toolchain generated crates build with: the one in dat's own
/// `rust-toolchain.toml`, so both run modes use the same std.
pub const TOOLCHAIN: &str = "1.99.0";

/// Traces map each native frame's debug location to a dat call, so code for
/// two different calls must never be shared. These turn off the
/// optimizations that share it:
///
/// - MergeFunctions folds functions whose optimized bodies are equal and
///   keeps the debug info of one, which would name the wrong function.
///   Stable rustc can't turn it off, so `RUSTC_BOOTSTRAP` enables the
///   unstable flag, for the pinned toolchain only.
/// - SimplifyCFG's hoisting and sinking, and tail merging in code
///   generation, turn calls in two branches into one call whose location
///   is neither.
///
/// Each costs little: they mostly save code size.
const TRACE_FLAGS: &[&str] =
    &["-Zmerge-functions=disabled", "-Cllvm-args=-simplifycfg-sink-common=false -simplifycfg-hoist-common=false -enable-tail-merge=false"];

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

/// dat's own lock file, which pins dat-rt's dependencies.
const WORKSPACE_LOCK: &str = include_str!("../../../Cargo.lock");

/// The `Cargo.lock` of the generated crate `name`: dat-rt and everything it
/// depends on, as dat's own lock file has them, plus the crate itself. It is
/// what Cargo would write, so `--locked` accepts it.
fn lock_file(name: &str) -> String {
    let mut parts = WORKSPACE_LOCK.split("\n[[package]]\n");
    let header = parts.next().unwrap_or_default();
    let packages: Vec<Package> = parts.map(Package::parse).collect();
    let mut wanted = vec!["dat-rt"];
    let mut keep: Vec<&Package> = Vec::new();
    while let Some(dep) = wanted.pop() {
        let mut words = dep.split(' ');
        let (dep_name, dep_version) = (words.next().unwrap_or_default(), words.next());
        if let Some(p) = packages.iter().find(|p| p.name == dep_name && dep_version.is_none_or(|v| v == p.version))
            && !keep.iter().any(|k| std::ptr::eq(*k, p))
        {
            keep.push(p);
            wanted.extend(p.deps.iter().copied());
        }
    }
    let root = format!("name = \"{name}\"\nversion = \"0.0.0\"\ndependencies = [\n \"dat-rt\",\n]\n");
    let mut blocks: Vec<(&str, &str, String)> = keep.iter().map(|p| (p.name, p.version, p.text(&keep))).collect();
    blocks.push((name, "0.0.0", root));
    blocks.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    let mut out = header.to_string();
    for (_, _, text) in blocks {
        out.push_str("\n[[package]]\n");
        out.push_str(&text);
    }
    out
}

/// One `[[package]]` entry of a lock file.
struct Package {
    name: &'static str,
    version: &'static str,
    /// Its lines before `dependencies`.
    head: &'static str,
    /// Each dependency as the lock file writes it: a name, followed by a
    /// version when the file holds two versions of that name.
    deps: Vec<&'static str>,
}

impl Package {
    fn parse(text: &'static str) -> Package {
        let field =
            |key: &str| text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(" = \"")?.strip_suffix('"')).unwrap_or_default();
        let (head, deps) = match text.split_once("dependencies = [\n") {
            Some((head, rest)) => {
                let list = rest.split_once(']').map_or(rest, |(l, _)| l);
                (head, list.lines().filter_map(|l| l.trim().strip_prefix('"')?.strip_suffix("\",")).collect())
            }
            None => (text.trim_end_matches('\n'), Vec::new()),
        };
        Package { name: field("name"), version: field("version"), head: head.trim_end_matches('\n'), deps }
    }

    /// The entry as it reads among `all`: a dependency's version is written
    /// only if `all` holds two versions of its name.
    fn text(&self, all: &[&Package]) -> String {
        let mut out = format!("{}\n", self.head);
        if !self.deps.is_empty() {
            out.push_str("dependencies = [\n");
            for dep in &self.deps {
                let name = dep.split(' ').next().unwrap_or_default();
                let shown = if all.iter().filter(|p| p.name == name).count() > 1 { dep } else { name };
                out.push_str(&format!(" \"{shown}\",\n"));
            }
            out.push_str("]\n");
        }
        out
    }
}

struct Crate {
    dir: PathBuf,
    name: String,
    /// Released when dropped. Otherwise a second run could rewrite
    /// `main.rs` while the first builds it, or replace the executable
    /// before the first starts it.
    _lock: std::fs::File,
}

fn prepare(programs: &[&Program], entry: &Entry, kind: &str) -> Result<Crate, String> {
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
    let guard = if crate::no_stack_guard() { "no-guard" } else { "guard" };
    let mut key: Vec<&str> = vec![kind, &cwd, guard];
    key.extend(programs.iter().map(|p| p.file.as_str()));
    let name = format!("dat_{:016x}", hash(&key));
    let dir = cache_dir().join("crates").join(&name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    let lock_path = dir.join(".lock");
    let lock = std::fs::File::create(&lock_path).map_err(|e| format!("can't create {}: {e}", lock_path.display()))?;
    lock.lock().map_err(|e| format!("can't lock {}: {e}", lock_path.display()))?;
    // dat-rt's dependencies only symbolize a trace after an error, so they
    // build without debug info and at opt-level 1, which symbolizes as fast
    // as 3 and builds a little faster.
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
         [dependencies]\ndat-rt = {{ path = {:?} }}\n\n\
         [profile.release]\nopt-level = 3\ndebug = \"line-tables-only\"\nstrip = false\npanic = \"unwind\"\nincremental = true\noverflow-checks = false\n\n\
         [profile.release.package.\"*\"]\nopt-level = 1\ndebug = false\n\n\
         [profile.release.package.dat-rt]\nopt-level = 3\ndebug = \"line-tables-only\"\n\n\
         [profile.dat-dist]\ninherits = \"release\"\nincremental = false\nlto = false\n{DIST_DEBUG_INFO}\n\
         [workspace]\n",
        runtime_dir().display().to_string()
    );
    write_if_changed(&dir.join("Cargo.toml"), &manifest)?;
    write_if_changed(&dir.join("Cargo.lock"), &lock_file(&name))?;
    write_if_changed(&dir.join("rust-toolchain.toml"), &format!("[toolchain]\nchannel = \"{TOOLCHAIN}\"\n"))?;
    write_if_changed(&dir.join("src/main.rs"), &emit_crate(programs, entry))?;
    Ok(Crate { dir, name, _lock: lock })
}

/// Where `dat build` executables keep the line tables their traces read. On
/// macOS they would stay in the build's object files, so rustc writes a
/// `.dSYM` that `dat build` puts next to the executable.
const DIST_DEBUG_INFO: &str = if cfg!(target_os = "macos") { "split-debuginfo = \"packed\"\n" } else { "" };

/// Linker arguments for `dat build` executables. Debug info is most of an
/// executable, and `backtrace` reads zlib-compressed sections, so on Linux
/// they're compressed: printing a trace then takes about 30 ms longer.
const DIST_LINK_ARGS: &[&str] = if cfg!(target_os = "linux") { &["-Clink-arg=-Wl,--compress-debug-sections=zlib"] } else { &[] };

fn cargo_build(c: &Crate, profile: &str) -> Result<PathBuf, String> {
    let target = cache_dir().join("target");
    let mut cmd = Command::new("cargo");
    let link_args = if profile == "dat-dist" { DIST_LINK_ARGS } else { &[] };
    // `cargo rustc` passes the arguments after `--` to the generated crate only.
    cmd.arg(if link_args.is_empty() { "build" } else { "rustc" });
    cmd.arg("--quiet").arg("--locked").arg("--profile").arg(profile).arg("--manifest-path").arg(c.dir.join("Cargo.toml"));
    if !link_args.is_empty() {
        cmd.arg("--").args(link_args);
    }
    for var in
        ["RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR", "CARGO_ENCODED_RUSTFLAGS"]
    {
        cmd.env_remove(var);
    }
    // rustc recurses over deeply nested expressions too; give it room.
    cmd.env("CARGO_TARGET_DIR", &target).env("RUSTUP_TOOLCHAIN", TOOLCHAIN).env("RUST_MIN_STACK", (256u64 << 20).to_string());
    cmd.env("RUSTC_BOOTSTRAP", "1").env("CARGO_ENCODED_RUSTFLAGS", TRACE_FLAGS.join("\u{1f}"));
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
    let mut child = cmd.spawn().map_err(|e| format!("can't run {}: {e}", exe.display()))?;
    // The program has started from this build, so another run may rebuild.
    drop(c);
    let status = child.wait().map_err(|e| format!("can't run {}: {e}", exe.display()))?;
    Ok(exit_code(status))
}

/// `dat build`: compiles `program` with the distribution profile and copies
/// the executable to `out`.
pub fn build_executable(program: &Program, out: &Path) -> Result<(), String> {
    let c = prepare(&[program], &Entry::Run, "build")?;
    let exe = cargo_build(&c, "dat-dist")?;
    std::fs::copy(&exe, out).map_err(|e| format!("can't write {}: {e}", out.display()))?;
    if cfg!(target_os = "macos") {
        copy_dsym(&exe, out)?;
    }
    Ok(())
}

/// Puts the line tables of `exe` next to its copy `out`, as `out.dSYM`,
/// where `backtrace` looks for them.
fn copy_dsym(exe: &Path, out: &Path) -> Result<(), String> {
    let mut dsym = out.as_os_str().to_owned();
    dsym.push(".dSYM");
    let dsym = PathBuf::from(dsym);
    let _ = std::fs::remove_dir_all(&dsym);
    let mut built = exe.as_os_str().to_owned();
    built.push(".dSYM");
    let built = PathBuf::from(built);
    if built.is_dir() {
        return copy_dir(&built, &dsym);
    }
    let status = Command::new("dsymutil").arg(exe).arg("-o").arg(&dsym).status().map_err(|e| format!("can't run dsymutil: {e}"))?;
    if !status.success() {
        return Err(format!("dsymutil couldn't write {}", dsym.display()));
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("can't create {}: {e}", to.display()))?;
    let entries = std::fs::read_dir(from).map_err(|e| format!("can't read {}: {e}", from.display()))?;
    for entry in entries.flatten() {
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| format!("can't write {}: {e}", dst.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn lock_file_holds_dat_rt_and_its_dependencies() {
        let lock = super::lock_file("dat_0");
        assert!(lock.starts_with("# This file is automatically @generated by Cargo."));
        assert!(lock.contains("\n[[package]]\nname = \"dat-rt\"\n"));
        assert!(lock.contains("\n[[package]]\nname = \"backtrace\"\n"));
        assert!(lock.contains("\n[[package]]\nname = \"dat_0\"\nversion = \"0.0.0\"\ndependencies = [\n \"dat-rt\",\n]\n"));
        assert!(!lock.contains("name = \"dat-check\""));
        assert!(lock.ends_with("]\n") || lock.ends_with("\"\n"));
    }

    #[test]
    fn toolchain_matches_the_repository_pin() {
        let toml = include_str!("../../../rust-toolchain.toml");
        assert!(toml.contains(&format!("channel = \"{}\"", super::TOOLCHAIN)));
    }
}
