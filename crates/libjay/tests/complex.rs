//! Complex numbers end to end, in both languages: the literal forms, the
//! display rule, the arithmetic, the parts, the real operations that now
//! leave the reals, and the places where a complex value is refused.
//!
//! Breadth against the references lives in the corpora (tests/corpus/j
//! and tests/corpus/apl, theme `complex`); this file carries the intent —
//! the exact values, the exact diagnostics, and the fusion fallback.

use jay::fmt::{format_array, FmtOpts};
use jay::{compile, Array, DType, Data, Dialect, ErrorKind, Lang};
use rstest::rstest;

fn run(lang: Lang, src: &str) -> Option<Array> {
    let program = compile(lang, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile of {src:?} failed:\n{}", e.render(src)));
    let mut sink = |_: &str| {};
    program
        .run(&[], &mut sink)
        .unwrap_or_else(|e| panic!("run of {src:?} failed:\n{}", program.render_error(&e)))
}

fn val(lang: Lang, src: &str) -> Array {
    run(lang, src).unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn err(lang: Lang, src: &str) -> jay::Error {
    let program = match compile(lang, src, &Dialect::default()) {
        Err(e) => return e,
        Ok(p) => p,
    };
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink).expect_err("expected an error")
}

fn shown(lang: Lang, src: &str) -> String {
    let fmt = if lang == Lang::J { FmtOpts::J } else { FmtOpts::APL };
    format_array(&val(lang, src), &fmt)
}

/// The complex elements of a result, which must be complex.
fn parts(lang: Lang, src: &str) -> Vec<[f64; 2]> {
    let a = val(lang, src);
    assert_eq!(a.dtype(), DType::Complex, "{src} is {}", a.dtype().name());
    a.as_complex_slice().expect("complex data").to_vec()
}

/// One complex result, to within a tolerance a 6-digit printer cannot see.
fn close(lang: Lang, src: &str, want: [f64; 2]) {
    let got = parts(lang, src);
    assert_eq!(got.len(), 1, "{src} is not a single value");
    let d = ((got[0][0] - want[0]).powi(2) + (got[0][1] - want[1]).powi(2)).sqrt();
    assert!(d < 1e-9, "{src}: got {:?}, want {want:?}", got[0]);
}

// --- literals -------------------------------------------------------------

#[rstest]
#[case(Lang::J, "3j4", [3.0, 4.0])]
#[case(Lang::J, "_1j_2", [-1.0, -2.0])]
#[case(Lang::J, "1j0", [1.0, 0.0])]
#[case(Lang::J, "2j0.5", [2.0, 0.5])]
#[case(Lang::J, "1e1j2", [10.0, 2.0])]
#[case(Lang::Apl, "3J4", [3.0, 4.0])]
#[case(Lang::Apl, "¯1J¯2", [-1.0, -2.0])]
#[case(Lang::Apl, "0.5J0.25", [0.5, 0.25])]
fn rectangular_literals(#[case] lang: Lang, #[case] src: &str, #[case] want: [f64; 2]) {
    close(lang, src, want);
}

/// J's polar forms: `ad` takes the angle in degrees, `ar` in radians. The
/// quadrant boundaries are exact — `2ad90` is `0j2`, not a cosine's
/// rounding of it.
#[rstest]
#[case("2ad90", [0.0, 2.0])]
#[case("1ad180", [-1.0, 0.0])]
#[case("1ad270", [0.0, -1.0])]
#[case("3ad_90", [0.0, -3.0])]
#[case("1ad45", [std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2])]
#[case("1ar1", [0.540_302_305_868_139_7, 0.841_470_984_807_896_5])]
#[case("2ar0", [2.0, 0.0])]
fn polar_literals(#[case] src: &str, #[case] want: [f64; 2]) {
    close(Lang::J, src, want);
}

/// The exponent letters bind loosest, so `1ar1p1` is the polar value
/// scaled by π and `1p1j1` is π raised to a complex power.
#[test]
fn a_polar_literal_can_carry_an_exponent() {
    close(Lang::J, "2ar1p1", [3.394_819_509_665_946_4, 5.287_118_128_162_912]);
    close(Lang::J, "1p1j1", [1.298_395_475_731_348_5, 2.860_729_555_496_241_5]);
}

// --- display --------------------------------------------------------------

/// A value whose imaginary part is exactly zero prints as its real part.
/// Only the DISPLAY demotes: the array is still complex, which is what
/// J's `3!:0` reports of it.
#[test]
fn a_zero_imaginary_part_prints_as_a_real() {
    assert_eq!(shown(Lang::J, "1j0"), "1");
    assert_eq!(shown(Lang::J, "(1j2)*(1j_2)"), "5");
    assert_eq!(val(Lang::J, "1j0").dtype(), DType::Complex);
    assert_eq!(val(Lang::J, "(1j2)*(1j_2)").dtype(), DType::Complex);
    // Element by element, so one real value in a complex vector loses its
    // `j0` while its neighbours keep theirs.
    assert_eq!(shown(Lang::J, "1j0 2j3"), "1 2j3");
    assert_eq!(shown(Lang::J, "3j4 0j0 1"), "3j4 0 1");
}

#[test]
fn each_language_spells_the_parts_its_own_way() {
    assert_eq!(shown(Lang::J, "3j4"), "3j4");
    assert_eq!(shown(Lang::J, "_1j_2"), "_1j_2");
    assert_eq!(shown(Lang::Apl, "3J4"), "3J4");
    assert_eq!(shown(Lang::Apl, "¯1J¯2"), "¯1J¯2");
}

#[test]
fn a_complex_matrix_aligns_its_columns() {
    assert_eq!(shown(Lang::J, "2 2 $ 3j4 1 2.5j_1 100"), "   3j4   1\n2.5j_1 100");
}

// --- arithmetic -----------------------------------------------------------

#[rstest]
#[case("3j4 + 1j1", [4.0, 5.0])]
#[case("3j4 - 1j1", [2.0, 3.0])]
#[case("3j4 * 2", [6.0, 8.0])]
#[case("0j1 * 0j1", [-1.0, 0.0])]
#[case("3j4 % 2j1", [2.0, 1.0])]
#[case("% 0j1", [0.0, -1.0])]
#[case("+ 3j4", [3.0, -4.0])]
#[case("- 3j4", [-3.0, -4.0])]
#[case("* 3j4", [0.6, 0.8])]
#[case("3j4 ^ 2", [-7.0, 24.0])]
#[case("0j1 ^ 2", [-1.0, 0.0])]
#[case("^. 0j1", [0.0, std::f64::consts::FRAC_PI_2])]
#[case("%: 3j4", [2.0, 1.0])]
#[case("<. 3.5j4.5", [4.0, 4.0])]
#[case(">. 3.5j4.5", [3.0, 5.0])]
#[case("<. 0.6j0.8", [0.0, 1.0])]
#[case("5 | 3j4", [3.0, -1.0])]
#[case("3j4 | 2", [-2.0, 3.0])]
#[case("0 | 3j4", [3.0, 4.0])]
fn complex_arithmetic(#[case] src: &str, #[case] want: [f64; 2]) {
    close(Lang::J, src, want);
}

/// Magnitude leaves the complex domain again: `| 3j4` is the float 5.
#[test]
fn magnitude_is_a_float() {
    let a = val(Lang::J, "| 3j4");
    assert_eq!(a.dtype(), DType::F64);
    assert_eq!(a.as_f64_slice(), Some(&[5.0][..]));
    assert_eq!(shown(Lang::Apl, "|¯3J4"), "5");
}

/// `+.` and `*.` on a complex value are the Gaussian-integer divisor and
/// multiple, and their answer is the associate in the first quadrant.
#[test]
fn gcd_and_lcm_are_the_gaussian_ones() {
    close(Lang::J, "3j4 +. 1j1", [1.0, 0.0]);
    close(Lang::J, "3j4 *. 1j2", [-5.0, 10.0]);
    close(Lang::J, "_2j_2 +. 4", [2.0, 2.0]);
    close(Lang::J, "1j1 +. 1j_1", [1.0, 1.0]);
}

// --- the reals that leave the reals --------------------------------------

/// `%:`, `^.`, `^` and the circle functions answer in complex where the
/// reals have no answer — and the whole pass widens, so one negative value
/// in a vector makes the result complex.
#[rstest]
#[case(Lang::J, "%: _4", [0.0, 2.0])]
#[case(Lang::J, "%: _1", [0.0, 1.0])]
#[case(Lang::J, "^. _1", [0.0, std::f64::consts::PI])]
#[case(Lang::J, "_1 ^ 0.5", [0.0, 1.0])]
#[case(Lang::J, "_4 ^ 0.5", [0.0, 2.0])]
#[case(Lang::J, "3 %: _8", [1.0, 1.732_050_807_568_877_2])]
#[case(Lang::J, "2 ^. _8", [3.0, 4.532_360_141_827_193])]
#[case(Lang::Apl, "(¯4)*0.5", [0.0, 2.0])]
#[case(Lang::Apl, "⍟¯1", [0.0, std::f64::consts::PI])]
fn a_real_argument_with_no_real_answer(
    #[case] lang: Lang,
    #[case] src: &str,
    #[case] want: [f64; 2],
) {
    close(lang, src, want);
}

#[test]
fn one_complex_element_widens_the_whole_result() {
    assert_eq!(parts(Lang::J, "%: _4 9"), vec![[0.0, 2.0], [3.0, 0.0]]);
    assert_eq!(shown(Lang::J, "%: _4 9"), "0j2 3");
}

/// An integer exponent keeps a negative base real, as it does in both
/// references: `_1 ^ 2` is 1, not `1j0`.
#[test]
fn an_integer_exponent_stays_real() {
    for src in ["_1 ^ 2", "_1 ^ _1", "%: 9", "^. 1", "2 %: 9"] {
        assert_ne!(val(Lang::J, src).dtype(), DType::Complex, "{src}");
    }
    assert_eq!(val(Lang::J, "_1 ^ 2").to_f64_vec(), Some(vec![1.0]));
    assert_eq!(val(Lang::J, "_1 ^ _1").to_f64_vec(), Some(vec![-1.0]));
}

// --- the parts, j. and r. -------------------------------------------------

#[test]
fn the_parts_come_out_as_a_trailing_axis() {
    assert_eq!(val(Lang::J, "+. 3j4"), Array::from_f64(vec![3.0, 4.0]));
    // The parts of a WHOLE number are whole, which is what `3!:0 (+. 3)`
    // reports as 4 in jconsole and what keeps a large integer exact.
    assert_eq!(val(Lang::J, "+. 3"), Array::from_i64(vec![3, 0]));
    // Rank 0, so a vector argument gains an axis rather than losing one.
    let m = val(Lang::J, "+. 3 4");
    assert_eq!(m.shape, vec![2, 2]);
    assert_eq!(m.to_f64_vec(), Some(vec![3.0, 0.0, 4.0, 0.0]));
    // `*.` is the polar pair: magnitude then angle.
    let p = val(Lang::J, "*. 3j4");
    assert_eq!(p.as_f64_slice().expect("floats")[0], 5.0);
    assert!((p.as_f64_slice().expect("floats")[1] - 0.927_295_218_001_612_2).abs() < 1e-12);
}

#[rstest]
#[case("j. 3", [0.0, 3.0])]
#[case("3 j. 4", [3.0, 4.0])]
#[case("r. 0", [1.0, 0.0])]
#[case("2 r. 0", [2.0, 0.0])]
fn j_and_r_build_complex_numbers(#[case] src: &str, #[case] want: [f64; 2]) {
    close(Lang::J, src, want);
}

// --- circle functions -----------------------------------------------------

/// 9 to 12 read a number's parts: real, magnitude, imaginary, phase. They
/// answer a real argument too, which is why they are no longer a gap.
#[rstest]
#[case(Lang::J, "9 o. 3j4", 3.0)]
#[case(Lang::J, "10 o. 3j4", 5.0)]
#[case(Lang::J, "11 o. 3j4", 4.0)]
#[case(Lang::J, "12 o. 3j4", 0.927_295_218_001_612_2)]
#[case(Lang::J, "9 o. 3", 3.0)]
#[case(Lang::J, "11 o. 3", 0.0)]
#[case(Lang::J, "10 o. _3", 3.0)]
#[case(Lang::J, "12 o. _3", std::f64::consts::PI)]
#[case(Lang::Apl, "9○3J4", 3.0)]
#[case(Lang::Apl, "11○3J4", 4.0)]
#[case(Lang::Apl, "10○3J4", 5.0)]
#[case(Lang::Apl, "12○3J4", 0.927_295_218_001_612_2)]
fn the_parts_circle_functions(#[case] lang: Lang, #[case] src: &str, #[case] want: f64) {
    let a = val(lang, src);
    let got = a.to_f64_vec().unwrap_or_else(|| panic!("{src} is not real: {a:?}"));
    assert!((got[0] - want).abs() < 1e-12, "{src}: {got:?}");
}

#[rstest]
#[case("1 o. 3j4", [3.853_738_037_919_37, -27.016_813_258_003_932])]
#[case("2 o. 3j4", [-27.034_945_603_074_224, -3.851_153_334_811_777_6])]
#[case("5 o. 3j4", [-6.548_120_040_911_001, -7.619_231_720_321_411])]
#[case("6 o. 3j4", [-6.580_663_040_551_157, -7.581_552_742_746_545])]
#[case("_1 o. 3j4", [0.633_983_865_639_176_6, 2.305_509_031_243_477])]
#[case("_2 o. 3j4", [0.936_812_461_155_719_9, -2.305_509_031_243_477])]
#[case("_3 o. 3j4", [1.448_306_995_231_357, 0.158_997_191_679_999_57])]
#[case("_5 o. 3j4", [2.299_914_040_879_302, 0.917_616_853_351_827_2])]
#[case("_6 o. 3j4", [2.305_509_031_243_477, 0.936_812_461_155_719_9])]
#[case("_7 o. 3j4", [0.117_500_907_311_433_66, 1.409_921_049_596_575])]
#[case("8 o. 3j4", [3.920_372_033_054_046, -3.060_933_988_314_991])]
#[case("_11 o. 3j4", [-4.0, 3.0])]
fn the_transcendental_circle_functions(#[case] src: &str, #[case] want: [f64; 2]) {
    close(Lang::J, src, want);
}

#[test]
fn a_circle_function_off_the_table_is_a_domain_error() {
    assert_eq!(err(Lang::J, "13 o. 1").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "1.5 o. 3j4").kind, ErrorKind::Domain);
}

// --- comparison and refusal ----------------------------------------------

#[test]
fn equality_is_tolerant_on_the_magnitude_of_the_difference() {
    assert_eq!(val(Lang::J, "3j4 = 3j4"), Array::scalar_bool(true));
    assert_eq!(val(Lang::J, "3j4 = 3.0000000000001j4"), Array::scalar_bool(true));
    assert_eq!(val(Lang::J, "1 = 1j0.0000001"), Array::scalar_bool(false));
    assert_eq!(val(Lang::J, "1j0 = 1"), Array::scalar_bool(true));
    assert_eq!(val(Lang::J, "1j1 ~: 1j1"), Array::scalar_bool(false));
    assert_eq!(val(Lang::Apl, "3J4=3J4"), Array::scalar_bool(true));
}

/// Ordering. In J a complex number has no order, so `< <: > >:` and the
/// dyadic `<. >.` all refuse it. APL's comparisons are total in the GNU
/// line — real part, then imaginary — but `⌈` and `⌊` are not part of
/// that and refuse a complex operand there too.
#[rstest]
#[case(Lang::J, "3j4 < 1j2")]
#[case(Lang::J, "3 < 3j4")]
#[case(Lang::J, "3j4 >: 1j2")]
#[case(Lang::J, "3j4 <. 1j2")]
#[case(Lang::J, "3j4 >. 1j2")]
#[case(Lang::Apl, "3J4⌈1J2")]
#[case(Lang::Apl, "3J4⌊1J2")]
fn ordering_a_complex_number_is_refused(#[case] lang: Lang, #[case] src: &str) {
    let e = err(lang, src);
    assert_eq!(e.kind, ErrorKind::Domain, "{src}: {}", e.msg);
    assert!(e.msg.contains("no order"), "{src}: {}", e.msg);
    assert!(e.span.is_some(), "{src}: no span");
}

/// APL's comparisons order a complex value by its real part and then its
/// imaginary one, a character by its codepoint, and a character below
/// every number.
#[rstest]
#[case("3J4<1J2", "0")]
#[case("1J2<1J3", "1")]
#[case("1J1<1J1", "0")]
#[case("2J5>2J4", "1")]
#[case("0J1<1", "1")]
#[case("'b'<'c'", "1")]
#[case("'a'<1", "1")]
#[case("1<'a'", "0")]
fn apl_comparisons_are_total(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::Apl, src), want, "{src}");
}

/// Under the Dyalog dialect they are not: only real numbers have an order
/// there, exactly as in J.
#[rstest]
#[case("3J4<1J2")]
#[case("'b'<'c'")]
#[case("'a'<1")]
fn the_dyalog_line_orders_numbers_alone(#[case] src: &str) {
    let p = compile(Lang::Apl, src, &Dialect::dyalog()).expect("compiles");
    let mut sink = |_: &str| {};
    let e = p.run(&[], &mut sink).expect_err("expected a refusal");
    assert!(matches!(e.kind, ErrorKind::Domain | ErrorKind::Type), "{src}: {}", e.msg);
}

#[rstest]
#[case(Lang::J, "3j4 + 'a'", "character")]
#[case(Lang::J, "3j4 + < 1", "boxed")]
fn a_complex_value_where_it_does_not_belong(
    #[case] lang: Lang,
    #[case] src: &str,
    #[case] msg: &str,
) {
    let e = err(lang, src);
    assert!(e.msg.contains(msg), "{src}: {}", e.msg);
}

// --- reductions, scans, structure ----------------------------------------

#[test]
fn reductions_and_scans_run_on_complex_data() {
    close(Lang::J, "+/ 3j4 1j1 2", [6.0, 5.0]);
    close(Lang::J, "*/ 1j1 1j1", [0.0, 2.0]);
    assert_eq!(
        parts(Lang::J, "+/\\ 1j1 2j2 3j3"),
        vec![[1.0, 1.0], [3.0, 3.0], [6.0, 6.0]]
    );
    close(Lang::Apl, "+/3J4 1J1 2", [6.0, 5.0]);
    // A reduction with no items has no complex identity to reach for; the
    // real one serves, as it does for floats.
    assert_eq!(val(Lang::J, "+/ 0j0 #~ 0"), Array::scalar_bool(false));
}

#[test]
fn the_structural_verbs_carry_complex_data_through() {
    assert_eq!(parts(Lang::J, "~. 3j4 3j4 1j1"), vec![[3.0, 4.0], [1.0, 1.0]]);
    assert_eq!(val(Lang::J, "3j4 1j1 i. 1j1"), Array::scalar_i64(1));
    assert_eq!(val(Lang::J, "3j4 e. 3j4 1"), Array::scalar_bool(true));
    assert_eq!(val(Lang::J, "3j4 -: 3j4"), Array::scalar_bool(true));
    assert_eq!(val(Lang::J, "3j4 -: 3"), Array::scalar_bool(false));
    // Grading is a permutation, not a claim about size: it orders by real
    // part and then imaginary, which is what J's `/:` answers.
    assert_eq!(val(Lang::J, "/: 3j1 3j_1 2j9"), Array::from_i64(vec![2, 1, 0]));
    assert_eq!(parts(Lang::J, "|. 3j4 1j1"), vec![[1.0, 1.0], [3.0, 4.0]]);
    assert_eq!(val(Lang::J, "$ 3 4 $ 1j1"), Array::from_i64(vec![3, 4]));
}

#[test]
fn catenation_promotes_reals_to_complex() {
    let a = val(Lang::J, "1j1 , 2 3");
    assert_eq!(a.dtype(), DType::Complex);
    assert_eq!(a.as_complex_slice(), Some(&[[1.0, 1.0], [2.0, 0.0], [3.0, 0.0]][..]));
}

// --- fusion ---------------------------------------------------------------

/// The blockwise kernel computes in one real type, so a chain that touches
/// complex data declines and the ordinary pipeline runs it. The answer has
/// to be the same either way, which is what this checks: the same chain
/// over reals is fused, over complex it is not, and both are right.
#[test]
fn a_fused_chain_falls_back_on_complex_data() {
    let real = "((1 2 3 + 4 5 6) * 2) - 1";
    let cplx = "((1j1 2j2 3j3 + 4 5 6) * 2) - 1";
    assert_eq!(val(Lang::J, real), Array::from_i64(vec![9, 13, 17]));
    assert_eq!(
        parts(Lang::J, cplx),
        vec![[9.0, 2.0], [13.0, 4.0], [17.0, 6.0]]
    );
    // A mixed chain: the complex operand arrives partway through.
    assert_eq!(
        parts(Lang::J, "(1 2 3 * 2) + 0j1 * 4 5 6"),
        vec![[2.0, 4.0], [4.0, 5.0], [6.0, 6.0]]
    );
}

/// A chain that WOULD fuse over reals answers the same over complex data,
/// where the kernel declines and the ordinary pipeline runs it.
#[test]
fn the_kernel_and_the_pipeline_agree_on_complex_arguments() {
    use jay::fuse::{is_fused, unfused};
    use jay::Buf;
    let program = compile(Lang::J, "(({z} + {z}) * 2) - 1", &Dialect::default())
        .unwrap_or_else(|e| panic!("compile failed: {}", e.msg));
    assert!(is_fused(&program), "the chain under test does not fuse at all");
    let plain = unfused(&program);
    let cplx = Array::new(vec![3], Data::Complex(Buf::from_vec(vec![[1.0, 1.0]; 3])));
    let mut sink = |_: &str| {};
    let fused = program.run(std::slice::from_ref(&cplx), &mut sink).expect("run");
    let unfused = plain.run(std::slice::from_ref(&cplx), &mut sink).expect("run");
    assert_eq!(fused, unfused);
    assert_eq!(
        fused.expect("a value").as_complex_slice(),
        Some(&[[3.0, 4.0]; 3][..])
    );
}
