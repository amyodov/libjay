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

**J: 100 green / 24 partial / 59 red of 183 valences in the inventory.**

**APL: 62 green / 21 partial / 33 red of 116 valences in the inventory.**

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
| `%:` | 🟢 sqrt; a negative gives a complex answer | 🟢 root; same |
| `<.` | 🟢 floor | 🟢 lesser of |
| `>.` | 🟢 ceiling | 🟢 larger of |
| `\|` | 🟢 magnitude | 🟢 residue |
| `<:` | 🟢 decrement | 🟢 less or equal |
| `>:` | 🟢 increment | 🟢 larger or equal |
| `+:` | 🟢 double | 🔴 not-or |
| `*:` | 🟢 square | 🔴 not-and |
| `-:` | 🟢 halve | 🟢 match |
| `-.` | 🟢 not (`1-y`) | 🔴 less |
| `+.` | 🟢 real/imaginary | 🟡 GCD; integral reals, or Gaussian integers |
| `*.` | 🟢 length/angle | 🟡 LCM; integral reals, or Gaussian integers |
| `!` | 🟡 factorial; a complex argument is a named gap | 🟡 out of; same |
| `o.` | 🟢 pi times | 🟢 circle; `_12` to `12`, real and complex |
| `%.` | 🟢 matrix inverse (Householder QR, f64) | 🟡 matrix divide; a right-hand side of rank 3 or more is refused |
| `j.` | 🟢 imaginary | 🟢 complex |
| `r.` | 🟢 angle | 🟢 polar |
| `p.` | 🔴 roots | 🔴 polynomial |
| `p..` | 🔴 poly. derivative | 🔴 poly. integral |
| `p:` | 🟢 the y-th prime | 🔴 primality and the factorisation table |
| `q:` | 🟢 prime factors | 🔴 prime exponents |
| `?` | 🟡 roll; libjay's own stream, not J's | 🟡 deal; same |
| `?.` | 🟡 roll, fixed seed; libjay's own stream | 🟡 deal, fixed seed; same |
| `x:` | 🟢 extend precision | 🟡 to rational; forms 1, 2, `_1`, `_2` |

### Comparison and logic

| Spelling | Monad | Dyad |
|---|---|---|
| `=` | 🔴 self-classify | 🟢 equal |
| `<` | 🟢 box | 🟢 less than |
| `>` | 🟢 open | 🟢 larger than |
| `~:` | 🔴 nub sieve | 🟢 not-equal |
| `~.` | 🟢 nub | — |

`-:` (halve / match) is in the arithmetic table. Comparison carries J's
default tolerance (2⁻⁴⁴); `!.` sets it per verb.

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
| `{::` | 🔴 map | 🟢 fetch |
| `i.` | 🟢 integers | 🟢 index of |
| `i:` | 🟢 steps | 🟢 index of last |
| `I.` | 🟢 indices | 🟢 interval index |
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
| `/.` | 🟢 oblique | 🟢 key |
| `~` | 🟢 reflex | 🟢 passive |
| `}` | 🟡 noun operand (`m} y` selects) | 🟡 noun operand; `u}` not yet |

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
| `^:` power | 🟡 literal count, `_`, or a verb count; negatives (the obverse) not yet |
| `.` dot product | 🔴 |
| `:` explicit definition | 🟡 `3 :` and `4 :`; `1 :`, `2 :`, `13 :` not yet |
| `;.` cut | 🟡 frets (`;.1` `;._1` `;.2` `;._2`) and `;.0`; `;.3` not yet |
| `!.` fit (tolerance) | 🟡 the tolerance meaning; `!.` as a fill not yet |
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
| `=.` vs `=:` scoping | 🟢 a definition has its own frame; `=:` names a global |
| Explicit definitions `3 : '…'`, `4 : '…'`, `{{ }}` | 🟢 verbs; `{{ }}` modifier forms named |
| Multi-line definition body `3 : 0` … `)` | 🟢 |
| Control words `if. while. for. select. try.` | 🟢 `whilst.`, `for_i.`, `fcase.`, `elseif.` included |
| Control words `throw. catcht. goto_x. label_x.` | 🔴 named |
| Locales and `18!:` | 🔴 |
| `$:` self-reference | 🟡 names the definition it stands in; the oracle self-applies |
| Recursion by name inside a definition | 🟢 bounded, with a diagnostic |
| `x` / `y` arguments | 🟢 |
| Verb rank machinery, frames, framing fill | 🟢 |
| Leading-prefix agreement | 🟢 |
| Overtake fill | 🟢 |
| Catenate with fill | 🔴 |
| Comparison tolerance (default `=`, `!.`) | 🟢 2⁻⁴⁴, and `u!.n` |
| Integer, float, `_` negative, `1e_3` exponent literals | 🟢 |
| Complex literals `1j2`, `1ad45`, `1ar1` | 🟢 |
| Extended literal `1x` | 🟢 |
| Rational literal `1r2` | 🟢 |
| Base and constant literals `16b1f`, `1p1`, `1x1` | 🟢 |
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
| `!` | 🟡 factorial; a complex argument is a named gap | 🟡 binomial; same |
| `○` | 🟢 pi times | 🟢 circle; `¯12` to `12`, real and complex |
| `?` | 🟡 roll; libjay's own stream, not GNU APL's | 🟡 deal; same |
| `⌹` | 🟢 matrix inverse (Householder QR, f64) | 🟡 matrix divide; a right-hand side of rank 3 or more is refused |

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
| `⊂` | 🟢 enclose | 🟡 partitioned enclose; a vector argument only |
| `⊃` | 🟢 disclose / mix | 🔴 pick |
| `⊆` | 🔴 no oracle: not in GNU APL's character set | 🔴 same |
| `⌷` | 🔴 materialise — GNU APL has no monad either | 🟢 index (APL2: one scalar per axis) |
| `⊥` | — | 🟡 decode; folds the last axis, not the leading one |
| `⊤` | — | 🟢 encode |

### Selection, search, sort

| Glyph | Monad | Dyad |
|---|---|---|
| `⍳` | 🟡 scalar only; vector argument is a named not-yet | 🟢 index of |
| `⍸` | 🟢 where | 🟢 interval index |
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
| `→` branch | 🔴 named (label-based goto) | — |
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
| `⍤` rank | 🟡 rank specification only; `f⍤g` is not in GNU APL either |
| `⍣` power | 🟡 literal count or a function operand (`f⍣≡`); negatives not yet |
| `∘` beside | 🔴 no oracle: GNU APL has no `∘` operator |
| `⍥` over | 🔴 no oracle: not in GNU APL's character set |
| `⍛` before | 🔴 no oracle: GNU APL rejects it |
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
| Dfns `{⍵+1}`, `⍺`/`⍵`, `⋄` bodies, nesting | 🟢 |
| Dfn assignment `F←{⍵×2}` | 🟢 |
| Dfn guards `cond:expr`, `⍺←default`, `∇` self-reference | 🟡 no oracle: GNU APL has none of the three |
| Dfn operators `⍺⍺` / `⍵⍵` | 🔴 named |
| Tradfns `∇ Z←L F R;locals` … `∇` | 🟢 including APL's global-by-default scope rule |
| Trains (forks and atops) | 🔴 |
| Bracket indexing `A[1]` | 🟢 reading and writing, elided slots included |
| Indexed assignment `A[i]←v`, `A[i;j]←v` | 🟢 copy-on-write on the named value |
| Axis specification `f[k]` | 🟡 `/` `⌿` `\` `⍀` `⌽` `⊖`; the rest named |
| `⎕IO` as a dialect setting of the compiler | 🟢 |
| `⎕`-system names as runtime variables | 🔴 |
| Control structures `:If :While :Repeat :For :Select` | 🟡 no oracle: GNU APL rejects them |
| `:Return` `:Leave` `:Continue` | 🟡 no oracle, as above |
| Exact-or-scalar conformability | 🟢 |
| `¯` negatives, `1E3` exponents | 🟢 |
| Complex literal `2J3` | 🟢 |
| `'strings'`, `⍝` comments, `⋄` and newline separators | 🟢 |
| `{name}` host-data interpolation | 🟢 |

## Data, boundary, runtime

| Item | Status |
|---|---|
| Boolean, i64, f64, character | 🟢 |
| Boxes | 🟢 structural verbs, display, Python conversion |
| i8/i16/i32, u8/u16/u32, f32, `Date32`, `Time32`, `Boolean` at the boundary | 🟡 widened or unpacked by one copy on entry |
| u64 | 🟡 refused above 2⁶³−1 |
| Complex | 🟢 core type, `[re, im]` pairs; numpy `complex128` zero-copy, Arrow `struct<re, im>` |
| Extended integer, rational | 🟢 core types, heap-backed; exact arithmetic, Python `int` and `fractions.Fraction` at the boundary |
| Decimal128 | 🔴 |
| float16, byte-swapped data | 🔴 |
| Arrow string, binary, list, dictionary columns | 🔴 |
| Nulls | ⚪ neither language has a missing value; the column is named and refused |
| Mixed-type table columns | ⚪ silent promotion would damage values above 2⁵³ |
| Non-contiguous numpy views | ⚪ refused rather than silently copied |
| Arrow zero-copy in (i64, f64, i64-physical temporal) | 🟢 |
| DataFrame M×N → matrix, rows leading | 🟡 two or more columns are rewoven row-major, one copy |
| Zero-copy out | 🟡 rank-1 machine-numeric only; rank ≥ 2, chars, boxes and the exact types go via `.tolist()` |
| Arrow carrier for the exact types | 🔴 Arrow has none; `.tolist()` gives exact Python objects, `_1 x:` machine numbers |
| Parallel execution (own pool, `LIBJAY_THREADS`) | 🟢 |
| Expression fusion (blockwise kernels) | 🟢 |
| SIMD dispatch | 🟢 hot loops (arithmetic, reductions, fused kernels); x86-64 baseline/v2/v3 and NEON, runtime-detected; no AVX-512 rung — no stable `target_feature` name on rustc 1.85 |
| GPU / device backend | 🟡 fused kernels only, via wgpu (Metal/Vulkan/DX12), compiled into the one artifact and dormant without an adapter. f64 needs `SHADER_F64`, which Metal has not; on such an adapter an f64 chain stays on the CPU unless the caller asks for `precision="f32"`. Integer chains, non-float results and `^` in f64 stay on the CPU. The f64 path is generated and validated but has not been executed anywhere yet — see [decisions.md](decisions.md) |
| Device placement API (`deploy`, `upload`, `DeviceArray`) | 🟡 `jay.j(...)` has no device by design; a result kept on the device is still materialised on the host once |
| C ABI: compile, bind, execute, errors, spans | 🟢 |
| C ABI: complex (`JAY_COMPLEX`, interleaved doubles) | 🟢 |
| C ABI: boxed results | 🔴 no descriptor for a box yet |
| C ABI: extended and rational results | 🔴 no descriptor for a bignum yet; `_1 x:` converts |
| Python: `jay.j`, t-strings, samples as live defaults | 🟢 |
| Rust compile-time checking of an expression (macro) | 🔴 |
| Sandbox: stdio open, other I/O closed | 🟢 no primitive reaches the filesystem or the network |
| Differential suites against J and GNU APL | 🟢 |

Details in [coverage.md](coverage.md); the reasoning behind the choices is in
[decisions.md](decisions.md).
