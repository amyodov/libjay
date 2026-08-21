# libjay 0.1.0

Independent, modern implementations of the J and APL array languages — as much of each language as possible — built to be embedded: a Rust crate, a Python wheel, and a stable C ABI for everything else. The relationship to your code is the one `re` has: a small language inside a string literal, compiled once, run many times.

```python
import jay

jay.j("+/ 1 2 3 4")                                        # 10
jay.j("(+/ % #) {x}", {"x": [3.0, 1.0, 4.0, 1.0, 5.0]})    # 2.8 — the mean
jay.apl("+⌿2 3⍴⍳6")                                        # [5 7 9]
```

This is the first release. Nothing here is a port or a wrapper: both frontends were written from the published documentation and lower to one language-agnostic IR, executed by one generic rank-and-agreement engine.

## What works

**The languages.** 135 of J's 180 published valences are implemented, 26 more partially, and 79 of APL's 115 with 25 more partial; the rest report "not implemented yet" by name rather than guessing. Trains, tacit and explicit definitions, control structures, dfns, boxes, complex numbers, and J's exact types — arbitrary-precision integers and rationals. The full, per-spelling matrix is [docs/status.md](status.md).

**Correctness is measured, not asserted.** 3816 J and 1024 APL expressions are recorded from black-box runs of the reference interpreters and replayed on every test run — no interpreter is needed to run the suite. libjay agrees with the references everywhere except 29 APL sentences where it diverges on purpose, each recorded with its reason.

**Speed is the point of writing it as an expression.** A 20-period Bollinger z-score, written as one J sentence, runs 20M rows in 404 ms where the equivalent Polars pipeline takes 755, and the two agree to 8.7e-10. Expressions are fused into blockwise kernels, parallelised across the crate's own thread pool, and dispatched to the widest SIMD level the CPU reports at run time. Fused kernels can also be placed on a GPU (`kernel.deploy("gpu")`, wgpu over Metal/Vulkan/DX12, in the ordinary wheel and dormant without an adapter); with data left resident, that is 1.4x to 7.3x the 8-thread CPU at 20M rows. Method and full tables: [bench/README.md](../bench/README.md).

**Real data crosses without copying.** Polars, pandas 2, PyArrow and numpy work natively through the Arrow C data interface and `__array_interface__`, with no dependency on any of them. Nulls, mixed-type columns and non-contiguous views are refused with an error that names the column and the fix — libjay reports and stops rather than guessing on your behalf.

## What is not here

- **One APL.** The APL2/ISO line that GNU APL embodies. Dyalog-specific
behaviour is a planned dialect switch, not a supported reading today.
- **Named gaps in both vocabularies**: dyadic transpose, J's `{` catalogue
and `e.` raze-in, format by specification, symbols, sparse arrays, locales and the foreign conjunction, adverb and conjunction assignment; APL's collating grade, `⍞`, I-beam, `⍢`, `⌺`, `⍠`, `&`, function assignment and trains.
- **The GPU f64 path has never been executed.** It is generated and
type-checked, but the measuring machine's Metal adapter has no `SHADER_F64`, and no other adapter has been in front of it yet. On such an adapter an f64 chain stays on the CPU rather than quietly computing in f32.
- **The C ABI copies its input** and has no descriptor for boxed, extended
or rational results; those are refused by name.
- Arrow string, binary, list and dictionary columns; Decimal128; float16.

## Try it

```sh
uvx libjay -e '(+/ % #) 3 1 4 1 5'              # 2.8, nothing installed
uvx libjay -e "⎕←'Hello, world!'" --lang apl
uv add libjay                                   # or: pip install libjay
```

Wheels are abi3, Python 3.10+, with no runtime dependencies: linux x86_64/aarch64, macOS x86_64/aarch64, windows x64, plus an sdist. Rust users `cargo add libjay`; C users take a `libjay-capi-<triple>.tar.gz` bundle from the assets below, which carries `jay.h` and that platform's shared and static libraries.

## Links

- [Changelog](../CHANGELOG.md)
- [Status matrix](status.md) — what is implemented, feature by feature
- [Language coverage](coverage.md) — what each frontend understands, and the
data boundary
- [Benchmarks](../bench/README.md)
- [Python](../python/README.md), [Rust](../crates/libjay/README.md) and
[C](embedding.md) surfaces
- [Examples](../examples/) — runnable, glyphs already in the files
