# Spectral Norm Benchmark (Power method eigenvalue calculation)
import time

def a(i: int, j: int) -> float:
    return 1.0 / (((i + j) * (i + j + 1)) // 2 + i + 1)

def multiply_av(n: int, v: list) -> list:
    res = [0.0] * n
    for i in range(n):
        s = 0.0
        for j in range(n):
            s += a(i, j) * v[j]
        res[i] = s
    return res

def multiply_atv(n: int, v: list) -> list:
    res = [0.0] * n
    for i in range(n):
        s = 0.0
        for j in range(n):
            s += a(j, i) * v[j]
        res[i] = s
    return res

def multiply_at_av(n: int, v: list) -> list:
    u = multiply_av(n, v)
    return multiply_atv(n, u)

n = 100
t0 = time.time()

u = [1.0] * n
v = [0.0] * n
for _ in range(10):
    v = multiply_at_av(n, u)
    u = multiply_at_av(n, v)

vBv = 0.0
vv = 0.0
for i in range(n):
    vBv += u[i] * v[i]
    vv += v[i] * v[i]

norm = (vBv / vv) ** 0.5
t1 = time.time()

print("spectral_norm(100):", round(norm, 8), "time:", t1 - t0)
