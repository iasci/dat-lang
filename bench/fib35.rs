// The hand-written baseline for bench/fib35.dat: the same algorithm, built
// with overflow checks on (`rustc -C opt-level=3 -C overflow-checks=on`).
fn fib(n: i64) -> i64 {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

fn main() {
    println!("{}", fib(35));
}
