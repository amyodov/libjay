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
  2026-08-21 — Ratified by the owner: Cargo.lock stays committed (consumers
  of the library crate ignore it; wheels and binaries stay reproducible);
  uv.lock stays ignored because libjay is a library with no runtime
  dependencies — the dev/test extras float on purpose.
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
- 2026-08-20 — Boxes (phase 8 core): `DType::Box`, `Data::Box(Buf<Array>)`;
  structural verbs work on boxes through the generic engine; J `< > ;` with
  the oracle-verified link rule (`x;y` = `(<x),y` iff y boxed), raze with
  fill, box fill `a:`, each as `u&.>`; APL2 semantics per GNU APL (`⊃`
  disclose, `↑` first — opposite of Dyalog), `⊂ ¨ ∊ ≡` and stranding
  (numeric-literal runs stay flat). One `Enclose` enum carries the J/APL
  split. J box drawing reproduced exactly (differentially tested against
  jconsole, +78 display expressions). Python: ragged/mixed lists box, boxes
  convert to nested Python values; C ABI declines boxed results for now.
  Named gaps (deep grade ordering, general `&.`, APL dyadic `⊂⊃`, monadic
  `↓`, mixed simple arrays) in docs/coverage.md.
- 2026-08-20 — Fusion across assignments: the pass moves a named chain into
  its consumers when every use lands inside one kernel (decided by re-running
  the real chain analysis), hoisting the value's reduced leaves into
  sentences of their own — the two-phase kernel std needs. The elided
  assignment leaves a `Yield::Tally` guard that raises exactly the errors the
  original would, in order. Per-block let slots dedupe repeated subtrees;
  structurally equal leaves share one input. Result: both std spellings
  compile to numba's two passes (18.1/21.0 ms vs numba 17.6 at 20M×8thr).
- 2026-08-20 — SIMD dispatch (phase 6) is function multiversioning, not
  intrinsics: the `multiversion` crate compiles each hot leaf loop once per
  CPU feature level and a cached runtime check picks the clone, so libjay
  writes no `core::arch` code and no `unsafe` and the owner invariant holds
  — the autovectoriser is still the only thing making vectors. Levels:
  baseline, x86-64-v2, x86-64-v3 (AVX2+FMA), and aarch64 NEON as the top
  rung there; no AVX-512 rung, because `avx512*` has no stable
  `target_feature` name on the pinned 1.85 toolchain. Nine loops carry the
  annotation — the four fused block kernels, the two unfused elementwise
  chunk passes, the typed reduce across an item's columns, the scan and the
  moving window — which is the whole arithmetic of the fast paths in about
  as many lines of attribute. `LIBJAY_CPU_LEVEL` pins the level, clamped to
  what the CPU can run, and `simd::set_level` turns the same knob in
  process so tests/simd.rs can compare every level on one data set (maps
  bit-identical, reduces to 1e-12). A vector clone is declined where the
  loop that would widen is shorter than 16 elements
  (`verb::VECTOR_COLUMNS`): measured on `+/ m` at 20M f64, 4 and 8 columns
  are ~1.5x slower under AVX2 and 16 columns and up are 1.2x to 1.6x
  faster, so narrow items take the baseline compilation. What it buys is
  modest and honest — the numbers are in bench/README.md.
- 2026-08-20 — `cargo test` is a closed system (owner): no subprocesses, no
  external binaries, predictable runtime. The differential suites became
  snapshot batteries — `tests/snapshots/j.snap` (2942 records),
  `apl.snap` (650) and `apl_divergences.snap` (26) hold each expression and
  the reference interpreter's answer, the generated corpora materialised so
  the generator runs only on a refresh. The batteries evaluate libjay against
  the recorded answers with the same tolerance-aware comparison as before;
  the oracles run only under `LIBJAY_REFRESH_ORACLE=1` (verify) or `=write`
  (rewrite), where a missing interpreter is a failure rather than a skip.
  Format: line-tagged plain text (`=` expression, `>` the reference's answer,
  `<` libjay's, `?` the note, `@ io=N`, `#` comment, `<error>` for a
  refusal), one line per output line so a diff reads as one line per changed
  answer. APL divergences record BOTH answers: the battery holds libjay to
  its side, the refresh re-checks that the pair still disagrees. Suite time
  29.3 s → 6.0 s warm (oracle 14.9 s → 0.21 s, oracle_apl 7.2 s → 0.03 s),
  and the differential corpus now runs on CI, which never had the oracles.
  Workflow: docs/testing.md.
- 2026-08-20 — APL monadic `⍳` has rank ∞, not 0 (owner-flagged bug): with
  rank 0 the frame machinery answered `⍳2 3` with a 2×3 array of its own
  invention. A non-scalar argument asks for an array of index vectors, so it
  is now `not_yet("nested index arrays (⍳ with an array argument)")` and a
  recorded divergence from GNU APL, which has them. J's `i.` reshapes on the
  same argument and is untouched.
- 2026-08-20 — Comparison tolerance, measured rather than assumed. Both
  languages compare reals with a relative tolerance, and the two rules are
  not the same one with a different constant. Probing each reference at the
  threshold settled it: J is `|x-y| < ct × (|x| ⌊ |y|)` with `ct` = 2⁻⁴⁴
  (`9!:18`), APL is `|x-y| < ⎕CT × (|x| ⌈ |y|)` with `⎕CT` = 1e¯13 — the
  SMALLER magnitude in one and the LARGER in the other, both strictly below.
  `1 = 1 + 2^_44` is 0 in jconsole while `1=1+2*¯44` is 1 in GNU APL, which
  is the pair that pins them apart; the ISO/Dictionary prose says "max" for
  both, so the oracle won. Exact equality (the infinities included) is
  equality whatever the tolerance is, and a comparison against zero is
  therefore exact. The tolerance lives on `EvalCfg` as a dialect value, next
  to `Agreement`, and reaches the comparisons, `-:`/`≡`, `e.`/`∊`,
  `i.`/`i:`/`⍳`, `~.`/`∪`, `I.`/`⍸` and the two roundings. It deliberately
  does NOT reach grade: both references leave `/:`, `\:`, `⍋` and `⍒`
  exact. Where the old code hashed for equality (nub, APL membership) the
  float case now compares directly — tolerant equality is not an
  equivalence relation, so no hash can stand in for it — and the integer,
  character and boxed cases keep the hash, which is the common path.
  `FusedKernel` carries the tolerance too, so a comparison inside a
  blockwise chain cannot answer differently from the same comparison
  outside one. J's `!.` is a new `Verb::Fit` that swaps the tolerance for
  the verb under it; it is accepted only on verbs whose meaning uses one,
  because on anything else J's `!.` specifies a fill, which is a separate
  feature and now says so by name. `⎕CT` as a runtime variable stays out,
  as `⎕IO` does.
- 2026-08-20 — Random numbers are libjay's own stream. `?` and `?.` (J) and
  `?` (APL) draw from MT19937, the published Mersenne Twister; `?.`
  restarts from a fixed seed on every invocation (which is the behaviour
  jconsole shows: the same sentence answers the same way twice in one
  process) and `?` is seeded once per process from the clock. Both
  references are in fact reproducible — jconsole's `?.` and GNU APL's `?`
  from `⎕RL←16807` — but neither publishes the stream, and the clean-room
  rule forbids the one way to find out, so matching them is not on offer.
  `?` and `?.` are therefore the two spellings kept out of the differential
  corpora; what is tested is the contract (range, shape, distinctness of a
  deal, and that `?.` repeats). Both are marked impure so their cells are
  never reordered across threads.
- 2026-08-20 — APL bracket indexing without a new IR node. `A[i;j]` lowers
  to one `SelectAxis` dyad per non-elided slot, applied from the LAST axis
  to the first so that a scalar slot dropping its axis leaves the axes still
  to come where they were. The slot applied first — the highest non-elided
  one — is the only one that sees the whole array, so it carries the check
  that the brackets held one slot per axis. Nothing else in the IR, the
  fusion pass or `explain` had to learn a new shape. Indexed assignment
  (`A[2]←99`) is deliberately out: the brackets are a reader.
- 2026-08-20 — `f[k]` (axis) is a verb wrapper, not a rank. `Verb::AlongAxis`
  brings axis k to the front, runs the verb on the leading axis, and puts
  the axis back when the result kept the argument's rank — which is exactly
  what separates a reduction (rank drops, remaining axes keep their order)
  from a scan or a reversal. Naming an axis also collapses each
  axis-choosing pair to one function, as GNU APL does: `f/[k]` and `f⌿[k]`
  are the same reduction and `⌽[k]` and `⊖[k]` the same reversal. Only those
  six spellings are wired; every other glyph reports
  `axis specification for X`.
- 2026-08-20 — GNU APL has no compose family at all: `∘` exists only as the
  left half of `∘.`, `⍥` and `⍛` are not in its character set, and `f⍤g`
  (atop) is a VALUE ERROR. So `∘`, `⍥`, `⍛` and `f⍤g` are no-oracle
  territory, not merely unimplemented, and docs/status.md says so rather
  than promising a differential test that cannot exist. `f⍣g` IS there and
  is implemented (`Verb::PowerUntil`: apply f until `new g old` holds).
  J's `u^:v` is a different operator with the same spelling — v supplies the
  COUNT, so `u^:v` alone is one conditional step and `(u^:v)^:_` is the
  while loop — and that is what `Verb::PowerV` does.
- 2026-08-20 — Matrix division is one Householder QR in f64, shared by J's
  `%.`/APL's `⌹` in both valences: the monad solves against an identity to
  get the (pseudo-)inverse, the dyad against the right-hand side. Both
  references refuse a matrix with more columns than rows and both refuse a
  rank-deficient one, so libjay does too — a length error and a domain error
  respectively, with the singularity detected from the QR diagonal relative
  to the largest magnitude in the argument. A vector argument is a column,
  as in both references, and keeps its own shape on the way out.

- 2026-08-20 — Collecting and testing split into two activities (owner).
  Collecting: `cargo run -p libjay-devtools -- record <j|apl>` runs the
  reference over `crates/libjay/tests/corpus/<lang>/*.txt` and writes
  `tests/snapshots/<lang>/*.snap`, one snapshot per corpus file, with
  `--check` (re-measure, write nothing, nonzero exit on drift), `--missing`
  (record only what is not recorded yet), `gen` (append generated
  expressions) and `stats`. Testing: `cargo test` replays, one rstest
  `#[files]` case per corpus file, so a failure names the theme; an
  expression with no record fails with `unrecorded: run jay-corpus record`.
  `LIBJAY_REFRESH_ORACLE` is gone — the recorder replaces it, and nothing in
  `cargo test` can reach an interpreter any more. The expressions moved out
  of the Rust arrays in `tests/oracle*.rs` into plain data files split by
  theme (at the move, J 3073 in 10 files and APL 766 in 10, with every
  recorded answer unchanged to the byte); the generator moved into the
  recorder and its output is now
  ordinary corpus lines. Corpus comments are `//`, not `#`, because `#` is
  J's tally and `# i. 5 2` is one of the expressions. Two new unpublished
  workspace members hold the shared code once: `libjay-testkit` (corpus and
  snapshot formats, comparison, replay — a dev-dependency of libjay and a
  dependency of the recorder) and `libjay-devtools` (the `jay-corpus`
  binary, the only thing in the repository that spawns an interpreter;
  recording a language takes ~4 s, one process per expression in parallel).
  CI is unchanged in what it runs — replay only, no oracle — and now also
  clippies the two new crates. Workflow: docs/testing.md.

- 2026-08-20 — Explicit definitions and control structures (phase 8, the
  language half). The oracles decided most of it; where they were probed the
  answer is in the corpus.

  **Scope.** The IR gained a `Scope` on every assignment and an `Env` with a
  stack of frames: J's `=.` writes the running definition's frame, `=:` the
  globals, and a lookup tries the frame then the globals. Frames do not
  nest — a definition called from another sees its own locals only — which
  is what jconsole does and what makes each recursive call independent. At
  the top level, which has no frame, both spellings name the same thing;
  the oracle agrees (`tl =. 55` is visible inside a verb defined after it).
  APL's dfns follow the same rule with `←`. APL's `∇`-definitions do NOT:
  there a name the header does not declare is global, and `;a;b` is what
  makes one local. Both were probed; both are in the corpus. A third scope,
  `LocalDefault`, carries APL's `⍺←v`, which assigns only where the name has
  no value yet.

  **Control-flow IR.** `Expr::Control(Box<Control>, Span)` rather than a
  separate statement type: the fusion pass, `explain`, the C ABI and the
  Python binding all walk `Vec<Expr>`, and a second node type would have
  rippled through every one of them for no gain. Control nodes only ever
  appear inside a definition's body — neither language allows a control word
  outside one, and jconsole calls `if.` at the top level a spelling error —
  so nothing above sees them. A block runs to a `Flow` (normal, return,
  break, continue) carrying the value in hand; `return.` produces no value of
  its own, which is why `3 : 'z =. 42 return. 7'` answers 42. One `Control`
  enum serves both languages: J's `whilst.` and APL's `:Repeat` are the same
  node with `body_first`, `:Until` adds `until`, and `fcase.` is a flag on a
  `select.` arm.

  **Recursion.** Two mechanisms: `Verb::Named(n)`, resolved when it is
  applied, so a definition can call itself by its own name (the frontend
  seeds the body's name table with it); and `Verb::SelfRef` for J's `$:` and
  APL's `∇`, resolved to the innermost definition then running. The guard is
  a depth limit — 64 — checked when a frame is pushed, raising a domain error
  that names the limit. 64 is not a language number: one level of a
  definition costs about 24 kB of machine stack in an unoptimised build, and
  the guard has to fire well inside the 2 MiB a test thread gets, or the
  process dies instead of reporting. It rises when the evaluator's frames do.

  **`$:` diverges from the oracle on purpose.** jconsole 9.6.3 reads `$:`
  inside an explicit definition as the largest verb in the SENTENCE, which is
  `$:` itself, so every recursion written that way is a stack error there —
  including the classic `fib =: 3 : 'if. y<2 do. y else. ($:y-1) + $:y-2 end.'`.
  libjay follows the published dictionary and names the definition. The J
  corpus leaves `$:` out (there is nothing to record) and
  tests/definitions.rs holds libjay to the documented rule. Recursion by name
  agrees with the reference and is in the corpus.

  **APL control structures have no oracle.** GNU APL raises SYNTAX ERROR for
  `:If` and every one of its relatives, at every indentation, through `⎕FX`
  as well; it branches with `→` and labels instead. Rather than leave the
  biggest red block in the language unimplemented, the `:If` family follows
  the published specification and is tested in tests/definitions.rs, with
  the absence of an oracle stated in docs/status.md and docs/coverage.md.
  `→` itself is a named gap. Dfn guards (`cond:expr`) and `∇` self-reference
  are in the same position and got the same treatment.

  **Indexed assignment** is `Expr::AmendIndex`: the name, one slot per axis,
  the value. It reads the name, copies, writes and reassigns, so a value
  taken from the array beforehand is untouched. It is its own node rather
  than a verb because the existing `Verb::Amend` takes a literal index and
  a verb cannot hold the slot expressions.

  **Recording multi-line APL.** `jay-corpus` fed GNU APL through `--eval`,
  which takes one line; a `∇` definition left the interpreter sitting in its
  definition editor. A program with a line break now goes in on stdin,
  closed with `)OFF`. Single-line programs still use `--eval`, so no existing
  recording moved.

  Deferred with a name: J's `1 :`, `2 :`, `13 :`, `{{ }}` modifier forms,
  `throw.`/`catcht.`/`goto_name.`/`label_name.`; APL's dfn operators
  (`⍺⍺`/`⍵⍵`), `→` branching, niladic `∇`-definitions.

- 2026-08-20 — **Complex numbers** (original record, Group 1; the DSP/audio
  scenario's prerequisite). `DType::Complex` with
  `Data::Complex(Buf<[f64; 2]>)`.

  **Layout: interleaved `[re, im]`.** Not two parallel buffers, and not a
  newtype. `[f64; 2]` is bit-identical to numpy's `complex128`, to C99's
  `double _Complex`, and to what a caller's own array already holds, so the
  numpy import BORROWS a complex block exactly as it borrows `float64`, and
  the C ABI takes and returns `double[2]` per element with no conversion at
  all. The cost is at the Arrow edge, which stores the parts apart; that is
  one boundary and it pays for zero-copy everywhere else. `Buf<[f64; 2]>`,
  `par::fill` and `zip_chunk` are already generic over the element type, so
  the parallel machinery took the new type without a line of change.

  **Demotion is DISPLAY only.** `1j0` prints as `1` and `(1j2)*(1j_2)` as
  `5`, per element, but the array stays complex — which is what J reports
  (`3!:0 ] 1j0` is 16, the complex type). Demoting the TYPE would have made
  every complex result's dtype depend on its values.

  **Ordering is refused, grading is not.** J answers `3j4 < 1j2` with a
  domain error, and so does libjay, in both languages — including dyadic
  `<.`/`>.`/`⌊`/`⌈`. GNU APL instead extends `< ≤ > ≥` to a lexicographic
  order on (real, imaginary); that is a GNU APL extension, not the standard,
  and it is recorded in corpus/apl/divergences.txt rather than copied.
  Grading is the other way round: `/:` on a complex vector works in J (a
  permutation is not a claim about size), GNU APL refuses it, and libjay
  follows J. Equality is tolerant on the MAGNITUDE of the difference, which
  is J's rule; GNU APL's complex tolerance is looser by about √⎕CT and is
  also a recorded divergence.

  **Escaping the reals.** `%: _4`, `^. _1`, `_4 ^ 0.5`, `2 ^. _8`,
  `_1 o. 2` and their relatives now answer in complex instead of reporting a
  gap. The decision is per PASS, not per element: a scan over the argument
  pair decides whether any element leaves the reals, and if one does the
  whole pass runs in complex — so `%: _4 9` is `0j2 3`, one complex array,
  as it is in J. The scan runs only for `^`, `^.`, `%:` and `o.`; nothing
  else can escape.
  Two rounding details came from the oracle rather than from a formula: a
  square root uses the algebraic form (so `%: _4` is exactly `0j2`, not
  `1.22e_16j2`), and a negative real raised to a real power turns on cos and
  sin of a multiple of π computed exactly at the quadrant boundaries. The
  same trick makes `2ad90` exactly `0j2`.

  **Fusion declines.** The blockwise kernel computes a whole chain in one
  type, and complex is not one of the two it has. `working_type` returns
  None as soon as an input is complex, and the ordinary pipeline runs the
  chain — with the same answer, which tests/complex.rs checks by running one
  chain both ways. Widening the kernel to a third working type is a
  separate, measurable change and nothing has asked for it.

  **Arrow: paired columns** (the original record's decision). Arrow has no
  complex type. A rank-1 complex result exports as
  `struct<re: f64, im: f64>` — the single-array form of a paired column,
  which a consumer splits with `unnest`. Import accepts both shapes: a
  `struct<re, im>` array, and two adjacent `f64` columns of a table named
  `x_re` and `x_im`, which become one complex column `x`. A `FixedSizeList`
  of 2 would have been zero-copy, but it names nothing, and naming the parts
  is what makes the convention readable at the far end.

  **FFT (original §7 item 8, resolved).** FFT is NOT a J or APL primitive:
  neither language spells it, so implementing it inside the primitive tables
  would invent vocabulary, which the clean-room rule and the "language
  coverage is the goal" rule both forbid. It belongs to the NAMED EXTENSION
  layer — the place for a carrier or a function that adds capability without
  changing any existing primitive's meaning — and it is not implemented in
  this wave. Complex numbers are the only thing that blocked it: a complex
  vector in and a complex vector out is the whole interface, and that now
  exists end to end (numpy zero-copy in, `struct<re, im>` out, `double[2]`
  across the C ABI). The criterion for anything else joining that layer is
  the same: it must add a carrier or a function without changing the
  semantics of a primitive either language defines.

  Deferred with a name: the gamma function of a complex argument (`!`),
  complex matrix inverse and matrix divide (`%.` / `⌹` stay f64), complex
  decode and encode (`#.`/`⊥`, `#:`/`⊤`), and rationals, which is the last
  of the Group-1 types.

- 2026-08-20 — **Extended-precision integers and rationals** (Group 4,
  category 2: present in J, absent from every APL, and the last of the
  "present in the language, not yet implemented" types).

  **Where they sit.** The numeric tower is
  `boolean < integer < extended < rational < float < complex`, oracle-checked
  pair by pair. Above the machine integers, so `1x + 1r2` stays exact; below
  the floats, so `1x + 1.5` rounds. The i64-overflow→f64 rule is untouched:
  `9223372036854775807 + 1` is still a float, and only an explicitly
  extended computation is exact. That keeps the fast path fast and makes
  exactness something the user asks for by writing `x`.

  **One rule decides the answer's type.** Compute exactly as rationals, then
  answer with an extended integer when both arguments were extended AND
  every value is whole; answer with a rational otherwise. That single rule
  reproduces every case the oracle showed: `4x % 2` extended, `1x % 3`
  rational, `1r2 - 1r2` a rational zero (a rational never falls back down
  the tower), `-: 1x` rational, `-: 2x` extended. Rounding and the sign are
  the named exceptions — `<.`, `>.` and `*` always answer whole. Computing
  everything through one rational kernel rather than two typed ones costs a
  denominator of 1 on the integer path, which a whole-number fast path in
  `Rat::add`/`sub`/`mul` takes back. An exact pass converts only the WINDOW
  of the buffer it really reads: a fold hands the same buffer to every step
  with a different offset, and converting all of it each time made `+/` over
  an extended vector quadratic (22 s at 20k elements, 0.05 s after).

  **Exact versus tolerant.** Two exact values compare exactly; no tolerance
  stands between them, so `(10x^30) = 1 + 10x^30` is 0. Against a float the
  pair's type is float and the ordinary tolerant rule applies — which is why
  `1r3 = 0.333333333333333333` is 1 and `1r3 < 0.333333333333333333` is 0.
  The reference agrees with both, and the two together are the whole rule:
  tolerance is a property of the TYPE the comparison happens in, not of the
  verb.

  **Our own rational, not `num-rational`.** `num-bigint` 0.5 is already in
  the tree (arrow pulls it into the Python crate); `num-rational` 0.4 is the
  newest published version and depends on `num-bigint` 0.4, which would put
  two incompatible bignums in one build. A rational in lowest terms is a
  gcd, a normalisation and eight operators — `crates/libjay/src/exact.rs` —
  and writing it keeps one bignum in the tree and puts J's own rules (the
  exactness tests, the residue's sign, the gcd of two rationals) where they
  are read.

  **Heap-backed, so no fusion and no SIMD.** `Data::Ext` and `Data::Rat` are
  pointer arrays like `Data::Box`: never foreign memory, never a blockwise
  kernel, never a typed fold. `working_type` declines them exactly as it
  declines complex, and tests/extended.rs runs the same chain both ways to
  check the fallback agrees. The generic cell machinery carries structure —
  reshape, take, catenate, grade, nub — for free.

  **Limits are named, not discovered.** A power whose result would need more
  than 2²⁶ bits is a domain error rather than an allocation the machine
  cannot serve. `x:` of an infinity is refused rather than passed through as
  J passes it, on the "refuse rather than guess" rule.

  **`x:` on a float.** J converts a non-integral double to a rational near
  it rather than to the exact binary fraction, so `x: 0.1` is `1r10`. libjay
  walks the continued fraction and stops at the first convergent within the
  comparison tolerance, which agrees with the reference on every value with
  a nice rational nearby and picks a smaller denominator than J's on values
  with none (recorded in coverage.md). An integral double converts exactly:
  `x: 1e30` keeps all thirty-one digits the double really holds.

  **The boundary.** Python has unbounded `int` and `fractions.Fraction`, so
  both types cross in both directions with nothing lost. Arrow has neither,
  and inventing a carrier would be a convention no consumer reads: an exact
  result refuses the Arrow export by name and points at `.tolist()` and
  `_1 x:`. Decimal128 stays a separate, later carrier. The C ABI takes the
  same path boxes take.

- 2026-08-20 — Phase 7, GPU/device execution.

  **Backend: wgpu, always compiled in.** One artifact per platform is a
  fixed point, so the device backend is not a cargo feature, not a second
  wheel and not a build-time choice: `wgpu` (Metal, Vulkan, DX12 behind one
  API, stable Rust, MSRV 1.84) is an ordinary dependency, and on a machine
  with no adapter it finds none and everything runs on the CPU as before.
  Shaders are WGSL text generated at RUN time and handed to the driver, so
  the build compiles no shader and knows nothing about the machine that will
  run it — the same hermetic rule the CPU feature levels follow. It lives in
  `device/` behind a `Backend` trait (`info`, `upload`, `dispatch`) so that a
  CUDA backend later is a second implementation rather than a rewrite: the
  code generator and the placement rules above it name no GPU API.
  It costs 4.55 MiB in the release cdylib — 5,912,576 bytes without it
  against 10,688,536 with, measured by building the same tree both ways —
  which is the price of one artifact that runs everywhere.

  **Scope: fused kernels only.** The fusion pass already reduces a chain of
  scalar verbs to a postfix program over blocks with an optional reduction
  absorbed — that IS a kernel description, and it is the only one libjay has.
  `device/codegen.rs` walks it and writes WGSL, exactly as `fuse.rs`'s block
  executor walks it and calls block loops. So the owner invariant holds on
  the device too: there is no hand-written shader per primitive, one arm of
  the generator per scalar operation is the whole of it, and adding a verb to
  the fusable set adds one arm. Everything outside a fused node runs where it
  always ran.

  **Placement is not binding.** `bind` gives a kernel data; `deploy` gives it
  a processor. Both return a new kernel, neither changes the value, the
  shape, the dtype or a diagnostic. In the core it is `Program::run_on`, and
  the device reaches exactly one place in the evaluator — `fuse::eval_on`,
  which offers the kernel to the device and runs the CPU kernel with a
  recorded reason when the device will not take it. A device therefore cannot
  change a result; it can only change where the arithmetic happened, and
  `explain` prints that as `device: gpu` or `device: cpu (reason)` beside the
  kernel's own decline reason. `jay.j(...)` keeps no device by design: there
  is nowhere in one call to say where, and one run rarely pays for an upload.

  **f32 is opt-in, never a fallback.** WGSL can express f64 and naga
  validates it, but almost no adapter implements it: Metal has no `double` at
  all, and on Vulkan `SHADER_F64` is optional. Silently computing an f64
  program in f32 to get it onto a GPU would be trading the user's precision
  for our benchmark, so an adapter without f64 DECLINES the chain to the CPU
  unless the caller passed `precision="f32"`. The other declines are the same
  shape and reported the same way: an i64 working type (WGSL has no 64-bit
  integer arithmetic on most adapters), a result that is not a float array,
  `^` in f64 (the exponential is a 32-bit builtin in both SPIR-V and MSL),
  and anything below half a million elements, where the dispatch and readback
  cost more than the whole CPU pass.

  **Residency rides on the buffer.** `Device::upload` hands back an ordinary
  `Array` whose buffer is foreign and whose owner handle holds the device
  allocation, so an uploaded value carries its own location without becoming
  a different kind of value — the CPU path can still read it, which is what
  makes a fallback transparent. That needed one new accessor,
  `Buf::owner`/`Data::owner`. Known limitation, named rather than hidden:
  `keep_on_device` materialises the host mirror at the same time, so a result
  handed straight to the next kernel still costs one readback. A device-to-
  device hand-off wants an array that can exist with no host pointer at all,
  which is a change to `Array` and belongs to its own phase.

  **Equivalence.** Elementwise `+ - *` in f64 are IEEE on both processors, so
  a map is bit-identical; division and transcendentals may differ by an ulp.
  A reduction regroups an associative fold, which §5.9 already licenses for
  the parallel CPU path, so it is compared with a tolerance: 1e-14 in f64,
  ~1e-4 in f32, and a few parts in ten thousand for a product over half a
  million factors. `tests/device.rs` is the battery and skips cleanly with no
  adapter; CI has none and stays green.

  **The validation gap (for the owner).** The measuring machine is a 2017
  MacBook Pro with a Radeon Pro 560 behind Metal, and Metal has no f64 in
  shaders. So on this machine ONLY the opted-in f32 path has ever executed.
  The f64 path is generated, parsed and type-checked — `naga` validates every
  chain's f64 WGSL under `Capabilities::FLOAT64` in a unit test — but no f64
  shader has run anywhere. It needs a Linux/Vulkan or Windows/DX12 box with
  an adapter that reports `SHADER_F64`. Until then the status matrix says
  🟡 and this entry is the reason.

  Measured at 20M rows against the 8-thread CPU (f32 on the device, f64 on
  the CPU): 1.5x on a weighted sum, 1.6x on the one-sentence standard
  deviation, 4.7x on a polynomial, 5.1x on a sum of exponentials — with the
  data already resident. Uploading 20M elements costs about 120 ms, ten to
  thirty times the kernel, so a device is worth naming only for data that
  stays there. Details in bench/README.md.

- 2026-08-21 — **Coverage wave 4.** Four design choices are worth recording;
  the rest of the wave is ordinary implementation and lives in the status
  matrix and in docs/coverage.md.

  **The obverse is a table, not a search.** `u&.v`, `u&.:v` and every
  negative power `u^:_n` need one answer: what undoes v? libjay answers it
  from a fixed table of the verbs whose inverse is another verb it can
  already write down — the self-inverses (`+` `-` `%` `-.` `|.` `|:`), the
  pairs (`^`/`^.`, `*:`/`%:`, `+:`/`-:`, `>:`/`<:`, `<`/`>`, `#.`/`#:`) and
  the bonded arithmetic (`n&+`/`-&n`, `n&*`/`%&n`, `^&n`, `n&^`, and `n&-`
  and `n&%`, which undo themselves). Everything derived inverts by inverting
  its parts, so the table stays small while `&.` reaches a long way past it:
  `(*:@:>:)^:_1` works because an atop inverts in the other order. The
  alternative — searching numerically for an inverse — would answer
  confidently and sometimes wrongly, so a verb outside the table says "the
  obverse of (+/ % #) is not supported yet" by name. `u :. v` is the escape
  hatch: it declares an obverse where the table has none, and changes
  nothing else about how u applies. `u&.v` is then built as
  `v^:_1 @: (u &: v)`, which is J's own definition and needed no new
  combinator; only `:.` did.

  **Execute is inside the sandbox, not a hole in it.** J's `". y` and APL's
  `⍎ y` compile their argument as a program of the same language and run it
  in the caller's own environment — the names the sentence can see are the
  names the string can see, in both directions, so `". 'a =. 3'` assigns
  where it stands. This does not widen the sandbox: the contract is about
  what a primitive may TOUCH (stdio open, other I/O closed), and evaluating
  an expression touches nothing the caller could not touch already. Two
  consequences fall out and are enforced. A `{name}` interpolation hole
  inside the string has no binding to reach and is refused rather than
  quietly reading the outer program's arguments. And a diagnostic from
  inside is re-pointed at the sentence that ran the string, carrying the
  inner rendering as a note, because the inner spans index a source the
  caller never sees — diagnostics are contract, and a caret into a string
  the user cannot see is not one.

  **A gerund is a parse fragment, not a boxed noun.** J spells a gerund as a
  boxed noun, which lets `` `: `` turn one back into a verb and lets a
  gerund be computed. libjay makes `` ` `` produce a fragment of the parse
  holding the verbs themselves, because `@.` — the only reader a gerund has
  today — wants verbs and not an encoding of them, and because a `Verb` is
  not a value the `Array` types can hold. The cost is named rather than
  hidden: `` `: `` (evoke gerund) stays a gap, and a computed gerund is not
  expressible. Making a gerund a real noun means giving `Array` a way to
  carry a verb, which is a change to the data model and belongs to whichever
  wave wants adverb and conjunction definitions anyway.

  *Superseded 2026-08-21 (Coverage wave 7): this was the mistake. A gerund
  does not need `Array` to carry a `Verb` — it is boxed data, one atomic
  representation (characters) per box. No data-model change. See wave 7,
  below.*

  **Catenate-with-fill is J's rule, and only J's.** J overtakes both sides
  of a ragged catenation to the larger length, so `1 2 3 , i. 2 2` is 3×3;
  APL2 refuses the same shapes with a LENGTH ERROR, and GNU APL confirms it.
  The fill is therefore keyed off the dialect's agreement rule
  (`Agreement::LeadingPrefix`) rather than applied everywhere, which is the
  same axis the two languages already differ on: J fills where APL insists
  on conformability. One `catenate` still serves both; it takes the flag.

- 2026-08-21 — **Coverage wave 5.** Five representation choices are worth
  recording; the rest is ordinary implementation and lives in the status
  matrix and in docs/coverage.md.

  **A mixed simple array is a box of simple scalars.** APL2's `1 'a'` has
  two items, shape `2` and depth 1: a SIMPLE array holding values of
  different types. libjay has no heterogeneous dense buffer and does not
  want one — every dtype in `Data` is a flat buffer of one kind, and that
  is what makes the boundary zero-copy. The representation chosen instead
  is the one APL itself makes unambiguous: a boxed array whose every
  element is a simple SCALAR. In APL enclosing a scalar is the identity
  (`⊂5` is `5`), so no other value can be spelled that way, and the depth
  rule already in place — one more than the deepest content — reports 1
  without a special case. Three places read the shape and act on it: the
  formatter draws such an array like a plain one (no nested spacing), `⊃`
  answers it with itself rather than taking its cells apart, and the
  stranding verb builds one where the types will not promote. No flag, no
  new dtype, no change to the buffers. The cost is that J, whose `<1` IS a
  real box, must never take the same shortcut: the two readings are kept
  apart by asking whether the elements share a type at all, which they do
  in J's `1;2` and do not in APL's `1 'a'`.

  **A memo's cache belongs to the derived verb.** J's `u M.` keeps what u
  has already answered. The cache is an `Arc<Mutex<HashMap<…>>>` held by
  the `Verb::Memo` node, so every clone of the derived verb shares it and
  it lives exactly as long as the compiled program does — not longer (no
  process-wide table to grow without bound) and not shorter (a cache reset
  between applications would be no cache at all). The key is the arguments
  encoded exactly: valence, then shape and dtype and one `u64` per element,
  recursively for a box. Exact integers and rationals have no cheap key, so
  a memo over them simply does not cache rather than risking a wrong hit —
  the answer is the same either way, which is the property that makes a
  memo safe to skip. `is_pure` on a memo is `is_pure` on the verb inside
  it: answering from a cache is only an optimisation for a verb whose
  answer depends on nothing else.

  **`→` runs the body as lines, not as a block.** Everything else in the
  IR is structured: a block's value is its last sentence's, and control
  words nest. APL's branch is a goto, and forcing it into that shape would
  have meant rewriting the body into a state machine. Instead `Flow` gained
  a `Goto(usize)` that leaves every enclosing block the way `break.` does,
  and a definition's body is run by a program counter rather than by
  `run_block`. Labels are not statements: they are recorded on the
  `ExplicitDef` as (name, statement index) and bound as local values —
  their line numbers — when the call's frame is built, which is exactly
  what makes `→(cond)/LABEL` work with no new syntax. The restriction that
  follows is named rather than papered over: a control structure folds
  several lines into one statement, so a label and a control structure in
  one definition is refused, and the reference has no control structures to
  disagree with that.

  **A niladic definition is called by naming it.** `∇Z←H ⋄ Z←42 ⋄ ∇` makes
  `H` a value, not a function, and there is no `⍎`-free way to write "apply
  H to nothing". The definition is stored as an ordinary `ExplicitDef`
  whose right-argument name is `"(no argument)"` — a name no sentence can
  spell, so the body cannot read the argument it never gets — and the
  frontend, when substituting names, turns such a name into a call rather
  than into a function token. Nothing in the evaluator changed.

  **A gerund is still a parse fragment.** Wave 4 recorded the reasoning and
  wave 5 leaves it standing: `` `: `` (evoke gerund) needs a gerund that is
  a real boxed noun, which needs `Array` to be able to carry a verb. The
  three forms were probed against the reference: `` `:0 `` applies each verb
  of the gerund to the argument in turn and frames the answers, which is
  plain enough; `` `:3 `` and `` `:6 `` build trains, and what the reference
  answers for them was not pinned down far enough to implement from. Both
  the data model and that measurement belong to the wave that wants J's
  adverb and conjunction definitions anyway.

  *Superseded 2026-08-21 (Coverage wave 7): no data-model change was needed,
  and the measurement is done — `` `:6 `` is the train, `` `:3 `` is the
  insert. See wave 7, below.*

  **The counting verbs carry exactness.** `$`, `#`, `#.`, `#:`, `p:` and
  `q:` answered with machine integers where J answers with extended ones —
  the values agreed and only the type differed, which was recorded as a
  divergence. It is gone: those six now widen their answer when an argument
  is extended or rational, which is a two-line rule (`carry_exact`) rather
  than a change to how they compute. `3!:0` would now report the same
  number on both sides.
- 2026-08-21 — A composed-expression fuzzer (`jay-corpus fuzz <lang>
  [--compare]`, `crates/libjay-devtools/src/fuzz.rs`) draws trees rather
  than one verb over one noun, and triages each disagreement with an oracle
  into differ / gap / we-refuse / they-refuse by reading the ErrorKind of
  our own refusal. What it found, and what the answers are:
  - J's `$` lays out ITEMS (`$ 3 $ i. 3 4` is `3 4`), APL's `⍴` lays out
    elements. The two rules are one op parameterised by the dialect, as
    `{.` already was; the corpus never caught it because every recorded
    reshape had a vector right argument, where the rules agree. An empty y
    is refused in J and fills in APL, which retires the `2 2⍴⍳0` divergence.
  - Equality is TOTAL: `'a' = 1` is 0 in both references, and `(<1) = 1` is
    0 in J. libjay refused, on a reading of "refuse rather than guess" that
    belongs to input DATA and not to language semantics. Ordering keeps its
    domain. APL's box case is a gap, not an answer, because APL pervades
    into the box instead — which libjay does not do yet.
  - A verb never runs on an empty frame, so its operand types never come up:
    `'a' + ''` and `%: ''` are empties, not type errors. Agreement is still
    checked, so `1 2 3 + ''` stays a length error.
  - The outfix `x u\. y` yields no results at all when x is longer than y,
    and a negative x leaves out non-overlapping runs. Frets are flags: only
    0 and 1, and a scalar one marks every item.
  - `x /: y` is `(/: y) { x`, so the lengths need not agree.
  - APL extends any frame of ONE cell, not only a rank-0 one.
  - J's `*` reads a magnitude below the comparison tolerance as zero
    (`* 1e_15` is 0, `* 6e_14` is 1); APL's `×` is exact there. `Tol::is_j`
    is the seam, since the tolerance is all a scalar verb is told about the
    dialect.
  - An infinite modulus: `_ | y` is y for y ≥ 0 and `_` otherwise.
  - A negative replication count in a VECTOR is legal (APL2, Dyalog) and a
    length error in GNU APL; libjay keeps the general rule and records the
    divergence.
- 2026-08-21 — A nesting ceiling of 400 applications, counted per thread at
  the verb and expression entries and once iteratively over each parsed
  sentence. A string is the interface, so a pathological one has to come
  back as a `limit error`; before this, a few thousand nested applications
  took the host process down with a stack overflow. The iterative
  measurement (`Expr::depth`) exists because the thing being measured is
  precisely what a recursive walk cannot survive.
- 2026-08-21 — A performance pass over the reductions, the cell machinery
  and the data boundary, measured before and after against a build of the
  same commit in the same session (numbers in bench/README.md, section
  "Reductions, cells and the boundary"). Five changes, every one of them
  semantics-preserving; both corpora, every suite and clippy unchanged.
  - `Buf` grew a third shape, `Slice`: a window `[off, off+len)` over a
    refcounted `Vec`. Slicing an owned buffer copied; slicing a foreign one
    never did. Now neither does, so taking a cell or a section out of an
    array is a pointer and a length whichever kind of array it is (`19 }. x`
    over 20M f64: 89 ms to nothing). A window keeps its whole allocation
    alive, exactly as a foreign slice keeps its owner alive, and `to_mut`
    copies a window out before any write, so a `Buf` still behaves as a
    private value. The change is worth nothing for arguments imported
    zero-copy, since those were already views.
  - An associative flat fold keeps eight accumulators in flight rather than
    one (`verb::FOLD_LANES`, `fuse::FOLD_LANES`, both with a vector clone).
    A single accumulator makes the fold a chain of four-cycle dependencies
    and leaves the vector registers unused; eight lanes are eight
    independent chains and a shape the autovectoriser widens. Only
    associative steps take it — the §5.9 regrouping a chunked fold already
    takes — and a run under 64 elements keeps one accumulator and its exact
    old rounding. `+/ x` at 20M on one thread: 23.2 to 7.9 ms. This closes
    the "multiple accumulators" half of phase 6; the width half was the
    earlier SIMD dispatch, and it had already shown that width was not the
    limit.
  - `u/"1 y` (`reduce_vector_cells`) folds every row out of the one buffer
    instead of building an array per cell, reducing it and framing 2.5M
    results back together. Each row folds right to left, the insert's own
    order, so nothing is regrouped and every operation is safe including the
    ones that are not associative. An empty cell, an integer product that
    leaves i64 and a comparison that decides its own result type all fall
    through to the general path. 2.5M x 8 f64: 539 to 9.3 ms on eight
    threads. Pinned by tests/hotpaths.rs against `u/ |: y`, which is the
    same fold along the leading axis.
  - The table boundary's column-major-to-rows-leading weave moved out of the
    binding crate and into the core as `Data::interleave`, because it is an
    array operation and the core is where the thread pool is. Isolated,
    2.5M x 8 f64: 104 to 49 ms on eight threads.
  - The Python front door memoises compiled programs in process, keyed by
    (language, source, index origin), bounded at 512, emptied by
    `jay.clear_cache()`. A `Kernel`'s inner program is a frozen pyclass that
    holds no data and is documented as shareable between threads, so the
    second compilation of a source already seen is the first one handed
    back. In memory only: nothing is written to disk. `jay.j("2+2")`: 7.3 to
    2.1 us.
- 2026-08-21 — Three things the same pass measured and did not do, with the
  numbers that rank them (bench/README.md, "Next"):
  - A **column-major layout flag on `Array`** would remove the boundary
    weave for DataFrame shapes, which is 68 of the 72.5 ms a row-wise
    reduction over a 2.5M x 8 frame costs. Declined here as not containable:
    every verb that indexes a buffer would have to honour the flag or refuse
    it explicitly, and the ones that quietly assume rows-leading are the
    bugs it would introduce. It wants a design, not a hot-path pass.
  - **Absorbing a window verb into the fused kernel** would take about 100
    of the Bollinger kernel's 378 ms. Declined: the kernel's invariant is
    that every input and every slot is read at the same index and has the
    same length, and a window reads `x[i .. i+k]` and yields `n-k+1`
    outputs. It needs a haloed block load, a block prefix/suffix stage and a
    carry between blocks — a change to the invariant rather than an addition
    to it.
  - **Widening inside the parallel pass** (a complex-plus-float pass widens
    the float side into a whole buffer and reads it back) is worth about 4
    of 13.7 ms at 2M elements, and needs every scalar chunk function to be
    told where its slice of the source begins — a signature change through
    the shared offset-and-divider agreement machinery. The widening itself
    is at least parallel now (`par::map` in `borrow_f64`/`borrow_cx`), and
    the sequential negative-argument check that `%:` needs became one
    branch-free parallel pass (`par::any`).
- 2026-08-21 — APL lineage (owner): v0.1.0 implements one APL — the APL2/ISO
  line that GNU APL embodies and that our oracle can verify (`↑` first, `⊃`
  disclose, floating nested model, one-index-per-axis `⌷`, rising-flag `⊂`,
  no trains). Dyalog (and other well-known dialects) are a planned future
  choice made the same way `⎕IO` is made today: a preinitialised `Dialect`
  object passed at compile time (e.g. `APL.Dialect.gnu` / `APL.Dialect.dyalog`),
  never global state and never a guess from the source text. Until then:
  every place the two lines diverge is to be implemented behind the
  dialect (a named knob or an `Agreement`-style enum), not hard-wired, and
  the divergence is to be listed in docs/coverage.md's "Which APL" section
  so the generalisation is a matter of filling in the second column. The
  Dyalog-only features already present (`⊆ ⍛ f⍤g ⌸ ⍤`) stay available in
  the GNU dialect as extensions with no oracle, marked as such.
- 2026-08-21 — The dialect became a settings object, with no change to what
  any program answers. `Dialect` now carries `index_origin` (`⎕IO`),
  `comparison_tolerance` (`⎕CT`), and one setting per point where the APL
  lines diverge: `nested_model` (floating / grounded), `first_disclose`
  (`↑` first and `⊃` disclose, or `↑` mix and `⊃` first), `index_form`
  (`⌷` over one scalar per axis, or over index vectors), `dfn_result` (the
  last sentence, or the first non-assignment one), `default_arg` (whether
  `⍺←v` evaluates `v` when the left argument arrived), `complex_order`
  (a grade by real-then-imaginary or by magnitude-then-angle) and `trains`.
  `Dialect::gnu_apl()` writes every one of them out and equals
  `Dialect::default()`, which tests/dialect.rs pins; `Dialect::j()` is the
  empty one. `Dialect::rules(lang)` is the single place a choice is made:
  it resolves the settings into `Rules` — carried by `Program` and by every
  `EvalCfg`, so the engine reads the same dialect the parser did — and
  refuses a reading libjay does not implement as a gap ("not supported
  yet"), by name, rather than answering with this line's meaning. Three
  rules turned out to be hard-wired past the dialect and now read it: `⌸`
  answered with positions from 1 whenever the language was APL, a `:For`
  header parsed its source at origin 1 whatever `⎕IO` said, and `⍎`
  compiled its nested program with the index origin alone. Python's
  `APL.Dialect` grew the same fields with the same defaults and an
  `APL.Dialect.gnu` preset; the C ABI keeps `index_origin` and gains
  nothing, the stable surface staying stable.
- 2026-08-21 — v0.1.0 released from tag `1f2fa37`: GitHub release with
  wheels/sdist/C bundles as assets, PyPI via trusted publishing (five abi3
  wheels + sdist, cold-verified by the publish job and again by hand with
  `uvx libjay`), crates.io via a one-time manual `cargo publish` with the
  owner's token (the bootstrap crates.io requires); the token was then
  removed locally and `CRATES_PUBLISH=true` set, so from 0.1.1 the crate
  publishes through OIDC in the same run as PyPI once the owner registers
  the trusted publisher on crates.io.
- 2026-08-21 — Post-release CI hardening, first-release hygiene pass. The
  releasing skill folded in what 0.1.0 taught: the tag may already be at
  HEAD with green CI (skip creation rather than fail), the crates.io
  bootstrap is now steady state (OIDC, `CRATES_PUBLISH=true`; the trusted
  publisher registration is a checklist line until confirmed), release notes
  are `--notes-file docs/release-notes-X.md` with the CHANGELOG section
  landing in the bump commit, and post-publish verification grew the checks
  that proved useful (release asset count, crates.io `max_version`, docs.rs
  200 with its ~30-minute lag, a cold `uvx --refresh`). CI itself gained:
  a weekly `schedule` cron on `publish.yml` that runs the full wheel/C-ABI
  matrix dry (the existing `workflow_dispatch` path, publishing nothing —
  the `publish`/`crates` jobs stay gated on `github.event_name == 'release'`
  and additionally on `vars.CRATES_PUBLISH`, both untouched); a `docs` job in
  `ci.yml` running `cargo doc -p libjay --no-deps` with
  `RUSTDOCFLAGS=-D warnings`, catching a docs.rs-breaking link before the
  push instead of after; `cargo publish -p libjay --dry-run` added to the
  `rust` job, cheap insurance against packaging drift; and one Python matrix
  leg (3.10) that builds the sdist with `uv build --sdist` and installs from
  it, since that path was previously only checked by hand. Dependabot
  (`.github/dependabot.yml`) watches cargo, pip and github-actions weekly,
  each grouped into one PR. README gained a three-badge line (PyPI version,
  crates.io version, CI status) under the title.
- 2026-08-21 — APL trains and function assignment ship as an EXTENSION, on
  by default, under the `trains` dialect setting that already existed and
  read false. The rule the wave settles: a feature the oracle merely LACKS
  is not a reason to withhold it, while a feature the oracle ANSWERS
  DIFFERENTLY still binds — `⍺←`, complex ordering and the vector
  replication count stay as they were, and `⊆ ∘ ⍥ ⍛ f⍤g ⌸` were already
  shipped on this reasoning. So `Dialect::gnu_apl()` (and therefore
  `Dialect::default()`, which tests/dialect.rs pins equal to it) now sets
  `trains: true`, and `Dialect::rules` no longer refuses the setting: both
  readings are implemented, which makes `trains` the one field on the
  object that is a CHOICE rather than a queue position. Trains and function
  assignment are one flag because they are one question — may a function
  stand where a value belongs — and splitting them would let a host ask for
  half a language. The rules are Dyalog's: 2-train atop, 3-train fork, a
  literal value as the left tine, longer runs grouped from the right, `⊢`
  and `⊣` as identity tines. They lower to the existing `Verb::Atop`,
  `Verb::Fork` and `Verb::NounFork` — the same engine J's forks use, so no
  new semantics reach the runtime, and a computed left tine is a named gap
  on both sides for the same reason. Implementation note: parentheses now
  close inside `fold_operators` rather than in a pass after it, because the
  operator to the right of `)` has to see the train as one function; that
  also fixed `(+)/1 2 3`, which GNU APL answers 6 and libjay had been
  refusing, and `1 0 1(/)1 2 3`, where the reference reads the parentheses
  around a bare operator glyph as transparent. No oracle covers a train, so
  the shapes are hand-tested in tests/wave6.rs and the extension is pinned
  against GNU APL's SYNTAX ERROR in corpus/apl/divergences.txt — if a later
  GNU APL grows trains, the recorded pair stops disagreeing and `record`
  says so.
- 2026-08-21 — J names adverbs and conjunctions: `m =. /`, `c =. @`. A
  modifier is applied while the sentence holding it is PARSED, so a named
  one has to be resolved then, exactly as a named verb is; `Names` grew a
  second table beside `verbs`, and a name can still change part of speech
  in either direction. `Expr::ModDef` is the resulting sentence — silent,
  like `Expr::VerbDef`, and carrying only the spelling so that `explain`
  can show it. What stays a named gap is WRITING a new modifier (`1 :`,
  `2 :`) and the tacit translator (`13 :`); so does displaying a bare
  modifier name, which now says so by name rather than as a syntax error.
- 2026-08-21 — `L:` and `S:` gained their dyads and `H.` landed. The dyadic
  level descends both arguments together and holds a side that has reached
  its level while the other descends, which is what makes `1 ,L:0 (3;4)`
  reach every leaf; two sides that both still have boxes must agree in
  shape. `m H. n` sums the generalised hypergeometric series from the ratio
  between neighbouring terms, with the parameters the two lists SHARE
  cancelled first — that cancellation is what makes `0 H. 0` the
  exponential rather than a term of `0÷0`, and the oracle confirms it.
  Wholly real arguments are summed in real arithmetic so that a zero
  denominator parameter gives the infinity J answers with rather than a
  complex NaN. A series that neither converges nor overflows within 2¹⁶
  terms is refused by name; the reference loops forever on the same
  sentence, which is one place a cap is better than fidelity.
- 2026-08-21 — J's explicit modifiers (`1 : '…'`, `2 : '…'`, their `: 0`
  bodies, and the `{{ }}` that names an operand), and the TWO PHASES the
  oracle confirms they have. A body that mentions `x` or `y` is the body of
  the derived VERB and runs when that verb is applied; a body that mentions
  neither runs when the modifier meets its OPERANDS, and its value — a
  tacit verb for `1 : 'u @ u'`, a noun for `1 : '3 + 4'` — is what the
  derivation produced. The part of speech of a `{{ }}` is read off the
  operand names its body uses: `v` or `n` makes a conjunction, `u` or `m`
  an adverb, neither a verb; a `{{)a` / `{{)c` / `{{)v` / `{{)d` / `{{)m`
  marker states it instead, and the reference takes a marker only where
  nothing else stands on its line.
  Both phases are done at PARSE time, by substituting the operand
  fragments into a copy of the body's words and parsing the result — which
  is J's own substitution rule, and the only way the derivation phase can
  produce a verb at all, since the IR has no verb-valued expression. It
  also keeps the frontend's one model of a modifier: `Names` already
  resolved a modifier name while the sentence holding it was parsed, and
  `Frag::Adverb`/`Frag::Conj` now carry a `Modifier` that is either a
  primitive spelling or an explicit body, so a written modifier composes
  with rank, trains, assignment and other modifiers with no new rule.
  A runtime `Verb::DerivedExplicit` was the alternative, and APL's
  `Verb::UserDerived` shows it works for verb operands; it cannot give the
  derivation phase, and would need noun-valued and verb-valued operand
  names in the evaluator's environment for no gain. The price is that a
  body which derives its OWN modifier — J's spelling of a recursive
  modifier — would parse for ever; libjay detects the re-entry and names it
  as a gap. `13 :`, the tacit translator, stays a named gap.
- 2026-08-21 — The input half of the sandbox, and the sandbox as an error
  class of its own. `Ctx` gained `inp` beside `out`, so a run has two
  halves of stdio and neither is a global; `Program::run_io` is the
  spelling that attaches one, and `Program::run` stays what it was — a run
  with NO input source, which is a different thing from a source that has
  ended and says so differently ("this run has no input source attached"
  against "the input has ended"). That distinction is why `inp` is an
  `Option<&mut dyn FnMut() -> Option<String>>` rather than a bare closure
  that yields None: one closure cannot tell a wiring mistake in the
  embedding from a program asking for one line more than it was given, and
  both diagnostics are owed to different people.
- 2026-08-21 — `ErrorKind::Sandbox`, labelled "closed by the sandbox". A
  feature libjay closes is not a property of J or of APL, so `Language`
  ("absent from the language itself") was the wrong class for it, and
  `NotYet` would be a promise nobody intends to keep. Three classes now
  answer three different questions: is it in the language, is it in libjay
  yet, and does the host let it out. `⎕TS`/`⎕AI`/`⎕FIO` and their
  relatives, `T.`, and the file, script, host, clock and shared-library
  foreigns all moved onto it; their messages dropped the words the label
  now carries and say instead what the feature would have reached.
- 2026-08-21 — J's `!:` as a dispatcher over the two literal numbers, not a
  general foreign mechanism. `1!:1 ]1` reads a line, `x 1!:2 ]2` writes one
  (the oracle: the left argument formatted as it displays, plus a newline,
  and the value is the left argument), and `3!:0` is J's type code — cheap,
  pure, and the thing a test reaches for when it wants to name a type. A
  stream number that is not the open one, and a boxed file NAME, are the
  same refusal, because J numbers its open files alongside its streams.
  Everything else divides at compile time into closed (0, 1, 2, 6, 15) and
  not yet (by number), which is the division `ErrorKind::Sandbox` exists to
  draw. Refusing a file foreign at COMPILE time rather than when it runs
  follows `T.`: the diagnostic points at the source, which is worth more
  than being catchable by `try.`.
- 2026-08-21 — The C ABI grew `jay_run_io` rather than a parameter on
  `jay_run`: the old signature is the published contract and stays
  byte-for-byte what it was, and "no input source" is exactly what it
  should mean. `jay_read_fn` takes a caller-owned buffer and returns the
  snprintf count, so there is no allocation ownership to document and no
  silent truncation of a long line — a return above the capacity means
  libjay grows its buffer and asks again. NULL for `read` is the process's
  own stdin, mirroring NULL for `write` being its stdout.
- 2026-08-21 — MSRV raised 1.85 → 1.89 and pinned in the repository: a
  `rust-toolchain.toml` names 1.89, the workspace `rust-version` says 1.89,
  and the CI/publish jobs pin the same number, so the compiler that reports
  a clippy finding is the compiler every contributor has. What forced it is
  AVX-512: `avx512f` and its neighbours became stable `target_feature`
  names in 1.89, and the "no v4 rung" note from the SIMD entry above is
  spent. Raising the floor rather than feature-gating the rung keeps the
  one-artifact rule — every x86-64 build carries the same set of clones.
- 2026-08-21 — SIMD gained the x86-64-v4 rung: `Level::V4`
  (avx512f+bw+cd+dq+vl), `LIBJAY_CPU_LEVEL=v4`, and one more
  `multiversion` clone of every annotated loop, x86-64 only — no other
  architecture has a rung above v3, and `detected` cannot return V4 there.
  Detection and clamping are the rules the ladder already had: a machine
  without the features reports v3 and a pinned v4 clamps to it, so `v4` is
  either a level that runs or a level that quietly is not one. What is NOT
  claimed is a measurement. No machine on hand has AVX-512, so the clone is
  proven only to exist — `nm` finds the `_avx512…` symbols in the built
  artifact — and bench/README.md says built-but-unmeasured rather than
  quoting a number. tests/simd.rs covers v4 through `available()`, which
  includes it exactly where it can run, and prints which case the machine
  is; CI runs that test with `--nocapture` so a runner with AVX-512 reports
  itself and compares the clone for real.
- 2026-08-21 — Dependencies bumped for 0.2.0, by hand on main rather than by
  merging Dependabot PR #1, which the bumps supersede: wgpu and naga 26 → 30, pollster 0.4 → 1.0, rstest 0.25 → 0.26,
  pyo3 0.25 → 0.29. wgpu 30 moved the device backend in four places: the
  instance descriptor is built by `new_without_display_handle_from_env`
  (libjay never presents), `enumerate_adapters` became a future,
  `RequestAdapterOptions` gained `apply_limit_buckets` — left false,
  because bucketing rounds the reported limits down to hide the adapter and
  libjay wants the real ones — and `push_error_scope` now returns a guard
  that is popped rather than a device method. The one that reached beyond
  the backend: a mapped buffer is written through a write-only reference
  now (mapped memory can be write-combining, where a spurious read is not
  free), so `codegen::write_bytes` became `codegen::byte_iter` and the
  upload feeds `WriteOnly::write_iter` — still no intermediate `Vec`, which
  was the point of the original.
- 2026-08-21 — A snapshot record became a MAP keyed by implementation:
  `= EXPR` then a `> IMPL: TEXT` group per implementation (`j`, `gnu`,
  `dyalog`, and whatever a later one is called — an unknown key is read and
  rewritten rather than dropped). Every existing snapshot was migrated
  mechanically, J's answers to `j` and APL's to `gnu`, with the answer text
  untouched; a full live re-recording of both languages then reproduced the
  migrated files byte for byte. The replay holds libjay to the key its
  dialect FOLLOWS (`j`, and `gnu` while `Dialect::default()` is the
  APL2/ISO line), so nothing about the battery's semantics changed. What
  the map buys is the second APL: one file can now say what GNU APL and
  Dyalog each answered to the same sentence, and a future Dyalog dialect
  reads the same file by switching the key rather than by recording
  everything again.
- 2026-08-21 — Dyalog is an oracle, and its recordings are BACKLOG rather
  than a gate. `record apl --impl dyalog` writes the `dyalog:` key and no
  other; the replay counts how many recorded Dyalog answers differ from
  libjay and prints the count, `stats apl --dialect-diff` lists them, and
  `--check --impl dyalog` reports agreement and fails on nothing, while
  `--check` on GNU APL fails on drift exactly as before. That asymmetry is
  the point: libjay implements one APL, and measuring the distance to the
  other one is not the same act as defending the one it implements.
  `corpus/apl/dyalog-probe.txt` is 71 expressions aimed at the "Which APL"
  table — `↑`/`⊃`, `⌷`, partitioned enclose, `⎕CT`, replicate, `⍸`, `⍤`,
  the printers, `⎕IO←0` — every one of which libjay and GNU APL agree on,
  so the first Dyalog run quantifies the gap without the file being a
  divergence file. The rows where libjay already differs from GNU APL live
  in divergences.txt and carry the same column.
- 2026-08-21 — The Dyalog runner was written blind, against the published
  documentation, on a machine with no Dyalog: `mapl -script FILE`, the
  sentence bracketed by two printed markers under pinned `⎕PW ⎕PP ⎕ML ⎕IO`
  and ended by `⎕OFF`, and only what is printed between the markers is
  kept — so a banner is dropped without being parsed and a missing closing
  marker IS the error signal, an abandoned script. Every assumption is
  listed at the top of `crates/libjay-devtools/src/dyalog.rs`, a `verify`
  step runs `2+2` before any recording and reports which assumption broke,
  and `LIBJAY_ORACLE_DYALOG_FLAGS` / `LIBJAY_ORACLE_DYALOG_STDIN` correct
  the two likeliest mistakes without a rebuild. An interpreter that is not
  installed is a skip; one that is installed and misbehaves is a failure,
  because those are different facts.
- 2026-08-21 — **Coverage wave 7.** A gerund is a boxed noun now, which is
  the representation change waves 4 and 5 kept deferring, and the rest of
  this wave falls out of it or out of one more careful oracle pass.

  **A gerund IS data: one box per atomic representation.** J does not have
  a parse-time gerund object at all. `` u`v `` is a boxed noun whose items
  are ATOMIC REPRESENTATIONS: a primitive is its own spelling as a
  character vector, a noun is the pair `('0'; <value)`, a train is
  `('2'; <parts)` or `('3'; <parts)`, and anything a modifier derived is
  `(spelling; <operands)`. `` +`- `` is two boxes holding `+` and `-`;
  `` +/`- `` is the box tree for `('/'; <,<'+')` beside `-`. The reference
  was probed for every one of those shapes before a line was written, and
  the display, `$`, `#`, `L.` and `3!:0` of each now match it.

  Waves 4 and 5 said this needed "`Array` able to carry a `Verb`". It does
  not, and that was the mistake: the representation is CHARACTERS, so
  nothing about the data model changes — no new `Data` variant, no new
  dtype, no change to the buffers, and the whole feature lives in one new
  module (`gerund.rs`) plus the frontend that reads it. Everything else
  follows for free: a gerund can be named (`` g =. +`- ``), catenated onto
  (`` g`* ``), written by hand (`('+';'-')@.1`), displayed, and tied with a
  noun on either side, because tying is nothing but catenating.

  Two costs are named rather than hidden. The representation is
  reconstructed from the verb tree, not kept from the source, so the
  spellings that differ only by the rank they set are recovered by matching
  that rank (`u@v` is `u@:v` at v's ranks; `u&v` and `u&.v` likewise) — and
  the two that leave no trace at all are refused rather than guessed at: a
  capped fork, which is an atop by the time the tree has it, and any verb
  libjay has no J spelling for. The second cost is that a gerund computed
  at RUN time still cannot be read, because `@.` and `` `: `` want their
  verbs while the sentence is parsed; a name holding a literal is looked up
  in the parser's own table, which covers the idiom that matters.

  **`` `:6 `` is the train and `` `:3 `` is the insert, not the other way
  round.** Both were guessed at in wave 5 and both guesses were wrong. The
  reference settles it: `` (+`-)`:6 `` DISPLAYS as `+ -`, and
  `` ((+`%`#)`:3) 1 2 3 `` is `1 + (2 % 3)` — the verbs are laid into the
  gaps between the items left to right, cycling, and folded right to left
  as insert does. `` `:0 `` applies every verb and frames the answers, and
  every other number is a domain error there too.

  **`t.` is not the Taylor series any more, and `t:`, `..` and `.:` are not
  J.** The status matrix carried four reds from the published vocabulary
  that the reference no longer honours. `^ t. 3` in J 9.6 answers with a
  BOX and ignores its right operand: `t.` is the TASK conjunction now, it
  runs a verb in one of J's thread pools and gives back a pyx. That is
  `T.`'s situation exactly, so it takes `T.`'s answer — closed by the
  sandbox, which is libjay's policy and not a queue position. `t:`, `..`
  and `.:` are rejected outright as invalid inflections, like `d.`, `D.`
  and `D:` before them, so they are `—` rather than 🔴: there is nothing
  there to promise. Four reds gone by measurement rather than by work,
  which is what an oracle is for.

  **A negative block size needs the movement written out.** `x u;.3 y` with
  a negative size reverses that axis, and with the movement row given
  (`(1 ,: _2) <;.3 i.5`) the reference agrees with the obvious reading
  exactly. Given a BARE vector of sizes it does something else: `_2`, `_3`,
  `_5` and `_8` all answer identically, the magnitude playing no part —
  every reversed prefix of the argument. libjay implements the form that is
  well defined and names the other rather than pinning a degenerate answer
  into the corpus.

  **One axis-permutation primitive serves both dyadic transposes.** J's
  `x |: y` moves the axes x names to the END, in that order, with the rest
  keeping their order in front; APL's `x ⍉ y` says, for each axis of y in
  turn, which axis of the RESULT it becomes. Both are a map from source
  axes to destination axes where several sources may share a destination —
  which is the diagonal, taken as short as the shortest of them. So
  `transpose_to` takes that map and the two frontends compute it; the
  diagonal, `(<0 1)|:` in J and `1 1⍉` in APL, needed no separate code, and
  J's refusal of a repeated axis in the unboxed form is the frontend's rule
  rather than the primitive's.

  **`⊥` is an inner product and `⍸` closes its interval on the left.** Two
  oracle corrections that had been recorded as gaps. APL's decode folds the
  LEADING axis of its right argument against the LAST axis of its left, so
  it is `+.×` and its answer is shaped `(¯1↓⍴x),(1↓⍴y)`; that retires the
  two pinned `⊥` divergences. And `1 3 5⍸3` is 2 where `1 3 5 I. 3` is 1 —
  APL counts a bound EQUAL to the value, J does not — so the shared
  operation carries a flag rather than the two languages sharing a bug.

  **`u b. _1` answers with a spelling, not a verb.** `+ b. _1` displays as
  `+`, which looks like the obverse until it is assigned: `obv =. + b. _1`
  and then `obv 5` is a syntax error, because what came back is the
  character vector `,'+'`. `u b. 1` is the same — `+ b. 1` is the nine
  characters `0 $~ }.@$` — so both are rendered from the obverse table and
  the identity-element table libjay already had, and neither needs a verb
  to survive a round trip through data.

  **`n&+` is undone from the right.** Probing `b. _1` found a real bug in
  the obverse table underneath it: `(2&+)^:_1 5` answered `_3` where the
  reference answers 3, because the inverse of `2 + y` was built as `2 - y`
  rather than `y - 2`. Adding and multiplying are commutative, so the noun
  can be bonded to either side, but the INVERSE always takes it off the
  right. `&.`, `⍢`, `^:_1` and `b. _1` all read the same table, so one line
  fixed four spellings.
- 2026-08-21 — **An array carries its layout, and a table crosses as its
  columns.** The item bench/README.md's "Next" list had at the top, taken as
  a design rather than a hot-path pass: 68 of the 72.5 ms a row-wise
  reduction over a 2.5M x 8 DataFrame cost were the weave at the boundary,
  and the weave existed only because every path in the runtime assumed
  rows-leading. Numbers in bench/README.md, section "Layout".
  - **The shape stays logical; a flag says how it indexes the buffer.**
    `Array` gained a private `layout: Layout` — `RowMajor` or `ColMajor`,
    two cases, no general strided arrays — and `shape` is unchanged: rows
    leading, `[M, N]` for an M-row table, whatever the buffer does. Rank 0
    and rank 1 have one possible layout and always carry `RowMajor`.
    `ColMajor` means the FIRST axis varies fastest, which at rank 2 is
    "each column is contiguous" and at any rank is "the full transpose of
    the row-major order" — one definition, so `|:` is exactly the flag.
    The field is private and `Array::new` still means row-major, so every
    construction site in the tree kept working and the ones that produce
    the other layout had to say so.
  - **Every consumer is one of three things, and the compiler and the
    debug build keep them apart.** A verb that reads a column-major buffer
    natively (the folds, the elementwise passes, the shape verbs); a verb
    that is indifferent because it reads every element at its own index and
    writes it back there (which is why `scalar_monad`, `Array::cast` and
    the fused block kernel simply propagate the flag); or a verb that is
    handed `Array::to_row_major()`, materialised once. The seam is
    `Verb::monad`/`Verb::dyad`: a column-major argument reaches
    `monad_columns` (five arms, each with an argument for why the order
    cannot reach the answer) or `to_row_major` and nothing else.
    `Array::cells`, `cell_at` and `atom` debug-assert the layout, so a verb
    that quietly indexed the buffer fails in the test suite rather than
    answering nonsense, and tests/layout.rs runs the whole primitive table
    over both layouts and compares.
  - **The columns are joined lazily, never eagerly.** `Buf` gained a fourth
    shape, `Cols`: several buffers end to end, each still borrowing its own
    Arrow memory, with the join made only if some reader asks for the flat
    slice — once, behind a `OnceLock` shared by every clone, and on the
    thread pool for the plain element types. A reader that can work column
    by column takes `Buf::parts` and the join is never made at all;
    `Buf::slice` inside one part is that part's own slice, so taking a
    column out of a table is free. This is what makes the import zero-copy
    rather than one-copy-per-column: the alternative, concatenating the
    columns into one owned block at the boundary, was rejected because the
    copy it costs is exactly the copy this pass exists to remove.
  - **What reads columns natively.** `u/ y` folds each column where it lies
    (a flat fold per column when a column is long enough to split, the run
    fold across columns when it is not); `u/"1 y` folds the rows in one
    pass that reads the columns side by side, right to left, which is the
    insert's own order and regroups nothing; every scalar monad and every
    scalar dyad over a scalar or over an argument of the same shape and
    layout keeps the flag; `$`, `#` and `|:` answer from the shape.
    Everything else — ravel, take, drop, indexing, grade, catenation,
    windows, scans, boxing, formatting, the C ABI, the Python conversions,
    a device upload — takes the rows, materialised by the same parallel
    weave that used to run at the boundary. The cost did not go up; it
    moved to the verbs that need it, and most programs over a table need
    none of them.
  - **`|:` moves no elements.** Reversing every axis is reading the same
    buffer in the other layout, so the transpose is a reversed shape, the
    same buffer and a flipped flag — at any rank, in both directions, in
    both languages. `+/"1 |: {df}` over 2.5M x 8 went from 480 to 76 ms.
  - **numpy Fortran order is accepted rather than refused.** A block that
    is contiguous in the other order is still contiguous, and now that the
    runtime carries that order there is no reason to ask for `.copy()`.
    `a.T` of a C-contiguous array, and `np.asfortranarray(a)`, cross
    borrowed; a view contiguous in neither order (strided, sub-block, a
    partial axis permutation) is refused exactly as before. Two tests that
    pinned the old refusal now pin the new reading.
  - **What stays a copy, deliberately.** A fused chain over a table reads
    one flat block, so it makes the join — the same 160 MB the boundary
    used to weave, now paid only by chains and now paid in parallel; giving
    the block kernel a part-by-part loader is a change to its invariant and
    is not worth it for one memcpy. A rank-2 result still leaves through
    `.tolist()` rather than as an Arrow table, so the reverse zero-copy
    path (a column-major result exported as one Arrow buffer per column)
    is written down here and not built: it wants a decision about column
    NAMES, which is an open question of its own.
- 2026-08-21 — **A moving window is a stage of the fused kernel, and the
  shapes decide its alignment.** The item bench/README.md's "Next" list had
  at the top: a window reads `x[i..i+k]` and yields `n-k+1` outputs, against
  a kernel whose invariant was same index, same length. The Bollinger
  z-score — two moving sums, a drop and eight elementwise steps — was ten
  passes over the column because every `20 +/\` broke the chain it stood in.
  It is now two kernels and a square root: 393 → 206 ms on eight threads
  over 20M rows, and a moving range 194 → 86. Numbers in bench/README.md,
  section "Windows in the kernel".
  - **Two instructions, not a new kind of node.** The postfix program the
    pass emits gained `Window(op, k)`, which folds every window of k items
    of the value on the stack into one item, and `Scan(op)`, which replaces
    it with its running fold. Both are ordinary steps: they take the top of
    the stack and leave one value there, they are counted in the ops a
    chain must have to be worth fusing, they can be a let, they can sit
    under an absorbed reduction, and the device declines them by the same
    door it declines anything it has no shader for.
  - **A chain with a window reads two axes.** Everything under a window
    step stands on the wide axis, everything beside it on the axis the
    result stands on, which is `k - 1` items shorter. Which axis a leaf is
    read on is settled where the chain is built, so nothing has to be
    inferred at run time and no index is ever shifted inside the kernel: an
    input arrives as the items it holds.
  - **The alignment rule is a shape rule and nothing else.** The inputs on
    one axis must agree with each other; the two axes must stand `k - 1`
    items apart. `19 }. y` beside `20 +/\ y` is a chain because it is
    nineteen items shorter; `18 }. y` beside it is not, and the sentence
    runs and raises the length error it was always going to raise. The
    alternative — recognising `(k-1) }. x` paired with `k f/\ x` as a
    pattern and aligning the two — was rejected: it decides from spelling
    what the lengths already decide, and it would have to be extended for
    every other way of writing the same drop.
  - **Halo, not stitching.** A block computes the items of the wide axis
    its own windows need — its own items, plus about three window lengths
    around them — and folds them where they lie. Carrying a neighbouring
    block's prefix instead would make the blocks a chain, and the halo is
    two per cent of a block at the window lengths a time series uses (the
    kernel declines a window past 1,024 items, where the halo would start
    to be most of the work). Blocks stay independent, so the pass still
    splits across threads with nothing shared.
  - **The windows are grouped exactly as they were.** The wide axis is cut
    into runs of `k` counted from the axis's own start, and a window is one
    run's suffix joined to the next run's prefix — the two-pass algorithm
    `verb.rs` already used, called from the kernel through
    `verb::windows_into` rather than written a second time. Because the cut
    is counted from the axis and not from the block, a window is folded
    from the same items in the same order however the blocks fell, and a
    fused window is bit-identical to an unfused one. The property that
    matters is the one that algorithm was chosen for: the float error of a
    window is the error of that window alone, and no accumulator runs
    longer than `k` steps.
  - **A running fold runs on one thread.** Its accumulator is handed from
    one block to the next, in the order the unfused scan takes them and
    rounding where that rounds, so a kernel holding one does not split. The
    unfused scan does not split either; what the kernel saves is the
    traffic around the scan, not the scan. It is not absorbed under a
    reduction, whose blocks run backwards.
  - **A window verb is now a value the pass will move.** `s =. 20 +/\ y`
    used to be a whole array however often it was read, because no kernel
    could take it; now every use lands in one and the pass inlines it,
    which folds the moving sum once per kernel that reads it rather than
    once for the program. For Bollinger that is three window folds in place
    of two. Measured both ways, paired, in one session: on eight threads it
    is the better trade by 11% on Bollinger (570 → 505 ms) and by 28% on the
    moving range (321 → 230), because what it removes is a 160 MB write and
    the reads of it; on one thread it costs Bollinger 6% (1142 → 1219),
    which is the one core paying for the fold it does twice. The rule is
    left as it is — the same rule for every named value, and the machine
    that matters has more than one core.
  - **What declines.** A second window length in one chain, a window
    inside a window (the outer one is the stage, the inner one is the pass
    it was), a window longer than the axis, a left argument that is not one
    integer the compiler knows, an argument that is not a vector, an input
    read on both axes that is not a scalar, and everything the kernel
    declined before. Each falls back to the chain it came from, which is
    the rule the fusion pass has always had.

- 2026-08-21 — **Ordering whole arrays: two orderings, derived from the two
  oracles, and a third named as a gap.** Grading a boxed argument was the
  last named gap either grade had, and the wave-5 note that described it
  ("type, then element count, then rank, then contents") was wrong in two
  places; what follows is what jconsole and GNU APL actually answer, probed
  pair by pair before a line of it was written.
  - **J's total array ordering** compares, in this order: the TYPE CLASS —
    numeric, then symbol (which libjay has not), then character, then boxed
    (`/: 1;'a';(1 2);(<<3)` is `0 2 1 3`, so a two-atom numeric list still
    precedes a character atom); the RANK, ascending, which beats the atom
    count (`/: (1 1$1);(1 2)` is `1 0`); the SHAPE, read with the LAST axis
    most significant (`/: (2 3$0);(3 2$0)` is `1 0`, and `/: (1 4$0);(2
    3$0)` is `1 0` although four atoms is fewer than six — so it is not a
    count at all); then the ATOMS in row-major order, a boxed atom by the
    same rules one level down. Element count is not a step, and rank comes
    before shape, not after: both corrections to the old note.
  - **An empty array has no atoms to take a class from**, and J treats it
    as the lowest class whatever its type: `/: (<''),(<<1)` puts the empty
    character list first, `/: (<0$'a'),(<i.0)` ties two empties of
    different types, and `/: (i.0);1` still puts the numeric SCALAR first
    because rank 0 precedes rank 1. Read as "an empty array is numeric",
    the rule is a total order; every other reading of those three answers
    is a cycle.
  - **GNU APL does not refuse a nested grade** — the premise this work
    started from. `⍋(1 2)(3 4)` is `1 2` there, and the APL2 comparator is
    a different one from J's at every step: the RANK first (a nested scalar
    precedes a simple vector), then the SHAPE read from the FIRST axis,
    then the ATOMS with a character before a number before a nested value —
    the opposite of J's class order — and two arrays with no atoms are
    separated by their types, which J ties. The oracle wins, so libjay
    implements that, and the entry pinning `⍋(1 2)(3 4)` as a divergence is
    gone.
  - **Dyalog's total array ordering is a third comparator**, and it stays a
    named gap: `Dialect.nested_grade` has an `Apl2` arm (the default, what
    GNU APL answers) and a `TotalOrder` arm that `Dialect::rules` refuses
    by name, as every other lineage setting does. The published rule
    compares the atoms first, pads the shorter array with an item below
    every type, extends a lower rank with leading 1s and puts numbers
    before characters — and it does not say where an enclosed item sorts
    against a simple one, which is the one thing a NESTED grade must know.
    Guessing it under a dialect name nobody can verify would ship a wrong
    answer wearing Dyalog's label; the refusal is honest, and a recorded
    Dyalog would lift it in an afternoon.
  - **Both comparators are exact**, and that is a deliberate divergence on
    the APL side: GNU APL's grade reads `⎕CT`, so `⍋2 (1+1E¯14) 1` ties the
    near pair there and separates it here (now pinned in
    divergences.txt). Tolerant equality is not transitive, a sort whose
    comparator is not a total order may refuse to run, and J — which is
    exact — agrees with libjay. A NaN still ties with everything, as it did
    before.
  - One comparator function serves both languages and every caller of a
    grade (`/:` `\:` monadic and dyadic, `⍋` `⍒`, the sort idioms, `A.`),
    so there is one place where the ordering is written down. The verbs
    that compare boxes by MATCH — `~.`, `e.`, `i.` — were left alone: they
    are tolerant, as both references have them, and match is not order.
- 2026-08-21 — **A pass over two element types promotes where it reads,
  not before it runs.** The item bench/README.md's "Next" list had at the
  top. `{c} + {f}` widened the float argument into a complex buffer of its
  own and then read it back; at 20M that is 320 MB written into freshly
  faulted pages, which the bandwidth probes in the same file already showed
  costs more than the arithmetic and does not parallelise. The narrow
  operand is now read in its own type and promoted element by element.
  `{c} + {f}` at 20M went 148.6 → 58.8 ms on eight threads, `{i} + {f}`
  110.5 → 50.8, `+/ {i} * {f}` 72.8 → 13.3; the same-type passes do not
  move. Numbers in bench/README.md, section "Mixed-type passes".
  - **A `Widen<T>` element trait, not a widened buffer.** `zip_chunk` — the
    shared offset-and-divider machinery every scalar pass runs on — now
    carries an element type per side rather than one for both, and the
    three typed chunk bodies (`dyad_i64_chunk`, `dyad_f64_chunk`,
    `dyad_cx_chunk`) are generic over `A: Widen<T>, B: Widen<T>` and
    promote in the step. They stay multiversioned: the macro already took
    generic parameters, so each combination is compiled per CPU feature
    level like every other leaf. The comparisons take the same adapter.
  - **The dispatch is a macro over the buffer's own variant**, one arm per
    fixed-width numeric type, and the exact types (`Ext`, `Rat`) keep the
    widened copy — a bignum has no fixed-width buffer to read element by
    element. The complex pass only takes the typed path when one side is
    already complex; two real arguments meeting in the complex plane (`j.`,
    a power that leaves the reals) widen both, which is what such a pass is
    for and holds the monomorphisation count to seven instead of sixteen.
    Release build time went from 2m39 to 3m20 for the whole workspace, most
    of it the complex leaf.
  - **The fused kernel stages a narrow argument at the block.** It used to
    call `to_f64` on every input, which allocated and filled a whole f64
    copy of an i64 or boolean argument — the one large buffer a fused
    reduction otherwise never touches. `Loaded` grew a `base` field naming
    the index its first element stands at, so a block-local buffer can
    stand in for a whole one, and each thread promotes the block it is
    about to read into a staging buffer it reuses. That is why
    `+/ {i} * {f}` at 20M now runs at the same 13 ms as the all-float
    `+/ {w} * {x}`, which is memory bandwidth.
  - **Bit-identical, by construction and by test.** Promoting an element
    and then operating is the same arithmetic on the same values as
    promoting the buffer and then operating; nothing is reordered and no
    step changes type. `hotpaths.rs` asserts it directly — every mixed pass
    against the same program handed the widened argument, bit for bit, NaN
    included, for both operand orders, a scalar operand, a fused chain and
    a window.
  - **The kernel's acceptance rate did not move, and should not have.**
    The type rule already promoted an integer LEAF into a float kernel:
    `working_type` marks an integer *step* and not a `Load`, because the
    unfused chain widens a leaf exactly once too, where an i64 step past 53
    bits is exact in i64 and not in f64. The fuzz battery runs 72 of its
    295 fused chains through the kernel before and after; of the 223
    declines, 212 are that rule and every one of them has an integer-typed
    step in it. `tests/fuse.rs` now prints the rate, so a change to the
    type rules shows up as a number.
  - **Left undone, deliberately:** the single-buffer folds. `+/ {b}` still
    widens a boolean buffer to i64 whole before folding it, because the
    fold family (`fold_items`, `fold_range`, `fold_flat`, `window_fold`,
    the scans) threads one element type through five functions where the
    dyadic passes thread it through two. It is the top entry under "Next"
    in bench/README.md, with its number: 63.2 ms at 20M.
- 2026-08-21 — v0.2.0 released from tag `f794936`: GitHub release (10 assets),
  PyPI (five abi3 wheels + sdist, cold-verified with `uvx`), and crates.io
  through OIDC trusted publishing for the first time — the `crates` job
  failed once ("no Trusted Publishing config") until the owner registered
  the publisher on crates.io, then succeeded on a `--failed` rerun with no
  rebuild. From here every release reaches all three targets unattended.
