# Benchmarks

Phase 4: the first honest measurement of libjay against Polars, numba and
numpy, and the first measurement of what threading buys. Phase 5 adds the
time-series gate at the bottom: one J expression against a multi-step
rolling pipeline.

Every table below was re-measured after expression fusion (`fuse.rs`), the
item this file used to head its own list of what to do next. Both the
before and the after numbers were taken in one session on the machine
described below, by building with the pass and without it.

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
| weighted sum | `+/ {w} * {x}` | 45.7 | 14.6 | 3.12x | 118.7 | 26.0 | 13.1 | 127.8 |
| column sums | `+/ {m}` | 14.2 | 5.4 | 2.62x | 5.6 | 14.1 | 28.8 | 33.4 |
| std, named value | `d =. {x} - (+/ {x}) % # {x}` … `%: (+/ d * d) % # d` | 204.1 | 80.9 | 2.52x | 23.5 | 51.4 | 13.6 | 138.0 |
| std, one sentence | `%: (+/ ({x} - (+/ {x}) % # {x}) * {x} - (+/ {x}) % # {x}) % # {x}` | 149.4 | 38.3 | 3.90x | 23.1 | 52.0 | 17.5 | 149.1 |
| sum of exponentials | `+/ ^ {x}` | 153.5 | 33.0 | 4.66x | 237.1 | 167.2 | 27.4 | 217.2 |

What fusion moved, libjay only, the same five scenarios built without the
pass and with it:

| scenario | 1 thread before | after | 8 threads before | after |
|---|---:|---:|---:|---:|
| weighted sum | 123.0 | 45.7 | 67.5 | 14.6 |
| column sums | 9.8 | 10.7 | 5.1 | 5.4 |
| std, named value | 235.1 | 204.1 | 136.4 | 80.9 |
| std, one sentence | 411.5 | 149.4 | 221.6 | 38.3 |
| sum of exponentials | 246.1 | 153.5 | 72.2 | 33.0 |

The column-sums row fuses nothing — `+/ {m}` is one verb over one array —
and it is the noisiest row in the file, being the only one that finishes in
ten milliseconds. Its two figures here are the best of five separate runs
of that scenario alone; the 14.2 in the table above is the same work
measured once, while another build was running on the same laptop. Read it
as unchanged, which is what it should be.

The whole session was measured on a laptop that had other work on it, and
repeating a row moves it by up to 20% — enough to reorder two figures that
are already within a few per cent of each other, not nearly enough to touch
what fusion did, which is factors.

Scaling, weighted sum, 20M rows:

| LIBJAY_THREADS | time (ms) | speedup over 1 |
|---:|---:|---:|
| 1 | 45.3 | 1.00x |
| 2 | 24.3 | 1.87x |
| 4 | 15.8 | 2.86x |
| 8 | 14.9 | 3.04x |

Four physical cores, so 4 threads is where the scaling ends. The eighth
thread now adds a little rather than nothing (2.86x to 3.04x): a fused
weighted sum reads two streams and writes none, so a hyperthread has
arithmetic to hide behind the loads, which the unfused three-stream version
did not.

## What the numbers say

**The gate was scaling, and it scales** — 2.5x to 4.7x on four cores now
that fusion has taken memory traffic out of the work, against 1.6x to 3.3x
before it. Nothing regressed: `LIBJAY_THREADS=1` takes the same sequential
code as before, and the differential corpora and every other suite pass
unchanged, fused and unfused alike.

**Where libjay stands.** On eight threads it is faster than numpy on all
five scenarios, than a serial numba loop on four, than Polars on three, and
than numba's parallel loop on one (column sums). The two it lost by a wide
margin before fusion it now loses by a little: the weighted sum is 14.6 ms
against `numba prange`'s 13.1 and the sum of exponentials 33.0 against 27.4,
both of which repeat runs put within a few per cent either way. The real
remaining gap is the standard deviations, where numba's one pass computes
the mean and the variance together and libjay reduces twice.

**Fusion is what moved them.** `+/ {w} * {x}` used to materialise the 160 MB
product and read it back to reduce it: three streams of memory traffic where
numba's loop has two and keeps each product in a register. The pass now
finds the chain at compile time and evaluates it a block at a time — 123.0
to 45.7 ms on one thread, 67.5 to 14.6 on eight, and 2.7x to 4.6x wherever
a chain of two or more elementwise verbs exists.

It fuses conservatively. A verb joins a kernel only if it cannot fail on
numeric data, which keeps square root, logarithm, dyadic power, APL's `÷`
and APL's `~` out and breaks the chain at each of them; it insists that its
non-scalar arguments have identical shapes and that one working type holds
every value in the chain; and an integer step that overflows throws the
block away. Anything it will
not do, the chain it replaced does instead, so a fused program computes
exactly what the unfused one computes — the same bits, for everything but a
regrouped float reduction — and reports the same errors in the same places.

What is left is *No SIMD*: a reduction is a serial dependency chain
— one f64 add every four cycles per thread — because a compiler may not
reassociate floats on its own. libjay may (§5.9) and does so across chunks,
but inside a chunk it still runs a single accumulator. That is phase 6.

**The one-sentence standard deviation now beats the named one**, which is
the other way round from before fusion, and both are right for their own
reason. Naming a value has been free since owned buffers became refcounted
(`Buf::Owned` is an `Arc<Vec<T>>`, copy-on-write through `Arc::make_mut`,
which took this scenario from 703.1 / 532.1 ms to 235.1 / 130.2 with no new
parallelism), so the named spelling's advantage is that it computes the mean
once. But `d =. {x} - mean` is a single verb over an array: there is no
chain to fuse, so it writes a 160 MB temporary that the next sentence reads
back. The one-sentence spelling computes the mean twice and then runs
`+/ (x - mean) * (x - mean)` as one fused map-reduce that writes nothing at
all — 38.3 ms against 80.9. Fusing across an assignment, so that a named
value can stay in the block too, is the obvious next thing here and is not
done.

**Streaming elementwise passes do not benefit from threads on this
machine.** One core already saturates the memory bus for a two-in/one-out
f64 pass, and the fresh result buffer of every pass has to be faulted in by
the kernel, which parallelises badly. The sweep below shows the multiply
sitting at 0.8-1.0x however many threads it gets, while the same-sized
`exp` gains 2.5-3.6x. Both are kept on the parallel path: the loss is small
and bounded, the win is large, and on a machine with more memory bandwidth
than this laptop the multiply gains too. A fused chain is a different case
and does scale — the weighted sum reaches 3.0x on eight threads — because
it reads its arguments once and writes one result instead of one buffer per
verb.

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

Fusion does not appear in this table and the numbers above are the phase-4
ones: all three sweep kernels are a single verb, and a single verb already
runs as one pass, so the pass leaves them alone. Re-run with fusion on,
every row repeated within its own noise (the largest move was `+/ {x}` at
1M elements, 6.03x to 6.48x).

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
| Bollinger z-score, window 20 | 822.2 | 437.4 | 1.88x | 767.6 | 200.5 | 130.7 | 1484.9 |

| LIBJAY_THREADS | time (ms) | speedup over 1 |
|---:|---:|---:|
| 1 | 849.0 | 1.00x |
| 2 | 547.3 | 1.55x |
| 4 | 412.2 | 2.06x |
| 8 | 436.7 | 1.94x |

Without fusion, in the same session: 1288.3 / 675.5 ms, which is the
phase-5 measurement (1307.3 / 687.2) repeated within 2%.

**The gate is met with room to spare**: 437 ms against Polars' 768, where
before fusion it was 675 against 748. The two still agree to 8.7e-10
relative over all 20M rows — the harness fails outright if they do not, and
the printed differences below are unchanged to the last digit, because a
fused map does the same arithmetic in the same order per element. Four
physical cores again cap the scaling at 4 threads.

**Where the time goes.** The kernel is about ten passes over a 160 MB
column: the two moving sums, the squares, and seven pieces of elementwise
arithmetic. Each pass is memory-bound at roughly 5 ms per 2M elements, and
the moving sums cost about one and a half passes each. Polars needs four.
libjay wins because a moving sum is cheaper than a rolling aggregation with
a null mask, and because both of its window folds are split across threads.
Fusion then took the eight elementwise passes down to four: two kernels of
three verbs each, one either side of the `%:`, plus `%:` and `*:` on their
own — the first because a square root can fail on a negative argument and
never joins a kernel, the second because a single verb is already one pass.
That is where the 238 ms went.

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
table, which is what one fused loop over the whole expression buys against
four passes over 160 MB.

## Next

In the order the measurements rank them:

1. ~~Refcounted buffers, so naming a value is free.~~ Done: `Buf::Owned` is
   an `Arc<Vec<T>>` with copy-on-write through `Arc::make_mut`.
2. ~~Fusion of elementwise chains into one pass, so `+/ w * x` never
   materialises `w * x`.~~ Done: `fuse.rs` replaces a chain of two or more
   elementwise primitives with one blockwise kernel, and absorbs an
   `+/`-style reduction over it.
3. SIMD and multiple accumulators inside a chunk (phase 6), which is where
   the remaining gap to numba lives.
4. Fusion across an assignment, which is what the named standard deviation
   is still paying for: `d =. {x} - mean` is one verb, so it materialises a
   160 MB column that the next sentence reads back twice.
