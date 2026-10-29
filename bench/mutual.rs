// The hand-written baseline for bench/mutual.dat, built like bench/fib35.rs.
fn female(n: i64) -> i64 {
    if n == 0 {
        return 1;
    }
    n - male(female(n - 1))
}

fn male(n: i64) -> i64 {
    if n == 0 {
        return 0;
    }
    n - female(male(n - 1))
}

fn main() {
    let mut total = 0i64;
    for i in 0..85 {
        total = total + female(i) + male(i);
    }
    println!("{}", total);
}
