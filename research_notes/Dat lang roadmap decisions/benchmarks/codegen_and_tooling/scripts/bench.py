#!/usr/bin/env python3
"""Time clean / no-op / body-edit / signature-edit builds for a generated project.

Usage: bench.py PROJECT LABEL EDIT_MODULE [--profile 'k=v;k=v'] [--rustflags '...'] [--release] [--reps N] [--clean-only]
"""
import os, re, subprocess, sys, time, json, statistics, shutil

proj, label, edit_mod = sys.argv[1], sys.argv[2], int(sys.argv[3])
args = sys.argv[4:]
def opt(name, default=None):
    return args[args.index(name) + 1] if name in args else default
profile = opt("--profile", "")
rustflags = opt("--rustflags", "")
release = "--release" in args
reps = int(opt("--reps", "3"))
clean_only = "--clean-only" in args

env = dict(os.environ, RUSTUP_TOOLCHAIN="1.99.0", CARGO_TERM_COLOR="never")
if rustflags:
    env["RUSTFLAGS"] = rustflags
    if "-Z" in rustflags:
        env["RUSTC_BOOTSTRAP"] = "1"

# rewrite the profile section
ct = open(f"{proj}/Cargo.toml").read()
ct = ct.split("\n[profile.")[0]
pname = "release" if release else "dev"
lines = ['panic = "unwind"'] + [kv.strip() for kv in profile.split(";") if kv.strip()]
ct += f"\n[profile.{pname}]\n" + "\n".join(lines) + "\n"
open(f"{proj}/Cargo.toml", "w").write(ct)

cmd = ["cargo", "build", "-q", "-p", "app"] + (["--release"] if release else [])
bin_path = f"{proj}/target/{'release' if release else 'debug'}/app"

def run(c):
    t = time.perf_counter()
    r = subprocess.run(c, cwd=proj, env=env, capture_output=True, text=True)
    dt = time.perf_counter() - t
    if r.returncode != 0:
        print(r.stderr[-3000:], file=sys.stderr)
        sys.exit(1)
    return dt, r.stdout

def find_module_file(m):
    for root, _, files in os.walk(proj):
        if "target" in root:
            continue
        if f"m{m}.rs" in files:
            return os.path.join(root, f"m{m}.rs")
    raise SystemExit("module not found")

shutil.rmtree(f"{proj}/target", ignore_errors=True)
# warm the runtime crate separately so "clean" = app crates only? No: report both.
clean, _ = run(cmd)
res = {"label": label, "clean_s": round(clean, 2)}
size = os.path.getsize(bin_path)
res["bin_mb"] = round(size / 1e6, 1)
if not clean_only:
    noop, _ = run(cmd)
    res["noop_s"] = round(noop, 3)
    path = find_module_file(edit_mod)
    src0 = open(path).read()
    body, sig, run_t = [], [], []
    for k in range(reps):
        # body edit: change a literal inside one function body
        src = open(path).read()
        new = re.sub(r"if acc > (\d+) \{", lambda mm: f"if acc > {int(mm.group(1)) + 1} {{", src, count=1)
        open(path, "w").write(new)
        dt, _ = run(cmd); body.append(dt)
        t = time.perf_counter(); subprocess.run([bin_path], capture_output=True); run_t.append(time.perf_counter() - t)
    for k in range(reps):
        # signature edit: add a new public function (changes the module's interface)
        with open(path, "a") as f:
            f.write(f"pub fn extra_{k}_{int(time.time()*1000)%100000}(x: i64) -> i64 {{ x + {k} }}\n")
        dt, _ = run(cmd); sig.append(dt)
    open(path, "w").write(src0)
    run(cmd)
    res["body_edit_s"] = round(statistics.median(body), 2)
    res["sig_edit_s"] = round(statistics.median(sig), 2)
    res["run_tests_s"] = round(statistics.median(run_t), 3)
print(json.dumps(res))
with open(os.path.join(os.path.dirname(__file__), "..", "results.jsonl"), "a") as f:
    f.write(json.dumps(res) + "\n")
