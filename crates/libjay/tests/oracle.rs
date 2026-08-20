//! Differential tests against the reference J implementation.
//!
//! `cargo test` runs the SNAPSHOT battery: every expression in
//! `tests/snapshots/j.snap` is evaluated by libjay and compared with the
//! answer the reference gave when the file was last refreshed. No
//! subprocess, no external binary, nothing outside the repository.
//!
//! The reference itself is run only by `refresh_against_reference`, gated on
//! `LIBJAY_REFRESH_ORACLE` (docs/testing.md). jconsole is a black-box oracle
//! there: never linked, never read.
//!
//! Comparison is textual with numeric tolerance: both sides print 6
//! significant digits, so parsing the tokens back gives at most ~5e-6
//! relative representation error per side; a relative tolerance of 1e-5
//! (with a 1e-9 absolute floor for values near zero) accepts exactly that
//! and still catches any real semantic difference.

mod common;

use std::io::Write;
use std::process::{Command, Stdio};

use common::{Record, Side};
use jay::fmt::{format_array, FmtOpts};
use jay::{compile, Dialect, Lang};

const SNAP: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots/j.snap");

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
    let path = std::env::var("LIBJAY_ORACLE_J").unwrap_or_else(|_| {
        format!(
            "{}/projects/libjay-oracles/j/j64/jconsole",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    assert!(
        std::path::Path::new(&path).exists(),
        "the refresh needs the J oracle; {path} is not there (set LIBJAY_ORACLE_J)"
    );
    path
}

/// Run one sentence through the reference interpreter. None on error.
fn oracle_eval(jconsole: &str, expr: &str) -> Option<String> {
    let mut child = Command::new(jconsole)
        .args(["-jprofile", "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // jconsole reports a failed sentence on stderr, so discarding it
        // would turn an error into an empty result.
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn jconsole");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{expr}\n").as_bytes())
        .expect("write to jconsole");
    let out = child.wait_with_output().expect("wait for jconsole");
    let text = String::from_utf8_lossy(&out.stdout);
    let complaint = String::from_utf8_lossy(&out.stderr);
    if (text.contains("error") && text.contains('|')) || !complaint.trim().is_empty() {
        return None;
    }
    Some(text.trim_end().to_string())
}

/// Run one sentence through libjay. None on error.
fn jay_eval(expr: &str) -> Option<String> {
    let program = compile(Lang::J, expr, &Dialect::default()).ok()?;
    let mut sink = |_: &str| {};
    let result = program.run(&[], &mut sink).ok()??;
    Some(format_array(&result, &FmtOpts::J))
}

fn parse_j_num(tok: &str) -> Option<f64> {
    match tok {
        "_" => return Some(f64::INFINITY),
        "__" => return Some(f64::NEG_INFINITY),
        "_." => return Some(f64::NAN),
        _ => {}
    }
    let s = tok.replace('_', "-");
    s.parse::<f64>().ok()
}

fn tokens_match(a: &str, b: &str) -> bool {
    match (parse_j_num(a), parse_j_num(b)) {
        (Some(x), Some(y)) => {
            if x.is_nan() || y.is_nan() {
                return x.is_nan() && y.is_nan();
            }
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

/// The whole expression list, in the order the snapshot records it.
fn all_exprs() -> Vec<(&'static str, Vec<String>)> {
    vec![
        ("fixed corpus", FIXED.iter().map(|s| s.to_string()).collect()),
        ("named verbs", NAMED.iter().map(|s| s.to_string()).collect()),
        ("generated corpus", generated_exprs()),
    ]
}

/// The closed test: libjay against the recorded answers, no oracle.
#[test]
fn snapshot_battery() {
    let records = common::read(SNAP);
    assert!(!records.is_empty(), "{SNAP} has no records");
    let mut failures = Vec::new();
    for record in &records {
        let ours = Side::of(jay_eval(&record.expr));
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

/// The open test: the live reference against the snapshot. Off unless asked.
#[test]
fn refresh_against_reference() {
    let Some(mode) = refresh_mode() else {
        eprintln!("refresh off; set LIBJAY_REFRESH_ORACLE=1 (or =write) to run the J oracle");
        return;
    };
    let jconsole = oracle_path();
    let sections: Vec<common::Section> = all_exprs()
        .into_iter()
        .map(|(title, exprs)| {
            let records = exprs
                .iter()
                .map(|e| Record::new(e, 1, Side::of(oracle_eval(&jconsole, e))))
                .collect();
            common::section(title, records)
        })
        .collect();
    let fresh: Vec<Record> = sections.iter().flat_map(|s| s.records.clone()).collect();
    if let Refresh::Write = mode {
        common::write(SNAP, "libjay reference snapshot: J (jconsole).", &sections);
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

const FIXED: &[&str] = &[
    "2 + 2",
    "1 2 3 * 10",
    "10 % 4",
    "0 % 0",
    "5 % 0",
    "_5 % 0",
    "% 0 2 4",
    "- 1 _2 3",
    "* _5 0 5",
    "2 ^ 10",
    "2 ^ 0.5",
    "%: 2 4 9",
    "| _3 3 _0.5",
    "3 | 1 2 3 4 5 6",
    "_3 | 1 2 3 4 5 6",
    "0 | 7 _7",
    "2 <. 1 3",
    "2 >. 1 3",
    "<. 1.5 _1.5",
    ">. 1.5 _1.5",
    "1 2 3 = 1 5 3",
    "1 2 3 < 2 2 2",
    "1 2 3 > 2 2 2",
    "1 2 3 <: 2 2 2",
    "1 2 3 >: 2 2 2",
    "+/ 1 2 3",
    "-/ 1 2 3 4",
    "*/ 1 2 3 4",
    "+/ i. 2 3",
    "+/ i. 3 2",
    "+/\"1 i. 2 3",
    "+/\"2 i. 2 3 4",
    "<./ 3 1 4",
    ">./ 3 1 4",
    "+/ i. 0",
    "*/ i. 0",
    // The rest of the identity table: every verb J gives a neutral cell.
    "-/ i. 0",
    "%/ i. 0",
    "^/ i. 0",
    "%:/ i. 0",
    "|/ i. 0",
    "!/ i. 0",
    "=/ i. 0",
    "~:/ i. 0",
    "</ i. 0",
    "<:/ i. 0",
    ">/ i. 0",
    ">:/ i. 0",
    "^./ i. 0",
    "o./ i. 0",
    "i. 4",
    "i. 2 3",
    "i. _3",
    "i. 2 _3",
    "$ i. 2 3 4",
    "# i. 5 2",
    ", i. 2 3",
    "2 3 $ 1 2 3 4 5 6",
    "3 2 $ 1 2 3",
    "0 3 $ 1",
    "|: i. 2 3",
    "|: i. 2 3 4",
    "2 {. 5 6 7 8",
    "_2 {. 5 6 7 8",
    "6 {. 1 2 3",
    "_5 {. 1 2 3",
    "1 2 {. i. 3 3",
    "{. 5 6 7",
    "}. 5 6 7",
    "1 }. i. 3 2",
    "_1 }. i. 3 2",
    "(+/ % #) 1 2 3 4",
    "(+/ % #) i. 2 3",
    "(* -) 3",
    "([: # $) i. 2 3 4",
    "(>./ - <./) 3 1 4 1 5 9",
    "(i. 2 3) + 10 20",
    "(i. 2 3) * i. 2",
    "10 + i. 2 3",
    "2 + 3.5",
    "1 2 3 + 0.5",
    "9223372036854775807 + 1",
    "'abc'",
    "3 ] 5",
    "3 [ 5",
    "+/ , i. 3 3",
    // reverse and rotate
    "|. 1 2 3 4",
    "|. i. 3 4",
    "|. 'abc'",
    "1 |. 1 2 3 4 5",
    "_1 |. 1 2 3 4 5",
    "2 |. i. 3 4",
    "1 1 |. i. 3 4",
    "1 _1 |. i. 3 4",
    "7 |. 1 2 3",
    "1 |. 5",
    "1 0 2 |. i. 2 3 4",
    "|. i. 2 3 4",
    // catenate, leading axis and stitched
    "1 2 , 3 4",
    "(i. 2 3) , i. 2 3",
    "(i. 2 3) , 10 20 30",
    "(10 20 30) , i. 2 3",
    "(i. 2 3) , 5",
    "5 , i. 2 3",
    "'ab' , 'cd'",
    "'ab' , 'c'",
    "(2 2 $ 'abcd') , 'ef'",
    "1 2 ,. 3 4",
    "(i. 2 3) ,. i. 2 2",
    "(i. 2 3) ,. 9",
    "1 2 ,. i. 2 3",
    "(i. 2 3 4) , i. 1 3 4",
    "(i. 2 3 4) , i. 3 4",
    "(i. 2 3 4) ,. i. 2 2 4",
    "~. 2 2 2 $ 0 1 2 3 0 1 2 3",
    "(i. 2 3 4) i. i. 3 4",
    "(i. 2 3) e. i. 2 2 3",
    "/: 2 2 2 $ 1 0 1 1 0 1 0 0",
    "(i. 2 2) { i. 4 2",
    // match
    "1 2 3 -: 1 2 3",
    "1 2 3 -: 1 2",
    "(i. 2 3) -: i. 2 3",
    "1 -: 1.0",
    "'a' -: 97",
    "-: 8",
    "-: 1 2 3",
    // nub and nub sieve
    "~. 3 1 4 1 5 9 2 6 5 3",
    "~. 'mississippi'",
    "~. 2 3 $ 1 2 3 1 2 3",
    "1 2 1 3 2 ~: 1 1 1 1 1",
    // LCM and GCD
    "4 *. 6",
    "12 +. 18",
    "_4 *. 6",
    "_4 +. 6",
    "0 +. 0",
    "0 *. 5",
    "*./ 4 6 8",
    "+./ 12 18 24",
    "*./ i. 0",
    "+./ i. 0",
    // logarithm and root
    "3 %: 27",
    "2 %: 9",
    "2 ^. 8",
    "^. 1 2",
    "^. 0",
    "10 ^. 100",
    // increment, decrement, double, square, one-minus
    "<: 5",
    ">: 5",
    "+: 5",
    "*: 5",
    "-. 0 1",
    "-. 0.25",
    "<: 2.5",
    "+: 1 2 3",
    "*: _3",
    // tail and curtail
    "{: 1 2 3",
    "}: 1 2 3",
    "{: i. 3 2",
    "}: i. 3 2",
    "{: i. 0 2",
    "}: i. 0 2",
    "{: 5",
    // from
    "2 0 { i. 3 3",
    "_1 { 5 6 7",
    "0 { i. 3 3",
    "(i. 2 2) { 10 20 30 40",
    "1 2 { 'abcdef'",
    "0 { 5",
    // membership
    "1 2 e. 1 3 5",
    "(i. 2 3) e. i. 3 3",
    "1 2 3 e. i. 2 3",
    "'ab' e. 'abc'",
    "5 e. 1 2 3",
    "(i. 2 3) e. 1 2 3",
    // index of
    "1 2 3 i. 2",
    "1 2 3 i. 4",
    "1 2 3 i. 2 3 9",
    "(i. 3 3) i. 3 4 5",
    "(i. 3 3) i. 1 1 1",
    "'abc' i. 'cab'",
    "5 i. 6",
    // grade
    "/: 3 1 4 1 5",
    "\\: 3 1 4 1 5",
    "/: 'hello'",
    "/: 1.5 0.5 2.5",
    "/: 2 3 $ 1 2 1 1 1 3",
    "\\: 2 3 $ 1 2 1 1 1 3",
    "3 1 4 1 5 /: 3 1 4 1 5",
    "'abc' /: 3 1 2",
    "'abc' \\: 3 1 2",
    "/: 1 1 1",
    "\\: 1 1 1",
    "1 2 3 4 5 /: \\: 1 2 3 4 5",
    // scans (prefixes)
    "+/\\ 1 2 3",
    "*/\\ 1 2 3 4",
    "-/\\ 1 2 3 4",
    "<./\\ 3 1 4 1 5",
    ">./\\ 3 1 4 1 5",
    "%/\\ 1 2 3 4",
    "+/\\ i. 2 3",
    "+/\\ i. 0",
    "+/\\ 5",
    "$ +/\\ 5",
    "#\\ 1 2 3",
    "(+/ % #)\\ 1 2 3",
    "+/\\ 1 0 1",
    // suffixes
    "+/\\. 1 2 3",
    "-/\\. 1 2 3 4",
    "+/\\. i. 2 3",
    "$ +/\\. i. 0",
    // moving windows
    "3 +/\\ 1 2 3 4 5",
    "2 +/\\ 1 2 3 4 5",
    "1 +/\\ 1 2 3",
    "0 +/\\ 1 2 3 4 5",
    "0 <./\\ 1 2 3",
    "$ 0 +/\\ i. 0",
    "5 +/\\ 1 2 3 4",
    "$ 9 +/\\ 1 2 3",
    "$ 3 +/\\ i. 2 3",
    "$ 3 -/\\ 1 2",
    "_2 +/\\ 1 2 3 4 5",
    "_3 +/\\ 1 2 3 4 5 6 7",
    "_9 +/\\ 1 2 3",
    "_1 +/\\ 1 2 3",
    "3 +/\\ i. 4 3",
    "_2 +/\\ i. 4 3",
    "2 <./\\ 3 1 4 1 5",
    "3 >./\\ 3 1 4 1 5",
    "3 */\\ 1 2 3 4 5",
    "_2 */\\ 1 2 3 4 5",
    "2 %/\\ 1 2 3 4",
    "2 3 4 -/\\ 1 2 3",
    "3 (+/ % #)\\ 1 2 3 4 5",
    "3 #\\ 1 2 3 4 5",
    "2 +/\\ 5",
    // commute
    "+~ 3",
    "2 -~ 5",
    "%~ 4",
    "3 %~ 12",
    "-~ 1 2 3",
    "2 3 +~ 4 5",
    "<~ 1 2 3",
    "2 %~ 8",
    // power
    "+:^:3 (1)",
    "+:^:0 (5)",
    "2 +^:3 (5)",
    "2 *^:4 (1)",
    "%:^:_ (100)",
    "%:^:_ (0.5)",
    "%:^:_ (1)",
    "%:^:_ (0)",
    "%:^:3 (100)",
    "$ +:^:2 (1 2 3)",
    "+:^:2 (1 2 3)",
    // circle functions
    "o. 1",
    "o. 0 1 2",
    "0 o. 0.5",
    "1 o. 0.5",
    "2 o. 0.5",
    "3 o. 0.5",
    "4 o. 0.5",
    "5 o. 0.5",
    "6 o. 0.5",
    "7 o. 0.5",
    "_1 o. 0.5",
    "_2 o. 0.5",
    "_3 o. 0.5",
    "_4 o. 2",
    "_4 o. _2",
    "_5 o. 0.5",
    "_6 o. 2",
    "_7 o. 0.5",
    "_7 o. 1",
    "1 o. 0",
    "1 2 3 o. 1",
    "2 o. o. 1",
    // replicate
    "1 0 1 # 1 2 3",
    "2 0 1 # 1 2 3",
    "1 # 1 2 3",
    "2 # 1 2 3",
    "0 # 1 2 3",
    "$ 0 # 1 2 3",
    "1 0 # i. 2 3",
    "2 # i. 2 3",
    "2 3 # 1 2",
    "2 # 5",
    "$ 2 # 5",
    "1 0 1 # 'abc'",
    "0 0 0 # 1 2 3",
    // atop and compose: `@` differs from `@:` only in rank
    "(+/ @ ,) i. 2 3",
    "(+/ @: ,) i. 2 3",
    "(+/ @ (,\"1)) i. 2 3",
    "(+/ @: (,\"1)) i. 2 3",
    "($ @ (]\"1)) i. 2 3",
    "($ @: (]\"1)) i. 2 3",
    "$ ($ @ (+\"0)) i. 2 3",
    "1 2 3 (+/ @ (,\"0)) 10 20 30",
    "1 2 3 (+/ @: (,\"0)) 10 20 30",
    "(<:@#) 1 2 3",
    "(*: @ +)/ 1 2 3",
    "(! @ >:) 3",
    "(+ & *:) 1 2 3",
    "2 (+ & *:) 3",
    "1 2 3 (+&*:) 1 2 3",
    "(+/ & ,) i. 2 3",
    "(+/ &: ,) i. 2 3",
    "(i.2 3) (+/ & ,) i. 2 3",
    "(i.2 3) (,&(+/\"1)) i. 2 3",
    "(i.2 3) (,&:(+/\"1)) i. 2 3",
    "1 2 (, & (+/)) 3 4",
    "(i.2 3) (, & (+/)) i. 2 3",
    // bonds
    "(1 & +) 5",
    "(1 & +) i. 2 3",
    "(^ & 2) 1 2 3",
    "(2 & ^) 1 2 3",
    "(10 & *) 1 2 3",
    "(- & 1) 5",
    "(1 & -) 5",
    "(1&+ @ *:) 1 2 3",
    "(^&2 @ (1&+)) 1 2 3",
    "(+&1 @ +&2) 5",
    "(1 & (,\"1)) i. 2 3",
    "((,\"1) & 1) i. 2 3",
    "(2 & {.) i. 3 3",
    "(1 2 3 & ,) 4 5",
    "(, & 4 5) 1 2 3",
    "(1 2 3 & ,) i. 2 3",
    // table (x u/ y)
    "1 2 3 +/ 10 20",
    "$ 1 2 3 +/ 10 20",
    "(i.2 2) +/ i.2",
    "1 2 3 */ 1 2 3",
    "2 3 4 +/ i. 2 2",
    "(i.2 3) +/ 10 20",
    "$ (i.2 3) +/ 10 20",
    "2 3 */ i. 2 2",
    "1 2 3 </ 2 3",
    "1 2 -/ 3 4",
    "1 2 %/ 3 4",
    "2 +/ 3",
    "(i.2 3) ,/ i. 1 3",
    "'ab' ,/ 'cd'",
    "(i.2 3) (,\"1)/ 1 2",
    "2 !/ 5 6",
    // factorial and binomial
    "! 0 1 2 3 4 5",
    "! 10",
    "! 20",
    "! 0.5",
    "! 2.5",
    "! _0.5",
    "! _1",
    "! _2",
    "! _3",
    "! i. 2 3",
    "2 ! 5",
    "0 ! 5",
    "5 ! 5",
    "6 ! 5",
    "_1 ! 5",
    "2 ! _5",
    "_3 ! _2",
    "_5 ! _3",
    "_2 ! _5",
    "2 ! 5.5",
    "0.5 ! 2",
    "2.5 ! 5",
    "1.5 ! 4.5",
    "_1.5 ! 2",
    "2 ! 5 6 7",
    "1 2 3 ! 5",
    "2 ! i. 2 3",
    "3 ! 100",
    "10 ! 100",
    "3 ! 3.0",
    // format
    "\": 5",
    "$ \": 5",
    "\": 2.5",
    "$ \": 2.5",
    "\": 1 2 3",
    "$ \": 1 2 3",
    "\": _1 22 333",
    "\": i. 2 3",
    "$ \": i. 2 3",
    "\": i. 2 3 4",
    "$ \": i. 2 3 4",
    "\": (2 2 $ 1 22 333 4)",
    "$ \": (2 2 $ 1 22 333 4)",
    "\": (2 2 $ 0.5 1 2 3)",
    "$ \": (2 2 $ 0.5 1 2 3)",
    "\": 'abc'",
    "$ \": 'abc'",
    "$ \": 'a'",
    "\": 1 2 3 = 1 5 3",
    "$ \": i. 0 3",
    "$ \": i. 2 0",
    "# \": 1 2 3",
    // decode
    "#. 1 0 1",
    "#. 1 1 1 1",
    "2 #. 1 0 1",
    "24 60 60 #. 1 2 3",
    "2 #. 1 2 3",
    "0 #. 1 2 3",
    "1 0 1 #. 1 2 3",
    "#. 2 3 $ 1 0 1 1 1 0",
    "#. i. 2 3",
    "2 #. i. 2 3",
    "#. 1.5 2",
    "$ #. i. 2 0",
    // encode
    "#: 5",
    "$ #: 5",
    "#: 2 5",
    "$ #: 2 5",
    "#: 0",
    "$ #: 0",
    "$ #: i. 0",
    "$ #: i. 0 3",
    "#: 8",
    "#: _5",
    "#: _1 _5",
    "#: 1 _5",
    "#: 0.5",
    "#: 2.5",
    "#: 1 0 1",
    "#: i. 2 3",
    "2 2 2 #: 5",
    "2 2 2 #: 5 6",
    "$ 2 2 #: i. 2 3",
    "0 0 #: 5",
    "24 60 60 #: 3723",
    "2 #: 5",
    "$ 2 #: 5",
    "$ 2 2 #: 5",
    "3 #: 1 5 6 _8",
    "$ 3 #: 1 5 6 _8",
    "_2 #: i. 2 3",
    "0 #: 5",
    "2 2 #: 2.5",
    // itemize and laminate
    ",: 1 2 3",
    "$ ,: 1 2 3",
    ",: i. 2 3",
    "$ ,: i. 2 3",
    "$ ,: 5",
    "$ ,: i. 0",
    "$ ,: i. 0 3",
    "1 2 ,: 3 4",
    "(i.2 3) ,: i. 2 3",
    "$ (i.2 3) ,: i. 2 3",
    "1 ,: 2",
    "$ 1 ,: 2",
    "1 2 3 ,: 4",
    "(i.2 3) ,: 1 2 3",
    "(i.2 3) ,: 5",
    "1 2 ,: 3 4 5",
    "1 2 ,: i. 0",
    "'ab' ,: 'cd'",
    // boxes: the drawing is part of the answer, so every one of these
    // compares the display jconsole prints
    "< 5",
    "$ < 5",
    "# < 5",
    "< i. 2 3",
    "< 'abc'",
    "< < 5",
    "> < 5",
    "> 5",
    "> 'abc'",
    "> < i. 2 3",
    "> 1;2 3",
    "> <\"0 i. 2 3",
    "<\"0 i. 2 3",
    "<\"1 i. 2 3",
    "$ <\"0 i. 2 3",
    "<\"0 i. 2 2 2",
    "1;2;3",
    "$ 1;2;3",
    "(1;2);3",
    "$ (1;2);3",
    "1;<2",
    "'ab';'cde'",
    "$ 'ab';'cde'",
    "1;2 3;'abc'",
    "1;(2 2 $ 1 2 3 4)",
    "1;2;3;'abc';(i. 2 2)",
    "; 1;2 3;4",
    "; 'ab';'cde'",
    "; 1;(i. 2 3)",
    "; (<i. 2 2),(<i. 2 3)",
    "; i. 2 3",
    "$ ; 1",
    "$ ; <''",
    "1 + &.> 1;2;3",
    "(1;2) ,&.> 3;4",
    "# &.> 'ab';'cde'",
    "1 ,&.> 1;2",
    "2 3 4 ,&.> 1;2;3",
    "+ &.> 1;2",
    "$ 1;2 3;'abc'",
    "# 1;2 3;'abc'",
    "{. 1;2 3;'abc'",
    "{: 1;2 3;'abc'",
    "}. 1;2 3;'abc'",
    "}: 1;2 3;'abc'",
    "1 { 1;2 3;'abc'",
    "|. 1;2 3;'abc'",
    "0 1 0 # 1;2 3;'abc'",
    "2 {. 1;2 3;'abc'",
    "4 {. 1;2 3;'abc'",
    "$ 4 {. 1;2 3;'abc'",
    "_4 {. 1;2 3;'abc'",
    "2 }. 1;2 3;'abc'",
    ",: 1;2 3;'abc'",
    "$ ,: 1;2 3;'abc'",
    "2 3 $ 1;2;3;4;5;6",
    "|: 2 3 $ 1;2;3;4;5;6",
    "2 2 $ 1;2;3",
    ", 2 3 $ 1;2;3;4;5;6",
    "3 {. 2 1 $ <'x'",
    "$ 3 {. 2 1 $ <'x'",
    "~. 1;2;1;2",
    "(1;2;3) i. <2",
    "(1;2;3) e. <2",
    "1 e. 1;2",
    "(1;2) -: (1;2)",
    "(1;2) -: (1;3)",
    "(<1) = (<1)",
    "(1;2) = (1;2)",
    "(<'') -: <i. 0",
    "'' -: i. 0",
    ",/ 1;2;3",
    "\": 1;2 3",
    "$ \": 1;2 3",
    "$ \": < 5",
    "$ \": 2 2 $ <'x'",
    "$ \": <\"0 i. 2 2 2",
    // --- comparison tolerance -----------------------------------------
    "1 = 1 + 2^_50",
    "1 = 1 + 2^_44",
    "1 = 1 + 2^_45",
    "1 = 1 - 2^_44",
    "1 = 1 - 2^_45",
    "2 = 2 + 2^_43",
    "2 = 2 + 2^_44",
    "4 = 4 + 2^_42",
    "1e10 = 1e10 + 1e_5",
    "1e10 = 1e10 + 1e_3",
    "1e_20 = 0",
    "0 = 1e_300",
    "_ = _",
    "__ = _",
    "_ = 1e300",
    "1 < 1 + 2^_50",
    "1 <: 1 - 2^_50",
    "1 > 1 - 2^_50",
    "1 >: 1 + 2^_50",
    "1 -: 1 + 2^_50",
    "(1 + 2^_50) e. 1 2 3",
    "(1 2 3) i. 1 + 2^_50",
    "<. 2.9999999999999",
    "<. 2.99999999999",
    ">. 3.0000000000001",
    "(<1) = (<1 + 2^_50)",
    "(2 2 $ 1 2 3 4) -: 2 2 $ 1 2 3 4.0000000000000004",
    // --- fit (u!.n) -----------------------------------------------------
    "1 =!.0 (1 + 2^_50)",
    "1 <!.0 (1 + 2^_50)",
    "1 ~:!.0 (1 + 2^_50)",
    "1 >:!.0 (1 + 2^_50)",
    "1 <:!.0 (1 + 2^_50)",
    "1 >!.0 (1 + 2^_50)",
    "1 -:!.0 (1 + 2^_50)",
    "1 e.!.0 (1 , 1 + 2^_50)",
    "(1 2 3) i.!.0 (1 + 2^_50)",
    "(<.!.0) 2.9999999999999",
    // --- indices and steps ----------------------------------------------
    "I. 0 1 0 2",
    "I. 1 0 1",
    "I. 0 0 0",
    "I. 1 1 1",
    "I. 2 3",
    "I. i.5",
    "$ I. 0 1 0 2",
    "I. 2 3 $ 1 0 1 0 1 0",
    "I. 0 1 0 2.5",
    "I. _1 2",
    "1 2 3 I. 0 1 2 3 4 5",
    "0 10 20 I. 5 15 25",
    "i: 3",
    "i: 2",
    "i: 0",
    "i: 2.5",
    "i: _3",
    "i: 0.5",
    "$ i: 3",
    "1 2 3 2 1 i: 2",
    "1 2 3 2 1 i: 5",
    "1 2 3 2 1 i. 2",
    "'abcba' i: 'b'",
    "2 i: 3",
    // --- key and oblique -------------------------------------------------
    "1 2 1 +//. 10 20 30",
    "1 2 1 </. 10 20 30",
    "(1 2 1) </. 'abc'",
    "'aab' </. 1 2 3",
    "1 1 2 2 #/. 'abcd'",
    "1 2 1 {./. 10 20 30",
    "</. i. 3 3",
    "+//. i. 3 3",
    "</. 'abc' ,: 'def'",
    "(0 1 0) </. i. 3 2",
    "1 2 +//. 10 20 30",
    // --- cut ---------------------------------------------------------------
    "<;._2 'a,b,c,'",
    "<;._1 ',a,b,c'",
    "<;._2 'a,b,,c,'",
    "<;._2 'abc'",
    "1 0 0 1 0 <;.1 'abcde'",
    "1 0 0 1 0 <;._1 'abcde'",
    "0 0 1 0 1 <;.2 'abcde'",
    "0 0 1 0 1 <;._2 'abcde'",
    "0 1 0 1 0 <;.1 'abcde'",
    "1 0 0 1 0 #;.1 'abcde'",
    "1 0 0 1 0 +/;.1 ] 1 2 3 4 5",
    "1 0 1 <;.1 i. 3 2",
    "<;.0 'abcde'",
    "] ;.0 i. 2 3",
    // --- amend and fetch ---------------------------------------------------
    "99 (1)} 10 20 30",
    "99 (0 2)} 10 20 30",
    "99 (_1)} 10 20 30",
    "99 (0 0)} 10 20 30",
    "(100 200) 0 2} 10 20 30",
    "(1 2) (0 1)} 10 20 30",
    "'x' 1} 'abc'",
    "(1 2) 0} i. 2 2",
    "99 (5)} 10 20 30",
    "1} 10 20 30",
    "1.5 (0)} 10 20 30",
    "(<9) (0)} 10 20 30",
    "99 (0)} 'abc'",
    "1 {:: 'abc' ; 'de' ; 'f'",
    "0 {:: <<1 2 3",
    "(0;1) {:: (1 2 3);(4 5 6)",
    "_1 {:: 1;2;3",
    "1 0 {:: (1 2 3);(4 5 6)",
    // --- matrix division ---------------------------------------------------
    "%. 2 2 $ 1 2 3 4",
    "%. 2 2 $ 4 7 2 6",
    "%. 3 3 $ 2 0 0 0 3 0 0 0 4",
    "%. 1 2 3",
    "%. 0.5",
    "%. 2 2 $ 1 2 2 4",
    "%. 2 3 $ i. 6",
    "(1 2) %. 2 2 $ 1 2 3 4",
    "(1 0 0) %. 3 2 $ 1 1 1 2 1 3",
    "(1 2 3) %. 3 1 $ 1 1 1",
    "(1 2 3) %. 3 3 $ 2 0 0 0 3 0 0 0 4",
    "(2 2 $ 1 0 0 1) %. 2 2 $ 1 2 3 4",
    // --- power with a verb operand -----------------------------------------
    "(>:^:(2&>)) 1",
    "(>:^:(2&>)) 5",
    "({.^:(1<#)^:_) 1 2 3",
    "(%:^:(1&<)^:_) 1e10",
    // --- primes -------------------------------------------------------------
    "p: 0",
    "p: i. 10",
    "p: 100",
    "p: 1000",
    "p: _1",
    "q: 12",
    "q: 1",
    "q: 97",
    "q: 1000000007",
    "$ q: 1",
    "q: 0",
];

/// Naming a verb changes how the sentences after it parse, so the reference
/// has the last word on what each of these programs means. One displayed
/// value per program: the comparison sees the last.
const NAMED: &[&str] = &[
    "mean =. +/ % #\nmean 1 2 3 4",
    "mean =. +/ % #\n(mean - {.) 1 2 3 4",
    "mean =. +/ % #\nmean\"1 i. 3 3",
    "mean =. +/ % #\n2 * mean 1 2 3 4",
    "n =. #\n2 n 1 2 3",
    "f =. +/\nf =. #\nf 1 2 3",
    "a =. 1 2 3\na =. +/\na 1 2 3",
    "f =. +/\nf =. 10 20\nf",
    "g =. +/ % #\nh =. g @: ,\nh i. 2 3",
    "sq =. *:\nsq 1 2 3",
    "d =. }.\nd 1 2 3",
];

/// Deterministic pseudo-random expressions over the implemented surface.
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
    // Verbs safe to fold over a vector of small integers.
    let dyads = ["+", "-", "*", "<.", ">.", "|", "=", "<", ">", "*.", "+."];
    // Verbs additionally safe with a scalar left argument. `{` is excluded:
    // its left argument has to index the right one, so it is generated on
    // its own below.
    let struct_dyads = ["|.", ",", "-:", "e.", ",:", "#.", "#:", "!"];
    let monads = [
        "-", "*", "|", "<.", ">.", "+/", ">./", "<./", "#", "$", ",", "|:", "|.", "~.", "{:",
        "}:", "<:", ">:", "+:", "*:", "-.", "-:", "/:", "\\:", ",:", "#.", "#:", "\":", "!",
    ];
    // Verbs whose reduction folds a window or a prefix without leaving the
    // reals, whatever small integers it is given.
    let folds = ["+", "*", "<.", ">."];
    let mut exprs = Vec::new();
    for _ in 0..300 {
        let n = 1 + (rng() % 5) as usize;
        let vec: Vec<String> = (0..n)
            .map(|_| {
                let v = (rng() % 19) as i64 - 9;
                if v < 0 { format!("_{}", -v) } else { v.to_string() }
            })
            .collect();
        let noun = match rng() % 3 {
            0 => vec.join(" "),
            1 => format!("i. {} {}", 1 + rng() % 3, 1 + rng() % 4),
            _ => format!("{} 4 $ {}", 1 + rng() % 3, vec.join(" ")),
        };
        // A noun with at least five items, so a window of 1 to 4 always has
        // both the full and the short cases available.
        let long = match rng() % 3 {
            0 => format!("i. {}", 5 + rng() % 4),
            1 => format!("{} 3 $ i. 12", 5 + rng() % 3),
            _ => format!("{} 4 $ {}", 5 + rng() % 4, vec.join(" ")),
        };
        let fold = |r: u64| folds[(r % folds.len() as u64) as usize];
        exprs.push(format!("{} +/\\ {long}", 1 + rng() % 4));
        exprs.push(format!("{}/\\ {long}", fold(rng())));
        exprs.push(format!("{}/\\. {long}", fold(rng())));
        exprs.push(format!("{} {}/\\ {long}", 1 + rng() % 4, fold(rng())));
        exprs.push(format!("_{} {}/\\ {long}", 1 + rng() % 4, fold(rng())));
        exprs.push(format!("{}~ {noun}", dyads[(rng() % dyads.len() as u64) as usize]));
        // The table: every cell of the left argument against every cell of
        // the right one.
        exprs.push(format!("{} {}/ {noun}", vec.join(" "), fold(rng())));
        let expr = match rng() % 4 {
            0 => format!("{} {}", monads[(rng() % monads.len() as u64) as usize], noun),
            1 => {
                let atom = (rng() % 7) as i64 - 3;
                let atom = if atom < 0 { format!("_{}", -atom) } else { atom.to_string() };
                let all = dyads.len() + struct_dyads.len();
                let k = (rng() % all as u64) as usize;
                let verb = if k < dyads.len() { dyads[k] } else { struct_dyads[k - dyads.len()] };
                format!("{atom} {verb} {noun}")
            }
            // Every generated noun has at least one item, so index 0 is
            // always in range.
            2 => format!("0 {{ {noun}"),
            _ => format!(
                "{}/ {}",
                dyads[(rng() % dyads.len() as u64) as usize],
                noun
            ),
        };
        exprs.push(expr);
    }
    exprs
}
