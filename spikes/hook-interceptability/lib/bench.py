#!/usr/bin/env python3
"""Wall-clock benchmark of one shell command (SPIKE-GRD-001, suite 06).

usage: bench.py <iterations> <cwd> <setup-cmd|-> <cmd>
Runs <setup-cmd> (untimed, if not '-') and then <cmd> (timed) <iterations>
times in <cwd> with `sh -c`, after BENCH_WARMUP (default 3) warm-up runs. Prints one TSV line:
n  p50_ms  p95_ms  max_ms  failures
"""
import os
import subprocess
import sys
import time


def run(cmd, cwd):
    return subprocess.run(["/bin/sh", "-c", cmd], cwd=cwd,
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode


def main():
    n, cwd, setup, cmd = int(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4]
    warmup = int(os.environ.get("BENCH_WARMUP", "3"))
    samples, failures = [], 0
    for i in range(n + warmup):
        if setup != "-":
            run(setup, cwd)
        t0 = time.perf_counter()
        rc = run(cmd, cwd)
        dt = (time.perf_counter() - t0) * 1000
        if i < warmup:
            continue
        failures += rc != 0
        samples.append(dt)
    samples.sort()
    p = lambda q: samples[min(len(samples) - 1, int(round(q * (len(samples) - 1))))]
    print(f"{n}\t{p(0.50):.2f}\t{p(0.95):.2f}\t{samples[-1]:.2f}\t{failures}")


if __name__ == "__main__":
    main()
