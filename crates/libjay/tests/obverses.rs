//! The obverse table: what undoes each verb.
//!
//! `u&.v`, `u&.:v`, `u^:_1`, `u b. _1`, APL's `⍢` and `f⍣¯1` all ask one
//! question — what undoes v — and one table answers it. The corpora in
//! tests/corpus/j/obverses.txt and the `⍣¯1` block of
//! tests/corpus/apl/dyalog-operators.txt carry the breadth against the
//! references; this file states the rules on data, one table of cases per
//! rule, and pins the refusals that stay refusals.

use jay::{compile, Array, Data, Dialect, Error, Lang};

fn run(lang: Lang, src: &str) -> Result<Option<Array>, Error> {
    let program = compile(lang, src, &Dialect::default())?;
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink)
}

fn val(lang: Lang, src: &str) -> Array {
    run(lang, src)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn j(src: &str) -> Array {
    val(Lang::J, src)
}

fn i64s(shape: &[usize], values: &[i64]) -> Array {
    Array::new(shape.to_vec(), Data::I64(values.to_vec().into()))
}

fn chars(shape: &[usize], text: &str) -> Array {
    Array::new(shape.to_vec(), Data::Char(text.chars().collect::<Vec<_>>().into()))
}

/// Every case is a sentence and the integer vector it answers, which is how
/// most of the table can be checked without a tolerance.
fn expect_ints(lang: Lang, cases: &[(&str, &[i64])]) {
    for &(src, want) in cases {
        let got = val(lang, src);
        let ints = got
            .to_i64_vec()
            .unwrap_or_else(|| panic!("{src:?} answered {got:?}, not whole numbers"));
        assert_eq!(ints, want.to_vec(), "{src:?}");
    }
}

/// The cases whose answer is a float: equal to a part in a billion is
/// enough, since every one of them is a round trip through a transcendental
/// function.
fn expect_near(lang: Lang, cases: &[(&str, &[f64])]) {
    for &(src, want) in cases {
        let got = val(lang, src);
        let vals = got.to_f64_vec().unwrap_or_else(|| panic!("{src:?} answered {got:?}"));
        assert_eq!(vals.len(), want.len(), "{src:?} answered {vals:?}");
        for (&a, &b) in vals.iter().zip(want) {
            assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{src:?}: {a} is not {b}");
        }
    }
}

/// The sentences the table does not reach: each must name the gap rather
/// than answer, and the name must say whose obverse is missing.
fn expect_gap(lang: Lang, cases: &[&str]) {
    for &src in cases {
        let msg = match run(lang, src) {
            Err(e) => e.msg,
            Ok(v) => panic!("{src:?} was expected to name a gap; it answered {v:?}"),
        };
        assert!(msg.contains("obverse"), "{src:?} complained {msg:?}");
        assert!(msg.contains("not supported yet"), "{src:?} complained {msg:?}");
    }
}

// --- the verbs that undo themselves --------------------------------------

/// Negation, reciprocal, complement, reversal, transposition, grading, the
/// two permutation forms, the matrix inverse, the roots of a polynomial and
/// the identity verbs all return where they came from, so the table answers
/// each of them with itself.
#[test]
fn the_self_inverses_answer_with_themselves() {
    expect_ints(
        Lang::J,
        &[
            ("-^:_1 ] 1 2 3", &[-1, -2, -3]),
            ("-.^:_1 ] 0 1", &[1, 0]),
            ("|.^:_1 ] 1 2 3", &[3, 2, 1]),
            ("]^:_1 ] 1 2 3", &[1, 2, 3]),
            ("[^:_1 ] 1 2 3", &[1, 2, 3]),
            ("/:^:_1 ] 2 0 1", &[1, 2, 0]),
            ("/:^:_1 /: 2 0 1", &[2, 0, 1]),
            ("C.^:_1 ] <2 1 0", &[2, 0, 1]),
            ("%.^:_1 %. 2 2 $ 1 0 0 1", &[1, 0, 0, 1]),
            (">:&.- 3", &[2]),
            ("*:&.|. 1 2 3", &[1, 4, 9]),
        ],
    );
    // Transposition and grading down, whose answers are not one vector.
    assert_eq!(j("|:^:_1 ] 2 3 $ i. 6"), i64s(&[3, 2], &[0, 3, 1, 4, 2, 5]));
    expect_ints(Lang::J, &[("\\:^:_1 ] 2 0 1", &[1, 0, 2])]);
}

// --- the pairs -----------------------------------------------------------

/// Each of these verbs is undone by another verb of the language, and the
/// pairing runs both ways.
#[test]
fn the_pairs_undo_one_another() {
    expect_ints(
        Lang::J,
        &[
            ("*:^:_1 ] 9", &[3]),
            ("%:^:_1 ] 3", &[9]),
            ("+:^:_1 ] 6", &[3]),
            (">:^:_1 ] 4", &[3]),
            ("<:^:_1 ] 4", &[5]),
            ("#.^:_1 ] 5", &[1, 0, 1]),
            ("#:^:_1 ] 1 0 1", &[5]),
            (",:^:_1 ] 1 2 3", &[1]),
            ("$ {.^:_1 ] 1 2 3", &[1, 3]),
            ("\":^:_1 ] '12'", &[12]),
        ],
    );
    expect_near(
        Lang::J,
        &[
            ("-:^:_1 ] 1.5", &[3.0]),
            ("^^:_1 ^ 2", &[2.0]),
            // `+:&.*:` doubles the square and takes the root back.
            ("+:&.*: 3", &[4.242640687119285]),
        ],
    );
    assert_eq!(j("\".^:_1 ] 12"), chars(&[2], "12"));
    assert_eq!(j("{.^:_1 ] 1 2 3"), i64s(&[1, 3], &[1, 2, 3]));
}

/// `x # y` has an obverse where `# y` has none: the expansion puts the items
/// back where the ones stood and a fill where each zero was, while a count
/// says nothing about what was counted.
#[test]
fn copy_inverts_and_tally_does_not() {
    expect_ints(Lang::J, &[("(1 0 1&#)^:_1 ] 1 3", &[1, 0, 3])]);
    assert_eq!(j("(1 0 1&#)^:_1 ] 'ab'"), chars(&[3], "a b"));
    // The reference will not NAME this one — `# b. _1` is a domain error
    // there — while answering `#^:_1` itself. libjay names it, and the
    // diagnostic for the monad says what is missing rather than guessing.
    assert_eq!(j("# b. _1"), chars(&[5], "#^:_1"));
    let refused = run(Lang::J, "#^:_1 ] 3").expect_err("a count cannot be undone");
    assert!(refused.msg.contains("no monadic meaning"), "{}", refused.msg);
}

// --- the two readings of a complex number --------------------------------

/// `+. y` and `*. y` split a complex number into a pair of reals; the pair
/// folds back together under the verb that made it.
#[test]
fn the_complex_readings_fold_back_together() {
    assert_eq!(j("+.^:_1 ] 3 4"), j("3j4"));
    assert_eq!(j("+.^:_1 ] 2 2 $ 1 0 2 1"), j("1 2j1"));
    assert_eq!(j("-&.+. 3j4"), j("_3j_4"));
    expect_near(Lang::J, &[("*.^:_1 &. +. ] 5 0", &[5.0, 0.0])]);
}

// --- the constants -------------------------------------------------------

/// Multiplying by pi is undone by multiplying by its reciprocal, and the
/// circle functions are numbered so that the negative index is the inverse.
#[test]
fn the_constants_and_the_circle_functions_invert_by_their_number() {
    expect_near(
        Lang::J,
        &[
            ("o.^:_1 o. 1", &[1.0]),
            ("o.^:_1 ] 0", &[0.0]),
            ("(2&o.)^:_1 ] 2&o. 1", &[1.0]),
            ("(1&o.)^:_1 ] 1&o. 1", &[1.0]),
            ("(0&o.)^:_1 ] 0.5", &[0.8660254037844386]),
            (">:&.o. 1", &[1.3183098861837907]),
        ],
    );
    expect_ints(
        Lang::J,
        &[("(2&^)^:_1 ] 8", &[3]), ("(^&2)^:_1 ] 9", &[3]), ("(%:&2)^:_1 ] 2", &[1])],
    );
    expect_near(Lang::J, &[("(2&%:)^:_1 ] 3", &[9.0]), ("(2&^.)^:_1 ] 3", &[8.0])]);
}

// --- the bonds -----------------------------------------------------------

/// A noun bonded to a verb inverts by the rule of that verb: the arithmetic
/// ones take the noun off the other side, the rotations turn the other way,
/// the drops take back what they dropped, and the appends drop what they
/// appended.
#[test]
fn a_bonded_noun_inverts_by_its_verb() {
    expect_ints(
        Lang::J,
        &[
            ("(2&+)^:_1 ] 5", &[3]),
            ("(+&2)^:_1 ] 5", &[3]),
            ("(2&-)^:_1 ] 1", &[1]),
            ("(-~&3)^:_1 ] 1", &[2]),
            ("(2&|.)^:_1 ] 3 1 2", &[1, 2, 3]),
            ("(2&}.)^:_1 ] 3 4", &[0, 0, 3, 4]),
            ("(_2&}.)^:_1 ] 1 2 3", &[1, 2, 3, 0, 0]),
            ("(,&1)^:_1 ] 1 2 1", &[1, 2]),
            ("(1&,)^:_1 ] 1 2 1", &[2, 1]),
            ("(,&(1 2))^:_1 ] 1 2 3 1 2", &[1, 2, 3]),
            ("((1 2)&,)^:_1 ] 1 2 3 1 2", &[3, 1, 2]),
            ("(2&A.)^:_1 ] 1 0 2", &[0, 1, 2]),
            ("(2&C.)^:_1 ] 1 0 2", &[1, 0, 2]),
        ],
    );
    expect_near(Lang::J, &[("(2&*)^:_1 ] 6", &[3.0]), ("(2&%)^:_1 ] 4", &[0.5])]);
    assert_eq!(j("(2 3&|.)^:_1 ] 2 3 $ i. 6"), i64s(&[2, 3], &[0, 1, 2, 3, 4, 5]));
    assert_eq!(j("(2&|.)^:_1 ] 'abcde'"), chars(&[5], "deabc"));
}

/// `n #. y` reads a list of digits in base n, and undoing it writes the
/// digits back — in as many places as the largest value asks for, which is
/// what makes the round trip land on the same width the reference chooses.
#[test]
fn the_base_conversions_choose_the_width_from_the_value() {
    expect_ints(
        Lang::J,
        &[
            ("(2&#.)^:_1 ] 5", &[1, 0, 1]),
            ("(2&#.)^:_1 ] 0", &[0]),
            ("(2&#.)^:_1 ] _1", &[1]),
            ("(3&#.)^:_1 ] 26", &[2, 2, 2]),
            ("(16&#.)^:_1 ] 255", &[15, 15]),
            ("(2&#:)^:_1 ] 1 0 1", &[5]),
            // `u&.v` runs v, then u, then the obverse of v — so the answer
            // comes back as the DIGITS the obverse writes.
            ("(1&+)&.(2&#.) 5", &[1, 1, 0]),
        ],
    );
    assert_eq!(j("(2&#.)^:_1 ] 3 5"), i64s(&[2, 3], &[0, 1, 1, 1, 0, 1]));
    // An empty argument gets the one place the width rule floors at.
    assert_eq!(j("$ (2&#.)^:_1 ] i. 0"), i64s(&[2], &[0, 1]));
}

// --- under each ----------------------------------------------------------

/// Boxing is its own inverse, so `u&.>` inverts by turning round only the
/// verb inside the box. That one rule is what the sweep found missing most
/// often.
#[test]
fn each_inverts_the_verb_inside_the_box() {
    expect_ints(
        Lang::J,
        &[
            ("> (*:&.>)^:_1 ] <9", &[3]),
            ("> ((2&*)&.>)^:_1 ] <6", &[3]),
            ("> (+&1&.>)^:_1 ] <5", &[4]),
            ("; (*:&.>)^:_1 ] 4;9", &[2, 3]),
            ("> (1&+)&.> <3", &[4]),
            ("> (+/\\&.>)^:_1 ] <1 3 6", &[1, 2, 3]),
            // The boxing itself comes back untouched.
            ("L. (*:&.>)^:_1 ] <9", &[1]),
            ("# (*:&.>)^:_1 ] 4;9", &[2]),
        ],
    );
}

// --- the running folds ---------------------------------------------------

/// A running sum inverts into the differences between neighbours and a
/// running product into the quotients; the subtracting and dividing folds
/// alternate, so their answers carry one further pass over the signs.
#[test]
fn the_running_folds_invert_into_the_steps_between_neighbours() {
    expect_ints(
        Lang::J,
        &[
            ("(+/\\)^:_1 ] 1 3 6", &[1, 2, 3]),
            ("(+/\\)^:_1 ] i. 0", &[]),
            ("(+/\\.)^:_1 ] 6 5 3", &[1, 2, 3]),
            ("(-/\\)^:_1 ] 1 1 2", &[1, 0, 1]),
            ("(-/\\)^:_1 ] 1 2 3 4", &[1, -1, 1, -1]),
            ("(-/\\.)^:_1 ] 1 2 3 4", &[3, 5, 7, 4]),
            ("(*/)^:_1 ] 12", &[2, 2, 3]),
        ],
    );
    expect_near(
        Lang::J,
        &[
            ("(*/\\)^:_1 ] 1 2 6", &[1.0, 2.0, 3.0]),
            ("(*/\\.)^:_1 ] 6 6 3", &[1.0, 2.0, 3.0]),
            ("(%/\\)^:_1 ] 1 2 3 4", &[1.0, 0.5, 1.5, 0.75]),
            ("(%/\\.)^:_1 ] 1 2 3 4", &[2.0, 6.0, 12.0, 4.0]),
            ("(1&+)&.(+/\\) 1 2 3", &[2.0, 2.0, 3.0]),
        ],
    );
    // The fold runs over ITEMS, so a table inverts row by row.
    expect_ints(
        Lang::J,
        &[
            ("$ (+/\\)^:_1 ] i. 2 3", &[2, 3]),
            (", (+/\\)^:_1 ] i. 2 3", &[0, 1, 2, 3, 3, 3]),
            (", (+/\\.)^:_1 ] i. 2 3", &[-3, -3, -3, 3, 4, 5]),
        ],
    );
}

// --- under ravel ---------------------------------------------------------

/// `,` has no obverse: a ravel says nothing about the shape it came from.
/// Under a fixed argument it has one all the same, so `u&., y` runs u over
/// the ravel and puts y's own shape back — and it has that one valence
/// only, as the reference does.
#[test]
fn under_ravel_puts_the_shape_back() {
    expect_ints(
        Lang::J,
        &[
            (", +:&., i. 2 3", &[0, 2, 4, 6, 8, 10]),
            ("$ +:&., i. 2 3", &[2, 3]),
            (", |.&., i. 2 3", &[5, 4, 3, 2, 1, 0]),
            // u need not answer as many values: the shape is put back by
            // reshaping, which cycles what there is.
            (", {.&., 2 2 $ 1 2 3 4", &[1, 1, 1, 1]),
            (", #&., 2 2 $ 1 2 3 4", &[4, 4, 4, 4]),
            ("+:&., 5", &[10]),
            ("$ +:&., 5", &[]),
            ("$ +:&., (i. 0)", &[0]),
        ],
    );
    // `,` itself has no obverse to be asked for, and under-ravel is monadic.
    expect_gap(Lang::J, &[", ^:_1 ] 1 2", "+:&.:, i. 2 3"]);
    let refused = run(Lang::J, "1 2 3 4 +&., 2 2 $ 1 2 3 4")
        .expect_err("under ravel has one valence");
    assert!(refused.msg.contains("no dyadic meaning"), "{}", refused.msg);
}

// --- the verbs the reference spells only as a negative power -------------

/// Three obverses have no spelling of their own in J: how many primes stand
/// below a number, the counting vector `I.` was given, and the dense form of
/// a sparse array. libjay writes each of them as the reference does, so
/// `u b. _1` answers the same characters.
#[test]
fn the_negative_powers_carry_their_own_spelling() {
    expect_ints(
        Lang::J,
        &[
            ("I.^:_1 ] 1 1 2", &[0, 2, 1]),
            ("I.^:_1 ] 0 0", &[2]),
            ("I.^:_1 ] 2 2", &[0, 0, 2]),
            ("I.^:_1 ] i. 0", &[]),
            ("I.^:_1 I. 0 2 1", &[0, 2, 1]),
            ("p:^:_1 ] 13", &[5]),
            ("p:^:_1 ] 12", &[5]),
            ("p:^:_1 ] 2", &[0]),
            ("p:^:_1 ] 100", &[25]),
            ("p:^:_1 p: 5", &[5]),
            ("$.^:_1 $. 1 0 2", &[1, 0, 2]),
        ],
    );
    assert_eq!(j("I. b. _1"), chars(&[6], "I.^:_1"));
    assert_eq!(j("p: b. _1"), chars(&[6], "p:^:_1"));
    assert_eq!(j("$. b. _1"), chars(&[6], "$.^:_1"));
}

// --- the rest of the named rows ------------------------------------------

/// The remaining rows the reference names: the prime factors multiply back,
/// the words join with a blank between them, and the exact, unicode and
/// symbol conversions each carry a form number that reverses them.
#[test]
fn the_conversions_reverse_by_their_form() {
    expect_ints(Lang::J, &[("q:^:_1 ] 2 2 3", &[12]), ("q:^:_1 ] i. 0", &[1])]);
    assert_eq!(j(";:^:_1 ;: 'ab cd'"), chars(&[5], "ab cd"));
    assert_eq!(j(";:^:_1 ] ,<'ab'"), chars(&[2], "ab"));
    expect_ints(Lang::J, &[("u:^:_1 u: 'abc'", &[97, 98, 99]), ("x:^:_1 ] 1 2 3x", &[1, 2, 3])]);
}

// --- what stays a gap ----------------------------------------------------

/// A verb the table does not reach says so by name rather than guessing at a
/// numerical inverse — and the reference refuses every one of these too.
#[test]
fn what_the_table_does_not_reach_is_named() {
    expect_gap(
        Lang::J,
        &[
            "* ^:_1 ] 1",
            "| ^:_1 ] 1",
            "<. ^:_1 ] 1",
            "= ^:_1 ] 1",
            "~. ^:_1 ] 1 2",
            "$ ^:_1 ] 1 2",
            ", ^:_1 ] 1 2",
            "i. ^:_1 ] 1 2",
            "; ^:_1 ] 1 2",
            "A. ^:_1 ] 1 2",
            "! ^:_1 ] 6",
            "(+/)^:_1 ] 6",
            "(<./\\)^:_1 ] 1 2",
            "(3&-~)^:_1 ] 1",
            "(2&{.)^:_1 ] 1 2",
            "(2&|)^:_1 ] 1",
        ],
    );
}

// --- APL reads the same table --------------------------------------------

/// `⍢` and `f⍣¯1` ask the obverse table the same question J's `&.` and
/// `^:_1` ask, so every row is reachable from either language.
#[test]
fn apl_reads_the_same_table() {
    expect_ints(
        Lang::Apl,
        &[
            ("⊢⍣¯1⊢1 2 3", &[1, 2, 3]),
            ("⌽⍣¯1⊢1 2 3", &[3, 2, 1]),
            ("(2∘⌽)⍣¯1⊢3 1 2", &[1, 2, 3]),
            ("(+\\)⍣¯1⊢1 3 6", &[1, 2, 3]),
            ("(-\\)⍣¯1⊢1 1 2", &[1, 0, 1]),
            ("⍕⍣¯1⊢'12'", &[12]),
            ("(1∘+)⍢⌽ 1 2 3", &[2, 3, 4]),
            ("⍋⍣¯1⊢2 0 1", &[2, 3, 1]),
        ],
    );
    expect_near(
        Lang::Apl,
        &[
            ("○⍣¯1⊢○1", &[1.0]),
            ("(2∘○)⍣¯1⊢2∘○1", &[1.0]),
            ("(*∘2)⍣¯1⊢9", &[3.0]),
            ("(÷\\)⍣¯1⊢1 2 3 4", &[1.0, 0.5, 1.5, 0.75]),
        ],
    );
    expect_near(Lang::Apl, &[(",⌹⍣¯1⌹2 2⍴1 2 3 4", &[1.0, 2.0, 3.0, 4.0])]);
    expect_gap(Lang::Apl, &["⌈⍣¯1⊢1", "(2∘↑)⍣¯1⊢1 2"]);
}
