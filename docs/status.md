# Language coverage status

One row per spelling, one circle per valence. The inventory is the whole
published vocabulary of each language, not the part libjay reaches today.

| | Meaning |
|---|---|
| 🟢 | implemented — differential-tested wherever an oracle covers it |
| 🟡 | partial: works, with the caveat named in the row |
| 🔴 | not yet — a promise, and the compiler says so by name |
| ⚪ | absent here by design (nulls and other things libjay refuses to guess at) |
| — | the language gives this spelling no meaning in that valence |

Nothing is permanently excluded. A 🔴 is a queue position; a ⚪ is a
deliberate refusal to invent data, not a closed door.

Counts below cover the primitive tables (verbs, adverbs, conjunctions,
nouns), one count per valence the language defines; the syntax/feature
tables are listed separately and not counted.

**J: 82 green / 14 partial / 87 red of 183 valences in the inventory.**

**APL: 59 green / 16 partial / 41 red of 116 valences in the inventory.**

## J — verbs

### Arithmetic and scalar

| Spelling | Monad | Dyad |
|---|---|---|
| `+` | 🟢 conjugate | 🟢 plus |
| `-` | 🟢 negate | 🟢 minus |
| `*` | 🟢 signum | 🟢 times |
| `%` | 🟢 reciprocal | 🟢 divide |
| `^` | 🟢 exponential | 🟢 power |
| `^.` | 🟢 natural log | 🟢 logarithm |
| `%:` | 🟡 sqrt; negative needs complex | 🟡 root; same |
| `<.` | 🟢 floor | 🟢 lesser of |
| `>.` | 🟢 ceiling | 🟢 larger of |
| `\|` | 🟢 magnitude | 🟢 residue |
| `<:` | 🟢 decrement | 🟢 less or equal |
| `>:` | 🟢 increment | 🟢 larger or equal |
| `+:` | 🟢 double | 🔴 not-or |
| `*:` | 🟢 square | 🔴 not-and |
| `-:` | 🟢 halve | 🟢 match |
| `-.` | 🟢 not (`1-y`) | 🔴 less |
| `+.` | 🔴 real/imaginary | 🟡 GCD; integral values only |
| `*.` | 🔴 length/angle | 🟡 LCM; integral values only |
| `!` | 🟢 factorial | 🟢 out of |
| `o.` | 🟢 pi times | 🟡 circle; real k only, 8–12 not yet |
| `%.` | 🔴 matrix inverse | 🔴 matrix divide |
| `j.` | 🔴 imaginary | 🔴 complex |
| `r.` | 🔴 angle | 🔴 polar |
| `p.` | 🔴 roots | 🔴 polynomial |
| `p..` | 🔴 poly. derivative | 🔴 poly. integral |
| `p:` | 🔴 primes | 🔴 primality |
| `q:` | 🔴 prime factors | 🔴 prime exponents |
| `?` | 🔴 roll | 🔴 deal |
| `?.` | 🔴 roll (fixed seed) | 🔴 deal (fixed seed) |
| `x:` | 🔴 extend precision | 🔴 to rational |

### Comparison and logic

| Spelling | Monad | Dyad |
|---|---|---|
| `=` | 🔴 self-classify | 🟢 equal |
| `<` | 🟢 box | 🟢 less than |
| `>` | 🟢 open | 🟢 larger than |
| `~:` | 🔴 nub sieve | 🟢 not-equal |
| `~.` | 🟢 nub | — |

`-:` (halve / match) is in the arithmetic table. Comparison is exact; J's default tolerance is a feature-table row below.

### Structural

| Spelling | Monad | Dyad |
|---|---|---|
| `$` | 🟢 shape of | 🟡 reshape; an empty argument is refused, not filled |
| `#` | 🟢 tally | 🟢 copy |
| `,` | 🟢 ravel | 🟡 append; unequal item shapes need fill |
| `,.` | 🔴 ravel items | 🟢 stitch |
| `,:` | 🟢 itemize | 🟢 laminate |
| `\|.` | 🟢 reverse | 🟢 rotate |
| `\|:` | 🟢 transpose | 🔴 dyadic transpose |
| `{.` | 🟢 head | 🟢 take |
| `}.` | 🟢 behead | 🟢 drop |
| `{:` | 🟢 tail | — |
| `}:` | 🟢 curtail | — |
| `#.` | 🟢 base 2 | 🟢 base |
| `#:` | 🟢 antibase 2 | 🟢 antibase |

### Selection, search, sort

| Spelling | Monad | Dyad |
|---|---|---|
| `{` | 🔴 catalogue | 🟡 from; atom indices, no boxed index specs |
| `{::` | 🔴 map | 🔴 fetch |
| `i.` | 🟢 integers | 🟢 index of |
| `i:` | 🔴 steps | 🔴 index of last |
| `I.` | 🔴 indices | 🔴 interval index |
| `e.` | 🔴 raze in | 🟢 member of |
| `E.` | — | 🔴 member of interval |
| `/:` | 🟡 grade up; boxes need total ordering | 🟢 sort |
| `\:` | 🟡 grade down; boxes need total ordering | 🟢 sort |
| `A.` | 🔴 anagram index | 🔴 anagram |
| `C.` | 🔴 cycle-direct | 🔴 permute |

### Boxes, format, system

| Spelling | Monad | Dyad |
|---|---|---|
| `;` | 🟢 raze | 🟢 link |
| `;:` | 🔴 words | 🔴 sequential machine |
| `L.` | 🔴 level of | — |
| `":` | 🟢 default format | 🔴 format by specification |
| `".` | 🔴 do — sandboxed design needed | 🔴 numbers |
| `u:` | 🔴 unicode | 🔴 unicode |
| `s:` | 🔴 symbol | 🔴 symbol |
| `[` | 🟢 same | 🟢 left |
| `]` | 🟢 same | 🟢 right |
| `echo` | 🟢 print | — |

### Nouns and constant verbs

| Spelling | Status |
|---|---|
| `a.` alphabet | 🔴 |
| `a:` ace (boxed empty) | 🔴 |
| `_` `__` infinities | 🟢 |
| `_.` indeterminate | 🔴 |
| `_9:` … `9:`, `_:` constant verbs | 🔴 |

## J — adverbs

| Spelling | Monad | Dyad |
|---|---|---|
| `/` | 🟢 insert | 🟢 table |
| `\` | 🟢 prefix | 🟢 infix |
| `\.` | 🟢 suffix | 🔴 outfix |
| `/.` | 🔴 oblique | 🔴 key |
| `~` | 🟢 reflex | 🟢 passive |
| `}` | 🔴 item amend | 🔴 amend |

## J — conjunctions

| Spelling | Status |
|---|---|
| `"` rank | 🟡 noun ranks; `u"v` not yet |
| `@` atop | 🟢 |
| `@:` at | 🟢 |
| `&` bond / compose | 🟡 verbs compose, literal nouns bond; computed nouns not yet |
| `&:` appose | 🟢 |
| `&.` under | 🟡 only `u&.>` (each); the general case needs inverses |
| `&.:` under | 🔴 |
| `^:` power | 🟡 literal count or `_`; `u^:v` and negatives not yet |
| `.` dot product | 🔴 |
| `:` explicit definition | 🔴 |
| `;.` cut | 🔴 |
| `!.` fit (tolerance) | 🔴 |
| `!:` foreign | 🔴 sandboxed design needed |
| `` ` `` tie (gerund) | 🔴 |
| `` `: `` evoke gerund | 🔴 |
| `@.` agenda | 🔴 |
| `[:` cap | 🟢 |
| `::` adverse | 🔴 |
| `:.` obverse | 🔴 |

Other modifiers, all 🔴: `b.` `d.` `D.` `D:` `f.` `H.` `L:` `M.` `S:` `t.`
`t:` `T.` `..` `.:` `$.` — 15 spellings, none recognised yet.

## J — syntax and features

| Feature | Status |
|---|---|
| Forks `(f g h)` | 🟢 |
| Hooks `(f g)` | 🟢 |
| Longer trains (5-verb and up) | 🟢 |
| Cap forks `([: g h)` | 🟢 |
| Noun forks `(n g h)` | 🟡 literal noun only |
| Verb (tacit) assignment `mean =. +/ % #` | 🟢 |
| Displaying a bare tacit-verb name (`mean` after `mean =. +/ % #`) | 🔴 named not-yet |
| `explain` facility | 🟢 |
| Adverb and conjunction assignment | 🔴 |
| Multiple assignment `'a b' =. …` | 🔴 |
| `=.` vs `=:` scoping | 🟡 one environment; the two do not yet differ |
| Explicit definitions `3 : '…'`, `4 : '…'`, `{{ }}` | 🔴 |
| Control words `if. while. for. select. try.` | 🔴 |
| Locales and `18!:` | 🔴 |
| `$:` self-reference | 🔴 |
| `x` / `y` arguments | 🔴 with explicit definitions |
| Verb rank machinery, frames, framing fill | 🟢 |
| Leading-prefix agreement | 🟢 |
| Overtake fill | 🟢 |
| Catenate with fill | 🔴 |
| Comparison tolerance (default `=`, `!.`) | 🟡 exact today; divergence recorded |
| Integer, float, `_` negative, `1e_3` exponent literals | 🟢 |
| Complex literal `1j2` | 🔴 |
| Extended literal `1x` | 🔴 |
| Rational literal `1r2` | 🔴 |
| Base and constant literals `16b1f`, `1p1` | 🔴 |
| `'strings'`, `NB.` comments, multi-sentence programs | 🟢 |
| `{name}` host-data interpolation | 🟢 |

## APL — functions

### Arithmetic and scalar

| Glyph | Monad | Dyad |
|---|---|---|
| `+` | 🟢 conjugate | 🟢 plus |
| `-` | 🟢 negate | 🟢 minus |
| `×` | 🟢 signum | 🟢 times |
| `÷` | 🟡 reciprocal; `÷0` is `∞`, not a domain error | 🟢 divide |
| `*` | 🟢 exponential | 🟢 power |
| `⍟` | 🟡 log; `⍟0` is `¯∞`, not a domain error | 🟢 logarithm |
| `⌈` | 🟢 ceiling | 🟢 maximum |
| `⌊` | 🟢 floor | 🟢 minimum |
| `\|` | 🟢 magnitude | 🟢 residue |
| `!` | 🟢 factorial | 🟢 binomial |
| `○` | 🟢 pi times | 🟡 circle; real k only |
| `?` | 🔴 roll | 🔴 deal |
| `⌹` | 🔴 matrix inverse | 🔴 matrix divide |

### Comparison and logic

| Glyph | Monad | Dyad |
|---|---|---|
| `=` | — | 🟢 equal |
| `≠` | 🔴 nub sieve | 🟢 not equal |
| `<` | — | 🟢 less than |
| `≤` | — | 🟢 less or equal |
| `>` | — | 🟢 greater than |
| `≥` | — | 🟢 greater or equal |
| `≡` | 🟢 depth | 🟢 match |
| `≢` | 🟢 tally | 🟢 not match |
| `∧` | — | 🟡 LCM/and; integral values only |
| `∨` | — | 🟡 GCD/or; integral values only |
| `⍲` | — | 🔴 nand |
| `⍱` | — | 🔴 nor |
| `~` | 🟢 not | 🔴 without |

### Structural

| Glyph | Monad | Dyad |
|---|---|---|
| `⍴` | 🟢 shape | 🟡 reshape; an empty argument is refused, not filled |
| `,` | 🟢 ravel | 🟡 catenate (last axis); unequal shapes need fill |
| `⍪` | 🟢 table | 🟡 catenate (leading axis); same |
| `⌽` | 🟢 reverse | 🟢 rotate |
| `⊖` | 🟢 reverse first | 🟡 rotate first; a vector left argument reads per axis |
| `⍉` | 🟢 transpose | 🔴 dyadic transpose |
| `↑` | 🟢 first | 🟡 take; overtaking a nested array fills with the empty box |
| `↓` | 🔴 split | 🟢 drop |
| `⊂` | 🟢 enclose | 🔴 partitioned enclose |
| `⊃` | 🟢 disclose / mix | 🔴 pick |
| `⊆` | 🔴 nest | 🔴 partition |
| `⌷` | 🔴 materialise | 🔴 index |
| `⊥` | — | 🟡 decode; folds the last axis, not the leading one |
| `⊤` | — | 🟢 encode |

### Selection, search, sort

| Glyph | Monad | Dyad |
|---|---|---|
| `⍳` | 🟡 scalar only; vector argument is a named not-yet | 🟢 index of |
| `⍸` | 🔴 where | 🔴 interval index |
| `∊` | 🟢 enlist | 🟢 membership |
| `⍷` | — | 🔴 find |
| `∪` | 🟢 unique | 🔴 union |
| `∩` | 🔴 | 🔴 intersection |
| `⍋` | 🟡 grade up; nested needs total ordering | 🔴 collating grade |
| `⍒` | 🟡 grade down; nested needs total ordering | 🔴 collating grade |

### Format, I/O, identity

| Glyph | Monad | Dyad |
|---|---|---|
| `⍕` | 🟢 format | 🔴 format by specification |
| `⍎` | 🔴 execute — sandboxed design needed | — |
| `⊢` | 🟢 same | 🟢 right |
| `⊣` | 🟢 same | 🟢 left |
| `⎕←` output | 🟢 | — |
| `⍞` character I/O | 🔴 | 🔴 |
| `→` branch | 🔴 | — |
| `⍬` zilde | 🔴 | — |
| `⌶` I-beam | 🔴 | 🔴 |

## APL — operators

| Glyph | Status |
|---|---|
| `/` reduce (last axis) | 🟢 |
| `⌿` reduce (leading axis) | 🟢 |
| `/` `⌿` replicate (after an operand) | 🟢 |
| `\` `⍀` scan | 🟢 |
| `\` `⍀` expand (after an operand) | 🔴 |
| `¨` each | 🟢 |
| `⍨` commute | 🟢 |
| `∘.` outer product | 🟢 |
| `⍤` rank | 🟡 rank specification only; `f⍤g` not yet |
| `⍣` power | 🟡 literal count; `f⍣g`, including `f⍣≡`, not yet |
| `∘` beside | 🔴 |
| `⍥` over | 🔴 |
| `⍛` before | 🔴 |
| `⍢` under | 🔴 |
| `⌸` key | 🔴 |
| `⌺` stencil | 🔴 |
| `⍠` variant | 🔴 |
| `&` spawn | 🔴 |

## APL — syntax and features

| Feature | Status |
|---|---|
| Stranding (vector notation) | 🟢 |
| Nested arrays | 🟡 structural verbs only; no mixed simple arrays |
| `←` assignment, including inline | 🟢 |
| Function assignment `F←+/` | 🔴 GNU APL rejects it; J's spelling has landed |
| Dfns `{⍵+1}`, `⍺`/`⍵` | 🔴 |
| Tradfns `∇` | 🔴 |
| Trains (forks and atops) | 🔴 |
| Bracket indexing `A[1]` | 🔴 |
| Axis specification `f[k]` | 🔴 |
| `⎕IO` as a dialect setting of the compiler | 🟢 |
| `⎕`-system names as runtime variables | 🔴 |
| Control structures `:If` … `:EndIf` | 🔴 |
| Exact-or-scalar conformability | 🟢 |
| `¯` negatives, `1E3` exponents | 🟢 |
| Complex literal `2J3` | 🔴 |
| `'strings'`, `⍝` comments, `⋄` and newline separators | 🟢 |
| `{name}` host-data interpolation | 🟢 |

## Data, boundary, runtime

| Item | Status |
|---|---|
| Boolean, i64, f64, character | 🟢 |
| Boxes | 🟢 structural verbs, display, Python conversion |
| i8/i16/i32, u8/u16/u32, f32, `Date32`, `Time32`, `Boolean` at the boundary | 🟡 widened or unpacked by one copy on entry |
| u64 | 🟡 refused above 2⁶³−1 |
| Complex | 🔴 |
| Decimal128 | 🔴 |
| Bigint, rational | 🔴 |
| float16, byte-swapped data | 🔴 |
| Arrow string, binary, list, struct, dictionary columns | 🔴 |
| Nulls | ⚪ neither language has a missing value; the column is named and refused |
| Mixed-type table columns | ⚪ silent promotion would damage values above 2⁵³ |
| Non-contiguous numpy views | ⚪ refused rather than silently copied |
| Arrow zero-copy in (i64, f64, i64-physical temporal) | 🟢 |
| DataFrame M×N → matrix, rows leading | 🟡 two or more columns are rewoven row-major, one copy |
| Zero-copy out | 🟡 rank-1 numeric only; rank ≥ 2, chars and boxes go via `.tolist()` |
| Parallel execution (own pool, `LIBJAY_THREADS`) | 🟢 |
| Expression fusion (blockwise kernels) | 🟢 |
| SIMD dispatch | 🟢 hot loops (arithmetic, reductions, fused kernels); x86-64 baseline/v2/v3 and NEON, runtime-detected; no AVX-512 rung — no stable `target_feature` name on rustc 1.85 |
| GPU / device backend | 🔴 |
| C ABI: compile, bind, execute, errors, spans | 🟢 |
| C ABI: boxed results | 🔴 no descriptor for a box yet |
| Python: `jay.j`, t-strings, samples as live defaults | 🟢 |
| Rust compile-time checking of an expression (macro) | 🔴 |
| Sandbox: stdio open, other I/O closed | 🟢 no primitive reaches the filesystem or the network |
| Differential suites against J and GNU APL | 🟢 |

Details in [coverage.md](coverage.md); the reasoning behind the choices is in
[decisions.md](decisions.md).
