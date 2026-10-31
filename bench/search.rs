// The hand-written baseline for bench/search.dat, built like bench/fib35.rs.
fn first_true(lo: i64, hi: i64, pred: &dyn Fn(i64) -> bool) -> i64 {
    let (mut a, mut b) = (lo, hi);
    while a < b {
        let mid = (a + b) / 2;
        if pred(mid) {
            b = mid;
        } else {
            a = mid + 1;
        }
    }
    a
}

fn main() {
    let mut total = 0i64;
    for n in 0..1_000_000i64 {
        total += first_true(0, 1_000, &|x| x * x >= n);
    }
    println!("{}", total);
}
