//! Which primitives consult the comparison tolerance, and how.
//!
//! Both languages compare reals loosely — J's `9!:18` tolerance, APL's
//! `⎕CT` — but they do not consult it in the same places, nor read it the
//! same way where they do. Every rule below was probed against jconsole or
//! GNU APL; the breadth is in tests/corpus/{j,apl}/tolerance.txt and this
//! file states one rule per case.
//!
//! The three rules that part the two languages:
//!
//! - `|` rounds the quotient in BOTH, but J answers zero when the product
//!   is tolerantly the dividend, where GNU APL reads the remainder against
//!   the MODULUS instead.
//! - `⌊` and `⌈` scale the gap by the magnitude in J and shift by `⎕CT`
//!   outright in GNU APL.
//! - Grade consults the tolerance in APL and never in J.

use jay::fmt::{format_array, FmtOpts};
use jay::frontend::{EncodeDigits, FloorRule, GcdRule, NearCount};
use jay::{compile, Array, Dialect, Lang};
use rstest::rstest;

fn run(lang: Lang, src: &str, dialect: &Dialect) -> Result<Option<Array>, jay::Error> {
    let program = compile(lang, src, dialect)?;
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink)
}

fn shown_as(lang: Lang, src: &str, dialect: &Dialect) -> String {
    let opts = match lang {
        Lang::J => FmtOpts::J,
        Lang::Apl => FmtOpts::APL,
    };
    let value = run(lang, src, dialect)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"));
    format_array(&value, &opts).trim_end().to_string()
}

fn shown(lang: Lang, src: &str) -> String {
    shown_as(lang, src, &Dialect::default())
}

// ------------------------------------------------------------- residue

/// J's `x | y` is `y - x * <. y % x` with the TOLERANT floor, and the
/// answer is an exact zero wherever the product is tolerantly the dividend.
/// `0.1 | 0.3` is the everyday case: the quotient is 2.9999999999999996, a
/// rounding error below 3.
#[rstest]
#[case("0.1 | 0.3", "0")]
#[case("2 | 4 - 1e_14", "0")]
#[case("2 | 4 + 1e_14", "0")]
#[case("3 | 9 - 1e_14", "0")]
#[case("2 | 2 - 1e_14", "0")]
#[case("0.7 | 2.1", "0")]
#[case("0.3 | 0.9", "0")]
#[case("1e_15 | 1", "0")]
#[case("2 | 1e100", "0")]
// Outside the tolerance the exact remainder stands.
#[case("2 | 4 - 1e_10", "2")]
#[case("0.3 | 0.1", "0.1")]
#[case("_2 | 0.3", "_1.7")]
#[case("0 | 0.3", "0.3")]
#[case("1 | 2.5", "0.5")]
fn j_residue_rounds_the_quotient(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}

/// J's scale is the DIVIDEND's, so a remainder that is small in itself but
/// nowhere near the dividend's magnitude survives. GNU APL reads the same
/// remainder against the modulus and calls it zero — the one place the two
/// languages give different answers to the same sentence.
#[test]
fn the_two_languages_scale_a_small_remainder_differently() {
    assert_eq!(shown(Lang::J, "2 | 1e_14"), "1e_14");
    assert_eq!(shown(Lang::Apl, "2|1E¯14"), "0");
    assert_eq!(shown(Lang::J, "1 | 1e_14"), "1e_14");
    assert_eq!(shown(Lang::Apl, "1|1E¯14"), "0");
}

/// GNU APL's `x|y` rounds the quotient too, and then reads the remainder
/// against the modulus: one within `⎕CT` of the modulus's magnitude is
/// zero, and one that rounding pushed out of `[0, x)` comes back into
/// range.
#[rstest]
#[case("0.1|0.3", "0")]
#[case("2|4-1E¯14", "0")]
#[case("2|4+1E¯14", "0")]
#[case("2|1E¯14", "0")]
#[case("1|¯1E¯14", "0")]
#[case("¯2|1E¯14", "0")]
#[case("0.001|1", "0")]
#[case("1E¯15|1", "0")]
// The modulus sets the scale, so a large enough one swallows the remainder
// outright: `1E14|3` in GNU APL is 0, and `1E13|3` is 3.
#[case("1E20|3", "0")]
#[case("1E20|1E7", "10000000")]
#[case("2|4-1E¯10", "2")]
#[case("0.3|0.1", "0.1")]
#[case("¯2|0.3", "¯1.7")]
fn apl_residue_reads_the_remainder_against_the_modulus(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::Apl, src), want);
}

/// The digits of `#:` and `⊤` are residues, so they round with it. Without
/// this `2 2 #: 4 - 1e_14` answers `1 2` — the un-rounded digit.
#[rstest]
#[case(Lang::J, "2 2 #: 4 - 1e_14", "0 0")]
#[case(Lang::J, "2 2 #: 4 + 1e_14", "0 0")]
#[case(Lang::J, "2 2 2 #: 8 - 1e_14", "0 0 0")]
#[case(Lang::Apl, "2 2⊤4-1E¯14", "0 0")]
#[case(Lang::Apl, "2 2⊤4+1E¯14", "0 0")]
#[case(Lang::Apl, "2 2 2⊤8-1E¯14", "0 0 0")]
fn the_encode_digits_are_residues(#[case] lang: Lang, #[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(lang, src), want);
}

/// The fused kernel answers what the plain path answers: `|` is fusable,
/// and a fused residue that skipped the tolerance would make the same
/// sentence mean two things.
#[test]
fn the_fused_residue_rounds_the_same_way() {
    assert_eq!(shown(Lang::J, "+/ 0.1 | 0.3 0.6 0.9"), "0");
    assert_eq!(shown(Lang::Apl, "+/0.1|0.3 0.6 0.9"), "0");
}

// --------------------------------------------------- floor and ceiling

/// J scales the gap to the integer by the magnitude, so `<. 99.999999999995`
/// is 100 — the gap of 5e¯12 is small BESIDE 100. GNU APL shifts by `⎕CT`
/// outright, so the same value floors to 99, while `⌊¯1E¯13` is 0.
#[rstest]
#[case(Lang::J, "<. 99.999999999995", "100")]
#[case(Lang::J, "<. _1e_14", "_1")]
#[case(Lang::Apl, "⌊99.999999999995", "99")]
#[case(Lang::Apl, "⌊999.99999999999", "999")]
#[case(Lang::Apl, "⌊¯1E¯14", "0")]
#[case(Lang::Apl, "⌊¯1E¯13", "0")]
#[case(Lang::Apl, "⌈1E¯13", "0")]
#[case(Lang::Apl, "⌊9.9999999999999", "10")]
fn the_two_languages_round_to_an_integer_differently(
    #[case] lang: Lang,
    #[case] src: &str,
    #[case] want: &str,
) {
    assert_eq!(shown(lang, src), want);
}

// --------------------------------------------------------------- grade

/// APL's `⍋` and `⍒` compare under `⎕CT`: two keys within the tolerance tie,
/// and the stable sort leaves them in the order they arrived.
#[rstest]
#[case("⍋1 1.0000000000001 1", "1 2 3")]
#[case("⍒1 1.0000000000001 1", "1 2 3")]
#[case("⍋1.0000000000001 1", "1 2")]
#[case("⍒1 1.0000000000001", "1 2")]
#[case("⍋1 1.0000000000001 1 0.9999999999999", "4 1 2 3")]
#[case("⍋3 2 1 1.0000000000001 2.0000000000001", "3 4 2 5 1")]
// Outside the tolerance the order is the values' own.
#[case("⍋1 1.001 1", "1 3 2")]
fn apl_grade_compares_under_the_tolerance(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::Apl, src), want);
}

/// The nested comparator reads it at every level, which is what makes a
/// DESCENDING grade of two items that differ inside the tolerance leave
/// them alone rather than swap them.
#[rstest]
#[case("⍋(1 2)(1 2.0000000000001)", "1 2")]
#[case("⍒(1 2)(1 2.0000000000001)", "1 2")]
#[case("⍋2 2⍴1 2 1 2.0000000000001", "1 2")]
fn the_nested_grade_reads_the_tolerance_too(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::Apl, src), want);
}

/// J's grade is exact, whatever the comparison tolerance is: jconsole
/// answers `/: 1 1.0000000000001 1` with `0 2 1`, the values in their own
/// order, and libjay must not converge on APL's reading here.
#[rstest]
#[case("/: 1 1.0000000000001 1", "0 2 1")]
#[case("\\: 1 1.0000000000001", "1 0")]
#[case("/: 1.0000000000001 1", "1 0")]
#[case("/: 1 , 1 + 2^_50", "0 1")]
fn j_grade_stays_exact(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}

// --------------------------------------------------------- GCD and LCM

/// GNU APL's `∨` hands a zero argument's WHOLE partner back with its sign;
/// a fractional one gives the magnitude, and so does J throughout.
#[rstest]
#[case(Lang::Apl, "¯3∨0", "¯3")]
#[case(Lang::Apl, "0∨¯3", "¯3")]
#[case(Lang::Apl, "¯3.5∨0", "3.5")]
#[case(Lang::Apl, "0∨¯3.5", "3.5")]
#[case(Lang::Apl, "0∧¯3.5", "0")]
#[case(Lang::J, "_3 +. 0", "3")]
#[case(Lang::J, "0 +. _3", "3")]
#[case(Lang::J, "_3.5 +. 0", "3.5")]
fn a_zero_argument_decides_the_gcd_sign(
    #[case] lang: Lang,
    #[case] src: &str,
    #[case] want: &str,
) {
    assert_eq!(shown(lang, src), want);
}

/// And GNU APL rounds before the Euclid runs: an argument within `⎕CT` of a
/// whole number is that number, and one no larger than `⎕CT` beside the
/// other is zero. Without the first, `1.0000000000001∧5` grinds out 5e13.
#[rstest]
#[case("1.0000000000001∧5", "5")]
#[case("1.0000000000001∨1", "1")]
#[case("1.0000000000001∨2", "1")]
#[case("2∨1.0000000000001", "1")]
#[case("1E¯14∨1", "1")]
#[case("1E¯13∨1", "1")]
#[case("1.5∨2.5", "0.5")]
#[case("0.1∨0.2", "0.1")]
fn the_apl_gcd_rounds_its_arguments(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::Apl, src), want);
}

/// Euclid on reals stops when the remainder is a rounding error rather
/// than when it is exactly zero, in both languages and whatever the
/// `gcd_rule`: `0.1+0.2` divides 0.3 because what is left over is one part
/// in 1e16 of it. Grinding on gives 4e¯17 and an LCM of 2.25e15.
#[rstest]
#[case(Lang::J, "0.3 +. 0.1+0.2", "0.3")]
#[case(Lang::J, "0.3 *. 0.1+0.2", "0.3")]
#[case(Lang::J, "0.7 +. 0.1+0.2", "0.1")]
#[case(Lang::J, "3 +. 0.1+0.2", "0.3")]
#[case(Lang::J, "0.2 +. 0.3", "0.1")]
#[case(Lang::J, "2.4 +. 3.6", "1.2")]
#[case(Lang::J, "1.23 +. 4.56", "0.03")]
#[case(Lang::J, "123.456 +. 78.9", "0.012")]
#[case(Lang::Apl, "0.3∧0.1+0.2", "0.3")]
#[case(Lang::Apl, "0.3∨0.1+0.2", "0.3")]
#[case(Lang::Apl, "0.7∨0.1+0.2", "0.1")]
#[case(Lang::Apl, "3∨0.1+0.2", "0.3")]
#[case(Lang::Apl, "1.23∧4.56", "186.96")]
fn a_tolerantly_zero_remainder_ends_the_gcd(
    #[case] lang: Lang,
    #[case] src: &str,
    #[case] want: &str,
) {
    assert_eq!(shown(lang, src), want);
}

/// A value that needs more than twelve significant digits to print back is
/// a rounding residue, not a decimal anyone wrote, so the decimal reading
/// stands aside for it and jconsole's own grind is what comes out.
#[test]
fn a_residue_is_not_read_as_the_decimal_it_prints_as() {
    assert_eq!(shown(Lang::J, "1.0000000000001 +. 1"), "9.99201e_14");
    assert_eq!(shown(Lang::J, "1 +. 1e_13"), "1e_13");
}

/// `gcd_rule` is the knob: Dyalog does none of the three, and its preset
/// says so. J's `+.` grinds the same way and has no knob to turn.
#[test]
fn the_gcd_rule_knob_selects_the_other_reading() {
    let exact = Dialect { gcd_rule: GcdRule::Exact, ..Dialect::default() };
    assert_eq!(shown_as(Lang::Apl, "¯3∨0", &exact), "3");
    assert_eq!(shown_as(Lang::Apl, "1E¯14∨1", &exact), "1e¯14");
    assert_eq!(Dialect::dyalog().gcd_rule, GcdRule::Exact);
    assert_eq!(Dialect::gnu_apl().gcd_rule, GcdRule::Tolerant);
}

// ----------------------------------------------- turning it off, and up

/// The dialect's tolerance reaches every rule above; at zero each of them
/// falls away and the arithmetic is the exact one.
#[test]
fn a_zero_tolerance_makes_every_rule_exact() {
    let exact = Dialect { comparison_tolerance: Some(0.0), ..Dialect::default() };
    assert_eq!(shown_as(Lang::Apl, "0.1|0.3", &exact), "0.1");
    assert_eq!(shown_as(Lang::Apl, "2|4-1E¯14", &exact), "2");
    assert_eq!(shown_as(Lang::Apl, "⍋1 1.0000000000001 1", &exact), "1 3 2");
    assert_eq!(shown_as(Lang::Apl, "2 2⊤4-1E¯14", &exact), "1 2");
    let jexact = Dialect { comparison_tolerance: Some(0.0), ..Dialect::default() };
    assert_eq!(shown_as(Lang::J, "0.1 | 0.3", &jexact), "0.1");
    assert_eq!(shown_as(Lang::J, "2 | 4 - 1e_14", &jexact), "2");
}

/// A larger tolerance widens them all by the same factor.
#[test]
fn a_wider_tolerance_reaches_further() {
    let wide = Dialect { comparison_tolerance: Some(1e-9), ..Dialect::default() };
    assert_eq!(shown_as(Lang::Apl, "2|4-1E¯11", &wide), "0");
    assert_eq!(shown_as(Lang::Apl, "⍋1 1.000000000001 1", &wide), "1 2 3");
    assert_eq!(shown(Lang::Apl, "⍋1 1.000000000001 1"), "1 3 2");
}

/// `⍠('CT' n)` and J's `u!.n` set it for one application. Both are refused
/// on a verb that consults no tolerance, so extending the rule above meant
/// extending the list of verbs that accept them.
#[rstest]
#[case(Lang::J, "0.1 (|!.0) 0.3", "0.1")]
#[case(Lang::J, "2 |!.0 (4 - 1e_14)", "2")]
#[case(Lang::J, "2 2 #:!.0 (4 - 1e_14)", "1 2")]
#[case(Lang::Apl, "0.1(|⍠('CT' 0))0.3", "0.1")]
#[case(Lang::Apl, "(⍋⍠('CT' 0))1 1.0000000000001 1", "1 3 2")]
#[case(Lang::Apl, "2 2(⊤⍠('CT' 0))4-1E¯14", "1 2")]
#[case(Lang::Apl, "¯3(∨⍠('CT' 0))0", "¯3")]
fn a_local_tolerance_reaches_the_new_rules(
    #[case] lang: Lang,
    #[case] src: &str,
    #[case] want: &str,
) {
    assert_eq!(shown(lang, src), want);
}

// -------------------------------------------- the Dyalog readings, by knob

/// `near_count` is where a float merely NEAR a whole number is admitted as
/// a count. GNU APL's window is an absolute `1E¯10`, so a large count buys
/// no room; Dyalog's is relative and follows `⎕CT`, so it is the other way
/// about at every magnitude. Neither is a superset of the other, and the
/// recorded Dyalog answers are what these expectations are.
#[rstest]
#[case("⍴⍳2+9E¯11", Some("2"), None)]
#[case("⍴⍳1E¯11", Some("0"), None)]
#[case("(2+9E¯11)⍴5", Some("5 5"), None)]
#[case("⍴⍳1000000+1E¯9", None, Some("1000000"))]
fn the_near_count_knob_moves_the_admission(
    #[case] src: &str,
    #[case] gnu: Option<&str>,
    #[case] dyalog: Option<&str>,
) {
    let check = |want: Option<&str>, d: &Dialect| match want {
        Some(want) => assert_eq!(shown_as(Lang::Apl, src, d), want),
        None => assert!(run(Lang::Apl, src, d).is_err(), "{src:?} should be refused"),
    };
    check(gnu, &Dialect::gnu_apl());
    check(dyalog, &Dialect::dyalog());
    assert_eq!(Dialect::dyalog().near_count, NearCount::Tolerant);
    assert_eq!(Dialect::gnu_apl().near_count, NearCount::Absolute);
}

/// The admission is not the comparison tolerance under either knob: a
/// count that rounds still compares unequal, and `⎕CT←0` leaves the GNU
/// window exactly where it was.
#[test]
fn the_near_count_is_not_the_comparison_tolerance() {
    assert_eq!(shown(Lang::Apl, "(2+9E¯11)=2"), "0");
    let exact = Dialect { comparison_tolerance: Some(0.0), ..Dialect::default() };
    assert_eq!(shown_as(Lang::Apl, "⍴⍳2+9E¯11", &exact), "2");
}

/// `floor_rule` is the step `⌊` and `⌈` take before rounding. GNU APL's is
/// `⎕CT` outright, so a gap of 5E¯12 is too wide however big the value;
/// Dyalog's grows with the magnitude but never falls below the tolerance
/// itself, which is what keeps `⌊¯1E¯14` at 0.
#[rstest]
#[case("⌊9.9999999999999", "10", "10")]
#[case("⌊999.99999999999", "999", "999")]
#[case("⌊99.999999999995", "99", "99")]
#[case("⌊2.9999999999999", "3", "2")]
#[case("⌈3.0000000000001", "3", "4")]
#[case("⌊¯1E¯14", "0", "0")]
#[case("⌊¯1E¯13", "0", "¯1")]
#[case("⌈1E¯13", "0", "1")]
fn the_floor_rule_knob_scales_the_step(
    #[case] src: &str,
    #[case] gnu: &str,
    #[case] dyalog: &str,
) {
    assert_eq!(shown(Lang::Apl, src), gnu);
    assert_eq!(shown_as(Lang::Apl, src, &Dialect::dyalog()), dyalog);
    assert_eq!(Dialect::dyalog().floor_rule, FloorRule::Scaled);
    assert_eq!(Dialect::gnu_apl().floor_rule, FloorRule::Shift);
}

/// `encode_digits` says whether `⊤` takes its digits with the tolerant
/// residue `|` uses. Dyalog does not, and the untouched remainder shows up
/// in the digits themselves.
#[rstest]
#[case("2 2⊤4-1E¯14", "0 0", "1 2")]
#[case("2 2 2⊤8-1E¯14", "0 0 0", "1 1 2")]
#[case("2 2⊤4-1E¯10", "1 2", "1 2")]
fn the_encode_digits_knob_stops_the_rounding(
    #[case] src: &str,
    #[case] gnu: &str,
    #[case] dyalog: &str,
) {
    assert_eq!(shown(Lang::Apl, src), gnu);
    assert_eq!(shown_as(Lang::Apl, src, &Dialect::dyalog()), dyalog);
    assert_eq!(Dialect::dyalog().encode_digits, EncodeDigits::Exact);
    assert_eq!(Dialect::gnu_apl().encode_digits, EncodeDigits::Tolerant);
}

/// Dyalog's grade reads no tolerance at all — the total array ordering is
/// exact — where the APL2 line ties two keys within `⎕CT`.
#[test]
fn the_dyalog_grade_is_exact() {
    assert_eq!(shown(Lang::Apl, "⍋2 (1+1E¯14) 1"), "2 3 1");
    assert_eq!(shown_as(Lang::Apl, "⍋2 (1+1E¯14) 1", &Dialect::dyalog()), "3 2 1");
}
