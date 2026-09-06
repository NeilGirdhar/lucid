# N-Body Simulation Benchmark (Symplectic integration of gravitational orbits)
import time

PI = 3.141592653589793
SOLAR_MASS = 4.0 * PI * PI
DAYS_PER_YEAR = 365.24

bodies = [
    # sun
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, SOLAR_MASS],
    # jupiter
    [4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01,
     1.66007664274403694e-03 * DAYS_PER_YEAR, 7.69901118481405016e-03 * DAYS_PER_YEAR, -6.90460016972063023e-05 * DAYS_PER_YEAR,
     9.54791938424326609e-04 * SOLAR_MASS],
    # saturn
    [8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01,
     -2.76742510726862411e-03 * DAYS_PER_YEAR, 4.99852801234917238e-03 * DAYS_PER_YEAR, 2.30417297573763929e-05 * DAYS_PER_YEAR,
     2.85885980666130812e-04 * SOLAR_MASS],
    # uranus
    [1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01,
     2.96460137564761618e-03 * DAYS_PER_YEAR, 2.37847173976487632e-03 * DAYS_PER_YEAR, -2.96589568540237556e-05 * DAYS_PER_YEAR,
     4.36624404335156298e-05 * SOLAR_MASS],
    # neptune
    [1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01,
     2.68067772490389322e-03 * DAYS_PER_YEAR, 1.62824170038242295e-03 * DAYS_PER_YEAR, -9.51592254519715870e-05 * DAYS_PER_YEAR,
     5.15138902046611451e-05 * SOLAR_MASS]
]

def offset_momentum(b_list: list) -> None:
    px = 0.0
    py = 0.0
    pz = 0.0
    for b in b_list:
        m = b[6]
        px += b[3] * m
        py += b[4] * m
        pz += b[5] * m
    b_list[0][3] = -px / SOLAR_MASS
    b_list[0][4] = -py / SOLAR_MASS
    b_list[0][5] = -pz / SOLAR_MASS

def energy(b_list: list) -> float:
    e = 0.0
    n = len(b_list)
    for i in range(n):
        bi = b_list[i]
        e += 0.5 * bi[6] * (bi[3] * bi[3] + bi[4] * bi[4] + bi[5] * bi[5])
        for j in range(i + 1, n):
            bj = b_list[j]
            dx = bi[0] - bj[0]
            dy = bi[1] - bj[1]
            dz = bi[2] - bj[2]
            d = (dx * dx + dy * dy + dz * dz) ** 0.5
            e -= (bi[6] * bj[6]) / d
    return e

def advance(b_list: list, dt: float, steps: int) -> None:
    n = len(b_list)
    for _ in range(steps):
        for i in range(n):
            bi = b_list[i]
            for j in range(i + 1, n):
                bj = b_list[j]
                dx = bi[0] - bj[0]
                dy = bi[1] - bj[1]
                dz = bi[2] - bj[2]
                d2 = dx * dx + dy * dy + dz * dz
                mag = dt / (d2 * (d2 ** 0.5))
                bi[3] -= dx * bj[6] * mag
                bi[4] -= dy * bj[6] * mag
                bi[5] -= dz * bj[6] * mag
                bj[3] += dx * bi[6] * mag
                bj[4] += dy * bi[6] * mag
                bj[5] += dz * bi[6] * mag
        for i in range(n):
            b_list[i][0] += dt * b_list[i][3]
            b_list[i][1] += dt * b_list[i][4]
            b_list[i][2] += dt * b_list[i][5]

offset_momentum(bodies)
e0 = energy(bodies)

t0 = time.time()
advance(bodies, 0.01, 1000)
t1 = time.time()

e1 = energy(bodies)
print("nbody(1000):", round(e0, 9), round(e1, 9), "time:", t1 - t0)
