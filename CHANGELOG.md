# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [semantic versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- APL trains: a run of bare functions now reads as a fork or an atop —
  `(f g h)` applies `f` and `h` to the argument and combines the results
  with `g`; `(g h)` applies `h` then `g`. A plain number may stand in a
  fork's left position, and `⊢`/`⊣` mean "the argument itself". This is an
  extension beyond strict APL2/GNU APL, on by default, with the strict
  reading available as an option.
- APL function assignment: a derived function or a whole train can be
  named (`F←+/`, `F←+/÷≢`) and then applied like any other function.
- J can name an adverb or conjunction on its own (`m =. /`, `c =. @`), not
  only a verb.
- J can define its own adverbs and conjunctions: `1 : '…'` and `2 : '…'`,
  their multi-line forms, and the `{{ … }}` direct-definition syntax,
  matching J's published vocabulary for writing them.
- J's `L:` and `S:` now take two arguments as well as one.
- J's `H.`, the generalised hypergeometric series.
- Reading input, not just writing it: APL's `⍞` (one line of text) and `⎕`
  (one line evaluated as APL), and J's `1!:1 ]1` and `3!:0` (a value's
  storage type), all read from the same standard input the host provides —
  piped, typed, or supplied by the embedding application. Every language
  surface (Rust, Python, C, and the command line) gained the matching call,
  alongside the existing output calls. The rest of J's `!:` foreign
  conjunction that would reach a file, the system clock, or another process
  is refused with a clear "closed by the sandbox" message, distinct from
  "not supported yet".
- Faster execution on newer x86-64 processors, using the CPU's AVX-512
  instructions when present; picked up automatically at startup, with an
  explicit override available. Not yet benchmarked on real AVX-512
  hardware.
- J's gerunds are ordinary data now, exactly as the language has them: a
  tie such as `` +`- `` produces a boxed value you can name, print, add to,
  and build by hand. `` `: `` (evoke gerund) works in all three of its
  forms — apply each verb and collect the answers, insert the verbs between
  the items, or read the gerund as a train.
- Dyadic transpose in both languages: J's `1 0 |: m` and APL's `2 1⍉m`,
  including the diagonal forms `` (<0 1)|: `` and `1 1⍉`.
- J's monadic `{` (catalogue: every combination of one element from each
  item) and monadic `e.` (raze-in).
- J's `_.`, the indeterminate value.
- J's `u b. 1` and `u b. _1`: what a verb's identity element and inverse
  are spelled as.
- J's `^:` accepts a list of counts, and the boxed forms that collect
  every intermediate result — `u^:(<n)` and `u^:a:`.
- J's tessellation `;.3` accepts a negative block size, which reverses that
  axis, where the movement row is written out.
- APL's `⍢` (under) and `⌺` (stencil), and the collating grades `x⍋y` and
  `x⍒y`.
- Grading BOXED (nested) arrays, in both languages, which had been the
  last thing a grade refused. J's `/:` and `\:` order whole arrays by J's
  total array ordering — the type class, then the rank, then the shape read
  with the last axis first, then the atoms, recursing through boxes — and
  APL's `⍋` and `⍒` order them by the APL2 rule GNU APL answers with, which
  is a different comparator at every step. The dyads (`x /: y`, sorting by
  a nested key) and the sort idioms follow from the same ordering. The new
  `nested_grade` dialect setting names Dyalog's total array ordering as the
  other reading, and refuses it rather than answering with this one.

### Changed

- Moving windows and running sums are part of a fused expression now.
  `k +/\ y`, `k >./\ y`, `k <./\ y` and `+/\ y` used to break the chain
  they stood in and run as a pass of their own over the whole column; they
  are steps of the compiled kernel, so a rolling expression reads its
  argument once instead of once per window and once per arithmetic step.
  Results are unchanged to the last bit, including the property that a
  window's rounding error is the error of that window alone.
- A DataFrame no longer costs a copy to read. Its columns cross the
  boundary borrowed, one Arrow buffer each, and libjay folds them where
  they lie: `+/ df` (column sums) and `+/"1 df` (row sums) over a
  2.5M x 8 table are 10 and 5 times faster end to end, and reading the
  table's shape costs nothing at all. Programs that need the elements in
  reading order — ravelling, indexing, printing — still pay for one copy,
  now at the point they ask for it instead of on every call.
- Transposing an array (`|:`, `⍉`) no longer moves any elements, at any
  rank.
- A Fortran-ordered numpy block — `np.asfortranarray(a)`, or the `.T` of an
  ordinary one — is read where it lies instead of being refused with a
  request to copy it. Views that are contiguous in neither order (strided
  slices, sub-blocks, partial axis permutations) are still refused, with
  the same message.
- Refusals that come from the sandbox (closed I/O, the system clock,
  threads) are now labelled distinctly from "not supported" and "not part
  of the language", so it reads as a deliberate boundary rather than a
  missing feature.
- Minimum required Rust version raised to 1.89: needed for the AVX-512
  support above, and for wgpu 30 (the GPU backend's dependency), which
  needs a newer compiler than the previous floor; pinned in the repository
  so every build uses the same compiler.
- Updated third-party dependencies (the GPU backend to wgpu 30, Python
  bindings, and test tooling) to their latest versions; no user-visible
  change.

### Deprecated

### Removed

### Fixed

- `(2&+)^:_1` and `(2&*)^:_1` computed the wrong inverse — the bonded
  number was applied from the left instead of taken off the right, so
  `(2&+)^:_1 5` answered `¯3` where J answers 3. Everything that undoes a
  verb (J's `&.`, `^:_1`, `u b. _1` and APL's `⍢`) is corrected by it.
- APL's `⊥` on an argument of rank 2 or more folded the wrong axis: it is
  an inner product and folds the LEADING axis of its right argument.
  Vectors, the common case, were always right.
- APL's `⍸` (interval index) placed a value exactly equal to a bound in the
  wrong interval: `1 3 5⍸3` is 2, not 1. J's `I.`, whose interval is open
  on that side, is unchanged.
- APL's `⌷` accepts an enclosed vector as an index, so `(⊂1 2)⌷5 6 7 8` is
  `5 6`.
- APL's `∊` finds a scalar held in a nested right argument:
  `1 2 3∊(1 2)(3)` is `0 0 1`.
- Two rarely-used APL operators (variant, I-beam) that
  aren't implemented yet are now reported by name as "not supported yet"
  instead of as an unrecognized character.
- Fixed APL operator precedence so a parenthesised function binds before
  an operator to its right, matching the reference implementation —
  `(+)/1 2 3` now evaluates to 6.
- APL's scalar functions reach inside a nested argument, as APL2 has them:
  `(1 2)(3 4)+1` is `(2 3)(4 5)`, and every arithmetic, comparison and
  logical function pervades to the simple values at the bottom. They used
  to refuse a nested argument outright.
- APL's `⊥` over an EMPTY radix axis crashed the printer: `(⍳0)⊥1 2 3` now
  answers 0, and the result's frame is `(¯1↓⍴x),1↓⍴y` whatever axis is
  empty.
- APL scalar extension between two frames of ONE cell kept the wrong one, so
  `⍴(,5)+¯3` was empty where it is `1`. A rank-0 frame gives way to the
  other side, and between two one-cell frames that are not scalars the
  answer keeps the right one.
- Take and drop count AXES. More counts than the argument has axes is a
  length error in both languages, and only a scalar right argument stretches
  to meet them (`1 2 {. 5` is a 1 by 2 table); APL wants exactly one count
  per axis where J is content with fewer. A count of zero on an axis after
  the first now empties that axis instead of leaving it alone.
- APL's replication extends an argument of one item along the axis, as it
  extends a scalar: `2 0 1/,5` is `5 5 5`.
- APL's dyadic `∪`, `∩` and `~` take vectors, as GNU APL has them; a grade
  needs an array rather than a scalar; and `≡` tells an empty character
  array from an empty numeric one.
- `E.`/`⍷` search every axis at once and answer in the shape of the right
  argument, so a table is found inside a table. An empty pattern matches
  everywhere.
- J's LCM and GCD accept numbers that are not whole: the pair is read as the
  decimals it prints as, so `1.23 +. 4.56` is `0.03` and `2.5 +. 5` is 2.5.
- J's `#.` accumulates in the exact types when it is given them, so a
  19-digit integer keeps every digit and `#. 1r2 1r3` is `4r3`.
- J's `m&v` and `u&n` apply to the whole argument, as `m&v b. 0` reports:
  `1 2&+ 1 2` is `2 4`, not a two-by-two table.
- J's `p.` answers `0 ; ''` for the zero polynomial instead of refusing it,
  and `j.` has an obverse, so `+/&.:j.` works.
- An empty array inside a box keeps its shape on screen: `<0 3⍴0` draws a
  cell three wide with no lines in it.

### Security

## 0.1.0 — 2026-08-21

First release: independent implementations of J and APL over one shared IR,
embeddable from Rust, Python and C.

### Languages

- J frontend: 135 of 180 valences in the published vocabulary implemented,
  26 partial, 18 not yet, 1 refused by design. Verbs, adverbs and
  conjunctions, trains (forks, hooks, caps and longer), tacit and explicit
  definitions, `if.`/`while.`/`for.`/`select.` control structures, gerunds
  under `@.`.
- APL frontend: 79 of 115 valences implemented, 25 partial, 11 not yet.
  Functions, operators, dfns, `⋄` and newline separators, `⎕IO` as a
  compiler setting rather than global state. The dialect is the APL2/ISO
  line that GNU APL embodies; the points where the lineages differ are
  named settings on a dialect object, and asking for the other reading is a
  "not implemented yet" error rather than a silently different answer.
- Both frontends lower to one language-agnostic IR — an `Expr` tree over a
  `Verb` combinator tree — executed by one generic rank-and-agreement
  engine. J's leading-axis reduction and APL's trailing-axis reduction are
  the same machinery with different rank.
- Diagnostics carry a span into the source: the offending text is quoted
  with a caret under it, shape errors print both shapes, and "the language
  lacks this" reads differently from "not implemented yet".

### Execution

- Parallel by default: elementwise passes, pure rank cells and leading-axis
  reductions split above 65,536 element operations, on the crate's own pool
  (`LIBJAY_THREADS`), never rayon's global one.
- Expression fusion: chains of elementwise primitives compile to one
  blockwise kernel, absorbing a trailing full-rank reduction and moving
  named values into the sentences that read them. Anything not fused falls
  back to the subtree it replaced, so results and error messages cannot
  change.
- Runtime SIMD dispatch over the hot loops: x86-64 baseline/v2/v3 and NEON,
  detected once per process (`LIBJAY_CPU_LEVEL`). No hand-written
  per-primitive kernels.
- GPU placement of fused kernels through wgpu (Metal, Vulkan, DX12), with
  WGSL generated at run time. Compiled into the one artifact and dormant on
  a machine with no adapter; `deploy`, `upload` and `keep_on_device` are the
  API. f64 needs `SHADER_F64` and stays on the CPU without it rather than
  quietly computing in f32.
- Headline: a 20-period Bollinger z-score written as one J expression runs
  20M rows in 404 ms against the equivalent Polars pipeline's 755 ms,
  agreeing to 8.7e-10 relative. Resident GPU data runs 1.4x to 7.3x the
  8-thread CPU on the same 20M rows. Numbers and method in
  [bench/README.md](bench/README.md).

### Surfaces

- Rust: the `libjay` crate, library name `jay`. `compile` → `Program::run`;
  a `Program` is immutable, holds no data and is `Send + Sync`.
- Python: the `libjay` wheel, import `jay`, abi3 for 3.10+, no runtime
  dependencies. `jay.j(...)` compiles, binds and executes in one call;
  `jay.j.compile(...)` returns a reusable kernel with `bind`, `deploy` and
  `explain`. Compiled programs are memoised in-process. On 3.14+, t-strings
  make interpolated values both the type contract and the live defaults.
- C: `crates/libjay-capi` builds `libjay.so`/`.dylib`/`jay.dll` plus a
  static library and a hand-written `jay.h`. Prebuilt bundles ride along
  with each GitHub release for four target triples.
- CLI: `libjay -e EXPR`, `libjay FILE` (`.ijs`/`.j`/`.apl`), `--lang`,
  `--explain`.
- Sandbox: stdout is open for `echo` and `⎕←`; no primitive reaches the
  filesystem or the network.

### Data

- Dense arrays of bool, i64, f64 and characters, row-major; complex numbers
  as a core type; boxes; J's exact types — extended-precision integers and
  rationals.
- Zero-copy in and out for i64, f64 and i64-physical temporal columns, over
  the Arrow C data interface and `__array_interface__`: Polars, pandas 2,
  PyArrow and numpy work natively, with no dependency on any of them.
  Narrower types widen with one copy.
- Nulls, mixed-type table columns and non-contiguous numpy views are
  refused with an error naming the column and the fix, never guessed at.

### Testing

- Differential suites against black-box runs of the reference interpreters:
  3816 J expressions and 1024 APL expressions, recorded as snapshots and
  replayed on every `cargo test` with no interpreter present. libjay agrees
  everywhere except 29 APL sentences where it diverges on purpose, each
  recorded with its reason.

### Not in this release

- J: dyadic transpose, `{` catalogue, `e.` raze-in, format by
  specification, `".` numbers, symbols, sparse arrays, the Taylor and
  hypergeometric conjunctions, foreign conjunction `!:`, locales, adverb and
  conjunction assignment, multiple assignment, `throw.`/`catcht.`/`goto.`.
- APL: dyadic transpose, collating grade, character I/O `⍞`, I-beam,
  `⍢` under, `⌺` stencil, `⍠` variant, `&` spawn, function assignment,
  trains.
- One APL dialect only: Dyalog-specific behaviour is a planned dialect
  switch, not a supported reading today.
- The GPU f64 path is generated and type-checked but has never been
  executed: the measuring machine's Metal adapter has no `SHADER_F64`.
- C ABI: boxed, extended and rational results have no descriptor yet and
  are refused by name; input is copied at the boundary rather than borrowed.
- Arrow string, binary, list and dictionary columns; Decimal128; float16;
  byte-swapped data. No Rust macro for compile-time checking of an
  expression.
