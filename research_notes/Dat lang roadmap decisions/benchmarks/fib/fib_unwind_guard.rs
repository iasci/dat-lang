use std::panic; use std::cell::Cell;
#[derive(Debug)] pub struct RangeError { pub message: String }
#[derive(Debug)] pub struct StackOverflowError { pub message: String }
thread_local! { static DEPTH: Cell<u32> = const { Cell::new(0) }; }
const MAX_DEPTH: u32 = 10_000;
struct Guard; impl Drop for Guard { #[inline(always)] fn drop(&mut self) { DEPTH.with(|d| d.set(d.get() - 1)); } }
#[inline(always)] fn enter() -> Guard { DEPTH.with(|d| { let v = d.get() + 1; if v > MAX_DEPTH { too_deep() } d.set(v); }); Guard }
#[inline(never)] #[cold] fn too_deep() -> ! { panic::panic_any(StackOverflowError { message: "maximum call depth exceeded".into() }) }
#[inline(never)] #[cold] fn overflow(op: &str, a: i64, b: i64) -> ! { panic::panic_any(RangeError { message: format!("integer overflow: {} {} {}", a, op, b) }) }
#[inline(always)] fn add(a: i64, b: i64) -> i64 { match a.checked_add(b) { Some(v) => v, None => overflow("+", a, b) } }
#[inline(always)] fn sub(a: i64, b: i64) -> i64 { match a.checked_sub(b) { Some(v) => v, None => overflow("-", a, b) } }
fn fib(n: i64) -> i64 { let _g = enter(); if n < 2 { n } else { add(fib(sub(n, 1)), fib(sub(n, 2))) } }
fn main() { panic::set_hook(Box::new(|_| {})); let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
  match panic::catch_unwind(|| fib(n)) { Ok(v) => println!("{}", v), Err(p) => { if let Some(e) = p.downcast_ref::<RangeError>() { eprintln!("RangeError: {}", e.message); } else if let Some(e) = p.downcast_ref::<StackOverflowError>() { eprintln!("StackOverflowError: {}", e.message); } std::process::exit(1) } } }
