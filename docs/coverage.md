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

## Known divergences from the references (deliberate, revisit later)

- Float comparisons are exact; J's default comparison tolerance (2⁻⁴⁴) is
  not yet implemented.
- Comparing characters with numbers is a type error here; J answers 0.
- Monadic `÷` (APL reciprocal) of 0 currently follows J's rule (infinity)
  instead of raising a domain error like dyadic `÷`.
- No boxes / nested arrays, scan, windows, catenate, index-of, dyadic
  transpose, `⎕`-variables, control words, verb/adverb definitions yet —
  all "not yet", category 2.
