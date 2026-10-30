# The CPython reference for bench/floats.dat interpreted. The loops are in
# functions, where CPython runs them fastest.
def escapes(cr, ci, limit):
    x = 0.0
    y = 0.0
    for i in range(limit):
        xx = x * x
        yy = y * y
        if xx + yy > 4.0:
            return i
        y = 2.0 * x * y + ci
        x = xx - yy + cr
    return limit


def main():
    size = 400
    inside = 0
    steps = 0
    for row in range(size):
        ci = row / size * 2.4 - 1.2
        for col in range(size):
            cr = col / size * 3.0 - 2.1
            n = escapes(cr, ci, 120)
            steps += n
            if n == 120:
                inside += 1
    print(inside, steps)


main()
