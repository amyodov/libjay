//! End-to-end tests for the primitives added beyond the phase-2 gate: one
//! test per family, both languages, with hand-checked values. Where J and
//! APL spell the same idea differently the divergence is asserted, not
//! smoothed over.

use jay::{compile, Array, DType, Data, Dialect, ErrorKind, Lang};

fn run(lang: Lang, src: &str) -> Option<Array> {
    run_dialect(lang, src, &Dialect::default())
}

fn run_dialect(lang: Lang, src: &str, dialect: &Dialect) -> Option<Array> {
    let program = compile(lang, src, dialect)
        .unwrap_or_else(|e| panic!("compile failed:\n{}", e.render(src)));
    let mut sink = |_: &str| {};
    program
        .run(&[], &mut sink)
        .unwrap_or_else(|e| panic!("run of {src:?} failed:\n{}", program.render_error(&e)))
}

/// The result of a program that must produce a value.
fn val(lang: Lang, src: &str) -> Array {
    run(lang, src).unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

/// The same, with `⎕IO` set to 0 (APL only).
fn val0(src: &str) -> Array {
    run_dialect(Lang::Apl, src, &Dialect { index_origin: Some(0), ..Dialect::default() })
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn err(lang: Lang, src: &str) -> jay::Error {
    let program = match compile(lang, src, &Dialect::default()) {
        Err(e) => return e,
        Ok(p) => p,
    };
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink).expect_err("expected an error")
}

fn i64s(shape: &[usize], values: &[i64]) -> Array {
    Array::new(shape.to_vec(), Data::I64(values.to_vec().into()))
}

fn f64s(shape: &[usize], values: &[f64]) -> Array {
    Array::new(shape.to_vec(), Data::F64(values.to_vec().into()))
}

fn bits(shape: &[usize], values: &[u8]) -> Array {
    Array::new(shape.to_vec(), Data::Bool(values.to_vec().into()))
}

fn text(shape: &[usize], s: &str) -> Array {
    Array::new(shape.to_vec(), Data::Char(s.chars().collect()))
}

// --- reverse and rotate -------------------------------------------------

#[test]
fn j_reverse_and_rotate() {
    assert_eq!(val(Lang::J, "|. 1 2 3 4"), i64s(&[4], &[4, 3, 2, 1]));
    assert_eq!(val(Lang::J, "|. 'abc'"), text(&[3], "cba"));
    // The items are reversed, so a matrix turns over its leading axis.
    assert_eq!(val(Lang::J, "|. i. 2 3"), i64s(&[2, 3], &[3, 4, 5, 0, 1, 2]));
    assert_eq!(val(Lang::J, "1 |. 1 2 3 4 5"), i64s(&[5], &[2, 3, 4, 5, 1]));
    assert_eq!(val(Lang::J, "_1 |. 1 2 3 4 5"), i64s(&[5], &[5, 1, 2, 3, 4]));
    // The amount wraps around.
    assert_eq!(val(Lang::J, "7 |. 1 2 3"), i64s(&[3], &[2, 3, 1]));
    // A scalar left argument rotates the leading axis.
    assert_eq!(val(Lang::J, "1 |. i. 2 3"), i64s(&[2, 3], &[3, 4, 5, 0, 1, 2]));
    // A vector gives one amount per axis.
    assert_eq!(val(Lang::J, "1 1 |. i. 2 3"), i64s(&[2, 3], &[4, 5, 3, 1, 2, 0]));
    assert_eq!(val(Lang::J, "0 _1 |. i. 2 3"), i64s(&[2, 3], &[2, 0, 1, 5, 3, 4]));
    // A scalar has nothing to rotate.
    assert_eq!(val(Lang::J, "3 |. 5"), Array::scalar_i64(5));
    // More amounts than axes is a length error.
    assert_eq!(err(Lang::J, "1 2 |. 1 2 3").kind, ErrorKind::Length);
}

#[test]
fn apl_reverse_and_rotate() {
    // `⌽` works on the LAST axis, `⊖` on the leading one.
    assert_eq!(val(Lang::Apl, "⌽1 2 3 4"), i64s(&[4], &[4, 3, 2, 1]));
    assert_eq!(val(Lang::Apl, "⌽2 3⍴⍳6"), i64s(&[2, 3], &[3, 2, 1, 6, 5, 4]));
    assert_eq!(val(Lang::Apl, "⊖2 3⍴⍳6"), i64s(&[2, 3], &[4, 5, 6, 1, 2, 3]));
    assert_eq!(val(Lang::Apl, "1⌽1 2 3 4 5"), i64s(&[5], &[2, 3, 4, 5, 1]));
    assert_eq!(val(Lang::Apl, "¯1⌽1 2 3 4 5"), i64s(&[5], &[5, 1, 2, 3, 4]));
    // Dyadic `⌽` rotates each row, and one amount per row is allowed.
    assert_eq!(val(Lang::Apl, "1⌽2 3⍴⍳6"), i64s(&[2, 3], &[2, 3, 1, 5, 6, 4]));
    assert_eq!(val(Lang::Apl, "1 2⌽2 3⍴⍳6"), i64s(&[2, 3], &[2, 3, 1, 6, 4, 5]));
    // Dyadic `⊖` rotates the leading axis.
    assert_eq!(val(Lang::Apl, "1⊖2 3⍴⍳6"), i64s(&[2, 3], &[4, 5, 6, 1, 2, 3]));
}

/// Same matrix, same idea, different axis: this is the J/APL divergence the
/// one IR has to keep straight.
#[test]
fn divergence_reverse_axis() {
    let j = val(Lang::J, "|. i. 2 3");
    let apl_last = val0("⌽2 3⍴⍳6");
    let apl_leading = val0("⊖2 3⍴⍳6");
    assert_eq!(j, i64s(&[2, 3], &[3, 4, 5, 0, 1, 2]));
    assert_eq!(apl_last, i64s(&[2, 3], &[2, 1, 0, 5, 4, 3]));
    assert_ne!(j, apl_last, "J reverses items, APL `⌽` reverses rows");
    assert_eq!(apl_leading, j, "APL `⊖` agrees with J `|.`");
}

// --- catenate -----------------------------------------------------------

#[test]
fn j_catenate_is_leading_axis() {
    assert_eq!(val(Lang::J, "1 2 , 3 4"), i64s(&[4], &[1, 2, 3, 4]));
    assert_eq!(
        val(Lang::J, "(i. 2 3) , i. 2 3"),
        i64s(&[4, 3], &[0, 1, 2, 3, 4, 5, 0, 1, 2, 3, 4, 5])
    );
    // Rank promotion: a vector becomes one item of the matrix.
    assert_eq!(
        val(Lang::J, "(i. 2 3) , 10 20 30"),
        i64s(&[3, 3], &[0, 1, 2, 3, 4, 5, 10, 20, 30])
    );
    assert_eq!(
        val(Lang::J, "10 20 30 , i. 2 3"),
        i64s(&[3, 3], &[10, 20, 30, 0, 1, 2, 3, 4, 5])
    );
    // A scalar spreads over one whole item.
    assert_eq!(val(Lang::J, "(i. 2 3) , 5"), i64s(&[3, 3], &[0, 1, 2, 3, 4, 5, 5, 5, 5]));
    assert_eq!(val(Lang::J, "1 2 3 , 5"), i64s(&[4], &[1, 2, 3, 5]));
    // Characters catenate; characters and numbers do not.
    assert_eq!(val(Lang::J, "'ab' , 'cd'"), text(&[4], "abcd"));
    assert_eq!(val(Lang::J, "'ab' , 'c'"), text(&[3], "abc"));
    assert_eq!(err(Lang::J, "1 2 , 'ab'").kind, ErrorKind::Type);
    // Item shapes that disagree are overtaken to the larger, which fills.
    assert_eq!(
        val(Lang::J, "1 2 3 , i. 2 2"),
        i64s(&[3, 3], &[1, 2, 3, 0, 1, 0, 2, 3, 0])
    );
    assert_eq!(
        val(Lang::J, "(i. 2 2) , 1 2 3"),
        i64s(&[3, 3], &[0, 1, 0, 2, 3, 0, 1, 2, 3])
    );
    assert_eq!(
        val(Lang::J, "(i. 2 3) , i. 2 2"),
        i64s(&[4, 3], &[0, 1, 2, 3, 4, 5, 0, 1, 0, 2, 3, 0])
    );
    // Two ranks apart is a rank error.
    assert_eq!(err(Lang::J, "1 2 3 , i. 2 2 2").kind, ErrorKind::Rank);
}

#[test]
fn j_stitch_is_catenate_on_one_cell_less() {
    // `,.` is `,"_1`: on vectors the items are atoms, so it makes columns.
    assert_eq!(val(Lang::J, "1 2 ,. 3 4"), i64s(&[2, 2], &[1, 3, 2, 4]));
    assert_eq!(
        val(Lang::J, "(i. 2 3) ,. i. 2 2"),
        i64s(&[2, 5], &[0, 1, 2, 0, 1, 3, 4, 5, 2, 3])
    );
    assert_eq!(val(Lang::J, "(i. 2 3) ,. 9"), i64s(&[2, 4], &[0, 1, 2, 9, 3, 4, 5, 9]));
    assert_eq!(val(Lang::J, "1 2 ,. i. 2 3"), i64s(&[2, 4], &[1, 0, 1, 2, 2, 3, 4, 5]));
}

#[test]
fn apl_catenate_is_last_axis() {
    assert_eq!(val(Lang::Apl, "1 2,3 4"), i64s(&[4], &[1, 2, 3, 4]));
    assert_eq!(
        val(Lang::Apl, "(2 3⍴⍳6),2 2⍴7 8 9 10"),
        i64s(&[2, 5], &[1, 2, 3, 7, 8, 4, 5, 6, 9, 10])
    );
    // A vector becomes one column.
    assert_eq!(val(Lang::Apl, "(2 3⍴⍳6),10 20"), i64s(&[2, 4], &[1, 2, 3, 10, 4, 5, 6, 20]));
    assert_eq!(val(Lang::Apl, "(2 3⍴⍳6),0"), i64s(&[2, 4], &[1, 2, 3, 0, 4, 5, 6, 0]));
    // `⍪` is the leading-axis catenate, which is J's `,`.
    assert_eq!(val(Lang::Apl, "1 2⍪3 4"), i64s(&[4], &[1, 2, 3, 4]));
    assert_eq!(
        val(Lang::Apl, "(2 3⍴⍳6)⍪7 8 9"),
        i64s(&[3, 3], &[1, 2, 3, 4, 5, 6, 7, 8, 9])
    );
    assert_eq!(val0("(2 3⍴⍳6)⍪7 8 9"), val(Lang::J, "(i. 2 3) , 7 8 9"));
}

// --- indexing -----------------------------------------------------------

#[test]
fn j_from_selects_items() {
    assert_eq!(val(Lang::J, "2 0 { i. 3 3"), i64s(&[2, 3], &[6, 7, 8, 0, 1, 2]));
    assert_eq!(val(Lang::J, "0 { i. 3 3"), i64s(&[3], &[0, 1, 2]));
    // Negative indexes count from the end.
    assert_eq!(val(Lang::J, "_1 { 5 6 7"), Array::scalar_i64(7));
    assert_eq!(val(Lang::J, "_1 _2 { 5 6 7"), i64s(&[2], &[7, 6]));
    // The left argument has rank 0, so its shape frames the result.
    assert_eq!(val(Lang::J, "(i. 2 2) { 10 20 30 40"), i64s(&[2, 2], &[10, 20, 30, 40]));
    assert_eq!(val(Lang::J, "1 2 { 'abcdef'"), text(&[2], "bc"));
    // A scalar has one item.
    assert_eq!(val(Lang::J, "0 { 5"), Array::scalar_i64(5));
    // Out of range names the index and the count.
    let e = err(Lang::J, "3 { i. 3");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains('3') && e.msg.contains("3 items"), "{}", e.msg);
    assert_eq!(err(Lang::J, "_4 { i. 3").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "1.5 { i. 3").kind, ErrorKind::Domain);
}

#[test]
fn index_of_reports_the_count_when_absent() {
    assert_eq!(val(Lang::J, "1 2 3 i. 2"), Array::scalar_i64(1));
    // Absent gives the number of items, which is one past every index.
    assert_eq!(val(Lang::J, "1 2 3 i. 4"), Array::scalar_i64(3));
    assert_eq!(val(Lang::J, "1 2 3 i. 2 3 9"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(val(Lang::J, "'abc' i. 'cab'"), i64s(&[3], &[2, 0, 1]));
    // Cells of the right argument have the shape of the left one's items.
    assert_eq!(val(Lang::J, "(i. 3 3) i. 3 4 5"), Array::scalar_i64(1));
    assert_eq!(val(Lang::J, "(i. 3 3) i. 1 1 1"), Array::scalar_i64(3));
    // APL counts from the index origin, absent included.
    assert_eq!(val(Lang::Apl, "1 2 3⍳2"), Array::scalar_i64(2));
    assert_eq!(val(Lang::Apl, "1 2 3⍳4"), Array::scalar_i64(4));
    assert_eq!(val0("1 2 3⍳4"), Array::scalar_i64(3));
    assert_eq!(val0("1 2 3⍳2"), val(Lang::J, "1 2 3 i. 2"));
    // Integers and floats of the same value are the same item.
    assert_eq!(val(Lang::J, "1 2 3 i. 2.0"), Array::scalar_i64(1));
}

// --- membership ---------------------------------------------------------

/// The same data in both languages: J asks about cells shaped like items of
/// the right argument, APL about single elements.
#[test]
fn membership_diverges_between_the_languages() {
    assert_eq!(val(Lang::J, "1 3 e. 2 2 $ 1 2 3 4"), Array::scalar_bool(false));
    assert_eq!(val(Lang::J, "1 2 e. 2 2 $ 1 2 3 4"), Array::scalar_bool(true));
    assert_eq!(val(Lang::Apl, "1 3∊2 2⍴1 2 3 4"), bits(&[2], &[1, 1]));
    assert_eq!(val(Lang::Apl, "1 5∊2 2⍴1 2 3 4"), bits(&[2], &[1, 0]));
    assert_ne!(
        val(Lang::J, "1 3 e. 2 2 $ 1 2 3 4").shape,
        val(Lang::Apl, "1 3∊2 2⍴1 2 3 4").shape
    );
}

#[test]
fn j_membership_by_cells() {
    assert_eq!(val(Lang::J, "1 2 e. 1 3 5"), bits(&[2], &[1, 0]));
    assert_eq!(val(Lang::J, "5 e. 1 2 3"), Array::scalar_bool(false));
    assert_eq!(val(Lang::J, "'ab' e. 'abc'"), bits(&[2], &[1, 1]));
    assert_eq!(val(Lang::J, "(i. 2 3) e. i. 3 3"), bits(&[2], &[1, 1]));
    // Items of the right argument are atoms here, so every cell is an atom.
    assert_eq!(val(Lang::J, "(i. 2 3) e. 1 2 3"), bits(&[2, 3], &[0, 1, 1, 1, 0, 0]));
    // A cell of the wrong shape is simply not an item.
    assert_eq!(val(Lang::J, "5 e. i. 2 3"), Array::scalar_bool(false));
}

#[test]
fn apl_membership_by_elements() {
    assert_eq!(val(Lang::Apl, "1 2 3∊2 3⍴⍳6"), bits(&[3], &[1, 1, 1]));
    assert_eq!(val(Lang::Apl, "(2 2⍴1 2 9 9)∊1 2 3"), bits(&[2, 2], &[1, 1, 0, 0]));
    assert_eq!(val(Lang::Apl, "'ab'∊'cab'"), bits(&[2], &[1, 1]));
    // Characters never occur among numbers.
    assert_eq!(val(Lang::Apl, "'ab'∊1 2 3"), bits(&[2], &[0, 0]));
}

// --- match --------------------------------------------------------------

#[test]
fn match_never_fails_on_shape() {
    assert_eq!(val(Lang::J, "1 2 3 -: 1 2 3"), Array::scalar_bool(true));
    assert_eq!(val(Lang::J, "1 2 3 -: 1 2"), Array::scalar_bool(false));
    assert_eq!(val(Lang::J, "(i. 2 3) -: i. 2 3"), Array::scalar_bool(true));
    assert_eq!(val(Lang::J, "(i. 2 3) -: i. 3 2"), Array::scalar_bool(false));
    // A one-item vector is not a scalar.
    assert_eq!(val(Lang::J, "1 -: 1 1 $ 1"), Array::scalar_bool(false));
    // The value matters, not the element type.
    assert_eq!(val(Lang::J, "1 -: 1.0"), Array::scalar_bool(true));
    // Characters never equal numbers.
    assert_eq!(val(Lang::J, "'a' -: 97"), Array::scalar_bool(false));
    assert_eq!(val(Lang::J, "'abc' -: 'abc'"), Array::scalar_bool(true));
    // APL spells it `≡`, and `≢` is its negation.
    assert_eq!(val(Lang::Apl, "1 2 3≡1 2 3"), Array::scalar_bool(true));
    assert_eq!(val(Lang::Apl, "1 2 3≡1 2"), Array::scalar_bool(false));
    assert_eq!(val(Lang::Apl, "1 2 3≢1 2 3"), Array::scalar_bool(false));
    assert_eq!(val(Lang::Apl, "1 2 3≢1 2"), Array::scalar_bool(true));
    // Monadic `≢` is still the tally.
    assert_eq!(val(Lang::Apl, "≢1 2 3"), Array::scalar_i64(3));
}

// --- ordering -----------------------------------------------------------

#[test]
fn grade_is_stable_and_lexicographic() {
    // Ties keep their original order, ascending and descending alike.
    assert_eq!(val(Lang::J, "/: 3 1 4 1 5"), i64s(&[5], &[1, 3, 0, 2, 4]));
    assert_eq!(val(Lang::J, "\\: 3 1 4 1 5"), i64s(&[5], &[4, 2, 0, 1, 3]));
    assert_eq!(val(Lang::J, "/: 1 1 1"), i64s(&[3], &[0, 1, 2]));
    assert_eq!(val(Lang::J, "\\: 1 1 1"), i64s(&[3], &[0, 1, 2]));
    assert_eq!(val(Lang::J, "/: 1.5 0.5 2.5"), i64s(&[3], &[1, 0, 2]));
    assert_eq!(val(Lang::J, "/: 'hello'"), i64s(&[5], &[1, 0, 2, 3, 4]));
    // Items of a matrix compare element by element, left to right.
    assert_eq!(val(Lang::J, "/: 2 3 $ 1 2 1 1 1 3"), i64s(&[2], &[1, 0]));
    assert_eq!(val(Lang::J, "\\: 2 3 $ 1 2 1 1 1 3"), i64s(&[2], &[0, 1]));
    // APL numbers the same permutation from its index origin.
    assert_eq!(val(Lang::Apl, "⍋3 1 4 1 5"), i64s(&[5], &[2, 4, 1, 3, 5]));
    assert_eq!(val(Lang::Apl, "⍒3 1 4 1 5"), i64s(&[5], &[5, 3, 1, 2, 4]));
    assert_eq!(val0("⍋3 1 4 1 5"), val(Lang::J, "/: 3 1 4 1 5"));
}

#[test]
fn dyadic_grade_sorts_one_array_by_another() {
    assert_eq!(val(Lang::J, "3 1 4 1 5 /: 3 1 4 1 5"), i64s(&[5], &[1, 1, 3, 4, 5]));
    assert_eq!(val(Lang::J, "3 1 4 1 5 \\: 3 1 4 1 5"), i64s(&[5], &[5, 4, 3, 1, 1]));
    assert_eq!(val(Lang::J, "'abc' /: 3 1 2"), text(&[3], "bca"));
    assert_eq!(val(Lang::J, "'abc' \\: 3 1 2"), text(&[3], "acb"));
    // The grade of y indexes x, so a longer key runs off the end of it.
    assert_eq!(err(Lang::J, "1 2 /: 1 2 3").kind, ErrorKind::Domain);
    // A SHORTER key is not an error: it selects fewer items.
    assert_eq!(val(Lang::J, "1 2 3 /: 'ab'"), i64s(&[2], &[1, 2]));
}

// --- LCM and GCD --------------------------------------------------------

#[test]
fn lcm_and_gcd() {
    assert_eq!(val(Lang::J, "4 *. 6"), Array::scalar_i64(12));
    assert_eq!(val(Lang::J, "12 +. 18"), Array::scalar_i64(6));
    // The GCD is never negative; the LCM keeps the sign of the product.
    assert_eq!(val(Lang::J, "_4 +. 6"), Array::scalar_i64(2));
    assert_eq!(val(Lang::J, "_4 *. 6"), Array::scalar_i64(-12));
    assert_eq!(val(Lang::J, "_4 *. _6"), Array::scalar_i64(12));
    // Zero cases.
    assert_eq!(val(Lang::J, "0 +. 0"), Array::scalar_i64(0));
    assert_eq!(val(Lang::J, "0 +. 5"), Array::scalar_i64(5));
    assert_eq!(val(Lang::J, "0 *. 5"), Array::scalar_i64(0));
    // Reductions have the identities 1 and 0.
    assert_eq!(val(Lang::J, "*./ 4 6 8"), Array::scalar_i64(24));
    assert_eq!(val(Lang::J, "+./ 12 18 24"), Array::scalar_i64(6));
    assert_eq!(val(Lang::J, "*./ i. 0"), Array::scalar_i64(1));
    assert_eq!(val(Lang::J, "+./ i. 0"), Array::scalar_i64(0));
    // On booleans the pair is exactly logical and / or, and stays boolean.
    let and = val(Lang::J, "(1 2 3 > 2) *. 1 2 3 > 1");
    assert_eq!(and, bits(&[3], &[0, 0, 1]));
    assert_eq!(and.dtype(), DType::Bool);
    let or = val(Lang::J, "(1 2 3 > 2) +. 1 2 3 > 1");
    assert_eq!(or, bits(&[3], &[0, 1, 1]));
    // APL spells them `∧` and `∨`, with no monadic meaning.
    assert_eq!(val(Lang::Apl, "4∧6"), Array::scalar_i64(12));
    assert_eq!(val(Lang::Apl, "12∨18"), Array::scalar_i64(6));
    assert_eq!(val(Lang::Apl, "(1 2 3>2)∧1 2 3>1"), bits(&[3], &[0, 0, 1]));
    assert_eq!(err(Lang::Apl, "∧1 2").kind, ErrorKind::Domain);
    // Integral floats compute; anything else waits.
    assert_eq!(val(Lang::J, "4.0 *. 6.0"), f64s(&[], &[12.0]));
    let e = err(Lang::J, "2.5 +. 5");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("LCM/GCD on floats"), "{}", e.msg);
}

// --- logarithm and root -------------------------------------------------

#[test]
fn logarithm_and_root() {
    assert_eq!(val(Lang::J, "^. 1"), f64s(&[], &[0.0]));
    assert_eq!(val(Lang::J, "2 ^. 8"), f64s(&[], &[3.0]));
    assert_eq!(val(Lang::J, "10 ^. 100"), f64s(&[], &[2.0]));
    assert_eq!(val(Lang::J, "^. 0"), f64s(&[], &[f64::NEG_INFINITY]));
    assert_eq!(val(Lang::J, "3 %: 27"), f64s(&[], &[3.0]));
    assert_eq!(val(Lang::J, "2 %: 9"), f64s(&[], &[3.0]));
    // A logarithm or a root that leaves the reals is complex.
    for src in ["^. _1", "2 %: _8", "_2 ^. 8"] {
        assert_eq!(val(Lang::J, src).dtype(), DType::Complex, "{src}");
    }
    // APL spells the logarithm `⍟`, both valences.
    assert_eq!(val(Lang::Apl, "⍟1"), f64s(&[], &[0.0]));
    assert_eq!(val(Lang::Apl, "2⍟8"), f64s(&[], &[3.0]));
}

// --- the small arithmetic verbs -----------------------------------------

#[test]
fn increment_decrement_double_halve_square() {
    assert_eq!(val(Lang::J, ">: 5"), Array::scalar_i64(6));
    assert_eq!(val(Lang::J, "<: 5"), Array::scalar_i64(4));
    assert_eq!(val(Lang::J, "+: 1 2 3"), i64s(&[3], &[2, 4, 6]));
    assert_eq!(val(Lang::J, "*: _3"), Array::scalar_i64(9));
    // Halving is always float; the rest stay integral where they can.
    assert_eq!(val(Lang::J, "-: 8"), f64s(&[], &[4.0]));
    assert_eq!(val(Lang::J, "-: 1 2 3"), f64s(&[3], &[0.5, 1.0, 1.5]));
    assert_eq!(val(Lang::J, "<: 2.5"), f64s(&[], &[1.5]));
    assert_eq!(val(Lang::J, ">: 5").dtype(), DType::I64);
    // Overflow widens the whole result, as everywhere else.
    let big = format!(">: {}", i64::MAX);
    assert_eq!(val(Lang::J, &big).dtype(), DType::F64);
    // J's `-.` is `1 - y` on any number; APL's `~` insists on 0 or 1.
    assert_eq!(val(Lang::J, "-. 0 1"), i64s(&[2], &[1, 0]));
    assert_eq!(val(Lang::J, "-. 0.25"), f64s(&[], &[0.75]));
    assert_eq!(val(Lang::J, "-. 5"), Array::scalar_i64(-4));
    assert_eq!(val(Lang::Apl, "~0 1"), bits(&[2], &[1, 0]));
    assert_eq!(err(Lang::Apl, "~0.25").kind, ErrorKind::Domain);
    // The dyads of `+:` and `*:` are the boolean pair, and read nothing
    // else; `-.` dyadically removes items rather than subtracting.
    assert_eq!(val(Lang::J, "1 0 1 +: 0 1 1"), bits(&[3], &[0, 0, 0]));
    assert_eq!(val(Lang::J, "1 0 1 *: 0 1 1"), bits(&[3], &[1, 1, 0]));
    assert_eq!(err(Lang::J, "2 +: 3").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "2 *: 3").kind, ErrorKind::Domain);
    assert_eq!(val(Lang::J, "1 2 3 4 -. 2 4"), i64s(&[2], &[1, 3]));
}

// --- nub ----------------------------------------------------------------

#[test]
fn nub_keeps_first_occurrences() {
    assert_eq!(val(Lang::J, "~. 3 1 4 1 5 9 2 6 5 3"), i64s(&[7], &[3, 1, 4, 5, 9, 2, 6]));
    assert_eq!(val(Lang::J, "~. 1.5 2.5 1.5 0.5"), f64s(&[3], &[1.5, 2.5, 0.5]));
    assert_eq!(val(Lang::J, "~. 'mississippi'"), text(&[4], "misp"));
    // Whole items, not elements.
    assert_eq!(val(Lang::J, "~. 2 3 $ 1 2 3 1 2 3"), i64s(&[1, 3], &[1, 2, 3]));
    assert_eq!(val(Lang::J, "~. i. 3 3"), val(Lang::J, "i. 3 3"));
    // A scalar is one item, so the nub is a one-item vector.
    assert_eq!(val(Lang::J, "~. 5"), i64s(&[1], &[5]));
    // APL spells it `∪`.
    assert_eq!(val(Lang::Apl, "∪3 1 4 1 5"), i64s(&[4], &[3, 1, 4, 5]));
    assert_eq!(val(Lang::Apl, "∪'mississippi'"), text(&[4], "misp"));
}

// --- tail and curtail ---------------------------------------------------

#[test]
fn tail_and_curtail() {
    assert_eq!(val(Lang::J, "{: 1 2 3"), Array::scalar_i64(3));
    assert_eq!(val(Lang::J, "}: 1 2 3"), i64s(&[2], &[1, 2]));
    assert_eq!(val(Lang::J, "{: i. 3 2"), i64s(&[2], &[4, 5]));
    assert_eq!(val(Lang::J, "}: i. 3 2"), i64s(&[2, 2], &[0, 1, 2, 3]));
    assert_eq!(val(Lang::J, "{: 'abc'"), text(&[], "c"));
    assert_eq!(val(Lang::J, "}: 'abc'"), text(&[2], "ab"));
    // With no items the tail is a cell of fills.
    assert_eq!(val(Lang::J, "{: i. 0 2"), i64s(&[2], &[0, 0]));
    assert_eq!(val(Lang::J, "}: i. 0 2"), i64s(&[0, 2], &[]));
    assert_eq!(val(Lang::J, "{: 0 $ 0"), Array::scalar_i64(0));
    // A scalar is one item: it is its own tail and curtails to nothing.
    assert_eq!(val(Lang::J, "{: 5"), Array::scalar_i64(5));
    assert_eq!(val(Lang::J, "}: 5"), Array::empty(DType::I64));
    // Neither has a dyadic meaning.
    assert_eq!(err(Lang::J, "1 {: 1 2 3").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "1 }: 1 2 3").kind, ErrorKind::Domain);
}

// --- composition --------------------------------------------------------

/// `u@v` and `u@:v` compute the same thing on different cells: `@` runs at
/// v's ranks, `@:` on the argument whole. That is the whole difference.
#[test]
fn atop_differs_from_atop_infinite_only_in_rank() {
    // `,` takes its argument whole either way, so the two agree.
    assert_eq!(val(Lang::J, "(+/ @ ,) i. 2 3"), Array::scalar_i64(15));
    assert_eq!(val(Lang::J, "(+/ @: ,) i. 2 3"), Array::scalar_i64(15));
    // `,"1` has rank 1: `@` sums each row, `@:` sums the columns of the
    // whole (unchanged) matrix.
    assert_eq!(val(Lang::J, "(+/ @ (,\"1)) i. 2 3"), i64s(&[2], &[3, 12]));
    assert_eq!(val(Lang::J, "(+/ @: (,\"1)) i. 2 3"), i64s(&[3], &[3, 5, 7]));
    assert_eq!(val(Lang::J, "($ @ (]\"1)) i. 2 3"), i64s(&[2, 1], &[3, 3]));
    assert_eq!(val(Lang::J, "($ @: (]\"1)) i. 2 3"), i64s(&[2], &[2, 3]));
    // Dyadically the ranks come from v as well.
    assert_eq!(
        val(Lang::J, "1 2 3 (+/ @ (,\"0)) 10 20 30"),
        i64s(&[3], &[11, 22, 33])
    );
    assert_eq!(
        val(Lang::J, "1 2 3 (+/ @: (,\"0)) 10 20 30"),
        i64s(&[2], &[6, 60])
    );
}

#[test]
fn compose_applies_the_right_verb_to_both_arguments() {
    // Monadically a composition is an atop.
    assert_eq!(val(Lang::J, "(+ & *:) 1 2 3"), i64s(&[3], &[1, 4, 9]));
    // Dyadically `x (u&v) y` is `(v x) u (v y)`.
    assert_eq!(val(Lang::J, "2 (+ & *:) 3"), Array::scalar_i64(13));
    assert_eq!(val(Lang::J, "1 2 3 (+&*:) 1 2 3"), i64s(&[3], &[2, 8, 18]));
    // `&` runs at v's monadic rank, `&:` on the arguments whole: here v
    // sums each row, and only `&` pairs the two arguments row by row.
    assert_eq!(
        val(Lang::J, "(i.2 3) (,&(+/\"1)) i. 2 3"),
        i64s(&[2, 2], &[3, 3, 12, 12])
    );
    assert_eq!(
        val(Lang::J, "(i.2 3) (,&:(+/\"1)) i. 2 3"),
        i64s(&[4], &[3, 12, 3, 12])
    );
}

#[test]
fn a_noun_operand_bonds_it_into_the_dyad() {
    assert_eq!(val(Lang::J, "(1 & +) 5"), Array::scalar_i64(6));
    assert_eq!(val(Lang::J, "(1 & +) i. 2 3"), i64s(&[2, 3], &[1, 2, 3, 4, 5, 6]));
    assert_eq!(val(Lang::J, "(^ & 2) 1 2 3"), i64s(&[3], &[1, 4, 9]));
    assert_eq!(val(Lang::J, "(2 & ^) 1 2 3"), i64s(&[3], &[2, 4, 8]));
    assert_eq!(val(Lang::J, "(10 & *) 1 2 3"), i64s(&[3], &[10, 20, 30]));
    // Which side the noun sits on is the difference between the two.
    assert_eq!(val(Lang::J, "(- & 1) 5"), Array::scalar_i64(4));
    assert_eq!(val(Lang::J, "(1 & -) 5"), Array::scalar_i64(-4));
    // Bonds inside larger trains.
    assert_eq!(val(Lang::J, "(1&+ @ *:) 1 2 3"), i64s(&[3], &[2, 5, 10]));
    assert_eq!(val(Lang::J, "(^&2 @ (1&+)) 1 2 3"), i64s(&[3], &[4, 9, 16]));
    assert_eq!(val(Lang::J, "(10&* + 1&+) 1 2 3"), i64s(&[3], &[12, 23, 34]));
    // The bond takes the rank of the side its argument arrives on: `,` is
    // infinite both ways, `,"1` reads rows.
    assert_eq!(val(Lang::J, "(1 & (,\"1)) i. 2 3"), i64s(&[2, 4], &[1, 0, 1, 2, 1, 3, 4, 5]));
    assert_eq!(val(Lang::J, "((,\"1) & 1) i. 2 3"), i64s(&[2, 4], &[0, 1, 2, 1, 3, 4, 5, 1]));
    // A bond has one valence only, as in J.
    assert_eq!(err(Lang::J, "3 (1 & +) 5").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "3 (^ & 2) 5").kind, ErrorKind::Domain);
}

// --- table (outer product) ----------------------------------------------

#[test]
fn table_pairs_every_cell_with_every_cell() {
    // The times table, spelled `u/` in J and `∘.u` in APL.
    let times = i64s(&[3, 3], &[1, 2, 3, 2, 4, 6, 3, 6, 9]);
    assert_eq!(val(Lang::J, "1 2 3 */ 1 2 3"), times);
    assert_eq!(val(Lang::Apl, "1 2 3∘.×1 2 3"), times);
    assert_eq!(val(Lang::J, "1 2 3 +/ 10 20"), i64s(&[3, 2], &[11, 21, 12, 22, 13, 23]));
    assert_eq!(val(Lang::Apl, "1 2 3∘.+10 20"), i64s(&[3, 2], &[11, 21, 12, 22, 13, 23]));
    assert_eq!(val(Lang::J, "1 2 3 </ 2 3"), bits(&[3, 2], &[1, 1, 0, 1, 0, 0]));
    // The frame is x's cells then y's cells, so it grows with both.
    assert_eq!(val(Lang::J, "(i.2 2) +/ i.2").shape, vec![2, 2, 2]);
    assert_eq!(val(Lang::J, "(i.2 3) +/ 10 20").shape, vec![2, 3, 2]);
    // The cells are the ones the verb's own ranks ask for: `,` takes both
    // arguments whole, so its table is a single catenation.
    assert_eq!(val(Lang::J, "'ab' ,/ 'cd'"), text(&[4], "abcd"));
    assert_eq!(val(Lang::J, "(i.2 3) (,\"1)/ 1 2"), i64s(&[2, 5], &[0, 1, 2, 1, 2, 3, 4, 5, 1, 2]));
    // Two scalars leave no frame at all.
    assert_eq!(val(Lang::J, "2 +/ 3"), Array::scalar_i64(5));
    // `∘` on its own is Dyalog's beside; a value operand is a separate gap.
    assert_eq!(val(Lang::Apl, "1 2 3+∘×1 2 3"), i64s(&[3], &[2, 3, 4]));
    assert_eq!(err(Lang::Apl, "1∘×2").kind, ErrorKind::NotYet);
}

// --- factorial and binomial ---------------------------------------------

#[test]
fn factorial_and_binomial() {
    // The factorial is J's gamma, so it is always float.
    assert_eq!(val(Lang::J, "! 0 1 2 3 4 5"), f64s(&[6], &[1.0, 1.0, 2.0, 6.0, 24.0, 120.0]));
    assert_eq!(val(Lang::J, "! 5").dtype(), DType::F64);
    let half = val(Lang::J, "! 0.5").to_f64_vec().expect("numeric")[0];
    assert!((half - 0.886_226_925_452_758).abs() < 1e-12, "{half}");
    let two_half = val(Lang::J, "! 2.5").to_f64_vec().expect("numeric")[0];
    assert!((two_half - 3.323_350_970_447_843).abs() < 1e-12, "{two_half}");
    // A negative integer is a pole; the limit alternates sign.
    assert_eq!(val(Lang::J, "! _1"), f64s(&[], &[f64::INFINITY]));
    assert_eq!(val(Lang::J, "! _2"), f64s(&[], &[f64::NEG_INFINITY]));
    // The binomial keeps whole answers whole.
    assert_eq!(val(Lang::J, "2 ! 5"), Array::scalar_i64(10));
    assert_eq!(val(Lang::J, "0 ! 5"), Array::scalar_i64(1));
    assert_eq!(val(Lang::J, "6 ! 5"), Array::scalar_i64(0));
    assert_eq!(val(Lang::J, "_1 ! 5"), Array::scalar_i64(0));
    assert_eq!(val(Lang::J, "10 ! 100"), Array::scalar_i64(17_310_309_456_440));
    // A negative pair follows the upper-negation identity, sign and all.
    assert_eq!(val(Lang::J, "2 ! _5"), Array::scalar_i64(15));
    assert_eq!(val(Lang::J, "_3 ! _2"), Array::scalar_i64(-2));
    assert_eq!(val(Lang::J, "_5 ! _3"), Array::scalar_i64(6));
    assert_eq!(val(Lang::J, "_2 ! _5"), Array::scalar_i64(0));
    // Fractional arguments go through the gamma quotient.
    assert_eq!(val(Lang::J, "2 ! 5.5"), f64s(&[], &[12.375]));
    let g = val(Lang::J, "0.5 ! 2").to_f64_vec().expect("numeric")[0];
    assert!((g - 1.697_652_726_36).abs() < 1e-9, "{g}");
    // APL spells both the same way and in the same argument order.
    assert_eq!(val(Lang::Apl, "!0 1 2 3 4 5"), f64s(&[6], &[1.0, 1.0, 2.0, 6.0, 24.0, 120.0]));
    assert_eq!(val(Lang::Apl, "2!5"), Array::scalar_i64(10));
    assert_eq!(val(Lang::Apl, "3!10"), Array::scalar_i64(120));
}

// --- format -------------------------------------------------------------

#[test]
fn format_yields_the_characters_of_the_display() {
    assert_eq!(val(Lang::J, "\": 2.5"), text(&[3], "2.5"));
    assert_eq!(val(Lang::J, "\": 5"), text(&[1], "5"));
    assert_eq!(val(Lang::J, "\": 1 2 3"), text(&[5], "1 2 3"));
    assert_eq!(val(Lang::J, "\": _1 22 333"), text(&[9], "_1 22 333"));
    // A matrix becomes a character matrix, one row per line.
    assert_eq!(val(Lang::J, "$ \": i. 2 3"), i64s(&[2], &[2, 5]));
    assert_eq!(val(Lang::J, "\": i. 2 3"), text(&[2, 5], "0 1 23 4 5"));
    // The column widths span the whole argument, so every line is one width
    // and the planes of a rank-3 array stay aligned.
    assert_eq!(val(Lang::J, "$ \": i. 2 3 4"), i64s(&[3], &[2, 3, 11]));
    assert_eq!(val(Lang::J, "\": 2 2 $ 1 22 333 4"), text(&[2, 6], "  1 22333  4"));
    // Characters are already their own display.
    assert_eq!(val(Lang::J, "\": 'abc'"), text(&[3], "abc"));
    assert_eq!(val(Lang::J, "\": 'a'"), text(&[], "a"));
    // APL prints its own negative sign.
    assert_eq!(val(Lang::Apl, "⍕¯1 22 333"), text(&[9], "¯1 22 333"));
    assert_eq!(val(Lang::Apl, "⍕2.5"), text(&[3], "2.5"));
}

// --- base conversion ----------------------------------------------------

#[test]
fn decode_reads_digits_in_a_radix() {
    assert_eq!(val(Lang::J, "#. 1 0 1"), Array::scalar_i64(5));
    assert_eq!(val(Lang::J, "2 #. 1 0 1"), Array::scalar_i64(5));
    // Mixed radix: hours, minutes, seconds.
    assert_eq!(val(Lang::J, "24 60 60 #. 1 2 3"), Array::scalar_i64(3723));
    // A scalar radix extends over the digits; a leading 0 contributes none.
    assert_eq!(val(Lang::J, "2 #. 1 2 3"), Array::scalar_i64(11));
    assert_eq!(val(Lang::J, "0 #. 1 2 3"), Array::scalar_i64(3));
    // Rank 1: one number per row.
    assert_eq!(val(Lang::J, "#. i. 2 3"), i64s(&[2], &[4, 25]));
    // A radix list of the wrong length is a length error.
    assert_eq!(err(Lang::J, "2 3 #. 1 2 3").kind, ErrorKind::Length);
    // APL spells it `⊥`, with no monadic meaning.
    assert_eq!(val(Lang::Apl, "2⊥1 0 1"), Array::scalar_i64(5));
    assert_eq!(val(Lang::Apl, "24 60 60⊥1 2 3"), Array::scalar_i64(3723));
    assert_eq!(err(Lang::Apl, "⊥1 0 1").kind, ErrorKind::Domain);
}

#[test]
fn encode_writes_a_number_in_a_radix() {
    // The monad picks a width wide enough for the largest value anywhere in
    // the argument, which is why it is not a rank-0 verb.
    assert_eq!(val(Lang::J, "#: 5"), i64s(&[3], &[1, 0, 1]));
    assert_eq!(val(Lang::J, "#: 0"), i64s(&[1], &[0]));
    assert_eq!(val(Lang::J, "#: 2 5"), i64s(&[2, 3], &[0, 1, 0, 1, 0, 1]));
    assert_eq!(val(Lang::J, "#: 8"), i64s(&[4], &[1, 0, 0, 0]));
    // A negative value wraps within that width.
    assert_eq!(val(Lang::J, "#: _5"), i64s(&[3], &[0, 1, 1]));
    // Dyadically the digit axis is the radix's own shape: a scalar radix
    // adds no axis, so `2 #: 5` is a scalar.
    assert_eq!(val(Lang::J, "2 2 2 #: 5"), i64s(&[3], &[1, 0, 1]));
    assert_eq!(val(Lang::J, "2 #: 5"), Array::scalar_i64(1));
    assert_eq!(val(Lang::J, "24 60 60 #: 3723"), i64s(&[3], &[1, 2, 3]));
    // A radix of 0 takes whatever is left over.
    assert_eq!(val(Lang::J, "0 0 #: 5"), i64s(&[2], &[0, 5]));
    // A fractional remainder lands in the last digit.
    assert_eq!(val(Lang::J, "2 2 #: 2.5"), f64s(&[2], &[1.0, 0.5]));
    // J frames the digits per atom; APL's `⊤` takes the whole right
    // argument, so the digits become the LEADING axis instead.
    assert_eq!(val(Lang::J, "2 2 2 #: 5 6"), i64s(&[2, 3], &[1, 0, 1, 1, 1, 0]));
    assert_eq!(val(Lang::Apl, "2 2 2⊤5 6"), i64s(&[3, 2], &[1, 1, 0, 1, 1, 0]));
    assert_eq!(val(Lang::Apl, "2 2 2⊤5"), i64s(&[3], &[1, 0, 1]));
    assert_eq!(val(Lang::Apl, "24 60 60⊤3723"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(err(Lang::Apl, "⊤5").kind, ErrorKind::Domain);
}

// --- itemize, laminate, table -------------------------------------------

#[test]
fn itemize_and_laminate() {
    // The monad adds a leading axis of one.
    assert_eq!(val(Lang::J, ",: 1 2 3"), i64s(&[1, 3], &[1, 2, 3]));
    assert_eq!(val(Lang::J, ",: i. 2 3").shape, vec![1, 2, 3]);
    assert_eq!(val(Lang::J, ",: 5"), i64s(&[1], &[5]));
    // The dyad makes the two arguments the items of a new leading axis.
    assert_eq!(val(Lang::J, "1 2 ,: 3 4"), i64s(&[2, 2], &[1, 2, 3, 4]));
    assert_eq!(val(Lang::J, "(i.2 3) ,: i. 2 3").shape, vec![2, 2, 3]);
    // Two atoms become one-element items, so the result has rank 2.
    assert_eq!(val(Lang::J, "1 ,: 2"), i64s(&[2, 1], &[1, 2]));
    // A scalar spreads over the other argument; unequal lists take fill.
    assert_eq!(val(Lang::J, "1 2 3 ,: 4"), i64s(&[2, 3], &[1, 2, 3, 4, 4, 4]));
    assert_eq!(val(Lang::J, "1 2 ,: 3 4 5"), i64s(&[2, 3], &[1, 2, 0, 3, 4, 5]));
    assert_eq!(
        val(Lang::J, "(i.2 3) ,: 1 2 3"),
        i64s(&[2, 2, 3], &[0, 1, 2, 3, 4, 5, 1, 2, 3, 0, 0, 0])
    );
    assert_eq!(val(Lang::J, "'ab' ,: 'cd'"), text(&[2, 2], "abcd"));
    assert_eq!(err(Lang::J, "'ab' ,: 1 2").kind, ErrorKind::Type);
}

/// APL's `⍪` monad: one row per item, holding that item's elements.
#[test]
fn apl_table_makes_a_matrix() {
    assert_eq!(val(Lang::Apl, "⍪1 2 3"), i64s(&[3, 1], &[1, 2, 3]));
    assert_eq!(val(Lang::Apl, "⍪5"), i64s(&[1, 1], &[5]));
    assert_eq!(val(Lang::Apl, "⍪2 3⍴⍳6"), i64s(&[2, 3], &[1, 2, 3, 4, 5, 6]));
    assert_eq!(val(Lang::Apl, "⍴⍪2 3 4⍴⍳24"), i64s(&[2], &[2, 12]));
    assert_eq!(val(Lang::Apl, "⍪'abc'"), text(&[3, 1], "abc"));
    assert_eq!(val(Lang::Apl, "⍴⍪⍳0"), i64s(&[2], &[0, 1]));
}

// --- the diagnostics of what is still missing ---------------------------

#[test]
fn newly_spelled_words_name_what_they_still_lack() {
    let cases = [
        (Lang::J, "{ 1 2", "catalogue"),
        (Lang::J, "e. 1 2", "raze-in"),
        (Lang::J, "2 ;: 'a b'", "sequential machine"),
        (Lang::Apl, "1 2⍋3 1 2", "collation"),
        (Lang::Apl, "1 2⍒3 1 2", "collation"),
        (Lang::J, "2 \": 1.5", "format with a specification"),
        (Lang::J, "+ &. (+/ % #) 1 2", "obverse"),
        (Lang::Apl, "1∘×2", "∘ with a value operand"),
        // Boxes arrived; what they still lack is named too.
        (Lang::J, "/: 1;2", "grading boxed arrays"),
        (Lang::Apl, "⍋⊂1 2", "grading boxed arrays"),
    ];
    for (lang, src, what) in cases {
        let e = err(lang, src);
        assert_eq!(e.kind, ErrorKind::NotYet, "{src}");
        assert!(e.msg.contains(what), "{src}: {}", e.msg);
    }
}
