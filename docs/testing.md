# Testing

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
```

One snapshot per corpus file, so a theme's inputs and recordings sit side by
side and a recording touches only the files whose corpus changed. A theme is
whatever keeps a diff reviewable — `arithmetic`, `structural`, `boxes`,
`definitions`, `divergences`, `generated` and so on; `ls tests/corpus/j` is
the list, and a new one is a new `.txt` with its snapshot beside it.

A corpus file is one expression per line. Every other line begins with a
marker no sentence of either language can begin with:

```
// a comment; a blank line is ignored too
@ io=0                 ⎕IO for the lines after it (APL; 1 unless said)
2 + 2                  an expression
? why the two differ   a note (divergences.txt only)
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

## Divergences

`corpus/apl/divergences.txt` is where libjay answers differently from GNU APL
on purpose, each expression with a `? ` note saying why. Its snapshot records
BOTH answers. The replay holds libjay to its own recorded side and fails if
the two recorded answers have converged; `record` re-measures both and fails
on a pair that no longer disagrees, which is the signal that the note (and the
entry in docs/coverage.md) should go.

## Dyalog

Dyalog is a second APL, recorded under the `dyalog:` key beside GNU APL's.
It is not a gate and never will be while `Dialect::default()` is the
APL2/ISO line: an expression where libjay and Dyalog differ is the BACKLOG
of the Dyalog dialect — the work that dialect still has to do — not a
regression in this one. The replay counts those expressions and says so in
its per-theme line; nothing fails.

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

Neither number is a gate — the gate is GNU APL's column, replayed by
`cargo test` — but the `gnu` number is a useful guard beside it. Dialect
work is meant to change what the preset answers and nothing else, so the
`gnu` figure moving means the default moved with it, which is a regression
whether or not a corpus expression happens to catch it. Run both.

`corpus/apl/dyalog-probe.txt` is the theme aimed at that question: every
line is a place docs/coverage.md's "Which APL" table says the two lines
part, and libjay agrees with GNU APL on all of them, so one Dyalog
recording measures the gap in one run. The rows where libjay ALREADY
differs from GNU APL (`⍺←`, complex ordering, the vector replication count,
trains) are in `divergences.txt` instead, and its records carry the same
`dyalog:` column.

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

## The rest of the suite

`tests/eval.rs`, `coverage.rs`, `boxes.rs`, `definitions.rs`, `fuse.rs`,
`timeseries.rs`, `simd.rs` and `explain.rs` are hand-written expectations: values checked by
hand, parametrised over data, never asserting on call wiring. They are the
place for a primitive's edge cases and for the exact text of a diagnostic. The
corpora are for breadth; these are for intent.

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
