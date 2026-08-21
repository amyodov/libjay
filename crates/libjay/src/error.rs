//! Error type carrying a position in the user's source expression.

use std::fmt;

/// Byte range into the display source of a compiled program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    pub fn merge(a: Span, b: Span) -> Span {
        Span { start: a.start.min(b.start), end: a.end.max(b.end) }
    }
}

/// Broad class of a failure. `NotYet` and `Language` are deliberately
/// distinct: the former is a promise, the latter is a property of J/APL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Parse,
    Rank,
    Length,
    Shape,
    Domain,
    Type,
    Value,
    /// Present in the language, not implemented yet.
    NotYet,
    /// Absent from the language itself; will never exist.
    Language,
    /// Present in the language and closed by libjay's sandbox: the host
    /// policy, not a property of J or APL, and not a queue position.
    Sandbox,
    /// Larger than libjay will allocate.
    Limit,
    Internal,
}

impl ErrorKind {
    pub fn label(self) -> &'static str {
        match self {
            ErrorKind::Parse => "parse error",
            ErrorKind::Rank => "rank error",
            ErrorKind::Length => "length error",
            ErrorKind::Shape => "shape error",
            ErrorKind::Domain => "domain error",
            ErrorKind::Type => "type error",
            ErrorKind::Value => "value error",
            ErrorKind::NotYet => "not supported yet",
            ErrorKind::Language => "not in the language",
            ErrorKind::Sandbox => "closed by the sandbox",
            ErrorKind::Limit => "limit error",
            ErrorKind::Internal => "internal error",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub msg: String,
    pub span: Option<Span>,
    pub notes: Vec<String>,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(kind: ErrorKind, msg: impl Into<String>, span: Option<Span>) -> Self {
        Error { kind, msg: msg.into(), span, notes: Vec::new() }
    }

    pub fn parse(msg: impl Into<String>, span: Span) -> Self {
        Self::new(ErrorKind::Parse, msg, Some(span))
    }

    pub fn not_yet(what: impl fmt::Display, span: Span) -> Self {
        Self::new(ErrorKind::NotYet, format!("{what} is not supported yet"), Some(span))
    }

    pub fn language(msg: impl Into<String>, span: Span) -> Self {
        Self::new(ErrorKind::Language, msg, Some(span))
    }

    /// A feature the language has and libjay's sandbox does not open. The
    /// message says what the feature would reach; the kind's label says who
    /// closed it.
    pub fn sandbox(msg: impl Into<String>, span: Span) -> Self {
        Self::new(ErrorKind::Sandbox, msg, Some(span))
    }

    pub fn domain(msg: impl Into<String>, span: Span) -> Self {
        Self::new(ErrorKind::Domain, msg, Some(span))
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, msg, None)
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Render with a caret line pointing into `src` (the display source).
    pub fn render(&self, src: &str) -> String {
        let mut out = format!("{}: {}", self.kind.label(), self.msg);
        if let Some(span) = self.span && let Some((line, col_start, col_len)) = locate(src, span) {
            out.push_str("\n  ");
            out.push_str(line);
            out.push_str("\n  ");
            out.push_str(&" ".repeat(col_start));
            out.push_str(&"^".repeat(col_len.max(1)));
        }
        for n in &self.notes {
            out.push_str("\nnote: ");
            out.push_str(n);
        }
        out
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.label(), self.msg)?;
        for n in &self.notes {
            write!(f, "\nnote: {n}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

/// Find the source line containing `span` and the span's position in it,
/// measured in characters (for caret alignment).
fn locate(src: &str, span: Span) -> Option<(&str, usize, usize)> {
    // A span that does not land on this source has no caret to draw; the
    // message still stands on its own.
    if span.start > src.len() || !src.is_char_boundary(span.start) {
        return None;
    }
    let line_start = src[..span.start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = src[span.start..].find('\n').map(|i| span.start + i).unwrap_or(src.len());
    let line = &src[line_start..line_end];
    let col_start = src[line_start..span.start].chars().count();
    let span_end = span.end.min(line_end).max(span.start);
    let col_len = if src.is_char_boundary(span_end) {
        src[span.start..span_end].chars().count()
    } else {
        1
    };
    Some((line, col_start, col_len))
}
