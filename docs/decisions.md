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
