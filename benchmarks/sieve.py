# Sieve of Eratosthenes Benchmark
import time

def sieve(limit: int) -> int:
    is_prime = [True] * (limit + 1)
    is_prime[0] = False
    is_prime[1] = False

    p = 2
    while p * p <= limit:
        if is_prime[p]:
            i = p * p
            while i <= limit:
                is_prime[i] = False
                i += p
        p += 1

    count = 0
    last_prime = 0
    for i in range(2, limit + 1):
        if is_prime[i]:
            count += 1
            last_prime = i
    return count

t0 = time.time()
n = 100000
count = sieve(n)
t1 = time.time()

print("sieve(100000):", count, "time:", t1 - t0)
