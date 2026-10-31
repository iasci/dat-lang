// The hand-written baseline for bench/wordcount.dat, built like bench/fib35.rs.
use std::collections::HashMap;

fn main() {
    let mut counts: HashMap<String, i64> = HashMap::new();
    for i in 0..500_000i64 {
        let word = format!("w{}", (i * 7919) % 5000);
        *counts.entry(word).or_insert(0) += 1;
    }
    println!("{} {} {}", counts.len(), counts["w0"], counts["w4999"]);
}
