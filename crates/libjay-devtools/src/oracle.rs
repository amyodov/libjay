//! Running a reference interpreter as a black-box subprocess.
//!
//! The interpreters are never linked and never read: one process per
//! sentence, in, out, and what came back is the recording. This is the only
//! place in the repository that spawns them.
//!
//! One implementation per [`Oracle`], named by the key its answers are
//! recorded under: `j` is jconsole, `gnu` is GNU APL, `dyalog` is Dyalog
//! APL (whose invocation lives in `dyalog.rs`). Adding an implementation is
//! adding an arm here and a key to `libjay_testkit::impls`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use libjay_testkit::{IMPL_DYALOG, IMPL_GNU, IMPL_J, Lang};

use crate::dyalog;

/// Why an implementation is not available. The two are answered
/// differently: an interpreter that is NOT INSTALLED is a fact about the
/// machine, and a run that merely records that implementation skips it,
/// while one that is installed and does not behave as its runner expects is
/// a fault to be shown and fixed.
pub enum Absent {
    NotInstalled(String),
    Misbehaving(String),
}

impl Absent {
    pub fn message(&self) -> &str {
        match self {
            Absent::NotInstalled(m) | Absent::Misbehaving(m) => m,
        }
    }

    /// Whether the caller may carry on without this implementation.
    pub fn is_skippable(&self) -> bool {
        matches!(self, Absent::NotInstalled(_))
    }
}

/// What one run of an interpreter came back with.
pub enum Reply {
    /// What it printed.
    Answer(String),
    /// It refused the sentence.
    Refused,
    /// It was still running when the limit ran out, and was killed. That is
    /// neither an answer nor a refusal: a composed sentence can ask a
    /// reference for work measured in hours, and nothing about it is
    /// recorded.
    TimedOut,
}

impl Reply {
    fn of(answer: Option<String>) -> Reply {
        match answer {
            Some(text) => Reply::Answer(text),
            None => Reply::Refused,
        }
    }

    /// The answer, with a refusal as `None`. A timeout is a refusal here,
    /// so a caller that has no opinion about it stays as it was.
    pub fn answer(self) -> Option<String> {
        match self {
            Reply::Answer(text) => Some(text),
            Reply::Refused | Reply::TimedOut => None,
        }
    }
}

/// How long one sentence may keep an interpreter busy, in seconds.
/// `LIBJAY_ORACLE_TIMEOUT` overrides it; 0 waits for ever.
fn limit() -> Option<std::time::Duration> {
    let secs: u64 = std::env::var("LIBJAY_ORACLE_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    (secs > 0).then(|| std::time::Duration::from_secs(secs))
}

/// Wait for a child that has already been written to, draining both pipes
/// in threads of their own so neither can fill and block, and killing it if
/// it outstays the limit. `None` is a kill.
fn wait_within(mut child: std::process::Child) -> Option<(String, String)> {
    fn drain<R: std::io::Read + Send + 'static>(r: R) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut r = r;
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            buf
        })
    }
    let out = drain(child.stdout.take().expect("piped stdout"));
    let err = drain(child.stderr.take().expect("piped stderr"));
    let deadline = limit().map(|d| std::time::Instant::now() + d);
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => break,
            Ok(None) => {}
        }
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let text = |h: std::thread::JoinHandle<Vec<u8>>| {
        String::from_utf8_lossy(&h.join().unwrap_or_default()).into_owned()
    };
    Some((text(out), text(err)))
}

/// Where the interpreter is, which implementation it is, and where its
/// scratch files may go.
pub struct Oracle {
    key: String,
    path: PathBuf,
    scratch: PathBuf,
}

impl Oracle {
    /// Locate one implementation's interpreter. `LIBJAY_ORACLE_J`,
    /// `LIBJAY_ORACLE_APL` and `LIBJAY_ORACLE_DYALOG` override the default
    /// install paths.
    ///
    /// A missing interpreter is an error here; whether that error stops the
    /// run or skips it is the caller's call, and depends on whether libjay
    /// is HELD to this implementation or merely records it.
    pub fn find(lang: Lang, key: &str) -> Result<Oracle, Absent> {
        let scratch = std::env::temp_dir().join(format!("libjay-corpus-{}", std::process::id()));
        std::fs::create_dir_all(&scratch)
            .map_err(|e| Absent::Misbehaving(format!("{}: {e}", scratch.display())))?;
        let (var, default) = match (lang, key) {
            (Lang::J, IMPL_J) => ("LIBJAY_ORACLE_J", "projects/libjay-oracles/j/j64/jconsole"),
            (Lang::Apl, IMPL_GNU) => {
                ("LIBJAY_ORACLE_APL", "projects/libjay-oracles/gnu-apl/install/bin/apl")
            }
            (Lang::Apl, IMPL_DYALOG) => {
                let path = dyalog::find().map_err(Absent::NotInstalled)?;
                let oracle = Oracle { key: key.to_string(), path, scratch };
                // Installed but answering something other than what the
                // runner assumes is a fault, not a skip: the assumptions at
                // the top of `dyalog.rs` are what needs correcting.
                dyalog::verify(&oracle.path, &oracle.thread_dir())
                    .map_err(Absent::Misbehaving)?;
                return Ok(oracle);
            }
            _ => {
                return Err(Absent::Misbehaving(format!(
                    "no {key} implementation of {}: the keys are {}",
                    libjay_testkit::lang_name(lang),
                    libjay_testkit::impls(lang).join(", ")
                )));
            }
        };
        let path = std::env::var(var).map(PathBuf::from).unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(default)
        });
        if !path.exists() {
            return Err(Absent::NotInstalled(format!(
                "recording needs the {} oracle; {} is not there (set {var})",
                libjay_testkit::impl_name(key),
                path.display()
            )));
        }
        Ok(Oracle { key: key.to_string(), path, scratch })
    }

    /// Run one sentence.
    pub fn eval(&self, expr: &str, index_origin: u8) -> Reply {
        match self.key.as_str() {
            IMPL_J => eval_j(&self.path, expr),
            IMPL_GNU => eval_apl(&self.path, &self.thread_dir(), expr, index_origin),
            IMPL_DYALOG => {
                Reply::of(dyalog::eval(&self.path, &self.thread_dir(), expr, index_origin))
            }
            other => panic!("no runner for the {other} implementation"),
        }
    }

    /// A working directory of this thread's own: GNU APL drops a history
    /// file into the directory it runs in and the Dyalog runner writes its
    /// script there, so parallel recordings must not share one. It is never
    /// the repository.
    fn thread_dir(&self) -> PathBuf {
        let dir = self.scratch.join(format!("t{}", rayon::current_thread_index().unwrap_or(0)));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}

/// jconsole's default output control is `0 256 0 222`: a result wider than
/// 256 columns or taller than 222 rows comes back ending in `...`, and an
/// abbreviation recorded as an answer is a wrong answer. The first sentence
/// of every run widens the page — it is the J side of GNU APL's `--PW`.
/// `0 0 $` swallows what it answers: some builds print an empty line for
/// the empty result, and that line would be recorded as the first line of
/// every answer.
const J_PREAMBLE: &str = "0 0 $ 9!:37 ] 0 4096 0 4096";

fn eval_j(jconsole: &Path, expr: &str) -> Reply {
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
        .write_all(format!("{J_PREAMBLE}\n{expr}\n").as_bytes())
        .expect("write to jconsole");
    let Some((text, complaint)) = wait_within(child) else {
        return Reply::TimedOut;
    };
    if (text.contains("error") && text.contains('|')) || !complaint.trim().is_empty() {
        return Reply::Refused;
    }
    Reply::Answer(text.trim_end().to_string())
}

/// `--script` silences the banner and the input echo, `--safe` and `--noSV`
/// keep the interpreter from opening sockets or loading a workspace, and a
/// wide `⎕PW` stops long vectors from wrapping onto continuation lines.
fn eval_apl(apl: &Path, cwd: &Path, expr: &str, index_origin: u8) -> Reply {
    let line = if index_origin == 1 { expr.to_string() } else { format!("⎕IO←0⋄{expr}") };
    // `--eval` takes one line. A `∇` definition needs several, so a program
    // with a line break goes in on stdin instead, closed with `)OFF` — the
    // interpreter otherwise sits in its definition editor waiting for more.
    let multiline = line.contains('\n');
    let mut command = Command::new(apl);
    command.args(["--script", "--safe", "--noSV", "--PW", "10000"]);
    if !multiline {
        command.args(["--eval", &line]);
    }
    let mut child = command
        .current_dir(cwd)
        .stdin(if multiline { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn GNU APL");
    if multiline {
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(format!("{line}\n)OFF\n").as_bytes())
            .expect("write to GNU APL");
    }
    let Some((stdout, stderr)) = wait_within(child) else {
        return Reply::TimedOut;
    };
    // GNU APL always exits 0; a failed sentence is reported on stderr as a
    // named error plus a caret line under the offending glyphs.
    if !stderr.trim().is_empty() || has_error_marker(&stdout) {
        return Reply::Refused;
    }
    Reply::Answer(normalize(&stdout))
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
