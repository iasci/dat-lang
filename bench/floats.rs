// The hand-written baseline for bench/floats.dat, built like bench/fib35.rs.
fn escapes(cr: f64, ci: f64, limit: i64) -> i64 {
    let mut x = 0.0f64;
    let mut y = 0.0f64;
    let mut i = 0;
    while i < limit {
        let xx = x * x;
        let yy = y * y;
        if xx + yy > 4.0 {
            return i;
        }
        y = 2.0 * x * y + ci;
        x = xx - yy + cr;
        i += 1;
    }
    limit
}

fn main() {
    let size = 400i64;
    let mut inside = 0i64;
    let mut steps = 0i64;
    for row in 0..size {
        let ci = row as f64 / size as f64 * 2.4 - 1.2;
        for col in 0..size {
            let cr = col as f64 / size as f64 * 3.0 - 2.1;
            let n = escapes(cr, ci, 120);
            steps += n;
            if n == 120 {
                inside += 1;
            }
        }
    }
    println!("{} {}", inside, steps);
}
