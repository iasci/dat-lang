def first_true(lo, hi, pred):
    a, b = lo, hi
    while a < b:
        mid = (a + b) // 2
        if pred(mid):
            b = mid
        else:
            a = mid + 1
    return a


total = 0
for n in range(1_000_000):
    total += first_true(0, 1_000, lambda x: x * x >= n)
print(total)
