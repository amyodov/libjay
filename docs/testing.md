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
reads as one line per changed answer:

```
= 2 + 2        the sentence
> 4            one line of the reference's answer
```

`< TEXT` is libjay's own answer, recorded only in the divergence file; `? TEXT`
is why the two are expected to differ; `@ io=N` sets `⎕IO` for the records
after it; `# TEXT` is a comment. A side whose single line is `<error>` refused
the sentence. The format is documented at the top of every snapshot file.

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
# corpus and snapshot sizes
cargo run -p libjay-devtools -- stats
```

`LIBJAY_ORACLE_J` and `LIBJAY_ORACLE_APL` override the interpreter paths; a
missing interpreter is a failure, not a skip. Recording the whole of one
language takes a few seconds — one process per expression, run in parallel.

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

## The rest of the suite

`tests/eval.rs`, `coverage.rs`, `boxes.rs`, `definitions.rs`, `fuse.rs`,
`timeseries.rs`, `simd.rs` and `explain.rs` are hand-written expectations: values checked by
hand, parametrised over data, never asserting on call wiring. They are the
place for a primitive's edge cases and for the exact text of a diagnostic. The
corpora are for breadth; these are for intent.

## Where the code is

- `crates/libjay-testkit` — the corpus and snapshot formats, the comparison,
  and the replay. Shared by the tests and the recorder, so neither has a
  private copy of it. Not published.
- `crates/libjay-devtools` — the `jay-corpus` binary: the subprocess logic for
  both interpreters, the generators, and the recording commands. Not
  published.
