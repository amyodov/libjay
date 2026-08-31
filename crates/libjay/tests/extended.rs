//! Extended-precision integers and rationals end to end: the literal forms,
//! the numeric tower, what stays exact and what falls to float, the display,
//! `x:`, and the boundaries that refuse them.
//!
//! Breadth against the reference lives in tests/corpus/j/extended.txt; this
//! file carries the intent — the exact digits, the exact types, the exact
//! diagnostics. The types are J's; APL has neither, so every case is J.

use jay::fmt::{format_array, FmtOpts};
use jay::{compile, Array, DType, Dialect, ErrorKind, Lang};
use rstest::rstest;

fn run(src: &str) -> Option<Array> {
    let program = compile(Lang::J, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile of {src:?} failed:\n{}", e.render(src)));
    let mut sink = |_: &str| {};
    program
        .run(&[], &mut sink)
        .unwrap_or_else(|e| panic!("run of {src:?} failed:\n{}", program.render_error(&e)))
}

fn val(src: &str) -> Array {
    run(src).unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn shown(src: &str) -> String {
    format_array(&val(src), &FmtOpts::J)
}

fn dtype(src: &str) -> DType {
    val(src).dtype()
}

fn err(src: &str) -> jay::Error {
    let program = match compile(Lang::J, src, &Dialect::default()) {
        Err(e) => return e,
        Ok(p) => p,
    };
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink).expect_err("expected an error")
}

// ------------------------------------------------------------- literals

#[rstest]
#[case("123x", "123")]
#[case("_5x", "_5")]
#[case("0x", "0")]
#[case("123456789012345678901234567890x", "123456789012345678901234567890")]
#[case("1r3", "1r3")]
// A rational is reduced on sight, and its sign lives in the numerator.
#[case("2r6", "1r3")]
#[case("1r_2", "_1r2")]
#[case("_1r2", "_1r2")]
#[case("12r4", "3")]
#[case("2r1", "2")]
// A whole vector takes the widest type any of its words reached.
#[case("1 2 3x", "1 2 3")]
#[case("1r2 1r3 1r6", "1r2 1r3 1r6")]
#[case("1 2 30000000000000000000000x", "1 2 30000000000000000000000")]
fn literals(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(src), want);
}

#[rstest]
#[case("123x", DType::Ext)]
#[case("1 2 3x", DType::Ext)]
#[case("1r3", DType::Rat)]
// The suffix, not the value, decides: `2r1` is a rational that happens to
// be whole, and it stays one.
#[case("2r1", DType::Rat)]
#[case("1 2 3", DType::I64)]
fn a_literal_carries_its_own_type(#[case] src: &str, #[case] want: DType) {
    assert_eq!(dtype(src), want);
}

/// `1x1` is a multiple of e and `1p1` of π; only a bare trailing `x` is the
/// extended suffix, and it takes whole decimal digits alone.
#[rstest]
#[case("1x2", DType::F64)]
#[case("1x1", DType::F64)]
#[case("1p2", DType::F64)]
fn the_e_and_pi_forms_are_untouched(#[case] src: &str, #[case] want: DType) {
    assert_eq!(dtype(src), want);
}

#[rstest]
#[case("1.5x")]
#[case("1e10x")]
#[case("1 2 3.5x")]
fn a_fractional_extended_literal_is_ill_formed(#[case] src: &str) {
    let e = err(src);
    assert_eq!(e.kind, ErrorKind::Parse);
    assert!(e.msg.contains("invalid number"), "{}", e.msg);
}

/// A zero denominator is not a rational: J reads `1r0` as an infinity, and
/// so does libjay — the value leaves the exact types on sight.
#[rstest]
#[case("1r0", "_")]
#[case("_1r0", "__")]
#[case("0r0", "0")]
fn a_zero_denominator_is_an_infinity(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(src), want);
}

// ---------------------------------------------------------- the tower

/// Bool < integer < extended < rational < float < complex. The pair's type
/// is the higher of the two, so an exact operand keeps a computation exact
/// only until a float joins it.
#[rstest]
#[case("1x + 1", DType::Ext)]
#[case("1 + 1x", DType::Ext)]
#[case("1x + 1r2", DType::Rat)]
#[case("1r2 + 1", DType::Rat)]
#[case("1r2 + 1x", DType::Rat)]
#[case("1x + 1.5", DType::F64)]
#[case("1r2 + 0.25", DType::F64)]
#[case("1x + 1j2", DType::Complex)]
#[case("1r2 + 1j2", DType::Complex)]
#[case("123x , 1.5", DType::F64)]
#[case("123x , 1r2", DType::Rat)]
#[case("1r2 , 1.5", DType::F64)]
fn the_numeric_tower(#[case] src: &str, #[case] want: DType) {
    assert_eq!(dtype(src), want, "{src}");
}

/// A rational never falls back down the tower, whatever its value; an
/// extended pair demotes a rational answer only when every value is whole.
#[rstest]
#[case("1r2 - 1r2", DType::Rat)]
#[case("1r2 * 2", DType::Rat)]
#[case("1r2 ^ _2", DType::Rat)]
#[case("4x % 2", DType::Ext)]
#[case("1x % 3", DType::Rat)]
#[case("1 2 3x % 2", DType::Rat)]
#[case("-: 1x", DType::Rat)]
#[case("-: 2x", DType::Ext)]
fn demotion_needs_extended_arguments_and_whole_answers(
    #[case] src: &str,
    #[case] want: DType,
) {
    assert_eq!(dtype(src), want, "{src}");
}

/// The i64 overflow rule is untouched: only an explicitly extended
/// computation stays exact.
#[test]
fn machine_integers_still_widen_to_float_on_overflow() {
    assert_eq!(dtype("9223372036854775807 + 1"), DType::F64);
    assert_eq!(shown("9223372036854775807x + 1"), "9223372036854775808");
}

// ------------------------------------------------------- exact arithmetic

#[rstest]
#[case("! 30x", "265252859812191058636308480000000")]
// An extended length makes the whole generated range exact.
#[case("*/ >: i. 25x", "15511210043330985984000000")]
#[case("2 ^ 100x", "1267650600228229401496703205376")]
#[case("10x ^ 30", "1000000000000000000000000000000")]
#[case("123456789012345678901234567890x + 1", "123456789012345678901234567891")]
#[case("123456789012345678901234567890x * 2", "246913578024691357802469135780")]
#[case("+/ 1 2 3x", "6")]
#[case("2 ! 100x", "4950")]
#[case("50 ! 100x", "100891344545564193334812497256")]
fn big_values_keep_every_digit(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(src), want);
}

#[rstest]
#[case("1r2 + 1r3", "5r6")]
#[case("1r2 - 1r3", "1r6")]
#[case("1r2 * 2r3", "1r3")]
#[case("1r2 % 1r3", "3r2")]
#[case("1r2 ^ 3", "1r8")]
#[case("2 ^ _3x", "1r8")]
#[case("+/ 1r2 1r3 1r6", "1")]
#[case("*/ 1r2 1r3", "1r6")]
#[case("1r2 +. 1r3", "1r6")]
#[case("1r2 *. 1r3", "1")]
#[case("<. 7r2", "3")]
#[case(">. 7r2", "4")]
#[case("<. _7r2", "_4")]
#[case("2x | _7", "1")]
#[case("_2x | 7", "_1")]
#[case("1r2 | 1r3", "1r3")]
fn rational_arithmetic(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(src), want);
}

/// Rounding and the sign answer with a whole number whatever they were
/// given: `<. 7r2` is the extended 3, not the rational one.
#[rstest]
#[case("<. 7r2", DType::Ext)]
#[case(">. 7r2", DType::Ext)]
#[case("* 1r2", DType::Ext)]
#[case("* _5x", DType::Ext)]
fn rounding_and_sign_leave_the_rationals(#[case] src: &str, #[case] want: DType) {
    assert_eq!(dtype(src), want, "{src}");
}

/// Only an exact answer stays exact; everything else widens, exactly as an
/// overflowing machine integer does.
#[rstest]
#[case("%: 9x", DType::Ext)]
#[case("%: 2x", DType::F64)]
#[case("%: 1r4", DType::Rat)]
#[case("%: 1r2", DType::F64)]
#[case("5 %: 32x", DType::Ext)]
#[case("2 %: 8x", DType::F64)]
// An exact root is looked for between whole numbers only.
#[case("3 %: 8r27", DType::F64)]
#[case("2x ^ 0.5", DType::F64)]
#[case("^. 2x", DType::F64)]
#[case("^ 1x", DType::F64)]
#[case("o. 1x", DType::F64)]
#[case("! 1r2", DType::F64)]
fn what_falls_to_float(#[case] src: &str, #[case] want: DType) {
    assert_eq!(dtype(src), want, "{src}");
}

/// A zero divisor answers an infinity, and it stays in the exact types:
/// `% 0x` reports the rational type in the reference too.
#[rstest]
#[case("% 0x", DType::Rat)]
#[case("2x % 0", DType::Rat)]
#[case("x: _", DType::Rat)]
#[case("(3 {. 123x) ^ _2", DType::Rat)]
fn an_exact_infinity_stays_exact(#[case] src: &str, #[case] want: DType) {
    assert_eq!(dtype(src), want, "{src}");
}

/// A negative exact value under a root or a logarithm leaves the reals, as
/// a negative float does.
#[rstest]
#[case("%: _4x", "0j2")]
#[case("%: _1r4", "0j0.5")]
#[case("_2x ^ 1r2", "0j1.41421")]
fn a_negative_exact_value_leaves_the_reals(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(src), want);
}

#[test]
fn division_by_zero_is_an_infinity_not_a_rational() {
    assert_eq!(shown("1x % 0"), "_");
    assert_eq!(shown("0x % 0"), "0");
    assert_eq!(shown("1r2 % 0"), "_");
}

/// A fold hands the whole buffer to every step with a different offset, so
/// an exact pass that read all of it each time would be quadratic. These
/// finish instantly when it reads only the window it uses.
#[test]
fn a_long_exact_reduction_stays_linear() {
    assert_eq!(shown("+/ i. 20000x"), "199990000");
    assert_eq!(shown("+/ 1r2 + i. 2000x"), "2000000");
}

// --------------------------------------------------------- comparison

/// Two exact values compare exactly: no tolerance stands between them, so a
/// difference of one in the thirtieth digit is a difference.
#[test]
fn exact_values_compare_without_tolerance() {
    assert_eq!(shown("(10x^30) = 1 + 10x^30"), "0");
    assert_eq!(shown("(10x^30) < 1 + 10x^30"), "1");
    assert_eq!(shown("1r3 = 1r3"), "1");
    assert_eq!(shown("(1r3 + 1r3 + 1r3) = 1"), "1");
}

/// Against a float the comparison is the float one, tolerance and all —
/// the pair's type is float, and float comparison is what that type does.
#[test]
fn a_float_operand_brings_the_tolerance_back() {
    assert_eq!(shown("1r3 = 0.333333333333333333"), "1");
    assert_eq!(shown("1r3 < 0.333333333333333333"), "0");
}

// --------------------------------------------------------- structure

#[rstest]
#[case("2 2 $ 1 2 3 4x", "1 2\n3 4")]
#[case("3 {. 1 2 3 4 5x", "1 2 3")]
#[case("1 2 3x , 4 5x", "1 2 3 4 5")]
#[case("|. 1 2 3x", "3 2 1")]
#[case("2 |. 1 2 3 4x", "3 4 1 2")]
#[case("1 2 3x #~ 1 0 1", "1 3")]
#[case("$ 1 2 3x", "3")]
#[case("# 1 2 3x", "3")]
fn structure_verbs_carry_exact_elements(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(src), want);
    assert!(val(src).dtype().is_exact() || src.starts_with('$') || src.starts_with('#'));
}

/// A grade orders by value, not by spelling: a big extended integer sorts
/// above a small one however many digits it has, and `2r4` grades where
/// `1r2` does.
#[test]
fn grading_orders_exact_values_by_value() {
    assert_eq!(shown("/: 10x 2x 33x"), "1 0 2");
    assert_eq!(shown("/: 1r2 1r3 1r6"), "2 1 0");
    assert_eq!(shown("/: 1000000000000000000000x 2x"), "1 0");
    assert_eq!(shown("~. 1r2 2r4 1r3"), "1r2 1r3");
    assert_eq!(shown("(10x^30) e. (10x^30) , 1"), "1");
}

#[test]
fn boxes_hold_exact_values() {
    assert_eq!(shown("< 1r2"), "+---+\n|1r2|\n+---+");
    assert_eq!(shown("> < 123x"), "123");
    assert_eq!(dtype("> < 1r3"), DType::Rat);
}

// ---------------------------------------------------------------- x:

#[rstest]
#[case("x: 2", "2")]
#[case("x: 1 2 3", "1 2 3")]
#[case("x: 1.5", "3r2")]
// A float becomes the simplest rational within the comparison tolerance of
// it, which is what makes `x: 0.1` a tenth and not the binary fraction.
#[case("x: 0.1", "1r10")]
#[case("x: _0.5", "_1r2")]
// An integral double is exact: every digit it really holds survives.
#[case("x: 1e30", "1000000000000000019884624838656")]
#[case("x: 1 2 3.5", "1 2 7r2")]
#[case("x: 1r3", "1r3")]
#[case("1 x: 1.5", "3r2")]
#[case("2 x: 1r3", "1 3")]
#[case("2 x: 3.5", "7 2")]
#[case("2 x: 2x", "2 1")]
#[case("2 x: 1 2 3", "1 1\n2 1\n3 1")]
#[case("_1 x: 1r3", "0.333333")]
#[case("_1 x: 1 2 3x", "1 2 3")]
#[case("_2 x: 1r3", "1r3")]
#[case("_2 x: 2", "2")]
fn exact_conversion(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(src), want);
}

#[rstest]
#[case("x: 2", DType::Ext)]
#[case("x: 1.5", DType::Rat)]
#[case("x: 1 2 3.5", DType::Rat)]
#[case("1 x: 1 2 3", DType::Rat)]
#[case("2 x: 1r3", DType::Ext)]
#[case("_1 x: 1 2 3x", DType::I64)]
#[case("_1 x: 1r3", DType::F64)]
#[case("_2 x: 2", DType::I64)]
fn exact_conversion_types(#[case] src: &str, #[case] want: DType) {
    assert_eq!(dtype(src), want, "{src}");
}

#[test]
fn an_extended_value_too_big_for_an_integer_converts_back_as_a_float() {
    assert_eq!(dtype("_1 x: 10x ^ 30"), DType::F64);
}

// ------------------------------------------------------------- refusals

#[test]
fn exact_conversion_refuses_what_it_cannot_read() {
    let e = err("x: 1j2");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("complex"), "{}", e.msg);

    let e = err("x: 'a'");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("character"), "{}", e.msg);

    let e = err("x: _.");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("NaN"), "{}", e.msg);

    let e = err("3 x: 1r3");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("1, 2, _1 or _2"), "{}", e.msg);
}

/// A bignum grows without warning, so the arithmetic refuses a result too
/// large to hold rather than exhausting the machine.
#[test]
fn an_impossible_power_is_refused_by_name() {
    let e = err("2x ^ 1000000000");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("bits"), "{}", e.msg);
}

#[test]
fn arithmetic_on_exact_and_character_data_is_refused() {
    let e = err("1x + 'a'");
    assert_eq!(e.kind, ErrorKind::Type);
    let e = err("1r2 + < 1");
    assert_eq!(e.kind, ErrorKind::Type);
}

// -------------------------------------------------------- the boundaries

/// A fused chain declines the exact types and the general path evaluates
/// it, so the two agree value for value.
#[rstest]
#[case("(1 2 3x + 1) * 2", "4 6 8")]
#[case("(1r2 + 1r3) * 6", "5")]
#[case("+/ (1 2 3x * 1 2 3) + 1", "17")]
fn a_fused_chain_falls_back_and_agrees(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(src), want);
    // The same computation on machine integers is the fusable one; the
    // exact answer must equal it wherever both are representable.
    let plain = src.replace('x', "");
    assert_eq!(shown(&plain), want, "the fusable spelling disagrees");
}

#[test]
fn an_exact_result_prints_the_way_the_language_writes_it() {
    // No `x` suffix on display, and a rational shows its two halves.
    assert_eq!(shown("123x"), "123");
    assert_eq!(shown("\": 1r3"), "1r3");
    assert_eq!(shown("\": 123x"), "123");
    // Columns line up on the widest element, exact values included.
    assert_eq!(shown("2 2 $ 1r2 1r16 1x 100x"), "1r2 1r16\n  1  100");
}
