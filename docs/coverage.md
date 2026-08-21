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
| `,` | ravel | catenate along the LEADING axis; axes other than that one are overtaken to the larger length, which fills (`1 2 3 , i. 2 2` is 3×3) |
| `,.` | ravel items, exactly `,"_1`: each item ravelled, so a list becomes a column | stitch, exactly `,"_1` |
| `,:` | itemize: a leading axis of 1 (`2 3` becomes `1 2 3`) | laminate: the two arguments as the items of a new leading axis (two atoms give shape `2 1`) |
| `#` | tally; extended where the argument is | replicate: item i repeated x[i] times (a scalar x applies to every item, and a scalar y is repeated for every count, so `1 0 1 # 5` is `5 5`) |
| `#.` | base-2 decode (rank 1) | mixed-radix decode; a scalar x is the radix of every digit, a radix of 0 contributes none |
| `#:` | base-2 encode; the width fits the largest magnitude in the WHOLE argument, so the verb has infinite rank | mixed-radix encode; the digit axis is x's own shape, so `2 #: 5` is a scalar and `2 2 2 #: 5` a 3-list |
| `!` | factorial — gamma(y+1), always float; a negative integer is a signed infinity; a complex argument is a named gap | binomial: x things chosen from y, defined on the reals through gamma; complex is the same gap |
| `j.` | `0j1 * y` | `x + 0j1 * y` |
| `r.` | `^ 0j1 * y`: the unit complex at angle y | `x * ^ 0j1 * y`: polar coordinates |
| `":` | format: the characters that display the argument | not supported yet (format with a specification) |
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
| `I.` | indices: index `i` repeated `y[i]` times (rank 1, so a table frames the rows) | interval index: how many items of the ascending x are strictly below each cell |
| `e.` | raze-in: for every element of y, which items of `; y` it holds — the answer is shaped `($y), #items of the raze` | member: cells of x shaped like items of y |
| `E.` | — | find: 1 at each position of y where a copy of x begins, shaped like y's items; a pattern longer than y matches nowhere |
| `A.` | anagram index: where the permutation y's items RANK as stands among the permutations of that length, lexicographically | the x-th permutation of y's items; a negative x counts back from the last. Characters have no anagram index monadically, as in J |
| `C.` | a direct permutation as its cycles (each written from its largest element, the cycles ordered by those), or boxed cycles as the direct permutation | permute. A boxed x is cycles and leaves everything unmentioned in place; a numeric x is a direct permutation, and one shorter than y applies to y's leading items with the rest brought round to the front (`0 1 C. 'abcde'` is `cdeab`). An atom left argument is a named gap |
| `u:` | codepoints become characters; characters are answered with themselves | form 3 gives codepoints, form 10 gives the characters they name; the byte-oriented forms are a named gap |
| `;:` | words: J's own tokeniser over a string, one box per word. A run of numeric literals separated by blanks is ONE word (`;: '1 2 3'` has one), `NB.` swallows the rest of the line, and an unclosed quote is a parse error | not supported yet (sequential machine) |
| `L.` | the boxing level: 0 for anything unboxed, one more than the deepest content otherwise. APL's `≡` counts the array itself as well, so the two differ by one on a simple array | — |
| `".` | do: the characters are compiled as a J program and run HERE, over the names the sentence itself can see — `". 'a =. 3'` assigns in the surrounding scope. A `{name}` hole inside the string has nothing to bind to and is refused | not supported yet (numbers from text) |
| `%.` | matrix inverse — the least-squares pseudo-inverse of a taller matrix; a wider one is refused, a singular one is a domain error | matrix divide: the least-squares solution of `y a = x` |
| `p.` | the roots of the polynomial whose ascending coefficients y holds, as the boxed pair `multiplier ; roots`; a boxed argument of that form converts back to coefficients | the polynomial with ascending coefficients x, at y (Horner); a boxed x is the `multiplier ; roots` form of the same polynomial |
| `p..` | the derivative of the polynomial y's ascending coefficients describe, as coefficients | the integral, with x as the constant term |
| `p:` | the y-th prime, counting from zero | the prime queries: `_1` counts the primes below y, `0` and `1` ask whether it is composite or prime, `2` gives the factorisation as a 2-row table and `3` its top row, `4` and `_4` step to the next and previous prime |
| `q:` | prime factors, ascending, with multiplicity (`q: 1` is empty) | the exponents of the first x primes; `__` gives the primes that divide y over their exponents, as a 2-row table. The negative forms are a named gap |
| `?` | roll: a random value below each element (`? 0` is a uniform double) | deal: x distinct values from `i. y` |
| `?.` | roll from a fixed seed, restarted on every invocation | deal from that fixed seed |
| `{::` | the map: y's box structure with every leaf replaced by the path that fetches it — a boxed list of one index per level descended, empty where the level is a boxed scalar | fetch: follow the path x into y, opening one level a step |
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
`(u y)} y` and `x u} y` is `x (x u y)} y`. `f.` (fix) answers the verb
itself — names are substituted where they are used, so there is nothing
left to fix — and `M.` (memo) keeps every answer u has given and returns it
again for the same arguments, in a cache that belongs to the derived verb.
`m b.` is one of the thirty-two boolean functions: 0 to 15 are the truth
tables on two bits, and sixteen higher is the same function on every bit of
a pair of integers, so `17 b.` is bitwise and and `22 b.` bitwise xor.
`u b. 0` answers u's three ranks; the other characteristics are named gaps.
The dyad of `\.` is the outfix:
`x u\. y` applies u to y with each run of x consecutive items left out, so
there are `1 + (#y) - x` results.

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
reversed, and `x u;.0 y` takes the one block x describes — x is a vector of
sizes, or two rows of origins and sizes, and a negative size reverses that
axis; `x u;.3 y` and `x u;._3 y` tessellate: x is the block size, or two
rows of movement and size, and u runs on every block — `;.3` keeps the short
blocks at the far edge, `;._3` takes only the whole ones. A negative block
size reverses its axis there too, but only where the movement row is
written out: given a bare vector of sizes the reference answers with
something its magnitude plays no part in, so libjay names that gap rather
than guessing at it); `!.` (fit: on the verbs whose
meaning uses the comparison tolerance it replaces that tolerance, so `=!.0`
compares exactly; on `|.` it gives the FILL instead — `x |.!.f y` shifts
rather than rotates, an item moved past an end is dropped and the place it
left takes f, and the monad `|.!.f y` is `_1 |.!.f y`. On any other verb
J's `!.` fill is a named gap); `[:` (cap); `&.` and `&.:` (under, see
below); `::` (adverse: `u :: v` applies u, and if the LANGUAGE refuses it
applies v to the same arguments instead — a gap in libjay is a promise, not
an error a program may handle, and goes straight through); `:.` (obverse:
`u :. v` declares v to be what undoes u, which is all that `^:_1` and `&.`
need of it); `` ` `` (tie) and `@.` (agenda).

`u L: n` and `u S: n` apply u at a boxing level: u runs on every subarray
whose `L.` is n or below, and `L:` puts each answer back in the box its
operand came from while `S:` spreads them into the items of one array. A
negative n counts down from the argument's own level. Dyadically both
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

The representation is reconstructed from the verb tree rather than kept
from the source, so the spellings that differ only by the rank they set are
recovered by matching that rank: `u@v` is `u@:v` at v's ranks, `u&v` and
`u&.v` likewise. Two do not survive the trip and are named rather than
guessed at: a capped fork (`[: f g` is an atop by the time the tree has it,
so it writes itself out as `@:`) and any verb libjay has no J spelling for,
which reports "the atomic representation of … is not supported yet".

### The obverse table

`u&.v`, `u&.:v` and the negative powers `u^:_n` all rest on one question:
what undoes v? libjay answers it from a table rather than by searching, and
the table holds exactly the verbs whose inverse is another verb it can
already write down:

- self-inverse: `+` `-` `%` `-.` `|.` `|:`
- paired: `^`/`^.`, `*:`/`%:`, `+:`/`-:`, `>:`/`<:`, `<`/`>`, `#.`/`#:`
- bonded arithmetic: `n&+` and `+&n` are both undone by `-&n`, `n&*` and
  `*&n` by `%&n` — the noun comes off the RIGHT whichever side it was bonded
  to — plus `^&n` (the n-th root), `n&^` (the base-n logarithm), and `n&-`
  and `n&%`, which undo themselves

Everything built out of those inverts by inverting its parts: `u@:v` and
`u&:v` invert in the other order, `u"r` and `u!.n` keep their modifier,
`u^:n` becomes the obverse applied n times, and `u :. v` supplies an answer
where the table has none. A verb the table does not reach says so by name —
"the obverse of (+/ % #) is not supported yet" — rather than guessing at a
numerical inverse. Three things read the table besides `&.`: J's `u b. _1`,
which answers with the obverse's SPELLING rather than the verb, APL's `⍢`
(under), and `⍣¯1`.

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
`crates/libjay/tests/corpus/apl/`). Every Dyalog-side cell below was written
from Dyalog's published documentation, not from a live comparison.

Dyalog IS recorded now, in its own `dyalog:` column of the same snapshots
(docs/testing.md), on the recording machine only. Those recordings gate
nothing: a difference from Dyalog is the backlog a future Dyalog dialect
would have to work through, and `jay-corpus stats apl --dialect-diff` is
what measures it. `corpus/apl/dyalog-probe.txt` is the theme aimed at the
table below. Where a recording ever contradicts a cell here, the recording
wins and the cell changes.

Every place the two lines are known to diverge, and which one libjay
follows today. Rows marked "verified against the oracle" were re-checked
directly against `LIBJAY_ORACLE_APL` while writing this table, not just
read off an old note:

| Feature | APL2 / GNU APL (oracle) | Dyalog (published docs, not run) | libjay follows |
|---|---|---|---|
| monadic `↑` | first: the first element of the ravel, disclosed | mix / disclose | GNU APL — `↑1 2 3` is `1` |
| monadic `⊃` | disclose / mix: items combined into one array, filled | first: pick the first item, disclosed | GNU APL — `⊃(1 2)(3 4)` mixes to a 2×2 array |
| dyadic `⌷` | one scalar index per axis of y | an enclosed index vector, one operand | GNU APL — `(⊂1 2)⌷y` is a RANK ERROR on both sides, verified against the oracle |
| `⊂`/`⊆` dyadic | `⊂` is partitioned enclose: opens where the left argument's flags rise | the same partition operation is spelled `⊆`; dyadic `⊂` is a different function | libjay ships both spellings for the one partition rule — `x⊂y` and `x⊆y` give the same answer, matching GNU APL's `⊂` |
| `⊂5` (enclosing a simple scalar) | identity: `5`, depth 0 | identity, same | both — not a divergence, listed because it underlies the row above |
| `⎕CT` default | `1e¯13`, scaled by the LARGER magnitude — verified against the oracle | `1e¯14` (per Dyalog's documentation) | GNU APL, value and rule both |
| dfn return value | the LAST statement — verified against the oracle (`2×{⍵+1⋄⍵+2}5` is `14`, i.e. `2×7`, not `2×6`) | the first statement that is not an assignment | GNU APL — GNU APL also *echoes* every unassigned statement's value to stdout on its way through, which is a separate display quirk, not a different return value |
| `⍺←` inside a dfn | assigns UNCONDITIONALLY — verified against the oracle (`F←{⍺←10⋄⍺+⍵}⋄3 F 5` is `15` there) | a DEFAULT: fills `⍺` only where no left argument arrived | neither running oracle — the published dfn model, which is `8` here, not `15` |
| ordering `< ≤ > ≥` on complex numbers | extends them to a lexicographic order on (real, imaginary) — verified against the oracle (`2J3<2J5` is `1` there) | refuses, as the standard does | neither running oracle — refused, matching the standard |
| negative replication count in a VECTOR (`¯1 2/1 2`) | a LENGTH ERROR — restricts a negative count to a scalar left argument, verified against the oracle | legal: a run of fills, the general rule | neither running oracle — the general APL2/Dyalog rule, which is `0 2 2` here |
| trains `(f g h)` | not in APL2, not in GNU APL — a SYNTAX ERROR, verified against the oracle | a Dyalog 14+ feature | DYALOG, as an extension shipped on by default (`Dialect.trains`); `Dialect(trains=False)` gives GNU APL's refusal back |
| tacit / function assignment `F←+/` | a SYNTAX ERROR, verified against the oracle | supported | DYALOG, under the same `trains` setting: a function may stand where a value belongs, or it may not, and one flag decides both |

Three of the rows above are where libjay follows neither running reference:
GNU APL's own implementation departs from the ISO/APL2 line it otherwise
embodies (`⍺←`, complex ordering, the vector replication count), and libjay
follows the published rule — which happens to be Dyalog's rule too in all
three cases — over the oracle's quirk. Everywhere else in this codebase,
GNU APL wins: these three are pinned as deliberate exceptions in
`crates/libjay/tests/corpus/apl/divergences.txt` precisely so a silent
convergence gets noticed. The dyadic `⌷` divergence between GNU APL and
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

v0.1.0 implements one APL: the table above is the checklist, not a
roadmap commitment. A Dyalog (or other) dialect is planned as a
preinitialised `Dialect` object chosen at compile time, the same way
`⎕IO` is chosen today (`APL.Dialect.gnu` / `APL.Dialect.dyalog`) — never
global state, never a guess from the source text. Every row above except
`trains` still stays hard-wired to the GNU/APL2 answer; the point of
pulling them into one table is that generalising later is a matter of
filling in a second column, not re-deriving the list, and `trains` is what
that second column looks like once one row is filled in.

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
| `=` `<` `≤` `>` `≥` | — | comparisons (0/1) |
| `≠` | nub sieve: 1 at each item that has not occurred before | not equal (0/1) |
| `∧` | — | LCM (logical and on booleans; the Gaussian one on complex) |
| `∨` | — | GCD (logical or on booleans; the Gaussian one on complex) |
| `~` | not (the argument must be 0 or 1) | without: x's items that y has not |
| `⍴` | shape of | reshape: x lays out y's ELEMENTS, cyclically, which is where APL and J part company above rank 1. An empty y fills with the type's fill |
| `⍳` | index generator (respects `⎕IO`): one length gives the counting vector, two or more give an array of that shape whose elements are the boxed coordinate vectors | index of (respects `⎕IO`; absent gives `⎕IO + ≢x`) |
| `⍉` | transpose | dyadic transpose: x says, for each axis of y in turn, which axis of the RESULT it becomes; a destination two axes share runs them together, which is the diagonal, and every axis of the result must be named |
| `↓` | split: the vectors along the LAST axis, each enclosed, laid out in the remaining axes' shape (no oracle — see "Which APL" above) | drop |
| `,` | ravel | catenate along the LAST axis. Axes other than that one must conform: APL refuses the ragged case that J fills, which is where the two rules part company |
| `⍪` | table: one row per item, holding that item's elements (a scalar gives 1×1, a vector n×1) | catenate along the LEADING axis |
| `!` | factorial (always float); a complex argument is a named gap | binomial, J's argument order; the same gap |
| `⍕` | format: the characters that display the argument | format by specification: x is one width-and-precision pair per column of y's last axis, one pair for all of them, or a lone precision, which takes the width the values need plus a separating blank. A value that does not fit its field is a domain error; a nested y is a named gap |
| `⊥` | — | mixed-radix decode |
| `⊤` | — | mixed-radix encode |
| `⌽` | reverse each row (last axis) | rotate each row (last axis) |
| `⊖` | reverse the items (leading axis) | rotate the leading axis |
| `≢` | tally | not match |
| `∊` | enlist: every leaf element, in ravel order, as a vector | membership, element by element (an element of a nested array is a whole array) |
| `⊂` | enclose — except that a simple scalar is its own enclosure, so `⊂5` is `5` | partitioned enclose: a partition opens where x rises (`x[i] > x[i-1]`, reading `x[¯1]` as 0) and an item flagged 0 is dropped. Rank 2 and above partitions the LAST axis, once per cross section, and the axes ahead of it frame the answer |
| `⍸` | where: index `i` repeated `y[i]` times, from `⎕IO`; a rank-2 or higher argument gives one boxed coordinate vector per occurrence | interval index: how many items of the ascending x are at or below each cell, plus `⎕IO - 1`. The interval is closed on the left here and open in J's `I.`: `1 3 5⍸3` is 2 where `1 3 5 I. 3` is 1 |
| `⌷` | materialise: the argument itself (no oracle — see "Which APL" above) | index: one item of x per axis of y, and the count must equal the rank. An item is a scalar, which drops its axis, or an ENCLOSED vector, which keeps it and selects that many — `(⊂1 2)⌷5 6 7 8` is `5 6` |
| `⌹` | matrix inverse — the pseudo-inverse of a taller matrix; wider is refused, singular is a domain error | matrix divide: the least-squares solution of `y a = x` |
| `?` | roll: a random value in `⎕IO .. ⎕IO+y-1` (`?0` is a domain error) | deal: x distinct values from that range |
| `⊃` | disclose: the items mixed into one array, filled where their shapes differ | pick: each item of x is one step of a path — a simple step indexes the items, a boxed one is a whole coordinate vector |
| `↑` | first: the first element of the ravel, disclosed; the type's fill when there is none | take; overtaking a NESTED array fills with the first item's prototype — that item's shape with a zero for every number and a blank for every character, nested to the same depth |
| `≡` | depth: 0 for a simple scalar, 1 for a simple array, one more than the deepest box | match: same shape and values, else 0 |
| `∪` | nub: distinct items, first-occurrence order | union: x's items, then y's items that are new. Only the right argument is sieved, so x keeps whatever repeats it has |
| `∩` | — | intersection: x's items that y also has, in x's order |
| `⍲` `⍱` | — | nand / nor; both arguments must already be 0 or 1 |
| `⍷` | — | find: 1 at each position of y where a copy of x begins |
| `⍎` | execute: the characters are compiled as an APL program and run HERE, over the names the sentence itself can see | — |
| `⎕UCS` | codepoints become characters, characters become their codepoints | — |
| `\` `⍀` | — | expand, after an operand: every 1 takes the next item, every 0 leaves a fill |
| `⍋` `⍒` | grade up / down (stable; respects `⎕IO`) | collating grade: every character of y is keyed by where it FIRST occurs in the collating array x — the coordinate read with the last axis most significant, and one past the end for a character x does not hold — and the items of y are ordered by those keys read left to right |
| `⊢` `⊣` | same | right / left |
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
and the whole strand is a single operand. Every primary contributes one
item, except a run of numeric literals, whose numbers are items of their
own — which is why `1 2 (3 4)` has three items, `'ab' 'cd'` has two, and
`1 2 3` is still one simple integer vector. A strand of simple scalars of
different types is APL2's MIXED SIMPLE array: `1 'a'` has two items and
depth 1. libjay keeps one as boxed scalars — a box holding a simple scalar
is that scalar in APL, so nothing else can be confused with it — and it
reports depth 1, displays without a nested display's spacing, and refuses
to be disclosed any further.

The missing valences in the table above are marked "not supported yet".
The glyphs and features with no oracle at all — because GNU APL lacks the
valence outright, or because the feature is Dyalog's own — are listed in
"Which APL" above.

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

A bare `∘` is Dyalog's beside `f∘g` — monadically `f g y`, dyadically
`x f (g y)`, so g prepares the RIGHT argument and the left one arrives
untouched. `⍥` (over) prepares both: `x f⍥g y` is `(g x) f (g y)`, which is
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

A dfn that names `⍺⍺` or `⍵⍵` is an OPERATOR rather than a function: it
takes the function on its left, and one on its right where it named `⍵⍵`.
`+{⍺⍺/⍵}1 2 3` is 6, and naming the operator keeps it one, so
`TWICE←{⍺⍺ ⍺⍺ ⍵} ⋄ -TWICE 5` is 5. The operands are bound under those two
names for as long as the body runs. GNU APL has no dfn operators either.

The `⎕`-names are read-only and pure: `⎕A` and `⎕D` are the ISO constants
(GNU APL has no value for either), `⎕IO` and `⎕CT` report the dialect the
compiler was given, and `⎕UCS` converts between characters and codepoints.
Assigning any of them is refused — the dialect fixed them before the
program ran. The ones that would read a clock, a workspace or a filesystem
(`⎕TS`, `⎕AI`, `⎕FIO` and their relatives) are refused with "closed by the
sandbox", which is the sandbox speaking rather than a queue position — see
"Sandbox" below. `⎕` and `⍞` on their own are input rather than names: see
the same section.

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
Indexed assignment (`A[2]←99`) works — see "Indexed assignment"
below; the brackets above are read-only in every OTHER respect (an axis
specification, a bare index expression) — only `A[i]←v`/`A[i;j]←v` write.

`/` and `⌿` are operators after a function and replicate after an operand;
names are always values in this subset, so which one is meant is decided by
the token to the left and nothing else. Parentheses around a bare operator
glyph are transparent, so `1 0 1(/)1 2 3` is the replication `1 0 1/1 2 3`
— the token OUTSIDE them is what decides the reading, which is what the
reference does with it too.

`←` assignment (incl. inline), `⎕←` output, `⋄` and newline sentence
separators, `⍝` comments, `¯` negatives, `''` strings. Index origin is a
dialect setting of the compiler instance (`⎕IO` as a variable is
deliberately not runtime state).

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
`*/ >: i. 25x` is the exact factorial. `$`, `#`, `#.`, `#:`, `p:` and `q:`
carry the argument's exactness into their answer, as J does: a count of an
extended or rational argument is an extended integer, and a count of a
machine one stays machine.

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

## Explicit definitions

A definition is a verb (a function) like any other: it takes a rank, joins a
train, is an operand to a modifier, and can be named.

J writes it four ways. `3 : '…'` is a monad whose argument arrives as `y`;
`4 : '…'` is a dyad with `x` and `y` as well. Either with `0` in place of
the string takes its body from the lines below, ending at a line that is a
lone `)`. `{{ … }}` is the direct form, on one line or over several, and its
own words decide its valence: a body that mentions `x` is a dyad. `13 : '…'`,
which reads an explicit body and writes the tacit verb that matches it, is
named as a gap.

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
`{{)n`, which makes a noun of the body's text, is named as a gap.

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
The consequence is that a body which derives its own modifier — J's way of
writing a recursive one — would parse for ever, and libjay names it as a gap
rather than looping.

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

APL's own control flow, the one the reference does have, is the BRANCH. A
line of a `∇`-definition may begin with a label — `L1:` — and `→` takes a
line number: an empty target falls through to the next line, a number that
is a line of this definition continues there, and anything else (`→0` above
all) leaves the definition. A label's value is its line number, so the
conditional branch is written `→(cond)/LABEL`: replicate answers the label
where the condition holds and an empty vector where it does not. A branching
definition is run statement by statement rather than as a block, which is
what lets a `→` inside a loop leave it. A label and a control structure in
one definition is a named gap: a control structure folds several lines into
one statement, and the numbering a label stands for would stop meaning
anything. libjay stops a definition that has branched 2²² times rather than
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

## Indexed assignment

APL's `A[i]←v` and `A[i;j]←v` read the named value, copy it, write the
selected part, and give the copy the name. Another name that was taken from
the array before the write keeps what it had. An elided slot selects its
whole axis; a scalar slot drops its axis from the shape the value has to
match, and a scalar value spreads over the whole selection. A value of a
wider type widens the array rather than being truncated into it. An index
outside its axis is a domain error naming the axis and its length.

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

**Neither.** A foreign that only computes is an ordinary queue position and
says "not supported yet" with its number: `9!:18` (the settings), `3!:1`
and the rest of the conversions, `4!:` (names), and the rest of `5!:`
(representation).
`3!:0` is implemented — it is the type code J reports for an element type
(boolean 1, character 2, integer 4, float 8, complex 16, box 32, extended
64, rational 128), which is cheap and useful for a test that wants to name
a type. `5!:1` is too: `5!:1 <'name'` answers with the ATOMIC
REPRESENTATION of whatever the name stands for, boxed — the same data a
gerund is made of, so a verb gives its spelling or its box tree and a value
gives the noun pair `('0'; <value)`.

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

- Monadic `÷` (APL reciprocal) of 0 currently follows J's rule (infinity)
  instead of raising a domain error like dyadic `÷`.
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
- `3!:0` reports the storage a value actually has, and libjay's storage of
  a J LITERAL is not always J's: jconsole narrows one whose atoms are all 0
  or 1 to boolean and answers 1, where libjay keeps `1` and `1 0 1` as
  integers and answers 4. Everything computed agrees — `3!:0 (1=1)` is 1 on
  both sides, `3!:0 (i.5)` is 4 on both — and every other type code matches
  exactly.
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

libjay follows J where APL2 stops at DOMAIN ERROR:

- monadic `÷0` is `∞` and `⍟0` is `¯∞` (the first is already listed above).
- `!¯1` is `∞` (the gamma pole) and `¯7○1` — artanh 1 — is `∞`.
- the neutral cell of `⌈` and `⌊` over no items is `¯∞`/`∞`, where GNU APL
  uses the largest representable magnitudes. Every other entry of the
  identity table now matches both references exactly.

libjay is more permissive:

- `∪` is nub over ITEMS, so a matrix is legal; GNU APL's monad takes vectors
  only. The DYADIC set functions — `∪`, `∩` and `~` — follow GNU APL and
  refuse anything deeper than a vector, which is where J's `-.` differs.
- monadic `↓`, monadic `⌷`, `⎕A`, `⎕D`, `∘`, `⍥`, `⍢` and `⌺` are not in
  GNU APL at all — no oracle, tested against hand-written expectations in
  tests/wave4.rs and tests/wave7.rs instead; see "Which APL" above.
- dyadic `⊖` reads a vector left argument per axis, GNU APL per column
  (already listed above).
- `⍺←v` inside a dfn, and what a dfn's return value is — both in "Which
  APL" above, with the oracle-verified numbers.
- a negative replication count inside a VECTOR left argument — see "Which
  APL" above.

libjay is stricter, or simply elsewhere:

- the nested DISPLAY is libjay's own: one space between items and one
  around the whole, where GNU APL spaces items more widely. Only the
  length of `⍕` makes the difference visible to the comparison, which
  ignores whitespace, so `⍴⍕(1 2)(3 4)` is the entry that pins it. Nested
  DISPLAYS are kept out of the corpus for that reason; what is compared
  there is structure — `⍴`, `≡`, `≢` and the leaves `∊` brings back.
- grading a nested array is answered there and named as a gap here. Pick
  agrees now: `1 2⊃(1 2)(3 4)` is 2 on both sides, and the divergence entry
  it used to have is gone.

- the circle functions on a REAL argument whose answer is imaginary (`8○3`,
  `¯8○3`, `¯2○2`): GNU APL carries a negative zero into the square root and
  lands on the lower branch, libjay takes the principal one — which is what
  J answers.
- ordering a complex number — see "Which APL" above. Grading goes the other
  way: libjay grades a complex vector by (real, imaginary), which is what
  J's `/:` answers, and GNU APL refuses.
- complex equality. libjay compares the magnitude of the difference against
  `⎕CT`, as its real comparison does; GNU APL's complex comparison is looser
  by roughly the square root of `⎕CT`, so `1J1=1.0000000001J1` is 1 there.
- a sequence yields its last sentence and prints nothing on the way, so
  `1 2 3⋄4 5` is `4 5`; GNU APL prints the value of every statement.
- `< ≤ > ≥` need numbers. GNU APL extends them to characters, ordering
  characters among themselves and before every number, so `'a'<'b'` and
  `'a'<5` are both 1 there. libjay refuses, as the standard and J do and as
  it already refuses to order a complex number.
- monadic `⊣` is the identity, which is what the status table promises and
  what the Dyalog line has. GNU APL gives it no result at all: `⊣1 2 3` and
  `⍴⊣1 2 3` both display nothing.
- matching two EMPTY nested arrays compares their prototypes in APL2, and
  libjay compares only the shape and whether the type is character. `⍬≡''`
  is 0 on both sides and `⍬≡0⍴⊂1` is 1 on both; `⍬≡0⍴⊂⍬` is where the
  prototype would decide, and that is a named gap here.
- an operator's left operand may itself be derived: `+/⍣0`, `⌈¨⍣2` and
  `+/⍤1` are the reduction, the each and the reduction-by-rows repeated or
  ranked. GNU APL binds the inner operator first — it reads `+(/⍣0)` — and
  raises SYNTAX ERROR, so every `f/⍣n` and `f/⍤r` parts company there. The
  fuzz generator parenthesises the operand for exactly this reason.

Two entries are GNU APL's bug rather than a dialect difference, pinned so
that a later release fixing them is noticed. A scan whose axis has length 1
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
- An explicit modifier whose body derives the modifier itself is refused:
  libjay derives at parse time, so the body would be parsed for ever.
  Recursion inside the derived verb's own body, by `$:` or by a verb's name,
  works as it does anywhere else. `13 : '…'` and `{{)n` are gaps too.
- Named on their own, beyond what
  the tables above already mark "not supported yet": J's dyad of `;:` (the
  sequential machine), `s:` (symbols), `$.` (sparse), the inner product
  `u . v`, a NEGATIVE block size in a tessellation with the movement row
  left implicit, the atomic representation of a capped fork or an explicit
  definition, and `!.` as a fill on any verb but `|.`; APL's `⍠` (variant),
  `⌶` (I-beam), `&` (spawn), a nested argument to dyadic `⍕`, a stencil
  with a movement row, and a label sharing a definition with a control
  structure.
  The APL glyphs are reported by NAME rather than as unknown
  characters: a glyph the language has and libjay has not reached is a
  queue position, and the diagnostic says which one. `T.` and `t.` are not
  queue positions: both run a verb in J's own threads, which the sandbox
  does not open (see "Sandbox" above), and `d.` `D.` `D:` `t:` `..` `.:`
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
  `⊆`, `⍛`, `⍢`, `⌺`, `f⍤g`, `⌸`, trains, function assignment, the branch
  `→` and the niladic `∇`. J's gerunds are boxed data now, so `` ` ``,
  `@.` and `` `: `` all read the same atomic representations. Boxes and
  complex numbers are both implemented; bigints and rationals (the exact
  types) are too — see "The numeric tower and the exact types" above.
- Complex numbers reach every scalar verb, the reductions, the scans and
  the structural verbs. They do NOT reach: `!` (the gamma function of a
  complex argument), matrix inverse and matrix divide (`%.` / `⌹`, which
  work in f64), `#.`/`⊥` and `#:`/`⊤` (decode and encode), and the fused
  blockwise kernel, which computes in one real type and declines a chain
  that touches complex data — the ordinary pipeline runs it instead, with
  the same answer.
