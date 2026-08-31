# Testing

How many expressions the corpus holds right now is a question for
`jay-corpus stats j` and `jay-corpus stats apl` — the per-theme and total
counts live there, not in prose that would go stale.

Two activities, and keeping them apart is the point.

**Collecting** takes a list of expressions, runs a reference interpreter on
each, and records what it answered. It is explicit, needs the interpreters,
and never happens during a test run: `cargo run -p libjay-devtools -- record`.

**Testing** replays those recordings: libjay evaluates every expression and
its answer is compared with the recorded one. `cargo test` is a closed
system — libjay plus the files checked into the repository, no subprocesses,
no external binaries, predictable runtime — and it is what CI runs.

## The files

```
crates/libjay/tests/corpus/j/arithmetic.txt        the inputs
crates/libjay/tests/snapshots/j/arithmetic.snap    what jconsole answered
crates/libjay/tests/expected/dyalog.txt            what a preset may differ on
```

One snapshot per corpus file, so a theme's inputs and recordings sit side by
side and a recording touches only the files whose corpus changed. The third
file is per DIALECT rather than per theme, and only a dialect that follows
an implementation libjay does not ship as its default has one ("The third
gate"). A theme is
whatever keeps a diff reviewable — `arithmetic`, `structural`, `boxes`,
`definitions`, `divergences`, `generated` and so on; `ls tests/corpus/j` is
the list, and a new one is a new `.txt` with its snapshot beside it.

A corpus file is one expression per line. Every other line begins with a
marker no sentence of either language can begin with:

```
// a comment; a blank line is ignored too
@ io=0                 ⎕IO for the lines after it (APL; 1 unless said)
@ reference=dyalog     the whole theme is that implementation's data
2 + 2                  an expression
? why the two differ   a note (divergences.txt and an expected list only)
```

The comment marker is `//`, not `#`, because `#` is J's tally: `# i. 5 2` is
an expression. `\n` inside an expression is a newline, so a multi-sentence
program is one line; `\\` is a backslash.

A snapshot is line-tagged plain text, one line per line of output, so a diff
reads as one line per changed answer. A record is a MAP: the sentence, then
one group of lines per implementation that has been run over it.

```
= 2+2          the sentence
> gnu: 4       one line of what GNU APL answered
> dyalog: 4    one line of what Dyalog answered
```

The implementation keys are `j` (jconsole), `gnu` (GNU APL) and `dyalog`
(Dyalog APL); the namespace is open, and a key this build does not know is
read and rewritten rather than dropped. libjay is held to the one its
dialect FOLLOWS — `j` for J, `gnu` for APL, since `Dialect::default()` is
the APL2/ISO line GNU APL embodies. Every other key is recorded data: see
"Dyalog" below.

`< TEXT` is libjay's own answer, recorded only in the divergence file; `? TEXT`
is why libjay and the implementation it follows are expected to differ;
`@ io=N` sets `⎕IO` for the records after it; `# TEXT` is a comment. A side
whose single line is `<error>` refused the sentence. The format is documented
at the top of every snapshot file.

## Collecting

```sh
# every corpus file of a language
cargo run -p libjay-devtools -- record j
# one theme (a path, or the bare name)
cargo run -p libjay-devtools -- record j arithmetic
# re-measure and fail on any drift, writing nothing
cargo run -p libjay-devtools -- record j --check
# record only what the snapshot does not have yet
cargo run -p libjay-devtools -- record j --missing
# a second implementation of the same language, into its own key
cargo run -p libjay-devtools -- record apl --impl dyalog
# corpus and snapshot sizes
cargo run -p libjay-devtools -- stats
```

`--impl` names the implementation to record: `j` for J, `gnu` (the default)
or `dyalog` for APL. A run writes THAT key and no other, so recording Dyalog
leaves every GNU APL answer, and libjay's own recorded side, exactly as they
were; `--missing` is per key as well.

`LIBJAY_ORACLE_J`, `LIBJAY_ORACLE_APL` and `LIBJAY_ORACLE_DYALOG` override
the interpreter paths. A missing interpreter is a failure for the
implementation libjay is held to, and a clean skip for one it merely
records. Recording the whole of one language takes a few seconds — one
process per expression, run in parallel.

Every runner pins the page before it asks anything, because an interpreter
that abbreviates a wide or a tall result would have that abbreviation
recorded as the answer: jconsole is fed `9!:37 ] 0 4096 0 4096` as its
first sentence (its default output control is `0 256 0 222`, which ends a
long vector in `...`), GNU APL is run with `--PW 10000`, and the Dyalog
script sets `⎕PW←32767`.

`jay-corpus` is the only thing in the repository that spawns an interpreter.
They stay black-box oracles: never linked, never read. The clean-room rule in
CLAUDE.md is not relaxed by any of this.

The generated corpora come from the same binary:

```sh
cargo run -p libjay-devtools -- gen j --count 300 --seed 0x9E3779B97F4A7C15
```

`--count` is rounds, each of which emits several expressions (eight for J,
twelve for APL); the defaults are the ones the checked-in files were drawn
with. It appends to `corpus/<lang>/generated.txt`, skipping what is already
there, and from then on the generated expressions are ordinary corpus lines:
the generator plays no part in a test run.

## Testing

`cargo test -p libjay` replays every corpus file — one parameterised case per
file, so a failure names the theme and lists every expression in it that
disagrees. The comparison is tolerance-aware: both sides are compared line by
line (the line structure carries the shape) and token by token within a line,
each token parsed back to `f64` with a 1e-5 relative tolerance and a 1e-9
absolute floor, which covers the rounding of a 6-significant-digit printer and
nothing more. Column padding is not compared. Both sides refusing is
agreement; error texts are never compared.

An expression in a corpus file with no record in the snapshot is a failure:
`unrecorded: run jay-corpus record`.

Three batteries replay those recordings: `oracle.rs` holds libjay to
jconsole, `oracle_apl.rs` holds it to GNU APL, and `oracle_dyalog.rs` holds
`Dialect::dyalog()` to the `dyalog:` column of the same APL snapshots. The
first two gate the dialects libjay ships; the third gates a preset, and has
an exemption list ("The third gate").

## Programs

`corpus/j/programs.txt` and `corpus/apl/programs.txt` are the theme where a
corpus line is a whole PROGRAM rather than a sentence: several assignments
feeding one another, a defined verb or two, control flow where the language
has it, and a final value small enough to read. A sieve, a discrete Fourier
transform and its inverse, an OHLCV pipeline, text statistics through key or
partitioned enclose, Game of Life, sorting and ranking, base-conversion round
trips, run-length coding, a regression by inner product, matrix powers, and a
loop with a branch inside it. The two files pose the same problems in the two
languages and answer alike wherever the problem does not turn on the index
origin.

They are recorded and replayed exactly like every other theme —
`cargo run -p libjay-devtools -- record j programs` — and the multi-line
programs are written the way the format has always allowed, one line with
`\n` between sentences.

Two rules make a program worth keeping:

- **The final value is modest.** Intermediates are as large as the program
  needs; what is recorded is a short vector, so the snapshot diff stays
  readable and a changed answer names itself.
- **Floats are rounded before the last sentence.** The replay's tolerance
  would cover them, but a recorded answer of `_268 12100 11266` is a fact
  and `9.16667` is a rounding of the printer's. Where a float is the point —
  a variance, a round trip — the program reports `-:` or `≡` of the
  comparison rather than the number.

A new program goes in the same way a new expression does: write it, run it
past the oracle by recording the theme, read the snapshot diff, and commit
the corpus and the snapshot together. The reference's verdict is the answer,
including its verdict that the program is illegal.

## Sweeping

`fuzz --compare` draws composed expressions, runs both sides over them and
reports where they part. With `--signature` each mismatch is first cut down
to the smallest sentence that still parts the two sides the same way, and
signed by its CAUSE — the verdict, libjay's answer class, and the
primitives the sentence names — so a wrapper can tell a batch that found a
new cause from one that found another spelling of a cause it has.

The run also reads `corpus/<lang>/divergences.txt` and measures each of its
rows against the oracle before it starts. A mismatch that matches a row —
by the minimised sentence, or by the cause signature — is a difference the
corpus already records with both answers and a reason, so it is counted
under `accepted`, kept out of the signature ranking, and printed with the
row that excused it. Both numbers are reported:

```
generation 2: 5000 expressions, 68 mismatches (1.4%)
  raw agreement                98.64%
  accepted-adjusted agreement  98.86%  (11 accepted, 57 unexplained)
  accepted        11  (0.2%)
  agree         4932  (98.6%)
  ...
accepted divergences matched (…/corpus/j/divergences.txt):
     10  5 <./\. (1;2)
      1  0 >./\. (<'abc')
```

`--no-accepted` turns the list off and counts every mismatch against
agreement. Nothing but a line of the divergence file is ever excused, and
each of those is reasoned in docs/coverage.md.

## Divergences

`corpus/apl/divergences.txt` and `corpus/j/divergences.txt` are where libjay
answers differently from GNU APL and from jconsole on purpose, each
expression with a `? ` note saying why. Its snapshot records
BOTH answers. The replay holds libjay to its own recorded side and fails if
the two recorded answers have converged; `record` re-measures both and fails
on a pair that no longer disagrees, which is the signal that the note (and the
entry in docs/coverage.md) should go.

## Extensions

A non-standard extension (docs/extensions.md) is never recorded against an
oracle: the corpus is what the reference implementations answer, and an
extension answers something else on purpose. Flagged behaviour is pinned by
hand in `crates/libjay/tests/extensions.rs` and
`python/tests/test_extensions.py`, each assertion beside the spec-correct
answer it replaces. Nothing in the corpus compiles under a flag, and the
recorder never sets one.

## Dyalog

Dyalog is a second APL, recorded under the `dyalog:` key beside GNU APL's.
The shipped dialect is not held to it — an expression where the APL2/ISO
line and Dyalog differ is not a regression in this one — so `oracle_apl.rs`
counts those expressions and says so in its per-theme line and fails on
none of them.

The PRESET aimed at that line is held to it. `oracle_dyalog.rs` is the
third gate, beside jconsole's and GNU APL's: it replays every recorded
`dyalog:` answer under `Dialect::dyalog()`, in the same closed system, and
fails on any difference that is not listed. See "The third gate" below.

```sh
# what the two disagree about, expression by expression
cargo run -p libjay-devtools -- stats apl --dialect-diff
# the same question asked of the Dyalog preset
cargo run -p libjay-devtools -- stats apl --dialect-diff --dialect dyalog
# how many answers each key holds
cargo run -p libjay-devtools -- stats apl
```

`--dialect NAME` names the preset libjay itself runs under while the
backlog is measured: `gnu` (the default, the shipped dialect) or `dyalog`.
It changes only libjay's side — the recorded answers are read as they
stand, and no interpreter is started — so it is the offline measure of how
much of the backlog the preset already answers, and the difference between
the two numbers is what a wave of dialect work bought. The closing line
names the dialect it ran under, so the two runs are told apart by their own
output.

The `dyalog` number is what the gate below enforces; the `gnu` number is a
guard beside it. Dialect work is meant to change what the preset answers
and nothing else, so the `gnu` figure moving means the default moved with
it, which is a regression whether or not a corpus expression happens to
catch it. Run both.

`corpus/apl/dyalog-probe.txt` is the theme aimed at that question: every
line is a place docs/coverage.md's "Which APL" table says the two lines
part, and libjay agrees with GNU APL on all of them, so one Dyalog
recording measures the gap in one run. The rows where libjay ALREADY
differs from GNU APL (`⍺←`, complex ordering, the vector replication count,
trains) are in `divergences.txt` instead, and its records carry the same
`dyalog:` column.

### Dyalog-only themes

`dyalog-dfns.txt`, `dyalog-dops.txt`, `dyalog-control.txt` and
`dyalog-operators.txt` are the other half of the question. Where the probe
asks what the two APLs answer differently, these hold what GNU APL cannot
be asked at all: dfn guards, `⍺←` defaults, `∇` recursion, `⍺⍺`/`⍵⍵`
operators, the `:If` family, and the operators GNU APL has no character for
(`⌸`, `⌺`, `⍥`, `⍛`, `⊆`, `⍠`, `∘`, `f⍤g`, `f⍣g`). Their records carry a
`dyalog:` line and no `gnu:` one, and their first line is the directive
that says so:

```
@ reference=dyalog
```

A theme marked that way is reference DATA for the SHIPPED dialect: the
recorder writes only that key into it and skips the file when asked for
another, and `oracle_apl.rs` evaluates every line, counts how many the
default already matches, and fails on none of them —
the same treatment a `dyalog:` answer gets anywhere else, applied to a
whole file. `every_corpus_file_is_recorded` holds such a file to having a
`dyalog:` answer per line rather than a `gnu:` one, so a line added and
never recorded is still caught. The Dyalog preset IS held to those lines,
by the gate below, exactly as it is held to a `dyalog:` answer in an
ordinary theme.

Control structures belong to a defined function, and in `dyalog-control.txt`
that function is fixed with `⎕FX` rather than written between two `∇`s. The
reason is the channel, not the language: Dyalog is driven here as a piped
session, where opening the `∇` editor makes it print a `[1]` prompt per line
and echo the body, so a `∇`-defined tradfn cannot be recorded through it.
`⎕FX` takes the same lines as a vector of character vectors and fixes the
same function.

The same channel is why a `∇`-definition anywhere else in the corpus —
`definitions.txt` and `wave5.txt` hold twenty of them — is sent to Dyalog as
the `⎕FX` that fixes the same function. `dyalog::as_fx` does the rewrite,
and it is the one place the text an oracle is asked is not the corpus text:
the corpus keeps the `∇` spelling, because that is the sentence libjay is
asked, and Dyalog's own account of `⎕FX` is that the two spellings define
the same function. `⎕FX`'s result is shy, so it displays nothing of its own.
A `∇` the rewrite is not sure of — one that never closes, or a body line
that opens another definition — is passed through untouched, so whatever
Dyalog says about it is still what gets recorded.

### The third gate

`cargo test -p libjay --test oracle_dyalog` replays every recorded
`dyalog:` answer under `Dialect::dyalog()` and fails on any expression the
preset does not match. It is the same closed system as the other two
batteries — one case per corpus theme, every mismatch in a theme reported
at once, no interpreter — asked of a different dialect. Where it lives:

```
crates/libjay/tests/expected/dyalog.txt    what may differ, and why
```

Nothing else may. A difference the file does not carry fails the run, and a
row of the file that has STOPPED differing fails it too, so closing a gap
means deleting its rows and the gate tightens by itself. The file is in the
corpus format, with the `? ` note required rather than forbidden:

```
⍴(1 1⍴5)+,3
? gap singleton-rank: the first argument's rank wins here
1 2 3⋄4 5
? divergence sequence-value: only the last sentence has a value here
```

A note reads `KIND TAG: reason`. The kind is the promise:

- `divergence` is a decision — a rule libjay keeps in every dialect, or a
  place the recorded answer is the reference's own edge. Nothing is queued.
- `gap` is a not-yet. Its TAG names a row of docs/status.md's Dyalog table,
  which is the queue, so a reader can go from an exempted expression to the
  work that would stop exempting it.

The tag groups rows by cause, and the battery prints the split — how many
rows are a divergence, how many a gap, and the largest causes — so the
shape of the backlog is one line of test output rather than a re-derivation.

`jay-corpus stats apl --dialect-diff --dialect dyalog` measures the same
set from the outside and lists the differing expressions with both answers,
which is how a row's text is obtained when one has to be added.

The default dialect is untouched by any of this. `oracle_apl.rs` still
holds it to GNU APL expression for expression, and a preset that changed
what the default answers fails there.

### Installing Dyalog and recording it

Dyalog's download is account-gated, so it is on the recording machine only:
never in the repository, never in CI, never in a wheel. The clean-room rule
holds for it as for every other oracle — run it, never read it.

1. Install Dyalog 19.0 for macOS. The recorder looks for
   `/Applications/Dyalog-19.0.app/Contents/Resources/Dyalog/mapl` (any
   `Dyalog-*.app`, highest version first), then `/opt/mdyalog/...`, then
   `mapl` or `dyalog` on `PATH`. Anywhere else, say where:

   ```sh
   export LIBJAY_ORACLE_DYALOG=/path/to/mapl
   ```

2. Record. The first run checks the invocation on `2+2` before touching
   anything, and reports what it found if the answer is not `4`:

   ```sh
   cargo run -p libjay-devtools -- record apl --impl dyalog
   ```

3. Read the summary — `libjay agrees on N and differs on M` — and the diff
   of the snapshots, which now carry a `dyalog:` line per record. Commit
   them: they are the dialect backlog, checked in.

4. From then on, `record apl --impl dyalog --check` re-measures and reports;
   it fails on nothing. `record apl --check` (GNU APL) keeps failing on
   drift, as before.

The invocation is `mapl -script FILE`: the sentence goes into a temporary
script bracketed by two printed markers, under `⎕PW←32767 ⎕PP←10 ⎕ML←1` and
the record's `⎕IO`, ending in `⎕OFF`. Only what the interpreter prints
BETWEEN the markers is kept, so a banner or any session noise is dropped
without being parsed; trailing spaces and the blank lines at either end go,
interior blank lines stay (they carry the shape of a rank-3 result). A
sentence whose closing marker never arrives — Dyalog abandons a script at
the first error — is recorded as `<error>`, as is one whose output holds a
named error or a caret line.

That invocation was written from Dyalog's published documentation with no
interpreter to try it on. Every assumption is listed at the top of
`crates/libjay-devtools/src/dyalog.rs`, and two environment variables
correct the two likeliest mistakes without a rebuild:
`LIBJAY_ORACLE_DYALOG_FLAGS` replaces `-script` (add the version's quiet
flag if a banner or an input echo shows up), and
`LIBJAY_ORACLE_DYALOG_STDIN` feeds the script on stdin instead of naming a
file.

## Adding a primitive

1. Add lines to the corpus file for the theme (a new theme is a new `.txt`,
   and its snapshot appears beside it).
2. `cargo run -p libjay-devtools -- record j corpus/j/arithmetic.txt` — or
   `--missing` to leave the rest of the file alone.
3. Read the diff of the snapshot. It is the reference's verdict on the new
   expressions, and on anything the change moved: every line of it should be
   a line you meant to change.
4. Commit the corpus and the snapshot together with the code.

Where a guess and the reference disagree, the reference wins. A new expression
whose recorded answer is `<error>` on the reference side is a claim that the
sentence is illegal in that language — check that it is, rather than pinning a
typo.

## Measuring the corpus

The corpus is a sample of a combinatoric space, and `jay-corpus coverage`
says which part of it the sample reaches:

```
cargo run -p libjay-devtools -- coverage j
cargo run -p libjay-devtools -- coverage apl --tsv /tmp/empty.tsv
```

It runs no interpreter and writes nothing to the corpus. Every expression
is compiled by libjay, the fusion pass is undone, and each operand subtree
is run so that the type-class and rank-class recorded are the ones the
primitive actually met. The report has four parts: the published
vocabulary from `docs/status.md` with the spellings the corpus never
mentions; a grid per valence, ordered so the primitives used often and
narrowly come first; the classes fewest primitives ever reach; and the
operator layer — for each modifier, the operand verbs and the noun classes
it was applied to.

A cell is one primitive in one valence meeting one operand class (a pair,
for a dyad). The denominator is the corpus's own reach — the cells it
builds for some primitive — not the full cross product, which would make
every row look empty. `--json` writes the whole measurement and `--tsv`
the empty cells alone, one per line, for a generation stage to read. What
the measurement cannot see is printed rather than assumed: sentences
libjay refuses, sites whose modifier hands its operand something not
nameable here, and the spellings the frontend rewrites into another form.
The reasoning is in `docs/decisions.md`.

## Fuzzing against the oracle

`jay-corpus fuzz` composes expressions rather than drawing one verb over one
noun, and `--compare` runs libjay and the oracle over them and reports where
they part. Nothing is written to the corpus: a line worth keeping is moved
into `corpus/<lang>/fuzz_found.txt` by hand, which is what turns a find into
a regression.

```
cargo run -p libjay-devtools -- fuzz apl --compare --count 500 --seed 1 --signature
```

`--signature` first CUTS each mismatch down. A composed sentence is a tree
with a bug somewhere inside it, so the run takes it apart by parenthesised
group — the inside of one on its own, or that group replaced by a plain `2`
— shortest candidate first, and keeps whatever still parts the two sides the
same way, repeating until nothing smaller does. What is reported, and what a
corpus line should be pasted from, is that smallest sentence; where it
differs from the drawn one a `cut from:` line names what it came out of.

The signature is then `<how the two sides parted>:<what libjay made of
it>|<the primitives the sentence names>`, and the summary counts the run's
distinct causes (the part before the bar) as well as its distinct
signatures. Because the sentence was cut down first, it names one or two
primitives rather than the eight or ten of the draw, which is what makes the
whole signature a name for the CAUSE — and a sentence still naming more than
three after the cut is one the cut could not take apart, so its field
becomes `…` and every such sentence of one class is one finding. A wrapper
that sweeps continuously should keep a set of whole signatures:
deduplicating on the expression text can never say "nothing new", the space
of compositions being effectively infinite, and deduplicating on the class
alone puts every numeric-vector wrong answer in one bucket.

Cutting costs one run of libjay and one of the oracle per candidate, and
only mismatches are cut, so a 150-expression batch pays about a second for
it. Candidates whose libjay answer already rules the verdict out are thrown
away without starting an interpreter.

The grammar carries a generation number (`fuzz::GENERATION`, printed in the
summary): two runs' find rates are comparable only when it matches. Anything
whose count is unbounded is deliberately absent from the pools — `u^:_`,
`u^:a:` and `f⍣≡` all converge, and a generator that can hang has no oracle.

## The rest of the suite

`tests/eval.rs`, `coverage.rs`, `boxes.rs`, `definitions.rs`, `fuse.rs`,
`timeseries.rs`, `simd.rs`, `stress.rs` and `explain.rs` are hand-written expectations: values checked by
hand, parametrised over data, never asserting on call wiring. They are the
place for a primitive's edge cases and for the exact text of a diagnostic. The
corpora are for breadth; these are for intent.

## The usage stress suite

The corpora ask what a sentence means. The stress suite asks what happens
when the same sentence is asked ten thousand times, from eight threads at
once, with the pool sized differently, and with refusals interleaved. It
answers in the only currency the rest of the suite uses — data — plus one
measurement that is not data:

```
crates/libjay/tests/stress.rs         the Rust surface
crates/libjay-capi/tests/stress.rs    the C ABI
python/tests/test_stress.py           the Python surface
```

Nothing in it is `#[ignore]`d and nothing needs an interpreter; it is part of
`cargo test` and `pytest` like everything else, and each binary is well under
a minute.

What each file holds:

- **Repetition.** Hundreds of compile-and-run cycles over the same handful of
  programs, every answer held to the first one's, with a failed compile and a
  failed run inside the cycle so that the error paths are exercised as often
  as the good ones.
- **Resident memory.** The one non-answer. It is read after a warm-up and
  again at the end, and compared as a RATIO (1.5, with a small additive slack)
  rather than a megabyte figure: a byte count is a property of the machine and
  its allocator, a ratio is a property of the code. Where the reading cannot
  be taken the case says so and passes — an environment without `ps` is not a
  regression. The baseline comes after the warm-up because what a first pass
  costs (the pool, the lazily built tables) is not a leak.
- **The pool.** `LIBJAY_THREADS` is read once per process and frozen, so the
  1/2/4 sweep is three CHILD PROCESSES, not three loops: the Rust case
  re-execs its own test binary with `--exact` and a marker in the
  environment, the Python case runs `sys.executable -c`. All three must print
  the same digest. The programs answer integers so that "the same" is exact
  and no reassociated float sum can make the case flap.
- **Concurrency.** One compiled program shared by eight threads, every thread
  checking every answer. A `Program` is read-only, and a `Kernel` bound in one
  thread must not be visible from another.
- **Refusals.** Every kind of refusal the surface offers, in a loop, each one
  held to its `ErrorKind` and to carrying a message — and after each round the
  good programs still answer what they answered.

Adding a case: put the program in the file's own work list rather than
writing a fresh one beside it, so that every case gains it. A program belongs
there if it ends in an integer, runs in milliseconds, and touches more than
one stage of the engine.

## CI

What runs where, and why the split matters:

- **Every push and pull request** (`ci.yml`): `cargo test` — the replay
  battery, snapshots only, no interpreter, no network — plus clippy, the
  Python suite on 3.10 and 3.14, a `cargo doc` pass with warnings denied
  (what docs.rs would fail on, caught before the push reaches it), a
  `cargo publish --dry-run` packaging check, and one Python matrix leg that
  builds the sdist and installs from it rather than from a wheel.
- **Weekly, and on demand** (`publish.yml`'s `schedule` and
  `workflow_dispatch` triggers): the full wheel matrix and the C ABI bundles
  build for real, publishing nothing — the `publish`/`crates` jobs stay
  gated on the release event. This is what catches a runner image or
  toolchain drifting under the matrix between releases, when nothing else
  would touch it.
- **Only on the recording machine, only by hand**: `jay-corpus record`,
  which is the one thing in the repository that spawns the reference
  interpreters (see CLAUDE.md's oracle paths). CI never runs it and never
  has the interpreters installed; the snapshots it produces are what CI
  replays.

## Where the code is

- `crates/libjay-testkit` — the corpus and snapshot formats, the comparison,
  and the replay. Shared by the tests and the recorder, so neither has a
  private copy of it. Not published.
- `crates/libjay-devtools` — the `jay-corpus` binary: the subprocess logic for
  both interpreters, the generators, the recording commands, and the
  coverage measurement (`coverage.rs`, with the vocabulary reader in
  `inventory.rs`). Not published.
