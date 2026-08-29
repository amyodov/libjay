//! Wave 11: APL's system names — the atomic vector, print precision, the
//! random link, name class, a definition's own text, the numbered
//! conversions, and the polynomial half of the `⌹` group.
//!
//! The corpus in tests/corpus/apl/sysvars.txt carries the breadth against
//! the oracle. This file states what the corpus cannot: that setting a
//! system name changes what the host displays, that the random link makes
//! a stream repeat, that one run's link does not reach the next, and how
//! the names libjay does not answer report themselves.

use jay::fmt::format_array;
use jay::{Array, Data, Dialect, Error, ErrorKind, Lang, compile};

fn run(src: &str) -> Result<Option<Array>, Error> {
    let program = compile(Lang::Apl, src, &Dialect::default())?;
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink)
}

fn apl(src: &str) -> Array {
    run(src)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

/// The answer as a session shows it: the run's own display conventions,
/// which is what a `⎕PP` in the program moves.
fn shown(src: &str) -> String {
    let program = compile(Lang::Apl, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("{src:?} did not compile: {}", e.msg));
    let mut sink = |_: &str| {};
    let outcome = program
        .run_detail(&[], &mut sink)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg));
    let value = outcome.value.unwrap_or_else(|| panic!("{src:?} yielded no value"));
    format_array(&value, &outcome.fmt)
}

fn err(src: &str) -> Error {
    match run(src) {
        Err(e) => e,
        Ok(v) => panic!("{src:?} was expected to fail; it answered {v:?}"),
    }
}

fn i64s(shape: &[usize], values: &[i64]) -> Array {
    Array::new(shape.to_vec(), Data::I64(values.to_vec().into()))
}

fn chars(shape: &[usize], text: &str) -> Array {
    Array::new(shape.to_vec(), Data::Char(text.chars().collect::<Vec<char>>().into()))
}

// --- ⎕AV -----------------------------------------------------------------

/// The atomic vector is 256 characters, its first 128 the code points 0 to
/// 127 in order, and indexing it is the inverse of finding a character in
/// it.
#[test]
fn the_atomic_vector_is_two_hundred_and_fifty_six_characters() {
    assert_eq!(apl("⍴⎕AV"), Array::from_i64(vec![256]));
    assert_eq!(apl("∧/(⎕AV⍳⎕AV)=⍳256"), apl("1=1"));
    assert_eq!(apl("⎕AV⍳'A'"), Array::scalar_i64(66));
    assert_eq!(apl("⎕UCS ⎕AV[1+⍳127]"), apl("⍳127"));
}

// --- ⎕PP -----------------------------------------------------------------

/// Print precision is read and set, and what is set is what the value is
/// displayed with — including the value the run answers with, which the
/// host formats after the program has finished.
#[test]
fn print_precision_moves_the_display() {
    assert_eq!(apl("⎕PP"), Array::scalar_i64(6));
    assert_eq!(apl("⎕PP←3 ⋄ ⎕PP"), Array::scalar_i64(3));
    assert_eq!(shown("÷3"), "0.333333");
    assert_eq!(shown("⎕PP←3 ⋄ ÷3"), "0.333");
    assert_eq!(shown("⎕PP←1 ⋄ ÷3"), "0.3");
    assert_eq!(shown("⎕PP←17 ⋄ ÷3"), "0.33333333333333331");
    // One program's setting is its own: the next run starts where every
    // run starts.
    assert_eq!(shown("÷3"), "0.333333");
}

/// A precision below one says nothing at all, and a value that is not one
/// whole number is not a precision.
#[test]
fn print_precision_refuses_what_is_not_a_precision() {
    assert_eq!(err("⎕PP←0").kind, ErrorKind::Domain);
    assert_eq!(err("⎕PP←¯1").kind, ErrorKind::Domain);
    assert_eq!(err("⎕PP←1.5").kind, ErrorKind::Domain);
    assert_eq!(err("⎕PP←'a'").kind, ErrorKind::Domain);
    assert_eq!(err("⎕PP←1 2").kind, ErrorKind::Length);
    assert_eq!(err("⎕PP←⍬").kind, ErrorKind::Length);
}

// --- ⎕RL -----------------------------------------------------------------

/// The random link reads back the seed it was set from, and the stream it
/// starts repeats: the same seed rolls the same numbers. The SEQUENCE is
/// libjay's own and no reference's.
#[test]
fn the_random_link_makes_a_stream_repeat() {
    assert_eq!(apl("⎕RL"), Array::scalar_i64(16807));
    assert_eq!(apl("⎕RL←42 ⋄ ⎕RL"), Array::scalar_i64(42));
    let once = apl("⎕RL←42 ⋄ ?10⍴1000");
    let again = apl("⎕RL←42 ⋄ ?10⍴1000");
    assert_eq!(once, again, "the same link starts the same stream");
    let other = apl("⎕RL←43 ⋄ ?10⍴1000");
    assert_ne!(once, other, "a different link starts a different stream");
}

/// A run's link belongs to that run. Without one, `?` draws from the
/// process's own stream, which does not repeat.
#[test]
fn a_link_does_not_reach_the_next_run() {
    let seeded = apl("⎕RL←7 ⋄ ?20⍴1000000");
    let after = apl("?20⍴1000000");
    let and_after = apl("?20⍴1000000");
    assert_ne!(seeded, after);
    assert_ne!(after, and_after);
}

#[test]
fn the_random_link_refuses_what_is_not_a_seed() {
    assert_eq!(err("⎕RL←1.5").kind, ErrorKind::Domain);
    assert_eq!(err("⎕RL←'a'").kind, ErrorKind::Domain);
    assert_eq!(err("⎕RL←1 2").kind, ErrorKind::Length);
}

// --- ⎕NC -----------------------------------------------------------------

/// A name's class is what it holds now: nothing, a value, a definition, or
/// a system name — and `¯1` for what is not a name at all.
#[test]
fn name_class_says_what_a_name_holds() {
    assert_eq!(apl("⎕NC 'A'"), Array::scalar_i64(0));
    assert_eq!(apl("A←5 ⋄ ⎕NC 'A'"), Array::scalar_i64(2));
    assert_eq!(apl("∇Z←F X\nZ←X+1\n∇\n⎕NC 'F'"), Array::scalar_i64(3));
    assert_eq!(apl("⎕NC '⎕IO'"), Array::scalar_i64(5));
    assert_eq!(apl("⎕NC '⎕FX'"), Array::scalar_i64(-1));
    assert_eq!(apl("⎕NC '1A'"), Array::scalar_i64(-1));
    assert_eq!(apl("⎕NC ''"), Array::scalar_i64(-1));
    // A matrix is one name per row, and a row's trailing blanks are not
    // part of the name it holds.
    assert_eq!(apl("A←5 ⋄ ⎕NC 2 1⍴'A '"), Array::from_i64(vec![2, -1]));
}

// --- ⎕CR -----------------------------------------------------------------

/// A definition hands back the lines it was written as, padded with blanks
/// to the longest of them. A name that is no definition has no text.
#[test]
fn char_rep_gives_a_definition_its_lines_back() {
    assert_eq!(apl("∇Z←F X\n Z←X+1\n∇\n⎕CR 'F'"), chars(&[2, 6], "Z←F X  Z←X+1"));
    assert_eq!(
        apl("Q←⎕FX 'Z←F X' 'Z←X+1' ⋄ ⎕CR 'F'"),
        chars(&[2, 5], "Z←F XZ←X+1")
    );
    assert_eq!(apl("⍴⎕CR 'A'"), Array::from_i64(vec![0, 0]));
    assert_eq!(apl("A←5 ⋄ ⍴⎕CR 'A'"), Array::from_i64(vec![0, 0]));
    assert_eq!(err("⎕CR 5").kind, ErrorKind::Domain);
    assert_eq!(err("⎕CR 2 2⍴'ABCD'").kind, ErrorKind::Rank);
}

/// A `{…}` is an expression rather than a listing of lines, so `⎕CR` of
/// one names the gap instead of inventing a rendering for it.
#[test]
fn char_rep_of_a_lambda_names_the_gap() {
    let e = err("F←{⍵+1} ⋄ ⎕CR 'F'");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("⎕CR"), "{}", e.msg);
}

/// The numbered conversions rewrite the same bytes another way, and every
/// pair of them undoes the other.
#[test]
fn the_numbered_conversions_round_trip() {
    assert_eq!(apl("5 ⎕CR 255 16"), chars(&[4], "FF10"));
    assert_eq!(apl("6 ⎕CR 255 16"), chars(&[4], "ff10"));
    assert_eq!(apl("5 ⎕CR 13 ⎕CR 'FF01'"), chars(&[4], "FF01"));
    assert_eq!(apl("16 ⎕CR 'ABC'"), chars(&[4], "QUJD"));
    assert_eq!(apl("17 ⎕CR 16 ⎕CR 'ABC'"), chars(&[3], "ABC"));
    assert_eq!(apl("18 ⎕CR 'é'"), Array::from_i64(vec![195, 169]));
    assert_eq!(apl("19 ⎕CR 18 ⎕CR 'héllo'"), chars(&[5], "héllo"));
    // The numbers that report on an interpreter's own display and storage
    // are named, not guessed at.
    let e = err("0 ⎕CR 2 2⍴⍳4");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert_eq!(err("99 ⎕CR 1").kind, ErrorKind::Domain);
}

// --- ⌹[8] and ⌹[9] -------------------------------------------------------

/// The bracket after `⌹` picks a function of the group, and its number is
/// the number written — not an axis, so `⎕IO` does not move it.
#[test]
fn the_polynomial_functions_are_picked_by_number_not_axis() {
    assert_eq!(apl("1 2 ⌹[8] 1 1"), Array::from_i64(vec![1, 3, 2]));
    let zero = compile(
        Lang::Apl,
        "1 2 ⌹[8] 1 1",
        &Dialect { index_origin: Some(0), ..Dialect::default() },
    )
    .expect("⌹[8] compiles under either origin");
    let mut sink = |_: &str| {};
    assert_eq!(
        zero.run(&[], &mut sink).expect("it runs"),
        Some(Array::from_i64(vec![1, 3, 2]))
    );
}

/// Division answers the quotient and the remainder, with the shapes long
/// division gives them.
#[test]
fn polynomial_division_answers_a_quotient_and_a_remainder() {
    let answer = apl("1 0 1 ⌹[9] 1 1");
    let Data::Box(cells) = &answer.data else { panic!("⌹[9] answers two items") };
    assert_eq!(answer.shape, vec![2]);
    assert_eq!(cells[0], Array::from_i64(vec![-1, 1]));
    assert_eq!(cells[1], Array::from_i64(vec![2]));
    assert_eq!(err("1 2 ⌹[8] ⍬").kind, ErrorKind::Length);
    assert_eq!(err("'ab' ⌹[8] 1 1").kind, ErrorKind::Domain);
}

/// The other members of the group are named rather than guessed at.
#[test]
fn the_rest_of_the_matrix_divide_group_is_named() {
    for src in ["⌹[1]3 2⍴1 2 3 4 5 6", "⌹[7]1 2 3"] {
        let e = err(src);
        assert_eq!(e.kind, ErrorKind::NotYet, "{src}");
        assert!(e.msg.contains('⌹'), "{src}: {}", e.msg);
    }
    assert_eq!(err("⌹[10]1 2").kind, ErrorKind::Domain);
}

// --- the names libjay does not answer ------------------------------------

/// Each of the three says which kind of thing it is: a page libjay's
/// display has no notion of, a table describing another interpreter, and
/// the shared-variable surface the sandbox closes.
#[test]
fn the_system_names_libjay_declines_say_why() {
    let pw = err("⎕PW");
    assert_eq!(pw.kind, ErrorKind::NotYet);
    assert!(pw.msg.contains("⎕PW"), "{}", pw.msg);

    let syl = err("⎕SYL");
    assert_eq!(syl.kind, ErrorKind::Language);
    assert!(syl.msg.contains("⎕SYL"), "{}", syl.msg);

    let svr = err("⎕SVR 'A'");
    assert_eq!(svr.kind, ErrorKind::Sandbox);
    assert!(svr.msg.contains("⎕SVR"), "{}", svr.msg);
}

/// A system name a program may not set says so, and says it before
/// anything is stored.
#[test]
fn the_read_only_system_names_refuse_an_assignment() {
    for src in ["⎕AV←'x'", "⎕LX←'2+2'", "⎕ET←1 2", "⎕EM←'x'", "⎕IO←0"] {
        let e = err(src);
        assert_eq!(e.kind, ErrorKind::Language, "{src}");
        assert!(e.msg.contains("read-only"), "{src}: {}", e.msg);
    }
}

/// The last error's type and message are the values that mean "no error
/// yet", for every program libjay can run: nothing it has catches an error
/// and carries on, so an error always ends the program instead.
#[test]
fn the_last_error_is_always_the_one_that_has_not_happened() {
    assert_eq!(apl("⎕ET"), Array::from_i64(vec![0, 0]));
    assert_eq!(apl("⍴⎕EM"), Array::from_i64(vec![3, 0]));
    assert_eq!(apl("⍴⎕LX"), i64s(&[1], &[0]));
}
