<!-- title: v0.3.0 — The Dyalog dialect, sparse arrays, nested arrays done right -->
# libjay 0.3.0

APL now speaks two dialects — the APL2/GNU line it always followed, and Dyalog's, as a preset verified against a running Dyalog 20 — J gained sparse arrays and an obverse table asked of the reference verb by verb, and the nested-array machinery was corrected deeply enough that John Scholes' Game of Life runs byte-for-byte.

## What changed

**The Dyalog dialect.** `Dialect::dyalog()` in Rust, `APL.Dialect.dyalog` in Python, `--dialect dyalog` on the command line. It swaps monadic `↑`/`⊃` (mix and first), reads `⌷` by leading axes, sets `⎕CT` to `1e¯14` with Dyalog's own floor, ceiling, encode and count readings, fixes functions with `⎕FX`, and gives dfns Dyalog's full model: guards, defaults, real lexical scoping, operators taking value operands, and `f⍣¯n` through the obverse table. Every semantic was derived from 2,012 recorded answers of a real Dyalog 20 — not from documentation — and the preset is held to those recordings by its own gate in the test suite: 1,941 agree, and each of the 71 that do not carries a named reason (23 deliberate divergences, 48 open gaps). The GNU/APL2 reading stays the default and is bit-for-bit unchanged.

**Sparse arrays.** J's `$.` — the last storage kind — with the monad, nine dyad forms, J's index/value display, and `3!:0`'s sparse type codes, exact against jconsole on every corpus row. With it, nothing in either language's primitive tables is red: every published spelling either works, works with a named caveat, or is refused by design. Sparseness is honest about its limit: `$.`'s own forms preserve it, other verbs answer through the dense value.

**Nested arrays done right.** APL applies its operators between the ITEMS of nested arguments — disclosed on the way in, enclosed on the way out — and now libjay does too, across `∘.f`, `f/`, `f⌿`, `f\`, `f⍀` and `f.g`. The visible casualty of the old reading was the canonical Game of Life one-liner; it now matches GNU APL byte for byte, as does the nested display's text-run spacing. Expand, replicate and first-of-empty recovered their prototype fills, and empty results keep the rank their frame promises.

**APL grew n-wise reduction and mixed arrays.** `2+/1 2 3` is `3 5` — windows of adjacent items, at every edge the reference defines. `1 2,'ab'` builds the mixed simple vector GNU APL builds, prints with its text-run rule, and flows through `⍪ ∪ ∩ ~ ⍷ ∊ ⍳ ≡`.

## Also in this release

### Language coverage

- The obverse table was rederived by asking jconsole `u b. _1` for every spelling: 42 inverse rules for `u&.v`, `u&.:v`, `u^:_1` and APL's `f⍣¯n` — including under-each, under-ravel, running-fold inverses and thirteen bonds — and 36 refusals pinned exactly where the reference refuses. One long-standing silent inversion bug (`(2&%:)^:_1`) died on the way.
- Tolerance is consulted where the references consult it, primitive by primitive: residue rounds its quotient (J scales by the dividend, APL by the modulus), APL's grade sorts tolerantly where J's stays exact, GCD/LCM terminate tolerantly, and a near-integer float is the count it stands for — under three different rules, one per reference, each probed to its boundary.
- `s:` symbols, the inner product `u . v` / `f.g`, `⍠` variant, the sequential machine, format and number reading by specification (carried forward from the 0.2.1 line's tail).

### Correctness

A three-layer differential hunt — a perpetual random sweeper, hypothesis-driven adversarial probing, and cluster triage — ran both languages against the live references throughout. The wave closed about fifty real defects, among them eight panics (one an attempted 8 EB allocation reachable from a format width), every known silent-wrong-data case (rotate conformability, transpose under cut, nub sieve's shape, `,.`'s rank), NaN/∞ discipline in both languages (J's `0 * _` is 0 by the factor rule; APL's `÷0` refuses), and empty-argument checks that fired where the references answer. Where GNU APL or jconsole is provably inconsistent with itself, libjay follows the published rule and records the difference — the corpus now carries every such pinned divergence with its reason.

### Testing

`cargo test` now replays three recorded oracles — jconsole, GNU APL, and Dyalog 20 — as hard gates; the corpus grew by roughly 2,500 expressions to ~7,600, including four Dyalog-only themes (dfns, dops, control structures, operators) recorded from the official Dyalog Docker image. A coverage tool measures which primitive × type × rank cells the corpus actually exercises, and the fuzzer's second-generation grammar composes the axes the first never reached.

## Status

J: 151 of 177 published valences implemented, 24 partial, 2 refused by design — none missing.

APL: 93 of 117 published valences implemented, 21 partial, 3 refused by design — none missing.

The full matrix is [docs/status.md](status.md); the Dyalog preset's remaining 71 rows are itemised there by cause.

## What is still not here

- **Dyalog is not the default**, and its 48 named gaps include the inner product over nested operands, shy results, `:AndIf`/`:CaseList`, and `⎕R`/`⎕S`.
- **Sparseness does not survive other verbs** — the answers are right, the storage kind densifies.
- **The C ABI copies its input** and has no descriptor for boxed, extended or rational results.
- AVX-512 and the GPU f64 path remain unmeasured for want of hardware.
- Arrow string, binary, list and dictionary columns; Decimal128; float16.

## Install

```sh
uvx libjay -e '(+/ % #) 3 1 4 1 5'                          # 2.8, nothing installed
uvx libjay --lang apl -e '⎕←2 3⍴⍳6'
uvx libjay --lang apl --dialect dyalog -e '↑(1 2)(3 4)'     # the other APL
uv add libjay                                               # or: pip install libjay
```

Wheels are abi3, Python 3.10+, no runtime dependencies: linux x86_64/aarch64, macOS x86_64/aarch64, windows x64, plus an sdist. Rust users `cargo add libjay`; C users take a `libjay-capi-<triple>.tar.gz` bundle from the release assets.

## Links

- [Changelog](../CHANGELOG.md)
- [Status matrix](status.md) — including the Dyalog preset's ledger
- [Language coverage](coverage.md) — "Which APL" explains the two dialects
- [Benchmarks](../bench/README.md), [measured workloads](../bench/workloads.md)
- [Python](../python/README.md), [Rust](../crates/libjay/README.md) and [C](embedding.md) surfaces
- [Examples](../examples/) — runnable, glyphs already in the files
