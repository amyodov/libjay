//! What the two languages answer where IEEE arithmetic has no value.
//!
//! Neither language leaves the question to the hardware, and they answer it
//! differently. J defines the cases it can — `0 * _` is 0 — and refuses the
//! rest with a NaN error, so an indeterminate quotient never reaches the
//! user as a value. GNU APL has no infinity in its arithmetic at all: `÷0`,
//! `⍟0` and `!¯3` are DOMAIN ERROR, and so is every path that reaches them.
//!
//! Every rule below was probed against jconsole or GNU APL; the breadth is
//! in tests/corpus/{j,apl}/nan.txt and this file states one rule per case.

use jay::fmt::{FmtOpts, format_array};
use jay::{Array, Dialect, ErrorKind, Lang, compile};
use rstest::rstest;

fn run(lang: Lang, src: &str) -> Result<Option<Array>, jay::Error> {
    let program = compile(lang, src, &Dialect::default())?;
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink)
}

fn shown(lang: Lang, src: &str) -> String {
    let opts = match lang {
        Lang::J => FmtOpts::J,
        Lang::Apl => FmtOpts::APL,
    };
    let value = run(lang, src)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"));
    format_array(&value, &opts).trim_end().to_string()
}

fn refusal(lang: Lang, src: &str) -> ErrorKind {
    match run(lang, src) {
        Err(e) => e.kind,
        Ok(v) => panic!("{src:?} was answered with {v:?}, not refused"),
    }
}

// ------------------------------------------------- J: a zero factor wins

/// `0 * _` is 0 in J, and the rule belongs to the FACTOR: it holds for
/// either operand order, for a NaN as readily as an infinity, and through a
/// product reduction. IEEE arithmetic makes all of these a NaN.
#[rstest]
#[case("0 * _", "0")]
#[case("0 * __", "0")]
#[case("_ * 0", "0")]
#[case("__ * 0", "0")]
#[case("0 * _.", "0")]
#[case("1e400 * 0", "0")]
#[case("0 * _ __ _.", "0 0 0")]
#[case("*/ 0 , _", "0")]
#[case("*/ _ 0 __", "0")]
fn a_zero_factor_wins(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}

/// A finite product is untouched by the rule, and the infinities multiply
/// among themselves as they always did.
#[rstest]
#[case("_ * _", "_")]
#[case("__ * __", "_")]
#[case("_ * __", "__")]
#[case("_ * 2", "_")]
#[case("_ * _1", "__")]
#[case("2 * 3", "6")]
#[case("0 * 5", "0")]
fn the_ordinary_products_stand(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}

/// A complex product is four real ones, and each follows the same rule.
/// This is what gives `j. _` its value: `0j1 * _` is `0j_`, because the
/// real part is `_ * 0` and not a NaN.
#[rstest]
#[case("j. _", "0j_")]
#[case("_ * 0j1", "0j_")]
#[case("0j1 * _", "0j_")]
#[case("_ * 1j1", "_j_")]
fn the_rule_reaches_a_complex_product(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}

// --------------------------------------- J: a NaN the arithmetic made

/// jconsole refuses arithmetic whose answer is a NaN it made itself, and
/// names the failure a NaN error. Every one of these is an IEEE NaN.
#[rstest]
#[case("_ - _")]
#[case("__ - __")]
#[case("_ + __")]
#[case("__ + _")]
#[case("1e400 - 1e400")]
#[case("_ % _")]
#[case("__ % _")]
#[case("_ % __")]
#[case("__ % __")]
#[case("2 | _")]
#[case("2 | __")]
#[case("_1 | _")]
#[case("0.5 | _")]
#[case("_ | _")]
#[case("5 #: _")]
#[case("! __")]
#[case("_ ! _")]
#[case("_ ! _1")]
#[case("_ ^. 0")]
#[case("0 ^. 0")]
#[case("_ ^. _")]
#[case("+/ _ , __")]
fn j_refuses_a_nan_it_made(#[case] src: &str) {
    assert_eq!(refusal(Lang::J, src), ErrorKind::Nan, "{src}");
}

/// A NaN the PROGRAM wrote is a value like any other: it travels through
/// the same arithmetic unrefused. The rule is about the operation, not the
/// operand, and telling the two apart is the whole of it.
#[rstest]
#[case("_.", "_.")]
#[case("_. + 1", "_.")]
#[case("_. - _.", "_.")]
#[case("_. * 2", "_.")]
#[case("2 | _.", "_.")]
#[case("! _.", "_.")]
#[case("<. _.", "_.")]
#[case("_ % _.", "_.")]
#[case("_. = _.", "0")]
fn a_written_nan_travels(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}

/// The values J DOES define at the same points, which are what keep the
/// refusal from spreading: a zero modulus never divides, an infinite one
/// has a limit, and division by zero is J's signed infinity.
#[rstest]
#[case("_ | 2", "2")]
#[case("_ | _2", "_")]
#[case("__ | 2", "__")]
#[case("__ | _2", "_2")]
#[case("_ | 0", "0")]
#[case("0 | _", "_")]
#[case("0 | __", "__")]
#[case("_ #: 5", "5")]
#[case("_ % 0", "_")]
#[case("0 % 0", "0")]
#[case("0 % _", "0")]
#[case("_ - __", "_")]
#[case("_ + _", "_")]
fn the_defined_edges_keep_their_values(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}

/// J's factorial overflows to infinity wherever its gamma cannot reach,
/// and refuses one argument alone.
#[rstest]
#[case("! _", "_")]
#[case("! 171", "_")]
#[case("! 1e308", "_")]
#[case("! _1e20", "_")]
#[case("2 ! _", "_")]
#[case("_ ! 2", "0")]
#[case("_ ! 0", "0")]
#[case("_1 ! _", "0")]
fn j_factorial_overflows_to_infinity(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}

/// A negative base under an infinite exponent alternates in sign for ever.
/// jconsole answers only where the magnitude falls to zero and the sign
/// stops mattering, and refuses the rest.
#[rstest]
#[case("_2 ^ __", "0")]
#[case("_0.5 ^ _", "0")]
#[case("0 ^ _", "0")]
#[case("0 ^ __", "_")]
#[case("__ ^ 0", "1")]
#[case("_ ^ 0", "1")]
#[case("_ ^ __", "0")]
#[case("2 ^ __", "0")]
#[case("1 ^ _", "1")]
fn the_infinite_powers_that_have_a_value(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}

#[rstest]
#[case("_1 ^ _")]
#[case("_2 ^ _")]
#[case("_1 ^ __")]
fn a_negative_base_has_no_infinite_power(#[case] src: &str) {
    assert_eq!(refusal(Lang::J, src), ErrorKind::Domain, "{src}");
}

// ----------------------------------------- APL: no infinity in the value

/// GNU APL's `÷0` is a DOMAIN ERROR, exactly as its dyadic `2÷0` is. The
/// monad has to refuse the pair the dyad refuses, and by the same rule
/// through each, reduce and scan, which all arrive at the same step.
#[rstest]
#[case("÷0")]
#[case("÷0 2 4")]
#[case("1÷0")]
#[case("2÷0")]
#[case("2÷2 0")]
#[case("÷¨0 2")]
fn apl_refuses_a_reciprocal_of_zero(#[case] src: &str) {
    assert_eq!(refusal(Lang::Apl, src), ErrorKind::Domain, "{src}");
}

/// `0÷0` is 1 in APL, which is what stops the refusal from swallowing the
/// whole of division.
#[rstest]
#[case("0÷0", "1")]
#[case("0÷2", "0")]
#[case("0÷0 0", "1 1")]
#[case("0 2÷0 2", "1 1")]
#[case("÷2 4", "0.5 0.25")]
fn apl_division_by_zero_that_has_a_value(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::Apl, src), want);
}

/// No logarithm of zero, and no infinite logarithm: a base of 1 has none
/// either.
#[rstest]
#[case("⍟0")]
#[case("⍟0 1 2")]
#[case("2⍟0")]
#[case("1⍟2")]
#[case("1⍟0")]
#[case("1 0⍟2")]
fn apl_refuses_a_logarithm_without_a_value(#[case] src: &str) {
    assert_eq!(refusal(Lang::Apl, src), ErrorKind::Domain, "{src}");
}

/// The two logarithms GNU APL defines where the ratio is a NaN are 1, each
/// of them a base raised to the first power.
#[rstest]
#[case("0⍟0", "1")]
#[case("1⍟1", "1")]
#[case("0⍟2", "0")]
#[case("2⍟8", "3")]
#[case("⍟1", "0")]
fn apl_logarithms_that_have_a_value(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::Apl, src), want);
}

/// A pole of the gamma function and an overflow of it are refused alike.
#[rstest]
#[case("!¯3")]
#[case("!¯1")]
#[case("!¯2 0 2")]
#[case("!171")]
#[case("!1E308")]
#[case("!¨¯3 2")]
fn apl_refuses_a_factorial_without_a_value(#[case] src: &str) {
    assert_eq!(refusal(Lang::Apl, src), ErrorKind::Domain, "{src}");
}

/// Zero has no negative power: `0⋆¯1` is a division by zero under another
/// name, and GNU APL refuses it as it refuses `÷0`.
#[rstest]
#[case("0*¯1")]
#[case("0*¯2")]
fn apl_refuses_a_negative_power_of_zero(#[case] src: &str) {
    assert_eq!(refusal(Lang::Apl, src), ErrorKind::Domain, "{src}");
}

#[rstest]
#[case("0*0", "1")]
#[case("0*2", "0")]
#[case("2*¯100000", "0")]
#[case("!0", "1")]
#[case("!5", "120")]
fn apl_powers_and_factorials_that_have_a_value(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::Apl, src), want);
}

// ------------------------------------------- the rules survive the paths

/// The rules are the verbs', not one path's: a fused sentence, a reduction
/// and a scan each answer what the plain verb does. A kernel that held an
/// infinity of its own would make `0 * _` mean two things.
#[rstest]
#[case("0 * _ + i. 3", "0 0 0")]
#[case("+/ 0 * _ , _ , _", "0")]
#[case("*/ 0 , _ , __", "0")]
#[case("0 * ^ 1e5", "0")]
fn the_rules_hold_under_fusion(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}

/// The same sentence at every width the blockwise kernels have: a value
/// past the block boundary must be read by the same rule as the first one.
#[rstest]
#[case(3)]
#[case(1000)]
#[case(10001)]
fn the_zero_factor_rule_holds_at_every_width(#[case] n: usize) {
    let src = format!("+/ 0 * _ * i. {n}");
    assert_eq!(shown(Lang::J, &src), "0");
}

/// And a refusal past the block boundary is still a refusal.
#[rstest]
#[case(3)]
#[case(1000)]
#[case(10001)]
fn a_made_nan_is_refused_at_every_width(#[case] n: usize) {
    let src = format!("+/ _ - _ * i. {n}");
    assert_eq!(refusal(Lang::J, &src), ErrorKind::Nan, "{src}");
}
