<!-- title: v0.2.0 — Zero-copy DataFrame columns, fused windows, APL trains -->
# libjay 0.2.0

A DataFrame no longer costs a copy to read, a moving window now joins the fused kernel around it, APL gained trains and function assignment, J gained explicit adverb and conjunction definitions, and both languages can now read from the sandbox's open stdin as well as write to it.

## What changed

**A DataFrame no longer costs a copy to read.** Its columns cross the boundary borrowed, one Arrow buffer each, and libjay folds them where they lie: `+/ df` (column sums) and `+/"1 df` (row sums) over a 2.5M x 8 table are 10 and 5 times faster end to end, and reading the table's shape costs nothing at all. Programs that need the elements in reading order — ravelling, indexing, printing — still pay for one copy, now at the point they ask for it instead of on every call.

**A moving window is now a step of the fused kernel, not a pass of its own.** `k +/\ y`, `k >./\ y`, `k <./\ y` and `+/\ y` used to break the chain they stood in; a rolling expression now reads its argument once instead of once per window and once per arithmetic step, with results unchanged to the last bit. The 20-period Bollinger z-score that has been this project's speed headline since 0.1.0 runs 20M rows in 231 ms now, against Polars' 787 — before this change it was 404 ms against 755, so the gap to Polars roughly tripled in libjay's favour on the same machine.

**APL gained trains and function assignment.** A run of bare functions now reads as a fork or an atop — `(f g h)` applies `f` and `h` to the argument and combines the results with `g`; `(g h)` applies `h` then `g`. A plain number may stand in a fork's left position, and `⊢`/`⊣` mean "the argument itself". A derived function or a whole train can then be named — `F←+/`, `F←+/÷≢` — and applied like any other function. This is an extension beyond strict APL2/GNU APL, on by default, with the strict reading available as an option.

**J's modifiers are first-class.** An adverb or conjunction can now be named on its own (`m =. /`, `c =. @`), not only a verb, and J can define its own: `1 : '…'` and `2 : '…'`, their multi-line forms, and the `{{ … }}` direct-definition syntax, matching J's published vocabulary for writing them.

**The sandbox can now read, not just write.** APL's `⍞` (one line of text) and `⎕` (one line evaluated as APL), and J's `1!:1 ]1` and `3!:0` (a value's storage type), all read from the same standard input the host provides — piped, typed, or supplied by the embedding application. Every language surface (Rust, Python, C, and the command line) gained the matching call, alongside the existing output calls. The rest of J's `!:` foreign conjunction that would reach a file, the system clock, or another process is still refused, with a "closed by the sandbox" message distinct from "not supported yet".

## Also in this release

### Language coverage

- J's `L:` and `S:` now take two arguments as well as one.
- J's `H.`, the generalised hypergeometric series.
- J's gerunds are ordinary data now, exactly as the language has them: a tie such as `` +`- `` produces a boxed value you can name, print, add to, and build by hand. `` `: `` (evoke gerund) works in all three of its forms — apply each verb and collect the answers, insert the verbs between the items, or read the gerund as a train.
- Dyadic transpose in both languages: J's `1 0 |: m` and APL's `2 1⍉m`, including the diagonal forms `` (<0 1)|: `` and `1 1⍉`.
- J's monadic `{` (catalogue: every combination of one element from each item) and monadic `e.` (raze-in).
- J's `_.`, the indeterminate value.
- J's `u b. 1` and `u b. _1`: what a verb's identity element and inverse are spelled as.
- J's `^:` accepts a list of counts, and the boxed forms that collect every intermediate result — `u^:(<n)` and `u^:a:`.
- J's tessellation `;.3` accepts a negative block size, which reverses that axis, where the movement row is written out.
- APL's `⍢` (under) and `⌺` (stencil), and the collating grades `x⍋y` and `x⍒y`.
- Grading and sorting whole arrays of boxes: J's `/:` and `\:` order boxed items by J's total array ordering (type class, then rank, then shape, then contents), and APL's `⍋`/`⍒` order nested arrays as APL2 does — both derived from the reference interpreters.

### Execution

- Faster execution on newer x86-64 processors, using the CPU's AVX-512 instructions when present; picked up automatically at startup, with an explicit override available. Not yet benchmarked on real AVX-512 hardware — no machine on hand has it.
- Transposing an array (`|:`, `⍉`) no longer moves any elements, at any rank.

### Data boundary

- A Fortran-ordered numpy block — `np.asfortranarray(a)`, or the `.T` of an ordinary one — is read where it lies instead of being refused with a request to copy it. Views that are contiguous in neither order (strided slices, sub-blocks, partial axis permutations) are still refused, with the same message.

### Diagnostics

- Refusals that come from the sandbox (closed I/O, the system clock, threads) are now labelled distinctly from "not supported" and "not part of the language", so it reads as a deliberate boundary rather than a missing feature.

### Build

- Minimum required Rust version raised to 1.89: needed for the AVX-512 support above, and for wgpu 30 (the GPU backend's dependency), which needs a newer compiler than the previous floor; pinned in the repository so every build uses the same compiler.
- Third-party dependencies (the GPU backend, Python bindings, and test tooling) updated to their latest versions; no user-visible change.

### Fixed

A fuzz sweep of 27,000 composed sentences per language, triaged against jconsole and GNU APL, found fifteen more disagreements on top of the seven already known; the reference won every time the two disagreed. All twenty-two are fixed in this release.

- `(2&+)^:_1` and `(2&*)^:_1` computed the wrong inverse — the bonded number was applied from the left instead of taken off the right, so `(2&+)^:_1 5` answered `¯3` where J answers 3. Everything that undoes a verb (J's `&.`, `^:_1`, `u b. _1` and APL's `⍢`) is corrected by it.
- APL's `⊥` on an argument of rank 2 or more folded the wrong axis: it is an inner product and folds the LEADING axis of its right argument. Vectors, the common case, were always right.
- APL's `⍸` (interval index) placed a value exactly equal to a bound in the wrong interval: `1 3 5⍸3` is 2, not 1. J's `I.`, whose interval is open on that side, is unchanged.
- APL's `⌷` accepts an enclosed vector as an index, so `(⊂1 2)⌷5 6 7 8` is `5 6`.
- APL's `∊` finds a scalar held in a nested right argument: `1 2 3∊(1 2)(3)` is `0 0 1`.
- Two rarely-used APL operators (variant, I-beam) that aren't implemented yet are now reported by name as "not supported yet" instead of as an unrecognized character.
- APL operator precedence: a parenthesised function now binds before an operator to its right, matching the reference implementation — `(+)/1 2 3` evaluates to 6.
- APL's scalar functions reach inside a nested argument, as APL2 has them: `(1 2)(3 4)+1` is `(2 3)(4 5)`, and every arithmetic, comparison and logical function pervades to the simple values at the bottom. They used to refuse a nested argument outright.
- APL's `⊥` over an empty radix axis crashed the printer: `(⍳0)⊥1 2 3` now answers 0.
- APL scalar extension between two frames of one cell kept the wrong one, so `⍴(,5)+¯3` was empty where it is `1`.
- Take and drop count axes, not elements: more counts than the argument has axes is a length error in both languages, and APL wants exactly one count per axis where J is content with fewer.
- APL's replication extends an argument of one item along the axis, as it extends a scalar: `2 0 1/,5` is `5 5 5`.
- APL's dyadic `∪`, `∩` and `~` take vectors, as GNU APL has them; a grade needs an array rather than a scalar; and `≡` tells an empty character array from an empty numeric one.
- `E.`/`⍷` search every axis at once and answer in the shape of the right argument, so a table is found inside a table.
- J's LCM and GCD accept numbers that are not whole: `1.23 +. 4.56` is `0.03`.
- J's `#.` accumulates in the exact types when it is given them, so a 19-digit integer keeps every digit.
- J's `m&v` and `u&n` apply to the whole argument, not atom by atom: `1 2&+ 1 2` is `2 4`, not a two-by-two table.
- J's `p.` answers `0 ; ''` for the zero polynomial instead of refusing it, and `j.` has an obverse, so `+/&.:j.` works.
- An empty array inside a box keeps its shape on screen: `<0 3⍴0` draws a cell three wide with no lines in it.

## Status

J: 148 of 177 published valences implemented, 20 partial, 7 not yet, 2 refused by design.

APL: 89 of 115 published valences implemented, 22 partial, 4 not yet.

The full, per-spelling matrix is [docs/status.md](status.md).

## What is still not here

- **One APL.** The APL2/ISO line that GNU APL embodies. Dyalog-specific behaviour is a planned dialect switch, not a supported reading today.
- **AVX-512 is unmeasured.** The x86-64-v4 dispatch rung is built into every x86-64 artifact and symbol-checked, but no machine that has run it has the hardware; whether it is faster than v3 is still an open question.
- **The GPU f64 path has never been executed.** It is generated and type-checked, but the measuring machine's Metal adapter has no `SHADER_F64`, and no other adapter has been in front of it yet. On such an adapter an f64 chain stays on the CPU rather than quietly computing in f32.
- **The C ABI copies its input** and has no descriptor for boxed, extended or rational results; those are refused by name.
- Named gaps remain in both vocabularies: J's format by specification, `".` numbers, symbols, sparse arrays, locales, multiple assignment, and the parts of `!:` that only compute; APL's I-beam, `⍠` variant, `&` spawn.
- Arrow string, binary, list and dictionary columns; Decimal128; float16.

## Install

```sh
uvx libjay -e '(+/ % #) 3 1 4 1 5'              # 2.8, nothing installed
uvx libjay -e "⎕←'Hello, world!'" --lang apl
uv add libjay                                   # or: pip install libjay
```

Wheels are abi3, Python 3.10+, with no runtime dependencies: linux x86_64/aarch64, macOS x86_64/aarch64, windows x64, plus an sdist. Rust users `cargo add libjay`; C users take a `libjay-capi-<triple>.tar.gz` bundle from the release assets, which carries `jay.h` and that platform's shared and static libraries.

## Links

- [Changelog](../CHANGELOG.md)
- [Status matrix](status.md) — what is implemented, feature by feature
- [Language coverage](coverage.md) — what each frontend understands, and the data boundary
- [Benchmarks](../bench/README.md)
- [Python](../python/README.md), [Rust](../crates/libjay/README.md) and [C](embedding.md) surfaces
- [Examples](../examples/) — runnable, glyphs already in the files
