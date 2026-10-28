// Two minimal AST-walking interpreters for: function fib(n:int):int { if (n < 2) { return n; } return fib(n-1)+fib(n-2); }
use std::collections::HashMap; use std::rc::Rc;
#[derive(Clone, Debug)] enum Value { Int(i64), Bool(bool), #[allow(dead_code)] Float(f64), #[allow(dead_code)] Str(Rc<str>) }
#[derive(Debug)] enum Ctl { Return(Value), Err(String) }
// ---------- naive: names are strings, env is a HashMap per call ----------
enum NE { Int(i64), Var(String), Lt(Box<NE>, Box<NE>), Add(Box<NE>, Box<NE>), Sub(Box<NE>, Box<NE>), Call(String, Vec<NE>) }
enum NS { If(NE, Vec<NS>), Return(NE) }
struct NF { params: Vec<String>, body: Vec<NS> }
struct Naive { funcs: HashMap<String, Rc<NF>> }
impl Naive {
  fn eval(&self, e: &NE, env: &HashMap<String, Value>) -> Result<Value, Ctl> {
    Ok(match e {
      NE::Int(i) => Value::Int(*i),
      NE::Var(n) => env.get(n).cloned().ok_or_else(|| Ctl::Err(format!("undefined {}", n)))?,
      NE::Lt(a, b) => match (self.eval(a, env)?, self.eval(b, env)?) { (Value::Int(x), Value::Int(y)) => Value::Bool(x < y), _ => return Err(Ctl::Err("type".into())) },
      NE::Add(a, b) => match (self.eval(a, env)?, self.eval(b, env)?) { (Value::Int(x), Value::Int(y)) => Value::Int(x.checked_add(y).ok_or_else(|| Ctl::Err("RangeError: integer overflow".into()))?), _ => return Err(Ctl::Err("type".into())) },
      NE::Sub(a, b) => match (self.eval(a, env)?, self.eval(b, env)?) { (Value::Int(x), Value::Int(y)) => Value::Int(x.checked_sub(y).ok_or_else(|| Ctl::Err("RangeError: integer overflow".into()))?), _ => return Err(Ctl::Err("type".into())) },
      NE::Call(name, args) => { let f = self.funcs.get(name).cloned().ok_or_else(|| Ctl::Err("nofn".into()))?;
        let mut env2 = HashMap::new(); for (p, a) in f.params.iter().zip(args) { env2.insert(p.clone(), self.eval(a, env)?); }
        match self.exec_block(&f.body, &env2) { Ok(()) => Value::Int(0), Err(Ctl::Return(v)) => v, Err(e) => return Err(e) } }
    })
  }
  fn exec_block(&self, b: &[NS], env: &HashMap<String, Value>) -> Result<(), Ctl> {
    for s in b { match s { NS::If(c, t) => { if let Value::Bool(true) = self.eval(c, env)? { self.exec_block(t, env)?; } }
                            NS::Return(e) => return Err(Ctl::Return(self.eval(e, env)?)) } } Ok(())
  }
}
// ---------- resolved: locals in slots, functions by index, frames reuse a Vec stack ----------
enum RE { Int(i64), Local(usize), Lt(Box<RE>, Box<RE>), Add(Box<RE>, Box<RE>), Sub(Box<RE>, Box<RE>), Call(usize, Vec<RE>) }
enum RS { If(RE, Vec<RS>), Return(RE) }
struct RF { nlocals: usize, body: Vec<RS> }
struct Resolved { funcs: Vec<RF>, stack: Vec<Value> }
enum Flow { Normal, Return(Value) }
impl Resolved {
  fn eval(&mut self, e: &RE, base: usize) -> Result<Value, String> {
    Ok(match e {
      RE::Int(i) => Value::Int(*i),
      RE::Local(s) => self.stack[base + s].clone(),
      RE::Lt(a, b) => match (self.eval(a, base)?, self.eval(b, base)?) { (Value::Int(x), Value::Int(y)) => Value::Bool(x < y), _ => return Err("type".into()) },
      RE::Add(a, b) => match (self.eval(a, base)?, self.eval(b, base)?) { (Value::Int(x), Value::Int(y)) => Value::Int(x.checked_add(y).ok_or("RangeError: integer overflow")?), _ => return Err("type".into()) },
      RE::Sub(a, b) => match (self.eval(a, base)?, self.eval(b, base)?) { (Value::Int(x), Value::Int(y)) => Value::Int(x.checked_sub(y).ok_or("RangeError: integer overflow")?), _ => return Err("type".into()) },
      RE::Call(fi, args) => { let nb = self.stack.len();
        for a in args { let v = self.eval(a, base)?; self.stack.push(v); }
        let f: *const RF = &self.funcs[*fi]; let f = unsafe { &*f }; // benchmark-only shortcut
        for _ in args.len()..f.nlocals { self.stack.push(Value::Int(0)); }
        let r = self.exec_block(&f.body, nb); self.stack.truncate(nb);
        match r? { Flow::Return(v) => v, Flow::Normal => Value::Int(0) } }
    })
  }
  fn exec_block(&mut self, b: &[RS], base: usize) -> Result<Flow, String> {
    for s in b { match s { RS::If(c, t) => { if let Value::Bool(true) = self.eval(c, base)? { if let Flow::Return(v) = self.exec_block(t, base)? { return Ok(Flow::Return(v)); } } }
                            RS::Return(e) => return Ok(Flow::Return(self.eval(e, base)?)) } } Ok(Flow::Normal)
  }
}
fn main() {
  let mode = std::env::args().nth(1).unwrap(); let n: i64 = std::env::args().nth(2).unwrap().parse().unwrap();
  if mode == "naive" {
    let v = || Box::new(NE::Var("n".into()));
    let body = vec![ NS::If(NE::Lt(v(), Box::new(NE::Int(2))), vec![NS::Return(NE::Var("n".into()))]),
      NS::Return(NE::Add(Box::new(NE::Call("fib".into(), vec![NE::Sub(v(), Box::new(NE::Int(1)))])), Box::new(NE::Call("fib".into(), vec![NE::Sub(v(), Box::new(NE::Int(2)))])))) ];
    let mut funcs = HashMap::new(); funcs.insert("fib".to_string(), Rc::new(NF { params: vec!["n".into()], body }));
    let it = Naive { funcs }; match it.eval(&NE::Call("fib".into(), vec![NE::Int(n)]), &HashMap::new()) { Ok(Value::Int(x)) => println!("{}", x), r => { eprintln!("{:?}", r.err()); std::process::exit(1) } }
  } else {
    let l = || Box::new(RE::Local(0));
    let body = vec![ RS::If(RE::Lt(l(), Box::new(RE::Int(2))), vec![RS::Return(RE::Local(0))]),
      RS::Return(RE::Add(Box::new(RE::Call(0, vec![RE::Sub(l(), Box::new(RE::Int(1)))])), Box::new(RE::Call(0, vec![RE::Sub(l(), Box::new(RE::Int(2)))])))) ];
    let mut it = Resolved { funcs: vec![RF { nlocals: 1, body }], stack: Vec::with_capacity(1024) };
    match it.eval(&RE::Call(0, vec![RE::Int(n)]), 0) { Ok(Value::Int(x)) => println!("{}", x), r => { eprintln!("{:?}", r.err()); std::process::exit(1) } }
  }
}
