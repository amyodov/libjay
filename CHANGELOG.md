# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [semantic versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- J's sparse arrays, `$.`. `$. 0 0 3 0 5` keeps only the positions that are
  not zero and prints them the way J does — one line per stored value, its
  position, `|`, then the value. The dyad takes a form number: `0 $.` moves
  to the other storage kind either way round, `1 $. 3 4` makes a new sparse
  array from a shape (`1 $. (3 4) ; 0 1 ; 5` gives it its own sparse axes
  and its own repeated element), `2 $.` `3 $.` `4 $.` `5 $.` and `7 $.` ask
  for the sparse axes, the element, the stored positions, the stored values
  and how many there are, `8 $.` drops stored values that have become the
  element again, and `_1 $.` gives shape, axes and element boxed together.
  `3!:0` reports the sparse type codes. Booleans, integers, floats and
  complex numbers can be stored sparsely; a sparse array of characters or
  boxes is named as a gap, as J refuses it too.

  A sparse array is the array it stands for: `($. 0 0 3 0 5) -: 0 0 3 0 5`
  is 1, and any verb other than `$.`, `$`, `#`, `":` and `3!:0` reads every
  position of it, so the answer is always the dense one's. Where J keeps
  `s + 1` sparse, libjay hands back the dense array; the value is the same
  and the saving is not. Python, Arrow and the C ABI carry the dense array.

### Changed

### Fixed

## 0.2.1 — 2026-08-22

### Added

- J's symbols, `s:`. A symbol is an atom whose value is a name, and the same
  text always gives the same symbol: `(s: <'a') = (s: <'a')` is 1 wherever
  the two sentences stand. ``s: '`red`green`blue'`` makes three of them from a
  delimited string, `s: ;: 'red green blue'` from boxed words, and a
  character table gives one name per row. They compare, sort (by name, in
  codepoint order), nub, search and index like any other data — `/:~`,
  `~.`, `i.`, `e.`, `I.`, `{`, `#`, `,` and the rest — print as `` `red
  `green `blue ``, and go into boxes. `4 s:` gives the names back as a
  character table and `5 s:` as boxes; `3!:0` reports 65536. Python gets the
  names as strings. Arithmetic on a symbol is a type error that names `5 s:`
  as the way to its characters, and the `s:` forms that report on an
  interpreter's own symbol table are named rather than guessed at.
- Dyadic `I.` now searches character and symbol lists as well as numeric
  ones: `'ace' I. 'bd'` is `1 2`.
- The inner product: J's `u . v` and APL's `f.g`. `+/ . *` and `+.×` are
  the matrix product, `*./ . =` and `∧.=` ask which rows match, `<./ . +`
  and `⌊.+` take a shortest-path step, and any pair of functions works at
  any rank. The matrix product over numbers is a blocked, parallel,
  vectorised pass over the two blocks rather than an interpreted loop:
  measured on a 1000×1000 pair of doubles it runs about 2.5× behind
  numpy's tuned BLAS, and on whole numbers — where BLAS has no path and
  numpy falls back to its own loop — about 25× ahead of it. Whole numbers
  in give whole numbers out.
- J's monadic `u . v`, the determinant: `-/ . * m` is the determinant
  proper, and the same expansion with other functions is available.
  Ordinary numbers go by elimination; exact integers and rationals keep
  their exactness.
- APL's `⍠`, the variant operator: one setting of the dialect overridden
  for one application. `1 (=⍠0) 1+1E¯14` compares exactly where the
  program's own comparison tolerance would not, and `⍳⍠('IO' 0)` counts
  from zero inside a program that counts from one.
- J's sequential machine, the dyad of `;:`: a table-driven tokeniser
  written as a state machine, in all six of its result forms — the words
  themselves, their positions and lengths, or a full trace of the run.
- J's format by specification, the dyad of `":`: a field width and a
  number of decimals per column, with an exponential form and the
  reference's asterisks for a value that does not fit.
- J's number reading, the dyad of `".`: the numbers a line of text spells,
  with a value of your choosing standing in for anything that is not one —
  `0 ". '1 2 x 3'` is `1 2 0 3`.
- `bench/cloud/`: a design, scripts and IAM policies for one-shot rented
  spot-instance runs — AVX-512, Graviton and an NVIDIA GPU, the three
  machines this project's own numbers have never been taken on. Nothing in
  it has been executed and every script refuses to start until the owner
  fills in his account's details.

### Changed

- Elementwise passes over two different element types — complex against
  float, integer or boolean against float, boolean against integer — now
  promote the narrower operand where they read it instead of widening it
  into a buffer of its own first, and the fused kernel promotes a narrow
  argument one block at a time. Results are unchanged to the last bit; at
  20M elements `{c} + {f}` runs 2.5x faster, `{i} + {f}` 2.2x and
  `+/ {i} * {f}` 5.5x. See bench/README.md, "Mixed-type passes".
- Reductions, scans and moving windows over a yes/no column now read it as
  it lies instead of expanding it to whole numbers first, so summing one is
  finally cheaper than summing a column of numbers rather than ten times
  dearer. Results are unchanged to the last bit; at 20M elements `+/ {b}`
  runs 24x faster, `>./ {b}` 38x, and the scans and moving windows two to
  three times. See bench/README.md, "Folds over one buffer".
- Four verbs that were correct but far slower than the work they describe
  are now the algorithm they describe, with the same answers. Between them
  they close the three losses bench/workloads.md diagnosed:
  - **The suffix scan `u/\.` is one pass.** Folding right to left is the
    insert's own order, so each suffix is one step past the suffix after
    it — for any verb, not just an arithmetic one. It used to fold every
    suffix from scratch. This is what J's spelling of an exponential
    smoothing rests on (`|. u/\. |. y`), and RSI(14) over 20 million bars
    went from n²/2 steps of a general dyad — about nine years — to 2.3
    seconds. Floats come out bit for bit what the old path gave.
  - **A first-order recurrence is recognised and run as one.** A scan whose
    step is the fork `[ + c * ]` (or its mirror `(c * ]) + [`) with a
    constant `c` becomes `acc = y + c*acc` over the buffer instead of an
    interpreted step per item — about ten nanoseconds an element rather
    than a microsecond.
    The rule matches that tree and nothing else; anything it declines takes
    the general path, which is itself linear backwards now.
  - **The key `u/.` hashes its keys.** It used to find each group by
    sweeping the whole key vector, which is rows × groups: a VWAP over 20
    million minute bars grouped by 13,889 days took hours and now takes 1.4
    seconds. Groups still come out in first-occurrence order, and keys
    compared under a tolerance — floats, complex numbers — still are. APL's
    `f⌸` gets the same fix.
  - **A reshape that keeps the elements shares the buffer.** `2 3 $ i. 6`
    and any other `$` whose result the argument's own elements already
    cover is now a refcount bump rather than an element-by-element copy of
    the ravel; a shape that cycles the ravel still copies. The frame-RMS
    workload's reshape of 16 million samples went from 457 ms to nothing
    measurable, and the whole workload from 535 ms to 49 on one thread.
- Two APL spellings moved from "not supported yet" to "absent by design",
  because neither is work that can be queued: `⌶` (I-beam) is defined by
  each interpreter for itself and has no published behaviour to follow,
  and `&` (spawn) starts an APL thread, which libjay's sandbox does not
  open — the same rule J's `T.` and `t.` already fell under. With those
  moved, nothing in APL's primitive tables is a promise, and J's remaining
  two, `s:` and `$.`, are storage kinds rather than primitives.

### Fixed

## 0.2.0 — 2026-08-21

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
