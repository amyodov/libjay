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
| `^.` | natural logarithm (`^. 0` is `__`) | logarithm to base x |
| `%:` | square root | x-th root (`x %: y` is `y^(1%x)`) |
| `\|` | magnitude | residue |
| `<.` | floor | min |
| `>.` | ceiling | max |
| `=` `<` `>` | — | comparisons (0/1) |
| `<:` | decrement | ≤ |
| `>:` | increment | ≥ |
| `+:` | double | — |
| `*:` | square | — |
| `-:` | halve (always float) | match: same shape and values, else 0 |
| `-.` | `1 - y` (any number, not only 0/1) | — |
| `*.` | — | LCM (logical and on booleans) |
| `+.` | — | GCD (logical or on booleans; `gcd 0 0` is 0) |
| `~:` | — | ≠ |
| `~.` | nub: distinct items, first-occurrence order | — |
| `$` | shape of | reshape (cyclic) |
| `,` | ravel | catenate along the LEADING axis |
| `,.` | — | stitch, exactly `,"_1` |
| `#` | tally | replicate: item i repeated x[i] times (a scalar x applies to every item) |
| `o.` | pi times y | circle function k (see below) |
| `{` | — | from: each atom of x selects an item (negative from the end) |
| `{.` | head | take (negative = from end; overtake fills) |
| `}.` | behead | drop |
| `{:` | tail (fill cell when there are no items) | — |
| `}:` | curtail | — |
| `\|.` | reverse the items | rotate axis k by `x[k]`, cyclically |
| `\|:` | transpose (reverse axes) | — |
| `i.` | integers (negative axis = reversed) | index of (absent gives the item count) |
| `e.` | — | member: cells of x shaped like items of y |
| `/:` | grade up (stable permutation) | x's items in the ascending order of y's |
| `\:` | grade down (stable permutation) | x's items in the descending order of y's |
| `]` `[` | same | right / left |
| `echo` | print (formatted) | — |

Words present with only one valence implemented say so by name: `,:`
(laminate), monadic `,.` (itemize), monadic `{` (catalogue), monadic `e.`
(raze-in), monadic `*.` (length/angle), monadic `+.` (real/imaginary),
monadic `~:` (nub sieve), dyadic `+:` (nor), dyadic `*:` (nand), dyadic `-.`
(less).

Adverbs: `/` (insert/reduce, leading axis, right-to-left fold), `\` (monad:
`u` applied to every prefix; dyad `x u\ y`: to every window of x items — a
negative x takes non-overlapping chunks with a short last one, zero takes
the n+1 empty runs, and a window longer than the argument yields none),
`\.` (monad: every suffix), `~` (commute: `u~ y` is `y u y`, `x u~ y` is
`y u x`). Conjunctions: `"` (rank, 1–3 atoms, `_` = infinite), `@:` (atop),
`^:` (power: `u^:n` applies u n times, `u^:_` iterates until the result
stops changing), `[:` (cap). Trains: forks `(f g h)`, noun forks
`(n g h)`, hooks `(f g)`. Assignment `=.`/`=:` (one environment for now),
multi-sentence programs, `NB.` comments, `'strings'`, `_`/`__` infinities,
`1e_3` exponents.

## APL

| Glyph | Monadic | Dyadic |
|---|---|---|
| `+` | identity | plus |
| `-` | negate | minus |
| `×` | signum | times |
| `÷` | reciprocal | divide (float; `0÷0` is `1`, `n÷0` is a domain error) |
| `*` | exponential | power |
| `⍟` | natural logarithm | logarithm to base x |
| `⌈` | ceiling | max |
| `⌊` | floor | min |
| `\|` | magnitude | residue |
| `=` `≠` `<` `≤` `>` `≥` | — | comparisons (0/1) |
| `∧` | — | LCM (logical and on booleans) |
| `∨` | — | GCD (logical or on booleans) |
| `~` | not (the argument must be 0 or 1) | — |
| `⍴` | shape of | reshape (cyclic) |
| `⍳` | index generator (scalar; respects `⎕IO`) | index of (respects `⎕IO`; absent gives `⎕IO + ≢x`) |
| `⍉` | transpose | — |
| `↑` | — | take |
| `↓` | — | drop |
| `,` | ravel | catenate along the LAST axis |
| `⍪` | — | catenate along the LEADING axis |
| `⌽` | reverse each row (last axis) | rotate each row (last axis) |
| `⊖` | reverse the items (leading axis) | rotate the leading axis |
| `≡` | — | match: same shape and values, else 0 |
| `≢` | tally | not match |
| `∊` | — | membership, element by element |
| `∪` | nub: distinct items, first-occurrence order | — |
| `⍋` `⍒` | grade up / down (stable; respects `⎕IO`) | — |
| `⊢` `⊣` | same | right / left |
| `○` | pi times y | circle function k (see below) |
| `/` `⌿` | — | replicate, after an operand: `/` counts the LAST axis, `⌿` the leading one |

Glyphs recognised with the missing valence named: monadic `∊` (enlist),
dyadic `∪` (union), `∩` (intersection), monadic `≡` (depth), dyadic `⍋`/`⍒`
(collation), `⍱`/`⍲` (nor/nand), dyadic `~` (without), monadic `⍪` (table).

Operators: `/` (reduce, LAST axis), `⌿` (reduce, leading axis), `\` (scan,
last axis), `⍀` (scan, leading axis), `⍤` (rank), `⍨` (commute), `⍣`
(power, a nonnegative count). A scan's k-th element is the reduce of the
first k, so it folds right to left like the reduce and not like a left
fold: `-\1 2 3` is `1 ¯1 2`.

`/` and `⌿` are operators after a function and replicate after an operand;
names are always values in this subset, so which one is meant is decided by
the token to the left and nothing else.

`←` assignment (incl. inline), `⎕←` output, `⋄` and newline sentence
separators, `⍝` comments, `¯` negatives, `''` strings. Index origin is a
dialect setting of the compiler instance (`⎕IO` as a variable is
deliberately not runtime state).

## The circle functions (`o.` / `○`)

Both languages share one table, indexed by the left argument:

| k | `k o. y` | k | `k o. y` |
|---:|---|---:|---|
| 0 | sqrt(1 - y²) | 4 | sqrt(1 + y²) |
| 1 2 3 | sine, cosine, tangent | 5 6 7 | sinh, cosh, tanh |
| ¯1 ¯2 ¯3 | arcsine, arccosine, arctangent | ¯5 ¯6 ¯7 | arsinh, arcosh, artanh |
| ¯4 | sqrt(y² - 1), signed like y | | |

Monadically the verb is `pi * y`. A k that would leave the reals (arcsine of
2, say) reports the same "complex numbers" gap `%:` of a negative number
does. The k values that only mean something for a complex argument (8 to 12
and their negatives) are named individually; J gives real answers for some
of them (`10 o. y` is the magnitude) and libjay does not implement those
yet.

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
- Catenating items whose shapes differ needs fill; J pads them, libjay says
  "catenate with fill is not supported yet" instead of guessing.
- LCM/GCD accept floats only when every value is integral. J computes a real
  GCD (`2.5 +. 5` is `2.5`); libjay reports "not supported yet".
- APL dyadic `⊖` with a vector left argument reads it as one amount per axis
  (J's rule) rather than one amount per column. Scalar amounts, and `⌽` in
  both valences, follow APL.
- Grade puts NaN wherever the comparison lands rather than at a defined end.
- A moving window of an associative verb (`+`, `*`, `<.`, `>.`) is folded in
  blocks rather than strictly right to left, which reorders the float
  rounding — the same regrouping reduction already takes (§5.9). Every other
  verb, and every prefix scan of a verb that does not associate, is folded
  exactly as the insert would.
- `x u/ y` (J's table, the outer product) is not implemented; the windows
  are `x u\ y`.
- No boxes / nested arrays, dyadic transpose, `⎕`-variables, control words,
  verb/adverb definitions yet — all "not yet", category 2. Named on their
  own: J's key adverb `u/.`, outfix `x u\. y`, `u^:v` and negative powers
  (the obverse), APL expand `x\y` and `f⍣≡`, the complex circle functions.
