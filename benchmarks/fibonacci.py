# Fibonacci Benchmark in Python
import time

def fib(n: int) -> int:
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

t0 = time.time()
n = 28
res = fib(n)
t1 = time.time()

print(f"fibonacci(28): {res} time: {t1 - t0}")
