"""libjay on a GPU against libjay on the CPU.

Run with the bench environment's interpreter:

    .venv-bench/bin/python bench/device.py

Four kernels at 20M rows, each timed three ways: on the CPU with the whole
thread pool, on the device with ordinary arguments (so every call uploads
them), and on the device with the arguments already resident. The third
number is the one that matters for a kernel called repeatedly over data that
lives where it is computed; the second is what a single call costs.

Precision is reported, not assumed. libjay computes in f64 and declines to
lose that quietly: where the adapter has no f64 in shaders — every Metal
machine — nothing reaches the device unless the caller asks for f32, and
this script then also times numpy in float32 so that there is an honest
same-precision figure on the CPU to compare against.
"""

from __future__ import annotations

import argparse
import math
import os
import platform
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402

import jay  # noqa: E402
from data import best_of, checksum, make_vectors  # noqa: E402

# Each kernel is one J sentence over `w` and `x`, or over `x` alone.
KERNELS = {
    "weighted sum": ("+/ {w} * {x}", ("w", "x")),
    "std, one sentence": (
        "%: (+/ ({x} - (+/ {x}) % # {x}) * {x} - (+/ {x}) % # {x}) % # {x}",
        ("x",),
    ),
    "polynomial": ("+/ 2.5 + {x} * 1.5 + {x} * 0.5 * {x}", ("x",)),
    "sum of exponentials": ("+/ ^ {x} % 100", ("x",)),
}


def numpy_rivals(w32, x32):
    """The same four kernels in numpy float32: the CPU at the precision the
    device may have had to fall back to."""
    n = float(x32.shape[0])
    return {
        "weighted sum": lambda: float((w32 * x32).sum(dtype=np.float32)),
        "std, one sentence": lambda: float(
            np.sqrt(((x32 - x32.mean(dtype=np.float32)) ** 2).sum(dtype=np.float32) / n)
        ),
        "polynomial": lambda: float(
            (2.5 + x32 * (1.5 + x32 * (0.5 * x32))).sum(dtype=np.float32)
        ),
        "sum of exponentials": lambda: float(np.exp(x32 / 100).sum(dtype=np.float32)),
    }


def fmt_ms(t: float | None) -> str:
    return "n/a" if t is None else f"{t * 1e3:.1f}"


def rel(a: float, b: float) -> float:
    scale = max(abs(a), abs(b), 1.0)
    return abs(a - b) / scale


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--rows", type=int, default=20_000_000)
    ap.add_argument("--repeat", type=int, default=5)
    args = ap.parse_args()

    adapters = jay.devices()
    print(f"machine   {platform.platform()}")
    print(f"cpu       {platform.processor()}, {os.cpu_count()} logical threads")
    if not adapters:
        print("adapters  none — this machine has no GPU; nothing to measure")
        return
    for a in adapters:
        print(f"adapter   {a.name} ({a.backend}, {a.kind}), f64 in shaders: {a.f64}")

    best = adapters[0]
    if best.f64:
        precision, note = "f64", "the device computes in f64, as the CPU does"
    else:
        precision = "f32"
        note = (
            "this adapter has no f64 in shaders, so only the opted-in f32 path "
            "runs on it; the CPU column is f64 and the numpy column is f32"
        )
    print(f"precision {precision} — {note}")
    print(f"sizes     {args.rows:,} f64 rows")
    print(f"method    best of {args.repeat} after one warmup, wall time in ms\n")

    w, x = make_vectors(args.rows)
    w32, x32 = w.astype(np.float32), x.astype(np.float32)
    rivals = numpy_rivals(w32, x32)

    rows = []
    for name, (src, params) in KERNELS.items():
        data = {"w": w, "x": x} if "w" in params else {"x": x}
        cpu_kernel = jay.j.compile(src).bind(data)
        gpu_kernel = cpu_kernel.deploy("gpu", precision=precision)
        # The same kernel again, with its arguments already on the device.
        resident = {k: gpu_kernel.upload(v) for k, v in data.items()}
        pinned = gpu_kernel.bind(resident)

        t_cpu, v_cpu = best_of(cpu_kernel, args.repeat)
        t_up, v_up = best_of(gpu_kernel, args.repeat)
        t_res, v_res = best_of(pinned, args.repeat)
        try:
            t_np, v_np = best_of(rivals[name], args.repeat)
        except Exception as e:  # pragma: no cover - a missing rival is not fatal
            print(f"  {name}: numpy unavailable: {e}", file=sys.stderr)
            t_np, v_np = None, None

        rows.append(
            {
                "name": name,
                "src": src,
                "cpu": t_cpu,
                "upload": t_up,
                "resident": t_res,
                "numpy32": t_np,
                "drift": rel(checksum(v_cpu), checksum(v_res)),
            }
        )
        if v_up is not None and v_res is not None:
            d = rel(checksum(v_up), checksum(v_res))
            if not math.isclose(d, 0.0, abs_tol=1e-9):
                print(f"  {name}: uploading changed the answer by {d:.2e}", file=sys.stderr)
        if t_np is not None and v_np is not None:
            print(
                f"  {name}: numpy f32 differs from the device by "
                f"{rel(float(v_np), checksum(v_res)):.2e}",
                file=sys.stderr,
            )

    print(
        "| kernel | J | libjay CPU | libjay GPU (with upload) | "
        "libjay GPU (resident) | speedup, resident | numpy f32 | drift vs CPU |"
    )
    print("|---|---|---:|---:|---:|---:|---:|---:|")
    for r in rows:
        speed = r["cpu"] / r["resident"]
        print(
            f"| {r['name']} | `{r['src']}` | {fmt_ms(r['cpu'])} | "
            f"{fmt_ms(r['upload'])} | {fmt_ms(r['resident'])} | {speed:.2f}x | "
            f"{fmt_ms(r['numpy32'])} | {r['drift']:.1e} |"
        )

    print("\nWhere each kernel ran (the real arguments, so the placement is the")
    print("one the table timed):\n")
    for name, (src, params) in KERNELS.items():
        data = {"w": w, "x": x} if "w" in params else {"x": x}
        k = jay.j.compile(src).deploy("gpu", precision=precision)
        for line in k.explain(data).splitlines():
            if "fused kernel" in line:
                print(f"  {name}: {line.strip()}")


if __name__ == "__main__":
    main()
