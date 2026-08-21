"""What the column-major layout is worth, at the DataFrame boundary.

A table arrives one buffer per column. libjay used to weave those buffers
into one rows-leading block at the boundary — the copy that dominated every
DataFrame measurement in this file — and now carries the layout instead:
the columns cross borrowed, the folds read them where they lie, and the
weave happens only for a verb that really wants the rows.

Every figure is measured in a subprocess of its own, because the thread
count is fixed the first time the pool is used. Run it against two builds
and compare:

    PYTHONPATH=/path/to/before/python .venv-bench/bin/python bench/layout.py
    PYTHONPATH=$PWD/python            .venv-bench/bin/python bench/layout.py
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from data import SEED, best_of, checksum  # noqa: E402

ROWS = 2_500_000
COLS = 8

# The programs, and what each of them is here to say.
SOURCE = {
    # The boundary on its own: the shape of a table, which reads no element.
    "import": "$ {df}",
    # The two folds a table is asked for.
    "column_sums": "+/ {df}",
    "row_sums": '+/"1 {df}',
    # A fused chain over the columns, then a fold of it.
    "fused_chain": "+/ ({df} * {df}) + 1",
    # A verb that wants the rows: the weave, still there, now on demand.
    "ravel": "+/ , {df}",
    # One column of the frame, through the phase-5 kernel.
    "bollinger": (
        "s =. 20 +/\\ {close}\n"
        "((20 * 19 }. {close}) - s) % %: (20 * 20 +/\\ *: {close}) - s * s"
    ),
    # The transpose, which is now a flag rather than 160 MB.
    "transpose_sums": '+/"1 |: {df}',
}


def frame(rows: int, cols: int):
    """A polars DataFrame of f64 columns — one Arrow buffer per column."""
    import numpy as np
    import polars as pl

    rng = np.random.default_rng(SEED + 1)
    return pl.DataFrame({f"c{j}": rng.random(rows) for j in range(cols)})


def measure(scenario: str, rows: int, cols: int, repeat: int) -> tuple[float, float]:
    import jay

    df = frame(rows, cols)
    if scenario == "bollinger":
        column = df["c0"]
        kernel = jay.j.compile(SOURCE[scenario])
        return best_of(lambda: kernel({"close": column}), repeat)
    kernel = jay.j.compile(SOURCE[scenario])
    # The bind is inside the timed call: the boundary is what is measured.
    return best_of(lambda: kernel({"df": df}), repeat)


def child(scenario: str, rows: int, cols: int, repeat: int) -> None:
    best, value = measure(scenario, rows, cols, repeat)
    print(json.dumps({"best": best * 1000.0, "checksum": checksum(value)}))


def run_child(scenario: str, threads: int, rows: int, cols: int, repeat: int) -> dict:
    env = dict(os.environ)
    env["LIBJAY_THREADS"] = str(threads)
    out = subprocess.run(
        [
            sys.executable,
            str(Path(__file__).resolve()),
            "--child",
            "--scenario",
            scenario,
            "--rows",
            str(rows),
            "--cols",
            str(cols),
            "--repeat",
            str(repeat),
        ],
        capture_output=True,
        text=True,
        env=env,
        check=True,
    )
    return json.loads(out.stdout.strip().splitlines()[-1])


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--child", action="store_true")
    ap.add_argument("--scenario", default=None)
    ap.add_argument("--rows", type=int, default=ROWS)
    ap.add_argument("--cols", type=int, default=COLS)
    ap.add_argument("--repeat", type=int, default=5)
    ap.add_argument("--threads", type=int, default=None)
    args = ap.parse_args()

    if args.child:
        child(args.scenario, args.rows, args.cols, args.repeat)
        return

    import jay

    where = Path(jay.__file__).resolve().parent
    print(f"build     {where}")
    print(f"table     {args.rows} x {args.cols} f64")
    print(f"method    best of {args.repeat} calls after one warmup, ms\n")
    threads = [args.threads] if args.threads else [1, 8]
    scenarios = [args.scenario] if args.scenario else list(SOURCE)
    print(f"{'scenario':<16}" + "".join(f"{t:>12} thread(s)" for t in threads) + "  checksum")
    for scenario in scenarios:
        cells = []
        checks = []
        for t in threads:
            r = run_child(scenario, t, args.rows, args.cols, args.repeat)
            cells.append(f"{r['best']:>20.1f}")
            checks.append(r["checksum"])
        print(f"{scenario:<16}" + "".join(cells) + f"  {checks[0]:.6g}")


if __name__ == "__main__":
    main()
