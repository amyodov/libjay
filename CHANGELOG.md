# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [semantic versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- APL trains, as an extension shipped on by default under the `trains`
  dialect setting: `(g h)` is an atop, `(f g h)` a fork, a literal value
  may be a fork's left tine, and longer runs group from the right. `⊢` and
  `⊣` are the identity tines. They lower to the same fork and atop verbs
  the J frontend builds.
- APL function assignment: `F←+/` names a derived function and `F←+/÷≢` a
  train, under the same `trains` setting. `Dialect(trains=False)` restores
  GNU APL's reading, where both are a syntax error.
- J names adverbs and conjunctions: `m =. /`, `c =. @`. The name is that
  modifier from the next sentence on.
- J writes adverbs and conjunctions: `1 : '…'`, `2 : '…'`, the `1 : 0` and
  `2 : 0` bodies on the lines below, and the `{{ … }}` whose body names an
  operand. Operands arrive as `u` and `v` when they are verbs and as `m`
  and `n` when they are nouns. A body that names `x` or `y` becomes the
  derived verb's body; one that names neither runs when the modifier meets
  its operands, so `1 : 'u @ u'` derives a tacit verb and `1 : '3 + 4'` a
  noun. A `{{ }}`'s part of speech follows the operand names its body uses,
  and the `{{)a` `{{)c` `{{)v` `{{)d` `{{)m` markers state it outright.
- J `L:` and `S:` gained their dyads: both arguments are descended together
  and a side that has reached its level is held while the other descends.
- J `H.`, the generalised hypergeometric series, with the parameters the
  two lists share cancelled first.
- The input half of the sandbox. APL `⍞` is one line of input as a
  character vector and `⎕` is one line evaluated as APL over the program's
  own names; `⍞←` writes its argument's characters and nothing else, where
  `⎕←` ends the line. J's `1!:1 ]1` reads a line and `x 1!:2 ]2` writes
  one. Reading past the end of the input is a reported error, never an
  empty line.
- J's `!:` foreign conjunction, as a dispatcher over its two numbers:
  `1!:1`, `1!:2` and `3!:0` (J's type code for an element type) are
  implemented, the foreigns that reach a file, a script, the host, the
  clock or a shared library are closed by the sandbox, and the ones that
  only compute name themselves as gaps.
- `Program::run_io` and `Program::run_on_io` attach an input source to a
  run; `Program::run` and `Program::run_on` are unchanged and have none.
  In Python, `Kernel.__call__`, `Kernel.run_display` and the one-shot take
  `input=`, a callable answering one line per call and None at the end; it
  defaults to this process's standard input, so `libjay -e '⍞' --lang apl`
  reads what is piped or typed. In C, `jay_run_io` takes a `jay_read_fn`
  beside the write callback; `jay_run` keeps its signature and its meaning.
- `ErrorKind::Sandbox`, labelled "closed by the sandbox": a feature the
  host closes, which is neither "not in the language" nor a promise to
  implement it later.

### Changed

- The refusals that were the sandbox speaking now carry
  `ErrorKind::Sandbox` rather than `ErrorKind::Language`: APL's `⎕TS`,
  `⎕AI`, `⎕FIO` and their relatives, and J's `T.`. The rendered text still
  says "closed by the sandbox", now as the error's label.

### Deprecated

### Removed

### Fixed

- APL's `⍢`, `⌺`, `⍠`, `⌶` and `⍞` are reported as named gaps rather than
  unknown characters.
- APL `(+)/1 2 3` is 6, as the reference answers it: a parenthesised
  function now closes before the operator to its right binds, and
  parentheses around a bare operator glyph (`1 0 1(/)1 2 3`) are
  transparent.

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
