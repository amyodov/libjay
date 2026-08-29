//! The correctness blockers a differential sweep against the two reference
//! interpreters turned up.
//!
//! The corpora carry the breadth: tests/corpus/apl/fuzz_found.txt and
//! tests/corpus/j/fuzz_found.txt hold what GNU APL and jconsole answer for
//! every sentence here. This file states the rules those sentences stand
//! for — APL's conformability rule for `⌽` and `⊖`, the two languages'
//! differing neutral cells for `⌈`/`⌊`, the two readings of the nub sieve,
//! and exact integers above 2^53 — plus the one thing no corpus can hold:
//! that a magnitude at the end of the machine range comes back as a value
//! or a diagnostic and never as a panic.

use jay::{compile, Array, Data, Dialect, Error, ErrorKind, Lang};
use rstest::rstest;

fn run(lang: Lang, src: &str) -> Result<Option<Array>, Error> {
    let program = compile(lang, src, &Dialect::default())?;
    program.run(&[], &mut |_: &str| {})
}

fn val(lang: Lang, src: &str) -> Array {
    run(lang, src)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn j(src: &str) -> Array {
    val(Lang::J, src)
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

fn bits(shape: &[usize], values: &[u8]) -> Array {
    Array::new(shape.to_vec(), Data::Bool(values.to_vec().into()))
}

// --- APL: the conformability of `⌽` and `⊖` -------------------------------

/// One axis moves, and x holds one amount for each vector along it: `⍴x` is
/// `⍴y` with that axis removed. `⌽` takes the last axis, `⊖` the leading
/// one, and `[k]` names one outright.
#[rstest]
#[case("1 2⌽2 3⍴⍳6", &[2usize, 3][..], &[2i64, 3, 1, 6, 4, 5][..])]
#[case("1 2 3⊖2 3⍴⍳6", &[2, 3], &[4, 2, 6, 1, 5, 3])]
#[case("1 2 3⌽[1]2 3⍴⍳6", &[2, 3], &[4, 2, 6, 1, 5, 3])]
#[case("1 2 3⌽3 4⍴⍳12", &[3, 4], &[2, 3, 4, 1, 7, 8, 5, 6, 12, 9, 10, 11])]
fn apl_rotate_reads_one_amount_per_vector(
    #[case] src: &str,
    #[case] shape: &[usize],
    #[case] want: &[i64],
) {
    assert_eq!(apl(src), i64s(shape, want));
}

/// A scalar left argument turns every vector by the same amount, and so
/// does the one-item vector the reference accepts as one.
#[rstest]
#[case("2⌽1 2 3", &[3usize][..], &[3i64, 1, 2][..])]
#[case("(1⍴1)⌽2 3⍴⍳6", &[2, 3], &[2, 3, 1, 5, 6, 4])]
#[case("2⌽[1]2 3⍴⍳6", &[2, 3], &[1, 2, 3, 4, 5, 6])]
fn a_scalar_rotate_amount_reaches_every_vector(
    #[case] src: &str,
    #[case] shape: &[usize],
    #[case] want: &[i64],
) {
    assert_eq!(apl(src), i64s(shape, want));
}

/// A scalar rotates to itself, whichever spelling asks.
#[rstest]
#[case("2⌽5")]
#[case("(1⍴1)⌽5")]
#[case("0⊖5")]
fn rotating_a_scalar_answers_the_scalar(#[case] src: &str) {
    assert_eq!(apl(src), Array::scalar_i64(5));
}

/// A left argument that is neither a scalar nor shaped like the axes that
/// remain is refused rather than broadcast into a bigger array. The rank
/// error and the length error are told apart: the ranks first, the lengths
/// only once the ranks agree.
#[rstest]
#[case("0 1 1 0⌽5", ErrorKind::Rank)]
#[case("1 2⌽3 4 5", ErrorKind::Rank)]
#[case("(⍳0)⊖1 2 3", ErrorKind::Rank)]
#[case("(⍳0)⌽5", ErrorKind::Rank)]
#[case("(1 1⍴2)⌽1 2 3", ErrorKind::Rank)]
#[case("(2 2⍴1)⌽2 2⍴⍳4", ErrorKind::Rank)]
#[case("(1 1⍴1)⌽2 3⍴⍳6", ErrorKind::Rank)]
#[case("1 2 3⌽2 3 4⍴⍳24", ErrorKind::Rank)]
#[case("1 2 3⌽2 3⍴⍳6", ErrorKind::Length)]
#[case("1 2⌽3 4⍴⍳12", ErrorKind::Length)]
#[case("(0⍴0)⌽2 3⍴⍳6", ErrorKind::Length)]
#[case("1 2⊖2 3⍴⍳6", ErrorKind::Length)]
#[case("1 2⌽[1]2 3⍴⍳6", ErrorKind::Length)]
#[case("1 2⊖3 4⍴⍳12", ErrorKind::Length)]
fn an_unconformable_rotate_is_refused(#[case] src: &str, #[case] kind: ErrorKind) {
    let e = err(Lang::Apl, src);
    assert_eq!(e.kind, kind, "{src}: {}", e.msg);
    // A shape error names both shapes, which is the diagnostics contract.
    assert!(e.msg.contains("rotate"), "{src}: {}", e.msg);
}

/// J's `|.` keeps its own rule — one amount per AXIS — and is untouched by
/// the APL one.
#[test]
fn j_rotate_still_takes_one_amount_per_axis() {
    assert_eq!(j("1 2 |. i. 3 4"), i64s(&[3, 4], &[6, 7, 4, 5, 10, 11, 8, 9, 2, 3, 0, 1]));
    assert_eq!(j("1 |. 1 2 3"), i64s(&[3], &[2, 3, 1]));
}

// --- the neutral cells of `⌈` and `⌊` over no items -----------------------

/// APL keeps no infinity among the reduce identities: the neutral cell of
/// `⌈` is the low extreme of the representable range and of `⌊` the high
/// one. J's are the infinities. The two are answered from the same table,
/// which reads the language.
#[test]
fn the_extreme_identities_follow_the_language() {
    let big = 1.7976e308;
    assert_eq!(apl("⌈/⍬").to_f64_vec().unwrap(), vec![-big]);
    assert_eq!(apl("⌊/⍬").to_f64_vec().unwrap(), vec![big]);
    assert_eq!(j(">./ i. 0").to_f64_vec().unwrap(), vec![f64::NEG_INFINITY]);
    assert_eq!(j("<./ i. 0").to_f64_vec().unwrap(), vec![f64::INFINITY]);
}

/// The identity is produced once per cell of the result, so a reduction
/// down an empty leading axis answers one for every column.
#[test]
fn an_empty_axis_yields_one_identity_per_cell() {
    let big = 1.7976e308;
    assert_eq!(apl("⌈⌿0 3⍴0").to_f64_vec().unwrap(), vec![-big; 3]);
    assert_eq!(apl("⌊⌿0 3⍴0").to_f64_vec().unwrap(), vec![big; 3]);
    assert_eq!(apl("÷⌿0 3⍴0"), bits(&[3], &[1, 1, 1]));
    assert_eq!(j(">./ 0 3 $ 0").to_f64_vec().unwrap(), vec![f64::NEG_INFINITY; 3]);
}

/// The entries the two languages share are unmoved by the split.
#[rstest]
#[case("+/⍬", 0)]
#[case("×/⍬", 1)]
#[case("∨/⍬", 0)]
#[case("∧/⍬", 1)]
#[case("=/⍬", 1)]
#[case("≠/⍬", 0)]
fn the_shared_identities_are_unchanged(#[case] src: &str, #[case] want: i64) {
    assert_eq!(apl(src).to_i64_vec().unwrap(), vec![want]);
}

// --- the nub sieve --------------------------------------------------------

/// APL's `≠` runs over the ELEMENTS in ravel order and keeps the argument's
/// own shape; J's `~:` runs over ITEMS and answers one bit each. The same
/// argument tells them apart.
#[test]
fn the_nub_sieve_counts_elements_in_apl_and_items_in_j() {
    assert_eq!(apl("≠2 2⍴1 2 1 2"), bits(&[2, 2], &[1, 1, 0, 0]));
    assert_eq!(j("~: 2 2 $ 1 2 1 2"), bits(&[2], &[1, 0]));
    assert_eq!(apl("≠2 3⍴⍳6"), bits(&[2, 3], &[1; 6]));
    assert_eq!(j("~: 2 3 $ i. 6"), bits(&[2], &[1, 1]));
}

/// A scalar has one element and no shape, so the sieve of one is a scalar.
#[test]
fn the_apl_nub_sieve_of_a_scalar_is_a_scalar() {
    assert_eq!(apl("≠5"), Array::scalar_bool(true));
    assert_eq!(apl("⍴≠5"), Array::new(vec![0], Data::I64(Vec::new().into())));
}

/// It compares the way everything else does, within the comparison
/// tolerance, and it reaches into boxes by content.
#[test]
fn the_apl_nub_sieve_uses_the_ordinary_comparison() {
    assert_eq!(apl("≠1 1.0000000000001"), bits(&[2], &[1, 0]));
    assert_eq!(apl("≠(1 2)(1 2)(3)"), bits(&[3], &[1, 0, 1]));
}

// --- integers above 2^53 --------------------------------------------------

/// An APL literal is read as an integer out of its text, so a value a
/// double cannot hold exactly still arrives exact.
#[rstest]
#[case("9223372036854775806", 9223372036854775806)]
#[case("9223372036854775806-9223372036854775800", 6)]
#[case("9223372036854775806+1", 9223372036854775807)]
#[case("3|9223372036854775806", 0)]
#[case("1000000000000000001-1000000000000000000", 1)]
#[case("¯9223372036854775806+9223372036854775806", 0)]
fn an_apl_literal_above_two_to_the_fiftythird_stays_exact(
    #[case] src: &str,
    #[case] want: i64,
) {
    assert_eq!(apl(src).to_i64_vec().unwrap(), vec![want]);
}

/// And it reaches the verbs as an integer, not as a refusal to convert one.
#[test]
fn a_large_literal_reaches_the_verbs_that_take_counts() {
    assert_eq!(apl("(⍳5)|9223372036854775806"), i64s(&[5], &[0, 0, 0, 2, 1]));
    assert_eq!(apl("⍴9223372036854775806↓0"), i64s(&[1], &[0]));
}

/// A rotate or shift amount is reduced modulo the axis before a coordinate
/// joins it, so the ends of the machine range answer rather than overflow.
#[rstest]
#[case("9223372036854775806 |. 1 2 3", &[1i64, 2, 3][..])]
#[case("9223372036854775806 9223372036854775806 |. i. 2 3", &[0, 1, 2, 3, 4, 5])]
#[case("_9223372036854775807 |. 1 2 3", &[3, 1, 2])]
#[case("(<: _9223372036854775807) |. 1 2 3", &[2, 3, 1])]
#[case("9223372036854775806 |.!.0 (1 2 3)", &[0, 0, 0])]
#[case("(<: _9223372036854775807) |.!.0 (1 2 3)", &[0, 0, 0])]
fn an_extreme_rotate_or_shift_amount_answers(#[case] src: &str, #[case] want: &[i64]) {
    assert_eq!(j(src).to_i64_vec().unwrap(), want.to_vec());
}

/// The rest of the family: every place a count the program wrote is turned
/// into an index. Each of these panicked on the arithmetic before the
/// counting moved out of i64 — and a debug panic is a release wrap, so both
/// builds were wrong. What each answers is the reference's business; that
/// none of them takes the process down is this one's.
#[rstest]
#[case("(<: _9223372036854775807) ];.0 i. 3 4")]
#[case("((<: _9223372036854775807) , 0) ];.0 i. 3 4")]
#[case("(9223372036854775806 , 0) ];.0 i. 3 4")]
#[case("(<: _9223372036854775807) ]\\. i. 5")]
#[case("9223372036854775807 ]\\. i. 5")]
#[case("(2 2 ,: (<: _9223372036854775807)) ];.3 i. 4 4")]
#[case("(9223372036854775807 1 ,: 2 2) ];.3 i. 4 4")]
#[case("(1 1 ,: 9223372036854775807 2) ];.3 i. 4 4")]
#[case("9223372036854775806 {. 1 2 3")]
#[case("(<: _9223372036854775807) {. 1 2 3")]
#[case("9223372036854775806 }. 1 2 3")]
#[case("(<: _9223372036854775807) }. 1 2 3")]
fn an_extreme_count_is_a_value_or_a_diagnostic_and_never_a_panic(#[case] src: &str) {
    // Either arm is acceptable; a panic is not, and would fail here.
    let _ = run(Lang::J, src);
}
