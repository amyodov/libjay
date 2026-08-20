# Decision log

What was chosen and why, dated. Consult before relitigating any of it; the
operative rules distilled from these live in CLAUDE.md. Newest at the end.

- 2026-08-20 — License: MIT (owner).
- 2026-08-20 — Layout: cargo workspace `crates/libjay` (core, lib name `jay`)
  + `crates/libjay-python` (pyo3 cdylib `_jay`, abi3-py310); Python package
  under `python/`; maturin builds the wheel. Edition 2024, MSRV 1.85.
- 2026-08-20 — CLI is Python (`libjay._cli`): `uvx libjay` is the actual
  requirement and the binding must exist anyway.
- 2026-08-20 — Python import renamed `libjay` → `jay` (owner, superseding
  the original record): matches Rust `use jay::` and C `-ljay`,
  pillow/PIL-style. The PyPI package and the CLI stay `libjay`.
- 2026-08-20 — IR: an Expr tree over a language-agnostic Verb combinator
  tree (Prim / Rank / Reduce / Fork / NounFork / Hook / Atop / Windowed /
  Compose / bonds). One generic rank/frame/agreement engine executes
  everything; frontends only lower syntax (APL `+/` = `Rank(Reduce(+),1)`).
  Tree-walking evaluator with elementwise and reduce fast paths; codegen
  only when profiling justifies.
- 2026-08-20 — Runtime error semantics (original §7 item 2): each language
  follows its own documented rules (J: `0%0`=0, `n%0`=∞, int overflow
  promotes to f64; APL: `0÷0`=1, `n÷0` domain error). Known deliberate
  divergences (exact float comparison, char-vs-number comparison refused,
  APL monadic `÷0`) are listed in docs/coverage.md.
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
  REFUSED (resolves original §7 item 1: visible, suggests .copy()). Arrow
  deps live in the binding crate only; the core stays dependency-free.
- 2026-08-20 — CI Rust toolchain pinned to the local 1.85 so clippy findings
  are reproducible; revisit at publishing.
- 2026-08-20 — Coverage wave 1 (reverse/rotate, catenate, grade, index-of,
  membership, from, match, lcm/gcd, log/root, nub, tail/curtail, inc/dec…)
  in both languages; the oracle corrected the spec (`,.` = `,"_1`, `-.` =
  1-y, LCM sign), which set the standing oracle-wins rule.
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
  sink, so a parallel path cannot capture the sink.
- 2026-08-20 — Owned buffers are refcounted (`Buf` Owned = Arc<Vec<T>>,
  copy-on-write via make_mut): naming a value and re-mentioning it is free
  (std_named 703→235 ms / 532→130 ms). Buf's Send/Sync bounds tightened to
  T: Send + Sync. Known remaining copy: `Buf::slice` on owned data
  (cells/items) — deliberate, revisit with the layout-aware runtime.
- 2026-08-20 — uv is the tool of choice for everything Python (owner).
- 2026-08-20 — Lockfiles: Cargo.lock committed (reproducible CI and release
  wheels; crate consumers ignore it anyway). uv.lock not used: the wheel has
  no runtime deps, and dev/test deps deliberately float so CI tests against
  the polars/pyarrow users actually install; revisit if upstream churn makes
  CI flaky (then: locked default + scheduled latest job).
- 2026-08-20 — Benchmark rivals (incl. numba, required by the original
  record's "measured against Polars and numba") live in the `bench` extra;
  the Intel-mac numba<0.62 pin is an environment marker there.
- 2026-08-20 — Phase 5: one `Verb::Windowed(u, kind)` covers J `u\`/`u\.`
  (prefix/suffix monads, infix/outfix dyads) and APL scan (its third valence
  pairing is why it isn't three variants). Window fast path is the two-pass
  block algorithm (suffix+prefix folds per block) — O(n), uniform across
  Add/Mul/Min/Max, and more accurate than sliding accumulators (window error
  = error of that window alone). Oracle corrected several window edge cases
  (x>n keeps the cell shape; x=0 gives n+1 identities; left rank 0) and the
  `_4 o.` sign. Gate met: Bollinger z-score one-kernel beat the Polars
  pipeline at 20M rows pre-fusion (687 vs 759 ms, 8.7e-10 agreement).
- 2026-08-20 — Reference APL oracle: GNU APL 2.0, built from the FSF tarball
  under `~/projects/libjay-oracles/gnu-apl/` (run-only, never linked, never
  read), overridable via LIBJAY_ORACLE_APL; differential corpus in
  tests/oracle_apl.rs. Triangulation fixed our empty-reduction identity
  table (every verb both references give a neutral cell now has one).
  Deliberate dialect differences are a KNOWN_DIVERGENCES list in that file,
  asserted to STAY divergent, plus a docs/coverage.md subsection.
- 2026-08-20 — Expression fusion (delegated, IR design): a compile-time pass
  (`fuse.rs`, run where Program is assembled) replaces a maximal subtree of
  elementwise primitives with `Expr::Fused` — a postfix kernel over
  cache-sized block buffers (8,192 elements; measured flat from 2K to 32K),
  parallel across blocks, absorbing a full-rank `+/ * / <./ >./` over a
  vector into the same pass. NumExpr-shaped, not codegen. Only verbs that
  cannot fail elementwise join a kernel (no `%:`, `^.`, `^`, APL `÷`, APL
  `~`), the arguments must have identical shapes with rank-0 broadcast, and
  one working type (i64 or f64, chosen by replaying the promotion rules over
  the program) must hold every value; anything else — a frame needing
  agreement, a narrowing dtype, an integer overflow — falls back to the
  original subtree, which the node keeps, so results and error messages
  cannot change and leaves must be replayable (no echo/assignment inside).
  Fused maps are bit-identical to unfused ones; a fused reduce regroups as
  §5.9 allows. 20M rows: `+/ w * x` 67.5→14.6 ms, one-sentence std
  221.6→38.3, `+/ ^ x` 72.2→33.0, Bollinger z-score 675→437.
- 2026-08-20 — Publishing: PyPI via trusted publishing from publish.yml
  (abi3 wheel matrix: linux x86_64/aarch64, mac x86_64/arm64 — Intel wheel
  cross-compiled, GitHub retired Intel runners — windows x64, plus sdist);
  crates.io job in the same pipeline behind CRATES_PUBLISH (first publish is
  manual — crates.io attaches trusted publishers only to existing crates).
  GitHub releases carry assets: wheels, sdist, and per-platform C ABI
  bundles (the C ABI's only distribution channel). Docs split by audience:
  root README (landing) / python/README.md (PyPI page, absolute links) /
  crates/libjay/README.md (crates.io page); the "not a DataFrame library"
  expectation-setting lives in the Python-facing page, not the root opening
  (owner, superseding the original §1.2 placement).
