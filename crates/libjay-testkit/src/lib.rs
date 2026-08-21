//! What the differential tests and the corpus recorder both need.
//!
//! The two activities are separate: *collecting* runs a reference
//! interpreter over a corpus file and records what it said (the
//! `jay-corpus` binary in `libjay-devtools`), *testing* replays those
//! recordings against libjay (`cargo test`). This crate holds everything
//! they share — the corpus format, the snapshot format, the
//! tolerance-aware comparison, and the libjay side of an evaluation — so
//! neither has a private copy of it.
//!
//! Both consumers use part of this, so unused items here are expected.
#![allow(dead_code)]

pub mod compare;
pub mod corpus;
pub mod eval;
pub mod replay;
pub mod snapshot;

pub use jay::{Error, ErrorKind, Lang};

/// How a language names itself in a path (`corpus/j`, `snapshots/apl`).
pub fn lang_dir(lang: Lang) -> &'static str {
    match lang {
        Lang::J => "j",
        Lang::Apl => "apl",
    }
}

/// The key jconsole's answers are recorded under.
pub const IMPL_J: &str = "j";
/// The key GNU APL's answers are recorded under.
pub const IMPL_GNU: &str = "gnu";
/// The key Dyalog APL's answers are recorded under.
pub const IMPL_DYALOG: &str = "dyalog";

/// The implementations a language's snapshots may hold answers from, in the
/// order a snapshot file lists them: the one libjay follows first.
///
/// The key namespace is open — a snapshot may carry a key this list does
/// not name, and reading and rewriting it keeps that key — so adding an
/// implementation is adding a recorder, not a format change.
pub fn impls(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::J => &[IMPL_J],
        Lang::Apl => &[IMPL_GNU, IMPL_DYALOG],
    }
}

/// The implementation the dialect libjay ships FOLLOWS, and so the one the
/// replay holds it to: jconsole for J, GNU APL for the APL2/ISO line that
/// `Dialect::default()` implements. A Dyalog dialect would switch this key
/// and read the same files.
pub fn followed_impl(lang: Lang) -> &'static str {
    match lang {
        Lang::J => IMPL_J,
        Lang::Apl => IMPL_GNU,
    }
}

/// The implementation whose recorded answers are BACKLOG rather than a
/// gate: a difference from it is what a future dialect would have to
/// explain, not a regression in this one.
pub fn backlog_impl(lang: Lang) -> Option<&'static str> {
    match lang {
        Lang::J => None,
        Lang::Apl => Some(IMPL_DYALOG),
    }
}

/// How an implementation key reads in a message.
pub fn impl_name(key: &str) -> &str {
    match key {
        IMPL_J => "J (jconsole)",
        IMPL_GNU => "GNU APL",
        IMPL_DYALOG => "Dyalog APL",
        other => other,
    }
}

/// A key is a lowercase word: it is read up to the first `:` of a snapshot
/// line, so it may hold nothing that a name would not.
pub fn is_impl_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// How a language names itself in a title.
pub fn lang_name(lang: Lang) -> &'static str {
    match lang {
        Lang::J => "J",
        Lang::Apl => "APL",
    }
}

/// The language a corpus or snapshot path belongs to.
pub fn lang_of_path(path: &std::path::Path) -> Option<Lang> {
    match path.parent()?.file_name()?.to_str()? {
        "j" => Some(Lang::J),
        "apl" => Some(Lang::Apl),
        _ => None,
    }
}
