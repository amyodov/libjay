//! Running a reference interpreter as a black-box subprocess.
//!
//! The interpreters are never linked and never read: one process per
//! sentence, in, out, and what came back is the recording. This is the only
//! place in the repository that spawns them.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use libjay_testkit::Lang;

/// Where the interpreter is, and where its scratch files may go.
pub struct Oracle {
    lang: Lang,
    path: PathBuf,
    scratch: PathBuf,
}

impl Oracle {
    /// Locate the interpreter for a language. `LIBJAY_ORACLE_J` and
    /// `LIBJAY_ORACLE_APL` override the default install path; a missing
    /// interpreter is an error, never a skip.
    pub fn find(lang: Lang) -> Result<Oracle, String> {
        let (var, default) = match lang {
            Lang::J => ("LIBJAY_ORACLE_J", "projects/libjay-oracles/j/j64/jconsole"),
            Lang::Apl => ("LIBJAY_ORACLE_APL", "projects/libjay-oracles/gnu-apl/install/bin/apl"),
        };
        let path = std::env::var(var).map(PathBuf::from).unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(default)
        });
        if !path.exists() {
            return Err(format!(
                "recording needs the {} oracle; {} is not there (set {var})",
                libjay_testkit::reference_name(lang),
                path.display()
            ));
        }
        let scratch = std::env::temp_dir().join(format!("libjay-corpus-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
        Ok(Oracle { lang, path, scratch })
    }

    /// Run one sentence. `None` is a refusal.
    pub fn eval(&self, expr: &str, index_origin: u8) -> Option<String> {
        match self.lang {
            Lang::J => eval_j(&self.path, expr),
            Lang::Apl => eval_apl(&self.path, &self.thread_dir(), expr, index_origin),
        }
    }

    /// A working directory of this thread's own: GNU APL drops a history
    /// file into the directory it runs in, and parallel recordings must not
    /// share one. It is never the repository.
    fn thread_dir(&self) -> PathBuf {
        let dir = self.scratch.join(format!("t{}", rayon::current_thread_index().unwrap_or(0)));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}

fn eval_j(jconsole: &Path, expr: &str) -> Option<String> {
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

/// `--script` silences the banner and the input echo, `--safe` and `--noSV`
/// keep the interpreter from opening sockets or loading a workspace, and a
/// wide `⎕PW` stops long vectors from wrapping onto continuation lines.
fn eval_apl(apl: &Path, cwd: &Path, expr: &str, index_origin: u8) -> Option<String> {
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
