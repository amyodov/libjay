//! Differential tests against GNU APL.
//!
//! `cargo test` runs the SNAPSHOT batteries: every expression in
//! `tests/snapshots/apl.snap` and `tests/snapshots/apl_divergences.snap` is
//! evaluated by libjay and compared with the recorded answer. No subprocess,
//! no external binary, nothing outside the repository.
//!
//! GNU APL itself is run only by the refresh tests, gated on
//! `LIBJAY_REFRESH_ORACLE` (docs/testing.md). It is built from the FSF
//! tarball into `~/projects/libjay-oracles/` and only ever executed;
//! `LIBJAY_ORACLE_APL` overrides the path.
//!
//! Comparison is textual with numeric tolerance. libjay prints 6 significant
//! digits and GNU APL prints `⎕PP` (10) of them, so each token is parsed back
//! to `f64` and compared with a relative tolerance of 1e-5 — the ~5e-6 our
//! own rounding can introduce, and no more — with a 1e-9 absolute floor for
//! values near zero.
//!
//! Both dialects are Iverson-family but not the same language. Where the
//! difference is deliberate it lives in `KNOWN_DIVERGENCES`, whose snapshot
//! records BOTH answers: the battery holds libjay to its side, and the
//! refresh re-checks that the two still disagree, so a silent drift on
//! either side is a test failure rather than a surprise.

mod common;

use std::process::{Command, Stdio};

use common::{Record, Side};
use jay::fmt::{format_array, FmtOpts};
use jay::{compile, Dialect, Lang};

const SNAP: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots/apl.snap");
const DIVERGENCES: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots/apl_divergences.snap");

/// What `LIBJAY_REFRESH_ORACLE` asks for. Absent means the refresh is off.
enum Refresh {
    /// Run the reference and fail on any drift from the snapshot.
    Verify,
    /// Run the reference and rewrite the snapshot.
    Write,
}

fn refresh_mode() -> Option<Refresh> {
    match std::env::var("LIBJAY_REFRESH_ORACLE").ok()?.as_str() {
        "" | "0" => None,
        "1" => Some(Refresh::Verify),
        "write" => Some(Refresh::Write),
        other => panic!("LIBJAY_REFRESH_ORACLE={other:?}: expected `1` or `write`"),
    }
}

fn oracle_path() -> String {
    let path = std::env::var("LIBJAY_ORACLE_APL").unwrap_or_else(|_| {
        format!(
            "{}/projects/libjay-oracles/gnu-apl/install/bin/apl",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    assert!(
        std::path::Path::new(&path).exists(),
        "the refresh needs the GNU APL oracle; {path} is not there (set LIBJAY_ORACLE_APL)"
    );
    path
}

/// `--script` silences the banner and the input echo, `--safe` and `--noSV`
/// keep the interpreter from opening sockets or loading a workspace, and a
/// wide `⎕PW` stops long vectors from wrapping onto continuation lines. The
/// interpreter drops a `.apl.history` file in its working directory, so it
/// is run somewhere that is not the repository.
fn oracle_eval(apl: &str, expr: &str, index_origin: u8) -> Option<String> {
    let line = if index_origin == 1 { expr.to_string() } else { format!("⎕IO←0⋄{expr}") };
    let child = Command::new(apl)
        .args(["--script", "--safe", "--noSV", "--PW", "10000", "--eval", &line])
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn GNU APL");
    let out = child.wait_with_output().expect("wait for GNU APL");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // GNU APL always exits 0; a failed sentence is reported on stderr as a
    // named error plus a caret line under the offending glyphs.
    if !stderr.trim().is_empty() || has_error_marker(&stdout) {
        return None;
    }
    Some(normalize(&stdout))
}

fn has_error_marker(text: &str) -> bool {
    text.lines().any(|line| {
        let t = line.trim();
        t.ends_with("ERROR")
            || t.ends_with("ERROR+")
            || (t.contains('^') && t.chars().all(|c| c == '^' || c == ' '))
    })
}

/// The two printers differ only at the edges of the page: GNU APL pads rows
/// on the right and ends every result with a blank line, libjay does
/// neither. Trailing spaces and the leading and trailing blank lines go;
/// INTERIOR blank lines stay, because both sides use them the same way — one
/// per axis step above the last two — and dropping them would stop the
/// comparison from seeing the shape of a rank-3 or rank-4 result.
fn normalize(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(lines.len());
    let end = lines.iter().rposition(|l| !l.is_empty()).map_or(start, |i| i + 1);
    lines[start..end].join("\n")
}

/// Run one sentence through libjay. None on error.
fn jay_eval(expr: &str, index_origin: u8) -> Option<String> {
    let dialect = Dialect { index_origin: Some(index_origin as i64) };
    let program = compile(Lang::Apl, expr, &dialect).ok()?;
    let mut sink = |_: &str| {};
    let result = program.run(&[], &mut sink).ok()??;
    Some(format_array(&result, &FmtOpts::APL))
}

/// Both printers spell a negative with `¯`; GNU APL writes the exponent
/// marker in upper case and libjay in lower, and `f64::from_str` accepts
/// either. `∞` is libjay's alone — GNU APL has no infinity — so it never
/// parses and falls through to the textual comparison, which is what a
/// divergence should do.
fn parse_apl_num(tok: &str) -> Option<f64> {
    tok.replace('¯', "-").parse::<f64>().ok()
}

fn tokens_match(a: &str, b: &str) -> bool {
    match (parse_apl_num(a), parse_apl_num(b)) {
        (Some(x), Some(y)) => {
            if x.is_infinite() || y.is_infinite() {
                return x == y;
            }
            let scale = x.abs().max(y.abs());
            (x - y).abs() <= 1e-9 + 1e-5 * scale
        }
        _ => a == b,
    }
}

fn outputs_match(ours: &str, theirs: &str) -> bool {
    // Compare line structure (it encodes shape) and tokens within lines.
    // Column padding is not compared: the two printers align a mixed column
    // differently, and the alignment is not the semantics.
    let ol: Vec<&str> = ours.lines().collect();
    let tl: Vec<&str> = theirs.lines().collect();
    if ol.len() != tl.len() {
        return false;
    }
    ol.iter().zip(&tl).all(|(o, t)| {
        let ot: Vec<&str> = o.split_whitespace().collect();
        let tt: Vec<&str> = t.split_whitespace().collect();
        ot.len() == tt.len() && ot.iter().zip(&tt).all(|(a, b)| tokens_match(a, b))
    })
}

/// Two answers agree when both are values that match, or both are refusals.
/// Error texts belong to their own implementations and are not compared.
fn sides_match(ours: &Side, theirs: &Side) -> bool {
    match (ours.text(), theirs.text()) {
        (Some(o), Some(t)) => outputs_match(o, t),
        (None, None) => true,
        _ => false,
    }
}

/// The whole expression list, in the order the snapshot records it. The
/// index origin travels with the expression: libjay carries `⎕IO` on the
/// dialect, and the oracle gets `⎕IO←0⋄` glued in front of the sentence.
fn all_exprs() -> Vec<(&'static str, Vec<(String, u8)>)> {
    vec![
        ("fixed corpus", FIXED.iter().map(|s| (s.to_string(), 1)).collect()),
        ("index origin 0", IO_ZERO.iter().map(|s| (s.to_string(), 0)).collect()),
        ("generated corpus", generated_exprs().into_iter().map(|s| (s, 1)).collect()),
    ]
}

/// The closed test: libjay against the recorded answers, no oracle.
#[test]
fn snapshot_battery() {
    let records = common::read(SNAP);
    assert!(!records.is_empty(), "{SNAP} has no records");
    let mut failures = Vec::new();
    for record in &records {
        let ours = Side::of(jay_eval(&record.expr, record.io));
        let theirs = record.reference();
        if !sides_match(&ours, theirs) {
            failures.push(format!(
                "{}\n  ours:     {}\n  snapshot: {}",
                record.expr,
                ours.describe(),
                theirs.describe()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} snapshot expressions differ:\n{}",
        failures.len(),
        records.len(),
        failures.join("\n")
    );
    eprintln!("snapshot agreement on {} expressions", records.len());
}

/// The recorded divergences, held to libjay's own side of each one. That the
/// pair still disagrees is re-measured by the refresh; here it is asserted
/// of the file, so a hand-edited record that has quietly converged is caught.
#[test]
fn divergences_battery() {
    let records = common::read(DIVERGENCES);
    assert!(!records.is_empty(), "{DIVERGENCES} has no records");
    let mut failures = Vec::new();
    for record in &records {
        let recorded = record.ours.as_ref().expect("a divergence records libjay's answer");
        let theirs = record.reference();
        let ours = Side::of(jay_eval(&record.expr, record.io));
        if !sides_match(&ours, recorded) {
            failures.push(format!(
                "{}\n  ours:     {}\n  snapshot: {}",
                record.expr,
                ours.describe(),
                recorded.describe()
            ));
        } else if sides_match(recorded, theirs) {
            failures.push(format!(
                "{}: the recorded answers agree, so the note should go",
                record.expr
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} recorded divergences are wrong:\n{}",
        failures.len(),
        records.len(),
        failures.join("\n")
    );
    eprintln!("{} recorded divergences hold", records.len());
}

/// The open test: the live reference against the snapshot. Off unless asked.
#[test]
fn refresh_against_reference() {
    let Some(mode) = refresh_mode() else {
        eprintln!("refresh off; set LIBJAY_REFRESH_ORACLE=1 (or =write) to run the APL oracle");
        return;
    };
    let apl = oracle_path();
    let sections: Vec<common::Section> = all_exprs()
        .into_iter()
        .map(|(title, exprs)| {
            let records = exprs
                .iter()
                .map(|(e, io)| Record::new(e, *io, Side::of(oracle_eval(&apl, e, *io))))
                .collect();
            common::section(title, records)
        })
        .collect();
    let fresh: Vec<Record> = sections.iter().flat_map(|s| s.records.clone()).collect();
    if let Refresh::Write = mode {
        common::write(SNAP, "libjay reference snapshot: APL (GNU APL).", &sections);
        eprintln!("wrote {} records to {SNAP}", fresh.len());
        return;
    }
    let drift = common::drift(&common::read(SNAP), &fresh, &sides_match);
    assert!(
        drift.is_empty(),
        "{} snapshot records no longer match the reference \
         (rerun with LIBJAY_REFRESH_ORACLE=write):\n{}",
        drift.len(),
        drift.join("\n")
    );
    eprintln!("{} snapshot records confirmed against the reference", fresh.len());
}

/// The same, for the divergences: both sides are measured live, and each
/// pair must still disagree.
#[test]
fn refresh_divergences_against_reference() {
    let Some(mode) = refresh_mode() else {
        eprintln!("refresh off; set LIBJAY_REFRESH_ORACLE=1 (or =write) to re-check divergences");
        return;
    };
    let apl = oracle_path();
    let mut records = Vec::new();
    let mut converged = Vec::new();
    for (expr, why) in KNOWN_DIVERGENCES {
        let ours = Side::of(jay_eval(expr, 1));
        let theirs = Side::of(oracle_eval(&apl, expr, 1));
        if sides_match(&ours, &theirs) {
            converged.push(format!("{expr}: {why}"));
        }
        records.push(Record {
            expr: expr.to_string(),
            io: 1,
            note: Some(why.to_string()),
            ours: Some(ours),
            theirs: Some(theirs),
        });
    }
    assert!(
        converged.is_empty(),
        "recorded divergences now agree, so the note should go:\n{}",
        converged.join("\n")
    );
    let sections = [common::section("known divergences", records.clone())];
    if let Refresh::Write = mode {
        common::write(DIVERGENCES, "libjay recorded divergences from GNU APL.", &sections);
        eprintln!("wrote {} divergences to {DIVERGENCES}", records.len());
        return;
    }
    let drift = common::drift(&common::read(DIVERGENCES), &records, &sides_match);
    assert!(
        drift.is_empty(),
        "{} recorded divergences have moved (rerun with LIBJAY_REFRESH_ORACLE=write):\n{}",
        drift.len(),
        drift.join("\n")
    );
    eprintln!("{} divergences confirmed against the reference", records.len());
}


/// Kept out of the corpora above: each of these is a place where libjay
/// follows a documented choice of its own (Dyalog-style, or the rule its J
/// frontend already uses) and GNU APL follows the ISO/APL2 one. The refresh
/// asserts they still DISAGREE, so if either side moves we hear about it,
/// and records both answers in `apl_divergences.snap`.
/// The same list appears in docs/coverage.md.
const KNOWN_DIVERGENCES: &[(&str, &str)] = &[
    // libjay's monadic `÷` takes J's rule for division by zero (an
    // infinity); GNU APL raises DOMAIN ERROR, as libjay's DYADIC `÷` does.
    // Recorded as a deliberate gap in docs/coverage.md.
    ("÷0", "monadic ÷0 is ∞ here, DOMAIN ERROR there"),
    ("÷0 2 4", "monadic ÷0 is ∞ here, DOMAIN ERROR there"),
    // `⍟0` is the same disagreement one function along.
    ("⍟0", "log 0 is ¯∞ here, DOMAIN ERROR there"),
    // Dyadic `⊖` with a vector left argument: libjay reads it as one amount
    // per AXIS (J's `|.` rule), APL as one amount per column of the leading
    // axis. `⌽` follows APL in both valences, so only `⊖` diverges.
    ("1 2⊖3 4⍴⍳12", "left argument is per-axis here, per-column there"),
    ("1 ¯1 2 0⊖3 4⍴⍳12", "left argument is per-axis here, per-column there"),
    // `∪` is nub over ITEMS here, so a matrix is legal; GNU APL restricts
    // the monad to vectors.
    ("∪2 3⍴1 2 3 1 2 3", "nub takes items of any rank here, vectors only there"),
    // Reshaping from an empty argument: APL2 fills with the prototype (0 or
    // blank), libjay refuses rather than invent data.
    ("2 2⍴⍳0", "empty reshape is a fill of zeros there, an error here"),
    // A circle function that leaves the reals: GNU APL has complex numbers
    // and libjay does not yet, so it names the gap instead of answering.
    ("0○2", "complex result there, `complex numbers` gap here"),
    ("¯2○2", "complex result there, `complex numbers` gap here"),
    // `⊥` on an argument of rank 2 or more: APL folds the LEADING axis (it
    // is an inner product), libjay folds the last one, which is J's `#.`
    // rank-1 framing. Vectors — the common case — agree. Fixing it needs an
    // axis-moving transpose the IR does not have yet, the same one dyadic
    // `⍉` is waiting for, so it is a named gap in docs/coverage.md.
    ("2⊥2 3⍴1 0 1 1 1 0", "decode folds the last axis here, the leading one there"),
    ("2⊥2 2 3⍴⍳12", "decode folds the last axis here, the leading one there"),
    // A sequence yields its last sentence and prints nothing on the way
    // (CLAUDE.md); GNU APL prints the value of every statement.
    ("1 2 3⋄4 5", "only the last sentence has a value here, every one prints there"),
    // GNU APL's own bug, kept here so we notice if a later release fixes
    // it: a scan whose axis has length 1 loses that axis, so a 2-by-1
    // matrix scans to a 2-vector and a 1-element vector to a scalar. Every
    // other axis length, and `⌽`/`⌿` on the same shapes, are fine there.
    ("+\\2 1⍴⍳12", "scan keeps the shape here, drops a length-1 axis there"),
    // The neutral cell of `⌈` and `⌊` over no items: J's infinities against
    // GNU APL's largest representable magnitudes. Every other entry of the
    // identity table matches.
    ("⌈/⍳0", "the neutral cell of ⌈ is ¯∞ here, ¯1.7976E308 there"),
    ("⌊/⍳0", "the neutral cell of ⌊ is ∞ here, 1.7976E308 there"),
    // Both continue their functions past the reals the way J does — the
    // gamma pole and artanh 1 are signed infinities — where GNU APL stops
    // at DOMAIN ERROR.
    ("!¯1", "the factorial of a negative integer is ∞ here, DOMAIN ERROR there"),
    ("¯7○1", "artanh 1 is ∞ here, DOMAIN ERROR there"),
    ("⍴+\\,5", "scan keeps the shape here, drops a length-1 axis there"),
    // Nested arrays. A vector of simple scalars of different types is a
    // simple MIXED array in APL2 and has no representation here yet.
    ("1 'a'", "a mixed simple vector there, a named gap here"),
    // Catenating a simple array to a nested one encloses the simple
    // items there; libjay refuses the pair rather than guess the depth.
    ("1 2,⊂3 4", "the simple items are enclosed there, a type error here"),
    // Overtaking a nested array fills with the first item's prototype
    // there and with the empty box (J's `a:`) here.
    ("4↑(1 2)(3 4)", "the fill is the item prototype there, an empty box here"),
    // GNU APL spaces a nested display more widely than libjay does, which
    // the whitespace-insensitive comparison above only sees through the
    // length of `⍕`.
    ("⍴⍕(1 2)(3 4)", "one space between items here, two there"),
    // Named gaps: ordering boxes needs J's total array ordering, and
    // these two glyphs have their nested valence still to come.
    ("⍋(1 2)(3 4)", "graded there, a `grading boxed arrays` gap here"),
    ("1 2⊃(1 2)(3 4)", "pick there, a named gap here"),
    // The index generator on a vector of lengths: an array of index vectors
    // there, a named gap here. Refusing it is the point — the monad has rank
    // ∞ so the rank machinery cannot quietly frame it into a plain 2 by 3.
    ("⍳2 3", "a nested index array there, a named gap here"),
];

const IO_ZERO: &[&str] = &[
    "⍳5",
    "⍸1 0 1 1",
    "⍸2 2⍴1 0 0 1",
    "0 10 20⍸5 15 25",
    "'abcde'[0]",
    "'abcde'[2]",
    "2⌷⍳5",
    "+/[0]2 3⍴⍳6",
    "⌽[0]2 3⍴⍳6",
    "1 0 1 0⊂'abcd'",
    "⍳0",
    "⍋3 1 4 1 5",
    "⍒3 1 4 1 5",
    "1 2 3⍳2",
    "1 2 3⍳9",
    "'abc'⍳'cab'",
    "2 3⍴⍳6",
    "+/2 3⍴⍳6",
    "⍋2 3⍴1 2 1 1 1 3",
    "⍒'hello'",
    "⍳3",
];

const FIXED: &[&str] = &[
    // --- arithmetic ---------------------------------------------------
    "2+2",
    "1 2 3×10",
    "10÷4",
    "0÷0",
    "5÷0",
    "0÷5",
    "-1 ¯2 3",
    "+1 ¯2 3",
    "×¯5 0 5",
    "2*10",
    "2*0.5",
    "2*¯1",
    "0*0",
    "2⍟8",
    "10⍟100",
    "⍟1 2",
    "|¯3 3 ¯0.5",
    "3|1 2 3 4 5 6",
    "¯3|1 2 3 4 5 6",
    "0|7 ¯7",
    "1.5|7",
    "2⌈1 3",
    "2⌊1 3",
    "⌈1.5 ¯1.5",
    "⌊1.5 ¯1.5",
    "⌈¯2.5",
    "⌊¯2.5",
    "¯3.5⌊2",
    "2+3.5",
    "1 2 3+0.5",
    "0.1+0.2",
    "1÷3",
    "1÷7",
    "9223372036854775807+1",
    // --- comparison and logic -----------------------------------------
    "1 2 3=1 5 3",
    "1 2 3≠1 5 3",
    "1 2 3<2 2 2",
    "1 2 3≤2 2 2",
    "1 2 3>2 2 2",
    "1 2 3≥2 2 2",
    "1 0 1∧1 1 0",
    "1 0 1∨1 1 0",
    "4∧6",
    "12∨18",
    "~0 1",
    "(2 3⍴⍳6)>3",
    // --- reduction ----------------------------------------------------
    "+/1 2 3",
    "-/1 2 3 4",
    "×/1 2 3 4",
    "⌈/3 1 4",
    "⌊/3 1 4",
    "+/2 3⍴⍳6",
    "+⌿2 3⍴⍳6",
    "×/2 3⍴⍳6",
    "×⌿2 3⍴⍳6",
    "+/2 3 4⍴⍳24",
    "+⌿2 3 4⍴⍳24",
    "+/⍳0",
    "×/⍳0",
    "⍴+/0 3⍴1",
    "⍴+/2 0⍴1",
    // --- scan ---------------------------------------------------------
    "+\\1 2 3",
    "-\\1 2 3",
    "×\\1 2 3 4",
    "⌈\\3 1 4 1 5",
    "÷\\1 2 4",
    "+\\2 3⍴⍳6",
    "+⍀2 3⍴⍳6",
    "⌊\\3 1 4 1 5",
    "+\\⍳0",
    // --- index generator, shape, reshape ------------------------------
    "⍳5",
    "⍳0",
    "⍴⍳0",
    "2 3⍴⍳6",
    "3 2⍴1 2 3",
    "⍴0 3⍴1",
    "0 3⍴1",
    "⍴2 3 4⍴⍳24",
    "⍴⍳5",
    "⍴5",
    "⍴'a'",
    "⍴'abc'",
    "2 3 4⍴⍳24",
    "2 2 2⍴⍳8",
    "2 2 1 2⍴⍳8",
    // --- index of -----------------------------------------------------
    "1 2 3⍳2",
    "1 2 3⍳4",
    "1 2 3⍳2 3 9",
    "'abc'⍳'cab'",
    "5⍳6",
    // --- transpose ----------------------------------------------------
    "⍉2 3⍴⍳6",
    "⍉2 3 4⍴⍳24",
    "⍉⍳3",
    "⍉5",
    // --- take and drop ------------------------------------------------
    "2↑5 6 7 8",
    "¯2↑5 6 7 8",
    "6↑1 2 3",
    "¯5↑1 2 3",
    "1 2↑3 3⍴⍳9",
    "2↓5 6 7 8",
    "¯1↓5 6 7 8",
    "5↓1 2 3",
    "1 1↓3 3⍴⍳9",
    // --- reverse and rotate -------------------------------------------
    "⌽1 2 3 4",
    "⌽3 4⍴⍳12",
    "⊖3 4⍴⍳12",
    "⌽'abc'",
    "⌽2 3⍴'abcdef'",
    "⊖2 3⍴'abcdef'",
    "1⌽1 2 3 4 5",
    "¯1⌽1 2 3 4 5",
    "2⌽3 4⍴⍳12",
    "1⊖3 4⍴⍳12",
    "1 ¯1 2⌽3 4⍴⍳12",
    "3⌽'abcde'",
    // --- catenate, table, ravel ---------------------------------------
    "1 2,3 4",
    "(2 3⍴⍳6),2 2⍴⍳4",
    "(2 3⍴⍳6),9",
    "(2 3⍴⍳6)⍪7 8 9",
    "(2 3⍴⍳6)⍪2 3⍴⍳6",
    ",2 3⍴⍳6",
    "⍪1 2 3",
    "⍪5",
    "⍪2 3⍴⍳6",
    "⍴⍪2 3 4⍴⍳24",
    "'ab','cd'",
    // --- tally, match, membership, nub --------------------------------
    "≢1 2 3",
    "≢2 3⍴⍳6",
    "≢5",
    "1 2 3≡1 2 3",
    "1 2 3≡1 2",
    "(2 3⍴⍳6)≡2 3⍴⍳6",
    "1 2 3≢1 2 3",
    "1 2 3≢1 2",
    "1 2∊1 3 5",
    "1 2 3∊2 3⍴⍳6",
    "(2 2⍴1 2 9 9)∊1 2 3",
    "'ab'∊'abc'",
    "∪3 1 4 1 5 9 2 6 5 3",
    "∪'mississippi'",
    // --- grade --------------------------------------------------------
    "⍋3 1 4 1 5",
    "⍒3 1 4 1 5",
    "⍋'hello'",
    "⍒'hello'",
    "⍋1.5 0.5 2.5",
    "⍋2 3⍴1 2 1 1 1 3",
    "⍒2 3⍴1 2 1 1 1 3",
    "⍋1 1 1",
    // --- factorial and binomial ---------------------------------------
    "!0 1 2 3 4 5",
    "!10",
    "!0.5",
    "!2.5",
    "2!5",
    "0!5",
    "5!5",
    "6!5",
    "2!5 6 7",
    "3!100",
    "2!5.5",
    // --- decode and encode --------------------------------------------
    "2⊥1 0 1",
    "24 60 60⊥1 2 3",
    "1 0 1⊥1 2 3",
    "2 2 2⊤5",
    "24 60 60⊤3723",
    "2 2⊤5 6",
    "⍴2 2⊤5 6",
    "0 2 2⊤5",
    "1 1⊤5",
    // --- format -------------------------------------------------------
    "⍕5",
    "⍴⍕5",
    "⍕2.5",
    "⍴⍕2.5",
    "⍕1 2 3",
    "⍴⍕1 2 3",
    "⍕¯1 22 333",
    "⍕2 3⍴⍳6",
    "⍴⍕2 3⍴⍳6",
    "⍕2 3 4⍴⍳24",
    "⍴⍕2 3 4⍴⍳24",
    "⍕2 2⍴0.5 1 2 3",
    "⍕1 2 3=1 5 3",
    // --- outer product ------------------------------------------------
    "1 2 3∘.×1 2 3",
    "1 2∘.+10 20 30",
    "⍴1 2 3∘.×1 2",
    "1 2 3∘.≤2 3",
    "1 2∘.⌈3 1",
    // --- replicate / compress -----------------------------------------
    "1 0 1/1 2 3",
    "2 0 1/1 2 3",
    "1/1 2 3",
    "2/1 2 3",
    "0/1 2 3",
    "⍴0/1 2 3",
    "2 3/1 2",
    "1 0 1/'abc'",
    "1 0 1/2 3⍴⍳6",
    "1 0⌿2 3⍴⍳6",
    // --- rank, commute, power -----------------------------------------
    "⌽⍤1⊢2 3⍴⍳6",
    "+⍤0⊢2 3⍴⍳6",
    "⌽⍤2⊢2 2 3⍴⍳12",
    "2-⍨5",
    "+⍨3",
    "÷⍨4",
    "1 2 3,⍨4 5",
    "-⍣3⊢5",
    "-⍣2⊢5",
    "⌽⍣2⊢1 2 3",
    "⌽⍣1⊢1 2 3",
    // --- circle functions ---------------------------------------------
    "○1",
    "○0 1 2",
    "1○0.5",
    "2○0.5",
    "3○0.5",
    "4○0.5",
    "5○0.5",
    "6○0.5",
    "7○0.5",
    "¯1○0.5",
    "¯2○0.5",
    "¯3○0.5",
    "¯4○2",
    "¯5○0.5",
    "¯6○2",
    "¯7○0.5",
    "0○0.5",
    "2○○1",
    "1 2 3○1",
    // --- characters and sentences -------------------------------------
    "'abc'",
    "'a'",
    "2 3⍴'abcdef'",
    "3⊢5",
    "3⊣5",
    "2+2 ⍝ a comment",
    // --- agreement, scalar extension, empties -------------------------
    "(2 3⍴⍳6)+2 3⍴⍳6",
    "(2 3⍴⍳6)+1 2 3",
    "(2 3⍴⍳6)×2",
    "2×2 3⍴⍳6",
    "1 2 3+1 2",
    "0⍴5",
    "⍴0⍴5",
    "+/0⍴5",
    "×/0⍴5",
    "-/⍳0",
    "÷/⍳0",
    "*/⍳0",
    "|/⍳0",
    "!/⍳0",
    "=/⍳0",
    "≠/⍳0",
    "</⍳0",
    "≤/⍳0",
    ">/⍳0",
    "≥/⍳0",
    "∧/⍳0",
    "∨/⍳0",
    "⍟/⍳0",
    "○/⍳0",
    "≢⍳0",
    "≢''",
    "∪⍳0",
    "⌽⍳0",
    "1⌽⍳0",
    "3↑⍳0",
    "⍕⍳0",
    "⍴⍕⍳0",
    "0=⍳0",
    "1 2 3⍳1 2 3",
    "'abc'⍳'z'",
    "'ab'≡'ab'",
    "≢2 3 4⍴⍳24",
    "(2 3⍴⍳6)⍪5",
    "5,2 3⍴⍳6",
    "2 3⍴'ab'",
    "⌽⍤1⊢2 2 3⍴⍳12",
    "×/2 2⍴0",
    "|¯0",
    "2*62",
    "2*63",
    "0∧5",
    "0∨0",
    "1 2 3∘.=1 2",
    // --- nested arrays -------------------------------------------------
    // The nested DISPLAY is libjay's own approximation (see
    // KNOWN_DIVERGENCES), so what is compared here is structure: shape,
    // depth, tally, and the leaves enlist brings back into the open.
    "⍴⊂1 2 3",
    "≡⊂1 2 3",
    "⊂5",
    "≡⊂5",
    "≡⊂⊂1 2",
    "⊃'ab' 'cd'",
    "⍴⊃'ab' 'cd'",
    "⊃3",
    "⊃⊂1 2",
    "⊃(1 2)(3 4 5)",
    "⍴⊃(1 2)(3 4 5)",
    "⍴⊃⍳0",
    "⊃'abc'",
    "2×¨1 2 3",
    "≢¨'ab' 'cde'",
    "+/¨(1 2)(3 4)",
    "∊⍴¨'ab' 'cde'",
    "∊1+¨(1 2)(3 4)",
    "∊(1 2)+¨(1 2)(3 4)",
    "⍴(1 2)(3 4)",
    "≡(1 2)(3 4)",
    "⍴'ab' 'cd'",
    "≡'ab' 'cd'",
    "⍴1 2 3",
    "≡1 2 3",
    "⍴1 (2 3)",
    "⍴1 2 (3 4)",
    "⍴'ab' 1 2",
    "∊(1 2)(3 4 5)",
    "∊'ab' 'cd'",
    "∊2 3⍴⍳6",
    "⍴∊5",
    "∊(1 2)((3 4)(5 6))",
    "≡1",
    "≡'abc'",
    "≡1(2(3 4))",
    "↑(1 2)(3 4)",
    "↑1 2 3",
    "⍴↑1 2 3",
    "↑2 3⍴⍳6",
    "↑⍳0",
    "⍴2 2⍴(1 2)(3 4)(5 6)(7 8)",
    "≡2 2⍴(1 2)(3 4)(5 6)(7 8)",
    "∊2 2⍴(1 2)(3 4)(5 6)(7 8)",
    "⍴∪(1 2)(3 4)(1 2)",
    "∊∪(1 2)(3 4)(1 2)",
    "⍴,⊂1 2",
    "∊⌽(1 2)(3 4)",
    "⍴2↑(1 2)(3 4)(5 6)",
    "∊2↑(1 2)(3 4)(5 6)",
    "⍴1↓(1 2)(3 4)(5 6)",
    "(1 2)(3 4)⍳⊂3 4",
    "(1 2)(3 4)⍳(3 4)",
    "(1 2)∊(1 2)(3 4)",
    "'ab' 'cd'∊'ab' 'xy'",
    "≢(1 2)(3 4)(5 6)",
    "⍴(⊂1 2),⊂3 4",
    "∊(⊂1 2),⊂3 4",
    // --- comparison tolerance (⎕CT) ---------------------------------------
    "1=1+1E¯15",
    "1=1+1E¯13",
    "1=1+1E¯12",
    "1=1+2E¯13",
    "0=1E¯20",
    "1<1+1E¯15",
    "1≤1+1E¯15",
    "1≡1+1E¯15",
    "(1+1E¯15)∊1 2",
    "1 2⍳1+1E¯15",
    "∪1,1+1E¯15",
    "⌊2.9999999999999",
    "⌊2.99999999999",
    "⌈3.0000000000001",
    "1 2 3⍳1.0000000000000002",
    "(2 2⍴1 2 3 4)≡2 2⍴1 2 3 4.0000000000000004",
    // --- where and interval index -----------------------------------------
    "⍸1 0 1 1",
    "⍸0 1 0 2",
    "⍸0 0 0",
    "⍸2 3",
    "⍸2 2⍴1 0 0 1",
    "⍴⍸1 0 1 1",
    "⍸1 0 1.5",
    "0 10 20⍸5 15 25",
    // --- bracket indexing ---------------------------------------------------
    "(⍳5)[2]",
    "(⍳5)[2 3]",
    "A←3 3⍴⍳9 ⋄ A[2;3]",
    "A←3 3⍴⍳9 ⋄ A[2;]",
    "A←3 3⍴⍳9 ⋄ A[;2]",
    "A←3 3⍴⍳9 ⋄ A[1 2;2 3]",
    "A←3 3⍴⍳9 ⋄ A[;]",
    "A←3 3⍴⍳9 ⋄ ⍴A[1 2;]",
    "A←3 3⍴⍳9 ⋄ A[2 2⍴1 2 2 1;1]",
    "'abcde'[2 4]",
    "((⍳5)[2])+1",
    "+/(⍳5)[2 3]",
    "(⍳5)[6]",
    "(⍳5)[0]",
    "A←3 3⍴⍳9 ⋄ A[2]",
    // --- squad ---------------------------------------------------------------
    "2⌷⍳5",
    "2 3⌷3 3⍴⍳9",
    "2⌷'abcde'",
    "2⌷3 3⍴⍳9",
    "1 2 3⌷3 3⍴⍳9",
    // --- axis specification ---------------------------------------------------
    "+/[1]2 3⍴⍳6",
    "+/[2]2 3⍴⍳6",
    "+⌿[1]2 3⍴⍳6",
    "⌽[1]2 3⍴⍳6",
    "⌽[2]2 3⍴⍳6",
    "⊖[2]2 3⍴⍳6",
    "+\\[1]2 3⍴⍳6",
    "+\\[2]2 3⍴⍳6",
    // --- partitioned enclose ---------------------------------------------------
    "1 1 0 1⊂'abcd'",
    "1 0 0 1⊂'abcd'",
    "0 1 0 1⊂'abcd'",
    "1 1 1 1⊂'abcd'",
    "0 0 0 0⊂'abcd'",
    "2 0 1 0⊂'abcd'",
    "1 2 0 1⊂'abcd'",
    "2 1 0 1⊂'abcd'",
    "1 1 2 2⊂'abcd'",
    "1 0 1 0⊂⍳4",
    "≢1 0 1 0⊂'abcd'",
    "⍴¨1 1 0 1⊂'abcd'",
    "1 0 1⊂'abcd'",
    "¯1 0 1 0⊂'abcd'",
    // --- matrix division ---------------------------------------------------
    "⌹2 2⍴1 2 3 4",
    "⌹2 2⍴4 7 2 6",
    "⌹3 3⍴2 0 0 0 3 0 0 0 4",
    "⌹1 2 3",
    "⌹0.5",
    "⌹2 2⍴1 2 2 4",
    "⌹2 3⍴⍳6",
    "(1 2)⌹2 2⍴1 2 3 4",
    "(1 0 0)⌹3 2⍴1 1 1 2 1 3",
    "(1 2 3)⌹3 1⍴1 1 1",
    "(1 2 3)⌹3 3⍴2 0 0 0 3 0 0 0 4",
    // --- power with a function operand ----------------------------------------
    "(⌽⍣2)1 2 3",
    "(⌽⍣3)1 2 3",
    "(⌊⍣≡)2.5",
    "(⍴⍣≡)2 3⍴⍳6",
    "(+/)1 2 3",
];

/// Deterministic pseudo-random sentences over the shared surface: no
/// primitive whose two dialects are known to part ways appears here.
/// Materialised into the snapshot; this runs only on a refresh.
fn generated_exprs() -> Vec<String> {
    // Small xorshift so runs are reproducible without any clock access.
    let mut state: u64 = 0x9E3779B97F4A7C15;
    let mut rng = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    // Scalar dyads, safe on any pair of small integers. `÷` is left out: its
    // zero divisor is a divergence of its own.
    let dyads = ["+", "-", "×", "⌈", "⌊", "|", "=", "≠", "<", "≤", ">", "≥"];
    // Monads safe on any small-integer array of rank 1 or 2.
    let monads = ["-", "+", "×", "|", "⌈", "⌊", "⍴", ",", "⍉", "⌽", "⊖", "≢", "⍕"];
    // Verbs that fold a whole axis without leaving the integers.
    let folds = ["+", "×", "⌈", "⌊"];
    let mut exprs = Vec::new();
    for _ in 0..25 {
        let n = 1 + (rng() % 5) as usize;
        let vec: Vec<String> = (0..n)
            .map(|_| {
                let v = (rng() % 19) as i64 - 9;
                if v < 0 { format!("¯{}", -v) } else { v.to_string() }
            })
            .collect();
        let vec = vec.join(" ");
        let noun = match rng() % 3 {
            0 => vec.clone(),
            1 => format!("{} {}⍴⍳{}", 1 + rng() % 3, 1 + rng() % 4, 1 + rng() % 12),
            _ => format!("{} 4⍴{vec}", 1 + rng() % 3),
        };
        // Scans only get arguments whose every axis is at least 2 long: a
        // length-1 axis is where GNU APL's scan loses the axis, which is
        // its bug and is pinned in KNOWN_DIVERGENCES rather than generated
        // over and over here.
        let dense = match rng() % 2 {
            0 => format!("{} {}⍴⍳{}", 2 + rng() % 2, 2 + rng() % 3, 6 + rng() % 12),
            _ => format!("{}⍴{}", 2 + rng() % 4, vec),
        };
        let fold = |r: u64| folds[(r % folds.len() as u64) as usize];
        let dyad = |r: u64| dyads[(r % dyads.len() as u64) as usize];
        let monad = |r: u64| monads[(r % monads.len() as u64) as usize];
        exprs.push(format!("{}/{noun}", fold(rng())));
        exprs.push(format!("{}⌿{noun}", fold(rng())));
        exprs.push(format!("{}\\{dense}", fold(rng())));
        exprs.push(format!("{}⍀{dense}", fold(rng())));
        exprs.push(format!("{} {noun}", monad(rng())));
        let atom = (rng() % 7) as i64 - 3;
        let atom = if atom < 0 { format!("¯{}", -atom) } else { atom.to_string() };
        exprs.push(format!("{atom}{}{noun}", dyad(rng())));
        exprs.push(format!("{}⍨{noun}", dyad(rng())));
        exprs.push(format!("{vec}∘.{}{vec}", dyad(rng())));
        // A rotation amount is always legal, whatever the shape.
        exprs.push(format!("{atom}⌽{noun}"));
        exprs.push(format!("{atom}⊖{noun}"));
        // The vector is its own index domain, so `⍳` always has an answer.
        exprs.push(format!("{vec}⍳{atom}"));
        exprs.push(format!("⍴{noun}"));
    }
    exprs
}
