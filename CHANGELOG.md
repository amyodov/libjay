# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [semantic versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

### Changed

### Deprecated

### Removed

### Fixed

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
