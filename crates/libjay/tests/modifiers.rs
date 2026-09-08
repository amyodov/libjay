//! J's explicit adverbs and conjunctions, end to end.
//!
//! tests/corpus/j/modifiers.txt carries what jconsole answers for the
//! ordinary cases. This file carries the rest: the part-of-speech rule the
//! body's words decide, the two moments a body can run at, the
//! diagnostics, and the forms libjay does not have yet.

use jay::{compile, Array, Data, Dialect, ErrorKind, Lang};
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

/// The text a sentence that reduces to a verb or a modifier displays.
fn text(src: &str) -> String {
    let a = run(src).unwrap_or_else(|| panic!("{src:?} yielded no value"));
    match a.row_major_data() {
        Data::Char(v) => v.as_slice().iter().collect(),
        other => panic!("{src:?} is not text: {other:?}"),
    }
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

/// A body that names an argument belongs to the DERIVED verb, and the
/// reference parses it only where that verb is applied — so a recursion
/// written there terminates for it and not here, where the whole body is
/// parsed at once.
#[test]
fn a_deferred_body_that_derives_itself_is_a_named_gap() {
    let e = fails("f =. 1 : 'if. y<1 do. 1 else. y * u f y-1 end.'\n] f 4");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("derives the modifier itself"), "{}", e.msg);
}

/// A body that names no argument runs where the modifier is derived, and
/// there a recursion over the operands does stop.
#[rstest]
#[case("pw =. 2 : 'if. n = 0 do. ] else. u @ (u pw (n-1)) end.'\n(+: pw 3) 1", 8)]
#[case("pw =. 2 : 'if. n = 0 do. ] else. u @ (u pw (n-1)) end.'\n(+: pw 0) 5", 5)]
fn a_derivation_that_derives_itself_stops_at_its_base_case(
    #[case] src: &str,
    #[case] want: i64,
) {
    assert_eq!(j(src), vec![want]);
}

#[test]
fn a_derivation_with_no_base_case_is_stopped_with_a_diagnostic() {
    let e = fails("bad =. 1 : 'u bad'\n(+ bad) 1");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("deep"), "{}", e.msg);
}

#[rstest]
#[case("f =. 13 : 'y + 1'\nf 3", 4)]
#[case("f =. 13 : 'x + y'\n2 f 3", 5)]
#[case("f =. 13 : '(+/ y) % # y'\nf 2 4 6", 4)]
fn a_tacit_definition_applies_like_any_verb(#[case] src: &str, #[case] want: i64) {
    assert_eq!(j(src), vec![want]);
}

/// A sentence that is a modifier displays it, and an explicit one gives
/// back the text it was written as.
#[test]
fn a_sentence_that_is_an_explicit_modifier_displays_it() {
    assert_eq!(text("f =. 1 : 'u u y'\nf"), "1 : 'u u y'");
}

/// `explain` names what the sentence defined.
#[test]
fn explain_names_an_explicit_modifier() {
    let src = "twice =. 1 : 'u u y'\n+: twice 3";
    let p = compile(Lang::J, src, &Dialect::default()).expect("compile");
    let text = p.explain(Some(&[]));
    assert!(text.contains("adverb definition twice = 1 : '...'"), "{text}");
}

// ------------------------------------------------------- the fold's stop

/// `n Z: v` inside a fold's stepping verb. Each control is a rule of its
/// own, and the four of them are what the reference was measured doing:
/// `1` keeps the step's result and ends the fold, `0` keeps the result as
/// the running value but leaves it out, `_1` throws the step away and
/// carries on from the value before it, `_2` throws it away and ends the
/// fold.
#[rstest]
#[case("(] F:. +) (1 2 3 4)", vec![3, 6, 10])]
#[case("(] F:. (+ [ (1 Z: (3 = [)))) (1 2 3 4)", vec![3, 6])]
#[case("(] F:. (+ [ (0 Z: (3 = [)))) (1 2 3 4)", vec![3, 10])]
#[case("(] F:. (+ [ (_1 Z: (3 = [)))) (1 2 3 4)", vec![3, 7])]
#[case("(] F:. (+ [ (_2 Z: (3 = [)))) (1 2 3 4)", vec![3])]
#[case("(] F.. (+ [ (1 Z: (3 = [)))) (1 2 3 4)", vec![6])]
#[case("(] F.. (+ [ (_1 Z: (3 = [)))) (1 2 3 4)", vec![7])]
#[case("(] F:: (+ [ (_1 Z: (3 = [)))) (1 2 3 4)", vec![6, 7])]
#[case("5 (] F:. (+ [ (1 Z: (3 = [)))) (1 2 3 4)", vec![6, 8, 11])]
#[case("(] F:. (+ [ (0 Z: 0:))) (1 2 3 4)", vec![3, 6, 10])]
fn a_fold_stop_carries_its_control(#[case] src: &str, #[case] want: Vec<i64>) {
    assert_eq!(j(src), want);
}

/// The stop's own refusals: a control outside `_2 _1 0 1`, a test that is
/// not one boolean atom, a fold left with nothing to answer, and a stop
/// with no fold around it.
#[rstest]
#[case("(] F:. (+ [ (2 Z: 0:))) (1 2 3 4)")]
#[case("(] F:. (+ [ (_3 Z: 0:))) (1 2 3 4)")]
#[case("(] F:. (+ [ (1 Z: 2:))) (1 2 3 4)")]
#[case("(] F:. (+ [ (1 Z: (0.5\"_)))) (1 2 3 4)")]
#[case("(] F:. (+ [ (1 Z: ((0 0)\"_)))) (1 2 3 4)")]
#[case("(] F.: (+ [ (0 Z: 1:))) (1 2 3 4)")]
#[case("(0 Z: 0:) 1")]
fn a_fold_stop_refuses_what_it_cannot_mean(#[case] src: &str) {
    assert_eq!(fails(src).kind, ErrorKind::Domain);
}

// ------------------------------- the outfix, the stitch and the divide

/// An outfix leaves a run of items OUT, so an infinite width names no run:
/// the reference refuses `_ u\. y` and `__ u\. y` whatever the argument,
/// where the INFIX takes both.
#[rstest]
#[case("_ +\\. (1 2 3)")]
#[case("__ +\\. (1 2 3)")]
#[case("_ <\\. (1 2 3)")]
#[case("_ +\\. (i. 0)")]
#[case("_ */\\. (1 2 3)")]
#[case("_ <./\\. (1 2 3)")]
#[case("__ -/\\. (1 2 3)")]
fn an_infinite_outfix_width_is_refused(#[case] src: &str) {
    assert_eq!(fails(src).kind, ErrorKind::Length);
}

/// The sum-insert is the reference's one exception, and it is its own
/// special code rather than a rule its neighbours share.
#[test]
fn the_sum_insert_keeps_an_infinite_outfix_width() {
    assert_eq!(j("$ (_ +/\\. (1 2 3))"), vec![0]);
    assert_eq!(j("__ +/\\. (1 2 3)"), vec![0]);
}

/// The infix keeps both infinities: `_ u\ y` is the one window of the
/// whole and `__ u\ y` the argument itself.
#[rstest]
#[case("$ (_ +\\ (1 2 3))", vec![0, 4])]
#[case("__ +\\ (1 2 3)", vec![1, 2, 3])]
fn an_infinite_infix_width_is_not(#[case] src: &str, #[case] want: Vec<i64>) {
    assert_eq!(j(src), want);
}

/// Catenation has an identity element and the STITCH has none.
#[test]
fn the_stitch_has_no_identity_element() {
    assert_eq!(j("$ (,/ (i. 0 3))"), vec![0]);
    assert_eq!(fails(",./ (i. 0)").kind, ErrorKind::Domain);
    assert_eq!(fails(",./ (i. 0 3)").kind, ErrorKind::Domain);
}

/// A one-unknown system over REAL data is the division wherever the
/// division has a value, and ZERO where it has none: a least-squares solve
/// reports the absence of a constraint as no constraint rather than as a
/// refusal.
#[rstest]
#[case("_ %. _", 0.0)]
#[case("__ %. __", 0.0)]
#[case("_ %. __", 0.0)]
#[case("0 %. 0", 0.0)]
#[case("2 %. 0", f64::INFINITY)]
#[case("_ %. 0", f64::INFINITY)]
#[case("2 %. 4", 0.5)]
#[case("(1 2) %. 4", 0.75)]
fn a_one_unknown_system_is_zero_where_the_division_has_no_value(
    #[case] src: &str,
    #[case] want: f64,
) {
    assert_eq!(floats(src), vec![want]);
}

/// And it is the division itself, NaN and all, wherever the division does
/// have a value.
#[rstest]
#[case("_. %. _")]
#[case("_. %. __")]
#[case("0 %. _.")]
#[case("_. %. _.")]
fn a_one_unknown_system_keeps_the_nan_the_division_makes(#[case] src: &str) {
    assert!(floats(src)[0].is_nan());
}

/// COMPLEX data on either side goes through the reciprocal instead, and a
/// left argument with no items — or a system with no rows — is a zero
/// whatever the other side is made of.
#[rstest]
#[case("0j_ %. 0j_", 0.0)]
#[case("(i. 0) %. (_.)", 0.0)]
#[case("(_j_) %. (i. 0)", 0.0)]
fn a_complex_or_empty_one_unknown_system(#[case] src: &str, #[case] want: f64) {
    assert_eq!(floats(src)[0], want);
}

/// `_j_ %. 0x` is `_j_` there, which the division refuses: the reciprocal
/// carries the complex infinity through where the division has no value.
#[test]
fn a_complex_one_unknown_system_goes_through_the_reciprocal() {
    assert_eq!(text("\": (_j_ %. 0x)"), "_j_");
}

/// A system whose coefficients hold a NaN has no pivot to find, and the
/// answer is a NaN rather than a refusal. A COLUMN system answers the
/// complex NaN and a system with no NaN in it keeps the plain one.
#[test]
fn a_system_that_holds_a_nan_answers_one() {
    assert!(floats("(2 2 $ _.) %. (2 2 $ _.)").iter().all(|v| v.is_nan()));
    assert!(floats("((_.) , (_.)) %. ((2) , (2))")[0].is_nan());
    let column = run("((1) , (1)) %. ((_.) , (2))").expect("a value");
    assert_eq!(column.dtype(), jay::DType::Complex);
}
