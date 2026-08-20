# libjay for Python

Independent, modern implementations of the [J](https://www.jsoftware.com/)
and APL array languages, embedded in Python. Not a framework: the
relationship to your code is the one `re` has — a small language inside a
string literal, compiled once, run many times.

```python
import jay

jay.j("+/ 1 2 3 4")        # 10  — "+/" inserts + between the numbers
jay.j("(+/ % #) {x}", {"x": [3.0, 1.0, 4.0, 1.0, 5.0]})   # 2.8 — the mean
```

The mean is written as a *fork*: sum (`+/`) divided by (`%`) count (`#`). No
loops, no axis keyword arguments, no intermediate allocations to name — the
expression *is* the dataflow graph, which is what lets libjay fuse and
parallelise it. `jay.apl` is the same entry point for APL, with its own
semantics (J reduces along the leading axis, APL along the trailing one).

## Install

Not yet on PyPI. Once published:

```sh
uvx libjay -e '(+/ % #) 3 1 4 1 5'      # try the CLI with no install
uv add libjay                            # or: pip install libjay
```

From a checkout today (Rust toolchain required):

```sh
uv venv && uv pip install maturin
uv run maturin develop
```

The names follow the pillow/PIL convention: the *package* (and the CLI) is
`libjay`, the *import* is `jay` — matching Rust (`use jay::`) and C (`-ljay`,
`jay.h`). Wheels are abi3, Python 3.10+, and have no runtime dependencies.

## Compile once, bind data, run

```python
import jay

k = jay.j.compile("+/ {weights} * {data}")
k({"weights": w, "data": chunk1})
k({"weights": w, "data": chunk2})

k2 = k.bind({"weights": w})      # a new kernel; w rides along
k2({"data": chunk3})             # only the changing part at call time
```

`jay.j(...)` is the one-shot form: compile, bind and execute in one call.
Kernels are immutable — `bind` returns a new one — and the compiled program
is shared and safe to run from several threads.

On Python 3.14+, t-strings make the same thing typo-safe — interpolated
values become both the type contract and the defaults:

```python
k = jay.j.compile(t"+/ {weights} * {data}")
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
them, via the Arrow C data interface and `__array_interface__`. libjay is
**not a replacement for Polars or pandas**: you stay in them for everything
tabular and hand libjay the numeric block where the heavy mathematics lives.

```python
import numpy as np, polars as pl
df = pl.DataFrame({"open": [...], "close": [...]})   # M rows × N columns
jay.j("+/ {df}", {"df": df})       # each column summed over all rows
jay.j('+/"1 {df}', {"df": df})     # each row summed

v = jay.j("2 * {x}", {"x": np.arange(10**8)})     # zero-copy in
pl.Series(v)                                      # zero-copy out
```

int64/float64 data (and timestamps/durations, which are physically int64)
crosses the boundary without copying, and the kernel keeps the source alive.
Narrower types widen with one copy. Columns with nulls, tables mixing int64
with float64, and non-contiguous numpy views are refused with an error that
names the column and suggests the cast — where information is missing, libjay
reports and stops rather than guessing on your behalf. The full table of what
is zero-copy, copied, refused and not supported yet is in
[docs/coverage.md](https://github.com/amyodov/libjay/blob/main/docs/coverage.md#the-data-boundary).

## Seeing what an expression became

A compiled expression is not the string you wrote. `+/ % #` is a *fork*;
`+/ w * x` is one blockwise kernel with the sum folded into it. `explain`
prints that structure, one section per sentence:

```python
k = jay.j.compile("+/ {w} * {x}", {"w": [1.0, 2.0, 3.0]})
print(k.explain({"x": [4.0, 5.0, 6.0]}))
```

```
sentence 1  |  +/ {w} * {x}
  fused kernel (1 op: *; +/ absorbed; block 8192)  → scalar float  [kernel ran]
    in 0:
      {x}  → 3 $ float
    in 1:
      {w}  → 3 $ float
    falls back to:
      monad +/
        ...
```

Values follow the same cascade as a call — interpolated, bound, call-time.
With every parameter filled the program is run and each node is annotated
with the shape and dtype it produced, and each fused node with whether its
kernel ran or handed the work back to the chain, and why. With a parameter
missing, the structure is printed alone. `libjay --explain -e '...'` is the
same thing from the shell.

## Device placement

Where an expression runs is separate from what it is bound to. `bind` gives
a kernel data; `deploy` gives it a processor. Both return a new kernel, and
neither changes the answer.

```python
jay.devices()
# [Device(name='AMD Radeon Pro 560', backend='metal',
#         kind='discrete GPU', f64=False)]

k = jay.j.compile("+/ {w} * {x}").bind({"w": w, "x": x})
g = k.deploy("gpu")
g()                                  # the same value, computed on the GPU
```

What reaches the GPU is the fused elementwise chains — the same blockwise
kernels `explain` shows, generated as shader code at run time. Everything
else runs on the CPU, and so does any chain the device cannot take;
`explain` says which and why (`device: gpu`, `device: cpu (…)`). Nothing
here is a separate build: the backend is in the ordinary wheel and is
dormant on a machine with no adapter.

**Precision is not silently traded away.** libjay computes floats in f64,
and most adapters have no f64 in shaders at all — Metal has none. On those
an f64 chain simply stays on the CPU. Single precision is available by
asking for it:

```python
g = k.deploy("gpu", precision="f32")   # yes, I want f32
```

**Data can stay where it is computed.** `upload` returns a value that
carries its own location, so calling a kernel repeatedly over it uploads
nothing after the first time:

```python
g = jay.j.compile("+/ {w} * {x}").deploy("gpu")
pinned = g.bind({"w": g.upload(w), "x": g.upload(x)})
pinned()                              # no upload
```

The one-call shortcut `jay.j("...")` has no device: there is nowhere in one
call to say where, and uploading data for a single run rarely pays for
itself.

## The CLI

```sh
libjay -e '(+/ % #) 3 1 4 1 5'                   # 2.8
libjay -e "⎕←'Hello, world!'" --lang apl         # APL
libjay examples/hello.apl                        # a file; the extension
                                                 # picks the language
libjay --explain -e '+/ {w} * {x}'               # the structure, not a result
```

`.ijs`/`.j` are J, `.apl` is APL; `--lang` overrides. `-e` defaults to J.

## More

- [Runnable examples](https://github.com/amyodov/libjay/tree/main/examples) —
  the glyphs are already in the files, no APL keyboard needed.
- [Language coverage](https://github.com/amyodov/libjay/blob/main/docs/coverage.md)
  — what each frontend understands today.
- [Benchmarks](https://github.com/amyodov/libjay/blob/main/bench/README.md) —
  against Polars, numba and numpy.
- The [Rust](https://github.com/amyodov/libjay/blob/main/crates/libjay/README.md)
  and [C](https://github.com/amyodov/libjay/blob/main/docs/embedding.md)
  surfaces of the same engine, and the
  [source](https://github.com/amyodov/libjay).

MIT licensed.
