//! Differential tests against the reference J implementation.
//!
//! Every expression in `tests/corpus/j/*.txt` is evaluated by libjay and
//! compared with the answer jconsole gave when the theme was last recorded
//! into `tests/snapshots/j/*.snap`. This is a replay: no subprocess, no
//! external binary, nothing outside the repository.
//!
//! Recording is the other activity, and the only one that runs the
//! reference: `cargo run -p libjay-devtools -- record j` (docs/testing.md).
//! jconsole is a black-box oracle there — never linked, never read.
//!
//! A record holds one answer per implementation, under its key; J has the
//! one, `j:`.
//!
//! One test case per corpus file, so a failure names the theme; every
//! mismatch inside the file is reported at once.

use std::path::PathBuf;

use libjay_testkit::{Lang, replay};
use rstest::rstest;

#[rstest]
fn corpus(#[files("tests/corpus/j/*.txt")] path: PathBuf) {
    replay::corpus_file(Lang::J, &path);
}

/// The glob above is expanded when this binary is compiled. This one reads
/// the corpus directory as it is now, so a file added since the last build
/// is still held to having a recording.
#[test]
fn every_corpus_file_is_recorded() {
    replay::every_file_is_recorded(Lang::J);
}
