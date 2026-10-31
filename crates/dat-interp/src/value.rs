//! The interpreter's dynamically typed value, and the value traits that
//! let dat-rt's collections hold it.

use dat_rt::{DatError, DatHash, Equal, List, Map, Order, Placeholder, Set, Show, Str};
use std::cmp::Ordering;
use std::hash::Hasher;
use std::rc::Rc;

/// A value of any dat type. The checker proves which variant each
/// expression gives, so the accessors never see the wrong one. It stays
/// 16 bytes: every payload is one word.
#[derive(Clone, Debug, Default)]
pub(crate) enum Value {
    #[default]
    Void,
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(Str),
    Error(Rc<DatError>),
    List(List<Value>),
    Set(Set<Value>),
    Map(Map<Value, Value>),
    Tuple(Rc<Vec<Value>>),
    Func(Rc<Closure>),
}

const _: () = assert!(std::mem::size_of::<Value>() == 16);

/// A function value: the function, and an arrow function's copies of what
/// it captured, which each call puts in its capture locals.
#[derive(Debug)]
pub(crate) struct Closure {
    pub func: usize,
    pub captures: Box<[Value]>,
}

impl Value {
    #[inline]
    pub(crate) fn int(&self) -> i64 {
        match self {
            Value::Int(n) => *n,
            _ => unreachable!("type checker guarantees an int"),
        }
    }

    #[inline]
    pub(crate) fn float(&self) -> f64 {
        match self {
            Value::Float(x) => *x,
            _ => unreachable!("type checker guarantees a float"),
        }
    }

    #[inline]
    pub(crate) fn bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            _ => unreachable!("type checker guarantees a bool"),
        }
    }

    pub(crate) fn str(&self) -> &Str {
        match self {
            Value::Str(s) => s,
            _ => unreachable!("type checker guarantees a string"),
        }
    }

    pub(crate) fn error(&self) -> &Rc<DatError> {
        match self {
            Value::Error(e) => e,
            _ => unreachable!("type checker guarantees an error"),
        }
    }

    pub(crate) fn into_str(self) -> Str {
        match self {
            Value::Str(s) => s,
            _ => unreachable!("type checker guarantees a string"),
        }
    }

    pub(crate) fn into_error(self) -> Rc<DatError> {
        match self {
            Value::Error(e) => e,
            _ => unreachable!("type checker guarantees an error"),
        }
    }

    pub(crate) fn list(&self) -> &List<Value> {
        match self {
            Value::List(l) => l,
            _ => unreachable!("type checker guarantees a list"),
        }
    }

    pub(crate) fn list_mut(&mut self) -> &mut List<Value> {
        match self {
            Value::List(l) => l,
            _ => unreachable!("type checker guarantees a list"),
        }
    }

    pub(crate) fn set(&self) -> &Set<Value> {
        match self {
            Value::Set(s) => s,
            _ => unreachable!("type checker guarantees a set"),
        }
    }

    pub(crate) fn set_mut(&mut self) -> &mut Set<Value> {
        match self {
            Value::Set(s) => s,
            _ => unreachable!("type checker guarantees a set"),
        }
    }

    pub(crate) fn map(&self) -> &Map<Value, Value> {
        match self {
            Value::Map(m) => m,
            _ => unreachable!("type checker guarantees a map"),
        }
    }

    pub(crate) fn map_mut(&mut self) -> &mut Map<Value, Value> {
        match self {
            Value::Map(m) => m,
            _ => unreachable!("type checker guarantees a map"),
        }
    }

    pub(crate) fn tuple(&self) -> &[Value] {
        match self {
            Value::Tuple(t) => t,
            _ => unreachable!("type checker guarantees a tuple"),
        }
    }

    pub(crate) fn func(&self) -> &Rc<Closure> {
        match self {
            Value::Func(f) => f,
            _ => unreachable!("type checker guarantees a function"),
        }
    }
}

impl Show for Value {
    fn show(&self, out: &mut String) {
        match self {
            Value::Void => out.push_str("void"),
            Value::Int(n) => n.show(out),
            Value::Float(x) => x.show(out),
            Value::Bool(b) => b.show(out),
            Value::Str(s) => s.show(out),
            Value::Error(e) => out.push_str(&e.to_display()),
            Value::List(l) => l.show(out),
            Value::Set(s) => s.show(out),
            Value::Map(m) => m.show(out),
            Value::Tuple(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    v.show(out);
                }
                out.push(']');
            }
            Value::Func(_) => out.push_str("<function>"),
        }
    }

    fn text(&self, out: &mut String) {
        match self {
            Value::Str(s) => out.push_str(s.as_str()),
            other => other.show(out),
        }
    }
}

impl Equal for Value {
    fn equal(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a.equal(b),
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a.equal(b),
            (Value::List(a), Value::List(b)) => a.equal(b),
            (Value::Set(a), Value::Set(b)) => a.equal(b),
            (Value::Map(a), Value::Map(b)) => a.equal(b),
            (Value::Tuple(a), Value::Tuple(b)) => a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.equal(y)),
            _ => unreachable!("type checker guarantees equatable operands of one type"),
        }
    }
}

impl DatHash for Value {
    fn dat_hash<H: Hasher>(&self, h: &mut H) {
        match self {
            Value::Int(n) => n.dat_hash(h),
            Value::Bool(b) => b.dat_hash(h),
            Value::Str(s) => s.dat_hash(h),
            Value::List(l) => l.dat_hash(h),
            Value::Tuple(items) => {
                h.write_usize(items.len());
                for v in items.iter() {
                    v.dat_hash(h);
                }
            }
            _ => unreachable!("type checker guarantees a hashable value"),
        }
    }
}

impl Order for Value {
    fn order(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a.order(b),
            (Value::Float(a), Value::Float(b)) => a.order(b),
            (Value::Str(a), Value::Str(b)) => a.order(b),
            _ => unreachable!("type checker guarantees comparable values of one type"),
        }
    }
}

impl Placeholder for Value {
    fn placeholder() -> Self {
        Value::Void
    }
}
