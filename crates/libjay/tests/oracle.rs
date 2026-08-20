//! Differential tests against the reference J implementation, run as a
//! black-box subprocess (never linked, never read). Skipped when the oracle
//! binary is absent.
//!
//! Comparison is textual with numeric tolerance: both sides print 6
//! significant digits, so parsing the tokens back gives at most ~5e-6
//! relative representation error per side; a relative tolerance of 1e-5
//! (with a 1e-9 absolute floor for values near zero) accepts exactly that
//! and still catches any real semantic difference.

use std::io::Write;
use std::process::{Command, Stdio};

use jay::fmt::{format_array, FmtOpts};
use jay::{compile, Dialect, Lang};

fn oracle_path() -> Option<String> {
    let path = std::env::var("LIBJAY_ORACLE_J").unwrap_or_else(|_| {
        format!(
            "{}/projects/libjay-oracles/j/j64/jconsole",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    std::path::Path::new(&path).exists().then_some(path)
}

/// Run one sentence through the reference interpreter. None on error.
fn oracle_eval(jconsole: &str, expr: &str) -> Option<String> {
    let mut child = Command::new(jconsole)
        .args(["-jprofile", "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
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
    if text.contains("error") && text.contains('|') {
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

fn check(jconsole: &str, exprs: &[String]) {
    let mut failures = Vec::new();
    let mut compared = 0;
    for expr in exprs {
        let ours = jay_eval(expr);
        let theirs = oracle_eval(jconsole, expr);
        match (ours, theirs) {
            (Some(o), Some(t)) => {
                compared += 1;
                if !outputs_match(&o, &t) {
                    failures.push(format!("{expr}\n  ours:   {o:?}\n  oracle: {t:?}"));
                }
            }
            // Both erroring is agreement; error texts are not compared.
            (None, None) => compared += 1,
            (Some(o), None) => {
                failures.push(format!("{expr}\n  ours: {o:?}\n  oracle: error"))
            }
            (None, Some(t)) => {
                failures.push(format!("{expr}\n  ours: error\n  oracle: {t:?}"))
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} differ from the reference:\n{}",
        failures.len(),
        compared,
        failures.join("\n")
    );
    eprintln!("oracle agreement on {compared} expressions");
}

#[test]
fn fixed_corpus_matches_reference() {
    let Some(jconsole) = oracle_path() else {
        eprintln!("J oracle not found; skipping (set LIBJAY_ORACLE_J)");
        return;
    };
    let exprs: Vec<String> = [
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
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    check(&jconsole, &exprs);
}

/// Deterministic pseudo-random expressions over the implemented surface.
#[test]
fn generated_corpus_matches_reference() {
    let Some(jconsole) = oracle_path() else {
        eprintln!("J oracle not found; skipping (set LIBJAY_ORACLE_J)");
        return;
    };
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
    let struct_dyads = ["|.", ",", "-:", "e."];
    let monads = [
        "-", "*", "|", "<.", ">.", "+/", ">./", "<./", "#", "$", ",", "|:", "|.", "~.", "{:",
        "}:", "<:", ">:", "+:", "*:", "-.", "-:", "/:", "\\:",
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
    check(&jconsole, &exprs);
}
