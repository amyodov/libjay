# libjay

Independent, embeddable implementations of the [J](https://www.jsoftware.com/)
and APL array languages. **Not a DataFrame library** — it does not replace
Polars or pandas; you stay in them for everything tabular and hand libjay the
numeric block where the heavy mathematics lives. Not a framework either: the
relationship to your code is the one `re` has — a small language inside a
string literal, compiled once, run many times.

```python
import libjay

libjay.j("+/ 1 2 3 4")        # 10  — "+/" inserts + between the numbers
libjay.j("(+/ % #) {x}", {"x": [3.0, 1.0, 4.0, 1.0, 5.0]})   # 2.8 — the mean
```

That last expression is the arithmetic mean, written as a *fork*: sum (`+/`)
divided by (`%`) count (`#`). No loops, no axis keyword arguments, no
intermediate allocations to name — the expression *is* the dataflow graph,
which is what will let libjay parallelise it for you.

## Why

Array languages describe whole-array computation declaratively; that makes
them compilable and parallelisable in a way imperative NumPy code is not. But
nobody wants to adopt an interpreter, an IDE and a keyboard layout to get
that. libjay embeds the languages instead, the way PCRE embedded regular
expressions: a compile/execute split, aggressive machinery hidden inside, a
thin boring surface outside.

Both J (ASCII) and APL (Unicode) compile to one shared representation.
They are real, independently implemented languages — same semantics you'd
find in their documentation, including where they disagree with each other:

```python
m = libjay.j("i. 2 3")        # 2x3 matrix: 0 1 2 / 3 4 5
libjay.j("+/ i. 2 3")         # [3 5 7]  — J sums along the LEADING axis
libjay.apl("+/2 3⍴⍳6")        # [6 15]   — APL sums along the TRAILING axis
libjay.apl("+⌿2 3⍴⍳6")        # [5 7 9]  — APL's leading-axis sum
```

## Try it

Not yet on PyPI. From a checkout (Rust toolchain required):

```sh
uv venv && uv pip install maturin
uv run maturin develop
uv run libjay -e "echo 'Hello, world!'"                 # J
uv run libjay -e "⎕←'Hello, world!'" --lang apl         # APL
uv run libjay -e '(+/ % #) 3 1 4 1 5'                   # 2.8
```

Or run a file — the extension picks the language (`.ijs`/`.j` → J,
`.apl` → APL):

```sh
uv run libjay examples/hello.apl
```

Once published, `uvx libjay -e '...'` will do all of the above with no setup.

## Compile once, bind data, run

```python
import libjay

k = libjay.j.compile("+/ {weights} * {data}")
k({"weights": w, "data": chunk1})
k({"weights": w, "data": chunk2})

k2 = k.bind({"weights": w})      # a new kernel; w rides along
k2({"data": chunk3})             # only the changing part at call time
```

On Python 3.14+, t-strings make the same thing typo-safe — interpolated
values become both the type contract and the defaults:

```python
k = libjay.j.compile(t"+/ {weights} * {data}")
k()                              # computes on the interpolated samples
k({"data": other})               # override at call time
```

Braces always mean *data binding*, never splicing text into the program.

Errors point into your expression, in both languages:

```
length error: left shape 2, right shape 3
  1 2 + 1 2 3
      ^
```

## Real data, zero-copy

Polars, pandas 2, PyArrow and numpy work natively — no dependency on any of
them, via the Arrow C data interface and the buffer protocols:

```python
import polars as pl
df = pl.DataFrame({"open": [...], "close": [...]})   # M rows × N columns
libjay.j("+/ {df}", {"df": df})       # each column summed over all rows
libjay.j('+/"1 {df}', {"df": df})     # each row summed

v = libjay.j("2 * {x}", {"x": np.arange(10**8)})     # zero-copy in
pl.Series(v)                                         # zero-copy out
```

int64/float64 data (and timestamps/durations, which are physically int64)
crosses the boundary without copying, and the kernel keeps the source
alive. Columns with nulls, or tables mixing int64 with float64, are refused
with an error that names the column and suggests the cast — where
information is missing, libjay reports and stops rather than guessing.

## Status

Early. What exists today: both frontends over one IR; arithmetic, reduction
(with the leading/trailing axis semantics of each language), the rank
operator (J `"`, APL `⍤`), iota/index origin, reshape/transpose/take/drop,
assignment and multi-sentence programs, `echo`/`⎕←`, the CLI, and the Arrow
data boundary above. Dense numeric arrays only; things the languages have
but libjay doesn't yet (boxes, nested arrays, scan, windows, …) fail with an
explicit "not supported yet".

Next on the roadmap: multithreaded execution benchmarked against Polars and
numba, time-series primitives, SIMD, GPU. Deeper documentation lives in
`docs/`.

## License

MIT.
