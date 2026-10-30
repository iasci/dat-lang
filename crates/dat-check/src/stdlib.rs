//! The standard library's interfaces, built into the compiler so nothing
//! is read or checked at startup. Each module is a table of its members:
//! the checker resolves imports, `ns.member` and calls through the table,
//! with no code for any one module, so a new module is a new entry here
//! and its operations in dat-rt. A member a later milestone implements is
//! listed by name only, so a use without its import gets the import as its
//! fix.

use crate::body::closest;
use dat_ir::Builtin::{self, *};
use dat_ir::Type::{self, Int};

#[derive(Debug)]
pub(crate) struct StdModule {
    /// The name after `@dat/`.
    pub name: &'static str,
    pub members: &'static [Member],
}

#[derive(Debug)]
pub(crate) struct Member {
    pub name: &'static str,
    pub kind: Kind,
}

#[derive(Debug)]
pub(crate) enum Kind {
    Float(f64),
    Func(Func),
    /// Implemented in a later milestone.
    Later,
}

#[derive(Debug)]
pub(crate) struct Func {
    /// Its forms, tried in order: a call uses the first whose parameters
    /// take its arguments, where a `float` parameter also takes an `int`.
    pub forms: &'static [Form],
    /// It takes one or more arguments and combines them from left to right
    /// with its two-parameter forms: `max(a, b, c)` is `max(max(a, b), c)`.
    pub variadic: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Form {
    /// The operation, applied to the arguments.
    Op(Builtin),
    /// One argument of this type, returned as it is: `floor` of an `int`.
    Same(&'static Type),
}

use Form::{Op, Same};

pub(crate) const MODULES: &[StdModule] = &[
    StdModule {
        name: "math",
        members: &[
            constant("PI", std::f64::consts::PI),
            constant("E", std::f64::consts::E),
            func("abs", &[Op(IntAbs), Op(FloatAbs)]),
            variadic("min", &[Op(IntMin), Op(FloatMin)]),
            variadic("max", &[Op(IntMax), Op(FloatMax)]),
            func("floor", &[Same(&Int), Op(FloatFloor)]),
            func("ceil", &[Same(&Int), Op(FloatCeil)]),
            func("round", &[Same(&Int), Op(FloatRound)]),
            func("trunc", &[Same(&Int), Op(FloatToInt)]),
            func("sqrt", &[Op(IntSqrt), Op(FloatSqrt)]),
            func("pow", &[Op(IntPow), Op(FloatPow)]),
        ],
    },
    StdModule {
        name: "tasks",
        members: &[later("taskGroup"), later("TaskGroup"), later("Shared"), later("OnFailure"), later("StoppedError")],
    },
];

const fn constant(name: &'static str, value: f64) -> Member {
    Member { name, kind: Kind::Float(value) }
}

const fn func(name: &'static str, forms: &'static [Form]) -> Member {
    Member { name, kind: Kind::Func(Func { forms, variadic: false }) }
}

const fn variadic(name: &'static str, forms: &'static [Form]) -> Member {
    Member { name, kind: Kind::Func(Func { forms, variadic: true }) }
}

const fn later(name: &'static str) -> Member {
    Member { name, kind: Kind::Later }
}

/// The module an import path names: `"@dat/math"`.
pub(crate) fn by_path(path: &str) -> Option<&'static StdModule> {
    let name = path.strip_prefix("@dat/")?;
    MODULES.iter().find(|m| m.name == name)
}

/// The module `name` is most likely a misspelling of, as `maths`.
pub(crate) fn similar_module(name: &str) -> Option<&'static StdModule> {
    let name = closest(name.trim_end_matches('/'), MODULES.iter().map(|m| m.name))?;
    MODULES.iter().find(|m| m.name == name)
}

/// The module that exports `member`.
pub(crate) fn exporting(member: &str) -> Option<&'static StdModule> {
    MODULES.iter().find(|m| m.member(member).is_some())
}

impl StdModule {
    pub fn path(&self) -> String {
        format!("@dat/{}", self.name)
    }

    pub fn member(&self, name: &str) -> Option<&'static Member> {
        let members: &'static [Member] = self.members;
        members.iter().find(|m| m.name == name)
    }

    /// The member `name` is most likely a misspelling of.
    pub fn similar(&self, name: &str) -> Option<&'static str> {
        let members: &'static [Member] = self.members;
        crate::body::closest(name, members.iter().map(|m| m.name))
    }

    /// This milestone implements some of it, so it can be imported.
    pub fn supported(&self) -> bool {
        self.members.iter().any(|m| !matches!(m.kind, Kind::Later))
    }
}

impl Form {
    pub fn params(self) -> &'static [Type] {
        match self {
            Op(op) => op.sig().0,
            Same(t) => std::slice::from_ref(t),
        }
    }
}

impl Func {
    /// The parameter type for argument `i`.
    pub fn param(&self, form: Form, i: usize) -> Type {
        let params = form.params();
        if self.variadic { params[0] } else { params[i] }
    }

    /// The number of arguments it takes, from `.0` to `.1` (`usize::MAX`:
    /// no limit).
    pub fn arity(&self) -> (usize, usize) {
        if self.variadic { (1, usize::MAX) } else { (self.forms[0].params().len(), self.forms[0].params().len()) }
    }

    /// The operation that computes it exactly when its one argument is a
    /// division of two `int`s, as `floor(a / b)` (§03).
    pub fn on_int_division(&self) -> Option<Builtin> {
        if self.variadic {
            return None;
        }
        self.forms.iter().find_map(|f| match f {
            Op(op) => op.on_int_division(),
            Same(_) => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The checker relies on these: a function's forms take the same
    /// number of arguments, and a variadic one's combine two values of one
    /// type into another of that type.
    #[test]
    fn tables_are_consistent() {
        for (i, m) in MODULES.iter().enumerate() {
            assert!(MODULES[..i].iter().all(|o| o.name != m.name), "module {} is listed twice", m.name);
            for (j, x) in m.members.iter().enumerate() {
                assert!(m.members[..j].iter().all(|o| o.name != x.name), "{} is listed twice in {}", x.name, m.name);
                let Kind::Func(f) = &x.kind else { continue };
                assert!(!f.forms.is_empty(), "{} has no forms", x.name);
                for form in f.forms {
                    let params = form.params();
                    if f.variadic {
                        let Op(op) = form else { panic!("{}: a variadic form is an operation", x.name) };
                        assert_eq!(params.len(), 2, "{}", x.name);
                        assert!(params[0] == params[1] && op.sig().1 == params[0], "{}", x.name);
                    } else {
                        assert_eq!(params.len(), f.forms[0].params().len(), "{}", x.name);
                    }
                }
            }
        }
    }
}
