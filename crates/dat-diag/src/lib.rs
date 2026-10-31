//! The diagnostic record every dat tool shares, and its two renderings.
//!
//! The record is the schema in `reference/21-command-line.md`: a stable code,
//! a byte span, a message, labels and `fixes[]` whose edits carry the exact
//! `before` text. The terminal form is the one people and agents read; the
//! JSON form is for `dat fix`, the LSP and test harnesses.

#![forbid(unsafe_code)]

mod json;

pub use json::{Json, json_string};

use std::fmt::Write as _;

/// A half-open byte range into one source file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const fn new(start: u32, end: u32) -> Span {
        Span { start, end }
    }

    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }

    pub fn len(self) -> u32 {
        self.end - self.start
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub u32);

/// One source file and the index needed to turn byte offsets into lines.
#[derive(Clone, Debug)]
pub struct SourceFile {
    /// The path as the user gave it; diagnostics and traces print it unchanged.
    pub path: String,
    pub text: String,
    line_starts: Vec<u32>,
}

impl SourceFile {
    pub fn new(path: impl Into<String>, text: impl Into<String>) -> SourceFile {
        let text = text.into();
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i as u32 + 1);
            }
        }
        SourceFile { path: path.into(), text, line_starts }
    }

    /// 1-based line and column of a byte offset. Columns count Unicode scalar values.
    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let start = self.line_starts[line] as usize;
        let end = (offset as usize).min(self.text.len());
        let col = self.text[start..end].chars().count() as u32 + 1;
        (line as u32 + 1, col)
    }

    /// The text of a 1-based line, without its line break.
    pub fn line_text(&self, line: u32) -> &str {
        let i = (line - 1) as usize;
        let start = self.line_starts[i] as usize;
        let end = self.line_starts.get(i + 1).map_or(self.text.len(), |&e| e as usize);
        self.text[start..end].trim_end_matches(['\n', '\r'])
    }

    pub fn line_count(&self) -> u32 {
        self.line_starts.len() as u32
    }

    pub fn slice(&self, span: Span) -> &str {
        &self.text[span.start as usize..span.end as usize]
    }
}

/// All files of one run, addressed by `FileId`.
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> SourceMap {
        SourceMap::default()
    }

    pub fn add(&mut self, file: SourceFile) -> FileId {
        self.files.push(file);
        FileId(self.files.len() as u32 - 1)
    }

    pub fn get(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    pub fn files(&self) -> impl Iterator<Item = (FileId, &SourceFile)> {
        self.files.iter().enumerate().map(|(i, f)| (FileId(i as u32), f))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// How sure the compiler is that a fix keeps the author's intent.
/// `dat fix` applies only `Safe` fixes. See `reference/README.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applicability {
    /// `fix:` — applied by `dat fix`.
    Safe,
    /// `fix (review):` — changes what the code means; shown, never applied.
    Review,
    /// `option:` — one of several fixes; the right one depends on intent.
    Option,
}

impl Applicability {
    pub fn as_str(self) -> &'static str {
        match self {
            Applicability::Safe => "safe",
            Applicability::Review => "review",
            Applicability::Option => "option",
        }
    }
}

/// One text replacement. `before` is the exact text being replaced, so tools
/// can detect stale offsets and agents can apply it as a string replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub file: FileId,
    pub span: Span,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug)]
pub struct Fix {
    pub title: String,
    pub applicability: Applicability,
    pub is_preferred: bool,
    pub edits: Vec<Edit>,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub file: FileId,
    pub span: Span,
    /// Shown after the carets under the primary span.
    pub label: Option<String>,
    pub fixes: Vec<Fix>,
}

impl Diagnostic {
    pub fn error(code: &'static str, file: FileId, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic { code, severity: Severity::Error, message: message.into(), file, span, label: None, fixes: Vec::new() }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Diagnostic {
        self.label = Some(label.into());
        self
    }

    /// Adds a fix. The first fix added is the preferred one.
    pub fn with_fix(mut self, title: impl Into<String>, applicability: Applicability, edits: Vec<Edit>) -> Diagnostic {
        let is_preferred = self.fixes.is_empty();
        self.fixes.push(Fix { title: title.into(), applicability, is_preferred, edits });
        self
    }

    pub fn is_fixable(&self) -> bool {
        self.fixes.iter().any(|f| f.applicability == Applicability::Safe)
    }
}

/// Builds an edit that replaces `span` in `file` with `after`.
pub fn edit(sources: &SourceMap, file: FileId, span: Span, after: impl Into<String>) -> Edit {
    Edit { file, span, before: sources.get(file).slice(span).to_string(), after: after.into() }
}

/// Applies edits to one file's text. Edits must not overlap. Returns `None` if
/// an edit's `before` text doesn't match, which means the offsets are stale.
pub fn apply_edits(text: &str, edits: &[&Edit]) -> Option<String> {
    let mut sorted: Vec<&Edit> = edits.to_vec();
    sorted.sort_by_key(|e| (e.span.start, e.span.end));
    let mut out = String::with_capacity(text.len());
    let mut pos = 0usize;
    for e in sorted {
        let (s, t) = (e.span.start as usize, e.span.end as usize);
        if s < pos || t > text.len() || text.get(s..t)? != e.before {
            return None;
        }
        out.push_str(&text[pos..s]);
        out.push_str(&e.after);
        pos = t;
    }
    out.push_str(&text[pos..]);
    Some(out)
}

/// Applies the preferred safe fix of every diagnostic in `file`, as `dat fix`
/// does. A fix whose edits overlap an earlier one waits for the next round.
/// Returns `None` when there is nothing to apply.
pub fn apply_safe_fixes(text: &str, file: FileId, diags: &[Diagnostic]) -> Option<String> {
    let mut chosen: Vec<&Edit> = Vec::new();
    for d in diags.iter().filter(|d| d.file == file) {
        let Some(fix) = d.fixes.iter().find(|f| f.applicability == Applicability::Safe) else { continue };
        let overlaps = fix.edits.iter().any(|e| {
            chosen.iter().any(|c| e.span.start < c.span.end.max(c.span.start + 1) && c.span.start < e.span.end.max(e.span.start + 1))
        });
        if overlaps || fix.edits.iter().any(|e| e.file != file) {
            continue;
        }
        chosen.extend(&fix.edits);
    }
    if chosen.is_empty() {
        return None;
    }
    apply_edits(text, &chosen)
}

/// Sorts diagnostics by file, position and code: the order every renderer uses.
pub fn sort(diags: &mut [Diagnostic]) {
    diags.sort_by(|a, b| (a.file, a.span.start, a.code, a.span.end).cmp(&(b.file, b.span.start, b.code, b.span.end)));
}

/// At most this many diagnostics are rendered in full.
pub const MAX_SHOWN: usize = 50;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub errors: usize,
    pub warnings: usize,
    pub fixable: usize,
}

pub fn summarize(diags: &[Diagnostic]) -> Summary {
    let mut s = Summary::default();
    for d in diags {
        match d.severity {
            Severity::Error => s.errors += 1,
            Severity::Warning => s.warnings += 1,
        }
        if d.is_fixable() {
            s.fixable += 1;
        }
    }
    s
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

/// The summary line that ends the terminal output, such as
/// `1 error, 1 fixable with dat fix`. Empty when there is nothing to report.
pub fn summary_line(s: Summary) -> String {
    let mut parts = Vec::new();
    if s.errors > 0 {
        parts.push(plural(s.errors, "error"));
    }
    if s.warnings > 0 {
        parts.push(plural(s.warnings, "warning"));
    }
    if parts.is_empty() {
        return String::new();
    }
    if s.fixable > 0 {
        parts.push(format!("{} fixable with dat fix", s.fixable));
    }
    parts.join(", ")
}

/// Renders one diagnostic in the compact terminal layout, without the summary.
pub fn render(sources: &SourceMap, d: &Diagnostic) -> String {
    let file = sources.get(d.file);
    let (line, col) = file.line_col(d.span.start);
    let fixed_lines: Vec<(&Fix, Vec<(u32, String)>)> = d.fixes.iter().map(|f| (f, fixed_line_preview(sources, d.file, f))).collect();
    let width = fixed_lines.iter().flat_map(|(_, ls)| ls.iter().map(|(n, _)| *n)).chain([line]).max().unwrap_or(line).to_string().len();
    let pad = " ".repeat(width);
    let mut out = String::new();
    let _ = writeln!(out, "{}[{}]: {}", d.severity.as_str(), d.code, d.message);
    let _ = writeln!(out, "{pad}--> {}:{}:{}", file.path, line, col);
    let text = file.line_text(line);
    let _ = writeln!(out, "{line:>width$} | {text}");
    // Carets: copy tabs so they line up, and stop at the end of the first line.
    let line_start = d.span.start as usize - text_prefix_len(text, col);
    let caret_pad: String = file.text[line_start..d.span.start as usize].chars().map(|c| if c == '\t' { '\t' } else { ' ' }).collect();
    let span_end = (d.span.end as usize).min(line_start + text.len()).max(d.span.start as usize);
    let carets = file.text[d.span.start as usize..span_end].chars().count().max(1);
    let _ = write!(out, "{pad} | {caret_pad}{}", "^".repeat(carets));
    if let Some(label) = &d.label {
        let _ = write!(out, " {label}");
    }
    out.push('\n');
    for (fix, lines) in &fixed_lines {
        let head = match fix.applicability {
            Applicability::Safe => "fix:",
            Applicability::Review => "fix (review):",
            Applicability::Option => "option:",
        };
        let _ = writeln!(out, "{head} {}", fix.title);
        for (n, t) in lines {
            let _ = writeln!(out, "{n:>width$} | {t}");
        }
    }
    out
}

fn text_prefix_len(text: &str, col: u32) -> usize {
    text.chars().take(col as usize - 1).map(char::len_utf8).sum()
}

/// The lines of `file` as they read after applying `fix`, limited to the lines its edits touch.
pub fn fixed_line_preview(sources: &SourceMap, file: FileId, fix: &Fix) -> Vec<(u32, String)> {
    let edits: Vec<&Edit> = fix.edits.iter().filter(|e| e.file == file).collect();
    let src = sources.get(file);
    let Some(new_text) = apply_edits(&src.text, &edits) else {
        return Vec::new();
    };
    let new_file = SourceFile::new(src.path.clone(), new_text);
    let mut sorted = edits.clone();
    sorted.sort_by_key(|e| e.span.start);
    let mut delta: i64 = 0;
    let mut lines: Vec<u32> = Vec::new();
    for e in sorted {
        let start = (e.span.start as i64 + delta) as u32;
        let end = start + e.after.len() as u32;
        delta += e.after.len() as i64 - e.span.len() as i64;
        if e.after.trim().is_empty() {
            // Deleting whole lines shows nothing; a deletion inside a line
            // shows what's left of it.
            if e.before.contains('\n') || new_file.line_text(new_file.line_col(start).0).trim().is_empty() {
                continue;
            }
            let (l, _) = new_file.line_col(start);
            if !lines.contains(&l) {
                lines.push(l);
            }
            continue;
        }
        // Skip the line breaks at the edges of an inserted block.
        let lead = e.after.len() - e.after.trim_start().len();
        let trail = e.after.len() - e.after.trim_end().len();
        let (a, _) = new_file.line_col(start + lead as u32);
        let (b, _) = new_file.line_col(end - trail as u32);
        for l in a..=b {
            if !lines.contains(&l) {
                lines.push(l);
            }
        }
    }
    lines.sort();
    lines.into_iter().map(|l| (l, new_file.line_text(l).to_string())).collect()
}

/// Renders a whole run for the terminal: each diagnostic in order, capped at
/// `MAX_SHOWN`, then the summary line.
pub fn render_all(sources: &SourceMap, diags: &[Diagnostic]) -> String {
    let mut out = String::new();
    for (i, d) in diags.iter().take(MAX_SHOWN).enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&render(sources, d));
    }
    if diags.len() > MAX_SHOWN {
        let _ = writeln!(out, "\nand {} more", diags.len() - MAX_SHOWN);
    }
    let summary = summary_line(summarize(diags));
    if !summary.is_empty() {
        out.push_str(&summary);
        out.push('\n');
    }
    out
}

/// Renders a whole run as the one JSON document of `--json`.
pub fn render_json(sources: &SourceMap, diags: &[Diagnostic]) -> String {
    let s = summarize(diags);
    let doc = Json::obj([
        ("schemaVersion", Json::Int(1)),
        ("diagnostics", Json::Arr(diags.iter().map(|d| diagnostic_json(sources, d)).collect())),
        (
            "summary",
            Json::obj([
                ("errors", Json::Int(s.errors as i64)),
                ("warnings", Json::Int(s.warnings as i64)),
                ("fixable", Json::Int(s.fixable as i64)),
            ]),
        ),
    ]);
    let mut out = doc.pretty();
    out.push('\n');
    out
}

fn diagnostic_json(sources: &SourceMap, d: &Diagnostic) -> Json {
    let file = sources.get(d.file);
    let (line, col) = file.line_col(d.span.start);
    let (end_line, end_col) = file.line_col(d.span.end);
    let labels = d.label.iter().map(|l| Json::obj([("message", Json::str(l))])).collect();
    let fixes = d
        .fixes
        .iter()
        .map(|f| {
            Json::obj([
                ("title", Json::str(&f.title)),
                ("applicability", Json::str(f.applicability.as_str())),
                ("isPreferred", Json::Bool(f.is_preferred)),
                (
                    "edits",
                    Json::Arr(
                        f.edits
                            .iter()
                            .map(|e| {
                                Json::obj([
                                    ("file", Json::str(&sources.get(e.file).path)),
                                    ("byteStart", Json::Int(e.span.start as i64)),
                                    ("byteEnd", Json::Int(e.span.end as i64)),
                                    ("before", Json::str(&e.before)),
                                    ("after", Json::str(&e.after)),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ])
        })
        .collect();
    Json::obj([
        ("code", Json::str(d.code)),
        ("severity", Json::str(d.severity.as_str())),
        ("message", Json::str(&d.message)),
        ("file", Json::str(&file.path)),
        (
            "span",
            Json::obj([
                ("byteStart", Json::Int(d.span.start as i64)),
                ("byteEnd", Json::Int(d.span.end as i64)),
                ("line", Json::Int(line as i64)),
                ("col", Json::Int(col as i64)),
                ("endLine", Json::Int(end_line as i64)),
                ("endCol", Json::Int(end_col as i64)),
            ]),
        ),
        ("labels", Json::Arr(labels)),
        ("fixes", Json::Arr(fixes)),
        ("rendered", Json::str(&render(sources, d))),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_counts_scalar_values() {
        let f = SourceFile::new("a.dat", "ab\nhé x\n");
        assert_eq!(f.line_col(0), (1, 1));
        assert_eq!(f.line_col(3), (2, 1));
        assert_eq!(f.line_col(6), (2, 3)); // after the two-byte é
        assert_eq!(f.line_text(2), "hé x");
    }

    #[test]
    fn apply_edits_checks_before_text() {
        let e = Edit { file: FileId(0), span: Span::new(0, 1), before: "a".into(), after: "x".into() };
        assert_eq!(apply_edits("abc", &[&e]).as_deref(), Some("xbc"));
        let stale = Edit { before: "b".into(), ..e };
        assert_eq!(apply_edits("abc", &[&stale]), None);
    }

    #[test]
    fn summary_line_format() {
        assert_eq!(summary_line(Summary { errors: 1, warnings: 0, fixable: 1 }), "1 error, 1 fixable with dat fix");
        assert_eq!(summary_line(Summary { errors: 2, warnings: 0, fixable: 0 }), "2 errors");
        assert_eq!(summary_line(Summary::default()), "");
    }
}
