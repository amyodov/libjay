# libjay

Independent implementations of J and APL — as much of each language as
possible — built to be easily embeddable: Rust, Python, and a stable C ABI
for everything else. Expressions compile from string literals and run in
parallel. The interface model is PCRE: compile/execute split, heavy
optimisation inside, a thin boring surface outside. Not a DataFrame library,
not a framework; the string is the interface. Performance is measured against
Polars and numba; language coverage is a goal in itself, not a by-product.

This file is the project's decision record. It stays short because it is
loaded into every session; prune sections once code, tests, or docs carry
them.

## Clean-room (owner rule, permanent)

libjay is an independent implementation, as RE2 is to PCRE.

**Never read the source code of any J or APL implementation.** Not
Jsoftware's `jsource`, not GNU APL, not Dyalog, not dzaima/APL, not ngn/apl,
not April, not BQN, not any other Iverson-family engine. Not a file, not a
snippet, not a vendored copy, not a search result or web page that happens to
contain it. There is no "just to check one thing" exception. If such code
lands in context by accident, do not use it and tell the owner.

Published documentation is the specification: read it freely, paraphrase it,
never copy its prose. Reference interpreters may be run as black-box
subprocess oracles (feed expression and data, compare output); that is the
differential test suite.

## Fixed points

- Names: Cargo package `libjay`, Rust lib `jay` (avoids `liblibjay.so`),
  PyPI package `libjay`, Python import `jay` (owner decision 2026-08-20,
  superseding the original record: the import matches Rust `use jay::` and
  C `-ljay`, pillow/PIL-style), CLI `libjay`. License: MIT.
- Two frontends (J + APL) over one language-agnostic IR from day one; the IR
  carries no J-specific assumptions. Early coverage = the divergence set:
  reduction axis, rank, indexing/index origin, transpose/reshape/take/drop.
- Dialect is an object (holds e.g. `⎕IO`). compile → bind → execute; `bind`
  returns a new object; override cascade literal → bind → call. Data is an
  explicit dict, options are keywords. Braces bind data, never splice code.
- Sequence semantics = Rust's block model: value of the last sentence;
  assignments yield nothing.
- t-strings (PEP 750) are the primary spelling; pre-3.14 uses explicit args,
  same model. Python floor 3.10; t-string code lives in a conditionally
  imported module. Samples are both the type/shape contract and live defaults
  (the kernel keeps them alive — document that). Rank/dtype fixed at compile
  time, axis lengths checked at bind.
- Shortcut `libjay.j(...)` = compile+bind+execute; `libjay.j` is a singleton
  with `__call__` plus attributes (the `re` module shape).
- Diagnostics are contract: errors point into the source string; shape errors
  show both shapes; there is a way to see what the expression became; data
  errors name the column and suggest the fix; "the language lacks this"
  (permanent) and "not implemented yet" (a promise) sound different.
- Data: Arrow (`arrow-rs`), not Polars; zero-copy is mandatory. DataFrame
  M×N → matrix, rows leading. Refuse nulls and mixed columns — report and
  stop rather than guess on the user's behalf. Supported: bool, i8–i64,
  f32/f64, complex (paired `_re`/`_im` columns at the Arrow boundary),
  decimal128, timestamp/date/duration as i64. Boxes, bigints, rationals come
  later — nothing is excluded permanently, so don't bake
  rectangular/homogeneous in as an unrelaxable invariant.
- Build: one artifact per platform, runtime CPU/GPU dispatch; stable Rust;
  abi3 wheels, user never sees rustc; the Rust macro and runtime function
  share one implementation; compilation stays hermetic. Zero hand-written
  per-primitive SIMD kernels — vectorisation is the backend's job (owner
  invariant).
- Sandbox: stdio open by default, other I/O closed; one entity, two surfaces
  (Rust/Python). Array pretty-printing (J-style planes) is ours.
- Tests compare results on data (parametrised: `rstest`,
  `pytest.parametrize`), not call wiring; floats within a justified
  tolerance. Corpora: OHLCV and DSP/audio.

## Style

Plain, factual prose everywhere. A docstring states the attached thing's
contract and purpose — not design history, not its callers. Few meaningful
commits. Local commits only: GitHub, PyPI, and crates publishing are
owner-driven.

## Phases

1 it runs (both parsers, hello world that prints, CLI, `uvx libjay`, README) ·
2 the IR bet settles (J/APL correct-and-different `+/` via one IR) ·
3 Arrow/Polars zero-copy in and out · 4 parallelism, first benchmarks ·
5 time series (moving windows, scan) · 6 SIMD runtime dispatch · 7 GPU/device ·
8 boxes, bigints, rationals, control structures.

## Decision log

Choices made during implementation, with reasoning. No entry = still open.

- 2026-08-20 — License: MIT (owner).
- 2026-08-20 — Layout: cargo workspace `crates/libjay` (core, lib name `jay`)
  + `crates/libjay-python` (pyo3 cdylib `_jay`, abi3-py310); Python package
  under `python/`; maturin builds the wheel. Edition 2024, MSRV 1.85.
- 2026-08-20 — CLI is Python (`libjay._cli`): `uvx libjay` is the actual
  requirement and the binding must exist anyway.
- 2026-08-20 — IR: an Expr tree over a language-agnostic Verb combinator
  tree (Prim / Rank / Reduce / Fork / NounFork / Hook / Atop). One generic
  rank/frame/agreement engine executes everything; frontends only lower
  syntax (APL `+/` = `Rank(Reduce(+),1)`). Tree-walking evaluator with
  elementwise and reduce fast paths; codegen only when profiling justifies.
- 2026-08-20 — Runtime error semantics (§7 item 2): each language follows
  its own documented rules (J: `0%0`=0, `n%0`=∞, int overflow promotes to
  f64; APL: `0÷0`=1, `n÷0` domain error). Known deliberate divergences
  (exact float comparison, char-vs-number comparison refused, APL monadic
  `÷0`) are listed in docs/coverage.md.
- 2026-08-20 — A sequence ending in an assignment (or `⎕←`) yields no value;
  inline assignment yields its value in expression position.
- 2026-08-20 — Plain-string interpolation is exactly `{identifier}`; any
  other `{` is program text (J's `{.` forced this). t-string interpolations
  must be identifiers; non-identifiers get an error telling the user to
  name the value.
- 2026-08-20 — Reference J oracle: official prebuilt binaries (never
  sources) under `~/projects/libjay-oracles/` (owner-approved), overridable
  via LIBJAY_ORACLE_J; differential tests compare output with 1e-5 relative
  / 1e-9 absolute tolerance (both sides print 6 significant digits, so
  representation error is ≤ ~5e-6 per side).
- 2026-08-20 — Every README example also lives runnable in `examples/`
  (owner: APL is hard to type; files are copy-paste-free).
- 2026-08-20 — Phase 3 boundary (details in docs/coverage.md): zero-copy for
  Arrow Int64/Float64 and the physically-i64 temporal types, and for numpy
  C-contiguous i64/f64 (via `__array_interface__` — pyo3's buffer module
  needs Py_3_11 and we build abi3-py310); narrower types widen with one
  copy; N≥2-column tables interleave column-major → rows-leading with one
  copy until the layout-aware runtime. Non-contiguous numpy input is
  REFUSED (resolves §7 item 1: visible, suggests .copy()). Arrow deps live
  in the binding crate only; the core stays dependency-free.
- 2026-08-20 — CI Rust toolchain pinned to the local 1.85 so clippy findings
  are reproducible; revisit at publishing.
- 2026-08-20 — Coverage wave 1 (reverse/rotate, catenate, grade, index-of,
  membership, from, match, lcm/gcd, log/root, nub, tail/curtail, inc/dec…)
  in both languages; 479-expression differential corpus, 100% oracle
  agreement. Standing rule: where a spec and the reference J disagree, the
  ORACLE WINS for J (it corrected `,.` = `,"_1`, `-.` = 1-y, LCM sign).
  Known scoped gaps are in docs/coverage.md.
- 2026-08-20 — C ABI: crates/libjay-capi, [lib] name "jay", cdylib+staticlib
  only (rlib would collide with the core's libjay.rlib) → libjay.dylib/.so +
  libjay.a, -ljay, hand-written include/jay.h. Inputs copied at the C
  boundary for now (no lifetime contract in the ABI yet); docs/embedding.md.
- 2026-08-20 — Threading (delegated): rayon; one pool owned by the core
  crate (not rayon's global one, so an embedding host keeps its own), sized
  by available_parallelism or LIBJAY_THREADS, read once. Parallel:
  elementwise monad/dyad passes (chunked; integer overflow folded per chunk,
  then the pass redone in f64), rank-machinery cells when the verb is pure
  (`Verb::is_pure` — false iff the tree contains echo), and leading-axis
  reduce (wide items split by column, which preserves the fold order for any
  verb; narrow items split into chunks of items, associative verbs only —
  the float reassociation is the §5.9 contract). Nothing splits below 65,536
  element operations. `Ctx` splits into a Copy `EvalCfg` plus the output
  sink, so a parallel path cannot capture the sink. Numbers, and the one
  thing that now costs more than threading buys (no fusion), are in
  bench/README.md.
- 2026-08-20 — Owned buffers are refcounted (`Buf` Owned = Arc<Vec<T>>,
  copy-on-write via make_mut): naming a value and re-mentioning it is now
  free (std_named 703→235 ms / 532→130 ms). Buf's Send/Sync bounds
  tightened to T: Send + Sync. Known remaining copy: `Buf::slice` on owned
  data (cells/items) — deliberate, revisit with the layout-aware runtime.
- 2026-08-20 — uv is the tool of choice for everything Python: venvs,
  installs, running (owner). `uvx libjay` is the recommended try-it-now
  path once published.
- 2026-08-20 — Lockfiles: Cargo.lock committed (reproducible CI and release
  wheels; crate consumers ignore it anyway). uv.lock not used: the wheel has
  no runtime deps, and dev/test deps deliberately float so CI tests against
  the polars/pyarrow users actually install; revisit if upstream churn makes
  CI flaky (then: locked default + scheduled latest job).
- 2026-08-20 — Benchmarks run in .venv-bench (Python 3.12): numba wheels for
  Intel macs stop at numba 0.61/llvmlite 0.44, so the bench venv pins
  numba<0.62 (the dev venv stays 3.14). Both venvs share
  python/jay/_jay.abi3.so — a dev `maturin develop` silently replaces the
  release build; rebuild --release before benchmarking.
- 2026-08-20 — Phase 5: one `Verb::Windowed(u, kind)` covers J `u\`/`u\.`
  (prefix/suffix monads, infix/outfix dyads) and APL scan (its third valence
  pairing is why it isn't three variants). Window fast path is the two-pass
  block algorithm (suffix+prefix folds per block) — O(n), uniform across
  Add/Mul/Min/Max, and more accurate than sliding accumulators (window error
  = error of that window alone). Oracle corrected several window edge cases
  (x>n keeps the cell shape; x=0 gives n+1 identities; left rank 0) and the
  `_4 o.` sign. Gate met: Bollinger z-score one-kernel beats the Polars
  pipeline at 20M rows (687 vs 759 ms, 8.7e-10 agreement); remaining gap to
  numba is fusion, the known next perf lever.

Open: APL oracle;
pure-expression assertion flag; primitive ordering; complex column naming;
sandbox surfacing; FFT-class operations.
Delegated (owner has no opinion, decide and log): codegen backend, IR design,
crate structure, CLI implementation language, caching/dispatch internals.
