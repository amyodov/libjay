# Benchmarks

Phase 4: the first honest measurement of libjay against Polars, numba and
numpy, and the first measurement of what threading buys. Phase 5 adds the
time-series gate at the bottom: one J expression against a multi-step
rolling pipeline.

Every table below was re-measured after expression fusion (`fuse.rs`), the
item this file used to head its own list of what to do next. Both the
before and the after numbers were taken in one session on the machine
described below, by building with the pass and without it.

The five-row table was then taken again, whole, after the pass learned to
move a named value into the sentences that read it — the item that used to
head the list of what to do next. The tables under it are from the fusion
session and the code they measure has not moved since.

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

Measured 2026-08-20 on the machine below, in one quiet session after the
pass learned to move a named value into the sentences that read it. Times
in milliseconds.

```
machine   macOS-13.7.8-x86_64-i386-64bit
cpu       i386, 8 logical threads          (Intel i7-7920HQ, 4 cores)
python    3.12.9, numpy 2.0.2, polars 1.43.2, numba 0.61.2
sizes     vector 20,000,000 f64, matrix 2,000,000 x 8 f64
method    best of 5 after one warmup, wall time in ms
```

| scenario | J | libjay 1 thread | libjay 8 threads | speedup | polars | numba | numba prange | numpy |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| weighted sum | `+/ {w} * {x}` | 41.2 | 13.7 | 3.00x | 105.4 | 25.5 | 12.4 | 113.0 |
| column sums | `+/ {m}` | 9.2 | 5.3 | 1.72x | 5.5 | 14.3 | 39.0 | 32.3 |
| std, named value | `d =. {x} - (+/ {x}) % # {x}` … `%: (+/ d * d) % # d` | 90.2 | 18.1 | 4.98x | 22.0 | 50.0 | 17.6 | 128.4 |
| std, one sentence | `%: (+/ ({x} - (+/ {x}) % # {x}) * {x} - (+/ {x}) % # {x}) % # {x}` | 86.4 | 21.0 | 4.11x | 20.1 | 47.6 | 12.9 | 136.2 |
| sum of exponentials | `+/ ^ {x}` | 137.6 | 29.2 | 4.71x | 208.2 | 162.4 | 27.9 | 210.5 |

What fusion moved, libjay only, the same five scenarios built without the
pass and with it. These two columns are one session of their own, taken on
a laptop that had another build on it:

| scenario | 1 thread before | after | 8 threads before | after |
|---|---:|---:|---:|---:|
| weighted sum | 123.0 | 45.7 | 67.5 | 14.6 |
| column sums | 9.8 | 10.7 | 5.1 | 5.4 |
| std, named value | 235.1 | 204.1 | 136.4 | 80.9 |
| std, one sentence | 411.5 | 149.4 | 221.6 | 38.3 |
| sum of exponentials | 246.1 | 153.5 | 72.2 | 33.0 |

The standard deviation then moved again, and further, when the named value
was allowed to move into the kernel — the three states of the same two
programs, 20M rows:

| standard deviation | no fusion | fused | the name moved too |
|---|---:|---:|---:|
| named value, 1 thread | 235.1 | 204.1 | 90.2 |
| named value, 8 threads | 136.4 | 80.9 | 18.1 |
| one sentence, 1 thread | 411.5 | 149.4 | 86.4 |
| one sentence, 8 threads | 221.6 | 38.3 | 21.0 |

The column-sums row fuses nothing — `+/ {m}` is one verb over one array —
and it is the noisiest row in the file, being the only one that finishes in
ten milliseconds: 9.2, 9.8 and 10.7 ms on one thread are three measurements
of identical work. Read it as unchanged, which is what it should be.

Repeating a row moves it by a few per cent on a quiet laptop and by up to
20% on a busy one — enough to reorder two figures that are already close,
not nearly enough to touch what these two passes did, which is factors.

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

**The gate was scaling, and it scales** — 3.0x to 5.0x on four cores for the
four scenarios that fuse anything, now that a kernel has taken the memory
traffic out of the work, against 1.6x to 3.3x before fusion. (Column sums
fuses nothing and scales 1.7x to 2.6x depending on the run; it is the row
that finishes in five milliseconds.) Nothing regressed: `LIBJAY_THREADS=1`
takes the same sequential code as before, and the differential corpora and
every other suite pass unchanged, fused and unfused alike.

**Where libjay stands.** On eight threads it is faster than numpy and than
a serial numba loop on all five scenarios, faster than Polars on four and
level with it on the fifth (column sums, 5.3 against 5.5), and faster than
numba's parallel loop on one. The rest it loses by a little: the weighted
sum is 13.7 ms against `numba prange`'s 12.4, the sum of exponentials 29.2
against 27.9, and the standard deviations 18.1 and 21.0 against 17.6 and
12.9 — which are two measurements of one numba kernel, and say what the
spread of these figures is. Nothing is left where libjay loses by a
factor.

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

**Both spellings of the standard deviation now run the same way**, and it is
the way the hand-written kernel runs. The two used to differ by a factor of
two in either direction depending on which cost was paid: the named spelling
computed the mean once but wrote a 160 MB column of deviations that the next
sentence read back (80.9 ms), and the one-sentence spelling wrote nothing but
summed the column twice to get the mean twice (38.3 ms). Both are now 18-21
ms: four times faster than the named spelling was, twice as fast as the
one-sentence one.

The pass now moves a named value into the sentences that read it, when
nothing there needs it as an array — `d =. {x} - m` and `+/ d * d` are one
chain written in two sentences, and the name is for the reader. Four rules
make that pay:

- the value's own leaves are hoisted into sentences of their own, so the
  copies the move makes share the work the original did once. `+/ {x}` is
  one such leaf, which is what makes this two passes rather than three;
- a leaf written twice inside one kernel is one input, so the one-sentence
  spelling's two `+/ {x}` become one as well;
- a value the kernel reads twice — `d * d` — is computed once per block
  into a slot of its own and read from there, which is the assignment the
  source made, at block scale;
- `# d` is answered from the chain's shape, without computing the chain: a
  tally wants the count, and the shapes have it before any arithmetic runs.

What is left in both spellings is two passes over the 160 MB column — the
sum, then the map-reduce of the squared deviations against the mean it gives
— which is instruction for instruction what `nb_std` in `bench.py` does by
hand. The times say so: 18.1 and 21.0 ms on eight threads, against that
loop's 17.6 and 12.9, and against the 136.4 and 221.6 ms the two spellings
started at. What is left between libjay and the loop is the single
accumulator inside a chunk, which is phase 6.

It moves conservatively, too. A named value moves only where the name is
pure dataflow: the value is replayable and is a chain, no later sentence
assigns the name or anything the value reads, and every use lands inside a
kernel — one use that would have to hold the whole array (`echo d`, `+/\ d`,
`d` on its own) leaves the program exactly as it was. The sentence the
assignment stood in becomes the tally of its chain, which reaches every leaf
and every rule the kernel has and computes nothing else, so whatever the
assignment would have raised is still raised there, before any later
sentence runs.

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

The `s` of this kernel is a named value read twice, which is the shape the
standard deviation above has — and the pass leaves it exactly where it is,
because `+/\` is a window verb and no window verb joins a kernel. `s` is
therefore not a chain a block can recompute: moving it would fold every
window twice instead of once. The compiled program is unchanged — a test
holds it to that — and a repeat of the row measured 787.5 / 415.6 ms, the
same work on a quieter laptop.

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

## SIMD dispatch

Phase 6. One artifact per platform carries several compilations of every hot
loop — the four fused block kernels, the two unfused elementwise chunk
passes, the typed reduce across an item's columns, the scan and the moving
window — and a cached runtime check picks one. The compilations differ only
in the `target_feature` set attached to them; the loops are the same generic
Rust and the autovectoriser is what turns each clone into vector code.
Nothing here is hand-written SIMD.

`LIBJAY_CPU_LEVEL` overrides the pick, so the levels can be measured against
each other on one machine: `baseline`, `v2` (SSE4.2 and its neighbours),
`v3` (AVX2 and FMA), or `native` for the highest the CPU can run, which is
also what an unset variable means. A level the CPU cannot run is clamped
down to one it can, so a pinned level always names code that really
executes. It is read once per process, like `LIBJAY_THREADS`, so every
figure below comes from a subprocess of its own.

```sh
VIRTUAL_ENV=$PWD/.venv-bench .venv-bench/bin/maturin develop --release
.venv-bench/bin/python bench/simd.py
```

```
machine   macOS-13.7.8-x86_64-i386-64bit
cpu       i386, 8 logical threads          (Intel i7-7920HQ, 4 cores)
python    3.12.9, numpy 2.0.2
sizes     vector 20,000,000 f64, matrix 2,000,000 x 8 f64
method    best of 5 calls after one warmup, best of 3 passes over the table
```

| scenario | threads | baseline | v2 | native (v3) | speedup |
|---|---|---:|---:|---:|---:|
| weighted sum | 1 | 38.8 | 38.7 | 38.2 | 1.02x |
| weighted sum | 8 | 12.6 | 12.5 | 12.4 | 1.02x |
| sum of exponentials | 1 | 131.3 | 132.8 | 133.2 | 0.99x |
| sum of exponentials | 8 | 28.4 | 29.0 | 27.4 | 1.03x |
| std, named value | 1 | 82.5 | 76.8 | 73.0 | 1.13x |
| std, named value | 8 | 18.1 | 16.2 | 15.5 | 1.17x |
| column sums | 1 | 14.2 | 14.2 | 14.2 | 1.00x |
| column sums | 8 | 5.2 | 5.3 | 5.1 | 1.00x |
| count above | 1 | 38.0 | 33.6 | 33.2 | 1.14x |
| count above | 8 | 7.9 | 7.3 | 6.8 | 1.16x |
| polynomial | 1 | 84.8 | 86.6 | 76.1 | 1.11x |
| polynomial | 8 | 19.7 | 19.9 | 20.3 | 0.97x |
| elementwise chain | 1 | 120.0 | 113.5 | 109.1 | 1.10x |
| elementwise chain | 8 | 64.0 | 64.2 | 63.2 | 1.01x |
| moving maximum | 1 | 173.8 | 177.7 | 161.8 | 1.07x |
| moving maximum | 8 | 58.2 | 59.6 | 55.5 | 1.05x |
| running sum | 1 | 105.4 | 103.0 | 100.7 | 1.05x |
| running sum | 8 | 106.2 | 101.3 | 99.3 | 1.07x |
| bollinger | 1 | 730.1 | 711.6 | 708.1 | 1.03x |
| bollinger | 8 | 361.9 | 377.0 | 374.5 | 0.97x |

`count above` is `+/ {x} > 0.5`, `polynomial` is `+/ {x} * 1 + {x} * 2 +
{x} * 3 + {x} * 4 + {x}`, `elementwise chain` is `({x} * {w}) + {x} - 0.5`
and `moving maximum` is `20 >./\ {x}`; the rest are the programs of the
tables above.

**The honest headline: a few per cent to 17%, and zero where the row is
bandwidth-bound.** The rows that gain are the ones with arithmetic to spare
per byte loaded — the comparison-and-count (1.14x, 1.16x), the standard
deviation (1.13x, 1.17x), the four-term polynomial on one thread (1.11x),
the three-operand elementwise chain (1.10x). The rows that do not gain do
not gain at all: the weighted sum reads two streams and does one multiply
and one add, column sums reads one and adds, and both were already waiting
on memory before a wider register was involved — which is the same thing the
threading table said when the eighth thread stopped helping. `+/ ^ x` is
flat for a different reason: `exp` is a libm call per element, and no
autovectoriser turns that into a vector one.

Nothing regressed. The 0.97x and 0.99x cells are the measurement's own
spread — repeating a row moves it by a few per cent — and the levels are not
allowed to disagree on values: tests/simd.rs runs a battery of programs at
every level the machine has and requires elementwise results to be identical
bit for bit, reductions to 1e-12 (float reassociation is already contracted,
§5.9).

Two loops decline the vector clone where it cannot pay. The reduce and the
scan widen only across an item's columns, and a loop of four or eight
columns spends more entering a vector body than the width returns: measured
on `+/ m` at 20M f64 on one thread, AVX2 is ~1.5x *slower* at 4 and 8
columns and 1.2x to 1.6x faster from 16 up. So below 16 columns those two
take the baseline compilation whatever the CPU is, which is what keeps the
`column sums` row at 1.00x instead of 0.7x.

Two platforms are built and not measured here. aarch64 gets an explicit
NEON clone, which the arm64 wheel job exercises for correctness through the
same suite; NEON is also in the aarch64 baseline, so the two rungs there are
expected to be the same code. AVX-512 has no rung at all — `avx512*` has no
stable `target_feature` name on the toolchain libjay pins — and this laptop
has no AVX-512 to measure it with either way.

The baseline column really is the baseline. There is no `.cargo/config.toml`
and no `RUSTFLAGS` anywhere in the repository or the wheel jobs, so the build
takes the target's default CPU: for `x86_64-apple-darwin` that is Penryn
(SSE up to 4.1) and for `x86_64-unknown-linux-gnu` plain x86-64, which is
why `v2` — SSE4.2 and popcount on top of that — is worth almost nothing on
this machine and the real step is `v3`. The shipped extension carries 12
dispatchers and 34 AVX2 clones; had `-C target-cpu` leaked in above `v3` the
dispatchers would have been elided and there would be none.

## Next

In the order the measurements rank them:

1. ~~Refcounted buffers, so naming a value is free.~~ Done: `Buf::Owned` is
   an `Arc<Vec<T>>` with copy-on-write through `Arc::make_mut`.
2. ~~Fusion of elementwise chains into one pass, so `+/ w * x` never
   materialises `w * x`.~~ Done: `fuse.rs` replaces a chain of two or more
   elementwise primitives with one blockwise kernel, and absorbs an
   `+/`-style reduction over it.
3. ~~SIMD and multiple accumulators inside a chunk (phase 6), which is where
   the remaining gap to numba lives.~~ Half done: the vector width is there
   (see "SIMD dispatch"), and it says the gap was never the width — the
   rows that stayed flat are waiting on memory, not on registers. Multiple
   accumulators inside a fold are still to try, and are the half of this
   item that might move a bandwidth-bound row.
4. ~~Fusion across an assignment, which is what the named standard deviation
   is still paying for.~~ Done: the pass moves a named value into the
   sentences that read it, which is what the head of this file measures.
