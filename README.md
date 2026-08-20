# libjay

Independent, modern implementations of the [J](https://www.jsoftware.com/)
and APL array languages — as much of each language as possible, embeddable
from Rust, Python and anything with a C FFI. Expressions compile from string
literals and run in parallel. Not a framework: the relationship to your code
is the one `re` has — a small language inside a string literal, compiled
once, run many times.

```python
import jay

jay.j("+/ 1 2 3 4")        # 10  — "+/" inserts + between the numbers
jay.j("(+/ % #) {x}", {"x": [3.0, 1.0, 4.0, 1.0, 5.0]})   # 2.8 — the mean
```

That last expression is the arithmetic mean, written as a *fork*: sum (`+/`)
divided by (`%`) count (`#`). No loops, no axis keyword arguments, no
intermediate allocations to name — the expression *is* the dataflow graph,
which is what lets libjay fuse and parallelise it for you.

Both J (ASCII) and APL (Unicode) compile to one shared representation. They
are real, independently implemented languages — same semantics you'd find in
their documentation, including where they disagree with each other:

```python
jay.j("+/ i. 2 3")         # [3 5 7]  — J sums along the LEADING axis
jay.apl("+/2 3⍴⍳6")        # [6 15]   — APL sums along the TRAILING axis
jay.apl("+⌿2 3⍴⍳6")        # [5 7 9]  — APL's leading-axis sum
```

## Try it

Not yet on PyPI. From a checkout (Rust toolchain required):

```sh
uv venv && uv pip install maturin
uv run maturin develop
uv run libjay -e '(+/ % #) 3 1 4 1 5'                   # 2.8
uv run libjay -e "⎕←'Hello, world!'" --lang apl         # APL
uv run libjay examples/hello.apl                        # or run a file
```

Once published, `uvx libjay -e '...'` will do all of that with no setup.

## Where next

| | |
|---|---|
| Using it from **Python** | [python/README.md](python/README.md) |
| Using it from **Rust** | [crates/libjay/README.md](crates/libjay/README.md) |
| Using it from **C**, or any language with a C FFI | [docs/embedding.md](docs/embedding.md) |
| Runnable examples — no APL keyboard needed | [examples/](examples/) |
| What's implemented, feature by feature (🟢🟡🔴) | [docs/status.md](docs/status.md) |
| What each language covers, and the data boundary | [docs/coverage.md](docs/coverage.md) |
| Honest numbers against Polars, numba and numpy | [bench/README.md](bench/README.md) |

## Status

Early. What's implemented, feature by feature, is the
[status matrix](docs/status.md): 82 green / 14 partial / 87 red of 183 J
valences, 59 green / 16 partial / 41 red of 116 APL valences. Both primitive
sets are differential-tested against the reference implementations: 2942
J and 676 APL expressions, recorded as snapshots from black-box runs of the
reference interpreters and replayed on every test run, 100% agreement. A
20-period Bollinger z-score written as one J kernel runs 20M rows in 437 ms
against the equivalent Polars pipeline's 768, agreeing to 8.7e-10. Dense numeric arrays and boxes; things the languages have but
libjay doesn't yet (bigints, rationals, complex numbers, a GPU backend, …)
fail with an explicit "not supported yet". Next on the roadmap: more of
both languages, GPU.

## License

MIT.
