# libjay

Independent, embeddable implementations of the [J](https://www.jsoftware.com/)
and APL array languages, as one dependency-light Rust crate. **Not a
DataFrame library** and not a framework: the relationship to your code is the
one PCRE has — a small language inside a string literal, compiled once, run
many times, with the heavy machinery hidden inside.

The Cargo package is `libjay`; the library is `jay` (so the linker artifact
is not `liblibjay`), matching the C `-ljay` and the Python `import jay`.

```sh
cargo add libjay
```

## Compile once, run many times

```rust
use jay::{compile, Array, Data, Dialect, Lang};

let program = compile(Lang::J, "(+/ % #) {x}", &Dialect::default())?;

let x = Array::new(vec![5], Data::F64(vec![3.0, 1.0, 4.0, 1.0, 5.0].into()));
let value = program.run(&[x], &mut |s| print!("{s}"))?;   // Some(2.8) — the mean
```

`{name}` holes become parameters. `program.params` reports them in the order
`run` expects; arguments are positional. `run` returns `Option<Array>` —
`None` when the last sentence yields no value (an assignment, or `echo`/`⎕←`).
The closure is the output sink for `echo` and `⎕←`; stdout is the sandbox
default and no other I/O is open.

`Dialect` carries the host's settings — today APL's `⎕IO`
(`Dialect { index_origin: Some(0) }`); J's index origin is 0 and is not
configurable. `Lang::Apl` selects the other frontend, with its own semantics:
J reduces along the leading axis, APL along the trailing one.

Errors carry a span into the source. `Error::render(source)` renders a
compile error, `Program::render_error` a run error, both the way the CLI
would:

```
length error: shapes do not agree: 3 and 2
  1 2 3 + 1 2
  ^^^^^^^^^^^
```

`jay::fmt::format_array(&value, &program.fmt)` renders a result the way its
language displays it.

## What is inside

Both frontends lower to one language-agnostic IR: an `Expr` tree over a
`Verb` combinator tree (Prim / Rank / Reduce / Fork / Hook / Atop /
Windowed). One generic rank-and-agreement engine executes everything, so
APL's `+/` is simply `Rank(Reduce(+), 1)` and no J assumption reaches the
runtime. A compile-time pass fuses chains of elementwise primitives into one
blockwise kernel, absorbing a trailing full-rank reduction; anything it will
not fuse falls back to the subtree it replaced, so results and error messages
cannot change. Dense arrays of bool / i64 / f64 / characters, row-major.

## Threads

Execution is parallel by default — part of what a compiled expression is,
not a switch the caller flips. Elementwise passes, pure rank cells and
leading-axis reductions split above 65,536 element operations.

A `Program` is immutable, holds no data, and is `Send + Sync`: share one
across threads, or wrap it in an `Arc` and run it concurrently. The pool is
the crate's own, not rayon's global one, so an embedding host that also uses
rayon keeps its pool intact; work started inside a rayon worker stays in that
worker's pool. `LIBJAY_THREADS` sets the size (read once, at first use),
otherwise it is the machine's available parallelism.

An associative float reduction may be regrouped across chunks, which
reorders the rounding; everything else is bit-identical to the sequential
path.

## More

- [Language coverage](https://github.com/amyodov/libjay/blob/main/docs/coverage.md)
  — what each frontend understands today, and the deliberate divergences from
  the reference implementations.
- [Benchmarks](https://github.com/amyodov/libjay/blob/main/bench/README.md) —
  against Polars, numba and numpy.
- [Embedding from C](https://github.com/amyodov/libjay/blob/main/docs/embedding.md)
  — the `libjay-capi` crate builds `libjay.so`/`.dylib` plus `libjay.a` and a
  hand-written `jay.h`.
- [Python surface](https://github.com/amyodov/libjay/blob/main/python/README.md)
  of the same engine.
- [Source and issues](https://github.com/amyodov/libjay).

MIT licensed.
