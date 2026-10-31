//! The table of compound types: lists, sets, maps, tuples and functions.
//!
//! A compound type is interned once per compilation, so two equal types
//! get the same [`TypeId`] and [`Type`] equality is structural: a
//! `list<int>` is a `list<int>` and nothing else, which is the invariance
//! §01 asks for, with no code. What a type supports (`==`, hashing,
//! ordering) is worked out once, when it's interned. Ids never reach
//! output or generated names.

use crate::{ErrorType, Type};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    List(Type),
    Set(Type),
    Map(Type, Type),
    Tuple(Vec<Type>),
    /// Parameter types and the return type. Parameter names are
    /// documentation only, so they aren't part of the type.
    Func(Vec<Type>, Type),
}

/// What a type supports (§01, "Equality, Hashing and Ordering").
#[derive(Clone, Copy, Debug, Default)]
struct Caps {
    equatable: bool,
    hashable: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Types {
    kinds: Vec<Kind>,
    caps: Vec<Caps>,
    ids: HashMap<Kind, TypeId>,
}

impl Types {
    pub fn intern(&mut self, kind: Kind) -> Type {
        if let Some(&id) = self.ids.get(&kind) {
            return Type::Of(id);
        }
        let caps = self.caps_of(&kind);
        let id = TypeId(self.kinds.len() as u32);
        self.kinds.push(kind.clone());
        self.caps.push(caps);
        self.ids.insert(kind, id);
        Type::Of(id)
    }

    pub fn list(&mut self, elem: Type) -> Type {
        self.intern(Kind::List(elem))
    }

    pub fn set(&mut self, elem: Type) -> Type {
        self.intern(Kind::Set(elem))
    }

    pub fn map(&mut self, key: Type, value: Type) -> Type {
        self.intern(Kind::Map(key, value))
    }

    pub fn tuple(&mut self, items: Vec<Type>) -> Type {
        self.intern(Kind::Tuple(items))
    }

    pub fn func(&mut self, params: Vec<Type>, ret: Type) -> Type {
        self.intern(Kind::Func(params, ret))
    }

    /// The kind of a compound type; `None` for a primitive one.
    pub fn kind(&self, t: Type) -> Option<&Kind> {
        match t {
            Type::Of(id) => Some(&self.kinds[id.0 as usize]),
            _ => None,
        }
    }

    pub fn list_elem(&self, t: Type) -> Option<Type> {
        match self.kind(t) {
            Some(Kind::List(e)) => Some(*e),
            _ => None,
        }
    }

    pub fn set_elem(&self, t: Type) -> Option<Type> {
        match self.kind(t) {
            Some(Kind::Set(e)) => Some(*e),
            _ => None,
        }
    }

    pub fn map_types(&self, t: Type) -> Option<(Type, Type)> {
        match self.kind(t) {
            Some(Kind::Map(k, v)) => Some((*k, *v)),
            _ => None,
        }
    }

    pub fn tuple_items(&self, t: Type) -> Option<&[Type]> {
        match self.kind(t) {
            Some(Kind::Tuple(items)) => Some(items),
            _ => None,
        }
    }

    pub fn func_sig(&self, t: Type) -> Option<(&[Type], Type)> {
        match self.kind(t) {
            Some(Kind::Func(params, ret)) => Some((params, *ret)),
            _ => None,
        }
    }

    /// `==` and `!=` work on it: everything but function values.
    pub fn equatable(&self, t: Type) -> bool {
        match t {
            Type::Of(id) => self.caps[id.0 as usize].equatable,
            Type::Int | Type::Float | Type::Bool | Type::Str => true,
            Type::Void | Type::Never | Type::Error(_) => false,
        }
    }

    /// It can be a set element or a map key: no floats, sets, maps or
    /// functions anywhere inside.
    pub fn hashable(&self, t: Type) -> bool {
        match t {
            Type::Of(id) => self.caps[id.0 as usize].hashable,
            Type::Int | Type::Bool | Type::Str => true,
            Type::Float | Type::Void | Type::Never | Type::Error(_) => false,
        }
    }

    /// `<` and `sort()` without a comparator work on it.
    pub fn comparable(&self, t: Type) -> bool {
        matches!(t, Type::Int | Type::Float | Type::Str)
    }

    fn caps_of(&self, kind: &Kind) -> Caps {
        match kind {
            Kind::List(e) => Caps { equatable: self.equatable(*e), hashable: self.hashable(*e) },
            Kind::Tuple(items) => {
                Caps { equatable: items.iter().all(|t| self.equatable(*t)), hashable: items.iter().all(|t| self.hashable(*t)) }
            }
            Kind::Set(e) => Caps { equatable: self.equatable(*e), hashable: false },
            Kind::Map(_, v) => Caps { equatable: self.equatable(*v), hashable: false },
            Kind::Func(..) => Caps { equatable: false, hashable: false },
        }
    }

    /// The type as dat writes it: `list<int>`, `[string, int]`, `(int) => bool`.
    pub fn name(&self, t: Type) -> String {
        match self.kind(t) {
            None => t.prim_name().to_string(),
            Some(Kind::List(e)) => format!("list<{}>", self.name(*e)),
            Some(Kind::Set(e)) => format!("set<{}>", self.name(*e)),
            Some(Kind::Map(k, v)) => format!("map<{}, {}>", self.name(*k), self.name(*v)),
            Some(Kind::Tuple(items)) => {
                let items: Vec<String> = items.iter().map(|t| self.name(*t)).collect();
                format!("[{}]", items.join(", "))
            }
            Some(Kind::Func(params, ret)) => {
                let params: Vec<String> = params.iter().map(|t| self.name(*t)).collect();
                format!("({}) => {}", params.join(", "), self.name(*ret))
            }
        }
    }
}

impl Type {
    /// The name of a primitive type; compound types are named by [`Types::name`].
    pub fn prim_name(self) -> &'static str {
        match self {
            Type::Int => "int",
            Type::Float => "float",
            Type::Bool => "bool",
            Type::Str => "string",
            Type::Void => "void",
            Type::Never => "never",
            Type::Error(e) => e.name(),
            Type::Of(_) => "{compound}",
        }
    }

    pub fn is_error(self) -> bool {
        matches!(self, Type::Error(_))
    }

    /// An error type other than the `e` of an untyped `catch (e)`.
    pub fn error(self) -> Option<ErrorType> {
        match self {
            Type::Error(e) => Some(e),
            _ => None,
        }
    }
}
