// The hand-written baseline for bench/append.dat, built like bench/fib35.rs.
fn append(mut xs: Vec<i64>, x: i64) -> Vec<i64> {
    xs.push(x);
    xs
}

fn main() {
    let mut items: Vec<i64> = Vec::new();
    for i in 0..2_000_000i64 {
        items = append(items, i * 2);
    }
    let mut sum = 0i64;
    for x in &items {
        sum += x;
    }
    println!("{} {}", items.len(), sum);
}
