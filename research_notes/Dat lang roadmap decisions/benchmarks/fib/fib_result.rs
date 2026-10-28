#[derive(Debug)] pub struct RangeError { pub message: String }
#[inline(never)] #[cold] fn overflow(op: &str, a: i64, b: i64) -> RangeError { RangeError { message: format!("integer overflow: {} {} {}", a, op, b) } }
#[inline(always)] fn add(a: i64, b: i64) -> Result<i64, RangeError> { a.checked_add(b).ok_or_else(|| overflow("+", a, b)) }
#[inline(always)] fn sub(a: i64, b: i64) -> Result<i64, RangeError> { a.checked_sub(b).ok_or_else(|| overflow("-", a, b)) }
fn fib(n: i64) -> Result<i64, RangeError> { if n < 2 { return Ok(n); } add(fib(sub(n, 1)?)?, fib(sub(n, 2)?)?) }
fn main() { let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap(); match fib(n) { Ok(v) => println!("{}", v), Err(e) => { eprintln!("RangeError: {}", e.message); std::process::exit(1) } } }
