"""Moving windows inside the fused kernel: the three shapes of a windowed
J sentence, timed against the rivals that have one.

    Bollinger      (close - mean(w)) / std(w), the phase-5 gate's kernel
    moving range   where the close sits inside the window's high-low range
    running sum    a scan and the arithmetic around it

Each is one sentence, so the only thing that moves between two runs of this
file is what the fusion pass made of it. Every libjay timing comes from a
subprocess because the thread count is fixed when the pool is first used;
each number is the best of --repeat wall times after a warmup call, and
every result is compared with numpy's before anything is timed.

    .venv-bench/bin/python bench/windows.py
"""

from __future__ import annotations

import argparse
import json
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

# `s` is the moving sum of the closes; the z-score needs only that and the
# moving sum of the squares. `19 }. {close}` drops the leading closes no
# window covers, which is what aligns both sides on the window's last item.
BOLLINGER = """s =. {w} +/\\ {close}
(({w} * {d} }. {close}) - s) % %: ({w} * {w} +/\\ *: {close}) - s * s"""

# Where the last close sits between the window's low and its high.
RANGE = """lo =. {w} <./\\ {close}
(({d} }. {close}) - lo) % 1e_12 + ({w} >./\\ {close}) - lo"""

# The running mean: a scan, and the arithmetic that turns it into one.
SCAN = """(+/\\ {close}) % 1.0 + i. # {close}"""


def sources(window: int) -> dict[str, str]:
    fill = lambda s: s.replace("{w}", str(window)).replace("{d}", str(window - 1))
    return {
        "Bollinger z-score": fill(BOLLINGER),
        "moving range position": fill(RANGE),
        "running mean": fill(SCAN),
    }


# ------------------------------------------------------------------ numpy


def numpy_bollinger(close: np.ndarray, w: int) -> np.ndarray:
    c = np.cumsum(close)
    q = np.cumsum(close * close)
    s = np.concatenate(([c[w - 1]], c[w:] - c[:-w]))
    s2 = np.concatenate(([q[w - 1]], q[w:] - q[:-w]))
    return (w * close[w - 1:] - s) / np.sqrt(w * s2 - s * s)


def numpy_range(close: np.ndarray, w: int) -> np.ndarray:
    view = np.lib.stride_tricks.sliding_window_view(close, w)
    lo = view.min(axis=1)
    hi = view.max(axis=1)
    return (close[w - 1:] - lo) / (1e-12 + hi - lo)


def numpy_scan(close: np.ndarray, w: int) -> np.ndarray:
    return np.cumsum(close) / (np.arange(close.shape[0]) + 1.0)


NUMPY = {
    "Bollinger z-score": numpy_bollinger,
    "moving range position": numpy_range,
    "running mean": numpy_scan,
}


def exact(name: str, close: np.ndarray, w: int) -> np.ndarray:
    """The reference: every window computed from its own items.

    The numpy pipelines above are the ones worth timing — a cumulative sum
    differenced is how the series is usually written — but the first of them
    loses three digits by 20M rows, so what libjay is CHECKED against is a
    window at a time. That is quadratic in the window, so the check runs
    over a slice.
    """
    view = np.lib.stride_tricks.sliding_window_view(close, w)
    if name == "Bollinger z-score":
        return (close[w - 1:] - view.mean(axis=1)) / view.std(axis=1)
    if name == "moving range position":
        lo, hi = view.min(axis=1), view.max(axis=1)
        return (close[w - 1:] - lo) / (1e-12 + hi - lo)
    return np.cumsum(close) / (np.arange(close.shape[0]) + 1.0)


# ----------------------------------------------------------------- libjay


def kernels(close: np.ndarray, window: int):
    import jay

    return {
        name: jay.j.compile(src).bind({"close": close}) for name, src in sources(window).items()
    }


def as_array(value) -> np.ndarray:
    import polars as pl

    return np.asarray(pl.Series(value), dtype=float)


def worker(args) -> None:
    out = {}
    for name, kernel in kernels(make_close(args.rows), args.window).items():
        best, value = best_of(kernel, args.repeat)
        v = as_array(value)
        out[name] = {"best": best, "n": int(v.shape[0]), "sum": float(v.sum())}
    print(json.dumps(out))


def run_libjay(threads: int, args) -> dict:
    env = dict(os.environ, LIBJAY_THREADS=str(threads))
    cmd = [
        sys.executable, str(HERE / "windows.py"), "--worker",
        "--rows", str(args.rows),
        "--window", str(args.window),
        "--repeat", str(args.repeat),
    ]
    out = subprocess.run(cmd, env=env, capture_output=True, text=True, check=True)
    return json.loads(out.stdout.strip().splitlines()[-1])


# ------------------------------------------------------------------- main


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--rows", type=int, default=20_000_000)
    ap.add_argument("--window", type=int, default=WINDOW)
    ap.add_argument("--repeat", type=int, default=5)
    ap.add_argument("--threads", type=int, default=os.cpu_count() or 1)
    ap.add_argument("--check", type=int, default=500_000,
                    help="rows the correctness check covers")
    ap.add_argument("--worker", action="store_true")
    args = ap.parse_args()
    if args.worker:
        return worker(args)

    close = make_close(args.rows)
    w = args.window

    print(f"machine   {platform.platform()}")
    print(f"cpu       {platform.processor()}, {os.cpu_count()} logical threads")
    print(f"sizes     {args.rows:,} f64 closes, window {w}")
    print(f"method    best of {args.repeat} after one warmup, wall time in ms\n")

    # Correctness first, untimed: every sentence against a reference that
    # computes each window from its own items, over the leading slice.
    head = close[:args.check]
    for name, kernel in kernels(head, w).items():
        got, want = as_array(kernel()), exact(name, head, w)
        assert got.shape == want.shape, (name, got.shape, want.shape)
        if not np.allclose(got, want, rtol=1e-9, atol=1e-9):
            d = np.abs(got - want)
            raise SystemExit(f"{name}: libjay and the reference differ by up to {d.max():.2e}")

    one = run_libjay(1, args)
    many = run_libjay(args.threads, args)
    theirs = {name: best_of(lambda: f(close, w), args.repeat)[0] for name, f in NUMPY.items()}

    print(f"| kernel | libjay 1 thread | libjay {args.threads} threads | speedup | numpy |")
    print("|---|---:|---:|---:|---:|")
    for name in sources(w):
        a, b, n = one[name]["best"], many[name]["best"], theirs[name]
        print(f"| {name} | {a * 1e3:.1f} | {b * 1e3:.1f} | {a / b:.2f}x | {n * 1e3:.1f} |")

    print("\nSentences:\n")
    for name, src in sources(w).items():
        print(f"  {name}: {src!r}")


if __name__ == "__main__":
    main()
