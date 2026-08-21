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
      --check      compare instead of writing; nonzero exit on any drift
      --missing    record only expressions the snapshot does not have yet
  jay-corpus gen <j|apl> [--count N] [--seed S]
                                        append generated expressions
  jay-corpus fuzz <j|apl> [--count N] [--seed S] [--depth D]
                                        print composed expressions
      --compare    run libjay and the oracle over them, report the mismatches
      --quiet      with --compare, the summary only
      --exprs FILE read the expressions from a corpus file instead
  jay-corpus stats [j|apl]              corpus and snapshot sizes

FILE is a corpus file: a path, or a bare name such as `arithmetic`. With no
FILE, every corpus file of the language. LIBJAY_ORACLE_J and
LIBJAY_ORACLE_APL say where the interpreters are.";

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
    let mut positional: Vec<&String> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--check" => check = true,
            "--missing" => missing_only = true,
            other if other.starts_with("--") => return Err(format!("unknown option {other:?}")),
            _ => positional.push(arg),
        }
    }
    let lang = parse_lang(positional.first().copied())?;
    let paths: Vec<PathBuf> = if positional.len() > 1 {
        positional[1..].iter().map(|n| resolve(lang, n)).collect::<Result<_, _>>()?
    } else {
        corpus::files(lang)
    };
    let oracle = oracle::Oracle::find(lang)?;

    let mut complaints: Vec<String> = Vec::new();
    for path in &paths {
        let label = corpus::label(path);
        let snap = corpus::snapshot_of(path);
        let entries = corpus::read(path);
        let recorded = snapshot::index(snapshot::read(&snap));
        let divergences = corpus::is_divergences(path);
        let fresh: Vec<Record> = entries
            .par_iter()
            .map(|entry| {
                let cached =
                    if missing_only { recorded.get(&(entry.expr.clone(), entry.io)) } else { None };
                if let Some(old) = cached {
                    return old.clone();
                }
                let theirs = Side::of(oracle.eval(&entry.expr, entry.io));
                let ours = divergences
                    .then(|| Side::of(eval::eval(lang, &entry.expr, entry.io)));
                Record {
                    expr: entry.expr.clone(),
                    io: entry.io,
                    note: entry.note.clone(),
                    ours,
                    theirs: Some(theirs),
                }
            })
            .collect();

        if divergences {
            for record in &fresh {
                if record.note.is_none() {
                    complaints.push(format!("{label}: {:?} has no `? ` note", record.expr));
                }
                let ours = record.ours.as_ref().expect("a divergence records libjay's answer");
                if compare::sides_match(lang, ours, record.reference()) {
                    complaints.push(format!(
                        "{label}: {:?} no longer diverges, so the note should go",
                        record.expr
                    ));
                }
            }
        }

        if check {
            complaints.extend(drift(lang, &label, &fresh, &recorded));
            println!("{label}: {} expressions checked", fresh.len());
        } else {
            let title = format!(
                "libjay reference snapshot: {} — corpus/{label}.",
                libjay_testkit::reference_name(lang)
            );
            snapshot::write(&snap, &title, &fresh);
            println!("{label}: {} expressions recorded to {}", fresh.len(), display(&snap));
        }
    }
    if complaints.is_empty() {
        return Ok(());
    }
    Err(format!("{} problems:\n{}", complaints.len(), complaints.join("\n")))
}

/// Every way a freshly measured file differs from what is recorded.
fn drift(
    lang: Lang,
    label: &str,
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
            ("libjay", &new.ours, &old.ours),
            ("reference", &new.theirs, &old.theirs),
        ] {
            match (new_side, old_side) {
                (Some(n), Some(o)) if !compare::sides_match(lang, n, o) => out.push(format!(
                    "{label}: {}\n  {side} now: {}\n  snapshot:  {}",
                    new.expr,
                    n.describe(),
                    o.describe()
                )),
                (Some(_), None) | (None, Some(_)) => out.push(format!(
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

    let oracle = oracle::Oracle::find(lang)?;
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
    let langs = match args.first() {
        None => vec![Lang::J, Lang::Apl],
        Some(_) => vec![parse_lang(args.first())?],
    };
    for lang in langs {
        let mut exprs = 0;
        let mut records = 0;
        for path in corpus::files(lang) {
            let entries = corpus::read(&path);
            let recorded = snapshot::read(&corpus::snapshot_of(&path));
            println!(
                "{:<24} {:>5} expressions {:>5} recorded",
                corpus::label(&path),
                entries.len(),
                recorded.len()
            );
            exprs += entries.len();
            records += recorded.len();
        }
        let total = format!("{} total", libjay_testkit::lang_dir(lang));
        println!("{total:<24} {exprs:>5} expressions {records:>5} recorded\n");
    }
    Ok(())
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
