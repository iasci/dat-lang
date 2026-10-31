// The hand-written baseline for bench/groups.dat, built like bench/fib35.rs.
use std::collections::HashMap;

fn main() {
    let mut by_team: HashMap<i64, Vec<i64>> = HashMap::new();
    for i in 0..1_000_000i64 {
        let team = (i * 31) % 1000;
        by_team.entry(team).or_default().push(i);
    }
    let mut total = 0;
    for members in by_team.values() {
        total += members.len();
    }
    println!("{} {} {} {}", by_team.len(), total, by_team[&7].len(), by_team[&7][0]);
}
