//! Symbols: J's `s:`, a storage kind whose elements are interned names.
//!
//! The corpus in tests/corpus/j/symbols.txt carries the breadth — every
//! spelling there is checked against jconsole. This file states the rules a
//! single expression cannot show: that two symbols made in two separately
//! compiled programs are the same symbol, that the intern table is opaque
//! (an index says nothing about order), what the storage costs, and where
//! symbols stop — arithmetic, fusion, Arrow and the C ABI.

use jay::{compile, Array, Data, DType, Dialect, Error, ErrorKind, Lang};

fn run(src: &str) -> Result<Option<Array>, Error> {
    let program = compile(Lang::J, src, &Dialect::default())?;
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink)
}

fn j(src: &str) -> Array {
    run(src)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn err(src: &str) -> Error {
    match run(src) {
        Err(e) => e,
        Ok(v) => panic!("{src:?} was expected to fail; it answered {v:?}"),
    }
}

fn ids(a: &Array) -> Vec<u32> {
    match &a.data {
        Data::Symbol(v) => v.as_slice().to_vec(),
        other => panic!("expected symbols, got {other:?}"),
    }
}

fn show(src: &str) -> String {
    jay::fmt::format_array(&j(src), &jay::fmt::FmtOpts::J)
}

// ------------------------------------------------------- the storage kind

#[test]
fn a_symbol_array_holds_one_index_per_element() {
    let a = j("s: ;: 'alpha beta gamma'");
    assert_eq!(a.dtype(), DType::Symbol);
    assert_eq!(a.shape, vec![3]);
    assert_eq!(ids(&a).len(), 3);
}

#[test]
fn the_same_name_is_the_same_symbol_in_two_separate_programs() {
    // Two compilations, two runs, one table: this is what makes `=` on
    // symbols a comparison of indices rather than of text.
    let first = ids(&j("s: <'libjay-two-programs'"));
    let second = ids(&j("s: <'libjay-two-programs'"));
    assert_eq!(first, second);
    assert_eq!(j("(s: <'libjay-two-programs') = (s: <'libjay-two-programs')").data,
               Data::Bool(vec![1].into()));
}

#[test]
fn distinct_names_take_distinct_indices() {
    let a = ids(&j("s: '`libjay distinct one`libjay distinct two'"));
    assert_ne!(a[0], a[1]);
}

#[test]
fn the_index_says_nothing_about_the_order() {
    // "zzz" is interned before "aaa" and still sorts after it: the order is
    // the NAME's, which is why every comparison resolves the table.
    let interned = ids(&j("s: '`libjay order zzz`libjay order aaa'"));
    assert!(interned[0] < interned[1], "the later name took the later index");
    assert_eq!(
        show("/:~ s: '`libjay order zzz`libjay order aaa'"),
        "`libjay order aaa `libjay order zzz"
    );
}

#[test]
fn the_empty_name_is_the_fill_element() {
    assert_eq!(ids(&j("4 {. s: ;: 'a b'"))[2..], [jay::symbol::EMPTY; 2]);
    assert_eq!(show("4 {. s: ;: 'a b'"), "`a `b ` `");
}

#[test]
fn interning_a_name_already_in_the_table_adds_nothing() {
    let first = jay::symbol::intern("libjay-interned-once");
    let before = jay::symbol::interned();
    for _ in 0..8 {
        assert_eq!(jay::symbol::intern("libjay-interned-once"), first);
    }
    // Other tests share the table and may add to it; what must not happen
    // is a second slot for a name it already holds.
    assert!(jay::symbol::interned() >= before);
    assert_eq!(&*jay::symbol::name(first), "libjay-interned-once");
}

// ------------------------------------------------------------- the display

#[test]
fn a_table_of_symbols_pads_its_columns_on_the_right() {
    // Numbers align right; names align left, which is what J prints.
    assert_eq!(show("2 2 $ s: ;: 'a bbbb cc d'"), "`a  `bbbb\n`cc `d   ");
}

#[test]
fn an_atom_and_a_list_print_as_backticked_names() {
    assert_eq!(show("s: <'q'"), "`q");
    assert_eq!(show("s: ;: 'a bb'"), "`a `bb");
    assert_eq!(show("s: <''"), "`");
}

// -------------------------------------------------------------- the limits

#[test]
fn arithmetic_on_symbols_is_refused_by_name() {
    let e = err("(s: <'a') + (s: <'b')");
    assert_eq!(e.kind, ErrorKind::Type);
    assert!(e.msg.contains("symbol"), "{}", e.msg);
    // The refusal names the way out.
    assert!(e.msg.contains("5 s:"), "{}", e.msg);
}

#[test]
fn a_symbol_never_mixes_with_another_type() {
    for src in ["(s: ;:'a b') , 'a'", "(s: ;:'a b') , 1", "(s: ;:'a b') , a:"] {
        assert_eq!(err(src).kind, ErrorKind::Type, "{src}");
    }
    // Equality across the boundary is total, though: nothing else IS a
    // symbol, so the answer is 0 rather than a complaint.
    assert_eq!(j("(s: <'a') = 'a'").data, Data::Bool(vec![0].into()));
    assert_eq!(j("(s: <'a') = 1").data, Data::Bool(vec![0].into()));
}

#[test]
fn ordering_a_symbol_against_a_non_symbol_is_refused() {
    let e = err("(s: <'a') < 'a'");
    assert_eq!(e.kind, ErrorKind::Type);
    assert!(e.msg.contains("symbol"), "{}", e.msg);
}

#[test]
fn the_symbol_table_forms_are_a_named_gap_not_a_syntax_error() {
    // The forms that report an interpreter's OWN table: its numbering of a
    // symbol is a fact about the table, not about the language.
    for form in ["0", "1", "6", "7", "_1"] {
        let e = err(&format!("{form} s: s: <'a'"));
        assert_eq!(e.kind, ErrorKind::NotYet, "{form} s:");
        assert!(e.msg.contains("symbol-table form"), "{}", e.msg);
    }
    // A form that names nothing at all is a domain error, not a gap.
    assert_eq!(err("8 s: s: <'a'").kind, ErrorKind::Domain);
}

#[test]
fn s_colon_reads_characters_and_boxes_only() {
    for src in ["s: 1 2 3", "s: <1", "s: <<'a'", "s: s: <'a'"] {
        assert_eq!(err(src).kind, ErrorKind::Domain, "{src}");
    }
    assert_eq!(err("s: <2 2 $ 'abcd'").kind, ErrorKind::Rank);
}

// ------------------------------------------------- the boundaries decline

#[test]
fn a_symbol_chain_is_never_fused() {
    // Nothing about a symbol is arithmetic, so the kernel must hand the
    // whole chain back rather than compute in the wrong type.
    let program = compile(Lang::J, "s: ;: 'a b c'", &Dialect::default()).expect("compiles");
    assert!(!jay::fuse::is_fused(&program), "a symbol chain was fused");
}

#[test]
fn the_apl_frontend_has_no_spelling_for_a_symbol() {
    // APL has no symbol; `s:` is not one of its words, so it never parses.
    assert!(compile(Lang::Apl, "s: 'a b'", &Dialect::default()).is_err());
}
