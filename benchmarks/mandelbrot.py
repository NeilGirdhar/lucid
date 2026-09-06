# Mandelbrot Set Benchmark (Complex plane escape-time computation)
import time

def mandelbrot(n: int, max_iter: int) -> int:
    count = 0
    inv_n = 2.0 / n
    for y in range(n):
        ci = y * inv_n - 1.0
        for x in range(n):
            cr = x * inv_n - 1.5
            zr = 0.0
            zi = 0.0
            iter = 0
            escaped = False
            while iter < max_iter:
                zr2 = zr * zr
                zi2 = zi * zi
                if zr2 + zi2 > 4.0:
                    escaped = True
                    break
                zi = 2.0 * zr * zi + ci
                zr = zr2 - zi2 + cr
                iter += 1
            if not escaped:
                count += 1
    return count

n = 200
t0 = time.time()
res = mandelbrot(n, 50)
t1 = time.time()

print("mandelbrot(200):", res, "time:", t1 - t0)
