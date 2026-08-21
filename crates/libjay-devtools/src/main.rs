//! `jay-corpus`: the collecting half of libjay's differential testing.
//!
//! Collecting and testing are two activities. This binary is the first one:
//! it takes the expressions in `crates/libjay/tests/corpus/<lang>/*.txt`,
//! runs the reference interpreter on each, and writes what it answered to
//! `crates/libjay/tests/snapshots/<lang>/*.snap`. The second activity is
//! `cargo test`, which replays those recordings and never runs an
//! interpreter. The workflow is in docs/testing.md.
//!
//! The interpreters stay black-box oracles: never linked, never read.
//!
//! A snapshot record is a map: one answer per implementation, under its own
//! key. `--impl` chooses which of them a run records, and recording one
//! never disturbs another's answers.

mod dyalog;
mod fuzz;
mod generate;
mod oracle;

use std::path::{Path, PathBuf};

use libjay_testkit::snapshot::{Record, Side};
use libjay_testkit::{Lang, compare, corpus, eval, snapshot};
use rayon::prelude::*;

const USAGE: &str = "\
jay-corpus — record what the reference interpreters answer to the corpus.

  jay-corpus record <j|apl> [FILE...]   run the oracle, write the snapshots
      --impl NAME  which implementation to record: `j` for J, `gnu`
                   (default) or `dyalog` for APL. Only that key is written.
      --check      compare instead of writing; nonzero exit on any drift.
                   An implementation libjay does not follow reports its
                   agreement instead, and fails on nothing.
      --missing    record only expressions the key does not have yet
  jay-corpus gen <j|apl> [--count N] [--seed S]
                                        append generated expressions
  jay-corpus fuzz <j|apl> [--count N] [--seed S] [--depth D]
                                        print composed expressions
      --compare    run libjay and the oracle over them, report the mismatches
      --quiet      with --compare, the summary only
      --exprs FILE read the expressions from a corpus file instead
  jay-corpus stats [j|apl]              corpus and snapshot sizes
      --dialect-diff  every expression whose recorded Dyalog answer differs
                      from libjay: the backlog of a future Dyalog dialect

FILE is a corpus file: a path, or a bare name such as `arithmetic`. With no
FILE, every corpus file of the language. LIBJAY_ORACLE_J, LIBJAY_ORACLE_APL
and LIBJAY_ORACLE_DYALOG say where the interpreters are; a Dyalog that is
not installed is a skip, not a failure.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    };
    std::process::exit(code);
}

fn run(args: &[String]) -> Result<(), String> {
    let (command, rest) = args.split_first().ok_or_else(|| USAGE.to_string())?;
    match command.as_str() {
        "record" => record(rest),
        "gen" => generate_corpus(rest),
        "fuzz" => fuzz_command(rest),
        "stats" => stats(rest),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    }
}

fn parse_lang(arg: Option<&String>) -> Result<Lang, String> {
    match arg.map(String::as_str) {
        Some("j") => Ok(Lang::J),
        Some("apl") => Ok(Lang::Apl),
        Some(other) => Err(format!("unknown language {other:?}: expected `j` or `apl`")),
        None => Err(format!("a language is needed\n\n{USAGE}")),
    }
}

/// A corpus file named as a path, or by the bare theme name.
fn resolve(lang: Lang, name: &str) -> Result<PathBuf, String> {
    let dir = corpus::root().join(libjay_testkit::lang_dir(lang));
    let candidates = [
        PathBuf::from(name),
        dir.join(name),
        dir.join(format!("{name}.txt")),
        corpus::root().join("..").join("..").join("..").join(name),
    ];
    for path in candidates {
        if path.is_file() {
            return path.canonicalize().map_err(|e| format!("{}: {e}", path.display()));
        }
    }
    Err(format!("no corpus file {name:?} for {}", libjay_testkit::lang_dir(lang)))
}

fn record(args: &[String]) -> Result<(), String> {
    let mut check = false;
    let mut missing_only = false;
    let mut chosen: Option<String> = None;
    let mut positional: Vec<&String> = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--check" => check = true,
            "--missing" => missing_only = true,
            "--impl" => {
                let value = it.next().ok_or("--impl needs an implementation name")?;
                chosen = Some(value.clone());
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other:?}")),
            _ => positional.push(arg),
        }
    }
    let lang = parse_lang(positional.first().copied())?;
    let key = match chosen {
        Some(name) => {
            if !libjay_testkit::impls(lang).contains(&name.as_str()) {
                return Err(format!(
                    "no {name} implementation of {}: the keys are {}",
                    libjay_testkit::lang_name(lang),
                    libjay_testkit::impls(lang).join(", ")
                ));
            }
            name
        }
        None => libjay_testkit::followed_impl(lang).to_string(),
    };
    // libjay is HELD to the implementation its dialect follows: drift there
    // is a regression and fails the run. Any other implementation is
    // recorded as data — its disagreement is a dialect backlog, and a
    // machine without that interpreter skips instead of failing.
    let followed = key == libjay_testkit::followed_impl(lang);
    let paths: Vec<PathBuf> = if positional.len() > 1 {
        positional[1..].iter().map(|n| resolve(lang, n)).collect::<Result<_, _>>()?
    } else {
        corpus::files(lang)
    };
    let oracle = match oracle::Oracle::find(lang, &key) {
        Ok(oracle) => oracle,
        Err(absent) if !followed && absent.is_skippable() => {
            println!("{key}: nothing recorded — {}", absent.message());
            return Ok(());
        }
        Err(absent) => return Err(absent.message().to_string()),
    };

    let mut complaints: Vec<String> = Vec::new();
    let mut agreement = (0usize, 0usize);
    for path in &paths {
        let label = corpus::label(path);
        let snap = corpus::snapshot_of(path);
        let entries = corpus::read(path);
        let recorded = snapshot::index(snapshot::read(&snap));
        let divergences = corpus::is_divergences(path);
        // A record starts from what the snapshot already holds, so that
        // recording one implementation leaves every other one's answers,
        // and libjay's own recorded side, exactly as they were.
        let fresh: Vec<Record> = entries
            .par_iter()
            .map(|entry| {
                let old = recorded.get(&(entry.expr.clone(), entry.io));
                let mut record = old.cloned().unwrap_or_else(|| Record {
                    expr: entry.expr.clone(),
                    io: entry.io,
                    note: None,
                    ours: None,
                    answers: Default::default(),
                });
                record.note = entry.note.clone();
                if !(missing_only && record.answer(&key).is_some()) {
                    record.set(&key, Side::of(oracle.eval(&entry.expr, entry.io)));
                }
                if divergences {
                    record.ours = Some(Side::of(eval::eval(lang, &entry.expr, entry.io)));
                }
                record
            })
            .collect();

        // Re-measuring whether a deliberate divergence still diverges is
        // about the implementation libjay follows; a Dyalog run leaves that
        // judgement alone.
        if divergences && followed {
            for record in &fresh {
                if record.note.is_none() {
                    complaints.push(format!("{label}: {:?} has no `? ` note", record.expr));
                }
                let ours = record.ours.as_ref().expect("a divergence records libjay's answer");
                if compare::sides_match(lang, ours, record.followed(lang)) {
                    complaints.push(format!(
                        "{label}: {:?} no longer diverges, so the note should go",
                        record.expr
                    ));
                }
            }
        }

        if !followed {
            let (agree, differ) = agreement_with(lang, &key, &entries, &fresh);
            agreement.0 += agree;
            agreement.1 += differ;
            println!(
                "{label}: {} expressions, libjay agrees with {} on {agree} and differs on {differ}",
                fresh.len(),
                libjay_testkit::impl_name(&key)
            );
        }
        if check {
            if followed {
                complaints.extend(drift(lang, &label, &key, followed, &fresh, &recorded));
                println!("{label}: {} expressions checked", fresh.len());
            } else {
                for line in drift(lang, &label, &key, followed, &fresh, &recorded) {
                    println!("  changed since the last recording: {line}");
                }
            }
        } else {
            let title =
                format!("libjay reference snapshot: {} — corpus/{label}.", libjay_testkit::lang_name(lang));
            snapshot::write(&snap, lang, &title, &fresh);
            println!("{label}: {} expressions recorded to {}", fresh.len(), display(&snap));
        }
    }
    if !followed {
        let (agree, differ) = agreement;
        println!(
            "\n{}: libjay agrees on {agree} and differs on {differ} of {} expressions. \
             The difference is the backlog of a future {} dialect, not a failure.",
            libjay_testkit::impl_name(&key),
            agree + differ,
            libjay_testkit::impl_name(&key)
        );
    }
    if complaints.is_empty() {
        return Ok(());
    }
    Err(format!("{} problems:\n{}", complaints.len(), complaints.join("\n")))
}

/// How often libjay's own answer matches one implementation's, over the
/// records just measured. This is a report, never a verdict: it is asked of
/// implementations libjay does not follow.
fn agreement_with(
    lang: Lang,
    key: &str,
    entries: &[corpus::Entry],
    fresh: &[Record],
) -> (usize, usize) {
    let mut agree = 0;
    let mut differ = 0;
    let mine: Vec<Side> = entries
        .par_iter()
        .map(|entry| Side::of(eval::eval(lang, &entry.expr, entry.io)))
        .collect();
    for (ours, record) in mine.iter().zip(fresh) {
        let Some(theirs) = record.answer(key) else { continue };
        if compare::sides_match(lang, ours, theirs) {
            agree += 1;
        } else {
            differ += 1;
        }
    }
    (agree, differ)
}

/// Every way a freshly measured file differs from what is recorded, for the
/// implementation being recorded and for libjay's own side.
fn drift(
    lang: Lang,
    label: &str,
    key: &str,
    followed: bool,
    fresh: &[Record],
    recorded: &std::collections::HashMap<(String, u8), Record>,
) -> Vec<String> {
    let mut out = Vec::new();
    for new in fresh {
        let Some(old) = recorded.get(&new.key()) else {
            out.push(format!("{label}: {:?} is not in the snapshot", new.expr));
            continue;
        };
        for (side, new_side, old_side) in [
            ("libjay", new.ours.clone(), old.ours.clone()),
            (libjay_testkit::impl_name(key), new.answer(key).cloned(), old.answer(key).cloned()),
        ] {
            match (new_side, old_side) {
                (Some(n), Some(o)) if !compare::sides_match(lang, &n, &o) => out.push(format!(
                    "{label}: {}\n  {side} now: {}\n  snapshot:  {}",
                    new.expr,
                    n.describe(),
                    o.describe()
                )),
                // An implementation libjay does not follow is expected to
                // have nothing recorded yet, which is not drift.
                (Some(_), None) if followed => out.push(format!(
                    "{label}: {}: the snapshot holds no {side} answer",
                    new.expr
                )),
                (None, Some(_)) => out.push(format!(
                    "{label}: {}: the record no longer holds {side}'s answer",
                    new.expr
                )),
                _ => {}
            }
        }
    }
    let live: std::collections::HashSet<(String, u8)> = fresh.iter().map(Record::key).collect();
    for key in recorded.keys() {
        if !live.contains(key) {
            out.push(format!("{label}: {:?} is recorded but no longer in the corpus", key.0));
        }
    }
    out
}

fn generate_corpus(args: &[String]) -> Result<(), String> {
    let mut positional: Vec<&String> = Vec::new();
    let mut rounds: Option<usize> = None;
    let mut seed = generate::DEFAULT_SEED;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--count" => {
                let value = it.next().ok_or("--count needs a number")?;
                rounds = Some(value.parse().map_err(|_| format!("--count {value:?}"))?);
            }
            "--seed" => {
                let value = it.next().ok_or("--seed needs a number")?;
                seed = parse_seed(value)?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other:?}")),
            _ => positional.push(arg),
        }
    }
    let lang = parse_lang(positional.first().copied())?;
    let rounds = rounds.unwrap_or(match lang {
        Lang::J => generate::DEFAULT_ROUNDS,
        Lang::Apl => generate::DEFAULT_ROUNDS_APL,
    });
    let exprs = generate::generate(lang, rounds, seed);
    let path = corpus::root().join(libjay_testkit::lang_dir(lang)).join("generated.txt");
    let written = corpus::append(&path, &exprs);
    println!(
        "{}: {written} of {} generated expressions appended ({} were already there)",
        corpus::label(&path),
        exprs.len(),
        exprs.len() - written
    );
    if written > 0 {
        let dir = libjay_testkit::lang_dir(lang);
        println!("record them: cargo run -p libjay-devtools -- record {dir} generated");
    }
    Ok(())
}

/// Composed expressions, printed or compared. Nothing is written to the
/// corpus: a line worth keeping is moved into `fuzz_found.txt` by hand.
fn fuzz_command(args: &[String]) -> Result<(), String> {
    let mut positional: Vec<&String> = Vec::new();
    let mut count = 200usize;
    let mut seed = fuzz::DEFAULT_SEED;
    let mut depth = 3u32;
    let mut compare_them = false;
    let mut quiet = false;
    let mut given: Option<String> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--count" => {
                let value = it.next().ok_or("--count needs a number")?;
                count = value.parse().map_err(|_| format!("--count {value:?}"))?;
            }
            "--seed" => {
                let value = it.next().ok_or("--seed needs a number")?;
                seed = parse_seed(value)?;
            }
            "--depth" => {
                let value = it.next().ok_or("--depth needs a number")?;
                depth = value.parse().map_err(|_| format!("--depth {value:?}"))?;
            }
            "--compare" => compare_them = true,
            "--quiet" => quiet = true,
            "--exprs" => {
                let value = it.next().ok_or("--exprs needs a file")?;
                given = Some(value.clone());
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other:?}")),
            _ => positional.push(arg),
        }
    }
    let lang = parse_lang(positional.first().copied())?;
    // A file of expressions takes the generator's place, which is how a
    // candidate line is put through the same triage before it is trusted
    // enough to become a corpus line.
    let exprs = match &given {
        Some(path) => corpus::read(std::path::Path::new(path))
            .into_iter()
            .map(|e| e.expr)
            .collect(),
        None => fuzz::fuzz(lang, count, seed, depth),
    };
    if !compare_them {
        for expr in &exprs {
            println!("{expr}");
        }
        return Ok(());
    }

    let oracle = oracle::Oracle::find(lang, libjay_testkit::followed_impl(lang))
        .map_err(|absent| absent.message().to_string())?;
    let io = 1u8;
    let verdicts: Vec<(String, fuzz::Verdict, String, String)> = exprs
        .par_iter()
        .map(|expr| {
            let ours = libjay_testkit::eval::eval_detail(lang, expr, io);
            let theirs = oracle.eval(expr, io);
            let verdict = fuzz::triage(lang, &ours, theirs.as_deref());
            let ours_text = match &ours {
                libjay_testkit::eval::Answer::Value(v) => v.clone(),
                libjay_testkit::eval::Answer::NoValue => "<no value>".to_string(),
                libjay_testkit::eval::Answer::Refused(e) => format!("<error> {e}"),
            };
            let theirs_text = theirs.unwrap_or_else(|| "<error>".to_string());
            (expr.clone(), verdict, ours_text, theirs_text)
        })
        .collect();

    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for (expr, verdict, ours, theirs) in &verdicts {
        *counts.entry(verdict.label()).or_default() += 1;
        if verdict.is_mismatch() && !quiet {
            println!("--- {} : {expr}", verdict.label());
            println!("  libjay:    {}", one_line(ours));
            println!("  reference: {}", one_line(theirs));
        }
    }
    let mismatches: usize = verdicts.iter().filter(|v| v.1.is_mismatch()).count();
    let total = verdicts.len();
    println!("\n{total} expressions, {mismatches} mismatches ({:.1}%)", ratio(mismatches, total));
    for (label, n) in counts {
        println!("  {label:<12} {n:>5}  ({:.1}%)", ratio(n, total));
    }
    Ok(())
}

fn ratio(n: usize, total: usize) -> f64 {
    if total == 0 { 0.0 } else { 100.0 * n as f64 / total as f64 }
}

/// A multi-line answer on one line, so a mismatch stays one screenful.
fn one_line(text: &str) -> String {
    let joined = text.replace('\n', " / ");
    if joined.chars().count() > 200 {
        let cut: String = joined.chars().take(200).collect();
        format!("{cut}…")
    } else {
        joined
    }
}

fn parse_seed(value: &str) -> Result<u64, String> {
    let parsed = match value.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => value.parse(),
    };
    parsed.map_err(|_| format!("--seed {value:?}: expected a number or 0xHEX"))
}

fn stats(args: &[String]) -> Result<(), String> {
    let mut dialect_diff = false;
    let mut positional: Vec<&String> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--dialect-diff" => dialect_diff = true,
            other if other.starts_with("--") => return Err(format!("unknown option {other:?}")),
            _ => positional.push(arg),
        }
    }
    let langs = match positional.first() {
        None => vec![Lang::J, Lang::Apl],
        Some(_) => vec![parse_lang(positional.first().copied())?],
    };
    for lang in langs {
        if dialect_diff {
            dialect_backlog(lang);
            continue;
        }
        let mut exprs = 0;
        let mut records = 0;
        let mut other = 0;
        let backlog_key = libjay_testkit::backlog_impl(lang);
        for path in corpus::files(lang) {
            let entries = corpus::read(&path);
            let recorded = snapshot::read(&corpus::snapshot_of(&path));
            let with_backlog = match backlog_key {
                Some(key) => recorded.iter().filter(|r| r.answer(key).is_some()).count(),
                None => 0,
            };
            println!(
                "{:<24} {:>5} expressions {:>5} recorded{}",
                corpus::label(&path),
                entries.len(),
                recorded.len(),
                backlog_column(backlog_key, with_backlog)
            );
            exprs += entries.len();
            records += recorded.len();
            other += with_backlog;
        }
        let total = format!("{} total", libjay_testkit::lang_dir(lang));
        println!(
            "{total:<24} {exprs:>5} expressions {records:>5} recorded{}\n",
            backlog_column(backlog_key, other)
        );
    }
    Ok(())
}

fn backlog_column(key: Option<&str>, count: usize) -> String {
    match key {
        Some(key) => format!(" {count:>5} {key}"),
        None => String::new(),
    }
}

/// What a future dialect would have to explain: every expression whose
/// recorded answer from an implementation libjay does NOT follow differs
/// from libjay's own. Nothing here is a failure — it is a list of work.
fn dialect_backlog(lang: Lang) {
    let Some(key) = libjay_testkit::backlog_impl(lang) else {
        println!("{}: no second implementation is recorded\n", libjay_testkit::lang_name(lang));
        return;
    };
    let name = libjay_testkit::impl_name(key);
    let mut recorded = 0;
    let mut differ = 0;
    for path in corpus::files(lang) {
        let label = corpus::label(&path);
        let mut here = 0;
        for record in snapshot::read(&corpus::snapshot_of(&path)) {
            let Some(theirs) = record.answer(key) else { continue };
            recorded += 1;
            let ours = Side::of(eval::eval(lang, &record.expr, record.io));
            if compare::sides_match(lang, &ours, theirs) {
                continue;
            }
            differ += 1;
            here += 1;
            println!("{label}: {}", record.expr);
            println!("  libjay: {}", one_line(&ours.describe()));
            println!("  {name}: {}", one_line(&theirs.describe()));
        }
        if here > 0 {
            println!("{label}: {here} differ\n");
        }
    }
    if recorded == 0 {
        println!(
            "no {name} answers are recorded yet: \
             cargo run -p libjay-devtools -- record {} --impl {key}",
            libjay_testkit::lang_dir(lang)
        );
        return;
    }
    println!("{name}: {differ} of {recorded} recorded answers differ from libjay");
}

fn display(path: &Path) -> String {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(p), Ok(r)) => {
            p.strip_prefix(&r).unwrap_or(p.as_path()).display().to_string()
        }
        _ => path.display().to_string(),
    }
}
