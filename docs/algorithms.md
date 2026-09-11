# Six algorithms written in J

Six pieces of numerical work that J is good at and that nobody appears to
have written in J: three indicators from technical analysis, one neural
network, one filter design, one associative memory.

Each is written here from its own mathematics — the paper, or the textbook
statement of it — and no implementation of any of them, in J or in anything
else, was consulted for any of it. Searching the J wiki, the addons and the
usual code hosts by name turned up none to consult in any case, which is a
weak claim on its own and not the one the work rests on.

Each has a numpy reference written the same way and independently of the J,
and each is checked three ways: libjay against the reference, jconsole
against the reference, and libjay against jconsole on the very same source
text.

They are in the repository as examples rather than as a library. The point
is partly the algorithms and partly what they show: a variance-ratio test is
nine sentences, a Savitzky-Golay filter of any width and order is two, and a
Hopfield network's whole recall loop is one.

## Where everything is

```
examples/algos/<name>.ijs                the J, definitions then a demo
bench/algos/<name>.py                    the numpy reference
bench/algos/cases.py                     one check per algorithm, and its tolerance
bench/algos/corpus.py                    the corpus lines, generated from the J
bench/algos_bench.py                     libjay against numpy, numba and jconsole
python/tests/test_algos.py               the checks, as pytest
crates/libjay/tests/corpus/j/algorithms.txt   the same programs, for the oracle
crates/libjay/tests/snapshots/j/algorithms.snap   what jconsole answered
```

Every `.ijs` file is in two halves, split by a line reading
`NB. === demo`. Above it are the definitions and nothing else; below it is a
runnable demonstration with `echo`. The tests and the benchmarks read the
half above the line straight out of the file, so there is exactly one copy
of each algorithm and it is the one the reader sees.

The corpus lines are generated from those same definitions:

```sh
python -m bench.algos.corpus                          # rewrite the corpus file
cargo run -p libjay-devtools -- record j algorithms   # ask jconsole again
```

A pytest holds the checked-in corpus to what the generator produces now, so
a definition cannot drift away from the source jconsole was recorded on.

Each corpus program ends by rounding its answer into integers — the
convention `programs.txt` already follows — so what is recorded is exact and
a change of answer names itself in a diff.

## The shared generator

Five of the six demonstrations need data that libjay, jconsole and numpy can
all produce, bit for bit, without carrying a million literals between them.
`RANDOM` is that: a counter run through a Mersenne prime's residues and
squared twice, every product kept under 2^63 so the integer arithmetic is
exact.

```j
RANDOM =. 4 : 0
  p =. 2147483647
  u =. p | 1103515245 * x + >: i. y
  u =. p | 12345 + 1103515245 * p | u * u
  u =. p | 12345 + 1103515245 * p | u * u
  u =. p | 48271 * u
  _1 + 2 * u % p
)
```

The squaring is the part that matters and the first version of this file did
not have it. A chain of multiplications modulo a prime composes into ONE
multiplication, so consecutive counters come out a fixed distance apart: the
outputs were uniformly distributed and looked fine, and had a lag-1
autocorrelation of −0.41. Every series built on them carried it. The Hurst
exponent of that "white noise" came out at 0.33 and a variance ratio built
to show momentum came out below 1 — both correct answers about a series that
was not what it claimed to be. Two squarings later the lag-1 to lag-6
correlations are all under 0.015 at n = 20000, the exponent is 0.56 and the
momentum series reads 1.45 against a theoretical 1.44.

## The algorithms

### Kaufman's Adaptive Moving Average (`kama.ijs`)

An exponential average whose smoothing constant is chosen bar by bar from
how efficiently the price moved: the net displacement over a lookback
divided by the path length walked in getting there, mapped between a fast
and a slow constant and squared. Trending markets get a short average,
choppy ones a long one.

*Why it is here*: adaptive moving averages are everywhere in trading
software and nowhere in J, and the algorithm is the awkward shape for an
array language — a recursion whose coefficient changes at every step, which
cannot be turned into a scan of a fixed operator. It is the one algorithm of
the six that shows J's fold rather than J's rank.

The efficiency ratio is pure array work; the recursion is a fold.

```j
ER =. 4 : 0
  path =. x +/\ | 2 -~/\ y
  net  =. | (x }. y) - (-x) }. y
  net % path + path = 0
)

KSTEP =. 4 : 'y + ({. x) * ({: x) - y'
...
  seed , seed (] F:. KSTEP) }. sc ,. q
```

`x +/\ | 2 -~/\ y` is the whole path length: pair up the bars, take the
size of each move, sum every window. The fold hands `KSTEP` the item on the
left and the running value on the right, so the item is a (constant, price)
pair and the answer is the average so far.

There is a closed form — the recursion is linear, so it unrolls into a
cumulative product of `1 - sc` and a cumulative sum divided by it — and it
is useless: the product underflows to zero within a few hundred bars. The
fold is the honest way to write it and the benchmark below is what a fold
costs.

*Reference*: `bench/algos/kama.py`, written from Kaufman's description in
*New Trading Systems and Methods*, with a numba version of the recursion so
the timing comparison is not just a Python loop. *Tolerance*: 1e-12
relative; measured, all three implementations agree to 3e-16.

### The Lo-MacKinlay variance-ratio test (`varratio.ijs`)

If a log price is a random walk its variance grows linearly with the
horizon, so the variance of a q-bar return over q times the variance of a
one-bar return is 1. The test is that ratio with the overlapping estimator
and the unbiased corrections, plus the two standardised statistics: `z1`
assumes homoskedastic increments and `z2` is robust to heteroskedasticity,
which is the one to read on real bars.

*Why it is here*: it is the standard econometric test for a random walk, it
is arithmetic all the way down — sums of squares, shifted inner products, a
weighted sum over lags — and it is nine sentences in J. The robust variance
is where an array language earns its keep:

```j
DELTA =. 4 : '+/ (x }. y) * (-x) }. y'
...
  js =. >: i. <: q
  dl =. (n * js DELTA"0 _ s2) % *: +/ s2
  th =. +/ dl * *: 2 * (q - js) % q
```

`js DELTA"0 _ s2` is every autocovariance of the squared demeaned returns at
once — rank 0 on the lags, rank infinity on the series — and the line after
it is Lo and MacKinlay's theta exactly as the paper writes it.

*Reference*: `bench/algos/varratio.py`, from the paper's equations (Review
of Financial Studies 1, 1988). *Tolerance*: 1e-11 relative; measured 2e-15.

Two things caught bugs. The first was J's right-to-left evaluation:
`q * (n - q + 1) * ...` is `n - (q + 1)` in J and not `(n - q) + 1`, which
moved the ratio by 7e-4 — enough to be visible against numpy and far too
small to notice by eye. The second was the reference and the J agreeing on a
wrong formula: both were missing the factor of n in the delta estimator,
which does not show up in a differential test at all. What showed it was the
DEMONSTRATION: a series built to have positive autocorrelation must give a
ratio above 1, and it did not. An algorithm needs a case whose answer is
known from theory, not only a second implementation.

### The Hurst exponent by rescaled range (`hurst.ijs`)

Cut the series into blocks; within each, take the running sum of deviations
from the block mean; R is the range of that and S the block's standard
deviation. R/S grows like (block length)^H, so the slope of log(R/S) against
log(length) is the exponent — a half for a random walk's increments, more
for a persistent series, less for a mean-reverting one.

*Why it is here*: it is the shape J was built for. Every block is a row of
one matrix and the whole statistic is four rank-1 verbs across it, with no
loop over blocks and no loop over block sizes:

```j
RS =. 4 : 0
  k   =. <. (# y) % x
  b   =. (k, x) $ (k * x) {. y
  dev =. b -"1 0 (+/"1 b) % x
  cs  =. +/\"1 dev
  r   =. (>./"1 cs) - <./"1 cs
  s   =. %: (+/"1 *: dev) % x
  (+/ r % s) % k
)
```

and `x RS"0 _ y` does every block size at once.

*Reference*: `bench/algos/hurst.py`, from Hurst's definition as Mandelbrot
and Wallis state it. *Tolerance*: 1e-11 relative; measured 8e-15.

The demonstration is the check that matters: white noise reads 0.57 (R/S is
known to sit above a half at these block sizes), and the same noise smoothed
over 64 samples — correlated with its 63 neighbours by construction — reads
0.94.

### An extreme learning machine (`elm.ijs`)

A one-hidden-layer network whose hidden layer is never trained: the input
weights are drawn once and left alone, and only the linear readout on top of
the hidden activations is fitted, which makes training one ridge-regularised
least-squares solve instead of an epoch loop.

*Why it is here*: it is a neural network that fits entirely inside array
notation, with no gradient, no optimiser and no iteration — the whole of
training is

```j
RIDGE =. 4 : 0
  h   =. > 0 { y
  t   =. > 1 { y
  ht  =. |: h
  ((ht +/ . * t)) %. ((ht +/ . * h) + x * =/~ i. {: $ h)
)
```

`=/~ i. h` is the identity matrix, `+/ . *` the matrix product and `%.` the
matrix divide, so `b %. a` solves `a mp z = b`. Forming H'H costs one pass
over the n-by-h features and leaves an h-by-h system: the number of samples
never enters the solve, which is the whole reason the machine is fast.

*Reference*: `bench/algos/elm.py` (Huang, Zhu and Siew, Neurocomputing 70,
2006), solving the same normal equations with `numpy.linalg.solve`.
*Tolerance*: 1e-8 relative on the fitted values; measured, libjay against
numpy 3e-11 and jconsole against numpy 1e-8. That gap is the finding below.

### A Hopfield network (`hopfield.ijs`)

Weights are the sum of the patterns' outer products with the diagonal
cleared; recall drives a corrupted probe towards the sign of its own local
field, sweep by sweep, descending the energy `_0.5 * s W s` into a stored
pattern.

*Why it is here*: an associative memory is three lines of array notation and
nobody seems to have written one in J. Storage is one matrix product,

```j
STORE =. 3 : 0
  n =. {: $ y
  (((|: y) +/ . * y) * -. =/~ i. n) % n
)
```

— `-. =/~ i. n` is the identity complemented, which is what clears the
self-connections — and a whole batch of probes recalls in one matrix product
per sweep, with `^:` counting the sweeps:

```j
RECALL =. 4 : '((> 0 { x) & FLIP) ^: (> 1 { x) y'
```

The demonstration stores three patterns over 64 units, corrupts twelve bits
of each and gets all twelve back in six sweeps, with the energy falling from
about −11 to about −31.

*Reference*: `bench/algos/hopfield.py` (Hopfield, PNAS 79, 1982).
*Tolerance*: 1e-12; measured 0 — every state is exactly ±1 and the three
implementations agree cell for cell, energies included.

### A Savitzky-Golay filter, designed (`savgol.ijs`)

The filter that fits a polynomial of degree k to every window of 2h+1
samples and reports the fitted value, or the fitted d-th derivative, at the
window's centre. Because the fit is linear in the samples the whole thing
collapses to one convolution, and the weights are a row of the pseudo-
inverse of the Vandermonde matrix over the window's offsets.

*Why it is here*: implementations of this filter almost always ship a TABLE
of coefficients for a handful of (h, k) pairs. Designing them is two
sentences, and then any half width, degree and derivative order is available
— including the ones no table lists:

```j
SGCOEF =. 3 : 0
  h =. 0 { y
  k =. 1 { y
  d =. 2 { y
  a =. (i: h) ^/ i. >: k
  g =. (%. (|: a) +/ . * a) +/ . * |: a
  (! d) * d { g
)

SGFILT =. 4 : '(# x) (+/ @ (x & *))\ y'
```

`(i: h) ^/ i. >: k` is the design matrix, `%.` inverts the normal equations,
and the d-th row of the pseudo-inverse already holds the d-th Taylor
coefficient — so the derivative wants only the factorial. The filter itself
is `n u\ y`: apply the weighted sum to every window of n items, one
sentence, no state.

`SGCOEF 2 2 0` answers 3/35 of `_3 12 17 12 _3` — the published five-point
quadratic smoother — and `SGCOEF 2 2 1` answers a tenth of `_2 _1 0 1 2`,
which is the five-point central difference. Both fall out of the design; no
table was consulted.

*Reference*: `bench/algos/savgol.py`, building the same design matrix and
taking `numpy.linalg.pinv` of it — a different route to the same weights.
*Tolerance*: 1e-10; measured, libjay against numpy 2e-13 and jconsole
against numpy 6e-9.

## What was checked, and against what

Three implementations, one J source. `libjay` and `jconsole` are given the
same string, character for character — the definitions read out of the
example file plus a few hundred bars of data built by `RANDOM`. numpy builds
the same data from the same generator and computes the answer its own way.

The error reported is the largest relative difference over the whole answer,
with a floor at a thousandth of the answer's own scale so that a value which
happens to sit near zero — a smoothed signal crossing its mean, a prediction
at a root — is judged against the answer and not against its own vanishing
magnitude.

The tolerance column is what `python/tests/test_algos.py` holds LIBJAY to;
jconsole's column is measured here and asserted nowhere, since what holds
libjay to jconsole is the corpus replay, on integers.

| algorithm | values compared | tolerance | libjay vs numpy | jconsole vs numpy | libjay vs jconsole |
|---|---:|---:|---:|---:|---:|
| KAMA | 490 | 1e-12 | 0 | 2.9e-16 | 2.9e-16 |
| variance ratio | 15 | 1e-11 | 1.2e-15 | 1.8e-15 | 1.0e-15 |
| Hurst R/S | 8 | 1e-11 | 8.2e-15 | 8.2e-15 | 2.2e-16 |
| extreme learning machine | 400 | 1e-8 | 3.4e-11 | 1.1e-8 | 1.1e-8 |
| Hopfield | 330 | 1e-12 | 0 | 0 | 0 |
| Savitzky-Golay | 96 | 1e-10 | 2.0e-13 | 6.1e-9 | 6.1e-9 |

The two loose rows are the two that go through `%.`; see the findings.

Beyond that, `python/tests/test_algos.py` runs four of the six over the
benchmark corpora themselves — the OHLCV close series and the DSP signal of
`bench/data.py`, twenty thousand bars of each, handed to libjay as numpy
memory rather than built inside J — against the same references, and runs
every example file whole so the demonstrations cannot rot.

## Performance

See `bench/algos_bench.py`. Every implementation is given the same input:
an OHLCV close series and a tone-plus-noise signal of the shapes
`bench/data.py` builds, but built by the shared generator instead, because
jconsole is measured in a subprocess of its own and a million numbers cannot
be handed to it as a literal. The price series feeds the four indicators,
the signal feeds the filter, and the learning machine is asked the question
those bars actually pose: predict the next return from the last sixteen.
The correctness tests do use `bench/data.py`'s own series — see above.

Machine: macOS 13.7.8, x86-64, 8 logical threads, `LIBJAY_THREADS=4`,
Python 3.12.9 with numpy 2.2.6 and numba 0.61.2. Best of three after a
warmup, in milliseconds. numba appears only where numpy is a Python loop
rather than an array expression, which is the KAMA recursion and nothing
else.

| algorithm | size | J | libjay | numpy | numba | jconsole |
|---|---|---|---:|---:|---:|---:|
| KAMA | 1,000,000 bars | `10 2 30 KAMA {close}` | 2800 | 483 | 21 | 497 |
| variance ratio | 1,000,000 bars, q=8 | `8 VR {p}` | 37 | 26 | — | 21 |
| Hurst R/S | 1,000,000 returns, 9 block sizes | `{lens} HURST {x}` | 320 | 119 | — | 94 |
| Savitzky-Golay | 1,000,000 samples, 17-tap cubic | `(SGCOEF 8 3 0) SGFILT {sig}` | 25 | 13 | — | 477 |
| extreme learning machine | 100,000 by 16, 64 hidden | `({w} ; {b} ; 0.1) ELMFIT {x} ; {t}` | 521 | 302 | — | 306 |
| Hopfield recall | 1024 units, 256 probes, 10 sweeps | `({w} ; 10) RECALL {probe}` | 186 | 63 | — | 654 |

libjay wins two of the six and loses four. The first reading of this table
lost five, and the losses concentrated in three constructs; two of the
three have fused paths now and the third has not. The Savitzky-Golay row
moved from 1577 ms to 25 — nineteen times faster than jconsole, and within
twice of numpy's BLAS — and the variance ratio from 805 to 37, both of them
by the same two fusions. What is left is the FOLD: KAMA is a fold over a
million rows with an explicit dyad and nothing else, and 2800 of its 2800
milliseconds are that fold. The Hurst exponent, the extreme learning
machine and the variance ratio are within 1.7 to 3.4 times of jconsole on
work that is neither — rank, sorting and one matrix solve — and the
Hopfield row, which is nothing but matrix products on a fused path, is 3.5
times faster than jconsole.

(The three measurements this table is compared against were taken on the
same machine a week earlier, and the numpy column moved by 10 to 20 per
cent between the two readings; the libjay column's movements below are
larger than that by one to two orders of magnitude, and the jconsole
column is measured beside each of them.)

## Findings

### jconsole's `%.` loses about half the digits on an ill-conditioned system

Not a libjay bug — the opposite — but worth recording, because it sets the
tolerance on any algorithm here that solves a linear system, and because it
is why the extreme learning machine's corpus program uses a ridge penalty
large enough to condition the system rather than the small one a fit would
prefer.

On the 8-by-8 Hilbert matrix against `1 2 3 4 5 6 7 8`:

| | residual `||Az - b||` | first component |
|---|---:|---:|
| jconsole | 7.9e-5 | −545.67 |
| libjay | 2.3e-9 | −511.999999852 |
| numpy `solve` | 4.0e-10 | −512.000032778 |

The condition number is 1.5e10, so a backward-stable solve should leave a
residual near 1e-10 and libjay and numpy both do. jconsole's is five orders
of magnitude larger, which is what solving through the normal equations
looks like: the condition number is squared and half the digits go. The
effect is visible in the table above wherever `%.` appears — 1.1e-8 on the
extreme learning machine, 6.1e-9 on the Savitzky-Golay weights — and absent
everywhere else.

Minimal sentence, if the agreement loop wants it:

```j
(1 2 3 4 5 6 7 8) %. % >: (i. 8) +/ i. 8
```

The exact solution is the integer vector
`_512 31752 _468720 2818200 _8316000 12756744 _9753744 2934360`. libjay's
first component is `_511.9999998519582` and jconsole's is
`_545.67260376550257`.

### An apostrophe in a comment quoted every interpolation hole after it

Found while wiring the tests, fixed in this branch because it is one line.
`SourceParts::from_source` — the scanner that splits `{name}` holes out of a
source string — toggled a quote flag on every apostrophe without knowing
that `NB.` runs to the end of the line. One `NB. Kaufman's` in a header
comment left the flag set, and every hole after it was taken for text inside
a string:

```python
jay.j("NB. it's a comment\n2 + {x}", {"x": 1})   # parse error: syntax error
jay.j("NB. plain comment\n2 + {x}", {"x": 1})    # 3
```

The same source without a hole always evaluated correctly, so the J lexer
was never the problem; only the pre-pass was. No string literal of either
language crosses a line, so an apostrophe still open at a newline never
opened one — clearing the flag there is the whole fix.

### The n-wise infix reduce folds every window, whatever the operation

The typed window path regrouped the windows into blocks, which is what
makes a moving sum cost two operations per result instead of `n` — and only
an ASSOCIATIVE operation can be regrouped, so `+`, `*`, `<.` and `>.` had
the path and the difference did not. `2 -/\ y` built an array for every
window and interpreted the verb over it.

A window folded on its own shares nothing with the window beside it, so it
needs no regrouping at all: where the operation cannot be blocked, each
window is folded directly inside the same typed loop — `n` steps per result
and no array built. Over a million doubles, best of three,
`LIBJAY_THREADS=4`:

| kernel | libjay before | libjay | jconsole |
|---|---:|---:|---:|
| `2 +/\ y` | 2.9 ms | 3.0 ms | 1.1 ms |
| `2 */\ y` | 3.4 ms | 2.2 ms | 1.2 ms |
| `2 -/\ y` | 481 ms | 1.6 ms | 1.0 ms |
| `2 -~/\ y` | 584 ms | 1.4 ms | 1.0 ms |
| `2 <./\ y` | — | 2.3 ms | 0.9 ms |
| `(1 }. y) - _1 }. y` | 1.6 ms | 1.3 ms | 0.9 ms |
| `10 +/\ y` | 1.9 ms | 2.2 ms | 1.6 ms |
| `10 -/\ y` | — | 4.1 ms | 148 ms |

The difference over an infix now costs what the sum does, which is what the
two spellings mean: `2 -~/\ y` and `(1 }. y) - _1 }. y` are within noise of
each other. A WIDE window shows the shape of the two roads — `10 -/\ y` is
ten operations per result on both engines, and libjay's stays in one typed
loop where the reference's does not.

`2 -~/\ y` is the bar-to-bar difference, the first line of almost every
time-series program: it is KAMA's path length and the variance ratio's
returns, and it is most of what moved the variance ratio from 805 ms to 37.

### A windowed reduce of a constant combination is a correlation

`SGFILT` is `(# x) (+/ @ (x & *))\ y` — the weighted sum of every window,
which is what makes the Savitzky-Golay filter one sentence. Read as a verb
it is a composition applied once per window; read as arithmetic it is a
correlation, and it is now recognised as one and run in a single pass with
the weights in registers.

| kernel, 1e6 samples, 17-tap | libjay before | libjay | jconsole | numpy |
|---|---:|---:|---:|---:|
| `17 (+/ @ (c & *))\ y` | 1686 ms | 21.5 ms | 473 ms | 12.3 ms |
| `17 (+/ @: (c & *))\ y` | — | 22.8 ms | 480 ms | — |
| `17 (c +/ . * ])\ y` | — | 23.0 ms | 401 ms | — |

The three spellings are one arithmetic and are recognised together, along
with `u&c` for the bond written the other way round and any scalar fold and
any scalar combination — `>./@(c&+)` is a dilation, not a filter, and takes
the same pass. numpy's number is `sliding_window_view` into one matrix
product, which is the bound a BLAS call sets; libjay is within a factor of
two of it and twenty-two times faster than the reference.

The recognition is narrow where narrowness is cheap: a vector argument,
real weights one per window item, all of them finite, an answer that is a
float either way, and a step that makes a NaN abandons the whole pass to
the general road, whose rules for the infinities and the signed zeroes are
the dialect's own.

### The fold is interpreted, and a defined step is what it costs

Where the step is a scalar PRIMITIVE there is nothing to interpret — `]`
keeps what the step made, and a primitive cannot take the fold's control —
and the whole fold is one typed loop over the argument's buffer. Where the
step is a DEFINED verb, no such thing is possible: the point of the
construct is that step i+1 needs step i, and what runs at every step is an
explicit definition.

| kernel, 1e6 items | libjay before | libjay | jconsole | numba |
|---|---:|---:|---:|---:|
| `0 (] F:. f) m`, `f` a defined dyad | 3877 ms | 2884 ms | 460 ms | 3.6 ms |
| `0 (] F:. +) y`, a primitive | 619 ms | 4.4 ms | 137 ms | — |
| `0 (] F.. +) y`, its single form | — | 4.5 ms | 45 ms | — |

The defined-dyad fold is a quarter faster than it was — the items are read
where the step needs them instead of all at once, the results are kept in a
flat buffer, the call's frame is reused, the stitch `sc ,. q` that builds
the argument is one pass, and a name with no underscore in it skips the
three scans a locative needs — and it is still six times slower than the
reference. A profile of it says where the rest is, at a million steps of
`y + ({. x) * ({: x) - y`: a quarter of the time is in `malloc` and `free`,
a sixth in the interpreter's own dispatch, and the rest spread over the
frame's hash table, the argument arrays each primitive makes and the
rank machinery each one walks. Every one of those is per-STEP work that a
compiled inner loop does not do at all, which is what numba's 3.6 ms is:
the same recursion as machine code over two float buffers.

What would close it is not another fusion but a different shape of
interpreter for the inside of a small explicit definition — a resolved
local slot instead of a hash lookup, an unboxed scalar instead of an array
per intermediate. That is a round of its own, and it is what the KAMA row
of the table above is waiting for.

The closed form is not a way out: the recursion unrolls into a cumulative
product of `1 - sc`, which underflows to zero within a few hundred bars.
