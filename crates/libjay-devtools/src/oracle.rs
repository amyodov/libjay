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

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
    /// It printed past the capture cap and was killed. Neither an answer
    /// nor a refusal either: what came back is the front of an answer, and
    /// an abbreviation recorded as an answer is a wrong answer.
    Overflowed,
    /// The interpreter died on the sentence — an abort or a fatal signal,
    /// not a diagnostic. It is not a refusal: a refusal is an answer of a
    /// kind ("this is not in the domain"), while a crash is the reference
    /// declining to have an opinion, and comparing against it would count a
    /// bug in the reference as a difference in libjay.
    Crashed,
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
            Reply::Refused | Reply::TimedOut | Reply::Overflowed | Reply::Crashed => None,
        }
    }

    /// Whether the interpreter died on the sentence.
    pub fn crashed(&self) -> bool {
        matches!(self, Reply::Crashed)
    }

    /// Whether what came back is comparable at all: an answer or a
    /// refusal, rather than a run cut short.
    pub fn is_comparable(&self) -> bool {
        matches!(self, Reply::Answer(_) | Reply::Refused)
    }

    /// How a run cut short is named where a recording would otherwise say
    /// what the reference answered.
    pub fn cut_short(&self) -> Option<&'static str> {
        match self {
            Reply::TimedOut => Some("did not finish (LIBJAY_ORACLE_TIMEOUT)"),
            Reply::Overflowed => Some("printed past the capture cap (LIBJAY_ORACLE_CAPTURE)"),
            Reply::Crashed => Some("crashed on the sentence"),
            Reply::Answer(_) | Reply::Refused => None,
        }
    }
}

/// How long one sentence may keep an interpreter busy, in seconds, when
/// nothing says otherwise. A SWEEP is throughput: it draws sentences that
/// ask a reference for work measured in hours, and waiting on each of them
/// is the whole run. A RECORDING is a gate: every line of it is one
/// somebody chose, the run is a few hundred of them, and a limit that a
/// loaded machine can reach turns the gate into a coin toss.
pub const SWEEP_LIMIT: u64 = 20;
pub const RECORD_LIMIT: u64 = 60;

/// The default the process runs under, which [`set_default_limit`] moves.
static DEFAULT_LIMIT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(SWEEP_LIMIT);

/// Say how patient this run is with an interpreter, in seconds.
/// `LIBJAY_ORACLE_TIMEOUT` still wins where it is set, and 0 waits for ever.
pub fn set_default_limit(secs: u64) {
    DEFAULT_LIMIT.store(secs, Ordering::Relaxed);
}

/// How long one sentence may keep an interpreter busy, in seconds.
/// `LIBJAY_ORACLE_TIMEOUT` overrides it; 0 waits for ever.
fn limit() -> Option<std::time::Duration> {
    let secs: u64 = std::env::var("LIBJAY_ORACLE_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| DEFAULT_LIMIT.load(Ordering::Relaxed));
    (secs > 0).then(|| std::time::Duration::from_secs(secs))
}

/// How much one run may print on one stream before it is cut off, in
/// bytes. An interpreter asked for a large array prints it — the J
/// preamble sets a 4096-column page — and a composed sentence reaches such
/// a request readily, so a recorder that buffers whatever comes back is one
/// sentence away from holding gigabytes. `LIBJAY_ORACLE_CAPTURE` overrides
/// it; 0 lifts the cap. Every answer a corpus holds is orders of magnitude
/// under it — the whole of the largest snapshot is a fifth of a megabyte —
/// so no recording changes because of it.
const CAPTURE_CAP: usize = 4 << 20;

fn capture_cap() -> Option<usize> {
    let bytes: usize = std::env::var("LIBJAY_ORACLE_CAPTURE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(CAPTURE_CAP);
    (bytes > 0).then_some(bytes)
}

/// One pipe being read in a thread of its own, and the flag it raises when
/// it has had to drop bytes.
struct Drain {
    reader: std::thread::JoinHandle<Vec<u8>>,
    over_cap: Arc<AtomicBool>,
}

/// Read one pipe until it ends or the cap is reached. On reaching the cap
/// the thread raises its flag and RETURNS, which drops the read end: the
/// interpreter's next write fails and it dies, instead of printing into a
/// buffer nobody will use.
fn drain<R: Read + Send + 'static>(reader: R, cap: Option<usize>) -> Drain {
    let over_cap = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&over_cap);
    let reader = std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let read = match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let room = cap.map_or(read, |cap| read.min(cap.saturating_sub(buf.len())));
            buf.extend_from_slice(&chunk[..room]);
            if room < read {
                flag.store(true, Ordering::Relaxed);
                break;
            }
        }
        buf
    });
    Drain { reader, over_cap }
}

/// Kill the child and everything it started. The child leads a process
/// group of its own (see `own_group`), and the signal goes to the group:
/// killing the child alone would leave a grandchild holding the pipes open,
/// and the drain threads waiting on them.
pub fn kill_group(child: &mut Child) {
    #[cfg(unix)]
    {
        // Negative pid is the group, by `kill(2)`. The child is its own
        // group leader, so the group is the child and its descendants.
        let group = -(child.id() as i32);
        unsafe { libc::kill(group, libc::SIGKILL) };
    }
    let _ = child.kill();
}

/// Start a child in a process group of its own, so that `kill_group` can
/// reach whatever it starts. jconsole and GNU APL are one process each
/// today, but an oracle reached through a wrapper script is not.
pub fn own_group(command: &mut Command) -> &mut Command {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

/// What a finished child printed, and whether it died abnormally — an
/// abort or a fatal signal rather than an exit of its own.
struct Finished {
    out: String,
    err: String,
    died: bool,
}

/// Wait for a child that has already been written to, draining both pipes
/// in threads of their own so neither can fill and block, and killing it if
/// it outstays the limit or prints past the cap. `Err` names why what came
/// back is not an answer.
///
/// Both drain threads are joined on EVERY path. A thread left running after
/// a kill still holds its buffer, and a recording of thousands of sentences
/// leaks one such buffer per sentence it cut short.
fn wait_within(mut child: Child) -> Result<Finished, Reply> {
    let cap = capture_cap();
    let out = drain(child.stdout.take().expect("piped stdout"), cap);
    let err = drain(child.stderr.take().expect("piped stderr"), cap);
    let deadline = limit().map(|d| std::time::Instant::now() + d);
    let mut cut_short = None;
    let mut died = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                died = abnormal(&status);
                break;
            }
            Err(_) => break,
            Ok(None) => {}
        }
        let over_cap =
            out.over_cap.load(Ordering::Relaxed) || err.over_cap.load(Ordering::Relaxed);
        if over_cap || deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            kill_group(&mut child);
            let _ = child.wait();
            cut_short = Some(if over_cap { Reply::Overflowed } else { Reply::TimedOut });
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let text = |h: std::thread::JoinHandle<Vec<u8>>| {
        String::from_utf8_lossy(&h.join().unwrap_or_default()).into_owned()
    };
    let Drain { reader: out_reader, over_cap: out_over } = out;
    let Drain { reader: err_reader, over_cap: err_over } = err;
    let (text_out, text_err) = (text(out_reader), text(err_reader));
    if let Some(reply) = cut_short {
        return Err(reply);
    }
    // A child that reached the cap and then died of the closed pipe on its
    // own is over the cap too, and the wait loop never saw it happen.
    if out_over.load(Ordering::Relaxed) || err_over.load(Ordering::Relaxed) {
        return Err(Reply::Overflowed);
    }
    Ok(Finished { out: text_out, err: text_err, died })
}

/// Whether a finished child died rather than exited: a fatal signal, or the
/// 128+n exit a wrapper shell reports one as. An interpreter that refuses a
/// sentence exits normally and says so on its streams; one that aborts has
/// no opinion to record.
fn abnormal(status: &std::process::ExitStatus) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal().is_some() {
            return true;
        }
    }
    status.code().is_some_and(|code| code >= 128)
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

    /// Run one sentence, and give a run the limit cut short one more
    /// chance. A recording is a gate, and whether a line of it can be
    /// recorded at all must not depend on what else the machine was doing:
    /// a sentence that genuinely does not finish does not finish twice
    /// either, and one that merely lost a race to a loaded machine answers
    /// on the second ask.
    pub fn eval_patiently(&self, expr: &str, index_origin: u8) -> Reply {
        match self.eval(expr, index_origin) {
            Reply::TimedOut => self.eval(expr, index_origin),
            answered => answered,
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
    let mut command = Command::new(jconsole);
    command
        .args(["-jprofile", "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // jconsole reports a failed sentence on stderr, so discarding it
        // would turn an error into an empty result.
        .stderr(Stdio::piped());
    let mut child = own_group(&mut command).spawn().expect("spawn jconsole");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{J_PREAMBLE}\n{expr}\n").as_bytes())
        .expect("write to jconsole");
    let Finished { out: text, err: complaint, died } = match wait_within(child) {
        Ok(streams) => streams,
        Err(reply) => return reply,
    };
    // A crash is not a refusal. jconsole announces one on stderr and aborts,
    // so the exit says as much as the message does; either alone is enough,
    // since a build with no abort handler dies silently on the signal.
    if died || complaint.contains("JE has crashed") {
        return Reply::Crashed;
    }
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
    command
        .current_dir(cwd)
        .stdin(if multiline { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = own_group(&mut command).spawn().expect("spawn GNU APL");
    if multiline {
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(format!("{line}\n)OFF\n").as_bytes())
            .expect("write to GNU APL");
    }
    let Finished { out: stdout, err: stderr, died } = match wait_within(child) {
        Ok(streams) => streams,
        Err(reply) => return reply,
    };
    if died {
        return Reply::Crashed;
    }
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
