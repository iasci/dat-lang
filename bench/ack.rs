// The hand-written baseline for bench/ack.dat, built like bench/fib35.rs.
fn ack(m: i64, n: i64) -> i64 {
    if m == 0 {
        return n + 1;
    }
    if n == 0 {
        return ack(m - 1, 1);
    }
    ack(m - 1, ack(m, n - 1))
}

fn main() {
    println!("{}", ack(3, 10));
}
