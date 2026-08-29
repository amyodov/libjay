//! Wave 10: the 0.4.0 audit — pervasive arithmetic over nested arrays, the
//! gamma function in the complex plane, the two glyph-repertoire `⎕CC`
//! classes that are matrices, what index brackets bind to, and the rank
//! `⌹` reads.
//!
//! The corpora in tests/corpus/{j,apl} carry the breadth against the
//! oracles. This file states the rules that are structural rather than
//! numeric: the depth pervasion descends without the call stack, the shape
//! and depth of a pervaded answer, and the refusals.

use jay::{Array, Data, Dialect, Error, ErrorKind, Lang, compile};

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

fn apl(src: &str) -> Array {
    val(Lang::Apl, src)
}

fn err(lang: Lang, src: &str) -> Error {
    match run(lang, src) {
        Err(e) => e,
        Ok(v) => panic!("{src:?} was expected to fail; it answered {v:?}"),
    }
}

fn i64s(shape: &[usize], values: &[i64]) -> Array {
    Array::new(shape.to_vec(), Data::I64(values.to_vec().into()))
}

// --- scalar functions pervade --------------------------------------------

/// A scalar function descends through the boxes and applies at the bottom.
/// The frame is the argument's, the depth is the argument's, and a simple
/// answer at every leaf leaves the result simple again.
#[test]
fn a_scalar_function_descends_through_the_boxes() {
    assert_eq!(apl("⊃1+⊂2 3"), i64s(&[2], &[3, 4]));
    assert_eq!(apl("≡1+⊂2 3"), Array::scalar_i64(2));
    assert_eq!(apl("⍴1+⊂2 3"), i64s(&[0], &[]));
    assert_eq!(apl("∊(1 2)(3 4 5)+10"), i64s(&[5], &[11, 12, 13, 14, 15]));
    assert_eq!(apl("≡1+((1 2)(3 4))((5 6)(7 8))"), Array::scalar_i64(3));
    // The two sides agree by the ordinary scalar rule at EVERY level.
    assert_eq!(apl("∊(1 2)(3 4)×(10 20)(1 2)"), i64s(&[4], &[10, 40, 3, 8]));
    assert_eq!(apl("⍴(2 2⍴⍳4)+⊂1 2"), i64s(&[2], &[2, 2]));
    // A leaf that is not a number is refused where it stands.
    assert_eq!(err(Lang::Apl, "1+⊂'ab'").kind, ErrorKind::Type);
    // J has no such rule: a box there is a type error.
    assert_eq!(err(Lang::J, "1 + < 2 3").kind, ErrorKind::Type);
}

/// Nesting depth is DATA, so the descent runs on a work stack of its own:
/// a value thousands of boxes deep answers instead of taking the process
/// down with the call stack.
#[test]
fn pervasion_descends_without_the_call_stack() {
    assert_eq!(apl("≡1+(⊂⍣2000)1 2"), Array::scalar_i64(2001));
    assert_eq!(apl("≡-(⊂⍣2000)1 2"), Array::scalar_i64(2001));
    assert_eq!(apl("∊1+(⊂⍣2000)1 2"), i64s(&[2], &[2, 3]));
}

// --- the gamma function in the complex plane -----------------------------

/// `!` and `x!y` on a complex argument. The values are the ones both
/// oracles answer; what is asserted here is that the real cases still take
/// the real path and that a complex value with no imaginary part is the
/// real it displays as.
#[test]
fn the_factorial_reaches_the_complex_plane() {
    let parts = |src: &str| match val(Lang::Apl, src).data {
        Data::Complex(v) => v[0],
        Data::F64(v) => [v[0], 0.0],
        Data::I64(v) => [v[0] as f64, 0.0],
        other => panic!("{src}: {other:?}"),
    };
    let close = |src: &str, want: [f64; 2]| {
        let got = parts(src);
        assert!(
            (got[0] - want[0]).abs() < 1e-9 && (got[1] - want[1]).abs() < 1e-9,
            "{src}: {got:?} is not {want:?}"
        );
    };
    close("!2j1", [0.962_865_153_023_787_8, 1.339_097_176_053_257]);
    close("!0j1", [0.498_015_668_118_356_1, -0.154_949_828_301_810_53]);
    close("2j1!5", [13.233_880_477_349_937, 4.411_293_492_449_982]);
    close("2!3j1", [2.5, 2.5]);
    // A complex value with no imaginary part is the real it displays as.
    assert_eq!(apl("!2j0"), Array::scalar_f64(2.0));
    // And the real path is untouched.
    assert_eq!(apl("!5"), Array::scalar_f64(120.0));
}

// --- ⎕CC: the glyph repertoires ------------------------------------------

/// Classes 5, 6, 7 and 9 are fixed tables, and the two frames are matrices
/// rather than vectors.
#[test]
fn the_glyph_repertoires_have_their_own_shapes() {
    assert_eq!(apl("⍴⎕CC 5"), i64s(&[1], &[21]));
    assert_eq!(apl("⍴⎕CC 6"), i64s(&[1], &[20]));
    assert_eq!(apl("⍴⎕CC 7"), i64s(&[2], &[6, 10]));
    assert_eq!(apl("⍴⎕CC 9"), i64s(&[2], &[4, 7]));
    assert_eq!(apl("'╬'∊⎕CC 7"), Array::scalar_bool(true));
    assert_eq!(apl("'ℚ'∊⎕CC 9"), Array::scalar_bool(true));
    assert_eq!(apl("'₇'∊⎕CC 6"), Array::scalar_bool(true));
    // A vector argument still gives one item per class.
    assert_eq!(apl("⍴⎕CC 5 7"), i64s(&[1], &[2]));
}

// --- index brackets bind to the value written before them ----------------

/// `1 2 3[2]` is `1 2` beside `3[2]`, and a scalar has no axis to index.
#[test]
fn index_brackets_take_the_last_number_of_a_run() {
    assert_eq!(err(Lang::Apl, "1 2 3[2]").kind, ErrorKind::Rank);
    assert_eq!(err(Lang::Apl, "1 2 3 [2]").kind, ErrorKind::Rank);
    assert_eq!(err(Lang::Apl, "0.5 1.5[1]").kind, ErrorKind::Rank);
    assert_eq!(apl("(1 2 3)[2]"), Array::scalar_i64(2));
    assert_eq!(apl("1 2 (3 4)[1]"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(apl("A←1 2 3 ⋄ 9 A[2]"), i64s(&[2], &[9, 2]));
    assert_eq!(apl("'abc'[2]"), Array::new(Vec::new(), Data::Char(vec!['b'].into())));
}

// --- ⌹ reads the whole argument ------------------------------------------

/// APL's `⌹` sees the argument as it stands: rank 3 or more is a rank
/// error. J's `%.` has rank 2 and runs over the 2-cells instead.
#[test]
fn matrix_division_in_apl_has_no_rank() {
    assert_eq!(err(Lang::Apl, "⌹2 2 2⍴⍳8").kind, ErrorKind::Rank);
    assert_eq!(err(Lang::Apl, "(2 2⍴⍳4)⌹2 2 2⍴⍳8").kind, ErrorKind::Rank);
    assert_eq!(err(Lang::Apl, "(2 2 2⍴⍳8)⌹2 2⍴⍳4").kind, ErrorKind::Rank);
    assert_eq!(apl("⌹2 2⍴1 0 0 1"), Array::new(vec![2, 2], Data::F64(vec![1.0, 0.0, 0.0, 1.0].into())));
    // J keeps its rank, so the same shape is a run over planes there.
    assert_eq!(val(Lang::J, "$ %. 2 2 2 $ 1 0 0 1 1 0 0 1"), i64s(&[3], &[2, 2, 2]));
}

// --- ⍎ of a program with no value ----------------------------------------

/// `⍎''` reaches no value at all. Every libjay verb answers with one, so
/// the sentence that executed it is refused where it stands.
#[test]
fn executing_the_empty_program_has_no_value() {
    let e = err(Lang::Apl, "⍎''");
    assert_eq!(e.kind, ErrorKind::Value);
    assert!(e.span.is_some(), "the refusal points into the source");
    assert!(e.msg.contains("no value"), "{}", e.msg);
    // J's empty program is an empty VALUE, which is a different rule.
    assert_eq!(val(Lang::J, "$ \". ''"), i64s(&[1], &[0]));
}
