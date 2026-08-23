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
- 2026-08-21 — The inner product, one engine for both languages. `u . v`
  and `f.g` are one IR node (`Verb::InnerProduct { u, v, apl }`) whose
  dyad pairs x's last axis with y's first and whose monad is J's
  determinant. Three choices behind it, all settled against the oracles:
  - **The shape rule is shared and the operand rule is not.** Probing
    jconsole settled J's: x is taken in cells at v's dyadic LEFT rank, or
    at rank 1 where that is smaller (`*` and `*"0` give a matrix product,
    `,` and `,:` read the whole of x), the WHOLE of y meets each cell, and
    u is applied MONADICALLY to the result — so `+/` folds because it is a
    fold, not because the conjunction makes one, and `(i.2 3) <. . * i.3 2`
    is a floor of a `2 3 2` table. GNU APL's `f.g` instead takes one vector
    from each side. The two agree for every scalar g, which is every
    published use, so the flag chooses the route and not the answer.
  - **APL's inner step runs under J's agreement rule.** APL conformability
    — equal shapes, or one side a scalar — describes a whole application,
    and the pairing an inner product asks for (row element against column
    element) is the leading-axis one J spells out. Running `∧.=` by the
    fast J route therefore needs that rule for the one dyad inside, and it
    is set and put back around the application. The alternative was a
    row-and-column loop for every scalar g, which is two small allocations
    per output cell; the loop is still there and runs for a non-scalar g,
    where the two languages genuinely differ.
  - **`+/ . *` is a kernel, everything else is the cell machinery.** Real
    machine numbers go through a blocked pass over the two buffers —
    blocked on the shared axis so a slice of y is reused across a block of
    output rows, split by whole rows (`par::fill_rows`), and multiversioned
    like every other hot loop; no hand-written SIMD, as the owner invariant
    requires. Integers keep their type, with one magnitude bound computed
    up front deciding whether the vectorising loop can overflow at all and
    a checked loop behind it. Measured at 1000×1000 f64: 55 ms against
    numpy/OpenBLAS's 20 ms, so about 2.5× off a tuned BLAS; at 1000×1000
    i64, 89 ms against numpy's 2268 ms, because BLAS has no integer path
    and numpy falls back to its own loop. Layout is not the kernel's
    business: a column-major block is materialised by the same rule every
    other verb uses.
  - **The determinant follows the published recursion, not a formula.**
    The oracle settled the expansion as being down the FIRST column, with
    v's identity element where the columns run out and u over nothing where
    the rows do — which is what makes `-/ . * 2 1 $ 5 6` be `5-6` and
    `-/ . * 0 0 $ 0` be 1. The recursion is memoised on the set of rows
    still in play, so it costs `2^n` rather than `n!`, and past 16 rows it
    names the limit. `-/ . *` over machine numbers goes by elimination
    instead, which is what jconsole does from three rows up (its answer
    there is a float even for whole input); the exact types keep the
    recursion and stay exact.
- 2026-08-21 — APL's `⍠` is a one-call override of a dialect knob, settled
  at compile time. GNU APL rejects the glyph, so there is no oracle and the
  published Dyalog shape is the whole specification; libjay implements the
  two options that correspond to settings it already has. `CT` — the
  principal option, so a bare number sets it — becomes `Verb::Fit`, which
  is the same mechanism J spells `!.`. `IO` derives the verb AGAIN with the
  other origin (`verb::with_origin` rewrites the origin every primitive
  carries), because the index origin is resolved into the primitives when
  the program is compiled: a variant is not an argument the verb reads at
  run time, it is the dialect answering differently for one application.
  That is also why an option must be a literal: `f⍠B` with a name or an
  expression is a computed variant, which would mean carrying the dialect
  into the runtime, and it is named rather than guessed at. A setting the
  verb does not consult (`+⍠0`, `⍋⍠0.5`) is not one of its options and says
  so; `⌶` stays out for a different reason — see below.
- 2026-08-21 — The sequential machine (dyadic `;:`) is implemented to the
  published table-driven form and no further. The dictionary gives the
  boxed argument `f ; s ; m ; ijrd`, a `p q 2` transition table, output
  codes 0 to 6 and result forms 0 to 5; every one of those was probed
  against jconsole and matches, including two things the page does not
  spell out — that the end of the input with `d` of `_1` ends the word in
  hand without a transition (so the trace has no row for it), and that the
  table position such a word reports is the one class 0 would have given in
  the state reached. What is NOT implemented is codes 4 and 5, which emit a
  VECTOR rather than a word: the reference coalesces successive vector
  emissions into one span, and nothing published says what a machine that
  mixes them with word emissions should answer. Guessing at it would put a
  wrong answer behind a right-looking spelling, so the code is named. A map
  over a numeric argument is named for the same reason: jconsole answers
  `(0;s;0 1) ;: 0 0 1 0 1 1` with a length error, so whatever it wants
  there is not "index the map by the value".
- 2026-08-21 — `⌶` (I-beam) moves from 🔴 to ⚪, and APL's `&` (spawn) with
  it. A 🔴 is a promise libjay has made; an I-beam has no published
  contract to promise — what it does is each interpreter's own business —
  so keeping it red claimed a queue position for work that can never be
  specified. `&` joins J's `T.` and `t.` under the sandbox rule. With
  those moved and this wave's four landed, nothing in APL's primitive
  tables is red and J's red is exactly `s:` and `$.`, which are storage
  kinds — an interned string and a sparse layout — rather than primitives:
  the 0.3 type work, named as such.
- 2026-08-21 — Supply-chain gate after the arrayref attack (RUSTSEC-2026-0260;
  the repo was verified unaffected: none of arrayref/internment/
  append-only-vec/proc-macro1 in any lockfile revision or the cargo cache):
  cargo-deny with the RustSec advisory DB, yanked = deny, explicit bans of
  the three poisoned versions and the proc-macro1 typosquat, a license
  allowlist, and unknown-registry/git denial; enforced locally (deny.toml)
  and in CI on every push.

- 2026-08-22 — The three measured losses in bench/workloads.md were one
  shape of mistake in three places: a general path recomputing what it
  already had. All three are algorithm changes inside verbs that were
  already correct, and the answers are unchanged. At the workload sizes,
  one thread: RSI(14) over 20M bars from n²/2 steps (about nine years) to
  2330 ms, VWAP over 13,889 days from rows × groups (hours) to 1438 ms,
  the frame-RMS reshape of 16M samples from 457 ms to nothing measurable
  and its whole row from 535 ms to 49.
  - **`u/\.` carries an accumulator, whatever the verb is.** Right to left
    is the insert's OWN order, so suffix k is item k folded with suffix
    k+1 — one step per item, for any dyad, in exactly the operations and
    the order the per-suffix fold took (bit for bit, floats included).
    Prefixes have no such relation: prefix k and prefix k+1 share their
    tail, not their head, so `u/\` stays a fold per prefix except where
    the step associates and the existing running scan already covers it.
    That asymmetry is the whole reason J spells an exponential smoothing
    `|. u/\. |. y` — the reversals put the recurrence in the direction the
    insert can carry. The suffix scan is O(n) for a boxed or character
    verb too; only an impure verb keeps the old path, so that the number
    of applications a side effect sees does not change.
  - **An affine step is recognised as the recurrence it is.** The rule is
    deliberately narrow and matches the TREE, not the meaning: a fork
    `[ + c * ]` or its mirror `(c * ]) + [`, where `+` and `*` are the
    primitives at scalar rank, `[` and `]` are the primitives at infinite
    rank, and `c` is a rank-0 numeric noun written in the source (a noun
    fork, which is what the parser makes of a constant in a train). Then
    `u/\.` is `acc = y[k] + c*acc` over the buffer and `u/\` is the same
    series carried forward as `p[k] = p[k-1] + c^k * y[k]`. Anything else
    — a name, a vector `c`, a bond, an atop, a dfn that computes the same
    number — falls through to the general path, which is now linear
    backwards anyway. Two limits, both deliberate: the fast path is taken
    only where the answer is float or complex (two integers fold exactly
    and are left alone), and the FORWARD direction declines outright once
    a power of `c` leaves the finite range, because that is the one case
    where the carried power and the fold it stands for disagree about
    where the overflow happens.
  - **The forward direction is a documented reassociation.** `p[k]` as a
    running sum of `c^i * y[i]` is not the Horner fold from the prefix's
    own tail, so its float rounding is not the insert's. That is the same
    licence the blocked window fold takes (§5.9) and it is recorded in
    docs/coverage.md beside it. Backwards nothing moves at all, which is
    what the tests assert bit for bit — and backwards is where every
    recurrence an analyst writes actually lives.
  - **The key hashes its keys.** `x u/. y` swept the whole key vector once
    per group, so 20M bars over 13,889 days was rows × groups. It now
    hashes each item's elements once (the content key `nub` already had)
    into a first-occurrence bucket list, and the tolerant cases — float or
    complex keys under a non-zero comparison tolerance, boxed or exact
    items — keep the comparison loop, because tolerant equality is not an
    equivalence a hash can stand in for. APL's `f⌸` shares the code and
    the fix. The hasher is a multiply-and-rotate mix rather than the
    default SipHash: the keys are already spread over a `u64`, and nothing
    here is exposed to a chosen key.
  - **A reshape that keeps the elements shares the buffer.** `x $ y` walked
    the ravel element by element even when the result asked for no more
    elements than the argument has, where the mapping is the identity;
    below that bound it is now a refcount bump on the same buffer, foreign
    (Arrow, numpy) as well as owned. A shape that has to go round the
    ravel again still copies, because it must. `,` already shared.
- 2026-08-22 — One-shot AWS spot runs, designed and not executed
  (bench/cloud/). Three claims in this repository cannot be checked on the
  machine that made them: the x86-64-v4 clones are built and symbol-checked
  but have never run, the GPU f64 shader path has never executed anywhere
  (Metal has no `double`), and there are no ARM numbers at all — the
  linux-aarch64 wheel publish.yml cross-compiles is marked `smoke: false`
  because the runner that builds it cannot run it. A rented machine answers
  all three; the design question was how to rent one from an unattended
  agent without either the credentials or the bill becoming a problem.

  **Two identities, and the launching one is nearly powerless.** A dedicated
  IAM user whose key lives only in the owner's `~/.aws` may launch spot
  instances of four types in one region from Amazon-published AMIs into one
  security group, terminate what it tagged, read the region's EC2
  inventory, and read and write three prefixes of one bucket. It may not
  launch on demand (an explicit `Deny` keyed on `ec2:InstanceMarketType`,
  which also fires when the key is absent), may not launch through Fleet,
  Spot Fleet, Auto Scaling, Batch, ECS, EKS, Lambda or SageMaker (each
  explicitly denied, since each would sidestep the `RunInstances`
  conditions), and may not create or modify any IAM identity. The instance
  is the second identity, a role that can write one S3 prefix and one log
  group and has no EC2 permission whatever — not even to terminate itself,
  because it does not need one.

  **What bounds a thief is a quota, not a policy.** IAM has no condition key
  for how many instances may exist, so the answer is the account's spot vCPU
  quotas, lowered as an owner setup step to 16 Standard and 4 G. With those,
  and with AWS never billing spot above the on-demand rate, the worst
  simultaneous burn is about \$1.72/hour. That is the honest number; the
  Budget alerts are what keep the exposure to hours rather than a month.
  Named rather than hidden: `ec2:Describe*` cannot be resource-scoped, so
  the region's inventory leaks; and the security group's egress is
  80/443/53/123 to anywhere, because the bootstrap needs PyPI, apt and two
  tarballs, so mining over 443 is possible and it is the quota that stops
  it. Mirroring the inputs into the bucket would let egress drop to the S3
  endpoint and close that; it is logged as an open question, not done.

  **The cost bound is arithmetic, not a promise.** A spot price cap times a
  lifetime three independent timers enforce, plus the volume, is a number:
  no run can cost more than \$2.33, and the default profiles are \$0.47 to
  \$1.21. The instance is launched with
  `instance-initiated-shutdown-behavior=terminate` and the launcher reads
  that attribute back and terminates immediately if EC2 did not take it —
  the whole story rests on that one attribute, so it is verified rather than
  assumed. There is no TTL sweeper, because a sweeper is infrastructure this
  design owns none of; "cannot cost more than \$2.33" is the stronger
  statement anyway.

  **The owner rejected a per-launch confirmation, so fourteen checks replace
  it**, each a refusal rather than a warning: placeholders present, caller
  is not the dedicated user, another tagged instance is alive (concurrency
  one), month-to-date spend over a guard read two ways, a lifetime over the
  ceiling, a market price over the cap, a `run-instances --dry-run` that the
  policy would refuse, and a ledger written to S3 before the launch. The
  spend guard is deliberately two readings because neither is enough: the
  Budget's actual covers the whole account but refreshes a few times a day,
  and the ledger is exact but only knows launches this script made. Cost
  Explorer was considered and rejected — \$0.01 a call, its own permission,
  and no fresher than the Budget.

  **Prebuild: the CI wheel, relayed through S3 — with one correction.** The
  measuring half needs no compiler: manylinux wheels exist for both linux
  architectures, as do polars', numpy's and numba's, so a released tag
  installs from PyPI and an unreleased tree takes publish.yml's dry-run
  artifact, which the LOCAL orchestrator downloads under the owner's `gh`
  login and uploads to the run prefix — the instance holds no GitHub
  credential and reads S3 only. But the VALIDATING half is Rust: the AVX-512
  equivalence battery is `tests/simd.rs` and the GPU f64 battery is
  `tests/device.rs`, neither reachable from a wheel, so those two profiles
  install the pinned toolchain as well. The wheel produces the numbers and
  the toolchain produces the verdicts. A custom AMI was rejected (minutes
  saved against a permanent lifecycle burden at one run a week) and
  cross-compiling from the Mac was rejected (CI already builds exactly these
  wheels, on the pinned compiler, from a clean checkout). Inputs are pinned
  by sha256 in the user-data, which is the one thing a stolen key cannot
  rewrite, so swapping a staged wheel before boot buys nothing.

  **Observability without a way in.** No SSH, no key pair, no ingress, no
  SSM in v1. The instance syncs its whole log to S3 every thirty seconds
  along with a one-word status and a file per phase, and mirrors the log to
  CloudWatch through a small `PutLogEvents` pump — S3 primary because it
  needs nothing the upload does not already need, CloudWatch secondary
  because the console is convenient. The CloudWatch agent was considered and
  not used: an install that can fail before any channel exists, to cover
  ground three lines of shell already cover. Phases upload as they finish,
  so a run that dies in its fourth still delivers three.
- 2026-08-22 — **A fold over one buffer promotes where it reads, as a pass
  over two already did.** The entry bench/README.md's "Next" list had at the
  top, and the half the mixed-type wave left undone: `+/ {b}` widened a 20 MB
  boolean buffer into 160 MB of `i64` and read that back to fold it, so the
  cheapest argument in the library was the most expensive one to reduce.
  Removing the copy took `+/ {b}` at 20M from 65.9 to 2.8 ms on eight threads
  and 101.0 to 8.3 on one, and `>./ {b}` from 65.2 to 1.7 — a boolean
  reduction is now faster than the same reduction over floats, which is what
  reading an eighth of the bytes should buy. Numbers in bench/README.md,
  section "Folds over one buffer".
  - **One more type parameter, not one more family.** The fold family reads
    `&[S]` and accumulates `T` where `S: Widen<T>` — the trait the mixed-type
    wave already introduced — so `Data::Bool` reaches the same leaf as
    `Data::I64` with `u8` in the slice and `i64` in the accumulator. Eleven
    functions carry the pair (`fold_range`, `fold_lanes`, `fold_flat`,
    `fold_items`, `fold_runs`, `fold_columns`, `fold_across`, `scan_flat`,
    `window_fold`, `window_fold_flat`, `window_fold_range`), and
    `par::try_fold_chunks` carries it too, because a chunked fold reads one
    type and combines another. The multiversioned leaves stay
    multiversioned: the macro already took generic parameters, so each
    combination is compiled per CPU feature level like every other leaf.
  - **Every buffer copy in the family goes, not just the boolean one.** The
    integer scan and the integer window retry in floats when a step
    overflows; the retry used to build an `f64` copy of the `i64` argument
    and now rereads the argument with an `f64` accumulator. The column-major
    table folds (`fold_columns`, `fold_across`) widened a boolean table
    column by column and no longer widen at all. What is left widened is
    what has no fixed-width buffer to read element by element: `Ext` and
    `Rat` fold through the general path, one step at a time, as before.
  - **Bit-identical, and that is the test.** Bool to `i64` is exact and
    nothing is reordered, so promoting each element and then folding is the
    same arithmetic on the same values as promoting the buffer first.
    `hotpaths.rs` asserts it directly — every fold, scan, window and row
    fold over a boolean argument against the same program over the integer
    array, whole values, across the sizes that straddle the parallel and the
    lane thresholds and the shapes that straddle the vectorised range fold.
    The one answer that legitimately differs in type is the reduction of a
    single item, which is that item and runs no insert at all; the test
    promotes it and compares values.
  - **The scans and the windows gain less, and the reason is the output.**
    `+/\ {b}` at 20M went 182.0 → 83.2 ms and `20 +/\ {b}` 140.5 → 50.1,
    two to three times rather than twenty, because a scan still writes one
    `i64` per element: the 160 MB the fold no longer reads is 160 MB the
    scan still has to produce. `+/\ {b}` now costs what `+/\ {x}` costs,
    which is that write. The remaining entry under "Next" — a reuse pool for
    the fresh output buffer — is the one that would move them again.
  - **The same-type folds did not move**, which is the control: identity
    promotion is `#[inline(always)]` and compiles away. `+/ {x}`, `+/ {i}`,
    `+/\ {x}` and `20 +/\ {i}` at 2M and 20M, on one thread and on eight,
    all repeat within the few per cent this file quotes as the noise floor.

- 2026-08-22 — Symbols (J `s:`) are a storage kind: `Data::Symbol(Buf<u32>)`
  over a PROCESS-WIDE append-only intern table, not a per-array one.

  **Why global.** The oracle settles it: `(s: <'x') = (s: <'x')` is 1 across
  sentences, and the two symbols an expression makes from the same text
  compare equal to symbols any other expression made. A per-array table
  would make equality a comparison of strings and would have to re-key on
  every catenation; a global one makes an array of symbols an array of small
  integers, so it copies, slices, reshapes and indexes on the same paths a
  boolean array does, and `=` is a `u32` comparison. The cost is that the
  table never shrinks — the same bargain J's own symbol table makes, and
  bounded by the distinct names a program actually interns.

  **The index is opaque.** J sorts symbols by their TEXT, not by the order
  they were interned in (`/:~` of the symbols made from "zzz" then "aaa"
  puts "aaa" first), so the u32 is an identity and nothing more: every
  ordering comparison and every grade resolves the name behind it. Equality,
  membership, nub and index-of still run on the raw index, which is where
  the interning pays.

  **The table.** `RwLock<Vec<Arc<str>>>` plus a `HashMap` for the reverse
  lookup, behind `symbol::intern`/`name`/`names`/`cmp`. Index 0 is the empty
  name, which makes it the fill element as well — overtaking an array of
  symbols needs no lookup at all. A lock-free append-only structure was
  considered and rejected for now: interning happens once per name at `s:`,
  and the read lock a comparison takes is uncontended in every measured
  shape. `symbol::names` resolves a whole slice under one lock, which is
  what display and grade use.

  **The corrections the oracle made.** Monadic `s:` on a character LIST is
  not "split on blanks": the first character is the DELIMITER, so `s: 'abc'`
  is the single name "bc" and `s: 'a b'` is the single name " b". A
  character TABLE is different again — one name per row, trailing blanks
  trimmed — and a BOXED argument takes each box's characters verbatim,
  trailing blank and all. Symbols also have an ORDER, unlike characters,
  so `<`, `<:`, `>`, `>:`, `<.` and `>.` all work on them where J refuses
  them on characters. `3!:0` reports 65536, and J's total array ordering
  puts the symbol class between the numbers and the characters — the class
  table in `verb.rs` had already reserved the slot.

  **What is left named.** `0 s:` … `3 s:`, `6 s:`, `7 s:` and `_1 s:` report
  on the interpreter's own symbol table: how many slots it holds, which are
  in use, the raw indices, the hash table. Those are an implementation's
  internals rather than the language, and libjay names them rather than
  inventing numbers that would describe a different table. `4 s:` (the names
  as a padded character table) and `5 s:` (the names as boxes) are
  implemented; they are the ones that ask about the DATA.

  **The boundaries.** Python gets the names as `str`; there is no carrier
  going in, so a Python `str` stays a character array and `s:` inside the
  expression is how one becomes a symbol. Arrow has no symbol type and the C
  ABI has no descriptor for a table index, so both refuse by name, pointing
  at `5 s:`. Fusion and the GPU decline through the existing
  `DType::is_numeric` gate.

  **One thing widened on the way.** J's dyadic `I.` searches an ordered list,
  and characters have an order it was already willing to use; libjay had
  made it numeric-only. It now searches character and symbol lists too,
  which is what makes `(/:~ symbols) I. symbol` the sorted lookup it is in J.

- 2026-08-22 — Sparse arrays (J `$.`) are a property of the ARRAY, not a
  variant of `Data`.

  **The shape of the thing.** A sparse array has no per-element storage, so
  it does not fit `Data`, which every buffer operation — `len`, `slice`,
  `push_from`, `cast`, `interleave` — assumes is one element per position.
  Adding a variant there would have meant a wrong answer in nine match arms
  per operation across 23 000 lines. Instead `Array` grows one field,
  `sparse: Option<Arc<Sparse>>`, and the meaning of the two it already had
  shifts under it: `shape` stays the LOGICAL shape and `data` holds the
  stored cells end to end. A sparse array of doubles therefore reports the
  same dtype a dense one does and formats its values through the same code,
  and cloning it stays a refcount bump. `Sparse` itself carries the sparse
  axes, the index rows, the sparse element and the entry count — the count
  because it is not derivable when there are no sparse axes at all, or when
  a dense axis has length zero.

  **Partial axes are in the representation from the start.** J's model is
  that SOME axes are sparse and the rest are dense, so a stored entry is a
  cell rather than an element. `$. y` only ever makes the all-axes-sparse
  case, and `1 $. shape ; axes` is the only way to ask for the other one —
  which builds an array with nothing stored in it. Modelling it anyway cost
  about thirty lines (a cell size, an offset plan, a scatter instead of a
  store) and keeps `4 $.`, `5 $.` and `0 $.` honest about their shapes,
  which the oracle checks: `$ 5 $. 1 $. (2 3 4) ; 1` is `0 2 4`.

  **Every other verb gets the dense expansion.** The alternative — teaching
  each primitive to propagate sparseness — is a rewrite of `verb.rs`, and
  the payoff is speed rather than meaning. So `Verb::monad` and `Verb::dyad`
  expand a sparse argument at the top unless the verb is one of the six that
  read the stored form: `$.` itself, `$`, `#`, `":`, `echo` and `3!:0`.
  Expansion is exact, so every ANSWER matches J; what differs is the storage
  kind of the result. J keeps `s + 1` sparse with a sparse element of 1,
  where libjay hands back the dense array — the one visible divergence, and
  the reason the row in status.md is 🟡 rather than 🟢. Those expressions
  are deliberately absent from the corpus; the corpus covers `$.` itself,
  the display, and the verbs whose answers do agree.

  **The corrections the oracle made.** `0 $. y` is not "make dense" — it
  converts to whichever storage kind the argument is not in, so `0 $. dense`
  is `$. dense`. `1 $. shape` with no element given makes a sparse array of
  DOUBLES (type 8192), not of integers. `2 $.` answers a DENSE argument with
  all of its axes rather than refusing it, while `3 4 5 7 8 _1 $.` all
  refuse one. `8 $.` drops the stored entries that hold the sparse element,
  which is the only way an array acquires any: amending a stored position
  back to the fill leaves the entry behind. A SCALAR passes through `$.`
  unchanged whatever its type — `$. 'a'` is `'a'` and `$. 1r2` is `1r2` —
  because there is no axis to store it along, so the type check has to come
  after the rank check. `$.` on a sparse array is that array. `x` must be an
  atom: `(0 1) $. y` is a rank error.

  **What is left named.** A sparse array of characters or of boxes: J has a
  type code for each and refuses to make either, so both are `NotYet` here
  too. The exact types have no sparse form at all and are a domain error in
  both. `1 $. shape` refuses a shape past `limits::MAX_ELEMENTS` even though
  building it allocates nothing, because every other verb would expand it —
  J, which propagates sparseness, holds much larger ones. Python, Arrow and
  the C ABI have no sparse carrier, so a sparse result crosses as the array
  it stands for.

  **One thing the corpus could not restate.** `3!:0` of a sparse boolean is
  1024, but only when the argument really is boolean: jconsole narrows a 0/1
  LITERAL to boolean storage where libjay keeps integers — an older
  divergence recorded in tests/input.rs. The boolean cases in the sparse
  corpus therefore compute their booleans (`1 0 1 0 = 1`) instead of writing
  them down.

## 2026-08-22 — Dyalog oracle: the Docker image, and the first recording

Dyalog 20.0 runs as the third oracle, from the official `dyalog/dyalog`
Docker image (amd64+arm64; free for non-commercial use, accepted by use) —
which sidesteps 20.0 having dropped Intel macOS. The interpreter runs in
piped session mode (`-b -q`): `-script` and `dyalogscript` do not display
bare expression values, the session does. A wrapper at
`~/projects/libjay-oracles/dyalog/dyalog-docker.sh` (quarantine dir, never
the repository) feeds the script on stdin and filters stderr, keeping real
errors: the session echoes input there six-spaces-indented, and the image
prints `Rebuilding user command cache... done` there on every start — the
first full recording read that line as a refusal and recorded 1325 errors
of 1377 before the filter caught it. The recorder's assumptions in
`dyalog.rs` held otherwise.

First clean recording, whole APL corpus, 1363 expressions: libjay agrees
with Dyalog on 1224, differs on 139 — the measured backlog of the future
dialect, gating nothing. The recording confirms all three places libjay
follows the published rule over GNU APL's own behaviour (`⍺←` as a
default records 8, `¯1 2/1 2` records 0 2 2, complex ordering refused):
each was a prediction until today. status.md gained "APL — the Dyalog
line": the queue of Dyalog features libjay does not ship (`⊇`, dfn
error-guards `::`, `f⍣¯n`, namespaces, the utility quads), with two
diagnostics gaps recorded — `⊇` parses as "unknown symbol" and `::` as a
bare syntax error where the contract wants the named promise.

## 2026-08-22 — Corpus coverage: measured, not assumed

The corpus is a sample of a combinatoric space — verb × valence × operand
type × operand rank × modifier — and nothing said which part of the space
it sampled. `jay-corpus coverage <j|apl>` measures it.

**Classified by evaluation, not by reading the text.** Each expression is
compiled by libjay, the fusion pass is undone (`jay::fuse::unfused`, which
leaves the tree the frontend built), and every operand subtree is RUN — as
a program made of the sentences before it plus that subtree — so the class
recorded is the class the primitive actually met. A lexical classifier
would have had to call every computed operand "derived", which is most of
them; running one costs a second over both corpora, and the numbers mean
something. The price is that libjay is the instrument: a sentence libjay
refuses is one the measurement cannot see into, and the count of those is
printed (3 of 4665 in J, 1 of 1363 in APL).

**Attribution is conservative on purpose.** A site is attributed only to
primitives that provably receive the site's own arguments: through forks
(both tines), `@:`, commute, fit, memo, obverse, adverse, and a power of
at least one. Two modifiers carve a piece whose shape is still computable
and are followed as such — a reduction hands its verb ITEMS, and `u"r`
hands it r-cells — and those attributions are marked `on cells` rather
than direct. Everything else (scan, each, key, cut, under, hooks, bonds)
hands its operand something this measurement cannot name, so the site is
counted as unattributable and appears in the operator table alone: 2344 of
9224 sites in J, 205 of 2785 in APL. Claiming those cells would have been
guessing.

**The taxonomy.** 14 type-classes (bool, int, int-big, i64-edge, float,
float-tol, float-inf, complex, extended, rational, char, box, box-nested,
symbol) and 10 rank-classes (scalar, and vector/matrix/rank3+ each in a
plain, a singleton-axis and an empty form), plus `refused` and `unknown`
for what could not be run. There is no `mixed`: libjay's arrays are
homogeneous and a heterogeneous value in either language is a box. An
empty array of a type is the cross product of the two axes rather than a
class of its own.

**The denominator is the corpus's own reach**, not the full cross product.
A cell counts as reachable when SOME primitive in the corpus is applied to
it; J builds 53 distinct monadic operand classes and 169 dyadic pairs, APL
30 and 125. The full product (140 monadic, 19 600 dyadic per valence) would
have made every row look empty and said nothing about what the corpus could
have covered. `docs/status.md` supplies the second denominator — the
published vocabulary, one row per spelling — read by the tool, so a
spelling the corpus never mentions is still counted. Parsing a document
means the tool reports how many rows it read, and a format drift shows up
as a number rather than as silence.

**Blind spots, stated in the report rather than hidden.** Spellings the
frontend rewrites into another form carry no node of their own — J's `&.`,
`&.:`, `f.`, `!:`, `$.` — and are listed as not visible rather than as
unexercised. The IR merges families that differ only by valence or by
context (`@`/`@:`, `/`/`∘.`), so an operator row can answer for more than
one spelling and says which. A definition body is walked once per place it
is applied, and its operands are `unknown` because nothing can be run
there.

**What the first measurement said.** J: 68 primitives with an attributed
monadic application and 64 with a dyadic one; `i.` is applied 1671 times
and meets three of the 53 operand classes; `i64-edge` is reached by 2
primitives, `int-big` by 3, `bool` by 4. APL: five type-classes — extended,
rational, symbol, i64-edge, float-inf — are reached by NO primitive, and
`rank3+-empty` by none either. 13 409 empty cells in J and 7381 in APL,
written out by `--tsv` for a generation stage to consume.

## 2026-08-22 — APL applies its operators between items, not between cells

`¯1 0 1∘.⌽⊂2 3⍴⍳6` answered three unrotated copies of the matrix. The
outer product was J's `x u/ y` — the table over the cells the operand's
rank asks for — and `⌽`'s left rank is 0, so the cell on the right was the
whole enclosure, one item, and rotating one item changes nothing. GNU APL
answers the three rotations of the matrix, and Dyalog agrees.

The rule the oracle is holding to is not about rotation and not about
boxes. APL applies a function between the ITEMS of its arguments: what the
function is handed is the CONTENTS of an item, and a value that is not a
simple scalar is enclosed again to take one place in the array being
built. `¨` is the operator named after the rule, but `∘.f`, `f/`, `f⌿`,
`f\`, `f⍀` and `f.g` all obey it. J reads every one of them by cells and
leaves its boxes shut. So this is a difference between the two LANGUAGES,
resolved on `cfg.rules.lang` the way `↑`, `↓` and `≡` already are, and not
a dialect setting: Dyalog reads the items exactly as GNU APL does.

Probing the family turned up more than the reported case, all of it
confirmed against GNU APL first and Dyalog second:

- The items are ELEMENTS whatever the operand's rank. `1 2∘.,3 4` is a
  two-by-two table of pairs and `'ab'∘.,'cd'` is `ac ad` / `bc bd`;
  libjay made a single catenation of each, because `,` takes its
  arguments whole and the rank machinery gave it both whole.
- The insert folds elements too, and encloses the fold. `,/1 2 3` is an
  enclosed vector, not a bare one; `,/2 3⍴⍳6` is two enclosed rows and
  `,⌿2 3⍴⍳6` is three enclosed columns, where libjay catenated whole
  cells. `⍴/2 3⍴⍳6` shows the same thing without a catenation in sight.
- The scan is the insert over each prefix, so it follows with no work of
  its own: `,\1 2 3` is `1`, `1 2`, `1 2 3`.
- The inner product is `f/¨ (⊂[last]x) ∘.g (⊂[first]y)`, and the each in
  that definition is load-bearing: `1 2,.+3 4` is an enclosed `4 6`, one
  level deeper than the fold alone would leave it. `+.×` and every other
  fold that ends in a number is unaffected, which is why the matrix
  product's blocked fast path needed no change.

What did NOT move: the arithmetic reductions. Folding elements and folding
cells agree for a scalar function, so the typed fold and the buffer fold
above the general path keep every `+/`, `×/` and `⌈/` on the fast route
and answer exactly as before. The corpus proves it — 1224 recorded APL
expressions replay unchanged.

One consequence needed a decision of its own. An APL array may hold a
simple scalar beside an enclosure — `,\1 2 3` is exactly that — while
libjay's framing refuses to mix boxed and unboxed results, as J does.
`assemble_items` encloses the simple cells when the collection is mixed
and hands the rest to `assemble` untouched; J still reaches `assemble`
directly and still refuses the mixture, because J refuses it.

The acceptance test is John Scholes' one-liner, which now runs as written
and matches GNU APL byte for byte, shape and depth included:

    {↑1 ⍵∨.∧3 4=+/,¯1 0 1∘.⊖¯1 0 1∘.⌽⊂⍵}

`tests/corpus/apl/life.txt` records it with its stages, and with the
element rules underneath them, against both GNU APL and Dyalog.

Two things the sweep surfaced and this wave deliberately left alone, both
older than it and neither in the outer/inner-product family: dyadic `⊂`
with a scalar left argument (`1 2∘.⊂(1 2)(3 4)`), and the mixed character
and numeric array GNU APL builds where libjay refuses one
(`'ab'∘.,(1 2)(3 4)`). Applying between items makes both reachable from
more spellings than before; neither answer changed. The same goes for the
recorded divergence over a vector left argument to `⌽` and `⊖`: libjay
answers where GNU APL raises a rank error, and now does so from inside
`∘.` too.

## 2026-08-22 — The fusion fallback counter belongs to the test measuring it

`random_chains_agree_with_the_interpreter` asserted that at least 60 of
the fused chains reached the kernel, and counted them by reading
`fallback_count()` before and after each run. That counter is one number
for the whole process, so any other test in the binary that made the
kernel decline during the window was counted against this test: it failed
twice with "only 58 of 295".

The measurement is worth keeping — the acceptance rate is what tells a
change to the type rules from a change to the kernel — so the fix gives it
the counter rather than weakening the assertion. `tests/fuse.rs` now
routes every run through an `RwLock`: an ordinary run takes it shared, a
measurement takes it exclusively, and `fallbacks_during` reads the counter
on both sides of that exclusive window. Ordinary tests still run beside
each other; only the five that measure serialise, and the rate they report
is now the same on every run (72 of 295).
- 2026-08-22 — **The sweep's correctness blockers: five clusters, one rule
  each.** A seeded differential sweep against both oracles produced a
  register of clusters; this wave closes A6, A1, A5, A12 and A4. Each
  decision below is the oracle's answer, re-confirmed one sentence at a
  time and not read out of the sweep.

  **A panic is never an answer, so fix the family and not the instance.**
  `9223372036854775806 |. 1 2 3` added the rotate amount to a coordinate
  before reducing it modulo the axis — an overflow, which is a panic in a
  debug build and a silent wrap in a release one, and a panic crossing the
  C ABI takes the host process down. The amount is now reduced first. Four
  more sites counted user integers the same way and were found by probing
  for them rather than waiting for a report: `|.!.f` (saturating, because
  an amount that cannot be added has carried the item off the axis by any
  measure), the cut rectangle in `u;.0`, the tessellation in `u;.3`, and
  the outfix `x u\.`. The three that compare or divide now do it in
  i128/u128, which holds `i64::MIN` without negating it. The unit tests
  assert only that each answers or refuses, never which: what the answer is
  belongs to the corpus, and what must never happen belongs here.

  **APL rotate is not J rotate.** `x⌽y` moves ONE axis and reads one amount
  for each vector along it, so `⍴x` must be `⍴y` with that axis removed
  unless x is a scalar. libjay had been treating x as J's one amount per
  axis and building a bigger array for sentences the language rejects —
  `0 1 1 0⌽5` answered `5 5 5 5` where GNU APL raises RANK ERROR, the
  largest single cluster in the APL sweep and the worst kind, a silent
  wrong shape. `⌽` and `⊖` now carry a primitive of their own
  (`DyadOp::RotateApl`) that picks its axis and checks conformability
  itself, rather than borrowing the rank operator's framing, which extends
  a scalar right argument where APL will not. GNU APL accepts a ONE-ITEM
  VECTOR as a scalar there and rejects a one-item matrix; libjay follows,
  because the oracle wins. The old "⊖ reads per axis" divergence is
  therefore gone, and with it the claim that `⌽` already followed APL.

  **A large rotate amount is where GNU APL is wrong, and libjay says so.**
  `9223372036854775806⌽1 2 3` is `2 3 1` there. It is not an f64 artefact:
  `3|9223372036854775806` is exactly 0, so the value is held exactly. The
  amount is truncated to a SIGNED 32-BIT integer before the modulo — the
  low 32 bits are ¯2, and ¯2 modulo 3 is 1. Sixteen magnitudes around
  2⋆52, 2⋆53 and 2⋆63 fit that and nothing else. Recorded as a divergence
  with the reasoning, not copied.

  **An APL literal is read as an integer, not through a double.** Every
  i64 above 2^53 rounds on the way through an f64, which is why
  `9223372036854775806⌽1 2 3` used to be refused as non-integral and
  `(⍳5)|9223372036854775806` answered from the rounded value. The lexer now
  parses the digits straight into an i64 and falls back to the double only
  where the text needs one. J's lexer already did this. NOT fixed, and
  reported as still open: `#:` and `#.` compute their digits in f64
  throughout, so `5 #: 9223372036854775806` is still 0 where jconsole says
  1 — the same demotion, but a redesign of the encode path rather than a
  line.

  **A column-major RESULT is a value like any other.** The runtime's rule
  is that a value reaching a verb has been made row-major; `|:` flips the
  layout flag instead of moving the buffer, and nothing had extended the
  rule to what a verb hands BACK. Framing cells spliced their raw buffers
  end to end, so `|:;._1 i. 3 4` came back with the shape transposed and
  the data untouched, and `|:"2` was wrong at every rank; opening a box and
  walking the leaves of a nested value read them the same way, so `∊⍉¨` had
  the wrong order and `; |:&.>` asserted outright. Three readers — the
  framing, `open_cell`, `leaves` — now take the rows. Normalising at the
  WRITE side was considered and rejected: boxes are built in forty-odd
  places and results in more, while the readers are countable.

  **The reduce identity table reads the language.** APL keeps no infinity
  among its neutral cells: over an empty axis `⌈` yields the low extreme of
  the representable range and `⌊` the high one. GNU APL's is not `f64::MAX`
  but exactly 1.7976e308 — a rounded constant, confirmed by arithmetic on
  the answer rather than by its printed digits — and libjay takes it,
  because the oracle wins on the value as well as on the rule. J's `>./`
  and `<./` keep `__` and `_`. Every other entry of the table is shared and
  is now pinned as such.

  **The nub sieve is two functions, not one.** APL's `≠` runs over the
  ELEMENTS in ravel order and keeps the argument's own shape (`≠2 3⍴⍳6` is
  a 2 by 3 table); J's `~:` runs over ITEMS and answers one bit each. Ours
  had been J's in both languages, which silently poisoned everything
  downstream of an APL `≠` — two of the sweep's "they refuse" rows were GNU
  APL rejecting OUR shape.

## 2026-08-22 — Every agent in its own worktree

The "engine core stays in the shared tree, at most two agents" rule is
retired. It was never truly parallel — one checkout holds one branch, as
the day's own work proved when a second engine agent had to take a
worktree anyway and landed cleanly. The real constraint is scope overlap,
which is the same in any tree; the orchestrator assigns disjoint scopes,
agents work in worktrees under .claude/worktrees/, and landing stays
serial and orchestrator-only.

## 2026-08-22 — The testkit's own two defects: a narrow page and a fixed origin

An oracle that abbreviates is an oracle that lies. jconsole's default
output control is `0 256 0 222`, so any answer wider than 256 columns came
back ending in `...` and would have been recorded as that; `9!:37 ] 0 4096
0 4096` now opens every J run, as `--PW 10000` and `⎕PW←32767` already
opened the GNU APL and Dyalog ones. Nothing recorded was corrupt — a full
`record j --check` after the change is clean — so this closes a latent
recorder bug and a standing source of false "differ" rows, not a snapshot
repair.

`fuzz --compare` set the index origin to 1 for both sides and dropped the
`@ io=0` directive of an `--exprs` file, so no index-origin disagreement
could ever be fuzzed. The origin now travels beside the sentence as a
`Probe`: an `--exprs` file is compared under the origin its directives
give it, the printed (non-compare) output re-emits those directives so a
kept line pastes into a corpus file intact, and the APL generator draws
one probe in eight at origin 0. J probes stay at 1 — J has no index
origin, and both sides ignore it.
## 2026-08-22 — The Dyalog dialect, wave 1: the recording taught the semantics

`Dialect::dyalog()` is a preset now, not a refusal. Seven settings moved,
and every one of them was decided by the recorded `dyalog:` column rather
than by a document: `⎕CT` is `1e¯14`, `↑` mixes and `⊃` takes the first,
`⌷` names the leading axes, a dyadic `⊂` counts partitions, `≡` signs its
depth, a dfn answers with its first sentence that is not an assignment,
and a nested grade uses the total array ordering. Measured with
`jay-corpus stats apl --dialect-diff --dialect dyalog`, which replays the
recorded column under a preset and starts no interpreter: 150 of 1479
recorded answers differed under the shipped dialect, 61 under the preset.
The two settings that are not a preset knob but a whole rule — the depth
sign and the ordering — are the ones the published descriptions got least
right, which is the point of this entry.

**`≡`'s sign is about DEPTH uniformity, not shape.** The obvious reading
of "uniform" is that the items agree in structure, shape included. It is
wrong, and the recording says so in one line: `≡,\1 2 3` is `¯2`. That
scan's items are `1`, `1 2` and `1 2 3` — different LENGTHS, and if length
counted the answer would still be negative for the wrong reason, but its
first item is a simple scalar, depth 0, beside two vectors of depth 1, and
that is what makes it `¯2`. The confirming line is `1 2∘.⍴3 4`, a 2 by 2
table whose elements are vectors of depth 1 and of lengths 1 and 2:
uniform, `2`, positive, though no two of its shapes agree.
A shape-based rule gets that one wrong and regresses the corpus. So
`DepthSign::Signed` compares depths and nothing else.

**The total array ordering's tie-breaks are backwards from the APL2 one.**
Three of its steps had to be read off `snapshots/apl/grade.snap` rather
than assumed. The shapes are BROUGHT TOGETHER, not compared: the lower
rank gains leading 1s and each axis is taken to the longer of the two, so
two arrays are read position by position over a shape that covers both,
and a position one has and the other lacks decides on the spot — what is
not there sorts below every value there is. Where no atom separates them
the type decides, and its order is numbers, then NESTED values, then
characters — nested in the middle, where APL2 puts it last. Only then the
shape, read with the LAST axis most significant, which is the reverse of
APL2's first-axis reading. Each of the three was a coin-flip from the
prose and a fact in the snapshot.

**A partitioned enclose answers a VECTOR whatever the rank it partitions.**
`Partition::Counts` reads the left argument as counts — each item says how
many partitions to open before it, so `1 0 1⊂1 2 3` is `(1 2)(3)` where
the flag reading gives `(1)(3)`, and a count above one opens a partition
nothing falls into: `2 0 1⊂1 2 3` is three, the first of them empty. The
shape of the answer was the surprise. `⍴1 0 1⊂2 3⍴⍳6` is `2` under the
preset and `2 2` under the shipped dialect: the partitions are a VECTOR,
and each of them keeps the leading axes whole, rather than the frame of
the argument carrying a shortened last axis. A left argument shorter than
the axis zero-pads rather than erroring, so `1 0⊂1 2 3` is one partition
holding all three. `⊆` is the flag reading in both lines and does not
move.

**What the recording could NOT settle stayed put, and is documented as
unsettled.** `⎕CT`'s value is `1e¯14`; whether it scales by the larger or
the smaller magnitude is a question no recorded answer separates, because
every corpus expression that reads the tolerance agrees under both rules.
`by_smaller` therefore stays `false` — the GNU reading libjay already
verified — and coverage.md says the value is what the preset changes. A
setting is not moved on a guess when the oracle is silent; the guess would
be indistinguishable from a measurement in six months.

The 61 that remain are itemised in status.md, "APL — the Dyalog line", and
they are mostly not dialect knobs. The largest, 20 rows, is not a
divergence at all: a `∇`-definition cannot reach Dyalog's editor over a
pipe, so the recorder holds an error for every tradfn line, and measuring
them means re-recording through `⎕FX`. Then 15 rows of inner product `f.g`
with a non-scalar `g`, where the two lines nest the intermediate
differently and the Life idiom is the visible casualty; 9 rows of libjay's
own pinned divergences from BOTH references (the infinity policy, the
empty-base `⊥`); 7 rows of dyadic `⍳` with a non-vector left argument; 5 of
display and prototype edges; 3 of complex floor, ceiling and the `¯7○`
branch cut; 2 of singletons of different rank conforming. Wave 2 is the
`⎕FX` re-recording and the inner product, in that order, since the first
changes what the second is measured against.

The shipped dialect is untouched, and that is checked rather than
asserted: `--dialect-diff` with no flag still reports 150, and GNU APL's
column — the actual gate — replays green.
- 2026-08-22 — Dyalog-only corpus themes, and `@ reference=` to hold them.

  Four themes — `dyalog-dfns`, `dyalog-dops`, `dyalog-control`,
  `dyalog-operators` — record what only Dyalog can answer: dfn guards, `⍺←`
  defaults, `∇` recursion, shy results, `⍺⍺`/`⍵⍵` operators, the `:If`
  family, and the operators GNU APL has no character for. Every one of them
  is a row docs/status.md marks "no oracle", which until now meant held to
  the published specification in `tests/definitions.rs` and nothing else.

  **A theme may name the implementation it belongs to.** The corpus format
  gained one file-level directive, `@ reference=NAME`. The recorder writes
  only that key into such a file and skips it when asked for another, so a
  full `record apl` run does not fill it with GNU APL refusals; the replay
  evaluates every line, counts what libjay already matches, and fails on
  none of it. The alternative — recording a `gnu:` column of `<error>` and
  demanding libjay refuse in step — would have pinned the OPPOSITE of the
  intent: libjay implements most of these, so agreement with GNU APL there
  is exactly what must not be required. This is the `dyalog:` rule already
  in force per record, lifted to a whole file.

  **Control structures are fixed with `⎕FX`, not written between two `∇`s.**
  Not a judgement about the language: the Dyalog oracle is driven as a piped
  session, where the `∇` editor prints a `[1]` prompt per line onto stdout
  and echoes the body onto stderr, so a `∇`-defined tradfn cannot be
  recorded through that channel — every one of them in `definitions.snap`
  reads `<error>` for exactly this reason. `⎕FX` takes the same lines and
  fixes the same function. libjay has no `⎕FX` yet, so the whole theme is
  backlog; the semantics of `:If`, `:While`, `:For` and `:Select` are
  recorded regardless, which is what a future dialect needs.

  **`⍢` is not a Dyalog glyph.** docs/status.md's under row names Dyalog as
  its reference; Dyalog 20.0 answers `SYNTAX ERROR: Invalid token: "⍢"`.
  The oracle wins: the theme keeps a few lines recording the refusal, the
  row is a libjay extension rather than a Dyalog feature, and the status
  table should say so.
## 2026-08-22 — Prototypes, fill cells and widths as lengths

**An empty nested array carries its prototype.** APL2's prototype is part
of a value, not a property of its type: `0⍴⊂2 3⍴9` and `0⍴⊂'ab'` are both
empty vectors of boxes and `↑` answers a 2 by 3 table of zeros for one and
two blanks for the other. Nothing in an empty buffer says which, so `Array`
gained a private `proto: Option<Arc<Array>>`, set by the operations that
make an empty out of a nested one — reshape, replicate, expand, take,
drop — and read by the fills, by `↑` and by the fill cell a mix runs on.
It is APL's alone: J fills a box with `a:` whatever the argument held, so
no J path sets it, and equality ignores it, which leaves `⍬≡0⍴⊂⍬` the
named gap it already was.

**A frame with no cells learns the cell's shape from a cell of fills.**
This is J's own rule, and it was already implemented for one case (a window
longer than its argument). Generalising it — one `empty_frame` helper, one
`fill_cell` — fixed the whole "an empty loses its rank" family at the
root: the rank conjunction, `⍤`, replicate and expand along an axis, the
scan, the infix, the outfix and the cut all framed cells and all answered
an empty of the frame alone. Two limits keep it honest: the verb must be
pure (running it to learn a shape must not run it for its effects) and the
fill cell must be small enough to be worth building; failing either, the
frame stands on its own. APL's scan is the one path that does NOT probe —
its shape is the argument's by definition, function or no function, and
GNU APL's `+\` and `×⍀` agree.

**A written number that becomes a width is a length.** `9223372036854775806
": 1` panicked in the formatter for the same reason `9223372036854775806
|. 1 2 3` panicked in the rotate: a number the program wrote was used as a
size without being checked. Widths and digit counts now go through
`limits::count`, in APL's `x⍕y` as well as J's `x ": y`. J itself answers a
4-billion-character field and stops answering above that, so the ceiling
lands where the reference's does; GNU APL instead falls back to E-format,
which is pinned as a divergence.

**Three GNU APL quirks pinned rather than chased.** The relational family
and `⍸` are ordered across types there and refused here (ISO and Dyalog
refuse too); `∊` of an empty answers one element there and the empty here
(Dyalog agrees with us); an integer near 2^63 goes through a double there
and stays exact here. Together they were about a third of the APL sweep's
mismatches. A fourth was found by this wave: GNU APL's `-\` and `-⍀` drop
an empty result to a rank-1 empty where its own `+\` keeps the shape.

## 2026-08-22 — N-wise reduction: `f/` is its own node, not J's `u/`

**`n f/ y` was a silent wrong answer, and the cause was one missing node.**
APL's `+/` compiled to `Rank(Reduce(+), [1,1,1])`, so its dyad reached J's
`Verb::Reduce` dyad, which in APL mode is the outer product `∘.` also
derives. Applied to `2` and `1 2 3` that is `2+1 2 3` — `3 4 5`, plausible
and wrong, with the shape wrong too (`⍴2+/1 2 3` said 3 where GNU APL says
2). Every moving sum, moving difference and pair-building idiom answered
like that with no diagnostic. `∘.` genuinely needs `Reduce`'s dyad to be
the outer product, so the two spellings cannot share a node: APL's `/` and
`⌿` now derive `Verb::NWise`, whose monad is the same insert and whose dyad
is the n-wise reduction. J's `u/` is untouched.

**The rank wrapper leaves the left cell unranked.** `+/` is
`Rank(NWise(+), [1, RANK_INF, 1])`, not `[1,1,1]`: the monadic rank 1 is
what makes `/` the LAST axis, and the unranked left cell is what makes n
one number rather than a frame of window lengths the way J's infix takes
it. `(1 1⍴2)+/1 2 3` is `3 5` because of it, and `1 1+/2 3` is the length
error GNU APL raises rather than a two-row answer. `⌿` is a bare `NWise`
and `+/[k]` is `AlongAxis(NWise, k)`, so all three spellings window the
axis their glyph already reduces, and the reduced axis stays in place —
`⍴2+/2 3⍴⍳6` is `2 2`, not `2`.

**A window is `f/` applied to it, which is where every operand rule comes
from for free.** The identity of an empty fold (so `0+/1 2 3 4 5` is six
zeros and `0×/1 2 3` is ones), the enclosure APL's insert puts round a
value that is not a simple scalar (so `2,/1 2 3` is two boxed pairs),
boxes, complex numbers, a dfn operand: none of it needed code. A positive
window takes the blockwise fold `infix` already had, so `2+/y` and J's
`2 +/\ y` run the same kernel and fuse into a chain the same way; the
fusion stage recognises the APL spelling through the same
`absorbable_reduce` that unwraps the rank wrapper for the monadic reduce,
and declines a negative or zero n, which are not plain moving windows.

**What the oracle taught, against the register's paraphrase.** Three
corrections. A window may be exactly one item longer than the axis and
answer an empty (`6+/1 2 3 4 5`); LONGER than that is a DOMAIN error, not a
shorter answer — so `2+/⍬` is a domain error too, not the length error the
register recorded. A negative n reverses each window before folding it
rather than reversing the order of the windows: `¯2-/1 2 3` is `1 1`. And a
rank-0 argument with `|n|` of 1 keeps its rank — `⍴1+/5` is empty where
`⍴2+/5` is `,0` and `⍴0+/5` is `,2` — because a window of one leaves the
argument exactly as it was, axis included.

**The disambiguation was never ambiguous.** `/` is the reduce operator
after a FUNCTION and replicate after a value; nothing about the left
argument enters into it. GNU APL agrees on every probe: `1 1/2 3` is `2 3`
(replicate — no operand), `1 1+/2 3` is a length error (n-wise — the
operand is `+`, and n is two numbers), `2 2/2 3` is `2 2 3 3` and
`2+/1 2 3` is `3 5`. A boolean left argument does not make a derived
function a compress and a count of 2 does not make a bare `/` a window.
That was already libjay's rule for the monadic case and it needed no
change; the corpus now pins it either way round.

**Three empty-argument corners pinned as divergences rather than copied.**
On an EMPTY argument of rank 2 or more GNU APL drops the reduced axis —
`⍴2+/0 3⍴⍳0` is `0` there and `0 2` here — but only when the axis would
have had a nonzero length and only when n is not zero, so its own
`⍴1+⌿0 3⍴⍳0` (`0 3`) and `⍴0+/0 3⍴⍳0` (`0 4`) keep it. Three cases, no
single rule; libjay applies the one rule everywhere. One step further,
`⍴2+/0 0⍴⍳0`: with no cells there is no window to be too long for one, so
the fill-cell probe answers an empty where GNU APL refuses — a consequence
of the prototype work, and the two agree on every non-empty argument. And
`0,/1 2 3` is four empty lists here, because libjay gives catenation the
empty list as its identity (J's rule, which its own `,/⍬` already follows)
where GNU APL refuses — though GNU APL's own `,/⍳0` answers a scalar 0, so
that one is not self-consistent either.

**Found in passing, not ours and not fixed.** GNU APL's `○` under `/`
answers a boolean: `○/1 2 3` is 0 there and `¯0.836022` here, and
`2○/1 2 3 4` is `0 1 0` against `0.909297 ¯0.989992 1.15782`. The MONADIC
reduce shows it too, so it predates this wave and is not about n-wise
reduction; `○` is kept out of the n-wise corpus for it.
## 2026-08-22 — Dyalog wave 2: `⎕FX`, the TAO prototype, dyadic `⍳`'s left rank

The dialect backlog went from 211 of 1892 recorded Dyalog answers to 124.

**`⎕FX` is fixed at compile time, from literal text.** libjay compiles
before it runs, and a definition is a compile-time object here: the parser
already turns `∇ … ∇` into an `ExplicitDef` and registers the name. `⎕FX`
therefore reduces to the same thing — its lines are lexed one at a time,
each line's spans brought back to the literal it came from, and the header
and body handed to the `build_tradfn` that `∇` already used. The sentence
that held the `⎕FX` keeps the name it answers with, and the definition is
emitted as the statement before it.

The alternative, a genuine run-time system function, needs the evaluator to
define verbs while the program runs and name resolution to become dynamic.
That is a language-model change, not a primitive, and it buys nothing the
corpus asks for: every recorded use fixes literal text. So the run-time
form is a named promise — "⎕FX on a definition that is not literal text in
the program", and "⎕FX inside another definition" — rather than a guess.
Dyalog answers a definition it cannot fix with the number of the offending
line; libjay reports the fault and points at the line, which is the
diagnostics contract, and either way the sentence that then calls the
function fails.

This is what makes `dyalog-control.txt` measurable: the theme is written
with `⎕FX` because the `∇` editor cannot be driven over a pipe, and libjay
now agrees with 68 of its 79 expressions where it agreed with 8. The 11
that remain are control-structure gaps, not `⎕FX` ones: `:AndIf`, `:OrIf`,
`:CaseList`, `:For a b :In`, a `:For` that does not disclose its items, a
definition that names one fixed after it, a top-level `:If`, and two places
libjay answers where Dyalog refuses. `:AndIf` was left alone deliberately —
Dyalog short-circuits it, and `Branch::test` is a block whose last sentence
is the condition, not a list of conditions to AND, so the desugaring is a
design question rather than a line of code.

**The recorder sends a `∇`-definition as the `⎕FX` that fixes it.** Twenty
rows in `definitions.txt` and `wave5.txt` were recorded as `<error>` for the
channel's sake alone, and status.md counted them as the largest single
"cause" of the backlog while being no divergence at all. `dyalog::as_fx`
rewrites the block before the script is built; all twenty now answer, and
all twenty agree with libjay. It is the one place the text an oracle is
asked is not the corpus text, so it is documented as an assumption at the
top of `dyalog.rs` and in docs/testing.md, and anything it is not sure of —
an unclosed `∇`, a body line that opens another definition — is passed
through untouched. The corpus keeps the `∇` spelling because that is the
sentence libjay is asked.

**The Dyalog grade separates two atomless arrays by their prototype.** The
comparator had one rule that came from a guess rather than the recording:
where no atom decided, it compared the TYPE of the buffer, with a box
placed between the numbers and the characters. Now that an empty nested
array carries its prototype, the honest comparison is available — the item
each array would have held, which is the prototype for a nested empty and
the type's own fill (a zero, a blank) for a simple one, compared by the
same total ordering. It reproduces both recorded rows, `⍋(0⍴⊂1 2)(⍳0)` and
`⍋(0⍴⊂1 2)('')`, and the box arm survives only for an empty that has
forgotten.

**Dyadic `⍳`'s left rank is a dialect setting, not a fix.** GNU APL and
Dyalog disagree about `(2 3⍴⍳6)⍳5` and about `5⍳6`: the APL2 line searches
the items of a left argument of any rank, Dyalog gives a RANK ERROR for
anything that is not a vector. `Dialect.lookup_left` carries both, the
default unchanged, and it is a field on the primitive rather than a rule
read at run time, so J's `i.` never sees it. Seven rows.
## 2026-08-22 — Which primitives consult the tolerance, and how each reads it

The adversarial round asked one question — which primitives consult `⎕CT`
and J's comparison tolerance — and answered it for 23 spellings. This wave
made the answer match the references, primitive by primitive, and the
surprise was that the two oracles do not read the tolerance the same way
even where both consult it. Every rule below was probed; none was inferred
from a specification.

**Residue rounds the quotient in both languages, by different rules.**
`0.1|0.3` is 0 in GNU APL and `0.1 | 0.3` is 0 in jconsole; libjay answered
`0.1` in both, because the quotient went into an untolerant floor. J's rule
came out of 70 probes as `k = <. y % x` with the TOLERANT floor, then an
exact zero wherever `tol.eq(y, x*k)` — the scale is the DIVIDEND's, which is
why `2 | 1e_14` keeps its `1e_14` while `2 | 4 + 1e_14` is 0. GNU APL reads
the remainder against the MODULUS instead: one within `⎕CT × |x|` is zero,
and one that rounding pushed out of `[0, x)` comes back into range. That
scale is real and was probed at both ends — `1E13|3` is 3 and `1E14|3` is 0,
the threshold moving with the modulus exactly as `⎕CT × |x|` predicts, and
Dyalog agrees under its own smaller `⎕CT` (`1E14|3` is 3 there, `1E20|3` is
0). The quotient's rounding needs both an absolute and a relative test to
match GNU: absolute is what makes `1|¯1E¯14` zero, relative what makes
`1E¯15|1` zero, where the quotient is 1e15 and the gap 0.1.

There is a band about four ulps wide, at a gap of `⎕CT` exactly, where GNU
answers as if its threshold were a hair under `⎕CT` — `1|2.9999999999999`
keeps its remainder there though the gap is 9.992e¯14 against a `⎕CT` of
1e¯13. Six hypotheses were tried against the boundary scan and every one
reduced to the same comparison, so the band is left as a known residual and
the corpus stays out of it.

**`#:` and `⊤` are residues and follow.** `2 2 #: 4 - 1e_14` was `1 2` and
is `0 0` once `encode_one` takes the digit with the dialect's rule. The
fused kernel and the wgpu shader carry the same rule; a fused sentence that
rounded differently would make `|` mean two things.

**`⌊` and `⌈` part company.** J scales the gap by the magnitude, so
`<. 99.999999999995` is 100. GNU APL shifts by `⎕CT` outright — `floor(y +
⎕CT)` reproduces every probe, including `⌊99.999999999995` at 99 and
`⌊¯1E¯13` at 0 — and Dyalog does the same under its own `⎕CT`. libjay had
J's rule in both.

**Grade is tolerant in APL and exact in J.** `⍋1.0000000000001 1` is `1 2`
in GNU APL: the keys tie and the stable sort leaves them alone. `/: 1
1.0000000000001 1` is `0 2 1` in jconsole whatever the tolerance is. The
comparator now carries a `Grading` — the total ordering AND the tolerance —
and forces `Tol::EXACT` for J, so the two cannot drift into each other. The
nested and Dyalog-TAO comparators read the same tolerance; Dyalog's own
answers are consistent with that once its `⎕CT` of 1e¯14 is used, so no
second rule was invented for it. This retires a deliberate divergence: the
note that said "tolerant there, exact here" is gone from the divergences
corpus.

**GCD/LCM is GNU's alone, and gets a knob.** GNU APL rounds an argument
within `⎕CT` of a whole number to it, treats one no larger than `⎕CT` beside
the other as zero, and hands a zero argument's WHOLE partner back with its
sign (`¯3∨0` is `¯3`; `¯3.5∨0` is `3.5`). Dyalog does none of the three and
neither does J — `¯3∨0` is 3 there, `1.0000000000001∧5` grinds out `1.0008E13`
— so this is a dialect setting, `gcd_rule`, not a language rule. Making it
one kept the Dyalog preset right rather than trading one wrong answer for
another.

**What was NOT changed, and why.** The adversarial probes also showed both
references accepting a near-integer float where a COUNT is wanted: `⍳2-1E¯14`
is `1 2`, `(2-1e_14) {. 1 2 3` is `1 2`, and the same for `⍴ ↑ ↓ ⌽ / { $ |.
# q: p:`. That acceptance is NOT tolerance consultation — `⎕CT←0` does not
disable it in GNU APL and `9!:19 (0)` does not disable it in jconsole, and
its threshold sits near 1e¯10 in both, unmoved by either knob. It is a fixed
near-integer admission rule, a separate defect, and bundling it into a
tolerance wave would have tied it to a knob it does not answer to.

## 2026-08-22 — Nine small rules, one oracle probe each

A batch of independent register clusters, every one of them a rule the
references state and libjay did not follow. They share no machinery, so
each was probed on its own and the oracle's answer settled it — three times
against what the brief expected.

**`,.` has a rank floor, so it is not `,"_1`.** `$ ,. 5` is `1 1` in
jconsole and was `1` here. The monad ravels each item into a row and never
answers below rank 2; `,"_1` alone stops one axis short for a rank-0
argument. `,.` had been spelled as the rank-wrapped `,` and now carries its
own monad at infinite rank with the dyadic ranks `_1 _1` on the primitive,
which is what makes `$ ,."0 (i. 3)` the `3 1 1` jconsole reports. The dyad
is unchanged.

**Dyadic `/:` refuses an over-long key, and it is an INDEX error.** The
register said length error; jconsole says index error, because `x /: y` is
`(/: y) { x` and the grade indexes x's items. An atom has one item, so
`5 /: 1` is 5 and `5 /: 1 2 3` asks for an item that is not there. libjay
returned the atom for any key at all — the A1 failure mode in J. libjay has
no Index kind, so the existing out-of-range domain message carries it.

**An outfix checks its operand's domain over the WHOLE argument.** The
oracle's rule is stranger than "refuse chars": `_2 +/\. 'ab'` is a domain
error although every piece it leaves behind is empty, and so is
`4 +/\. 'abc'`, which produces no piece at all. Only an argument of one
item or none escapes it, and then only when no piece is folded — `2 +/\.
,'a'` answers an empty. So the check is not "did the fold apply", it is
"does the operand mean anything for this data", and libjay asks it by
folding the argument once before the loop. The probe is spent on characters
and boxes only, and a one-item argument is asked with its item twice, since
`+/ ,'a'` applies no `+` and would answer nothing. Infix needed no change:
its numeric fast path was already guarded.

**Decode extends a single, and the two languages mean different things by
one.** `1 2 3⊥5` is 50 in GNU APL and `1 2 3 #. 5` is 50 in jconsole, but
`1 2 3 #. ,5` is a LENGTH ERROR while `1 2 3⊥,5` is 50: J spreads a rank-0
atom, APL2 spreads a single element at any rank. Both are implemented as
what they are rather than as one shared rule. J's `#.` needed nothing for
matrices — its rank-1 primitive already frames them — so `(2 2$2) #. 5`
came out right for free. An empty axis on either side weighs nothing rather
than raising a length error, which is what `1 2⊥''` and `(i.0) #. 5` both
report.

**Partition extends one flag, and only one.** `1⊂1 2 3` is the whole vector
enclosed. GNU APL reads `⊂` by the rising-flag rule — `1 1 1⊂1 2 3` is ONE
partition, not three, and `2 0 1⊂1 2 3` drops the middle item — which is
the reading libjay already had under `Partition::Flags`, so only the
extension was missing. The count reading, `Partition::Counts` under
`Dialect::dyalog()`, already extends a scalar and has no oracle here (GNU
APL cannot read `⊆`), so it was left exactly as it was rather than guessed
at.

**`E.` reads two atoms as one-item lists.** `0 E. 5` is 0, `1 E. 1` is 1,
and the answer is a scalar. A rank-1 pattern in a rank-0 argument still
fits nowhere.

**`I.` orders boxes by J's total order.** `(1;2 3) I. (1;2;3)` is `0 1 1`,
and the order that produces it is the one `/:` already grades boxes with —
class, then rank, then shape, then atoms. The box arm is gated on J's TAO:
GNU APL's `⍸` over nested bounds is its own extension, pinned in
divergences.txt as C8, and answering it here would quietly unpin it.

**A zero-imaginary complex is read at the USE, not demoted at the making.**
The brief said to fix the constructor. `3!:0 (j. 0)` is 16 in jconsole —
the value stays complex — while `1 <. j. 0` is 0 and `i. 3j0` is `0 1 2`.
So J does not narrow the value; it accepts a complex with no imaginary part
wherever a real is wanted. Demoting in the constructor would have made
`3!:0` report a float and broken a recorded answer to fix six. The reading
lives in `to_f64_vec`/`to_i64_vec`, which reaches `$ # {. }. |. i. | #. #:
":` at once, and in the ordering verbs, which take the complex path before
that coercion runs.

**An empty carries no type to refuse.** `#. ''` is 0 and `¯3⊥''` is 0: an
empty holds no value of the wrong type, so it is acceptable numeric data
wherever numeric data is wanted. The rule covers empty characters and
symbols and stops there — jconsole answers `#. 0$<1` but refuses
`2 #. 0$<1`, and including empty boxes would have traded one refusal for a
silent wrong answer in the other. Two J rows stay refused for that reason
and are the honest residue of the cluster.
## 2026-08-22 — Where IEEE has no value: J's NaN discipline, APL's refusals

The two references answer the arithmetic IEEE leaves undefined, and neither
answers it the way the hardware does. libjay had been handing the hardware's
result back — `_.` in J, `∞` in APL — which was wrong in both languages at
once. Every rule below came out of the oracles; none was inferred.

**J defines what it can and refuses the rest.** A zero factor wins: `0 * _`
is 0, and so are `_ * 0`, `0 * _.` and `*/ 0 , _`. The rule belongs to the
FACTOR, not the product, which is why `0 * _.` is 0 too, and it is the whole
explanation of `j. _` being `0j_` — a complex product is four real products,
and each follows it. Where J has no value it refuses with a NaN error, and
the test that decides is exactly whether the arithmetic MADE the NaN: `_ - _`
is refused while `_. + 1` is `_.`. That distinction is cheap to compute (both
operands are in hand) and impossible to fake afterwards, which is why the
rule lives in the scalar step rather than in a pass over the result.

Probing turned up more than the register listed. Residue has no value for an
infinite DIVIDEND under any nonzero modulus — `2 | _`, `0.5 | _`, `_1 | _`
and `_ | _` are refused alike, while `0 | _` is `_` because a zero modulus
never divides — and `#:` inherits that, so `5 #: _` is refused too. The
binomial at an infinity is a nineteen-entry table, not a formula: an infinite
left argument gives 0 unless the right one sits on a gamma pole (`_ ! _2.5`
is 0, `_ ! _2` is refused), an infinite right one is read off the left's
sign, and of the four infinite pairs only `__ ! _` has a value. A negative
base under an infinite exponent alternates in sign for ever, so J answers
only where the magnitude falls to zero: `_2 ^ __` is 0, `_1 ^ _` is a domain
error. And J's factorial answers `_` wherever its gamma overflows — `! 171`,
`! 1e308`, `! _1e20` — refusing `! __` alone.

**APL has no infinity in a value at all.** `÷0`, `⍟0`, `!¯3` and `0⋆¯1` are
DOMAIN ERROR in GNU APL, and libjay's monadic `÷0` had been answering `∞`
while its own DYADIC `2÷0` refused — an inconsistency inside one primitive,
pinned as a deliberate divergence since the first week and retired here. The
each, reduce and scan paths reach the same scalar step, so fixing the step
fixed `÷¨`, `÷/` and `÷\` with it. Two exceptions keep the refusal from
spreading and were probed rather than guessed: `0÷0` is 1, and `0⍟0` and
`1⍟1` are 1 while every other non-finite logarithm is refused — so the
dyadic log reads "a NaN ratio is 1, an infinite one has no value".

**What was NOT matched, and why.** GNU APL also refuses an arithmetic
OVERFLOW — `1E308×2` and `2⋆1E10` are DOMAIN ERROR — but not uniformly: the
same infinity reached by `+` (`1E308+1E308`), by `*` (`*710`) or by an
integer exponent (`2⋆10000`) is answered. No published definition asks for
the split, and following it would mean refusing `1E308×2` while answering
`1E308+1E308` in the same expression. libjay draws the line at arithmetic
with no VALUE and lets an overflow answer with the infinity it reached; the
five expressions are pinned in divergences.txt. `!¯1E20` is pinned for the
same reason in the other direction — GNU APL prints the gamma function's
underflowed 0 at a pole, where the value is undefined and the nearer poles
`!¯3` and `!¯1` are refused on both sides.

**Keeping the fused and unfused answers identical.** The rules live in the
unfused verbs, and the blockwise kernels — elementwise, fold, scan, window —
now decline a block holding a NaN and let the sentence be redone unfused,
which is the road an integer overflow already takes. Declining on a NaN
rather than on any non-finite value is deliberate: an infinity is an ordinary
value in J, and in APL the primitives that would refuse one (`÷ ⍟ ! ⋆ ○`) do
not fuse at all, `÷`'s zero being caught in the kernel itself. The wider
rule cost two percentage points of the fused acceptance rate for nothing.
A GPU answer is held to the stricter test — anything non-finite comes back
to the CPU — because a shader carries none of these rules and the device's
standing invariant is that it cannot change a result.

**One place the rule had to be applied twice.** `__ ^ 0.5` is `0j_` and
`__ ^ 1.5` is `0j__`, while `__ ^ 0.25` is `_j_`. The polar form already
took an exact cosine at a half turn, so the magnitude met an exact zero
there and multiplied to a NaN; the zero-factor rule finishes it. The pattern
is a good hint that jconsole reaches its own answer by rounding the cosine
with its comparison tolerance, but every probed case is explained by the
exact-angle path alone, so nothing was built on the hint.

## 2026-08-23 — The fuzzer's second generation, and a signature that names the cause

The perpetual sweeper's find rate had been flat across 109 batches while its
cause discovery had stopped: 33 of 39 clusters appeared in the first fifth of
the run, and the last three quarters cost about 230 new mismatch rows per new
cause. Both halves of that have one explanation. The comparator deduplicated
on the expression text, and the space of depth-5 compositions is effectively
infinite, so a fresh string was always available and a flat row count could
never mean "nothing new"; and the grammar drew from pools so narrow that the
same ten leaves carried nearly every finding.

**The signature is hierarchical, coarse first.** A mismatch line under
`--signature` carries `<class>|<primitives>`, where the class is libjay's own
refusal with its numbers replaced by `#`, or the shape and kind of its answer,
and the primitives are the sorted set the sentence names. The first draft put
the primitives first, as the triage suggested; at depth 3 that made every one
of 500 mismatches unique, because a composed sentence names eight or ten
primitives. The class alone collapses a batch into a dozen buckets and is the
level that answers "is this new"; the pair stays available for "is this the
same sentence again". Both counts are printed. The J primitive lexer reads an
inflection as part of the primitive before it (`{::` and `p..` are one token)
and skips numbers and literals, so `0.1` is not a determinant and `'a.'` is
not a verb; the APL one is a glyph set with the same two skips, and `[` is in
it so a bracket axis shows in the signature of the sentence that used one.

**The generation number is part of the contract.** `fuzz::GENERATION` is
printed in the summary, and two runs' find rates are comparable only when it
matches. Generation 2 adds the J conjunctions the tree never composed (`^:`
with a negative, listed or boxed count, `L:`, `S:`, `&.` with a named
obverse, `&.:`, `!.`, both valences of `;:`), an empty of every type and rank
and nests three deep in both leaf pools, tolerance-edge pairs fed to every
dyad that reads its arguments as values, APL bracket axis in its three shapes,
and J ranks of two and three elements.

**Old coverage keeps its absolute weight rather than its share.** The new
productions are new arms on a widened draw — J's 21 arms became 30, APL's 18
became 26 — so the old arms keep their proportion to one another exactly, and
the leaf pools are drawn two times in three from the original list. A
generation that reweights the old arms would make the recorded findings
incomparable with the new ones for no gain.

**Nothing that converges is in the pools.** `u^:_`, `u^:a:` and `f⍣≡` are all
unbounded, and the fuzzer has no timeout on its own side: the oracle's
timeout only produces an `unfinished` verdict, while a libjay that hangs ends
the run. The same reasoning already kept the verbs that turn a value into a
length out of the tolerance-pair arm.

**What the first run found.** 500 expressions per language at seed 1: J 23
mismatches (4.6%), APL 32 (6.4%), 15 distinct causes each. Twelve of APL's 32
are one already-named gap — bracket axis, which generation 1 never wrote, so
the gap cost nothing and showed nothing — and the J side does the same for
`L:_`, `S:_` and `,!.n`. Four causes were new, and the tolerance-pair axis
produced the best of them: GCD and LCM of two arguments that are equal under
the comparison tolerance but not exactly equal answer the value itself in
both oracles, and grind through the float algorithm in libjay. That is the
rule the tolerance wave's residual said it could not find. They are in the
bug register; none was fixed here.
## 2026-08-23 — A near-integer float is a count, and each language has its own width

The register's cluster N16: both references accept a float that is merely
NEAR a whole number where a count, a length or an index is wanted, and
libjay refused all of it. `⍳2-1E¯14` is `1 2` in GNU APL, `(2-1e_14) {. 1 2 3`
is `1 2` in jconsole, and libjay answered "needs an integer" to both. It was
filed separately from the comparison-tolerance wave for a reason that the
probing confirmed: **no setting moves it.** `⎕CT←0` and `9!:19 (0)` leave the
admission exactly where it was, and with the tolerance off jconsole still
answers `(2+1e_13) = 2` with 0 while `i. 2+1e_13` still counts to 2. The two
questions are asked in the same expression and answered differently.

**The two widths, measured rather than assumed.** The register recorded the
threshold as "near 1e¯10 in both". It is not the same rule in both, and the
first bisection said so: at 2, GNU APL takes `9.99996E¯11` and refuses
`9.99998E¯11`, and the double for the second is 1.0000000827e¯10 — an
ABSOLUTE `1e¯10`, and the apparent oddity of the boundary is only the
floating-point grid. Raising the magnitude leaves it where it was: `20+5E¯10`
and `1000000+1E¯9` are refused, `1000000+1E¯11` is taken. Below the window
there is no lower edge either — `⍳1E¯11` is the empty vector, because 1e¯11
reads as 0.

jconsole's is RELATIVE. At 2 the boundary sits between `1.1369e_13` and
`1.14e_13`, and dividing the accepted delta by the whole number gives
2^_44 to the last bit; at 20 the boundary is ten times wider, at 1e6 it is
`5e_8` taken and `6e_8` refused. So J admits `|x - n| ≤ 2^_44 × max(|x|,|n|)`
— the shape of J's own tolerant comparison, at a FIXED constant that
`9!:19` cannot touch, which is why the two coincide numerically at the
default and part company the moment a program moves the tolerance.

The two windows cross at about 1760. Below it APL is the permissive one
(`⍳2+9E¯11` answers, `i. 2+9e_11` does not); above it J is (`i. 1000000+1e_9`
answers, `⍳1000000+1E¯9` does not). Neither is a superset, so one shared
rule was never an option.

**Where it lives.** `Array::to_i64_vec` stays exact — it is also how the
engine asks whether a value IS an integer, and rounding there would make
`x:`, the fusion step detector and the sparse paths lie. The admission is a
second reader, `to_i64_vec_near(NearInt)`, and `NearInt` is threaded from
`EvalCfg::near()` to the leaves that read a count. That is about thirty
signatures, and passing the rule rather than deriving it at the leaf is
deliberate: an `Array` does not know which language is running, and a
thread-local would not survive the rayon workers the cell paths use.

**It is not universal, and the oracle drew the line.** The count positions
take it — every one probed did, including `?`, `⎕UCS`, `⊃`, `I.`, `C.`, `|:`,
`u^:n` and `f`g @. n`. Operand SELECTORS do not: jconsole refuses
`3 u: 65+1e_13`, `s: 65+1e_13`, `(2+1e_13) b. 3` and both cut modes while
answering `u: 65+1e_13`. So the rule is the count's, not every noun's, and
the exact readers were left exact. Two of the operand positions are read at
compile time, in the J frontend, and take the admission there instead:
`(>: ^: 2.0000000000001) 3` is 5.

**A third GNU/Dyalog split, found while recording.** The Dyalog column for
`corpus/apl/tolerance.txt` shows Dyalog's admission is neither GNU's nor
absent: it is RELATIVE and it follows `⎕CT`. `⍴⍳1000000+1E¯9` answers in
Dyalog (1e¯15 of the magnitude, inside `⎕CT` 1e¯14) and is a domain error in
GNU APL, while every `2±9E¯11` case is the other way about. libjay's dyalog
preset follows GNU, so those seventeen rows are a preset gap, itemised in
docs/status.md. Closing it is a dialect setting rather than an engine
change now that `NearInt` is threaded, but it is a new knob on the public
Dialect object and was left for the wave that decides it.

## 2026-08-23 — Four corrections: the tolerant GCD, `C.`'s abbreviated permutation, repeated roots, and APL's mixed arrays

**Euclid on reals stops at a rounding error, and the tolerance is measured
against the LARGER argument.** `0.3 *. 0.1+0.2` was 2.25e15 because the
remainder 5.5e¯17 was not zero and Euclid ground on to a divisor of 4e¯17.
Both references answer 0.3. The rule that reproduces jconsole bit for bit
over the whole probe set is: keep the tolerant floor already there, and
treat a remainder no larger than `⎕CT × max(|a|,|b|)` — the scale the
division sequence started at, not the current divisor — as zero.
`1.23 +. 4.56` then grinds out 0.029999999999994476, which is the value
jconsole prints, and `1.0000000000001 +. 1` gives 9.99201e¯14, closing a
row the A7 tolerance wave had left as residue. GNU APL's answers agree
except where the two magnitudes are more than about `⎕CT` apart, where its
tolerance is relative to the current divisor instead: `1E6∨1E6+1E¯7` is
1.16e¯10 there and 1.00001e¯7 in J. Those grind cases stay out of the
corpus rather than being modelled twice.

The decimal reading — 1.23 and 4.56 as 123 and 456 hundredths — stays,
because it is what makes `123.456 +. 78.9` print as 0.012, but it now
refuses a value that needs more than twelve significant digits to print
back. Such a value is a rounding residue, not a decimal anyone wrote, and
reading `0.1+0.2` as 30000000000000004 seventeenths of a decimal place is
what turned the noise into a divisor in the first place.

**A short direct permutation names the items it OMITS.** `C. 3 4 2`
answers the cycles of a permutation of five, not a length error: J reads a
direct permutation shorter than `1 + >./ y` as the abbreviation whose
missing items come first, in ascending order, with the given list as the
tail. One rule covers the monad and the dyad and replaces two separate
pieces of guesswork — the monad's "a permutation of as many items as it
has" and the dyad's "the items past it come round to the front". It also
disposes of the atom case (`2 C. i.5`), which was a named gap and is
simply an abbreviation of one item. `C. b. 0` reports `1 1 _`, so the
ranks went in with it.

**A repeated root is found through the derivative, not through the
polynomial.** Durand–Kerner converges on a root of multiplicity m only to
about the m-th root of the machine epsilon, and no refinement against the
polynomial itself can do better: near such a root `p(z)` cancels to exactly
zero over a ball of that radius, so Newton has nothing to divide. The m-1st
DERIVATIVE has the same root simply and with none of that cancellation —
`1 3 3 1`'s second derivative is `6 6`, whose one root is ¯1 — so roots
that cluster are gathered, counted, and put back on the root their
derivative names. The grouping radius is a guess, so the result is kept
only if the polynomial rebuilt from it fits the coefficients at least as
well as the raw roots do, and the widest radius that passes is the one
taken. Order was also unspecified in libjay and is not in jconsole:
descending magnitude, then descending real part, then descending imaginary
part reproduces every probed answer but the tie-order within a repeated
conjugate pair.

**APL's mixed simple arrays are built, not refused.** `1 'a'` has always
evaluated in libjay — held as rank-0 boxes, since enclosing a simple scalar
is no change at all in APL — but every verb that would have BUILT one
refused the pair instead. Catenate, union, intersection, without, find,
member, index-of, match and enlist now build and read them. Two mechanisms
carry it: a simple array beside one held as boxed scalars is spread into
the same form before the two are compared, and every APL result passes back
through the opposite step, so a boxed form that turns out to share one type
is the plain array again. That second step is what makes `2↓1 2,'ab'` match
`'ab'` and `+/1 2 3∩'a' 2` answer 2 rather than refusing. It is APL-only:
J's `<2` is a value of its own and never the same as `2`, and J still
refuses `1 2 , 'ab'`.

The display rule came from the oracle too: in a mixed VECTOR a run of
characters beside each other is text and prints with no separator, so
`1 2,'ab'` shows as `1 2 ab`. At rank 2 and above each character is a
column of its own again and the separator comes back, which is why
`2 3⍴1 2,'abcd'` prints as `1 2 a` over `b c d`.
## 2026-08-23 — Dyalog wave 3: an array as an operand, a dfn's scope, and the third tolerance family

The recording said 141 of 1989 rows still differed under `Dialect::dyalog()`.
Four causes were taken; the residue is 74, itemised in docs/status.md.
Every answer below is the recorded Dyalog one — no oracle was run, and
where a guess disagreed with the recording the recording won.

**The near-integer count is Dyalog's third rule, and it is a dialect
setting.** The previous entry left it named and unimplemented. It is
`Dialect.near_count`: `Absolute` (GNU APL's flat `1E¯10`) or `Tolerant`
(Dyalog's, the dialect's own tolerant equality against the whole number).
`NearInt` gained a `Tolerant(Tol)` arm rather than a bare relative
constant, because the window MOVES with `⎕CT` — `⎕CT←0` closes it — and
carrying the `Tol` is how `EvalCfg::near()` hands the tolerance in force to
a reader thirty signatures away. Seventeen rows.

**The floor is a third rule too, and the obvious formula loses to
rounding.** Dyalog's `⌊` is documented as `⌊y+⎕CT×1⌈|y`, and every recorded
row agrees with it — except `⌊999.99999999999`, which Dyalog answers 999
and the formula, evaluated in doubles, answers 1000: adding `9.9999999999999E¯12`
to it rounds up to a clean `1000.0` where the exact sum is still below. So
the `Scaled` arm compares the GAP against the step instead of adding it,
and the `Shift` arm keeps the addition GNU APL's answers were verified
against. Two arms, one field, no shared code path.

**`⊤` does not round in Dyalog.** `2 2⊤4-1E¯14` is `1 2` there and `0 0`
here, the last digit being `1.99999999999999` rather than a tolerant zero.
`Dialect.encode_digits` sets the tolerance aside for that one reading;
nothing else in the sentence changes.

**Dyalog's grade reads no tolerance at all.** `⍋2 (1+1E¯14) 1` is `3 2 1`
there and `2 3 1` under the APL2 comparator, where the two near-equal keys
tie. That rides on `NestedGrade::TotalOrder` rather than a field of its
own: the ordering and the tolerance it reads are one comparator, and the
preset already selects it.

**An array where a function operand belongs.** `2∘×` is the bond J already
has (`Verb::BondLeft`/`BondRight`), so `∘` with a literal array on either
side lowers to it and nothing new runs. A COMPUTED operand — `(⍳3)∘+` —
stays a named gap: the IR holds an operand's ARRAY, not its expression, and
giving it one would mean deciding when an operand is evaluated. J has the
same gap under the same name ("bonds over a non-literal noun").

**A dfn operator's array operand changes how the BODY PARSES.** `⍺⍺+⍵` is a
train when `⍺⍺` is a function and a sum when it is an array, and the body is
parsed once, at definition, while the operands arrive later — possibly in
another sentence (`BOTHARR←{⍺⍺,⍵⍵,⍵} ⋄ (1 BOTHARR 2) 3`). So the body is
parsed under all four readings and `OpDef` keeps them, indexed by which
operands are arrays; a reading that will not parse keeps its diagnostic
instead of failing the definition, because a body only has to make sense
under the operands it is given. The alternative — deferring the parse until
the operands are known — would have meant carrying tokens in the IR.

**A dfn is ambivalent, and its guard is strict.** Two rules libjay had
guessed, both wrong against the recording: `3 {⍵×2} 5` is 10 (a left
argument the dfn has no name for is dropped, not refused) and `{2:1 ⋄ 0} 5`
is a DOMAIN ERROR (a guard wants exactly one 0 or 1). The first is
`ExplicitDef.spare_left`, false for `∇` definitions and for J's; the second
is a `Control::Guard` node of its own, because `:If` keeps the loose
reading and the two cannot share one. Neither is behind a dialect setting:
dfns are a Dyalog-only extension with no APL2 reading to be the other arm,
so the recording is simply the rule, in every dialect.

**A dfn has no control words, so `:` inside one is always a guard.**
`{a←⍵×2 ⋄ a>10:a ⋄ a+100}` used to fail on "unknown control word: :a". The
lexer already counts braces; inside them the `:` is a guard whatever
follows.

**Lexical scope, by lexical ancestry rather than by frame depth.** A dfn
written inside another reads the enclosing one's locals. Walking the frame
stack outward would give DYNAMIC scoping, which agrees with the recording
on every row but is a different language: a dfn named elsewhere and called
from inside one would see its caller's locals. So each dfn gets an id and
the ids of the dfns it is written inside, and a name not in the top frame
is looked for in the frames below whose definition this one is written
INSIDE. The chain is collected while the body is parsed, in a thread-local
stack, because the parse of a nested dfn is reached through the statement
splitter, the guard reader and the sentence parser, none of which has any
other reason to carry it.

**A dfn may name a function of its own.** `F←{G←{⍵×2} ⋄ G ⍵}` answered 0,
because the flag that stops a `∇` body from registering a name in the
ENCLOSING program's verb table also stopped a dfn body from registering one
in its own — and a dfn body parses against a clone. The flag now says what
it means (`shared_verbs`), and a dfn registers. The Dyalog result rule
needed the same correction: naming a function is an assignment, so it is
not the "first sentence that is not an assignment".

**`f⍣¯n` is J's `u^:_n`.** The obverse table was already there and already
shared; only the APL parser refused a negative count. Four of the five
recorded rows now answer. The two that do not are obverse gaps: a bond
(`(2∘↑)⍣¯1`) is not in the table, and `⍵⍵⍣¯1` names a verb that is only
known when the operator runs, where the obverse is taken at parse time.

**What was left, and why.** A SHY result — a dfn whose answer came from an
assignment has a value the session does not print — needs a channel libjay
does not have: every call yields a value and every top-level value is
printed. So does a dfn that falls off its end with no result at all. Six
rows, and the shape of the change (a third state beside `Some`/`None`
threaded through every verb application) is a wave of its own. Two more
rows are a collision rather than a gap: `{a}`, a dfn whose whole body is
one identifier, is libjay's interpolation hole, and the brace binding is a
fixed point of the embedding.

## 2026-08-23 — The nested display's own spacing, oracle-probed

The register carried one residual from the mixed-array wave: `'ab',⊂1 2`
prints ` ab  1 2` in GNU APL and ` a b 1 2` here, and the whitespace-blind
sweep comparator could not see the gap width, only the length of `⍕` —
`⍴⍕(1 2)(3 4)` was the pinned entry that carried it. Probed vectors,
matrices, character runs and depth ≥2 wrapping against GNU APL directly
(the raw subprocess output, not the tolerant comparator) to find the rule.

**The gap between two items in a nested vector is one baseline space plus
however much the more complex neighbour's own shape asks for.** A scalar
asks for nothing. A non-scalar item — a plain array of rank ≥1 sitting
directly in the array, or anything under `⊂` — asks for one column per
axis of its fully-unwrapped content: a vector costs one, a matrix two.
A character array costs one column FEWER than its rank, because a row of
characters already reads as text on its own — a character vector then
costs nothing (same as a scalar) and a character matrix costs one. Two
adjacent lone characters merge into one run with no separator at all,
whatever box either of them sits inside. The vector's own margin, front
and back, is how many `⊂` layers wrap its first and its last item
respectively (never fewer than one baseline space).

The rule was found by holding every other variable fixed across a probe
grid — `1,⊂1 2` against `1,⊂2 2⍴1 2 3 4` isolates rank from character-ness,
`'x',⊂'abc'` against `'x',⊂2 3⍴'abcdef'` isolates a character vector from a
character matrix, `⊂⊂1 2,1` against `1,⊂1 2` isolates the margin from the
gap — until every probed combination (16 corpus lines, plus the standalone
depth cases and a scan over a nested vector) matched the model in one pass.

One combination did NOT fit and was deliberately left alone: GNU APL's
STRAND notation, `(⊂1 2)(⊂3 4)`, keeps its operands more deeply enclosed
than the same value built by catenate, `(⊂1 2),(⊂3 4)` — `≡` reports 3 for
the first and 2 for the second, though both read as "a two-item vector of
boxed pairs" at a glance. That is a value-level quirk of GNU's `,` and `⊂`
interacting with strand, not a display rule, and libjay's own catenate was
found to have the mirroring gap — `1,⊂⊂1 2` collapses the double box to a
single one on the way through `,` — a pre-existing verb.rs issue, filed for
a future wave rather than fixed here. Nothing in the corpus exercises
either shape, so the printer fix stands on its own.

**Scope: vectors only.** A mixed array at rank 2 or above (a matrix whose
cells mix scalars and non-scalars) still draws with libjay's own uniform
one-space cells; GNU's column alignment for that shape was not probed
deeply enough to be confident of a rule, and no example in the register
needed it. `format_boxed` (J's fenced drawing, and every rank ≥2 array
regardless of language) is untouched — only APL's rank-1 `BoxStyle::Spaced`
path gained the new `nested_vector_line`. `mixed_simple_texts` and
`mixed_vector_line` were both widened to peel through `⊂⊂x` box-of-box
chains to their leaf before asking whether an item is a scalar, which is
also what GNU does (`1,⊂⊂5` draws as `1 5`, not with box padding).

`⍴⍕(1 2)(3 4)` converged and its divergences.txt entry is gone, per that
file's own stated policy ("a signal that the note ... should go").
## 2026-08-23 — The Dyalog preset becomes a gate, and the exemptions are named

The `dyalog:` column had been recorded data for three waves: measured by
`jay-corpus stats apl --dialect-diff --dialect dyalog`, reported by the
replay, and asserted by nothing. A number a run prints and no test enforces
drifts, and the direction it drifts in is silent — the preset could lose an
answer between waves and only the next reading would notice. So the preset
is now held to the recording the way the shipped dialects are held to
theirs: `tests/oracle_dyalog.rs` replays every one of the 1989 recorded
Dyalog answers under `Dialect::dyalog()` and fails on any that differs.

**The exemption list is data, not a skip.** A gate over a preset that does
not implement the whole line needs a way to say "not this one, and here is
why", and the two ways it could have been said are an attribute in the code
and a file the gate reads. The file wins: it is reviewable in a diff, it
lives beside the corpus it talks about, and it cannot be spelled without a
reason. `crates/libjay/tests/expected/dyalog.txt` is in the corpus format
with the `? ` note REQUIRED rather than forbidden — the same reader, one
flag apart — so the format learned nothing new and a reader who knows
`divergences.txt` knows this file. A note is `KIND TAG: reason`. The kind
is a promise: `divergence` says libjay keeps answering this way, `gap` says
the work is queued and the TAG names the row of docs/status.md's Dyalog
table that would close it. Fixing that row deletes these rows, which the
gate then insists on — a listed expression that has stopped differing is a
failure of its own, exactly as a converged divergence record is. The list
can only shrink by being edited, and the gate tightens by itself.

**The 69 rows, classified.** 48 are gaps and 21 divergences. The largest
gaps: the inner product `f.g` with a non-scalar right operand (15 rows, the
Life idiom), the control words libjay does not have (9), `⎕R`/`⎕S` (5) and
the shy result (4). The divergences are mostly already pinned against GNU
APL too — the empty-base `⊥` (5), a count above 2⋆53 (3), the value of a
diamond-separated program — plus two rules this wave stated: a preset
chooses a dialect's rules, it does not WITHDRAW an extension libjay ships
in every dialect (so `⍢` and `⍠` answering where the recorded Dyalog
refuses is a divergence, not a gap), and a `{name}` collision is the
embedding's fixed point rather than a queue item.

**What is deliberately not in this wave.** No default moved. The gate is
additive: `oracle_apl.rs` still holds `Dialect::default()` to GNU APL
expression for expression, and a preset change that touched the default
fails there before it fails here. The gate was verified red by hand — with
`depth_sign` in `dyalog()` reverted to the GNU reading, three theme cases
fail with five newly differing `≡` rows and the APL battery stays green.

## 2026-08-23 — Empty arguments, and a size checked before it is allocated

Round 3 of the sweep reported one panic and a family of about twenty rows
where libjay refuses an argument the reference answers. Every rule below
was probed against jconsole or GNU APL first, and where the register's
paraphrase and the oracle disagreed, the oracle won — three times.

**The panic: a cycle asked for its permutation before anything checked the
index.** `(<9223372036854775806) C. 1 2 3` built `(0..top).collect()` with
`top` one past the largest element any cycle names, and 2⋆63 usize values
is a capacity overflow, not an error message. The index is now checked
first, and the check has two readings because the two valences do: with an
argument to permute, an element is an index INTO it — negative counts back
from the end, which is `(<_1 0) C. 1 2 3` = `3 2 1` and, since a cycle of
one moves nothing, `(<_1) C. 1 2 3` = `1 2 3`; without one, `C. y` may name
any length it likes and the length alone is held to the element ceiling.
Both match jconsole, whose answers are an index error and a limit error.
The sibling paths were audited: `permutation_span` already went through
`limits::count`, and `direct_permutation_of` and `anagram_from` size their
allocations from the ARGUMENT, never from a value in it.

**The outfix rule was derived from one verb, and that verb was the
exception.** The A14 wave concluded that J holds an insert's operand to its
domain over the whole argument, from `+/\.` refusing characters. It does —
for `+/`, `*/`, `<./`, `>./` and `+./`, which is the set of folds J has
special code for. Every other operand is asked piece by piece: `2 %/\.
'abc'` is `ca`, because a piece of one item applies nothing. `2 *./\. 'abc'`
answers although `*.` looks like it belongs to that set, and `2 +./\. 'abc'`
refuses although `*.` and `+.` are usually spoken of together — the set is
the oracle's, taken verb by verb rather than by family resemblance. The
probe is now spent only where the operand is one of the five.

**An empty operand takes the other side's type.** J's `,` with an empty
character list beside a numeric one answers the numeric one; so does an
empty box. The retyping happens BEFORE the ragged fill is worked out, which
is what makes `(2 0 3$0) , 'hello'` come out as spaces rather than zeros —
the fill belongs to the result's type. Two empties settle it by container:
box over character over number, probed both ways round. The same rule in
`assemble` is what `,:` and every framing operation needs, and it can only
turn a refusal into an answer: a cell with elements still has to agree.
Catenate also learned J's wider rank gap (`1 2 3 , i. 2 2 2` is a rank-3
answer of shape `3 2 3`), which the coverage test had pinned the other way
on an assumption nobody had asked jconsole about. APL keeps the one-rank
rule, which is GNU's.

**The empty-argument family is per verb, not one flag.** Each of `A.`,
`;:`, `".`, `p.`, `p..`, `;.n`, `/:` and `\:`, and APL's `⊥`, `⊤` and `⊂`,
was probed on its own, and they do not agree with each other:

- `0.5 A. i.0` answers, `1.5 A. i.0` does not — with nothing to permute
  there is one arrangement and no digit to read, so only the RANGE is
  checked, and 0.5 is in it. A character is still no index.
- `p.. (0$'a')` answers and `1 p.. (0$'a')` refuses: the integral reads its
  argument strictly and the derivative does not. Two functions, one
  spelling, two rules — so libjay has a strict reader and a relaxed one
  side by side rather than one relaxed rule.
- An empty fret list is not "no frets": `(0$0) <;.1 'abc'` is the whole
  argument in ONE piece, while `0 0 0 <;.1 'abc'` is no piece at all. A
  fret list of rank 2 or more is J's per-axis form, and an empty one there
  names no axis and answers nothing.
- APL's `⊂` relaxes only when the flags AND the items are both empty:
  `(0⍴0.5)⊂1 2 3` is still a length error.

**What was found and deliberately left.** `(<1) <;.1 y` — a boxed left
argument, J's per-axis frets — is a feature libjay does not have; its
diagnostic changed from "cut frets must be integers" to a named gap, which
is the contract. `⍎(0⍴0)` yields NO VALUE in GNU APL, and a libjay verb has
no channel for that, so it goes on refusing with "the executed string
yielded no value" — the honest message for what happens, and the reason
that row is not in the corpus. `1 p.. (<1 2)` now answers where it used to
refuse, but in floats where jconsole keeps exact rationals (`_3r2 1r3`);
the value is right and the type is not, and it stays out of the corpus
until the polynomial paths carry exact types. GNU's `≡` of an empty nested
array is 2 and libjay's is 1 — the prototype is not consulted — which is
older than this wave and is registered, not fixed here.


## 2026-08-23 — The obverse table, from the reference outward

The sweeper's round-3 log was 57% one sentence: "the obverse of X is not
supported yet". One table answers `u&.v`, `u&.:v`, `u^:_1`, `u b. _1`,
APL's `f⍢g` and `f⍣¯1`, and it held a dozen rows.

**The reference is the whole specification for it.** Every J verb spelling,
and the bonded and derived forms J documents as invertible, was asked
`v b. _1` at the oracle before a line was written. Of the seventy J verb
spellings, forty-three name an obverse there and twenty-seven refuse;
libjay now holds every one of the forty-three but `!`, and refuses exactly
what the oracle refuses. That is the
only defensible rule for a feature whose content is a list: a guess here is
a silently wrong answer, not a missing one.

Probing by name was not enough on its own, and three of the corrections
came from data:

- **`(2&%:)^:_1` raised n to the argument and should have raised the
  argument to n.** `2&%:` is the square root; its obverse is `^&2`, not
  `2&^`. The row was already in the table and had been wrong since it was
  written; `(2&%:)^:_1 ] 3` answered 8 where the oracle answers 9. Nothing
  in the corpus reached it, which is the argument for asking each row on
  data as well as by name.
- **`n&#.`'s obverse chooses its width from the value.** The reference
  writes `1 + <. n ^. 1 >. >./ | , y` digits, so `(2&#.)^:_1 ] 5` is
  `1 0 1` and `(16&#.)^:_1 ] 255` is `15 15`. A fixed width would have
  round-tripped and still disagreed with the reference everywhere else.
- **`n&}.` pads on the side it did not drop from**, and the sign of n
  decides which: `(2&}.)^:_1 ] 3 4` is `0 0 3 4` and `(_2&}.)^:_1 ] 1 2 3`
  is `1 2 3 0 0`.

**Two unders are not built out of an inverse.** `u&.>` was already one;
`u&.,` is the other. `,` has no obverse — a ravel says nothing about the
shape it came from, and `, b. _1` and `,^:_1` are domain errors in the
reference too — but the shape is in hand while the sentence runs. It gets
its own node rather than a fork of `$`, `$` and `u@,`, because the
reference gives it ONE valence and a fork would have answered dyadically
where the reference raises a valence error. In a 1200-expression sweep it
was four of the five obverse rows still open.

**What is deliberately left.** `!^:_1` is named, not implemented: the
reference's Newton iteration runs in the complex plane (`!^:_1 _1` is
`8.91115j18.2226`), and a real-only iteration answers where the reference
raises a NaN error — `!^:_1 ] 1.5` and `!^:_1 ] 0` among them. It waits on
a complex gamma function, which is the same gap as `! 3j4`. `|.!.f`'s
obverse the reference answers with `]`, an identity that does not undo the
shift it is the obverse of; libjay names the gap rather than copying an
answer it can show is wrong. Both are registered.

**`u b. _1` answers a spelling, and libjay writes its own.** Where the
reference prints `0.318309886183790691&*`, libjay prints `(n&*)`: a
derived verb's name says `n` for a noun operand, everywhere, and that name
is what diagnostics quote. Rather than teach `Verb::name` to render nouns —
which would rewrite every "the obverse of (n&#.)" message and the explain
output with it — the corpus asks by name only the rows the two spell alike,
and asks the rest with `^:_1` and `&.`, which compare values. Three
obverses J spells only as a negative power (`p:^:_1`, `I.^:_1`, `$.^:_1`)
carry that spelling as their primitive name, as `#^:_1` already did, so
those rows do agree by name.

**One table, two languages.** Every row is reachable from APL's `⍣¯1` and
`⍢`, and GNU APL implements no negative power at all, so Dyalog is the only
reference for the APL side; the rows went into `corpus/apl/dyalog-operators.txt`.
Dyalog does not hold three of them — grading is not its own obverse there,
and its own inverse of `○` divides by the argument and so raises DOMAIN
ERROR at zero — and they are exempted by name in `tests/expected/dyalog.txt`
as divergences, on the same rule as `⍢`: a preset chooses a dialect's
rules, it does not withdraw an extension libjay ships in every dialect.

**Measured.** On 1200 depth-5 generated expressions, replayed against
jconsole before and after: 55 mismatches with 15 obverse rows became 42
with 1. The oracle also crashes on `(*:^:2)^:_1 ] 16` — a jconsole
segfault, not a refusal — so that expression is out of the corpus and in
the register.

- 2026-08-23 — Dyalog wave 4: the inner product's each, and the control
  words. Twenty-six of the preset's 71 exempt rows, in two groups.

  **The each moves from the fold to the pairing.** libjay's `f.g` is the
  GNU reading `f/¨ (⊂[last]x) ∘.g (⊂[first]y)`, and the each in it is
  load-bearing: `1 2,.+3 4` is an enclosed `4 6`, one level deeper than the
  fold alone would leave it. The recorded Dyalog answers say the each is on
  the other half. Derived from the rows rather than from a definition:
  `1 2+.,3 4` is `3 7` there and `10` here, so the `,` is not meeting the
  two whole vectors but the two ELEMENTS at each position; and
  `≡1 2,.+3 4` is 2 there and 3 here, so the fold's value is the cell
  rather than something enclosed for an each to collect. Both differences
  are one statement: Dyalog's is `f/ row g¨ column`, GNU's is
  `f/¨ (row g column)`. `(2 2⍴⍳4),.,2 2⍴⍳4` opens with `1 1 2 3` under the
  first and `1 2 1 3` under the second, and every one of the 15 rows —
  including the four Life rows, where the `∨.∧` folds enclosed planes —
  follows from it. Since a scalar `g` pervades, the two readings agree
  wherever `g` is scalar AND the fold ends in a number, which is every
  published use: `+.×` is one sentence in both, and the blocked matrix
  product needed no change. `Dialect.inner_each` names the choice, and only
  the general cell path reads it.

  **`:AndIf` and `:OrIf` are an `:If` in the test.** A test is a block
  whose value is its last sentence's, so a continuation cannot simply be
  appended: `[A, B]` would run B whatever A answered and then take B's
  value. The desugaring is an `If` node standing where the test was —
  `:If A :AndIf B` becomes `if A then B else 0`, `:OrIf` becomes
  `if A then 1 else B` — which short-circuits by construction and chains
  left to right without a precedence rule. `:While`'s test is desugared the
  same way and re-evaluated each iteration, which is what
  `':While R>0' ':AndIf Z<3'` needs.

  **The rest of the control group, and where each rule lives.** `:CaseList`
  is a flag on `Branch` (`list`), read where `Select` compares; `:For a b
  :In` turns `Control::For`'s one optional name into a list, and several
  names take an item apart between them. Two rules are the LANGUAGE and not
  the dialect: `:For` binds an item's contents in APL and its cell in J —
  the same items-versus-cells fork `↑`, `↓` and `≡` already turn on — and a
  control structure may stand outside a definition in APL, where J's
  reference calls one at the top level a spelling error. Two are the
  dialect, because Dyalog refuses what libjay answers:
  `Dialect.control_strictness` makes a condition a single value and gives
  `:Leave` a loop to belong to. And one is neither: a body calling a
  function fixed AFTER it works because the names every `∇` and `⎕FX` in
  the program will define are now collected before anything is parsed, each
  standing as a `Verb::Named` resolved when it is applied. APL settles a
  name's class when the line runs; libjay compiles first, and this is the
  cheapest honest way to keep the two agreeing.

  **What did not move.** GNU APL has no control structures at all — every
  one of these words is a SYNTAX ERROR there — so nothing the GNU column
  records could turn on them, and `record apl --check` confirms it. The
  inner-product change is a dialect setting, so the shipped reading is
  untouched: `--dialect-diff` with no flag went from 216 to 207, and all
  nine are the control words the language now has in both presets.
