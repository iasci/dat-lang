// Micro-benchmarks for dat-lang error-model decisions.
// Method: each case runs 9 times, the fastest run is reported (min-of-N).
use std::cell::Cell;
use std::hint::black_box;
use std::panic::{self, AssertUnwindSafe};
use std::time::Instant;

#[derive(Debug)]
pub struct RangeError {
    pub message: String,
}
#[derive(Debug)]
pub struct StackOverflowError;

#[cold]
#[inline(never)]
fn throw_range_resume() -> ! {
    panic::resume_unwind(Box::new(RangeError { message: String::from("overflow") }))
}
#[cold]
#[inline(never)]
fn throw_range_panic_any() -> ! {
    panic::panic_any(RangeError { message: String::from("overflow") })
}
#[cold]
#[inline(never)]
fn throw_so() -> ! {
    panic::resume_unwind(Box::new(StackOverflowError))
}
#[cold]
#[inline(never)]
fn err_box() -> Box<RangeError> {
    Box::new(RangeError { message: String::from("overflow") })
}
#[cold]
#[inline(never)]
fn err_big() -> RangeError {
    RangeError { message: String::from("overflow") }
}

fn bench<F: FnMut() -> u64>(name: &str, ops: u64, mut f: F) -> f64 {
    let mut best = f64::INFINITY;
    let mut sink = 0u64;
    for _ in 0..9 {
        let t = Instant::now();
        sink ^= f();
        let el = t.elapsed().as_secs_f64();
        if el < best {
            best = el;
        }
    }
    black_box(sink);
    let ns = best * 1e9 / ops as f64;
    println!("{name:<62} {:>9.3} ms {:>10.3} ns/op", best * 1e3, ns);
    ns
}

// ---------------------------------------------------------------- recursion guards
const LIMIT: u32 = 1_000_000;
thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    static SP_LIMIT: Cell<usize> = const { Cell::new(0) };
    static SHADOW: Cell<usize> = const { Cell::new(0) };
}
static mut SHADOW_STACK: [u32; 1 << 20] = [0; 1 << 20];

#[inline(never)]
fn fib_plain(n: u64) -> u64 {
    if n < 2 { n } else { fib_plain(n - 1).wrapping_add(fib_plain(n - 2)) }
}

#[inline(always)]
fn depth_enter() {
    DEPTH.with(|d| {
        let v = d.get();
        if v >= LIMIT { throw_so() }
        d.set(v + 1);
    })
}
#[inline(always)]
fn depth_leave() {
    DEPTH.with(|d| d.set(d.get() - 1));
}
#[inline(never)]
fn fib_depth_tls(n: u64) -> u64 {
    depth_enter();
    let r = if n < 2 { n } else { fib_depth_tls(n - 1).wrapping_add(fib_depth_tls(n - 2)) };
    depth_leave();
    r
}

#[inline(never)]
fn fib_depth_arg(n: u64, depth: u32) -> u64 {
    if depth >= LIMIT { throw_so() }
    if n < 2 { n } else { fib_depth_arg(n - 1, depth + 1).wrapping_add(fib_depth_arg(n - 2, depth + 1)) }
}

#[inline(always)]
fn sp_check_local() {
    let marker = 0u8;
    let sp = &marker as *const u8 as usize; // safe: address of a local approximates SP
    if sp < SP_LIMIT.with(|l| l.get()) { throw_so() }
}
#[inline(never)]
fn fib_sp_local(n: u64) -> u64 {
    sp_check_local();
    if n < 2 { n } else { fib_sp_local(n - 1).wrapping_add(fib_sp_local(n - 2)) }
}

#[inline(always)]
fn read_sp() -> usize {
    let sp: usize;
    #[cfg(target_arch = "aarch64")]
    unsafe { core::arch::asm!("mov {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags)); }
    #[cfg(target_arch = "x86_64")]
    unsafe { core::arch::asm!("mov {}, rsp", out(reg) sp, options(nomem, nostack, preserves_flags)); }
    sp
}
#[inline(never)]
fn fib_sp_asm(n: u64) -> u64 {
    if read_sp() < SP_LIMIT.with(|l| l.get()) { throw_so() }
    if n < 2 { n } else { fib_sp_asm(n - 1).wrapping_add(fib_sp_asm(n - 2)) }
}

#[inline(never)]
fn fib_stacker_remaining(n: u64) -> u64 {
    if stacker::remaining_stack().map_or(false, |r| r < 128 * 1024) { throw_so() }
    if n < 2 { n } else { fib_stacker_remaining(n - 1).wrapping_add(fib_stacker_remaining(n - 2)) }
}

#[inline(never)]
fn fib_stacker_grow(n: u64) -> u64 {
    stacker::maybe_grow(128 * 1024, 1024 * 1024, || {
        if n < 2 { n } else { fib_stacker_grow(n - 1).wrapping_add(fib_stacker_grow(n - 2)) }
    })
}

// depth guard restored by a Drop guard (needed if the catch site does not restore depth)
struct DepthGuard;
impl Drop for DepthGuard {
    #[inline(always)]
    fn drop(&mut self) { depth_leave(); }
}
#[inline(never)]
fn fib_depth_drop(n: u64) -> u64 {
    depth_enter();
    let _g = DepthGuard;
    if n < 2 { n } else { fib_depth_drop(n - 1).wrapping_add(fib_depth_drop(n - 2)) }
}

// shadow stack: push a function id per call (depth guard + dat stack trace in one)
#[inline(always)]
fn shadow_push(id: u32) {
    SHADOW.with(|s| {
        let i = s.get();
        if i >= (1 << 20) { throw_so() }
        unsafe { *(&raw mut SHADOW_STACK).cast::<u32>().add(i) = id; }
        s.set(i + 1);
    })
}
#[inline(always)]
fn shadow_pop() { SHADOW.with(|s| s.set(s.get() - 1)); }
#[inline(never)]
fn fib_shadow(n: u64) -> u64 {
    shadow_push(7);
    let r = if n < 2 { n } else { fib_shadow(n - 1).wrapping_add(fib_shadow(n - 2)) };
    shadow_pop();
    r
}

// tree traversal (more work per call than fib)
struct Node { l: Option<Box<Node>>, r: Option<Box<Node>>, v: u64 }
fn build(d: u32, v: &mut u64) -> Option<Box<Node>> {
    if d == 0 { return None; }
    *v = v.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let me = *v >> 33;
    Some(Box::new(Node { l: build(d - 1, v), r: build(d - 1, v), v: me }))
}
#[inline(never)]
fn tree_plain(n: &Option<Box<Node>>) -> u64 {
    match n { None => 0, Some(b) => b.v.wrapping_add(tree_plain(&b.l)).wrapping_add(tree_plain(&b.r)) }
}
#[inline(never)]
fn tree_depth_tls(n: &Option<Box<Node>>) -> u64 {
    depth_enter();
    let r = match n { None => 0, Some(b) => b.v.wrapping_add(tree_depth_tls(&b.l)).wrapping_add(tree_depth_tls(&b.r)) };
    depth_leave();
    r
}
#[inline(never)]
fn tree_sp_asm(n: &Option<Box<Node>>) -> u64 {
    if read_sp() < SP_LIMIT.with(|l| l.get()) { throw_so() }
    match n { None => 0, Some(b) => b.v.wrapping_add(tree_sp_asm(&b.l)).wrapping_add(tree_sp_asm(&b.r)) }
}
#[inline(never)]
fn tree_stacker_grow(n: &Option<Box<Node>>) -> u64 {
    stacker::maybe_grow(128 * 1024, 1024 * 1024, || match n {
        None => 0,
        Some(b) => b.v.wrapping_add(tree_stacker_grow(&b.l)).wrapping_add(tree_stacker_grow(&b.r)),
    })
}

// ---------------------------------------------------------------- Result vs unwind on the happy path
#[inline(never)]
fn fib_chk_unwind(n: i64) -> i64 {
    if n < 2 { return n; }
    match fib_chk_unwind(n - 1).checked_add(fib_chk_unwind(n - 2)) { Some(v) => v, None => throw_range_resume() }
}
#[inline(never)]
fn fib_chk_result_box(n: i64) -> Result<i64, Box<RangeError>> {
    if n < 2 { return Ok(n); }
    let a = fib_chk_result_box(n - 1)?;
    let b = fib_chk_result_box(n - 2)?;
    a.checked_add(b).ok_or_else(err_box)
}
#[inline(never)]
fn fib_chk_result_big(n: i64) -> Result<i64, RangeError> {
    if n < 2 { return Ok(n); }
    let a = fib_chk_result_big(n - 1)?;
    let b = fib_chk_result_big(n - 2)?;
    a.checked_add(b).ok_or_else(err_big)
}
#[inline(never)]
fn fib_wrap(n: i64) -> i64 {
    if n < 2 { n } else { fib_wrap(n - 1).wrapping_add(fib_wrap(n - 2)) }
}

// struct-returning function chain: the Ok payload is larger than 2 registers
#[derive(Clone, Copy)]
pub struct P3 { x: i64, y: i64, z: i64 }
#[inline(never)]
fn p3_unwind(p: P3, k: i64) -> P3 {
    let x = p.x.checked_add(k).unwrap_or_else(|| throw_range_resume());
    P3 { x, y: p.y ^ k, z: p.z.wrapping_add(1) }
}
#[inline(never)]
fn p3_result(p: P3, k: i64) -> Result<P3, Box<RangeError>> {
    let x = p.x.checked_add(k).ok_or_else(err_box)?;
    Ok(P3 { x, y: p.y ^ k, z: p.z.wrapping_add(1) })
}

// ---------------------------------------------------------------- throw cost
#[inline(never)]
fn deep_throw(d: u32) -> u64 {
    if d == 0 { throw_range_resume() }
    let s = String::from("x"); // a value with a destructor in every frame -> a landing pad
    black_box(&s);
    let r = deep_throw(d - 1);
    black_box(&s);
    r + 1
}
#[inline(never)]
fn deep_err(d: u32) -> Result<u64, Box<RangeError>> {
    if d == 0 { return Err(err_box()); }
    let s = String::from("x");
    black_box(&s);
    let r = deep_err(d - 1)?;
    black_box(&s);
    Ok(r + 1)
}

#[inline(never)]
fn deep_throw_nodrop(d: u32) -> u64 {
    if d == 0 { throw_range_resume() }
    let r = deep_throw_nodrop(d - 1);
    black_box(r) + 1
}
#[inline(never)]
fn tree_depth_arg(n: &Option<Box<Node>>, depth: u32) -> u64 {
    if depth >= LIMIT { throw_so() }
    match n { None => 0, Some(b) => b.v.wrapping_add(tree_depth_arg(&b.l, depth + 1)).wrapping_add(tree_depth_arg(&b.r, depth + 1)) }
}
static SP_LIMIT_GLOBAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[inline(never)]
fn tree_sp_global(n: &Option<Box<Node>>) -> u64 {
    if read_sp() < SP_LIMIT_GLOBAL.load(std::sync::atomic::Ordering::Relaxed) { throw_so() }
    match n { None => 0, Some(b) => b.v.wrapping_add(tree_sp_global(&b.l)).wrapping_add(tree_sp_global(&b.r)) }
}
#[inline(never)]
fn fib_sp_global(n: u64) -> u64 {
    if read_sp() < SP_LIMIT_GLOBAL.load(std::sync::atomic::Ordering::Relaxed) { throw_so() }
    if n < 2 { n } else { fib_sp_global(n - 1).wrapping_add(fib_sp_global(n - 2)) }
}
#[inline(never)]
fn fib_chk_result_box_arg(n: i64, depth: u32) -> Result<i64, Box<RangeError>> {
    if depth >= LIMIT { throw_so() }
    if n < 2 { return Ok(n); }
    let a = fib_chk_result_box_arg(n - 1, depth + 1)?;
    let b = fib_chk_result_box_arg(n - 2, depth + 1)?;
    a.checked_add(b).ok_or_else(err_box)
}
#[inline(never)]
fn fib_chk_unwind_arg(n: i64, depth: u32) -> i64 {
    if depth >= LIMIT { throw_so() }
    if n < 2 { return n; }
    match fib_chk_unwind_arg(n - 1, depth + 1).checked_add(fib_chk_unwind_arg(n - 2, depth + 1)) { Some(v) => v, None => throw_range_resume() }
}

// ---------------------------------------------------------------- overflow-check cost on integer kernels
#[inline(never)]
fn sum_wrap(xs: &[i64]) -> i64 { xs.iter().fold(0i64, |a, &x| a.wrapping_add(x)) }
#[inline(never)]
fn sum_chk(xs: &[i64]) -> i64 {
    let mut a = 0i64;
    for &x in xs { a = match a.checked_add(x) { Some(v) => v, None => throw_range_resume() }; }
    a
}
#[inline(never)]
fn matmul_wrap(a: &[i64], b: &[i64], c: &mut [i64], n: usize) {
    for i in 0..n { for k in 0..n { let aik = a[i * n + k]; for j in 0..n {
        c[i * n + j] = c[i * n + j].wrapping_add(aik.wrapping_mul(b[k * n + j]));
    } } }
}
#[inline(never)]
fn matmul_chk(a: &[i64], b: &[i64], c: &mut [i64], n: usize) {
    for i in 0..n { for k in 0..n { let aik = a[i * n + k]; for j in 0..n {
        let m = aik.checked_mul(b[k * n + j]).unwrap_or_else(|| throw_range_resume());
        c[i * n + j] = c[i * n + j].checked_add(m).unwrap_or_else(|| throw_range_resume());
    } } }
}
#[inline(never)]
fn collatz_wrap(limit: i64) -> i64 {
    let mut total = 0i64;
    for s in 1..limit { let mut n = s; while n != 1 { n = if n % 2 == 0 { n / 2 } else { n.wrapping_mul(3).wrapping_add(1) }; total = total.wrapping_add(1); } }
    total
}
#[inline(never)]
fn collatz_chk(limit: i64) -> i64 {
    let mut total = 0i64;
    for s in 1..limit { let mut n = s; while n != 1 {
        n = if n % 2 == 0 { n / 2 } else { n.checked_mul(3).and_then(|v| v.checked_add(1)).unwrap_or_else(|| throw_range_resume()) };
        total = total.checked_add(1).unwrap_or_else(|| throw_range_resume());
    } }
    total
}
#[inline(never)]
fn sieve_wrap(n: usize) -> usize {
    let mut is = vec![true; n + 1]; let mut i = 2usize;
    while i.wrapping_mul(i) <= n { if is[i] { let mut j = i.wrapping_mul(i); while j <= n { is[j] = false; j = j.wrapping_add(i); } } i = i.wrapping_add(1); }
    is.iter().filter(|&&b| b).count()
}
#[inline(never)]
fn sieve_chk(n: usize) -> usize {
    let mut is = vec![true; n + 1]; let mut i = 2usize;
    while i.checked_mul(i).unwrap_or_else(|| throw_range_resume()) <= n {
        if is[i] { let mut j = i.checked_mul(i).unwrap_or_else(|| throw_range_resume()); while j <= n { is[j] = false; j = j.checked_add(i).unwrap_or_else(|| throw_range_resume()); } }
        i = i.checked_add(1).unwrap_or_else(|| throw_range_resume());
    }
    is.iter().filter(|&&b| b).count()
}
#[inline(never)]
fn hash_wrap(xs: &[i64]) -> i64 { let mut h = 1469598103934665603i64; for &x in xs { h = (h ^ x).wrapping_mul(1099511628211); } h }
#[inline(never)]
fn poly_chk(xs: &[i64]) -> i64 { // polynomial evaluation mod p, the checked version that cannot overflow
    let p = 1_000_000_007i64; let mut h = 0i64;
    for &x in xs { h = h.checked_mul(31).and_then(|v| v.checked_add(x)).unwrap_or_else(|| throw_range_resume()) % p; } h
}
#[inline(never)]
fn poly_wrap(xs: &[i64]) -> i64 {
    let p = 1_000_000_007i64; let mut h = 0i64;
    for &x in xs { h = h.wrapping_mul(31).wrapping_add(x) % p; } h
}

#[inline(never)]
fn work(x: u64) -> u64 { x.wrapping_mul(31).wrapping_add(7) }

fn run() {
    // SP limit for this thread: thread stack is 512 MiB, leave 1 MiB reserve
    let base = read_sp();
    SP_LIMIT.with(|l| l.set(base - (512 << 20) + (1 << 20)));

    println!("== recursion guards, fib(32) = 7.05M calls (worst case: almost no work per call)");
    let calls = 7_049_155u64;
    let n = 32;
    let b0 = bench("fib plain (no guard)", calls, || fib_plain(black_box(n)));
    let r = |x: f64| println!("{:>62}  -> +{:.2} ns/call, {:+.1}%", "", x - b0, (x / b0 - 1.0) * 100.0);
    r(bench("fib + depth counter in thread_local (inc/dec)", calls, || fib_depth_tls(black_box(n))));
    r(bench("fib + depth counter, Drop guard (dec on unwind)", calls, || fib_depth_drop(black_box(n))));
    r(bench("fib + depth passed as hidden argument", calls, || fib_depth_arg(black_box(n), 0)));
    r(bench("fib + SP check (address of local vs TLS limit)", calls, || fib_sp_local(black_box(n))));
    r(bench("fib + SP check (inline asm SP vs TLS limit)", calls, || fib_sp_asm(black_box(n))));
    r(bench("fib + shadow stack push/pop (TLS index)", calls, || fib_shadow(black_box(n))));
    r(bench("fib + stacker::remaining_stack() check", calls, || fib_stacker_remaining(black_box(n))));
    r(bench("fib + stacker::maybe_grow (heap stack segments)", calls, || fib_stacker_grow(black_box(n))));

    println!("== recursion guards, binary tree sum, 2^22-1 nodes (8.4M calls incl. leaves)");
    let mut seed = 42;
    let tree = build(22, &mut seed);
    let tcalls = (1u64 << 23) - 1;
    let t0 = bench("tree plain", tcalls, || tree_plain(black_box(&tree)));
    let rt = |x: f64| println!("{:>62}  -> +{:.2} ns/call, {:+.1}%", "", x - t0, (x / t0 - 1.0) * 100.0);
    rt(bench("tree + depth counter TLS", tcalls, || tree_depth_tls(black_box(&tree))));
    rt(bench("tree + SP check asm", tcalls, || tree_sp_asm(black_box(&tree))));
    rt(bench("tree + depth as hidden argument", tcalls, || tree_depth_arg(black_box(&tree), 0)));
    SP_LIMIT_GLOBAL.store(base - (512 << 20) + (1 << 20), std::sync::atomic::Ordering::Relaxed);
    rt(bench("tree + SP check asm vs global (non-TLS) limit", tcalls, || tree_sp_global(black_box(&tree))));
    r(bench("fib + SP check asm vs global (non-TLS) limit", calls, || fib_sp_global(black_box(n))));
    rt(bench("tree + stacker::maybe_grow", tcalls, || tree_stacker_grow(black_box(&tree))));

    println!("== happy path: checked fib(32), unwinding vs Result");
    let w0 = bench("fib wrapping (no check, plain i64 return)", calls, || fib_wrap(black_box(32)) as u64);
    let rw = |x: f64| println!("{:>62}  -> +{:.2} ns/call, {:+.1}% vs wrapping", "", x - w0, (x / w0 - 1.0) * 100.0);
    rw(bench("fib checked_add, unwind on overflow (returns i64)", calls, || fib_chk_unwind(black_box(32)) as u64));
    rw(bench("fib checked_add, Result<i64, Box<E>> (16 B, 2 regs)", calls, || fib_chk_result_box(black_box(32)).unwrap() as u64));
    rw(bench("fib checked_add, Result<i64, E{String}> (in memory)", calls, || fib_chk_result_big(black_box(32)).unwrap() as u64));
    rw(bench("fib checked, unwind + depth arg (realistic combo)", calls, || fib_chk_unwind_arg(black_box(32), 0) as u64));
    rw(bench("fib checked, Result<i64,Box<E>> + depth arg", calls, || fib_chk_result_box_arg(black_box(32), 0).unwrap() as u64));
    println!("   size_of Result<i64,Box<E>> = {}, Result<i64,E> = {}, Result<P3,Box<E>> = {}",
        std::mem::size_of::<Result<i64, Box<RangeError>>>(), std::mem::size_of::<Result<i64, RangeError>>(), std::mem::size_of::<Result<P3, Box<RangeError>>>());
    let it = 20_000_000u64;
    let p0 = bench("P3 chain, unwind (20M non-inlined calls)", it, || { let mut p = P3 { x: 0, y: 0, z: 0 }; for i in 0..it { p = p3_unwind(p, black_box((i & 7) as i64)); } p.x as u64 });
    let p1 = bench("P3 chain, Result<P3, Box<E>> + ?", it, || { let mut p = P3 { x: 0, y: 0, z: 0 }; for i in 0..it { p = p3_result(p, black_box((i & 7) as i64)).unwrap(); } p.x as u64 });
    println!("{:>62}  -> {:+.2} ns/call, {:+.1}%", "", p1 - p0, (p1 / p0 - 1.0) * 100.0);

    println!("== catch_unwind when nothing is thrown");
    let m = 50_000_000u64;
    let c0 = bench("plain non-inlined call", m, || { let mut s = 0u64; for i in 0..m { s = s.wrapping_add(work(black_box(i))); } s });
    let c1 = bench("same call inside catch_unwind", m, || { let mut s = 0u64; for i in 0..m {
        match panic::catch_unwind(AssertUnwindSafe(|| work(black_box(i)))) { Ok(v) => s = s.wrapping_add(v), Err(_) => s = s.wrapping_add(1) } } s });
    println!("{:>62}  -> +{:.2} ns per catch_unwind entry", "", c1 - c0);

    println!("== cost of one throw (caught)");
    panic::set_hook(Box::new(|_| {}));
    let k = 200_000u64;
    bench("panic_any + catch_unwind, silent hook, 0 frames", k, || { let mut c = 0; for _ in 0..k {
        if let Err(e) = panic::catch_unwind(|| { if black_box(true) { throw_range_panic_any() } 0u64 }) { if e.downcast_ref::<RangeError>().is_some() { c += 1 } } } c });
    bench("resume_unwind + catch_unwind (no hook call), 0 frames", k, || { let mut c = 0; for _ in 0..k {
        if let Err(e) = panic::catch_unwind(|| { if black_box(true) { throw_range_resume() } 0u64 }) { if e.downcast_ref::<RangeError>().is_some() { c += 1 } } } c });
    for d in [10u32, 100] {
        bench(&format!("resume_unwind through {d} frames with a String each"), k / 10, || { let mut c = 0; for _ in 0..k / 10 {
            if let Err(e) = panic::catch_unwind(|| deep_throw(black_box(d))) { if e.downcast_ref::<RangeError>().is_some() { c += 1 } } } c });
        bench(&format!("Err(Box<E>) via ? through {d} frames with a String each"), k / 10, || { let mut c = 0; for _ in 0..k / 10 {
            if deep_err(black_box(d)).is_err() { c += 1 } } c });
    }
    for d in [10u32, 100] {
        bench(&format!("resume_unwind through {d} frames, no destructors"), k / 10, || { let mut c = 0; for _ in 0..k / 10 {
            if let Err(e) = panic::catch_unwind(|| deep_throw_nodrop(black_box(d))) { if e.downcast_ref::<RangeError>().is_some() { c += 1 } } } c });
    }
    bench("Backtrace::force_capture() (unresolved), at depth ~5", 2_000, || { let mut c = 0; for _ in 0..2_000 {
        let bt = std::backtrace::Backtrace::force_capture(); c += black_box(&bt) as *const _ as u64 & 1; } c });
    let _ = panic::take_hook();

    println!("== overflow checks (checked ops that unwind) vs wrapping ops");
    let xs: Vec<i64> = (0..1_000_000).map(|i| (i * 7919) % 1000).collect();
    let pair = |name: &str, ops: u64, a: &mut dyn FnMut() -> u64, b: &mut dyn FnMut() -> u64| {
        let w = bench(&format!("{name} wrapping"), ops, a);
        let c = bench(&format!("{name} checked"), ops, b);
        println!("{:>62}  -> {:+.1}%", "", (c / w - 1.0) * 100.0);
    };
    pair("sum 1M i64 (vectorizable)", 1_000_000, &mut || sum_wrap(black_box(&xs)) as u64, &mut || sum_chk(black_box(&xs)) as u64);
    let nn = 256usize;
    let a: Vec<i64> = (0..nn * nn).map(|i| (i % 17) as i64).collect();
    let b2: Vec<i64> = (0..nn * nn).map(|i| (i % 13) as i64).collect();
    pair("matmul 256x256 i64", (nn * nn * nn) as u64,
        &mut || { let mut c = vec![0i64; nn * nn]; matmul_wrap(black_box(&a), black_box(&b2), &mut c, nn); c[7] as u64 },
        &mut || { let mut c = vec![0i64; nn * nn]; matmul_chk(black_box(&a), black_box(&b2), &mut c, nn); c[7] as u64 });
    pair("collatz total steps 1..300k", 300_000, &mut || collatz_wrap(black_box(300_000)) as u64, &mut || collatz_chk(black_box(300_000)) as u64);
    pair("sieve 10M", 10_000_000, &mut || sieve_wrap(black_box(10_000_000)) as u64, &mut || sieve_chk(black_box(10_000_000)) as u64);
    pair("poly hash mod p, 1M", 1_000_000, &mut || poly_wrap(black_box(&xs)) as u64, &mut || poly_chk(black_box(&xs)) as u64);
    black_box(hash_wrap(&xs));
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    if arg == "only-throw" { std::thread::Builder::new().stack_size(64 << 20).spawn(|| {
        panic::set_hook(Box::new(|_| {}));
        let k = 20_000u64;
        for d in [0u32, 1, 2, 5, 10, 20, 50, 100] {
            bench(&format!("throw-only resume_unwind, {d} frames no drop"), k, || { let mut c = 0; for _ in 0..k {
                if panic::catch_unwind(|| deep_throw_nodrop(black_box(d))).is_err() { c += 1 } } c });
        }
    }).unwrap().join().unwrap(); return; }
    if arg == "overflow" {
        // demonstrate default Rust behaviour on stack overflow inside catch_unwind
        let r = panic::catch_unwind(|| fib_depth_arg_unbounded(0));
        println!("caught? {:?}", r.is_err());
        return;
    }
    if arg == "dropchain" {
        // a 1M-long singly linked list (what `next: Node?` compiles to) dropped inside catch_unwind
        struct L { next: Option<Box<L>>, _v: u64 }
        let r = panic::catch_unwind(|| {
            let mut head: Option<Box<L>> = None;
            for i in 0..1_000_000u64 { head = Some(Box::new(L { next: head, _v: i })); }
            drop(head);
        });
        println!("drop finished: {:?}", r.is_ok());
        return;
    }
    if arg == "depth" {
        // how deep can a small compiled function recurse on an 8 MiB stack before the guard fires?
        for kb in [8usize, 64, 1024] {
            let h = std::thread::Builder::new().stack_size(kb << 20 >> 10 << 10).spawn(move || {
                let base = read_sp();
                SP_LIMIT.with(|l| l.set(base - (kb << 10) + (64 << 10)));
                MAXD.with(|m| m.set(0));
                let _ = panic::catch_unwind(|| guarded_count(0));
                println!("stack {kb} KiB: guarded small fn reached depth {}", MAXD.with(|m| m.get()));
            }).unwrap();
            h.join().unwrap();
        }
        return;
    }
    if arg == "guarded" {
        // a depth-guarded deep recursion on a thread with a known stack: caught as a typed error
        let h = std::thread::Builder::new().stack_size(8 << 20).spawn(|| {
            let base = read_sp();
            SP_LIMIT.with(|l| l.set(base - (8 << 20) + (256 << 10)));
            let r = panic::catch_unwind(|| guarded_unbounded(0));
            match r { Err(e) if e.downcast_ref::<StackOverflowError>().is_some() => println!("caught StackOverflowError"), _ => println!("not caught") }
            let r2 = panic::catch_unwind(|| guarded_unbounded(0));
            println!("second time: {}", if r2.is_err() { "caught again" } else { "?" });
        }).unwrap();
        h.join().unwrap();
        return;
    }
    let h = std::thread::Builder::new().stack_size(512 << 20).spawn(run).unwrap();
    h.join().unwrap();
}

#[inline(never)]
fn fib_depth_arg_unbounded(d: u64) -> u64 {
    let buf = [d; 16];
    black_box(&buf);
    fib_depth_arg_unbounded(d + 1).wrapping_add(buf[3])
}
thread_local! { static MAXD: Cell<u64> = const { Cell::new(0) }; }
#[inline(never)]
fn guarded_count(d: u64) -> u64 {
    if read_sp() < SP_LIMIT.with(|l| l.get()) { MAXD.with(|m| m.set(d)); throw_so() }
    let x = black_box(d);
    guarded_count(d + 1).wrapping_add(x)
}
#[inline(never)]
fn guarded_unbounded(d: u64) -> u64 {
    if read_sp() < SP_LIMIT.with(|l| l.get()) { throw_so() }
    let buf = [d; 16];
    black_box(&buf);
    guarded_unbounded(d + 1).wrapping_add(buf[3])
}
