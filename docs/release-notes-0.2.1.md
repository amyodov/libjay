<!-- title: v0.2.1 — Inner product, J symbols, quadratic paths made linear -->
# libjay 0.2.1

Both languages gained the inner product, J gained symbols and the last of its text-formatting verbs, and four paths that were correct but quadratic — or worse — now run as the algorithm they describe: a grouped average that took hours takes 1.4 seconds, and an exponential smoothing that would have taken years takes 2.3.

## What changed

**The inner product.** J's `u . v` and APL's `f.g`, for any pair of functions at any rank: `+/ . *` and `+.×` are the matrix product, `*./ . =` and `∧.=` ask which rows match, `<./ . +` and `⌊.+` take a shortest-path step. The numeric matrix product is a blocked, parallel, vectorised pass rather than an interpreted loop: on a 1000×1000 pair of doubles it runs about 2.5× behind numpy's tuned BLAS, and on whole numbers — where BLAS has no path — about 25× ahead of numpy's own loop, keeping whole numbers whole. The monadic form is J's determinant, `-/ . *`, exact over exact numbers.

**J's symbols, `s:`.** A symbol is an atom whose value is a name, and the same text is always the same symbol, so comparing two of them costs one word. ``s: '`red`green`blue'`` makes three from a delimited string; they compare, sort, nub, search and index like any other data, print as `` `red `green `blue ``, and go into boxes; `4 s:` and `5 s:` give the names back as characters. Python receives them as strings. With `s:` landed, exactly one cell in J's primitive tables is still red: sparse arrays, `$.`.

**Four slow paths are now the algorithm they describe, with the same answers.** The suffix scan `u/\.` folds right to left in one pass instead of folding every suffix from scratch, so RSI(14) over 20 million bars went from about nine years of general-dyad steps to 2.3 seconds. The key `u/.` (and APL's `f⌸`) hashes its keys instead of sweeping the key vector once per group, so a VWAP over 20 million minute bars grouped by 13,889 days went from hours to 1.4 seconds. A scan whose step is the fork `[ + c * ]` is recognised as a first-order recurrence and run as `acc = y + c*acc`, ten nanoseconds an element rather than a microsecond. And a reshape whose result the argument's elements already cover shares the buffer instead of copying the ravel, which took a frame-RMS workload from 535 ms to 49 on one thread. Floats come out bit for bit what the old paths gave.

**Folds read narrow data as it lies.** A reduction, scan or moving window over a yes/no column no longer expands it to whole numbers first: at 20 million elements `+/ {b}` runs 24× faster and `>./ {b}` 38×. Elementwise passes over two different element types promote the narrower operand where they read it instead of copying it wide first: `{c} + {f}` 2.5× faster, `+/ {i} * {f}` 5.5×. Results are unchanged to the last bit. See bench/README.md, "Mixed-type passes" and "Folds over one buffer".

## Also in this release

### Language coverage

- APL's `⍠`, the variant operator: one setting of the dialect overridden for one application — `1 (=⍠0) 1+1E¯14` compares exactly, `⍳⍠('IO' 0)` counts from zero inside a program that counts from one.
- J's sequential machine, the dyad of `;:`: a table-driven tokeniser in all six of its result forms.
- J's format by specification, the dyad of `":`: field widths and decimals per column, the exponential form, and the reference's asterisks for a value that does not fit.
- J's number reading, the dyad of `".`: the numbers a line of text spells, with a stand-in of your choosing for anything that is not one.
- Dyadic `I.` searches character and symbol lists as well as numeric ones.
- APL's `⌶` (I-beam) and `&` (spawn) moved from "not yet" to "absent by design" — the first has no published behaviour to follow, the second starts a thread the sandbox does not open. Nothing left in APL's primitive tables is a promise.

### Measured workloads

bench/workloads.md now holds twelve end-to-end jobs — OHLCV finance and DSP/audio — each written four ways (libjay J, libjay APL, Polars, numba) and measured, so the comparison is on work someone would actually run, not microbenchmarks. The three losses that table diagnosed are the three algorithm fixes above.

### Supply chain

The build is now gated by cargo-deny: advisories (yanked crates refused), an explicit ban list carrying the crate versions poisoned in this August's crates.io attack (`arrayref` 0.3.10, `internment` 0.8.7, `append-only-vec` 0.1.9, and the `proc-macro1` typosquat), a license allowlist, and registry-source pinning. The lockfile's entire history was audited clean of all four before the gate went in.

### Benchmarking farther afield

bench/cloud/ holds a design, scripts and IAM policies for one-shot rented spot-instance runs — AVX-512, ARM with SVE, and an NVIDIA GPU, the three machines this project's numbers have never been taken on. It is a design: nothing in it has been executed, and every script refuses to start until the owner's account details are filled in.

## Status

J: 151 of 177 published valences implemented, 23 partial, 1 not yet (sparse arrays, `$.`), 2 refused by design.

APL: 90 of 116 published valences implemented, 23 partial, 3 refused by design — none missing.

The full, per-spelling matrix is [docs/status.md](status.md).

## What is still not here

- **Sparse arrays.** J's `$.`, the one red cell left; in progress.
- **One APL.** Dyalog-specific behaviour is a planned dialect switch behind the existing `Dialect` object, not a supported reading today.
- **AVX-512 and the GPU f64 path are still unmeasured** — no machine that has run them has the hardware; the cloud design above exists to close exactly this.
- **The C ABI copies its input** and has no descriptor for boxed, extended or rational results; those are refused by name.
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
- [Benchmarks](../bench/README.md), [measured workloads](../bench/workloads.md)
- [Python](../python/README.md), [Rust](../crates/libjay/README.md) and [C](embedding.md) surfaces
- [Examples](../examples/) — runnable, glyphs already in the files
