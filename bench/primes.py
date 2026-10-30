# The CPython reference for bench/primes.dat interpreted. The loop is in a
# function, where CPython runs it fastest.
def is_prime(n):
    if n < 2:
        return False
    d = 2
    while d * d <= n:
        if n % d == 0:
            return False
        d += 1
    return True


def main():
    count = 0
    for n in range(150_000):
        if is_prime(n):
            count += 1
    print(count)


main()
