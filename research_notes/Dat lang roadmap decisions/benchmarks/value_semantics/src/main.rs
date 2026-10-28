// Micro-benchmarks for dat-lang value-model decisions.
// Run: cargo +1.99.0 run --release -- [all|lists|maps|strings|rc|structs|rec|overflow <kind> <n>]
use std::collections::HashMap;
use std::hash::BuildHasher;
use std::hint::black_box;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ecow::{EcoString, EcoVec};
use indexmap::IndexMap;

fn fmt_ns(ns: f64) -> String {
    if ns < 1e3 {
        format!("{:.2} ns", ns)
    } else if ns < 1e6 {
        format!("{:.2} us", ns / 1e3)
    } else if ns < 1e9 {
        format!("{:.2} ms", ns / 1e6)
    } else {
        format!("{:.2} s", ns / 1e9)
    }
}

/// Median ns per call: calibrate a batch to >= 20 ms, then 7 samples (3 if one call > 200 ms).
fn bench<R>(name: &str, mut f: impl FnMut() -> R) -> f64 {
    let mut reps: u64 = 1;
    let mut first = Duration::ZERO;
    loop {
        let t = Instant::now();
        for _ in 0..reps {
            black_box(f());
        }
        let e = t.elapsed();
        if reps == 1 {
            first = e;
        }
        if e >= Duration::from_millis(20) || reps >= (1 << 30) {
            break;
        }
        reps *= 2;
    }
    let samples = if first > Duration::from_millis(200) { 3 } else { 7 };
    let mut s = Vec::with_capacity(samples);
    for _ in 0..samples {
        let t = Instant::now();
        for _ in 0..reps {
            black_box(f());
        }
        s.push(t.elapsed().as_nanos() as f64 / reps as f64);
    }
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = s[samples / 2];
    println!("{:<88} {:>12}", name, fmt_ns(med));
    med
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

// ---------------------------------------------------------------- A. lists
fn lists() {
    println!("\n== A. list<int> representations ==");
    for &n in &[1_000usize, 100_000] {
        let v: Vec<i64> = (0..n as i64).collect();
        let r = Rc::new(v.clone());
        let a = Arc::new(v.clone());
        let ev: EcoVec<i64> = v.iter().copied().collect();
        let iv: imbl::Vector<i64> = v.iter().copied().collect();
        let rv: rpds::Vector<i64> = v.iter().copied().collect();
        bench(&format!("A1 copy n={n}: Vec<i64>::clone (deep copy)"), || v.clone());
        bench(&format!("A1 copy n={n}: Rc<Vec<i64>>::clone (share)"), || r.clone());
        bench(&format!("A1 copy n={n}: Arc<Vec<i64>>::clone (share)"), || a.clone());
        bench(&format!("A1 copy n={n}: ecow::EcoVec::clone (share)"), || ev.clone());
        bench(&format!("A1 copy n={n}: imbl::Vector::clone (persistent)"), || iv.clone());
        bench(&format!("A1 copy n={n}: rpds::Vector::clone (persistent)"), || rv.clone());
    }

    const N: i64 = 1_000_000;
    println!("-- push 1M ints into an empty list");
    bench("A2 push 1M: Vec::push", || {
        let mut v = Vec::new();
        for i in 0..N {
            v.push(i)
        }
        v
    });
    bench("A2 push 1M: Rc<Vec> + Rc::make_mut on every push (unique)", || {
        let mut v: Rc<Vec<i64>> = Rc::new(Vec::new());
        for i in 0..N {
            Rc::make_mut(&mut v).push(i)
        }
        v
    });
    bench("A2 push 1M: Arc<Vec> + Arc::make_mut on every push (unique)", || {
        let mut v: Arc<Vec<i64>> = Arc::new(Vec::new());
        for i in 0..N {
            Arc::make_mut(&mut v).push(i)
        }
        v
    });
    bench("A2 push 1M: ecow::EcoVec::push", || {
        let mut v = EcoVec::new();
        for i in 0..N {
            v.push(i)
        }
        v
    });
    bench("A2 push 1M: imbl::Vector::push_back", || {
        let mut v = imbl::Vector::new();
        for i in 0..N {
            v.push_back(i)
        }
        v
    });
    bench("A2 push 1M: rpds::Vector::push_back_mut", || {
        let mut v = rpds::Vector::new();
        for i in 0..N {
            v.push_back_mut(i)
        }
        v
    });

    let n = N as usize;
    let v: Vec<i64> = (0..N).collect();
    let r = Rc::new(v.clone());
    let ev: EcoVec<i64> = v.iter().copied().collect();
    let iv: imbl::Vector<i64> = v.iter().copied().collect();
    let rv: rpds::Vector<i64> = v.iter().copied().collect();
    println!("-- read 1M ints");
    bench("A3 sum via iterator 1M: Vec", || v.iter().sum::<i64>());
    bench("A3 sum via iterator 1M: Rc<Vec>", || r.iter().sum::<i64>());
    bench("A3 sum via iterator 1M: EcoVec", || ev.iter().sum::<i64>());
    bench("A3 sum via iterator 1M: imbl::Vector", || iv.iter().sum::<i64>());
    bench("A3 sum via iterator 1M: rpds::Vector", || rv.iter().sum::<i64>());
    bench("A3 sum via xs[i] loop 1M: Vec (bounds-checked)", || {
        let mut s = 0i64;
        for i in 0..n {
            s += v[i]
        }
        s
    });
    bench("A3 sum via xs[i] loop 1M: imbl::Vector", || {
        let mut s = 0i64;
        for i in 0..n {
            s += iv[i]
        }
        s
    });
    bench("A3 sum via xs[i] loop 1M: rpds::Vector", || {
        let mut s = 0i64;
        for i in 0..n {
            s += rv[i]
        }
        s
    });

    println!("-- xs[i] += 1 over 1M (list already unique)");
    let mut v2 = v.clone();
    bench("A4 xs[i]+=1 1M: Vec", || {
        for i in 0..n {
            v2[i] += 1
        }
    });
    let mut r2 = Rc::new(v.clone());
    bench("A4 xs[i]+=1 1M: Rc<Vec>, make_mut per write (naive codegen)", || {
        for i in 0..n {
            Rc::make_mut(&mut r2)[i] += 1
        }
    });
    bench("A4 xs[i]+=1 1M: Rc<Vec>, one make_mut hoisted before loop", || {
        let m = Rc::make_mut(&mut r2);
        for i in 0..n {
            m[i] += 1
        }
    });
    let mut ev2: EcoVec<i64> = v.iter().copied().collect();
    bench("A4 xs[i]+=1 1M: EcoVec, make_mut per write", || {
        for i in 0..n {
            ev2.make_mut()[i] += 1
        }
    });
    let mut iv2: imbl::Vector<i64> = v.iter().copied().collect();
    bench("A4 xs[i]+=1 1M: imbl::Vector get_mut", || {
        for i in 0..n {
            *iv2.get_mut(i).unwrap() += 1
        }
    });
    let mut rv2: rpds::Vector<i64> = v.iter().copied().collect();
    bench("A4 xs[i]+=1 1M: rpds::Vector set_mut", || {
        for i in 0..n {
            let x = rv2[i];
            rv2.set_mut(i, x + 1);
        }
    });

    println!("-- list<list<int>> 1000x1000: copy, then change one element of the copy");
    let vv: Vec<Vec<i64>> = (0..1000).map(|_| (0..1000).collect()).collect();
    bench("A5 nested: Vec<Vec<i64>> deep clone + set", || {
        let mut c = vv.clone();
        c[500][500] = 1;
        c
    });
    let rr: Rc<Vec<Rc<Vec<i64>>>> = Rc::new((0..1000).map(|_| Rc::new((0..1000).collect())).collect());
    bench("A5 nested: Rc<Vec<Rc<Vec>>> COW (outer: 1000 Rc bumps, 1 inner copied)", || {
        let mut c = rr.clone();
        Rc::make_mut(&mut Rc::make_mut(&mut c)[500])[500] = 1;
        c
    });
    let ee: EcoVec<EcoVec<i64>> = (0..1000).map(|_| (0..1000).collect()).collect();
    bench("A5 nested: EcoVec<EcoVec> COW", || {
        let mut c = ee.clone();
        c.make_mut()[500].make_mut()[500] = 1;
        c
    });
    let ii: imbl::Vector<imbl::Vector<i64>> = (0..1000).map(|_| (0..1000).collect()).collect();
    bench("A5 nested: imbl::Vector<imbl::Vector> path copy", || {
        let mut c = ii.clone();
        c.get_mut(500).unwrap().set(500, 1);
        c
    });

    println!("-- list held in a struct field, pushed through a mut parameter (&mut)");
    struct CartRc {
        items: Rc<Vec<i64>>,
        total: i64,
    }
    struct CartVec {
        items: Vec<i64>,
        total: i64,
    }
    #[inline(never)]
    fn add_rc(c: &mut CartRc, x: i64) {
        Rc::make_mut(&mut c.items).push(x);
        c.total += x;
    }
    #[inline(never)]
    fn add_vec(c: &mut CartVec, x: i64) {
        c.items.push(x);
        c.total += x;
    }
    bench("A6 1M addItem(mut cart, x): field Vec<i64>", || {
        let mut c = CartVec { items: vec![], total: 0 };
        for i in 0..N {
            add_vec(&mut c, i)
        }
        c.total + c.items.len() as i64
    });
    bench("A6 1M addItem(mut cart, x): field Rc<Vec<i64>> + make_mut", || {
        let mut c = CartRc { items: Rc::new(vec![]), total: 0 };
        for i in 0..N {
            add_rc(&mut c, i)
        }
        c.total + c.items.len() as i64
    });

    println!("-- spec §16 grouping example: let m = byTeam.get(k, []); m.push(x); byTeam[k] = m;");
    for &(items, groups) in &[(10_000u32, 10u32), (50_000, 10)] {
        bench(&format!("A7 group {items} items/{groups} keys: get-copy-push-store, naive COW"), || {
            let mut m: IndexMap<u32, Rc<Vec<u32>>> = IndexMap::new();
            for i in 0..items {
                let key = i % groups;
                let mut members = m.get(&key).cloned().unwrap_or_default();
                Rc::make_mut(&mut members).push(i);
                m.insert(key, members);
            }
            m
        });
        bench(&format!("A7 group {items} items/{groups} keys: same code, compiler moves value out (take+restore)"), || {
            let mut m: IndexMap<u32, Option<Rc<Vec<u32>>>> = IndexMap::new();
            for i in 0..items {
                let key = i % groups;
                let mut members = m.get_mut(&key).and_then(|s| s.take()).unwrap_or_default();
                Rc::make_mut(&mut members).push(i);
                m.insert(key, Some(members));
            }
            m
        });
        bench(&format!("A7 group {items} items/{groups} keys: entry API in place"), || {
            let mut m: IndexMap<u32, Rc<Vec<u32>>> = IndexMap::new();
            for i in 0..items {
                Rc::make_mut(m.entry(i % groups).or_default()).push(i);
            }
            m
        });
    }

    println!("-- spec §07 for..of over a snapshot while the body pushes to the same list");
    let n0 = 100_000i64;
    bench("A8 build 100k + for..of with push in body: Vec, index loop, no snapshot", || {
        let mut q: Vec<i64> = (0..n0).collect();
        for i in 0..n0 as usize {
            let x = q[i];
            q.push(x * 10)
        }
        q
    });
    bench("A8 build 100k + for..of with push in body: Rc COW snapshot (1 copy on first push)", || {
        let mut q: Rc<Vec<i64>> = Rc::new((0..n0).collect());
        let snap = q.clone();
        for &x in snap.iter() {
            Rc::make_mut(&mut q).push(x * 10)
        }
        q
    });
    bench("A8 build 100k + for..of with push in body: Vec, eager snapshot clone", || {
        let mut q: Vec<i64> = (0..n0).collect();
        let snap = q.clone();
        for &x in snap.iter() {
            q.push(x * 10)
        }
        q
    });
}

// ---------------------------------------------------------------- B. maps & sets
fn map_suite<S: BuildHasher + Default + Clone>(label: &str, keys: &[u64]) {
    let n = keys.len();
    bench(&format!("B1 insert {n} u64: std HashMap + {label}"), || {
        let mut m: HashMap<u64, u64, S> = HashMap::with_hasher(S::default());
        for &k in keys {
            m.insert(k, k);
        }
        m
    });
    bench(&format!("B1 insert {n} u64: IndexMap + {label}"), || {
        let mut m: IndexMap<u64, u64, S> = IndexMap::with_hasher(S::default());
        for &k in keys {
            m.insert(k, k);
        }
        m
    });
    bench(&format!("B1 insert {n} u64: hashlink::LinkedHashMap + {label}"), || {
        let mut m: hashlink::LinkedHashMap<u64, u64, S> = hashlink::LinkedHashMap::with_hasher(S::default());
        for &k in keys {
            m.insert(k, k);
        }
        m
    });
    let hm: HashMap<u64, u64, S> = keys.iter().map(|&k| (k, k)).collect();
    let im: IndexMap<u64, u64, S> = keys.iter().map(|&k| (k, k)).collect();
    let mut lm: hashlink::LinkedHashMap<u64, u64, S> = hashlink::LinkedHashMap::with_hasher(S::default());
    for &k in keys {
        lm.insert(k, k);
    }
    bench(&format!("B2 lookup {n} hits: std HashMap + {label}"), || {
        let mut s = 0u64;
        for &k in keys {
            s = s.wrapping_add(hm[&k])
        }
        s
    });
    bench(&format!("B2 lookup {n} hits: IndexMap + {label}"), || {
        let mut s = 0u64;
        for &k in keys {
            s = s.wrapping_add(im[&k])
        }
        s
    });
    bench(&format!("B2 lookup {n} hits: LinkedHashMap + {label}"), || {
        let mut s = 0u64;
        for &k in keys {
            s = s.wrapping_add(*lm.get(&k).unwrap())
        }
        s
    });
    bench(&format!("B3 iterate {n}: std HashMap + {label}"), || hm.values().fold(0u64, |a, &b| a.wrapping_add(b)));
    bench(&format!("B3 iterate {n}: IndexMap + {label}"), || im.values().fold(0u64, |a, &b| a.wrapping_add(b)));
    bench(&format!("B3 iterate {n}: LinkedHashMap + {label}"), || lm.values().fold(0u64, |a, &b| a.wrapping_add(b)));
}

fn str_map_suite<S: BuildHasher + Default + Clone>(label: &str, words: &[String], probes: &[usize]) {
    let hm: HashMap<String, u64, S> = words.iter().enumerate().map(|(i, w)| (w.clone(), i as u64)).collect();
    let im: IndexMap<String, u64, S> = words.iter().enumerate().map(|(i, w)| (w.clone(), i as u64)).collect();
    bench(&format!("B5 {} string-key lookups (100k keys): std HashMap + {label}", probes.len()), || {
        let mut s = 0u64;
        for &p in probes {
            s = s.wrapping_add(hm[words[p].as_str()])
        }
        s
    });
    bench(&format!("B5 {} string-key lookups (100k keys): IndexMap + {label}", probes.len()), || {
        let mut s = 0u64;
        for &p in probes {
            s = s.wrapping_add(im[words[p].as_str()])
        }
        s
    });
}

fn maps() {
    println!("\n== B. insertion-ordered map/set and hashers ==");
    let keys: Vec<u64> = (0..1_000_000u64).map(splitmix64).collect();
    map_suite::<std::hash::RandomState>("SipHash-1-3 (std RandomState)", &keys);
    map_suite::<foldhash::fast::RandomState>("foldhash::fast", &keys);
    map_suite::<foldhash::quality::RandomState>("foldhash::quality", &keys);
    map_suite::<ahash::RandomState>("ahash", &keys);
    map_suite::<rustc_hash::FxBuildHasher>("FxHash (rustc-hash 2)", &keys);

    println!("-- removal from a 100k-entry map, removing 10k keys (every 10th)");
    let k100: Vec<u64> = keys[..100_000].to_vec();
    let base: IndexMap<u64, u64, foldhash::fast::RandomState> = k100.iter().map(|&k| (k, k)).collect();
    let rm: Vec<u64> = k100.iter().step_by(10).copied().collect();
    bench("B4 IndexMap 100k: clone only (baseline)", || base.clone());
    bench("B4 IndexMap 100k: clone + swap_remove 10k (O(1) each, moves last entry into the hole)", || {
        let mut m = base.clone();
        for k in &rm {
            m.swap_remove(k);
        }
        m
    });
    bench("B4 IndexMap 100k: clone + shift_remove 10k (O(n) each, keeps order)", || {
        let mut m = base.clone();
        for k in &rm {
            m.shift_remove(k);
        }
        m
    });
    bench("B4 IndexMap 100k: clone + retain (one O(n) pass, keeps order)", || {
        let mut m = base.clone();
        m.retain(|k, _| splitmix64(*k) % 10 != 0);
        m
    });
    let mut lbase: hashlink::LinkedHashMap<u64, u64, foldhash::fast::RandomState> = hashlink::LinkedHashMap::with_hasher(Default::default());
    for &k in &k100 {
        lbase.insert(k, k);
    }
    bench("B4 LinkedHashMap 100k: clone only (baseline)", || lbase.clone());
    bench("B4 LinkedHashMap 100k: clone + remove 10k (O(1), keeps order)", || {
        let mut m = lbase.clone();
        for k in &rm {
            m.remove(k);
        }
        m
    });
    let rc_base = Rc::new(base.clone());
    bench("B6 copy map 100k: Rc<IndexMap> clone (share)", || rc_base.clone());

    println!("-- string keys");
    let words: Vec<String> = (0..100_000).map(|i| format!("word{i}")).collect();
    let probes: Vec<usize> = (0..1_000_000u64).map(|i| (splitmix64(i) % 100_000) as usize).collect();
    str_map_suite::<std::hash::RandomState>("SipHash-1-3", &words, &probes);
    str_map_suite::<foldhash::fast::RandomState>("foldhash::fast", &words, &probes);
    str_map_suite::<ahash::RandomState>("ahash", &words, &probes);
    str_map_suite::<rustc_hash::FxBuildHasher>("FxHash", &words, &probes);
}

// ---------------------------------------------------------------- C. strings
fn strings() {
    use compact_str::CompactString;
    use smol_str::SmolStr;
    println!("\n== C. string representations ==");
    println!(
        "C0 size_of: String={} Box<str>={} Rc<str>={} Arc<str>={} Rc<String>={} CompactString={} SmolStr={} EcoString={} Option<EcoString>={} Option<Rc<str>>={}",
        std::mem::size_of::<String>(),
        std::mem::size_of::<Box<str>>(),
        std::mem::size_of::<Rc<str>>(),
        std::mem::size_of::<Arc<str>>(),
        std::mem::size_of::<Rc<String>>(),
        std::mem::size_of::<CompactString>(),
        std::mem::size_of::<SmolStr>(),
        std::mem::size_of::<EcoString>(),
        std::mem::size_of::<Option<EcoString>>(),
        std::mem::size_of::<Option<Rc<str>>>(),
    );
    for &len in &[5usize, 15, 16, 24, 25, 100] {
        let s: String = "x".repeat(len);
        let rc: Rc<str> = Rc::from(s.as_str());
        let ar: Arc<str> = Arc::from(s.as_str());
        let cs = CompactString::from(s.as_str());
        let ss = SmolStr::new(s.as_str());
        let es = EcoString::from(s.as_str());
        bench(&format!("C1 copy len={len}: String::clone (malloc+memcpy)"), || s.clone());
        bench(&format!("C1 copy len={len}: Rc<str>::clone"), || rc.clone());
        bench(&format!("C1 copy len={len}: Arc<str>::clone"), || ar.clone());
        bench(&format!("C1 copy len={len}: CompactString::clone (inline<=24, else heap copy)"), || cs.clone());
        bench(&format!("C1 copy len={len}: SmolStr::clone (inline<=23, else Arc)"), || ss.clone());
        bench(&format!("C1 copy len={len}: EcoString::clone (inline<=15, else refcount)"), || es.clone());
    }
    println!("-- s += \"ab\" in a loop (spec: strings are immutable values; `+=` rebinds)");
    for &n in &[10_000usize, 40_000] {
        bench(&format!("C2 append x{n}: String::push_str (unique buffer, amortized O(1))"), || {
            let mut s = String::new();
            for _ in 0..n {
                s.push_str("ab")
            }
            s
        });
        bench(&format!("C2 append x{n}: Rc<str>, new string per += (O(n^2))"), || {
            let mut s: Rc<str> = Rc::from("");
            for _ in 0..n {
                let mut t = String::with_capacity(s.len() + 2);
                t.push_str(&s);
                t.push_str("ab");
                s = Rc::from(t);
            }
            s
        });
        bench(&format!("C2 append x{n}: Rc<String> + make_mut push_str (in place when unique)"), || {
            let mut s: Rc<String> = Rc::new(String::new());
            for _ in 0..n {
                Rc::make_mut(&mut s).push_str("ab")
            }
            s
        });
        bench(&format!("C2 append x{n}: EcoString::push_str (in place when unique)"), || {
            let mut s = EcoString::new();
            for _ in 0..n {
                s.push_str("ab")
            }
            s
        });
        bench(&format!("C2 append x{n}: CompactString::push_str"), || {
            let mut s = CompactString::new("");
            for _ in 0..n {
                s.push_str("ab")
            }
            s
        });
    }
    println!("-- build a 1000-word line with `line = line + w + \" \"` while a copy of line is alive each step");
    let ws: Vec<String> = (0..1000).map(|i| format!("w{i}")).collect();
    bench("C3 1000 appends, previous value kept alive each step: EcoString (copies each time)", || {
        let mut line = EcoString::new();
        let mut keep = Vec::with_capacity(1000);
        for w in &ws {
            keep.push(line.clone());
            line.push_str(w);
            line.push_str(" ");
        }
        (line, keep)
    });
}

// ---------------------------------------------------------------- D. Rc vs Arc
fn rc_arc() {
    println!("\n== D. Rc vs Arc ==");
    let r = Rc::new(vec![1i64; 10]);
    let a = Arc::new(vec![1i64; 10]);
    bench("D1 clone+drop: Rc", || {
        let c = r.clone();
        black_box(&c);
    });
    bench("D1 clone+drop: Arc (uncontended)", || {
        let c = a.clone();
        black_box(&c);
    });
    bench("D2 1000 clones into a Vec, then drop all: Rc", || (0..1000).map(|_| r.clone()).collect::<Vec<_>>());
    bench("D2 1000 clones into a Vec, then drop all: Arc", || (0..1000).map(|_| a.clone()).collect::<Vec<_>>());
    let mut r2 = Rc::new(vec![1i64; 10]);
    let mut a2 = Arc::new(vec![1i64; 10]);
    bench("D3 make_mut on unique value + write: Rc", || {
        Rc::make_mut(&mut r2)[0] += 1;
    });
    bench("D3 make_mut on unique value + write: Arc", || {
        Arc::make_mut(&mut a2)[0] += 1;
    });
    let per_thread = 5_000_000u64;
    for &threads in &[1usize, 2, 4, 8] {
        let shared = Arc::new(0u64);
        let t = Instant::now();
        std::thread::scope(|sc| {
            for _ in 0..threads {
                let s = &shared;
                sc.spawn(move || {
                    for _ in 0..per_thread {
                        let c = Arc::clone(s);
                        black_box(&c);
                    }
                });
            }
        });
        let e = t.elapsed().as_nanos() as f64 / per_thread as f64;
        println!("{:<88} {:>12}", format!("D4 Arc clone+drop, SAME Arc from {threads} threads (wall ns per op per thread)"), fmt_ns(e));
        let t = Instant::now();
        std::thread::scope(|sc| {
            for _ in 0..threads {
                sc.spawn(move || {
                    let own = Arc::new(0u64);
                    for _ in 0..per_thread {
                        let c = Arc::clone(&own);
                        black_box(&c);
                    }
                });
            }
        });
        let e = t.elapsed().as_nanos() as f64 / per_thread as f64;
        println!("{:<88} {:>12}", format!("D4 Arc clone+drop, one Arc PER thread, {threads} threads (wall ns per op per thread)"), fmt_ns(e));
    }
}

// ---------------------------------------------------------------- E. structs
#[derive(Clone)]
#[allow(dead_code)]
struct AddrRc {
    city: Rc<str>,
    zip: Rc<str>,
}
#[derive(Clone)]
#[allow(dead_code)]
struct UserRc {
    name: Rc<str>,
    email: Option<Rc<str>>,
    active: bool,
    age: i64,
    score: f64,
    tags: Rc<Vec<Rc<str>>>,
    address: AddrRc,
}
#[derive(Clone)]
#[allow(dead_code)]
struct AddrArc {
    city: Arc<str>,
    zip: Arc<str>,
}
#[derive(Clone)]
#[allow(dead_code)]
struct UserArc {
    name: Arc<str>,
    email: Option<Arc<str>>,
    active: bool,
    age: i64,
    score: f64,
    tags: Arc<Vec<Arc<str>>>,
    address: AddrArc,
}
#[derive(Clone)]
#[allow(dead_code)]
struct AddrOwned {
    city: String,
    zip: String,
}
#[derive(Clone)]
#[allow(dead_code)]
struct UserOwned {
    name: String,
    email: Option<String>,
    active: bool,
    age: i64,
    score: f64,
    tags: Vec<String>,
    address: AddrOwned,
}

fn structs() {
    println!("\n== E. structs (User: 5 string/list fields + 3 scalars) ==");
    let u = UserRc {
        name: "Ada Lovelace".into(),
        email: Some("ada@example.com".into()),
        active: true,
        age: 36,
        score: 1.5,
        tags: Rc::new(vec!["admin".into(), "ops".into()]),
        address: AddrRc { city: "London".into(), zip: "W1".into() },
    };
    let ua = UserArc {
        name: "Ada Lovelace".into(),
        email: Some("ada@example.com".into()),
        active: true,
        age: 36,
        score: 1.5,
        tags: Arc::new(vec!["admin".into(), "ops".into()]),
        address: AddrArc { city: "London".into(), zip: "W1".into() },
    };
    let uo = UserOwned {
        name: "Ada Lovelace".into(),
        email: Some("ada@example.com".into()),
        active: true,
        age: 36,
        score: 1.5,
        tags: vec!["admin".into(), "ops".into()],
        address: AddrOwned { city: "London".into(), zip: "W1".into() },
    };
    let ru = Rc::new(u.clone());
    bench("E1 copy one User: unboxed struct, Rc fields (5 refcount bumps)", || u.clone());
    bench("E1 copy one User: unboxed struct, Arc fields (5 atomic bumps)", || ua.clone());
    bench("E1 copy one User: Rc<User> (1 refcount bump)", || ru.clone());
    bench("E1 copy one User: owned String/Vec fields (deep, 6 mallocs)", || uo.clone());
    let lu: Rc<Vec<UserRc>> = Rc::new((0..10_000).map(|_| u.clone()).collect());
    let lb: Rc<Vec<Rc<UserRc>>> = Rc::new((0..10_000).map(|_| Rc::new(u.clone())).collect());
    let lo: Vec<UserOwned> = (0..10_000).map(|_| uo.clone()).collect();
    bench("E2 list<User> 10k: copy, then set users[5].age: Rc<Vec<User>> (10k struct clones)", || {
        let mut c = lu.clone();
        Rc::make_mut(&mut c)[5].age += 1;
        c
    });
    bench("E2 list<User> 10k: copy, then set users[5].age: Rc<Vec<Rc<User>>> (10k bumps + 1 struct)", || {
        let mut c = lb.clone();
        Rc::make_mut(&mut Rc::make_mut(&mut c)[5]).age += 1;
        c
    });
    bench("E2 list<User> 10k: copy, then set users[5].age: Vec<User> owned (deep)", || {
        let mut c = lo.clone();
        c[5].age += 1;
        c
    });
    let mut lu2: Rc<Vec<UserRc>> = Rc::new((0..10_000).map(|_| u.clone()).collect());
    bench("E3 for (mut u of users) u.age += 1, 10k: Rc<Vec<User>> unique, iter_mut", || {
        for x in Rc::make_mut(&mut lu2).iter_mut() {
            x.age += 1
        }
    });
    let mut lb2: Rc<Vec<Rc<UserRc>>> = Rc::new((0..10_000).map(|_| Rc::new(u.clone())).collect());
    bench("E3 for (mut u of users) u.age += 1, 10k: Rc<Vec<Rc<User>>> unique, make_mut each", || {
        for x in Rc::make_mut(&mut lb2).iter_mut() {
            Rc::make_mut(x).age += 1
        }
    });
}

// ---------------------------------------------------------------- F. recursive types
#[derive(Clone)]
struct NodeBox {
    value: i64,
    next: Option<Box<NodeBox>>,
}
#[derive(Clone)]
struct NodeRc {
    value: i64,
    next: Option<Rc<NodeRc>>,
}
fn build_box(n: usize) -> Option<Box<NodeBox>> {
    let mut head: Option<Box<NodeBox>> = None;
    for i in 0..n {
        head = Some(Box::new(NodeBox { value: i as i64, next: head }));
    }
    head
}
fn build_rc(n: usize) -> Option<Rc<NodeRc>> {
    let mut head: Option<Rc<NodeRc>> = None;
    for i in 0..n {
        head = Some(Rc::new(NodeRc { value: i as i64, next: head }));
    }
    head
}
fn rec() {
    println!("\n== F. recursive struct ListNode {{ value, next: ListNode? }} ==");
    let lb = build_box(10_000);
    let lr = build_rc(10_000);
    bench("F1 copy 10k-node list: Option<Box<Node>> (deep, recursive clone)", || lb.clone());
    bench("F1 copy 10k-node list: Option<Rc<Node>> (share)", || lr.clone());
    bench("F1 copy 10k-node list + set head.value: Option<Rc<Node>> (path copy of 1 node)", || {
        let mut c = lr.clone();
        Rc::make_mut(c.as_mut().unwrap()).value = 7;
        c
    });
    bench("F1 copy 10k-node list + set head.value: Option<Box<Node>> (deep)", || {
        let mut c = lb.clone();
        c.as_mut().unwrap().value = 7;
        c
    });
    // keep values alive to avoid unused warnings
    black_box((lb.map(|b| b.value), lr.map(|r| r.value)));
}

fn overflow(kind: &str, n: usize) {
    match kind {
        "box-drop" => {
            let l = build_box(n);
            drop(l);
        }
        "box-clone" => {
            let l = build_box(n);
            let c = l.clone();
            std::mem::forget(c);
            std::mem::forget(l);
        }
        "rc-drop" => {
            let l = build_rc(n);
            drop(l);
        }
        _ => panic!("unknown kind"),
    }
    println!("ok {kind} {n}");
}


fn extra() {
    println!("\n== X. loop shapes over a unique 1M list ==");
    let n = 1_000_000usize;
    let mut v: Vec<i64> = (0..n as i64).collect();
    let mut r: Rc<Vec<i64>> = Rc::new((0..n as i64).collect());
    let mut a: Arc<Vec<i64>> = Arc::new((0..n as i64).collect());
    bench("X1 Vec: for i in 0..len { xs[i] += 1 }", || { let m = &mut v; for i in 0..m.len() { m[i] += 1 } });
    bench("X1 Vec: iter_mut", || { for x in v.iter_mut() { *x += 1 } });
    bench("X1 Rc<Vec>: make_mut once, then for i in 0..len", || { let m = Rc::make_mut(&mut r); for i in 0..m.len() { m[i] += 1 } });
    bench("X1 Rc<Vec>: make_mut once, then iter_mut (for (mut x of xs))", || { for x in Rc::make_mut(&mut r).iter_mut() { *x += 1 } });
    bench("X1 Rc<Vec>: make_mut per element write", || { for i in 0..n { Rc::make_mut(&mut r)[i] += 1 } });
    bench("X1 Arc<Vec>: make_mut per element write", || { for i in 0..n { Arc::make_mut(&mut a)[i] += 1 } });
    bench("X1 Rc<Vec>: Rc::get_mut().unwrap() per element write", || { for i in 0..n { Rc::get_mut(&mut r).unwrap()[i] += 1 } });
}


#[inline(never)]
fn append_borrowed(xs: &Rc<Vec<i64>>, x: i64) -> Rc<Vec<i64>> {
    // dat: function append(xs: list<int>, x: int): list<int> { let r = xs; r.push(x); return r; }
    let mut r = xs.clone(); // param is &T, so `let r = xs` is a refcount bump
    Rc::make_mut(&mut r).push(x); // caller still holds xs -> full copy
    r
}
#[inline(never)]
fn append_owned(xs: Rc<Vec<i64>>, x: i64) -> Rc<Vec<i64>> {
    let mut r = xs; // owned param, moved in by the caller's last use
    Rc::make_mut(&mut r).push(x); // unique -> in place
    r
}
fn fbip() {
    println!("\n== G. spec §06 style: acc = append(acc, i) (return a new value instead of mut) ==");
    for &n in &[10_000i64, 40_000] {
        bench(&format!("G1 acc = append(acc, i) x{n}: param as &T, caller keeps acc alive (clone)"), || {
            let mut acc: Rc<Vec<i64>> = Rc::new(Vec::new());
            for i in 0..n {
                acc = append_borrowed(&acc, i);
            }
            acc
        });
        bench(&format!("G1 acc = append(acc, i) x{n}: param owned, caller moves acc (last use)"), || {
            let mut acc: Rc<Vec<i64>> = Rc::new(Vec::new());
            for i in 0..n {
                acc = append_owned(acc, i);
            }
            acc
        });
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let what = args.get(1).map(|s| s.as_str()).unwrap_or("all");
    println!("rustc 1.99.0, target {} ", std::env::consts::ARCH);
    match what {
        "lists" => lists(),
        "maps" => maps(),
        "strings" => strings(),
        "rc" => rc_arc(),
        "structs" => structs(),
        "rec" => rec(),
        "extra" => extra(),
        "fbip" => fbip(),
        "overflow" => overflow(&args[2], args[3].parse().unwrap()),
        _ => {
            lists();
            maps();
            strings();
            rc_arc();
            structs();
            rec();
        }
    }
}
