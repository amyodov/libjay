"""Where splitting a pass across threads starts to pay.

`bench/sweep.py` times three one-pass kernels over a range of argument
lengths, once with LIBJAY_THREADS=1 and once with the machine's thread
count, and prints the two side by side. This is the measurement behind
`par::MIN_WORK`, the element count below which the runtime does not split
anything.

Run with the bench environment's interpreter:

    .venv-bench/bin/python bench/sweep.py
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402

from data import best_of  # noqa: E402

SIZES = (4_096, 16_384, 32_768, 65_536, 131_072, 262_144, 1_048_576)
KERNELS = {
    "{w} * {x}": ("w", "x"),
    "^ {x}": ("x",),
    "+/ {x}": ("x",),
}


def measure(repeat: int) -> dict:
    import jay

    rng = np.random.default_rng(1)
    out = {}
    for n in SIZES:
        w, x = rng.random(n), rng.random(n)
        for src, params in KERNELS.items():
            data = {"w": w, "x": x}
            kernel = jay.j.compile(src).bind({p: data[p] for p in params})
            best, _ = best_of(kernel, repeat)
            out[f"{n}|{src}"] = best
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repeat", type=int, default=200)
    ap.add_argument("--threads", type=int, default=os.cpu_count() or 1)
    ap.add_argument("--emit", action="store_true", help="print raw timings as JSON")
    args = ap.parse_args()

    if args.emit:
        print(json.dumps(measure(args.repeat)))
        return

    def child(threads: int) -> dict:
        env = dict(os.environ, LIBJAY_THREADS=str(threads))
        cmd = [sys.executable, __file__, "--emit", "--repeat", str(args.repeat)]
        done = subprocess.run(cmd, env=env, capture_output=True, text=True, check=True)
        return json.loads(done.stdout.strip().splitlines()[-1])

    one, many = child(1), child(args.threads)
    print(f"| elements | kernel | 1 thread (us) | {args.threads} threads (us) | speedup |")
    print("|---:|---|---:|---:|---:|")
    for n in SIZES:
        for src in KERNELS:
            key = f"{n}|{src}"
            a, b = one[key] * 1e6, many[key] * 1e6
            print(f"| {n:,} | `{src}` | {a:.1f} | {b:.1f} | {a / b:.2f}x |")


if __name__ == "__main__":
    main()
