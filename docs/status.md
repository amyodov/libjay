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

**J: 159 green / 16 partial / 2 absent by design, of 177 valences in the
inventory. No row in J's primitive tables is red.**

**APL: 107 green / 18 partial / 3 absent by design, of 128 valences in the
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
| `!` | 🟢 factorial — the gamma function, in the complex plane as well as on the reals | 🟡 out of; zero wherever a whole y sits under a whole x, at every size. Past the width at which the falling factorial is taken (4096) the gamma quotient stands in, and the two shapes it cannot reach — a NEGATIVE whole y (`100000000 ! _2` is 100000001) and a fractional one (`1e10 ! 2.5` is `_1.05786e_35`) — are named: both need the logarithms of the gamma function rather than its values |
| `o.` | 🟢 pi times | 🟢 circle; `_12` to `12`, real and complex |
| `%.` | 🟢 matrix inverse (Householder QR, f64) | 🟢 matrix divide. Its left rank is infinite, so a right-hand side of rank 3 or more is solved whole — one column per element of an item — and the answer keeps every axis but the leading one. APL's `⌹` refuses that shape, which is its own reference's rule |
| `j.` | 🟢 imaginary | 🟢 complex |
| `r.` | 🟢 angle | 🟢 polar |
| `p.` | 🟡 roots, by Durand–Kerner in f64, with a repeated one refined through its m-1st derivative. The numbers agree with jconsole; their STORAGE need not. jconsole factors a polynomial of degree 2 or more over the coefficients' own exact type and answers rationals where it succeeds — whole roots for whole coefficients (`p. 6 _5 1` is `3 2`), rational ones for rational coefficients (`p. 1r2 _3r2 1` is `1 1r2`) — and falls back to floats for a linear polynomial, or where one root is not of that type (`p. 1 _3 2` is `1 0.5` there too). libjay computes in f64 throughout; the two cases that part are pinned in `corpus/j/divergences.txt`. A boxed argument is the root form and answers the coefficients; the multiplier may go unsaid, so `p. (<1 2)` is `2 _3 1` | 🟢 polynomial; a boxed `multiplier ; roots` left argument too, the multiplier optional |
| `p..` | 🟢 poly. derivative; a boxed argument is the root form | 🟡 poly. integral, x the constant term; a boxed argument is the root form. The integral divides by the power, and jconsole keeps that division exact for an EXTENDED or rational argument (`0 p.. 1 1 1x` is `0 1 1r2 1r3`) where libjay answers floats — pinned in `corpus/j/divergences.txt` |
| `p:` | 🟢 the y-th prime; extended where y is | 🟢 the prime queries: `_1` `0` `1` `2` `3` `4` `_4` |
| `q:` | 🟢 prime factors, exact however many digits the number has — trial division, then Miller–Rabin and Pollard's rho. The whole argument is read at once: one row per item, padded with 1s | 🟢 prime exponents: the first `x` primes' exponents, and for a negative `x` the last `\|x\|` columns of the factor table, all of them for `__` |
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
| `#` | 🟢 tally; extended where the argument is | 🟢 copy; a COMPLEX count is copies and fills — `1j2 # 'a'` is an `a` and two spaces — and `#^:_1` is the expansion that undoes it |
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
| `/:` | 🟢 grade up; boxes by the total array ordering — class, then rank, then the ITEMS one by one, and only then the shape, so `<'aa'` precedes `<,'b'` | 🟢 sort; the grade indexes x's ITEMS, so an atom answers only the first |
| `\:` | 🟢 grade down; boxes by the total array ordering | 🟢 sort; same |
| `A.` | 🟢 anagram index | 🟢 anagram |
| `C.` | 🟢 cycle-direct; a short direct permutation is the abbreviated one | 🟢 permute; direct, abbreviated, an atom, or cyclic — a cycle's element counts back from the end where it is negative, and every one is checked before the permutation is built |

### Boxes, format, system

| Spelling | Monad | Dyad |
|---|---|---|
| `;` | 🟢 raze | 🟢 link |
| `;:` | 🟢 words | 🟡 sequential machine: the table-driven form `(f;s;m;ijrd) ;: y`, with output codes 0 to 3 and 6 and every result form 0 to 5. The map turns a CHARACTER into a class — it has one entry per byte, and beside a numeric argument, whose values are the classes themselves, the reference refuses it, as libjay now does. The two vector codes (4 and 5) are named: black-box probing showed them marking a boundary inside the word being collected rather than ending it — a later code 3 then emits the pieces the marks divide — but what the machine emits at the END of the input after them did not follow from any rule the probes could confirm, and libjay will not guess at it |
| `L.` | 🟢 level of | — |
| `":` | 🟢 default format | 🟢 format by specification: `w j d` per column, width 0 for what the values need, a negative width for the exponential form, asterisks for what does not fit |
| `".` | 🟢 do; the string runs over the names around it | 🟢 numbers, x standing in for a word that is not one |
| `u:` | 🟡 unicode: the widened value has the same items and the same codes, and libjay has one character type where J has three, so the two differ only in DISPLAY — pinned in `corpus/j/divergences.txt` | 🟢 unicode, every form the reference defines: 3 (codepoints) and 10 (the characters they name), and the byte-oriented 1, 2, 8 and 9. libjay's one character type holds codepoints, so 2 and 10 change nothing, 1 keeps a codepoint modulo 256, 8 packs one into its UTF-8 bytes and 9 reads them back — a character list every one of whose codepoints is below 256 is what stands for J's byte string. The same one-type divergence in DISPLAY is pinned beside the monad's |
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
| `}` | 🟢 noun or verb operand, a boxed index specification, and a LIST of them. A gerund operand amends nothing monadically: it SELECTS — v gives the indices and w the array they index, and u is not applied at all | 🟢 the same, plus the gerund `` u`v`w} ``: u makes the replacement, v the indices, w the array they go into |

## J — conjunctions

| Spelling | Status |
|---|---|
| `"` rank | 🟢 noun ranks on the right; a VERB on the right lends its own three (`u"v` is `u"(v b. 0)`, so `<"(+/)` boxes the whole argument and `<"(<"1)` boxes each row); and a noun on the LEFT is the constant verb `m"n` — it ignores both arguments and answers m at rank n |
| `@` atop | 🟢 |
| `@:` at | 🟢 |
| `&` bond / compose | 🟡 verbs compose, literal nouns bond; computed nouns not yet |
| `&:` appose | 🟢 |
| `&.` under | 🟢 `v^:_1 @: u &: v`, over the obverse table — docs/coverage.md's "The obverse table" lists what is in it and the two verbs still named. `&.>` and `&.,` are the two unders not built out of an inverse: box by box, and over the ravel with the shape put back |
| `&.:` under | 🟢 the same on whole arguments, over the same table |
| `^:` power | 🟢 a count the program COMPUTES as readily as a literal one — a name, an expression, a definition's own argument — read where the derived verb is applied and not while the program compiles; a list of counts (mixed signs included), `_`, the traces `u^:(<n)` and `u^:a:`, a verb count, and negatives — which obverse a negative count runs is settled when the arguments arrive, so `x u^:_1 y` undoes the bond `x&u` and the monad undoes u |
| `.` dot product | 🟡 both valences: `x u . v y` at every rank, with `+/ . *` (the matrix product) a blocked parallel pass over the two buffers; the monad is the determinant by minors down the first column, whose base case is `u` applied to the last column's own values — so `u . v y` of a vector or an atom, each read as one column, is `u y` — and `-/ . *` over machine numbers goes by elimination instead. A determinant by minors of more than 16 rows is named — the expansion is exponential |
| `:` explicit definition | 🟡 `0 :` (the noun definition, whose lines below are its text), `1 :`, `2 :`, `3 :`, `4 :`, the `m : 0` body on the lines below, the same body given as a BOXED list of lines (`3 : ('a =. *: y' ; 'a + a')`), and `u : v`, which joins two VERBS into one ambivalent verb; `13 :` not yet, and an explicit MODIFIER (`1 :`, `2 :`) whose body is boxed is named |
| `;.` cut | 🟢 frets (`;.1` `;._1` `;.2` `;._2`), the rectangle `;.0` in both valences, and the tessellations `;.3` `;._3` with negative block sizes. With the movement row written out a negative size measures the block and reverses its axis; with the movement left implicit it does not measure it at all — the block runs to the END of its axis, reversed, whatever the magnitude said, which is what the reference answers. The left rank is finite — 2 for the rectangles, 1 for the frets — so a longer left argument is a FRAME of cuts, one per cell. An empty fret list is the whole argument in one piece, and a BOXED left argument is J's per-axis frets: one box per leading axis, the rest of the axes taken whole |
| `!.` fit | 🟢 both meanings. On a verb that compares it is the tolerance; on one whose answer can reach past what its argument holds — `{.` `$` `,` `,.` `,:` `#` `;` `>` `\|.` — it is the ELEMENT that stands where the value runs out. A fill of a wider type widens the answer, one of another kind entirely is refused, and a verb J gives no fit to refuses one here too |
| `!:` foreign | 🟡 `1!:1` (read a line from stdin), `1!:2` (write a line to stdout), `3!:0` (type code), `5!:1` (the atomic representation of a name) and the locale family `18!:0 18!:1 18!:2 18!:3 18!:5 18!:55`; the ones that reach a file, a script, the host, the clock or a shared library are ⚪ closed by the sandbox, `18!:4` and `18!:6` are ⚪ (the reference build defines neither as a meaning: one is absent, the other dumps the interpreter's own name tables), and the ones that only compute are 🔴 named |
| `` ` `` tie (gerund) | 🟡 the gerund is boxed data — one atomic representation per box, so it can be named, computed and displayed; a verb the representation cannot spell (a capped fork, an explicit definition) is named. As an ADVERB'S operand it cycles: `` u`v/ `` inserts the verbs between the items and `` u`v\ ``, `` u`v\. `` and `` u`v/. `` give one verb to each prefix, suffix, group, window or diagonal in turn, dyadic infix and outfix included; under any other adverb it is still named. Two CONJUNCTIONS hand it out per piece too — the cut `` u`v;.n `` one verb per piece, and `` u`v"n `` one per cell of the rank's frame, where a single box or a rank infinite in all three places is the constant verb `m"n` instead and the dyad has no meaning |
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
| `L:` level | 🟢 both valences; the dyad descends both arguments together. The two infinities are the two ends of the descent: `_` is the whole argument, however deeply it is boxed, and `__` is its leaves — level 0 written the other way round |
| `S:` spread | 🟢 both valences, as `L:` |
| `b.` boolean / characteristics | 🟢 `m b.` (the 32 boolean and bitwise functions) and all three characteristics: `0` the ranks, `1` the identity function, `_1` the obverse. `_1` answers a SPELLING, and libjay writes its own — a derived verb whose operand is a noun spells as `(n&+)` here where the reference writes the noun out |
| `$.` sparse | 🟡 the storage kind, the monad, the dyad's atomic forms `_1 0 1 2 3 4 5 7 8`, and the display — including a stored cell of its own shape, which is drawn as the array of all the cells is. The BOXED left arguments too: `(2;a)` stores the same value under other sparse axes, `(3;e)` gives it another sparse element — which changes what every position it does not store holds — and `(2 2;a)` says how many cells other axes would store. `(2 1;a)`, which asks how many BYTES they would take, is ⚪ not in the language: it reports one interpreter's own storage layout, which another implementation has no counterpart to. A sparse array reaching any other verb is expanded first, so the ANSWER always matches and the storage kind does not survive the step — where J keeps `s + 1` sparse, libjay gives the dense array. Sparse characters and boxes are named gaps, as they are in J |
| `H.` hypergeometric | 🟢 both valences: the monad sums the series to its limit, with the shared parameters cancelled, and `x (m H. n) y` stops after x terms — a whole nonnegative count, paired with the argument element by element. A series that neither converges nor overflows is refused by name |
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
| Displaying a bare tacit-verb name (`mean` after `mean =. +/ % #`) | 🟡 the linear representation, bracketed where the spelling would otherwise read as something else; a cap fork is written `f@:g` and `u"b a b` as `u"a b`, which are the same functions under another spelling, and a definition whose body is on the lines below keeps no text to give back |
| Displaying a bare modifier name (`m` after `m =. /`) | 🟡 a primitive modifier and an explicit one written inline; one whose body is on the lines below, or a `{{ }}`, keeps no text to give back |
| `explain` facility | 🟢 |
| Adverb and conjunction assignment `m =. /` | 🟢 the name is that modifier from the next sentence on |
| Multiple assignment `'a b' =. …` | 🟢 the value's items are shared out whatever its rank, a scalar goes to every name, one name takes the whole value |
| `=.` vs `=:` scoping | 🟢 a definition has its own frame; `=:` names a global |
| Explicit definitions `3 : '…'`, `4 : '…'`, `{{ }}` | 🟢 a body with no sentences at all has neither valence, and refuses |
| Multi-line definition body `3 : 0` … `)` | 🟢 a lone `:` line separates the monad case from the dyad case |
| Explicit adverb `1 : '…'` and conjunction `2 : '…'` | 🟢 both phases; operands as `u`/`v` or `m`/`n` |
| Multi-line modifier body `1 : 0` / `2 : 0` … `)` | 🟢 |
| `{{ }}` modifier forms | 🟢 the part of speech is read off the operand names the body uses |
| `{{)a` `{{)c` `{{)v` `{{)d` `{{)m` markers | 🟢 the marker line states the part of speech |
| `{{)n` noun direct definition | 🟢 the lines below are text, and the `}}` that ends it starts a line |
| A modifier body that derives the modifier itself | 🟡 a body that names no argument settles its `if.` where the modifier is derived and stops at its base case, bounded at 16 deep; one that names an argument belongs to the derived verb, which libjay parses whole here, and stays a named gap |
| Tacit definition `13 : '…'` | 🟡 the translation and what it computes; a cap fork is displayed `f@:g` where the reference writes `[: f g`, and a body the abstraction cannot reach becomes the explicit definition, as the reference's own fallback does |
| Control words `if. while. for. select. try.` | 🟢 `whilst.`, `for_i.`, `fcase.`, `elseif.` included |
| Control words `throw. catcht. goto_x. label_x.` | 🟢 a branch lands on the body statement its label stands on, and the target is settled while the definition is built, so a missing label, a doubled one and one written inside a control structure are all refused there; `throw.` leaves the definition it stands in and only a `catcht.` in a CALLER's `try.` block takes it |
| Locales | 🟢 named and numbered locales, the locative `name_locale_` and the indirect `name__var`, `cocurrent` and `coclass`, a definition's body reading its own home locale, and the search path with `z` on it. A `cocurrent` whose locale name is COMPUTED changes the locale at run time but not the one the sentences after it are read in, so a name whose part of speech only that switch would settle is a 🔴 named gap; so is a verb named by an indirect locative |
| `18!:` locale foreigns | 🟢 `18!:0` (the locale class), `18!:1` (the names alive), `18!:2` (the search path, read and written), `18!:3` (make one), `18!:5` (the current locale) and `18!:55` (erase a numbered one) |
| Ill-formed names | 🟢 a name ends in an underscore only as the locative `name_locale_`, so `a_` is refused where `a_b_`, `a__` and `cc__` are names |
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
| Base and constant literals `16b1f`, `1p1`, `1x1` | 🟢 the base is a number in its own right (`3r4b11`, `3j4b11`, `2e1b11`) and every letter after the `b` is a digit (`36bxyz`, `2b11p1`); a `.` among the digits starts the negative powers |
| `'strings'`, `NB.` comments, multi-sentence programs | 🟢 a quoted literal is a BYTE vector, as J's literal type is: `# 'é'` is 2, and length, shape, indexing, `a.`, `e.`, `i.` and `":` all count the UTF-8 bytes the source spells. One item per character is the opt-in `j_unicode_strings` extension ([extensions.md](extensions.md)) |
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
| `!` | 🟢 factorial — the gamma function by the Lanczos approximation, in the complex plane as well as on the reals; a value with no imaginary part is the real it displays as | 🟢 binomial; the same function |
| `○` | 🟢 pi times | 🟢 circle; `¯12` to `12`, real and complex |
| `?` | 🟡 roll; libjay's own stream, not GNU APL's | 🟡 deal; same |
| `⌹` | 🟢 matrix inverse (Householder QR, f64); an argument of rank 3 or more is a rank error, as it is in the reference — J's `%.` has rank 2 and runs over the 2-cells instead | 🟢 matrix divide; the same rank rule on both sides. The bracket form `⌹[K]` is not this function with an axis but a GROUP of unrelated ones — see the `⌹[K]` row in the syntax table |

### Comparison and logic

| Glyph | Monad | Dyad |
|---|---|---|
| `=` | — | 🟢 equal |
| `≠` | 🟢 unique mask over the ELEMENTS, keeping the argument's shape. `unique_mask` names Dyalog's reading, one bit per major cell and always a vector | 🟢 not equal |
| `<` | — | 🟢 less than; total, so characters order by codepoint, a character stands below every number and a complex value orders by its real part then its imaginary one. `Dialect.order_domain` names the narrow reading Dyalog and J take |
| `≤` | — | 🟢 less or equal; total, so characters order by codepoint, a character stands below every number and a complex value orders by its real part then its imaginary one. `Dialect.order_domain` names the narrow reading Dyalog and J take |
| `>` | — | 🟢 greater than; total, so characters order by codepoint, a character stands below every number and a complex value orders by its real part then its imaginary one. `Dialect.order_domain` names the narrow reading Dyalog and J take |
| `≥` | — | 🟢 greater or equal; total, so characters order by codepoint, a character stands below every number and a complex value orders by its real part then its imaginary one. `Dialect.order_domain` names the narrow reading Dyalog and J take |
| `≡` | 🟢 depth | 🟢 match |
| `≢` | 🟢 tally | 🟢 not match |
| `∧` | — | 🟢 LCM / and; the ASCII `^` is read as the same glyph |
| `∨` | — | 🟢 GCD / or; GNU APL's rounding and zero-sign rules (`gcd_rule`) |
| `⍲` | — | 🟢 nand |
| `⍱` | — | 🟢 nor |
| `~` | 🟢 not | 🟢 without |
| `⊤∧` | 🟢 the argument as the integer it stands for | 🟢 bit-wise and |
| `⊤∨` | 🟢 the same | 🟢 bit-wise or |
| `⊤⍲` | — | 🟢 bit-wise nand |
| `⊤⍱` | 🟢 bit-wise not | 🟢 bit-wise nor |
| `⊤=` | — | 🟢 bit-wise complement of exclusive or |
| `⊤≠` | — | 🟢 bit-wise exclusive or |

### Structural

| Glyph | Monad | Dyad |
|---|---|---|
| `⍴` | 🟢 shape | 🟢 reshape, laying out elements; an empty argument fills |
| `,` | 🟢 ravel; `,[K]` runs a RUN of neighbouring axes together, and a fractional `,[K.5]` adds a new axis of length one at the gap | 🟢 catenate (last axis); a simple side joined to a nested one has its items enclosed, and two that share no type make a mixed simple array. `,[k]` joins the named axis, and a fractional `,[k.5]` LAMINATES — the two arguments beside each other along a new axis there |
| `⍪` | 🟢 table; `⍪[K]` is `,[K]`, as the reference reads an axis on either glyph | 🟢 catenate (leading axis); same, `⍪[k]` and its laminate included |
| `⌽` | 🟢 reverse; `⌽[k]` reverses the named axis | 🟢 rotate; `⌽[k]` rotates it |
| `⊖` | 🟢 reverse first; `⊖[k]` is `⌽[k]` — the axis settles which, whichever glyph was written | 🟢 rotate first; one amount per column, as `⌽` takes one per row; `⊖[k]` likewise |
| `⍉` | 🟢 transpose | 🟢 dyadic transpose; x says which axis of the result each axis of y becomes, and a repeated destination runs those axes together |
| `↑` | 🟢 first; an empty nested argument answers the prototype it remembers, and `↑[K]` takes one item along each named axis, the axis staying. Under `first_disclose` it is MIX — `↑[K]` places the item axes where K names, a fractional `↑[K.5]` putting them all at one gap — which frames characters beside numbers into a mixed simple array, each cell padded with its own prototype | 🟢 take; overtaking a nested array fills with the first item's prototype. One count per axis; `axis_counts` names Dyalog's reading, where fewer counts leave the trailing axes whole. `x↑[K]y` takes one count per axis K names, in the order written, and leaves every other axis whole |
| `↓` | 🟡 no oracle: GNU APL has no monadic `↓`; Dyalog's split, `↓[k]` recorded against Dyalog in `corpus/apl/dyalog-axis.txt` | 🟢 drop; one count per axis, and `axis_counts` names Dyalog's reading, where fewer counts drop nothing from the trailing axes. `x↓[K]y` drops one count per axis K names, in the order written, and nothing from the rest |
| `⊂` | 🟢 enclose; `⊂[K]` makes the named axes the shape of each item, in the order written, and the rest the shape of the answer | 🟢 partitioned enclose; rank 2 and above partitions the last axis; a single flag extends over every item, so `1⊂1 2 3` is one partition, and no flag against no item is the empty nested vector. `x⊂[k]y` partitions the named axis in place |
| `⊃` | 🟢 disclose / mix; a character cell frames beside a numeric one into a mixed simple array. `⊃[K]` places the item axes at the positions K names | 🟢 pick: one item of the path is one LEVEL and holds one index per axis of the value at that level, counted from `⎕IO` upwards |
| `⊆` | 🟡 no oracle: not in GNU APL's character set; Dyalog's nest, which takes no axis there | 🟡 no oracle; Dyalog's partition, which is GNU APL's dyadic `⊂`; `x⊆[k]y` partitions the named axis, recorded against Dyalog in `corpus/apl/dyalog-axis.txt` |
| `⌷` | 🟡 no oracle: materialise, which Dyalog makes the identity | 🟢 index (APL2: one item of x per axis, a scalar or an enclosed vector). `x⌷[K]y` indexes only the axes K names and leaves the rest whole; `Dialect.axis_order` names which index goes with which axis, ascending here and as written under Dyalog's |
| `⊥` | — | 🟢 decode; the inner product `+.×` over x's last axis and y's leading one; a SINGLE on either side extends along the other's axis, and an empty axis weighs nothing — with no digit to weigh the radix is never read, so `'a'⊥(0⍴0)` is 0 |
| `⊤` | — | 🟢 encode; with no value to write the radix is never read, so `'a'⊤(0⍴0)` is the empty. `A⊤[N]B` encodes to N copies of the single radix A — N counted from one whatever `⎕IO` is — and `A⊤[0]B` works the width out, one digit more when a value is negative |

### Selection, search, sort

| Glyph | Monad | Dyad |
|---|---|---|
| `⍳` | 🟢 index generator; a shape of two lengths or more gives the nested array of coordinate vectors | 🟢 index of; a left argument of rank 2 or more is searched element by element and each answer is the enclosed coordinate vector that finds it, or the enclosed empty vector where it is absent. `lookup_left` names Dyalog's reading, where the left argument's MAJOR CELLS are searched — a matrix looks up rows and answers one number per cell of the right argument, and a scalar has no cell to search |
| `⍸` | 🟢 where; `where_rank` names Dyalog's reading, where an index is a vector as long as the rank at rank 0 too | 🟢 interval index; `lookup_left` names Dyalog's reading, where the bounds are the left argument's major cells |
| `∊` | 🟢 enlist | 🟢 membership |
| `⍷` | — | 🟢 find; reads a mixed simple array element for element |
| `∪` | 🟢 unique | 🟢 union; builds a mixed simple array where the two share no type |
| `∩` | — | 🟢 intersection; reads a mixed simple array element for element |
| `⍋` | 🟢 grade up; compares under `⎕CT`; nested by the APL2 rule (`nested_grade`) | 🟢 collating grade |
| `⍒` | 🟢 grade down; compares under `⎕CT`; nested by the APL2 rule (`nested_grade`) | 🟢 collating grade |

### Format, I/O, identity

| Glyph | Monad | Dyad |
|---|---|---|
| `⍕` | 🟢 format | 🟡 format by specification: width and precision pairs, a width of 0 as wide as the column needs plus a blank, a negative precision the scaled form, and a half rounded away from zero. `format_spec` names Dyalog's reading of the four rules that part; a nested argument is named |
| `⍎` | 🟡 execute; the string runs over the names around it. An EMPTY program yields no value at all in GNU APL, where a whole SENTENCE may be one and print nothing; a libjay verb has no way to answer that, so `⍎''` is a value error pointing at the `⍎` — pinned in `corpus/apl/divergences.txt` | — |
| `⊢` | 🟢 same | 🟢 right; `A⊢[M]B` is the selection function — a 1 in M takes the element of B, a 0 the element of A, and the three agree by the scalar rule |
| `⊣` | 🟢 same | 🟢 left |
| `⎕←` / `⍞←` output | 🟢 `⎕←` ends the line; `⍞←` writes the characters and nothing else. Both assign, so both pass their value on and neither ends the definition they stand in — a dfn body may print and go on computing under either reading of `Dialect.dfn_result` | — |
| `⍞` character input | 🟢 one line from the input source, terminator dropped | — |
| `⎕` evaluated input | 🟢 one line, run as APL over the program's own names | — |
| `→` branch | 🟡 inside a `∇` definition: labels, `→0`, `→(cond)/L`, `→⍬`, and a label beside a control structure — a label is the number of its LINE and the body carries the line each statement began at. Branching INTO a control structure has no statement to land on and is named. No oracle: GNU APL has no control structures in a `∇` definition at all | 🟢 `A→B` branches A lines on from the line it stands on when B holds — `0→B` runs the line again and a step past the body ends the definition |
| `⍬` zilde | 🟢 | — |
| `⌶` I-beam | ⚪ implementation-defined | ⚪ the same |

## APL — operators

| Glyph | Status |
|---|---|
| `/` reduce (last axis) | 🟢 between the ELEMENTS along the axis, each disclosed and the fold's value enclosed: `,/1 2 3` is an enclosed vector. `f/[k]` folds the named axis instead, and one whole axis is all it takes |
| `⌿` reduce (leading axis) | 🟢 the same rule down the first axis: `,⌿2 3⍴⍳6` pairs the columns; `f⌿[k]` and `f/[k]` are the same function |
| `/` `⌿` n-wise reduction (dyadic) | 🟢 `n f/ y` folds every window of n items along the axis the glyph chooses: `2+/1 2 3` is `3 5`. n is one number — a negative one reverses each window, zero answers the operand's identity once per gap, and a window may reach one item past the axis before it is a domain error. A positive window shares the blockwise kernel with J's `n u/\ y`. `n f/[k] y` folds the windows along the named axis. The shape of an EMPTY argument of rank ≥2 diverges from GNU APL — see divergences |
| `/` `⌿` replicate (after an operand) | 🟢 a negative count leaves that many prototype fills; `x/[k]y` replicates along the named axis |
| `\` `⍀` scan | 🟢 the reduce over each prefix, so it collects the same enclosures; `f\[k]` and `f⍀[k]` scan the named axis |
| `\` `⍀` expand (after an operand) | 🟢 the gap holds the first item's prototype; `x\[k]y` expands the named axis. A boolean mask; `expansion` names Dyalog's reading, where any integer vector serves — a positive count repeats that item, a negative one leaves that many fills, 0 means `¯1`, and the result is `+/1⌈|X` items long |
| `¨` each | 🟢 including results of different depth: an each that answers a number for one item and a list for another frames them into the nested vector such a result is |
| `⍨` commute | 🟢 |
| `∘.` outer product | 🟢 the function between every pair of ELEMENTS whatever its rank, each disclosed on the way in and the result enclosed on the way into the table: `1 2∘.,3 4` is a two-by-two of pairs, `¯1 0 1∘.⌽⊂m` rotates the matrix |
| `⍤` rank / atop | 🟡 a rank specification, or Dyalog's atop with a function operand; no oracle for the latter |
| `⍣` power | 🟢 a count the program COMPUTES as readily as a literal one — a name, a parenthesised expression, a definition's own argument — read where the derived function is applied; the count is ONE operand, so `f⍣N+1` reads `N` and leaves the `+1` to the sentence. Negatives included, answered from the obverse table, and a dyadic `x f⍣¯n y` undoes the bond `x∘f`; a function operand is `f⍣≡`. GNU APL implements no negative MONADIC power, so those rows are recorded against Dyalog in `corpus/apl/dyalog-operators.txt`; it does answer the dyadic ones, and reads `x-⍣¯1 y` differently, pinned in `corpus/apl/divergences.txt` |
| `∘` beside | 🟡 Dyalog's `f∘g` has no oracle: GNU APL rejects two function operands. `A∘f` and `f∘A` bind the array as f's argument here — Dyalog's bind, and J's `m&v` — where GNU APL DOES answer, by reading the `∘` as its matrix product against f's monadic result (`2∘× 3 4 5` is `2 2 2` there); the divergence is pinned in `corpus/apl/divergences.txt`. Where neither operand is a function the matrix product is what libjay answers too — a left vector is a row and a right one a column, a scalar operand makes it the element-wise `×`, and inner lengths that differ are padded with zeros |
| `⍥` over | 🟡 no oracle: not in GNU APL's character set; Dyalog's `f⍥g` |
| `⍛` before | 🟡 no oracle: GNU APL rejects it; Dyalog's `f⍛g` |
| `⍢` under | 🟡 no oracle, and no reference either: GNU APL rejects the glyph and Dyalog 20.0 answers `SYNTAX ERROR: Invalid token` (recorded in `corpus/apl/dyalog-operators.txt`), so ours is an extension — `g⍣¯1 ⊢ (g x) f (g y)`, over the same obverse table J's `&.:` uses |
| `⌸` key | 🟡 no oracle: GNU APL rejects it; Dyalog's, with the operand taking the key and its group |
| `@` at | 🟡 no oracle: GNU APL rejects the glyph; Dyalog's monadic form — a VALUE right operand is the positions, a function's result is a boolean mask over the items, a value left operand replaces what stands there and a function is applied to the selection. The dyadic `x f@g y` is named |
| `⌺` stencil | 🟡 no oracle: GNU APL rejects it; Dyalog's monadic-window form — one size per leading axis, the windows centred and the edges filled. The two-row form that also gives the movement is named |
| `.` inner product | 🟢 `f.g` is `f/¨` over the outer product: `+.×` is the matrix product, `∧.=` asks which rows match. Each vector along x's last axis meets each vector along y's first under g, f folds the result, and the each encloses it. `Dialect.inner_each` names Dyalog's reading, which puts the each on the pairing instead — g meets one element from each side and the fold's own value is the cell |
| `⍠` variant | 🟡 one dialect setting overridden for one application: `⎕CT` (the principal option, so a bare number sets it) and `⎕IO`, as literal options — `=⍠0`, `⍳⍠('IO' 0)`. Another option, or one that is not settled when the program is compiled, is named. No oracle: GNU APL rejects the glyph, and Dyalog takes a variant only on its search-and-replace family — both `=⍠0` and `⍳⍠('IO' 0)` are a DOMAIN ERROR there, recorded in `corpus/apl/dyalog-operators.txt` |
| `&` spawn | ⚪ starts an APL thread, which the sandbox does not open — as J's `T.` and `t.` do not |

## APL — syntax and features

| Feature | Status |
|---|---|
| Stranding (vector notation) | 🟢 |
| Nested arrays | 🟡 structural verbs, mixed simple arrays (built by `,` `⍪` `∪` `∩` `~` `⍷` `∊` `⍳` `≡`, and printed with a run of characters as text) and prototype fills; the operators apply between items, so `∘.`, `/`, `⌿`, `\`, `⍀` and `.` disclose what they take and enclose what they collect. The scalar functions PERVADE: they descend through the boxes to the simple values at the bottom, on a work stack of their own so that depth costs no call stack |
| `←` assignment, including inline | 🟢 |
| Function assignment `F←+/`, `F←+/÷≢` | 🟡 no oracle: GNU APL rejects it; the same extension as trains, and off with it |
| Dfns `{⍵+1}`, `⍺`/`⍵`, `⋄` bodies, nesting | 🟢 |
| Dfn assignment `F←{⍵×2}` | 🟢 |
| Dfn guards `cond:expr`, `⍺←default`, `∇` self-reference | 🟡 guards and `∇` have no oracle (absent from GNU APL); `⍺←` follows the published default-only rule where GNU APL assigns unconditionally (recorded divergence) |
| Dfn operators `⍺⍺` / `⍵⍵` | 🟡 no oracle: GNU APL has neither; a dfn naming one is an operator, and naming the operator keeps it one |
| SHY results | 🟡 no oracle: GNU APL has no dfns to have them; a definition whose answer came from an assignment answers shyly, `Program::run_detail` reports it, and an operator that ends by applying the definition passes it on (`{a←⍵×2}¨1 2 3`) |
| Tradfns `∇ Z←L F R;locals` … `∇` | 🟢 including APL's global-by-default scope rule, the niladic form, which naming calls, and a body whose lines carry the `∇` editor's line numbers (`[1]`, `[1.1]`), which is how every printed definition is written |
| Trains (forks and atops) | 🟡 no oracle: GNU APL rejects them; Dyalog's rules, shipped as an extension (`Dialect.trains`, on by default) — 2-train atop, 3-train fork, a value left tine, longer trains grouped from the right |
| Bracket indexing `A[1]` | 🟢 reading and writing, elided slots included. The brackets bind to the value written immediately before them, which in a run of numbers is the LAST number: `1 2 3[2]` is `1 2` beside `3[2]`, and indexing a scalar is a rank error, as the reference has it |
| Indexed assignment `A[i]←v`, `A[i;j]←v` | 🟢 copy-on-write on the named value. It is an expression like any other, so it may stand inside a larger sentence and chain (`B←A[2]←5`, `2+A[1]←9`, `A[1]←C[2]←9`); its value is the value ASSIGNED, not the array it was written into. The target is a NAME — writing through an expression (`(A,4)[1]←9`) is refused there too |
| Axis specification `f[K]` | 🟢 for every function that takes one: `/` `⌿` `\` `⍀` after an operand and the dyadic `x/[k]y` and `x\[k]y`; `⌽` `⊖`; `,` `⍪` in both valences, laminate's FRACTIONAL axis (`x,[0.5]y`) included; `↑` `↓` `⌷` `⊂` `⊆`, monadic `↑` (first here, mix under Dyalog's dialect, where `↑[K.5]` places the item axes at a gap) and `⊃`; and the SCALAR functions, where the argument of lower rank lines up with the named axes. The axis may be COMPUTED — a name, an expression, a definition's own argument (`K←1 ⋄ ⌽[K]M`) — and is then read where the function is applied and held to the argument's rank there. A list of axes is read where the function takes one — `,[1 2]`, `1 2↑[3 1]`, `⊂[2 1]`, `(2 3⍴1)+[1 2]` — and `Dialect.axis_order` names which end of the pairing Dyalog reads differently. An explicit definition reads the brackets as a value of its own rather than as an axis: a `∇` header may declare its name (`∇Z←AV[X] B`) and a `{…}` reads it as `χ`, verbatim and with no `⎕IO` adjustment; a definition whose header names none refuses one. Three brackets are not axes at all: `⊤[N]`, a digit count; `⊢[M]`, a selection mask; and `⌹[K]`, which picks one of a group of unrelated functions — the first and the last choose which function the glyph stands for and so stay settled before the program runs, while `⊢[M]` may be computed. A function with no axis form (`⍉` `∊` `≡` `⍴` `⍒`, an operator's derived function) is named as a gap |
| `⎕IO` as a dialect setting of the compiler | 🟢 |
| Dialect object (`⎕IO`, `⎕CT`, the lineage settings) | 🟢 two presets — `Dialect::gnu_apl()`, the APL2/ISO line plus the extensions, which is the default, and `Dialect::dyalog()` (`APL.Dialect.dyalog` in Python). Every point where the lineages diverge is a setting on it: `⎕CT`, `↑`/`⊃`, `⌷`, dyadic `⊂`, `≡`'s sign, the dfn result, the nested grade, dyadic `⍳`'s left rank, an axis list's order (`axis_order`), what `< ≤ ≥ >` may order and trains are all implemented in both readings; the nested model, `⍺←`'s laziness and the complex order are implemented in one, and asking for the other is refused as not implemented yet |
| `⎕`-system names as runtime variables | 🟡 the pure ones, read-only: `⎕A` `⎕D` `⎕AV` `⎕IO` `⎕CT` `⎕LX` `⎕ET` `⎕EM`, and the system functions `⎕UCS` `⎕CC` `⎕NC` `⎕CR` `⎕FX`. `⎕CC` answers every class that is a set anyone can state, and the four glyph repertoires (5, 6, 7, 9) as the reference's own tables — 7 and 9 as the 6-by-10 and 4-by-7 frames they are. `⎕AV` is the 256-character atomic vector, whose content the standard leaves to the implementation and which libjay measured from the reference and adopted, so that `⎕AV⍳c` means the same in both. `⎕LX` is empty and stays empty — libjay loads no workspace for a latent expression to be latent for — and `⎕ET`/`⎕EM` are the values that mean "no error yet", which is what every program libjay can run reads: nothing in it catches an error and carries on. The ones that read a clock or a filesystem, and the whole shared-variable surface (`⎕SVO` `⎕SVQ` `⎕SVR` `⎕SVC` `⎕SVE` `⎕SVS`), are ⚪ closed by the sandbox (`ErrorKind::Sandbox`). `⎕SYL` is ⚪ not in the language: it reports one interpreter's own build limits — cores configured, hash-table size, input line length — and another implementation has no counterpart to put there. `⎕PW` is a named gap: libjay's display writes a value in full and folds no line to a page width |
| `⎕PP` and `⎕RL` as settings a program changes | 🟢 both are read and set while the program runs, and what they control follows: setting `⎕PP` moves the significant digits of every float displayed afterwards, the run's answer included (`Outcome::fmt` carries the conventions the run ended on, so a host displays what the program asked for), and setting `⎕RL` starts libjay's random stream from that seed, so the same seed rolls the same numbers. `⎕PP` starts at libjay's own six digits rather than the reference's ten (a recorded divergence), and the SEQUENCE `⎕RL` starts is libjay's own — the seed is reproducible, the numbers are no reference's. A run's link belongs to that run and does not reach the next |
| `⎕NC` name class | 🟢 what a name holds now: `¯1` not a name at all, `0` a name with nothing in it, `2` a variable, `3` a defined function, `5` a system variable, `6` an argument of a `{…}`. A vector is one name and a matrix one per row, trailing blanks aside. A name libjay would not accept is `¯1` here — its own lexer's rule, which is why `_` parts company with the reference |
| `⎕CR` character representation | 🟡 monadic `⎕CR` gives a `∇` or `⎕FX` definition the lines it was written as, header first, padded with blanks to the longest; a name that is no definition has no text, and a `{…}` — an expression rather than a listing — is a named gap. Dyadic `n ⎕CR y` answers the conversions that rewrite the same bytes another way: 5 and 6 (hexadecimal, either case), 13 (back from it), 16 and 17 (base 64, RFC 4648), 18 and 19 (UTF-8, both ways). The numbers that report on an interpreter's own display and storage — the boxed listings, its internal record of a value, its cell-type codes — are named gaps |
| Structured variables (`P.x←3`) | 🔴 named: a name-space feature, not a primitive. Every assignment, lookup and scope rule would have to learn about dotted paths, and the reference gives the whole structure a display of its own |
| The `⌹[K]` group | 🟡 the bracket after `⌹` picks a function of a group rather than an axis, and K is its number whatever `⎕IO` is. `⌹[8]` multiplies two polynomials given by their coefficients, lowest power first, and `⌹[9]` divides them, answering the quotient and the remainder — the quotient lowest power first and the remainder the other way round, which is the reference's own asymmetry. Both are over vectors; rank 2 and above, and a dividend with fewer coefficients than the divisor (where the reference contradicts its own shapes), are named. `⌹[1]`, a QR factorization, and `⌹[7]`, a polynomial written out as text, are named gaps |
| `⎕FX` | 🟢 fix a definition from its lines, answering with its name — the same lines a `∇ … ∇` takes, control words included. 🟡 the lines must be literal text the compiler can read: one assembled at run time, or a `⎕FX` inside another definition's body, is named as a gap. libjay resolves a name to the function it stands for while it compiles, so a definition that does not exist until the program runs has no caller that could name it |
| Control structures `:If :While :Repeat :For :Select :AndIf :OrIf :CaseList` | 🟡 GNU APL rejects them, so the oracle is Dyalog's recording in `corpus/apl/dyalog-control.txt`: all 79 of its expressions agree. `:AndIf` and `:OrIf` short-circuit the condition above them, `:CaseList` takes any one of its items, `:For a b :In` takes each item apart, `:For` binds an item's contents, a body may call a function the program fixes after it, and a control structure may stand outside a definition |
| `:Return` `:Leave` `:Continue` | 🟡 the same recording; `:Leave` outside a loop is accepted here and refused there |
| Exact-or-scalar conformability | 🟢 |
| `¯` negatives, `1E3` exponents | 🟢 |
| Complex literal `2J3` | 🟢 |
| `"strings"` with C escapes | 🟢 always a vector, where `'Q'` is a scalar; `\a \b \f \n \r \t \v`, `\\`, `\"` and `\0` are read, and `\` before anything else keeps its backslash |
| `$ff` hexadecimal literals | 🟢 one scalar in either case of letter, stranding as a decimal literal does |
| Conditional `test →→ body ←→ otherwise ←←` | 🟢 the test is read as strictly as a dfn guard's, each marker may end a line, and a clause that is not taken shows nothing. Only the block model divides us from GNU APL there: a clause of several statements displays the last one's value, not every one |
| Distributed assignment (`(a b)←1 2`) | 🟢 the names in the brackets share the value out between them — one item each, or a scalar to all of them; a rank above one and a length that does not match are refused as the reference refuses them |
| Stranding a specification (`3 V←1 2`) | 🟢 specification binds tighter than a strand item, so the assignment's value is one item |
| `'strings'`, `⍝` comments, `⋄` and newline separators | 🟢 |
| Unicode look-alike glyphs (`∣ ∈ ∼ ⋆ −`) | 🟢 read as `\| ∊ ~ * -`, which is what the reference does with them; harvested APL is full of them, and the list is closed — `∗`, `∸`, `‾` and `∅` are refused there and here |
| `{name}` host-data interpolation | 🟢 |

## APL — the Dyalog line

The inventory above is the APL2/ISO vocabulary, which is the line libjay's
APL follows by default (docs/coverage.md, "Which APL"). The Dyalog line is
a preset of the dialect object rather than a second engine:
`Dialect::dyalog()`, `APL.Dialect.dyalog` in Python. It answers 2616 of the
2658 expressions Dyalog 20.0 has been recorded on — the default answers
2324 of them — and the 42 it does not are itemised below.

That is a GATE, not a measurement: `cargo test -p libjay --test
oracle_dyalog` replays the recorded `dyalog:` column under the preset and
fails on any expression not on the exemption list,
`crates/libjay/tests/expected/dyalog.txt`. The list carries a reason per
row — 23 of them a divergence libjay keeps on purpose, 19 a gap — and the
`Tag` column below is the tag those gap rows name, so closing a row here
deletes its exemptions and tightens the gate. Nothing is exempt silently.
`jay-corpus stats apl --dialect-diff --dialect dyalog` measures the same
set from the outside, replaying the recorded column with no interpreter; it
includes the four Dyalog-only theme files and the tolerance theme.

What the preset changes, each of it verified against the recording:
`⎕CT` is `1e¯14`; `↑` is mix and `⊃` is first; `⌷` names the leading axes,
so a shorter index takes the trailing ones whole and an enclosed index
vector keeps its axis; `⌷[K]` and a scalar function's `f[K]` pair their
axes with what accompanies them in the order K was written, where the
default reads K as a set; a dyadic `⊂` counts partitions (partitioned
enclose) while `⊆` stays the partition both lines share; `≡` negates the
depth of an array whose items do not share one; a dfn answers with its
first sentence that is not an assignment; a nested grade uses the total
array ordering; dyadic `⍳` and `⍸` search their left argument's MAJOR
CELLS, so a matrix looks up rows and a scalar is a rank error; `↑` and `↓`
take a left argument shorter than the rank, the counts applying to the
leading axes; monadic `≠` marks major cells and answers a vector; `\` takes
any integer count list, not a boolean mask alone; monadic `⍸` gives a
rank-0 argument an EMPTY index vector; dyadic `⍕` rounds a half on the
decimal that names the value, keeps a one-digit mantissa's point, pads the
scaled form's exponent field out to four characters and fills a field too
narrow with asterisks; a near-integer count is admitted relatively,
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
| Complex floor and ceiling | `complex-floor` | 2 | 🔴 Dyalog rounds to a Gaussian integer of the fundamental parallelogram; libjay takes each part |
| The obverse of a bound verb — `(2∘↑)⍣¯1` — and of an operand known only at run time | `obverse-of-bond` | 2 | 🔴 the obverse table reads the verb tree, and neither is in it |
| An operand in PARENTHESES to the right of an operator: `=⍥(2∘|)`, `⌽HALF (2∘↑)` | `operand-parens` | 2 | 🔴 the operator folder reaches it before the `)` has closed — a parser ordering gap, not a missing meaning |
| Two singletons of different rank conforming (`(1 1⍴5)+,3`) | `singleton-rank` | 2 | 🔴 the higher rank wins there, the first argument here |
| The `¯7○` branch cut | `circle-branch` | 1 | 🔴 the conjugate branch there |
| `⍺←` with a FUNCTION as the default left argument | `function-default` | 1 | 🔴 libjay takes an array alone |
| A dfn that falls off its end | `no-result` | 1 | 🔴 no result at all there, a value here |
| `⌺` over an empty | `stencil-empty` | 1 | 🔴 answered here, refused there |
| An axis Dyalog refuses: a SCALAR spread under `f[K]` (`1+[1]M`), and `⍪[K]`, which takes no axis there | `axis-strict` | 2 | 🔴 the preset is the more permissive of the two; each needs a rule of its own |
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
`f⍣¯n` runs the inverse, and an answer that came from an assignment is
shy.

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
| Non-standard extensions (`LIBJAY_{LANG}_*`, `Dialect::extensions`, `extensions=`, `--extension`, `jay_compile_ext`) | 🟢 opt-in departures from what the references answer, never on unless named and never recorded against the corpus; one flag so far, `j_unicode_strings`. Not dialect settings — see [extensions.md](extensions.md) |
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
