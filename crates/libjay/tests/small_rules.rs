//! Nine small rules the reference interpreters state and libjay did not
//! follow, each independent of the others.
//!
//! The corpora in tests/corpus/{j,apl}/fuzz_found.txt carry the breadth.
//! This file states one rule per assertion: the rank `,.` never falls
//! below, what an atom means to dyadic `/:`, whose domain an outfix
//! honours, what a single argument means to decode and to partition, how
//! `E.` and `I.` read atoms and boxes, when a complex value is read as a
//! real, and why an empty is acceptable numeric data.

use jay::fmt::{format_array, FmtOpts};
use jay::{compile, Array, Data, Dialect, Error, Lang};
use rstest::rstest;

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

fn apl(src: &str) -> Array {
    val(Lang::Apl, src)
}

fn shown(lang: Lang, src: &str) -> String {
    let opts = if lang == Lang::J { FmtOpts::J } else { FmtOpts::APL };
    format_array(&val(lang, src), &opts).trim_end().to_string()
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

// --- `,.` and the rank it never falls below ------------------------------

/// `,. y` is one row per item, the item raveled along that row, so the
/// answer is a table however low the argument's rank is: an atom has one
/// item whose ravel is one element, and `$ ,. 5` is `1 1`. A rank
/// conjunction makes the atoms the cells, and each of them gains the two
/// axes.
#[rstest]
#[case("$ ,. 5", &[2], &[1, 1])]
#[case("$ ,. 'a'", &[2], &[1, 1])]
#[case("$ ,. <5", &[2], &[1, 1])]
#[case("$ ,. 1 2 3", &[2], &[3, 1])]
#[case("$ ,. i. 2 3", &[2], &[2, 3])]
#[case("$ ,. i. 2 3 4", &[2], &[2, 12])]
#[case("$ ,. i. 0", &[2], &[0, 1])]
#[case("$ ,. i. 0 3", &[2], &[0, 3])]
#[case("$ ,.\"0 (i. 3)", &[3], &[3, 1, 1])]
#[case("$ ,.\"_1 'abc'", &[3], &[3, 1, 1])]
fn ravel_items_never_answers_below_rank_two(
    #[case] src: &str,
    #[case] shape: &[usize],
    #[case] want: &[i64],
) {
    assert_eq!(j(src), i64s(shape, want));
}

/// Only the monad ravels items. Dyadically `,.` is still `,"_1`, which
/// leaves two atoms a two-item list rather than a table.
#[test]
fn the_dyad_of_ravel_items_keeps_its_rank() {
    assert_eq!(j("1 ,. 2"), i64s(&[2], &[1, 2]));
    assert_eq!(j("$ (i. 2 3) ,. (i. 2)"), i64s(&[2], &[2, 4]));
}

// --- dyadic grade: an atom is one item -----------------------------------

/// `x /: y` is `(/: y) { x`: the grade of y indexes the ITEMS of x. An atom
/// holds one item, so the only index it answers is the first — a longer key
/// asks for an item that is not there.
#[rstest]
#[case(Lang::J, "5 /: 1", "5")]
#[case(Lang::J, "5 /: 5", "5")]
#[case(Lang::J, "'a' /: 1", "a")]
#[case(Lang::J, "5 \\: 5", "5")]
fn a_scalar_left_argument_answers_only_the_first_index(
    #[case] lang: Lang,
    #[case] src: &str,
    #[case] want: &str,
) {
    assert_eq!(shown(lang, src), want);
}

#[rstest]
#[case("5 /: 1 2 3")]
#[case("_3 /: 1 2 3")]
#[case("_3 \\: 0.1 0.2 0.3")]
#[case("5 \\: 1 2 3")]
#[case("5 /: 1 2")]
fn a_scalar_left_argument_refuses_a_longer_key(#[case] src: &str) {
    assert!(err(Lang::J, src).msg.contains("out of range"), "{src}");
}

// --- outfix and the operand's domain -------------------------------------

/// An outfix holds its operand to the operand's OWN domain over the whole
/// argument, not only over the pieces it folds: `+` has no meaning for
/// characters, so `_2 +/\. 'ab'` is a domain error although every piece it
/// leaves behind is empty. An operand that does have a meaning for them —
/// `,` or `[` — is untouched.
#[rstest]
#[case("_2 +/\\. 'ab'")]
#[case("_3 +/\\. 'a'")]
#[case("3 +/\\. 'abc'")]
#[case("4 +/\\. 'abc'")]
#[case("0 +/\\. <'abc'")]
#[case("_4 +/\\. (1;2 3)")]
#[case("_1 */\\. 1;2")]
fn an_outfix_refuses_data_its_operand_has_no_meaning_for(#[case] src: &str) {
    // The complaint is the operand's own — the one `+/ 'ab'` makes — so it
    // names the type it cannot add rather than the outfix that asked.
    let kind = err(Lang::J, src).kind;
    assert!(
        matches!(kind, jay::ErrorKind::Type | jay::ErrorKind::Domain),
        "{src} failed with {kind:?}"
    );
}

#[rstest]
#[case("2 ,/\\. 'abc'", "ca")]
#[case("1 [/\\. 'abc'", "baa")]
#[case("0 +/\\ 'abc'", "0 0 0 0")]
#[case("1 +/\\ 'abc'", "abc")]
fn an_outfix_leaves_an_operand_that_does_have_one_alone(
    #[case] src: &str,
    #[case] want: &str,
) {
    assert_eq!(shown(Lang::J, src), want);
}

// --- decode and its single argument --------------------------------------

/// J spreads an ATOM of digits over the radices — `2 7 1 8 #. 123x` reads
/// four 123s — and a one-item LIST is not an atom, so it does not spread.
#[rstest]
#[case("1 2 3 #. 5", 50)]
#[case("2 3 4 #. 1", 17)]
#[case("(,2) #. 5", 5)]
#[case("5 #. 1 2 3", 38)]
fn j_decode_spreads_an_atom_of_digits(#[case] src: &str, #[case] want: i64) {
    assert_eq!(j(src), Array::scalar_i64(want));
}

/// No radix at all weighs nothing, and the zero it answers with is the
/// running total's own — a FLOAT for ordinary numbers, the INTEGER where
/// BOTH sides are boolean, the boolean where either side is exact and the
/// complex where either is complex. `3!:0 ((i. 0) #. 5)` is 8 in the
/// reference, `3!:0 ((0 $ 0) #. 1)` 4, `3!:0 ((i. 0) #. 1)` 8 again — the
/// radices being integers — and `3!:0 ((i. 0) #. 5x)` 1.
#[rstest]
#[case("(i. 0) #. 5", "0", "8")]
#[case("(0 $ 0) #. 1", "0", "4")]
#[case("(i. 0) #. 1", "0", "8")]
#[case("(i. 0) #. 5x", "0", "1")]
#[case("(i. 0) #. 5j1", "0", "16")]
#[case("('') #. 5", "0", "8")]
fn j_decode_with_no_radix_answers_the_running_total(
    #[case] src: &str,
    #[case] value: &str,
    #[case] kind: &str,
) {
    assert_eq!(shown(Lang::J, src), value);
    assert_eq!(shown(Lang::J, &format!("3!:0 ({src})")), kind);
}

#[test]
fn j_decode_refuses_a_one_item_list_of_digits() {
    assert_eq!(err(Lang::J, "1 2 3 #. ,5").kind, jay::ErrorKind::Length);
    // The extended digit keeps its type through the spreading.
    assert_eq!(shown(Lang::J, "2 7 1 8 #. 123x"), "8979");
}

/// APL extends a SINGLE — one element, whatever rank it is written at —
/// along the other argument's axis, and an empty axis on either side weighs
/// nothing at all. A digit axis of some other length is still a length
/// error.
#[rstest]
#[case("1 2 3⊥5", "50")]
#[case("1 2 3⊥,5", "50")]
#[case("1 2 3⊥1 1⍴5", "50")]
#[case("(,2)⊥1 2 3", "11")]
#[case("(1 1⍴2)⊥1 2 3", "11")]
#[case("(2 2⍴2)⊥5", "15 15")]
#[case("5⊥1 2 3", "38")]
#[case("1 2⊥''", "0")]
#[case("(⍳0)⊥5", "0")]
#[case("2⊥⍳0", "0")]
fn apl_decode_extends_a_single(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::Apl, src), want);
}

#[rstest]
#[case("1 2 3⊥4 5")]
#[case("1 2 3⊥2 2⍴5")]
#[case("1 2 3⊥2 3⍴⍳6")]
fn apl_decode_still_refuses_a_digit_axis_of_another_length(#[case] src: &str) {
    assert_eq!(err(Lang::Apl, src).kind, jay::ErrorKind::Length, "{src}");
}

// --- partition and its single flag ---------------------------------------

/// One flag is the flag of every item, so `1⊂1 2 3` opens one partition
/// over the whole vector and `0⊂1 2 3` opens none. Two flags for three
/// items has no such reading and stays a length error.
#[rstest]
#[case("≢1⊂1 2 3", 1)]
#[case("≢(,1)⊂1 2 3", 1)]
#[case("≢2⊂1 2 3", 1)]
#[case("≢0⊂1 2 3", 0)]
#[case("≢1⊂'abc'", 1)]
#[case("≢1 0 1⊂1 2 3", 2)]
#[case("≢1 1 1⊂1 2 3", 1)]
fn partition_extends_a_single_flag(#[case] src: &str, #[case] want: i64) {
    assert_eq!(apl(src), Array::scalar_i64(want));
}

#[test]
fn one_flag_encloses_the_whole_argument() {
    assert_eq!(apl("∊1⊂1 2 3"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(apl("≡1⊂1 2 3"), Array::scalar_i64(2));
    assert_eq!(err(Lang::Apl, "1 2⊂1 2 3").kind, jay::ErrorKind::Length);
}

// --- `E.` and `I.` --------------------------------------------------------

/// `E.` reads an atom as a one-item list on BOTH sides: a pattern of one
/// atom has exactly one place to sit in an argument of one atom, and the
/// answer is the scalar that says whether it does.
#[rstest]
#[case("0 E. 5", 0)]
#[case("1 E. 1", 1)]
#[case("0 E. 0", 1)]
#[case("'a' E. 'a'", 1)]
fn find_reads_two_atoms_as_one_item_lists(#[case] src: &str, #[case] want: i64) {
    let got = j(src);
    assert!(got.shape.is_empty(), "{src} answered shape {:?}", got.shape);
    assert_eq!(shown(Lang::J, src), want.to_string());
}

/// A rank-1 pattern in a rank-0 argument still fits nowhere, and saying so
/// is a rank error rather than an answer.
#[test]
fn find_still_refuses_a_pattern_of_a_higher_rank() {
    assert_eq!(err(Lang::J, "(,0) E. 5").kind, jay::ErrorKind::Rank);
}

/// `I.` orders boxed bounds by the same total order `/:` grades them with,
/// which is the order J defines over boxed values. A boxed bound against an
/// unboxed value has nothing to compare and stays a domain error.
#[rstest]
#[case("(1;2 3) I. (1;2;3)", "0 1 1")]
#[case("(<1) I. (<2)", "1")]
#[case("('a';'b') I. (<'ab')", "2")]
fn interval_index_orders_boxes_by_the_total_order(
    #[case] src: &str,
    #[case] want: &str,
) {
    assert_eq!(shown(Lang::J, src), want);
}

#[test]
fn interval_index_still_refuses_a_boxed_bound_against_a_plain_value() {
    assert_eq!(err(Lang::J, "(1;2) I. 3").kind, jay::ErrorKind::Domain);
}

// --- a complex value with no imaginary part ------------------------------

/// A complex value whose imaginary part is zero is ordered by the real it
/// displays as. The reading is at the USE and not at the making: the value
/// keeps its complex type, which `3!:0` still reports as 16.
#[rstest]
#[case("1 <. j. 0", "0")]
#[case("(j. 0) < 1", "1")]
#[case("1 <. 0j0", "0")]
#[case("3j0 < 4", "1")]
#[case("(3j4 - 3j4) < 1", "1")]
#[case("i. 3j0", "0 1 2")]
#[case("1 2 3 I. 3j0", "2")]
fn a_zero_imaginary_part_is_read_as_the_real(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}

#[rstest]
#[case("3!:0 (j. 0)")]
#[case("3!:0 (3j4 - 3j4)")]
#[case("3!:0 (1 2 + j. 0 0)")]
fn the_value_keeps_the_complex_type_it_was_made_with(#[case] src: &str) {
    assert_eq!(j(src), Array::scalar_i64(16), "{src}");
}

/// A value that really is complex has no order, and saying so is the point
/// of the refusal.
#[test]
fn a_genuine_complex_value_still_has_no_order() {
    assert_eq!(err(Lang::J, "1 <. 3j4").kind, jay::ErrorKind::Domain);
}

// --- an empty of the wrong type ------------------------------------------

/// An EMPTY holds no value of the wrong type, so it is acceptable numeric
/// data wherever numeric data is wanted. An empty BOX is not: J refuses
/// `2 #. 0$<1` where it answers `2 #. ''`.
#[rstest]
#[case(Lang::J, "#. ''", "0")]
#[case(Lang::J, "2 #. ''", "0")]
#[case(Lang::J, "'' #. ''", "0")]
#[case(Lang::J, "i. ''", "0")]
#[case(Lang::Apl, "¯3⊥''", "0")]
fn an_empty_is_acceptable_numeric_data(
    #[case] lang: Lang,
    #[case] src: &str,
    #[case] want: &str,
) {
    assert_eq!(shown(lang, src), want);
}

/// An empty of BOXES holds no element to be the wrong type either, so it
/// is numeric data wherever an empty of characters is. jconsole reads one
/// the same way — `#. 0$<1` is 0 and `A. 0$<1` is 0. The one exception is
/// the DYADIC decode, whose radices have to be of a kind that reads boxes:
/// `2 #. 0$<1` is a domain error there and `(<1) #. 0$<1` is 0.
#[rstest]
#[case("#. 0$<1", "0")]
#[case("i. 0$<1", "0")]
#[case("A. 0$<1", "0")]
#[case("$ #: 0$<1", "0 0")]
fn an_empty_box_is_numeric_data(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}
