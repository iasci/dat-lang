// The hand-written baseline for bench/sortcmp.dat, built like bench/fib35.rs.
fn main() {
    let mut xs: Vec<i64> = Vec::new();
    let mut x: i64 = 42;
    for _ in 0..500_000 {
        x = (x * 1103515245 + 12345) % 2147483648;
        xs.push(x % 1_000_000);
    }
    xs.sort_by(|a, b| b.cmp(a));
    let mut by_last = xs.clone();
    by_last.sort_by_key(|v| v % 1000);
    println!("{} {} {} {} {}", xs[0], xs[xs.len() - 1], xs[250_000], by_last[0], by_last[499_999]);
}
