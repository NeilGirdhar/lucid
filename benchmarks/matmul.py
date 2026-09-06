# Matrix Multiplication Benchmark (O(N^3) dense matrix product)
import time

def matmul(n: int) -> float:
    a = [[0.0 for _ in range(n)] for _ in range(n)]
    b = [[0.0 for _ in range(n)] for _ in range(n)]
    c = [[0.0 for _ in range(n)] for _ in range(n)]

    for i in range(n):
        for j in range(n):
            a[i][j] = ((i + j) % 100) * 0.01
            b[i][j] = ((i * j) % 100) * 0.01

    for i in range(n):
        for k in range(n):
            aik = a[i][k]
            for j in range(n):
                c[i][j] += aik * b[k][j]

    total = 0.0
    for i in range(n):
        for j in range(n):
            total += c[i][j]
    return total

n = 100
t0 = time.time()
res = matmul(n)
t1 = time.time()

print("matmul(100):", int(round(res, 0)), "time:", t1 - t0)
