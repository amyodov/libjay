//! End-to-end tests for the seventh coverage wave: gerunds as boxed nouns
//! and the evoke-gerund forms, dyadic transpose in both languages, J's
//! catalogue, raze-in and verb characteristics, APL's under and stencil
//! operators, and the collating grade.
//!
//! Everything an oracle covers is in tests/corpus/{j,apl}/wave7.txt. This
//! file carries what neither reference answers — Dyalog's `⍢` and `⌺`,
//! which GNU APL has not — the round trip through the atomic
//! representation, and the gaps this wave leaves named.

use jay::gerund::Ar;
use jay::{compile, Array, Data, Dialect, ErrorKind, Lang};

fn run(lang: Lang, src: &str) -> Option<Array> {
    let program = compile(lang, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile of {src:?} failed:\n{}", e.render(src)));
    let mut sink = |_: &str| {};
    program
        .run(&[], &mut sink)
        .unwrap_or_else(|e| panic!("run of {src:?} failed:\n{}", program.render_error(&e)))
}

fn val(lang: Lang, src: &str) -> Array {
    run(lang, src).unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn err(lang: Lang, src: &str) -> jay::Error {
    let program = match compile(lang, src, &Dialect::default()) {
        Err(e) => return e,
        Ok(p) => p,
    };
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink).expect_err("expected an error")
}

fn j(src: &str) -> Array {
    val(Lang::J, src)
}

fn apl(src: &str) -> Array {
    val(Lang::Apl, src)
}

fn i64s(shape: &[usize], values: &[i64]) -> Array {
    Array::new(shape.to_vec(), Data::I64(values.to_vec().into()))
}

fn chars(s: &str) -> Array {
    Array::from_chars(s.chars().collect())
}

// --- gerunds --------------------------------------------------------------

#[test]
fn a_gerund_is_boxed_data() {
    // Every property of `` +`- `` is a property of a boxed noun: it has a
    // shape, a tally, a depth and a type code, and its items are the
    // spellings of the verbs.
    assert_eq!(j("$ +`-"), i64s(&[1], &[2]));
    assert_eq!(j("#+`-"), Array::scalar_i64(2));
    assert_eq!(j("L. +`-"), Array::scalar_i64(1));
    assert_eq!(j("3!:0 (+`-)"), Array::scalar_i64(32));
    assert_eq!(j("> 0 { +`-"), chars("+"));
    assert_eq!(j("> 1 { +`-"), chars("-"));
}

#[test]
fn a_gerund_can_be_named_and_extended() {
    // Data, so an ordinary assignment keeps it and an ordinary catenation
    // grows it — and the reader finds it under the name.
    assert_eq!(j("g =. +`-\n$ g`*"), i64s(&[1], &[3]));
    assert_eq!(j("g =. +`-\n2 (g@.0) 3"), Array::scalar_i64(5));
    assert_eq!(j("g =. +`-\n2 (g@.1) 3"), Array::scalar_i64(-1));
    // A gerund written out by hand reads the same way.
    assert_eq!(j("2 (('+';'-')@.0) 3"), Array::scalar_i64(5));
}

#[test]
fn the_atomic_representation_round_trips() {
    // Encoding a verb and reading it back gives a verb that answers the
    // same way, for the derivations the representation names.
    for (verb, arg) in [
        ("+/", "1 2 3"),
        ("+ % #", "1 2 3"),
        ("2&+", "1 2 3"),
        ("+\"1", "1 2 3"),
        ("+&.>", "1 2 3"),
        ("+@-", "1 2 3"),
        ("[: +/ ,", "1 2 3"),
        ("+/\\", "1 2 3"),
        ("+~", "1 2 3"),
        ("< ;. 1", "1 0 1 0"),
    ] {
        // Tying the verb writes its representation out; agenda reads it
        // back. The answer has to be the one the verb gives directly.
        let through = val(Lang::J, &format!("((({verb})`-)@.0) {arg}"));
        let direct = val(Lang::J, &format!("({verb}) {arg}"));
        assert_eq!(through, direct, "{verb} did not round trip");
    }
}

#[test]
fn the_representation_of_a_primitive_is_its_spelling() {
    let plus = Ar::Prim("+".to_string());
    assert_eq!(plus.to_array(), chars("+"));
    assert_eq!(Ar::from_array(&chars("+")), Some(plus));
    // A derived one is the modifier's spelling over its operands.
    let sum = Ar::Derived("/".to_string(), vec![Ar::Prim("+".to_string())]);
    assert_eq!(Ar::from_array(&sum.to_array()), Some(sum.clone()));
    assert_eq!(j("> 0 { +/`-"), sum.to_array());
}

#[test]
fn the_foreign_answers_with_a_representation() {
    // `5!:1 <'name'` is the same data a tie writes out, boxed.
    assert_eq!(j("f =. +\n> 5!:1 <'f'"), chars("+"));
    assert_eq!(j("f =. +\n3!:0 (5!:1 <'f')"), Array::scalar_i64(32));
    // A value stands for itself, so its representation is the noun pair.
    assert_eq!(j("f =. 1 2 3\n> 0 { > 5!:1 <'f'"), chars("0"));
    assert_eq!(j("f =. 1 2 3\n> 1 { > 5!:1 <'f'"), i64s(&[3], &[1, 2, 3]));
    // A name with no meaning yet represents ITSELF, as the reference
    // answers; text that is no name at all represents nothing.
    assert_eq!(j("> 5!:1 <'zz'"), chars("zz"));
    assert_eq!(err(Lang::J, "5!:1 <'i.'").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "5!:1 'f'").kind, ErrorKind::Domain);
}

#[test]
fn evoke_applies_inserts_and_trains() {
    // `` `:0 `` applies every verb and frames the answers.
    assert_eq!(j("((+`-)`:0) 5"), i64s(&[2], &[5, -5]));
    assert_eq!(j("5 ((+`-)`:0) 3"), i64s(&[2], &[8, 2]));
    // `` `:3 `` inserts the verbs between the items, cycling.
    assert_eq!(j("((+`*)`:3) 1 2 3 4"), Array::scalar_i64(15));
    assert_eq!(j("((+`-`*)`:3) 1 2 3 4"), Array::scalar_i64(-9));
    // `` `:6 `` is the train the gerund spells: a hook of two, a fork of
    // three, and longer ones grouped from the right.
    assert_eq!(j("((+`-)`:6) 1 2 3"), i64s(&[3], &[0, 0, 0]));
    assert_eq!(j("((<.`+`>.)`:6) 1 2 3 4"), i64s(&[4], &[2, 4, 6, 8]));
    assert_eq!(j("((+/`%`#)`:6) 1 2 3 4"), Array::scalar_f64(2.5));
}

#[test]
fn the_gerund_gaps_name_themselves() {
    // `` `: `` reads data, not a verb; only three forms exist; and a verb
    // libjay has no J spelling for cannot be tied.
    assert_eq!(err(Lang::J, "(+`:6) 5").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "((+`-)`:1) 5").kind, ErrorKind::Domain);
    let e = err(Lang::J, "2 ((1 2)@.0) 3");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("boxed data"), "{}", e.msg);
}

// --- dyadic transpose -----------------------------------------------------

#[test]
fn dyadic_transpose_moves_axes_in_both_languages() {
    // J names the axes that move to the END; APL names, for each axis of
    // the argument, the axis of the result it becomes.
    assert_eq!(j("1 0 |: i.2 3"), i64s(&[3, 2], &[0, 3, 1, 4, 2, 5]));
    assert_eq!(j("$ 0 |: i.2 3 4"), i64s(&[3], &[3, 4, 2]));
    assert_eq!(apl("2 1⍉2 3⍴⍳6"), i64s(&[3, 2], &[1, 4, 2, 5, 3, 6]));
    assert_eq!(apl("⍴3 1 2⍉2 3 4⍴⍳24"), i64s(&[3], &[3, 4, 2]));
    // A repeated destination runs those axes together: the diagonal.
    assert_eq!(j("(<0 1) |: i.3 3"), i64s(&[3], &[0, 4, 8]));
    assert_eq!(apl("1 1⍉3 3⍴⍳9"), i64s(&[3], &[1, 5, 9]));
    // J refuses an axis named twice, where APL's spelling makes it the
    // diagonal instead.
    assert_eq!(err(Lang::J, "0 0 |: i.3 3").kind, ErrorKind::Domain);
    // Every axis of the result must be named.
    assert_eq!(err(Lang::Apl, "3 1⍉2 3⍴⍳6").kind, ErrorKind::Domain);
}

// --- catalogue, raze-in, characteristics ----------------------------------

#[test]
fn the_catalogue_takes_one_element_from_each_item() {
    assert_eq!(j("$ { (1 2);(3 4)"), i64s(&[2], &[2, 2]));
    assert_eq!(j("> (<0 0) { { (1 2);(3 4)"), i64s(&[2], &[1, 3]));
    assert_eq!(j("> (<1 1) { { (1 2);(3 4)"), i64s(&[2], &[2, 4]));
    assert_eq!(j("> { 3;4"), i64s(&[2], &[3, 4]));
    assert_eq!(j("> (<0 1) { { 'ab';'cd'"), chars("ad"));
}

#[test]
fn raze_in_tests_every_element_against_the_raze() {
    assert_eq!(j("e. 1 2 3"), Array::new(vec![3, 3], Data::Bool(vec![1, 0, 0, 0, 1, 0, 0, 0, 1].into())));
    assert_eq!(
        j("e. (1 2);(2 3)"),
        Array::new(vec![2, 4], Data::Bool(vec![1, 1, 1, 0, 0, 1, 1, 1].into()))
    );
}

#[test]
fn verb_characteristics_answer_with_a_spelling() {
    // `u b. _1` is the obverse and `u b. 1` the identity function; both
    // answer with the verb's spelling, which is a character vector.
    assert_eq!(j("^ b. _1"), chars("^."));
    assert_eq!(j("%: b. _1"), chars("*:"));
    assert_eq!(j("+ b. 1"), chars("0 $~ }.@$"));
    assert_eq!(j("* b. 1"), chars("1 $~ }.@$"));
    assert_eq!(j("<. b. 1"), chars("_ $~ }.@$"));
    // A verb with no known inverse says so rather than guessing.
    let e = err(Lang::J, "* b. _1");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("obverse"), "{}", e.msg);
}

#[test]
fn a_bond_is_undone_from_the_right() {
    // `2&+` is undone by taking 2 off, not by subtracting from 2 — the
    // reference answers 3 for every one of these.
    for src in ["(2&+)^:_1 (5)", "(+&2)^:_1 (5)", "(2&*)^:_1 (6)", "(*&2)^:_1 (6)"] {
        assert_eq!(val(Lang::J, src).to_f64_vec().unwrap(), vec![3.0], "{src}");
    }
}

// --- power over several counts -------------------------------------------

#[test]
fn power_over_a_list_or_a_box_frames_the_answers() {
    assert_eq!(j("(>:^:2 3) 5"), i64s(&[2], &[7, 8]));
    assert_eq!(j("(>:^:(0 1 2)) 5"), i64s(&[3], &[5, 6, 7]));
    // A boxed count traces: `u^:(<n)` is `u^:(i.n)`, and a negative one
    // traces the obverse.
    assert_eq!(j("(>:^:(<3)) 5"), i64s(&[3], &[5, 6, 7]));
    assert_eq!(j("(>:^:(<_3)) 5"), i64s(&[3], &[5, 4, 3]));
    assert_eq!(j("(<.@-:^:a:) 100"), i64s(&[8], &[100, 50, 25, 12, 6, 3, 1, 0]));
}

// --- tessellation with a negative block size ------------------------------

#[test]
fn a_negative_block_size_reverses_its_axis() {
    // The movement has to be written out: `(m ,: s) u;.3 y`.
    assert_eq!(j("> 0 { (1 ,: _2) <;.3 i.5"), i64s(&[2], &[1, 0]));
    assert_eq!(j("> 4 { (1 ,: _2) <;.3 i.5"), i64s(&[1], &[4]));
    assert_eq!(j("$ (2 ,: _2) <;.3 i.5"), i64s(&[1], &[3]));
    // Without one the size does not measure the block at all: it runs to
    // the end of its axis and comes back reversed, so the magnitude plays
    // no part and the frame is the axis's own length.
    assert_eq!(j("> 0 { _2 <;.3 i.5"), i64s(&[5], &[4, 3, 2, 1, 0]));
    assert_eq!(j("> 3 { _2 <;.3 i.5"), i64s(&[2], &[4, 3]));
    assert_eq!(j("$ _2 <;.3 i.5"), i64s(&[1], &[5]));
}

// --- APL's under and stencil ----------------------------------------------
//
// GNU APL has neither glyph, so these follow Dyalog's published rules:
// `f⍢g` is `g⍣¯1 ⊢ (g ⍺) f (g ⍵)`, and `f⌺w` applies f to the window of w
// cells centred on each cell of the argument, the edges filled.

#[test]
fn under_runs_a_function_and_undoes_the_preparation() {
    assert_eq!(apl("+/⍢⍟1 2 3"), Array::scalar_f64(6.0));
    assert_eq!(apl("÷⍢-4"), Array::scalar_f64(0.25));
    assert_eq!(apl("⌽⍢⌽1 2 3"), i64s(&[3], &[3, 2, 1]));
    assert_eq!(apl("1↓⍢⌽1 2 3 4"), i64s(&[3], &[1, 2, 3]));
    // The obverse table is the same one J's `&.` uses, and a verb outside
    // it says so by name.
    let e = err(Lang::Apl, "1+⍢⌊2.5");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("obverse"), "{}", e.msg);
}

#[test]
fn a_stencil_applies_over_centred_windows() {
    // Five windows over five cells, the two at the edges filled with 0.
    assert_eq!(apl("(+/⌺3)1 2 3 4 5"), i64s(&[5], &[3, 6, 9, 12, 9]));
    assert_eq!(apl("(+/⌺1)1 2 3"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(apl("(+/⌺5)1 2 3"), i64s(&[3], &[6, 6, 6]));
    // Two sizes window the two leading axes.
    assert_eq!(apl("⍴(⊂⌺3 3)3 3⍴⍳9"), i64s(&[2], &[3, 3]));
    // One size per leading axis, and no more than the argument has.
    assert_eq!(err(Lang::Apl, "(+/⌺3 3)1 2 3").kind, ErrorKind::Rank);
}

// --- gaps this wave leaves ------------------------------------------------

#[test]
fn the_gaps_this_wave_leaves_name_themselves() {
    // `t.` runs a verb in one of J's thread pools: closed by the sandbox,
    // which is libjay's own policy and not a queue position.
    assert_eq!(err(Lang::J, "(^ t. 3) 1").kind, ErrorKind::Sandbox);
    // `t:` is not a J inflection at all — the reference rejects the
    // spelling, so there is nothing to promise.
    assert_eq!(err(Lang::J, "(^ t: 3) 1").kind, ErrorKind::Language);
    // `⌶` is still queued: what an I-beam does is the implementation's own
    // business, so there is nothing published to follow.
    assert_eq!(err(Lang::Apl, "1(⌶)2").kind, ErrorKind::NotYet);
}

#[test]
fn the_indeterminate_is_a_value() {
    // `_.` is J's indeterminate: a NaN, which prints as itself and is
    // equal to nothing, not even to itself.
    assert_eq!(j("_. = _."), Array::scalar_bool(false));
    assert!(j("_.").to_f64_vec().unwrap()[0].is_nan());
    assert_eq!(j("$ 1 2 , _."), i64s(&[1], &[3]));
}
