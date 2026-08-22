# libjay

[![PyPI](https://img.shields.io/pypi/v/libjay)](https://pypi.org/project/libjay/)
[![crates.io](https://img.shields.io/crates/v/libjay)](https://crates.io/crates/libjay)
[![CI](https://github.com/amyodov/libjay/actions/workflows/ci.yml/badge.svg)](https://github.com/amyodov/libjay/actions/workflows/ci.yml)

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

No setup, no Rust toolchain — the wheels are prebuilt:

```sh
uvx libjay -e '(+/ % #) 3 1 4 1 5'                      # 2.8
uvx libjay -e "⎕←'Hello, world!'" --lang apl            # APL
uv add libjay                                           # or: pip install libjay
```

From a checkout (Rust toolchain required; `rust-toolchain.toml` names the
version, and rustup installs it on the first build):

```sh
uv venv && uv pip install maturin
uv run maturin develop
uv run libjay -e '(+/ % #) 3 1 4 1 5'                   # 2.8
uv run libjay examples/hello.apl                        # or run a file
```

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
| Whole workloads — RSI, VWAP, drawdown, RMS — four ways | [bench/workloads.md](bench/workloads.md) |

## Status

Early — 0.1.0 is the first release; what has landed since is in
[CHANGELOG.md](CHANGELOG.md). What's implemented, feature by feature, is the
[status matrix](docs/status.md): 146 green / 22 partial / 7 red / 2 absent by
design of 177 J valences, 87 green / 24 partial / 4 red of 115 APL valences.
Both primitive sets are differential-tested against the reference
implementations: 4028 J and 1155 APL expressions, recorded as snapshots from
black-box runs of the reference interpreters and replayed on every test run.
The two agree everywhere except 32 APL sentences where libjay diverges on
purpose, each recorded with the reason. A 20-period Bollinger z-score written
as one J kernel runs 20M rows in 404 ms against the equivalent Polars
pipeline's 755, agreeing to 8.7e-10. The data model is dense numeric arrays,
complex numbers, boxes, J's exact types — extended-precision integers and
rationals — and its two other storage kinds, symbols and sparse arrays; what
a language has and libjay does not yet (Decimal128 and Arrow
string/binary/list/dictionary columns among them) fails with an explicit
"not supported yet" rather than a wrong answer.
Fused kernels can be placed on a GPU (`kernel.deploy("gpu")`, wgpu over
Metal/Vulkan/DX12, in the ordinary wheel and dormant without an adapter);
resident data runs 1.4x to 7.3x the 8-thread CPU at 20M rows. Next on the
roadmap: more of both languages. The APL implemented today is the APL2/ISO
line, verified against GNU APL; Dyalog-specific behaviour is a planned
dialect switch (see [docs/coverage.md#which-apl](docs/coverage.md#which-apl)).

## License

MIT.
