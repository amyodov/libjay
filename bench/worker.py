"""One libjay measurement, in a process of its own.

The thread count is read once per process, so every libjay timing in the
table comes from a fresh interpreter started with LIBJAY_THREADS set for it.
Prints one JSON object: the best wall time and a checksum of the result.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import libjay  # noqa: E402
from data import best_of, checksum, make_matrix, make_vectors  # noqa: E402

# The J of each scenario. Sentences are separated by newlines; the value of
# the program is the value of the last one.
SOURCE = {
    "weighted_sum": "+/ {w} * {x}",
    "column_sums": "+/ {m}",
    "std_named": "d =. {x} - (+/ {x}) % # {x}\n%: (+/ d * d) % # d",
    "std_inline": "%: (+/ ({x} - (+/ {x}) % # {x}) * {x} - (+/ {x}) % # {x}) % # {x}",
    "sum_exp": "+/ ^ {x}",
}


def bind(scenario: str, rows: int, mrows: int, cols: int):
    kernel = libjay.j.compile(SOURCE[scenario])
    if scenario == "column_sums":
        return kernel.bind({"m": make_matrix(mrows, cols)})
    w, x = make_vectors(rows)
    if scenario == "weighted_sum":
        return kernel.bind({"w": w, "x": x})
    return kernel.bind({"x": x})


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--scenario", required=True, choices=sorted(SOURCE))
    ap.add_argument("--rows", type=int, required=True)
    ap.add_argument("--mrows", type=int, required=True)
    ap.add_argument("--cols", type=int, required=True)
    ap.add_argument("--repeat", type=int, default=5)
    args = ap.parse_args()

    kernel = bind(args.scenario, args.rows, args.mrows, args.cols)
    best, value = best_of(kernel, args.repeat)
    print(json.dumps({"best": best, "checksum": checksum(value)}))


if __name__ == "__main__":
    main()
