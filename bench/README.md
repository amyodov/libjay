# Benchmarks

Phase 4: the first honest measurement of libjay against Polars, numba and
numpy, and the first measurement of what threading buys. Phase 5 adds the
time-series gate at the bottom: one J expression against a multi-step
rolling pipeline.

```sh
uv pip install --python .venv-bench maturin
VIRTUAL_ENV=$PWD/.venv-bench .venv-bench/bin/maturin develop --release
.venv-bench/bin/python bench/bench.py            # the phase-4 table
.venv-bench/bin/python bench/sweep.py            # the threshold sweep
.venv-bench/bin/python bench/timeseries.py       # the phase-5 gate
```

Both virtualenvs install the extension in place, at `python/jay/_jay.abi3.so`,
so a plain `maturin develop` from the dev environment overwrites the release
build with a debug one. Run the release build again before measuring
anything.

`bench.py` measures the rivals in its own process and every libjay number in
a subprocess, because the thread count is fixed the first time the pool is
used: `worker.py` is that subprocess. Each figure is the best wall time of
five calls after a warmup (numba is JIT-warmed by the same warmup call), and
every implementation's result is checked against libjay's, so the table
compares work actually done.

The J is the whole program, compiled once and called with the data bound —
what an embedding caller would write. numba's kernels are hand-written
loops; `numba prange` is the same loop with `parallel=True`. Polars runs
multi-threaded by default; numpy and plain numba are single-threaded.

## Results

Measured 2026-08-20 on the machine below. Times in milliseconds.

```
machine   macOS-13.7.8-x86_64-i386-64bit
cpu       i386, 8 logical threads          (Intel i7-7920HQ, 4 cores)
python    3.12.9, numpy 2.0.2, polars 1.43.2, numba 0.61.2
sizes     vector 20,000,000 f64, matrix 2,000,000 x 8 f64
method    best of 5 after one warmup, wall time in ms
```

| scenario | J | libjay 1 thread | libjay 8 threads | speedup | polars | numba | numba prange | numpy |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| weighted sum | `+/ {w} * {x}` | 119.1 | 74.1 | 1.61x | 109.4 | 25.4 | 12.4 | 106.4 |
| column sums | `+/ {m}` | 9.2 | 5.1 | 1.82x | 5.7 | 13.3 | 13.6 | 35.3 |
| std, named value | `d =. {x} - (+/ {x}) % # {x}` … `%: (+/ d * d) % # d` | 235.1 | 130.2 | 1.81x | 22.2 | 49.6 | 12.5 | 132.3 |
| std, one sentence | `%: (+/ ({x} - (+/ {x}) % # {x}) * {x} - (+/ {x}) % # {x}) % # {x}` | 375.1 | 195.3 | 1.92x | 20.9 | 49.6 | 12.5 | 120.5 |
| sum of exponentials | `+/ ^ {x}` | 217.7 | 65.5 | 3.32x | 205.5 | 116.7 | 26.5 | 207.6 |

The named-value row was re-measured after owned buffers became refcounted
(see below); it read 703.1 / 532.1 / 1.32x before that change, and the rest
of the table is unchanged by it (`+/ ^ {x}`, re-measured as a control on the
same machine, repeated within 2%).

Scaling, weighted sum, 20M rows:

| LIBJAY_THREADS | time (ms) | speedup over 1 |
|---:|---:|---:|
| 1 | 120.7 | 1.00x |
| 2 | 80.1 | 1.51x |
| 4 | 62.2 | 1.94x |
| 8 | 67.2 | 1.80x |

Four physical cores, so 4 threads is where the scaling ends; the eighth
thread shares an execution unit with the fourth and this work is not the
kind that fills the gaps.

## What the numbers say

**The gate was scaling, and it scales** — 1.6x to 3.3x on four cores,
close to linear (1.94x on 4 threads) where the work is arithmetic rather
than memory traffic. Nothing regressed: `LIBJAY_THREADS=1` takes the same
sequential code as before, and the 479-expression differential corpus and
every other suite pass unchanged.

**Where libjay stands.** On eight threads it is the fastest of the five on
column sums (5.1 ms against Polars' 5.7 and numpy's 35.3), it beats Polars
and numpy on three scenarios of five, and it beats a serial numba loop on
two. It loses to numba's parallel loop on four of five, and both
standard-deviation spellings still trail Polars and numba by a wide margin
(the named one now edges past numpy). Two structural reasons, neither of them
a threading problem, and neither addressed by this phase:

* *No fusion.* `+/ {w} * {x}` materialises the 160 MB product before
  reducing it; numba's loop keeps each product in a register. Three streams
  of memory traffic against one. Fusing an elementwise chain into one pass
  is the single biggest win available.
* *No SIMD.* A reduction is a serial dependency chain — one f64 add every
  four cycles per thread, which is what the 1.2 ns per element of the
  one-thread `+/` measures — because a compiler may not reassociate floats
  on its own. libjay may (§5.9) and does so across chunks, but inside a
  chunk it still runs a single accumulator.

**Naming a value used to cost a copy; now it is free.** `d =. …` and every
later mention of `d` each deep-copied the array — one 16 MB copy on the
assignment and another on each of the two uses, about 16 ms and 19 ms apiece
on 20M rows — which made the named spelling of the standard deviation 330 ms
slower than the one-sentence spelling that computes the same thing twice.
`Buf::Owned` now holds an `Arc<Vec<T>>`: cloning is a refcount bump, and
`to_mut` goes through `Arc::make_mut`, so a copy happens only when someone
writes to a buffer that is still shared. Copy-on-write semantics are exactly
as before — no caller could tell, and no call site changed — but the scenario
went from 703.1 / 532.1 ms (1 / 8 threads) to 235.1 / 130.2, a 3.0x and 4.1x
win with no new parallelism. Naming a value is now cheaper than repeating the
expression, which is the right way round: at 130.2 ms the named spelling beats
the one-sentence spelling's 195.3, because the mean is computed once.

**Streaming elementwise passes do not benefit from threads on this
machine.** One core already saturates the memory bus for a two-in/one-out
f64 pass, and the fresh result buffer of every pass has to be faulted in by
the kernel, which parallelises badly. The sweep below shows the multiply
sitting at 0.8-1.0x however many threads it gets, while the same-sized
`exp` gains 2.5-3.6x. Both are kept on the parallel path: the loss is small
and bounded, the win is large, and on a machine with more memory bandwidth
than this laptop the multiply gains too.

## The parallel threshold

`par::MIN_WORK` is 65,536 element operations: below it nothing is split.
`bench/sweep.py`, same machine, best of 100 calls:

| elements | kernel | 1 thread (us) | 8 threads (us) | speedup |
|---:|---|---:|---:|---:|
| 4,096 | `{w} * {x}` | 13.4 | 14.1 | 0.95x |
| 4,096 | `^ {x}` | 28.7 | 30.3 | 0.95x |
| 4,096 | `+/ {x}` | 10.7 | 11.2 | 0.95x |
| 16,384 | `{w} * {x}` | 22.7 | 24.6 | 0.92x |
| 16,384 | `^ {x}` | 93.6 | 98.8 | 0.95x |
| 16,384 | `+/ {x}` | 24.0 | 26.0 | 0.92x |
| 32,768 | `{w} * {x}` | 32.2 | 35.5 | 0.91x |
| 32,768 | `^ {x}` | 178.8 | 191.9 | 0.93x |
| 32,768 | `+/ {x}` | 41.6 | 45.4 | 0.92x |
| 65,536 | `{w} * {x}` | 55.0 | 67.5 | 0.81x |
| 65,536 | `^ {x}` | 339.5 | 134.4 | 2.53x |
| 65,536 | `+/ {x}` | 76.6 | 39.4 | 1.95x |
| 131,072 | `{w} * {x}` | 89.4 | 93.8 | 0.95x |
| 131,072 | `^ {x}` | 663.2 | 205.7 | 3.22x |
| 131,072 | `+/ {x}` | 141.9 | 49.2 | 2.88x |
| 262,144 | `{w} * {x}` | 235.3 | 253.8 | 0.93x |
| 262,144 | `^ {x}` | 1285.5 | 372.0 | 3.46x |
| 262,144 | `+/ {x}` | 284.0 | 64.6 | 4.40x |
| 1,048,576 | `{w} * {x}` | 2523.6 | 2549.5 | 0.99x |
| 1,048,576 | `^ {x}` | 6958.9 | 1951.6 | 3.57x |
| 1,048,576 | `+/ {x}` | 1234.9 | 204.9 | 6.03x |

Rows under 65,536 run identical code in both columns; the 5-8% gap there is
the bias between two processes, and it is the noise floor for everything
else in the table. From 65,536 up, the split engages: 2.5x for `exp` and
2.0x for the reduction on the first size that takes it, against 0.81x for
the bandwidth-bound multiply. That is the trade the threshold is set to
make. A reduction over a million elements reaches 6x on eight threads —
more than the four cores — because the sequential fold is latency-bound and
chunking gives the machine independent chains to interleave.

## Phase 5: one expression against a rolling pipeline

The phase-5 gate is a Bollinger z-score over a synthetic OHLCV close series
— a reflected random walk in log price, 20M rows of f64, window 20:

    z[i] = (close[i] - mean(close[i-19 .. i])) / std(close[i-19 .. i])

Polars spells it as a rolling mean, a rolling standard deviation and the
arithmetic between them. libjay spells it as one compiled kernel, in which
everything is a moving sum:

```j
s =. 20 +/\ {close}
((20 * 19 }. {close}) - s) % %: (20 * 20 +/\ *: {close}) - s * s
```

`s` is the moving sum of the closes and the second `+/\` is the moving sum
of their squares, so the standard deviation comes out of `20Q - s²` without
a second pass over the windows; `19 }. {close}` drops the 19 leading closes
the windows do not cover, which is what aligns the two sides of the
subtraction. Both moving sums take the O(n) window path.

`bench/timeseries.py`, same machine, 20M rows, best of 5 after a warmup:

| kernel | libjay 1 thread | libjay 8 threads | speedup | polars | numba | numba prange | numpy |
|---|---:|---:|---:|---:|---:|---:|---:|
| Bollinger z-score, window 20 | 1307.3 | 687.2 | 1.90x | 758.7 | 193.6 | 141.4 | 1494.5 |

| LIBJAY_THREADS | time (ms) | speedup over 1 |
|---:|---:|---:|
| 1 | 1303.0 | 1.00x |
| 2 | 898.5 | 1.45x |
| 4 | 646.1 | 2.02x |
| 8 | 661.2 | 1.97x |

**The gate is met**: 687 ms against Polars' 759 ms, and the two agree to
8.7e-10 relative over all 20M rows (the harness fails outright if they do
not). Four physical cores again cap the scaling at 4 threads.

**Where the time goes.** The kernel is about ten passes over a 160 MB
column: the two moving sums, the squares, and seven pieces of elementwise
arithmetic. Each pass is memory-bound at roughly 5 ms per 2M elements, and
the moving sums cost about one and a half passes each. Polars needs four.
libjay wins anyway because a moving sum is cheaper than a rolling
aggregation with a null mask, and because both of its window folds are split
across threads. Fusing the elementwise chain — still the biggest item on the
list below — would roughly halve what is left.

**Precision is the reason the window fold costs two passes.** A moving sum
is usually written as one accumulator slid along the series, or as a
cumulative sum differenced; both carry the drift of the whole series into
every window. libjay instead cuts the series into blocks of the window
length, folds each block from both ends once, and combines one block's
suffix with the next block's prefix — so no accumulator ever runs for more
than 20 steps and each window's error is the error of computing that window
on its own. The harness prints what that buys, against libjay:

```
  polars        max abs 1.52e-09, max rel 8.74e-10 over |z| >= 1e-3
  numba         max abs 3.13e-06, max rel 2.39e-06 over |z| >= 1e-3
  numba prange  max abs 1.85e-09, max rel 1.52e-09 over |z| >= 1e-3
  numpy         max abs 2.68e-03, max rel 1.40e-03 over |z| >= 1e-3
```

`numba` is the natural hand-rolled loop with a sliding accumulator and
`numpy` is the cumulative-sum-and-difference spelling; both lose three to
twelve digits by 20M rows. `numba prange` recomputes every window from
scratch — 20x the arithmetic, and it is still the fastest thing in the
table, which is what an unfused interpreter costs against a fused loop.

## Next

In the order the measurements rank them:

1. ~~Refcounted buffers, so naming a value is free.~~ Done: `Buf::Owned` is
   an `Arc<Vec<T>>` with copy-on-write through `Arc::make_mut`.
2. Fusion of elementwise chains into one pass, so `+/ w * x` never
   materialises `w * x`.
3. SIMD and multiple accumulators inside a chunk (phase 6), which is where
   the remaining gap to numba lives.
