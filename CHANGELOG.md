# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [semantic versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- The hypergeometric series reads the TERM COUNT before it refuses an
  infinity: a count that stops before the term after it leaves the
  infinity the sum reached, so `2 (2 H. 2) __` is `__` where
  `3 (2 H. 2) __` alternates into `_ + __` and has no value. A NaN the
  arithmetic itself made is refused at any count.
- `_2 x:` is the OBVERSE of form 2 — `%/"1`, the alternating quotient over
  the last axis — where it used to return its argument untouched. It puts
  back together what form 2 split apart and keeps going over a longer axis,
  so `_2 x: (1 2 3)` is 1.5 and `_2 x: (1 2 3x)` the exact `3r2`. The
  division is the ordinary one, so a complex pair divides as complex; an
  axis with nothing along it folds to the identity 1 and an axis of one item
  is that item, never divided.

- `%.` keeps the EXACT types wherever the answer is exact, in the rational
  type: a vector's pseudo-inverse is `y % (+/ y*y)` and a square matrix is
  Gauss-Jordan over the rationals, so `%. (2 2 $ 1 2 3 4x)` is
  `_2 1 / 3r2 _1r2`. A SCALAR under it is a reciprocal and no matrix at all
  (`%. 0` is `_` where `%. (1 1 $ 0)` is refused). A scalar right-hand side
  is the whole column of it, a vector one is a column whose unknown is a
  single number, and a system with NO ROWS constrains nothing and answers
  the zero `0 % 0` gives.

- There is always a next prime, so `4 p:` answers one past the machine word
  in the extended integers rather than refusing:
  `4 p: 9223372036854775806` is 9223372036854775837.

- Round 7B's numeric rules, measured against jconsole. `x p: y` over a y
  that is NOT WHOLE, which only the two forms that factorise refuse: every
  other form asks where y sits among the primes and reads any real, so
  nothing fractional is prime, the count below y is the count below its
  ceiling, and `4 p:` and `_4 p:` step to the primes strictly either side
  (`4 p: 4.0` is 5). An infinity is refused there, except by `4 p:`, which
  answers the smallest prime; a NaN is no prime rather than no answer.
  `x ! x` is 1 at every magnitude, where the whole-number rules used to
  stop at 1e17 and leave the rest to a gamma quotient that overflowed into
  no value at all. The alternating sign of the upper-negation identity is
  the parity of `y - x` read off the DOUBLE, which is what the reference
  has past 2^53. And the logarithm of an EXACT 1 is an exact 0 — the one
  argument whose natural logarithm keeps the extended type.

- Round 7A's grammar rules, measured against jconsole. A CHARACTER CODE is
  read as wide as the type that holds it: the `u:` monad and `4 u:` answer
  the two-byte type, so a code is sixteen bits and a negative one names a
  character near the top of that range, while `9 u:` and `10 u:` read
  thirty-two. The byte forms `4 u:`, `5 u:`, `6 u:` and `7 u:` are
  implemented, `6 u:` reading a byte pair as one character. `x u:` and the
  symbol forms `4 s:` and `5 s:` over an argument with NO ELEMENTS answer
  the empty of the form's own result type rather than objecting to a type
  they never saw, and so does `128!:3`, which is the CRC of no bytes.
- `%.` of an ATOM is the plain reciprocal, exact type and all, so `%. 0` is
  an infinity where `%. (,0)` is a matrix too singular to invert; `x %. y`
  over an atom y is the items' sum over it.
- An explicit modifier's NOUN operand answers to both of its spellings —
  `m` and `u` are one operand written two ways — while a verb operand
  answers only to the verb spelling.
- `x ":` writes BOXED data as it stands and lays a COMPLEX value out by its
  real part.
- `4!:0` and `4!:55` read a box holding nothing as the empty name, which is
  not a name a program could have written; `4!:55` answers 1 for a name and
  0 for what is not one.
- The BINARY REPRESENTATION of an extended number and of a rational:
  `3!:1`, `3!:2` and `3!:3` write and read the reference's own bytes for
  both, one nested magnitude block an item and two for a rational.
- `__:`, the constant verb of the infinity below, which the lexer had read
  as a number and a definition's colon.
- The ranks a verb REPORTS under `u b. 0`: a negative rank leaves a fixed
  number of frame axes, so what it takes of any argument has no bound and
  the reference reports it as infinite — `(2"_1) b. 0` and `,. b. 0` are
  both `_ _ _`. The CATALOGUE's own monad rank is 1, so `{ (i. 2 3)` is
  the two rows boxed rather than the catalogue of the whole table.
- The FETCH `{::` has a left rank of 1, so a path of higher rank is a
  FRAME of paths — `(2 1 $ 0 1) {:: ((1 2);(3 4))` stacks the two boxes
  it opens — and a frame with no cell is the empty the frame spells.
- The hypergeometric series refuses a running total that becomes a NaN of
  the arithmetic's own making, as the reference does: `3 (2 H. 2) _` is
  `_` where `3 (2 H. 2) __` and `3 (2 H. 2) (1j_)` have no value, and a
  NaN the program itself wrote travels through. A parameter list written
  out rather than typed in — `(i. 0) H. 2` — is folded where the sentence
  is parsed instead of being refused as computed.
- A GERUND under the OUTFIXES over an argument with no items answers the
  boxed empty, where the same gerund under the prefixes and under key
  takes the shape a run would have made.
- The numbered symbol forms over an argument with NO ITEMS, which are
  answered whatever its type, since no value of one is read: `5 s: (i. 0)`
  is the boxed empty where `5 s: 5` is a domain error. `2 s:` (the names
  razed into one list) and `3 s:` are implemented besides; `0 s:` and
  `1 s:` dump an interpreter's own table and `6 s:` … `_2 s:` number a
  symbol within it, so those stay named gaps over symbols, and a number
  outside `_2` to `7` is now a domain error rather than a gap.
- The TESSELATING cut takes no gerund. `` (+`,);.3 `` and `` (+`,);._3 ``
  are refused as the reference refuses them, where every other cut form
  cuts with one verb per piece.
- The type and the shape a RANKED scalar dyad answers with when its frame
  has no cell. A fill the verb refused stands in as a BOOLEAN zero, the
  narrowest numeric type there is, so it names no type of its own in the
  answer — `3!:0 ((2 3 $ 1) *"1 _ 0 (0 $ 'a'))` is boolean where integer
  zeros had widened it. A LOGARITHM and a ROOT are not asked again with
  numbers at all: their frame stands alone in the boolean type and no
  cell shape is learnt, so `$ ((2 3 $ 2) ^."1 _ 0 (0 $ 'a'))` is `0`
  where every other scalar dyad answers `0 2 3`.
- The whole of the boolean-function conjunction `m b.`: its ONE-ARGUMENT
  form, which is the same table read with a left argument of zero, so
  `(4 b.) y` is y itself and `(8 b.) y` its negation; the three numbers
  that move bits rather than pair them — 32 rotates y left by x places,
  33 shifts it logically and 34 arithmetically, a negative x moving the
  other way; and the numbers that name no function, which the reference
  tells apart — `_1 b.` down to `_16 b.` refuses where a value is read,
  so an empty argument leaves the empty, and anything outside `_16` to
  `34` is out of range where the verb is made. The domain refusal of a
  table below 16 — its two arguments must be bits — now survives an
  empty frame, so `2 (0 b.) (i. 0)` refuses where `1 (0 b.) (i. 0)` is
  the empty.
- Round 6B's structural rules, measured grid by grid against jconsole. The
  identity of an INDEXING fold — `({)/` and `(C.)/` answer `i. #` of the
  cell, which they had no identity for before. The type a BOOLEAN fold
  keeps: `*/`, `<./` and `>./` over booleans stay boolean where a sum or a
  difference widens. Which radix kinds DECODE will read, and the type of
  the zero a radix list with nothing in it answers with. The order two
  empty sides settle a JOIN's type by, now followed by `,:`, `,.` and a
  ranked `,"n` as it already was by `,`, with an empty side naming no type
  even where the two would have promoted. The depth rule of a FETCH path:
  `{::` descends only through boxes, so a value no box held takes another
  step only while it is an atom. The type of an empty frame under a RANKED
  dyad, which is what the NUMBERS made of the fill cells, a side that holds
  a value keeping its own type. And the rank of the obverse of a
  COMPOSITION, which is the rank of the obverse it composes, not the one
  the original carried — `(|.@%:)^:_1 (1 2 3)` is `9 4 1`.

- The TYPE an empty answer carries, monadic and dyadic, measured against
  the oracle cell by cell: nothing ran, so the verb and the types the
  arguments were written in are all there is to settle it. A non-numeric
  empty reads as a boolean one, and a dyad with a non-numeric side reads
  both sides that way.
- J's FOLD family, `u F.. v`, `u F.: v`, `u F:. v` and `u F:: v`: v folds
  the items of y into a running value, the item on its left, and u is
  applied to each running value. The first inflection says whether the
  answer is the last result or every one framed, the second which end the
  items are taken from, and a left argument is where the running value
  starts. `F.` and `F:`, whose step count a test decides, stay named: each
  is unbounded.
- `%.` over COMPLEX data, solved as the real system of twice the size, and
  over an argument with no elements, which answers the empty its inverse
  would have had.

- The polynomial family's EMPTY FRAME and its types. `p.` and `p..` used
  monadically leave a frame whose fill cell they refuse standing alone in
  the boolean type; the dyad refuses its left argument, and a symbol on the
  right, before the frame, and holds the type a value would have had —
  `(1 2 3x) p. (0 $ 'a')` extended, `(1 2 3) p. (0 $ 0j1)` complex,
  `(0 $ 1r2) p. (0 $ 0)` an integer. A polynomial written with a complex
  number keeps the complex type, an empty polynomial integrates to its
  constant term alone in that term's own type, and `n p.. ^:_1` is the
  derivative.
- The J foreigns that only compute. `3!:1` writes an array as the bytes
  that stand for it and `3!:2` reads them back — booleans, literals,
  integers, floats, complex numbers and boxes nested to any depth — with
  `3!:3` giving the same bytes as hexadecimal, a word to a row. `x 3!:4 y`
  and `x 3!:5 y` write whole numbers and floating-point numbers as bytes at
  the widths J gives (`1` two bytes, `2` four, `3` eight, `4` four
  unsigned) and the negative of each reads them back.
- J's `4!:` name table: `4!:0` says what class each boxed name has — 0 a
  noun, 1 an adverb, 2 a conjunction, 3 a verb, `_1` a name with no meaning
  yet, `_2` text that is no name — `4!:1` lists the names of the classes
  asked for, sorted, and `4!:55` erases them. A name has one class at a
  time: `n =: 5` then `n =: +/` leaves `n` a verb.
- J's `5!:` representations. `5!:2` draws what a name stands for as the
  words it is spelled with, one box a part; `5!:5` writes it back as J
  source with a bracket only where one is needed (`+/ % #`); and `5!:6`
  brackets every part that is more than one word (`(+/) % #`). `5!:0` is
  the adverb that reads an atomic representation back: `(5!:1 <'f') 5!:0`
  is `f` again. A name with no meaning yet is its own representation, as
  the reference answers.
- J's `8!:` formats — `8!:0` a box an atom, `8!:1` a box a column, `8!:2` a
  character array — which spell a number for the world outside J: `-1.5`
  where J writes `_1.5`, columns padded to one width, and a literal
  `width.decimals` on the left setting the field, with `*` where the width
  is too narrow.
- J's `9!:10` and `9!:11` (print precision) and `9!:18` and `9!:19`
  (comparison tolerance), read and written. A setting takes effect on every
  sentence after it in the same program: `9!:11 ] 3` then `% 3` shows
  `0.333`, and `9!:19 ] 0` makes every comparison exact.
- J's `128!:3`, the CRC-32 of a byte string.
- J's `#.` and `#:` over COMPLEX numbers. `#. 3j4 1j_1` is `7j7`,
  `2j1 #. 1 2 3` is `10j6`, and each digit of an encode is a complex
  residue, which rounds with the complex floor: `2 #: 5j1` is `_1j1` and
  `#: 3j4` is `0j1 1 1`. The monadic `#:` counts its digits off the largest
  MAGNITUDE in the whole argument.
- A NAMED noun in a fork's left tine. `c =: 5` and then `f =: c + ]` gives
  the fork `5 + ]`, which is the value c held where the fork was written
  and stays that when c is given another one. A tine the program computes —
  a bound `{name}` among them — is read where the fork is applied.
- A VERB named by an indirect locative. `n =: <'cc'` and then `g__n 4` runs
  the `g` of whatever locale `n` holds, chosen where the verb is applied,
  with `z` on the search path answering where the named locale does not.
  Reading and writing a noun that way already worked.
- The capped fork gives back the spelling it was written with. `f =: [: +: *:`
  and then `f` answers `[: +: *:`, not `+:@:*:`, and so do its
  representations: `5!:1` is the three-part train with `[:` in front,
  `5!:2` draws the three tines, `5!:5` and `5!:6` write the cap. A
  `13 : '…'` translation that composes monadically is written the same way
  — `13 : '*: +/ y'` is `[: *: +/`, `13 : 'i. # y'` is `[: i. #` — and the
  cap counts as a train where the translation decides which tine needs a
  bracket, so `13 : '(*: +/ y) + y'` is `] + [: *: +/`. `[: f g` and `f@:g`
  compute the same thing, as they always did.
- The representations of an explicit definition. `5!:1` of `3 : 'y + 1'` is
  the `:` conjunction over the valence and the body, `5!:2` draws the three
  parts, and `5!:5` and `5!:6` write the header form — for a `4 : '…'`, an
  adverb `1 : '…'`, a conjunction `2 : '…'`, a body written on the lines
  below (which comes back as a character matrix, one row a line) and a
  `{{ … }}` alike. A `{{ … }}` DISPLAYS as the words between its braces and
  represents itself as `3 : '…'`, which is what the reference answers.
- A name that holds a modifier answers for the modifier. `m =. /` and then
  `5!:5 <'m'` is `/`; `a =. 1 : 'u y'` and then `5!:1 <'a'` is the `:`
  phrase its body was written as. Displaying such a name works however the
  body was written — inline, on the lines below, or between braces.
- The atomic representation of the verb shapes that had no words for it:
  the inner product `+/ . *`, the ambivalent `u : v`, the shift with a fill
  `|.!.0`, the gerund amend `` +:`*:`]} ``, the under-ravel `+:&.,`, the
  hypergeometric `2 H. 3` and a fit that is a fill, `{.!.9`.
- The linear representation holds a modifier off the word before it where
  running them together would make one word of the two: `2 H.3` and
  `2 3 H.4`, but `+/M.` and `<L:0`. A gerund operand is written as the tie
  its boxes stand for.
- Displaying a definition whose body is written on the LINES BELOW.
  `g =: 3 : 0` over one line of body gives `3 : 'y + 1'` back, indentation
  and doubled quotes included; a longer body comes back under its own
  header with the lines beneath it.

- J's locales — named namespaces for the program's globals. `V_alpha_ =. 5`
  puts `V` in locale `alpha` and `V_alpha_` reads it back; `f_alpha_ =. 3 :
  'y + 100'` defines a verb there, and its body reads the names of ITS OWN
  locale whichever locale the caller is in. `cocurrent 'alpha'` (and
  `coclass`, which does the same) makes a locale current for the sentences
  after it, and a `cocurrent` inside a definition lasts only as long as the
  call. `V__` and `V_base_` are the same name as `V` in the base locale, a
  locative on the left of an assignment writes a global wherever it stands,
  and `V__n` reads and writes the locale the name `n` holds. A name not
  found in its own locale is looked for in the locales on that locale's
  search path, `z` by default.
- The `18!:` locale foreigns: `18!:0` says whether a locale exists and
  whether it is numbered, `18!:1` lists the ones alive, `18!:2` reads a
  search path and `x 18!:2 y` writes one, `18!:3` makes a locale — an empty
  argument hands out a numbered one — `18!:5` names the current locale, and
  `18!:55` destroys a numbered one.
- J's branch words `goto_name.` and `label_name.`. A branch lands on the
  body line its label stands on, forwards or backwards, and out of an `if.`,
  a `for.`, a `while.`, a `select.` or a `try.` block. A `goto_` with no
  label, a label written twice, and a label written inside a control
  structure are all refused where the definition is written, with a message
  that says which.
- J's `throw.` and `catcht.`. A `throw.` leaves the definition it stands in
  at once and is caught by a `catcht.` block in a CALLER's `try.` — never by
  one in the same definition, and never by a `catch.`, which answers only
  for the language's own errors. A `try.` may carry both rescue blocks, in
  either order, and a throw nothing catches stops the program and says so.
- J's tacit definition, `13 : '…'`. It reads an explicit body and answers
  the tacit verb that computes the same thing: `13 : '(+/y) % #y'` is
  `+/ % #`, `13 : 'y+1'` is `1 + ]`, `13 : 'x - y - 1'` is `[ - 1 -~ ]`,
  and `13 : '3'` is `3:`. A body the translation cannot reach becomes the
  ordinary `3 : '…'` or `4 : '…'`, which is what J itself falls back to,
  and the verb computes the same thing either way.
- A sentence that is a verb or a modifier displays it. `mean =. +/ % #` and
  then `mean` on a line of its own answers `+/ % #`; `m =. /` and then `m`
  answers `/`; `f =. 3 : 'y + 1'` and then `f` gives the definition's own
  text back. Brackets appear only where the spelling would otherwise read
  as something else.
- `'a b' =. 1 2` names several nouns at once. The value's items are shared
  out along its leading axis whatever its rank, a scalar goes to every
  name, a boxed item is opened, and one name on the left takes the whole
  value. `=:` names globals the same way, and a mismatched count is a
  length error naming both numbers.
- `{{)n`, the direct-definition spelling of `0 : 0`. The lines below it are
  its text: `b =. {{)n` … `}}` gives `b` those lines with their newlines,
  the `}}` that ends it starts a line, and whatever follows it there
  belongs to the sentence again.
- A modifier whose body derives the modifier again — J's way of writing a
  recursive one. `pw =. 2 : 'if. n = 0 do. ] else. u @ (u pw (n-1)) end.'`
  and then `(+: pw 3) 1` is 8: the condition is settled where the modifier
  is derived, so the recursion stops at its base case. A body with no base
  case says so instead of running out of stack.
- J's `!.` gives a FILL to every verb the language gives one to, not only
  to the shift. `5 {. !.9 ] 1 2 3` is `1 2 3 9 9`, `(2 2) $!.9 ] 1 2 3` is
  `1 2` over `3 9`, `'ab' ,:!.'z' 'cde'` pads the short row with `z`, and
  `>` and `;` pad the pieces they frame the same way. A fill of a wider
  kind widens the answer, so `5 {.!.9.5 ] 1 2 3` is a list of floats; a
  fill of another kind entirely is refused, and so is a fit on a verb that
  takes none.
- A cut takes J's per-axis frets. A BOXED left argument holds one list of
  frets per leading axis and leaves the rest whole, so
  `` ((<1 0 1),(<1 0 0)) <;.1 i.3 3 `` cuts a table both ways at once. A
  negative block size now works with the movement row left out too: the
  block runs to the end of its axis and comes back reversed.
- `u"v` — a verb on the right of the rank conjunction lends its own three
  ranks, so `<"(+/)` boxes the whole argument and `<"(<"1)` boxes each row.
- `%.` divides a right-hand side of rank 3 or more instead of refusing it:
  it is solved whole, one column per element of an item, and the answer
  keeps the item's own axes.
- The byte-oriented `u:` conversions: `1 u:` keeps a codepoint modulo 256,
  `2 u:` widens, `8 u:` packs a codepoint into its UTF-8 bytes and `9 u:`
  reads those bytes back.
- `$.` takes the boxed forms that respecify storage: `(2;a)` stores the
  same value under other sparse axes, `(3;e)` gives it another sparse
  element, and `(2 2;a)` says how many cells other axes would store. A
  stored cell with axes of its own is now drawn as the array of all the
  cells is.
- The gerund amend's monadic valence. `` u`v`w} y `` amends nothing: v
  gives the indices, w the array they index, and the answer is the
  selection — `` (+:)`(1&{)`]} i.5 `` is 1.

- J's bond reads a noun the program computes. `mp =. +/ . *` then
  `(m & mp) ^: 9 m` raises the Fibonacci matrix to a power, and
  `(%&(}: c) - 1:) }. c` turns a series of closes into returns; both used
  to say "bonds over a non-literal noun is not supported yet". The noun is
  read where the derived verb is APPLIED, as a `^:` count is, so a
  definition's own argument may decide it and a name reassigned between two
  applications is read afresh at each.
- J's amend reads indices the program computes. `j =. 2 * i. 3` then
  `9 j } i. 6` amends at whatever `j` holds, which is how a sieve crosses
  off a stride. A computed GERUND amend (`` u`v`w} `` reached through a
  name) stays a named gap and says so: which three verbs the gerund holds
  decides how the amend parses.
- A program-scale corpus. `corpus/j/programs.txt` and
  `corpus/apl/programs.txt` hold whole programs rather than sentences —
  several assignments feeding one another, a defined verb or two, control
  flow where the language has it, and a final value small enough to read —
  recorded against jconsole and GNU APL like every other theme. A sieve
  written both as a loop and as a table of products, a discrete Fourier
  transform with its inverse, an OHLCV pipeline (returns, moving average,
  running peak, drawdown, volume-weighted price), text statistics through
  J's key adverb and APL's partitioned enclose, Game of Life, sorting and
  ranking, base-conversion round trips, run-length coding, a regression by
  inner product, matrix powers, and a Collatz loop with a branch inside it.
  The two files pose the same problems in the two languages and answer
  alike wherever the answer does not turn on the index origin.
- A usage stress suite: `crates/libjay/tests/stress.rs`,
  `crates/libjay-capi/tests/stress.rs` and `python/tests/test_stress.py`.
  Hundreds of compile-and-run cycles with refusals interleaved, resident
  memory held to a growth RATIO rather than a megabyte figure, a
  `LIBJAY_THREADS` 1/2/4 sweep run as child processes (the pool size is
  frozen per process) that must agree exactly, one compiled program shared
  by eight threads, and every kind of refusal in a loop with the good
  programs still answering after each round. No interpreter, no new
  dependency, seconds to run.
- The two infinite levels of `L:` and `S:`. `u L:_ y` is u applied to the
  whole argument, however deeply it is boxed — there is nothing to descend
  into and nothing to collect — so `# L:_ (1;2;<3 4)` is 3 where `# L:0` is
  `1;1;2`; `L:__` is the leaves, which is level 0 written the other way
  round. Both valences, both spellings.
- A GERUND under the cut and under the rank conjunction, one verb per
  piece: `` (+:`*:);.1 ] 1 2 3 `` is `2 4 6`, and `` (+:`*:)"1 ] 2 3$i.6 ``
  doubles the first row and squares the second. A boxed left operand of `"`
  is still the constant verb `m"n` where it holds ONE box, or where the
  rank is infinite in all three places; the dyad has no meaning, as it has
  none in the reference. The dyadic infix and outfix hand their verbs out
  per window too, which they did not before.

### Changed

- **The prime count is computed sublinearly**, not by testing every
  candidate, which is what lets `p:^:_1` and `_1 p:` reach the reference's
  bound of 2^31 at all — there are 105097564 primes below it. libjay
  carries how many values survive each prefix of the sieve for the roughly
  `2*sqrt(n)` distinct values of `n/i`: about `n^(3/4)` steps and
  `sqrt(n)` words, and no prime is ever listed. Past that bound it is a
  limit error, as the reference has. Primality itself is now the strong
  probable-prime test over the first twelve prime bases — a proof for every
  value a machine word holds — so `1 p:`, `4 p:` and `_4 p:` cost
  logarithms where they used to cost a square root.

- How a verb writes itself back out: a HOOK's right tine is always
  bracketed, only a train that already counts out odd absorbs an odd one in
  its last place, a constant verb is never taken apart, and `{::` and a
  constant verb of a negative atom are held off the conjunction that took
  them.
- **A sweep survives the sentence that kills the runner.** `fuzz --compare`
  measures in a WORKER process and reports from a journal the worker
  appends to as it goes: every sentence is announced before it is measured
  and its result written after. A sentence that takes the process down —
  one asking for an array of two thousand million items, which no
  `catch_unwind` can hold — no longer ends a sweep with no output. The
  supervisor names what the worker had in flight, marks it `runner-died`,
  and carries on; a worker that writes nothing for `LIBJAY_SWEEP_STALL`
  seconds (600 by default) is killed the same way. `--journal FILE` keeps
  the journal, so an interrupted sweep resumes; `--no-supervise` measures
  in the one process as before.
- A `~ ` line under a row of `divergences.txt` is a FAMILY RULE: the row
  stands for a class of sentences rather than one, named by the cause
  classes it covers, the verbs it is about, what else the sentence may
  name, and the class of the two answers. It is the third way the accepted
  list excuses a mismatch, after the sentence and the cause signature, and
  a sweep reports the three apart. It is what pins an arithmetic family
  whose every spelling is new — a GCD of two values with no common measure,
  the obverse of a factorial — where the sentence never repeats and the
  cause signature is the signature of every arithmetic difference there is.
- A RECORDING waits 60 s on an interpreter where a SWEEP waits 20, and
  gives a run the limit cut short one more chance, so that the gate does
  not turn on what else the machine was doing.
- `jay-corpus fuzz --compare` now reports TWO agreement numbers. It measures
  every line of `corpus/<lang>/divergences.txt` against the oracle before it
  starts, and a mismatch that matches one — by the minimised sentence, or by
  the cause signature — is counted under `accepted` rather than against
  agreement, kept out of the signature ranking, and printed with the row
  that excused it. Raw and accepted-adjusted agreement are both printed;
  `--no-accepted` turns the list off.
- A float takes an exponent as soon as its exponent reaches the print
  precision, which is what jconsole does: `1234567.5` is `1.23457e6` at six
  significant digits and `1234567.5` at nine, and `o. 1e8` is `3.14159e8`
  rather than the `314159000` that showed padding zeros as though they were
  measured. The small end is fixed at `1e_5`. APL keeps the thresholds it
  had.
- The trigonometric circle functions now refuse an angle of `π × 2^27.5` or
  more, which is where jconsole's limit error begins. `^` of a complex
  number and `r.` are held to it by the exponent's imaginary part; the
  hyperbolic pair is not.
- `x u;.0 y` clamps its rectangle to the axis rather than refusing it:
  `5 <;.0 (1 2 3)` is the whole of it, `(_1 ,: 3) <;.0 (1 2 3 4 5)` is
  `3 4 5`, and only an origin that is no position in the axis is an index
  error.
- `jay-corpus fuzz --compare --signature` now cuts every mismatch down to
  the smallest sentence that still parts libjay from the oracle the same
  way, and reports and signs THAT sentence — a composed sentence names eight
  or ten primitives that are a property of the draw, so signing it made one
  cause into one finding per subset it could be drawn inside. The signature
  also names how the two sides parted, and at most three primitives. A
  wrapper that deduplicates on it now converges instead of growing without
  end.

### Fixed

- **Round 6A, second pass.** `#: y` took its width by halving a double and
  refused anything past 1e15, so a list holding both a huge whole number
  and a fractional one — what an under of `#.` makes of a polar pair — was
  refused where every part of it answers on its own; the width is now read
  off the largest magnitude's exponent and a whole value keeps every bit of
  its own even on the float path. The ZEROTH ROOT is the limit `y ^ _`:
  zero under a magnitude of one, one at 1, an infinity above it, and no
  value where a NEGATIVE magnitude reaches one. An extremum reads a
  complex value as the real it TOLERANTLY equals (`2 >. 1j1e_15` is 2),
  and a logical verb reads a value tolerantly equal to 0 or 1 as that
  value (`(1.00000000000005) *: 1` is 0). An infinite MODULUS leaves a
  value of its own sign alone and sends every other one — a NaN included —
  to that infinity, and a real-valued complex pair follows the same rule.
  A NaN in either part of a complex number takes the whole root with it.
- **Round 6A of the agreement sweep, the numeric singletons.** The BINOMIAL
  at an infinity: a whole left argument makes `x ! y` a polynomial in y of
  that degree, so at either infinity it takes the leading term's value and
  the sign alternates with x (`1 ! __` is `__` and `2 ! __` is `_`), while
  a fractional one makes a ratio of gammas whose limit is a number in
  neither direction and is refused. Over a NEGATIVE WHOLE y under a
  fractional x, Γ(y+1) sits on a pole and nothing below it does, so the
  answer is an infinity — the pole's own sign, alternating with its index,
  times the sign of the finite half — where libjay worked the gammas out
  and read the pole as a large finite number. AN EXPONENTIAL WHOSE
  MAGNITUDE HAS UNDERFLOWED IS ZERO WHATEVER ITS ANGLE, so the circle
  functions' angle limit is never reached: `1 r. (1.7e9j1000)` is 0 where
  `1 r. (1.7e9j100)` is a limit error, which is what let `*. ^:_1` answer
  over a cell of more than two items. The identity a SCAN'S OBVERSE shifts
  into the vacated place is a whole number rather than a float, so
  `*/\ ^:_1 (1r2 1r3)` comes back rational.
- **Round 5B of the agreement sweep, the numeric residue.** The types
  arithmetic answers in: A REAL POWER LEAVES THE INTEGERS (`3!:0 (3^3)` is
  the float type, only the exponents 0 and 1 keeping an integer base
  integral); A SIGN NEEDS NO FLOAT and a boolean argument stays boolean
  under every monad whose answer cannot leave {0, 1}; AN EMPTY ARITHMETIC
  ANSWER TAKES BOTH SIDES' TYPES rather than the first numeric one; a
  non-numeric empty answers as a boolean empty would, verb by verb. `#:`
  keeps every bit of a whole number past the machine word — `#: 1e300` is
  997 digits — and writes them in the argument's type narrowed one step,
  the boundary between an integer and a float digit being exactly 9e15. A
  COMPLEX MAGNITUDE BELOW THE TOLERANCE IS ZERO and its signum with it; an
  EXTREMUM reads real-valued complex data as the reals it holds wherever it
  arrives from; TWO INFINITE PARTS NAME NO DIRECTION, so `%: _j_` and `_j_
  ^ 0.5` are `_.j_.`. An exact factorial that would exhaust the machine is
  refused rather than widened to a float infinity. A RANK CONJUNCTION WHOSE
  RANKS ARE THE VERB'S OWN is not written out: `#."1 1` spells itself back
  as `#.`.
- The MAP `{::` is its own fixed point: the coordinate of a boxed scalar is
  a BOXED empty and an argument with no box in it is one leaf whose path is
  a BOOLEAN empty, so `{:: {:: y` is `{:: y`.
- A rank AT OR ABOVE the verb's own frames nothing the verb would not frame
  itself, and a frame the rank made is never asked again with numbers where
  its fill cell was refused.
- Types at full size that the empty probes turned up: a square, a
  factorial, a floor, a ceiling and a sign keep a BOOLEAN boolean; a sign
  over floats answers in INTEGERS; base-2 digits are BOOLEAN; a roll over a
  boolean argument is FLOAT.
- An empty cycle specification moves nothing and leaves an atom an atom; a
  root reads its exponent's type before an empty frame; the polynomial dyad
  reads a one-box root form as the roots, the monad only as the sparse
  table; the obverse of a running or an outfix fold asks a scalar to be a
  number; two boxed sides of `u L: n` pair by leading-prefix agreement; a
  boxed empty above rank 2 draws its plane separators as lines of its own.
- Two EMPTIES catenated take the type the reference's own order gives them:
  a boolean loses to everything, then a character, a whole number, a box,
  an extended number, a rational, a float, a complex number, and a symbol
  last. It is not the promotion two numbers with something in them take —
  `(0 $ 1x) , (0 $ 1.5)` is a float empty.
- The identity of a CATENATION keeps the cell's remaining axes, so
  `$ ,/ (0 3 4 $ 0)` is `0 4` and every scan and cut built on `,/` over an
  argument with no item follows.

- **Round 5C of the agreement sweep: what the generator could not reach.**
  The J fuzz grammar is generation 3, and reaches the whole conjunction
  layer for the first time — the reflex and passive `~`, explicit
  definitions in every spelling and the control words in them, gerunds and
  the modifiers that hand them out, `@.` `:.` `::` `M.` `f.` `b.` `H.`, the
  inner product, a verb's ranks and a verb's power count, names, the fold
  family, the conversion verbs, the sandbox-safe foreigns and the literals
  the pools had no spelling for. What it found: `u@.n` reads its index the
  way `{` reads one, so a negative counts back from the end of the gerund
  and a LIST of indices is the train the verbs it picks spell; `u^:v` means
  by its computed count what the same count written as a literal means;
  a `4!:` or `5!:` foreign asked about no name answers an empty rather than
  reading the empty argument's type; `}. y` of an ATOM is the empty list of
  the atom's own type; `x (m H. n) y` reads every element of y rather than
  its first, and a term count of zero reads none of it; `":` reads a
  complex specification as J's `wjd` and refuses a fractional one; `3 p: y`
  is `q: y` written the other way and pads each row with 1s rather than
  framing per element with a zero. A verb assignment used as a VALUE inside
  a larger sentence is now a named gap where it was a syntax error.
- **Round 4 of the agreement sweep, everything but the polynomial verbs.**
  A rank and a level SPELL THEMSELVES BACK as they were written: `u"2 _ 2`
  and `u"_ 2` are one verb, and the reference writes each out the way it
  came. A FILL is examined only where an atom of it is placed, so `!.` on a
  reshape or a catenation that never fills is no complaint about its kind,
  while a take and a drop keep their eager check. The two verbs that make a
  complex number settle their domain from the right argument alone, and
  interval index compares nothing where it has no bound. A NaN the program
  handed in travels through a GCD and an LCM where an infinity is refused,
  and over the complex numbers an infinite part is refused in the divisor,
  the multiple and the residue alike. Opening an array with no box in it is
  no change AT ANY RANK, `a:` holds a BOOLEAN empty, and an infinite
  magnitude to a negative real power is zero. A reshape length written `_`
  asks for as many as make the total come out exactly. An empty takes the
  other side's type from the arguments as given. One numeric word is read
  at ONE PRECISION, so an `x` suffix forbids a float spelling beside it. A
  radix held in a wide numeric type reads boxes. Format by specification
  spells the infinities the way the plain format does.
- The recursion guard stops explicit definitions at 48 levels rather than
  64: one level costs about 34 kB of stack in an unoptimised build, and the
  old number reached the end of a 2 MiB thread stack before the guard fired.
- J's exact types carry the two INFINITIES. `% 0x` is `_` and reports the
  rational type, `(3 {. 123x) ^ _2` is one rational array holding a
  fraction beside two infinities, and `x: _` is `_` rather than a refusal.
  An infinity swallows a finite addend, two opposite ones cancel to
  nothing, an infinity times a zero is a zero, and a zero over a zero is a
  zero; anything with no value at all widens the pass to floats, where J's
  NaN rules take over. That closes the last row of the exact-storage
  family.
- An exact base raised to the ATOM 0.5 is the exact square root: `4x ^ 0.5`
  is the extended 2 and `(1r4) ^ 0.5` is `1r2`. `^ 0.5` reads as `%:`
  throughout, which is why a BOOLEAN base keeps its type under both.
- The dyadic outfix's eager folds are two rules: a sum types its whole
  argument wherever the fold is asked for anything, the other four only
  where some piece really holds an item. `3 <./\. (1;2;3)` is the identity
  rather than a domain error, and six divergence rows close with it.
- `x ! y` takes the SHORT side of its symmetry where the two arguments are
  close: `4503599627370496 ! 4503599627370497` is y itself, not the 6.2e27
  a gamma quotient of two huge factorials cancels to.
- `x C. y` refuses a left argument that is no permutation of y's items —
  the Dictionary does not define one, and the reference's answers for one
  follow no rule — with a diagnostic naming the value that spoils it.
- Which type refusals survive an EMPTY frame is now the reference's own
  table: `^.`, `j.` and `r.` settle their domain from the whole argument,
  `%:` keeps a character's refusal on either side and a box's only on the
  left, and every other arithmetic verb answers the empty. Three
  divergence rows close with it.
- J's logarithm divides the way J's `%` does, so `1 ^. 1` is 0; a complex
  quotient of an infinite part over a zero has no value, which makes
  `1 ^. __` a NaN error where `2 ^. __` keeps its `_j4.53236`; and an
  infinite magnitude along an axis stays on it, so `_ ^ 0.5` is `_`.
- A NEGATIVE zero divisor turns the infinity over, in the fused kernel and
  the shader as well as the plain path, so `% ^:_2 (_ __ 0)` is `_ __ 0`.
- A polynomial's boxed root form is a boxed scalar or a two-box list, and a
  two-column TABLE where the roots go spells the polynomial sparsely, one
  term per row as `coefficient, exponent`; a sparse form written in the
  exact types stays exact through the derivative and the integral. A
  polynomial value the arithmetic could not make is no value.
- A tessellation of complete blocks moves one place at a time even where
  the block holds no items: `$ 0 <;._3 (i. 5)` is 5.
- `u&.>` boxes every answer it makes, so an argument with no atom to open
  still leaves boxed data; `x A. y` over an atom y answers y itself where x
  names no permutation at all.
- A boxed array's row heights span the whole array as its column widths do,
  which lines up the rules of a rank-3 answer plane by plane.
- `I.` searches a COMPLEX bound or value by the same total order the grades
  use, and refuses a numeric bound against a boxed value whatever their
  ranks.
- J's `L:` and `S:` read their level operand like a RANK — 1 atom for every
  valence, 2 atoms `left right` with the monadic level taken from the
  right, 3 atoms in full — and read it when the derived verb is APPLIED, so
  `] S:_ 2` is a verb that shows itself rather than a parse error. The
  operand's own faults stay the sentence's: a character, a box or a
  fraction is a domain error, none or more than three atoms a length error,
  a table a rank error. `S:` also SPREADS at a level of `_`, so
  `$ *: S:_ (2 3 $ i. 6)` is `1 2 3`.
- A NOUN operand to J's `@` is the constant verb `n"_`: `*:@_1` answers 1
  and `,@_1&* 2` answers `_1`, where both were named refusals. Only `@`
  takes one, and only on the right.
- The shape a scan, an infix, a cut or a tessellation leaves when it has NO
  piece at all. The verb is asked what one piece would have come to, and
  the piece differs by site: a positive infix asks about a run of x fills,
  a negative one about the empty chunk the argument has left, and a cut or
  a tessellation about the empty piece — `$ _4 +/\ (0 3 $ 'a')` is `0 3`
  and `$ 2 2 ];.3 (i. 0 0 5)` is `0 0 0 0 5`. Where the verb has nothing to
  say about that piece either, the prefixes, the infix and the cut leave a
  list of empty lists (`$ 3 +/\ ''` is `0 0`) and the suffixes, the outfix
  and the tessellation a bare empty.
- J's `;:` has a monadic rank of 1: `;: (2 2 $ 'ab')` is two words, not
  one, and an empty frame keeps the frame it was given.
- A fill whose type the argument does not take now stands where the
  argument holds NOTHING — `2 {.!.'z' (0 $ 2)` is `zz` and
  `2 {.!.2 (0 $ 'a')` is `2 2` — and a non-empty argument still refuses it.
- A NaN the weighing itself made is a NaN error: `#. _ __ 0` refuses, as
  the reference does, where `#. _ 1` is `_`.
- `i.!.1` is accepted and searches at the default tolerance, which is what
  the reference does with the one tolerance above 2^-34 it takes.
- A complex value whose imaginary part is negligible beside its own
  magnitude orders as the real number it tolerantly equals: `2 <: 1j1e_14`
  is 0 and `2 <: (^. _ __ 0)` is `1 1 0`, where `2 <: 1e10j1` still carries
  no order.
- The obverse of `!` — the smallest argument at or above zero whose
  factorial is the value — is no longer a named gap: `!^:_1 ] 6` is 3 and
  `!^:_1 ] 1` is 0.
- The exact arithmetic runs wherever either side is stored exactly: `p..`
  differentiates and integrates there and `#:` encodes there, so
  `p.. (1r2 1r3)` is `1r3`, `1 p.. (1r2 1r3)` is `1 1r2 1r6` and
  `#: 5r2` is `1 1r2`. `+.` keeps an exact argument's storage as it already
  kept a whole one's. Six pinned exact-storage divergences are gone.
- A scan's obverse leaves a SCALAR alone: `+/\ ^:_1 ] 5` is 5, where a
  difference against a shifted-in fill made 0.
- J's `I.` reads the MAJOR CELLS of its left argument and has an infinite
  left rank, so `(2 2$1 2 3 4) I. 2 3` is 1 rather than a framed pair, and
  a right argument with no cell of that shape is a rank error. It also
  SEARCHES rather than counts — bisecting bounds it takes for sorted, the
  direction read off the ends — so `3 1 4 1 5 I. 2` is 0; and whole bounds
  are compared whole, so `9007199254740992 I. 9007199254740993` is 1.
- Rank framing over an EMPTY frame now keeps the fill run's shape
  refusals: `(1 2) +"1 (i. 0 3)` is a length error, as it is in jconsole,
  where it was the empty. A refusal about the fill's own value or type
  still leaves the frame alone, which is what makes `'' { 1 2 3` an empty.
- `p.` takes a first-degree polynomial's root outright, which keeps
  `p. _ 1` at `1 ; __`; a root the arithmetic could not find is a NaN
  error rather than a list of NaNs; and a root form's multiplier is one
  number, so `((1 2);2 3) p. 2` is a rank error.
- A complex sine follows its zero rather than the NaN an overflowed
  hyperbolic cosine makes: `1 o. 0j1e10` is `0j_`.
- Monadic tessellation, `u;.3 y` and `u;._3 y`, which was a named gap: the
  window is a cube of the smallest axis, moved one step at a time along
  every axis.
- `x ^!.n y`, the STOPE, which was refused as a fit `^` does not take: the
  product of `y` terms starting at `x` and stepping by `n`, so `2 ^!.5 3`
  is 168 and `2 ^!.0 3` the ordinary power.
- A descent with no pair to apply its operand to asks it once, of the fills
  the pieces would have been, and keeps a refusal about their shapes:
  `(1 2 3) *. L:0 (0 $ a:)` is a length error, three items against none.
- An empty frame keeps the cells' SHAPE even where the verb has nothing to
  say about the fill's type, so `$ 'a' ,"0 (i. 0)` is `0 2`.
- Euclid's remainder is a rounding artefact only where a subtraction left
  one: `(1 - 1.00000000000001) +. 2` is that difference and was 2.
- `+.` of a whole number answers whole parts, which keeps
  `+. 9223372036854775806` exact.
- `(i. 0) #: 'ab'` is the empty: no radix is no digit, so nothing of the
  value is read and its type never comes up.
- `L. 0$<0` is 0 — an empty of boxes has no level to count — where it was 1.
- `*` and `E.` take a fit, which they ignore where the tolerance means
  nothing: `2 (*!.1e_13) 3` is 6 and `'ab' (E.!.0) 'abc'` is `1 0 0`.
- `j./` and `r./` of an empty are 0, the identity jconsole folds to, where
  they were refused for having none.
- `x ! y` below its diagonal for a large left argument. The answer is zero
  wherever a whole y sits under a whole x, and past the width at which the
  falling factorial is taken the gamma quotient read a pole over a pole as
  no value at all: `100000000 ! 2` raised a NaN error and is 0.
- `5!:1 <'zz'` for a name with no meaning yet. It answers `<'zz'` — the
  name stands for itself — where it used to stop with a value error; text
  that is no name at all, such as `5!:1 <'i.'`, is now refused as an
  ill-formed name, which is what J calls it.
- `x #: y` over whole numbers past 2⁵². `2 #: 4503599627370497` is 1 and
  was 0, and `#: 4503599627370497` is 53 digits rather than a refusal: the
  digits are taken in exact integers where every radix and every value is a
  whole number, and the float path — which is where the tolerance lives —
  keeps everything else.
- `x ! y` for a large whole x with a negative whole or a fractional y.
  `100000000 ! _2` is 100000001, `5000 ! 2.5` is `_1.19787e_13` and
  `1e10 ! 2.5` is `_1.05786e_35`; each used to say the expression had no
  value, because the gamma quotient behind it leaves the double range. The
  quotient is now read in logarithms, and a negative whole y through the
  upper negation, which stays exact.
- An EMPTY of BOXES is acceptable numeric data, as an empty of characters
  already was: `#. 0$<1` is 0, `i. 0$<1` is 0, `A. 0$<1` is 0 and
  `$ #: 0$<1` is `0 0`.
- `;` (raze) over boxes whose contents have unequal rank. An opened value
  whose items have fewer axes than the others is ONE item of the common
  shape, not several of a smaller one, so `; ('ab';(2 2$'wxyz'))` is three
  rows and was four.
- A sequential machine's map is refused beside a numeric argument, whose
  values are its classes already, and must have one entry per byte — which
  is what the reference does, where libjay used to name a gap.
- The least common multiple of two EXACT numbers keeps its sign. `_5x *. 2`
  is `_10` in J and `¯5∧2` is `¯10` in APL; both answered `10`. The
  machine-integer path was always right; the extended and rational one took
  the positive LCM of the numerators.
- J's `I.` (interval index) compares exactly. It is the one comparison in J
  that does not consult the comparison tolerance:
  `1 2 3 I. 1.9999999999999998` is 1, the answer flipping at the bound
  itself. APL's `⍸` does consult `⎕CT` and is unchanged.
- An EMPTY axis list on `,` and `⍪` adds an axis of length one at the end
  each glyph works from — after the last for `,`, before the first for `⍪`.
  `⍴⍪[⍳0]2 4 6` is `1 3`. It is also the one axis a scalar takes:
  `⍴,[⍳0]5` is `1` where `⍴,[1]5` is the empty.

## 0.4.0 — 2026-08-29

### Added

- Operands the program computes. A `⍣` or `^:` count, an `f[K]` axis and an
  array standing where a function operand belongs may now be a name or an
  expression instead of a literal, and are read where the derived function
  is applied rather than while the program compiles. `N←3 ⋄ ⌽⍣N⊢1 2 3`
  reverses three times; `K←1 ⋄ +/[K]M` sums the columns; `n =. 3` then
  `>:^:n ] 0` is 3 in J. Because the operand is read at every application,
  a definition's own argument may decide it: `∇Z←S K ⋄ Z←+/[K]B ∇` sums
  whichever axis the caller asks for. The operand is one operand — `⌽⍣N+1`
  reads the count `N` and leaves the `+1` to the sentence, as the
  references do.
- An axis a definition reads for itself. A `∇` header may name one
  (`∇Z←SUM[X] B ⋄ Z←+/[X]B ∇`, called as `SUM[2] M`), and a `{…}` reads it
  as `χ`: `F←{⍵+χ} ⋄ F[10]5` is 15. The value arrives exactly as written,
  with no index-origin adjustment, and belongs to the one call that wrote
  it. A definition whose header names no axis refuses one.
- APL's indexed assignment may stand inside a larger sentence, and its
  value is the value assigned: `B←A[2]←5` gives `B` the 5, `2+A[1]←9` is
  11, and `A[1]←C[2]←9` writes a 9 into both names.
- APL's system names. `⎕AV` is the 256-character atomic vector, so
  `⎕AV⍳'A'` and `⎕AV[66]` answer what a GNU APL session answers. `⎕PP` and
  `⎕RL` can be READ and SET while a program runs: `⎕PP←3 ⋄ ÷3` shows
  `0.333`, and the answer a program hands back is displayed at the
  precision the program asked for, in Python and through the C ABI as well
  as in the corpus. `⎕RL←42` starts libjay's random stream from that seed,
  so the same seed rolls the same numbers — the seed is reproducible, the
  sequence is libjay's own and matches no other implementation's, and a
  run's seed does not reach the next run. `⎕LX` reads as the empty vector
  (libjay loads no workspace for a latent expression to be latent for), and
  `⎕ET` and `⎕EM` are the values that mean "no error yet", which is what
  every program libjay can run reads.
- `⎕NC` — what a name holds now: `¯1` not a name at all, `0` a name with
  nothing in it, `2` a variable, `3` a defined function, `5` a system
  variable, `6` an argument of a `{…}`. A character vector asks about one
  name and a character matrix about one per row.
- `⎕CR` — a definition's own text. `⎕CR 'F'` gives back the lines `F` was
  written as, header first, as a character matrix padded to the longest of
  them, whether `F` came from a `∇ … ∇` or from `⎕FX`. The dyadic form
  answers the numbered conversions that rewrite the same bytes another way:
  `5 ⎕CR` and `6 ⎕CR` write them as hexadecimal in either case and
  `13 ⎕CR` reads it back, `16 ⎕CR` and `17 ⎕CR` are base 64, and `18 ⎕CR`
  and `19 ⎕CR` are UTF-8 both ways.
- APL's polynomial arithmetic, `⌹[8]` and `⌹[9]`. The bracket after `⌹`
  picks a function rather than an axis, and its number is the number
  written whatever `⎕IO` is: `1 2 ⌹[8] 1 1` is `1 3 2`, the product of
  `1+2x` and `1+x` as coefficients lowest power first, and `⌹[9]` divides
  two polynomials and answers the quotient and the remainder.
- APL's axis specification `f[K]` for every function that reads one. The
  brackets after a function say which axis it works along, and may name
  several where the function takes several:

  - `,[K]` and `⍪[K]` run neighbouring axes together — `,[1 2]` of a
    2×3×4 answers 6×4 — and a fractional `,[K.5]` adds a new axis of
    length one at the gap it names.
  - `x,[k]y` catenates along the named axis, and `x,[k.5]y` LAMINATES:
    the two arguments beside each other along a new axis, so
    `1 2 3,[0.5]4 5 6` is a 2×3 matrix and `[1.5]` a 3×2 one.
  - `x↑[K]y` and `x↓[K]y` take and drop one count per named axis, in the
    order written, leaving every other axis whole.
  - `x⌷[K]y` indexes only the named axes; the rest come through whole.
  - `⊂[K]y` makes the named axes the shape of each item and the rest the
    shape of the answer, and `x⊂[k]y` / `x⊆[k]y` partition the named axis
    in place.
  - `↑[K]y` takes one item along each named axis; `⊃[K]y` mixes, placing
    the item axes where K names. Under Dyalog's dialect the two swap and
    `↑[K.5]` puts every item axis at one gap.
  - a SCALAR function takes an axis dyadically: `1 2+[1]2 3⍴⍳6` adds 1 to
    the first row and 2 to the second, and `(2 3⍴1)+[1 2]2 3 4⍴⍳24` lines
    a matrix up with two axes of a three-axis argument.

  What already worked — `f/[k]` `f⌿[k]` `f\[k]` `f⍀[k]`, `⌽[k]` `⊖[k]`,
  and the dyadic `x/[k]y` and `x\[k]y` — is unchanged. An axis outside the
  argument's rank now says which axes there are, and a function that takes
  one whole axis says so rather than reporting a gap.

- `Dialect.axis_order`, one setting for the one place the two APL lines
  read an axis LIST differently: `⌷[K]` and a scalar function's `f[K]`
  pair their axes with what accompanies them ascending by default and in
  the order written under `Dialect.dyalog()`, so `2 1⌷[2 1]M` answers the
  element `2 1⌷[1 2]M` does by default and the one `1 2⌷[1 2]M` does
  there. `↑` `↓` `,` and `⊂` keep the order written under both.

- The gamma function in the complex plane. `!2j1` and `!2J1` answer
  `0.962865j1.339097`, and the binomial follows it, so `2j1!5` and
  `2!3j1` have values too. A number with no imaginary part is still the
  real it displays as, and the real path is untouched.

- APL's `⎕CC` answers its four glyph repertoires: the superscripts (5),
  the subscripts (6), the box-drawing frame (7) and the mathematical
  symbols (9). The two frames are matrices — `⍴⎕CC 7` is `6 10` and
  `⍴⎕CC 9` is `4 7`.

- A `∇` definition may carry a label and a control structure at once. A
  label is the number of its LINE, and a `→` finds the statement that line
  began, so a `→L` loop and an `:If` block live in one definition. A
  branch INTO a control structure has no statement to land on and says so.

### Changed

- The system names libjay does not answer now say which kind of thing they
  are instead of naming a queue position. `⎕SYL` reports one interpreter's
  own build — how many cores it was configured for, how big its hash table
  is — and another implementation has nothing to put in those rows, so it
  is refused as not being in the language. `⎕SVR` joins `⎕SVO` and `⎕SVQ`
  behind the sandbox: it retracts the offer of a shared variable, and
  libjay shares no variable with anything. `⎕PW` is still a promise, with
  the reason given — libjay's display writes a value in full and folds no
  line, so there is no page whose width could be set. And `P.x` reports "a
  structured variable" rather than the inner product it is not.
- APL's scalar functions PERVADE a nested argument: they descend through
  the boxes to the simple values at the bottom, so `1+⊂2 3` is `⊂3 4`,
  `(1 2)(3 4 5)+10` adds ten under both boxes, and `2 3⌈⊂1 5` spreads a
  scalar over items as it does over elements. The descent runs on a work
  stack rather than the call stack, so a value thousands of boxes deep
  answers instead of taking the process down. (The rule itself already
  worked; the depth and the documentation did not.)

- APL's index brackets bind to the value written immediately before them,
  which in a run of numbers is the LAST number: `1 2 3[2]` is `1 2` beside
  `3[2]`, and indexing a scalar is a rank error. `(1 2 3)[2]` is 2 as
  before. This is what the reference does; libjay used to index the whole
  strand.

- APL's `⌹` reads the whole argument, so an argument of rank 3 or more is
  a rank error on either side, as it is in the reference. J's `%.` keeps
  its rank of 2 and still runs over the 2-cells.

- `⍎` of a program that produced no value — the empty program among them —
  is now a VALUE error rather than a domain error, and says so at the `⍎`.

### Fixed

- `⎕FX` with a line that is not literal text — a name, or a parenthesised
  expression — used to fix a definition from the literal lines it had
  read so far and hand the rest back as data, so `T←'Z←N×2' ⋄ ⎕FX 'Z←F N' T`
  quietly defined an empty `F`. It now says the definition is not literal
  text in the program, which is the gap it always was.
- APL's indexed assignment answered the whole amended array where it stood
  in expression position: `F←{A←1 2 3 ⋄ A[⍵]←5} ⋄ 0+F 2` was `1 5 3` and is
  now 5, which is what the value assigned is.

- APL's enlist `∊` raised an internal error wherever an EMPTY leaf of one
  type stood beside a value of another: `∊1 2 ''`, `∊'abc' ⍬`, `∊'' 1 2`,
  `∊(1 2)(0 3⍴'')(3 4)` and `∊''` on its own all failed. An empty leaf has
  no element to convert, so it now takes the result's type outright, the
  way an empty side of a catenation always has; where every leaf is empty
  the answer is an empty of the first leaf's type.

- APL's membership `∊` compared two integers as the double each stands
  for, so any two past 2⋆53 were the same number to it and
  `9007199254740992∊9007199254740993` was 1. Integers are compared as
  integers now; an integer still finds a float of the same value, and `=`,
  `⍳`, `∪` and `~` were already exact.

## 0.3.2 — 2026-08-29

### Added

- Non-standard extensions: opt-in departures from what the reference
  implementations answer, named one by one and off unless asked for. The
  environment sets a process default (`LIBJAY_J_UNICODE_STRINGS=1`), and
  every surface can override it for one compiler — Rust's
  `Dialect { extensions: Some(...) }`, Python's
  `jay.j("...", extensions="j_unicode_strings")` and
  `J.create_compiler(extensions=...)`, the CLI's
  `libjay --extension j_unicode_strings`, and the C ABI's
  `jay_compile_ext` — so an embedded libjay is never at the mercy of its
  host process's environment. They are not dialect settings, and
  docs/extensions.md says why and lists what there is.
- APL's bit-wise logical functions, `⊤∧ ⊤∨ ⊤⍲ ⊤⍱ ⊤= ⊤≠`. Each reads its
  arguments as 64-bit two's-complement integers and runs the operation on
  every bit at once, so `12 ⊤∧ 10` is 8 and `12 ⊤≠ 10` is 6. `⊤` and the
  glyph after it are one function, blanks between them included. Three of
  the six have a monad: `⊤∧` and `⊤∨` give the argument as the integer it
  stands for, and `⊤⍱ 5` is ¯6.

- `A⊤[N]B`, which encodes to N copies of the single radix A — `2⊤[4]13` is
  `1 1 0 1` — and `A⊤[0]B`, which works the width out for itself. N counts
  digits and is not an axis, so `⎕IO` does not move it.

- `A⊢[M]B`, APL's selection function: a 1 in the mask takes the element of
  B that stands there, a 0 the element of A, and the three agree by the
  ordinary scalar rule. `'Q'⊢[1 0 1](1 2 3)` is `1 Q 3`.

- `A∘B` between two values is the matrix product. A left vector is read as
  a row and a right one as a column, so the answer is always a matrix; a
  scalar operand makes it the element-wise `×`; and inner lengths that
  differ are padded with zeros rather than refused.

- Hexadecimal literals, `$ff`, in either case of letter. Each is one scalar
  and strands as a decimal literal does: `$10 $20` is `16 32`.

- Double-quoted strings with C escapes, `"a\nb"`. A double-quoted string is
  always a vector where `'Q'` is a scalar, `\a \b \f \n \r \t \v`, `\\`,
  `\"` and `\0` are read, and a backslash before anything else keeps
  itself.

- APL's conditional, `test →→ body ←→ otherwise ←←`. The test is read as
  strictly as a dfn guard's — one 0 or one 1 — and each of the three
  markers may end a line, so a conditional written down a `∇` definition's
  lines works as one written on a single line does.

- `A→B` inside a `∇` definition: branch A lines on from the line it stands
  on when B holds. `1→cond` reaches the next line, `¯1→cond` the one
  before, `0→cond` runs the line again, and a step that leaves the body
  ends the definition.

- `⎕CC`, the numbered character classes: the digits, the two cases of the
  Latin and Greek alphabets, ASCII, the printable range, the octal and
  hexadecimal digits, and the RFC 4648 alphabets. Several numbers give one
  class per item, nested. Four classes are one implementation's own glyph
  repertoire and are refused by name.

- A gerund may be the operand of `/`, `\`, `\.` and `/.` in J, and every
  one of them cycles through its verbs. `` (+`-)/ 1 2 3 `` inserts them
  between the items and folds right to left, so it is `1 + (2 - 3)`; with
  no items at all the answer is the identity element of the verb the fold
  would have reached first. The other three give one verb to each piece in
  turn — `` (+`-)\ 1 2 3 `` applies `+` to the first prefix, `-` to the
  second and `+` to the third, and `` 1 0 1 (+`-)/. 1 2 3 `` does the same
  over the key's groups. Under any other adverb a gerund is still refused
  by name.

- J's `_k q:` for a negative k: the last `|k|` primes that divide y, over
  their exponents, as a two-row table — `_1 q: 2310` is `11` over `1`.
  Asking for more columns than the number has keeps the whole table.

- APL reads the ASCII `^` as `∧`, which is what a program typed on a
  keyboard without an APL layout holds: `4^6` is 12.

- J's constant verb, `m"n`: `"` with a noun on its LEFT ignores both
  arguments and answers that noun, at the rank the right operand gives.
  `('ab'"_) 7` is `ab`, `(3"0) i.2 3` is a table of threes, and `0"_` is
  how a train writes a zero.

- J's noun definition, `0 : 0`: the lines below it, up to a lone `)`, are
  read as text and become the value — one character vector, each line
  followed by a line break. `0 : 'text'` writes the same thing inline.

- J's monad-dyad conjunction, `u : v`: one verb out of two, u its monad and
  v its dyad. `f =: (+/) : (-/)` sums a list and subtracts a pair.

- J's `#` takes a COMPLEX count, which is copies and fills: the real part
  says how many copies of the item to make and the imaginary part how many
  fills to put after them. `1j2 # 'a'` is an `a` and two spaces, and
  `2j1 1j0 # 'ab'` is `aa b` — the form a fixed-width layout is built with.

- J's `}` takes a LIST of index specifications — `7 8 (0 1; 2 0) } 3 3$0`
  amends two cells and takes one replacement for each — and a single value
  now fills a whole item, so `9 (1 1) } 3 3$0` writes 9 across the row it
  names.

- J's gerund amend, `` x u`v`w} y ``: u makes the replacement, v the
  indices and w the array they go into, each reading both arguments.
  `1 3 (+:@:{`[`]}) 9 8 7 6` doubles the items it names in place.

- `$:` may stand in a gerund, which is how a recursion is written with an
  agenda: `` (base`$:)@.test ``.

- APL reads five Unicode look-alikes as the glyphs they stand for — `∣` as
  `|`, `∈` as `∊`, `∼` as `~`, `⋆` as `*` and `−` as `-` — which is what
  the reference does with them, and what a definition copied out of a
  typeset page holds. A character literal is untouched: `'∣'` is still that
  character.

- APL's distributed assignment, `(a b)←1 2`: the names in the brackets
  share the value out between them, one item each, and a scalar goes to all
  of them. `(a b)←b a` swaps two names, and the sentence still yields the
  value.

- A `∇` definition's body may carry the line numbers the `∇` editor prints
  in front of it — `[1]`, `[2]`, `[1.1]` — which is how every printed APL
  definition is written, so a listing can be pasted in and run. A label
  keeps its meaning behind one: `[4] L:r←1`.

- APL's `@` (at, Dyalog's): the positions the right operand names, changed
  by the left one, and everything else left alone. A value right operand is
  the positions and a function's result is a boolean mask over the items; a
  value left operand replaces what stands there and a function is applied
  to the selection. `9@2 ⊢ 0 0 0` is `0 9 0` and
  `(×∘10)@(2∘|) ⊢ 1 2 3 4 5` is `10 2 30 4 50`. Recorded against Dyalog:
  GNU APL has no `@`.

### Changed

- A J quoted literal is a vector of BYTES, which is what J's literal type
  holds: `# 'é'` is 2, `# '日本'` is 6, `a. i. 'é'` is `195 169`, and a
  reshape, a take or an index can land between the bytes of one character.
  The display writes those bytes out again, so the text still looks like
  what was typed and a byte taken out of the middle of a character shows as
  one that could not be read — all of it as jconsole answers. Every text
  verb over non-ASCII text used to disagree with it. The old reading, one
  item per character, is the `j_unicode_strings` extension. APL is
  unchanged: its characters were always Unicode, and GNU APL agrees.
- A label at the head of a `∇` definition's line parses. `L:` used to be
  read as a control word and reported as an unknown one, which blocked
  every loop written the classical way; `∇Z←C1 ⋄ Z←0 ⋄ L:Z←Z+1 ⋄ →(Z<4)/L`
  now runs.

- APL's `< ≤ ≥ >` are total, as the line libjay follows has them:
  characters order by their codepoint, a character stands below every
  number, and a complex value orders by its real part and then its
  imaginary one. `'b'<'c'`, `'a'<1` and `1J2<1J3` all answer 1 where each
  was a type or domain error before. `Dialect.order_domain` (Python:
  `order_domain="numeric"`) is the other reading, where only real numbers
  have an order — that is what the Dyalog preset uses, and what J does.
  `⌈` and `⌊` are not comparisons and stay numeric in both.

- Dyadic `⍳` with a left argument of rank 2 or more answers coordinates:
  `(2 2⍴⍳4)⍳3` is the enclosed `2 1`, and a value the table does not hold
  gives the enclosed empty vector. It used to ravel the left argument and
  answer one number.

- An assignment stranded with a value beside it is one item of the vector:
  `3 V←1 2` is the two-item nested vector `3` and `1 2`. It used to be a
  syntax error.

- `!` of a complex value with no imaginary part is the real it displays as,
  so `!2j0` is 2. A value with an imaginary part is still a named gap.

- A `[X]` axis in a `∇` definition header is named as a feature libjay does
  not have yet, rather than reported as a malformed header.

- J's `b` numeric literal takes any number for its base, and every letter
  after the `b` is a digit. `3r4b11` counts in three quarters (1.75),
  `1r10b12` is 2.1, `3j4b11` counts in a complex base, `36bxyz` is 44027
  and `2b11p1` is 63. A `.` among the digits starts the negative powers, so
  `2b11.1` is 3.5. All four used to be rejected as invalid numbers.

- J's `q:` reads its whole argument at once rather than one number at a
  time: `q: 2 3 4 5` is a four-by-two whose rows are padded with 1s, so
  each row still multiplies back to the item it came from. It used to pad
  with zeros.

- J's `3 p: y` answers the prime factors with multiplicity, as the
  reference does — `3 p: 12` is `2 2 3`. It used to drop the repeats.

- J's grade now gives a NaN a place of its own: `/:` puts it after every
  number, so `/: _. , 1 , 2` is `1 2 0`, and `\:` leads with it. It used to
  tie with everything, which left the permutation to chance.
- J's `-:` (match) answers 1 for a NaN against a NaN: `_. -: _.`,
  `1 2 _. -: 1 2 _.` and `(_. ; 1) -: (_. ; 1)` are all 1. `=` is
  unchanged — a NaN still equals nothing, `-.` and `~.` still keep two
  NaNs apart.
- J's `i.`, `i:`, `e.` and the `=` monad follow the reference in reading a
  NaN as indistinguishable from any single number: `1 2 3 i. _.` is 0,
  `_. e. 1 2 3` is 1, and `= _. , 1 , 2` is the single row `1 1 1`. A cell
  of two or more numbers is still compared as a whole, where a NaN matches
  nothing.

### Fixed

- `$.^:(1 1) 1 0 1` — a power with a LIST of counts over a sparse array —
  crashed. The results are collected into one array, and a sparse one holds
  only its stored entries while its shape is the logical one, so the answer
  was sized from a shape its buffer could not fill. Every collecting form
  now makes a cell dense before framing it, which is what the rest of
  libjay already did with sparse values.

- `u . v y` handed `u` a vector of ones instead of the argument's own
  values wherever `u` was not an insert, so `(*: . >) 1 2` answered `1 1`
  where the reference answers `1 4`, and a character argument reached a
  numeric comparison it should never have seen. The expansion now bottoms
  out on the last column itself: `u . v y` of a vector or an atom, each
  read as one column, is `u y`.

- `x (m H. n) y` — the hypergeometric series stopped after x terms — said
  it had no dyadic meaning. `8 (1 H. 1) i. 6` now sums eight terms of the
  exponential; the count is a whole nonnegative number and pairs with the
  argument element by element.

- An explicit definition whose body has no sentences at all — `3 : ''`,
  `3 : 'NB. nothing'` — was applied and answered nothing, which is not a
  value a verb may give. It now refuses at either valence. A body that RAN
  and produced nothing is unchanged: an untaken branch still yields J's
  empty result.

- `3 : ('a =. *: y' ; 'a + a')`, the multi-line body given as a boxed list
  of lines, was refused with a message about a different construct. It is
  now the ordinary multi-line definition, one box per line, and a lone `:`
  between the lines separates the monad case from the dyad case — in this
  spelling and in the `3 : 0` body below the sentence alike.

- A J numeric literal whose atoms are all 0 or 1 is BOOLEAN, which is what
  `3!:0 (1 0 1)` reports of it in the reference; libjay called every
  numeric literal an integer. The type now travels the way J's does:
  through the structural verbs, through `*` `<.` `>.` `^` `|` `!`, which
  cannot leave `{0, 1}`, and out through `+` and `-`, which widen. The
  identity element of an empty reduction is boolean too.

- `a_ =: 3` was accepted. A trailing underscore closes a locative and
  nothing else, so `a_` is not a name; `a_b_`, `a__` and `cc__` still are.

- `(2;a) $. y` and its relatives — the boxed left arguments that respecify
  a sparse array's axes or element — refused with a rank error, which read
  as though the language did not allow them. They are named gaps now.

- J's boxed total ordering compared the SHAPE before the atoms, so `/:`,
  `\:` and `I.` quietly answered wrongly on the commonest boxed data there
  is — a list of words of different lengths. The reference walks the two
  arrays item by item after the class and the rank, and the shape speaks
  only where every shared item ties: `/: (<'aa'), (<,'b')` is `0 1`, not
  `1 0`, and a sorted table of words now looks up the slot it should.

- An APL each whose results are not all of one depth refused to build an
  array at all ("cannot frame boxed and unboxed results"). `⌽¨ 1 (2 3)` is
  the nested `1` and `3 2` again, and a dfn that answers a number for some
  items and a list for others now works.

- The gerund form of `}` was read as a boxed index specification and failed
  on its shape, and `$:` inside a gerund was called a verb no atomic
  representation may name. Both diagnostics named the wrong feature; both
  spellings now mean what the reference means by them.

- `x u^:_n y` applied u forward n times instead of undoing it, in both
  languages, and answered the opposite of what was asked with no
  diagnostic. A negative power is now settled when the arguments arrive:
  applied monadically it runs u's obverse, and applied dyadically the
  obverse of the bond `x&u`, which is what `x u^:_1 y` means. `7 (+^:_2) 20`
  is 6, `3 (|.^:_1) 1 2 3 4 5` is `3 4 5 1 2`, `2 (#.^:_1) 9` is `1 0 0 1`,
  and `4(+⍣¯3)20` is 8. Two verbs that used to refuse now answer, because
  the bond has an inverse where the monad has none: `2 (*^:_1) 6` is 3.
  A list of counts may mix the signs — `2 (+^:_1 2 3) 20` is `18 24 26`.
  A verb neither reading can turn round is still named, now when the
  sentence runs rather than when it compiles.

- `u~` reported infinite ranks, so anything that framed by them framed
  wrongly. Its ranks are u's with the two argument ranks exchanged, and an
  infinite monadic one. The visible consequence was the table: `(>.~)/~ 5 2 9`
  answered the unframed `5 2 9` instead of the three-by-three, and
  `3 4 (%~)/ 10 20 30` was a length error rather than a two-by-three.

- `u;.n` reported infinite ranks too, so a left argument holding SEVERAL
  cuts was refused instead of framed. The left rank is 2 for the rectangle
  and tessellation forms and 1 for the interval ones, so
  `(2 2 2$1 1 2 2 0 0 2 2) <;.0 i.5 5` now answers two blocks and
  `(2 3$1 0 0 0 1 0) <;.1 i.3 3` two cuttings.

- `u :: v` (adverse) reported u's ranks; they are infinite, since which of
  the two verbs will run is not known until one of them fails. `u :. v`,
  which runs u, is unchanged.

- `q:` and `x q:` refused every integer above 2⋆63, extended type and all:
  the value was put through a machine integer before anything else
  happened. They now factor exactly however many digits the number has, so
  `q: 2^70x` and `q: 999999999999999999999x` answer, and a float is
  admitted on being a whole number rather than on fitting a machine word
  (`q: 6.5e19`).

- `+//. i. 0 0` crashed. An oblique or a key with no cells to work on —
  `u/.` over a table with no rows or no columns, or over an empty list —
  now answers the empty array the reference does, with the axes `u` would
  have given a cell: `+//. i. 0 3` is an empty list and `,//. i. 0 3` a
  0 by 0 table.
- `⎕←` and `⍞←` inside a dfn body under the Dyalog dialect. There, a dfn
  answers with its first sentence that is not an assignment — and printing
  IS an assignment, to `⎕`. libjay was treating it as an ordinary
  expression, so the first `⎕←` in a body became the dfn's answer and every
  sentence after it was dropped: `{⎕←⍵ ⋄ ⍵+1} 5` printed 5 and answered 5
  instead of printing 5 and answering 6. A body may now print and go on
  computing, in guards, in nested dfns and in operator bodies alike. The
  default dialect, which answers with a body's last sentence, was never
  affected.

- Dyadic `⍕` refused three things it should answer, and rounded a fourth
  the wrong way. A width of 0 — given, or left out with a lone precision —
  now means "as wide as the column needs, plus a separating blank", so
  `0 2⍕1.5` is ` 1.50`. A NEGATIVE precision is the scaled form, with that
  many mantissa digits and the exponent after an `E`: `0 ¯2⍕123.45` is
  ` 1.2E2`. And a half now rounds AWAY from zero rather than to the nearest
  even digit, so `4 0⍕2.5` is 3, `3 1⍕1.25` is 1.3 and `4 1⍕0.35` is 0.4;
  only the ties that fell on an odd digit ever differed.

- `⊃` (pick) answered where both references refuse. One item of the left
  argument is one LEVEL of the path and holds one index per axis of the
  value at that level, so `(2 2)⊃matrix` asks for two levels of nesting and
  is a rank error where `(⊂2 2)⊃matrix` — one two-axis index — still
  answers; an empty index picks from a simple scalar and nothing else
  (`(⊂⍬)⊃5` is 5, `(⊂⍬)⊃1 2 3` is refused); no index picks from a scalar at
  all, so `1⊃⊂1 2 3` is refused; and an index below `⎕IO` is out of range
  rather than an index from the end, so `0⊃1 2 3` and `¯1⊃1 2 3` are
  refused.

- Mix — `⊃` in the default dialect, `↑` in Dyalog's — refused to frame a
  character item beside a numeric one. `⊃('ab')(1 2)` is now the two-row
  mixed simple array both references answer, and each row is padded with
  ITS OWN prototype, so `⊃(1 2)('abc')` pads the numeric row with a zero.

- The `[k]` axis on the DYADIC `/` and `\\`: `1 0 1/[1]3 3⍴⍳9` keeps the
  first and last rows, `1 0 1\\[2]2 2⍴⍳4` opens a column of fills. The pair
  `/`/`⌿` and the pair `\\`/`⍀` are one primitive each at two ranks, so a
  named axis is what picks between them. `⊆[k]`, `↑[k]` and `⌷[k]` are still
  named gaps, and a FRACTIONAL axis (`↑[0.5]`) now names itself as one
  rather than reporting a malformed axis.

### Dialect

- Six settings were added to the dialect object, each naming a rule where
  the two APL lines part. `axis_counts`: `↑` and `↓` take a left argument
  SHORTER than the rank, the counts applying to the leading axes and the
  rest taken whole or dropped from not at all — `2↑matrix` is the first two
  rows. `unique_mask`: monadic `≠` marks MAJOR CELLS and always answers a
  vector as long as `≢Y`. `expansion`: dyadic `\` takes any integer count
  list — `2 2\'ab'` is `aabb`, `¯2\1` is `0 0` — the result being
  `+/1⌈|X` items long. `where_rank`: monadic `⍸` gives a rank-0 argument an
  EMPTY index vector, so `⍸1` is a one-item nested vector. `format_spec`:
  dyadic `⍕` rounds a half on the shortest decimal that names the value,
  keeps a one-digit mantissa's point, pads the scaled form's exponent out
  to four characters under a given width, and fills a field too narrow with
  asterisks rather than refusing. And `lookup_left`, which already named
  Dyalog's `⍳`, now covers `⍸` as well and reads MAJOR CELLS rather than
  refusing every left argument that is not a vector: `(2 3⍴⍳6)⍳1 2 3` is 1
  and `(2 2⍴1 2 3 4)⍸1 2` is 1, while a scalar left argument — having no
  major cell — is a rank error. All six ship in `Dialect::dyalog()` /
  `APL.Dialect.dyalog`; the default dialect is unchanged by any of them.

## 0.3.1 — 2026-08-23

### Added

- SHY results in APL. A definition whose answer came from an assignment
  answers shyly: `{a←⍵×2} 5` has the value 10 and a session displays
  nothing, while `1+{a←⍵×2} 5` is 11 and displays it. The value is there
  either way — it is an argument, it is assigned, it is printed by a caller
  that asks for it — and what shyness settles is only whether a session
  that prints results unasked prints this one. Shyness belongs to the
  application: every application starts out not shy, only a definition's
  own last sentence makes it shy, and so an operator that ends by applying
  that definition passes it on — `{a←⍵×2}¨1 2 3` and `F⍣2⊢5` are shy —
  while a primitive over the same value is not: `+/F¨1 2 3` and `⌽F 5 6`
  display. `Program::run_detail` returns the flag beside the value;
  `Program::run` is unchanged and returns the value alone. J has no shy
  results: a definition ending in an assignment answers with the assigned
  value, displayed like any other.

- APL's control words `:AndIf`, `:OrIf` and `:CaseList`, and `:For a b :In`.
  `:AndIf` and `:OrIf` continue the `:If`, `:ElseIf`, `:While` or `:Until`
  line above them and short-circuit — the second test does not run where the
  first has settled the answer. `:CaseList 1 2 3` takes its arm where the
  subject matches any one of the list's items, where `:Case` compares the
  list as a whole. `:For a b :In (1 2)(3 4)` takes each item apart between
  the names, one of its own items each. All three are the language, not a
  dialect setting, so both presets answer them.

- A control structure may stand OUTSIDE a definition:
  `:If 1 ⋄ 5 ⋄ :EndIf` is 5, and `T←0 ⋄ :For I :In 1 2 3 4 ⋄ T←T+I ⋄
  :EndFor ⋄ T` is 10. Its value is the value of the last sentence the branch
  it chose ran, which is the block model every other sequence follows. J
  still holds its control words to a definition's body, as the reference
  does.

- A definition's body may call a function the program fixes AFTER it:
  `N←⎕FX 'Z←F R' 'Z←G R' ⋄ M←⎕FX 'Z←G R' 'Z←R×3' ⋄ F 5` is 15. APL settles a
  name's class when the line runs, so every name a `∇` or a `⎕FX` anywhere
  in the program gives a function now stands for a verb resolved when it is
  applied.

- `Dialect.inner_each`: where the each in the inner product's definition
  sits. GNU APL puts it on the FOLD — `f/¨ (⊂[last]x) ∘.g (⊂[first]y)` — so
  `g` meets a whole vector from each side and the fold's answer is enclosed
  once more; Dyalog puts it on the PAIRING — `f/ row g¨ column` — so `g`
  meets one element from each side and the fold's own value is the cell.
  `1 2+.,3 4` is `10` under the first and an enclosed `3 7` under the
  second. Every scalar `g` whose fold ends in a number agrees, so `+.×` is
  one sentence in either reading and John Scholes' Life one-liner differs
  only in depth. `Dialect::dyalog()` carries the second reading.

- `Dialect.control_strictness`: how strictly a control structure reads what
  it is given. The shipped reading is lenient — a condition is true where
  its first atom is, and a `:Leave` outside a loop leaves the definition.
  Dyalog reads both strictly and says so instead; `Dialect::dyalog()`
  carries that.

### Changed

- `:For` binds an item's CONTENTS in APL, as Dyalog does:
  `:For p :In (1 2)(3 4 5)` gives `p` a pair and then a triple, not an
  enclosure of each. J's `for.` still leaves its boxes shut.

- The Dyalog preset now answers 1967 of the 2012 recorded expressions, up
  from 1941; `tests/expected/dyalog.txt` is down from 71 exempt rows to 45,
  23 of them a divergence and 22 a gap. The whole `inner-product` group (15
  rows, the Life idiom), the whole `control-words` group (9) and the
  `control-strictness` group (2) are closed. GNU APL's column is unchanged,
  expression for expression.

### Fixed

## 0.3.0 — 2026-08-23

### Added

- The obverse table now holds nearly every verb the reference is willing to
  name one for, which is what `u&.v`, `u&.:v`, `u^:_1`, `u b. _1` and APL's
  `f⍢g` and `f⍣¯1` all read. Newly reachable: `/:` `\:` `C.` `%.` `p.` `]`
  `[` `,:` `{.` `":` `".` `o.` `j.` `r.` `+.` `*.` `q:` `;:` `x:` `u:` `s:`
  `I.` `p:` `$.`, the running folds `+/\` `-/\` `*/\` `%/\` and their suffix
  forms, the product `*/`, `u&.>` and `u¨` (which turn round only the verb
  inside the box), and the bonds `n&|.` `n&}.` `n&,` `,&n` `n&#` `n&#.`
  `n&#:` `n&o.` `n&A.` `n&C.` `u~&n` `%:&n` `^.&n`. So
  `(+/\)^:_1 ] 1 3 6` is `1 2 3`, `(*:&.>)^:_1 ] <9` is `<3`,
  `(2&#.)^:_1 ] 5` is `1 0 1`, `;:^:_1 ;: 'ab cd'` is `ab cd`, and
  `p:^:_1 ] 100` is 25 — a hundred has twenty-five primes below it.

- J's `u&.,`, under ravel: `,` has no obverse, since a ravel says nothing
  about the shape it came from, but the shape is in hand while the sentence
  runs. `+:&., i. 2 3` doubles every element and gives the 2-by-3 back. As
  in the reference it has one valence, and `,^:_1` stays a refusal.

- APL's `f⍣¯1` reaches the whole table, so `(+\)⍣¯1⊢1 3 6` is `1 2 3`,
  `⍕⍣¯1⊢'12'` is 12 and `(2∘⌽)⍣¯1⊢3 1 2` is `1 2 3`.

- The Dyalog preset is a GATE. `cargo test -p libjay --test oracle_dyalog`
  replays all 2012 recorded Dyalog answers under `Dialect::dyalog()` and
  fails on any expression the preset does not match — the third differential
  battery, beside jconsole's and GNU APL's, and the same closed system: no
  subprocess, no interpreter, one case per corpus theme. What may differ is
  listed one expression at a time in `crates/libjay/tests/expected/dyalog.txt`,
  each row carrying its reason: 23 rows are a divergence libjay keeps on
  purpose and 48 a named gap, whose tag is the row of docs/status.md's
  Dyalog table that would close it. Nothing is exempt silently, and a row
  that has stopped differing fails the run, so closing a gap deletes its
  rows and the gate tightens by itself.

  The shipped dialects are untouched: `Dialect::default()` is still the
  APL2/ISO line, held to GNU APL expression for expression as before.

- The command line takes `--dialect gnu|dyalog` for APL, so `uvx libjay` can answer in either line: `uvx libjay --lang apl --dialect dyalog -e '↑(1 2)(3 4)'` is the 2×2 mix.

- J's `#^:_1`, the obverse of copy: `1 0 1 #^:_1 ] 1 3` is `1 0 3`, putting
  the items back where the ones stand and a fill in the place of every
  zero. It is the expansion APL spells `\`, and it fills as J does.

- J's sparse arrays, `$.`. `$. 0 0 3 0 5` keeps only the positions that are
  not zero and prints them the way J does — one line per stored value, its
  position, `|`, then the value. The dyad takes a form number: `0 $.` moves
  to the other storage kind either way round, `1 $. 3 4` makes a new sparse
  array from a shape (`1 $. (3 4) ; 0 1 ; 5` gives it its own sparse axes
  and its own repeated element), `2 $.` `3 $.` `4 $.` `5 $.` and `7 $.` ask
  for the sparse axes, the element, the stored positions, the stored values
  and how many there are, `8 $.` drops stored values that have become the
  element again, and `_1 $.` gives shape, axes and element boxed together.
  `3!:0` reports the sparse type codes. Booleans, integers, floats and
  complex numbers can be stored sparsely; a sparse array of characters or
  boxes is named as a gap, as J refuses it too.

  A sparse array is the array it stands for: `($. 0 0 3 0 5) -: 0 0 3 0 5`
  is 1, and any verb other than `$.`, `$`, `#`, `":` and `3!:0` reads every
  position of it, so the answer is always the dense one's. Where J keeps
  `s + 1` sparse, libjay hands back the dense array; the value is the same
  and the saving is not. Python, Arrow and the C ABI carry the dense array.

- A `gcd_rule` dialect setting, `"tolerant"` (the shipped default, GNU
  APL's reading of `∨` and `∧`) or `"exact"` (Dyalog's and J's). It decides
  three probed differences at once: whether a zero argument hands its whole
  partner back with its sign, whether an argument within `⎕CT` of a whole
  number is rounded to it first, and whether one no larger than `⎕CT` beside
  the other counts as zero. `Dialect::dyalog()` sets it to `"exact"`.

- A second APL dialect: `Dialect::dyalog()` in Rust, `APL.Dialect.dyalog` in
  Python. The shipped default is unchanged — the APL2/ISO line GNU APL
  embodies, which is what the differential suite gates — and the preset is
  the other line wherever a recorded Dyalog answer says what it is. `⎕CT` is
  `1e¯14`; `↑` mixes and `⊃` takes the first, the swap carrying their ranks
  with it, so `⊃2 3⍴⍳6` is `1` and not the first row; `⌷` names the LEADING
  axes, so `2⌷3 3⍴⍳9` is `4 5 6` and an
  enclosed index vector keeps its axis; a dyadic `⊂` counts the partitions
  to open before each item, so `1 0 1⊂1 2 3` is `(1 2)(3)`, and the answer
  is a vector of sub-arrays whatever the rank of the argument; `≡` negates
  the depth of an array whose items do not all have the same depth, so
  `≡1(2(3 4))` is `¯3`; a dfn answers with its first sentence that is
  neither an assignment nor a guard; and a nested `⍋`/`⍒` compares over the
  total array ordering rather than the APL2 one. Settings libjay implements
  only one reading of — the grounded nested model, a lazy `⍺←`, the complex
  ordering — still refuse the other at compile time rather than answer
  differently in silence, and the preset leaves them where they were.

  Dyadic `⍳` takes a VECTOR on its left under the preset and gives a rank
  error for anything else, scalars included, where the default searches the
  items of a left argument of any rank.

  Against the 1892 corpus expressions Dyalog 20.0 has been recorded on,
  libjay's default answers 1667 and the preset answers 1768.
  `jay-corpus stats apl --dialect-diff --dialect dyalog` itemises the rest;
  it replays the recorded column and runs no interpreter.

- APL's `⎕FX`, which fixes a definition from its text and answers with its
  name. `⎕FX 'Z←F R' 'Z←R×2'` defines `F` and gives back `'F'`: one line per
  item of a vector of character vectors, the first of them the header, and
  the same lines a `∇ … ∇` takes, control words included. libjay compiles
  before it runs, so the lines have to be literal text the compiler can
  read — a definition assembled while the program runs, or a `⎕FX` inside
  another definition's body, is named as not implemented yet rather than
  answered — and a definition that will not fix is reported as the fault it
  is, pointing at the line that carries it, where Dyalog answers the number
  of the offending line.

  This is what makes Dyalog's own control-structure theme measurable:
  `corpus/apl/dyalog-control.txt` writes its functions with `⎕FX` because
  the `∇` editor cannot be driven over a pipe, and libjay now agrees with 68
  of its 79 expressions where it agreed with 8.

- APL's `∘` binds an ARRAY where a function operand belongs: `A∘f y` is
  `A f y` and `f∘A y` is `y f A`, so `2∘× 5` is 10, `(÷∘2) 7` is 3.5 and
  `(1∘↓)⍣2⊢1 2 3 4 5` is `3 4 5`. Both are monadic only, as J's `m&v` and
  `u&n` are. The array has to be written out; a computed operand
  (`(⍳3)∘+`) is still named as not implemented yet.

- A dfn operator takes an ARRAY operand too, and the body reads `⍺⍺` or
  `⍵⍵` as that array rather than as a function: `2{⍺⍺+⍵}3` is 5,
  `'ab'{⍺⍺,⍵}'cd'` is `abcd`, and `BOTHARR←{⍺⍺,⍵⍵,⍵} ⋄ (1 BOTHARR 2) 3`
  is `1 2 3`. Which of the two an operand is decides how the body parses —
  `⍺⍺+⍵` is a train under one reading and a sum under the other — so the
  body is read both ways when the dfn is defined and the operands choose
  when they arrive. The operand binds tighter than the argument, so the
  array to the right of an operator is its operand.

- APL's `f⍣¯n` runs f's inverse n times: `⌽⍣¯1⊢1 2 3` is `3 2 1`,
  `(1∘+)⍣¯1⊢5` is 4, `⍟⍣¯1⊢1` is `e`. It reads the same obverse table J's
  `u^:_n` reads, and a function with no known inverse is named rather than
  answered wrongly.

- A dfn written INSIDE another reads the names the enclosing one made
  local: `F←{a←10 ⋄ {a+⍵} ⍵} ⋄ F 5` is 15 and
  `F←{n←⍵ ⋄ +/{n×⍵}¨1 2 3} ⋄ F 10` is 60. Its own assignments stay its
  own, so `F←{a←1 ⋄ G←{a←2 ⋄ a} ⋄ (G 0),a} ⋄ F 0` is `2 1`. Only a
  lexically enclosing dfn's names are reachable: a dfn named elsewhere and
  called from inside one still sees the globals and nothing of its caller.

- A dfn may name a function of its own and use it in the sentences after:
  `F←{G←{⍵×2} ⋄ G ⍵} ⋄ F 5` is 10. The name does not escape the dfn.

- Three more APL dialect settings, all of them recorded Dyalog readings
  that `Dialect::dyalog()` now carries:

  - `near_count`: how a float merely NEAR a whole number is admitted where
    a count, a length or an index belongs. `"absolute"` (the shipped
    default, GNU APL's) is a flat `1E¯10` at every magnitude; `"tolerant"`
    (Dyalog's) is relative and follows `⎕CT`, so `⍴⍳1000000+1E¯9` answers
    under it and `⍳2+9E¯11` is a domain error. Neither window is a superset
    of the other, and neither is the comparison tolerance.
  - `floor_rule`: `"shift"` (the default) is `⌊y+⎕CT`, an absolute step;
    `"scaled"` is `⌊y+⎕CT×1⌈|y`, so `⌊9.9999999999999` is 10 and
    `⌊¯1E¯13` is `¯1`.
  - `encode_digits`: whether `⊤` takes its digits with the tolerant
    residue `|` uses. `"tolerant"` is the default; under `"exact"`,
    `2 2⊤4-1E¯14` is `1 2` rather than `0 0`.

### Changed

- An APL dfn is ambivalent whatever its body mentions: a left argument it
  has no name for is dropped rather than refused, so `3 {⍵×2} 5` is 10 and
  `F←{⍵} ⋄ 1 F 2` is 2. A `∇` definition still binds its arguments by the
  names its header gives and refuses the one it cannot bind. This is the
  recorded Dyalog answer, and dfns have no other reference.

- A dfn guard reads its condition strictly: exactly one element, and that
  element 0 or 1. `{2:1 ⋄ 0} 5`, `{1 1:1 ⋄ 0} 5`, `{⍬:1 ⋄ 0} 5` and
  `{'x':1 ⋄ 0} 5` are domain errors where they used to take the first
  element of whatever they were given. A control structure's `:If` keeps
  the loose reading.

- A `:` inside a dfn always opens a guard, whatever follows it, so
  `{a←⍵×2 ⋄ a>10:a ⋄ a+100}` reads as it should instead of complaining
  about an unknown control word. A dfn has no control words to confuse it
  with.

- Under the Dyalog dialect's "the first sentence that is not an
  assignment" rule, naming a function or an indexed assignment counts as an
  assignment, so `{G←{⍵×2} ⋄ 7} 5` is 7.

- `Dialect` gained three fields (`near_count`, `floor_rule`,
  `encode_digits`) and `Tol` gained one (`floor_rule`). A host that builds
  either struct literally has to name them; `Dialect::default()`,
  `Dialect::gnu_apl()` and the Python keywords are unchanged in behaviour.

- APL's `⌈/` and `⌊/` over an empty axis answer the extremes of the
  representable range rather than the infinities. The reduce identity table
  now reads the language: J's neutral cells for `>./` and `<./` stay `__`
  and `_`, and every other entry is shared as before.
- APL's `≠` (the nub sieve) runs over the ELEMENTS in ravel order and keeps
  its argument's shape, so `≠2 3⍴⍳6` is a 2 by 3 table of ones and `≠5` is a
  scalar. J's `~:` still runs over items. The two spellings are not the same
  function, and the corpus now holds both.
- Under the Dyalog nested grade, two arrays with no atoms to separate them
  are ordered by the item they WOULD have held rather than by the type of
  their buffer: a nested empty's prototype, and for a simple one the fill
  its type implies. `⍋(0⍴⊂1 2)('')` was already right; it is now right for
  the reason the recording gives rather than by a rule guessed for the box.

- **APL's n-wise reduction, `n f/ y`.** The dyadic case of a `/`-derived
  function folds every window of n items along the axis the glyph chooses:
  `2+/1 2 3` is `3 5`, `3+/⍳5` is `6 9 12`, `2,/1 2 3` builds the pairs,
  and `2+⌿m` runs down the columns while `2+/[1]m` names the axis outright.
  n is one number however it is shaped — two of them is a length error, not
  a compress. A negative n reverses each window before folding it, so
  `¯2-/1 2 3` is `1 1` where `2-/1 2 3` is `¯1 ¯1`; a zero answers what
  `f/` gives an empty argument, once per gap, so `0+/1 2 3 4 5` is six
  zeros. A window may be one item longer than the axis, which leaves an
  empty; longer than that is a domain error. A positive window over
  arithmetic runs through the same blockwise kernel as J's `n u/\ y`, and
  fuses into a chain the same way.

### Fixed

- `(2&%:)^:_1` and `%:&.`: the obverse of the n-th root is the n-th POWER,
  so `(2&%:)^:_1 ] 3` is 9. It raised n to the argument before.

- **`(<9223372036854775806) C. 1 2 3` is an error, not a crash.** A cycle
  named an index and the permutation it asked for was allocated before the
  index was checked, so a huge one took the process down with a capacity
  overflow. Every element of a cycle is now checked first — against the
  argument it is about to permute for `x C. y`, and against the value
  ceiling for `C. y`. A negative element counts back from the end while it
  is at it, so `(<_1 0) C. 1 2 3` is `3 2 1` and `(<_1) C. 1 2 3` is the
  argument itself, where both used to be refused.

- **`2 %/\. 'abc'` is `ca`.** An outfix piece of one item applies nothing at
  all, so the operand's domain never comes up: the characters are never
  divided. libjay used to type the whole argument first for every operand,
  which refused `-`, `%`, `!`, `|`, `^`, `,` and the rest of them. The five
  folds J has its own special code for — `+/`, `*/`, `<./`, `>./` and
  `+./` — do type the argument up front, and go on refusing the same data.

- **An empty operand takes the other side's type instead of clashing with
  it.** `(0$'a') , 1 2 3` is `1 2 3`, `1 2 3 , (0$'a')` is `1 2 3` and an
  empty box vanishes beside characters the same way; framing fills the empty
  row out with the RESULT's fill, so `(0$'a') ,: 1 2 3` is a numeric table
  of two rows. Where both sides are empty the wider container wins — a box
  over a character, a character over a number. The table dyad answers
  through the same rule: `(0 0$0) ,/ 'ab'` is the `ab`-shaped empty.

- **J's `,` takes a rank gap of any width.** `1 2 3 , (2 1 3$1)` is a rank-3
  answer whose first item is the vector, filled out to the other side's item
  shape; it used to be a rank error past a gap of one. APL still holds the
  two ranks to within one of each other, which is what GNU APL does.

- **A check that applies per element vanishes when there is no element.**
  `0.5 A. i.0` is the empty (the RANGE of an anagram index is still
  checked, so `1.5 A. i.0` is not), `;: (0$1 2 3)` is the empty list of
  words, `". i.0` and `0.5 ". i.0` are the empty, `(0$0) <;.1 'abc'` is the
  whole argument in one piece, and `0.5 /: i.0` is the empty rather than the
  atom. In APL, `'a'⊥(0⍴0)` is 0, `'a'⊤(0⍴0)` is the empty and
  `(0⍴⊂⍳3)⊂(0⍴0)` is the empty nested vector: with nothing to weigh, write
  or partition, the argument that says HOW is never read. Where there are
  items, every one of those checks stands as before.

- **A boxed polynomial argument is its roots.** J lets the multiplier go
  unsaid, so `p. (<1 2)` is `2 _3 1` and `p.. (<1 2 3)` is `11 _12 3`, and
  the derivative and the integral read the root form as well. A root list
  with no roots is no root rather than a type to refuse: `p. (<i.0)` is
  `,1` and `p. (0$'a')` is the zero polynomial's root form.

- **`0.3 *. 0.1+0.2` is `0.3`, not `2.25e15`.** Euclid on two reals cannot
  reach a remainder of exactly zero, so it stops once the remainder is
  within the comparison tolerance of the larger argument. Without that
  stop, `0.3` and `0.1+0.2` ground down to a common divisor of 4e¯17 and
  the LCM blew up with it. `0.3∧0.1+0.2` and `0.3∨0.1+0.2` in APL likewise.
  A value needing more than twelve significant digits to print back is a
  rounding residue rather than a decimal anyone wrote, and is no longer
  read as one: `1.0000000000001 +. 1` is `9.99201e_14`, which is what
  jconsole answers, where it used to be `1e_13`.

- **`C. 3 4 2` is a permutation of five items.** A direct permutation
  shorter than the one it names is ABBREVIATED: the items it never
  mentions come first, in ascending order, and the list is the tail. So
  `C. 3 4 2` is the cycles of `0 1 3 4 2`, `C. 2 3` is the identity on
  four, `3 4 2 C. i.5` is `0 1 3 4 2` and `2 C. i.5` is the same
  permutation written as an atom — which used to be refused as "not
  supported yet". `C.` also has its ranks now (`1 1 _`), so it applies to
  each row of a table rather than to the whole of it.

- **`p. 1 2 1` is `_1 _1`, not a pair of complex values.** The root finder
  reaches a root of multiplicity m only to about the m-th root of the
  machine epsilon, so a double root came out as two values 1e¯9 either side
  of the answer, each with an imaginary part that does not exist. Roots
  that sit on top of one another are now put back on the root the m-1st
  derivative names, which is exact: `p. 1 3 3 1` is `_1 _1 _1` and
  `p. 1 4 6 4 1` is `_1 _1 _1 _1`. Roots also come out in jconsole's
  order — the largest magnitude first, then the largest real part, then
  the largest imaginary part — so `p. _6 1 1` is `_3 2`, not `2 _3`.

- **`1 2,'ab'` is a mixed vector, not a type error.** APL builds a MIXED
  SIMPLE array — depth 1, one element per position, no one type over all
  of them — wherever two simple arrays share no type, and libjay's value
  model already held such arrays (`1 'a'` has always evaluated). Catenate
  `,`, catenate-first `⍪`, union `∪`, intersection `∩`, without `~`, find
  `⍷`, member `∊`, index-of `⍳`, match `≡` and enlist `∊` all build and
  read them now, and an answer that turns out to share a type after all is
  the plain array again: `1 2 3∩'a' 2` is the number 2 and `+/` over it is
  2. A run of characters beside each other in such a vector is text and
  prints as text, so `1 2,'ab'` shows as `1 2 ab`. J has no such value and
  still refuses the pair.

- **`$ ,. 5` is `1 1`, not `1`.** J's `,.` ravels each item into a row of a
  table and never answers below rank 2, so an atom becomes a one-by-one
  table. The answer was one axis short for every rank-0 argument, and under
  a rank conjunction every cell lost that axis with it: `$ ,."0 (i. 3)` said
  `3 1` where it is `3 1 1`. A wrong shape with no diagnostic, so anything
  downstream of it was wrong too.

- **`5 /: 1 2 3` is refused.** `x /: y` sorts the ITEMS of x by the grade of
  y, and an atom has one item: the only index it can answer is the first.
  libjay handed the atom back for any key at all, so `_3 \: 0.1 0.2 0.3`
  answered `_3` where jconsole reports an index error. `5 /: 1` and
  `'a' /: 1` still answer, since one key needs only that one item.

- **An outfix honours its operand's own domain.** `_2 +/\. 'ab'` answered 0
  and is now refused, as jconsole refuses it: `+` has no meaning for
  characters, and an outfix asks the question even where every piece it
  leaves behind is empty. Boxes are refused the same way (`_1 */\. 1;2`),
  while an operand that does have a meaning for them — `2 ,/\. 'abc'`,
  `1 [/\. 'abc'` — is untouched.

- **Decode extends a single argument.** `1 2 3 #. 5` is 50 and `1 2 3⊥5` is
  50: one digit stands in every position the radices name. J spreads an
  ATOM, so `1 2 3 #. ,5` stays a length error; APL extends a SINGLE — one
  element at any rank — so `1 2 3⊥,5` and `1 2 3⊥1 1⍴5` are 50 as well, and
  a single radix spreads the same way (`(,2)⊥1 2 3` is 11). An empty axis on
  either side weighs nothing rather than raising a length error: `1 2⊥''`
  and `(⍳0)#.5` are both 0.

- **`1⊂1 2 3` encloses the whole vector.** One partition flag is the flag of
  every item, so a single left argument extends along the axis: `1⊂1 2 3` is
  one partition and `0⊂1 2 3` is none. Two flags for three items has no such
  reading and remains a length error. The same rule reaches `⊆`, which
  spells the partition in the Dyalog line.

- **`0 E. 5` is 0.** J reads an atom as a one-item list on both sides of
  `E.`, so a pattern of one atom has exactly one place to sit in an argument
  of one atom; `1 E. 1` is 1. A rank-1 pattern in a rank-0 argument still
  fits nowhere and is still a rank error.

- **`I.` searches boxed bounds.** `(1;2 3) I. (1;2;3)` is `0 1 1`: J defines
  a total order over boxed values — the order `/:` grades them with — and
  the interval index now uses it. A boxed bound against an unboxed value has
  nothing to compare and stays a domain error.

- **A complex value with no imaginary part is ordered by its real part.**
  `1 <. j. 0` is 0 and `3j0 < 4` is 1, where both were refused for want of
  an order. The reading is at the USE and not at the making, which is how
  jconsole has it: the value keeps its complex type, and `3!:0 j. 0` still
  reports 16. A value that really is complex still has no order. The same
  reading reaches everywhere a real is wanted, so `i. 3j0` is `0 1 2` and
  `2 3j0 $ 1` builds the matrix.

- **An empty is acceptable numeric data whatever type it was written at.**
  `#. ''` is 0 in J and `¯3⊥''` is 0 in GNU APL: an empty holds no value of
  the wrong type to refuse. `2 #. ''`, `#: ''` and `i. ''` answer for the
  same reason. An empty BOX is not numeric data — jconsole refuses
  `2 #. 0$<1` — and neither is a non-empty character array.
- **`0 * _` is 0, and `_ - _` is refused.** J defines arithmetic where IEEE
  has only a NaN, and libjay was handing the NaN back as `_.`. A zero
  factor now wins, whatever the other one is — `0 * _`, `_ * 0`, `0 * _.`
  and `*/ 0 , _` are all 0 — and that one rule is what gives `j. _` its
  value `0j_`, since a complex product is four real ones. Where J has no
  value it refuses, with a NaN error naming the pair: `_ - _`, `_ + __`,
  `_ % _`, `2 | _`, `0.5 | _`, `5 #: _`, `0 ^. 0`, `_ ^. _`, `! __`,
  `_ ! _` and `1 o. _`. A NaN the program itself wrote still travels
  through unrefused — `_. + 1` is `_.` — because the rule is about the
  operation and not the operand.

  The values J does define at the same points came with it: `! _`, `! 171`
  and `! 1e308` are `_`; `_ ! 2` is 0 and `2 ! _` is `_`, over the whole
  table of binomials at an infinity; `_ | 2` is 2 and `0 | _` is `_`;
  `_2 ^ __` is 0 while `_1 ^ _` is a domain error; `__ ^ 0.5` is `0j_`;
  and `_ #. 2` is 2. The infinite complex literals `_j_`, `_j1` and `_.j_`
  are read as numbers now instead of failing to parse.

- **APL raises DOMAIN ERROR where it has no value, instead of answering
  `∞`.** `÷0` and `⍟0` were an infinity and are now refused, as dyadic
  `2÷0` already was — including through `¨`, `/` and `\`, which reach the
  same step. So are `!¯3`, `!¯1`, `!171` and `!1E308` (a pole of the gamma
  function and an overflow of it alike), `0⋆¯1`, `1⍟2`, `2⍟0`, `1 0⍟2`,
  `¯1⍟0` and `¯7○1`. The values GNU APL defines at those same points are
  unchanged and are what keep the refusal from spreading: `0÷0` is 1,
  `0⍟0` and `1⍟1` are 1, `0⍟2` is 0, `0⋆0` is 1.

- **`2+/1 2 3` is `3 5`, not `3 4 5`.** APL's `/`-derived functions had no
  dyadic meaning of their own: the derivation was dropped and the operand
  applied with the left argument extended, so every moving sum, moving
  difference and pair-building idiom answered a plausible array that was
  wrong, with no diagnostic. `2×/`, `2-/`, `3+/`, `2+⌿`, `+/[k]` and the
  same under `¨` were all affected. The shape was wrong too: `⍴2+/1 2 3`
  said 3 where it is 2.
- **Residue reads the comparison tolerance, in both languages.** `0.1|0.3`
  was `0.1` and is 0, which is what GNU APL and jconsole both answer; the
  quotient is rounded before the remainder is taken. The two references
  round it differently and libjay now follows each: J takes the tolerant
  floor and answers an exact zero wherever the product is tolerantly the
  dividend, so `2 | 4 + 1e_14` is 0 while `2 | 1e_14` is still `1e_14`; GNU
  APL reads the remainder against the MODULUS instead, so `2|1E¯14` is 0 and
  a modulus large enough swallows the remainder outright (`1E20|3` is 0,
  `1E13|3` is 3). The digits of `#:` and `⊤` are residues and round with it:
  `2 2 #: 4 - 1e_14` was `1 2` and is `0 0`. The fused kernel and the GPU
  shader round the same way, so a fused sentence cannot mean something else.

- **APL's `⌊` and `⌈` shift by `⎕CT` rather than scaling by the magnitude.**
  `⌊99.999999999995` was 100 and is 99 — a gap of 5e¯12 is larger than `⎕CT`
  however big the value is — while `⌊¯1E¯13` is 0 rather than `¯1`. Both
  readings were probed; J keeps the relative one, so `<. 99.999999999995`
  stays 100 and `<. _1e_14` stays `_1`.

- **APL's `⍋` and `⍒` compare under `⎕CT`.** `⍋1.0000000000001 1` was `2 1`
  and is `1 2`: two keys within the tolerance tie, and the stable sort
  leaves them in the order they arrived. The nested comparator reads it at
  every level, so `⍒(1 2)(1 2.0000000000001)` no longer swaps two items that
  differ inside the tolerance. J's grade is defined exact and stays exact —
  `/: 1 1.0000000000001 1` is `0 2 1`, as jconsole answers it.

- **GNU APL's `∨` and `∧` round before the Euclid runs.** An argument within
  `⎕CT` of a whole number is that number, so `1.0000000000001∧5` was `5e13`
  and is 5; one no larger than `⎕CT` beside the other is zero, so `1E¯14∨1`
  is 1; and a zero argument hands its WHOLE partner back with its sign, so
  `¯3∨0` was 3 and is `¯3` (`¯3.5∨0` is `3.5` — only whole numbers keep it).
  Dyalog does none of the three and neither does J, which is what the new
  `gcd_rule` dialect setting names.
- **A field width near the end of the integer range no longer panics.**
  `9223372036854775806 ": 1` multiplied the rows by the summed column width
  and handed the product to an allocator: a capacity overflow in a debug
  build and an 8-exabyte request in a release one. A width and a digit
  count are lengths now, checked against the same ceiling a shape is, so an
  absurd one is a limit error naming the request. APL's `x⍕y` had the same
  unchecked pair and takes the same check.
- **APL fills a nested argument with its prototype.** The gap an expansion,
  a replication or an overtake leaves holds the first item's shape with a
  zero for every number and a blank for every character, nested as deeply
  as the item was: `1 0 1\(1 2)(3 4)` is ` 1 2  0 0  3 4` and `¯2/⊂⍳3` is
  two vectors of zeros, where both used to leave a blank. An array with no
  items remembers that prototype, so `↑0⍴⊂2 3⍴9` is the 2 by 3 table of
  zeros, `4⍴0⍴⊂'ab'` is four pairs of blanks, and `⊃` of such an empty
  keeps the axes its items had. J's own rule is untouched: it fills a box
  with the empty box, whatever the argument held.
- **An empty result keeps the shape a cell would have had.** An application
  with no cells to frame used to answer an empty of the frame alone, which
  dropped every axis the cells carried: `$ 1 >./\. (0 3 $ 0)` was `0`
  where J says `0 3`, and `⍴0/0 3⍴0` was `0` where APL says `0 0`. The
  cell's shape now comes from running the verb once on a cell of fills —
  J's own rule — and reaches every path that frames cells: the rank
  conjunction and `⍤`, replicate and expand along an axis, the scan, the
  infix, the outfix and the cut. APL's scan keeps the shape it was given,
  which is its own rule and not the fill cell's. A verb that refuses the
  fill cell leaves the frame standing on its own.
- **The width of a J infix or outfix is one atom.** `2 3 +/\. 1 2 3`
  answers one row per width, as `2 3 +/\ 1 2 3` already did, and an empty
  list of widths frames nothing instead of being refused as "an outfix
  width needs an integer".
- APL's operators now apply their function between the ITEMS of their
  arguments, which is what the language has always meant by them. An item
  is disclosed on the way in and a result that is not a simple scalar is
  enclosed again on the way into the array being built.

  `∘.f` reaches inside an enclosure — `¯1 0 1∘.⌽⊂2 3⍴⍳6` now rotates the
  MATRIX and answers three rotations of it, where it used to rotate the
  enclosure (which is one item, so nothing moved) and answer three copies.
  It also pairs elements whatever the function's rank, so `1 2∘.,3 4` is
  the two-by-two table of pairs `(1 3)(1 4)` / `(2 3)(2 4)` and
  `'ab'∘.,'cd'` is `ac ad` / `bc bd`; both used to be a single catenation.

  `f/` and `f⌿` fold the ELEMENTS along the axis they reduce and enclose
  what the fold makes: `,/1 2 3` is an enclosed three-element vector,
  `,/2 3⍴⍳6` is two enclosed rows and `,⌿2 3⍴⍳6` is three enclosed
  columns. `f\` and `f⍀` are the reduce over each prefix, so `,\1 2 3` is
  now `1`, `1 2`, `1 2 3` rather than a padded table. The arithmetic
  reductions are untouched: folding elements and folding cells agree for a
  scalar function, so `+/`, `×/`, `⌈/` and the rest answer as before.

  `f.g` is `f/¨` over that outer product, so the each's enclosure is part
  of it: `1 2,.+3 4` is an enclosed `4 6`. `+.×` and the other folds to a
  number are unchanged.

  With this, John Scholes' Game of Life runs as written:
  `{↑1 ⍵∨.∧3 4=+/,¯1 0 1∘.⊖¯1 0 1∘.⌽⊂⍵}`.

  J is a different language here and keeps its own reading: `u/` tables by
  cells, `u/` inserts between cells, and a box stays shut.
- **A rotate amount near the end of the integer range no longer panics.**
  `9223372036854775806 |. 1 2 3` added the amount to a coordinate before
  reducing it modulo the axis, which overflows: a panic in a debug build and
  a silent wrap in a release one. The amount is reduced first. The same
  counting is fixed across the family it belongs to — `|.!.f` (shift),
  `u;.0` and `u;.3` (the cut rectangle and the tessellation) and `x u\.`
  (outfix) each turned a number the program wrote into an index without
  room for it, and each now counts in a width that holds it.
- APL's `⌽` and `⊖` check conformability. They rotate ONE axis and read one
  amount for each vector along it, so `⍴x` must be `⍴y` with that axis
  removed unless x is a scalar (or the one-item vector the reference takes
  as one). `0 1 1 0⌽5` answered `5 5 5 5` and `1 2⌽3 4 5` built a matrix out
  of two vectors; both are conformability errors, and the axis forms
  `⌽[k]`/`⊖[k]` follow the same rule instead of J's one amount per axis.
- An APL literal above 2^53 keeps every digit. The lexer read every number
  through a double, so `9223372036854775806⌽1 2 3` was refused as
  non-integral and `(⍳5)|9223372036854775806` answered from the rounded
  value. Digits alone are now read straight into a machine integer.
- A COLUMN-MAJOR result is read as one. `|:` flips the layout flag rather
  than moving the buffer, and framing cells, opening a box or walking the
  leaves of a nested value spliced the raw buffer instead: `|:;._1 i. 3 4`
  came back with its shape transposed and its data untouched, `|:"2` was
  wrong at every rank, `∊⍉¨` read the wrong order, and `; |:&.>` asserted
  outright. Every reader of a result now takes the rows.
- **A float that is merely NEAR a whole number counts as that whole
  number.** `⍳2-1E¯14` is `1 2` and `(2-1e_14) {. 1 2 3` is `1 2`, as both
  references answer; libjay refused every one of them, so arithmetic that
  had drifted by a rounding error could not be used as a length, a count or
  an index. The admission runs through the whole family that reads one —
  `⍳ ⍴ ↑ ↓ ⌽ ⊖ / \ ⌷ ⊃ [;] ⎕UCS` in APL, `i. $ {. }. |. |.!. # { A. I. q:
  p: u: ^: @.` in J — and each language keeps its own width. J's
  is relative, `2^_44` of the number's own magnitude, so the room grows
  with the count and closes beside zero: `i. 2+1.1e_13` counts and
  `i. 1e_14` does not. APL's is a flat `1E¯10` at every magnitude, so
  `⍳1E¯11` is the empty vector and `⍳1000000+1E¯9` is still a domain
  error. Neither is the comparison tolerance: setting `⎕CT` to zero leaves
  both exactly where they were, and `(2+9E¯11)=2` is still 0. A float a
  real distance from any whole number — `⍳2.5` — is refused as before, and
  an operand SELECTOR (`3 u:`, `s:`, `m b.`, a cut mode) is still read
  exactly, which is what the references do.

- **APL's nested VECTOR display matches GNU APL's spacing.** A run of
  adjacent characters is text with no separator between them, so
  `'ab',⊂1 2` shows as `ab  1 2`, not `a b 1 2`. Elsewhere the gap next to
  an item widens by how much visual weight that item carries: nothing
  extra for a scalar, one column per axis for a numeric or boxed
  structure, and one column fewer for a character array, since a row of
  characters already reads as text on its own — a character vector costs
  nothing extra and a character matrix costs one column. How many `⊂`
  layers wrap the first and the last item sets the vector's own margin.
  A mixed array at rank 2 or above still draws with libjay's own uniform
  spacing, unchanged. J's box drawing is unaffected throughout.

## 0.2.1 — 2026-08-22

### Added

- J's symbols, `s:`. A symbol is an atom whose value is a name, and the same
  text always gives the same symbol: `(s: <'a') = (s: <'a')` is 1 wherever
  the two sentences stand. ``s: '`red`green`blue'`` makes three of them from a
  delimited string, `s: ;: 'red green blue'` from boxed words, and a
  character table gives one name per row. They compare, sort (by name, in
  codepoint order), nub, search and index like any other data — `/:~`,
  `~.`, `i.`, `e.`, `I.`, `{`, `#`, `,` and the rest — print as `` `red
  `green `blue ``, and go into boxes. `4 s:` gives the names back as a
  character table and `5 s:` as boxes; `3!:0` reports 65536. Python gets the
  names as strings. Arithmetic on a symbol is a type error that names `5 s:`
  as the way to its characters, and the `s:` forms that report on an
  interpreter's own symbol table are named rather than guessed at.
- Dyadic `I.` now searches character and symbol lists as well as numeric
  ones: `'ace' I. 'bd'` is `1 2`.
- The inner product: J's `u . v` and APL's `f.g`. `+/ . *` and `+.×` are
  the matrix product, `*./ . =` and `∧.=` ask which rows match, `<./ . +`
  and `⌊.+` take a shortest-path step, and any pair of functions works at
  any rank. The matrix product over numbers is a blocked, parallel,
  vectorised pass over the two blocks rather than an interpreted loop:
  measured on a 1000×1000 pair of doubles it runs about 2.5× behind
  numpy's tuned BLAS, and on whole numbers — where BLAS has no path and
  numpy falls back to its own loop — about 25× ahead of it. Whole numbers
  in give whole numbers out.
- J's monadic `u . v`, the determinant: `-/ . * m` is the determinant
  proper, and the same expansion with other functions is available.
  Ordinary numbers go by elimination; exact integers and rationals keep
  their exactness.
- APL's `⍠`, the variant operator: one setting of the dialect overridden
  for one application. `1 (=⍠0) 1+1E¯14` compares exactly where the
  program's own comparison tolerance would not, and `⍳⍠('IO' 0)` counts
  from zero inside a program that counts from one.
- J's sequential machine, the dyad of `;:`: a table-driven tokeniser
  written as a state machine, in all six of its result forms — the words
  themselves, their positions and lengths, or a full trace of the run.
- J's format by specification, the dyad of `":`: a field width and a
  number of decimals per column, with an exponential form and the
  reference's asterisks for a value that does not fit.
- J's number reading, the dyad of `".`: the numbers a line of text spells,
  with a value of your choosing standing in for anything that is not one —
  `0 ". '1 2 x 3'` is `1 2 0 3`.
- `bench/cloud/`: a design, scripts and IAM policies for one-shot rented
  spot-instance runs — AVX-512, Graviton and an NVIDIA GPU, the three
  machines this project's own numbers have never been taken on. Nothing in
  it has been executed and every script refuses to start until the owner
  fills in his account's details.

### Changed

- Elementwise passes over two different element types — complex against
  float, integer or boolean against float, boolean against integer — now
  promote the narrower operand where they read it instead of widening it
  into a buffer of its own first, and the fused kernel promotes a narrow
  argument one block at a time. Results are unchanged to the last bit; at
  20M elements `{c} + {f}` runs 2.5x faster, `{i} + {f}` 2.2x and
  `+/ {i} * {f}` 5.5x. See bench/README.md, "Mixed-type passes".
- Reductions, scans and moving windows over a yes/no column now read it as
  it lies instead of expanding it to whole numbers first, so summing one is
  finally cheaper than summing a column of numbers rather than ten times
  dearer. Results are unchanged to the last bit; at 20M elements `+/ {b}`
  runs 24x faster, `>./ {b}` 38x, and the scans and moving windows two to
  three times. See bench/README.md, "Folds over one buffer".
- Four verbs that were correct but far slower than the work they describe
  are now the algorithm they describe, with the same answers. Between them
  they close the three losses bench/workloads.md diagnosed:
  - **The suffix scan `u/\.` is one pass.** Folding right to left is the
    insert's own order, so each suffix is one step past the suffix after
    it — for any verb, not just an arithmetic one. It used to fold every
    suffix from scratch. This is what J's spelling of an exponential
    smoothing rests on (`|. u/\. |. y`), and RSI(14) over 20 million bars
    went from n²/2 steps of a general dyad — about nine years — to 2.3
    seconds. Floats come out bit for bit what the old path gave.
  - **A first-order recurrence is recognised and run as one.** A scan whose
    step is the fork `[ + c * ]` (or its mirror `(c * ]) + [`) with a
    constant `c` becomes `acc = y + c*acc` over the buffer instead of an
    interpreted step per item — about ten nanoseconds an element rather
    than a microsecond.
    The rule matches that tree and nothing else; anything it declines takes
    the general path, which is itself linear backwards now.
  - **The key `u/.` hashes its keys.** It used to find each group by
    sweeping the whole key vector, which is rows × groups: a VWAP over 20
    million minute bars grouped by 13,889 days took hours and now takes 1.4
    seconds. Groups still come out in first-occurrence order, and keys
    compared under a tolerance — floats, complex numbers — still are. APL's
    `f⌸` gets the same fix.
  - **A reshape that keeps the elements shares the buffer.** `2 3 $ i. 6`
    and any other `$` whose result the argument's own elements already
    cover is now a refcount bump rather than an element-by-element copy of
    the ravel; a shape that cycles the ravel still copies. The frame-RMS
    workload's reshape of 16 million samples went from 457 ms to nothing
    measurable, and the whole workload from 535 ms to 49 on one thread.
- Two APL spellings moved from "not supported yet" to "absent by design",
  because neither is work that can be queued: `⌶` (I-beam) is defined by
  each interpreter for itself and has no published behaviour to follow,
  and `&` (spawn) starts an APL thread, which libjay's sandbox does not
  open — the same rule J's `T.` and `t.` already fell under. With those
  moved, nothing in APL's primitive tables is a promise, and J's remaining
  two, `s:` and `$.`, are storage kinds rather than primitives.

### Fixed

## 0.2.0 — 2026-08-21

### Added

- APL trains: a run of bare functions now reads as a fork or an atop —
  `(f g h)` applies `f` and `h` to the argument and combines the results
  with `g`; `(g h)` applies `h` then `g`. A plain number may stand in a
  fork's left position, and `⊢`/`⊣` mean "the argument itself". This is an
  extension beyond strict APL2/GNU APL, on by default, with the strict
  reading available as an option.
- APL function assignment: a derived function or a whole train can be
  named (`F←+/`, `F←+/÷≢`) and then applied like any other function.
- J can name an adverb or conjunction on its own (`m =. /`, `c =. @`), not
  only a verb.
- J can define its own adverbs and conjunctions: `1 : '…'` and `2 : '…'`,
  their multi-line forms, and the `{{ … }}` direct-definition syntax,
  matching J's published vocabulary for writing them.
- J's `L:` and `S:` now take two arguments as well as one.
- J's `H.`, the generalised hypergeometric series.
- Reading input, not just writing it: APL's `⍞` (one line of text) and `⎕`
  (one line evaluated as APL), and J's `1!:1 ]1` and `3!:0` (a value's
  storage type), all read from the same standard input the host provides —
  piped, typed, or supplied by the embedding application. Every language
  surface (Rust, Python, C, and the command line) gained the matching call,
  alongside the existing output calls. The rest of J's `!:` foreign
  conjunction that would reach a file, the system clock, or another process
  is refused with a clear "closed by the sandbox" message, distinct from
  "not supported yet".
- Faster execution on newer x86-64 processors, using the CPU's AVX-512
  instructions when present; picked up automatically at startup, with an
  explicit override available. Not yet benchmarked on real AVX-512
  hardware.
- J's gerunds are ordinary data now, exactly as the language has them: a
  tie such as `` +`- `` produces a boxed value you can name, print, add to,
  and build by hand. `` `: `` (evoke gerund) works in all three of its
  forms — apply each verb and collect the answers, insert the verbs between
  the items, or read the gerund as a train.
- Dyadic transpose in both languages: J's `1 0 |: m` and APL's `2 1⍉m`,
  including the diagonal forms `` (<0 1)|: `` and `1 1⍉`.
- J's monadic `{` (catalogue: every combination of one element from each
  item) and monadic `e.` (raze-in).
- J's `_.`, the indeterminate value.
- J's `u b. 1` and `u b. _1`: what a verb's identity element and inverse
  are spelled as.
- J's `^:` accepts a list of counts, and the boxed forms that collect
  every intermediate result — `u^:(<n)` and `u^:a:`.
- J's tessellation `;.3` accepts a negative block size, which reverses that
  axis, where the movement row is written out.
- APL's `⍢` (under) and `⌺` (stencil), and the collating grades `x⍋y` and
  `x⍒y`.
- Grading BOXED (nested) arrays, in both languages, which had been the
  last thing a grade refused. J's `/:` and `\:` order whole arrays by J's
  total array ordering — the type class, then the rank, then the shape read
  with the last axis first, then the atoms, recursing through boxes — and
  APL's `⍋` and `⍒` order them by the APL2 rule GNU APL answers with, which
  is a different comparator at every step. The dyads (`x /: y`, sorting by
  a nested key) and the sort idioms follow from the same ordering. The new
  `nested_grade` dialect setting names Dyalog's total array ordering as the
  other reading, and refuses it rather than answering with this one.

### Changed

- Moving windows and running sums are part of a fused expression now.
  `k +/\ y`, `k >./\ y`, `k <./\ y` and `+/\ y` used to break the chain
  they stood in and run as a pass of their own over the whole column; they
  are steps of the compiled kernel, so a rolling expression reads its
  argument once instead of once per window and once per arithmetic step.
  Results are unchanged to the last bit, including the property that a
  window's rounding error is the error of that window alone.
- A DataFrame no longer costs a copy to read. Its columns cross the
  boundary borrowed, one Arrow buffer each, and libjay folds them where
  they lie: `+/ df` (column sums) and `+/"1 df` (row sums) over a
  2.5M x 8 table are 10 and 5 times faster end to end, and reading the
  table's shape costs nothing at all. Programs that need the elements in
  reading order — ravelling, indexing, printing — still pay for one copy,
  now at the point they ask for it instead of on every call.
- Transposing an array (`|:`, `⍉`) no longer moves any elements, at any
  rank.
- A Fortran-ordered numpy block — `np.asfortranarray(a)`, or the `.T` of an
  ordinary one — is read where it lies instead of being refused with a
  request to copy it. Views that are contiguous in neither order (strided
  slices, sub-blocks, partial axis permutations) are still refused, with
  the same message.
- Refusals that come from the sandbox (closed I/O, the system clock,
  threads) are now labelled distinctly from "not supported" and "not part
  of the language", so it reads as a deliberate boundary rather than a
  missing feature.
- Minimum required Rust version raised to 1.89: needed for the AVX-512
  support above, and for wgpu 30 (the GPU backend's dependency), which
  needs a newer compiler than the previous floor; pinned in the repository
  so every build uses the same compiler.
- Updated third-party dependencies (the GPU backend to wgpu 30, Python
  bindings, and test tooling) to their latest versions; no user-visible
  change.

### Deprecated

### Removed

### Fixed

- `(2&+)^:_1` and `(2&*)^:_1` computed the wrong inverse — the bonded
  number was applied from the left instead of taken off the right, so
  `(2&+)^:_1 5` answered `¯3` where J answers 3. Everything that undoes a
  verb (J's `&.`, `^:_1`, `u b. _1` and APL's `⍢`) is corrected by it.
- APL's `⊥` on an argument of rank 2 or more folded the wrong axis: it is
  an inner product and folds the LEADING axis of its right argument.
  Vectors, the common case, were always right.
- APL's `⍸` (interval index) placed a value exactly equal to a bound in the
  wrong interval: `1 3 5⍸3` is 2, not 1. J's `I.`, whose interval is open
  on that side, is unchanged.
- APL's `⌷` accepts an enclosed vector as an index, so `(⊂1 2)⌷5 6 7 8` is
  `5 6`.
- APL's `∊` finds a scalar held in a nested right argument:
  `1 2 3∊(1 2)(3)` is `0 0 1`.
- Two rarely-used APL operators (variant, I-beam) that
  aren't implemented yet are now reported by name as "not supported yet"
  instead of as an unrecognized character.
- Fixed APL operator precedence so a parenthesised function binds before
  an operator to its right, matching the reference implementation —
  `(+)/1 2 3` now evaluates to 6.
- APL's scalar functions reach inside a nested argument, as APL2 has them:
  `(1 2)(3 4)+1` is `(2 3)(4 5)`, and every arithmetic, comparison and
  logical function pervades to the simple values at the bottom. They used
  to refuse a nested argument outright.
- APL's `⊥` over an EMPTY radix axis crashed the printer: `(⍳0)⊥1 2 3` now
  answers 0, and the result's frame is `(¯1↓⍴x),1↓⍴y` whatever axis is
  empty.
- APL scalar extension between two frames of ONE cell kept the wrong one, so
  `⍴(,5)+¯3` was empty where it is `1`. A rank-0 frame gives way to the
  other side, and between two one-cell frames that are not scalars the
  answer keeps the right one.
- Take and drop count AXES. More counts than the argument has axes is a
  length error in both languages, and only a scalar right argument stretches
  to meet them (`1 2 {. 5` is a 1 by 2 table); APL wants exactly one count
  per axis where J is content with fewer. A count of zero on an axis after
  the first now empties that axis instead of leaving it alone.
- APL's replication extends an argument of one item along the axis, as it
  extends a scalar: `2 0 1/,5` is `5 5 5`.
- APL's dyadic `∪`, `∩` and `~` take vectors, as GNU APL has them; a grade
  needs an array rather than a scalar; and `≡` tells an empty character
  array from an empty numeric one.
- `E.`/`⍷` search every axis at once and answer in the shape of the right
  argument, so a table is found inside a table. An empty pattern matches
  everywhere.
- J's LCM and GCD accept numbers that are not whole: the pair is read as the
  decimals it prints as, so `1.23 +. 4.56` is `0.03` and `2.5 +. 5` is 2.5.
- J's `#.` accumulates in the exact types when it is given them, so a
  19-digit integer keeps every digit and `#. 1r2 1r3` is `4r3`.
- J's `m&v` and `u&n` apply to the whole argument, as `m&v b. 0` reports:
  `1 2&+ 1 2` is `2 4`, not a two-by-two table.
- J's `p.` answers `0 ; ''` for the zero polynomial instead of refusing it,
  and `j.` has an obverse, so `+/&.:j.` works.
- An empty array inside a box keeps its shape on screen: `<0 3⍴0` draws a
  cell three wide with no lines in it.

### Security

## 0.1.0 — 2026-08-21

First release: independent implementations of J and APL over one shared IR,
embeddable from Rust, Python and C.

### Languages

- J frontend: 135 of 180 valences in the published vocabulary implemented,
  26 partial, 18 not yet, 1 refused by design. Verbs, adverbs and
  conjunctions, trains (forks, hooks, caps and longer), tacit and explicit
  definitions, `if.`/`while.`/`for.`/`select.` control structures, gerunds
  under `@.`.
- APL frontend: 79 of 115 valences implemented, 25 partial, 11 not yet.
  Functions, operators, dfns, `⋄` and newline separators, `⎕IO` as a
  compiler setting rather than global state. The dialect is the APL2/ISO
  line that GNU APL embodies; the points where the lineages differ are
  named settings on a dialect object, and asking for the other reading is a
  "not implemented yet" error rather than a silently different answer.
- Both frontends lower to one language-agnostic IR — an `Expr` tree over a
  `Verb` combinator tree — executed by one generic rank-and-agreement
  engine. J's leading-axis reduction and APL's trailing-axis reduction are
  the same machinery with different rank.
- Diagnostics carry a span into the source: the offending text is quoted
  with a caret under it, shape errors print both shapes, and "the language
  lacks this" reads differently from "not implemented yet".

### Execution

- Parallel by default: elementwise passes, pure rank cells and leading-axis
  reductions split above 65,536 element operations, on the crate's own pool
  (`LIBJAY_THREADS`), never rayon's global one.
- Expression fusion: chains of elementwise primitives compile to one
  blockwise kernel, absorbing a trailing full-rank reduction and moving
  named values into the sentences that read them. Anything not fused falls
  back to the subtree it replaced, so results and error messages cannot
  change.
- Runtime SIMD dispatch over the hot loops: x86-64 baseline/v2/v3 and NEON,
  detected once per process (`LIBJAY_CPU_LEVEL`). No hand-written
  per-primitive kernels.
- GPU placement of fused kernels through wgpu (Metal, Vulkan, DX12), with
  WGSL generated at run time. Compiled into the one artifact and dormant on
  a machine with no adapter; `deploy`, `upload` and `keep_on_device` are the
  API. f64 needs `SHADER_F64` and stays on the CPU without it rather than
  quietly computing in f32.
- Headline: a 20-period Bollinger z-score written as one J expression runs
  20M rows in 404 ms against the equivalent Polars pipeline's 755 ms,
  agreeing to 8.7e-10 relative. Resident GPU data runs 1.4x to 7.3x the
  8-thread CPU on the same 20M rows. Numbers and method in
  [bench/README.md](bench/README.md).

### Surfaces

- Rust: the `libjay` crate, library name `jay`. `compile` → `Program::run`;
  a `Program` is immutable, holds no data and is `Send + Sync`.
- Python: the `libjay` wheel, import `jay`, abi3 for 3.10+, no runtime
  dependencies. `jay.j(...)` compiles, binds and executes in one call;
  `jay.j.compile(...)` returns a reusable kernel with `bind`, `deploy` and
  `explain`. Compiled programs are memoised in-process. On 3.14+, t-strings
  make interpolated values both the type contract and the live defaults.
- C: `crates/libjay-capi` builds `libjay.so`/`.dylib`/`jay.dll` plus a
  static library and a hand-written `jay.h`. Prebuilt bundles ride along
  with each GitHub release for four target triples.
- CLI: `libjay -e EXPR`, `libjay FILE` (`.ijs`/`.j`/`.apl`), `--lang`,
  `--explain`.
- Sandbox: stdout is open for `echo` and `⎕←`; no primitive reaches the
  filesystem or the network.

### Data

- Dense arrays of bool, i64, f64 and characters, row-major; complex numbers
  as a core type; boxes; J's exact types — extended-precision integers and
  rationals.
- Zero-copy in and out for i64, f64 and i64-physical temporal columns, over
  the Arrow C data interface and `__array_interface__`: Polars, pandas 2,
  PyArrow and numpy work natively, with no dependency on any of them.
  Narrower types widen with one copy.
- Nulls, mixed-type table columns and non-contiguous numpy views are
  refused with an error naming the column and the fix, never guessed at.

### Testing

- Differential suites against black-box runs of the reference interpreters:
  3816 J expressions and 1024 APL expressions, recorded as snapshots and
  replayed on every `cargo test` with no interpreter present. libjay agrees
  everywhere except 29 APL sentences where it diverges on purpose, each
  recorded with its reason.

### Not in this release

- J: dyadic transpose, `{` catalogue, `e.` raze-in, format by
  specification, `".` numbers, symbols, sparse arrays, the Taylor and
  hypergeometric conjunctions, foreign conjunction `!:`, locales, adverb and
  conjunction assignment, multiple assignment, `throw.`/`catcht.`/`goto.`.
- APL: dyadic transpose, collating grade, character I/O `⍞`, I-beam,
  `⍢` under, `⌺` stencil, `⍠` variant, `&` spawn, function assignment,
  trains.
- One APL dialect only: Dyalog-specific behaviour is a planned dialect
  switch, not a supported reading today.
- The GPU f64 path is generated and type-checked but has never been
  executed: the measuring machine's Metal adapter has no `SHADER_F64`.
- C ABI: boxed, extended and rational results have no descriptor yet and
  are refused by name; input is copied at the boundary rather than borrowed.
- Arrow string, binary, list and dictionary columns; Decimal128; float16;
  byte-swapped data. No Rust macro for compile-time checking of an
  expression.
