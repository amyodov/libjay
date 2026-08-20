//! Exact numbers: arbitrary-precision integers and rationals.
//!
//! These are the two element types that never round. An [`Ext`] is J's
//! "extended" integer, a `num-bigint` `BigInt`; a [`Rat`] is a pair of them
//! in lowest terms with a positive denominator.
//!
//! Functions here are the arithmetic only, plus the exactness tests the
//! languages' type rules turn on — "is this square exact", "is this power a
//! whole number". Where an operation has no exact answer the function says
//! so (`None`) and the caller widens to float; the type rules and the
//! diagnostics live in `verb.rs`.

use std::cmp::Ordering;
use std::fmt;

use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{FromPrimitive, One, Signed, ToPrimitive, Zero};

/// An extended-precision integer (J `x`).
pub type Ext = BigInt;

/// The largest magnitude a power is allowed to reach, in bits. A bignum
/// power grows without warning — `2 ^ 10000000x` is a gigabyte — so the
/// arithmetic refuses rather than exhausts the machine.
pub const MAX_BITS: u64 = 1 << 26;

/// A rational number in lowest terms; the denominator is always positive
/// and never zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rat {
    num: BigInt,
    den: BigInt,
}

impl Rat {
    /// `num / den`, reduced. None when the denominator is zero: an infinite
    /// rational is not a rational, and the caller answers in floats.
    pub fn new(num: BigInt, den: BigInt) -> Option<Rat> {
        if den.is_zero() {
            return None;
        }
        let mut r = Rat { num, den };
        r.normalize();
        Some(r)
    }

    fn normalize(&mut self) {
        if self.den.sign() == Sign::Minus {
            self.num = -std::mem::take(&mut self.num);
            self.den = -std::mem::take(&mut self.den);
        }
        let g = self.num.gcd(&self.den);
        if !g.is_one() && !g.is_zero() {
            self.num /= &g;
            self.den /= &g;
        }
    }

    pub fn from_int(v: BigInt) -> Rat {
        Rat { num: v, den: BigInt::one() }
    }

    pub fn zero() -> Rat {
        Rat::from_int(BigInt::zero())
    }

    pub fn one() -> Rat {
        Rat::from_int(BigInt::one())
    }

    pub fn numer(&self) -> &BigInt {
        &self.num
    }

    pub fn denom(&self) -> &BigInt {
        &self.den
    }

    pub fn is_zero(&self) -> bool {
        self.num.is_zero()
    }

    pub fn is_integer(&self) -> bool {
        self.den.is_one()
    }

    /// The value as a whole number, when it is one.
    pub fn to_int(&self) -> Option<BigInt> {
        self.is_integer().then(|| self.num.clone())
    }

    pub fn to_f64(&self) -> f64 {
        ratio_to_f64(&self.num, &self.den)
    }

    pub fn neg(&self) -> Rat {
        Rat { num: -self.num.clone(), den: self.den.clone() }
    }

    pub fn abs(&self) -> Rat {
        Rat { num: self.num.abs(), den: self.den.clone() }
    }

    /// -1, 0 or 1 as a whole number, which is the type J answers `*` with.
    pub fn signum(&self) -> BigInt {
        match self.num.sign() {
            Sign::Minus => -BigInt::one(),
            Sign::NoSign => BigInt::zero(),
            Sign::Plus => BigInt::one(),
        }
    }

    /// True when both values are whole, which is the common case an
    /// extended-integer computation carries all the way through.
    fn both_whole(&self, other: &Rat) -> bool {
        self.den.is_one() && other.den.is_one()
    }

    pub fn add(&self, other: &Rat) -> Rat {
        if self.both_whole(other) {
            return Rat::from_int(&self.num + &other.num);
        }
        let mut r = Rat {
            num: &self.num * &other.den + &other.num * &self.den,
            den: &self.den * &other.den,
        };
        r.normalize();
        r
    }

    pub fn sub(&self, other: &Rat) -> Rat {
        if self.both_whole(other) {
            return Rat::from_int(&self.num - &other.num);
        }
        self.add(&other.neg())
    }

    pub fn mul(&self, other: &Rat) -> Rat {
        if self.both_whole(other) {
            return Rat::from_int(&self.num * &other.num);
        }
        let mut r = Rat { num: &self.num * &other.num, den: &self.den * &other.den };
        r.normalize();
        r
    }

    /// Division; None when the divisor is zero.
    pub fn div(&self, other: &Rat) -> Option<Rat> {
        Rat::new(&self.num * &other.den, &self.den * &other.num)
    }

    pub fn recip(&self) -> Option<Rat> {
        Rat::new(self.den.clone(), self.num.clone())
    }

    /// The greatest integer at or below the value.
    pub fn floor(&self) -> BigInt {
        self.num.div_floor(&self.den)
    }

    pub fn ceil(&self) -> BigInt {
        -(-self.num.clone()).div_floor(&self.den)
    }

    /// `self ^ n` for a whole exponent. None when the value is zero and the
    /// exponent negative.
    pub fn pow(&self, n: i64) -> Option<Rat> {
        if n == 0 {
            return Some(Rat::one());
        }
        let k = n.unsigned_abs();
        if k > u32::MAX as u64 {
            return None;
        }
        let k = k as u32;
        let (a, b) = (self.num.magnitude().bits(), self.den.magnitude().bits());
        if a.max(b).saturating_mul(u64::from(k)) > MAX_BITS {
            return None;
        }
        let p = Rat { num: self.num.pow(k), den: self.den.pow(k) };
        if n > 0 { Some(p) } else { p.recip() }
    }

    /// The exact square root, when both halves have one.
    pub fn sqrt(&self) -> Option<Rat> {
        if self.num.sign() == Sign::Minus {
            return None;
        }
        Some(Rat { num: exact_sqrt(&self.num)?, den: exact_sqrt(&self.den)? })
    }
}

impl Ord for Rat {
    fn cmp(&self, other: &Rat) -> Ordering {
        // Both denominators are positive, so cross-multiplying keeps the
        // sense of the comparison.
        (&self.num * &other.den).cmp(&(&other.num * &self.den))
    }
}

impl PartialOrd for Rat {
    fn partial_cmp(&self, other: &Rat) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::hash::Hash for Rat {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.num.hash(state);
        self.den.hash(state);
    }
}

/// The J spelling, with `-` where the language's own negative sign goes:
/// `3r4`, and a whole value as the integer alone.
impl fmt::Display for Rat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_integer() {
            return write!(f, "{}", self.num);
        }
        write!(f, "{}r{}", self.num, self.den)
    }
}

// ------------------------------------------------------------ conversions

/// `num / den` as the nearest double. Dividing the two `to_f64`s overflows
/// as soon as either half leaves the double range, so the ratio is scaled
/// by its own bit lengths first.
pub fn ratio_to_f64(num: &BigInt, den: &BigInt) -> f64 {
    if den.is_zero() {
        return match num.sign() {
            Sign::Minus => f64::NEG_INFINITY,
            Sign::NoSign => 0.0,
            Sign::Plus => f64::INFINITY,
        };
    }
    if let (Some(a), Some(b)) = (num.to_f64(), den.to_f64()) {
        if a.is_finite() && b.is_finite() {
            return a / b;
        }
    }
    // Shift both halves down until they fit, keeping the quotient's value:
    // the same shift on both leaves the ratio unchanged.
    let bits = num.magnitude().bits().max(den.magnitude().bits());
    let shift = bits.saturating_sub(900);
    let scale = |v: &BigInt| (v >> shift as usize).to_f64().unwrap_or(f64::NAN);
    scale(num) / scale(den)
}

pub fn ext_to_f64(v: &BigInt) -> f64 {
    v.to_f64().unwrap_or(match v.sign() {
        Sign::Minus => f64::NEG_INFINITY,
        _ => f64::INFINITY,
    })
}

pub fn ext_to_i64(v: &BigInt) -> Option<i64> {
    v.to_i64()
}

/// A double as an exact integer, when it is one.
pub fn f64_to_ext(x: f64) -> Option<BigInt> {
    if !x.is_finite() || x.fract() != 0.0 {
        return None;
    }
    BigInt::from_f64(x)
}

/// J's comparison tolerance, `2^-44` — the one `x:` reads a float through.
const CT_BITS: usize = 44;

/// A double as the simplest rational within J's comparison tolerance of it:
/// the first continued-fraction convergent that is close enough. An integral
/// value is exact, so `x: 1e30` keeps every digit the double really holds.
pub fn f64_to_rat(x: f64) -> Option<Rat> {
    if !x.is_finite() {
        return None;
    }
    let exact = f64_exact(x)?;
    if exact.is_integer() || x == 0.0 {
        return Some(exact);
    }
    let target = exact.abs();
    let (mut h0, mut h1) = (BigInt::zero(), BigInt::one());
    let (mut k0, mut k1) = (BigInt::one(), BigInt::zero());
    let mut a = target.clone();
    let tolerance = Rat::new(BigInt::one(), BigInt::one() << CT_BITS)?;
    let limit = target.mul(&tolerance);
    loop {
        let n = a.floor();
        let (nh, nk) = (&n * &h1 + &h0, &n * &k1 + &k0);
        h0 = std::mem::replace(&mut h1, nh);
        k0 = std::mem::replace(&mut k1, nk);
        let cand = Rat::new(h1.clone(), k1.clone())?;
        let err = cand.sub(&target).abs();
        if err <= limit {
            return Some(if x < 0.0 { cand.neg() } else { cand });
        }
        let frac = a.sub(&Rat::from_int(n));
        if frac.is_zero() {
            return Some(if x < 0.0 { cand.neg() } else { cand });
        }
        a = frac.recip()?;
    }
}

/// A double as the rational it exactly is: mantissa over a power of two.
pub fn f64_exact(x: f64) -> Option<Rat> {
    if !x.is_finite() {
        return None;
    }
    if x == 0.0 {
        return Some(Rat::zero());
    }
    let bits = x.to_bits();
    let sign = if bits >> 63 == 1 { -1i8 } else { 1 };
    let raw_exp = ((bits >> 52) & 0x7ff) as i64;
    let frac = bits & 0x000f_ffff_ffff_ffff;
    let (mantissa, exp) = if raw_exp == 0 {
        (frac, -1074i64)
    } else {
        (frac | 0x0010_0000_0000_0000, raw_exp - 1075)
    };
    let mut num = BigInt::from(mantissa);
    if sign < 0 {
        num = -num;
    }
    if exp >= 0 {
        Some(Rat::from_int(num << exp as usize))
    } else {
        Rat::new(num, BigInt::one() << (-exp) as usize)
    }
}

// -------------------------------------------------------- exact arithmetic

/// The exact square root, when the argument has one.
pub fn exact_sqrt(v: &BigInt) -> Option<BigInt> {
    if v.sign() == Sign::Minus {
        return None;
    }
    let r = v.sqrt();
    (&r * &r == *v).then_some(r)
}

/// The exact `n`-th root, when the argument has one. `n` must be positive.
pub fn exact_root(n: u32, v: &BigInt) -> Option<BigInt> {
    if n == 0 {
        return None;
    }
    if n == 1 {
        return Some(v.clone());
    }
    if v.sign() == Sign::Minus && n % 2 == 0 {
        return None;
    }
    let r = v.nth_root(n);
    (r.pow(n) == *v).then_some(r)
}

/// `base ^ exp` for a whole nonnegative exponent, refusing a result too big
/// to hold.
pub fn ext_pow(base: &BigInt, exp: u64) -> Option<BigInt> {
    if exp > u32::MAX as u64 {
        return None;
    }
    let bits = base.magnitude().bits();
    if bits.saturating_mul(exp) > MAX_BITS {
        return None;
    }
    Some(base.pow(exp as u32))
}

/// `! y` on a whole number: the exact factorial. None for a negative
/// argument (a pole) or one large enough to exhaust the machine.
pub fn ext_factorial(v: &BigInt) -> Option<BigInt> {
    let n = v.to_u64().filter(|&n| n <= 200_000)?;
    let mut acc = BigInt::one();
    for k in 2..=n {
        acc *= k;
    }
    Some(acc)
}

/// `x ! y` on whole numbers: the number of ways to choose x things from y.
/// Follows J for the degenerate cases — a negative or oversized x gives 0,
/// and `0 ! y` is 1.
pub fn ext_binomial(x: &BigInt, y: &BigInt) -> Option<BigInt> {
    if y.sign() == Sign::Minus {
        // The gamma-function extension takes over below zero.
        return None;
    }
    if x.sign() == Sign::Minus || x > y {
        return Some(BigInt::zero());
    }
    // Choose the smaller of the two factors, so `2 ! 1000000x` is cheap.
    let rest = y - x;
    let k = if *x <= rest { x } else { &rest };
    let k = k.to_u64().filter(|&k| k <= 1_000_000)?;
    let mut acc = BigInt::one();
    for i in 0..k {
        acc = acc * (y - BigInt::from(i)) / BigInt::from(i + 1);
    }
    Some(acc)
}

/// `x | y`: y reduced modulo x, the residue taking x's sign; `0 | y` is y.
pub fn ext_residue(x: &BigInt, y: &BigInt) -> BigInt {
    if x.is_zero() {
        return y.clone();
    }
    let r = y.mod_floor(&x.abs());
    if x.sign() == Sign::Minus && !r.is_zero() { r - x.abs() } else { r }
}

/// `x | y` on rationals: `y - x * <. y % x`, as on the reals.
pub fn rat_residue(x: &Rat, y: &Rat) -> Rat {
    if x.is_zero() {
        return y.clone();
    }
    let q = y.div(x).expect("x is not zero");
    y.sub(&x.mul(&Rat::from_int(q.floor())))
}

/// The greatest common divisor of two rationals: `gcd(numerators) over
/// lcm(denominators)`, which is the largest rational dividing both a whole
/// number of times.
pub fn rat_gcd(a: &Rat, b: &Rat) -> Rat {
    let num = a.num.gcd(&b.num);
    let den = a.den.lcm(&b.den);
    Rat::new(num, den).expect("denominators are positive")
}

/// The least common multiple of two rationals, `lcm(numerators)` over
/// `gcd(denominators)`; zero when either is zero.
pub fn rat_lcm(a: &Rat, b: &Rat) -> Rat {
    if a.is_zero() || b.is_zero() {
        return Rat::zero();
    }
    let num = a.num.lcm(&b.num);
    let den = a.den.gcd(&b.den);
    Rat::new(num, den).expect("denominators are positive")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: i64, d: i64) -> Rat {
        Rat::new(BigInt::from(n), BigInt::from(d)).expect("nonzero denominator")
    }

    #[test]
    fn rationals_are_kept_in_lowest_terms_with_a_positive_denominator() {
        assert_eq!(r(2, 6), r(1, 3));
        assert_eq!(r(1, -2), r(-1, 2));
        assert_eq!(r(6, 3).to_string(), "2");
        assert_eq!(r(-1, 2).to_string(), "-1r2");
        assert!(Rat::new(BigInt::one(), BigInt::zero()).is_none());
    }

    #[test]
    fn rational_arithmetic_is_exact() {
        assert_eq!(r(1, 2).add(&r(1, 3)), r(5, 6));
        assert_eq!(r(1, 2).mul(&r(2, 3)), r(1, 3));
        assert_eq!(r(1, 2).div(&r(1, 3)), Some(r(3, 2)));
        assert_eq!(r(1, 2).pow(-2), Some(r(4, 1)));
        assert_eq!(r(1, 2).sub(&r(1, 2)), Rat::zero());
        assert_eq!(r(1, 4).sqrt(), Some(r(1, 2)));
        assert_eq!(r(1, 2).sqrt(), None);
    }

    #[test]
    fn rationals_order_by_value_however_they_are_spelled() {
        assert!(r(1, 3) < r(1, 2));
        assert!(r(2, 4) == r(1, 2));
        assert_eq!(r(7, 2).floor(), BigInt::from(3));
        assert_eq!(r(7, 2).ceil(), BigInt::from(4));
        assert_eq!(r(-7, 2).floor(), BigInt::from(-4));
    }

    #[test]
    fn residue_takes_the_sign_of_its_left_argument() {
        let e = |v: i64| BigInt::from(v);
        assert_eq!(ext_residue(&e(2), &e(-7)), e(1));
        assert_eq!(ext_residue(&e(-2), &e(7)), e(-1));
        assert_eq!(ext_residue(&e(0), &e(7)), e(7));
        assert_eq!(ext_residue(&e(3), &e(10)), e(1));
    }

    #[test]
    fn exact_roots_are_found_only_where_they_exist() {
        assert_eq!(exact_sqrt(&BigInt::from(9)), Some(BigInt::from(3)));
        assert_eq!(exact_sqrt(&BigInt::from(8)), None);
        assert_eq!(exact_root(5, &BigInt::from(32)), Some(BigInt::from(2)));
        assert_eq!(exact_root(2, &BigInt::from(8)), None);
    }

    #[test]
    fn a_float_becomes_the_simplest_rational_near_it() {
        assert_eq!(f64_to_rat(0.1), Some(r(1, 10)));
        assert_eq!(f64_to_rat(1.5), Some(r(3, 2)));
        assert_eq!(f64_to_rat(-0.5), Some(r(-1, 2)));
        assert_eq!(f64_to_rat(2.0), Some(r(2, 1)));
        // An integral double keeps every digit it really holds.
        assert_eq!(f64_to_rat(1e30).map(|v| v.to_string()), Some("1000000000000000019884624838656".to_string()));
    }

    #[test]
    fn gcd_and_lcm_of_rationals() {
        assert_eq!(rat_gcd(&r(1, 2), &r(1, 3)), r(1, 6));
        assert_eq!(rat_lcm(&r(1, 2), &r(1, 3)), r(1, 1));
        assert_eq!(rat_gcd(&r(5, 1), &r(15, 1)), r(5, 1));
    }

    #[test]
    fn factorials_and_binomials_stay_whole() {
        assert_eq!(
            ext_factorial(&BigInt::from(30)).map(|v| v.to_string()),
            Some("265252859812191058636308480000000".to_string())
        );
        assert_eq!(ext_binomial(&BigInt::from(2), &BigInt::from(5)), Some(BigInt::from(10)));
        assert_eq!(ext_binomial(&BigInt::from(6), &BigInt::from(5)), Some(BigInt::zero()));
        assert_eq!(ext_binomial(&BigInt::from(0), &BigInt::from(5)), Some(BigInt::one()));
    }

    #[test]
    fn a_power_that_would_exhaust_the_machine_is_refused() {
        assert!(ext_pow(&BigInt::from(2), 1000).is_some());
        assert!(ext_pow(&BigInt::from(2), 1 << 30).is_none());
    }

    #[test]
    fn huge_ratios_reach_a_float_without_overflowing() {
        let big = BigInt::from(10).pow(400);
        assert!((ratio_to_f64(&(&big * 3), &big) - 3.0).abs() < 1e-12);
    }
}
