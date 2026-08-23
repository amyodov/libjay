//! Differential tests of the Dyalog PRESET against Dyalog APL.
//!
//! The third gate, beside `oracle.rs` (jconsole) and `oracle_apl.rs` (GNU
//! APL). Those two hold the shipped dialects to the implementations they
//! follow; this one holds `Dialect::dyalog()` to the `dyalog:` column of the
//! same snapshots. It is the same replay — no subprocess, no external
//! binary, nothing outside the repository — asked of a different dialect.
//!
//! Every recorded Dyalog answer is gated, wherever it lives: the four
//! Dyalog-only themes marked `@ reference=dyalog`, the probe theme, the
//! divergence file, and the ordinary themes that carry the column beside
//! GNU APL's. What may differ is listed, one expression per row with the
//! reason, in `tests/expected/dyalog.txt` — a `divergence` libjay keeps on
//! purpose, or a `gap` that is queued in docs/status.md. Anything else
//! differing fails, and so does a listed row that has stopped differing, so
//! closing a gap deletes its row and the gate tightens.
//!
//! The default dialect is untouched by any of this: `oracle_apl.rs` still
//! holds it to GNU APL, expression for expression.

use std::path::PathBuf;

use jay::{Dialect, Lang};
use libjay_testkit::{IMPL_DYALOG, dialect};
use rstest::rstest;

#[rstest]
fn corpus(#[files("tests/corpus/apl/*.txt")] path: PathBuf) {
    dialect::gate_file(Lang::Apl, IMPL_DYALOG, Dialect::dyalog(), &path);
}

/// The glob above is expanded when this binary is compiled, and it is per
/// theme. This reads the corpus as it is now and holds every row of the
/// list to naming an expression that still has a recorded answer.
#[test]
fn expected_list_is_live() {
    dialect::list_is_live(Lang::Apl, IMPL_DYALOG);
}
