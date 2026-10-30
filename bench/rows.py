# The CPython reference for bench/rows.dat interpreted. The loop is in a
# function because CPython appends in place only to a function's variables.
def row(i):
    return f"{i},item{i % 100},{i * 3}"


def main():
    out = ""
    small = 0
    for i in range(200_000):
        r = row(i)
        if r < "5":
            small += 1
        out += r + "\n"
    print(small)
    print("empty" if out == "" else "built")


main()
