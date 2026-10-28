import subprocess, time, statistics, sys, os
os.chdir(sys.argv[1])
def t(cmd, runs):
    xs=[]
    for i in range(runs+1):
        s=time.perf_counter(); subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True); e=time.perf_counter()
        if i>0: xs.append(e-s)
    return min(xs), statistics.median(xs)
startup = {
 "rust empty binary": ["./empty"], "python3 -c pass": ["python3","-I","-c","pass"], "node -e ''": ["node","-e",""],
 "bun -e ''": ["bun","-e",""], "lua -e ''": ["lua","-e",""]}
print("== startup (ms) min / median, 20 runs")
for k,c in startup.items():
    a,b=t(c,20); print(f"{k:28s} {a*1000:8.1f} {b*1000:8.1f}")
progs = {
 "rust hand (wrapping)": ["./fib_hand"], "rust hand overflow-checks": ["./fib_hand_oc"],
 "rust gen Result+checked": ["./fib_result"], "rust gen unwind+checked": ["./fib_unwind"],
 "rust gen unwind+depth guard": ["./fib_unwind_guard"],
 "tree-walker resolved (Rust)": ["./tw","resolved"], "tree-walker naive (Rust)": ["./tw","naive"],
 "lua 5.5": ["lua","fib.lua"], "python 3.15": ["python3","-I","fib.py"], "node 26": ["node","fib.js"], "bun 1.3": ["bun","fib.js"]}
for n,runs in ((30,7),(35,3)):
    print(f"== fib({n}) wall time incl. startup (ms) min / median, {runs} runs")
    for k,c in progs.items():
        a,b=t(c+[str(n)],runs); print(f"{k:28s} {a*1000:9.1f} {b*1000:9.1f}")
