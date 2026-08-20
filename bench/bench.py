"""libjay against Polars, numba and numpy on five realistic kernels.

Run with the bench environment's interpreter:

    .venv-bench/bin/python bench/bench.py

Every libjay timing comes from a subprocess (see worker.py) because the
thread count is fixed when the pool is first used; the other libraries are
measured in this process. Each number is the best of `--repeat` wall times
after a warmup call, and every implementation's result is checked against
libjay's so the table compares work actually done.
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
import polars as pl  # noqa: E402
import numba  # noqa: E402
from numba import njit, prange  # noqa: E402

from data import best_of, checksum, make_matrix, make_vectors  # noqa: E402
from worker import SOURCE  # noqa: E402

HERE = Path(__file__).resolve().parent
WORKER = HERE / "worker.py"


# ------------------------------------------------------------ numba kernels


@njit(cache=True)
def nb_weighted_sum(w, x):
    s = 0.0
    for i in range(x.shape[0]):
        s += w[i] * x[i]
    return s


@njit(cache=True, parallel=True)
def nb_weighted_sum_par(w, x):
    s = 0.0
    for i in prange(x.shape[0]):
        s += w[i] * x[i]
    return s


@njit(cache=True)
def nb_column_sums(m):
    out = np.zeros(m.shape[1])
    for i in range(m.shape[0]):
        for j in range(m.shape[1]):
            out[j] += m[i, j]
    return out


@njit(cache=True, parallel=True)
def nb_column_sums_par(m):
    out = np.zeros(m.shape[1])
    for j in prange(m.shape[1]):
        acc = 0.0
        for i in range(m.shape[0]):
            acc += m[i, j]
        out[j] = acc
    return out


@njit(cache=True)
def nb_sum_exp(x):
    s = 0.0
    for i in range(x.shape[0]):
        s += math.exp(x[i])
    return s


@njit(cache=True, parallel=True)
def nb_sum_exp_par(x):
    s = 0.0
    for i in prange(x.shape[0]):
        s += math.exp(x[i])
    return s


@njit(cache=True)
def nb_std(x):
    n = x.shape[0]
    s = 0.0
    for i in range(n):
        s += x[i]
    mu = s / n
    v = 0.0
    for i in range(n):
        d = x[i] - mu
        v += d * d
    return math.sqrt(v / n)


@njit(cache=True, parallel=True)
def nb_std_par(x):
    n = x.shape[0]
    s = 0.0
    for i in prange(n):
        s += x[i]
    mu = s / n
    v = 0.0
    for i in prange(n):
        d = x[i] - mu
        v += d * d
    return math.sqrt(v / n)


# ------------------------------------------------------------------ running


def run_libjay(scenario: str, threads: int, args) -> dict:
    """One libjay measurement in a subprocess with LIBJAY_THREADS set."""
    env = dict(os.environ, LIBJAY_THREADS=str(threads))
    cmd = [
        sys.executable,
        str(WORKER),
        "--scenario", scenario,
        "--rows", str(args.rows),
        "--mrows", str(args.mrows),
        "--cols", str(args.cols),
        "--repeat", str(args.repeat),
    ]
    out = subprocess.run(cmd, env=env, capture_output=True, text=True, check=True)
    return json.loads(out.stdout.strip().splitlines()[-1])


def timed(f, repeat: int) -> tuple[float, float]:
    best, value = best_of(f, repeat)
    return best, checksum(value)


def scenarios(args, w, x, m):
    """Every scenario as a name, a J source line, and the rival kernels."""
    df = pl.DataFrame({"w": w, "x": x})
    dfm = pl.DataFrame(m, schema=[f"c{i}" for i in range(m.shape[1])])
    n = float(x.shape[0])
    std = {
        "polars": lambda: df.select(pl.col("x").std(ddof=0)).item(),
        "numba": lambda: nb_std(x),
        "numba_par": lambda: nb_std_par(x),
        "numpy": lambda: math.sqrt(((x - x.mean()) ** 2).sum() / n),
    }
    return [
        {
            "name": "weighted sum",
            "size": f"{args.rows:,} rows",
            "polars": lambda: df.select((pl.col("w") * pl.col("x")).sum()).item(),
            "numba": lambda: nb_weighted_sum(w, x),
            "numba_par": lambda: nb_weighted_sum_par(w, x),
            "numpy": lambda: (w * x).sum(),
        },
        {
            "name": "column sums",
            "size": f"{args.mrows:,} x {args.cols}",
            "polars": lambda: dfm.sum().row(0),
            "numba": lambda: nb_column_sums(m),
            "numba_par": lambda: nb_column_sums_par(m),
            "numpy": lambda: m.sum(axis=0),
        },
        {"name": "std, named value", "size": f"{args.rows:,} rows", **std},
        {"name": "std, one sentence", "size": f"{args.rows:,} rows", **std},
        {
            "name": "sum of exponentials",
            "size": f"{args.rows:,} rows",
            "polars": lambda: df.select(pl.col("x").exp().sum()).item(),
            "numba": lambda: nb_sum_exp(x),
            "numba_par": lambda: nb_sum_exp_par(x),
            "numpy": lambda: np.exp(x).sum(),
        },
    ]


def fmt_ms(t: float | None) -> str:
    return "n/a" if t is None else f"{t * 1e3:.1f}"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--rows", type=int, default=20_000_000)
    ap.add_argument("--mrows", type=int, default=2_000_000)
    ap.add_argument("--cols", type=int, default=8)
    ap.add_argument("--repeat", type=int, default=5)
    ap.add_argument("--threads", type=int, default=os.cpu_count() or 1)
    args = ap.parse_args()

    keys = ["weighted_sum", "column_sums", "std_named", "std_inline", "sum_exp"]
    w, x = make_vectors(args.rows)
    m = make_matrix(args.mrows, args.cols)

    print(f"machine   {platform.platform()}")
    print(f"cpu       {platform.processor()}, {os.cpu_count()} logical threads")
    print(f"python    {platform.python_version()}, numpy {np.__version__}, "
          f"polars {pl.__version__}, numba {numba.__version__}")
    print(f"sizes     vector {args.rows:,} f64, matrix {args.mrows:,} x {args.cols} f64")
    print(f"method    best of {args.repeat} after one warmup, wall time in ms\n")

    rows = []
    for key, sc in zip(keys, scenarios(args, w, x, m)):
        one = run_libjay(key, 1, args)
        many = run_libjay(key, args.threads, args)
        row = {
            "name": sc["name"],
            "size": sc["size"],
            "j": SOURCE[key].replace("\n", "  |  "),
            "libjay1": one["best"],
            "libjayN": many["best"],
            "speedup": one["best"] / many["best"],
        }
        want = one["checksum"]
        for who in ("polars", "numba", "numba_par", "numpy"):
            try:
                t, got = timed(sc[who], args.repeat)
            except Exception as e:  # a missing threading layer, mostly
                print(f"  {sc['name']}: {who} unavailable: {e}", file=sys.stderr)
                row[who] = None
                continue
            row[who] = t
            if not math.isclose(got, want, rel_tol=1e-9, abs_tol=1e-9):
                print(f"  {sc['name']}: {who} computed {got!r}, libjay {want!r}",
                      file=sys.stderr)
        rows.append(row)

    print("| scenario | J | libjay 1 thread | libjay {n} threads | speedup "
          "| polars | numba | numba prange | numpy |".format(n=args.threads))
    print("|---|---|---:|---:|---:|---:|---:|---:|---:|")
    for r in rows:
        print(f"| {r['name']} | `{r['j']}` | {fmt_ms(r['libjay1'])} | "
              f"{fmt_ms(r['libjayN'])} | {r['speedup']:.2f}x | "
              f"{fmt_ms(r['polars'])} | {fmt_ms(r['numba'])} | "
              f"{fmt_ms(r['numba_par'])} | {fmt_ms(r['numpy'])} |")

    print("\nScaling, weighted sum:\n")
    print("| LIBJAY_THREADS | time (ms) | speedup over 1 |")
    print("|---:|---:|---:|")
    base = None
    for t in (1, 2, 4, 8):
        best = run_libjay("weighted_sum", t, args)["best"]
        base = base or best
        print(f"| {t} | {best * 1e3:.1f} | {base / best:.2f}x |")


if __name__ == "__main__":
    main()
