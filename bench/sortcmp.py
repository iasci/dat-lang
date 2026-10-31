from functools import cmp_to_key

xs = []
x = 42
for i in range(500_000):
    x = (x * 1103515245 + 12345) % 2147483648
    xs.append(x % 1_000_000)
xs.sort(key=cmp_to_key(lambda a, b: b - a))
by_last = sorted(xs, key=lambda v: v % 1000)
print(xs[0], xs[len(xs) - 1], xs[250_000], by_last[0], by_last[499_999])
