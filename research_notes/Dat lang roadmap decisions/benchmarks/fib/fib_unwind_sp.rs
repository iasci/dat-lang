// unwinding runtime errors + stack-pointer guard against a global limit (no per-call TLS, no decrement on return)
use std::panic; use std::sync::atomic::{AtomicUsize, Ordering};
#[derive(Debug)] pub struct RangeError { pub message: String }
#[derive(Debug)] pub struct StackOverflowError { pub message: String }
static STACK_LIMIT: AtomicUsize = AtomicUsize::new(0);
#[inline(always)] fn check_stack() { let x = 0u8; let sp = &x as *const u8 as usize; if sp < STACK_LIMIT.load(Ordering::Relaxed) { too_deep() } }
#[inline(never)] #[cold] fn too_deep() -> ! { panic::panic_any(StackOverflowError { message: "maximum call depth exceeded".into() }) }
#[inline(never)] #[cold] fn overflow(op: &str, a: i64, b: i64) -> ! { panic::panic_any(RangeError { message: format!("integer overflow: {} {} {}", a, op, b) }) }
#[inline(always)] fn add(a: i64, b: i64) -> i64 { match a.checked_add(b) { Some(v) => v, None => overflow("+", a, b) } }
#[inline(always)] fn sub(a: i64, b: i64) -> i64 { match a.checked_sub(b) { Some(v) => v, None => overflow("-", a, b) } }
fn fib(n: i64) -> i64 { check_stack(); if n < 2 { n } else { add(fib(sub(n, 1)), fib(sub(n, 2))) } }
fn main() { panic::set_hook(Box::new(|_| {}));
  let base = 0u8; STACK_LIMIT.store((&base as *const u8 as usize).saturating_sub(7 * 1024 * 1024), Ordering::Relaxed); // main thread stack is 8 MiB on macOS/Linux by default
  let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
  match panic::catch_unwind(|| fib(n)) { Ok(v) => println!("{}", v), Err(p) => { if let Some(e) = p.downcast_ref::<RangeError>() { eprintln!("RangeError: {}", e.message); } else if let Some(e) = p.downcast_ref::<StackOverflowError>() { eprintln!("StackOverflowError: {}", e.message); } std::process::exit(1) } } }
