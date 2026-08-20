# libjay

Independent implementations of J and APL — as much of each language as
possible — built to be easily embeddable: Rust, Python, and a stable C ABI
for everything else. Expressions compile from string literals and run in
parallel. The interface model is PCRE: compile/execute split, heavy
optimisation inside, a thin boring surface outside. Not a DataFrame library,
not a framework; the string is the interface. Performance is measured against
Polars and numba; language coverage is a goal in itself, not a by-product.

This file holds only the rules a session needs to act correctly. The dated
decision history with reasoning is docs/decisions.md — consult it before
relitigating anything; git history carries the rest.

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
subprocess oracles; that is the differential test suite — and where a spec
or a guess disagrees with the reference, THE ORACLE WINS.

## Fixed points

- Names: Cargo package `libjay`, Rust lib `jay` (avoids `liblibjay.so`),
  PyPI package `libjay`, Python import `jay`, CLI `libjay`. License: MIT.
- Two frontends (J + APL) over one language-agnostic IR; the IR carries no
  J-specific assumptions.
- Dialect is an object (holds e.g. `⎕IO`). compile → bind → execute; `bind`
  returns a new object; override cascade literal → bind → call. Data is an
  explicit dict, options are keywords. Braces bind data, never splice code.
- Sequence semantics = Rust's block model: value of the last sentence;
  assignments yield nothing.
- t-strings (PEP 750) are the primary spelling; pre-3.14 uses explicit args,
  same model. Python floor 3.10; t-string code lives in a conditionally
  imported module. Samples are both the type/shape contract and live
  defaults (the kernel keeps them alive). Rank/dtype fixed at compile time,
  axis lengths checked at bind.
- `jay.j(...)` = compile+bind+execute; `jay.j` is a callable singleton with
  attributes (the `re` module shape).
- Diagnostics are contract: errors point into the source string; shape
  errors show both shapes; data errors name the column and suggest the fix;
  "the language lacks this" (permanent) and "not implemented yet" (a
  promise) sound different.
- Data: Arrow (`arrow-rs`), not Polars; zero-copy is mandatory. DataFrame
  M×N → matrix, rows leading. Refuse nulls, mixed columns, non-contiguous
  views — report and stop rather than guess on the user's behalf. Boxes,
  bigints, rationals come later — nothing is excluded permanently, so don't
  bake rectangular/homogeneous in as an unrelaxable invariant.
- Build: one artifact per platform, runtime CPU/GPU dispatch; stable Rust;
  abi3 wheels, user never sees rustc; compilation stays hermetic. Zero
  hand-written per-primitive SIMD kernels — vectorisation is the backend's
  job (owner invariant).
- Sandbox: stdio open by default, other I/O closed; one entity, two
  surfaces. Array pretty-printing (J-style planes) is ours.
- Tests compare results on data (parametrised), never call wiring; floats
  within a justified tolerance. Corpora: OHLCV and DSP/audio.
- uv for everything Python. Publishing (PyPI, crates.io, releases) is
  owner-driven; push milestones to GitHub freely.

## Style

Plain, factual prose everywhere. A docstring states the attached thing's
contract and purpose — not design history, not its callers. Few meaningful
commits. Decisions with reasoning go to docs/decisions.md, not here.

## Working notes

- Oracles (run-only quarantine): J at
  `~/projects/libjay-oracles/j/j64/jconsole -jprofile /dev/null`
  (LIBJAY_ORACLE_J), GNU APL at
  `~/projects/libjay-oracles/gnu-apl/install/bin/apl` (LIBJAY_ORACLE_APL).
  Differential suites: tests/oracle.rs, tests/oracle_apl.rs.
- Venvs: `.venv` (3.14, dev), `.venv-bench` (3.12, `'.[bench]'`). Both share
  python/jay/_jay.abi3.so — rebuild `maturin develop --release` before
  benchmarking. LIBJAY_THREADS caps the pool; LIBJAY_CPU_LEVEL pins the CPU
  feature level (`baseline`, `v2`, `v3`, `native`), read once per process.
- Phases (original roadmap): 1–6 done (frontends, IR, Arrow, parallelism,
  time series, SIMD dispatch) + C ABI, fusion, dual oracles, boxes; next:
  7 GPU/device, 8 bigints/rationals/control structures.

## Open

Pure-expression assertion flag; primitive ordering; complex column naming;
sandbox surfacing; FFT-class operations. Delegated (decide and log):
codegen backend, IR internals, caching/dispatch internals.
