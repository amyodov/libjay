# Language coverage status

One row per spelling, one circle per valence. The inventory is the whole
published vocabulary of each language, not the part libjay reaches today.

| | Meaning |
|---|---|
| 🟢 | implemented — differential-tested wherever an oracle covers it |
| 🟡 | partial: works, with the caveat named in the row |
| 🔴 | not yet — a promise, and the compiler says so by name |
| ⚪ | absent here by design (nulls, threads, and other things libjay refuses to guess at) |
| — | the language gives this spelling no meaning in that valence |

Nothing is permanently excluded. A 🔴 is a queue position; a ⚪ is a
deliberate refusal to invent data or behaviour, not a closed door. The
⚪ rows a reader is most likely to look for: `⌶` is implementation-defined
— what an I-beam does is the interpreter's own business, and libjay will
not invent a contract for a spelling that has no published one — while `&`
(APL) and `T.`/`t.` (J) start the language's own threads, which the sandbox
does not open.

Counts below cover the primitive tables (verbs, adverbs, conjunctions,
nouns), one count per valence the language defines; the syntax/feature
tables are listed separately and not counted.

**J: 151 green / 24 partial / 2 absent by design, of 177 valences in the
inventory. No row in J's primitive tables is red.**

**APL: 93 green / 21 partial / 3 absent by design, of 117 valences in the
inventory. Nothing in APL's primitive tables is red.**

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
| `\|` | 🟢 magnitude | 🟢 residue; the quotient is rounded with the tolerance |
| `<:` | 🟢 decrement | 🟢 less or equal |
| `>:` | 🟢 increment | 🟢 larger or equal |
| `+:` | 🟢 double | 🟢 nor; both arguments must be 0 or 1 |
| `*:` | 🟢 square | 🟢 nand; same |
| `-:` | 🟢 halve | 🟢 match |
| `-.` | 🟢 not (`1-y`) | 🟢 less (the items of x that y has not) |
| `+.` | 🟢 real/imaginary | 🟢 GCD (reals and Gaussian integers) |
| `*.` | 🟢 length/angle | 🟢 LCM (reals and Gaussian integers) |
| `!` | 🟡 factorial; a complex argument is a named gap | 🟡 out of; same |
| `o.` | 🟢 pi times | 🟢 circle; `_12` to `12`, real and complex |
| `%.` | 🟢 matrix inverse (Householder QR, f64) | 🟡 matrix divide; a right-hand side of rank 3 or more is refused |
| `j.` | 🟢 imaginary | 🟢 complex |
| `r.` | 🟢 angle | 🟢 polar |
| `p.` | 🟡 roots, by Durand–Kerner in f64, with a repeated one refined through its m-1st derivative; jconsole answers an exact rational for some quadratics and libjay a float. A boxed argument is the root form and answers the coefficients; the multiplier may go unsaid, so `p. (<1 2)` is `2 _3 1` | 🟢 polynomial; a boxed `multiplier ; roots` left argument too, the multiplier optional |
| `p..` | 🟢 poly. derivative; a boxed argument is the root form | 🟡 poly. integral, x the constant term; a boxed argument is the root form, but its coefficients come out in floats where jconsole keeps exact rationals |
| `p:` | 🟢 the y-th prime; extended where y is | 🟢 the prime queries: `_1` `0` `1` `2` `3` `4` `_4` |
| `q:` | 🟢 prime factors; extended where y is | 🟡 prime exponents; `x>0` and `__`, the negative forms named |
| `?` | 🟡 roll; libjay's own stream, not J's | 🟡 deal; same |
| `?.` | 🟡 roll, fixed seed; libjay's own stream | 🟡 deal, fixed seed; same |
| `x:` | 🟢 extend precision | 🟡 to rational; forms 1, 2, `_1`, `_2` |

### Comparison and logic

| Spelling | Monad | Dyad |
|---|---|---|
| `=` | 🟢 self-classify | 🟢 equal |
| `<` | 🟢 box | 🟢 less than |
| `>` | 🟢 open | 🟢 larger than |
| `~:` | 🟢 nub sieve | 🟢 not-equal |
| `~.` | 🟢 nub | — |

`-:` (halve / match) is in the arithmetic table. Comparison carries J's
default tolerance (2⁻⁴⁴); `!.` sets it per verb.

### Structural

| Spelling | Monad | Dyad |
|---|---|---|
| `$` | 🟢 shape of; extended where the argument is | 🟢 reshape, laying out ITEMS |
| `#` | 🟢 tally; extended where the argument is | 🟢 copy; `#^:_1` is the expansion that undoes it |
| `,` | 🟢 ravel | 🟢 append; unequal item shapes are overtaken, which fills, and a rank gap of any width makes the lower-ranked side one item. An operand with no elements takes the other side's type rather than clashing with it |
| `,.` | 🟢 ravel items; never below rank 2, so `$ ,. 5` is `1 1` | 🟢 stitch |
| `,:` | 🟢 itemize | 🟢 laminate |
| `\|.` | 🟢 reverse | 🟢 rotate |
| `\|:` | 🟢 transpose | 🟢 dyadic transpose; the named axes move to the end, a boxed x groups them into a diagonal |
| `{.` | 🟢 head | 🟢 take |
| `}.` | 🟢 behead | 🟢 drop |
| `{:` | 🟢 tail | — |
| `}:` | 🟢 curtail | — |
| `#.` | 🟢 base 2 | 🟢 base; extended where an argument is; an ATOM of digits spreads over the radices (`1 2 3 #. 5` is 50), a one-item list does not |
| `#:` | 🟢 antibase 2 | 🟢 antibase; extended where an argument is |

### Selection, search, sort

| Spelling | Monad | Dyad |
|---|---|---|
| `{` | 🟢 catalogue | 🟢 from; atom indices and boxed index specifications, complements included |
| `{::` | 🟢 map | 🟢 fetch |
| `i.` | 🟢 integers | 🟢 index of |
| `i:` | 🟢 steps | 🟢 index of last |
| `I.` | 🟢 indices | 🟢 interval index; boxed bounds are ordered by the total array ordering |
| `e.` | 🟢 raze in | 🟢 member of |
| `E.` | — | 🟢 member of interval; two atoms are read as one-item lists |
| `/:` | 🟢 grade up; boxes by the total array ordering | 🟢 sort; the grade indexes x's ITEMS, so an atom answers only the first |
| `\:` | 🟢 grade down; boxes by the total array ordering | 🟢 sort; same |
| `A.` | 🟢 anagram index | 🟢 anagram |
| `C.` | 🟢 cycle-direct; a short direct permutation is the abbreviated one | 🟢 permute; direct, abbreviated, an atom, or cyclic — a cycle's element counts back from the end where it is negative, and every one is checked before the permutation is built |

### Boxes, format, system

| Spelling | Monad | Dyad |
|---|---|---|
| `;` | 🟢 raze | 🟢 link |
| `;:` | 🟢 words | 🟡 sequential machine: the table-driven form `(f;s;m;ijrd) ;: y`, with output codes 0 to 3 and 6 and every result form 0 to 5; the two vector codes (4 and 5) and a map over a numeric argument are named |
| `L.` | 🟢 level of | — |
| `":` | 🟢 default format | 🟢 format by specification: `w j d` per column, width 0 for what the values need, a negative width for the exponential form, asterisks for what does not fit |
| `".` | 🟢 do; the string runs over the names around it | 🟢 numbers, x standing in for a word that is not one |
| `u:` | 🟢 unicode | 🟡 unicode; forms 3 and 10, the byte-oriented ones named |
| `s:` | 🟢 symbol: a character list cut on its own leading delimiter, a character table one name per row, a boxed argument one name per box | 🟡 the name forms `4 s:` (a blank-padded character table) and `5 s:` (boxes); the symbol-table queries `0 s:` … `3 s:`, `6 s:`, `7 s:` and `_1 s:` report an interpreter's internal table and are named |
| `[` | 🟢 same | 🟢 left |
| `]` | 🟢 same | 🟢 right |
| `echo` | 🟢 print | — |

### Nouns and constant verbs

| Spelling | Status |
|---|---|
| `a.` alphabet | 🟢 |
| `a:` ace (boxed empty) | 🟢 |
| `_` `__` infinities | 🟢 |
| `_.` indeterminate | 🟢 a NaN, which prints as itself |
| `_9:` … `9:`, `_:` constant verbs | 🟢 |

## J — adverbs

| Spelling | Monad | Dyad |
|---|---|---|
| `/` | 🟢 insert | 🟢 table |
| `\` | 🟢 prefix | 🟢 infix |
| `\.` | 🟢 suffix | 🟢 outfix; a piece of one item applies nothing, and only the five folds J has special code for (`+/` `*/` `<./` `>./` `+./`) type the whole argument first |
| `/.` | 🟢 oblique | 🟢 key |
| `~` | 🟢 reflex | 🟢 passive |
| `}` | 🟢 noun or verb operand; a boxed index specification too | 🟢 the same |

## J — conjunctions

| Spelling | Status |
|---|---|
| `"` rank | 🟡 noun ranks; `u"v` not yet |
| `@` atop | 🟢 |
| `@:` at | 🟢 |
| `&` bond / compose | 🟡 verbs compose, literal nouns bond; computed nouns not yet |
| `&:` appose | 🟢 |
| `&.` under | 🟢 `v^:_1 @: u &: v`, over the obverse table — docs/coverage.md's "The obverse table" lists what is in it and the two verbs still named. `&.>` and `&.,` are the two unders not built out of an inverse: box by box, and over the ravel with the shape put back |
| `&.:` under | 🟢 the same on whole arguments, over the same table |
| `^:` power | 🟡 a literal count, a list of them, `_`, the traces `u^:(<n)` and `u^:a:`, a verb count, and negatives (the obverse); a computed count not yet |
| `.` dot product | 🟡 both valences: `x u . v y` at every rank, with `+/ . *` (the matrix product) a blocked parallel pass over the two buffers; the monad is the determinant by minors down the first column, and `-/ . *` over machine numbers goes by elimination instead. A determinant by minors of more than 16 rows is named — the expansion is exponential |
| `:` explicit definition | 🟡 `1 :`, `2 :`, `3 :`, `4 :`, and the `m : 0` body on the lines below; `13 :` not yet |
| `;.` cut | 🟡 frets (`;.1` `;._1` `;.2` `;._2`), the rectangle `;.0` in both valences, and the tessellations `;.3` `;._3`, negative block sizes included where the movement row is written out; a negative size with the movement left implicit is named. An empty fret list is the whole argument in one piece; a BOXED left argument — J's per-axis frets — is named |
| `!.` fit | 🟡 the tolerance meaning, and the fill for `\|.` (the shift); a fill on any other verb is named |
| `!:` foreign | 🟡 `1!:1` (read a line from stdin), `1!:2` (write a line to stdout), `3!:0` (type code) and `5!:1` (the atomic representation of a name); the ones that reach a file, a script, the host, the clock or a shared library are ⚪ closed by the sandbox, and the ones that only compute are 🔴 named |
| `` ` `` tie (gerund) | 🟡 the gerund is boxed data — one atomic representation per box, so it can be named, computed and displayed; a verb the representation cannot spell (a capped fork, an explicit definition) is named |
| `` `: `` evoke gerund | 🟢 the three forms J gives it: `0` applies each verb, `3` inserts them, `6` is the train |
| `@.` agenda | 🟢 |
| `[:` cap | 🟢 |
| `::` adverse | 🟢 |
| `:.` obverse | 🟢 declares what undoes a verb |

The rest of the modifiers, which the vocabulary lists apart from the
conjunctions above:

| Spelling | Status |
|---|---|
| `f.` fix | 🟢 names are substituted where used, so a fixed verb is the verb |
| `M.` memo | 🟢 the cache belongs to the derived verb and lives as long as the program |
| `L:` level | 🟢 both valences; the dyad descends both arguments together |
| `S:` spread | 🟢 both valences, as `L:` |
| `b.` boolean / characteristics | 🟢 `m b.` (the 32 boolean and bitwise functions) and all three characteristics: `0` the ranks, `1` the identity function, `_1` the obverse. `_1` answers a SPELLING, and libjay writes its own — a derived verb whose operand is a noun spells as `(n&+)` here where the reference writes the noun out |
| `$.` sparse | 🟡 the storage kind, the monad and the dyad's forms `_1 0 1 2 3 4 5 7 8`, and the display. A sparse array reaching any other verb is expanded first, so the ANSWER always matches and the storage kind does not survive the step — where J keeps `s + 1` sparse, libjay gives the dense array. Sparse characters and boxes are named gaps, as they are in J |
| `H.` hypergeometric | 🟢 the series, with the shared parameters cancelled; a series that neither converges nor overflows is refused by name |
| `t.` task | ⚪ the reference spells `t.` the TASK conjunction — it runs a verb in one of J's thread pools and answers with a pyx. The sandbox does not open those threads, as it does not for `T.` |
| `t:` | — the reference rejects the spelling as an invalid inflection, as it does `d.`, `D.` and `D:` |
| `..` even | — the reference rejects the spelling as an invalid inflection |
| `.:` odd | — the same |
| `T.` threads | ⚪ starts J's own threads, which the sandbox does not open — `ErrorKind::Sandbox` |
| `d.` `D.` `D:` derivative | — the reference J rejects all three as invalid inflections |

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
| Displaying a bare modifier name (`m` after `m =. /`) | 🔴 named not-yet |
| `explain` facility | 🟢 |
| Adverb and conjunction assignment `m =. /` | 🟢 the name is that modifier from the next sentence on |
| Multiple assignment `'a b' =. …` | 🔴 |
| `=.` vs `=:` scoping | 🟢 a definition has its own frame; `=:` names a global |
| Explicit definitions `3 : '…'`, `4 : '…'`, `{{ }}` | 🟢 |
| Multi-line definition body `3 : 0` … `)` | 🟢 |
| Explicit adverb `1 : '…'` and conjunction `2 : '…'` | 🟢 both phases; operands as `u`/`v` or `m`/`n` |
| Multi-line modifier body `1 : 0` / `2 : 0` … `)` | 🟢 |
| `{{ }}` modifier forms | 🟢 the part of speech is read off the operand names the body uses |
| `{{)a` `{{)c` `{{)v` `{{)d` `{{)m` markers | 🟢 the marker line states the part of speech |
| `{{)n` noun direct definition | 🔴 named |
| A modifier body that derives the modifier itself | 🔴 named; the derivation happens at parse time |
| Tacit definition `13 : '…'` | 🔴 named |
| Control words `if. while. for. select. try.` | 🟢 `whilst.`, `for_i.`, `fcase.`, `elseif.` included |
| Control words `throw. catcht. goto_x. label_x.` | 🔴 named |
| Locales and `18!:` | 🔴 |
| `$:` self-reference | 🟡 names the definition it stands in; the oracle self-applies |
| Recursion by name inside a definition | 🟢 bounded, with a diagnostic |
| `x` / `y` arguments | 🟢 |
| Verb rank machinery, frames, framing fill | 🟢 |
| Leading-prefix agreement | 🟢 |
| Overtake fill | 🟢 |
| Catenate with fill | 🟢 |
| Comparison tolerance (default `=`, `!.`) | 🟢 2⁻⁴⁴, and `u!.n` |
| Integer, float, `_` negative, `1e_3` exponent literals | 🟢 |
| Complex literals `1j2`, `1ad45`, `1ar1` | 🟢 |
| Extended literal `1x` | 🟢 |
| Rational literal `1r2` | 🟢 |
| Base and constant literals `16b1f`, `1p1`, `1x1` | 🟢 |
| `'strings'`, `NB.` comments, multi-sentence programs | 🟢 |
| `{name}` host-data interpolation | 🟢 |
| An EMPTY where numeric data is wanted (`#. ''`, `i. ''`, `¯3⊥''`) | 🟡 an empty of characters or symbols is accepted, as both references accept it; an empty of BOXES is refused, which is what jconsole does with `2 #. 0$<1` and not what it does with `#. 0$<1` |

## APL — functions

### Arithmetic and scalar

| Glyph | Monad | Dyad |
|---|---|---|
| `+` | 🟢 conjugate | 🟢 plus |
| `-` | 🟢 negate | 🟢 minus |
| `×` | 🟢 signum | 🟢 times |
| `÷` | 🟢 reciprocal; `÷0` is a domain error, as the dyad is | 🟢 divide |
| `*` | 🟢 exponential | 🟢 power |
| `⍟` | 🟢 log; `⍟0` is a domain error | 🟢 logarithm |
| `⌈` | 🟢 ceiling | 🟢 maximum |
| `⌊` | 🟢 floor | 🟢 minimum |
| `\|` | 🟢 magnitude | 🟢 residue; the quotient is rounded with the tolerance |
| `!` | 🟡 factorial; a complex argument is a named gap | 🟡 binomial; same |
| `○` | 🟢 pi times | 🟢 circle; `¯12` to `12`, real and complex |
| `?` | 🟡 roll; libjay's own stream, not GNU APL's | 🟡 deal; same |
| `⌹` | 🟢 matrix inverse (Householder QR, f64) | 🟡 matrix divide; a right-hand side of rank 3 or more is refused |

### Comparison and logic

| Glyph | Monad | Dyad |
|---|---|---|
| `=` | — | 🟢 equal |
| `≠` | 🟢 nub sieve | 🟢 not equal |
| `<` | — | 🟢 less than |
| `≤` | — | 🟢 less or equal |
| `>` | — | 🟢 greater than |
| `≥` | — | 🟢 greater or equal |
| `≡` | 🟢 depth | 🟢 match |
| `≢` | 🟢 tally | 🟢 not match |
| `∧` | — | 🟢 LCM / and |
| `∨` | — | 🟢 GCD / or; GNU APL's rounding and zero-sign rules (`gcd_rule`) |
| `⍲` | — | 🟢 nand |
| `⍱` | — | 🟢 nor |
| `~` | 🟢 not | 🟢 without |

### Structural

| Glyph | Monad | Dyad |
|---|---|---|
| `⍴` | 🟢 shape | 🟢 reshape, laying out elements; an empty argument fills |
| `,` | 🟢 ravel | 🟢 catenate (last axis); a simple side joined to a nested one has its items enclosed, and two that share no type make a mixed simple array |
| `⍪` | 🟢 table | 🟢 catenate (leading axis); same |
| `⌽` | 🟢 reverse | 🟢 rotate |
| `⊖` | 🟢 reverse first | 🟢 rotate first; one amount per column, as `⌽` takes one per row |
| `⍉` | 🟢 transpose | 🟢 dyadic transpose; x says which axis of the result each axis of y becomes, and a repeated destination runs those axes together |
| `↑` | 🟢 first; an empty nested argument answers the prototype it remembers | 🟢 take; overtaking a nested array fills with the first item's prototype |
| `↓` | 🟡 no oracle: GNU APL has no monadic `↓`; Dyalog's split | 🟢 drop |
| `⊂` | 🟢 enclose | 🟢 partitioned enclose; rank 2 and above partitions the last axis; a single flag extends over every item, so `1⊂1 2 3` is one partition, and no flag against no item is the empty nested vector |
| `⊃` | 🟢 disclose / mix | 🟢 pick |
| `⊆` | 🟡 no oracle: not in GNU APL's character set; Dyalog's nest | 🟡 no oracle; Dyalog's partition, which is GNU APL's dyadic `⊂` |
| `⌷` | 🟡 no oracle: materialise, which Dyalog makes the identity | 🟢 index (APL2: one item of x per axis, a scalar or an enclosed vector) |
| `⊥` | — | 🟢 decode; the inner product `+.×` over x's last axis and y's leading one; a SINGLE on either side extends along the other's axis, and an empty axis weighs nothing — with no digit to weigh the radix is never read, so `'a'⊥(0⍴0)` is 0 |
| `⊤` | — | 🟢 encode; with no value to write the radix is never read, so `'a'⊤(0⍴0)` is the empty |

### Selection, search, sort

| Glyph | Monad | Dyad |
|---|---|---|
| `⍳` | 🟢 index generator; a shape of two lengths or more gives the nested array of coordinate vectors | 🟢 index of; the items of a left argument of any rank are searched, and `lookup_left` names Dyalog's vector-only reading |
| `⍸` | 🟢 where | 🟢 interval index |
| `∊` | 🟢 enlist | 🟢 membership |
| `⍷` | — | 🟢 find; reads a mixed simple array element for element |
| `∪` | 🟢 unique | 🟢 union; builds a mixed simple array where the two share no type |
| `∩` | — | 🟢 intersection; reads a mixed simple array element for element |
| `⍋` | 🟢 grade up; compares under `⎕CT`; nested by the APL2 rule (`nested_grade`) | 🟢 collating grade |
| `⍒` | 🟢 grade down; compares under `⎕CT`; nested by the APL2 rule (`nested_grade`) | 🟢 collating grade |

### Format, I/O, identity

| Glyph | Monad | Dyad |
|---|---|---|
| `⍕` | 🟢 format | 🟡 format by specification: width and precision pairs; a nested argument is named |
| `⍎` | 🟡 execute; the string runs over the names around it. An EMPTY program yields no value at all in GNU APL, and a libjay verb has no way to answer that, so it refuses and says so | — |
| `⊢` | 🟢 same | 🟢 right |
| `⊣` | 🟢 same | 🟢 left |
| `⎕←` / `⍞←` output | 🟢 `⎕←` ends the line; `⍞←` writes the characters and nothing else | — |
| `⍞` character input | 🟢 one line from the input source, terminator dropped | — |
| `⎕` evaluated input | 🟢 one line, run as APL over the program's own names | — |
| `→` branch | 🟡 inside a `∇` definition: labels, `→0`, `→(cond)/L`, `→⍬`; a label and a control structure in one definition is named | — |
| `⍬` zilde | 🟢 | — |
| `⌶` I-beam | ⚪ implementation-defined | ⚪ the same |

## APL — operators

| Glyph | Status |
|---|---|
| `/` reduce (last axis) | 🟢 between the ELEMENTS along the axis, each disclosed and the fold's value enclosed: `,/1 2 3` is an enclosed vector |
| `⌿` reduce (leading axis) | 🟢 the same rule down the first axis: `,⌿2 3⍴⍳6` pairs the columns |
| `/` `⌿` n-wise reduction (dyadic) | 🟢 `n f/ y` folds every window of n items along the axis the glyph chooses: `2+/1 2 3` is `3 5`. n is one number — a negative one reverses each window, zero answers the operand's identity once per gap, and a window may reach one item past the axis before it is a domain error. A positive window shares the blockwise kernel with J's `n u/\ y`. The shape of an EMPTY argument of rank ≥2 diverges from GNU APL — see divergences |
| `/` `⌿` replicate (after an operand) | 🟢 a negative count leaves that many prototype fills |
| `\` `⍀` scan | 🟢 the reduce over each prefix, so it collects the same enclosures |
| `\` `⍀` expand (after an operand) | 🟢 the gap holds the first item's prototype |
| `¨` each | 🟢 |
| `⍨` commute | 🟢 |
| `∘.` outer product | 🟢 the function between every pair of ELEMENTS whatever its rank, each disclosed on the way in and the result enclosed on the way into the table: `1 2∘.,3 4` is a two-by-two of pairs, `¯1 0 1∘.⌽⊂m` rotates the matrix |
| `⍤` rank / atop | 🟡 a rank specification, or Dyalog's atop with a function operand; no oracle for the latter |
| `⍣` power | 🟡 literal count — negatives included, answered from the obverse table — or a function operand (`f⍣≡`); a count computed at run time is named. No oracle for the negatives: GNU APL implements no negative power at all, so they are recorded against Dyalog in `corpus/apl/dyalog-operators.txt` |
| `∘` beside | 🟡 no oracle: GNU APL has no `∘` operator; Dyalog's `f∘g`, function operands only |
| `⍥` over | 🟡 no oracle: not in GNU APL's character set; Dyalog's `f⍥g` |
| `⍛` before | 🟡 no oracle: GNU APL rejects it; Dyalog's `f⍛g` |
| `⍢` under | 🟡 no oracle, and no reference either: GNU APL rejects the glyph and Dyalog 20.0 answers `SYNTAX ERROR: Invalid token` (recorded in `corpus/apl/dyalog-operators.txt`), so ours is an extension — `g⍣¯1 ⊢ (g x) f (g y)`, over the same obverse table J's `&.:` uses |
| `⌸` key | 🟡 no oracle: GNU APL rejects it; Dyalog's, with the operand taking the key and its group |
| `⌺` stencil | 🟡 no oracle: GNU APL rejects it; Dyalog's monadic-window form — one size per leading axis, the windows centred and the edges filled. The two-row form that also gives the movement is named |
| `.` inner product | 🟢 `f.g` is `f/¨` over the outer product: `+.×` is the matrix product, `∧.=` asks which rows match. Each vector along x's last axis meets each vector along y's first under g, f folds the result, and the each encloses it. `Dialect.inner_each` names Dyalog's reading, which puts the each on the pairing instead — g meets one element from each side and the fold's own value is the cell |
| `⍠` variant | 🟡 one dialect setting overridden for one application: `⎕CT` (the principal option, so a bare number sets it) and `⎕IO`, as literal options — `=⍠0`, `⍳⍠('IO' 0)`. Another option, or one that is not settled when the program is compiled, is named. No oracle: GNU APL rejects the glyph, and Dyalog takes a variant only on its search-and-replace family — both `=⍠0` and `⍳⍠('IO' 0)` are a DOMAIN ERROR there, recorded in `corpus/apl/dyalog-operators.txt` |
| `&` spawn | ⚪ starts an APL thread, which the sandbox does not open — as J's `T.` and `t.` do not |

## APL — syntax and features

| Feature | Status |
|---|---|
| Stranding (vector notation) | 🟢 |
| Nested arrays | 🟡 structural verbs, mixed simple arrays (built by `,` `⍪` `∪` `∩` `~` `⍷` `∊` `⍳` `≡`, and printed with a run of characters as text) and prototype fills; the operators apply between items, so `∘.`, `/`, `⌿`, `\`, `⍀` and `.` disclose what they take and enclose what they collect. The arithmetic still refuses a boxed operand |
| `←` assignment, including inline | 🟢 |
| Function assignment `F←+/`, `F←+/÷≢` | 🟡 no oracle: GNU APL rejects it; the same extension as trains, and off with it |
| Dfns `{⍵+1}`, `⍺`/`⍵`, `⋄` bodies, nesting | 🟢 |
| Dfn assignment `F←{⍵×2}` | 🟢 |
| Dfn guards `cond:expr`, `⍺←default`, `∇` self-reference | 🟡 guards and `∇` have no oracle (absent from GNU APL); `⍺←` follows the published default-only rule where GNU APL assigns unconditionally (recorded divergence) |
| Dfn operators `⍺⍺` / `⍵⍵` | 🟡 no oracle: GNU APL has neither; a dfn naming one is an operator, and naming the operator keeps it one |
| Tradfns `∇ Z←L F R;locals` … `∇` | 🟢 including APL's global-by-default scope rule, and the niladic form, which naming calls |
| Trains (forks and atops) | 🟡 no oracle: GNU APL rejects them; Dyalog's rules, shipped as an extension (`Dialect.trains`, on by default) — 2-train atop, 3-train fork, a value left tine, longer trains grouped from the right |
| Bracket indexing `A[1]` | 🟢 reading and writing, elided slots included |
| Indexed assignment `A[i]←v`, `A[i;j]←v` | 🟢 copy-on-write on the named value |
| Axis specification `f[k]` | 🟡 `/` `⌿` `\` `⍀` `⌽` `⊖`; the rest named |
| `⎕IO` as a dialect setting of the compiler | 🟢 |
| Dialect object (`⎕IO`, `⎕CT`, the lineage settings) | 🟢 two presets — `Dialect::gnu_apl()`, the APL2/ISO line plus the extensions, which is the default, and `Dialect::dyalog()` (`APL.Dialect.dyalog` in Python). Every point where the lineages diverge is a setting on it: `⎕CT`, `↑`/`⊃`, `⌷`, dyadic `⊂`, `≡`'s sign, the dfn result, the nested grade, dyadic `⍳`'s left rank and trains are all implemented in both readings; the nested model, `⍺←`'s laziness and the complex order are implemented in one, and asking for the other is refused as not implemented yet |
| `⎕`-system names as runtime variables | 🟡 the pure ones (`⎕A` `⎕D` `⎕IO` `⎕CT` `⎕UCS`), read-only; the ones that read a clock or a filesystem are ⚪ closed by the sandbox (`ErrorKind::Sandbox`) |
| `⎕FX` | 🟢 fix a definition from its lines, answering with its name — the same lines a `∇ … ∇` takes, control words included. 🟡 the lines must be literal text the compiler can read: one assembled at run time, or a `⎕FX` inside another definition's body, is named as a gap |
| Control structures `:If :While :Repeat :For :Select :AndIf :OrIf :CaseList` | 🟡 GNU APL rejects them, so the oracle is Dyalog's recording in `corpus/apl/dyalog-control.txt`: all 79 of its expressions agree. `:AndIf` and `:OrIf` short-circuit the condition above them, `:CaseList` takes any one of its items, `:For a b :In` takes each item apart, `:For` binds an item's contents, a body may call a function the program fixes after it, and a control structure may stand outside a definition |
| `:Return` `:Leave` `:Continue` | 🟡 the same recording; `:Leave` outside a loop is accepted here and refused there |
| Exact-or-scalar conformability | 🟢 |
| `¯` negatives, `1E3` exponents | 🟢 |
| Complex literal `2J3` | 🟢 |
| `'strings'`, `⍝` comments, `⋄` and newline separators | 🟢 |
| `{name}` host-data interpolation | 🟢 |

## APL — the Dyalog line

The inventory above is the APL2/ISO vocabulary, which is the line libjay's
APL follows by default (docs/coverage.md, "Which APL"). The Dyalog line is
a preset of the dialect object rather than a second engine:
`Dialect::dyalog()`, `APL.Dialect.dyalog` in Python. It answers 1967 of the
2012 expressions Dyalog 20.0 has been recorded on — the default answers
1805 of them — and the 45 it does not are itemised below.

That is a GATE, not a measurement: `cargo test -p libjay --test
oracle_dyalog` replays the recorded `dyalog:` column under the preset and
fails on any expression not on the exemption list,
`crates/libjay/tests/expected/dyalog.txt`. The list carries a reason per
row — 23 of them a divergence libjay keeps on purpose, 22 a gap — and the
`Tag` column below is the tag those gap rows name, so closing a row here
deletes its exemptions and tightens the gate. Nothing is exempt silently.
`jay-corpus stats apl --dialect-diff --dialect dyalog` measures the same
set from the outside, replaying the recorded column with no interpreter; it
includes the four Dyalog-only theme files and the tolerance theme.

What the preset changes, each of it verified against the recording:
`⎕CT` is `1e¯14`; `↑` is mix and `⊃` is first; `⌷` names the leading axes,
so a shorter index takes the trailing ones whole and an enclosed index
vector keeps its axis; a dyadic `⊂` counts partitions (partitioned
enclose) while `⊆` stays the partition both lines share; `≡` negates the
depth of an array whose items do not share one; a dfn answers with its
first sentence that is not an assignment; a nested grade uses the total
array ordering; dyadic `⍳` takes a vector on its left and gives a rank
error for anything else; a near-integer count is admitted relatively,
scaled by `⎕CT`; `⌊` and `⌈` scale their step by the magnitude; `⊤`
takes its digits exactly; an inner product `f.g` puts the each on the
PAIRING rather than on the fold, so `g` meets one element from each side
and the fold's own value is the cell; and a control structure is read
strictly, refusing a condition that is not a single value and a `:Leave`
outside a loop. The preset is Dyalog's default `⎕ML`, which is what the
recording ran under.

Where it differs, every row of it exempted by name in
`tests/expected/dyalog.txt`. `Tag` is the tag those rows carry: 🔴 is a
gap, whose rows go when the row here is closed, and ⚪ a divergence libjay
keeps.

| Cause | Tag | Rows | Status |
|---|---|---|---|
| `⎕R` and `⎕S` | `regex` | 5 | 🔴 refused by name; pure computation, so the sandbox is no obstacle — they are simply not written |
| A SHY result: a dfn whose answer came from an assignment has one, and the session does not print it (`{a←⍵×2} 5` shows nothing, `⎕←F 5` shows `10`) | `shy-result` | 4 | 🔴 libjay has no channel for either — every call yields a value, every value at the top level is printed, and there is no `⎕←` |
| Complex floor and ceiling | `complex-floor` | 2 | 🔴 Dyalog rounds to a Gaussian integer of the fundamental parallelogram; libjay takes each part |
| The obverse of a bound verb — `(2∘↑)⍣¯1` — and of an operand known only at run time | `obverse-of-bond` | 2 | 🔴 the obverse table reads the verb tree, and neither is in it |
| An operand in PARENTHESES to the right of an operator: `=⍥(2∘|)`, `⌽HALF (2∘↑)` | `operand-parens` | 2 | 🔴 the operator folder reaches it before the `)` has closed — a parser ordering gap, not a missing meaning |
| Two singletons of different rank conforming (`(1 1⍴5)+,3`) | `singleton-rank` | 2 | 🔴 the higher rank wins there, the first argument here |
| The `¯7○` branch cut | `circle-branch` | 1 | 🔴 the conjugate branch there |
| A COMPUTED array where a function operand belongs: `(⍳3){⍺⍺+⍵}0`. A literal one binds | `computed-operand` | 1 | 🔴 nothing in the IR holds an operand's expression to evaluate when the derived function is built |
| `⍺←` with a FUNCTION as the default left argument | `function-default` | 1 | 🔴 libjay takes an array alone |
| A dfn that falls off its end | `no-result` | 1 | 🔴 no result at all there, a value here |
| `⌺` over an empty | `stencil-empty` | 1 | 🔴 answered here, refused there |
| The empty-base `⊥` | `empty-base` | 5 | ⚪ zeros here; GNU APL agrees about the SHAPE and refuses to print the value, Dyalog refuses outright. Pinned in `corpus/apl/divergences.txt`, with the same rows in `fuzz_found.txt` |
| A rotate amount or a modulus above 2⋆53 | `large-count` | 3 | ⚪ the count the program wrote, reduced exactly; pinned in `divergences.txt` |
| `⍢` with a structural operand | `under-extension` | 3 | ⚪ libjay answers and the recorded Dyalog refuses; a preset chooses a dialect's rules, it does not withdraw an extension libjay ships in every dialect |
| Rows of the obverse table Dyalog does not hold: `⍋⍣¯1`, `⍒⍣¯1`, and `○⍣¯1` of a zero | `obverse-table` | 3 | ⚪ one table over both languages — grade is its own obverse there because it is in J, and Dyalog's own inverse of `○` divides by the argument and raises DOMAIN ERROR at zero |
| `⍠` with an option Dyalog's variant does not offer | `variant-extension` | 3 | ⚪ the same rule |
| A `{name}` dfn whose whole body is one identifier, which libjay reads as an interpolation hole (`a←1 ⋄ F←{{a} ⍵} ⋄ F 0`) | `name-hole` | 2 | ⚪ the brace-binding syntax is a fixed point of the embedding; the collision is real APL and has no answer yet |
| A diamond-separated sentence's value | `sequence-value` | 1 | ⚪ the block model: the last sentence's value, and nothing prints on the way |
| `⍬≡0⍴⊂⍬` | `empty-prototype` | 1 | ⚪ an empty nested array carries no prototype here |
| A derived function displayed where a value belongs (`×∘2 5`) | `function-display` | 1 | ⚪ Dyalog shows the function's source; libjay has no display for one |
| `6 2⍕'a'` | `format-character` | 1 | ⚪ dyadic `⍕` pads a character here, as GNU APL does |

The extensions libjay already ships (marked "no oracle" against GNU APL in
the tables above — `⊆`, `∘`, `⍥`, `⌺`, `f⍤g`, `⌸`, dfn guards and `∇` and
`⍺⍺`/`⍵⍵`, the control structures, trains and function assignment) now
have Dyalog's own answers recorded in `corpus/apl/dyalog-dfns.txt`,
`dyalog-dops.txt`, `dyalog-control.txt` and `dyalog-operators.txt` —
reference data under the `dyalog:` key alone, gating nothing. Where those
extensions had no APL2 reading to follow they now follow Dyalog's recorded
one in EVERY dialect, the shipped one included: a dfn is ambivalent, its
guard wants a single 0 or 1, a dfn written inside another reads the
enclosing one's locals, an array binds where a function operand belongs,
and `f⍣¯n` runs the inverse.

`⊇` and the other Dyalog features libjay does not implement at all are
below; they are the queue for a later wave, not counted in the APL totals.
Each will be recorded under the `dyalog:` snapshot key before it is
implemented, and the recording wins over anything a document says.

| Feature | Status |
|---|---|
| `⊇` select | 🔴 not yet — and today the parser calls it an unknown symbol rather than naming the promise, which is itself a diagnostics gap |
| Dfn error-guards `num::expr` | 🔴 not yet — today a bare syntax error at the second `:`, same diagnostics gap |
| `f⍣¯n` inverse powers | 🟢 the count may be negative, and the obverse table answers it — one table over both languages, so every row J's `^:_1` reaches is reachable here. Three rows Dyalog does not hold are named in `tests/expected/dyalog.txt` |
| Namespaces (`⎕NS`, `#.`, dotted names) | 🔴 refused by name |
| `⎕JSON`, `⎕R`/`⎕S`, `⎕CSV`, `⎕DT`, `⎕C` | 🔴 refused by name — pure computation, so the sandbox is no obstacle; they are simply not written |
| `&` spawn | ⚪ the sandbox does not open threads, in any dialect — the row is in the operator table above |

## Data, boundary, runtime

| Item | Status |
|---|---|
| Boolean, i64, f64, character | 🟢 |
| Boxes | 🟢 structural verbs, display, Python conversion |
| i8/i16/i32, u8/u16/u32, f32, `Date32`, `Time32`, `Boolean` at the boundary | 🟡 widened or unpacked by one copy on entry |
| u64 | 🟡 refused above 2⁶³−1 |
| Complex | 🟢 core type, `[re, im]` pairs; numpy `complex128` zero-copy, Arrow `struct<re, im>`. A value whose imaginary part is zero is read as the real it displays as wherever a real is wanted — ordering, `i.`, `$`, `#`, `I.` — while keeping the complex type `3!:0` reports |
| Extended integer, rational | 🟢 core types, heap-backed; exact arithmetic, Python `int` and `fractions.Fraction` at the boundary |
| Symbol | 🟢 core type: one `u32` per element into a process-wide intern table, so a symbol array copies and slices like an integer one. Ordering, `~.`, `i.`, `e.`, `/:` and the structural verbs all carry it; Python gets the names as `str` |
| Decimal128 | 🔴 |
| float16, byte-swapped data | 🔴 |
| Arrow string, binary, list, dictionary columns | 🔴 |
| Nulls | ⚪ neither language has a missing value; the column is named and refused |
| Mixed-type table columns | ⚪ silent promotion would damage values above 2⁵³ |
| Fortran-ordered numpy blocks | 🟢 read where they lie, as a column-major array |
| numpy views contiguous in neither order | ⚪ refused rather than silently copied |
| Arrow zero-copy in (i64, f64, i64-physical temporal) | 🟢 |
| DataFrame M×N → matrix, rows leading | 🟢 the columns cross borrowed and are folded where they lie; the shape is the logical one. A verb that reads elements in row order lays them out once, when it is applied |
| Zero-copy out | 🟡 rank-1 machine-numeric only; rank ≥ 2, chars, symbols, boxes and the exact types go via `.tolist()` |
| Arrow carrier for the exact types | 🔴 Arrow has none; `.tolist()` gives exact Python objects, `_1 x:` machine numbers |
| Parallel execution (own pool, `LIBJAY_THREADS`) | 🟢 |
| Expression fusion (blockwise kernels) | 🟢 |
| SIMD dispatch | 🟢 hot loops (arithmetic, reductions, fused kernels); x86-64 baseline/v2/v3/v4 and NEON, runtime-detected. The v4 (AVX-512) clone is compiled into every x86-64 artifact but not yet measured: no machine here has the features, so it is built, symbol-checked and unbenchmarked |
| GPU / device backend | 🟡 fused kernels only, via wgpu (Metal/Vulkan/DX12), compiled into the one artifact and dormant without an adapter. f64 needs `SHADER_F64`, which Metal has not; on such an adapter an f64 chain stays on the CPU unless the caller asks for `precision="f32"`. Integer chains, non-float results and `^` in f64 stay on the CPU. The f64 path is generated and validated but has not been executed anywhere yet — see [decisions.md](decisions.md) |
| Device placement API (`deploy`, `upload`, `DeviceArray`) | 🟡 `jay.j(...)` has no device by design; a result kept on the device is still materialised on the host once |
| C ABI: compile, bind, execute, errors, spans | 🟢 |
| C ABI: complex (`JAY_COMPLEX`, interleaved doubles) | 🟢 |
| C ABI: boxed results | 🔴 no descriptor for a box yet |
| C ABI: extended and rational results | 🔴 no descriptor for a bignum yet; `_1 x:` converts |
| C ABI: symbol results | 🔴 no descriptor for a table index yet; `5 s:` gives the names |
| Python: a `str` argument as a symbol | 🔴 a `str` arrives as a character array; `s:` inside the expression is how one becomes a symbol |
| Python: `jay.j`, t-strings, samples as live defaults | 🟢 |
| Rust compile-time checking of an expression (macro) | 🔴 |
| Sandbox: stdio open, other I/O closed | 🟢 no primitive reaches the filesystem or the network |
| Differential suites against J and GNU APL | 🟢 |

Details in [coverage.md](coverage.md); the reasoning behind the choices is in
[decisions.md](decisions.md).
