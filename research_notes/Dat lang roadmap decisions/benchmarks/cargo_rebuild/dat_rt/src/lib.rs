use std::panic;
#[derive(Debug)] pub struct RangeError { pub message: String }
#[inline(never)] #[cold] pub fn overflow(op: &str, a: i64, b: i64) -> ! { panic::panic_any(RangeError { message: format!("int overflow: {} {} {}", a, op, b) }) }
#[inline(always)] pub fn add(a: i64, b: i64) -> i64 { match a.checked_add(b) { Some(v) => v, None => overflow("+", a, b) } }
#[inline(always)] pub fn sub(a: i64, b: i64) -> i64 { match a.checked_sub(b) { Some(v) => v, None => overflow("-", a, b) } }
pub fn run_main(f: impl FnOnce() + panic::UnwindSafe) { panic::set_hook(Box::new(|_| {})); if let Err(p) = panic::catch_unwind(f) { if let Some(e) = p.downcast_ref::<RangeError>() { eprintln!("RangeError: {}", e.message); } std::process::exit(1) } }
