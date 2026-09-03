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

mod coverage;
mod dyalog;
mod fuzz;
mod generate;
mod inventory;
mod journal;
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
      --signature  with --compare, cut every mismatch down to the smallest
                   sentence that still parts the two sides the same way,
                   report that sentence, and prefix it with a cause
                   signature, so a wrapper can deduplicate by cause
      --exprs FILE read the expressions from a corpus file instead, under
                   the index origin its `@ io=` directives give them
      --no-accepted  with --compare, do not read the accepted-divergence
                   list; every mismatch counts against agreement
      --journal FILE  with --compare, keep the sweep's journal here. Every
                   measurement is written down as it is made, so a sweep
                   that is interrupted resumes rather than starting again
      --no-supervise  with --compare, measure in this process instead of a
                   worker. Faster to start, and one sentence that kills the
                   runner takes the whole sweep with it
  jay-corpus coverage <j|apl>           which primitive × operand cells the
                                        recorded corpus exercises, and which
                                        are empty
      --top N      how many rows each section of the report prints
      --json FILE  the whole measurement, machine-readable
      --tsv FILE   the empty cells alone, one per line
  jay-corpus stats [j|apl]              corpus and snapshot sizes
      --dialect-diff  every expression whose recorded Dyalog answer differs
                      from libjay: the backlog of the Dyalog dialect
      --dialect NAME  run libjay under a dialect preset (gnu, dyalog) while
                      measuring that backlog, which is how much of it the
                      preset already answers

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
        "coverage" => coverage_command(rest),
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
    // A recording is a gate rather than a sweep, so it waits longer on the
    // interpreter than a sweep does.
    oracle::set_default_limit(oracle::RECORD_LIMIT);
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
    // Sentences the interpreter never answered, gathered across the files
    // and reported as complaints: a corpus line has to be recordable.
    let unrecordable: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    for path in &paths {
        let label = corpus::label(path);
        // A theme marked `@ reference=NAME` is that implementation's alone:
        // no other one can answer it, so recording another key into it
        // would fill the file with refusals. Skip it and say so.
        if let Some(named) = corpus::reference(path)
            && named != key
        {
            println!("{label}: skipped — recorded against {named} only");
            continue;
        }
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
                    let reply = oracle.eval_patiently(&entry.expr, entry.io);
                    // A corpus line the reference cannot answer — it never
                    // finished, or it printed more than one run may hold —
                    // is one no recording can hold: the previous answer
                    // stays and the run says which line it was.
                    match reply.cut_short() {
                        Some(why) => unrecordable
                            .lock()
                            .expect("the unanswered list")
                            .push(format!("{:?} {why}", entry.expr)),
                        None => record.set(&key, Side::of(reply.answer())),
                    }
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

        for line in unrecordable.lock().expect("the unanswered list").drain(..) {
            complaints.push(format!("{label}: {line}"));
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
    let mut signatures = false;
    let mut accepted_wanted = true;
    let mut given: Option<String> = None;
    let mut journal: Option<String> = None;
    let mut journal_run: Option<String> = None;
    let mut supervise = true;
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
            "--signature" => signatures = true,
            "--no-accepted" => accepted_wanted = false,
            "--exprs" => {
                let value = it.next().ok_or("--exprs needs a file")?;
                given = Some(value.clone());
            }
            "--journal" => {
                journal = Some(it.next().ok_or("--journal needs a file")?.clone());
            }
            "--journal-run" => {
                journal_run = Some(it.next().ok_or("--journal-run needs a file")?.clone());
            }
            "--no-supervise" => supervise = false,
            other if other.starts_with("--") => return Err(format!("unknown option {other:?}")),
            _ => positional.push(arg),
        }
    }
    let lang = parse_lang(positional.first().copied())?;
    // A file of expressions takes the generator's place, which is how a
    // candidate line is put through the same triage before it is trusted
    // enough to become a corpus line.
    // The index origin travels with the expression: a corpus file's
    // `@ io=0` directive says under which origin its lines mean what they
    // are recorded to mean, and comparing them under any other origin
    // compares something nobody wrote.
    let probes: Vec<fuzz::Probe> = match &given {
        Some(path) => corpus::read(std::path::Path::new(path))
            .into_iter()
            .map(|e| fuzz::Probe { expr: e.expr, io: e.io })
            .collect(),
        None => fuzz::fuzz(lang, count, seed, depth),
    };
    if !compare_them {
        let mut io = 1u8;
        for probe in &probes {
            if probe.io != io {
                println!("@ io={}", probe.io);
                io = probe.io;
            }
            println!("{}", probe.expr);
        }
        return Ok(());
    }

    // The worker half of a supervised sweep: measure what the journal does
    // not hold yet, write each result down as it comes, and report nothing.
    // The supervisor does the reporting, from the journal, so that a run
    // the worker does not survive still has every measurement it made.
    if let Some(path) = journal_run {
        let oracle = oracle::Oracle::find(lang, libjay_testkit::followed_impl(lang))
            .map_err(|absent| absent.message().to_string())?;
        return journal::work(lang, &oracle, &probes, std::path::Path::new(&path), signatures);
    }

    let oracle = oracle::Oracle::find(lang, libjay_testkit::followed_impl(lang))
        .map_err(|absent| absent.message().to_string())?;
    let accepted = if accepted_wanted { accepted_divergences(lang, &oracle) } else { Accepted::none() };
    let findings: Vec<Finding> = if supervise {
        journal::supervise(lang, &probes, signatures, journal.as_deref())?
    } else {
        measure_here(lang, &oracle, &probes, signatures)
    };

    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    let mut by_signature = std::collections::BTreeMap::<String, usize>::new();
    let mut matched_rows = std::collections::BTreeMap::<(Excuse, String), usize>::new();
    let mut by_excuse = std::collections::BTreeMap::<Excuse, usize>::new();
    let mut accepted_count = 0usize;
    for (finding, probe) in findings.iter().zip(&probes) {
        if !finding.drawn.is_mismatch() {
            *counts.entry(finding.drawn.label()).or_default() += 1;
            continue;
        }
        let seen = &finding.seen;
        // The signature names the cause rather than the sentence, so a
        // wrapper that keeps a set of them can tell a batch that found
        // something new from a batch that found another spelling of a
        // finding it already has.
        let sig = fuzz::signature(lang, seen.verdict, &finding.expr, &seen.ours_text);
        // A mismatch the divergence file already accounts for is not a
        // failure to close: it is a difference the corpus records with both
        // answers and a reason. It keeps its own class rather than the
        // verdict's, and stays out of the signature ranking, which is the
        // list of causes still to explain.
        let row = accepted.row_for(lang, &finding.expr, &sig, seen);
        match &row {
            Some((row, how)) => {
                accepted_count += 1;
                *by_excuse.entry(*how).or_default() += 1;
                *counts.entry("accepted").or_default() += 1;
                *matched_rows.entry((*how, (*row).clone())).or_default() += 1;
            }
            None => {
                *counts.entry(finding.drawn.label()).or_default() += 1;
                *by_signature.entry(sig.clone()).or_default() += 1;
            }
        }
        if !quiet {
            // A reported line is meant to be pasted into a corpus file, and
            // at origin 0 it means nothing without the directive.
            let origin = if finding.io == 1 { String::new() } else { format!(" [io={}]", finding.io) };
            let field = if signatures { format!("sig={sig} ") } else { String::new() };
            let label = match &row {
                Some((_, how)) => format!("accepted-{}/{}", how.label(), seen.verdict.label()),
                None => seen.verdict.label().to_string(),
            };
            println!("--- {field}{label}{origin} : {}", finding.expr);
            println!("  libjay:    {}", one_line(&seen.ours_text));
            println!("  reference: {}", one_line(&seen.theirs_text));
            if let Some((row, how)) = &row {
                println!("  accepted:  {row}   (by {})", how.label());
            }
            if finding.expr != probe.expr {
                println!("  cut from:  {}", probe.expr);
            }
        }
    }
    let aborts: Vec<&Finding> =
        findings.iter().filter(|f| f.drawn == fuzz::Verdict::OracleAbort).collect();
    let mismatches: usize = findings.iter().filter(|f| f.drawn.is_mismatch()).count();
    // An expression the oracle never finished was not compared, so it is
    // not in the denominator either.
    let total = findings.iter().filter(|f| f.drawn.is_compared()).count();
    let unexplained = mismatches - accepted_count;
    println!(
        "\ngeneration {}: {total} expressions, {mismatches} mismatches ({:.1}%)",
        fuzz::GENERATION,
        ratio(mismatches, total)
    );
    println!("  raw agreement                {:.2}%", ratio(total - mismatches, total));
    println!(
        "  accepted-adjusted agreement  {:.2}%  ({accepted_count} accepted, {unexplained} unexplained)",
        ratio(total - unexplained, total)
    );
    // The three ways a mismatch is excused, apart: a sentence and a
    // signature each match something measured, a FAMILY matches a class,
    // and the reader of a sweep is owed the difference.
    println!(
        "    accepted by sentence {:>5}, by signature {:>5}, by family {:>5}",
        by_excuse.get(&Excuse::Sentence).copied().unwrap_or(0),
        by_excuse.get(&Excuse::Signature).copied().unwrap_or(0),
        by_excuse.get(&Excuse::Family).copied().unwrap_or(0),
    );
    for (label, n) in counts {
        // A class that was never compared is not a share of the compared
        // total, so it is reported as a plain count.
        if label == "oracle-abort" || label == "unfinished" || label == "runner-died" {
            println!("  {label:<12} {n:>5}");
        } else {
            println!("  {label:<12} {n:>5}  ({:.1}%)", ratio(n, total));
        }
    }
    if !aborts.is_empty() {
        // The reference died on these, so neither side was measured. They
        // belong in the bug register, not in the divergence list: a corpus
        // row would kill the recorder on the sentence it records.
        println!("\n{} sentences the oracle crashed on (not compared):", aborts.len());
        let mut shown = std::collections::BTreeSet::<&str>::new();
        for finding in &aborts {
            if shown.insert(finding.expr.as_str()) {
                println!("  {}", finding.expr);
                println!("    libjay: {}", one_line(&finding.seen.ours_text));
            }
        }
    }
    let killed: Vec<&Finding> =
        findings.iter().filter(|f| f.drawn == fuzz::Verdict::RunnerDied).collect();
    if !killed.is_empty() {
        // Neither side was measured, and the fault is ours: a sentence that
        // takes the process down with it is a libjay bug of its own, and
        // the supervisor's whole purpose is that the sweep still reports it.
        println!("\n{} sentences the runner died on (not compared):", killed.len());
        let mut shown = std::collections::BTreeSet::<&str>::new();
        for finding in &killed {
            if shown.insert(finding.expr.as_str()) {
                println!("  {}", finding.expr);
            }
        }
    }
    if !matched_rows.is_empty() {
        // Which pinned row each accepted mismatch was excused by, so the
        // exclusion is auditable rather than a number to be trusted.
        println!("\naccepted divergences matched ({}):", accepted.path.display());
        let mut ranked: Vec<(&(Excuse, String), &usize)> = matched_rows.iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for ((how, row), n) in ranked {
            println!("  {n:>5}  {:<9}  {row}", how.label());
        }
    }
    if signatures {
        // The coarse half first: it is the half that answers "did this batch
        // find a cause the sweeper has not already got", which a count of
        // rows or of whole signatures cannot.
        let mut by_class = std::collections::BTreeMap::<&str, usize>::new();
        for (sig, n) in &by_signature {
            *by_class.entry(fuzz::cause_class(sig)).or_default() += n;
        }
        println!("\n{} distinct unexplained causes:", by_class.len());
        let mut ranked: Vec<(&&str, &usize)> = by_class.iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (class, n) in ranked {
            println!("  {n:>5}  {class}");
        }
        let mut ranked: Vec<(&String, &usize)> = by_signature.iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        println!("\n{} distinct unexplained signatures", ranked.len());
    }
    Ok(())
}

/// The differences libjay makes on purpose: `corpus/<lang>/divergences.txt`,
/// every line of it a sentence whose two answers are both recorded and whose
/// `? ` note says why they part.
///
/// A sweep counts a mismatch it matches under its own class rather than
/// against agreement, so the headline number measures what is still to
/// explain. Matching is by the sentence and by the CAUSE SIGNATURE: a pinned
/// row and a sentence the fuzzer drew that parts the same way over the same
/// primitives are one difference written twice, and the list would otherwise
/// have to hold every spelling a generator can reach. The list is small and
/// each row is reasoned in docs/coverage.md; nothing else is excused.
struct Accepted {
    path: std::path::PathBuf,
    exprs: std::collections::HashSet<String>,
    /// A signature to the row that earned it.
    signatures: std::collections::HashMap<String, String>,
    /// The `~ ` family rules, each with the sentence it hangs under.
    families: Vec<(fuzz::Family, String)>,
}

/// Which of the three ways a mismatch was excused. They are counted apart
/// because they are trusted apart: a sentence matches one recorded pair, a
/// signature matches one recorded cause, and a family matches a class of
/// sentences nobody has measured one by one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Excuse {
    Sentence,
    Signature,
    Family,
}

impl Excuse {
    fn label(self) -> &'static str {
        match self {
            Excuse::Sentence => "sentence",
            Excuse::Signature => "signature",
            Excuse::Family => "family",
        }
    }
}

impl Accepted {
    fn none() -> Accepted {
        Accepted {
            path: std::path::PathBuf::from("(none)"),
            exprs: std::collections::HashSet::new(),
            signatures: std::collections::HashMap::new(),
            families: Vec::new(),
        }
    }

    /// The pinned row a mismatch is an instance of and how it was matched,
    /// and `None` where it is something still to explain.
    fn row_for(
        &self,
        lang: libjay_testkit::Lang,
        expr: &str,
        signature: &str,
        seen: &Outcome,
    ) -> Option<(&String, Excuse)> {
        if let Some(row) = self.exprs.get(expr) {
            return Some((row, Excuse::Sentence));
        }
        if let Some(row) = self.signatures.get(signature) {
            return Some((row, Excuse::Signature));
        }
        self.families
            .iter()
            .find(|(family, _)| {
                family.covers(lang, seen.verdict, expr, &seen.ours_text, &seen.theirs_text)
            })
            .map(|(_, row)| (row, Excuse::Family))
    }
}

/// Read the divergence file and measure each of its rows, so that a sweep
/// can recognise the same difference drawn another way. A row that no longer
/// parts the two sides contributes nothing — `record --check` is what fails
/// on that, and a sweep should not pretend it is still a divergence.
fn accepted_divergences(lang: libjay_testkit::Lang, oracle: &oracle::Oracle) -> Accepted {
    let path = corpus::root().join(libjay_testkit::lang_dir(lang)).join("divergences.txt");
    if !path.exists() {
        return Accepted { path, ..Accepted::none() };
    }
    let entries = corpus::read(&path);
    let mut out = Accepted { path, ..Accepted::none() };
    let measured: Vec<(&corpus::Entry, Outcome)> = entries
        .par_iter()
        .map(|entry| (entry, put(lang, oracle, &entry.expr, entry.io)))
        .collect();
    for (entry, seen) in measured {
        if !seen.verdict.is_mismatch() {
            continue;
        }
        let expr = entry.expr.clone();
        let sig = fuzz::signature(lang, seen.verdict, &expr, &seen.ours_text);
        out.signatures.entry(sig).or_insert_with(|| expr.clone());
        // A malformed family rule is a malformed corpus file: it is said
        // once, by name, rather than silently excusing nothing.
        if let Some(rule) = &entry.family {
            let family = fuzz::Family::parse(rule)
                .unwrap_or_else(|e| panic!("{expr:?}: the family rule {rule:?}: {e}"));
            assert!(
                family.covers(lang, seen.verdict, &expr, &seen.ours_text, &seen.theirs_text),
                "{expr:?}: its own family rule does not cover it, so it covers the wrong thing"
            );
            out.families.push((family, expr.clone()));
        }
        out.exprs.insert(expr);
    }
    out
}

/// One sentence put to both sides.
#[derive(Clone)]
struct Outcome {
    verdict: fuzz::Verdict,
    /// libjay's answer as a printed line: `<panic>`, `<no value>`,
    /// `<error> …`, or the value.
    ours_text: String,
    theirs_text: String,
}

/// What a probe came to: the sentence a reader should look at — the drawn
/// one, or the smallest cut of it that parts the two sides the same way —
/// what that sentence came to, and how the drawn sentence itself parted,
/// which is what the run's tallies count.
#[derive(Clone)]
struct Finding {
    expr: String,
    io: u8,
    drawn: fuzz::Verdict,
    seen: Outcome,
}

/// Measure every probe in this process. The plain path: fast, and one
/// sentence that kills the process takes the whole measurement with it,
/// which is what [`journal::supervise`] exists to prevent.
fn measure_here(
    lang: libjay_testkit::Lang,
    oracle: &oracle::Oracle,
    probes: &[fuzz::Probe],
    signatures: bool,
) -> Vec<Finding> {
    probes.par_iter().map(|probe| measure_one(lang, oracle, probe, signatures)).collect()
}

/// One probe, put to both sides and — where it parted them and the caller
/// asked for signatures — cut down to the smallest sentence that still
/// parts them the same way.
///
/// A composed sentence is a tree with a bug somewhere inside it, and the
/// whole tree names eight or ten primitives that are a property of the
/// draw. The cut is what makes the signature name the cause: without it one
/// cause is signed once per subset it can be drawn inside, and a seen-set
/// grows for ever without learning anything.
fn measure_one(
    lang: libjay_testkit::Lang,
    oracle: &oracle::Oracle,
    probe: &fuzz::Probe,
    signatures: bool,
) -> Finding {
    let (expr, io) = (probe.expr.as_str(), probe.io);
    let drawn = put(lang, oracle, expr, io);
    if !signatures || !drawn.verdict.is_mismatch() {
        return Finding { expr: expr.to_string(), io, drawn: drawn.verdict, seen: drawn };
    }
    let smallest = fuzz::reduce(expr, fuzz::REDUCE_BUDGET, |candidate| {
        let ours = ours_of(lang, candidate, io);
        fuzz::could_part(drawn.verdict, ours.as_ref())
            && compare(lang, oracle, candidate, io, ours).verdict == drawn.verdict
    });
    let verdict = drawn.verdict;
    let seen = if smallest == expr { drawn } else { put(lang, oracle, &smallest, io) };
    Finding { expr: smallest, io, drawn: verdict, seen }
}

/// libjay's answer to one sentence, or `None` where it panicked. A panic is
/// a crash, not a diagnostic: catching it keeps one bad sentence from ending
/// a run of thousands, and reports it under its own name.
fn ours_of(
    lang: libjay_testkit::Lang,
    expr: &str,
    io: u8,
) -> Option<libjay_testkit::eval::Answer> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        libjay_testkit::eval::eval_detail(lang, expr, io)
    }))
    .ok()
}

/// One sentence put to both sides.
fn put(
    lang: libjay_testkit::Lang,
    oracle: &oracle::Oracle,
    expr: &str,
    io: u8,
) -> Outcome {
    let ours = ours_of(lang, expr, io);
    compare(lang, oracle, expr, io, ours)
}

/// The oracle's half of a comparison, over an answer libjay has already
/// given. Splitting it from [`put`] is what lets a cut-down search throw a
/// candidate away on libjay's answer alone, without starting an interpreter
/// for it.
fn compare(
    lang: libjay_testkit::Lang,
    oracle: &oracle::Oracle,
    expr: &str,
    io: u8,
    ours: Option<libjay_testkit::eval::Answer>,
) -> Outcome {
    let theirs = oracle.eval(expr, io);
    // A run the oracle never answered — killed at the limit, or cut off for
    // printing more than one run may hold — was not compared, and neither
    // side is at fault for it.
    let unanswered = !theirs.is_comparable();
    let crashed = theirs.crashed();
    let theirs = theirs.answer();
    // A sentence the oracle DIED on is the reference's own bug, and libjay's
    // answer to it stands unmeasured: reporting it as a difference would
    // count the crash against libjay. A panic of ours is still ours, so it
    // is named first.
    let verdict = match (&ours, crashed, unanswered) {
        (None, _, _) => fuzz::Verdict::Panicked,
        (_, true, _) => fuzz::Verdict::OracleAbort,
        (_, _, true) => fuzz::Verdict::Unfinished,
        (Some(ours), _, false) => fuzz::triage(lang, ours, theirs.as_deref()),
    };
    let ours_text = match &ours {
        None => "<panic>".to_string(),
        Some(libjay_testkit::eval::Answer::Value(v)) => v.clone(),
        Some(libjay_testkit::eval::Answer::NoValue) => "<no value>".to_string(),
        Some(libjay_testkit::eval::Answer::Refused(e)) => format!("<error> {e}"),
    };
    let theirs_text = theirs.unwrap_or_else(|| {
        match (crashed, unanswered) {
            (true, _) => "<oracle crashed>".to_string(),
            (_, true) => "<unfinished>".to_string(),
            _ => "<error>".to_string(),
        }
    });
    Outcome { verdict, ours_text, theirs_text }
}

/// Measure the corpus against the primitive × operand grid and report it.
/// Nothing is run but libjay itself: no oracle, and nothing is written to
/// the corpus or the snapshots.
fn coverage_command(args: &[String]) -> Result<(), String> {
    let mut positional: Vec<&String> = Vec::new();
    let mut top = coverage::DEFAULT_TOP;
    let mut json: Option<String> = None;
    let mut tsv: Option<String> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--top" => {
                let value = it.next().ok_or("--top needs a number")?;
                top = value.parse().map_err(|_| format!("--top {value:?}"))?;
            }
            "--json" => json = Some(it.next().ok_or("--json needs a file")?.clone()),
            "--tsv" => tsv = Some(it.next().ok_or("--tsv needs a file")?.clone()),
            other if other.starts_with("--") => return Err(format!("unknown option {other:?}")),
            _ => positional.push(arg),
        }
    }
    let lang = parse_lang(positional.first().copied())?;
    // Classifying means running every operand subtree, and a subtree that
    // panics is a measurement that stops rather than a run that ends. The
    // panic is caught where it happens; the hook only keeps the noise off
    // the report.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let cov = coverage::measure_corpus(lang);
    std::panic::set_hook(previous);

    let inv = coverage::inventory_of(lang);
    print!("{}", coverage::report(lang, &cov, &inv, top));
    if let Some(path) = json {
        std::fs::write(&path, coverage::json(lang, &cov, &inv))
            .map_err(|e| format!("writing {path}: {e}"))?;
        println!("\nthe whole measurement: {path}");
    }
    if let Some(path) = tsv {
        std::fs::write(&path, coverage::tsv(lang, &cov))
            .map_err(|e| format!("writing {path}: {e}"))?;
        println!("the empty cells: {path}");
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
    let mut dialect = jay::Dialect::default();
    let mut dialect_name = "gnu".to_string();
    let mut positional: Vec<&String> = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--dialect-diff" => dialect_diff = true,
            "--dialect" => {
                let name = it.next().ok_or("--dialect needs a name")?;
                dialect = eval::preset(name)
                    .ok_or_else(|| format!("unknown dialect {name:?} (gnu, dyalog)"))?;
                dialect_name = name.clone();
            }
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
            dialect_backlog(lang, dialect, &dialect_name);
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
fn dialect_backlog(lang: Lang, dialect: jay::Dialect, dialect_name: &str) {
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
            let ours = Side::of(eval::eval_as(lang, &record.expr, record.io, dialect));
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
    println!(
        "{name}: {differ} of {recorded} recorded answers differ from libjay \
         under the {dialect_name} dialect"
    );
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
