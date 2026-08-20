# libjay

Embeds array languages — J, and APL alongside it — into host languages:
compile an expression from a string literal, run it in parallel. The model is
PCRE: compile/execute split, heavy optimisation inside, thin stable C ABI
outside. Not a DataFrame library, not a framework; the string is the
interface. Performance is measured against Polars and numba.

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
  PyPI/import `libjay`, CLI `libjay`. License: MIT.
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

Open: non-contiguous inputs; runtime error semantics; APL oracle;
pure-expression assertion flag; primitive ordering; complex column naming;
sandbox surfacing; FFT-class operations.
Delegated (owner has no opinion, decide and log): codegen backend, IR design,
threading, crate structure, CLI implementation language, caching/dispatch
internals.
