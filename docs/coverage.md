# Language coverage

What each frontend understands today. Everything a language has that libjay
does not yet implement fails with an explicit "not supported yet" naming the
feature — that is a promise, not a refusal.

## Common semantics (one IR)

- Dense arrays of booleans, 64-bit integers, 64-bit floats, characters.
  Integer arithmetic promotes to float on overflow, as J does.
- Verb rank machinery with frames, J framing fill (shorter cells extended by
  leading 1-axes, padded with 0 / space).
- Dyadic agreement is per-language: J leading-prefix agreement (a 2×3 matrix
  pairs with a 2-vector row-wise), APL exact-shape-or-scalar.
- A sequence's value is its last sentence's; a sentence that is an
  assignment (or `⎕←`) yields no value. Inline assignment inside an
  expression yields the assigned value.
- Errors carry the source span; shape errors report both shapes and the
  first diverging axis.

## J

| Spelling | Monadic | Dyadic |
|---|---|---|
| `+` | conjugate (identity on reals) | plus |
| `-` | negate | minus |
| `*` | signum | times |
| `%` | reciprocal | divide (float; `0%0` is `0`, `n%0` is `_`) |
| `^` | exponential | power |
| `%:` | square root | — |
| `\|` | magnitude | residue |
| `<.` | floor | min |
| `>.` | ceiling | max |
| `=` `<` `>` `<:` `>:` | — | comparisons (0/1) |
| `$` | shape of | reshape (cyclic) |
| `,` | ravel | — |
| `#` | tally | — |
| `{.` | head | take (negative = from end; overtake fills) |
| `}.` | behead | drop |
| `\|:` | transpose (reverse axes) | — |
| `i.` | integers (negative axis = reversed) | — |
| `]` `[` | same | right / left |
| `echo` | print (formatted) | — |

Adverb `/` (insert/reduce, leading axis, right-to-left fold), conjunction
`"` (rank, 1–3 atoms, `_` = infinite), `@:` (atop), `[:` (cap). Trains:
forks `(f g h)`, noun forks `(n g h)`, hooks `(f g)`. Assignment `=.`/`=:`
(one environment for now), multi-sentence programs, `NB.` comments,
`'strings'`, `_`/`__` infinities, `1e_3` exponents.

## APL

| Glyph | Monadic | Dyadic |
|---|---|---|
| `+` | identity | plus |
| `-` | negate | minus |
| `×` | signum | times |
| `÷` | reciprocal | divide (float; `0÷0` is `1`, `n÷0` is a domain error) |
| `*` | exponential | power |
| `⌈` | ceiling | max |
| `⌊` | floor | min |
| `\|` | magnitude | residue |
| `=` `≠` `<` `≤` `>` `≥` | — | comparisons (0/1) |
| `⍴` | shape of | reshape (cyclic) |
| `⍳` | index generator (scalar; respects `⎕IO`) | — |
| `⍉` | transpose | — |
| `↑` | — | take |
| `↓` | — | drop |
| `,` | ravel | — |
| `≢` | tally | — |
| `⊢` `⊣` | same | right / left |

Operators `/` (reduce, LAST axis), `⌿` (reduce, leading axis), `⍤` (rank).
`←` assignment (incl. inline), `⎕←` output, `⋄` and newline sentence
separators, `⍝` comments, `¯` negatives, `''` strings. Index origin is a
dialect setting of the compiler instance (`⎕IO` as a variable is
deliberately not runtime state).

## Interpolation

`{name}` in program text binds host data; braces never splice program text.
In plain strings, only the exact form `{identifier}` is a hole — any other
`{` belongs to the language (J spells take `{.`). In Python 3.14 t-strings,
interpolations must be plain identifiers (values are captured as defaults).

## The data boundary

Host data enters through the first protocol the object offers:

| Protocol | Sources | Result |
|---|---|---|
| `__arrow_c_array__` | PyArrow arrays | one column, shape [M] |
| `__arrow_c_stream__` | Polars/pandas DataFrames and Series, PyArrow tables and chunked arrays | N columns × M rows → shape [M, N], rows leading; N = 1 → shape [M] |
| `__array_interface__` | numpy | any rank, C-contiguous only |

Nothing crosses in the other direction except through the Arrow C data
interface: a rank-1 numeric result has `__arrow_c_array__`, so
`polars.Series(v)` and `pyarrow.array(v)` work. Higher-rank and character
results go out through `.tolist()` for now.

Zero-copy (the source memory is borrowed, and the kernel keeps the source
object alive for as long as it holds the data):

- Arrow `Int64` and `Float64`, and the types that are physically i64 —
  `Timestamp` (any unit), `Date64`, `Duration`, `Time64`. Reinterpretation is
  reading, not converting: a timestamp difference is plain integer
  arithmetic, in the column's own unit, with no type restored on the way out.
- numpy C-contiguous `int64` and `float64` of any rank.
- Every rank-1 `integer`/`float` result on the way out.

Copied (widened or unpacked, once): Arrow `Int8/16/32`, `UInt8/16/32`,
`Date32`, `Time32`, `Float32`, `Boolean` (bit-packed at the source);
`UInt64` when every value fits i64; the same numpy dtypes; boolean results on
the way out; any table of two or more columns, which must be woven from
column-major into row-major.

Refused, with the column named and an action suggested:

- any column holding nulls — J has no missing value;
- columns of a table that do not agree on one element type (int64 beside
  float64), because promoting silently damages values above 2⁵³;
- a numpy view that is not C-contiguous (transposed or strided);
- `UInt64` values above `2⁶³-1`.

Not supported yet (a promise, not a refusal): decimals, strings, binary,
lists, structs, dictionaries, float16, byte-swapped data, and exporting
results of rank ≥ 2.

## Known divergences from the references (deliberate, revisit later)

- Float comparisons are exact; J's default comparison tolerance (2⁻⁴⁴) is
  not yet implemented.
- Comparing characters with numbers is a type error here; J answers 0.
- Monadic `÷` (APL reciprocal) of 0 currently follows J's rule (infinity)
  instead of raising a domain error like dyadic `÷`.
- No boxes / nested arrays, scan, windows, catenate, index-of, dyadic
  transpose, `⎕`-variables, control words, verb/adverb definitions yet —
  all "not yet", category 2.
