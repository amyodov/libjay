//! Non-standard extensions: what each flag changes, and that nothing
//! changes without one.
//!
//! Flagged behaviour departs from what the reference interpreters answer,
//! so none of it may be recorded against the oracle corpus — it is pinned
//! here instead, by hand, beside the spec-correct answer it replaces.
//! docs/extensions.md is the prose version of this file.

use jay::extensions::Extensions;
use jay::{compile, Array, Dialect, Lang};

/// The shipped J: no extension at all.
fn plain() -> Dialect {
    Dialect { extensions: Some(Extensions::NONE), ..Dialect::j() }
}

/// J with one flag on.
fn with(flags: Extensions) -> Dialect {
    Dialect { extensions: Some(flags), ..Dialect::j() }
}

fn value(src: &str, dialect: &Dialect) -> Array {
    let program = compile(Lang::J, src, dialect)
        .unwrap_or_else(|e| panic!("{src:?} did not compile: {}", e.msg));
    let mut sink = |_: &str| {};
    program
        .run(&[], &mut sink)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn shown(src: &str, dialect: &Dialect) -> String {
    let program = compile(Lang::J, src, dialect)
        .unwrap_or_else(|e| panic!("{src:?} did not compile: {}", e.msg));
    let mut sink = |_: &str| {};
    let value = program
        .run(&[], &mut sink)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"));
    jay::fmt::format_array(&value, &program.fmt).trim_end().to_string()
}

fn count(src: &str, dialect: &Dialect) -> i64 {
    value(src, dialect).to_i64_vec().expect("a count")[0]
}

// ------------------------------------------------- j_unicode_strings

/// Every one of these is the jconsole answer, which is what libjay gives
/// without the flag. The flagged column is the other reading.
#[test]
fn a_literal_is_bytes_unless_the_flag_says_characters() {
    let bytes = plain();
    let chars = with(Extensions::J_UNICODE_STRINGS);
    for (src, spec, flagged) in [
        ("# 'é'", 2, 1),
        ("# 'héllo'", 6, 5),
        ("# 'αβγ'", 6, 3),
        ("# '日本'", 6, 2),
        ("# 'naïve café'", 12, 10),
        ("$ 'héllo'", 6, 5),
        ("# 'abc' , 'é'", 5, 4),
        ("# ~. 'αβγαβγ'", 4, 3),
        ("# ''", 0, 0),
        ("# 'abc'", 3, 3),
    ] {
        assert_eq!(count(src, &bytes), spec, "{src} without the flag");
        assert_eq!(count(src, &chars), flagged, "{src} with j_unicode_strings");
    }
}

#[test]
fn the_codes_are_the_bytes_or_the_codepoints() {
    let bytes = plain();
    let chars = with(Extensions::J_UNICODE_STRINGS);
    for (src, spec, flagged) in [
        ("a. i. 'é'", vec![195, 169], vec![233]),
        ("3 u: 'é'", vec![195, 169], vec![233]),
        ("a. i. '日'", vec![230, 151, 165], vec![256]),
        ("3 u: 'αβγ'", vec![206, 177, 206, 178, 206, 179], vec![945, 946, 947]),
    ] {
        assert_eq!(value(src, &bytes).to_i64_vec().unwrap(), spec, "{src} without the flag");
        assert_eq!(
            value(src, &chars).to_i64_vec().unwrap(),
            flagged,
            "{src} with j_unicode_strings"
        );
    }
    // `a.` is the 256 bytes either way, so a character outside it is
    // simply not found — which is what the flagged `a. i. '日'` reports.
    assert_eq!(count("# a.", &bytes), 256);
    assert_eq!(count("# a.", &chars), 256);
}

#[test]
fn indexing_takes_a_byte_or_a_character() {
    let bytes = plain();
    let chars = with(Extensions::J_UNICODE_STRINGS);
    assert_eq!(shown("1 { 'héllo'", &chars), "é");
    // Byte 195 alone is not a character, and a session shows it as one
    // that could not be read — which is what jconsole writes too.
    assert_eq!(shown("1 { 'héllo'", &bytes), "\u{fffd}");
    assert_eq!(shown("0 { 'héllo'", &bytes), "h");
    assert_eq!(shown("2 {. 'héllo'", &bytes), "h\u{fffd}");
    assert_eq!(shown("3 {. 'héllo'", &bytes), "hé");
    assert_eq!(shown("2 {. 'héllo'", &chars), "hé");
}

#[test]
fn the_display_writes_bytes_and_the_flag_writes_characters() {
    let bytes = plain();
    let chars = with(Extensions::J_UNICODE_STRINGS);
    for src in ["'héllo'", "'αβγ'", "< 'héllo'", "'ab' ; 'héllo'"] {
        assert_eq!(shown(src, &bytes), shown(src, &chars), "{src} shows the same text");
    }
    // A reshape counts items, so the rows are cut in different places.
    assert_eq!(shown("2 3 $ 'héllo!'", &bytes), "hé\nllo");
    assert_eq!(shown("2 3 $ 'héllo!'", &chars), "hél\nlo!");
    // A box is fenced to what the text occupies, not to what it weighs.
    assert_eq!(shown("< 'héllo'", &bytes), "+-----+\n|héllo|\n+-----+");
}

#[test]
fn formatting_a_literal_keeps_the_items_it_had() {
    let bytes = plain();
    let chars = with(Extensions::J_UNICODE_STRINGS);
    assert_eq!(count("# \": 'héllo'", &bytes), 6);
    assert_eq!(count("# \": 'héllo'", &chars), 5);
    // The lines a fenced box takes are as wide as the widest of them, in
    // whatever a character is.
    assert_eq!(value("$ \": < 'é'", &bytes).to_i64_vec().unwrap(), vec![3, 4]);
    assert_eq!(value("$ \": < 'é'", &chars).to_i64_vec().unwrap(), vec![3, 3]);
}

/// A definition written as a literal is source text again: the body is read
/// back as the characters it spells, under either reading.
#[test]
fn a_quoted_definition_body_reads_the_same_either_way() {
    for dialect in [plain(), with(Extensions::J_UNICODE_STRINGS)] {
        assert_eq!(count("f =. 3 : '# y'\nf 'abc'", &dialect), 3);
    }
    assert_eq!(count("f =. 3 : '# ''é'''\nf 0", &plain()), 2);
    assert_eq!(count("f =. 3 : '# ''é'''\nf 0", &with(Extensions::J_UNICODE_STRINGS)), 1);
}

/// `".` compiles at run time, and it compiles under the same extensions as
/// the program that reached it.
#[test]
fn a_nested_compilation_inherits_the_extensions() {
    assert_eq!(count("\". '# ''é'''", &plain()), 2);
    assert_eq!(count("\". '# ''é'''", &with(Extensions::J_UNICODE_STRINGS)), 1);
}

// ------------------------------------------------------- the mechanism

#[test]
fn apl_is_untouched_by_the_j_flag() {
    for flags in [Extensions::NONE, Extensions::J_UNICODE_STRINGS] {
        let dialect = Dialect { extensions: Some(flags), ..Dialect::default() };
        let program = compile(Lang::Apl, "≢'héllo'", &dialect).expect("compiles");
        let mut sink = |_: &str| {};
        let value = program.run(&[], &mut sink).expect("runs").expect("a value");
        assert_eq!(value.to_i64_vec().unwrap(), vec![5]);
    }
}

/// The set a program compiles under is on its rules, so a host can read
/// back what it asked for.
#[test]
fn the_rules_carry_the_set_that_was_resolved() {
    let program = compile(Lang::J, "1", &with(Extensions::J_UNICODE_STRINGS)).unwrap();
    assert!(program.rules.extensions.has(Extensions::J_UNICODE_STRINGS));
    let program = compile(Lang::J, "1", &plain()).unwrap();
    assert!(!program.rules.extensions.has(Extensions::J_UNICODE_STRINGS));
}

/// Flags combine with `|`, and asking for none is the shipped language.
#[test]
fn flags_combine() {
    let all = Extensions::NONE | Extensions::J_UNICODE_STRINGS;
    assert!(all.has(Extensions::J_UNICODE_STRINGS));
    assert_eq!(Extensions::parse("j_unicode_strings").unwrap(), all);
    assert_eq!(Extensions::selected(all), vec!["j_unicode_strings"]);
}
