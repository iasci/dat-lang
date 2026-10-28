import json,re
term = """error[E0204]: condition must be bool
  --> src/users.dat:14:7
   |
14 |   if (user) {
   |       ^^^^ `user` is `User?`
   |
fix: compare with null
14 |   if (user != null) {"""
short = """src/users.dat:14:7: error[E0204]: condition must be bool; `user` is `User?`
  fix: `if (user) {` -> `if (user != null) {`"""
js = {"code":"E0204","file":"src/users.dat","span":{"line":14,"col":7,"endCol":11},"message":"condition must be bool","fix":{"title":"compare with null","edits":[{"line":14,"col":7,"endCol":11,"text":"user != null"}]}}
pretty = json.dumps(js, indent=2)
compact = json.dumps(js, separators=(",",":"))
def est(s):
    # crude: count word pieces and punctuation runs, a common proxy for BPE token counts
    return len(re.findall(r"[A-Za-z]+|\d+|[^\sA-Za-z\d]", s))
for name,s in [("terminal (roadmap)",term),("short text (hypothetical)",short),("json pretty",pretty),("json compact",compact)]:
    print(f"{name:28s} chars={len(s):4d} lines={s.count(chr(10))+1:3d} approx_pieces={est(s)}")
