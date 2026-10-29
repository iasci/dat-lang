//! The `dat` command. See `reference/21-command-line.md`.

#![forbid(unsafe_code)]

use dat_check::{Compiled, Options};
use dat_diag::{Diagnostic, SourceMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
usage: dat <command> [options] [path]

commands:
  check [path]       type check without running; reports every error
  run <file>         run a program, starting at its main function
  test [path]        run tests
  build <file>       compile through Rust into a native executable
  fix [path]         apply every safe fix, then report the errors that are left
  ir <file>          print the typed IR
  rust <file>        print the generated Rust

options:
  --compiled         (run, test) compile to Rust and run that instead of the interpreter
  --json             print diagnostics as JSON
  --filter <text>    (test) run only the tests whose name contains text
  --verbose          (test) also list each test that passes
  -o <path>          (build) where to write the executable
";

struct Args {
    command: String,
    paths: Vec<String>,
    compiled: bool,
    json: bool,
    verbose: bool,
    filter: Option<String>,
    output: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let command = it.next().ok_or_else(|| "missing command".to_string())?;
    if command == "--help" || command == "-h" || command == "help" {
        print!("{USAGE}");
        std::process::exit(0);
    }
    let mut a = Args { command, paths: Vec::new(), compiled: false, json: false, verbose: false, filter: None, output: None };
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--compiled" => a.compiled = true,
            "--json" => a.json = true,
            "--verbose" => a.verbose = true,
            "--filter" => a.filter = Some(it.next().ok_or("--filter needs a value")?),
            "-o" | "--output" => a.output = Some(it.next().ok_or("-o needs a path")?),
            s if s.starts_with('-') => return Err(format!("unknown option `{s}`")),
            _ => a.paths.push(arg),
        }
    }
    Ok(a)
}

/// The checker and code generator recurse over the program, so they run on a
/// thread with room for deeply nested code.
const COMPILER_STACK: usize = 256 << 20;

fn main() -> ExitCode {
    let worker = std::thread::Builder::new().name("dat".into()).stack_size(COMPILER_STACK).spawn(run_cli);
    match worker.map(|w| w.join()) {
        Ok(Ok(code)) => code,
        _ => ExitCode::from(2),
    }
}

fn run_cli() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("dat: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let code = match args.command.as_str() {
        "check" => cmd_check(&args),
        "run" => cmd_run(&args),
        "test" => cmd_test(&args),
        "build" => cmd_build(&args),
        "fix" => cmd_fix(&args),
        "ir" => cmd_ir(&args),
        "rust" => cmd_rust(&args),
        other => {
            eprintln!("dat: unknown command `{other}`\n\n{USAGE}");
            2
        }
    };
    ExitCode::from(code as u8)
}

/// The `.dat` files a path names: the file itself, or every `.dat` file under a folder.
fn dat_files(paths: &[String]) -> Result<Vec<String>, String> {
    let roots: Vec<String> = if paths.is_empty() { vec![".".into()] } else { paths.to_vec() };
    let mut out = Vec::new();
    for root in roots {
        let p = Path::new(&root);
        if p.is_file() {
            out.push(root);
        } else if p.is_dir() {
            walk(p, &mut out);
        } else {
            return Err(format!("`{root}` doesn't exist"));
        }
    }
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else if name.ends_with(".dat") {
            let s = p.to_string_lossy();
            out.push(s.strip_prefix("./").unwrap_or(&s).to_string());
        }
    }
}

fn single_file(args: &Args) -> Result<String, i32> {
    match args.paths.as_slice() {
        [one] if Path::new(one).is_file() => Ok(one.clone()),
        [one] => {
            eprintln!("dat: `{one}` doesn't exist or isn't a file");
            Err(2)
        }
        _ => {
            eprintln!("dat {}: expected one file\n\n{USAGE}", args.command);
            Err(2)
        }
    }
}

/// Compiles every file; returns the results and all diagnostics in order.
fn compile_all(files: &[String], opts: Options) -> Result<(SourceMap, Vec<Compiled>), i32> {
    let mut sources = SourceMap::new();
    let mut out = Vec::new();
    for f in files {
        let text = match std::fs::read_to_string(f) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("dat: can't read `{f}`: {e}");
                return Err(2);
            }
        };
        out.push(dat_check::compile(&mut sources, f, text, opts));
    }
    Ok((sources, out))
}

fn all_diags(compiled: &[Compiled]) -> Vec<Diagnostic> {
    let mut diags: Vec<Diagnostic> = compiled.iter().flat_map(|c| c.diags.iter().cloned()).collect();
    dat_diag::sort(&mut diags);
    diags
}

fn report_diags(sources: &SourceMap, diags: &[Diagnostic], json: bool) {
    if json {
        eprint!("{}", dat_diag::render_json(sources, diags));
    } else if !diags.is_empty() {
        eprint!("{}", dat_diag::render_all(sources, diags));
    }
}

fn has_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.severity == dat_diag::Severity::Error)
}

fn cmd_check(args: &Args) -> i32 {
    let files = match dat_files(&args.paths) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("dat: {e}");
            return 2;
        }
    };
    let (sources, compiled) = match compile_all(&files, Options::default()) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let diags = all_diags(&compiled);
    report_diags(&sources, &diags, args.json);
    if has_errors(&diags) { 1 } else { 0 }
}

/// Compiles one file for `run` or `build`. Compile errors give status 2 for
/// `run`, which couldn't do its job, and 1 for `build`, which found errors.
fn compile_program(args: &Args, error_status: i32) -> Result<dat_ir::Program, i32> {
    let file = single_file(args)?;
    let (sources, mut compiled) = compile_all(&[file], Options { require_main: true })?;
    let c = compiled.pop().unwrap();
    report_diags(&sources, &c.diags, args.json && !c.diags.is_empty());
    c.program.ok_or(error_status)
}

fn cmd_run(args: &Args) -> i32 {
    let program = match compile_program(args, 2) {
        Ok(p) => p,
        Err(code) => return code,
    };
    if args.compiled {
        return match dat_codegen::build::run_compiled(&[&program], dat_codegen::build::Mode::Run) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("dat: {e}");
                2
            }
        };
    }
    dat_interp::run_main(&program)
}

fn cmd_test(args: &Args) -> i32 {
    let files = match dat_files(&args.paths) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("dat: {e}");
            return 2;
        }
    };
    let (sources, compiled) = match compile_all(&files, Options::default()) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let diags = all_diags(&compiled);
    if has_errors(&diags) {
        report_diags(&sources, &diags, args.json);
        return 2;
    }
    let programs: Vec<dat_ir::Program> = compiled.into_iter().filter_map(|c| c.program).collect();
    let refs: Vec<&dat_ir::Program> = programs.iter().filter(|p| !p.tests.is_empty()).collect();
    let filter = args.filter.as_deref();
    let matching: usize = refs.iter().map(|p| p.tests.iter().filter(|t| filter.is_none_or(|f| t.name.contains(f))).count()).sum();
    if matching == 0 {
        match filter {
            Some(f) => eprintln!("dat test: no test name contains `{f}`"),
            None => eprintln!("dat test: no tests found"),
        }
        return 2;
    }
    if args.compiled {
        let mode = dat_codegen::build::Mode::Test { filter: args.filter.clone(), verbose: args.verbose };
        return match dat_codegen::build::run_compiled(&refs, mode) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("dat: {e}");
                2
            }
        };
    }
    dat_interp::run_tests(&refs, filter, args.verbose)
}

fn cmd_build(args: &Args) -> i32 {
    let program = match compile_program(args, 1) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let out = args
        .output
        .clone()
        .unwrap_or_else(|| Path::new(&args.paths[0]).file_stem().and_then(|s| s.to_str()).unwrap_or("a.out").to_string());
    match dat_codegen::build::build_executable(&program, Path::new(&out)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("dat: {e}");
            2
        }
    }
}

fn cmd_fix(args: &Args) -> i32 {
    let files = match dat_files(&args.paths) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("dat: {e}");
            return 2;
        }
    };
    // Apply safe fixes until none are left; each round re-checks.
    for _ in 0..4 {
        let (sources, compiled) = match compile_all(&files, Options::default()) {
            Ok(r) => r,
            Err(code) => return code,
        };
        let mut changed = false;
        for (c, path) in compiled.iter().zip(&files) {
            let text = &sources.get(c.file).text;
            if let Some(fixed) = dat_diag::apply_safe_fixes(text, c.file, &c.diags)
                && fixed != *text
            {
                if let Err(e) = std::fs::write(path, &fixed) {
                    eprintln!("dat: can't write `{path}`: {e}");
                    return 2;
                }
                changed = true;
            }
        }
        if !changed {
            let diags = all_diags(&compiled);
            report_diags(&sources, &diags, args.json);
            return if has_errors(&diags) { 1 } else { 0 };
        }
    }
    let (sources, compiled) = match compile_all(&files, Options::default()) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let diags = all_diags(&compiled);
    report_diags(&sources, &diags, args.json);
    if has_errors(&diags) { 1 } else { 0 }
}

fn cmd_ir(args: &Args) -> i32 {
    match compile_program_opts(args, Options::default()) {
        Ok(p) => {
            print!("{}", dat_ir::dump(&p));
            0
        }
        Err(code) => code,
    }
}

/// Prints the crate `dat run --compiled` builds, or for a file with tests
/// and no `main`, the one `dat test --compiled` builds.
fn cmd_rust(args: &Args) -> i32 {
    let p = match compile_program_opts(args, Options::default()) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let entry = match (p.main, p.tests.is_empty()) {
        (Some(_), _) => dat_codegen::Entry::Run,
        (None, false) => dat_codegen::Entry::Test,
        // Neither: report the missing `main` as `dat run` does.
        (None, true) => return compile_program_opts(args, Options { require_main: true }).err().unwrap_or(1),
    };
    print!("{}", dat_codegen::emit_crate(&[&p], &entry));
    0
}

fn compile_program_opts(args: &Args, opts: Options) -> Result<dat_ir::Program, i32> {
    let file = single_file(args)?;
    let (sources, mut compiled) = compile_all(&[file], opts)?;
    let c = compiled.pop().unwrap();
    report_diags(&sources, &c.diags, args.json && !c.diags.is_empty());
    c.program.ok_or(1)
}
