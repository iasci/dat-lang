use std::panic;
#[derive(Debug)] pub struct RangeError { pub message: String }
#[inline(never)] #[cold] fn overflow(op: &str, a: i64, b: i64) -> ! { panic::panic_any(RangeError { message: format!("integer overflow: {} {} {}", a, op, b) }) }
#[inline(always)] fn add(a: i64, b: i64) -> i64 { match a.checked_add(b) { Some(v) => v, None => overflow("+", a, b) } }
#[inline(always)] fn sub(a: i64, b: i64) -> i64 { match a.checked_sub(b) { Some(v) => v, None => overflow("-", a, b) } }
fn fib(n: i64) -> i64 { if n < 2 { n } else { add(fib(sub(n, 1)), fib(sub(n, 2))) } }
fn main() { panic::set_hook(Box::new(|_| {})); let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
  match panic::catch_unwind(|| fib(n)) { Ok(v) => println!("{}", v), Err(p) => { if let Some(e) = p.downcast_ref::<RangeError>() { eprintln!("RangeError: {}", e.message); } std::process::exit(1) } } }
