// The hand-written baseline for bench/fib35.dat: the same algorithm, built
// with overflow checks on and functions aligned as in generated crates
// (`rustc -C opt-level=3 -C overflow-checks=on -C llvm-args=-align-all-functions=6`).
// Unaligned, its speed depends on where the linker puts `fib`.
fn fib(n: i64) -> i64 {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

fn main() {
    println!("{}", fib(35));
}
