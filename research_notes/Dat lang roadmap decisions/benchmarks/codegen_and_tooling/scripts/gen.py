#!/usr/bin/env python3
"""Generate a dat-shaped Rust project for compile-time measurements.

Usage: gen.py OUT_DIR N_MODULES N_CRATES [--derive] [--dispatch=dyn|enum]
  N_CRATES=1 -> one crate with N modules; N_CRATES=k -> modules grouped into k lib crates
  (crate g depends on crates < g), plus a bin crate `app` with the test harness main.
"""
import os, sys

out, n_mod, n_crates = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
derive = "--derive" in sys.argv
dispatch = "enum" if "--dispatch=enum" in sys.argv else "dyn"
star = "--star" in sys.argv
FNS = 10

RT = r'''
use std::rc::Rc;
use std::fmt;
use std::cell::{Cell, OnceCell};

pub struct Loc { pub file: &'static str, pub line: u32, pub col: u32 }

#[derive(Debug, Clone)]
pub enum RuntimeError { Range(String), Type(String), Assertion(String) }

#[cold]
#[inline(never)]
pub fn throw_range(msg: &str, loc: &'static Loc) -> ! {
    std::panic::panic_any(RuntimeError::Range(format!("{} at {}:{}:{}", msg, loc.file, loc.line, loc.col)))
}
#[cold]
#[inline(never)]
pub fn throw_assert(msg: &str, loc: &'static Loc) -> ! {
    std::panic::panic_any(RuntimeError::Assertion(format!("{} at {}:{}:{}", msg, loc.file, loc.line, loc.col)))
}

#[inline] pub fn add(a: i64, b: i64, loc: &'static Loc) -> i64 { match a.checked_add(b) { Some(v) => v, None => throw_range("int overflow", loc) } }
#[inline] pub fn sub(a: i64, b: i64, loc: &'static Loc) -> i64 { match a.checked_sub(b) { Some(v) => v, None => throw_range("int overflow", loc) } }
#[inline] pub fn mul(a: i64, b: i64, loc: &'static Loc) -> i64 { match a.checked_mul(b) { Some(v) => v, None => throw_range("int overflow", loc) } }
#[inline] pub fn rem(a: i64, b: i64, loc: &'static Loc) -> i64 { match a.checked_rem(b) { Some(v) => v, None => throw_range("int % 0", loc) } }
#[inline] pub fn assert(c: bool, loc: &'static Loc) { if !c { throw_assert("assertion failed", loc) } }

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Str(Rc<str>);
impl Str {
    pub fn lit(s: &'static str) -> Str { Str(Rc::from(s)) }
    pub fn len(&self) -> i64 { self.0.len() as i64 }
    pub fn concat(&self, o: &Str) -> Str { let mut s = String::with_capacity(self.0.len() + o.0.len()); s.push_str(&self.0); s.push_str(&o.0); Str(Rc::from(s)) }
    pub fn from_int(i: i64) -> Str { Str(Rc::from(i.to_string())) }
    pub fn upper(&self) -> Str { Str(Rc::from(self.0.to_uppercase())) }
}
impl fmt::Display for Str { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) } }

pub struct List<T>(Rc<Vec<T>>);
impl<T> Clone for List<T> { fn clone(&self) -> Self { List(self.0.clone()) } }
impl<T: PartialEq> PartialEq for List<T> { fn eq(&self, o: &Self) -> bool { *self.0 == *o.0 } }
impl<T: Clone> List<T> {
    pub fn new() -> Self { List(Rc::new(Vec::new())) }
    pub fn from_vec(v: Vec<T>) -> Self { List(Rc::new(v)) }
    pub fn push(&mut self, x: T) { Rc::make_mut(&mut self.0).push(x) }
    pub fn len(&self) -> i64 { self.0.len() as i64 }
    pub fn get(&self, i: i64, loc: &'static Loc) -> &T { if i < 0 || i as usize >= self.0.len() { throw_range("index out of range", loc) } &self.0[i as usize] }
    pub fn try_get(&self, i: i64) -> Option<&T> { if i < 0 { None } else { self.0.get(i as usize) } }
    pub fn iter(&self) -> std::slice::Iter<'_, T> { self.0.iter() }
    pub fn map<U: Clone>(&self, f: &dyn Fn(&T) -> U) -> List<U> { let mut v = Vec::with_capacity(self.0.len()); for x in self.0.iter() { v.push(f(x)); } List::from_vec(v) }
    pub fn filter(&self, f: &dyn Fn(&T) -> bool) -> List<T> { let mut v = Vec::new(); for x in self.0.iter() { if f(x) { v.push(x.clone()); } } List::from_vec(v) }
}

pub fn print(s: &Str) { println!("{}", s) }

/// Module const: lazily computed per thread, with re-entry detection.
pub struct ModConst<T> { cell: OnceCell<T>, busy: Cell<bool> }
impl<T: Clone> ModConst<T> {
    pub const fn new() -> Self { ModConst { cell: OnceCell::new(), busy: Cell::new(false) } }
    pub fn get(&self, init: fn() -> T, loc: &'static Loc) -> T {
        if let Some(v) = self.cell.get() { return v.clone(); }
        if self.busy.get() { throw_assert("module constant depends on itself", loc) }
        self.busy.set(true);
        struct Reset<'a>(&'a Cell<bool>);
        impl Drop for Reset<'_> { fn drop(&mut self) { self.0.set(false) } }
        let _r = Reset(&self.busy);
        let v = init();
        let _ = self.cell.set(v.clone());
        v
    }
}

pub fn run_tests(tests: &[(&'static str, fn())]) -> (usize, usize) {
    std::panic::set_hook(Box::new(|_| {}));
    let mut pass = 0; let mut fail = 0;
    for (name, t) in tests {
        match std::panic::catch_unwind(*t) { Ok(()) => pass += 1, Err(e) => { fail += 1; if let Some(r) = e.downcast_ref::<RuntimeError>() { println!("FAIL {} {:?}", name, r); } } }
    }
    (pass, fail)
}
'''

def crate_of(m):
    return m * n_crates // n_mod

def mod_path(m, from_m):
    if n_crates == 1:
        return f"crate::m{m}"
    c = crate_of(m)
    if c == crate_of(from_m):
        return f"crate::m{m}"
    return f"c{c}::m{m}"

def gen_module(i):
    L = []
    w = L.append
    w("#![allow(unused_variables, unused_mut, dead_code, unused_imports, non_snake_case, non_camel_case_types)]")
    w("use datrt::{Str, List, Loc, add, sub, mul, rem};")
    nloc = 40
    for k in range(nloc):
        w(f'static L{k}: Loc = Loc {{ file: "m{i}.dat", line: {k+1}, col: 5 }};')
    # structs
    for k in range(3):
        if derive:
            w("#[derive(Clone, PartialEq)]")
        w(f"pub struct S{i}_{k} {{ pub a: i64, pub b: f64, pub name: Str, pub items: List<i64> }}")
        if not derive:
            w(f"impl Clone for S{i}_{k} {{ fn clone(&self) -> Self {{ S{i}_{k} {{ a: self.a, b: self.b, name: self.name.clone(), items: self.items.clone() }} }} }}")
            w(f"impl PartialEq for S{i}_{k} {{ fn eq(&self, o: &Self) -> bool {{ self.a == o.a && self.b == o.b && self.name == o.name && self.items == o.items }} }}")
        w(f"impl S{i}_{k} {{")
        w(f"    pub fn area(&self) -> f64 {{ self.b * self.b * {k+1}.0 }}")
        w(f"    pub fn bump(&mut self, n: i64) {{ self.a = add(self.a, n, &L{k}); self.items.push(n); }}")
        w(f"    pub fn make(a: i64) -> S{i}_{k} {{ S{i}_{k} {{ a, b: a as f64 * 0.5, name: Str::lit(\"s{k}\"), items: List::new() }} }}")
        w("}")
    # union of S0 | S1 | S2
    if derive:
        w("#[derive(Clone, PartialEq)]")
    w(f"pub enum U{i} {{ S0(S{i}_0), S1(S{i}_1), S2(S{i}_2) }}")
    if not derive:
        w(f"impl Clone for U{i} {{ fn clone(&self) -> Self {{ match self {{ U{i}::S0(v) => U{i}::S0(v.clone()), U{i}::S1(v) => U{i}::S1(v.clone()), U{i}::S2(v) => U{i}::S2(v.clone()) }} }} }}")
        w(f"impl PartialEq for U{i} {{ fn eq(&self, o: &Self) -> bool {{ match (self, o) {{ (U{i}::S0(a), U{i}::S0(b)) => a == b, (U{i}::S1(a), U{i}::S1(b)) => a == b, (U{i}::S2(a), U{i}::S2(b)) => a == b, _ => false }} }} }}")
    for k in range(3):
        w(f"impl From<S{i}_{k}> for U{i} {{ fn from(v: S{i}_{k}) -> Self {{ U{i}::S{k}(v) }} }}")
    # interface
    w(f"pub trait Shape{i} {{")
    w("    fn area(&self) -> f64;")
    w("    fn name(&self) -> Str;")
    w("    fn describe(&self) -> Str { self.name().concat(&Str::from_int(self.area() as i64)) }")
    w(f"    fn clone_box(&self) -> Box<dyn Shape{i}>;")
    w("}")
    for k in range(3):
        w(f"impl Shape{i} for S{i}_{k} {{ fn area(&self) -> f64 {{ S{i}_{k}::area(self) }} fn name(&self) -> Str {{ self.name.clone() }} fn clone_box(&self) -> Box<dyn Shape{i}> {{ Box::new(self.clone()) }} }}")
    if dispatch == "enum":
        # closed-world lowering of the interface value type
        w(f"pub enum DynShape{i} {{ S0(S{i}_0), S1(S{i}_1), S2(S{i}_2) }}")
        w(f"impl Clone for DynShape{i} {{ fn clone(&self) -> Self {{ match self {{ DynShape{i}::S0(v) => DynShape{i}::S0(v.clone()), DynShape{i}::S1(v) => DynShape{i}::S1(v.clone()), DynShape{i}::S2(v) => DynShape{i}::S2(v.clone()) }} }} }}")
        w(f"impl DynShape{i} {{ pub fn area(&self) -> f64 {{ match self {{ DynShape{i}::S0(v) => Shape{i}::area(v), DynShape{i}::S1(v) => Shape{i}::area(v), DynShape{i}::S2(v) => Shape{i}::area(v) }} }}")
        w(f"  pub fn describe(&self) -> Str {{ match self {{ DynShape{i}::S0(v) => Shape{i}::describe(v), DynShape{i}::S1(v) => Shape{i}::describe(v), DynShape{i}::S2(v) => Shape{i}::describe(v) }} }} }}")
    # module const
    w(f"fn __init_LIMIT{i}() -> List<i64> {{ List::from_vec(vec![1, 2, {i}]) }}")
    w(f"thread_local! {{ static LIMIT{i}: datrt::ModConst<List<i64>> = const {{ datrt::ModConst::new() }}; }}")
    w(f"pub fn limit{i}() -> List<i64> {{ LIMIT{i}.with(|c| c.get(__init_LIMIT{i}, &L39)) }}")
    # leaf fn other modules call
    w(f"pub fn leaf{i}(x: i64) -> i64 {{ add(mul(x, 3, &L30), {i}, &L31) }}")
    # functions
    for j in range(FNS):
        dep = (i * 7 + j) % i if i > 0 else None
        if star and dep is not None and crate_of(dep) not in (0, crate_of(i)):
            dep = dep % max(1, n_mod // n_crates)
        w(f"pub fn f{i}_{j}(x: i64, xs: &List<i64>, p: &S{i}_0) -> i64 {{")
        w("    let mut acc: i64 = 0;")
        w(f"    for v in xs.iter() {{ acc = add(acc, mul(*v, x, &L{j}), &L{j+1}); }}")
        w(f"    if acc > {100 + j} {{ acc = sub(acc, p.a, &L{j+2}); }} else {{ acc = add(acc, {j}, &L{j+3}); }}")
        w(f"    let u: U{i} = if rem(x, 2, &L{j+4}) == 0 {{ U{i}::from(p.clone()) }} else {{ U{i}::from(S{i}_1::make(x)) }};")
        w(f"    let r = match &u {{ U{i}::S0(c) => c.a, U{i}::S1(s) => add(s.a, 1, &L{j+5}), U{i}::S2(t) => t.a }};")
        w(f"    acc = add(acc, r, &L{j+6});")
        if dispatch == "dyn":
            w(f"    let shapes: Vec<Box<dyn Shape{i}>> = vec![Box::new(p.clone()), Box::new(S{i}_1::make(x)), Box::new(S{i}_2::make({j}))];")
            w("    let mut total = 0.0;")
            w("    for s in shapes.iter() { total += s.area(); }")
            w("    let copy: Vec<Box<dyn Shape" + str(i) + ">> = shapes.iter().map(|s| s.clone_box()).collect();")
            w("    let d = copy[0].describe();")
        else:
            w(f"    let shapes: Vec<DynShape{i}> = vec![DynShape{i}::S0(p.clone()), DynShape{i}::S1(S{i}_1::make(x)), DynShape{i}::S2(S{i}_2::make({j}))];")
            w("    let mut total = 0.0;")
            w("    for s in shapes.iter() { total += s.area(); }")
            w(f"    let copy: Vec<DynShape{i}> = shapes.clone();")
            w("    let d = copy[0].describe();")
        w(f"    let mut q = S{i}_2::make(acc);")
        w(f"    q.bump({j});")
        w(f"    let doubled = xs.map(&|v: &i64| mul(*v, 2, &L{j+7}));")
        w(f"    let evens = doubled.filter(&|v: &i64| rem(*v, 4, &L{j+8}) == 0);")
        w(f"    acc = add(acc, evens.len(), &L{j+9});")
        w(f"    let name = Str::lit(\"item\").concat(&Str::from_int(acc));")
        w(f"    if name.len() > 100 {{ datrt::print(&name); }}")
        w(f"    let lim = limit{i}();")
        w(f"    acc = add(acc, *lim.get(0, &L{j+10}), &L{j+11});")
        if dep is not None:
            w(f"    acc = add(acc, {mod_path(dep, i)}::leaf{dep}(x), &L{j+12});")
        w(f"    acc = add(acc, total as i64, &L{j+13});")
        w(f"    acc = add(acc, d.len(), &L{j+14});")
        w("    acc")
        w("}")
    # tests
    tests = []
    for t in range(3):
        w(f"pub fn test_{i}_{t}() {{")
        w(f"    let xs = List::from_vec(vec![1, 2, 3, {t}]);")
        w(f"    let p = S{i}_0::make({t});")
        w(f"    let r = f{i}_{t}(2, &xs, &p);")
        w(f"    datrt::assert(r > 0, &L{20+t});")
        w(f"    let mut s = S{i}_1::make(1); s.bump(2); datrt::assert(s.a == 3, &L{25+t});")
        w("}")
        tests.append(f'("m{i}::test_{i}_{t}", test_{i}_{t} as fn())')
    w(f"pub static TESTS: &[(&str, fn())] = &[{', '.join(tests)}];")
    return "\n".join(L) + "\n"

def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)

os.makedirs(out, exist_ok=True)
members = ["datrt", "app"]
write(f"{out}/datrt/Cargo.toml", '[package]\nname = "datrt"\nversion = "0.1.0"\nedition = "2021"\n')
write(f"{out}/datrt/src/lib.rs", RT)

app_deps = 'datrt = { path = "../datrt" }\n'
if n_crates == 1:
    mods = "".join(f"pub mod m{i};\n" for i in range(n_mod))
    for i in range(n_mod):
        write(f"{out}/app/src/m{i}.rs", gen_module(i))
    all_tests = " ".join(f"v.extend_from_slice(m{i}::TESTS);" for i in range(n_mod))
    main = mods + f"fn main() {{ let mut v: Vec<(&str, fn())> = Vec::new(); {all_tests} let (p, f) = datrt::run_tests(&v); println!(\"{{}} passed, {{}} failed\", p, f); }}\n"
    write(f"{out}/app/src/main.rs", main)
else:
    for c in range(n_crates):
        members.append(f"c{c}")
        deps = 'datrt = { path = "../datrt" }\n' + "".join(f'c{d} = {{ path = "../c{d}" }}\n' for d in (([0] if c > 0 else []) if star else range(c)))
        write(f"{out}/c{c}/Cargo.toml", f'[package]\nname = "c{c}"\nversion = "0.1.0"\nedition = "2021"\n\n[dependencies]\n{deps}')
        ms = [i for i in range(n_mod) if crate_of(i) == c]
        write(f"{out}/c{c}/src/lib.rs", "".join(f"pub mod m{i};\n" for i in ms))
        for i in ms:
            write(f"{out}/c{c}/src/m{i}.rs", gen_module(i))
        app_deps += f'c{c} = {{ path = "../c{c}" }}\n'
    all_tests = " ".join(f"v.extend_from_slice(c{crate_of(i)}::m{i}::TESTS);" for i in range(n_mod))
    write(f"{out}/app/src/main.rs", f"fn main() {{ let mut v: Vec<(&str, fn())> = Vec::new(); {all_tests} let (p, f) = datrt::run_tests(&v); println!(\"{{}} passed, {{}} failed\", p, f); }}\n")

write(f"{out}/app/Cargo.toml", f'[package]\nname = "app"\nversion = "0.1.0"\nedition = "2021"\n\n[dependencies]\n{app_deps}')
ws = "[workspace]\nresolver = \"2\"\nmembers = [" + ", ".join(f'"{m}"' for m in members) + "]\n"
ws += "\n[profile.dev]\npanic = \"unwind\"\n"
write(f"{out}/Cargo.toml", ws)
print(f"generated {n_mod} modules in {n_crates} crate(s) at {out} (derive={derive}, dispatch={dispatch})")
