#!/usr/bin/env python3
"""Interface lowering micro-benchmark: Box<dyn Shape> vs closed-world enum.
Usage: gen_dispatch.py OUT K M MODE   (MODE = dyn | enum)
K implementing structs, M functions that copy/mutate/dispatch/downcast a list<Shape>.
"""
import os, sys
out, K, M, mode = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
os.makedirs(f"{out}/src", exist_ok=True)
T = "Box<dyn Shape>" if mode == "dyn" else "ShapeV"

s = ["#![allow(dead_code, unused_variables)]"]
w = s.append
w("pub trait Shape {")
w("    fn area(&self) -> f64;")
w("    fn name(&self) -> &'static str;")
w("    fn scale(&mut self, f: f64);")
w("    fn describe(&self) -> String { let mut r = String::from(self.name()); r.push(':'); r.push_str(&(self.area() as i64).to_string()); r }")
if mode == "dyn":
    w("    fn clone_box(&self) -> Box<dyn Shape>;")
    w("    fn tag(&self) -> u32;")
    w("    fn as_s0(&self) -> Option<&S0> { None }")
w("}")
for k in range(K):
    w(f"pub struct S{k} {{ pub w: f64, pub h: f64, pub tags: Vec<i64> }}")
    w(f"impl Clone for S{k} {{ fn clone(&self) -> Self {{ S{k} {{ w: self.w, h: self.h, tags: self.tags.clone() }} }} }}")
    extra = ""
    if mode == "dyn":
        extra = f" fn clone_box(&self) -> Box<dyn Shape> {{ Box::new(self.clone()) }} fn tag(&self) -> u32 {{ {k} }}"
        if k == 0:
            extra += " fn as_s0(&self) -> Option<&S0> { Some(self) }"
    w(f"impl Shape for S{k} {{ fn area(&self) -> f64 {{ self.w * self.h * {k + 1}.0 }} fn name(&self) -> &'static str {{ \"s{k}\" }} fn scale(&mut self, f: f64) {{ self.w *= f; self.h *= {1 + k % 3}.0 * f; }}{extra} }}")
if mode == "enum":
    variants = ", ".join(f"S{k}(S{k})" for k in range(K))
    w(f"pub enum ShapeV {{ {variants} }}")
    def arms(body):
        return " ".join(f"ShapeV::S{k}(v) => {body.format(k=k)}," for k in range(K))
    w(f"impl Clone for ShapeV {{ fn clone(&self) -> Self {{ match self {{ {arms('ShapeV::S{k}(v.clone())')} }} }} }}")
    w("impl ShapeV {")
    w(f"    pub fn area(&self) -> f64 {{ match self {{ {arms('Shape::area(v)')} }} }}")
    w(f"    pub fn name(&self) -> &'static str {{ match self {{ {arms('Shape::name(v)')} }} }}")
    w(f"    pub fn scale(&mut self, f: f64) {{ match self {{ {arms('Shape::scale(v, f)')} }} }}")
    w(f"    pub fn describe(&self) -> String {{ match self {{ {arms('Shape::describe(v)')} }} }}")
    w("}")
open(f"{out}/src/shapes.rs", "w").write("\n".join(s) + "\n")

f = ["#![allow(dead_code, unused_variables, unused_imports)]", "use crate::shapes::*;"]
for m in range(M):
    f.append(f"pub fn f{m}(xs: &[{T}], factor: f64) -> f64 {{")
    f.append("    let mut total = 0.0;")
    f.append(f"    let mut copy: Vec<{T}> = Vec::with_capacity(xs.len());")
    if mode == "dyn":
        f.append("    for s in xs.iter() { copy.push(s.clone_box()); }")
    else:
        f.append("    for s in xs.iter() { copy.push(s.clone()); }")
    f.append("    for s in copy.iter_mut() { s.scale(factor); }")
    f.append("    for s in copy.iter() {")
    f.append("        total += s.area();")
    if mode == "dyn":
        f.append(f"        if s.tag() == {m % K} {{ total += 1.0; }}")
        f.append("        if let Some(c) = s.as_s0() { total += c.w; }")
    else:
        f.append(f"        if let ShapeV::S{m % K}(_) = s {{ total += 1.0; }}")
        f.append("        if let ShapeV::S0(c) = s { total += c.w; }")
    f.append("    }")
    f.append(f"    if total > {m}.0 {{ total -= copy[0].describe().len() as f64; }}")
    f.append("    total")
    f.append("}")
open(f"{out}/src/funcs.rs", "w").write("\n".join(f) + "\n")

mk = "Box::new(S{k} {{ w: i as f64 * 0.001, h: 2.0, tags: vec![1] }})" if mode == "dyn" else "ShapeV::S{k}(S{k} {{ w: i as f64 * 0.001, h: 2.0, tags: vec![1] }})"
arms = " ".join(f"{k} => {mk.format(k=k)}," for k in range(K - 1)) + f" _ => {mk.format(k=K-1)},"
clone_m = "clone_box" if mode == "dyn" else "clone"
main = f"""mod shapes; mod funcs;
use shapes::*;
fn main() {{
    let n: usize = std::env::args().nth(1).map(|a| a.parse().unwrap()).unwrap_or(10);
    let mut xs: Vec<{T}> = Vec::with_capacity(n);
    for i in 0..n {{ xs.push(match i % {K} {{ {arms} }}); }}
    let t = std::time::Instant::now();
    let all: Vec<fn(&[{T}], f64) -> f64> = vec![{", ".join(f"funcs::f{m}" for m in range(M))}];
    let few: Vec<{T}> = xs.iter().take(4).map(|s| s.{clone_m}()).collect();
    let mut acc = 0.0; for f in all.iter() {{ acc += f(&few, 1.0); }}
    for _r in 0..20 {{ acc += funcs::f{M - 1}(&xs, 1.0001); }}
    // pure dispatch loop: no allocation
    let t2 = std::time::Instant::now();
    let mut a2 = 0.0;
    for _r in 0..50 {{ for s in xs.iter() {{ a2 += s.area(); }} }}
    println!("copy+scale+dispatch {{:?}}  dispatch-only {{:?}}  {{}} {{}}", t.elapsed() - t2.elapsed(), t2.elapsed(), acc, a2);
}}
"""
open(f"{out}/src/main.rs", "w").write(main)
open(f"{out}/Cargo.toml", "w").write(f'[package]\nname = "disp"\nversion = "0.1.0"\nedition = "2021"\n\n[profile.dev]\npanic = "unwind"\n[profile.release]\npanic = "unwind"\n')
