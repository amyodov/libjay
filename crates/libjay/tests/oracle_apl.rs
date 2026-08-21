//! Differential tests against GNU APL.
//!
//! Every expression in `tests/corpus/apl/*.txt` is evaluated by libjay and
//! compared with the answer GNU APL gave when the theme was last recorded
//! into `tests/snapshots/apl/*.snap`. This is a replay: no subprocess, no
//! external binary, nothing outside the repository.
//!
//! A record holds one answer per implementation. libjay is held to `gnu:`,
//! the line its shipped dialect follows; a `dyalog:` answer is recorded
//! data for a future Dyalog dialect, and the battery COUNTS the expressions
//! where it differs rather than failing on them —
//! `jay-corpus stats apl --dialect-diff` lists them.
//!
//! Recording is the other activity, and the only one that runs the
//! reference: `cargo run -p libjay-devtools -- record apl`
//! (docs/testing.md). GNU APL is a black-box oracle there — never linked,
//! never read.
//!
//! Both dialects are Iverson-family but not the same language. Where the
//! difference is deliberate it lives in `corpus/apl/divergences.txt`, whose
//! snapshot records BOTH answers: this replay holds libjay to its side, and
//! the recording re-checks that the two still disagree, so a silent drift on
//! either side is a failure rather than a surprise.
//!
//! One test case per corpus file, so a failure names the theme; every
//! mismatch inside the file is reported at once. The index origin travels
//! with the expression: `@ io=0` in a corpus file puts the lines after it
//! under `⎕IO←0`.

use std::path::PathBuf;

use libjay_testkit::{Lang, replay};
use rstest::rstest;

#[rstest]
fn corpus(#[files("tests/corpus/apl/*.txt")] path: PathBuf) {
    replay::corpus_file(Lang::Apl, &path);
}

/// The glob above is expanded when this binary is compiled. This one reads
/// the corpus directory as it is now, so a file added since the last build
/// is still held to having a recording.
#[test]
fn every_corpus_file_is_recorded() {
    replay::every_file_is_recorded(Lang::Apl);
}
