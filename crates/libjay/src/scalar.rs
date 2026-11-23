//! The scalar arithmetic of the float primitives, one definition each.
//!
//! Every road that computes a float primitive element by element — the
//! interpreter's passes, the fused kernels and the compiled fold steps —
//! takes its arithmetic from here, so the roads cannot drift apart: the
//! contract between them is bit-identity. What a language adds on top of
//! the arithmetic (tolerant comparison, J's NaN and infinity rules, APL's
//! refusals, the type an answer narrows to) stays with the caller; these
//! functions are the IEEE operation each primitive performs once those
//! rules have let a pair through.
//!
//! Each function is `extern "C"`, so compiled code can call it by address.
//! Where a function is one IEEE instruction, or a compare and a select, a
//! code generator emits that instruction sequence instead of the call, and
//! the definition here is what the sequence has to equal.
//!
//! The extrema are an explicit compare and select rather than
//! `f64::min`/`f64::max`, whose answer for two operands that compare equal
//! is unspecified: a negative and a positive zero would come out either
//! way depending on the platform and the compiler.

/// `a + b`.
#[inline(always)]
pub extern "C" fn add(a: f64, b: f64) -> f64 {
    a + b
}

/// `a - b`.
#[inline(always)]
pub extern "C" fn sub(a: f64, b: f64) -> f64 {
    a - b
}

/// `a × b`.
#[inline(always)]
pub extern "C" fn mul(a: f64, b: f64) -> f64 {
    a * b
}

/// J's division `a % b`: IEEE division, except that `0 % 0` is 0. A zero
/// divisor otherwise gives the infinity whose sign is the product of the
/// operands' signs, a negative zero counting as negative.
#[inline(always)]
pub extern "C" fn div(a: f64, b: f64) -> f64 {
    if a == 0.0 && b == 0.0 { 0.0 } else { a / b }
}

/// The smaller of two floats: `a` where `a < b`, otherwise `b`. A tie
/// (`-0` beside `0`) and a NaN on either side answer `b`.
#[inline(always)]
pub extern "C" fn min(a: f64, b: f64) -> f64 {
    if a < b { a } else { b }
}

/// The larger of two floats: `a` where `a > b`, otherwise `b`. A tie and a
/// NaN on either side answer `b`.
#[inline(always)]
pub extern "C" fn max(a: f64, b: f64) -> f64 {
    if a > b { a } else { b }
}

/// [`min`] where a NaN never wins: `a` where `b` is a NaN, otherwise
/// `min(a, b)`. A tie answers `b`.
#[inline(always)]
pub extern "C" fn min_num(a: f64, b: f64) -> f64 {
    if b.is_nan() { a } else { min(a, b) }
}

/// [`max`] where a NaN never wins: `a` where `b` is a NaN, otherwise
/// `max(a, b)`. A tie answers `b`.
#[inline(always)]
pub extern "C" fn max_num(a: f64, b: f64) -> f64 {
    if b.is_nan() { a } else { max(a, b) }
}

/// Whether `a` and `b` are the two zeros, one of each sign: the one pair on
/// which the extrema answer by position rather than by value.
#[inline(always)]
pub fn signed_zero_tie(a: f64, b: f64) -> bool {
    a == b && a.to_bits() != b.to_bits()
}

/// `-a`.
#[inline(always)]
pub extern "C" fn neg(a: f64) -> f64 {
    -a
}

/// The magnitude `|a|`: `-a` where `a < 0`, otherwise `a`. It selects a
/// sign rather than clearing one, so a negative zero, which is not below
/// zero, keeps its sign, and a NaN passes through.
#[inline(always)]
pub extern "C" fn abs(a: f64) -> f64 {
    if a < 0.0 { -a } else { a }
}

/// `a + 1`.
#[inline(always)]
pub extern "C" fn inc(a: f64) -> f64 {
    a + 1.0
}

/// `a - 1`.
#[inline(always)]
pub extern "C" fn dec(a: f64) -> f64 {
    a - 1.0
}

/// `a + a`.
#[inline(always)]
pub extern "C" fn double(a: f64) -> f64 {
    a + a
}

/// `a ÷ 2`.
#[inline(always)]
pub extern "C" fn halve(a: f64) -> f64 {
    a / 2.0
}

/// `a × a`.
#[inline(always)]
pub extern "C" fn square(a: f64) -> f64 {
    a * a
}

/// `1 - a`.
#[inline(always)]
pub extern "C" fn one_minus(a: f64) -> f64 {
    1.0 - a
}

/// `1 ÷ a`. A zero gives the infinity of its own sign.
#[inline(always)]
pub extern "C" fn recip(a: f64) -> f64 {
    1.0 / a
}

/// The square root, correctly rounded.
#[inline(always)]
pub extern "C" fn sqrt(a: f64) -> f64 {
    a.sqrt()
}

/// `e` to the power `a`, as the platform's `exp` computes it.
#[inline(always)]
pub extern "C" fn exp(a: f64) -> f64 {
    a.exp()
}

/// The natural logarithm, as the platform's `log` computes it.
#[inline(always)]
pub extern "C" fn ln(a: f64) -> f64 {
    a.ln()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    const NZ: f64 = -0.0;

    #[rstest]
    #[case(1.0, 2.0, 1.0, 2.0)]
    #[case(2.0, 1.0, 1.0, 2.0)]
    #[case(NZ, 0.0, 0.0, 0.0)]
    #[case(0.0, NZ, NZ, NZ)]
    #[case(f64::NEG_INFINITY, 0.0, f64::NEG_INFINITY, 0.0)]
    #[case(f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::INFINITY)]
    fn extrema_answer_the_right_operand_on_a_tie(
        #[case] a: f64,
        #[case] b: f64,
        #[case] lo: f64,
        #[case] hi: f64,
    ) {
        assert_eq!(min(a, b).to_bits(), lo.to_bits());
        assert_eq!(max(a, b).to_bits(), hi.to_bits());
        assert_eq!(min_num(a, b).to_bits(), lo.to_bits());
        assert_eq!(max_num(a, b).to_bits(), hi.to_bits());
    }

    #[test]
    fn a_nan_is_the_right_operand_of_a_select_and_never_wins_a_num() {
        assert!(min(1.0, f64::NAN).is_nan());
        assert_eq!(min(f64::NAN, 1.0), 1.0);
        assert!(max(1.0, f64::NAN).is_nan());
        assert_eq!(max(f64::NAN, 1.0), 1.0);
        assert_eq!(min_num(1.0, f64::NAN), 1.0);
        assert_eq!(min_num(f64::NAN, 1.0), 1.0);
        assert_eq!(max_num(1.0, f64::NAN), 1.0);
        assert_eq!(max_num(f64::NAN, 1.0), 1.0);
        assert!(min_num(f64::NAN, f64::NAN).is_nan());
    }

    #[test]
    fn magnitude_and_reciprocal_keep_the_sign_of_a_zero() {
        assert_eq!(abs(NZ).to_bits(), NZ.to_bits());
        assert_eq!(abs(-2.5), 2.5);
        assert_eq!(recip(NZ), f64::NEG_INFINITY);
        assert_eq!(recip(0.0), f64::INFINITY);
        assert_eq!(div(0.0, 0.0), 0.0);
        assert_eq!(div(1.0, NZ), f64::NEG_INFINITY);
        assert_eq!(div(-1.0, NZ), f64::INFINITY);
        assert!(div(f64::NAN, 0.0).is_nan());
        assert!(signed_zero_tie(NZ, 0.0));
        assert!(!signed_zero_tie(0.0, 0.0));
        assert!(!signed_zero_tie(1.0, -1.0));
    }
}
