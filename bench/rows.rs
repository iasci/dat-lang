// The hand-written baseline for bench/rows.dat, built like bench/fib35.rs.
fn row(i: i64) -> String {
    format!("{},item{},{}", i, i % 100, i * 3)
}

fn main() {
    let mut out = String::new();
    let mut small = 0i64;
    for i in 0..200_000 {
        let r = row(i);
        if r.as_str() < "5" {
            small += 1;
        }
        out.push_str(&r);
        out.push('\n');
    }
    println!("{}", small);
    println!("{}", if out.is_empty() { "empty" } else { "built" });
}
