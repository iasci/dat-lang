//! Name resolution, type checking and lowering to the typed IR.
//!
//! One pass over each function body checks it and lowers it at once. Errors
//! give an expression the poison type, which absorbs further checks, so each
//! root cause is reported once. A program is produced only when there are no
//! errors.

#![forbid(unsafe_code)]

mod body;
pub mod codes;
mod module;
mod stdlib;

use dat_diag::{Diagnostic, FileId, SourceFile, SourceMap};

pub use module::check;

/// A type as the checker sees it: an IR type, or `Poison` for an
/// expression that already has an error, which absorbs every check so the
/// error is reported once. A compound type never holds `Poison`: a list of
/// poison is poison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ty {
    Int,
    Float,
    Bool,
    Str,
    Void,
    Never,
    Error(dat_ir::ErrorType),
    /// A list, set, map, tuple or function type, from the module's table.
    Of(dat_ir::TypeId),
    Poison,
}

impl Ty {
    pub(crate) fn ir(self) -> dat_ir::Type {
        match self {
            Ty::Int => dat_ir::Type::Int,
            Ty::Float => dat_ir::Type::Float,
            Ty::Bool => dat_ir::Type::Bool,
            Ty::Str => dat_ir::Type::Str,
            Ty::Void | Ty::Poison => dat_ir::Type::Void,
            Ty::Never => dat_ir::Type::Never,
            Ty::Error(e) => dat_ir::Type::Error(e),
            Ty::Of(id) => dat_ir::Type::Of(id),
        }
    }

    pub(crate) fn of(t: dat_ir::Type) -> Ty {
        match t {
            dat_ir::Type::Int => Ty::Int,
            dat_ir::Type::Float => Ty::Float,
            dat_ir::Type::Bool => Ty::Bool,
            dat_ir::Type::Str => Ty::Str,
            dat_ir::Type::Void => Ty::Void,
            dat_ir::Type::Never => Ty::Never,
            dat_ir::Type::Error(e) => Ty::Error(e),
            dat_ir::Type::Of(id) => Ty::Of(id),
        }
    }

    /// Has a `toString()` that templates and `print` can use.
    pub(crate) fn is_printable(self) -> bool {
        !matches!(self, Ty::Void | Ty::Never | Ty::Poison)
    }

    pub(crate) fn is_number(self) -> bool {
        matches!(self, Ty::Int | Ty::Float)
    }
}

/// The result of compiling one file.
pub struct Compiled {
    pub file: FileId,
    /// Sorted by position.
    pub diags: Vec<Diagnostic>,
    /// `Some` when there are no errors.
    pub program: Option<dat_ir::Program>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Require a `main` function, as `dat run` and `dat build` do.
    pub require_main: bool,
}

/// Parses and checks one file, adding it to `sources`.
pub fn compile(sources: &mut SourceMap, path: &str, text: String, opts: Options) -> Compiled {
    let file = sources.add(SourceFile::new(path, text));
    let parsed = dat_syntax::parse(file, &sources.get(file).text);
    let mut diags = parsed.diags;
    let program = check(sources, &parsed.ast, opts, &mut diags);
    dat_diag::sort(&mut diags);
    let has_errors = diags.iter().any(|d| d.severity == dat_diag::Severity::Error);
    Compiled { file, diags, program: if has_errors { None } else { program } }
}
