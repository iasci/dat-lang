// Generated-code sketches for dat's hardest mappings. Compiled with rustc 1.99, edition 2024.
#![allow(dead_code, unused_variables, non_camel_case_types)]
use std::rc::Rc;

#[derive(Clone, PartialEq)] pub struct Str(Rc<str>);
fn s(x: &str) -> Str { Str(Rc::from(x)) }

// ---------- structs ----------
#[derive(Clone, PartialEq)] pub struct Address { pub city: Str, pub zip: Str }
#[derive(Clone, PartialEq)] pub struct User { pub name: Str, pub address: Option<Address> }

// (1a) field-path narrowing that survives an early return:
// dat: if (user.address == null) { return "unknown"; } return user.address.city;
pub fn city(user: &User) -> Str {
    let Some(user__address) = &user.address else { return s("unknown"); };
    user__address.city.clone()
}

// (1b) narrowing on a `let` local, survives an unrelated sibling-field assignment,
// ends when the path itself is assigned (the generated code goes back to `user.address`).
pub fn rename(mut user: User) -> Str {
    let Some(user__address) = &user.address else { return s("none"); };
    let c = user__address.city.clone();
    user.name = s("renamed");                   // disjoint field: borrow of user.address stays valid
    let z = user__address.zip.clone();          // still narrowed
    user.address = None;                        // assignment ends narrowing; no later use of user__address
    if user.address.is_none() { c } else { z }
}

// (1c) `||` with early exit: if (x == null || x.isEmpty()) { log(); return 0; }  x is non-null after.
// Labelled block with break-value (stable since 1.65): one copy of the diverging branch, no unwrap.
pub fn first_len(x: &Option<Vec<i64>>) -> i64 {
    let x__n: &Vec<i64> = 'narrow: {
        if let Some(v) = x { if !v.is_empty() { break 'narrow v; } }
        println!("empty");
        return 0;
    };
    x__n.len() as i64
}

// (1d) && with let-chains (edition 2024, Rust 1.88+): if (x != null && x.isEmpty()) { ... }
pub fn and_chain(x: &Option<Vec<i64>>) -> bool {
    if let Some(v) = x && v.is_empty() { return true; }
    false
}

// (1e) narrowing a `let` local then mutating through the narrowed name.
pub fn push_if_present(mut x: Option<Vec<i64>>) -> Option<Vec<i64>> {
    if let Some(x__n) = &mut x { x__n.push(1); }
    x
}

// ---------- unions ----------
// Canonical enum per flattened, sorted member set; null is always the outer Option.
// dat: int | string | null  ==>  Option<U_int_string>
#[derive(Clone, PartialEq)] pub enum U_int_string { int(i64), string(Str) }
impl From<i64> for U_int_string { fn from(v: i64) -> Self { U_int_string::int(v) } }
impl From<Str> for U_int_string { fn from(v: Str) -> Self { U_int_string::string(v) } }

// dat: function describe(value: int | string | null): string { if (value is int) ...; if (value is string) ...; return "nothing"; }
pub fn describe(value: &Option<U_int_string>) -> Str {
    if let Some(U_int_string::int(value__n)) = value { return s(&format!("number {}", value__n + 1)); }
    if let Some(U_int_string::string(value__n)) = value { return value__n.clone(); }
    s("nothing")
}
// after `if (value == null) return ...;` the narrowed type `int | string` is exactly the payload: zero-cost.
pub fn non_null(value: &Option<U_int_string>) -> i64 {
    let Some(value__n) = value else { return -1; };
    match value__n { U_int_string::int(i) => *i, U_int_string::string(t) => t.0.len() as i64 }
}

#[derive(Clone, PartialEq)] pub struct Circle { pub label: Str, pub radius: f64 }
#[derive(Clone, PartialEq)] pub struct Square { pub label: Str, pub side: f64 }
#[derive(Clone, PartialEq)] pub struct Triangle { pub label: Str, pub base: f64 }
#[derive(Clone, PartialEq)] pub enum U_Circle_Square { Circle(Circle), Square(Square) }
#[derive(Clone, PartialEq)] pub enum U_Circle_Square_Triangle { Circle(Circle), Square(Square), Triangle(Triangle) }
// widening between unions that share members: generated only for pairs the program actually uses
impl From<U_Circle_Square> for U_Circle_Square_Triangle {
    fn from(v: U_Circle_Square) -> Self { match v { U_Circle_Square::Circle(c) => Self::Circle(c), U_Circle_Square::Square(q) => Self::Square(q) } }
}
// shared field read without narrowing: generated accessor
impl U_Circle_Square_Triangle {
    pub fn label(&self) -> &Str { match self { Self::Circle(v) => &v.label, Self::Square(v) => &v.label, Self::Triangle(v) => &v.label } }
}
// sub-union narrowing: after `if (shape is Triangle) return 0.0;` shape: Circle | Square.
// Keep the original enum; the compiler knows the excluded arm is dead.
pub fn area(shape: &U_Circle_Square_Triangle) -> f64 {
    if let U_Circle_Square_Triangle::Triangle(_) = shape { return 0.0; }
    match shape {
        U_Circle_Square_Triangle::Circle(c) => 3.14159 * c.radius * c.radius,
        U_Circle_Square_Triangle::Square(q) => q.side * q.side,
        U_Circle_Square_Triangle::Triangle(_) => unreachable!("dat: narrowed away"),
    }
}

// ---------- interfaces ----------
pub trait Shape {
    fn area(&self) -> f64;
    fn name(&self) -> Str;
    fn describe(&self) -> Str { s(&format!("{} with area {:.2}", self.name().0, self.area())) }
    // closed-world helpers emitted by the dat compiler (no Any, no TypeId):
    fn __tag(&self) -> u32;
    fn __clone_box(&self) -> Box<dyn Shape>;
    fn __as_Circle(&self) -> Option<&Circle> { None }   // only for structs used as `is`/`as?` targets
}
impl Shape for Circle {
    fn area(&self) -> f64 { 3.14159 * self.radius * self.radius }
    fn name(&self) -> Str { s("circle") }
    fn __tag(&self) -> u32 { 0 }
    fn __clone_box(&self) -> Box<dyn Shape> { Box::new(self.clone()) }
    fn __as_Circle(&self) -> Option<&Circle> { Some(self) }
}
impl Shape for Square {
    fn area(&self) -> f64 { self.side * self.side }
    fn name(&self) -> Str { s("square") }
    fn __tag(&self) -> u32 { 1 }
    fn __clone_box(&self) -> Box<dyn Shape> { Box::new(self.clone()) }
}
impl Clone for Box<dyn Shape> { fn clone(&self) -> Self { self.__clone_box() } }
// open-world value: Box<dyn Shape>;  `if (shape is Circle) print(shape.radius)`
pub fn radius_or_zero(shape: &Box<dyn Shape>) -> f64 {
    if let Some(shape__n) = shape.__as_Circle() { return shape__n.radius; }
    0.0
}
// closed-world value: every implementor known at build time
#[derive(Clone)] pub enum I_Shape { Circle(Circle), Square(Square) }
impl I_Shape {
    #[inline] pub fn area(&self) -> f64 { match self { Self::Circle(v) => Shape::area(v), Self::Square(v) => Shape::area(v) } }
    #[inline] pub fn describe(&self) -> Str { match self { Self::Circle(v) => Shape::describe(v), Self::Square(v) => Shape::describe(v) } }
}
pub fn total(shapes: &[I_Shape]) -> f64 { let mut t = 0.0; for sh in shapes { t += sh.area(); if let I_Shape::Circle(c) = sh { t += c.radius; } } t }

// ---------- closures ----------
pub struct Button { pub on_click: Rc<dyn Fn(i64) -> i64> }   // stored function value: cheap to copy
pub fn apply(x: i64, transform: &dyn Fn(i64) -> i64) -> i64 { transform(x) }   // parameter: no alloc, no monomorphization
pub fn make_adder(rate: i64) -> Rc<dyn Fn(i64) -> i64> { Rc::new(move |p| p + rate) }  // captures a copy
fn square(n: i64) -> i64 { n * n }

fn main() {
    let u = User { name: s("Ada"), address: Some(Address { city: s("Paris"), zip: s("75") }) };
    assert!(city(&u).0.as_ref() == "Paris");
    assert!(rename(u.clone()).0.as_ref() == "Paris");
    assert!(first_len(&Some(vec![1, 2])) == 2 && first_len(&None) == 0);
    assert!(describe(&Some(U_int_string::from(41))).0.as_ref() == "number 42");
    let w: U_Circle_Square_Triangle = U_Circle_Square::Circle(Circle { label: s("c"), radius: 1.0 }).into();
    assert!(w.label().0.as_ref() == "c" && area(&w) > 3.0);
    let b: Box<dyn Shape> = Box::new(Circle { label: s("c"), radius: 2.0 });
    let b2 = b.clone();
    assert!(radius_or_zero(&b2) == 2.0);
    let btn = Button { on_click: make_adder(2) };
    let copy = Button { on_click: btn.on_click.clone() };
    assert!(apply(3, &square) == 9 && apply(3, &*copy.on_click) == 5);
    println!("all sketches ok");
}
