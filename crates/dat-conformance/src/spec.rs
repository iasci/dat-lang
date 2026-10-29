//! Spec-as-tests: every code block in `reference/` becomes cases.
//!
//! The reference's own comments are the annotation language:
//!
//! - `// ❌ E0301: message` with `//    fix: ...`, `//    fix (review, place): ...`
//!   and `//    option: ...` lines: an expected diagnostic and its fixes.
//! - `print(x); // text`: expected output.
//! - `<expr> // <literal>`: an expected value, checked with `assert`.
//! - `// throws RangeError` or `// throws RangeError: message`: the statement throws.
//!
//! A block with ❌ lines gives one case per ❌ line, with the other ❌ lines
//! removed, and one positive case with all of them removed. Blocks that are
//! output samples, shell commands or `{ ... }` placeholders are doc-only.
//! Cases that use something the milestone doesn't implement yet are
//! `unsupported`. The list of passing cases is a ratchet: it may only grow.

use crate::Opts;
use crate::run::{Outcome, dat};
use dat_diag::{Applicability, Diagnostic, SourceMap};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

struct Block {
    file: String,
    /// The line of the opening fence, 1-based.
    line: usize,
    lines: Vec<String>,
}

fn extract(root: &Path) -> Vec<Block> {
    let dir = root.join("reference");
    let mut files: Vec<_> =
        std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "md")).collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let mut open: Option<(usize, Vec<String>)> = None;
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                match open.take() {
                    None => open = Some((i + 1, Vec::new())),
                    Some((start, lines)) => out.push(Block { file: name.clone(), line: start, lines }),
                }
            } else if let Some((_, lines)) = &mut open {
                lines.push(line.to_string());
            }
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    Pass,
    Fail(String),
    Unsupported(String),
    DocOnly(String),
    XFail(String),
}

impl Status {
    fn label(&self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Fail(_) => "fail",
            Status::Unsupported(_) => "unsupported",
            Status::DocOnly(_) => "doc-only",
            Status::XFail(_) => "xfail",
        }
    }
}

/// A fix as the spec writes it.
#[derive(Clone, Debug)]
struct SpecFix {
    applicability: Applicability,
    /// The place in parentheses, such as `declaration`. The edit's position
    /// already says where it goes, so it's informational.
    #[allow(dead_code)]
    place: Option<String>,
    text: String,
}

#[derive(Clone, Debug)]
struct Expected {
    /// Index into the block's lines.
    line: usize,
    code: String,
    fixes: Vec<SpecFix>,
    /// The annotation's continuation lines, removed with it.
    extra_lines: usize,
    /// The line's code is only brackets, such as the `}` that ends a
    /// function, so it stays in every variant.
    structural: bool,
}

/// The code part of a line, without a trailing `//` comment outside strings.
fn split_comment(line: &str) -> (&str, Option<&str>) {
    let b = line.as_bytes();
    let mut in_str: Option<u8> = None;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match in_str {
            Some(q) => {
                if c == b'\\' {
                    i += 1;
                } else if c == q {
                    in_str = None;
                }
            }
            None => {
                if c == b'"' || c == b'`' {
                    in_str = Some(c);
                } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
                    return (&line[..i], Some(line[i + 2..].trim()));
                }
            }
        }
        i += 1;
    }
    (line, None)
}

fn parse_fix_line(comment: &str) -> Option<SpecFix> {
    let (head, text) = comment.split_once(':')?;
    let head = head.trim();
    let (kind, place) = match head.split_once('(') {
        Some((k, rest)) => (k.trim(), Some(rest.trim_end_matches(')').trim().to_string())),
        None => (head, None),
    };
    let (applicability, place) = match (kind, place) {
        ("fix", None) => (Applicability::Safe, None),
        ("fix", Some(p)) => match p.split_once(',') {
            Some((a, rest)) if a.trim() == "review" => (Applicability::Review, Some(rest.trim().to_string())),
            None if p == "review" => (Applicability::Review, None),
            _ => (Applicability::Safe, Some(p)),
        },
        ("option", p) => (Applicability::Option, p),
        _ => return None,
    };
    Some(SpecFix { applicability, place, text: text.trim().to_string() })
}

fn expectations(lines: &[String]) -> Vec<Expected> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let (_, comment) = split_comment(&lines[i]);
        if let Some(c) = comment
            && let Some(rest) = c.strip_prefix("❌")
        {
            let code = rest.trim().split(':').next().unwrap_or("").trim().to_string();
            let mut fixes = Vec::new();
            let mut j = i + 1;
            while j < lines.len() {
                let t = lines[j].trim_start();
                let Some(cont) = t.strip_prefix("//") else { break };
                if let Some(f) = parse_fix_line(cont.trim()) {
                    fixes.push(f);
                }
                j += 1;
            }
            let structural = split_comment(&lines[i]).0.trim().chars().all(|c| "{}()[]; ".contains(c));
            out.push(Expected { line: i, code, fixes, extra_lines: j - i - 1, structural });
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

fn doc_only_reason(b: &Block) -> Option<&'static str> {
    let first = b.lines.iter().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("");
    let all = b.lines.join("\n");
    if first.starts_with("dat ") {
        return Some("shell commands");
    }
    if first.starts_with("FAIL ") || first.starts_with("error[") || first == "{" {
        return Some("output sample");
    }
    if first.split(':').next().is_some_and(|w| w.ends_with("Error") && !w.contains(' ') && !w.contains('(')) && !all.contains(';') {
        return Some("output sample");
    }
    if b.lines.iter().any(|l| l.trim_end().ends_with('/') && !l.contains("//")) {
        return Some("folder listing");
    }
    if !all.contains(';') && !all.contains('{') && !all.contains('(') && all.contains(',') {
        return Some("keyword list");
    }
    if all.contains("{ ... }") {
        return Some("placeholder body");
    }
    None
}

/// Lines that start a top-level declaration.
fn is_item_start(line: &str) -> bool {
    ["function ", "export ", "test \"", "struct ", "enum ", "interface ", "type ", "import "].iter().any(|k| line.starts_with(k))
}

/// How much a line changes the nesting of `{`, or of every bracket.
fn brace_delta(code: &str) -> i32 {
    nesting(code, false)
}

fn bracket_delta(code: &str) -> i32 {
    nesting(code, true)
}

fn nesting(code: &str, all: bool) -> i32 {
    let mut d = 0;
    let mut in_str: Option<char> = None;
    let mut esc = false;
    for c in code.chars() {
        if let Some(q) = in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == q {
                in_str = None;
            }
            continue;
        }
        match c {
            '"' | '`' => in_str = Some(c),
            '{' => d += 1,
            '}' => d -= 1,
            '(' | '[' if all => d += 1,
            ')' | ']' if all => d -= 1,
            _ => {}
        }
    }
    d
}

fn is_literal(s: &str) -> bool {
    let s = s.trim();
    s == "true" || s == "false" || (s.starts_with('"') && s.ends_with('"') && s.len() >= 2) || (s.parse::<i64>().is_ok())
}

/// One generated program, with a map from its lines back to the block's.
struct CaseSource {
    text: String,
    /// For each line of `text`, the block line it came from.
    origin: Vec<Option<usize>>,
    /// Block lines rewritten as `const __eN = <expr>;`: the prefix added.
    wrapped: BTreeMap<usize, String>,
    /// Expected stdout, when every `print` in the case states its output.
    stdout: Option<String>,
}

/// Builds a program from block lines, skipping `drop`ped lines. `prelude`
/// statements start the generated `main`, for names the block uses but
/// doesn't declare.
fn build_case(lines: &[String], drop: &[bool], keep_comments: bool, prelude: &[String]) -> CaseSource {
    let mut items: Vec<(String, Option<usize>)> = Vec::new();
    let mut body: Vec<(String, Option<usize>)> = prelude.iter().map(|p| (format!("  {p}"), None)).collect();
    let mut wrapped = BTreeMap::new();
    let mut prints_all_stated = true;
    let mut has_print = false;
    let mut stdout = String::new();
    let mut depth = 0;
    let mut nest = 0;
    let mut in_item = false;
    let mut counter = 0;
    for (i, raw) in lines.iter().enumerate() {
        if drop[i] {
            continue;
        }
        let (code, comment) = split_comment(raw);
        let line = if keep_comments { raw.clone() } else { code.trim_end().to_string() };
        if depth == 0 && is_item_start(raw) {
            in_item = true;
        }
        let d = brace_delta(code);
        if in_item {
            if code.contains("print(") {
                prints_all_stated = false;
                has_print = true;
            }
            items.push((line, Some(i)));
            depth += d;
            if depth <= 0 {
                depth = 0;
                in_item = false;
            }
            continue;
        }
        let trimmed = code.trim();
        let at_top = depth == 0;
        let in_expr = nest > 0;
        depth += d;
        let bd = bracket_delta(code);
        nest += bd;
        if trimmed.contains("print(") {
            has_print = true;
            match comment {
                Some(c) if !c.starts_with("prints") && !c.contains(" then ") && !c.starts_with('❌') && !c.starts_with('✅') => {
                    stdout.push_str(c);
                    stdout.push('\n');
                }
                _ => prints_all_stated = false,
            }
        }
        // A bare expression line: check its value, or at least that it compiles.
        let first_word = trimmed.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
        let bare = at_top
            && !in_expr
            && bd == 0
            && !trimmed.is_empty()
            && !trimmed.ends_with(';')
            && !trimmed.ends_with('{')
            && !trimmed.starts_with('}')
            && !["if", "for", "while", "try", "else", "return", "let", "const", "throw", "break", "continue", "catch", "match"]
                .contains(&first_word);
        if bare {
            counter += 1;
            let value = comment.map(|c| c.split(',').next().unwrap_or("").trim()).filter(|c| is_literal(c));
            let text = match value {
                Some(v) => format!("  assert(({trimmed}) == ({v}));"),
                None => {
                    let prefix = format!("const __e{counter} = ");
                    wrapped.insert(i, prefix.clone());
                    format!("  {prefix}{trimmed};")
                }
            };
            body.push((text, Some(i)));
            continue;
        }
        if let Some(c) = comment
            && let Some(rest) = c.strip_prefix("throws ")
            && !rest.contains(" if ")
            && at_top
            && trimmed.ends_with(';')
        {
            let (ty, msg) = match rest.split_once(':') {
                Some((t, m)) => (t.trim(), Some(m.trim())),
                None => (rest.trim(), None),
            };
            let check = msg.map(|m| format!(" assert(e.message == {m:?});")).unwrap_or_default();
            body.push((format!("  try {{ {trimmed} fail(\"expected a {ty}\"); }} catch (e: {ty}) {{{check} }}"), Some(i)));
            continue;
        }
        body.push((format!("  {line}"), Some(i)));
    }
    let mut text = String::new();
    let mut origin = Vec::new();
    for (l, o) in &items {
        text.push_str(l);
        text.push('\n');
        origin.push(*o);
    }
    let has_main = items.iter().any(|(l, _)| l.starts_with("function main("));
    if body.iter().any(|(l, o)| o.is_some() && !l.trim().is_empty()) {
        let name = if has_main { "__fragment" } else { "main" };
        text.push_str(&format!("function {name}(): void {{\n"));
        origin.push(None);
        for (l, o) in &body {
            text.push_str(l);
            text.push('\n');
            origin.push(*o);
        }
        text.push_str("}\n");
        origin.push(None);
    }
    let stdout = if has_print && prints_all_stated {
        Some(stdout)
    } else if has_print {
        None
    } else {
        Some(String::new())
    };
    CaseSource { text, origin, wrapped, stdout }
}

struct Case {
    id: String,
    status: Status,
}

fn compile(text: &str) -> (SourceMap, Vec<Diagnostic>, bool) {
    let mut sources = SourceMap::new();
    let c = dat_check::compile(&mut sources, "case.dat", text.to_string(), dat_check::Options::default());
    let has_main = c.program.as_ref().is_some_and(|p| p.main.is_some());
    (sources, c.diags, has_main)
}

/// Parses and checks a block's text as it stands; a panic is a bug
/// whatever the block contains (criterion 1).
fn never_panics(text: &str) -> Result<(), String> {
    let text = text.to_string();
    std::panic::catch_unwind(move || {
        compile(&text);
        compile(&format!("function main(): void {{\n{text}\n}}\n"));
    })
    .map_err(|e| e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default())
}

fn unsupported_reason(diags: &[Diagnostic]) -> Option<String> {
    let mut reasons: Vec<String> = diags.iter().filter(|d| d.code == "E0002").map(|d| d.message.clone()).collect();
    reasons.dedup();
    if !reasons.is_empty() {
        return Some(reasons.join("; "));
    }
    let undefined: Vec<String> =
        diags.iter().filter(|d| d.code == "E1401").map(|d| d.message.trim_end_matches(" is not defined").to_string()).collect();
    if !undefined.is_empty() {
        return Some(format!("needs a prelude for {}", undefined.join(", ")));
    }
    None
}

/// Runs a clean case in both modes.
fn run_case(opts: &Opts, id: &str, src: &CaseSource) -> Status {
    let dir = opts.root.join("target/conformance-tmp/spec");
    std::fs::create_dir_all(&dir).unwrap();
    let file = format!("{}.dat", id.replace([':', '#', '@', '.'], "_"));
    std::fs::write(dir.join(&file), &src.text).unwrap();
    let interp = dat(&dir, &["run", &file], &[]);
    let want = |o: &Outcome| -> Option<String> {
        if o.status != 0 || !o.stderr.is_empty() {
            return Some(format!("exit {}: {}{}", o.status, o.stderr.lines().next().unwrap_or(""), o.stdout.lines().next().unwrap_or("")));
        }
        match &src.stdout {
            Some(expected) if *expected != o.stdout => Some(format!("stdout {:?}, expected {:?}", o.stdout, expected)),
            _ => None,
        }
    };
    if let Some(e) = want(&interp) {
        return Status::Fail(format!("interpreter: {e}"));
    }
    if opts.interp_only {
        return Status::Pass;
    }
    let compiled = dat(&dir, &["run", "--compiled", &file], &[]);
    if compiled != interp {
        return Status::Fail(format!("modes differ: interpreter {interp:?}, compiled {compiled:?}"));
    }
    Status::Pass
}

/// A ❌ case: exactly the expected diagnostic, on its line, with the spec's fixes.
fn check_error_case(block: &Block, e: &Expected, src: &CaseSource) -> Status {
    let (sources, diags, _) = compile(&src.text);
    let blocking: Vec<Diagnostic> =
        diags.iter().filter(|d| d.code == "E0002" || (d.code == "E1401" && e.code != "E1401")).cloned().collect();
    if let Some(r) = unsupported_reason(&blocking) {
        return Status::Unsupported(r);
    }
    let file = sources.files().next().unwrap().1;
    let lines: Vec<String> = diags
        .iter()
        .map(|d| {
            let (l, _) = file.line_col(d.span.start);
            let at = src.origin.get(l as usize - 1).copied().flatten().map(|o| block.line + 1 + o);
            format!("{} at {} ({})", d.code, at.map_or("?".into(), |a| a.to_string()), d.message)
        })
        .collect();
    if diags.len() != 1 {
        return Status::Fail(format!("expected only {} on line {}, got: {}", e.code, block.line + 1 + e.line, lines.join("; ")));
    }
    let d = &diags[0];
    let (l, _) = file.line_col(d.span.start);
    if d.code != e.code || src.origin.get(l as usize - 1).copied().flatten() != Some(e.line) {
        return Status::Fail(format!("expected {} on line {}, got {}", e.code, block.line + 1 + e.line, lines[0]));
    }
    if d.fixes.len() != e.fixes.len() {
        return Status::Fail(format!("expected {} fixes, got {}", e.fixes.len(), d.fixes.len()));
    }
    for (spec, ours) in e.fixes.iter().zip(&d.fixes) {
        if spec.applicability != ours.applicability {
            return Status::Fail(format!(
                "fix `{}` should be {}, but it's {}",
                spec.text,
                spec.applicability.as_str(),
                ours.applicability.as_str()
            ));
        }
        let preview = dat_diag::fixed_line_preview(&sources, d.file, ours);
        let mut shown: Vec<String> = preview.iter().map(|(_, t)| split_comment(t).0.trim().to_string()).collect();
        if let (Some(prefix), [one]) = (src.wrapped.get(&e.line), shown.as_mut_slice()) {
            let inner = one.strip_prefix(prefix.as_str()).and_then(|s| s.strip_suffix(';')).map(str::to_string);
            if let Some(inner) = inner {
                *one = inner;
            }
        }
        let got = shown.join(" ");
        if got != spec.text {
            return Status::Fail(format!("fix should read `{}`, but reads `{got}`", spec.text));
        }
        // Applying a safe fix removes the root cause without new errors.
        if ours.applicability == Applicability::Safe {
            let edits: Vec<&dat_diag::Edit> = ours.edits.iter().collect();
            if let Some(fixed) = dat_diag::apply_edits(&file.text, &edits) {
                let (_, after, _) = compile(&fixed);
                if !after.is_empty() {
                    return Status::Fail(format!("after the safe fix: {}", after.iter().map(|d| d.code).collect::<Vec<_>>().join(", ")));
                }
            }
        }
    }
    Status::Pass
}

fn block_cases(opts: &Opts, block: &Block, prelude: &[String]) -> Vec<Case> {
    let base = format!("{}:{}", block.file, block.line);
    if let Some(reason) = doc_only_reason(block) {
        return vec![Case { id: base, status: Status::DocOnly(reason.into()) }];
    }
    if block.lines.first().is_some_and(|l| l.trim_start().starts_with("// ") && l.trim_end().ends_with(".dat") || l.contains(".dat:")) {
        return vec![Case { id: base, status: Status::Unsupported("not supported yet: multi-file examples, which need imports".into()) }];
    }
    let exps = expectations(&block.lines);
    let mut cases = Vec::new();
    // The positive case: every ❌ line removed. A block whose ❌ line is
    // structure, such as a function's closing `}`, has none.
    let mut drop = vec![false; block.lines.len()];
    for e in &exps {
        for k in 0..=e.extra_lines {
            drop[e.line + k] = true;
        }
    }
    let src = build_case(&block.lines, &drop, true, prelude);
    let (_, diags, has_main) = compile(&src.text);
    let status = if exps.iter().any(|e| e.structural) {
        Status::DocOnly("its ❌ line is structure, so only the ❌ case runs".into())
    } else if let Some(r) = unsupported_reason(&diags) {
        Status::Unsupported(r)
    } else if !diags.is_empty() {
        let d = &diags[0];
        Status::Fail(format!(
            "{}: {}{}",
            d.code,
            d.message,
            if diags.len() > 1 { format!(" (and {} more)", diags.len() - 1) } else { String::new() }
        ))
    } else if has_main {
        run_case(opts, &format!("{base}#ok"), &src)
    } else {
        Status::Pass
    };
    cases.push(Case { id: format!("{base}#ok"), status });
    for e in &exps {
        let mut drop = vec![false; block.lines.len()];
        for other in exps.iter().filter(|o| o.line != e.line) {
            for k in usize::from(other.structural)..=other.extra_lines {
                drop[other.line + k] = true;
            }
        }
        for k in 1..=e.extra_lines {
            drop[e.line + k] = true;
        }
        let src = build_case(&block.lines, &drop, false, prelude);
        let status = check_error_case(block, e, &src);
        cases.push(Case { id: format!("{base}#{}@{}", e.code, block.line + 1 + e.line), status });
    }
    cases
}

/// `conformance/preludes.txt`: `<block id> <statement>` lines. Each block's
/// statements, in order, start its generated `main`.
fn read_preludes(path: &Path) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for l in std::fs::read_to_string(path).unwrap_or_default().lines() {
        if l.trim().is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some((id, stmt)) = l.split_once(' ') {
            out.entry(id.to_string()).or_default().push(stmt.trim().to_string());
        }
    }
    out
}

fn read_list(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| match l.split_once(' ') {
            Some((id, why)) => (id.to_string(), why.trim().to_string()),
            None => (l.trim().to_string(), String::new()),
        })
        .collect()
}

pub fn run(opts: &Opts) -> bool {
    let blocks = extract(&opts.root);
    let ratchet_path = opts.root.join("conformance/spec-pass.txt");
    let xfail_path = opts.root.join("conformance/spec-xfail.txt");
    let ratchet = read_list(&ratchet_path);
    let xfail = read_list(&xfail_path);
    let preludes = read_preludes(&opts.root.join("conformance/preludes.txt"));
    let mut cases = Vec::new();
    let mut panics = 0;
    std::panic::set_hook(Box::new(|_| {}));
    for b in &blocks {
        let id = format!("{}:{}", b.file, b.line);
        if let Err(msg) = never_panics(&b.lines.join("\n")) {
            println!("FAIL  {id}: the compiler panicked: {msg}");
            panics += 1;
        }
        cases.extend(block_cases(opts, b, preludes.get(&id).map_or(&[], Vec::as_slice)));
    }
    for c in &mut cases {
        if let Some(why) = xfail.get(&c.id) {
            c.status = match &c.status {
                Status::Fail(_) => Status::XFail(why.clone()),
                Status::Pass => Status::Fail(format!("xpass: listed in spec-xfail.txt ({why}) but passes; remove it")),
                other => other.clone(),
            };
        }
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for c in &cases {
        *counts.entry(c.status.label()).or_default() += 1;
    }
    let _ = std::panic::take_hook();
    let mut ok = panics == 0;
    let mut report = String::new();
    for c in &cases {
        let detail = match &c.status {
            Status::Pass => String::new(),
            Status::Fail(s) | Status::Unsupported(s) | Status::DocOnly(s) | Status::XFail(s) => format!("  {s}"),
        };
        let _ = writeln!(report, "{:<11} {}{detail}", c.status.label(), c.id);
        if matches!(c.status, Status::Fail(_)) {
            ok = false;
        }
    }
    let passing: Vec<&str> = cases.iter().filter(|c| c.status == Status::Pass).map(|c| c.id.as_str()).collect();
    for id in ratchet.keys() {
        if !passing.contains(&id.as_str()) {
            let _ = writeln!(report, "REGRESSION  {id} was passing (conformance/spec-pass.txt)");
            ok = false;
        }
    }
    let new: Vec<&&str> = passing.iter().filter(|id| !ratchet.contains_key(**id)).collect();
    if opts.verbose || !ok {
        print!("{report}");
    } else {
        for line in report.lines().filter(|l| l.starts_with("fail") || l.starts_with("REGRESSION")) {
            println!("{line}");
        }
    }
    let classified = blocks.len();
    println!(
        "spec: {classified} of {} blocks classified, {panics} compiler panics; {} cases: {}",
        blocks.len(),
        cases.len(),
        counts.iter().map(|(k, v)| format!("{v} {k}")).collect::<Vec<_>>().join(", ")
    );
    if opts.bless {
        let mut text = String::from(
            "# Spec cases that pass in both modes. The list may only grow:\n# `dat-conformance spec --bless` adds new passes.\n",
        );
        for id in &passing {
            text.push_str(id);
            text.push('\n');
        }
        std::fs::write(&ratchet_path, text).unwrap();
        println!("spec: wrote {} passing cases to {}", passing.len(), ratchet_path.display());
    } else if !new.is_empty() {
        println!("spec: {} new passing cases; run with --bless to add them to the ratchet", new.len());
    }
    ok
}
