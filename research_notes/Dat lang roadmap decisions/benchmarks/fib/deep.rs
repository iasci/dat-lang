// deep recursion: sum(n) = n + sum(n-1); with and without the stack-pointer guard
use std::panic; use std::sync::atomic::{AtomicUsize, Ordering};
#[derive(Debug)] pub struct StackOverflowError;
static STACK_LIMIT: AtomicUsize = AtomicUsize::new(0);
#[inline(always)] fn check_stack() { let x = 0u8; if (&x as *const u8 as usize) < STACK_LIMIT.load(Ordering::Relaxed) { too_deep() } }
#[inline(never)] #[cold] fn too_deep() -> ! { panic::panic_any(StackOverflowError) }
#[inline(never)] fn sum_guarded(n: i64) -> i64 { check_stack(); if n == 0 { 0 } else { n.checked_add(sum_guarded(n - 1)).unwrap() } }
#[inline(never)] fn sum_plain(n: i64) -> i64 { if n == 0 { 0 } else { n.checked_add(sum_plain(n - 1)).unwrap() } }
fn main() { panic::set_hook(Box::new(|_| {}));
  let base = 0u8; STACK_LIMIT.store((&base as *const u8 as usize).saturating_sub(7 * 1024 * 1024), Ordering::Relaxed);
  let mode = std::env::args().nth(1).unwrap(); let n: i64 = std::env::args().nth(2).unwrap().parse().unwrap();
  let r = panic::catch_unwind(|| if mode == "guarded" { sum_guarded(n) } else { sum_plain(n) });
  match r { Ok(v) => println!("{}", v), Err(p) => { if p.downcast_ref::<StackOverflowError>().is_some() { println!("caught StackOverflowError"); } std::process::exit(1) } } }
