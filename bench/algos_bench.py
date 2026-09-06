"""The six algorithms of `examples/algos`: libjay against numpy, numba and
jconsole.

Run with the bench environment's interpreter:

    LIBJAY_THREADS=8 .venv-bench/bin/python bench/algos_bench.py

Every implementation is handed the SAME input. The series are built by the
deterministic generator the examples carry — a random walk for the price
series, a tone plus noise for the signal — because jconsole is measured in a
subprocess of its own and a million numbers cannot be handed to it as a
literal; both sides compute the input rather than exchange it. libjay and
numpy are checked against each other on the same data before anything is
timed, and the J that runs is read out of `examples/algos/*.ijs`.

Each number is the best wall time of `--repeat` runs after a warmup, which
is the measurement least polluted by whatever else the machine is doing.
libjay's thread pool is fixed when it is first used, so LIBJAY_THREADS is
read from the environment of this process and reported with the table.
"""

from __future__ import annotations

import argparse
import os
import platform
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402

import jay  # noqa: E402
from algos import RANDOM_J, hash_random, jsource  # noqa: E402
from algos import elm as r_elm  # noqa: E402
from algos import hopfield as r_hop  # noqa: E402
from algos import hurst as r_hurst  # noqa: E402
from algos import kama as r_kama  # noqa: E402
from algos import savgol as r_sg  # noqa: E402
from algos import varratio as r_vr  # noqa: E402

JCONSOLE = os.environ.get(
    "LIBJAY_ORACLE_J", os.path.expanduser("~/projects/libjay-oracles/j/j64/jconsole")
)
LENGTHS = [16, 32, 64, 128, 256, 512, 1024, 2048, 4096]


def best_of(f, repeat):
    """One warmup, then the best of `repeat` wall times."""
    value = f()
    best = float("inf")
    for _ in range(repeat):
        t0 = time.perf_counter()
        value = f()
        best = min(best, time.perf_counter() - t0)
    return best, value


def jconsole(setup: str, timed: str, repeat: int) -> float | None:
    """The best of `repeat` timings of one sentence, measured by J's own
    `6!:2`, with the data built outside the timed sentence."""
    if not os.path.exists(JCONSOLE):
        return None
    script = (
        "9!:11 ] 17\n" + setup
        + "".join(f"t{i} =. 6!:2 '{timed}'\n" for i in range(repeat + 1))
        + "<./ " + ", ".join(f"t{i}" for i in range(1, repeat + 1)) + "\n"
    )
    out = subprocess.run(
        [JCONSOLE, "-jprofile", "/dev/null"], input=script,
        capture_output=True, text=True, timeout=1800,
    ).stdout
    for line in reversed(out.splitlines()):
        line = line.strip()
        if line and not line.startswith("|"):
            return float(line.replace("e_", "e-").replace("_", "-"))
    print("jconsole failed:\n" + out[-800:], file=sys.stderr)
    return None


# --------------------------------------------------------------- the inputs


def price(n: int) -> np.ndarray:
    return 100.0 * np.exp(np.cumsum(0.002 * hash_random(7, n)))


J_PRICE = "close =. 100 * ^ +/\\ 0.002 * 7 RANDOM n\n"


def signal(n: int) -> np.ndarray:
    t = np.arange(n) / 48000.0
    return (np.sin(2 * np.pi * 440.0 * t) + 0.5 * np.sin(2 * np.pi * 997.0 * t)
            + 0.1 * hash_random(13, n))


J_SIGNAL = (
    "t =. (i. n) % 48000\n"
    "sig =. (1 o. 2p1 * 440 * t) + (0.5 * 1 o. 2p1 * 997 * t) + 0.1 * 13 RANDOM n\n"
)


def lagged(n: int, d: int):
    """d lagged returns of the price series, and the next return."""
    r = np.diff(np.log(price(n + d + 1)))
    x = np.lib.stride_tricks.sliding_window_view(r, d)[:n]
    return np.ascontiguousarray(x), np.ascontiguousarray(r[d:n + d]).reshape(-1, 1)


J_LAGGED = (
    "r =. 2 -~/\\ ^. 100 * ^ +/\\ 0.002 * 7 RANDOM n + d + 1\n"
    "x =. n {. d ]\\ r\n"
    "tg =. ,. d }. (n + d) {. r\n"
)


# -------------------------------------------------------------- the workloads


def workloads(args):
    n = args.rows

    close = price(n)
    logp = np.log(close)
    rets = np.diff(logp)
    sig = signal(n)
    ex, et = lagged(args.elm_rows, args.elm_inputs)
    ew = 4.0 * hash_random(5, args.elm_inputs * args.hidden).reshape(args.elm_inputs, args.hidden)
    eb = 2.0 * hash_random(9, args.hidden)
    units, probes = args.units, args.probes
    pat = np.sign(hash_random(17, 8 * units)).reshape(8, units)
    hw = r_hop.store(pat)
    hp = np.sign(hash_random(29, probes * units)).reshape(probes, units)

    return [
        dict(
            name="KAMA", size=f"{n:,} bars",
            j="10 2 30 KAMA {close}", data={"close": close},
            numpy=lambda: r_kama.kama(close, 10, 2, 30),
            numba=(lambda: r_kama.kama_numba(close, 10, 2, 30)) if r_kama.kama_numba else None,
            setup="n =. " + str(n) + "\n" + J_PRICE, timed="z =. 10 2 30 KAMA close",
        ),
        dict(
            name="variance ratio", size=f"{n:,} bars, q=8",
            j="8 VR {p}", data={"p": logp},
            numpy=lambda: np.array(r_vr.variance_ratio(logp, 8)), numba=None,
            setup="n =. " + str(n) + "\n" + J_PRICE + "p =. ^. close\n",
            timed="z =. 8 VR p",
        ),
        dict(
            name="Hurst R/S", size=f"{n:,} returns, {len(LENGTHS)} block sizes",
            j="{lens} HURST {x}",
            data={"lens": np.array(LENGTHS, dtype=np.int64), "x": rets},
            numpy=lambda: np.array(r_hurst.hurst(rets, LENGTHS)), numba=None,
            setup="n =. " + str(n) + "\n" + J_PRICE
                  + "x =. 2 -~/\\ ^. close\nlens =. " + " ".join(map(str, LENGTHS)) + "\n",
            timed="z =. lens HURST x",
        ),
        dict(
            name="Savitzky-Golay", size=f"{n:,} samples, 17-tap cubic",
            j="(SGCOEF 8 3 0) SGFILT {sig}", data={"sig": sig},
            numpy=lambda: r_sg.filt(sig, r_sg.coefficients(8, 3, 0)), numba=None,
            setup="n =. " + str(n) + "\n" + J_SIGNAL,
            timed="z =. (SGCOEF 8 3 0) SGFILT sig",
        ),
        dict(
            name="extreme learning machine",
            size=f"{args.elm_rows:,} by {args.elm_inputs}, {args.hidden} hidden",
            j="({w} ; {b} ; 0.1) ELMFIT {x} ; {t}",
            data={"w": ew, "b": eb, "x": ex, "t": et},
            numpy=lambda: r_elm.fit(ex, et, ew, eb, 0.1), numba=None,
            setup=(f"n =. {args.elm_rows}\nd =. {args.elm_inputs}\nh =. {args.hidden}\n"
                   + J_LAGGED
                   + "w =. (d, h) $ 4 * 5 RANDOM d * h\nb =. 2 * 9 RANDOM h\n"),
            timed="z =. (w ; b ; 0.1) ELMFIT x ; tg",
        ),
        dict(
            name="Hopfield recall",
            size=f"{units} units, {probes} probes, 10 sweeps",
            j="({w} ; 10) RECALL {probe}", data={"w": hw, "probe": hp},
            numpy=lambda: r_hop.recall(hp, hw, 10), numba=None,
            setup=(f"n =. {units}\nq =. {probes}\n"
                   "pat =. * (8, n) $ 17 RANDOM 8 * n\nw =. STORE pat\n"
                   "probe =. * (q, n) $ 29 RANDOM q * n\n"),
            timed="z =. (w ; 10) RECALL probe",
        ),
    ]


NAMES = {"KAMA": "kama", "variance ratio": "varratio", "Hurst R/S": "hurst",
         "Savitzky-Golay": "savgol", "extreme learning machine": "elm",
         "Hopfield recall": "hopfield"}


def flat(value):
    if hasattr(value, "tolist"):
        value = value.tolist()
    out = []
    stack = [value]
    while stack:
        v = stack.pop()
        if isinstance(v, (list, tuple)):
            stack.extend(v)
        else:
            out.append(float(v))
    return np.array(out)


def ms(t):
    return "n/a" if t is None else f"{t * 1e3:.1f}"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--rows", type=int, default=1_000_000)
    ap.add_argument("--elm-rows", type=int, default=100_000)
    ap.add_argument("--elm-inputs", type=int, default=16)
    ap.add_argument("--hidden", type=int, default=64)
    ap.add_argument("--units", type=int, default=1024)
    ap.add_argument("--probes", type=int, default=256)
    ap.add_argument("--repeat", type=int, default=3)
    ap.add_argument("--only", default=None)
    args = ap.parse_args()

    threads = os.environ.get("LIBJAY_THREADS", "unset (all cores)")
    print(f"machine   {platform.platform()}")
    print(f"cpu       {platform.processor()}, {os.cpu_count()} logical threads")
    print(f"python    {platform.python_version()}, numpy {np.__version__}")
    print(f"threads   LIBJAY_THREADS={threads}")
    print(f"method    best of {args.repeat} after one warmup, wall time in ms\n")

    rows = []
    for job in workloads(args):
        if args.only and args.only not in job["name"]:
            continue
        print(f"  {job['name']} ...", file=sys.stderr, flush=True)
        src = jsource(NAMES[job["name"]]) + job["j"]
        kernel = jay.j.compile(src).bind(job["data"])
        t_lj, v_lj = best_of(kernel, args.repeat)
        t_np, v_np = best_of(job["numpy"], args.repeat)

        a, b = flat(v_lj), flat(v_np)
        if a.shape != b.shape or not np.allclose(a, b, rtol=1e-8, atol=1e-10):
            print(f"  {job['name']}: libjay and numpy disagree", file=sys.stderr)

        t_nb = best_of(job["numba"], args.repeat)[0] if job["numba"] else None
        print(f"    libjay {t_lj * 1e3:.1f} ms, numpy {t_np * 1e3:.1f} ms; jconsole next",
              file=sys.stderr, flush=True)
        t_jc = jconsole(
            RANDOM_J + jsource(NAMES[job["name"]]) + job["setup"], job["timed"], args.repeat
        )
        rows.append((job["name"], job["size"], job["j"], t_lj, t_np, t_nb, t_jc))

    print("| algorithm | size | J | libjay | numpy | numba | jconsole | libjay vs jconsole |")
    print("|---|---|---|---:|---:|---:|---:|---:|")
    for name, size, j, lj, npy, nb, jc in rows:
        ratio = f"{jc / lj:.2f}x" if jc else "n/a"
        print(f"| {name} | {size} | `{j}` | {ms(lj)} | {ms(npy)} | {ms(nb)} | "
              f"{ms(jc)} | {ratio} |")


if __name__ == "__main__":
    main()
