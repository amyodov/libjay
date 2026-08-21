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

/// The reference interpreter a language is recorded against.
pub fn reference_name(lang: Lang) -> &'static str {
    match lang {
        Lang::J => "J (jconsole)",
        Lang::Apl => "APL (GNU APL)",
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
