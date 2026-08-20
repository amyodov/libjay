"""One J expression against a multi-step Polars pipeline: the Bollinger
z-score of a synthetic OHLCV close series.

    z[i] = (close[i] - mean(close[i-19 .. i])) / std(close[i-19 .. i])

Polars spells it as a rolling mean, a rolling standard deviation and the
arithmetic between them — three passes over the column, each materialised.
libjay spells it as one compiled kernel: two moving sums (of the closes and
of their squares), then arithmetic. numba is the hand-rolled loop.

Run with the bench environment's interpreter:

    .venv-bench/bin/python bench/timeseries.py

Every libjay timing comes from a subprocess (this file, with --worker)
because the thread count is fixed when the pool is first used. Each number
is the best of --repeat wall times after a warmup call, and libjay's result
is compared with Polars' before anything is timed.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402

from data import best_of, make_close  # noqa: E402

HERE = Path(__file__).resolve().parent
WINDOW = 20

# The whole kernel. `s` is the moving sum of the closes; the second sentence
# is the z-score written to need only that and the moving sum of the squares:
#
#     mean = s % w                variance = (w*Q - s*s) % (w*w)
#     z    = (w*close - s) % %: (w*Q - s*s)
#
# `19 }. {close}` drops the w-1 leading closes the windows do not cover, so
# both sides of the subtraction are aligned on the window's last item.
SOURCE = """s =. {w} +/\\ {close}
(({w} * {d} }. {close}) - s) % %: ({w} * {w} +/\\ *: {close}) - s * s"""


def kernel_source(window: int) -> str:
    return SOURCE.replace("{w}", str(window)).replace("{d}", str(window - 1))


# ------------------------------------------------------------------ numba


def numba_kernel():
    """The hand-rolled loop, compiled on demand so the worker need not pay
    for numba's import."""
    import numba
    from numba import njit, prange

    @njit(cache=True)
    def nb_bollinger(x, w):
        n = x.shape[0]
        out = np.empty(n - w + 1)
        s = 0.0
        q = 0.0
        for i in range(w):
            s += x[i]
            q += x[i] * x[i]
        for i in range(n - w + 1):
            if i > 0:
                s += x[i + w - 1] - x[i - 1]
                q += x[i + w - 1] * x[i + w - 1] - x[i - 1] * x[i - 1]
            mu = s / w
            v = q / w - mu * mu
            out[i] = (x[i + w - 1] - mu) / math.sqrt(v)
        return out

    @njit(cache=True, parallel=True)
    def nb_bollinger_par(x, w):
        n = x.shape[0]
        out = np.empty(n - w + 1)
        for i in prange(n - w + 1):
            s = 0.0
            q = 0.0
            for k in range(i, i + w):
                s += x[k]
                q += x[k] * x[k]
            mu = s / w
            v = q / w - mu * mu
            out[i] = (x[i + w - 1] - mu) / math.sqrt(v)
        return out

    return numba.__version__, nb_bollinger, nb_bollinger_par


# ----------------------------------------------------------------- polars


def polars_pipeline(close: np.ndarray, window: int):
    import polars as pl

    df = pl.DataFrame({"close": close})
    ma = pl.col("close").rolling_mean(window_size=window)
    sd = pl.col("close").rolling_std(window_size=window, ddof=0)

    def run():
        # The leading window-1 rows are null, which is the alignment libjay
        # gets by dropping them; slicing them off is part of the pipeline.
        return df.select(((pl.col("close") - ma) / sd).alias("z")).to_series()[window - 1:]

    return run


def numpy_pipeline(close: np.ndarray, window: int):
    def run():
        c = np.cumsum(close)
        q = np.cumsum(close * close)
        s = np.concatenate(([c[window - 1]], c[window:] - c[:-window]))
        s2 = np.concatenate(([q[window - 1]], q[window:] - q[:-window]))
        mu = s / window
        var = s2 / window - mu * mu
        return (close[window - 1:] - mu) / np.sqrt(var)

    return run


# ----------------------------------------------------------------- libjay


def libjay_kernel(rows: int, window: int):
    import jay

    close = make_close(rows)
    return jay.j.compile(kernel_source(window)).bind({"close": close})


def as_array(value) -> np.ndarray:
    """A libjay result as numpy. A rank-1 numeric result leaves through the
    Arrow C data interface, which polars reads without copying."""
    import polars as pl

    return np.asarray(pl.Series(value), dtype=float)


def worker(args) -> None:
    kernel = libjay_kernel(args.rows, args.window)
    best, value = best_of(kernel, args.repeat)
    v = as_array(value)
    print(json.dumps({"best": best, "n": int(v.shape[0]), "sum": float(v.sum())}))


def run_libjay(threads: int, args) -> dict:
    env = dict(os.environ, LIBJAY_THREADS=str(threads))
    cmd = [
        sys.executable, str(HERE / "timeseries.py"), "--worker",
        "--rows", str(args.rows),
        "--window", str(args.window),
        "--repeat", str(args.repeat),
    ]
    out = subprocess.run(cmd, env=env, capture_output=True, text=True, check=True)
    return json.loads(out.stdout.strip().splitlines()[-1])


# ------------------------------------------------------------------- main


def fmt_ms(t: float | None) -> str:
    return "n/a" if t is None else f"{t * 1e3:.1f}"


def difference(a: np.ndarray, b: np.ndarray) -> str:
    """How far apart two z-score series are.

    The relative figure skips the rows where z is nearly zero: there the
    quantity is a difference of two prices that all but cancel, so its own
    relative error is large however the moving average was computed, and it
    says nothing about the algorithms being compared.
    """
    d = np.abs(a - b)
    big = np.abs(a) >= 1e-3
    rel = (d[big] / np.abs(a[big])).max() if big.any() else 0.0
    return f"max abs {d.max():.2e}, max rel {rel:.2e} over |z| >= 1e-3"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--rows", type=int, default=20_000_000)
    ap.add_argument("--window", type=int, default=WINDOW)
    ap.add_argument("--repeat", type=int, default=5)
    ap.add_argument("--threads", type=int, default=os.cpu_count() or 1)
    ap.add_argument("--worker", action="store_true")
    args = ap.parse_args()
    if args.worker:
        return worker(args)

    import polars as pl

    close = make_close(args.rows)
    w = args.window

    print(f"machine   {platform.platform()}")
    print(f"cpu       {platform.processor()}, {os.cpu_count()} logical threads")
    print(f"sizes     {args.rows:,} f64 closes, window {w}")
    print(f"kernel    {kernel_source(w)!r}\n")

    # Correctness first, in this process and untimed: the whole point of the
    # comparison is that the two spellings compute the same series.
    ours = as_array(libjay_kernel(args.rows, w)())
    theirs = np.asarray(polars_pipeline(close, w)(), dtype=float)
    assert ours.shape == theirs.shape, (ours.shape, theirs.shape)
    if not np.allclose(ours, theirs, rtol=1e-9, atol=1e-9):
        raise SystemExit(f"libjay and polars differ: {difference(ours, theirs)}")

    numba_version, nb, nb_par = numba_kernel()
    print(f"python    {platform.python_version()}, numpy {np.__version__}, "
          f"polars {pl.__version__}, numba {numba_version}")
    print(f"method    best of {args.repeat} after one warmup, wall time in ms\n")

    rivals = {
        "polars": polars_pipeline(close, w),
        "numba": lambda: nb(close, w),
        "numba prange": lambda: nb_par(close, w),
        "numpy": numpy_pipeline(close, w),
    }
    times = {}
    print(f"difference from libjay over {ours.shape[0]:,} rows:")
    print(f"  polars        {difference(ours, theirs)}")
    for name, f in rivals.items():
        t, value = best_of(f, args.repeat)
        times[name] = t
        if name != "polars":
            got = np.asarray(value, dtype=float)
            print(f"  {name:<13} {difference(ours, got)}")
    print()

    one = run_libjay(1, args)
    many = run_libjay(args.threads, args)

    print(f"| kernel | libjay 1 thread | libjay {args.threads} threads | speedup | "
          "polars | numba | numba prange | numpy |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|")
    print(f"| Bollinger z-score, window {w} | {fmt_ms(one['best'])} | "
          f"{fmt_ms(many['best'])} | {one['best'] / many['best']:.2f}x | "
          f"{fmt_ms(times['polars'])} | {fmt_ms(times['numba'])} | "
          f"{fmt_ms(times['numba prange'])} | {fmt_ms(times['numpy'])} |")

    print("\nScaling:\n")
    print("| LIBJAY_THREADS | time (ms) | speedup over 1 |")
    print("|---:|---:|---:|")
    base = None
    for t in (1, 2, 4, 8):
        best = run_libjay(t, args)["best"]
        base = base or best
        print(f"| {t} | {best * 1e3:.1f} | {base / best:.2f}x |")


if __name__ == "__main__":
    main()
