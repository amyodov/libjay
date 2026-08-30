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
  expression yields the assigned value. An APL definition whose answer came
  from an assignment answers SHYLY: the value is there for whatever
  consumes it, and a session displays nothing.
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
| `=` | self-classify: one row per distinct item, 1 where that item stands (a scalar is one item, so `= 5` is 1×1) | equal (0/1); compares boxes by content |
| `<` | box: the whole argument in one box | less than (0/1); refuses boxed operands — open them first |
| `>` | open: rank 0, so cells of different shapes are framed with fill; a non-box opens to itself | greater than (0/1); refuses boxed operands — open them first |
| `;` | raze: the items of the opened boxes, catenated (a scalar spreads, unequal items are padded) | link: `(<x)` before y, which joins as it is when already boxed |
| `<:` | decrement | ≤ |
| `>:` | increment | ≥ |
| `+:` | double | nor; both arguments must already be 0 or 1 |
| `*:` | square | nand; the same |
| `-:` | halve (always float) | match: same shape and values, else 0 |
| `-.` | `1 - y` (any number, not only 0/1) | less: x's items that y has not. y's values are read at the rank one of x's items has, so `(i.3 2) -. 2 3` removes the ROW |
| `*.` | length/angle: the polar pair, as a new trailing axis | LCM (logical and on booleans; the Gaussian one on complex) |
| `+.` | real/imaginary: the rectangular pair, as a new trailing axis | GCD (logical or on booleans; `gcd 0 0` is 0; the Gaussian one on complex) |
| `~:` | nub sieve: 1 at each item that has not occurred before | ≠ |
| `~.` | nub: distinct items, first-occurrence order | — |
| `$` | shape of | reshape: x lays out y's ITEMS, so the result's shape is x followed by an item's shape (`$ 3 $ i. 3 4` is `3 4`) and `'' $ y` is y's first item. An empty y is refused, not filled, when the result needs an item it was not given (`2 3 $ i. 0`) |
| `,` | ravel | catenate along the LEADING axis; axes other than that one are overtaken to the larger length, which fills (`1 2 3 , i. 2 2` is 3×3), and a rank gap of any width makes the lower-ranked side one item of the answer. An operand with no elements takes the other side's type instead of clashing with it: `(0$'a') , 1 2 3` is `1 2 3` |
| `,.` | ravel items, exactly `,"_1`: each item ravelled, so a list becomes a column | stitch, exactly `,"_1` |
| `,:` | itemize: a leading axis of 1 (`2 3` becomes `1 2 3`) | laminate: the two arguments as the items of a new leading axis (two atoms give shape `2 1`) |
| `#` | tally; extended where the argument is | replicate: item i repeated x[i] times (a scalar x applies to every item, and a scalar y is repeated for every count, so `1 0 1 # 5` is `5 5`) |
| `#.` | base-2 decode (rank 1) | mixed-radix decode; a scalar x is the radix of every digit, a radix of 0 contributes none |
| `#:` | base-2 encode; the width fits the largest magnitude in the WHOLE argument, so the verb has infinite rank | mixed-radix encode; the digit axis is x's own shape, so `2 #: 5` is a scalar and `2 2 2 #: 5` a 3-list |
| `!` | factorial — gamma(y+1), always float; a negative integer is a signed infinity in J and a domain error in APL, which has no infinite value; a complex argument reaches gamma through the Lanczos approximation | binomial: x things chosen from y, defined through gamma on the reals and in the complex plane alike |
| `j.` | `0j1 * y` | `x + 0j1 * y` |
| `r.` | `^ 0j1 * y`: the unit complex at angle y | `x * ^ 0j1 * y`: polar coordinates |
| `":` | format: the characters that display the argument | format by specification: x is one complex `w j d` per column of y's last axis, or one for all of them — `w` the field width, `d` the digits after the point, and the rounding half-to-even. A width of 0 takes what the column needs, with one blank in front of every column but the first; a NEGATIVE width asks for the exponential form, written from the left behind one column of sign. A value too wide for its field is written as that many asterisks rather than refused, and a character or boxed argument is a domain error |
| `o.` | pi times y | circle function k (see below) |
| `{` | catalogue: one element from each item of y, in every combination — the shapes of the items make the result's shape and each element of it is one choice, boxed | from: each atom of x selects an item (negative from the end). A BOXED x is J's index specification: `<A` with a simple A reads A's last axis as one index per leading axis, the axes ahead of it framing the result; `<(c0;c1;…)` gives one component per axis, a scalar component dropping its axis and a boxed one meaning the complement — which is how `a:` selects a whole axis |
| `{.` | head | take (negative = from end; overtake fills) |
| `}.` | behead | drop |
| `{:` | tail (fill cell when there are no items) | — |
| `}:` | curtail | — |
| `\|.` | reverse the items | rotate axis k by `x[k]`, cyclically |
| `\|:` | transpose (reverse axes) | dyadic transpose: the axes x names move to the END in that order and the rest keep their order in front; a BOXED x groups axes, and the axes of one group are run together, which is the diagonal. An axis named twice is an index error, as it is in the reference |
| `i.` | integers (negative axis = reversed) | index of (absent gives the item count) |
| `i:` | steps: `-y` to `y` one apart, `1 + <. 2 * \| y` of them; a negative y counts down | index of the LAST occurrence (absent gives the item count) |
| `I.` | indices: index `i` repeated `y[i]` times (rank 1, so a table frames the rows) | interval index: how many items of the ascending x are strictly below each cell. Characters and symbols have an order of their own and are searched by it; the two sides must be the same kind |
| `e.` | raze-in: for every element of y, which items of `; y` it holds — the answer is shaped `($y), #items of the raze` | member: cells of x shaped like items of y |
| `E.` | — | find: 1 at each position of y where a copy of x begins, shaped like y's items; a pattern longer than y matches nowhere |
| `A.` | anagram index: where the permutation y's items RANK as stands among the permutations of that length, lexicographically | the x-th permutation of y's items; a negative x counts back from the last. Characters have no anagram index monadically, as in J |
| `C.` | a direct permutation as its cycles (each written from its largest element, the cycles ordered by those), or boxed cycles as the direct permutation. A list shorter than the permutation it names stands for one over `1 + >./ y` items | permute. A boxed x is cycles and leaves everything unmentioned in place; a numeric x is a direct permutation of y's items, ABBREVIATED where it is shorter — the items it never names come first, in ascending order, so `0 1 C. 'abcde'` is `cdeab`, `3 4 2 C. 'abcde'` is `abdec` and an atom is such a list of one. An element of a cycle is an index INTO y and counts back from the end where it is negative, so `(<_1 0) C. 1 2 3` is `3 2 1`; an index y has no item for is refused before the permutation is built |
| `u:` | codepoints become characters; characters are answered with themselves | form 3 gives codepoints, 10 the characters they name, 1 a codepoint modulo 256, 2 the same characters widened, 8 the UTF-8 bytes of a codepoint and 9 the codepoints a run of UTF-8 bytes spells |
| `;:` | words: J's own tokeniser over a string, one box per word. A run of numeric literals separated by blanks is ONE word (`;: '1 2 3'` has one), `NB.` swallows the rest of the line, and an unclosed quote is a parse error | the sequential machine — see below |
| `s:` | symbols: the argument's text, interned. A character LIST carries its own delimiter in its first position, so ``s: '`a`b'`` is the two symbols `` `a `` and `` `b `` while `s: 'a b'` is the one name `" b"`, and the empty list has no delimiter and no names. A character TABLE gives one name per row with trailing blanks trimmed, the leading axes becoming the result's shape. A BOXED argument gives one name per box, the characters taken exactly as they stand — trailing blank and all. Anything else is a domain error; a box holding a rank-2 array is a rank error | the name forms: `4 s:` lays the names out as a character table, blank-padded to the longest (the shape gains that width as a trailing axis), and `5 s:` boxes them one apiece, keeping the shape. `0 s:` … `3 s:`, `6 s:`, `7 s:` and `_1 s:` report on an interpreter's own symbol table — how many slots it holds, which are in use, how it hashes them — and are named gaps rather than guesses |
| `L.` | the boxing level: 0 for anything unboxed, one more than the deepest content otherwise. APL's `≡` counts the array itself as well, so the two differ by one on a simple array | — |
| `".` | do: the characters are compiled as a J program and run HERE, over the names the sentence itself can see — `". 'a =. 3'` assigns in the surrounding scope. A `{name}` hole inside the string has nothing to bind to and is refused | the numbers a line of text spells: the line is split at blanks and every word read as a J numeric literal, with the atom x standing in for a word that is not one. One word gives a scalar, as reading that line as a noun would, and several give a vector of that many. The right rank is 1, so a character matrix is read a row at a time and the rows framed with fills |
| `%.` | matrix inverse — the least-squares pseudo-inverse of a taller matrix; a wider one is refused, a singular one is a domain error | matrix divide: the least-squares solution of `y a = x` |
| `p.` | the roots of the polynomial whose ascending coefficients y holds, as the boxed pair `multiplier ; roots`, largest magnitude first, then largest real part, then largest imaginary part; roots that sit on top of one another are refined through the m-1st derivative, so a repeated one is exact; a boxed argument of that form converts back to coefficients, and the multiplier may go unsaid — one box is the roots alone, so `p. (<1 2)` is `2 _3 1` | the polynomial with ascending coefficients x, at y (Horner); a boxed x is the `multiplier ; roots` form of the same polynomial, the multiplier optional |
| `p..` | the derivative of the polynomial y's ascending coefficients describe, as coefficients; a boxed y is the root form and is differentiated through the coefficients it stands for | the integral, with x as the constant term; a boxed y is the root form here too, though its coefficients come out in floats where jconsole keeps exact rationals |
| `p:` | the y-th prime, counting from zero | the prime queries: `_1` counts the primes below y, `0` and `1` ask whether it is composite or prime, `2` gives the factorisation as a 2-row table and `3` its top row, `4` and `_4` step to the next and previous prime |
| `q:` | prime factors, ascending, with multiplicity (`q: 1` is empty), exact however many digits the number has. The whole argument is read at once — one row per item, padded with 1s | the exponents of the first x primes; a NEGATIVE x gives the last `\|x\|` primes that divide y over their exponents, as a 2-row table, and `__` gives all of them |
| `?` | roll: a random value below each element (`? 0` is a uniform double) | deal: x distinct values from `i. y` |
| `?.` | roll from a fixed seed, restarted on every invocation | deal from that fixed seed |
| `{::` | the map: y's box structure with every leaf replaced by the path that fetches it — a boxed list of one index per level descended, empty where the level is a boxed scalar | fetch: follow the path x into y, opening one level a step |
| `/:` | grade up (stable permutation). A BOXED argument orders by J's total array ordering: the type class (numeric, then symbol, then character, then boxed — and an empty array takes the lowest class whatever its type), then the rank, then the shape read with the LAST axis most significant, then the atoms in row-major order, a boxed atom recursively | x's items in the ascending order of y's |
| `\:` | grade down (stable permutation), the same ordering read backwards | x's items in the descending order of y's |
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

`u&.>` (each) is the one `&.` that is not built out of an inverse: it opens
every box, applies u, and boxes the result again — `# &.> 'ab';'cde'` is
`2;3`. Dyadically it pairs boxes at rank 0, so `1 ,&.> 1;2` extends the atom
over both. Every OTHER `u&.v` is `v^:_1 @: (u &: v)`, and reaches as far as
the obverse table does — see "The obverse table" below.

A word with only one valence implemented says so in the table above
as "not supported yet".

The constant nouns and verbs: `a.` is the 256 characters of J's alphabet in
codepoint order, `a:` is the ace (one box holding an empty numeric list),
and `_9:` … `9:` and `_:` are the constant verbs, which yield their noun
whatever the arguments are and in either valence. A constant verb is one
word: `2 3: 4` is 2, the verb `3:` and 4, while `3 : 'y+1'` — with a blank
between the digit and the colon — is still an explicit definition.

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
is an error, and `m} y` with a single index selects instead; a BOXED m is
the index specification `{` reads, so `99 (<a:;1)} i.3 3` replaces a whole
column). `u}` computes the indices instead of naming them: `u} y` is
`(u y)} y` and `x u} y` is `x (x u y)} y`. A GERUND operand amends only
dyadically — `` x u`v`w} y `` has u make the replacement, v the indices and
w the array they go into — while `` u`v`w} y `` amends nothing: it is the
noun amend's own monad, `(v y) m} (w y)`, a SELECTION at the indices v
gives from the array w gives, and u is not applied at all.
`f.` (fix) answers the verb
itself — names are substituted where they are used, so there is nothing
left to fix — and `M.` (memo) keeps every answer u has given and returns it
again for the same arguments, in a cache that belongs to the derived verb.
`m b.` is one of the thirty-two boolean functions: 0 to 15 are the truth
tables on two bits, and sixteen higher is the same function on every bit of
a pair of integers, so `17 b.` is bitwise and and `22 b.` bitwise xor.
`u b. 0` answers u's three ranks; the other characteristics are named gaps.
The ranks a derived verb reports are what everything that frames by it
reads, so they are part of its meaning and not a report about it: `u~` has
u's dyadic ranks EXCHANGED and an infinite monadic one — it takes on the
left what u takes on the right — which is what makes `(>.~)/~ 5 2 9` the
table rather than one elementwise pass, and `u :: v` has infinite ranks,
since which of the two will run is not settled until one of them fails.
`u :. v` runs u and so reports u's.
The dyad of `\.` is the outfix:
`x u\. y` applies u to y with each run of x consecutive items left out, so
there are `1 + (#y) - x` results. A piece of one item applies nothing —
`2 %/\. 'abc'` is `ca`, the characters never divided — and a piece of none
is the fold's identity. The exception is J's own: `+/`, `*/`, `<./`, `>./`
and `+./` have special code that types the whole argument before any piece
is cut, so those five refuse the same characters.

Conjunctions: `"` (rank: 1–3 atoms, `_` = infinite, or a VERB on the right,
which lends its own three — `u"v` is `u"(v b. 0)`, so `<"(+/)` boxes the
whole argument and `<"(<"1)` boxes each row); `@:` (atop: monad
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
reversed, and `x u;.0 y` takes the one block x describes — x is a vector of
sizes, or two rows of origins and sizes, and a negative size reverses that
axis; `x u;.3 y` and `x u;._3 y` tessellate: x is the block size, or two
rows of movement and size, and u runs on every block — `;.3` keeps the short
blocks at the far edge, `;._3` takes only the whole ones. A negative block
size reverses its axis there too where the movement row is written out.
With the movement left implicit a negative size does not measure the block
at all: the reference runs it to the END of its axis and reverses it,
whatever the magnitude said, so `_2 <;.3 i.6` and `_3 <;.3 i.6` answer the
same six reversed prefixes of the reversed argument and `_2 <;._3 i.6` the
one complete block. The cut's LEFT RANK is finite — 2 for the rectangle
and tessellation forms, whose cell is two rows of origins and sizes, and 1
for the interval forms, whose cell is one list of frets — so a longer left
argument is an ordinary frame of cuts, one cut per cell:
`(2 2 2$…) <;.0 i.5 5` is two blocks and `(2 3$…) <;.1 i.3 3` two
cuttings. A fret list with no frets in it is the whole argument
in ONE piece, and an empty fret list of rank 2 or more — J's per-axis form
with no axis named — is no piece at all. A BOXED left argument is that
per-axis form written out: one box of frets per leading axis, the rest of
the axes taken whole, and the result framed by how many intervals each
named axis was cut into — `((<1 0 1),(<1 0 0)) <;.1 i.3 3` is a 2 by 1
frame of blocks); `!.` (fit: on the verbs whose
meaning uses the comparison tolerance it replaces that tolerance, so `=!.0`
compares exactly; on a verb whose answer can reach past what its argument
holds it gives the FILL instead — the element that stands where the value
runs out. Those verbs are `{.` (`5 {.!.9 ] 1 2 3` is `1 2 3 9 9`), `$`
(which then stops repeating the ravel: `(2 2) $!.9 ] 1 2 3` is
`1 2 / 3 9`), `,` `,.` and `,:` (which pad a ragged join), `;` and `>`
(which pad the pieces they frame), `#` (which accepts one and has nothing
to put it in), and `|.` — `x |.!.f y` shifts
rather than rotates, an item moved past an end is dropped and the place it
left takes f, and the monad `|.!.f y` is `_1 |.!.f y`. A fill of a wider
type widens the answer (`5 {.!.9.5 ] 1 2 3` is a float list), one of
another kind entirely is refused, and a verb the reference gives no fit to
refuses one here too); `[:` (cap); `&.` and `&.:` (under, see
below); `::` (adverse: `u :: v` applies u, and if the LANGUAGE refuses it
applies v to the same arguments instead — a gap in libjay is a promise, not
an error a program may handle, and goes straight through); `:.` (obverse:
`u :. v` declares v to be what undoes u, which is all that `^:_1` and `&.`
need of it); `` ` `` (tie) and `@.` (agenda).

`u L: n` and `u S: n` apply u at a boxing level: u runs on every subarray
whose `L.` is n or below, and `L:` puts each answer back in the box its
operand came from while `S:` spreads them into the items of one array. A
negative n counts down from the argument's own level, and the two
infinities are the two ends of that descent: `_` is the whole argument,
however deeply it is boxed, so `# L:_ (1;2;<3 4)` is 3 where `# L:0` is
`1;1;2`, and `__` is its leaves, which is level 0 written the other way
round. Dyadically both
arguments are descended together and u is applied to each pair; a side that
has already reached its level is held while the other descends, so `1 ,L:0
(3;4)` reaches every leaf with the same left argument. Two sides that both
still have boxes must agree in shape.

A gerund is boxed data: `` u`v `` is one box per tied entity, each holding
that entity's ATOMIC REPRESENTATION. A primitive is its own spelling as a
character vector, a noun is the pair `('0'; <value)`, a train is
`('2'; <parts)` or `('3'; <parts)`, and anything a modifier derived is
`(spelling; <operands)` — so `` +`- `` displays as two boxes holding `+`
and `-`, and `` +/`- `` as the box tree for `('/'; <,<'+')` beside `-`.
Being data is the whole point: a gerund can be named, catenated onto,
computed and displayed like any other noun, and tying two nouns is just
catenating them (`` 0`1 `` is `0 1`).

Two conjunctions read it. `m@.n` picks a verb by a literal index and `m@.v`
runs v on the arguments and picks by its value, so `` (<.`>.)@.(2&<) ``
floors below 2 and ceilings above. `` m`:n `` is evoke, in the three forms
J gives it: `` `:0 `` applies every verb to the arguments and frames the
answers (`` ((+`-)`:0) 5 `` is `5 _5`), `` `:3 `` inserts the verbs between
the items of y, taking them left to right and cycling, and folding right to
left as insert does (`` ((+`*)`:3) 1 2 3 4 `` is `1 + 2 * 3 + 4`), and
`` `:6 `` is the TRAIN the gerund spells — a hook of two, a fork of three,
and longer ones grouped from the right. Any other number is a domain error,
as it is in the reference, and `` `: `` reads data and not a verb.

Four ADVERBS read a gerund too, and every one of them cycles through its
verbs left to right. `` u`v/ `` is `` `:3 `` written the short way: the
verbs go between the items and the fold runs right to left, so
`` (+`-)/ 1 2 3 `` is `1 + (2 - 3)`. With no items there is no insertion at
all and the answer is the identity element of the verb the fold would have
reached first (`` (*`-)/ i.0 `` is 1). The other three give one verb to
each PIECE, monadically: `` u`v\ `` to each prefix, `` u`v\. `` to each
suffix, and `` u`v/. `` to each diagonal of the oblique or, dyadically, to
each group of the key. The pieces have different lengths, so the answers
are framed and padded as any other list of cells is — `` (+`-)\ 1 2 3 ``
is a three-by-three whose rows are `+1`, `-(1 2)` and `+(1 2 3)`. The
DYADIC infix and outfix hand their verbs out the same way, one per window
(`` 2 (+:`*:)\ 1 2 3 4 `` doubles the first window and squares the second).
Under any other adverb a gerund is still a named gap.

Two CONJUNCTIONS hand a gerund out per piece as well. The cut gives one
verb to each piece the frets make — `` (+:`*:);.1 ] 1 2 3 `` is `2 4 6` —
and the rank conjunction gives one to each cell the rank names, so
`` (+:`*:)"1 ] 2 3$i.6 `` doubles the first row and squares the second.
The rank conjunction is the one place where the reading has to be chosen:
a boxed left operand is ordinarily the CONSTANT verb `m"n`, and it stays
that where the gerund holds one box (`` (<'+:')"0 ] 1 2 3 `` is that box
three times) or where the rank is infinite in all three places
(`` (+:`*:)"_ `` is the gerund itself, `` (+:`*:)"_ 0 `` cycles). The dyad
has no meaning: `` 1 (+:`*:)"0 ] 1 2 3 `` is a domain error, as it is in
the reference.

The representation is reconstructed from the verb tree rather than kept
from the source, so the spellings that differ only by the rank they set are
recovered by matching that rank: `u@v` is `u@:v` at v's ranks, `u&v` and
`u&.v` likewise. Two do not survive the trip and are named rather than
guessed at: a capped fork (`[: f g` is an atop by the time the tree has it,
so it writes itself out as `@:`) and any verb libjay has no J spelling for,
which reports "the atomic representation of … is not supported yet".

### The inner product (`u . v`, APL `f.g`)

`x u . v y` pairs x's LAST axis with y's FIRST and leaves what is left of
each as the shape of the answer, which is the one rule behind the matrix
product `+/ . *` (`+.×`), the row match `*./ . =` (`∧.=`), the shortest-path
step `<./ . +` (`⌊.+`) and the rest. An argument of any rank goes through
it: `(i.2 3 4) +/ . * i. 4 5` is shaped `2 3 5`. A scalar on either side
stands for as many copies of itself as the shared axis asks for.

The two languages part on how the operand is applied, and both readings are
implemented. J takes x in cells at v's dyadic LEFT rank — or at rank 1
where that rank is smaller — hands the WHOLE of y to v once per cell, and
applies u MONADICALLY to what comes back: so `+/ . ,` catenates a row onto
the whole table (`(i.2 3) +/ . , i.3 2` is a 3-list), and u need not fold at
all — `(i.2 3) <. . * i.3 2` is shaped `2 3 2`. APL's operand takes one
vector from each side: the vector along x's last axis and the vector along
y's first, so `(2 2⍴⍳4)+., 2 2⍴⍳4` catenates two 2-lists and folds the four
elements. Where g is a scalar function — which is every published use — the
two readings are the same value, and libjay computes it by the J route,
under the leading-axis pairing that route needs.

The two APL lines part again over where the EACH in the definition sits,
which `Dialect.inner_each` names. GNU APL puts it on the fold — `f/¨
(⊂[last]x) ∘.g (⊂[first]y)` — so g meets a whole vector from each side and
what the fold makes of a pair is enclosed once more. Dyalog puts it on the
pairing — `f/ row g¨ column` — so g meets one ELEMENT from each side and
the fold's own value stands as the cell. `1 2+.,3 4` is `10` under the
first and an enclosed `3 7` under the second; `(2 2⍴⍳4),.,2 2⍴⍳4` opens
with `1 2 1 3` there and `1 1 2 3` here. Every scalar g whose fold ends in
a number agrees under both, which is why `+.×` is one sentence in either
reading and John Scholes' Life one-liner differs only in depth.

`+/ . *` and `+.×` over real machine numbers do not go through the cell
machinery at all: the product runs as a blocked pass over the two buffers,
blocked on the shared axis so that a slice of y is reused across a block of
output rows, split by whole rows across the thread pool, and compiled once
per CPU feature level like every other hot loop. Whole numbers stay whole,
with one bound computed up front deciding whether the vectorising loop can
overflow at all; where it might, a checked loop runs and leaving i64
anywhere sends the whole product to floats, as it does everywhere else.
Neither argument's layout matters — a column-major block is materialised by
the same rule any other verb uses.

Monadically — J's alone; APL gives `f.g` no monadic valence — `u . v y` is
the determinant by minors down the FIRST column: for each row in turn, that
row's leading element under v with the determinant of the table the row and
the column leave behind, all folded by u. `-/ . *` is the determinant
proper and `+/ . *` the same expansion without the alternating sign. Two
base cases finish it: with no columns left the value is v's identity
element (so `-/ . * 0 0 $ 0` is 1), and with no rows left it is u over
nothing (so `-/ . * 0 2 $ 0` is 0). The argument is read as a table of
items, so a list is a single column and `-/ . * 1 2 3` is `-/ 1 2 3`; the
monad's rank is 2, so an argument of higher rank gives one determinant per
table.

`-/ . *` over machine numbers goes by Gaussian elimination with partial
pivoting instead, which is what the reference does from three rows up — and
why its answer there is a float even where every element is whole. Every
other combination expands by minors, memoised on the set of rows still in
play, which is `2^n` work rather than `n!`; past 16 rows the diagnostic
names the limit rather than running out of memory. The exact types take
that route too, so an extended argument gives an exact determinant.

### The sequential machine (`x ;: y`)

The dyad of `;:` runs a table-driven state machine over y. x is the boxed
description `f ; s ; m ; ijrd`, of which `m` and `ijrd` may be left off:

- `s` is the transition table, shaped `p q 2`. At state `r` and input class
  `c`, `s[r;c;0]` is the state to go to and `s[r;c;1]` the output code.
- `m` maps an input element to its class, indexed by the character's
  codepoint — `' ' = a.` classifies blanks as 1 and everything else as 0.
  With no map at all a numeric argument IS the classes; a map over a
  numeric argument is a named gap.
- `ijrd` is four numbers, `0 _1 0 _1` by default: where to start reading,
  where the word in hand began (`_1` for none), the starting state, and
  what the end of the input does — a class to make one last transition
  with, or `_1` to end the word in hand and stop.
- `f` picks the answer: `0` the boxed words, `1` their elements catenated,
  `2` each word's position and length, `3` the table position that ended
  it (`c + q*r`), `4` both of those, `5` the whole trace, one row per
  transition holding `i`, the word's start, the state, the class, and the
  table entry those two chose.

The output codes are `0` (nothing), `1` (a word starts here), `2` (end the
word and start another here), `3` (end the word), and `6` (stop). Codes 4
and 5 emit a VECTOR rather than a word; that reading is named rather than
guessed at. Ending a word before one has begun is an error, as it is in the
reference.

### The obverse table

`u&.v`, `u&.:v` and the negative powers `u^:_n` all rest on one question:
what undoes v? libjay answers it from a table rather than by searching, and
the table holds the verbs whose inverse is another verb it can already
write down. The reference is the whole of the specification for it: every
row below is a row the reference names when asked `v b. _1`, and a verb it
refuses to name is refused here too.

- self-inverse: `+` `-` `%` `-.` `|.` `|:` `/:` `C.` `%.` `p.` `]` `[`.
  Grading a permutation gives the permutation that undoes it, `C.` converts
  between the direct and cycle forms, a matrix inverse and a set of
  polynomial roots return where they came from, and the identity verbs are
  identity either way round.
- paired: `^`/`^.`, `*:`/`%:`, `+:`/`-:`, `>:`/`<:`, `<`/`>`, `#.`/`#:`,
  `,:`/`{.` (itemise and head), `":`/`".` (format and evaluate — in APL
  `⍕` and `⍎`).
- built from other verbs: `\:` is `/:@|.`; `q:` is `*/"1`, since a list of
  prime factors multiplies back into its number; `;:` puts a blank after
  each word and razes them, less the trailing blank; `o.` multiplies by the
  reciprocal of pi; `j.` is `-@j.` and `r.` that same quarter turn applied
  to the logarithm; `+.` and `*.` split a complex number into a pair of
  reals, and the pair folds back together under `j./"1` and `r./"1`.
- carrying a form number: `x:` is `_1&x:`, `u:` is `3&u:`, `s:` is `5&s:`.
- spelled only as a negative power, in the reference as here: `p:^:_1` is
  how many primes stand below y (which sends the y-th prime back to y),
  `I.^:_1` is the counting vector `I.` was given, and `$.^:_1` is the dense
  form of a sparse array.
- dyad only: `x # y` is undone by the expansion `x #^:_1 y`, which puts the
  items back where the ones stand and a fill where each zero was. `# y`
  counts, and a count cannot be undone, so the entry has no monad —
  jconsole refuses to NAME this one (`# b. _1` is a domain error there)
  while answering `#^:_1` itself, and libjay names it.
- bonded arithmetic: `n&+` and `+&n` are both undone by `-&n`, `n&*` and
  `*&n` by `%&n` — the noun comes off the RIGHT whichever side it was bonded
  to — plus `^&n` (the n-th root), `n&^` (the base-n logarithm), `%:&n` and
  `^.&n`, which pair with one another, `n&o.` (the circle functions are
  numbered so the negative index is the inverse), and `n&-` and `n&%`,
  which undo themselves. `u~&n` inverts as `n&u` does; `n&u~` the reference
  gives no obverse, and neither does this.
- bonded rearrangement: `n&|.` rotates the other way, `n&}.` takes back what
  it dropped (the vacated places taking a fill), `n&,` and `,&n` drop as
  many items as the noun brought, `n&#` keeps the expansion, `n&#:` is
  `n&#.`, and `n&A.` and `n&C.` invert through the permutation they make of
  `i. # y`. `n&#.` reads digits in base n, so undoing it writes them back —
  in as many places as the largest value asks for, which is what makes the
  round trip land on the width the reference chooses.
- the running folds: `+/\` inverts into the differences between neighbours
  (`- |.!.0`, the hook), `*/\` into the quotients, and the suffix forms `+/\.`
  and `*/\.` into the same against the neighbour on the other side. `-/\`
  and `%/\` alternate, so their answers carry one further pass over the
  signs `1 _1 1 _1 …`. `*/` is undone by `q:`.

Everything built out of those inverts by inverting its parts: `u@:v` and
`u&:v` invert in the other order, `u"r` and `u!.n` keep their modifier,
`u&.>` and `u¨` turn round only the verb inside the box, `u^:n` becomes the
obverse applied n times (and `u^:_n` becomes `u^:n`), and `u :. v` supplies
an answer where the table has none.

Which row a NEGATIVE power reads depends on the valence it is applied in,
so the table is consulted when the arguments arrive and not when the
sentence compiles. `u^:_1 y` undoes u. `x u^:_1 y` undoes the BOND `x&u`,
which is a different verb and often a different answer: `2 *^:_1 6` is 3
because `2&*` is undone by `%&2`, though `*` itself — signum — has no
obverse at all, and `3 |.^:_1 y` rotates back rather than reversing. A verb
neither reading can turn round is named when the sentence runs, which is
where the reference names it too.

Two unders are not built out of an inverse at all. `u&.>` is the each: open
each box, apply u, box the result again. `u&.,` is the other — `,` has no
obverse, since a ravel says nothing about the shape it came from, but the
shape is in hand while the sentence runs, so `u&., y` is u over the ravel
reshaped to y's own shape. Like the reference, it has one valence only, and
`,^:_1`, `, b. _1` and `u&.:,` stay refusals.

Two rows the reference has and libjay does not, and they say so by name:
`!^:_1`, whose Newton iteration the reference runs in the COMPLEX plane
(`!^:_1 _1` is `8.91115j18.2226` there) and which waits on a complex gamma
function — the same gap as `! 3j4`; and `|.!.f`, whose obverse the
reference answers with `]`, an identity that does not undo the shift it is
the obverse of.

A verb the table does not reach says so by name — "the obverse of (+/ % #)
is not supported yet" — rather than guessing at a numerical inverse.

Three things read the table besides `&.`: J's `u b. _1`, APL's `⍢` (under),
and `⍣¯1`. `u b. _1` answers a SPELLING rather than the verb, and libjay
writes its own: where the reference prints `0.318309886183790691&*` libjay
prints `(n&*)`, because a derived verb's name says `n` for a noun operand.
The rows whose spelling the two write alike are in
`tests/corpus/j/obverses.txt` by name; the rest are asked there with `^:_1`
and `&.`, which compare values.

The table is one table over both languages, so a row J's `^:_1` reaches is
reachable from APL's `⍣¯1` too. Three of them Dyalog does not hold — `⍋⍣¯1`,
`⍒⍣¯1` and `○⍣¯1` of a zero — and `tests/expected/dyalog.txt` names them.

`". y` (do) compiles the characters of y as a J program and runs it in the
sentence's own scope: the names it can see are the names the sentence can
see, in both directions, so `". 'a =. 3'` assigns where it stands. It
reaches nothing the caller could not reach — the sandbox contract is about
what a primitive may touch, and evaluation touches nothing new — and a
`{name}` hole inside the string has nothing to bind to and is refused. A
diagnostic from inside is reported at the sentence that ran the string,
with the inner one carried as a note, because the inner spans point into a
source the caller never sees. APL's `⍎` is the same verb.

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
Naming a bare adverb or conjunction — `m =. /`, `c =. @` — works the same
way and for the same reason: a modifier is applied while the sentence
holding it is parsed, so the name has to be resolved then, and from the
next sentence on it parses exactly as the glyph would. What is still a
named gap is WRITING a new modifier (`1 :`, `2 :`) and the tacit translator
(`13 :`), and displaying a bare verb or modifier name. The explicit
definitions themselves — `3 : '...'`, `4 : '...'`, `{{ }}` — work; see
"Explicit definitions" below.

## APL

### Which APL

The reference is GNU APL 2.0, which embodies the APL2/ISO line; the
semantics libjay implements are that line, verified differentially against
it (`crates/libjay/tests/oracle_apl.rs`, corpus in
`crates/libjay/tests/corpus/apl/`). Every Dyalog-side cell below was originally written from Dyalog's
published documentation; as of 2026-08-22 the recording below has
checked them against a running Dyalog.

Dyalog IS recorded: Dyalog 20.0 (the official Docker image, run as a
quarantined black box like every oracle, on the recording machine only)
answered the whole corpus on 2026-08-22 into its own `dyalog:` column of
the same snapshots (docs/testing.md). Under the shipped dialect libjay
agrees with it on 1771 of 1989 expressions; under `Dialect::dyalog()`, on
1915. `jay-corpus stats apl --dialect-diff [--dialect gnu|dyalog]` replays
the recorded column under either preset and lists every disagreement — it
runs no interpreter, so it needs no Docker. Neither number is a gate: the
gate is GNU APL. `corpus/apl/dyalog-probe.txt` is the theme aimed at the
table below. Where a recording contradicts a cell here, the recording wins
and the cell changes.

Every place the two lines are known to diverge, and which one libjay
follows today. Rows marked "verified against the oracle" were re-checked
directly against `LIBJAY_ORACLE_APL` while writing this table, not just
read off an old note:

| Feature | APL2 / GNU APL (oracle) | Dyalog 20.0 (recorded) | libjay follows |
|---|---|---|---|
| monadic `↑` | first: the first element of the ravel, disclosed | mix / disclose | BOTH — `Dialect.first_disclose`: `↑1 2 3` is `1` by default, `1 2 3` under `Dialect::dyalog()` |
| monadic `⊃` | disclose / mix: items combined into one array, filled | first: pick the first item, disclosed | BOTH — the same setting, the same swap: `⊃(1 2)(3 4)` is a 2×2 array by default and `1 2` under the Dyalog preset |
| dyadic `⌷` | one scalar index per axis of y, all of them named | one index per LEADING axis; an enclosed one keeps its axis, and a shorter index takes the trailing axes whole | BOTH — `Dialect.index_form`. `2⌷3 3⍴⍳9` is a rank error by default and `4 5 6` under the Dyalog preset; `(⊂2 3)⌷3 3⍴⍳9` is the last two rows there and a rank error here, verified against both oracles |
| `⊂`/`⊆` dyadic | `⊂` is the partition: opens where the left argument's flags rise, and a zero drops its item | that partition is spelled `⊆`; dyadic `⊂` is partitioned enclose, where the left argument COUNTS the partitions to open before each item — `1 0 1⊂1 2 3` is `(1 2)(3)` there and `(1)(3)` here | BOTH — `Dialect.partition`. `⊆` is the partition in either reading; only `⊂` moves |
| `⊂5` (enclosing a simple scalar) | identity: `5`, depth 0 | identity, same | both — not a divergence, listed because it underlies the row above |
| `⎕CT` default | `1e¯13`, scaled by the LARGER magnitude — verified against the oracle | `1e¯14`, the same scaling: no recorded answer separates the two rules, and both references scale by the larger | BOTH — the value is what the preset changes |
| dfn return value | the LAST statement — verified against the oracle (`2×{⍵+1⋄⍵+2}5` is `14`, i.e. `2×7`, not `2×6`) | the first statement that is not an assignment | BOTH — `Dialect.dfn_result`. A guard is not that statement: it answers only when it holds, and neither is `⎕←`, which assigns to `⎕` — `{⎕←⍵⋄⍵+1}5` prints `5` and answers `6` in both readings, verified against Dyalog. GNU APL also *echoes* every unassigned statement's value to stdout on its way through, which is a separate display quirk, not a different return value |
| `⍺←` inside a dfn | assigns UNCONDITIONALLY — verified against the oracle (`F←{⍺←10⋄⍺+⍵}⋄3 F 5` is `15` there) | a DEFAULT: fills `⍺` only where no left argument arrived | neither running oracle — the published dfn model, which is `8` here, not `15` |
| ordering `< ≤ > ≥` over characters, mixed pairs and complex numbers | TOTAL: a character orders by its codepoint and stands below every number, and a complex value orders by its real part then its imaginary one — verified against the oracle (`2J3<2J5` and `'a'<1` are both `1` there) | refuses all three, as the standard does | BOTH — `Dialect.order_domain`. `'b'<'c'` is `1` by default and a DOMAIN ERROR under `Dialect::dyalog()`. `⌈` and `⌊` are not comparisons and stay numeric in either reading, as both references have them |
| negative replication count in a VECTOR (`¯1 2/1 2`) | a LENGTH ERROR — restricts a negative count to a scalar left argument, verified against the oracle | legal: a run of fills, the general rule | neither running oracle — the general APL2/Dyalog rule, which is `0 2 2` here |
| trains `(f g h)` | not in APL2, not in GNU APL — a SYNTAX ERROR, verified against the oracle | a Dyalog 14+ feature | DYALOG, as an extension shipped on by default (`Dialect.trains`); `Dialect(trains=False)` gives GNU APL's refusal back |
| tacit / function assignment `F←+/` | a SYNTAX ERROR, verified against the oracle | supported | DYALOG, under the same `trains` setting: a function may stand where a value belongs, or it may not, and one flag decides both |

Two of the rows above are where libjay follows neither running reference:
GNU APL's own implementation departs from the ISO/APL2 line it otherwise
embodies (`⍺←` and the vector replication count), and libjay follows the
published rule — which happens to be Dyalog's rule too in both cases —
over the oracle's quirk. Everywhere else in this codebase, GNU APL wins:
these two are pinned as deliberate exceptions in
`crates/libjay/tests/corpus/apl/divergences.txt` precisely so a silent
convergence gets noticed. The 2026-08-22 recording confirms both: Dyalog
answers `8` to the `⍺←` sentence and `0 2 2` to `¯1 2/1 2` — libjay's own
two answers. Ordering was a third such row until the 2026-08-28 manual
crawl; it is now a dialect setting instead, with GNU APL's total order the
shipped reading and the narrow one a preset away. The dyadic `⌷` divergence between GNU APL and
Dyalog runs the other way from what the glyph table might suggest: it is
GNU APL, not libjay, that refuses Dyalog's enclosed-index-vector form.

Dyalog-only features libjay already ships as extensions, implemented from
published documentation with no oracle to check them against — hand-tested
instead, and marked 🟡 "no oracle" in [status.md](status.md): `⊆` (monadic
nest, dyadic partition — see above), `∘` (beside), `⍥` (over), `⍛`
(before), `⍢` (under), `⌺` (stencil), `f⍤g` (atop with a function operand —
the rank-specification form
of `⍤` does have an oracle), `⌸` (key), dfn guards (`cond:expr`), `∇`
self-reference, dfn operators (`⍺⍺`/`⍵⍵`), and the tradfn control
structures (`:If :While :Repeat :For :Select`, `:Return :Leave :Continue`),
TRAINS and FUNCTION ASSIGNMENT.
Monadic `↓` (split) and monadic `⌷` (materialise) are in the same "no
oracle" boat for the same reason — GNU APL lacks the valence outright — but
read from the published Dyalog and ISO definitions together, not from
Dyalog alone.

Trains and function assignment are the two of those that a GNU APL run CAN
answer — it refuses them — so they are pinned in
`crates/libjay/tests/corpus/apl/divergences.txt` alongside the deliberate
divergences above, and the shapes themselves are hand-tested in
`crates/libjay/tests/wave6.rs`. They are one setting, `Dialect.trains`,
because they are one question: may a function stand where a value belongs?
It ships on. A feature the oracle merely LACKS is not a reason to withhold
it; a feature the oracle ANSWERS DIFFERENTLY is, and those still follow GNU
APL. `Dialect(trains=False)` restores the strict sentence, and both
readings are implemented — it is the one setting on the object that is a
choice rather than a gap.

A train is a run of functions where a value belongs, and it is a function
itself. It forms inside parentheses, and on the right of an assignment,
which is the whole of the rule: nowhere else does APL leave a function
standing. Two tines are an ATOP — `(g h) ⍵` is `g (h ⍵)` and `⍺ (g h) ⍵` is
`g (⍺ h ⍵)`. Three are a FORK — `(f g h) ⍵` is `(f ⍵) g (h ⍵)` and
`⍺ (f g h) ⍵` is `(⍺ f ⍵) g (⍺ h ⍵)`. The leftmost tine may be a VALUE,
which stands where `f ⍵` would: `(3+×) 4` is `3 + × 4`. Longer runs group
from the right — an odd count forks its first two tines over the train the
rest makes, an even one atops its first over that train — so `(f g h j k)`
is `(f g (h j k))` and `(f g h j)` is `(f (g h j))`. `⊢` and `⊣` are the
identity tines. These lower to the same `Fork`, `Atop` and `NounFork` the
J frontend builds, so no new semantics reach the engine; a value tine has
to be a literal, exactly as J's noun fork does. `F←+/÷≢` then names the
train, and `F←+/` names a derived function, by the same machinery that
already names a dfn.

The Dyalog dialect is a preinitialised `Dialect` object chosen at compile
time, the same way `⎕IO` is chosen — never global state, never a guess
from the source text: `Dialect::dyalog()` in Rust, `APL.Dialect.dyalog`
in Python, `--dialect dyalog` when the corpus tooling measures it. Two
more rows are not in it, and are the reason it is a preset rather than a
second language: `≡` negates the depth of an array whose items do not
share one (`≡1(2(3 4))` is `¯3` there, `3` here — `Dialect.depth_sign`,
and items of one depth and different lengths stay uniform, which
`1 2∘.⍴3 4` pins), and a nested grade is the total array ordering
(`Dialect.nested_grade`), which compares two arrays over the shape that
covers both — the lower rank gaining leading 1s, each axis taken to the
longer of the two, a position one array lacks sorting below every value
there is, and numbers before nested values before characters where no
atom separates them. That comparator was derived from the recorded
answers in `snapshots/apl/grade.snap`, which is what pins it. A third is
`Dialect.gcd_rule`: GNU APL's `∨` hands a zero argument's whole partner back
with its sign and rounds a near-whole or vanishing argument before the
Euclid runs, and Dyalog does neither (`¯3∨0` is 3 there, `¯3` here).

Seven more rows are the preset's. `Dialect.lookup_left`: dyadic `⍳` and `⍸`
search their left argument's MAJOR CELLS, so a matrix looks up rows and
answers one number per cell of the right argument, and a scalar — having no
cell — is a rank error, where the APL2 line searches the ELEMENTS of a left
argument of any rank. `Dialect.axis_counts`: `↑` and `↓` take a left
argument shorter than the rank, the counts applying to the leading axes and
the rest being taken whole or dropped from not at all, so `2↑matrix` is the
first two rows. `Dialect.axis_order`: the axes of `⌷[K]` and of a scalar
function's `f[K]` pair with what accompanies them in the order K was
written, so `2 1⌷[2 1]M` means `1 2⌷[1 2]M`, where the APL2 line reads K as
a set and pairs them ascending, making the same sentence `2 1⌷[1 2]M`.
`↑` `↓` `,` and `⊂` keep the order written under both. `Dialect.unique_mask`: monadic `≠` marks major cells and
always answers a vector as long as `≢Y`, where the APL2 line marks the
elements and keeps the shape. `Dialect.expansion`: dyadic `\` takes any
integer count list — a positive count repeats that item, a negative one
leaves that many fills, 0 means `¯1`, and the result is `+/1⌈|X` items long
— where the APL2 line takes a boolean mask alone. `Dialect.where_rank`:
monadic `⍸` gives a rank-0 argument an empty index vector, so `⍸1` is a
one-item nested vector. `Dialect.format_spec`: dyadic `⍕` rounds a half on
the shortest decimal that names the value rather than on the double scaled
by the precision, keeps a one-digit mantissa's point, pads the scaled
form's exponent out to four characters under a given width, and fills a
field too narrow with asterisks rather than refusing.

Three more are the tolerance readings: `Dialect.near_count` (a float near
a whole number is admitted as a count relatively, scaled by `⎕CT`, rather
than within an absolute `1E¯10`), `Dialect.floor_rule` (`⌊` and `⌈` scale
their step by the magnitude) and `Dialect.encode_digits` (`⊤` takes its
digits exactly rather than as tolerant residues).

Two more are structural: `Dialect.inner_each` (an inner product puts
the each on the PAIRING rather than on the fold — see "The inner product"
above) and `Dialect.control_strictness` (a control structure's condition is
a single value, and `:Leave` belongs to a loop).

What the preset still leaves at the GNU reading, and what that costs
against the recording, is the table in [status.md](status.md), "APL — the
Dyalog line". The largest item is `⎕R`/`⎕S`.

| Glyph | Monadic | Dyadic |
|---|---|---|
| `+` | identity | plus |
| `-` | negate | minus |
| `×` | signum | times |
| `÷` | reciprocal (`÷0` is a domain error, as the dyad is) | divide (float; `0÷0` is `1`, `n÷0` is a domain error) |
| `*` | exponential | power; a negative base with a fractional exponent gives a complex answer |
| `⍟` | natural logarithm; a negative argument gives a complex answer | logarithm to base x; the same |
| `⌈` | ceiling | max |
| `⌊` | floor | min |
| `\|` | magnitude | residue |
| `=` `<` `≤` `>` `≥` | — | comparisons (0/1). `< ≤ > ≥` are TOTAL in this line: a character orders by its codepoint and stands below every number, and a complex value orders by its real part then its imaginary one. `Dialect.order_domain` names the narrow reading, where each of the three is a domain error. `⌈` and `⌊` are not comparisons and stay numeric under either |
| `⊤∧` `⊤∨` `⊤⍲` `⊤⍱` `⊤=` `⊤≠` | the argument as the integer it stands for (`⊤∧`, `⊤∨`) or every bit of it complemented (`⊤⍱`); the other three have no monad | the logical operation on every bit of two 64-bit two's-complement integers, so `12 ⊤∧ 10` is 8. `⊤` and the glyph after it are one function, blanks between them included; an argument that is not a whole number within `⎕CT`, or does not fit in 64 bits, is a domain error |
| `≠` | unique mask: 1 at each ELEMENT, in ravel order, that has not occurred before, keeping the argument's shape. `Dialect.unique_mask` names Dyalog's reading, one bit per major cell and always a vector as long as `≢y` | not equal (0/1) |
| `∧` | — | LCM (logical and on booleans; the Gaussian one on complex) |
| `∨` | — | GCD (logical or on booleans; the Gaussian one on complex) |
| `~` | not (the argument must be 0 or 1) | without: x's items that y has not |
| `⍴` | shape of | reshape: x lays out y's ELEMENTS, cyclically, which is where APL and J part company above rank 1. An empty y fills with the type's fill |
| `⍳` | index generator (respects `⎕IO`): one length gives the counting vector, two or more give an array of that shape whose elements are the boxed coordinate vectors | index of (respects `⎕IO`; absent gives `⎕IO + ≢x`). The items of a left argument of ANY rank are searched; above rank 1 that search is per ELEMENT and each answer is the enclosed coordinate vector that finds it, or the enclosed empty vector where it is absent, so `(2 2⍴⍳4)⍳3` is the enclosed `2 1`. `Dialect.lookup_left` names Dyalog's reading, where the left argument's MAJOR CELLS are searched instead — a matrix looks up rows, the result shape is `(1-⍴⍴x)↓⍴y`, and a scalar is a rank error |
| `⍉` | transpose | dyadic transpose: x says, for each axis of y in turn, which axis of the RESULT it becomes; a destination two axes share runs them together, which is the diagonal, and every axis of the result must be named |
| `↓` | split: the vectors along the LAST axis, each enclosed, laid out in the remaining axes' shape (no oracle — see "Which APL" above) | drop: one count per axis. `Dialect.axis_counts` names Dyalog's reading, where fewer counts leave the trailing axes alone |
| `,` | ravel | catenate along the LAST axis. Axes other than that one must conform: APL refuses the ragged case that J fills, which is where the two rules part company |
| `⍪` | table: one row per item, holding that item's elements (a scalar gives 1×1, a vector n×1) | catenate along the LEADING axis |
| `!` | factorial (always float); a value with an imaginary part reaches the complex gamma function, and one without is the real it displays as | binomial, J's argument order; the same function |
| `⍕` | format: the characters that display the argument | format by specification: x is one width-and-precision pair per column of y's last axis, one pair for all of them, or a lone precision. A width of 0, given or left out, is the width the column needs plus a separating blank. A NEGATIVE precision is the scaled form, with that many mantissa digits and the exponent after an `E`. A half rounds away from zero. A value that does not fit its field is a domain error; a nested y is a named gap. `Dialect.format_spec` names Dyalog's reading of the four rules that part |
| `⊥` | — | mixed-radix decode; with no digit to weigh the radix is never read, so `'a'⊥(0⍴0)` is the empty sum 0 whatever the radix was written as |
| `⊤` | — | mixed-radix encode; with no value to write the radix is never read either, so `'a'⊤(0⍴0)` is the empty. `A⊤[N]B` encodes to N copies of the single radix A, N counted from one whatever `⎕IO` is; `A⊤[0]B` works the width out — the smallest that reaches the largest value, and one digit more when any value is negative |
| `⌽` | reverse each row (last axis) | rotate each row (last axis) |
| `⊖` | reverse the items (leading axis) | rotate the leading axis |
| `≢` | tally | not match |
| `∊` | enlist: every leaf element, in ravel order, as a vector | membership, element by element (an element of a nested array is a whole array) |
| `⊂` | enclose — except that a simple scalar is its own enclosure, so `⊂5` is `5` | partitioned enclose: a partition opens where x rises (`x[i] > x[i-1]`, reading `x[¯1]` as 0) and an item flagged 0 is dropped. Rank 2 and above partitions the LAST axis, once per cross section, and the axes ahead of it frame the answer. No flag against no item is the empty nested vector, and nothing about the flags has to be a flag; where there ARE items the flags are read as always |
| `⍸` | where: index `i` repeated `y[i]` times, from `⎕IO`; a rank-2 or higher argument gives one boxed coordinate vector per occurrence. `Dialect.where_rank` names Dyalog's reading, where an index is a vector as long as the rank at rank 0 too, so `⍸1` is a one-item nested vector holding `⍬` | interval index: how many items of the ascending x are at or below each cell, plus `⎕IO - 1`. The interval is closed on the left here and open in J's `I.`: `1 3 5⍸3` is 2 where `1 3 5 I. 3` is 1. `Dialect.lookup_left` names Dyalog's reading, where the bounds are the left argument's MAJOR CELLS — a matrix of them searches rows — and a scalar is a rank error |
| `⌷` | materialise: the argument itself (no oracle — see "Which APL" above) | index: one item of x per axis of y, and the count must equal the rank. An item is a scalar, which drops its axis, or an ENCLOSED vector, which keeps it and selects that many — `(⊂1 2)⌷5 6 7 8` is `5 6` |
| `⌹` | matrix inverse — the pseudo-inverse of a taller matrix; wider is refused, singular is a domain error. APL's `⌹` reads the whole argument, so rank 3 or more is a rank error; J's `%.` has rank 2 and runs over the 2-cells | matrix divide: the least-squares solution of `y a = x`, under the same rank rule. J's `%.` takes its right-hand SIDE whole — its left rank is infinite — so one of rank 3 or more is solved as one column per element of an item and the answer keeps every axis but the leading one; APL's `⌹` refuses that shape, as its reference does |
| `?` | roll: a random value in `⎕IO .. ⎕IO+y-1` (`?0` is a domain error) | deal: x distinct values from that range |
| `⊃` | disclose: the items mixed into one array, filled where their shapes differ; a character item beside a numeric one gives a MIXED SIMPLE array, each cell padded with its own prototype | pick: each item of x is one LEVEL of a path, and holds one index per axis of the value at that level — so an empty index picks from a scalar and nothing else, and `(2 2)⊃matrix` asks for two levels where `(⊂2 2)⊃matrix` asks for one two-axis index. Indices count upwards from `⎕IO`; one below it is out of range, not an index from the end |
| `↑` | first: the first element of the ravel, disclosed; the type's fill when there is none. Under `Dialect.first_disclose` it is MIX instead, which is `⊃`'s monad here | take; overtaking a NESTED array fills with the first item's prototype — that item's shape with a zero for every number and a blank for every character, nested to the same depth. One count per axis; `Dialect.axis_counts` names Dyalog's reading, where fewer counts take the trailing axes whole |
| `≡` | depth: 0 for a simple scalar, 1 for a simple array, one more than the deepest box | match: same shape and values, else 0 |
| `∪` | nub: distinct items, first-occurrence order | union: x's items, then y's items that are new. Only the right argument is sieved, so x keeps whatever repeats it has |
| `∩` | — | intersection: x's items that y also has, in x's order |
| `⍲` `⍱` | — | nand / nor; both arguments must already be 0 or 1 |
| `⍷` | — | find: 1 at each position of y where a copy of x begins |
| `⍎` | execute: the characters are compiled as an APL program and run HERE, over the names the sentence itself can see. An EMPTY program — `⍎''`, or an empty of any other type — yields no value at all in GNU APL; a libjay verb has no way to answer that, so it refuses and says the executed string yielded no value | — |
| `⎕UCS` | codepoints become characters, characters become their codepoints | — |
| `⎕CC` | the characters of the numbered class — the digits (1, 10), the two cases of the Latin (2, 26, 3, ¯26, 52) and Greek (48) alphabets, ASCII (4, 128), the printable range (95), the octal (8) and hexadecimal (16, ¯16, 17) digits, and the RFC 4648 alphabets (33, 65). Several numbers give one class per item, nested. The four glyph repertoires the reference states — superscripts (5), subscripts (6), the box-drawing frame (7) and the mathematical symbols (9) — are the reference's own tables, and 7 and 9 are the 6-by-10 and 4-by-7 MATRICES they are there, not vectors | — |
| `⎕FX` | fix a definition from its lines and answer with its name; the lines must be literal text (see below) | — |
| `⎕NC` | the class of every name in the argument: `¯1` not a name at all, `0` a name with nothing in it, `2` a variable, `3` a defined function, `5` a system variable, `6` an argument of a `{…}`. A character vector is one name and a character matrix one per row; a row's trailing blanks are not part of the name it holds, and an empty argument of any type names nothing. Anything else is a domain error | — |
| `⎕CR` | the lines a `∇` or `⎕FX` definition was written as: the header first, then one line each, as a character matrix padded with blanks to the longest of them. A name that is not a definition has no text at all, which is the 0-by-0 matrix — the same answer for a variable and for a name nothing has used | the numbered conversion, which rewrites the same bytes another way: 5 and 6 write bytes as hexadecimal in either case (a character counts as its code point, and the shape is the argument's with its last axis twice as long), 13 reads hexadecimal back, 16 and 17 are base 64 as RFC 4648 spells it, and 18 and 19 are UTF-8 both ways |
| `\` `⍀` | — | expand, after an operand: every 1 takes the next item, every 0 leaves a fill. `Dialect.expansion` names Dyalog's reading, where the left argument is any integer vector — a positive count repeats that item, a negative one leaves that many fills, 0 means `¯1`, and the result is `+/1⌈|x` items long |
| `⍋` `⍒` | grade up / down (stable; respects `⎕IO`). A NESTED argument orders by the APL2 rule: the rank, then the shape read from the FIRST axis, then the atoms in row-major order — a character before a number before a nested value, a nested one recursively — and two arrays with no atoms are separated by their types instead. It is a different comparator from J's at every step; `Dialect.nested_grade` names Dyalog's total array ordering as the other reading | collating grade: every character of y is keyed by where it FIRST occurs in the collating array x — the coordinate read with the last axis most significant, and one past the end for a character x does not hold — and the items of y are ordered by those keys read left to right |
| `⊢` `⊣` | same | right / left. `A⊢[M]B` is the selection function: a 1 in M takes the element of B that stands there, a 0 the element of A, and M, A and B agree by the ordinary scalar rule. M is settled before the program runs |
| `○` | pi times y | circle function k (see below) |
| `/` `⌿` | — | replicate, after an operand: `/` counts the LAST axis, `⌿` the leading one |

`⊥` and `⊤` have no monadic meaning in APL at all; J spells those `#.` and
`#:`. `x ⊤ y` takes its right argument whole, so the digits become the
LEADING axis and the result has shape `(⍴x),(⍴y)` — the transpose of what
J's `x #: y` produces, which frames the digits per atom of y; where x has
rank 2 or more its LEADING axis is the digits and its remaining axes frame
the answer along with y's. `⊥` is the matching inner product `+.×`: it
folds x's LAST axis against y's LEADING one, so `(2 2⍴2)⊥2 3⍴⍳6` is a 2-by-3
answer and the two axes have to agree in length.

Vector notation is real: juxtaposed operands are the items of one vector,
and the whole strand is a single operand — except under index brackets,
which bind to the value written immediately before them and not to the
strand: `1 2 3[2]` is `1 2` beside `3[2]`, which is a rank error because a
scalar has no axis to index, while `(1 2 3)[2]` is 2. Every primary contributes one
item, except a run of numeric literals, whose numbers are items of their
own — which is why `1 2 (3 4)` has three items, `'ab' 'cd'` has two, and
`1 2 3` is still one simple integer vector. A strand of simple scalars of
different types is APL2's MIXED SIMPLE array: `1 'a'` has two items and
depth 1. libjay keeps one as boxed scalars — a box holding a simple scalar
is that scalar in APL, so nothing else can be confused with it — and it
reports depth 1, displays without a nested display's spacing, and refuses
to be disclosed any further.

Such an array is BUILT wherever two simple arrays share no one type, and
read element for element wherever one meets a simple array: `1 2,'ab'` is a
four-element vector, and `⍪` `∪` `∩` `~` `⍷` `∊` `⍳` `≡` and enlist all
follow. Since the form says nothing the value did not already say, every
APL result passes back the other way too — a boxed form whose elements turn
out to share one type is the plain array again, which is why
`2↓1 2,'ab'` matches `'ab'` and `+/1 2 3∩'a' 2` is 2 rather than a refusal.
Arithmetic over a mixture is still a type error, as it is in the reference.
In a mixed VECTOR a run of characters beside each other is text and prints
with no separator (`1 2,'ab'` shows as `1 2 ab`); at rank 2 and above each
character is a column of its own again. J has no such value and refuses
`1 2 , 'ab'` outright.

The missing valences in the table above are marked "not supported yet".
The glyphs and features with no oracle at all — because GNU APL lacks the
valence outright, or because the feature is Dyalog's own — are listed in
"Which APL" above.

APL's SCALAR functions pervade a nested argument: they descend through the
boxes, item by item, and apply to the simple values at the bottom, so
`1+⊂2 3` is `⊂3 4` and `(1 2)(3 4 5)+10` adds ten to five numbers under two
boxes. The two sides agree by the ordinary scalar rule at every level, so a
scalar spreads over a nested array's items as it does over a simple array's
elements, and a shape that does not agree is a length error at the level it
does not agree on. A leaf that is not a number is refused where it stands:
`1+⊂'ab'` is a type error. Cells that all come back simple scalars make a
simple array again, so pervasion never adds a level the argument did not
have. The descent runs on a work stack of its own rather than the call
stack, because nesting depth is data: a value thousands of boxes deep
answers instead of taking the process down. J has no such rule — a box
there is a type error, and `>` opens one first.

Every APL operator that collects the values of several applications does so
between ITEMS: what it hands the function is the CONTENTS of an item, and a
result that is not a simple scalar is enclosed again to take one place in
the array being built. `¨` says so in its name; `∘.f`, `f/`, `f⌿`, `f\`,
`f⍀` and `f.g` follow the same rule, which is why `,/1 2 3` is an enclosed
vector and `¯1 0 1∘.⌽⊂m` rotates the matrix inside the enclosure. J reads
all of them by cells and leaves its boxes shut; the two languages part here
and nowhere in between.

Operators: `/` (reduce, LAST axis), `⌿` (reduce, leading axis), `\` (scan,
last axis), `⍀` (scan, leading axis), the N-WISE REDUCTION `n f/ y` and
`n f⌿ y` (the DYADIC case of a `/`-derived function: every window of n items
along that axis, folded by `f/` — `2+/1 2 3` is `3 5` and `2,/1 2 3` the two
pairs; n is one number however it is shaped, a negative one reverses each
window before folding it, zero answers `f/⍬` once per gap, a window may
reach one item past the axis and no further, and a bracket axis names the
axis as it does for the reduce), `⍤` (rank), `⍨` (commute), `⍣`
(power; a NEGATIVE count runs the inverse that many times, over the same
obverse table J's `u^:_n` reads, and a verb with no inverse says so), `¨`
(each: the function runs on the contents
of every item and its result goes back into an item — a simple scalar
result stays simple, so `2×¨1 2 3` is flat and `⍴¨'ab' 'cde'` is nested),
`∘.f` (outer product — the table J spells `x u/ y`, but read between the
ELEMENTS of both arguments rather than between the cells the function's
rank asks for: `1 2 3∘.×1 2 3`, `1 2∘.,3 4`), and `f.g` (the inner product,
`+.×` above all — see "The inner product" under J, which carries the shape
rule both languages share and the one place their readings of `g` part). A `.` is the
inner-product operator only where it is neither the start of a number nor
the tail of `∘.`, so `2.5×2` and `2 3∘.×1 2` read as they always did. A
scan's k-th element is the reduce of the
first k, so it folds right to left like the reduce and not like a left
fold: `-\1 2 3` is `1 ¯1 2`. `⍣` also takes a FUNCTION right operand:
`f⍣g` applies f until `new g old` holds, so `f⍣≡` is the fixed point.

A bare `∘` is Dyalog's beside `f∘g` — monadically `f g y`, dyadically
`x f (g y)`, so g prepares the RIGHT argument and the left one arrives
untouched. It also BINDS an array where an operand belongs: `A∘f y` is
`A f y` and `f∘A y` is `y f A`, so `2∘× 5` is 10 and `(÷∘2) 7` is 3.5.
Both are monadic only, as J's `m&v` and `u&n` are, and the array has to
be a literal — a computed operand (`(⍳3)∘+`) is a named gap. `⍥` (over) prepares both: `x f⍥g y` is `(g x) f (g y)`, which is
the composition J spells `&:`. `⍛` (before) is the mirror of beside: `f⍛g`
prepares the LEFT argument, so `x f⍛g y` is `(f x) g y` and `f⍛g y` is
`(f y) g y`. `f⍤g` with a FUNCTION on the right is Dyalog's atop —
monadically `f g y`, dyadically `f (x g y)` — while a value on the right is
still the rank specification. `f⌸` is the key: the distinct major cells of
the left argument, in first-occurrence order, each handed to f with what
shares it — the positions it occupies monadically, the right argument's
items there dyadically; an operand with no `⍺` gets the group alone. `f⍢g`
is under, which is over UNDONE: the published definition is
`g⍣¯1 ⊢ (g x) f (g y)`, so it prepares both arguments with g, applies f,
and puts the answer back through g's obverse — the same table J's `&.:`
reads, and a g outside it says so by name. `f⌺w` is the stencil: f applies
to the window of `w` cells CENTRED on each cell of the argument, one size
per leading axis, the edges filled with 0 or a blank, so `(+/⌺3)1 2 3 4 5`
is `3 6 9 12 9`; Dyalog's two-row form, which also gives the movement, is
a named gap. None of these six operators is in GNU APL — see "Which APL"
above.

`f⍠B` is the variant: one setting of the dialect overridden for ONE
application and no other. A bare number is the principal option, which for
every function libjay gives a variant is the comparison tolerance, so
`1 (=⍠0) 1+1E¯14` is 0 where `1 = 1+1E¯14` is 1. The named forms are
parenthesised literal pairs — `⍳⍠('IO' 0)`, `=⍠('CT' 0)` — and several may
be given at once, applied left to right. `CT` is the same mechanism J
spells `!.`; `IO` derives the verb again with the other index origin, which
is what makes the variant an override of the dialect rather than an
argument to the verb, and a verb that has no origin to change says so. An
option libjay does not have, or one that is not settled when the program is
compiled, is named. GNU APL rejects the glyph outright, so `⍠` has no
oracle: tests/wave9.rs holds its rules instead.

A dfn that names `⍺⍺` or `⍵⍵` is an OPERATOR rather than a function: it
takes the operand on its left, and one on its right where it named `⍵⍵`.
`+{⍺⍺/⍵}1 2 3` is 6, and naming the operator keeps it one, so
`TWICE←{⍺⍺ ⍺⍺ ⍵} ⋄ -TWICE 5` is 5. An operand may be an ARRAY, and the
body then reads the name as that array: `2{⍺⍺+⍵}3` is 5. Which of the two
it is decides how the body PARSES, not merely what it computes — `⍺⍺+⍵` is
a train under one reading and a sum under the other — so the body is
parsed under every reading when the dfn is defined and the operands choose
one when they arrive. The operands are bound under those two names for as
long as the body runs. GNU APL has no dfn operators either.

A dfn is AMBIVALENT whatever its body mentions: a left argument it has no
name for is dropped rather than refused, so `3 {⍵×2} 5` is 10, where a `∇`
definition binds its arguments by the names its header gives. A guard's
condition is read strictly — exactly one element, and that element 0 or 1
— so `{2:1 ⋄ 0} 5` and `{1 1:1 ⋄ 0} 5` are domain errors where a control
structure's `:If` takes the first element of whatever it is given. A dfn
written INSIDE another reads the names the enclosing one made local
(`{a←10 ⋄ {a+⍵} ⍵} 5` is 15) while its own assignments stay its own; only
a lexical parent's locals are reachable, so a dfn named elsewhere and
called from inside one sees the globals and nothing of its caller. A
function named inside a dfn is a function to the sentences after it, and
the name does not escape the dfn.

A dfn whose answer came from an ASSIGNMENT answers SHYLY: `{a←⍵×2} 5` has
the value 10 and displays nothing, while `1+{a←⍵×2} 5` is 11 and displays
it. Shyness belongs to the application, not to the value: every
application starts out not shy, only a definition's own last sentence
makes it shy, and so an operator that ends by applying that definition
passes it on (`{a←⍵×2}¨1 2 3` and `F⍣2⊢5` are shy) while a primitive over
the same value does not (`+/F¨1 2 3`, `⌽F 5 6`). Assigning it, naming it,
or handing it to any verb is consuming it. `Program::run_detail` reports
the flag beside the value; `Program::run` returns the value alone, since a
caller that asked for it is consuming it. J has no shy results: an
explicit definition ending in an assignment answers with the assigned
value, displayed like any other. All of that is the recorded Dyalog
answer, which is the only reference these extensions have.

The `⎕`-names are read-only and pure: `⎕A` and `⎕D` are the ISO constants
(GNU APL has no value for either), `⎕IO` and `⎕CT` report the dialect the
compiler was given, and `⎕UCS` converts between characters and codepoints.

`⎕FX` fixes a definition from its text and answers with its name:
`⎕FX 'Z←F R' 'Z←R×2'` defines `F` and gives back `'F'`, one line per item of
a vector of character vectors, the first of them the header. It takes the
same lines a `∇ … ∇` would, control words included. libjay compiles before
it runs, so the lines have to be literal text the compiler can read: a
definition assembled while the program runs, or a `⎕FX` inside another
definition's body, is named as not implemented yet rather than answered.
The reason is the compile-then-run split rather than the fixing itself:
libjay decides while it compiles whether `F 3` is an application or a
two-item strand, and it decides that from the source. "A computed operand"
below sets out what the 2026-08-29 wave measured — which half of the
machinery already exists, and why the reachable middle would answer wrong
rather than refuse.
Where Dyalog answers a definition it cannot fix with the number of the
offending line, libjay reports the fault, pointing at the line that carries
it.

Assigning any of the `⎕`-names is refused — the dialect fixed them before the
program ran. The ones that would read a clock, a workspace or a filesystem
(`⎕TS`, `⎕AI`, `⎕FIO` and their relatives) are refused with "closed by the
sandbox", which is the sandbox speaking rather than a queue position — see
"Sandbox" below. `⎕` and `⍞` on their own are input rather than names: see
the same section.

An axis specification `f[K]` is real for every function that reads one, and
the axes are counted from `⎕IO`. Two kinds of function take one.

Where a PAIR of glyphs differs only in which axis it picks, naming an axis
collapses the pair to one function: `f/[k]` and `f⌿[k]` both reduce axis k,
`f\[k]` and `f⍀[k]` both scan it, `⌽[k]` and `⊖[k]` both reverse or rotate
it, and the DYADIC `x/[k]y` and `x\[k]y` replicate and expand along it.
Each of these folds or picks ONE whole axis, so a list of them, or a
fractional one, is a domain error naming the glyph.

The rest read the axis themselves, and several of them take a LIST:

- `,[K]` and `⍪[K]` run a RUN of neighbouring axes together (`,[1 2]` of a
  2×3×4 is 6×4); the axes must follow one another. A fractional `,[K.5]`
  adds a new axis of length one at the gap. Dyadically, `x,[k]y` joins the
  named axis and `x,[k.5]y` LAMINATES — the two arguments beside each other
  along a new axis, a scalar spreading over the other's shape.
- `x↑[K]y` and `x↓[K]y` take one count from x per axis K names, in the
  order written; every axis K leaves out is taken whole and dropped from
  not at all.
- `x⌷[K]y` indexes only the axes K names, the rest coming through whole.
- `⊂[K]y` makes the named axes the shape of each item, in the order
  written, and the axes K left out the shape of the answer; `⊂[⍳0]y` names
  none, and `⊂[1 2 3]` of a rank-3 argument is one enclosure. Dyadically,
  `x⊂[k]y` and `x⊆[k]y` partition the named axis in place.
- Monadic `↑[K]` is first here, taking one item along each named axis and
  keeping the axis; `⊃[K]` is mix, placing the item axes at the positions
  K names. Under `Dialect.first_disclose` the two swap, and `↑[K.5]` then
  places every item axis at one gap.
- A SCALAR function takes an axis dyadically: the argument of lower rank is
  shaped like the axes K names and is stretched along the rest, so
  `1 2+[1]2 3⍴⍳6` adds 1 to the first row and 2 to the second. Its rank
  must equal the number of axes; a scalar spreads as it would with no axis.
  Monadically there is nothing to line up, and it says so.

`Dialect.axis_order` settles which axis of K goes with which item of what
accompanies it, where the two lines disagree: `↑` `↓` `,` and `⊂` read K in
the order written in both, while `⌷` and the scalar functions read it as a
SET here (`2 1⌷[2 1]M` is `1 2⌷[1 2]M`) and in the order written under
Dyalog's preset.

Every other glyph reports `axis specification for X` as a gap — `⍉` `∊` `≡`
`⍴` `⍒`, a dfn operand's own brackets, and an operator's derived function
(`⌽⍤0[1]`) among them.

Bracket indexing `A[i;j]` is real: one slot per axis, an elided slot meaning
the whole axis, indices counted from `⎕IO`, and the result's shape the
slots' shapes spliced in — so a scalar slot drops its axis and a matrix slot
adds rank. The slots are applied from the last axis to the first, which is
what keeps the earlier axis numbers valid as scalar slots drop theirs.
Indexed assignment (`A[2]←99`) works — see "Indexed assignment"
below; the brackets above are read-only in every OTHER respect (an axis
specification, a bare index expression) — only `A[i]←v`/`A[i;j]←v` write.

`/` and `⌿` are operators after a function and replicate after an operand;
names are always values in this subset, so which one is meant is decided by
the token to the left and nothing else. Parentheses around a bare operator
glyph are transparent, so `1 0 1(/)1 2 3` is the replication `1 0 1/1 2 3`
— the token OUTSIDE them is what decides the reading, which is what the
reference does with it too. The LEFT ARGUMENT never enters into that
choice: `1 1/2 3` is a replicate because nothing stands to the `/`'s left,
and `1 1+/2 3` is an n-wise reduction whose n is two numbers, which is a
length error rather than a compress.

`←` assignment (incl. inline), `⎕←` output, `⋄` and newline sentence
separators, `⍝` comments, `¯` negatives, `''` strings. Index origin is a
dialect setting of the compiler instance (`⎕IO` as a variable is
deliberately not runtime state).

`^` reads as `∧`, which is what a program typed on a keyboard without an
APL layout holds and what both references accept. It is one alias in the
tokeniser and nothing else: the primitive reports itself under the glyph,
so a diagnostic about `4^6` names `∧`. The other ASCII substitutes the
question raises are already the language's own spellings — `~` IS the
glyph for not and without in both references — so there is nothing else to
alias.

Function assignment (`F←+/`, `F←+/÷≢`) and trains are extensions under the
`trains` dialect setting, which ships on — see "Which APL" above.

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

## Numeric literals

J's base and constant forms: `16b1f` is 31, `2b101` is 5, a fractional or
negative base works (`2.5b10`, `_16b11`), a `_` in front of the digits
negates the value (`16b_1`), a `.` among them starts the negative powers
(`2b11.1` is 3.5), and digits run `0`–`9` then `a`–`z`. `1p1` is π and
`1p2` is π², `1x1` is e and `2x1` is 2e — `apb` is a×π^b and `axb` is
a×e^b, with either part allowed a sign, a fraction or an exponent.

The `b` binds looser than the whole of the rest of that grammar, on both
sides. Its left part is a number in its own right, so `3r4b11` counts in
three quarters (1.75), `3j4b11` in a complex base and `2e1b11` in twenty;
and every letter to its right is a DIGIT, so `36bj` is 19, `36bxyz` is
44027 and `2b11p1` is 63 rather than a multiple of π.

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
raised to the power `1j1` — except where a `b` splits the word first, since
its own binding is looser still.

## Character literals

J's `literal` type holds one BYTE per item, and a quoted literal holds the
UTF-8 bytes of the text it was written with. So `# 'é'` is 2, `# '日本'` is
6, `a. i. 'é'` is `195 169`, `2 3 $ 'héllo!'` cuts the character in half,
and a reshape, a take or an index can land between the bytes of one
character. The display writes those bytes out again — which is why the text
still looks like what was typed, and why a byte taken out of the middle of
one shows as a character that could not be read. `corpus/j/literals.txt`
holds the family, recorded against jconsole.

One item per character is available as the opt-in `j_unicode_strings`
extension; see [extensions.md](extensions.md), which is where every
non-standard behaviour is described. APL is Unicode-native and needs
nothing: `⍴'héllo'` is 5 in GNU APL and here.

libjay has one character type where J has three — one, two and four bytes
per item — and the wider two differ from the narrow one only in how they
are WRITTEN OUT; the items and their codes are the same. The four sentences
that costs are in "Known divergences" below.

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
| `__array_interface__` | numpy | any rank, contiguous in either order (C or Fortran) |

### Which layout crosses

A block that is contiguous is read where it lies, in whichever of the two
orders it lies in. The SHAPE is always the logical one — rows leading, so a
table of N columns and M rows is `[M, N]` whatever its buffer does — and the
layout is carried alongside it:

- a C-contiguous numpy block, and every result libjay computes for itself,
  is row-major;
- a Fortran-contiguous numpy block (`np.asfortranarray`, and the `.T` of a
  C-contiguous one) is column-major, and crosses borrowed rather than
  refused;
- a table of two or more columns is column-major by construction: the
  columns cross borrowed, one buffer each, and are never woven at the
  boundary.

A verb that reads the columns where they lie — the leading-axis fold `+/`,
the row fold `+/"1`, every elementwise verb, `$`, `#`, `|:` — answers from
the buffer as it is. Any other verb is given the rows, materialised once at
the point it is applied. Either way the answer is the answer the same data
in the other layout gives, which `tests/layout.rs` holds the whole
primitive table to.

`|:` (APL `⍉`) reverses the axes by flipping that flag, so a transpose
moves no elements at all.

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
- numpy C-contiguous `int64` and `float64` of any rank, and the same types
  Fortran-contiguous, which cross as column-major.
- A table of two or more Arrow columns that agree on type: the columns are
  borrowed where they are, and the value libjay works on is those columns
  end to end. Nothing is woven unless a verb asks for the rows.
- Every rank-1 `integer`/`float` result on the way out.

Copied (widened or unpacked, once): Arrow `Int8/16/32`, `UInt8/16/32`,
`Date32`, `Time32`, `Float32`, `Boolean` (bit-packed at the source);
`UInt64` when every value fits i64; the same numpy dtypes; boolean results
on the way out. A column-major value is copied once more if — and only if —
a verb that reads rows is applied to it, or it is handed back to Python,
printed, or exported.

Refused, with the column named and an action suggested:

- any column holding nulls — J has no missing value;
- columns of a table that do not agree on one element type (int64 beside
  float64), because promoting silently damages values above 2⁵³;
- a numpy view that is contiguous in neither order: a strided or reversed
  slice (`a[::2]`, `a[::-1]`), a sub-block (`a[:, :2]`), or a permutation of
  axes that is neither the original nor its full reversal
  (`a.transpose(1, 0, 2)`);
- `UInt64` values above `2⁶³-1`.

Not supported yet as an input carrier (a promise, not a refusal):
decimals, strings, binary, lists, dictionaries, float16, byte-swapped data.
An Arrow list column, and any struct that is not the `re`/`im` pair above,
is still refused rather than boxed: the mapping from Arrow nesting to box
nesting is its own decision and has not been taken.

Not zero-copy yet, not refused: a result of rank ≥ 2, or a boxed result,
crosses through `.tolist()` today rather than through Arrow or numpy — see
"The data boundary" above. That is a missing fast path, not a missing
capability.

## The numeric tower and the exact types

Six element types, ordered:

    boolean < integer < extended < rational < float < complex

A dyadic pair computes in the higher of its two types, which is what keeps
`1x + 1r2` exact and lets `1x + 1.5` round.

BOOLEAN is where a J literal starts when every one of its atoms is 0 or 1:
`3!:0 (1 0 1)` is 1, as it is in jconsole, and the type travels with the
value through the structural verbs. The arithmetic widens it exactly where
J's does — `+` and `-` answer integers, while `*` `<.` `>.` `^` `|` `!`
stay boolean because they cannot leave `{0, 1}` — and the identity element
an empty reduction reaches is boolean too, whatever it was folding. An
empty SCAN takes its type from the argument wherever that identity is all
it would otherwise have to go on, so `+/\ 0$'a'` is an empty CHARACTER
list and `+/\ i. 0` an empty integer one. The two EXACT types are J's:
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
`*/ >: i. 25x` is the exact factorial. `$`, `#`, `#.`, `#:`, `p:` and `q:`
carry the argument's exactness into their answer, as J does: a count of an
extended or rational argument is an extended integer, and a count of a
machine one stays machine.

`q:` factors exactly rather than through a machine integer: what admits a
value is that it IS a whole number, not that it fits sixty-four bits, so
`q: 2^70x` and `q: 6.5e19` both answer. Small factors come off by trial
division, and what is left is tested for primality (Miller–Rabin over the
first twelve prime bases) and split by Pollard's rho until every factor is
prime — the same shape of work the reference does, and as slow as the
reference on a number whose factors are genuinely hard.

## Symbols

A symbol is an atom whose value is a NAME. `s:` is the only way to make
one: see the `s:` row above for what each shape of argument means.

Two symbols made from the same text are the same atom, whatever sentence,
program or thread made them — `(s: <'a') = (s: <'a')` is 1 — so the text
lives once in a process-wide table and an array of symbols is an array of
`u32` indices into it. That makes a symbol array flat: it copies, slices,
reshapes and indexes exactly as an integer array does, and equality is a
comparison of indices rather than of strings. The table is append-only, so
an index names the same text for the life of the process; it never shrinks,
which is the same bargain J's own symbol table makes.

The INDEX is opaque. Symbols order by their TEXT, in codepoint order, so
every comparison and every grade resolves the table first: `` `A `` sorts
before `` `a ``, `` `a `` before `` `aa ``, and the order two names were
interned in says nothing about which comes first.

**What symbols do.** `= ~: -: < <: > >: <. >.` (the orderings read the
names; `<.` and `>.` answer the smaller and the larger name), `/: \:`,
`~. ~: i. e. I. { # , $ |. |:` and the rest of the structural verbs, `":`,
boxing, and `3!:0`, which reports 65536. The fill element is the EMPTY
symbol, so `4 {. s: ;: 'a b'` is `` `a `b ` ` ``.

**What they do not.** Arithmetic — a symbol is not a number, and `+` on one
is a type error naming `5 s:` as the way to its characters. A symbol mixes
with nothing else: catenating one to a character, a number or a box is a
type error. Equality across the boundary is still TOTAL, as J's is:
`(s: <'a') = 'a'` is 0, not a complaint, because nothing but a symbol is
one. Fusion and the GPU decline symbols for the same reason they decline
characters — there is no arithmetic to compile.

**At the boundaries.** Python gets the names as `str`: a symbol atom is a
string and `.tolist()` of a symbol array is a list of them. There is no
carrier going the other way — a Python `str` arrives as a character array,
and `s:` inside the expression is how one becomes a symbol. Arrow has no
symbol type, so exporting one is refused by name. The C ABI has no
descriptor for a table index either, and says so, naming `5 s:`.

APL has no symbol of its own, and `s:` is not one of its spellings.

## Sparse arrays

A sparse array has the shape of the array it stands for and holds only the
positions that differ from ONE repeated element — the sparse element, zero
for anything `$. y` makes. Some axes are stored sparsely and the rest are
dense, so a stored entry is a whole cell over the dense axes: an index row
naming its position along the sparse axes, and that cell's elements. `$. y`
makes every axis sparse, which is the familiar list of coordinates and
values.

The display is one line per stored entry — its position, `|`, then what it
holds — with the positions and the values each aligned in their own column.
Where the stored cell has axes of its own it is drawn as the ARRAY of all
the cells is, so a cell keeps its rows and the planes keep the blank line
between them; the position stands beside the cell's first line and the
column is drawn down the rest. An array with nothing stored shows nothing,
whatever its sparse element is.
`":` gives that display as a table of lines whatever the array's own rank.

**The verb.** `$. y` converts a dense array; a scalar has no axis to store
along and comes back dense, as it does in J. `x $. y` takes a numbered
form: `0` moves to the other storage kind in whichever direction the
argument is not already in, `1` builds a new sparse array from a shape (or
from a boxed `shape ; axes`, or `shape ; axes ; element`), `2` the sparse
axes, `3` the sparse element, `4` the stored positions as a table, `5` the
stored cells, `7` how many entries are stored, `8` the array with the
entries that hold the sparse element dropped, and `_1` the shape, axes and
element boxed together. `2 $.` answers a DENSE argument with all of its
axes; every other query refuses one, as J does. `3!:0` reports the sparse
type codes: the dense code times 1024.

A BOXED left argument respecifies the storage rather than asking about it.
`(2;a) $. y` stores the same value under the sparse axes a — a dense
argument too, with zero the sparse element, and a negative axis counting
from the end — so the value does not change, only which axes an entry is
indexed by and therefore how many entries there are. `(3;e) $. y` gives a
sparse array another sparse element, keeping every stored entry exactly as
it is, so every position none of them names now holds e and the array's own
VALUE changes: `0 $. (3;5) $. $. 2 3 $ 0 1 0 2 0 3` is `5 1 5 / 2 5 3`.
The element takes the wider of the two types, as an amendment does.
`(2 2;a) $. y` says how many cells the array would store under the sparse
axes a, without building it. `(2 1;a) $. y` asks how many BYTES they would
take: that is a measurement of the interpreter that answers it and not of
the language, so libjay refuses it by name — its own layout is not J's and
a number from it would mean nothing.

**Which element types.** Boolean, integer, floating and complex. J has a
code for a sparse literal and a sparse box and makes neither — libjay names
both as gaps — and the exact types (extended, rational) have no sparse form
at all, which is a domain error in both.

**Where sparseness stops.** A sparse array reaching any verb but `$.`,
`$`, `#`, `":`, `echo` and `3!:0` is expanded first. The ANSWER is the
dense array's and is therefore always right; what does not survive is the
storage kind. J propagates it — `s + 1` there is sparse with a sparse
element of 1 — where libjay gives back the dense array. `-:` compares
values, so a sparse array and the dense one it stands for match. Indexed
assignment writes into the expansion, and a fused chain reads it.

The same holds where results are COLLECTED rather than passed on: `u^:n y`
over a list of counts, the scans, the cuts and every ranked application
frame their cells into one array, and a cell is made dense on the way in.
A sparse cell holds only its stored entries while its shape is the logical
one, so framing it as it lies would size the answer from a shape its buffer
cannot fill; the corpus rows for `$.^:(1 1) 1 0 1` and its relatives ask
the shape and the sum, which are the questions storage does not decide.

**At the boundaries.** Neither Python, Arrow nor the C ABI has a sparse
carrier, so a sparse result crosses as the array it stands for.

**The ceiling.** `1 $. shape` refuses a shape past the element ceiling
(`limits::MAX_ELEMENTS`) even though building it allocates nothing, because
every other verb would expand it. J, which propagates sparseness, holds
much larger ones.

APL has no sparse form of its own, and `$.` is not one of its spellings.

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
`I.` in J, the same family plus `≡` `∊` `⍳` `∪` `⍸` in APL, the residue
`|` and the encode `#:`/`⊤` whose digits are residues, and the two roundings
`<.`/`⌊` and `>.`/`⌈`. Dyalog's `⊤` is the exception: it takes its digits
exactly, so `2 2⊤4-1E¯14` is `1 2` there and `0 0` here —
`Dialect.encode_digits` names it. Grade reads the tolerance in the APL2
line and NOT in J or in Dyalog: `⍋1.0000000000001 1` is `1 2` — the keys
tie and the stable sort leaves them where they were — while
`/: 1 1.0000000000001 1` is `0 2 1`, both as the references answer, and
`⍋2 (1+1E¯14) 1` is `3 2 1` under the total array ordering, which reads no
tolerance at all.
A tolerant comparison is not transitive, so a non-transitive triple leaves
the order to the sort; the ties that matter in practice are the ordinary
two-key ones.

Each of those reads the tolerance its reference's way, and the two ways
differ:

- `<.`/`>.` scale the gap to the integer by the magnitude in J, so
  `<. 99.999999999995` is 100. `⌊`/`⌈` shift by `⎕CT` outright in APL, so
  `⌊99.999999999995` is 99 and `⌊¯1E¯13` is 0. Dyalog's is a third
  reading, `⌊y+⎕CT×1⌈|y` — scaled by the magnitude but never below the
  tolerance itself, so `⌊9.9999999999999` is 10 and `⌊¯1E¯13` is `¯1`;
  `Dialect.floor_rule` names it.
- `x | y` rounds the quotient in both. J then answers an exact zero wherever
  the product is tolerantly the DIVIDEND, so `2 | 1e_14` keeps its `1e_14`.
  APL reads the remainder against the MODULUS, so `2|1E¯14` is 0 and a large
  enough modulus swallows the remainder outright: `1E13|3` is 3, `1E14|3` is
  0. There is a band about four ulps wide, at a gap of `⎕CT` exactly, where
  GNU APL's threshold sits a hair below its own `⎕CT` and libjay's does not.
- `∨`/`∧` are GNU APL's alone: a zero argument hands its whole partner back
  with its sign, and a near-whole or vanishing argument is rounded first.
  `Dialect.gcd_rule` names Dyalog's and J's reading, which does neither.
- The GCD of two non-integral reals is Euclid on the values, and it STOPS
  once the remainder is no larger than the tolerance times the larger of
  the two arguments — the scale the division sequence started at. Both
  references answer that way and it is what makes `0.3 +. 0.1+0.2` be
  `0.3`: grinding on gives 4e¯17 and an LCM of 2.25e15. Where the two
  magnitudes are more than about `⎕CT` apart the references part company —
  `1E6∨1E6+1E¯7` is 1.16e¯10 in GNU APL and `1e6 +. 1e6+1e_7` is
  1.00001e¯7 in jconsole — and libjay follows J's reading in both
  languages. A pair that both print as short decimals is read as those
  decimals instead, which is what makes `123.456 +. 78.9` be `0.012`; a
  value needing more than twelve significant digits to print back is a
  rounding residue rather than a written decimal and is left to Euclid.

A count is a different matter, and libjay follows both references there
without consulting the tolerance at all. A float near a whole number reads
as that whole number wherever a count, a length or an index is wanted —
`⍳2-1E¯14` is `1 2`, `(2-1e_14) {. 1 2 3` is `1 2` — and the admission is
not the comparison tolerance: neither `⎕CT←0` nor `9!:19 (0)` turns it off,
and with the tolerance off `(2+1e_13) = 2` is 0 while `i. 2+1e_13` still
counts to 2. The widths differ:

- J's is RELATIVE, `2^_44` of the whole number's own magnitude, so the room
  grows with the count and closes beside zero: `i. 2+1.1e_13` counts,
  `i. 2+1.2e_13` and `i. 1e_14` do not, and at a million `5e_8` is still
  inside.
- APL's is ABSOLUTE, `1E¯10` at every magnitude: `⍳1E¯11` is the empty
  vector because 1e¯11 reads as 0, and `⍳1000000+1E¯9` is a domain error
  where J answers.

The two cross at about 1760, so neither is a superset of the other. The
whole family that reads a count goes through it — `⍳ ⍴ ↑ ↓ ⌽ ⊖ / \ ⌷ ⊃
[;] ? ⎕UCS` and `i. $ {. }. |. |.!. # { A. I. C. |: q: p: u: ^: @. ?` — and
a float a real distance from any whole number is refused as before. An
operand SELECTOR is not part of it: jconsole refuses `3 u:`, `s:`, `m b.`
and a cut mode on a near-integer, and libjay reads those exactly too.
Dyalog's admission is a third reading again — relative, and moving with
`⎕CT`, so `⍴⍳1000000+1E¯9` answers there and `⍳2+9E¯11` is a domain error.
`Dialect.near_count` names it, and the Dyalog preset carries it.

J's `!.` sets it per verb: `=!.0` compares bit for bit, and any tolerance
above 2⁻³⁴ is refused, as J refuses it. APL's `⍠('CT' n)` does the same for
one application, and both now reach `|`, `#:`/`⊤`, `⍋`/`⍒` and `∨`/`∧`.
`⎕CT` as a runtime variable is not implemented — APL's tolerance is the
dialect's, as `⎕IO` is.

A fused kernel carries the tolerance the program was compiled with, so a
comparison inside a blockwise chain answers exactly as the same comparison
outside one does.

## Explicit definitions

A definition is a verb (a function) like any other: it takes a rank, joins a
train, is an operand to a modifier, and can be named.

J writes it four ways. `3 : '…'` is a monad whose argument arrives as `y`;
`4 : '…'` is a dyad with `x` and `y` as well. Either with `0` in place of
the string takes its body from the lines below, ending at a line that is a
lone `)`. `{{ … }}` is the direct form, on one line or over several, and its
own words decide its valence: a body that mentions `x` is a dyad.

### `13 : '…'`, the tacit definition

`13 : '…'` reads an explicit body and answers the TACIT verb that computes
the same thing. The body is parsed as an ordinary sentence with `x` and `y`
standing as names, and the tree that comes back is abstracted over them: a
part that reads neither argument is folded to its value, `y` becomes `]` and
`x` becomes `[`, and each application becomes the train, noun fork or
composition that says the same thing without naming an argument. A body that
mentions `x` derives a dyad, and there both valences are kept apart — `f y`
becomes `f@:]` rather than `f` — while a body that mentions only `y` derives
a monad, where the argument arrives whole.

Three shortenings follow the reference. A constant that is an integer atom
between `_9` and `9` is written `n:` and everything else `m"_`. `x f y` is
written `f` and `y f x` its commutation, with the commutation dropped for
the dyads that say the same thing either way round (`+ * <. >. = ~: +. *.
+: *: -:`) and replaced by the other spelling where the mirror has one
(`<`/`>`, `<:`/`>:`). And a fork whose LEFT tine would need brackets and
whose right one would not is written the other way round.

A body the abstraction cannot reach becomes the ordinary explicit definition
— `3 : '…'` or `4 : '…'` — which is what the reference falls back to as
well; libjay falls back in more places than it does, and the verb computes
the same thing either way. A control word in a tacit body is refused, as the
reference refuses it.

The one difference in what is DISPLAYED is the cap fork. `[: f g` and
`f@:g` are one function in J and two spellings, and libjay's tree keeps only
the one node, so a translation that composes monadically is displayed
`f@:g` where the reference writes `[: f g`. The corpus records the
translations that have no cap, and the application of the ones that do.

### The linear representation

A sentence that reduces to a verb or a modifier displays it, and what a
session shows is the LINEAR REPRESENTATION: the text the entity would be
written as. `mean =. +/ % #` and then `mean` answers `+/ % #`; `m =. /` and
then `m` answers `/`. Brackets go only where the spelling would otherwise
read as something else — a train as a modifier's left operand, anything a
modifier made as a conjunction's right one, a train in a train's last place
whose words would count out differently — and `{` and `}` carry a space of
their own so that two never read as `{{` or `}}`.

Three spellings are libjay's own where the reference has two. A cap fork is
written `f@:g`; `u"b a b` is written `u"a b`, the shorter of the two
spellings of one rank; and a noun of rank 2 or more has no spelling here at
all, where the reference writes an expression that BUILDS the value. An
explicit definition gives back its own header and body where it was written
inline (`3 : 'y + 1'`); one whose body is on the lines below, or a `{{ }}`,
keeps no text and is a named gap.

### Explicit adverbs and conjunctions

`1 : '…'` is an adverb and `2 : '…'` a conjunction; `1 : 0` and `2 : 0` take
the body from the lines below in the same way, and a `{{ … }}` whose body
mentions an operand name is one too. The operands arrive as `u` and `v` when
they are verbs and as `m` and `n` when they are nouns — the same operand
under two names, and reaching for the one the operand is not gives an
undefined name, as the reference does.

A `{{ … }}` is read for its part of speech rather than told: a body that
mentions `v` or `n` is a conjunction, one that mentions `u` or `m` is an
adverb, and one that mentions neither is a verb. A marker line — `{{)a`,
`{{)c`, `{{)v`, `{{)d`, `{{)m` — states it outright instead, and the
reference takes a marker only where nothing else stands on its line.

`{{)n` makes a noun of the body's text, which is the direct-definition
spelling of `0 : 0`. Its body is text and not source, so the lexer takes the
lines below whole: the `}}` that ends it has to START a line, whatever
follows it there belongs to the sentence again, and every line of the body
keeps its own newline. An empty body is the empty vector, not one newline.

The body runs at one of two moments, and which one it is depends on whether
it mentions an argument. A body that mentions `x` or `y` becomes the body of
the DERIVED VERB, run when that verb is applied to its arguments; its valence
follows the same rule an explicit verb's does, so `1 : 'x u y'` derives a
dyad and nothing else and `1 : 'u u y'` a monad and nothing else. A body
that mentions neither runs at DERIVATION, when the modifier meets its
operands, and what it produces is what the modifier produced: `1 : 'u @ u'`
applied to `+:` is the tacit verb `+:@+:`, and `1 : '3 + 4'` is the noun 7.

libjay derives at parse time, so both phases are settled before the program
runs: the operands are substituted into the body's words and the body is
parsed with them in place, which is what J's substitution rule describes.
The derivation phase settles its own `if.` blocks: the operands are known
there, so the condition is read now and only the arm that holds is parsed.
That is what lets a body which derives its own modifier — J's way of writing
a recursive one — stop at its base case, as in
`2 : 'if. n = 0 do. ] else. u @ (u pw (n-1)) end.'`. The nesting is bounded
at 16, which is a diagnostic for a body with no base case rather than a
parse that runs out of machine stack; the reference raises a stack error for
the same body. A body that names an argument belongs to the DERIVED verb,
which the reference parses only where that verb is applied and libjay parses
whole here, so a recursion written there is still a named gap.

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
`return.`, `break.`, `continue.`, `try. catch. catcht. end.`, and the
branch pair `goto_name.` / `label_name.`.

A `label_name.` is a target only where it stands on the body's own statement
list: the target is settled while the definition is BUILT, so a
`goto_name.` with no label, a label written twice, and a label inside a
control structure are all refused there rather than when the branch runs —
which is where the reference refuses them too. A label yields nothing at
all, not even the empty value an untaken branch gives, so the value the body
had in hand survives it. A branch out of a `for.`, `while.`, `select.`,
`try.` or `catch.` block leaves the block and lands on the body's own line.

`throw.` leaves the definition it stands in at once. A `catcht.` block in a
CALLER's `try.` takes it — never one in the same definition, which the
reference lets the throw straight past, and never a `catch.`, which answers
only for the languages' own errors. A `try.` may carry both rescue blocks,
in either order. A `throw.` nothing catches stops the program with an
uncaught-throw diagnostic.

A test is true when the argument is empty, or when its first atom is not
zero — J's rule, checked against the reference. `select.` matches with `-:`
(match), not membership, so `case. 1 2` takes the list `1 2` and not the
atom `1`.

`try.` answers for the languages' own errors. A gap in libjay itself — "not
supported yet" — goes straight through it: swallowing a promise would turn
it into a wrong answer.

APL's `:If :ElseIf :Else :EndIf`, `:AndIf`, `:OrIf`, `:While :EndWhile`,
`:Repeat :Until`, `:For … :In … :EndFor`, `:Select :Case :CaseList :Else
:EndSelect`, `:Return`, `:Leave` and `:Continue` work inside a
`∇`-definition and outside one alike; `:End` closes any of them. GNU APL
raises a SYNTAX ERROR for every one of these words, so the oracle is
Dyalog's recording in `corpus/apl/dyalog-control.txt`, and
`crates/libjay/tests/definitions.rs` carries the rest.

`:AndIf` and `:OrIf` continue the `:If`, `:ElseIf`, `:While` or `:Until`
line above them, and both short-circuit: the second test does not run where
the first has settled the answer. `:CaseList` takes the arm where the
subject matches ANY ONE of the list's items, where `:Case` compares the
list as a whole. `:For` binds an item's CONTENTS — `:For p :In (1 2)(3 4)`
gives `p` a pair of numbers, not an enclosure of one — and several names
take each item apart between them, one of its own items each. A body may
call a function the program fixes AFTER it: APL settles a name's class when
the line runs, so every name a `∇` or `⎕FX` in the program gives a function
stands for a verb resolved when it is applied.

`Dialect.control_strictness` names how strictly a structure reads what it
is given. The shipped reading is lenient: a condition is true where its
first atom is, and a `:Leave` outside a loop leaves the definition. Dyalog
reads both strictly and says so instead — `:If 1 1` is an error there, and
a `:Leave` needs its loop.

APL's own control flow, the one the reference does have, is the BRANCH. A
line of a `∇`-definition may begin with a label — `L1:` — and `→` takes a
line number: an empty target falls through to the next line, a number that
is a line of this definition continues there, and anything else (`→0` above
all) leaves the definition. A label's value is its line number, so the
conditional branch is written `→(cond)/LABEL`: replicate answers the label
where the condition holds and an empty vector where it does not. A branching
definition is run statement by statement rather than as a block, which is
what lets a `→` inside a loop leave it. A label and a control structure stand in
one definition: a control structure folds several lines into one statement,
so the definition carries the line each of its statements began at, and a
`→` reads that to find where to go. A branch INTO a control structure has no
statement of its own to land on and is a named gap. No oracle covers any of
this — GNU APL has no control structures inside a `∇` definition at all, and
refuses `:If` there outright. libjay stops a definition that has branched 2²² times rather than
letting it hang.

A `∇`-definition whose header names no argument is NILADIC: naming it is
what calls it, so `∇Z←H ⋄ Z←42 ⋄ ∇` makes `2×H` 84.

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

## Locales

J's globals live in namespaces, and every name belongs to one. `base` and
`z` exist from the start; a locale is made by naming it — a locative
mentioned anywhere, `cocurrent`, `coclass`, or `18!:3`. Reading `V_qq_` is
enough: the reference creates `qq` at that moment and then reports that `V`
in it has no value.

`name_locale_` is the locative spelling. It is a GLOBAL wherever it stands,
so `R_cc_ =. y` inside a definition writes locale `cc` rather than the
call's frame, whichever assignment arrow it uses. `name__` names `base`,
and `name_base_` is the same name as the bare `name` there.

`name__var` is the INDIRECT locative: `var` holds one boxed locale name and
the locale is settled while the program runs. It reads and writes; a VERB
spelled that way is a named gap, because what part of speech the name has
would not be known until the locale is.

A definition's body reads and writes ITS OWN locale, not the caller's: the
locale is the one the definition's name put it in (`f_x_ =. …` makes it
x's), or the one the sentence that defined it belonged to. `18!:5` inside
the body answers that locale, and the one in force outside comes back when
the call returns — a `cocurrent` the body runs lasts only as long as the
call.

A name not found in its own locale is looked for in the locales that
locale's search path names, ONE step and no further: a path that names a
locale whose own path names `z` does not reach `z`. Every locale but `z`
starts with `z` on its path, which is where the language's own words live.
`18!:2` reads a path and its dyad writes one.

Numbered locales come from `18!:3 ''` and from nowhere else: naming
`V_5_` where `18!:3` never handed out a `5` is refused rather than creating
one. `18!:55` destroys a numbered locale; a NAMED one survives it, and the
answer is 1 either way — which is what the reference does.

`cocurrent` and `coclass` are the same verb here, as they are in the
reference with a bare profile: each makes the locale it names current,
creating it if it is new, and answers an empty value. Both live in `z`, so
a program is free to give either name a meaning of its own.

The locale a sentence belongs to decides what part of speech its names
have, so libjay follows a `cocurrent` while it READS the program as well as
while it runs it. Only a whole sentence that applies `cocurrent` or
`coclass` to a literal counts: one whose locale name is computed still
switches the locale at run time, but the sentences after it are read in the
locale that was in force, and a name whose part of speech only that switch
would settle is a named gap.

Of the `18!:` family, the reference build libjay is measured against
defines `18!:0` (0 for a named locale that exists, 1 for a numbered one,
_1 for none), `18!:1` (`,0` the named locales alive, `,1` the numbered
ones, sorted), `18!:2`, `18!:3`, `18!:5` and `18!:55`. `18!:4` is not a
verb there at all, and `18!:6` answers a dump of the interpreter's own name
tables; both are refused by name here rather than guessed at.

## The foreigns that compute

`m !: n` is chosen by two numbers that have to be known while the sentence
is read, so neither may be computed. What each family answers:

**`3!:` — types, bytes and the binary form.** `3!:0` is the type code
(boolean 1, character 2, integer 4, float 8, complex 16, box 32, extended
64, rational 128). `3!:1` writes an array as the bytes that stand for it and
`3!:2` reads them back; `3!:3` is the same bytes in hexadecimal, a word to
a row. The form is the reference's, measured from it: each block opens with
the word 227, then the type code, the element count and the rank, then the
shape, then the elements — byte-wide types padded out to a whole word with
room for a terminator, wider ones a word or two apiece. A boxed block
carries one OFFSET word a box, each measured from the start of the block it
sits in, and then the nested blocks in order, so boxes nest to any depth.
The exact types are the one hole left: the reference stores an extended
integer and a rational as digit blocks of its own, and libjay names that
rather than guessing at it.

`x 3!:4 y` writes whole numbers as bytes and reads them back: `1` two bytes
each, `2` four, `3` eight, `4` four unsigned, and the negative of each reads
that width back — `_4` unsigned, everything else two's complement. `x 3!:5
y` does the same for floating point, `1` a single and `2` a double. Writing
takes a list, and reading needs a length that divides by the width.

**`4!:` — the name table.** `4!:0` gives each boxed name its class: 0 a
noun, 1 an adverb, 2 a conjunction, 3 a verb, `_1` a name with no meaning
yet, `_2` text that is no name at all. `4!:1` lists the names of the classes
asked for, sorted and boxed, from the locale in force. `4!:55` erases, and
answers 1 for each name whether or not it stood for anything. A name has ONE
class at a time: giving a name a verb takes away the value it held, which is
what `4!:0` then reports. A modifier is applied while a sentence is parsed,
so what the run keeps of `m =: /` is the class alone — enough for these two
and for nothing else.

**`5!:` — representations.** `5!:1` is the atomic representation, the same
boxed data a gerund is made of: a verb gives its spelling or its box tree,
a value gives the noun pair `('0'; <value)`, and a name that stands for
nothing yet stands for ITSELF, as the reference answers. Text that is no
name is an ill-formed name and is refused. `5!:2` draws the same tree as
the words it is spelled with, one box a part; `5!:5` is the linear
representation, the J source with a bracket only where one is needed; and
`5!:6` is the parenthesised one, with a bracket around every part that is
more than one word and no flattening of a train. An explicit definition
writes back as the source it was given.

`5!:0` is an ADVERB — the inverse of `5!:1` — and libjay settles it while
the sentence is read, because what the representation names decides how the
rest of the sentence parses. It therefore reads a literal representation,
or the `5!:1` that made one (`(5!:1 <'f') 5!:0`), and names a
representation the program computes some other way as a gap.

**`8!:` — formats for the world outside J.** `8!:0` boxes each atom, `8!:1`
boxes each column with its rows in it, and `8!:2` is a plain character
array. All three spell a number as C does, `-1.5` where J writes `_1.5`, and
pad each column to one width. Without a specification a number keeps the
shortest decimal digits that read back as itself, cut to fourteen
significant digits and nine decimal places; outside `1e_9` to `2e9` it is
written in exponential form with ten significant digits in the mantissa —
all measured from the reference. A literal `width.decimals` on the left sets
the field instead, right-aligned, and a width too narrow for the answer is
filled with `*`. Characters, boxes and symbols are a named gap; a complex
number has no format here, and the reference refuses one too.

**`9!:` — the two global parameters libjay honours.** `9!:10` reads the
print precision and `9!:11` sets it, from 1 to 17 digits; `9!:18` reads the
comparison tolerance and `9!:19` sets it, from 0 up to but not including
2⁻³⁴. Both take effect on every sentence AFTER them in the same program, so
`9!:11 ] 3` then `% 3` shows `0.333` and `9!:19 ] 0` makes every comparison
exact. Everything else in the family is the interpreter's own machinery.

**`128!:3` — the crc.** The CRC-32 of a byte string, the reflected
polynomial, answered as a SIGNED 32-bit number. The decompositions the rest
of the family holds are named gaps.

## Indexed assignment

APL's `A[i]←v` and `A[i;j]←v` read the named value, copy it, write the
selected part, and give the copy the name. Another name that was taken from
the array before the write keeps what it had. An elided slot selects its
whole axis; a scalar slot drops its axis from the shape the value has to
match, and a scalar value spreads over the whole selection. A value of a
wider type widens the array rather than being truncated into it. An index
outside its axis is a domain error naming the axis and its length.

It is an EXPRESSION, not a statement: it may stand inside a larger
sentence, and its value is the value assigned rather than the array the
value went into. `B←A[2]←5` gives B the 5; `2+A[1]←9` is 11; `A[1]←C[2]←9`
writes a 9 into each of the two names. The value is shy, so a session shows
nothing for the assignment on its own. The TARGET is a name — writing
through an expression (`(A,4)[1]←9`) is a named gap here and a SYNTAX ERROR
in GNU APL.

## A computed operand

An operator's operand may be a name or an expression rather than a
literal — a `⍣`/`^:` count, an `f[K]` axis, an array where a function
operand belongs — and then the derived function cannot be built while the
program compiles. libjay keeps the operand's EXPRESSION in the derived
function and reads it where the function is applied, in the names that
application can see, and builds the real derived function from what came
back. Nothing is cached: a count read from a definition's argument is read
again at every call, which is the point of reading it late.

The operand is one token's worth, which is what the references do: a
literal, a name, or a parenthesised expression. `f⍣N+1` therefore reads the
count `N` and leaves the `+1` to the sentence.

Everything a literal operand means, a computed one means too, because the
value is read the same way: J's list of counts and its boxed trace
(`u^:(<n)`), a negative count over the obverse table, an axis list, a
fractional laminate axis. What the value cannot decide is anything the
PARSE depends on: whether an operator's operand is an array or a function
chooses how the body of `{⍺⍺+⍵}` is read, so that stays a compile-time
decision and only the array's value waits.

Two brackets stay settled before the program runs, because they pick which
function the glyph stands for rather than name an axis: `⊤[N]`, the digit
count, and `⌹[K]`, which selects one of a group of unrelated functions.
Both name the gap if the program computes them. The third bracket that is
not an axis, `⊢[M]`, names DATA rather than a function — a selection mask —
so it may be computed like any other operand.

An explicit definition reads `f[K]` as a value of its own rather than as an
axis. A `∇` header may declare the name (`∇Z←A F[X] B`) and a `{…}` reads
it as `χ`; the value arrives verbatim, with no `⎕IO` adjustment, and the
body decides what it means. It belongs to the one call that wrote it — a
definition applied inside the body does not inherit it — and a definition
whose header names none refuses one rather than naming a gap.

### `⎕FX` on text the program assembles: the gap that stays

`⎕FX` fixes a definition from its lines. Where every line is literal text
in the program, libjay reads them while it compiles, and a later sentence
calling the function is compiled as an application. Where a line is
computed the definition stays a named gap, and the reason is not `⎕FX`:

- The run-time half already exists. A definition's name is bound at run
  time (`Env::define`), a name standing for a function is applied at run
  time (`Verb::Named` looks the name up in the environment when the verb
  is applied), and a whole program can be compiled and run from a string
  while another one runs (`⍎`). A `⎕FX` that built its definition at run
  time and bound the name would fit all of that.
- The half that does not is the PARSE of the sentence that calls it. `F 3`
  is an application if `F` is a function and a two-item strand if it is a
  value, and libjay settles that while it compiles — from a pre-pass over
  the source that reads every `∇` header and every literal `⎕FX` header.
  A definition whose header is computed is invisible to that pass, so `F 3`
  cannot be given the reading it needs, whatever happens at run time.
- The reachable middle is a trap. Where the HEADER is literal and only the
  BODY is computed, the pre-pass does find the name, so `F 3` would parse.
  But the body would then be compiled at run time against a function table
  built from that text alone: a body calling another of the program's own
  functions would read the call as a strand and answer a wrong value — the
  same shape as the `⎕FX` bug the 2026-08-29 audit found and fixed. Closing
  that means the compiler taking its function table from the running
  environment, which is the name-resolution redesign this gap has named
  from the start.

So the gap stays, whole, and is refused by name in both halves.

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
                       #         kind='discrete GPU', f64=False),
                       #  Device(name='Intel(R) HD Graphics 630', backend='metal',
                       #         kind='integrated GPU', f64=False)]

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
(a comparison at the root, a tally); it holds a moving window or a running
fold, which read items the shader's own element does not; it holds `^` and
the device computes in f64 (the exponential is a 32-bit builtin in both
SPIR-V and MSL); the data
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

## Sandbox

libjay runs an expression, not a program on a machine. Standard input,
standard output and standard error are open; nothing else is. That is one
policy over two surfaces — the Rust library, the Python module and the C
ABI all get the same one — and it is libjay's own, not a property of J or
of APL. So a refusal it makes carries its own error class, `Sandbox`,
labelled **closed by the sandbox**, and it reads as neither of the other
two: not "not in the language" (the feature is in the language) and not
"not supported yet" (no release will open it).

**Open.** Output: J's `echo`, APL's `⎕←` and `⍞←`, and J's `x 1!:2 ]2`,
which writes its left argument as it displays plus a newline and yields it.
Input: APL's `⍞` (one line, as a character vector, with no terminator),
APL's `⎕` (one line, evaluated as APL in the program's own dialect and over
its own names, through the same machinery `⍎` uses), and J's `1!:1 ]1` (one
line, as a character vector). The host decides what those are attached to:
the process's stdin and stdout by default, a callable or a callback where
the embedding says so, and nothing at all for `Program::run` and `jay_run`,
where an expression that reads reports that it has no input source rather
than reading something. An empty line is a line; the end of the input is a
diagnostic, never an empty result.

**Closed.** Files and directories (J's `1!:` family beyond the two stream
forms — a boxed file name and any stream number but 1 and 2 alike), scripts
(`0!:`), the host and its environment (`2!:`), the clock and sleeping
(`6!:`), shared libraries (`15!:`), J's own threads (`T.`), and the
`⎕`-names that read a clock, a workspace or a filesystem (`⎕TS`, `⎕AI`,
`⎕FIO` and their relatives). Each names what it would have reached: "1!:21
reaches the filesystem, which is outside the program".

**Machinery.** Three families are the reference interpreter's own workings
rather than a meaning a second implementation could answer: `7!:` measures
its allocator, `13!:` drives its debugger, and most of `9!:` sets things
that belong to the interpreter and not to J — its error message table, its
box-drawing characters, its build string, its jitter settings. Those are
refused permanently and by name, in the same words a `⎕`-name libjay will
never have is refused: not a promise, and not a queue position.

**Neither.** What is left is arithmetic on the argument alone, and it is
implemented — see "The foreigns that compute" below. A member of one of
those families that is still missing is an ordinary queue position and
says "not supported yet" with its number.

Executing a string (`". y`, `⍎ y`) is inside the sandbox rather than a hole
in it: the nested program reaches exactly what the caller reaches, and `⎕`
is that same machinery over a line that was read rather than a string that
was written.

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

## Known divergences (deliberate, revisit later)

Every entry below is a place libjay's answer differs from a reference
interpreter's on purpose — a documented choice, not an accident. The
language and reference each entry compares against is named inline;
"Differences from GNU APL" below is the subset checked against that
oracle directly, one entry per line of
`crates/libjay/tests/corpus/apl/divergences.txt`.

- The linear representation writes one spelling where J keeps two. `[: f g`
  and `f@:g` are one function, and libjay's tree holds one node for both, so
  a verb built either way is displayed `f@:g` — which matters most for
  `13 : '…'`, whose translations compose monadically. `u"b a b` is displayed
  `u"a b` for the same reason: two spellings of one rank, one node.
- J's `u:` widens a literal to a wider character type, which libjay does
  not have: the widened value has the same items and the same codes here
  (`# u: 'é'` is 2 and `3 u: u: 'é'` is `195 169` in both), and only the
  DISPLAY differs, because libjay writes a character below 256 as that byte.
  So `u: 'é'` shows `é` here and `Ã©` there, `u: 233` shows a byte no text
  can hold, and `3!:0 u: 'é'` reports the one character type. Every code at
  256 or above — the ones that need the wide type to exist — displays alike.
  The byte conversions read the same way: `8 u:` leaves alone a character
  list every one of whose codepoints is a byte and packs any other into its
  UTF-8 bytes, `9 u:` reads such a list back, and `2 u:` — which WIDENS
  bytes into characters in J — is the identity here, so `2 u: 8 u: u: 955`
  writes as `λ` here and as `Î»` there.
  One entry per line of `crates/libjay/tests/corpus/j/divergences.txt`.
- jconsole computes a polynomial's roots exactly where it can factor it
  over the coefficients' own exact type — degree 2 or more, whole roots for
  whole coefficients and rational ones for rational coefficients — and
  stores the answer as rationals; its polynomial INTEGRAL divides exactly
  for an extended or rational argument. libjay's Durand–Kerner and its
  integral work in f64 throughout, so `p. 1r2 _3r2 1` is `1 1r2` there and
  `1 0.5` here, and `0 p.. 1 1 1x` is `0 1 1r2 1r3` there and
  `0 1 0.5 0.333333` here. The numbers are the same; their storage is not.
  The obverse of a base conversion reads the same way: `*: &.#. (1r2 1r3)`
  is `16r9` there and `1.77778` here.
- The dyadic outfix types its whole argument before any piece is cut, which
  is what makes `2 +/\. 'abc'` a domain error although every piece it
  leaves behind holds one character. Over BOXES the reference does not
  follow that rule of its own: `5 +/\. (1;2)` is a domain error there while
  `5 <./\. (1;2)` and `5 */\. (2;5)` — the same shape, the same argument,
  no piece left over — are the empty, and `0 >./\. (<'abc')` answers a pair
  of boxes no maximum was ever taken of. Two of those cannot both be a
  rule. libjay asks the operand once, of the whole argument, whichever fold
  it is, and refuses where that has no meaning.
- Two whole numbers a double cannot tell apart. Once the verb is not one of
  jconsole's integer special cases it holds every number in a double, so
  `9007199254740992` and `9007199254740993` are one value to its `*.` and
  `9007199254740992 *. 9007199254740993` is `9.0072e15` there; libjay keeps
  them as the integers they were written as and answers their real least
  common multiple. The comparison tolerance is not what parts the two — the
  numbers are equal in the reference's arithmetic before any tolerance is
  consulted. The same boundary runs the other way for `2 #:`, where the
  reference is the exact one and libjay is not; that one is a gap and is
  listed above rather than here.
- APL dyadic `⌽` and `⊖` reduce a large rotate amount whole; GNU APL
  truncates it to a signed 32-bit integer first, so `9223372036854775806⌽1 2 3`
  is `1 2 3` here and `2 3 1` there.
- J's comparisons answer a NaN pair from the SCALAR rule at every length,
  and jconsole does not: its vectorised pass compares by "no difference
  large enough", which a NaN never has, so `(2 $ _.) = (2 $ _.)` is `1 1`
  there and `0 0` here — while its own `_. = _.` is 0, as ours is. `<:`,
  `>:` and `~:` split the same way. libjay keeps one answer at every length.
  The Dictionary's own `_.` page (Indeterminate) names `< <: = >: > ~: -:
  <. >.` among the primitives that do not produce consistent results on an
  argument holding `_.`, so there is no documented answer here to match, and
  J for C Programmers says the same thing less formally — `_.` gives wildly
  unpredictable results. The tolerance clause on the Dictionary's `=` page
  is written for a FINITE floating-point or complex operand, which a NaN is
  not, so the tolerant reading never reaches this pair and the exact
  comparison is what remains.
- `/:~` and `\:~` sort by the grade. jconsole special-cases the reflexive
  sort into a routine that puts a NaN somewhere its own `/:` does not:
  `/:~ 2 1 _. 0` is `0 1 _. 2` there, `0 1 2 _.` here, while
  `(2 1 _. 0) /: (2 1 _. 0)` is `0 1 2 _.` on both sides. The Dictionary's
  `_.` page puts `/:` and `\:` on the same list of verbs that do not produce
  consistent results on such an argument, so where a NaN lands in a sorted
  result is not documented; agreeing with ourselves across the reflexive and
  the dyadic spelling is the property left worth keeping.
- `\:` on a TABLE whose rows hold a NaN orders the rows by the same total
  order `/:` uses, a NaN greatest. jconsole answers the reversal of that for
  some such tables (`\: 2 2 $ _. , 1 , 0 , 2`), agreeing with itself neither
  across valences nor across ranks — which is the Dictionary's "`/: \:` do
  not produce consistent results on arguments containing `_.`" seen from the
  inside. One total order across every rank and valence is our reading of an
  undocumented case, not a departure from a documented one.
- `-:` matches a NaN with a NaN and with nothing else. jconsole also matches
  a NaN with any NUMBER — `_. -: 1` and `_. -: _` are 1 there — which is the
  same "no difference large enough" reading as the comparison above. `-:` is
  named on the Dictionary's `_.` list too, so this ground is undocumented as
  well; we take the reading that leaves `-:` an equivalence relation.
- A moving window of an associative verb (`+`, `*`, `<.`, `>.`) is folded in
  blocks rather than strictly right to left, which reorders the float
  rounding — the same regrouping reduction already takes (§5.9). A PREFIX
  scan of an affine step (`[ + c * ]` and its mirror) is regrouped for the
  same reason: the k-th prefix is the sum of `c^i × y[i]`, and one pass
  carries the power of `c` forward instead of folding every prefix from its
  own tail. Every other verb — and every SUFFIX scan, which is the insert's
  own direction and is carried step for step — is folded exactly as the
  insert would.
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
- Neither `⍞` nor `⎕` is in the APL corpus: `jay-corpus` runs one process
  per sentence with nowhere to put a line of input for it. Two things they
  would pin if it could. GNU APL prints a `⎕:` prompt before reading an
  evaluated line and libjay prints nothing — a prompt belongs to a REPL,
  and libjay is not one. And GNU APL ends the output line when the input
  line that wrote it ends, so a bare `⍞←'ab'` there looks as though it
  ended the line; libjay writes exactly the characters, which is what makes
  `⍞←'ab' ⋄ ⍞←'cd'` one `abcd` on both sides. tests/input.rs holds libjay
  to both.
- Catenating a boxed array to an unboxed one is a type error in both
  languages. J agrees; APL2 encloses the simple items instead.
- J's `$:` inside an explicit definition names that definition, which is
  what the dictionary says it does. The jconsole this repository tests
  against reads it as the largest verb in the SENTENCE — which is `$:`
  itself — and raises a stack error for every recursion written with it, so
  the J corpus leaves `$:` out and tests/definitions.rs holds libjay to the
  published rule instead. Recursion by NAME (`fac =. 3 : '… fac …'`) agrees
  with the reference and is in the corpus.

### Differences from GNU APL

Which of these are lineage — GNU APL's ISO/APL2 line against Dyalog's — and
which are libjay's own choice is the subject of "Which APL" above; this
section is the full list of what the corpus actually pins, one entry per
line of `crates/libjay/tests/corpus/apl/divergences.txt`, which asserts that
they keep disagreeing — a silent convergence is a test failure, not a quiet
win. Everything else in the corpus agrees.

`A∘f` binds the array as f's LEFT argument here — Dyalog's bind, and what
J's `m&v` does with the same shape. GNU APL reads the `∘` between two
values as its matrix product and takes it against f's MONADIC answer, so
`1∘+ 10 20 30` is `10 20 30` there and `11 21 31` here, and `2∘× 3 4 5` is
`2 2 2` there and `6 8 10` here. Where neither operand is a function the
matrix product is what libjay answers too, so the reading libjay does not
follow is still reachable.

libjay answers an OVERFLOW where GNU APL refuses one:

- `1E308×2`, `1E308×1E308` and `2⋆1E10` are `∞` here and DOMAIN ERROR
  there, and an infinite operand still multiplies and divides
  (`0×1E308+1E308`, `(1E308+1E308)÷2`). GNU APL's own rule is not uniform —
  it refuses an overflow under `×` `÷` `⋆` and lets one through under `+`
  and `*`, so `1E308+1E308` is `∞` and `2⋆10000` is `∞` there — and no
  published definition asks for the split. libjay refuses arithmetic with
  no VALUE (`÷0`, `⍟0`, `!¯3`, `0⋆¯1`, `¯7○1`, each of which GNU APL
  refuses too) and lets an overflow answer with the infinity it reached.
- `!¯1E20` is refused here and `0` there: the gamma function underflows far
  down the negative axis and GNU APL prints that zero even at a pole, where
  the value is undefined.

libjay is more permissive:

- `∪` is nub over ITEMS, so a matrix is legal; GNU APL's monad takes vectors
  only. The DYADIC set functions — `∪`, `∩` and `~` — follow GNU APL and
  refuse anything deeper than a vector, which is where J's `-.` differs.
- monadic `↓`, monadic `⌷`, `⎕A`, `⎕D`, `∘`, `⍥`, `⍢` and `⌺` are not in
  GNU APL at all — no oracle, tested against hand-written expectations in
  tests/wave4.rs and tests/wave7.rs instead; see "Which APL" above.
- `⍺←v` inside a dfn, and what a dfn's return value is — both in "Which
  APL" above, with the oracle-verified numbers.
- a negative replication count inside a VECTOR left argument — see "Which
  APL" above.

libjay is stricter, or simply elsewhere:

- a large rotate amount is reduced whole. GNU APL truncates the amount to a
  signed 32-bit integer before reducing it modulo the axis, so
  `9223372036854775806⌽1 2 3` is `1 2 3` here — the amount divides 3 — and
  `2 3 1` there, which is the low 32 bits (`¯2`) reduced instead. Probed
  over sixteen magnitudes around 2⋆52, 2⋆53 and 2⋆63; the truncation
  accounts for every one and nothing else does. Everything the amount is
  used for otherwise, `3|9223372036854775806` included, agrees exactly.
- the nested VECTOR display agrees now: a run of adjacent characters is
  text with no separator, and elsewhere the gap widens by the more complex
  neighbour's own shape — nothing extra for a scalar, one column per axis
  for a numeric or boxed structure, one column fewer for a character array
  (a character vector costs nothing, a character matrix one column), and
  the outer margin follows how many `⊂` layers wrap the first and the last
  item. `⍴⍕(1 2)(3 4)` — the entry that used to pin the gap — is gone;
  corpus/apl/nested_display.txt carries the byte-exact cases. A mixed
  array at rank 2 or above still draws with libjay's own uniform spacing;
  what is compared there stays structure — `⍴`, `≡`, `≢` and the leaves
  `∊` brings back.
- grading a nested array is answered there and named as a gap here. Pick
  agrees now: `1 2⊃(1 2)(3 4)` is 2 on both sides, and the divergence entry
  it used to have is gone.

- the circle functions on a REAL argument whose answer is imaginary (`8○3`,
  `¯8○3`, `¯2○2`): GNU APL carries a negative zero into the square root and
  lands on the lower branch, libjay takes the principal one — which is what
  J answers.
- grading a complex vector: libjay orders it by (real, imaginary), which is
  what J's `/:` answers, and GNU APL refuses. The COMPARISONS now follow
  GNU APL's total order — see "Which APL" above — so the two have swapped
  which of them is the permissive one here.
- complex equality. libjay compares the magnitude of the difference against
  `⎕CT`, as its real comparison does; GNU APL's complex comparison is looser
  by roughly the square root of `⎕CT`, so `1J1=1.0000000001J1` is 1 there.
- a sequence yields its last sentence and prints nothing on the way, so
  `1 2 3⋄4 5` is `4 5`; GNU APL prints the value of every statement.
- `⍸` needs numbers. GNU APL extends its one internal total order to `⍸`
  and to nested values as well as to the comparisons, so `'a'⍸1 1` and
  `(⊂⍳3)⍸¯2 0 2` answer there and are refused here. `< ≤ > ≥` themselves
  follow that order now (`Dialect.order_domain`, "Which APL" above); `⌈`
  and `⌊` do not, in either reference.
- the n-wise reduction of an EMPTY argument of rank 2 or more. The reduced
  axis is `1+≢-|n|` items long there as anywhere else, so libjay's answer
  has the argument's rank: `⍴2+/0 3⍴⍳0` is `0 2`. GNU APL drops the axis
  instead — the shape its ORDINARY reduce gives — but only when that length
  is not zero and only when n is not zero, so `⍴1+⌿0 3⍴⍳0` is `0 3` and
  `⍴0+/0 3⍴⍳0` is `0 4` there with the axis kept. The three cannot come
  from one rule; libjay applies the one rule everywhere. One step further,
  `⍴2+/0 0⍴⍳0`: with no cells to fold there is no window to be too long for
  one, so libjay learns the shape from a cell of fills and answers an empty
  where GNU APL measures n against the axis first and refuses. On a
  NON-empty argument the two agree that an over-long window is a domain
  error.
- `0 f/ y` is `1+≢y` copies of what `f/` answers for an empty argument, and
  libjay gives catenation the empty list as its identity — J's rule, and the
  one its own `,/⍬` already follows — so `0,/1 2 3` answers four of them.
  GNU APL says catenation has no identity and refuses, though its own
  `,/⍳0` answers a scalar 0.
- monadic `⊣` is the identity, which is what the status table promises and
  what the Dyalog line has. GNU APL answers a COMMITTED integer scalar 0
  instead: `⊣1 2 3` and `⍴⊣1 2 3` both display nothing, and `(⊣5),9` is
  `0 9` there where it is `5 9` here. The display and the value are the one
  choice, not two.
- the DISPLAY of a complex value in GNU APL drops the imaginary part
  whenever it is smaller than about 1E¯10 in absolute terms, whatever the
  real part is: `1J1E¯11` prints as `1`, and so does `1E¯20J1E¯21`, whose
  imaginary part is a tenth of its real one. The value keeps it —
  `11○1E¯20J1E¯21` is 1E¯21 there — so this is the printer rather than the
  arithmetic. libjay prints both parts of every complex number.
- `⍎''` is the empty program and produces no value at all. GNU APL lets a
  whole SENTENCE be one and prints nothing, the way an assignment does, and
  complains only where the value is wanted (`⍴⍎''` is a VALUE ERROR there).
  Every libjay verb answers with a value, so a `⍎` that reached none is the
  same complaint wherever it stands, pointing at the `⍎`.
- a `{…}` whose body mentions neither `⍺` nor `⍵`. GNU APL runs the body
  where it stands and leaves a VALUE — `{42}` is 42, and `N←{42} ⋄ N 5` is
  the pair `42 5`; the manual lists it as a pitfall. libjay keeps `{…}` a
  FUNCTION whatever its body mentions, which is the Dyalog reading its dfn
  support is built on and the one `corpus/apl/dyalog-dfns.txt` records.
- matching two EMPTY nested arrays compares their prototypes in APL2, and
  libjay compares only the shape and whether the type is character — it
  now CARRIES an empty nested array's prototype, which is what `↑0⍴⊂2 3⍴9`
  answers from, but equality does not read it. `⍬≡''`
  is 0 on both sides and `⍬≡0⍴⊂1` is 1 on both; `⍬≡0⍴⊂⍬` is where the
  prototype would decide, and that is a named gap here.
- an operator's left operand may itself be derived: `+/⍣0`, `⌈¨⍣2` and
  `+/⍤1` are the reduction, the each and the reduction-by-rows repeated or
  ranked. GNU APL binds the inner operator first — it reads `+(/⍣0)` — and
  raises SYNTAX ERROR, so every `f/⍣n` and `f/⍤r` parts company there. The
  fuzz generator parenthesises the operand for exactly this reason.

- `∊` of an empty argument is the empty vector, which is what APL2's
  definition (the simple scalars of the argument, in order) gives and what
  Dyalog answers. GNU APL answers a ONE-element vector holding a zero, so
  `⍴∊⍳0` is 0 here and 1 there.
- an integer near 2⋆63 stays exact. libjay reads such a literal out of its
  text; GNU APL puts it through a double, which rounds it to 2⋆63 before
  the function sees it, so `9223372036854775806|2 3⍴⍳6` is the exact
  residues here and the residues of 2⋆63 there. The encodings and the
  rotations of such a number differ by the rounding alone.
- a field width or a precision past the element ceiling is a limit error
  here: `9223372036854775806⍕1` names the request instead of allocating for
  it. GNU APL falls back to its exponential form and answers ` 1.0E0` as
  though no width had been asked for.
- `x f⍣¯n y` undoes the BOND `x∘f`, one table over both languages, so
  `3-⍣¯1⊢10` undoes `3-⍵` — its own inverse — and answers ¯7. GNU APL
  answers 13, undoing `⍵-3` instead, and only for subtraction: its own
  `3÷⍣¯1⊢10` and `3*⍣¯1⊢8` agree with the bond rule, and the reference J
  reads all three as libjay does.
- libjay's obverse table reaches past GNU APL's, so `⌽⍣¯1`, and the rest of
  the rearranging rows above, answer here and are a DOMAIN ERROR there.

- a dfn operand takes an axis: `{+/⍵}/[1]M` folds the rows exactly as a
  primitive operand would, which is Dyalog's reading. GNU APL takes an axis
  only after a PRIMITIVE function and answers a SYNTAX ERROR for the dfn.
- the enlist of an argument whose every leaf is EMPTY is an empty here,
  keeping its type — `∊''` is `''` and `∊⍬` is `⍬`, which is Dyalog's
  answer too. GNU APL produces ONE element, the prototype: `⍴∊''` is 1
  there and `⍴∊0⍴⊂1 2` is 2. It is the same length its own assertion
  guards elsewhere, since `∊⊂''`, `∊'' ''` and `∊(0⍴0)(0⍴0)` abort the
  interpreter rather than answer, so those are left out of the corpus
  entirely. Where any leaf is non-empty the two agree, and
  `corpus/apl/empty_enlist.txt` records that.
- the empty reduction of the STRUCTURAL functions. GNU APL answers a scalar
  `0` for `,/⍬`, `⍴/⍬`, `↑/⍬`, `⌽/⍬`, `⊖/⍬`, `⍉/⍬`, `⊂/⍬` and `⌹/⍬`, and a
  DOMAIN ERROR for the same empty reduction folded more than once —
  `,/1 0⍴0` is refused there while `+/3 0⍴0` is `0 0 0`. The two halves are
  not one rule, and the `0` half discards the argument's type (`,/''` is 0
  there). Catenation's identity is the empty list here, whatever the cells'
  type, which is also what the J frontend answers for `,/i.0`; the rest are
  named refusals.
- dyadic `∪` sieves only the RIGHT argument, so the left keeps whatever
  repeats it has and `1 1 2∪3` is `1 1 2 3` — the reading the table above
  states and `corpus/apl/dyalog-doc-crawl.txt` records. GNU APL uniques the
  whole result, so its answer is `1 2 3` and `'aab'∪''` is `ab`.
- `⍷` with an EMPTY pattern matches wherever a run of no elements fits, at
  every rank: `''⍷'abc'` is `1 1 1` and `(0 3⍴0)⍷(2 3⍴⍳6)` is `1 0 0`
  twice, both of which GNU APL agrees with. It parts company only where the
  pattern's LAST axis is 0 inside an argument of rank 2 or more, matching
  nowhere there — even though the same pattern in a rank-1 argument matches
  everywhere.
- `x!y` below the diagonal. GNU APL answers 0 for every y under x, integer
  or not, so `1!0.5` is 0 there and 0.5 here; libjay's `!` is the
  generalised binomial its row promises, on the reals and in the complex
  plane.
- `|` and `⋆` under the comparison tolerance. GNU APL tolerates two
  integers one unit apart at 2⋆52 into equality, so
  `4503599627370496|4503599627370497` is 0 there and 1 here, and it reads a
  near-integer exponent as the integer it is near, so
  `¯1⋆¯1.0000000000001` is the real `¯1` there where libjay takes the
  principal value and answers a complex number.
- the display of a nested array whose items are themselves several rows
  deep: GNU APL puts a blank line between the rows of the outer array
  (`3 1⍴(⊂2 2⍴⍳4)`), libjay does not. Layout only — the values match.
- the SYSTEM NAMES, in five places. `⎕PP` starts at libjay's own print
  precision — six significant digits, what every other libjay display uses
  and what its J side uses too — where GNU APL starts at ten; SETTING it
  agrees exactly, and `corpus/apl/sysvars.txt` records that. `⎕A` and `⎕D`
  are names libjay answers and this GNU APL build does not have at all, so
  `⎕NC '⎕A'` is 5 here and ¯1 there. A name libjay accepts starts with a
  letter, so `⎕NC '_'` is ¯1 here and 0 there — its own lexer's rule.
  `⎕SVR` is closed by the sandbox with the rest of the shared-variable
  surface, where GNU APL answers 0 for a variable nothing shared. And `⎕LX`
  is read-only: libjay loads no workspace, so a latent expression has
  nothing to be latent for, and storing one where it could never run would
  be a setting that quietly does nothing.

Three entries are GNU APL's bug rather than a dialect difference, pinned so
that a later release fixing them is noticed. An axis outside the argument's
rank is let through on SOME glyphs there and refused on others, with no rule
joining the two sets: `,[0]M` under `⎕IO←1`, `,[9]M`, `⍪[0]M`, `⌽[0]M`,
`⊖[0]M`, `2⌽[0]M` and `⍪[2]'abc'` all answer with the argument unchanged,
while `⌽[3]M`, `2⌽[9]M`, `⊂[0]M`, `⊂[9]M`, `+/[0]M`, `+⌿[0]M`, `+\[0]M`,
`1↑[0]M`, `1 2/[0]M`, `,[0 4]M` and `M,[0]M` are AXIS ERRORs — and `1↓[0]M`
aborts the interpreter outright with an internal assertion. An EMPTY axis
list splits the same way (`⍪[⍳0]v`, `,[⍳0]v`, `1 2↓[⍳0]M` and `⊂[⍳0]M`
accepted, `⌽[⍳0]M`, `+/[⍳0]M` and `2⌽[⍳0]M` not, `2↑[⍳0]M` a LENGTH ERROR),
and a FRACTIONAL axis outside the range is clamped rather than refused, so
`,[¯0.5]M` is the argument and `,[3.5]M` is `,[2.5]M`. libjay holds every
axis to the rank on every glyph and names the valid range when it refuses;
Dyalog refuses these as well. A scan over an EMPTY argument keeps the
argument's shape when the function is `+` or `×` there and drops it to a
rank-1 empty when it is `-`, so `⍴-\0 3⍴1` is `0 3` here and `0` there
while `⍴+\0 3⍴1` is `0 3` on both sides. A scan whose axis has length 1
loses that axis there, so `+\2 1⍴⍳12` comes back as a 2-vector and `+\,5` as
a scalar; `⌽`, `⌿` and every other axis length are fine. And a decode whose
radix axis is EMPTY — `(⍳0)⊥2 3⍴⍳6`, `(2 0⍴0)⊥⍳0` — has a shape there that
GNU APL agrees with (`⍴` answers 3 and 2) and a value it cannot print: asking
for the value raises DOMAIN ERROR. Every answer is the empty sum, which is
zero, and that is what libjay gives.

## Known gaps

"Not supported yet" is a promise, not a permanent refusal — the
compiler names the feature and the corresponding cell in
[status.md](status.md) is 🔴. Every gap named inline in the
sections above is also collected here.

- Catenating items whose shapes differ fills in J and is refused in APL, as
  each reference has it: `1 2 3 , i. 2 2` overtakes both sides to 3 columns,
  while `(2 2⍴⍳4) ⍪ 1 2 3` is a length error.
- A bonded noun (`n&v`, `u&n`) and an amend's indices (`m}`) may be
  COMPUTED in J: `mp =. +/ . *` then `(m & mp) ^: 9 m`, or `j =. 2 * i. 5`
  then `0 j } b`, are read where the derived verb is applied, as a `^:`
  count is. A noun fork's left tine still has to be a literal, and so does
  APL's `∘` bind (`(⍳3)∘+`), whose operand is folded at the token level
  before any expression exists to defer. A computed GERUND amend —
  `` u`v`w} `` where the gerund is a name — stays a gap on purpose: which
  three verbs the gerund holds decides how the amend PARSES, so it has to
  be known while the program compiles.
- A COMPLEX array grades by (real, imaginary), which is what J's `/:`
  answers and GNU APL refuses; the ordering verbs still refuse complex
  operands, because a permutation is not a claim about size. Ordering
  BOXED items is implemented in both languages now — see the `/:` and `⍋`
  rows above — and so is `Dialect.nested_grade`'s other reading, Dyalog's
  total array ordering, derived from the recorded answers in
  `snapshots/apl/grade.snap` rather than from a document. Two arrays with no
  atoms to separate them are ordered there by the item they WOULD have held:
  a nested empty's prototype, and for a simple one the fill its type
  implies.
- `$:` in a TACIT verb: the reference makes it name the largest verb
  containing it, and libjay resolves it against the explicit definition it
  stands in. A gerund may name it now (`` (base`$:)@.test ``, which is how
  a recursion is written with an agenda), and applying one outside a
  definition says "a self-reference in a tacit verb" rather than calling it
  an error in the program.
- APL's dyadic `@`: `x f@g y`, where both operands read the left argument
  too. The monad is implemented.
- A `cocurrent` or `coclass` whose locale name is COMPUTED: the sentence
  changes the locale at run time, but the sentences after it are READ in the
  locale that was in force, so a name whose part of speech only that switch
  would settle is refused by name. A literal locale name is followed at both
  times.
- A VERB named by an indirect locative (`f__var y`): the locale is a value,
  so what part of speech the name has is not known while the sentence is
  read. Reading and writing a NOUN through one works.
- An explicit modifier whose body NAMES AN ARGUMENT and derives the
  modifier itself is refused: that body belongs to the derived verb, which
  the reference parses only where the verb is applied and libjay parses
  whole at parse time, so it would be parsed for ever. A body that names no
  argument derives at parse time and does stop at its base case, bounded at
  16 deep. Recursion inside the derived verb's own body, by `$:` or by a
  verb's name, works as it does anywhere else.
- Displaying a definition whose body is written on the LINES BELOW, and a
  `{{ }}`: libjay keeps a definition's text only where it was written
  inline, so `f =. 3 : 0` … `)` and then `f` is a named gap where the
  reference gives the listing back. The inline forms display.
- Named on their own, beyond what
  the tables above already mark "not supported yet": a sparse array of
  CHARACTERS or of BOXES, which J has a type code for and refuses to make
  as well; sparseness surviving a verb (see the sparse arrays section
  above); the symbol-table forms of `s:`
  (`0 s:` … `3 s:`, `6 s:`, `7 s:`, `_1 s:`), which describe an
  interpreter's own table rather than the language; a determinant by minors
  of more than 16 rows (the expansion is exponential, and only `-/ . *` over machine numbers has a
  direct method); the two vector output codes of the sequential machine,
  which black-box probing did not pin down — they mark a boundary inside
  the word being collected rather than ending it, and what the machine
  emits at the end of the input after them followed from no rule the probes
  could confirm; and the atomic representation of a
  capped fork or an explicit definition. The 2026-08-30 certification sweep
  added three more: `#.` and `#:` over COMPLEX numbers, which the reference
  answers (`#. 3j4 1j_1` is `7j7`) and libjay refuses by name; `x ! y` for
  a whole x past the width at which the falling factorial is taken with a
  negative or fractional y, which needs the logarithms of the gamma
  function rather than its values; and `2 #: y` for a whole y past 2⁵², where
  libjay converts to a double and loses the last digit that the reference
  keeps (`2 #: 4503599627370497` is 1 there and 0 here). In APL: a variant option other than `⎕CT` and `⎕IO`, or one that is
  not settled when the program is compiled; a nested argument to dyadic
  `⍕`; a stencil with a movement row; a label sharing a definition with
  a control structure; and an axis specification on a function that is not
  a primitive — a dfn (`{⍵}[1]`), a train, or an operator's derived
  function (`⌽⍤0[1]`) — which is refused by name, as are the primitives
  that have no axis form at all.
- The system-name family the 2026-08-28 manual crawl and the 2026-08-29
  certification sweep collected is now answered, and what is left of it is
  four rows with a reason apiece. Answered: `⎕AV` (the atomic vector, whose
  content the standard leaves to the implementation and which libjay
  measured from the reference and adopted, so a code that indexes it means
  the same in both), `⎕PP` and `⎕RL` (read AND set while the program runs,
  each moving what it controls), `⎕NC`, `⎕LX`, `⎕ET`, `⎕EM`, monadic `⎕CR`,
  the dyadic `⎕CR` conversions that rewrite the same bytes another way (5,
  6, 13, 16, 17, 18, 19), and the polynomial half of the `⌹[K]` group (8
  and 9). What is left:
  - `⎕PW`, a page width. libjay's display writes a value in full and folds
    no line, so there is no page to set the width of. A named gap, and one
    a display that wraps would close.
  - `⎕SYL`, ⚪. It reports one interpreter's own build — cores configured,
    hash-table size, input line length, the `./configure` settings — and
    another implementation has no counterpart to put in those rows.
    Answering it would mean impersonating a particular build.
  - `⎕SVR`, ⚪ closed by the sandbox, with `⎕SVO`, `⎕SVQ`, `⎕SVC`, `⎕SVE`
    and `⎕SVS`: it retracts the offer of a SHARED variable, and libjay
    shares no variable with anything. The reference answers 0 because
    nothing was shared there either; libjay closes the surface rather than
    answering about a mechanism it does not have.
  - The rest of dyadic `⎕CR` — the boxed listings (0 to 4, 7 to 9, 20), the
    interpreter's internal record of a value (11, 12, 14, 15), the
    cell-type codes (26) — and the rest of the `⌹[K]` group: `⌹[1]`, a QR
    factorization, and `⌹[7]`, a polynomial written out as text. Each is a
    piece of numerical or presentation work of its own.
- Still named, with the reason sharpened by the 2026-08-29 probes:
  structured variables and associative arrays (`P.x←3`, with intermediate
  members created implicitly) — a name-space feature, not a primitive:
  every assignment, lookup and scope rule would have to learn about dotted
  paths, and the reference gives the whole structure a display of its own
  (`P.x←3 ⋄ P` is an 8-by-2 character matrix there).
- `⎕FX` on text the program ASSEMBLES stays named; "A computed operand"
  above sets out what was tried and where it breaks. It is a named refusal,
  never a wrong answer.

  Every gap is reported by NAME rather than as an unknown
  character: a glyph the language has and libjay has not reached is a
  queue position, and the diagnostic says which one. Three spellings are
  NOT queue positions. `T.` and `t.` (J) and `&` (APL) run in the
  language's own threads, which the sandbox does not open (see "Sandbox"
  above). `⌶` (I-beam) is implementation-defined: what it does is each
  interpreter's own business, and there is no published contract to follow,
  so libjay refuses to invent one rather than promising a behaviour it
  would have to guess at. And `d.` `D.` `D:` `t:` `..` `.:`
  are not in the language the reference implements at all — it rejects
  every one of those spellings as an invalid inflection.
  J's map `{::`, the index specifications of `{` and `}`, amend with a verb
  operand `u}`, the tessellations `;.3` and `;._3`, the rectangle cut
  `x u;.0 y`, the fill shift `|.!.f`, `f.` (fix), `M.` (memo), `L:` and
  `S:` in both valences, `H.` (the hypergeometric series),
  `p.` and `p..` (the polynomial verbs), `m b.` (the boolean and
  bitwise functions) and the explicit modifiers `1 :`, `2 :` and the
  `{{ }}` that names an operand all work, as do APL's `⍳` on a shape, mixed simple
  arrays, prototype fills, partitioned enclose at any rank, dyadic `⍕`,
  `⊆`, `⍛`, `⍢`, `⌺`, `f⍤g`, `⌸`, `f.g`, `f⍠B`, trains, function
  assignment, the branch `→` and the niladic `∇`. J's inner product
  `u . v` works in both valences, and so do the sequential machine
  (dyadic `;:`), format by specification (dyadic `":`) and reading numbers
  out of text (dyadic `".`). J's gerunds are boxed data now, so `` ` ``,
  `@.` and `` `: `` all read the same atomic representations, and `/`,
  `\`, `\.` and `/.` cycle through one; any OTHER adverb over a noun
  operand still says "noun-operand adverbs is not supported yet". Boxes and
  complex numbers are both implemented; bigints and rationals (the exact
  types) are too — see "The numeric tower and the exact types" above — and
  so are symbols, see "Symbols".
- Complex numbers reach every scalar verb, `!` included, the reductions,
  the scans and the structural verbs. They do NOT reach: matrix inverse
  and matrix divide (`%.` / `⌹`, which
  work in f64), `#.`/`⊥` and `#:`/`⊤` (decode and encode), and the fused
  blockwise kernel, which computes in one real type and declines a chain
  that touches complex data — the ordinary pipeline runs it instead, with
  the same answer.
