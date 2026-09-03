//! A sweep that survives the sentence that kills the runner.
//!
//! libjay's answer to a drawn sentence is taken behind `catch_unwind`, so a
//! panic is one finding rather than the end of a run. A fatal SIGNAL is not
//! a panic: a stack that runs off its guard page, an allocator that gives
//! up, a kernel that kills the process for its size — none of them unwind,
//! and one such sentence in fifty thousand ends a sweep with no output at
//! all, which is exactly what the round-5 seed 130002 did.
//!
//! So the measuring is done in a WORKER process and the reporting in the
//! SUPERVISOR that started it. Every result the worker gets is appended to
//! a journal before it goes on, and every sentence it is about to measure
//! is announced there first. When the worker dies, the supervisor knows
//! which sentences were in flight — at most one per thread — marks them
//! [`fuzz::Verdict::RunnerDied`], and starts another worker on what is
//! left. The journal is the measurement; the process that made it is
//! replaceable.
//!
//! The journal is a plain text file, one record per line, tab-separated,
//! with the corpus escapes (`\n`, `\t`, `\\`) inside every field:
//!
//! ```text
//! ?<TAB>io<TAB>drawn                              a sentence being measured
//! =<TAB>verdict<TAB>io<TAB>drawn<TAB>cut<TAB>ours<TAB>theirs      its result
//! ```

use std::io::Write;

use libjay_testkit::{Lang, corpus};

use crate::fuzz;
use crate::oracle;
use crate::{Finding, Outcome, measure_one};

/// How a supervised sweep is reported while it runs: one line per worker,
/// so a sweep of an hour says what it is doing.
const PROGRESS: &str = "  ";

/// How long a worker may go without writing anything down before the
/// supervisor takes it for stuck and kills it, in seconds.
/// `LIBJAY_SWEEP_STALL` overrides it; 0 waits for ever.
///
/// The sentences a generator draws include ones that ask libjay for an
/// array of two thousand million items, which is not a crash and not an
/// infinite loop: it is a request the machine cannot fill, and the process
/// grinds until the kernel kills it. Nothing INSIDE the process can stop
/// that — a thread cannot be interrupted — so the limit is kept outside it.
/// It is generous because a cut-down search legitimately spends many
/// interpreter runs on one mismatch, and a false kill costs a measurement.
const STALL: u64 = 600;

fn stall() -> Option<std::time::Duration> {
    let secs: u64 = std::env::var("LIBJAY_SWEEP_STALL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(STALL);
    (secs > 0).then(|| std::time::Duration::from_secs(secs))
}

/// Measure `probes` under a worker process, restarting it past any sentence
/// that kills it, and give back one finding per probe in the probes' own
/// order.
///
/// `keep` is where the journal is kept when the caller wants it afterwards;
/// with `None` it goes to a scratch directory that is removed at the end.
pub fn supervise(
    lang: Lang,
    probes: &[fuzz::Probe],
    signatures: bool,
    keep: Option<&str>,
) -> Result<Vec<Finding>, String> {
    let scratch = std::env::temp_dir().join(format!("libjay-sweep-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
    let exprs = scratch.join("probes.txt");
    write_probes(&exprs, probes)?;
    let path = match keep {
        Some(given) => std::path::PathBuf::from(given),
        None => scratch.join("journal.tsv"),
    };
    // A journal from an earlier run of another sweep would be read as this
    // one's measurements, so a named journal is started fresh.
    let _ = std::fs::remove_file(&path);

    // A generator draws the same sentence twice readily, and the journal
    // holds one record per sentence, so what the sweep is waiting for is
    // every DISTINCT probe.
    let wanted: std::collections::HashSet<Key> =
        probes.iter().map(|p| (p.expr.clone(), p.io)).collect();
    let exe = std::env::current_exe().map_err(|e| format!("finding this binary: {e}"))?;
    loop {
        let (results, in_flight) = read(&path);
        if results.len() >= wanted.len() {
            break;
        }
        if !in_flight.is_empty() {
            // A worker died with these in its hands. One of them is the
            // sentence that kills it and the others were merely running at
            // the time; nothing outside the process can tell which, so all
            // of them are named and none of them is measured.
            let mut file = append_to(&path)?;
            for (expr, io) in &in_flight {
                writeln!(file, "{}", died(expr, *io)).map_err(|e| format!("journal: {e}"))?;
            }
            drop(file);
            for (expr, io) in &in_flight {
                let origin = if *io == 1 { String::new() } else { format!(" [io={io}]") };
                println!("{PROGRESS}the runner died with this in flight{origin}: {expr}");
            }
            continue;
        }
        let before = results.len();
        let mut command = std::process::Command::new(&exe);
        command.args(["fuzz", libjay_testkit::lang_dir(lang), "--compare"]);
        command.arg("--probe-list").arg(&exprs);
        command.arg("--journal-run").arg(&path);
        if signatures {
            command.arg("--signature");
        }
        let ended = run_worker(&mut command, &path)?;
        let (after, in_flight) = read(&path);
        // A worker that neither finished nor left anything behind cannot be
        // stepped past: there is nothing to blame and nothing to skip.
        if !ended && after.len() == before && in_flight.is_empty() {
            return Err(format!(
                "the sweep worker stopped at {before} of {} sentences, having announced none",
                wanted.len()
            ));
        }
    }

    let (results, _) = read(&path);
    let findings = probes
        .iter()
        .map(|probe| {
            let key = (probe.expr.clone(), probe.io);
            results.get(&key).cloned().unwrap_or_else(|| Finding {
                expr: probe.expr.clone(),
                io: probe.io,
                drawn: fuzz::Verdict::RunnerDied,
                seen: Outcome {
                    verdict: fuzz::Verdict::RunnerDied,
                    ours_text: String::new(),
                    theirs_text: String::new(),
                },
            })
        })
        .collect();
    if keep.is_none() {
        let _ = std::fs::remove_dir_all(&scratch);
    } else {
        let _ = std::fs::remove_file(&exprs);
        println!("{PROGRESS}the journal is {}", path.display());
    }
    Ok(findings)
}

/// Run one worker to its end, killing it if it goes quiet for longer than
/// [`stall`] allows. `true` where it finished of its own accord.
///
/// The worker leads a process group, so the kill reaches the interpreter it
/// had running as well: killing the worker alone would leave a jconsole
/// holding a core.
fn run_worker(command: &mut std::process::Command, path: &std::path::Path) -> Result<bool, String> {
    let mut child =
        oracle::own_group(command).spawn().map_err(|e| format!("starting the sweep worker: {e}"))?;
    let limit = stall();
    let mut last = (written(path), std::time::Instant::now());
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Err(e) => return Err(format!("waiting for the sweep worker: {e}")),
            Ok(None) => {}
        }
        let now = written(path);
        if now != last.0 {
            last = (now, std::time::Instant::now());
        } else if limit.is_some_and(|d| last.1.elapsed() >= d) {
            println!("{PROGRESS}the worker wrote nothing for {:?}; killing it", limit.unwrap());
            oracle::kill_group(&mut child);
            let _ = child.wait();
            return Ok(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// How much the journal holds, as the measure of a worker's progress.
fn written(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// The worker half: measure every probe the journal does not hold yet,
/// writing each result down as it is made. Nothing is printed — the
/// supervisor reports, from the journal.
pub fn work(
    lang: Lang,
    oracle: &oracle::Oracle,
    probes: &[fuzz::Probe],
    path: &std::path::Path,
    signatures: bool,
) -> Result<(), String> {
    use rayon::prelude::*;

    let (results, _) = read(path);
    let file = std::sync::Mutex::new(append_to(path)?);
    let mut seen = std::collections::HashSet::<Key>::new();
    let left: Vec<&fuzz::Probe> = probes
        .iter()
        .filter(|p| {
            let key = (p.expr.clone(), p.io);
            !results.contains_key(&key) && seen.insert(key)
        })
        .collect();
    left.par_iter().for_each(|probe| {
        // The announcement goes down BEFORE the sentence is run, and the
        // journal is flushed with it: a record still in a buffer when the
        // process dies is a record that was never made.
        say(&file, &attempt(&probe.expr, probe.io));
        let finding = measure_one(lang, oracle, probe, signatures);
        say(&file, &result(&probe.expr, &finding));
    });
    Ok(())
}

fn say(file: &std::sync::Mutex<std::fs::File>, line: &str) {
    let mut file = file.lock().expect("the journal");
    let _ = writeln!(file, "{line}");
    let _ = file.flush();
}

fn append_to(path: &std::path::Path) -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn attempt(expr: &str, io: u8) -> String {
    format!("?\t{io}\t{}", corpus::escape(expr))
}

fn died(expr: &str, io: u8) -> String {
    format!(
        "=\t{}\t{io}\t{}\t{}\t\t",
        fuzz::Verdict::RunnerDied.label(),
        corpus::escape(expr),
        corpus::escape(expr)
    )
}

fn result(drawn: &str, finding: &Finding) -> String {
    format!(
        "=\t{}\t{}\t{}\t{}\t{}\t{}",
        finding.drawn.label(),
        finding.io,
        corpus::escape(drawn),
        corpus::escape(&finding.expr),
        corpus::escape(&finding.seen.ours_text),
        corpus::escape(&finding.seen.theirs_text)
    )
}

/// What a journal holds: the results by the sentence they were drawn for,
/// and the announcements no result ever followed — the sentences a worker
/// had in flight when it died.
type Key = (String, u8);
fn read(path: &std::path::Path) -> (std::collections::HashMap<Key, Finding>, Vec<Key>) {
    let mut results: std::collections::HashMap<Key, Finding> = std::collections::HashMap::new();
    let mut attempted: Vec<Key> = Vec::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return (results, attempted);
    };
    // A line half-written when the process died is not a record, and
    // neither is one whose escapes do not read: both are skipped in
    // silence, and the sentence they were about is measured again.
    let text_of = |field: &str| corpus::try_unescape(field).ok();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.first() {
            Some(&"?") if fields.len() == 3 => {
                let (Some(expr), Ok(io)) = (text_of(fields[2]), fields[1].parse()) else { continue };
                attempted.push((expr, io));
            }
            Some(&"=") if fields.len() == 7 => {
                let Some(verdict) = fuzz::Verdict::of_label(fields[1]) else { continue };
                let Ok(io) = fields[2].parse() else { continue };
                let (Some(drawn), Some(cut), Some(ours), Some(theirs)) = (
                    text_of(fields[3]),
                    text_of(fields[4]),
                    text_of(fields[5]),
                    text_of(fields[6]),
                ) else {
                    continue;
                };
                let finding = Finding {
                    expr: cut,
                    io,
                    drawn: verdict,
                    seen: Outcome { verdict, ours_text: ours, theirs_text: theirs },
                };
                results.insert((drawn, io), finding);
            }
            _ => {}
        }
    }
    let mut seen = std::collections::HashSet::<Key>::new();
    attempted.retain(|key| !results.contains_key(key) && seen.insert(key.clone()));
    (results, attempted)
}

/// The probes a supervisor hands its worker: `io<TAB>sentence`, escaped, one
/// per line. It is NOT the corpus format, because a drawn sentence is
/// arbitrary text — `? 5` is a roll — and the corpus format reads a line
/// beginning `? ` as a note about the line above it. A sweep must not lose
/// a sentence to the file it was written down in.
fn write_probes(path: &std::path::Path, probes: &[fuzz::Probe]) -> Result<(), String> {
    let mut text = String::new();
    for probe in probes {
        text.push_str(&format!("{}\t{}\n", probe.io, corpus::escape(&probe.expr)));
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Read back what [`write_probes`] wrote.
pub fn read_probes(path: &std::path::Path) -> Result<Vec<fuzz::Probe>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let (io, expr) = line.split_once('\t').ok_or_else(|| format!("line {}", i + 1))?;
        let io = io.parse().map_err(|_| format!("line {}: index origin {io:?}", i + 1))?;
        let expr = corpus::try_unescape(expr).map_err(|e| format!("line {}: {e}", i + 1))?;
        out.push(fuzz::Probe { expr, io });
    }
    Ok(out)
}
