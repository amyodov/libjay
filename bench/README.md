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

It was taken a third time after the reduction, cell and boundary pass that
"Reductions, cells and the boundary" below measures. That section is the one
to read for what moved and by how much: the five-row table is one script run
of five figures and the pass is measured there against a baseline built from
the same commit in the same session, program by program.

```sh
uv pip install --python .venv-bench maturin
VIRTUAL_ENV=$PWD/.venv-bench .venv-bench/bin/maturin develop --release
.venv-bench/bin/python bench/bench.py            # the phase-4 table
.venv-bench/bin/python bench/sweep.py            # the threshold sweep
.venv-bench/bin/python bench/timeseries.py       # the phase-5 gate
.venv-bench/bin/python bench/windows.py          # the windowed sentences
```

Both virtualenvs install the extension in place, at `python/jay/_jay.abi3.so`,
so a plain `maturin develop` from the dev environment overwrites the release
build with a debug one. Run the release build again before measuring
anything.

`rust-toolchain.toml` began pinning a compiler version only with the MSRV
raise to 1.89 (see "SIMD dispatch"); sessions measured before that pin name
no rustc version because none was fixed yet — whatever `rustc stable` gave
on the day is what built them. "SIMD dispatch" names its version explicitly
because the pin is precisely what that section studies.

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

Measured 2026-08-21 on the machine below, in one quiet session after the
reduction, cell and boundary pass. Times in milliseconds.

```
machine   macOS-13.7.8-x86_64-i386-64bit
cpu       i386, 8 logical threads          (Intel i7-7920HQ, 4 cores)
python    3.12.9, numpy 2.0.2, polars 1.43.2, numba 0.61.2
sizes     vector 20,000,000 f64, matrix 2,000,000 x 8 f64
method    best of 5 after one warmup, wall time in ms
```

| scenario | J | libjay 1 thread | libjay 8 threads | speedup | polars | numba | numba prange | numpy |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| weighted sum | `+/ {w} * {x}` | 21.4 | 12.5 | 1.71x | 107.7 | 25.6 | 12.9 | 108.9 |
| column sums | `+/ {m}` | 14.1 | 5.4 | 2.61x | 4.7 | 11.9 | 15.2 | 34.7 |
| std, named value | `d =. {x} - (+/ {x}) % # {x}` … `%: (+/ d * d) % # d` | 40.9 | 14.5 | 2.83x | 21.9 | 48.7 | 12.5 | 135.5 |
| std, one sentence | `%: (+/ ({x} - (+/ {x}) % # {x}) * {x} - (+/ {x}) % # {x}) % # {x}` | 45.3 | 14.7 | 3.08x | 21.9 | 49.1 | 12.9 | 143.4 |
| sum of exponentials | `+/ ^ {x}` | 119.3 | 27.2 | 4.39x | 210.7 | 166.1 | 28.0 | 206.7 |

The one-thread column is the one that moved: a reduction now keeps eight
accumulators in flight instead of one, and that is worth two to four times
on a fold that used to be a chain of dependent adds. The eight-thread
column barely moved because it was already waiting on memory — see "Where
the bandwidth is" below — and the speedup columns therefore *fell*, which
is what a faster denominator does. Nothing in the table got slower.

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
| 1 | 23.1 | 1.00x |
| 2 | 17.1 | 1.35x |
| 4 | 13.7 | 1.68x |
| 8 | 14.5 | 1.59x |

Four physical cores, so 4 threads is where the scaling ends. This table used
to read 45.3 / 24.3 / 15.8 / 14.9, or 3.04x on eight threads. Every figure
in it is the same or better and the *speedup* halved, because one thread now
reads 320 MB at 13.8 GB/s where it used to read it at 7.1: there is only so
much bandwidth left for the other seven to add.

## What the numbers say

**The gate was scaling, and it scaled** — 3.0x to 5.0x on four cores when
fusion landed, against 1.6x to 3.3x before it. It reads 1.7x to 4.4x now,
and the reason is in the one-thread column: a reduction on one thread got
two to four times faster, so there is less left for the other threads to
take. Both columns are the same or better than they have ever been.

**Where libjay stands.** On eight threads it is faster than numpy and than
a serial numba loop on all five scenarios, faster than Polars on four and
level with it on the fifth (column sums, 5.4 against 4.7), and level with or
faster than numba's parallel loop on three. What it still loses it loses by
a little: the standard deviations are 14.5 and 14.7 ms against `numba
prange`'s 12.5 and 12.9. Nothing is left where libjay loses by a factor.

**On one thread it is now faster than a serial numba loop on four of the
five**, which it was not before: the weighted sum is 21.4 ms against 25.6
and the two standard deviations 40.9 and 45.3 against 48.7 and 49.1.

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

What was left after fusion was *No SIMD*: a reduction was a serial
dependency chain — one f64 add every four cycles per thread — because a
compiler may not reassociate floats on its own. libjay may (§5.9) and did so
across chunks, but inside a chunk it ran a single accumulator. Phase 6 then
answered it in two halves, the vector width and the lanes; "Reductions,
cells and the boundary" is the second.

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
hand. The times say so: 14.5 and 14.7 ms on eight threads, against that
loop's 12.5 and 12.9, and against the 136.4 and 221.6 ms the two spellings
started at. What was left between libjay and the loop was the single
accumulator inside a chunk; there are eight of them now, and on one thread
the same two spellings are 40.9 and 45.3 ms against the loop's 48.7 and
49.1.

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
| Bollinger z-score, window 20 | 794.4 | 403.8 | 1.97x | 754.7 | 201.8 | 164.2 | 1458.6 |

| LIBJAY_THREADS | time (ms) | speedup over 1 |
|---:|---:|---:|
| 1 | 794.0 | 1.00x |
| 2 | 500.0 | 1.59x |
| 4 | 490.5 | 1.62x |
| 8 | 403.8 | 2.03x |

The same row measured 822.2 / 437.4 before the reduction, cell and boundary
pass, and the baseline built from the same commit in the same session as the
figures above measured 790.7 / 429.0. On this kernel the pass is worth 1.06x
and no more, because its argument arrives from numpy: the whole gain the
free owned slice buys `19 }. {close}` was already there for a buffer libjay
borrows rather than owns. Given the same kernel over an argument libjay owns
— the shape it has inside a longer program — the pass is worth 1.33x on
eight threads and 1.17x on one.

Without fusion, in the same session: 1288.3 / 675.5 ms, which is the
phase-5 measurement (1307.3 / 687.2) repeated within 2%.

The `s` of this kernel is a named value read twice, which is the shape the
standard deviation above has — and the pass left it exactly where it was,
because `+/\` was a window verb and no window verb could join a kernel. `s`
was therefore not a chain a block could recompute: moving it would fold
every window twice instead of once. The compiled program was unchanged — a
test held it to that — and a repeat of the row measured 787.5 / 415.6 ms,
the same work on a quieter laptop. Both of those statements stopped being
true when a window became a step of the kernel; see "Windows in the kernel"
below for what `s` does now and what it is worth.

**The gate is met with room to spare**: 404 ms against Polars' 755, where
before fusion it was 675 against 748, and 231 against 787 once the windows
joined the kernel. The two still agree to 8.7e-10
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

The `%:` that fusion cannot absorb has since got its negative-argument check
off the sequential path, which is what the last 25 ms of this row are. What
was left after that was four unfused passes over 160 MB, and closing it
meant putting a window verb inside a kernel: that is what "Windows in the
kernel" below did, and this row is 231 ms there.

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
`v3` (AVX2 and FMA), `v4` (AVX-512), or `native` for the highest the CPU can
run, which is also what an unset variable means. A level the CPU cannot run is clamped
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
method    best of 5 calls after one warmup, best of 2 passes over the table
```

Re-measured 2026-08-21, after the lanes of "Reductions, cells and the
boundary" gave the folds a shape a vector clone can widen. The compiler is a
variable in this table and these cells were taken on rustc 1.85; a re-run on
1.89, the version the repository now pins, moved individual cells in both
directions and nothing in one direction — but it was taken while the machine
had other work on it, so it is not a replacement, and the table stands as it
was until it can be re-taken on a quiet one:

| scenario | threads | baseline | v2 | native (v3) | speedup |
|---|---|---:|---:|---:|---:|
| weighted sum | 1 | 29.2 | 27.5 | 25.0 | 1.17x |
| weighted sum | 8 | 13.6 | 14.2 | 13.0 | 1.05x |
| sum of exponentials | 1 | 124.7 | 122.1 | 120.5 | 1.03x |
| sum of exponentials | 8 | 27.3 | 29.2 | 30.7 | 0.89x |
| std, named value | 1 | 60.2 | 46.6 | 41.6 | 1.45x |
| std, named value | 8 | 17.3 | 15.6 | 14.9 | 1.16x |
| column sums | 1 | 13.6 | 13.5 | 13.5 | 1.01x |
| column sums | 8 | 5.9 | 5.6 | 5.5 | 1.07x |
| count above | 1 | 119.1 | 92.2 | 91.7 | 1.30x |
| count above | 8 | 23.8 | 19.1 | 17.4 | 1.36x |
| polynomial | 1 | 74.3 | 72.4 | 55.9 | 1.33x |
| polynomial | 8 | 20.0 | 19.7 | 19.5 | 1.02x |
| elementwise chain | 1 | 126.3 | 122.3 | 120.1 | 1.05x |
| elementwise chain | 8 | 69.5 | 76.2 | 68.3 | 1.02x |
| moving maximum | 1 | 185.1 | 185.2 | 180.5 | 1.02x |
| moving maximum | 8 | 61.4 | 66.7 | 64.1 | 0.96x |
| running sum | 1 | 111.1 | 111.9 | 107.0 | 1.04x |
| running sum | 8 | 112.3 | 113.3 | 115.6 | 0.97x |
| bollinger | 1 | 851.5 | 811.6 | 795.0 | 1.07x |
| bollinger | 8 | 404.0 | 417.7 | 412.3 | 1.00x |

`count above` is `+/ {x} > 0.5`, `polynomial` is `+/ {x} * 1 + {x} * 2 +
{x} * 3 + {x} * 4 + {x}`, `elementwise chain` is `({x} * {w}) + {x} - 0.5`
and `moving maximum` is `20 >./\ {x}`; the rest are the programs of the
tables above.

**The honest headline: a few per cent to 45%, and zero where the row is
bandwidth-bound.** The rows that gain are the ones with arithmetic to spare
per byte loaded — the standard deviation (1.45x, 1.16x), the
comparison-and-count (1.30x, 1.36x), the four-term polynomial on one thread
(1.33x), the weighted sum on one thread (1.17x). The rows that do not gain
do not gain at all: on eight threads the weighted sum reads two streams and
does one multiply and one add, column sums reads one and adds, and both are
waiting on memory before a wider register is involved — which is the same
thing the threading table says when the eighth thread stops helping. `+/ ^
x` is flat for a different reason: `exp` is a libm call per element, and no
autovectoriser turns that into a vector one.

The one-thread cells are where this table moved, and they moved because the
lanes moved. The first time it was taken, the standard deviation gained
1.13x from AVX2 and the weighted sum 1.02x, because a fold with one
accumulator has nothing for a wide register to hold; with eight
accumulators the same rows gain 1.45x and 1.17x. Width and independence are
one item, and this is what it looks like when both halves are there.

Nothing regressed. The cells under 1.00x are the measurement's own spread —
repeating a row moves it by a few per cent — and the levels are not allowed
to disagree on values: tests/simd.rs runs a battery of programs at every
level the machine has and requires elementwise results to be identical bit
for bit, reductions to 1e-12 (float reassociation is already contracted,
§5.9).

Some loops decline the vector clone where it cannot pay. The fold across an
item's columns and the scan widen only over the columns, and a loop of four
or eight of them spends more entering a vector body than the width returns:
measured on `+/ m` at 20M f64 on one thread, AVX2 is ~1.5x *slower* at 4 and
8 columns and 1.2x to 1.6x faster from 16 up. So below 16 columns those two
take the baseline compilation whatever the CPU is, which is what keeps the
`column sums` row at 1.01x instead of 0.7x. The row fold that answers
`u/"1` takes the same rule for the same reason — the loop that could widen
is the run it folds — and the flat fold that answers `u/` over a vector has
no such limit, since its lanes are as long as the argument.

Two platforms are built and not measured here. aarch64 gets an explicit
NEON clone, which the arm64 wheel job exercises for correctness through the
same suite; NEON is also in the aarch64 baseline, so the two rungs there are
expected to be the same code. x86-64-v4 — AVX-512 — is the other: since the
MSRV moved to 1.89, where `avx512f` and its neighbours became stable target
features, every x86-64 artifact carries a v4 clone of every annotated loop,
and this laptop's i7-7920HQ has no AVX-512 to run one with. What can be
checked without the hardware has been: `nm` on the shipped extension finds
83 `…avx512bw_avx512cd_avx512dq_avx512f_avx512vl…` clones, exactly as many
as the AVX2 ones, and the disassembly of the release build is full of `zmm`
operands, so the clones are 512-bit code and not v3 recompiled. What has
NOT been checked is whether they are *faster*; no number in this file is a
v4 number. tests/simd.rs prints which levels the machine it runs on can
compare, and CI runs it with `--nocapture`, so the first runner with AVX-512
both says so and puts the clone through the equivalence battery for real.

The baseline column really is the baseline. There is no `.cargo/config.toml`
and no `RUSTFLAGS` anywhere in the repository or the wheel jobs, so the build
takes the target's default CPU: for `x86_64-apple-darwin` that is Penryn
(SSE up to 4.1) and for `x86_64-unknown-linux-gnu` plain x86-64, which is
why `v2` — SSE4.2 and popcount on top of that — is worth almost nothing on
this machine and the real step is `v3`. The shipped extension carries 12
`multiversioned!` dispatchers, 83 AVX2 clones of their bodies and 83 AVX-512
ones; had `-C target-cpu` leaked in above `v3` the dispatchers would have
been elided and there would be none.

## GPU placement

Phase 7, measured 2026-08-21. `bench/device.py` times the same fused
kernels on the CPU and on this machine's GPU, at 20M f64 rows, three ways:
the CPU with the whole thread pool, the device with ordinary arguments (so
every call uploads them), and the device with the arguments already
resident.

```sh
.venv-bench/bin/python bench/device.py
```

The measuring machine is a 2017 MacBook Pro: an AMD Radeon Pro 560 behind
Metal. **Metal has no `double` in shaders**, so `SHADER_F64` is absent and
libjay's f64 chains stay on the CPU there; the only thing measurable on this
machine is the explicitly opted-in f32 path, and the table says so. The CPU
column is f64 and the numpy column is f32, so the two are not the same
computation — the drift column is how far apart the answers ended up.

| kernel | J | libjay CPU (8 threads, f64) | libjay GPU, f32, uploading each call | libjay GPU, f32, resident | speedup, resident | numpy f32 | drift vs CPU |
|---|---|---:|---:|---:|---:|---:|---:|
| weighted sum | `+/ {w} * {x}` | 13.4 | 247.5 | 4.1 | 3.29x | 33.4 | 1.1e-07 |
| std, one sentence | `%: (+/ ({x} - (+/ {x}) % # {x}) * …) % # {x}` | 20.2 | 143.2 | 14.8 | 1.37x | 45.7 | 2.7e-10 |
| polynomial | `+/ 2.5 + {x} * 1.5 + {x} * 0.5 * {x}` | 16.2 | 124.3 | 5.9 | 2.74x | 102.4 | 1.2e-10 |
| sum of exponentials | `+/ ^ {x} % 100` | 30.4 | 127.5 | 4.2 | 7.28x | 76.7 | 9.9e-10 |

What the numbers say:

- **Upload dominates.** Sending 20M elements costs about 120 ms — ten to
  thirty times the kernel itself. A device is worth naming only for data
  that stays there, which is what `upload` and `keep_on_device` are for.
- **Arithmetic per element decides the win.** The weighted sum moves two
  columns and does one multiply: 3.3x, and it is the row where both
  processors spend the most of their time waiting on memory. The polynomial
  does five operations on one column and the sum of exponentials a
  transcendental: 2.7x and 7.3x. The pattern is the same one the SIMD
  section found — the width was never the limit, the memory was — read the
  other way round.
- **The standard deviation is a mixed placement**, and honestly so: the two
  `+/ {x}` passes it needs are single verbs, which the fusion pass does not
  fuse and the device therefore never sees. Only the map-reduce over them
  runs on the GPU, and the row's 1.4x is what that is worth. It used to read
  1.6x, and it fell because the CPU half of it got faster: the two `+/ {x}`
  passes are exactly what "Reductions, cells and the boundary" sped up.

## Reductions, cells and the boundary

Measured 2026-08-21. Five changes, every one of them semantics-preserving:
the Rust suites, the Python suite, `jay-corpus record --check` for both
languages and clippy all pass exactly as they did before.

The figures here do not come from `bench.py`. They come from a harness that
holds two builds of libjay — the commit these changes sit on, and that
commit with them applied — and alternates between the two, so a laptop that
gets busy halfway through moves both columns and not one. Each figure is the best
of five calls, and the pair is the best of three alternating passes. Vector
arguments are 20,000,000 f64 and matrices 2,500,000 x 8, both owned by
libjay rather than borrowed, unless the row says otherwise.

Eight threads:

| program | before | after | |
|---|---:|---:|---:|
| `+/"1 {m}` — 2.5M x 8 | 538.7 | 9.3 | 58.2x |
| `19 }. {x}` | 88.9 | 0.0 | — |
| Bollinger, owned argument | 503.3 | 378.3 | 1.33x |
| `+/ ^ {x}` | 30.0 | 25.9 | 1.16x |
| `>./ {x}` | 6.6 | 5.8 | 1.14x |
| `%: {x}` | 61.9 | 54.6 | 1.13x |
| `+/ {x} > 0.5` | 23.2 | 20.8 | 1.12x |
| `+/ {x} * 1 + {x} * 2 + …` | 19.9 | 18.2 | 1.09x |
| `{c} + {f}` — complex plus float, 2M | 14.8 | 13.7 | 1.08x |
| `+/ {x}` | 6.2 | 5.7 | 1.07x |
| `+/ {w} * {x}` | 12.0 | 12.3 | 0.97x |
| `+/ {m}` — 2.5M x 8 | 6.3 | 6.4 | 0.99x |
| `({x} * {w}) + {x} - 0.5` | 52.6 | 53.0 | 0.99x |
| `20 +/\ {x}`, `20 >./\ {x}`, `+/\ {x}` | 49.0, 47.8, 98.0 | 47.9, 47.8, 95.3 | 1.00-1.03x |
| `- {x}`, `^ {x}`, `*: {x}`, `{w} + {x}` | 48.5, 50.1, 47.0, 50.5 | 48.2, 49.4, 47.0, 49.3 | 1.00-1.02x |

One thread:

| program | before | after | |
|---|---:|---:|---:|
| `+/"1 {m}` — 2.5M x 8 | 1095.1 | 15.7 | 69.7x |
| `19 }. {x}` | 89.6 | 0.0 | — |
| `>./ {x}` | 42.5 | 10.7 | 3.98x |
| `+/ {x}` | 23.2 | 7.9 | 2.94x |
| `d =. …` `%: (+/ d * d) % # d` | 74.2 | 40.9 | 1.81x |
| `+/ {w} * {x}` | 39.1 | 23.2 | 1.68x |
| `+/ {x}` — complex, 2M | 2.7 | 1.7 | 1.59x |
| `+/ {x} * 1 + {x} * 2 + …` | 76.2 | 58.2 | 1.31x |
| `+/ {x} > 0.5` | 143.2 | 117.0 | 1.22x |
| Bollinger, owned argument | 894.0 | 764.2 | 1.17x |
| `+/ ^ {x}` | 138.1 | 118.9 | 1.16x |
| everything else | | | 0.95-1.03x |

**What each change is.**

*An owned slice is a view.* `Buf` had two shapes, a refcounted `Vec` and a
borrowed foreign pointer, and slicing the first copied while slicing the
second did not. It now has a third, a window over a refcounted `Vec`, so
taking a cell or a section out of an owned array copies nothing whichever
kind of array it is. `19 }. {x}` over 160 MB stops being a 160 MB copy and
becomes a pointer and a length. A window holds its whole allocation alive
for as long as it lives, which is what a foreign slice has always done with
its owner, and writing to one copies first, so it still behaves as a private
value.

Two rows of the table are this change on its own — the drop, and most of the
Bollinger kernel, which contains one — but its reach is wider than either:
every cell of every rank-applied verb goes through `Buf::slice`. It is worth
nothing at all when the argument came from numpy or Arrow, because a
borrowed slice was already free, which is why the Bollinger row of the
phase-5 gate above moves by 1.06x where the same kernel over an owned
argument moves by 1.33x.

*A flat associative fold keeps eight accumulators.* A reduction was a chain
of dependent steps — one f64 add every four cycles, per chunk, whatever the
register width — which is the item this file has carried as unfinished since
phase 6, and which the SIMD section correctly said the width would not fix.
Eight lanes fix it: lane `j` takes every eighth element, which is both eight
independent chains and a shape the autovectoriser widens, and the lanes
combine at the end. Only associative steps fold this way, which is the
regrouping a chunked fold already takes (§5.9), and a run shorter than 64
elements keeps its single accumulator and its exact old rounding. The fused
kernel's reduction got the same treatment, one block at a time.

On one thread `+/ {x}` fell from 23.2 ms to 7.9 — 160 MB at 20.3 GB/s
against 6.9 — and `>./ {x}` from 42.5 to 10.7. On eight threads it is worth
almost nothing, because eight chunks of a serial fold were already enough to
saturate the bus.

*`u/"1` folds rows out of the buffer.* A reduction over vector cells was the
general rank machinery: an array per cell, a reduction of it, and 2.5 M
results framed back together — three allocations a row. It is now one pass
that folds every row where it lies. Each row folds right to left, the
insert's own order, so nothing is regrouped and every operation is safe,
including the ones that are not associative; the cases it does not cover
(an empty cell, an integer product that leaves i64, a comparison that
decides its own result type) fall through to the path that always ran.
2.5M x 8: 539 ms to 9.3.

*The table weave takes the pool.* A DataFrame arrives one buffer per column
and libjay works rows-leading, so the elements are woven once at the
boundary. The weave was a `push` per element in the binding crate, which has
no thread pool; it is now `Data::interleave` in the core, which has one.
Isolated, 2.5M x 8 f64: 103.8 ms to 48.8 on eight threads, 103.0 to 86.9 on
one.

*The check that guards a square root reads the whole buffer.* `%: y` is
complex for a negative `y`, so the real path has to know first whether any
element is negative. That check was a short-circuiting `any`, which is
sequential and does not vectorise, and which — since the answer is almost
always no — read the whole buffer anyway. It is now one branch-free pass
that the pool splits: 61.9 ms to 54.6 for `%: {x}` at 20M.

## Layout

Measured 2026-08-21. A table used to be woven into one rows-leading block at
the boundary; it now crosses as its columns and is folded where it lies.
The figures come from `bench/layout.py`, run against two builds of libjay —
the commit these changes sit on, and that commit with them applied, both
compiled by the pinned toolchain (rustc 1.89) — with the passes alternating,
so a laptop that gets busy halfway through moves both columns and not one.
Each figure is the best of five calls after a warmup, and each is the best
of two alternating passes. The table is 2,500,000 x 8 f64 in a polars
DataFrame, and the bind is inside the timed call: the boundary is part of
what is measured, because for a DataFrame it used to be most of it.

| program | before, 1 thread | after | before, 8 threads | after | |
|---|---:|---:|---:|---:|---:|
| `$ {df}` — the boundary alone | 99.7 | 0.0 | 59.8 | 0.0 | — |
| `+/ {df}` — column sums | 116.9 | 8.9 | 64.3 | 6.2 | 10-13x |
| `+/"1 {df}` — row sums | 114.3 | 22.0 | 69.0 | 12.5 | 5.2-5.5x |
| `+/"1 \|: {df}` — transposed, then row sums | 516.1 | 128.2 | 475.5 | 67.7 | 4.0-7.0x |
| `+/ ({df} * {df}) + 1` — a fused chain | 309.6 | 308.2 | 186.3 | 184.5 | 1.00x |
| `+/ , {df}` — a verb that wants the rows | 108.9 | 107.5 | 65.7 | 63.1 | 1.02x |
| Bollinger over one column of the frame | 53.9 | 48.0 | 34.9 | 34.1 | 1.02x |

**The boundary is gone, not faster.** `$ {df}` reads no element and now
costs nothing at all: the columns cross borrowed, one Arrow buffer each,
and the value libjay works on is those buffers end to end with a flag
saying so. The weave that used to run there — 100 ms on one thread, 60 on
eight, for every call — runs only when a verb asks for the rows.

**The two folds a table is asked for read the columns where they lie.**
`+/ {df}` folds each column, which is contiguous, so it is a flat fold per
column and nothing is transposed: 13x on one thread, 10x on eight. `+/"1
{df}` folds the rows in one pass that reads the eight columns side by side,
right to left — the insert's own order, no regrouping — for 5.2x and 5.5x.
Both are now doing the arithmetic and nothing else: 6.2 ms is 160 MB read
at 26 GB/s, which is the bus.

**A transpose moves no elements.** `|:` reverses the shape and flips the
flag, so `+/"1 |: {df}` — which used to weave the table, transpose the
result and then fold rows — is now the column fold under another name.

**The rows still cost what they cost.** `+/ , {df}` ravels, which reads the
elements in row-major order, so the weave happens: the same 100 ms it used
to cost, moved from the boundary to the verb that needs it. A fused chain
is the same story for a different reason — the block kernel reads one flat
block, so it joins the columns once — and both rows come out level. What
changed is that a program which needs neither pays for neither.

**A single column never went through the weave** and does not move:
Bollinger over `df["c0"]` is the same measurement it was, which is the
control this table needs.

## Windows in the kernel

The item this file's "Next" list had at the top: `k +/\ y` reads `x[i..i+k]`
and yields `n-k+1` outputs, against a kernel whose invariant was that every
input and every slot is read at the same index and has the same length. It
is now a step of the kernel — one instruction that folds every window of the
value on the stack, with a running fold (`+/\ y`) beside it — and the
invariant it broke became two axes instead of one. What that cost in
machinery, and what decides the alignment, is in docs/decisions.md.

Three windowed sentences, 20M f64 closes, window 20, `bench/windows.py` and
`bench/timeseries.py --worker`. libjay only: main's build against this
branch's, alternated three times through in one session and the best of each
taken, because the machine had a neighbour on it. Times in milliseconds.

| kernel | 1 thread before | after | 8 threads before | after |
|---|---:|---:|---:|---:|
| Bollinger z-score | 792.3 | 517.9 | 393.1 | 205.8 |
| moving range position | 501.3 | 307.6 | 194.1 | 86.2 |
| running mean | 469.3 | 385.8 | 387.9 | 354.8 |

```j
NB. Bollinger: the phase-5 gate's kernel, two moving sums
s =. 20 +/\ {close}
((20 * 19 }. {close}) - s) % %: (20 * 20 +/\ *: {close}) - s * s

NB. moving range: where the last close sits between the window's ends
lo =. 20 <./\ {close}
((19 }. {close}) - lo) % 1e_12 + (20 >./\ {close}) - lo

NB. running mean: a scan and the arithmetic around it
(+/\ {close}) % 1.0 + i. # {close}
```

**Every figure the two builds computed is identical to the last bit** — the
harness prints the sum of each 20M-row result and the two builds print the
same digits — which is the point of folding the windows in blocks cut from
the axis rather than from the block boundary. The rounding of a window is
still the rounding of that window computed on its own.

The moving range gains most (2.25x on eight threads) because everything in
it fuses: two windows, a drop and five elementwise steps become one pass
over the column instead of seven. Bollinger gains 1.91x rather than more
because `%:` can fail on a negative argument and never joins a kernel, so
what is left is two kernels, the square root, and the leading drop.

The running mean gains least, and it is the interesting one. A running fold
carries an accumulator from one block to the next, so a kernel holding one
runs on a single thread — where the unfused program ran a serial scan and
then a parallel divide. Fusing it therefore trades parallelism for traffic:
one pass that reads the closes and writes the means, against a pass that
writes the whole scan to memory and a pass that reads it back. The traffic
wins, but only by 1.09x on eight threads and 1.22x on one, and on a machine
with more cores than this one has it could go the other way. It is fused
because the arithmetic around a scan is usually more than one divide.

Against the rivals, the phase-5 gate's own table, taken with this branch's
build in one session:

| kernel | libjay 1 thread | libjay 8 threads | speedup | polars | numba | numba prange | numpy |
|---|---:|---:|---:|---:|---:|---:|---:|
| Bollinger z-score, window 20 | 552.9 | 231.1 | 2.39x | 786.9 | 198.7 | 168.2 | 1373.6 |

| LIBJAY_THREADS | time (ms) | speedup over 1 |
|---:|---:|---:|
| 1 | 534.8 | 1.00x |
| 2 | 331.1 | 1.62x |
| 4 | 266.9 | 2.00x |
| 8 | 222.6 | 2.40x |

Polars is where it was and libjay is 3.4 times faster than it; the gap to
`numba`'s hand-rolled sliding accumulator is 16% and the gap to `numba
prange`, which recomputes every window from scratch across all eight
threads, is 37%. The accuracy figures the harness prints are unchanged to
the last digit — `numba` still loses six digits to its accumulator and
`numpy` three to its differenced cumulative sum — so what closed is the
distance to the fastest thing in the table, not the distance to its
answers. Scaling reaches 4 threads and holds to 8 for the first time on
this kernel, because the window folds no longer take a pass of their own.

The kernel used to be about ten passes over a 160 MB column. It is now two
kernels, a square root and a drop — four, of which the two kernels do all
the window folding as they go.

**Is folding the moving sum twice worth not writing it down?** `s` is now a
value the pass moves into both kernels that read it, so its windows are
folded in each. Measured both ways — the pass as it is, against a build
that refuses to move a value with a window in it — paired in one busy
session, so read the pairs and not the absolute figures:

| kernel | 1 thread `s` kept | `s` moved | 8 threads `s` kept | `s` moved |
|---|---:|---:|---:|---:|
| Bollinger z-score | 1142 | 1219 | 570 | 505 |
| moving range position | 667 | 508 | 321 | 230 |

Yes on eight threads, by 11% and 28%; no on one thread for Bollinger, by
6%, where the single core pays for the fold it does twice and there is no
one else waiting on the bus. The pass keeps one rule for every named value.

## Where the bandwidth is

The weighted sum has been within a millisecond or two of `numba prange` for
three sessions running, and this is why. Measured 2026-08-21 on the same
laptop with plain Rust loops and no libjay in the picture, one thread:

| what | time | GB/s |
|---|---:|---:|
| `memcpy` (read + write, 320 MB moved) | 17.4 ms | 18.4 |
| read 160 MB with eight accumulators | 8.8 ms | 18.1 |
| read + scale + write 320 MB, buffer reused | 20.5 ms | 15.6 |
| read 160 MB with one accumulator | 23.0 ms | 7.0 |

One core reaches about 18 GB/s and one accumulator reaches 7.0, which is the
whole of the story the table above tells: eight lanes do not make the
machine wider, they stop the fold from leaving it idle.

Against that, on eight threads:

| program | bytes moved | time | GB/s |
|---|---:|---:|---:|
| `+/ {x}` | 160 MB read | 5.7 ms | 28.0 |
| `+/ {w} * {x}` | 320 MB read | 12.3 ms | 26.1 |
| `numba prange` weighted sum | 320 MB read | 12.9 ms | 24.8 |

26 to 28 GB/s is what two channels of DDR4-2400 give in practice on this
machine (38.4 GB/s is the number on the datasheet). The weighted sum is at
memory bandwidth, it has been at memory bandwidth since fusion landed, and
neither a wider register nor a ninth thread nor a better kernel will move
it. The rows that still gain are the ones with arithmetic per byte to spare.

One more thing the probes say, which explains why an elementwise pass costs
several times what its arithmetic does: a fresh 160 MB output buffer has to
be faulted in by the kernel on first write, and that cost is *larger* than
the pass itself. The same read-scale-write loop takes 20.5 ms into a buffer
already faulted in and 79.4 ms into a freshly allocated one, and none of the
59 ms difference parallelises. Fusion's real win was never the arithmetic —
it was one output buffer instead of one per verb.

## The Python side

Measured 2026-08-21. Compiling is not free and `jay.j(...)` compiles every
time it is called. It now answers from a bounded in-process table instead,
keyed by the language,
the source and the index origin: a compiled program is frozen, holds no
data and is already documented as shareable between threads, so the second
compilation of a source already seen is the first one handed back.
`jay.clear_cache()` empties it. Nothing is written to disk.

The kernel's parameter list is also read once per kernel rather than two or
three times per call.

Best of many calls, microseconds:

| what | before | after | |
|---|---:|---:|---:|
| `jay.j.compile("+/ {x} * {w}")` | 6.3 | 0.9 | 7.0x |
| `jay.j.compile("2+2")` | 4.5 | 0.7 | 6.4x |
| `jay.j("2+2")` — compile, bind, run | 7.3 | 2.1 | 3.5x |
| `jay.j(src, data)` — one shot, 1000 f64 | 30.1 | 17.9 | 1.68x |
| a compiled kernel called again | 1.3 | 1.1 | 1.18x |

And end to end, milliseconds, 2,500,000 rows:

| what | before | after | | for scale |
|---|---:|---:|---:|---|
| `+/"1 {m}`, numpy 2-D, zero-copy | 515.2 | 12.0 | 42.9x | numpy `sum(axis=1)` 45.8 |
| `+/"1 {m}`, polars DataFrame | 674.9 | 72.5 | 9.3x | |
| `+/ {m}`, polars DataFrame | 118.7 | 67.7 | 1.75x | numpy 2-D, zero-copy: 6.5 |
| `+/ {a}`, 2M f64 | 572.8 (us) | 436.1 (us) | 1.31x | numpy `.sum()` 836 us |
| `+/ {a}`, 2M complex | 1062 (us) | 1163 (us) | 1.0x | numpy `.sum()` 1666 us |

Row sums over a numpy matrix are now four times faster than numpy's own,
and a 2M-element sum is twice as fast as numpy's for floats and 1.4 times
for complex. The DataFrame rows carried the weave when this table was taken
— 68 of those 72.5 ms were the boundary copy, not the arithmetic — which
was the layout question under "Next". It has since been answered: the same
two programs are 12.5 and 6.2 ms under "Layout" above.

## Next

In the order the measurements rank them:

1. ~~Refcounted buffers, so naming a value is free.~~ Done: `Buf::Owned` is
   an `Arc<Vec<T>>` with copy-on-write through `Arc::make_mut`.
2. ~~Fusion of elementwise chains into one pass, so `+/ w * x` never
   materialises `w * x`.~~ Done: `fuse.rs` replaces a chain of two or more
   elementwise primitives with one blockwise kernel, and absorbs an
   `+/`-style reduction over it.
3. ~~SIMD and multiple accumulators inside a chunk (phase 6), which is where
   the remaining gap to numba lives.~~ Done: the vector width came first
   (see "SIMD dispatch") and said the gap was never the width; the
   accumulators came second and moved a one-thread reduction by two to four
   times. What is left of the gap is the bus, and the numbers are under
   "Where the bandwidth is".
4. ~~Fusion across an assignment, which is what the named standard deviation
   is still paying for.~~ Done: the pass moves a named value into the
   sentences that read it, which is what the head of this file measures.
5. ~~Owned slices as views, so a cell of an owned array is not a copy.~~
   Done: `Buf` grew a window variant.
6. ~~A row-wise reduction that does not build an array per cell.~~ Done:
   `u/"1` folds every row out of the one buffer.
7. ~~The compiled-program cache the Python front door was missing.~~ Done,
   in process and bounded, `jay.clear_cache()` to empty it.

What is left, in the order the measurements rank it:

1. ~~**A column-major layout flag on `Array`.**~~ Done, as a design of its
   own: `Array` carries a `Layout`, a table crosses as its columns without
   a copy, and the folds read them where they lie. What it is worth is the
   "Layout" section above; what the flag had to be honoured or refused by,
   and how that was enforced, is in docs/decisions.md.
2. ~~**Windows inside the fused kernel.**~~ Done, and worth more than the
   100 ms this entry estimated: the Bollinger row went 393 → 206 ms on eight
   threads and the moving range 194 → 86. A window step reads a haloed block
   and folds it where it lies; what it cost the invariant is a second axis,
   and the alignment between the two is decided by shapes alone. The
   numbers are under "Windows in the kernel", the design in
   docs/decisions.md.
3. **Widening inside the parallel pass.** A complex-plus-float pass widens
   the float argument into a whole buffer and reads it back. Doing it per
   chunk would save two round trips of the working set — about 4 ms of the
   13.7 at 2M — and needs the chunk functions to be told where their slice
   of the source begins, which is a signature change through the shared
   offset-and-divider machinery every scalar pass uses. The widening itself
   is at least parallel now.
4. **The fresh output buffer.** An elementwise pass spends longer faulting
   its 160 MB result in than computing it, and that cost does not
   parallelise. A reuse pool would remove it; nothing portable and stable
   will.
5. **AVX-512 measurement.** The x86-64-v4 rung is built and symbol-checked
   (see "SIMD dispatch") but has never run: no machine on hand has the
   hardware. Pending a runner that does; `tests/simd.rs --nocapture` reports
   itself the day one shows up in CI.
