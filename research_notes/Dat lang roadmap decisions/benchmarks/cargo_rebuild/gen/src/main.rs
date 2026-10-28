fn fib(n: i64) -> i64 { if n < 2 { n } else { dat_rt::add(fib(dat_rt::sub(n, 1)), fib(dat_rt::sub(n, 2))) } }
fn main() { dat_rt::run_main(|| println!("{}", fib(30))); }
