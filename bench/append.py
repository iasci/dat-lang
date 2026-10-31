def append(xs, x):
    xs.append(x)
    return xs

items = []
for i in range(2_000_000):
    items = append(items, i * 2)
s = 0
for x in items:
    s += x
print(len(items), s)
