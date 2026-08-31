//! Complex arithmetic on interleaved `[re, im]` pairs.
//!
//! The element type is `[f64; 2]`, which is the layout numpy's `complex128`,
//! C99's `double _Complex` and a pair of Arrow `Float64` children all agree
//! on, so a complex buffer crosses every boundary without a conversion.
//!
//! Functions here are the mathematics only; the languages' type rules and
//! diagnostics live in `verb.rs`.

/// One complex number: `[real, imaginary]`.
pub type Cx = [f64; 2];

pub const ZERO: Cx = [0.0, 0.0];
pub const ONE: Cx = [1.0, 0.0];
pub const I: Cx = [0.0, 1.0];

#[inline]
pub fn from_real(x: f64) -> Cx {
    [x, 0.0]
}

#[inline]
pub fn add(a: Cx, b: Cx) -> Cx {
    [a[0] + b[0], a[1] + b[1]]
}

#[inline]
pub fn sub(a: Cx, b: Cx) -> Cx {
    [a[0] - b[0], a[1] - b[1]]
}

#[inline]
pub fn neg(a: Cx) -> Cx {
    [-a[0], -a[1]]
}

#[inline]
pub fn conj(a: Cx) -> Cx {
    [a[0], -a[1]]
}

/// A complex product is four real ones, and each of them follows J's rule
/// that a zero factor wins: `_ * 0j1` is `0j_` and `0j_ * 0j_` is `__`
/// only when `_ * 0` is 0 rather than a NaN. It is also what gives `j. _`
/// its value, because `j.` multiplies by the imaginary unit. GNU APL never
/// reaches the case — it refuses an infinite operand to `×` outright — so
/// the rule costs nothing there.
#[inline]
fn prod(x: f64, y: f64) -> f64 {
    if (x == 0.0 || y == 0.0) && !(x.is_finite() && y.is_finite()) {
        return 0.0;
    }
    x * y
}

#[inline]
pub fn mul(a: Cx, b: Cx) -> Cx {
    [prod(a[0], b[0]) - prod(a[1], b[1]), prod(a[0], b[1]) + prod(a[1], b[0])]
}

/// Division, with J's rule for a zero divisor carried onto both parts:
/// `0 % 0` is 0 and anything else over zero is a signed infinity.
#[inline]
pub fn div(a: Cx, b: Cx) -> Cx {
    if b[0] == 0.0 && b[1] == 0.0 {
        // An INFINITE part over a zero has no value here, where a finite
        // one is an infinity: `(2j3) % 0` is `_j_` there and `(_j2) % 0`
        // is a NaN error. The reals divide the same infinity happily, so
        // the rule belongs to the complex quotient alone.
        if a[0].is_infinite() || a[1].is_infinite() {
            return [f64::NAN, f64::NAN];
        }
        let step = |x: f64| if x == 0.0 { 0.0 } else { f64::INFINITY.copysign(x) };
        return [step(a[0]), step(a[1])];
    }
    // Smith's scaling keeps the denominator from overflowing. The cross
    // terms multiply as J does — an infinity by a zero is a zero — so a
    // real divisor leaves an infinite part where it found one:
    // `(_j3.14159) % 0.693` is `_j4.53236` and not a NaN.
    let cross = |x: f64, r: f64| if r == 0.0 { 0.0 } else { x * r };
    if b[0].abs() >= b[1].abs() {
        let r = b[1] / b[0];
        let d = b[0] + b[1] * r;
        [(a[0] + cross(a[1], r)) / d, (a[1] - cross(a[0], r)) / d]
    } else {
        let r = b[0] / b[1];
        let d = b[0] * r + b[1];
        [(cross(a[0], r) + a[1]) / d, (cross(a[1], r) - a[0]) / d]
    }
}

#[inline]
pub fn abs(z: Cx) -> f64 {
    z[0].hypot(z[1])
}

/// The argument, in radians; `arg(0)` is 0.
#[inline]
pub fn arg(z: Cx) -> f64 {
    // A negative zero imaginary part would put a real negative value on the
    // lower branch; every value that reaches here as a widened real has to
    // land on the principal one.
    z[1].atan2(z[0])
}

/// `y % | y`: the unit complex in y's direction, and 0 at the origin.
#[inline]
pub fn signum(z: Cx) -> Cx {
    let m = abs(z);
    if m == 0.0 { ZERO } else { [z[0] / m, z[1] / m] }
}

#[inline]
pub fn recip(z: Cx) -> Cx {
    div(ONE, z)
}

/// A real value widened for a function that leaves the reals keeps a
/// positive zero imaginary part, so it lands on the principal branch.
#[inline]
fn principal(z: Cx) -> Cx {
    if z[1] == 0.0 { [z[0], 0.0] } else { z }
}

#[inline]
pub fn exp(z: Cx) -> Cx {
    let m = z[0].exp();
    // An infinite magnitude along an axis stays on it: J multiplies an
    // infinity by a zero to a zero, which is what makes `_ ^ 0.5` the `_`
    // it answers rather than `_j_.`.
    let scale = |c: f64| if c == 0.0 { 0.0 } else { m * c };
    [scale(z[1].cos()), scale(z[1].sin())]
}

/// The principal logarithm; `ln 0` is negative infinity, as on the reals.
#[inline]
pub fn ln(z: Cx) -> Cx {
    let z = principal(z);
    [abs(z).ln(), arg(z)]
}

/// The principal square root, by the algebraic form: `sqrt _4` has to be
/// exactly `0j2`, which halving the argument and taking a cosine does not
/// give.
#[inline]
pub fn sqrt(z: Cx) -> Cx {
    let z = principal(z);
    if z[0] == 0.0 && z[1] == 0.0 {
        return ZERO;
    }
    let t = ((abs(z) + z[0].abs()) / 2.0).sqrt();
    if z[0] >= 0.0 {
        [t, z[1] / (2.0 * t)]
    } else {
        [z[1].abs() / (2.0 * t), t.copysign(z[1])]
    }
}

/// `x ^ y`. An integer exponent is repeated multiplication, which keeps
/// `0j1 ^ 2` exactly `_1` rather than a rounded neighbour of it.
pub fn pow(a: Cx, b: Cx) -> Cx {
    if b[1] == 0.0 && b[0].fract() == 0.0 && b[0].abs() <= 1024.0 {
        let n = b[0] as i64;
        if n == 0 {
            return ONE;
        }
        let mut acc = ONE;
        let mut base = if n < 0 { recip(a) } else { a };
        let mut k = n.unsigned_abs();
        while k > 0 {
            if k & 1 == 1 {
                acc = mul(acc, base);
            }
            base = mul(base, base);
            k >>= 1;
        }
        return acc;
    }
    if a[0] == 0.0 && a[1] == 0.0 {
        return if b[0] == 0.0 && b[1] == 0.0 { ONE } else { ZERO };
    }
    // A negative real raised to a real power turns on cos and sin of a
    // multiple of pi, where the general form rounds `_4 ^ 0.5` to
    // `1.22465e_16j2`. Both references answer `0j2`.
    if a[1] == 0.0 && a[0] < 0.0 && b[1] == 0.0 {
        let m = (-a[0]).powf(b[0]);
        let (c, s) = cos_sin_pi(b[0]);
        // `prod`, not `*`: at a half turn the cosine is an exact zero, and
        // an infinite magnitude beside it is the zero-factor case again.
        // `__ ^ 0.5` is `0j_` and `__ ^ 1.5` is `0j__`.
        return [prod(m, c), prod(m, s)];
    }
    exp(mul(b, ln(a)))
}

/// `(cos pi*t, sin pi*t)`, exact where the true values are 0 and ±1.
fn cos_sin_pi(t: f64) -> (f64, f64) {
    let r = t.rem_euclid(2.0);
    let half_turns = r * 2.0;
    if half_turns.fract() == 0.0 {
        return match half_turns as i64 {
            0 => (1.0, 0.0),
            1 => (0.0, 1.0),
            2 => (-1.0, 0.0),
            _ => (0.0, -1.0),
        };
    }
    let angle = std::f64::consts::PI * r;
    (angle.cos(), angle.sin())
}

/// `x ^. y`: the logarithm of y to base x.
#[inline]
pub fn log(base: Cx, z: Cx) -> Cx {
    div(ln(z), ln(base))
}

/// `x %: y`: the x-th root of y.
#[inline]
pub fn root(x: Cx, y: Cx) -> Cx {
    pow(y, recip(x))
}

/// McDonnell's complex floor: the Gaussian integer at or below y, chosen so
/// that the residue keeps a magnitude below one. Published in the J
/// dictionary's account of `<.`; both references answer with it.
pub fn floor(z: Cx) -> Cx {
    let (bx, by) = (z[0].floor(), z[1].floor());
    let (r, s) = (z[0] - bx, z[1] - by);
    if r + s < 1.0 {
        [bx, by]
    } else if r >= s {
        [bx + 1.0, by]
    } else {
        [bx, by + 1.0]
    }
}

/// The ceiling is the floor reflected through the origin.
#[inline]
pub fn ceil(z: Cx) -> Cx {
    neg(floor(neg(z)))
}

/// `x | y`: y reduced modulo x, with the complex floor doing the rounding.
#[inline]
pub fn residue(x: Cx, y: Cx) -> Cx {
    if x[0] == 0.0 && x[1] == 0.0 {
        return y;
    }
    sub(y, mul(x, floor(div(y, x))))
}

/// The Gaussian-integer greatest common divisor, by Euclid with the nearest
/// Gaussian integer as the quotient. `gcd(0, 0)` is 0.
pub fn gcd(a: Cx, b: Cx) -> Cx {
    let (mut a, mut b) = (a, b);
    // Bounded because each step strictly shrinks |b|; the cap is there so
    // that arguments that are not Gaussian integers stop rather than spin.
    for _ in 0..1024 {
        if b[0] == 0.0 && b[1] == 0.0 {
            return first_quadrant(a);
        }
        let q = div(a, b);
        let rounded = [round_half_away(q[0]), round_half_away(q[1])];
        let r = sub(a, mul(b, rounded));
        if abs(r) >= abs(b) {
            return first_quadrant(b);
        }
        a = b;
        b = r;
    }
    first_quadrant(a)
}

/// A divisor is fixed only up to a unit, so the reference picks one: the
/// associate with a positive real part and a non-negative imaginary one,
/// which is what makes `+.` of two reals the positive divisor as well.
fn first_quadrant(z: Cx) -> Cx {
    let mut z = z;
    for _ in 0..4 {
        if z[0] > 0.0 && z[1] >= 0.0 {
            return z;
        }
        if z[0] == 0.0 && z[1] == 0.0 {
            return ZERO;
        }
        z = mul(I, z);
    }
    z
}

/// `x *. y`: the least common multiple, `(x * y) % gcd`.
#[inline]
pub fn lcm(a: Cx, b: Cx) -> Cx {
    let g = gcd(a, b);
    if g[0] == 0.0 && g[1] == 0.0 { ZERO } else { div(mul(a, b), g) }
}

fn round_half_away(x: f64) -> f64 {
    if x < 0.0 { -(-x + 0.5).floor() } else { (x + 0.5).floor() }
}

// --------------------------------------------------------- transcendentals

/// One factor of a hyperbolic product, where a zero beats an infinity.
/// `1 o. 0j1e10` is `0j_` in jconsole: the sine of the real part is zero
/// and the hyperbolic cosine of the imaginary part has overflowed, and the
/// answer follows the zero rather than the NaN the multiplication makes.
#[inline]
fn scaled(a: f64, b: f64) -> f64 {
    if a == 0.0 && b.is_infinite() { 0.0 } else { a * b }
}

#[inline]
pub fn sin(z: Cx) -> Cx {
    [scaled(z[0].sin(), z[1].cosh()), scaled(z[0].cos(), z[1].sinh())]
}

#[inline]
pub fn cos(z: Cx) -> Cx {
    [scaled(z[0].cos(), z[1].cosh()), -scaled(z[0].sin(), z[1].sinh())]
}

#[inline]
pub fn tan(z: Cx) -> Cx {
    div(sin(z), cos(z))
}

#[inline]
pub fn sinh(z: Cx) -> Cx {
    [scaled(z[1].cos(), z[0].sinh()), scaled(z[1].sin(), z[0].cosh())]
}

#[inline]
pub fn cosh(z: Cx) -> Cx {
    [scaled(z[1].cos(), z[0].cosh()), scaled(z[1].sin(), z[0].sinh())]
}

#[inline]
pub fn tanh(z: Cx) -> Cx {
    div(sinh(z), cosh(z))
}

/// `_1 o. y`: `-i ln(iy + sqrt(1 - y^2))`.
pub fn asin(z: Cx) -> Cx {
    let w = sqrt(sub(ONE, mul(z, z)));
    mul([0.0, -1.0], ln(add(mul(I, z), w)))
}

/// `_2 o. y`: the arcsine's complement.
pub fn acos(z: Cx) -> Cx {
    sub([std::f64::consts::FRAC_PI_2, 0.0], asin(z))
}

/// `_3 o. y`: `(i/2)(ln(1 - iy) - ln(1 + iy))`, the two-logarithm form,
/// which puts the branch cuts where both references put them.
pub fn atan(z: Cx) -> Cx {
    let iz = mul(I, z);
    mul([0.0, 0.5], sub(ln(sub(ONE, iz)), ln(add(ONE, iz))))
}

/// `_5 o. y`: `ln(y + sqrt(y^2 + 1))`.
pub fn asinh(z: Cx) -> Cx {
    ln(add(z, sqrt(add(mul(z, z), ONE))))
}

/// `_6 o. y`: `i * arccos y`.
pub fn acosh(z: Cx) -> Cx {
    mul(I, acos(z))
}

/// `_7 o. y`: `(ln(1 + y) - ln(1 - y)) / 2`, again as two logarithms.
pub fn atanh(z: Cx) -> Cx {
    mul([0.5, 0.0], sub(ln(add(ONE, z)), ln(sub(ONE, z))))
}

/// The unit complex at `degrees`, exact on the quadrant boundaries — both
/// references answer `2ad90` with `0j2`, not with a cosine's rounding of it.
pub fn from_degrees(magnitude: f64, degrees: f64) -> Cx {
    let turn = degrees.rem_euclid(360.0);
    if turn % 90.0 == 0.0 {
        let (c, s) = match (turn / 90.0) as i64 {
            0 => (1.0, 0.0),
            1 => (0.0, 1.0),
            2 => (-1.0, 0.0),
            _ => (0.0, -1.0),
        };
        return [magnitude * c, magnitude * s];
    }
    from_radians(magnitude, degrees * std::f64::consts::PI / 180.0)
}

/// The complex of the given magnitude at the given angle in radians.
#[inline]
pub fn from_radians(magnitude: f64, radians: f64) -> Cx {
    [magnitude * radians.cos(), magnitude * radians.sin()]
}

/// The circle function `k` on a complex argument. `None` for a k the table
/// does not define.
pub fn circle(k: i64, y: Cx) -> Option<Cx> {
    let one_plus_sq = add(ONE, mul(y, y));
    Some(match k {
        0 => sqrt(sub(ONE, mul(y, y))),
        1 => sin(y),
        2 => cos(y),
        3 => tan(y),
        4 => sqrt(one_plus_sq),
        5 => sinh(y),
        6 => cosh(y),
        7 => tanh(y),
        8 => sqrt(neg(one_plus_sq)),
        9 => from_real(y[0]),
        10 => from_real(abs(y)),
        11 => from_real(y[1]),
        12 => from_real(arg(y)),
        -1 => asin(y),
        -2 => acos(y),
        -3 => atan(y),
        -4 => sqrt(sub(mul(y, y), ONE)),
        -5 => asinh(y),
        -6 => acosh(y),
        -7 => atanh(y),
        -8 => neg(sqrt(neg(one_plus_sq))),
        -9 => y,
        -10 => conj(y),
        -11 => mul(I, y),
        -12 => exp(mul(I, y)),
        _ => return None,
    })
}

// ------------------------------------------------------------------ gamma

/// The Lanczos coefficients for g = 7 and nine terms, which give the gamma
/// function to about fifteen significant digits over the whole plane.
const LANCZOS_G: f64 = 7.0;
const LANCZOS: [f64; 9] = [
    0.999_999_999_999_809_9,
    676.520_368_121_885_1,
    -1_259.139_216_722_402_8,
    771.323_428_777_653_1,
    -176.615_029_162_140_6,
    12.507_343_278_686_905,
    -0.138_571_095_265_720_12,
    9.984_369_578_019_572e-6,
    1.505_632_735_149_311_6e-7,
];

/// The gamma function of a complex argument, by the Lanczos
/// approximation. The half-plane left of ½ is reached through the
/// reflection formula, so a pole — a non-positive whole real argument —
/// answers with an infinity rather than a value.
pub fn gamma(z: Cx) -> Cx {
    if z[0] < 0.5 {
        // Γ(z)Γ(1−z) = π ÷ sin πz.
        let s = sin(mul(from_real(std::f64::consts::PI), z));
        return div(from_real(std::f64::consts::PI), mul(s, gamma(sub(ONE, z))));
    }
    let z = sub(z, ONE);
    let mut x = from_real(LANCZOS[0]);
    for (i, &c) in LANCZOS.iter().enumerate().skip(1) {
        x = add(x, div(from_real(c), add(z, from_real(i as f64))));
    }
    let t = add(z, from_real(LANCZOS_G + 0.5));
    let front = pow(t, add(z, from_real(0.5)));
    mul(mul(from_real((2.0 * std::f64::consts::PI).sqrt()), front), mul(exp(neg(t)), x))
}

/// `!z`: the factorial, which is Γ(z+1).
pub fn factorial(z: Cx) -> Cx {
    gamma(add(z, ONE))
}

/// `x!y`: the binomial, Γ(y+1) ÷ Γ(x+1)Γ(y−x+1).
pub fn binomial(x: Cx, y: Cx) -> Cx {
    div(factorial(y), mul(factorial(x), factorial(sub(y, x))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Cx, b: Cx) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    #[test]
    fn multiplication_and_division_are_inverse() {
        let a = [3.0, 4.0];
        let b = [1.0, -2.0];
        assert!(close(div(mul(a, b), b), a));
        assert_eq!(mul([1.0, 2.0], [1.0, -2.0]), [5.0, 0.0]);
    }

    #[test]
    fn dividing_by_zero_follows_the_real_rule_on_both_parts() {
        assert_eq!(div(ZERO, ZERO), ZERO);
        assert_eq!(div(ONE, ZERO), [f64::INFINITY, 0.0]);
        assert_eq!(div(I, ZERO), [0.0, f64::INFINITY]);
    }

    #[test]
    fn square_root_of_a_negative_real_takes_the_principal_branch() {
        assert!(close(sqrt([-4.0, 0.0]), [0.0, 2.0]));
        // A negative zero imaginary part must not flip the branch.
        assert!(close(sqrt([-4.0, -0.0]), [0.0, 2.0]));
    }

    #[test]
    fn an_integer_power_is_exact() {
        assert_eq!(pow(I, [2.0, 0.0]), [-1.0, 0.0]);
        assert_eq!(pow([3.0, 4.0], [2.0, 0.0]), [-7.0, 24.0]);
    }

    #[test]
    fn complex_floor_keeps_the_residue_inside_the_unit_disc() {
        assert_eq!(floor([3.0, 4.0]), [3.0, 4.0]);
        assert_eq!(floor([0.6, 0.8]), [0.0, 1.0]);
        assert_eq!(floor([3.5, 4.5]), [4.0, 4.0]);
        assert!(close(residue([5.0, 0.0], [3.0, 4.0]), [3.0, -1.0]));
    }

    #[test]
    fn the_gamma_function_matches_its_whole_number_values() {
        // Γ(n) is (n−1)! on the positive whole numbers.
        assert!(close(gamma([1.0, 0.0]), ONE));
        assert!(close(gamma([5.0, 0.0]), [24.0, 0.0]));
        assert!(close(factorial([5.0, 0.0]), [120.0, 0.0]));
        // Γ(½) is the square root of π, on both sides of the reflection.
        assert!(close(gamma([0.5, 0.0]), [std::f64::consts::PI.sqrt(), 0.0]));
        assert!(close(gamma([-0.5, 0.0]), [-2.0 * std::f64::consts::PI.sqrt(), 0.0]));
        // The binomial of whole numbers is the count it always was.
        assert!(close(binomial([2.0, 0.0], [5.0, 0.0]), [10.0, 0.0]));
    }

    #[test]
    fn gaussian_gcd_and_lcm() {
        assert!(close(gcd([3.0, 4.0], [1.0, 2.0]), ONE));
        assert!(close(lcm([3.0, 4.0], [1.0, 2.0]), [-5.0, 10.0]));
    }
}
