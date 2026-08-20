"""The CPU feature levels against each other.

One artifact carries several compilations of every hot loop and picks one at
startup; `LIBJAY_CPU_LEVEL` overrides the pick. This measures the same
programs at each level the machine can run, so the table says what the
vector width is worth on top of what threading already bought.

Run with the bench environment's interpreter:

    .venv-bench/bin/python bench/simd.py

Every timing comes from a subprocess (this file, with --worker), because
both the level and the thread count are read once per process.
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

from data import best_of, make_close, make_matrix, make_vectors  # noqa: E402

HERE = Path(__file__).resolve().parent
WINDOW = 20

# The programs. Each is one compiled kernel, called with the data bound.
SOURCE = {
    "weighted sum": "+/ {w} * {x}",
    "sum of exponentials": "+/ ^ {x}",
    "std, named value": "d =. {x} - (+/ {x}) % # {x}\n%: (+/ d * d) % # d",
    "column sums": "+/ {m}",
    "count above": "+/ {x} > 0.5",
    "polynomial": "+/ {x} * 1 + {x} * 2 + {x} * 3 + {x} * 4 + {x}",
    "elementwise chain": "({x} * {w}) + {x} - 0.5",
    "moving maximum": f"{WINDOW} >./\\ {{x}}",
    "running sum": "+/\\ {x}",
    "bollinger": (
        "s =. %d +/\\ {close}\n"
        "((%d * %d }. {close}) - s) %% %%: (%d * %d +/\\ *: {close}) - s * s"
        % (WINDOW, WINDOW, WINDOW - 1, WINDOW, WINDOW)
    ),
}


def bind(name: str, rows: int, mrows: int, cols: int):
    import jay

    kernel = jay.j.compile(SOURCE[name])
    if name == "column sums":
        return kernel.bind({"m": make_matrix(mrows, cols)})
    if name == "bollinger":
        return kernel.bind({"close": make_close(rows)})
    w, x = make_vectors(rows)
    if name in ("weighted sum", "elementwise chain"):
        return kernel.bind({"w": w, "x": x})
    return kernel.bind({"x": x})


# ----------------------------------------------------------------- worker


def signature(value) -> float:
    """One float standing for a result, so the harness can check that every
    level computed the same thing. numpy does the summing: a whole result
    array is 20M numbers and Python would spend longer adding them up than
    libjay spent producing them."""
    if hasattr(value, "tolist"):
        value = value.tolist()
    return float(np.asarray(value, dtype=float).sum())


def worker(args) -> None:
    kernel = bind(args.scenario, args.rows, args.mrows, args.cols)
    best, value = best_of(kernel, args.repeat)
    print(json.dumps({"best": best, "signature": signature(value)}))


# ---------------------------------------------------------------- harness


def measure(scenario: str, level: str, threads: int, args) -> dict:
    env = dict(os.environ)
    env["LIBJAY_CPU_LEVEL"] = level
    env["LIBJAY_THREADS"] = str(threads)
    out = subprocess.run(
        [
            sys.executable,
            str(HERE / "simd.py"),
            "--worker",
            "--scenario",
            scenario,
            "--rows",
            str(args.rows),
            "--mrows",
            str(args.mrows),
            "--cols",
            str(args.cols),
            "--repeat",
            str(args.repeat),
        ],
        env=env,
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(out.stdout)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--worker", action="store_true")
    ap.add_argument("--scenario")
    ap.add_argument("--rows", type=int, default=20_000_000)
    ap.add_argument("--mrows", type=int, default=2_000_000)
    ap.add_argument("--cols", type=int, default=8)
    ap.add_argument("--repeat", type=int, default=5)
    ap.add_argument("--levels", default="baseline,v2,native")
    ap.add_argument("--threads", default="1,8")
    ap.add_argument("--rounds", type=int, default=2)
    args = ap.parse_args()
    if args.worker:
        worker(args)
        return

    levels = args.levels.split(",")
    threads = [int(t) for t in args.threads.split(",")]

    print(f"machine   {platform.platform()}")
    print(f"cpu       {platform.processor()}, {os.cpu_count()} logical threads")
    print(f"python    {platform.python_version()}, numpy {np.__version__}")
    print(f"levels    {', '.join(levels)} (LIBJAY_CPU_LEVEL)")
    print(f"sizes     vector {args.rows:,} f64, matrix {args.mrows:,} x {args.cols} f64")
    print(
        f"method    best of {args.repeat} calls after one warmup, "
        f"best of {args.rounds} passes over the table, wall time in ms\n"
    )

    # The whole table is measured `rounds` times and each cell keeps its
    # best: on a laptop the interference comes in bursts of tens of seconds,
    # which repeating one cell cannot see past but repeating the table can.
    best: dict[tuple[str, int, str], float] = {}
    for _ in range(args.rounds):
        for scenario in SOURCE:
            for t in threads:
                row = [measure(scenario, lv, t, args) for lv in levels]
                sig = row[0]["signature"]
                off = [
                    r["signature"]
                    for r in row[1:]
                    if abs(r["signature"] - sig) > 1e-9 * max(1.0, abs(sig))
                ]
                if off:
                    print(f"MISMATCH in {scenario}: {sig} against {off}", file=sys.stderr)
                for lv, r in zip(levels, row):
                    key = (scenario, t, lv)
                    best[key] = min(best.get(key, float("inf")), r["best"])

    head = ["scenario", "threads"] + levels + ["speedup"]
    print("| " + " | ".join(head) + " |")
    print("|" + "---|" * len(head))
    for scenario in SOURCE:
        for t in threads:
            times = [best[(scenario, t, lv)] for lv in levels]
            cells = " | ".join(f"{v * 1e3:.1f}" for v in times)
            print(f"| {scenario} | {t} | {cells} | {times[0] / times[-1]:.2f}x |")


if __name__ == "__main__":
    main()
