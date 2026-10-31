// The hand-written baseline for bench/bfs.dat, built like bench/fib35.rs.
use std::collections::VecDeque;

fn main() {
    let n = 1_000_000usize;
    let mut dist = vec![-1i64; n];
    dist[0] = 0;
    let mut queue = VecDeque::from([0usize]);
    let mut reached = 1;
    while let Some(v) = queue.pop_front() {
        for w in [(v * 2) % n, (v + 1) % n, (v * 3 + 1) % n] {
            if dist[w] == -1 {
                dist[w] = dist[v] + 1;
                reached += 1;
                queue.push_back(w);
            }
        }
    }
    println!("{} {} {}", reached, dist[n - 1], dist[12345]);
}
