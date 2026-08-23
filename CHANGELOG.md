# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [semantic versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- The Dyalog preset is a GATE. `cargo test -p libjay --test oracle_dyalog`
  replays all 1989 recorded Dyalog answers under `Dialect::dyalog()` and
  fails on any expression the preset does not match — the third differential
  battery, beside jconsole's and GNU APL's, and the same closed system: no
  subprocess, no interpreter, one case per corpus theme. What may differ is
  listed one expression at a time in `crates/libjay/tests/expected/dyalog.txt`,
  each row carrying its reason: 21 rows are a divergence libjay keeps on
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
