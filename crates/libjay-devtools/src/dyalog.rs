//! Dyalog APL as a black-box oracle.
//!
//! Dyalog is a SECOND APL, recorded under its own key (`dyalog:`) beside
//! GNU APL's (`gnu:`). libjay does not follow it: what it answers is data
//! for a future Dyalog dialect, never a gate. Like every oracle here it is
//! run and never read — no source, no workspace, no library of theirs
//! enters this repository.
//!
//! # What this module assumes
//!
//! It was written against Dyalog's published documentation, with no
//! interpreter to try it on, so every assumption is listed here and each is
//! a one-line fix if the first real run says otherwise. `verify` below runs
//! `2+2` before any recording starts and reports which of these broke.
//!
//! 1. The binary to run is `mapl`, the wrapper script that sets `DYALOG`,
//!    `WSPATH` and the rest of the environment the interpreter needs. The
//!    bare `dyalog` binary works too if that environment is already set.
//! 2. `-script` makes it read APL from a file (or stdin), one statement per
//!    line, without a session, without a banner, and without echoing the
//!    input. If a banner survives, it is dropped anyway: only the text
//!    between the two markers below is read. If the INPUT is echoed,
//!    `verify` says so — add `-q` (or whatever the installed version's
//!    quiet flag is) to `LIBJAY_ORACLE_DYALOG_FLAGS`.
//! 3. The value of a statement that is not an assignment is displayed, as
//!    in a session. This is the assumption `verify` is really testing.
//! 4. `⎕OFF` ends the run; reaching the end of the script does too.
//! 5. An error abandons the script, so the closing marker never prints.
//!    Should a version instead report the error and carry on, the error
//!    text inside the markers is recognised as well.
//! 6. A `∇`-definition is sent as the `⎕FX` that fixes the same function
//!    (`as_fx` below), because the `∇` editor cannot be driven over a pipe.
//! 7. `⎕PW`, `⎕PP`, `⎕ML` and `⎕IO` are assignable system variables.
//!    Pinning them is what makes a recording reproducible on another
//!    machine: `⎕ML←1` is Dyalog's own migration level (`↑` mix, `⊃`
//!    first), which is the dialect being recorded, and `⎕PW←32767` keeps a
//!    long vector on one line.
//!
//! Everything tunable without a rebuild:
//!
//! - `LIBJAY_ORACLE_DYALOG` — the interpreter's path.
//! - `LIBJAY_ORACLE_DYALOG_FLAGS` — the command line, space separated,
//!   replacing `-script`.
//! - `LIBJAY_ORACLE_DYALOG_STDIN` — set to feed the script on stdin
//!   instead of naming a file.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The command line, unless `LIBJAY_ORACLE_DYALOG_FLAGS` replaces it.
const FLAGS: &[&str] = &["-script"];

/// Printed by the script itself, so that whatever the interpreter says on
/// its way in and out is dropped rather than parsed. Nothing in a corpus
/// answer looks like these.
const BEGIN: &str = "###LIBJAY-BEGIN###";
const END: &str = "###LIBJAY-END###";

/// Where a macOS install puts the interpreter, and the names it takes on
/// `PATH`. The `.app` bundle is the account-gated download; the `/opt`
/// path is what the UNIX tarball unpacks to.
fn candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    for apps in [PathBuf::from("/Applications"), home.join("Applications")] {
        // `Dyalog-19.0.app` first, then any other version, newest name last
        // so that the highest version wins.
        let mut bundles: Vec<PathBuf> = std::fs::read_dir(&apps)
            .map(|dir| {
                dir.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with("Dyalog") && n.ends_with(".app"))
                    })
                    .collect()
            })
            .unwrap_or_default();
        bundles.sort();
        bundles.reverse();
        for bundle in bundles {
            let dir = bundle.join("Contents/Resources/Dyalog");
            out.push(dir.join("mapl"));
            out.push(dir.join("dyalog"));
        }
    }
    for dir in ["/opt/mdyalog/19.0/64/unicode", "/usr/local/bin", "/opt/homebrew/bin"] {
        out.push(PathBuf::from(dir).join("mapl"));
        out.push(PathBuf::from(dir).join("dyalog"));
    }
    for name in ["mapl", "dyalog"] {
        out.extend(on_path(name));
    }
    out
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(name)).find(|p| p.is_file())
}

/// Locate the interpreter. `LIBJAY_ORACLE_DYALOG` names it outright;
/// otherwise the install paths above are tried in order. The error text is
/// what a machine without Dyalog is told, and it is a skip rather than a
/// failure: Dyalog is not installed on every machine that records.
pub fn find() -> Result<PathBuf, String> {
    if let Ok(named) = std::env::var("LIBJAY_ORACLE_DYALOG") {
        let path = PathBuf::from(&named);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!("LIBJAY_ORACLE_DYALOG is {named}, which is not a file"));
    }
    if let Some(found) = candidates().into_iter().find(|p| p.is_file()) {
        return Ok(found);
    }
    Err("Dyalog APL is not installed here (no `mapl` in /Applications/Dyalog-*.app, \
         in /opt/mdyalog, or on PATH); set LIBJAY_ORACLE_DYALOG to its `mapl`"
        .to_string())
}

fn flags() -> Vec<String> {
    match std::env::var("LIBJAY_ORACLE_DYALOG_FLAGS") {
        Ok(given) => given.split_whitespace().map(str::to_string).collect(),
        Err(_) => FLAGS.iter().map(|f| f.to_string()).collect(),
    }
}

/// A `∇`-definition, rewritten as the `⎕FX` that fixes the same function.
///
/// Dyalog is driven here as a piped script, and opening the `∇` editor over
/// that channel makes it print a line prompt per line and echo the body, so
/// a definition written between two `∇`s cannot be recorded through the
/// channel at all. `⎕FX` takes the same lines as a vector of character
/// vectors and fixes the same function; its result is shy, so nothing but
/// the sentences after it displays.
///
/// This is the ONE place the text sent to an oracle is not the corpus text.
/// The corpus keeps the `∇` spelling, because that is the sentence libjay
/// is asked, and the two spellings define the same function by Dyalog's own
/// account of `⎕FX`. Anything the rewrite is not sure of — a `∇` that never
/// closes, a line inside the body that opens another definition — is passed
/// through untouched, so what Dyalog says about it is still recorded.
fn as_fx(expr: &str) -> String {
    if !expr.contains('∇') {
        return expr.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    let mut lines = expr.lines();
    while let Some(line) = lines.next() {
        let Some(header) = line.trim().strip_prefix('∇').map(str::trim) else {
            out.push(line.to_string());
            continue;
        };
        if header.is_empty() {
            return expr.to_string();
        }
        let mut body = vec![header.to_string()];
        let mut closed = false;
        for line in lines.by_ref() {
            let t = line.trim();
            if t == "∇" {
                closed = true;
                break;
            }
            if t.starts_with('∇') {
                return expr.to_string();
            }
            body.push(t.to_string());
        }
        if !closed {
            return expr.to_string();
        }
        let quoted: Vec<String> =
            body.iter().map(|l| format!("'{}'", l.replace('\'', "''"))).collect();
        out.push(format!("⎕FX {}", quoted.join(" ")));
    }
    out.join("\n")
}

/// The script one sentence is run as. The pins come first, the markers
/// bracket the sentence, and `⎕OFF` ends the run.
fn script(expr: &str, index_origin: u8) -> String {
    let mut out = String::new();
    out.push_str("⎕PW←32767\n⎕PP←10\n⎕ML←1\n");
    out.push_str(&format!("⎕IO←{index_origin}\n"));
    out.push_str(&format!("⎕←'{BEGIN}'\n"));
    out.push_str(as_fx(expr).trim_end());
    out.push('\n');
    out.push_str(&format!("⎕←'{END}'\n"));
    out.push_str("⎕OFF\n");
    out
}

/// Everything one run wrote, and whether it ended in an orderly way.
struct Run {
    stdout: String,
    stderr: String,
    ok: bool,
}

fn run(bin: &Path, cwd: &Path, text: &str) -> Result<Run, String> {
    let on_stdin = std::env::var_os("LIBJAY_ORACLE_DYALOG_STDIN").is_some();
    let mut command = Command::new(bin);
    command.args(flags());
    // A temporary script file per call: `-script FILE` is the documented
    // form. Named after the process and the thread, so parallel recordings
    // do not share one, and never inside the repository.
    let file = cwd.join("sentence.apls");
    if on_stdin {
        command.stdin(Stdio::piped());
    } else {
        std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
        command.arg(&file);
        // Closing stdin matters: if the flags are wrong and the
        // interpreter opens a session instead, it reads EOF and exits
        // rather than waiting for input that never comes.
        command.stdin(Stdio::null());
    }
    let mut child = command
        .current_dir(cwd)
        .env("TERM", "dumb")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawning {}: {e}", bin.display()))?;
    if on_stdin {
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(text.as_bytes())
            .map_err(|e| format!("writing to {}: {e}", bin.display()))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("waiting for Dyalog: {e}"))?;
    Ok(Run {
        stdout: String::from_utf8_lossy(&out.stdout).replace('\r', ""),
        stderr: String::from_utf8_lossy(&out.stderr).replace('\r', ""),
        ok: out.status.success(),
    })
}

/// The text between the markers, or `None` when the closing one never
/// arrived — which is how an abandoned script reads.
fn between_markers(stdout: &str) -> Option<&str> {
    let (_, after) = stdout.split_once(BEGIN)?;
    let after = after.strip_prefix('\n').unwrap_or(after);
    let (body, _) = after.split_once(END)?;
    Some(body)
}

/// A Dyalog error inside the markers, for the case where a version reports
/// one and carries on instead of abandoning the script: a named error, or
/// the caret line under the offending glyphs.
fn has_error_marker(text: &str) -> bool {
    text.lines().any(|line| {
        let t = line.trim();
        t.ends_with("ERROR")
            || t == "WS FULL"
            || (!t.is_empty() && t.chars().all(|c| c == '^' || c == ' '))
    })
}

/// Trailing spaces and the blank lines at either end go; interior blank
/// lines stay, because they carry the shape of a rank-3 result.
fn normalize(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(lines.len());
    let end = lines.iter().rposition(|l| !l.is_empty()).map_or(start, |i| i + 1);
    lines[start..end].join("\n")
}

/// Run one sentence. `None` is a refusal — Dyalog abandoned the script, or
/// reported an error inside it.
pub fn eval(bin: &Path, cwd: &Path, expr: &str, index_origin: u8) -> Option<String> {
    let text = script(expr, index_origin);
    let run = match run(bin, cwd, &text) {
        Ok(run) => run,
        // A spawn that fails mid-recording is not the sentence's fault;
        // recording stops rather than writing a refusal that is really a
        // broken harness.
        Err(message) => panic!("{message}"),
    };
    let body = between_markers(&run.stdout)?;
    if !run.ok && body.is_empty() {
        return None;
    }
    if has_error_marker(body) || !run.stderr.trim().is_empty() {
        return None;
    }
    Some(normalize(body))
}

/// Run `2+2` and check the answer, before a recording of thousands of
/// sentences starts on assumptions that may not hold. The message names the
/// assumption that broke and what to do about it.
pub fn verify(bin: &Path, cwd: &Path) -> Result<(), String> {
    let text = script("2+2", 1);
    let run = run(bin, cwd, &text)?;
    let report = |what: &str| {
        format!(
            "{what}\n  ran: {} {}\n  script:\n{}\n  stdout:\n{}\n  stderr:\n{}\n\
             The assumptions are listed at the top of crates/libjay-devtools/src/dyalog.rs; \
             LIBJAY_ORACLE_DYALOG_FLAGS and LIBJAY_ORACLE_DYALOG_STDIN adjust them without a \
             rebuild.",
            bin.display(),
            flags().join(" "),
            indent(&text),
            indent(&run.stdout),
            indent(&run.stderr)
        )
    };
    let Some(body) = between_markers(&run.stdout) else {
        return Err(report("Dyalog printed no marker, so its output cannot be read"));
    };
    let answer = normalize(body);
    if answer == "4" {
        return Ok(());
    }
    if answer.contains("2+2") {
        return Err(report("Dyalog echoed the input, which would land in every recording"));
    }
    Err(report(&format!("Dyalog answered {answer:?} to `2+2`, not \"4\"")))
}

fn indent(text: &str) -> String {
    text.lines().map(|l| format!("    {l}")).collect::<Vec<_>>().join("\n")
}
