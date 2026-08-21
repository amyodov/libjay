//! J's explicit adverbs and conjunctions, end to end.
//!
//! tests/corpus/j/modifiers.txt carries what jconsole answers for the
//! ordinary cases. This file carries the rest: the part-of-speech rule the
//! body's words decide, the two moments a body can run at, the
//! diagnostics, and the forms libjay does not have yet.

use jay::{compile, Array, Dialect, ErrorKind, Lang};
use rstest::rstest;

fn run(src: &str) -> Option<Array> {
    let program = compile(Lang::J, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile failed:\n{}", e.render(src)));
    program
        .run(&[], &mut |_: &str| {})
        .unwrap_or_else(|e| panic!("run failed:\n{}", program.render_error(&e)))
}

fn j(src: &str) -> Vec<i64> {
    let a = run(src).unwrap_or_else(|| panic!("{src:?} yielded no value"));
    a.to_i64_vec().unwrap_or_else(|| panic!("{src:?} is not integral: {a:?}"))
}

fn floats(src: &str) -> Vec<f64> {
    let a = run(src).unwrap_or_else(|| panic!("{src:?} yielded no value"));
    a.to_f64_vec().unwrap_or_else(|| panic!("{src:?} is not numeric: {a:?}"))
}

/// The error a program raises, at compile time or at run time.
fn fails(src: &str) -> jay::Error {
    match compile(Lang::J, src, &Dialect::default()) {
        Err(e) => e,
        Ok(p) => match p.run(&[], &mut |_: &str| {}) {
            Err(e) => e,
            Ok(v) => panic!("expected {src:?} to fail, got {v:?}"),
        },
    }
}

// --- the derived verb -----------------------------------------------------

/// An adverb takes the verb on its left, a conjunction the verbs on both
/// sides, and what comes out is an ordinary verb.
#[rstest]
#[case("twice =. 1 : 'u u y'\n+: twice 3", 12)]
#[case("twice =. 1 : 'u u y'\n*: twice 3", 81)]
#[case("twice =. {{ u u y }}\n+: twice 3", 12)]
#[case("twice =. 1 : 0\nu u y\n)\n+: twice 3", 12)]
#[case("pair =. 2 : 'u v y'\n(+: pair *:) 3", 18)]
#[case("pair =. {{ u v y }}\n(+: pair *:) 3", 18)]
#[case("pair =. 2 : 0\nu v y\n)\n(+: pair *:) 3", 18)]
fn an_explicit_modifier_derives_a_verb(#[case] src: &str, #[case] want: i64) {
    assert_eq!(j(src), vec![want]);
}

/// The derived verb is applied to x and y like any other explicit verb, and
/// its valence follows the same rule: a body that names `x` is a dyad only.
#[rstest]
#[case("addy =. 1 : 'x u y'\n3 (+ addy) 4", 7)]
#[case("both =. 2 : 'x u v y'\n3 (+ both *:) 4", 19)]
#[case("both =. {{ x u v y }}\n3 (+ both *:) 4", 19)]
fn a_derived_verb_takes_a_left_argument(#[case] src: &str, #[case] want: i64) {
    assert_eq!(j(src), vec![want]);
}

#[test]
fn a_derived_verb_has_only_the_valence_its_body_names() {
    let e = fails("addy =. 1 : 'x u y'\n(+ addy) 4");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("no monadic definition"), "{}", e.msg);
    let e = fails("twice =. 1 : 'u u y'\n3 (+: twice) 4");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("no dyadic definition"), "{}", e.msg);
}

/// An operand arrives under `u` and `v` when it is a verb and under `m` and
/// `n` when it is a noun. Reaching for the other name is an undefined name,
/// which is what the reference reports too.
#[rstest]
#[case("addm =. 1 : 'm + y'\n5 addm 3", 8)]
#[case("joinn =. 2 : 'm , y , n'\n(2 joinn 3) 9", 2)]
#[case("nm =. {{ m + y }}\n5 nm 3", 8)]
fn a_noun_operand_arrives_as_m_or_n(#[case] src: &str, #[case] first: i64) {
    assert_eq!(j(src)[0], first);
}

#[rstest]
#[case("f =. 1 : 'm + y'\n+: f 3")]
#[case("f =. 1 : 'u y'\n5 f 3")]
fn an_operand_named_against_its_part_of_speech_is_undefined(#[case] src: &str) {
    let e = fails(src);
    assert!(
        matches!(e.kind, ErrorKind::Value | ErrorKind::Parse),
        "{:?}: {}",
        e.kind,
        e.msg
    );
}

// --- the two moments a body runs at ---------------------------------------

/// A body that names neither `x` nor `y` runs when the modifier is applied
/// to its operands, and what it makes is what the modifier produced: a
/// tacit verb, or a noun.
#[rstest]
#[case("sq =. 1 : 'u @ u'\n+: sq 3", 12)]
#[case("sq =. 1 : 'u @ u'\nd =. +: sq\nd 5", 20)]
#[case("comp =. 2 : 'u @: v'\n(+: comp *:) 3", 18)]
#[case("lit =. 1 : '3 + 4'\n+: lit", 7)]
fn a_body_naming_no_argument_runs_at_derivation(#[case] src: &str, #[case] want: i64) {
    assert_eq!(j(src), vec![want]);
}

/// The derivation-time phase produces a verb, so the sentence that names it
/// is a verb definition and the one that applies it is ordinary work.
#[test]
fn a_derivation_time_body_yields_a_tacit_verb() {
    let src = "sq =. 1 : 'u @ u'\nd =. +: sq\nd 5";
    let p = compile(Lang::J, src, &Dialect::default()).expect("compile");
    let text = p.explain(Some(&[]));
    assert!(text.contains("verb definition d"), "{text}");
}

/// The same body written with `y` runs later instead — when the derived
/// verb is applied — and the answers agree.
#[test]
fn the_two_phases_agree_where_both_are_writable() {
    assert_eq!(j("f =. 1 : 'u @ u'\n+: f 5"), j("f =. 1 : 'u u y'\n+: f 5"));
}

// --- the part of speech a `{{ }}` body declares ---------------------------

/// J reads the part of speech off the operand names the body uses: `v` or
/// `n` makes a conjunction, `u` or `m` an adverb, neither a verb.
#[rstest]
#[case("f =. {{ y + 1 }}\nf 4", 5)]
#[case("f =. {{ u u y }}\n+: f 3", 12)]
#[case("f =. {{ m + y }}\n5 f 3", 8)]
#[case("f =. {{ u v y }}\n(+: f *:) 3", 18)]
#[case("f =. {{ v y }}\n(+: f *:) 3", 9)]
#[case("f =. {{ n + y }}\n(+: f 1) 3", 4)]
fn the_body_words_decide_the_part_of_speech(#[case] src: &str, #[case] want: i64) {
    assert_eq!(j(src), vec![want]);
}

/// `{{)a` and its relatives state it outright instead.
#[rstest]
#[case("f =. {{)a\nu u y\n}}\n+: f 3", 12)]
#[case("f =. {{)c\nu v y\n}}\n(+: f *:) 3", 18)]
#[case("f =. {{)v\ny + 1\n}}\nf 4", 5)]
#[case("f =. {{)d\nx + y\n}}\n3 f 4", 7)]
#[case("f =. {{)m\n- y\n}}\nf 4", -4)]
fn a_marker_states_the_part_of_speech(#[case] src: &str, #[case] want: i64) {
    assert_eq!(j(src), vec![want]);
}

/// The reference takes a marker only where nothing follows it on the line.
#[test]
fn a_marker_has_to_end_its_line() {
    let e = fails("f =. {{)a u y }}\n+: f 3");
    assert_eq!(e.kind, ErrorKind::Parse);
    assert!(e.msg.contains("last thing on its line"), "{}", e.msg);
}

// --- composition ----------------------------------------------------------

/// A named modifier composes with everything a primitive one does.
#[rstest]
#[case("tw =. 1 : 'u u y'\n(>: @ (+: tw)) 3", 13)]
#[case("tw =. 1 : 'u u y'\nh =. +: tw\n(h , h) 3", 12)]
#[case("tw =. 1 : 'u u y'\nq =. 1 : '(u tw) tw'\n+: q 1", 16)]
#[case("tw =. 1 : 'u u y'\ng =. 2 : 'u tw v y'\n(+: g *:) 3", 36)]
#[case("tw =. 1 : 'u u y'\n(+: tw) tw 1", 16)]
#[case("q =. 10\nf =. 1 : 'q + u y'\n+: f 3", 16)]
fn a_named_modifier_composes(#[case] src: &str, #[case] first: i64) {
    assert_eq!(j(src)[0], first);
}

#[test]
fn a_derived_verb_takes_a_rank() {
    assert_eq!(j("tw =. 1 : 'u u y'\n(+: tw)\"0 i. 3"), vec![0, 4, 8]);
    assert_eq!(j("tw =. 1 : 'u u y'\n(+:\"0 tw) i. 3"), vec![0, 4, 8]);
}

#[test]
fn a_derived_verb_is_a_tine_of_a_train() {
    let src = "ins =. 1 : 'u/ y'\nmean =. (+ ins) % #\nmean 1 2 3 4";
    assert_eq!(floats(src), vec![2.5]);
}

/// The body of a deferred modifier is an explicit definition like any
/// other, so it holds control structures and several sentences.
#[rstest]
#[case("ml =. 1 : 0\nz =. u y\nz + z\n)\n+: ml 3", 12)]
#[case("ml =. 2 : 0\nif. x > 0 do. u y else. v y end.\n)\n1 (+: ml *:) 5", 10)]
#[case("ml =. 2 : 0\nif. x > 0 do. u y else. v y end.\n)\n0 (+: ml *:) 5", 25)]
fn a_modifier_body_is_an_explicit_definition(#[case] src: &str, #[case] want: i64) {
    assert_eq!(j(src), vec![want]);
}

// --- what is named but not implemented ------------------------------------

#[test]
fn a_modifier_that_derives_itself_is_a_named_gap() {
    let e = fails("f =. 1 : 'if. y<1 do. 1 else. y * u f y-1 end.'\n] f 4");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("derives the modifier itself"), "{}", e.msg);
}

#[test]
fn a_tacit_definition_is_a_named_gap() {
    let e = fails("f =. 13 : 'y + 1'\nf 3");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("tacit definitions"), "{}", e.msg);
}

/// A modifier is still a modifier, and a sentence that is one has no value
/// to display yet.
#[test]
fn a_sentence_that_is_an_explicit_modifier_is_a_named_gap() {
    let e = fails("f =. 1 : 'u u y'\nf");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("displaying a modifier"), "{}", e.msg);
}

/// `explain` names what the sentence defined.
#[test]
fn explain_names_an_explicit_modifier() {
    let src = "twice =. 1 : 'u u y'\n+: twice 3";
    let p = compile(Lang::J, src, &Dialect::default()).expect("compile");
    let text = p.explain(Some(&[]));
    assert!(text.contains("adverb definition twice = 1 : '...'"), "{text}");
}
