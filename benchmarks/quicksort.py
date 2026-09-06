# Quicksort Benchmark (Recursive in-place partitioning of pseudo-random data)
import time
import sys
sys.setrecursionlimit(20000)

def partition(arr: list, low: int, high: int) -> int:
    pivot = arr[high]
    i = low - 1
    for j in range(low, high):
        if arr[j] <= pivot:
            i += 1
            temp = arr[i]
            arr[i] = arr[j]
            arr[j] = temp
    temp = arr[i + 1]
    arr[i + 1] = arr[high]
    arr[high] = temp
    return i + 1

def quicksort(arr: list, low: int, high: int) -> None:
    if low < high:
        pi = partition(arr, low, high)
        quicksort(arr, low, pi - 1)
        quicksort(arr, pi + 1, high)

n = 10000
seed = 42
data = []
for _ in range(n):
    seed = (seed * 1664525 + 1013904223) % 2147483647
    data.append(seed)

t0 = time.time()
quicksort(data, 0, n - 1)
t1 = time.time()

sorted_ok = "ok"
for i in range(n - 1):
    if data[i] > data[i + 1]:
        sorted_ok = "fail"

print("quicksort(10000):", sorted_ok, data[0], data[n - 1], sum(data[:10]), "time:", t1 - t0)
