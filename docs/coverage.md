# Language coverage

What each frontend understands today. Everything a language has that libjay
does not yet implement fails with an explicit "not supported yet" naming the
feature — that is a promise, not a refusal.

## Common semantics (one IR)

- Dense arrays of booleans, 64-bit integers, 64-bit floats, characters,
  and BOXES, whose every element is itself a whole array (J `<`, APL `⊂`).
  Integer arithmetic promotes to float on overflow, as J does.
- A boxed array is dense like any other, so the structural verbs — shape,
  reshape, take, drop, catenate, reverse, rotate, transpose, from, nub,
  match, index-of, membership, replicate — work on boxes without knowing
  they are boxes. Arithmetic does not: it names the box and says to open
  it. The fill of a boxed array is J's `a:`, a box holding an empty
  numeric list; two empty arrays of the same shape match whatever their
  element types are, as both references have it.
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
| `^.` | natural logarithm (`^. 0` is `__`); a negative argument gives a complex answer | logarithm to base x; the same |
| `%:` | square root; a negative argument gives a complex answer | x-th root (`x %: y` is `y^(1%x)`); the same |
| `\|` | magnitude | residue |
| `<.` | floor | min |
| `>.` | ceiling | max |
| `=` | not supported yet (self-classify) | equal (0/1); compares boxes by content |
| `<` | box: the whole argument in one box | less than (0/1); refuses boxed operands — open them first |
| `>` | open: rank 0, so cells of different shapes are framed with fill; a non-box opens to itself | greater than (0/1); refuses boxed operands — open them first |
| `;` | raze: the items of the opened boxes, catenated (a scalar spreads, unequal items are padded) | link: `(<x)` before y, which joins as it is when already boxed |
| `<:` | decrement | ≤ |
| `>:` | increment | ≥ |
| `+:` | double | — |
| `*:` | square | — |
| `-:` | halve (always float) | match: same shape and values, else 0 |
| `-.` | `1 - y` (any number, not only 0/1) | — |
| `*.` | length/angle: the polar pair, as a new trailing axis | LCM (logical and on booleans; the Gaussian one on complex) |
| `+.` | real/imaginary: the rectangular pair, as a new trailing axis | GCD (logical or on booleans; `gcd 0 0` is 0; the Gaussian one on complex) |
| `~:` | — | ≠ |
| `~.` | nub: distinct items, first-occurrence order | — |
| `$` | shape of | reshape (cyclic); an empty right argument is refused, not filled, when the left argument asks for a nonzero shape (`2 3 $ i. 0`) |
| `,` | ravel | catenate along the LEADING axis |
| `,.` | — | stitch, exactly `,"_1` |
| `,:` | itemize: a leading axis of 1 (`2 3` becomes `1 2 3`) | laminate: the two arguments as the items of a new leading axis (two atoms give shape `2 1`) |
| `#` | tally | replicate: item i repeated x[i] times (a scalar x applies to every item) |
| `#.` | base-2 decode (rank 1) | mixed-radix decode; a scalar x is the radix of every digit, a radix of 0 contributes none |
| `#:` | base-2 encode; the width fits the largest magnitude in the WHOLE argument, so the verb has infinite rank | mixed-radix encode; the digit axis is x's own shape, so `2 #: 5` is a scalar and `2 2 2 #: 5` a 3-list |
| `!` | factorial — gamma(y+1), always float; a negative integer is a signed infinity; a complex argument is a named gap | binomial: x things chosen from y, defined on the reals through gamma; complex is the same gap |
| `j.` | `0j1 * y` | `x + 0j1 * y` |
| `r.` | `^ 0j1 * y`: the unit complex at angle y | `x * ^ 0j1 * y`: polar coordinates |
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
| `i:` | steps: `-y` to `y` one apart, `1 + <. 2 * \| y` of them; a negative y counts down | index of the LAST occurrence (absent gives the item count) |
| `I.` | indices: index `i` repeated `y[i]` times (rank 1, so a table frames the rows) | interval index: how many items of the ascending x are strictly below each cell |
| `e.` | — | member: cells of x shaped like items of y |
| `%.` | matrix inverse — the least-squares pseudo-inverse of a taller matrix; a wider one is refused, a singular one is a domain error | matrix divide: the least-squares solution of `y a = x` |
| `p:` | the y-th prime, counting from zero | — |
| `q:` | prime factors, ascending, with multiplicity (`q: 1` is empty) | — |
| `?` | roll: a random value below each element (`? 0` is a uniform double) | deal: x distinct values from `i. y` |
| `?.` | roll from a fixed seed, restarted on every invocation | deal from that fixed seed |
| `{::` | — | fetch: follow the path x into y, opening one level a step |
| `/:` | grade up (stable permutation) | x's items in the ascending order of y's |
| `\:` | grade down (stable permutation) | x's items in the descending order of y's |
| `]` `[` | same | right / left |
| `echo` | print (formatted) | — |

A boxed array draws the classic J table: cells fenced with `+`, `-` and
`|`, each holding its contents' own display, column widths spanning the
whole array, shorter cells padded below and to the right, and planes above
the last two axes separated by a blank line exactly as numbers are. `":`
hands that drawing back as characters, so a boxed argument formats to a
character array whose last two axes are the display's own rows and columns
(`$ ": 1;2 3` is `3 7`).

The format of a rank-0 argument is a character VECTOR (`$ ": 5` is 1); of a
rank-r one, a character array of rank r whose lines all have one width
(`$ ": i. 2 3 4` is `2 3 11`), because the column widths span the whole
argument.

`u&.>` (each) is the one `&.` libjay derives: it opens every box, applies
u, and boxes the result again — `# &.> 'ab';'cde'` is `2;3`. Dyadically it
pairs boxes at rank 0, so `1 ,&.> 1;2` extends the atom over both.

Words present with only one valence implemented say so by name: monadic `,.`
(ravel items), monadic `{` (catalogue), monadic `e.` (raze-in),
monadic `~:` (nub sieve),
dyadic `+:` (nor), dyadic `*:` (nand), dyadic `-.` (less), dyadic `":`
(format with a specification), monadic `{::` (map), dyadic `p:` (prime
metadata), dyadic `q:` (prime exponents), `L.` (level of).

Adverbs: `/` (monad: insert/reduce, leading axis, right-to-left fold; dyad
`x u/ y`: the table, u applied to every pair of cells — the cells u's own
ranks ask for, so `1 2 3 +/ 10 20` is a 3-by-2 table of sums while
`'ab' ,/ 'cd'` is one catenation), `\` (monad: `u` applied to every prefix;
dyad `x u\ y`: to every window of x items — a negative x takes
non-overlapping chunks with a short last one, zero takes the n+1 empty runs,
and a window longer than the argument yields none), `\.` (monad: every
suffix), `~` (commute: `u~ y` is `y u y`, `x u~ y` is `y u x`), `/.` (dyad
`x u/. y`: the key — u over each group of items of y sharing an item of x,
the groups in the order their keys first appear, the answers framed with
fill; monad `u/. y`: the oblique — u over each anti-diagonal of a table,
starting at the leading corner), `}` with a noun operand (`x m} y`: y with
the items at the indices m replaced by x, one replacement cell for all of
them or one each; a negative index counts from the end, an out-of-range one
is an error, and `m} y` with a single index selects instead). `u}` — amend
with a verb operand — is named as a gap.

Conjunctions: `"` (rank, 1–3 atoms, `_` = infinite); `@:` (atop: monad
`u v y`, dyad `u (x v y)`, at infinite rank) and `@` (the same thing at v's
own ranks — one v-cell at a time, u run on each result, which is the entire
difference between the two); `&:` (compose: monad `u v y`, dyad
`(v x) u (v y)`, at infinite rank) and `&` (that composition at v's monadic
rank on both sides); `&` with a noun operand instead bonds it into the dyad
— `1&+` increments, `^&2` squares — and J gives a bond no dyadic valence at
all, so `x (1&+) y` is an error; `^:` (power: `u^:n` applies u n times, `u^:_` iterates until the result
stops changing, and `u^:v` asks the verb v for the count — so `u^:v` alone
is one conditional step and `(u^:v)^:_` is the while loop the idiom is
written with); `;.` (cut: `x u;.1 y` and `x u;._1 y` open an interval at
each fret, `x u;.2 y` and `x u;._2 y` close one there, and the negative
spellings drop the fret itself; monadically the fret is the argument's own
first item for ±1 and its last for ±2, which is the string-splitting idiom
`<;._2 'a,b,c,'`; `u;.0 y` applies u to the argument with every axis
reversed, and `;.3` and the rest are named gaps); `!.` (fit: on the verbs
whose meaning uses the comparison tolerance it replaces that tolerance, so
`=!.0` compares exactly — on any other verb J's `!.` gives a fill instead,
which libjay names as its own gap); `[:` (cap).

Trains: forks `(f g h)`, noun forks `(n g h)`, hooks `(f g)`. Assignment
`=.`/`=:` (one environment for now), multi-sentence programs, `NB.`
comments, `'strings'`, `_`/`__` infinities, `1e_3` exponents.

Naming a verb — `mean =. +/ % #` — is settled while the program is parsed:
the name is recorded and substituted into the sentences after it, which is
what lets `mean"1`, `(mean - {.)` and `2 n 1 2 3` parse as the trains and
applications they are. The sentence itself yields no value, as an
assignment does. A name may change part of speech in either direction, and
the last assignment before a sentence decides how that sentence reads; that
is enough for the straight-line programs this frontend compiles, since
there is no control flow for a definition to reach backwards through.
Adverb and conjunction assignment, and the explicit definitions
(`3 : '...'`, `4 : '...'`, `{{ }}`), are recognised and reported as not
supported yet.

## APL

| Glyph | Monadic | Dyadic |
|---|---|---|
| `+` | identity | plus |
| `-` | negate | minus |
| `×` | signum | times |
| `÷` | reciprocal | divide (float; `0÷0` is `1`, `n÷0` is a domain error) |
| `*` | exponential | power; a negative base with a fractional exponent gives a complex answer |
| `⍟` | natural logarithm; a negative argument gives a complex answer | logarithm to base x; the same |
| `⌈` | ceiling | max |
| `⌊` | floor | min |
| `\|` | magnitude | residue |
| `=` `≠` `<` `≤` `>` `≥` | — | comparisons (0/1) |
| `∧` | — | LCM (logical and on booleans; the Gaussian one on complex) |
| `∨` | — | GCD (logical or on booleans; the Gaussian one on complex) |
| `~` | not (the argument must be 0 or 1) | — |
| `⍴` | shape of | reshape (cyclic) |
| `⍳` | index generator (scalar argument only; respects `⎕IO`); a non-scalar argument is not supported yet (nested index arrays) | index of (respects `⎕IO`; absent gives `⎕IO + ≢x`) |
| `⍉` | transpose | — |
| `↓` | — | drop |
| `,` | ravel | catenate along the LAST axis |
| `⍪` | table: one row per item, holding that item's elements (a scalar gives 1×1, a vector n×1) | catenate along the LEADING axis |
| `!` | factorial (always float); a complex argument is a named gap | binomial, J's argument order; the same gap |
| `⍕` | format: the characters that display the argument | — |
| `⊥` | — | mixed-radix decode |
| `⊤` | — | mixed-radix encode |
| `⌽` | reverse each row (last axis) | rotate each row (last axis) |
| `⊖` | reverse the items (leading axis) | rotate the leading axis |
| `≢` | tally | not match |
| `∊` | enlist: every leaf element, in ravel order, as a vector | membership, element by element (an element of a nested array is a whole array) |
| `⊂` | enclose — except that a simple scalar is its own enclosure, so `⊂5` is `5` | partitioned enclose: a partition opens where x rises (`x[i] > x[i-1]`, reading `x[¯1]` as 0) and an item flagged 0 is dropped |
| `⍸` | where: index `i` repeated `y[i]` times, from `⎕IO`; a rank-2 or higher argument gives one boxed coordinate vector per occurrence | interval index: how many items of the ascending x are strictly below each cell, plus `⎕IO - 1` |
| `⌷` | — | index: one scalar index per axis of y, and the count must equal the rank (APL2's rule, not Dyalog's enclosed-index form) |
| `⌹` | matrix inverse — the pseudo-inverse of a taller matrix; wider is refused, singular is a domain error | matrix divide: the least-squares solution of `y a = x` |
| `?` | roll: a random value in `⎕IO .. ⎕IO+y-1` (`?0` is a domain error) | deal: x distinct values from that range |
| `⊃` | disclose: the items mixed into one array, filled where their shapes differ | — |
| `↑` | first: the first element of the ravel, disclosed; the type's fill when there is none | take |
| `≡` | depth: 0 for a simple scalar, 1 for a simple array, one more than the deepest box | match: same shape and values, else 0 |
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

Vector notation is real: juxtaposed operands are the items of one vector,
and the whole strand is a single operand. Every primary contributes one
item, except a run of numeric literals, whose numbers are items of their
own — which is why `1 2 (3 4)` has three items, `'ab' 'cd'` has two, and
`1 2 3` is still one simple integer vector. A strand of simple scalars of
different types would need a mixed simple array, which libjay names as a
gap rather than boxing behind the user's back.

Glyphs recognised with the missing valence named: dyadic `∪` (union), `∩`
(intersection), dyadic `⍋`/`⍒` (collation), `⍱`/`⍲` (nor/nand), dyadic `~`
(without), dyadic `⍕` (format with a specification), monadic `↓` (split —
GNU APL has no monadic `↓` either), monadic `⌷` (materialise — GNU APL has
no monadic `⌷` either), dyadic `⊃` (pick). Partitioned enclose on an
argument of rank 2 or more is named separately from the vector case, which
works.

Operators: `/` (reduce, LAST axis), `⌿` (reduce, leading axis), `\` (scan,
last axis), `⍀` (scan, leading axis), `⍤` (rank), `⍨` (commute), `⍣`
(power, a nonnegative count), `¨` (each: the function runs on the contents
of every item and its result goes back into an item — a simple scalar
result stays simple, so `2×¨1 2 3` is flat and `⍴¨'ab' 'cde'` is nested),
`∘.f` (outer product — the same table J spells
`x u/ y`, e.g. `1 2 3∘.×1 2 3`). A scan's k-th element is the reduce of the
first k, so it folds right to left like the reduce and not like a left
fold: `-\1 2 3` is `1 ¯1 2`. `⍣` also takes a FUNCTION right operand:
`f⍣g` applies f until `new g old` holds, so `f⍣≡` is the fixed point.

A bare `∘` is Dyalog's compose `f∘g`, which is named as its own gap — and
will stay one until there is something to be differential against: GNU APL
has no `∘` operator at all (it is only the left half of `∘.`), no `⍥`, no
`⍛`, and no `f⍤g` atop. The whole compose family is therefore no-oracle
territory here, not merely unimplemented.

An axis specification `f[k]` is supported for the four spellings where an
explicit axis is the whole point: `f/[k]` and `f⌿[k]` both reduce axis k,
`f\[k]` and `f⍀[k]` both scan it, and `⌽[k]` and `⊖[k]` both reverse it —
naming an axis collapses each pair to one function. The axis is counted from
`⎕IO`. Every other glyph reports `axis specification for X` as a gap.

Bracket indexing `A[i;j]` is real: one slot per axis, an elided slot meaning
the whole axis, indices counted from `⎕IO`, and the result's shape the
slots' shapes spliced in — so a scalar slot drops its axis and a matrix slot
adds rank. The slots are applied from the last axis to the first, which is
what keeps the earlier axis numbers valid as scalar slots drop theirs.
Indexed assignment (`A[2]←99`) is not implemented; the brackets are a
reader, not a writer.

`/` and `⌿` are operators after a function and replicate after an operand;
names are always values in this subset, so which one is meant is decided by
the token to the left and nothing else.

`←` assignment (incl. inline), `⎕←` output, `⋄` and newline sentence
separators, `⍝` comments, `¯` negatives, `''` strings. Index origin is a
dialect setting of the compiler instance (`⎕IO` as a variable is
deliberately not runtime state).

Function assignment (`F←+/`) is not supported yet: GNU APL, the reference
this frontend follows, rejects it as a syntax error, so there is nothing to
be differential against. J's `mean =. +/ % #` is the spelling libjay has for
the idea today.

## The circle functions (`o.` / `○`)

Both languages share one table, indexed by the left argument:

| k | `k o. y` | k | `k o. y` |
|---:|---|---:|---|
| 0 | sqrt(1 - y²) | 4 | sqrt(1 + y²) |
| 1 2 3 | sine, cosine, tangent | 5 6 7 | sinh, cosh, tanh |
| ¯1 ¯2 ¯3 | arcsine, arccosine, arctangent | ¯5 ¯6 ¯7 | arsinh, arcosh, artanh |
| ¯4 | sqrt(y² - 1), signed like y | ¯8 | -sqrt(-(1 + y²)) |
| 8 | sqrt(-(1 + y²)) | ¯9 ¯10 ¯11 ¯12 | y, conjugate, i×y, e^(iy) |
| 9 10 11 12 | real part, magnitude, imaginary part, phase | | |

Monadically the verb is `pi * y`. The whole table runs on complex arguments,
and a real argument whose answer is not real (arcsine of 2, say) turns the
pass complex, exactly as `%:` of a negative number does. 9 to 12 read a
number's parts, so their answer is real however complex the argument was —
J reports them as floats, and libjay does the same. A k outside ¯12..12, or
a fractional one, is a domain error.

## The numeric tower and the exact types

Six element types, ordered:

    boolean < integer < extended < rational < float < complex

A dyadic pair computes in the higher of its two types, which is what keeps
`1x + 1r2` exact and lets `1x + 1.5` round. The two EXACT types are J's:
`extended` is an arbitrary-precision integer and `rational` an exact ratio
of two of them, held in lowest terms with a positive denominator. Both are
heap-backed pointer arrays, like boxes — never foreign memory, never fused,
never vectorised. APL has neither type, and none of this reaches it.

Where a machine integer overflows, libjay widens to float; that rule is
untouched. `9223372036854775807 + 1` is still `9.22337e18`, and only an
explicitly extended computation is exact:
`9223372036854775807x + 1` is `9223372036854775808`.

**What stays exact.** `+ - * % ^ | <. >. +. *. !` and the reductions and
scans built on them, plus `%:` where the root really is one. The type of an
answer follows one rule: compute exactly, then answer with an extended
integer when BOTH arguments were extended AND every value is whole, and
with a rational otherwise. That is why `4x % 2` is extended, `1x % 3` is
rational, and `1r2 - 1r2` is a rational zero — a rational never falls back
down the tower, which is what the reference reports of it. Rounding and the
sign are the exceptions that always answer whole: `<. 7r2`, `>. 7r2` and
`* 1r2` are extended.

**What falls to float.** Anything with no exact answer, by the same
mechanism an overflowing machine integer uses: `%: 2x`, `^ 1x`, `^. 2x`,
`o. 1x`, `! 1r2`, a fractional power (`2x ^ 0.5`), a division by zero
(`1x % 0` is `_`), and `x %: y` on rationals — the reference answers
`3 %: 8r27` with `0.666667`, not with `2r3`, so exact roots are looked for
between whole numbers only. A negative exact value under `%:` leaves the
reals exactly as a negative float does: `%: _4x` is `0j2`.

**Comparison.** Two exact values compare exactly — no tolerance stands
between them, so `(10x^30) = 1 + 10x^30` is 0 where the float answer would
be 1. Against a float the pair's type is float, and the float rule applies
tolerance and all: `1r3 = 0.333333333333333333` is 1. Grade, nub and
membership order and match exact values by value, so `2r4` grades and
matches exactly where `1r2` does.

**Display.** No `x` suffix is shown — `123x` prints as `123` — and a
rational prints as `numerator r denominator`, or as the integer alone when
its denominator is 1. `":` formats them the same way.

**`x:`.** The monad converts to the exact types: whole values become
extended integers, and anything else becomes the SIMPLEST rational within
the comparison tolerance of the float, which is what makes `x: 0.1` a tenth
rather than the binary fraction a double really holds. An integral double
is exact, so `x: 1e30` keeps all thirty-one digits it carries. The dyad
takes the form on the left: `1 x: y` is the rational form, `2 x: y` the
numerator and denominator as a new trailing axis, `_1 x: y` the conversion
back to a machine number, and `_2 x: y` the argument unchanged. Any other
left argument is a domain error, as it is in J.

**Limits.** A bignum grows without warning, so a power whose result would
need more than 2²⁶ bits is refused by name rather than exhausting the
machine. `i.` carries an extended length into extended indices, so
`*/ >: i. 25x` is the exact factorial; `#.`, `#:`, `p:` and `q:` answer
with machine integers whose values agree with the reference but whose type
is `integer` where J reports `extended`.

## Comparison tolerance

Both languages compare reals with a relative tolerance, and libjay carries
the dialect's own: J's `9!:18` value, 2⁻⁴⁴ ≈ 5.68e¯14, and GNU APL's `⎕CT`,
1e¯13. The two rules are not the same one with a different constant, and
each was measured against its own reference rather than assumed:

- J: `x` and `y` are equal when `x = y` exactly, or when
  `|x-y| < ct × (|x| ⌊ |y|)` — the SMALLER magnitude, strictly below.
- APL: the same, with the LARGER magnitude — `|x-y| < ⎕CT × (|x| ⌈ |y|)`.

The difference shows only at the threshold itself, which is exactly where
the references were probed: `1 = 1 + 2^_44` is 0 in J and `1=1+2*¯44` is 1
in GNU APL, and the pair pins the two rules apart. A comparison against zero
is therefore exact — nothing is relatively near it — and so is any
comparison of integers, characters or boxes with anything but a float
inside. Equal infinities are equal; unequal ones are not.

Complex values take the same rule on the MAGNITUDE of the difference:
`|x-y| < ct × (|x| ⌊ |y|)` in J, the larger magnitude in APL. That is what
J answers — `3j4 = 3.0000000000001j4` is 1 — and GNU APL is far looser here
(see divergences.txt).

The tolerance reaches `=` `~:` `<` `<:` `>` `>:` `-:` `e.` `i.` `i:` `~.`
`I.` in J, the same family plus `≡` `∊` `⍳` `∪` `⍸` in APL, and the two
roundings `<.`/`⌊` and `>.`/`⌈`, which snap to a neighbouring integer when
they are tolerantly equal to it. It does NOT reach grade (`/:`, `\:`, `⍋`,
`⍒`), which both references leave exact.

J's `!.` sets it per verb: `=!.0` compares bit for bit, and any tolerance
above 2⁻³⁴ is refused, as J refuses it. `⎕CT` as a runtime variable is not
implemented — APL's tolerance is the dialect's, as `⎕IO` is.

A fused kernel carries the tolerance the program was compiled with, so a
comparison inside a blockwise chain answers exactly as the same comparison
outside one does.

## Random numbers

`?` and `?.` (J) and `?` (APL) draw from MT19937, the published Mersenne
Twister, seeded by libjay: `?.` restarts from a fixed seed on every
invocation, so the same sentence always answers the same way, and `?` is
seeded once per process from the clock. That reproduces the BEHAVIOUR the
references define, not their numbers — neither jconsole nor GNU APL
publishes the stream it draws from, and libjay does not read either one to
find out. Both spellings are therefore kept out of the differential corpora;
what is tested is the contract: the range, the shape, the distinctness of a
deal, and that `?.` repeats.

## Explicit definitions

A definition is a verb (a function) like any other: it takes a rank, joins a
train, is an operand to a modifier, and can be named.

J writes it four ways. `3 : '…'` is a monad whose argument arrives as `y`;
`4 : '…'` is a dyad with `x` and `y` as well. Either with `0` in place of
the string takes its body from the lines below, ending at a line that is a
lone `)`. `{{ … }}` is the direct form, on one line or over several, and its
own words decide its valence: a body that mentions `x` is a dyad. A body
that mentions `u`, `v`, `m` or `n` is a modifier in J, and libjay says so by
name rather than guessing — as it does for `1 :`, `2 :` and `13 :`.

APL writes it two ways. `{…}` is a dfn: `⍵` is the right argument, `⍺` the
left, `⋄` and a line break separate its statements, and it nests. `∇ Z←L F R;a`
… `∇` is the multi-line form, where APL's control structures live.

The body is a block, and a block's value is its last sentence's — the same
rule the top level follows, except that an assignment inside a definition
does yield the value it assigned. `return.` (`:Return`) leaves with the
value in hand. A branch that ran nothing yields J's empty `i. 0 0`; in APL
a definition that produced no value is an error, and a `∇`-definition reads
its result from the name its header gave.

Recursion works by name (`fac =. 3 : '… fac …'`, `∇Z←FC R … FC R-1 … ∇`) and
through J's `$:` and APL's `∇`, which name the innermost definition then
running. It is bounded: 64 nested calls raise a domain error naming the
limit rather than letting the machine stack give out. The number is set by
the evaluator's stack frames, not by either language, and can rise.

An explicit definition is never fused and never runs its cells on several
threads: it reads and writes the program's names, so the order it runs in is
part of its meaning.

## Control structures

J's control words are legal only inside an explicit definition — the
reference calls one at the top level a spelling error, and so does libjay.
Implemented: `if. do. elseif. else. end.`, `while. do. end.`,
`whilst. do. end.` (body first), `for. do. end.` and `for_i. do. end.`
(`i` is the item, `i_index` its position), `select. case. fcase. do. end.`
(a `case.` with no test is the default; `fcase.` runs the next body too),
`return.`, `break.`, `continue.`, and `try. catch. end.`. `throw.`,
`catcht.`, `goto_name.` and `label_name.` are named gaps.

A test is true when the argument is empty, or when its first atom is not
zero — J's rule, checked against the reference. `select.` matches with `-:`
(match), not membership, so `case. 1 2` takes the list `1 2` and not the
atom `1`.

`try.` answers for the languages' own errors. A gap in libjay itself — "not
supported yet" — goes straight through it: swallowing a promise would turn
it into a wrong answer.

APL's `:If :ElseIf :Else :EndIf`, `:While :EndWhile`, `:Repeat :Until`,
`:For … :In … :EndFor`, `:Select :Case :Else :EndSelect`, `:Return`,
`:Leave` and `:Continue` work inside a `∇`-definition; `:End` closes any of
them. There is no oracle for any of this — GNU APL raises a SYNTAX ERROR for
every one of these words — so they follow the published specification and
are tested in `crates/libjay/tests/definitions.rs` rather than in the
corpus.

## Scope

J: `=.` names a local and `=:` a global. Inside an explicit definition the
two differ — a local lives in that call's own frame and is gone when the
call returns, a global outlives it — and at the top level, which has no
frame, both name the same thing. A name is looked for in the running call's
frame and then among the globals; frames do not nest, so a definition called
from another sees its own locals and the globals, never the caller's. Every
call gets a fresh frame, which is what makes recursion work.

APL's dfns follow the same rule with `←`: a name assigned inside one is
local to the call. A `∇`-definition follows APL's own rule instead — a name
the header does not declare is GLOBAL, and `;a;b` after the header is what
makes names local, along with the result name and the two argument names.
Both rules are the reference's; the corpus records them.

## Indexed assignment

APL's `A[i]←v` and `A[i;j]←v` read the named value, copy it, write the
selected part, and give the copy the name. Another name that was taken from
the array before the write keeps what it had. An elided slot selects its
whole axis; a scalar slot drops its axis from the shape the value has to
match, and a scalar value spreads over the whole selection. A value of a
wider type widens the array rather than being truncated into it. An index
outside its axis is a domain error naming the axis and its length.

## Numeric literals

J's base and constant forms: `16b1f` is 31, `2b101` is 5, a fractional or
negative base works (`2.5b10`, `_16b11`), a `_` in front of the digits
negates the value (`16b_1`), and digits run `0`–`9` then `a`–`z`. `1p1` is
π and `1p2` is π², `1x1` is e and `2x1` is 2e — `apb` is a×π^b and `axb` is
a×e^b, with either part allowed a sign, a fraction or an exponent.

`1x` with nothing after it is an extended-precision integer and `1r2` a
rational — the two exact types, described below. The `x` reads as the
suffix only when nothing follows it, so `1x1` stays a multiple of e; the
suffix takes whole decimal digits alone, and `1.5x` and `1e10x` are
ill-formed numbers, as they are in the reference. A rational's two halves
each take J's negative sign, and the value is reduced on sight, so `2r6` is
`1r3` and `1r_2` is `_1r2`. A denominator of zero is not a rational at all:
J reads `1r0` as infinity and `0r0` as 0, and so does libjay — the value
leaves the exact types where it is written.

Digits that overflow a machine word and carry no `x` become a float, as
they do in J: `1000000000000000000000` is `1e21`, and the suffix is what
asks for the exact value instead.

Complex literals: `3j4` in J and `3J4` in APL are the rectangular form. J
also has the polar ones — `1ad45` takes the angle in degrees, `1ar1` in
radians — and both are exact on the quadrant boundaries, so `2ad90` is
`0j2` and not a cosine's rounding of it. The exponent letters bind loosest
of all, so `1ar1p1` is the polar value `1ar1` scaled by π and `1p1j1` is π
raised to the power `1j1`. A `b` earlier in the word makes the `j` or the
`a` a base-literal digit instead (`36bj` is 19).

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

A Python list whose items do not share one shape and element type — a list
of strings, a ragged list of lists — becomes a BOXED vector: each item is
converted and then boxed. The dense path is tried first, so nothing that
worked before changes shape. `j("# &.> {names}", {"names": ["ab", "cde"]})`
is `[2, 3]`.

A boxed result converts to plain nested Python data at every level: a box
hands back what it holds, a character vector is a `str`, anything else is
a (nested) list. `Value.dtype` is `"boxed"`, `Value.depth` counts the
nesting the way APL's `≡` does, and `repr` is the J drawing. A rank-0 box
is its contents at whatever shape they have, so `<'abc'` is `"abc"`.

The C ABI has no descriptor for a box — its elements are arrays, not
numbers — so a boxed result comes back from `jay_run` as the error "boxed
results are not in the C ABI yet". An extended or rational result takes the
same path, and its message names `_1 x:` as the way to a machine number.

### The exact types at the boundary

Python's integers are unbounded, so they map onto J's extended type in both
directions: an `int` too large for a machine word arrives as an extended
value rather than as a refusal, and an extended result comes back as an
`int` with every digit. A rational maps onto `fractions.Fraction` — a
`Fraction` argument becomes a rational, and a rational result a `Fraction`.

Arrow has no carrier for either, so `polars.Series(v)` and `pyarrow.array(v)`
refuse an exact result by name. The two ways out are `.tolist()`, which
gives exact Python objects, and `_1 x:`, which converts to integers and
floats inside the expression. Decimal128 is a separate future carrier and
is not one of them.

Nothing crosses in the other direction except through the Arrow C data
interface: a rank-1 numeric result has `__arrow_c_array__`, so
`polars.Series(v)` and `pyarrow.array(v)` work. Higher-rank and character
results go out through `.tolist()` for now.

### Complex at the boundary

Arrow has no complex type, so complex data crosses as a PAIR of float
columns — real part and imaginary part — in one of two shapes:

| Direction | Form |
|---|---|
| In (a single array) | `struct<re: f64, im: f64>` → a complex column, shape [M] |
| In (a table) | two adjacent `f64` columns named `x_re` and `x_im` → one complex column `x` |
| Out | a rank-1 complex result exports as `struct<re: f64, im: f64>` |

The struct is the single-array form of the paired-column convention: a
consumer that wants two table columns back gets them with `unnest`. Neither
Arrow direction is zero-copy — libjay holds the two parts interleaved and
Arrow holds them apart, and one of the two has to move.

numpy is zero-copy in both languages' favour: `complex128` is two
contiguous doubles per element, which is exactly libjay's own layout, so a
`complex128` array is BORROWED like `float64` is. There is no complex
result type on the numpy side yet; `.tolist()` yields Python `complex`
values and a rank-0 complex result is a Python `complex`.

The C ABI tag is `JAY_COMPLEX = 5`, with `double[2]` per element in and
out — the layout of C99's `double _Complex`, so a caller passes its own
array straight through. A real argument can produce a complex result
(`%: _4`), so a C caller reads `jay_result_dtype` rather than assuming the
type it passed in.

Zero-copy (the source memory is borrowed, and the kernel keeps the source
object alive for as long as it holds the data):

- numpy C-contiguous `complex128`, whose two-doubles-per-element layout is
  libjay's own.
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
lists, dictionaries, float16, byte-swapped data, and exporting results of
rank ≥ 2 or of boxes. An Arrow list column, and any struct that is not the
`re`/`im` pair above, is still refused rather than boxed: the mapping from
Arrow nesting to box nesting is its own decision and has not been taken.

## Device placement

Where an expression runs is not part of what it means. A compiled kernel is
bound to data with `bind` and placed on a processor with `deploy`; the two
are separate, both return a new kernel, and neither changes the value, the
shape, the dtype or the diagnostics. Whatever a device cannot run runs on
the CPU, and `explain` names each fused node's placement and, when it is the
CPU, the reason.

```python
import jay

jay.devices()          # [Device(name='AMD Radeon Pro 560', backend='metal',
                       #         kind='discrete GPU', f64=False)]

k = jay.j.compile("+/ {w} * {x}").bind({"w": w, "x": x})
g = k.deploy("gpu", precision="f32")
g()                    # the same answer, computed on the GPU
g.explain()            # ... device: AMD Radeon Pro 560 (metal, discrete GPU) ...
                       # fused kernel (2 ops: * +; +/ absorbed) [kernel ran; device: gpu]
```

`jay.j(...)` — compile, bind and execute in one call — has no device and
never will: there is nowhere in one call to say where, and uploading data
for a single run rarely pays for itself.

**What runs on a device.** Fused elementwise chains, and only those. The
fusion pass already reduces a chain of scalar verbs to a postfix program
over blocks with an optional reduction folded in; that program is compiled
to WGSL at run time and dispatched as one compute pass (a map) or a
workgroup reduction whose partials are folded on the host (a reduction).
Everything else — structural verbs, sorts, windows, anything the fusion pass
did not reach — runs where it always ran. The generated shader has no
per-primitive hand-written code in it: one arm of the generator per scalar
operation is the whole of it, as one block loop per operation is the whole
of the CPU kernel.

**A chain stays on the CPU when** its working type is i64 (WGSL has no
64-bit integer arithmetic on most adapters); its result is not a float array
(a comparison at the root, a tally); it holds `^` and the device computes in
f64 (the exponential is a 32-bit builtin in both SPIR-V and MSL); the data
is smaller than half a million elements, below which the dispatch and
readback cost more than the whole pass; or the fused kernel would have
declined it anyway. Each of these is reported by `explain`, in the same
sentence as the kernel's own decline reason.

**Precision.** libjay computes floats in f64. WGSL can express f64 and
naga validates it, but almost no adapter implements it: Metal has no double
at all, and on Vulkan it is the optional `SHADER_F64` feature. On an adapter
without it an f64 chain is **declined to the CPU** rather than quietly
computed in f32 — losing precision is not a performance decision libjay
takes on anyone's behalf. `deploy(..., precision="f32")` is the caller
saying they want single precision anyway.

**How close the answers are.** Elementwise `+`, `-` and `*` in f64 are
exactly what IEEE says they are on both processors, so a map is
bit-identical; division and the transcendentals may differ by an ulp. A
reduction regroups an associative fold, which is the same licence the
parallel CPU path already takes (see the float contract), so it is compared
with a relative tolerance: 1e-14 in f64, and in f32 whatever single
precision is worth — about 1e-4 for a sum over millions of values, and a few
parts in ten thousand for a product over half a million factors.

**Residency.** `kernel.upload(x)` returns a `DeviceArray`: an ordinary value
— shape, dtype, `download()` — that also carries the device allocation, so
passing it to a later run on the same device uploads nothing. It stays
readable by the CPU path, which is what lets a fallback use it without
asking anyone. `kernel(data, keep_on_device=True)` returns the result as one
of these. Known limitation: the host copy is materialised at the same time,
so a result handed straight to the next kernel still costs one readback; a
device-to-device hand-off is not implemented yet.

**Build and artifact.** The backend is compiled into the one artifact per
platform and asks the machine at run time what it has, exactly as the CPU
feature levels do. There is no feature flag, no second wheel, and no shader
compiled at build time: WGSL is generated and handed to the driver when a
program first runs on a device, and the compiled pipelines are cached per
kernel and entry point.

## The reference oracles

Two interpreters are run as black-box subprocesses — fed an expression on
stdin or the command line, compared on their printed output, never linked
and never read:

- J: the official prebuilt jconsole, `LIBJAY_ORACLE_J`, corpus in
  `crates/libjay/tests/corpus/j/`.
- APL: GNU APL 2.0, built from the FSF tarball into
  `~/projects/libjay-oracles/gnu-apl/`, `LIBJAY_ORACLE_APL`, corpus in
  `crates/libjay/tests/corpus/apl/`. It is run with
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
- `x:` of an infinity is a domain error here; J answers `_` unchanged.
  Refusing beats guessing at which exact number an infinity was meant to be.
- `x:` of a float with no nice rational near it picks the simplest
  convergent within the comparison tolerance; J picks a different, larger
  one (`x: 3.14159265358979` is `5419351r1725033` here and
  `6686425096436r2128355211423` there). Both are within tolerance of the
  argument, and every value with a nice rational nearby — `0.1`, `1.5`,
  `2.5` — agrees exactly.
- `$`, `#`, `#.`, `#:`, `p:` and `q:` answer with machine integers where J
  answers with extended ones. The values agree; only the type J's `3!:0`
  reports differs.
- A bonded noun (`n&v`, `u&n`) has to be a literal, as a noun fork's left
  tine does; a computed one says "bonds over a non-literal noun is not
  supported yet".
- Ordering boxes needs J's total array ordering — which sorts by type,
  then by element count, then by rank, then by contents — so `/:`, `\:`,
  `⍋` and `⍒` name it as a gap when the array being graded is boxed.
  Sorting boxed items BY an unboxed key works. A COMPLEX array grades by
  (real, imaginary), which is what J's `/:` answers; the ordering verbs
  still refuse complex operands, because a permutation is not a claim about
  size.
- Catenating a boxed array to an unboxed one is a type error in both
  languages. J agrees; APL2 encloses the simple items instead.
- J's `$:` inside an explicit definition names that definition, which is
  what the dictionary says it does. The jconsole this repository tests
  against reads it as the largest verb in the SENTENCE — which is `$:`
  itself — and raises a stack error for every recursion written with it, so
  the J corpus leaves `$:` out and tests/definitions.rs holds libjay to the
  published rule instead. Recursion by NAME (`fac =. 3 : '… fac …'`) agrees
  with the reference and is in the corpus.
- No dyadic transpose, `⎕`-variables or adverb/conjunction definitions yet —
  all "not yet", category 2. Named on their own: J's key adverb `u/.`, outfix `x u\. y`, `u^:v` and negative
  powers (the obverse), under `u&.v` other than `u&.>` (it needs verb
  inverses), `L.`, APL expand `x\y`, `f⍣≡`, compose `f∘g`, dyadic `⊂` and
  `⊃`, monadic `↓`, APL's `⌷`. Bigints and rationals are still the other
  half of the "boxes, bigints, rationals" promise; complex numbers arrived
  with this wave.

- Complex numbers reach every scalar verb, the reductions, the scans and
  the structural verbs. They do NOT reach: `!` (the gamma function of a
  complex argument), matrix inverse and matrix divide (`%.` / `⌹`, which
  work in f64), `#.`/`⊥` and `#:`/`⊤` (decode and encode), and the fused
  blockwise kernel, which computes in one real type and declines a chain
  that touches complex data — the ordinary pipeline runs it instead, with
  the same answer.

### Differences from GNU APL

GNU APL is ISO/APL2-flavoured and libjay's APL takes a Dyalog-style choice
in a few places, so the two part company on purpose. Each line below is one
entry of `crates/libjay/tests/corpus/apl/divergences.txt`, which
asserts that they keep disagreeing — a silent convergence is a test failure,
not a quiet win. Everything else in the corpus agrees.

libjay follows J where APL2 stops at DOMAIN ERROR:

- monadic `÷0` is `∞` and `⍟0` is `¯∞` (the first is already listed above).
- monadic `⍳` on a vector is a nested index array there; here it is a
  named gap (`⍳2 3` → not supported yet: nested index arrays).
- `!¯1` is `∞` (the gamma pole) and `¯7○1` — artanh 1 — is `∞`.
- the neutral cell of `⌈` and `⌊` over no items is `¯∞`/`∞`, where GNU APL
  uses the largest representable magnitudes. Every other entry of the
  identity table now matches both references exactly.

libjay is more permissive:

- `∪` is nub over ITEMS, so a matrix is legal; GNU APL's monad takes vectors
  only.
- dyadic `⊖` reads a vector left argument per axis, GNU APL per column
  (already listed above).
- `⍺←v` inside a dfn is a DEFAULT — it gives the left argument a value only
  where none arrived — which is what the published dfn model says. GNU APL
  assigns unconditionally, so there the default overwrites the argument.
- a dfn's value is its LAST statement and nothing else is displayed, which
  is libjay's sequence rule everywhere. GNU APL prints the value of every
  statement of the body that is not an assignment on its way through.

libjay is stricter, or simply elsewhere:

- a vector of simple scalars of different types (`1 'a'`) is a simple
  MIXED array in APL2; libjay has no such array yet and names the gap.
- catenating a simple array to a nested one (`1 2,⊂3 4`) encloses the
  simple items there and is a type error here.
- overtaking a nested array fills with the first item's prototype there
  (`4↑(1 2)(3 4)` gives two `0 0`) and with the empty box here.
- the nested DISPLAY is libjay's own: one space between items and one
  around the whole, where GNU APL spaces items more widely. Only the
  length of `⍕` makes the difference visible to the comparison, which
  ignores whitespace, so `⍴⍕(1 2)(3 4)` is the entry that pins it. Nested
  DISPLAYS are kept out of the corpus for that reason; what is compared
  there is structure — `⍴`, `≡`, `≢` and the leaves `∊` brings back.
- grading a nested array and pick (`1 2⊃…`) are answered there and named as
  gaps here.

- `2 2⍴⍳0` fills with the prototype in APL2 and is an error here — reshape
  does not invent data.
- the circle functions on a REAL argument whose answer is imaginary (`8○3`,
  `¯8○3`, `¯2○2`): GNU APL carries a negative zero into the square root and
  lands on the lower branch, libjay takes the principal one — which is what
  J answers.
- ordering a complex number. GNU APL extends `< ≤ > ≥` to complex operands
  as a lexicographic order on (real, imaginary); libjay refuses it, as the
  standard and J both do. Grading goes the other way: libjay grades a
  complex vector by (real, imaginary), which is what J's `/:` answers, and
  GNU APL refuses.
- complex equality. libjay compares the magnitude of the difference against
  `⎕CT`, as its real comparison does; GNU APL's complex comparison is looser
  by roughly the square root of `⎕CT`, so `1J1=1.0000000001J1` is 1 there.
- `⊥` folds the last axis of a rank ≥ 2 argument, APL the leading one (see
  the `⊥`/`⊤` note above).
- a sequence yields its last sentence and prints nothing on the way, so
  `1 2 3⋄4 5` is `4 5`; GNU APL prints the value of every statement.

One entry is GNU APL's bug rather than a dialect difference, pinned so that a
later release fixing it is noticed: a scan whose axis has length 1 loses that
axis there, so `+\2 1⍴⍳12` comes back as a 2-vector and `+\,5` as a scalar.
`⌽`, `⌿` and every other axis length are fine.
