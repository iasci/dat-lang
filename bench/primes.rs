// The hand-written baseline for bench/primes.dat, built like bench/fib35.rs.
fn is_prime(n: i64) -> bool {
    if n < 2 {
        return false;
    }
    let mut d = 2;
    while d * d <= n {
        if n % d == 0 {
            return false;
        }
        d += 1;
    }
    true
}

fn main() {
    let mut count = 0i64;
    for n in 0..150_000 {
        if is_prime(n) {
            count += 1;
        }
    }
    println!("{}", count);
}
