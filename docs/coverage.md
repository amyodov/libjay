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
| `,:` | itemize: a leading axis of 1 (`2 3` becomes `1 2 3`) | laminate: the two arguments as the items of a new leading axis (two atoms give shape `2 1`) |
| `#` | tally | replicate: item i repeated x[i] times (a scalar x applies to every item) |
| `#.` | base-2 decode (rank 1) | mixed-radix decode; a scalar x is the radix of every digit, a radix of 0 contributes none |
| `#:` | base-2 encode; the width fits the largest magnitude in the WHOLE argument, so the verb has infinite rank | mixed-radix encode; the digit axis is x's own shape, so `2 #: 5` is a scalar and `2 2 2 #: 5` a 3-list |
| `!` | factorial — gamma(y+1), always float; a negative integer is a signed infinity | binomial: x things chosen from y, defined on the reals through gamma |
| `":` | format: the characters that display the argument | — |
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

The format of a rank-0 argument is a character VECTOR (`$ ": 5` is 1); of a
rank-r one, a character array of rank r whose lines all have one width
(`$ ": i. 2 3 4` is `2 3 11`), because the column widths span the whole
argument.

Words present with only one valence implemented say so by name: monadic `,.`
(ravel items), monadic `{` (catalogue), monadic `e.` (raze-in), monadic `*.`
(length/angle), monadic `+.` (real/imaginary), monadic `~:` (nub sieve),
dyadic `+:` (nor), dyadic `*:` (nand), dyadic `-.` (less), dyadic `":`
(format with a specification).

Adverbs: `/` (monad: insert/reduce, leading axis, right-to-left fold; dyad
`x u/ y`: the table, u applied to every pair of cells — the cells u's own
ranks ask for, so `1 2 3 +/ 10 20` is a 3-by-2 table of sums while
`'ab' ,/ 'cd'` is one catenation), `\` (monad: `u` applied to every prefix;
dyad `x u\ y`: to every window of x items — a negative x takes
non-overlapping chunks with a short last one, zero takes the n+1 empty runs,
and a window longer than the argument yields none), `\.` (monad: every
suffix), `~` (commute: `u~ y` is `y u y`, `x u~ y` is `y u x`).

Conjunctions: `"` (rank, 1–3 atoms, `_` = infinite); `@:` (atop: monad
`u v y`, dyad `u (x v y)`, at infinite rank) and `@` (the same thing at v's
own ranks — one v-cell at a time, u run on each result, which is the entire
difference between the two); `&:` (compose: monad `u v y`, dyad
`(v x) u (v y)`, at infinite rank) and `&` (that composition at v's monadic
rank on both sides); `&` with a noun operand instead bonds it into the dyad
— `1&+` increments, `^&2` squares — and J gives a bond no dyadic valence at
all, so `x (1&+) y` is an error; `^:` (power: `u^:n` applies u n times,
`u^:_` iterates until the result stops changing); `[:` (cap).

Trains: forks `(f g h)`, noun forks `(n g h)`, hooks `(f g)`. Assignment
`=.`/`=:` (one environment for now), multi-sentence programs, `NB.`
comments, `'strings'`, `_`/`__` infinities, `1e_3` exponents.

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
| `⍪` | table: one row per item, holding that item's elements (a scalar gives 1×1, a vector n×1) | catenate along the LEADING axis |
| `!` | factorial (always float) | binomial, J's argument order |
| `⍕` | format: the characters that display the argument | — |
| `⊥` | — | mixed-radix decode |
| `⊤` | — | mixed-radix encode |
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

`⊥` and `⊤` have no monadic meaning in APL at all; J spells those `#.` and
`#:`. `x ⊤ y` takes its right argument whole, so the digits become the
LEADING axis and the result has shape `(⍴x),(⍴y)` — the transpose of what
J's `x #: y` produces, which frames the digits per atom of y. `⊥` has not
had the matching treatment yet: an APL decode is an inner product and folds
the LEADING axis of its right argument, while libjay folds the last one,
which is J's `#.` at rank 1. Vectors — the whole of the common case — agree;
rank 2 and above do not, and the fix waits on the axis-moving transpose that
dyadic `⍉` needs anyway.

Glyphs recognised with the missing valence named: monadic `∊` (enlist),
dyadic `∪` (union), `∩` (intersection), monadic `≡` (depth), dyadic `⍋`/`⍒`
(collation), `⍱`/`⍲` (nor/nand), dyadic `~` (without), dyadic `⍕` (format
with a specification).

Operators: `/` (reduce, LAST axis), `⌿` (reduce, leading axis), `\` (scan,
last axis), `⍀` (scan, leading axis), `⍤` (rank), `⍨` (commute), `⍣`
(power, a nonnegative count), `∘.f` (outer product — the same table J spells
`x u/ y`, e.g. `1 2 3∘.×1 2 3`). A scan's k-th element is the reduce of the
first k, so it folds right to left like the reduce and not like a left
fold: `-\1 2 3` is `1 ¯1 2`. A bare `∘` is Dyalog's compose `f∘g`, which is
named as its own gap.

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

## The reference oracles

Two interpreters are run as black-box subprocesses — fed an expression on
stdin or the command line, compared on their printed output, never linked
and never read:

- J: the official prebuilt jconsole, `LIBJAY_ORACLE_J`, corpus in
  `crates/libjay/tests/oracle.rs`.
- APL: GNU APL 2.0, built from the FSF tarball into
  `~/projects/libjay-oracles/gnu-apl/`, `LIBJAY_ORACLE_APL`, corpus in
  `crates/libjay/tests/oracle_apl.rs`. It is run with
  `--script --safe --noSV --PW 10000 --eval`, which silences the banner,
  keeps it from opening sockets or loading a workspace, and stops long
  vectors wrapping onto continuation lines. GNU APL always exits 0 and
  reports a failed sentence on stderr, so a non-empty stderr is what "the
  reference refused this" means.

Both sides are compared line by line (the line structure carries the shape)
and token by token within a line, each token parsed back to `f64` and
accepted within 1e-5 relative / 1e-9 absolute. Column padding is not
compared: libjay right-aligns a mixed numeric column where GNU APL aligns on
the decimal point, and that is typography, not semantics.

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
- The binomial `x ! y` returns an exact integer wherever the whole-number
  answer fits i64; J switches to float earlier (`28 ! 56` prints
  `7.64869e15` there and exactly here). The values agree to well within the
  differential tolerance; only the printed form differs.
- A bonded noun (`n&v`, `u&n`) has to be a literal, as a noun fork's left
  tine does; a computed one says "bonds over a non-literal noun is not
  supported yet".
- No boxes / nested arrays, dyadic transpose, `⎕`-variables, control words,
  verb/adverb definitions yet — all "not yet", category 2. Named on their
  own: J's key adverb `u/.`, outfix `x u\. y`, `u^:v` and negative powers
  (the obverse), under `u&.v` (it needs verb inverses), APL expand `x\y`,
  `f⍣≡`, compose `f∘g`, the complex circle functions, APL's `⌷`.

### Differences from GNU APL

GNU APL is ISO/APL2-flavoured and libjay's APL takes a Dyalog-style choice
in a few places, so the two part company on purpose. Each line below is one
entry of `KNOWN_DIVERGENCES` in `crates/libjay/tests/oracle_apl.rs`, which
asserts that they keep disagreeing — a silent convergence is a test failure,
not a quiet win. Everything else in a 592-expression corpus agrees.

libjay follows J where APL2 stops at DOMAIN ERROR:

- monadic `÷0` is `∞` and `⍟0` is `¯∞` (the first is already listed above).
- `!¯1` is `∞` (the gamma pole) and `¯7○1` — artanh 1 — is `∞`.
- the neutral cell of `⌈` and `⌊` over no items is `¯∞`/`∞`, where GNU APL
  uses the largest representable magnitudes. Every other entry of the
  identity table now matches both references exactly.

libjay is more permissive:

- `∪` is nub over ITEMS, so a matrix is legal; GNU APL's monad takes vectors
  only.
- dyadic `⊖` reads a vector left argument per axis, GNU APL per column
  (already listed above).

libjay is stricter, or simply elsewhere:

- `2 2⍴⍳0` fills with the prototype in APL2 and is an error here — reshape
  does not invent data.
- a circle function that leaves the reals (`0○2`) is a complex number there
  and the named "complex numbers" gap here.
- `⊥` folds the last axis of a rank ≥ 2 argument, APL the leading one (see
  the `⊥`/`⊤` note above).
- a sequence yields its last sentence and prints nothing on the way, so
  `1 2 3⋄4 5` is `4 5`; GNU APL prints the value of every statement.

One entry is GNU APL's bug rather than a dialect difference, pinned so that a
later release fixing it is noticed: a scan whose axis has length 1 loses that
axis there, so `+\2 1⍴⍳12` comes back as a 2-vector and `+\,5` as a scalar.
`⌽`, `⌿` and every other axis length are fine.
