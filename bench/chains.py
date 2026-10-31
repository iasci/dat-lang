from functools import reduce

nums = list(range(1_000_000))
total = 0
for r in range(5):
    total += reduce(lambda acc, x: acc + x, [x for x in [x * 3 + r for x in nums] if x % 2 == 0], 0)
print(total, "true" if any(x == 999_999 for x in nums) else "false", sum(1 for x in nums if x % 7 == 0))
