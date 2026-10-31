// The hand-written baseline for bench/chains.dat, built like bench/fib35.rs.
fn main() {
    let nums: Vec<i64> = (0..1_000_000).collect();
    let mut total = 0i64;
    for round in 0..5 {
        let mapped: Vec<i64> = nums.iter().map(|x| x * 3 + round).collect();
        let kept: Vec<i64> = mapped.into_iter().filter(|x| x % 2 == 0).collect();
        total += kept.iter().fold(0, |acc, x| acc + x);
    }
    println!(
        "{} {} {}",
        total,
        if nums.iter().any(|&x| x == 999_999) { "true" } else { "false" },
        nums.iter().filter(|&&x| x % 7 == 0).count()
    );
}
