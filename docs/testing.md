# Testing

`cargo test` is a closed system: libjay plus the files checked into the
repository, no subprocesses, no external binaries, predictable runtime. The
reference interpreters — jconsole for J, GNU APL for APL — are run only in an
explicit refresh step, and what they say is frozen into a snapshot file that
the ordinary test battery reads.

## The two modes

**Battery (always).** `crates/libjay/tests/oracle.rs` and `oracle_apl.rs`
evaluate every expression in `crates/libjay/tests/snapshots/` with libjay and
compare the answer to the recorded one, using the tolerance-aware token
comparison the suites have always used (1e-5 relative, 1e-9 absolute floor;
line structure is significant, column padding is not). A mismatch names every
differing expression. Both sides refusing is agreement; error texts are never
compared. This runs on CI for free.

**Refresh (on request).** The same two files also hold the tests that run the
live interpreters. They do nothing unless `LIBJAY_REFRESH_ORACLE` is set:

```sh
# re-measure and fail on any drift from the snapshots
LIBJAY_REFRESH_ORACLE=1 cargo test -p libjay --test oracle --test oracle_apl

# re-measure and rewrite the snapshots
LIBJAY_REFRESH_ORACLE=write cargo test -p libjay --test oracle --test oracle_apl
```

`LIBJAY_ORACLE_J` and `LIBJAY_ORACLE_APL` override the interpreter paths; in
refresh mode a missing interpreter is a failure, not a skip. Refreshing takes
about 30 seconds, nearly all of it spawning one process per expression.

The interpreters stay black-box oracles: never linked, never read. The
clean-room rule in CLAUDE.md is not relaxed by any of this.

## Snapshot files

`snapshots/j.snap`, `snapshots/apl.snap` and `snapshots/apl_divergences.snap`
are generated; the format is documented at the top of each. One record per
expression, line-tagged so a diff reads as one line per changed answer:

```
= 2 + 2        the sentence (`\n` is a newline in it, `\\` a backslash)
> 4            one line of the reference's answer
```

`< TEXT` is libjay's own answer, recorded only in the divergence file; `? TEXT`
is why the two are expected to differ; `@ io=N` sets `⎕IO` for the records
after it; `# TEXT` is a comment. A side whose single line is `<error>` refused
the sentence. Records are in expression-list order: the fixed lists as they
appear in the source, then the generated list in generator order. The
generator itself runs only on a refresh — the expressions it produced are
materialised in the file, so a normal test run just reads them.

The divergence file is the `KNOWN_DIVERGENCES` list: the places where libjay
deliberately answers differently from GNU APL. It records BOTH answers. The
battery holds libjay to its own recorded side; the refresh re-measures both
and fails if a pair has quietly converged, which is the signal that the note
(and the entry in docs/coverage.md) should go.

## Adding a primitive

1. Extend `FIXED` (or `NAMED`, `IO_ZERO`, `KNOWN_DIVERGENCES`, or the
   generator) in the relevant oracle test file.
2. Run the refresh in write mode against the live interpreters.
3. Read the diff of the snapshot file. It is the reference's verdict on the
   new expressions, and on anything the change moved: every line of it should
   be a line you meant to change.
4. Commit the snapshot with the code.

Where a guess and the reference disagree, the reference wins. A new expression
whose recorded answer is `<error>` on the reference side is a claim that the
sentence is illegal in that language — check that it is, rather than pinning a
typo.

## The rest of the suite

`tests/eval.rs`, `coverage.rs`, `boxes.rs`, `fuse.rs`, `timeseries.rs`,
`simd.rs` and `explain.rs` are hand-written expectations: values checked by
hand, parametrised over data, never asserting on call wiring. They are the
place for a primitive's edge cases and for the exact text of a diagnostic. The
snapshots are for breadth; these are for intent.
