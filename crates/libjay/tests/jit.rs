//! The compiled fold step against the interpreter, bit for bit.
//!
//! Every program here runs in one thread once with the compiler off, and
//! once more for each code generator built in, with that generator forced
//! on for every fold it can lower however short. Every answer must be the
//! same array with the same bits, or the same refusal with the same
//! message. The cases name whether the compiled road is expected to be the
//! one taken, so a case cannot pass by quietly falling back; the cases that
//! must fall back — a NaN, an infinity, `0 % 0`, integer data — are here
//! for the opposite reason. The generators lower the same expression, so
//! one compiles a case exactly when the other does, and every case checks
//! that too.
//!
//! The J corpus is replayed the same way, file by file: whatever a corpus
//! sentence folds, every road must answer it identically.

use std::path::PathBuf;

use jay::jit::{backends, runs_by, with_mode, Backend, JitMode};
use jay::{compile, Array, Data, Dialect, Extensions, Lang};
use libjay_testkit::corpus;
use rstest::rstest;

/// splitmix64, so every run sees the same data.
fn floats(n: usize, seed: u64) -> Vec<f64> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = s;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            (z >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        })
        .collect()
}

/// What a run answered, written so that two answers are equal exactly
/// when their shapes, types and bits are: a float's `{:?}` is the shortest
/// spelling that reads back to the same bits, the sign of a zero included.
fn answer(lang: Lang, src: &str, args: &[Array]) -> String {
    let dialect = Dialect { extensions: Some(Extensions::NONE), ..Dialect::default() };
    let program = match compile(lang, src, &dialect) {
        Ok(p) => p,
        Err(e) => return format!("compile error: {e}"),
    };
    let mut printed = String::new();
    let mut sink = |s: &str| printed.push_str(s);
    match program.run(args, &mut sink) {
        Ok(Some(v)) => format!("{printed}|{:?} {:?} {:?}", v.shape, v.dtype(), bits(&v)),
        Ok(None) => format!("{printed}|no value"),
        Err(e) => format!("{printed}|error: {e}"),
    }
}

/// The bits of every float, or the data's own spelling for other types.
fn bits(a: &Array) -> String {
    match &a.data {
        Data::F64(v) => format!("{:?}", v.iter().map(|x| x.to_bits()).collect::<Vec<_>>()),
        d => format!("{d:?}"),
    }
}

/// Run `src` interpreted and with every generator forced, and hold every
/// answer to the interpreter's. Each generator must have taken the
/// compiled road exactly when `compiled` says, which also holds the
/// generators to compiling the same cases.
fn check(src: &str, args: &[Array], compiled: bool) {
    let off = with_mode(JitMode::Off, || answer(Lang::J, src, args));
    let mut taken: Vec<(Backend, bool)> = Vec::new();
    for b in backends() {
        let before = runs_by(b);
        let on = with_mode(JitMode::forcing(b), || answer(Lang::J, src, args));
        assert_eq!(off, on, "{src} ({b:?})");
        taken.push((b, runs_by(b) > before));
    }
    assert!(
        taken.windows(2).all(|w| w[0].1 == w[1].1),
        "{src}: the generators differ on whether they compile it: {taken:?}"
    );
    for (b, ran) in taken {
        assert_eq!(ran, compiled, "{src} ({b:?}): compiled road taken = {ran}");
    }
}

fn vector(n: usize, seed: u64) -> Array {
    Array::new(vec![n], Data::F64(floats(n, seed).into()))
}

/// Pairs (constant in 0..1, price) one per row, the shape of a KAMA fold.
fn pairs(n: usize, seed: u64) -> Array {
    let a = floats(n, seed);
    let b = floats(n, seed + 1);
    let mut v = Vec::with_capacity(2 * n);
    for i in 0..n {
        v.push(a[i] + 0.5);
        v.push(100.0 + b[i]);
    }
    Array::new(vec![n, 2], Data::F64(v.into()))
}

fn with(v: &mut [f64], at: &[(usize, f64)]) -> Array {
    for &(i, x) in at {
        v[i] = x;
    }
    Array::new(vec![v.len()], Data::F64(v.to_vec().into()))
}

const KSTEP: &str = "4 : 'y + ({. x) * ({: x) - y'";

#[rstest]
// The fold family over rows, an explicit step, every direction and keeping.
#[case::kama_every(format!("100 (] F:. {KSTEP}) {{y}}"), pairs(3000, 1), true)]
#[case::kama_last(format!("100 (] F.. {KSTEP}) {{y}}"), pairs(3000, 2), true)]
#[case::kama_every_back(format!("100 (] F:: {KSTEP}) {{y}}"), pairs(3000, 3), true)]
#[case::kama_last_back(format!("100 (] F.: {KSTEP}) {{y}}"), pairs(3000, 4), true)]
#[case::kama_short(format!("100 (] F:. {KSTEP}) {{y}}"), pairs(3, 5), true)]
#[case::kama_one(format!("100.5 (] F:. {KSTEP}) {{y}}"), pairs(1, 5), true)]
#[case::row_index(
    "0 (] F:. 4 : 'y + (1 { x) - (_2 { x) * y') {y}".to_string(), pairs(2000, 6), true
)]
#[case::row_rank("0 (] F:. ([: {.\"1 [) + ]) {y}".to_string(), pairs(2000, 7), true)]
// Lists, with and without a seed.
#[case::train_unseeded("(] F:. ([ + 0.9 * ])) {y}".to_string(), vector(5000, 8), true)]
#[case::train_seeded("0.25 (] F:: (] - 0.5 * [)) {y}".to_string(), vector(5000, 9), true)]
#[case::direct(
    "0 (] F:. {{ a =. x - y\n a * a + y }}) {y}".to_string(), vector(4000, 10), true
)]
#[case::hook_and_atop("(] F:. (+ -:)) {y}".to_string(), vector(4000, 11), true)]
#[case::bonds("(] F:. ([ + 0.5&*@])) {y}".to_string(), vector(4000, 12), true)]
#[case::commute("(] F:. (0.9&*@[ -~ ])) {y}".to_string(), vector(4000, 13), true)]
#[case::monads(
    "(] F:. ([: -. [: | [: - [: >: [: <: [: +: [: -: [: *: [ - 0.5 * ])) {y}".to_string(),
    vector(4000, 14), true
)]
#[case::roots("(] F:. ([: %: [: | [ + 0.5 * ])) {y}".to_string(), vector(4000, 15), true)]
#[case::exp_ln("(] F:. ([: ^. 1 + [: ^ [ - ])) {y}".to_string(), vector(4000, 16), true)]
#[case::extrema("(] F:. ([ <. 0.5 >. ])) {y}".to_string(), vector(4000, 17), true)]
#[case::reciprocal("(] F:. ([: % 3 + [ * ])) {y}".to_string(), vector(4000, 18), true)]
// The insert and the suffix scan.
#[case::insert("([ + 0.5 * ])/ {y}".to_string(), vector(5000, 19), true)]
#[case::insert_explicit("(4 : 'x + 0.5 * y')/ {y}".to_string(), vector(5000, 20), true)]
#[case::suffix_scan("(] + 0.5 * [)/\\. {y}".to_string(), vector(5000, 21), true)]
#[case::suffix_scan_div("(] % 1.5 + |@[)/\\. {y}".to_string(), vector(5000, 22), true)]
// Where the compiled loop must stop and leave the answer to the interpreter.
#[case::zero_over_zero(
    "(] F:. (] % [)) {y}".to_string(), with(&mut vec![1.0; 50], &[(0, 0.0), (10, 0.0)]), false
)]
#[case::overflow("(] F:. ([ * 1e300 * ])) {y}".to_string(), vector(50, 23), false)]
#[case::nan_in_data(
    "(] F:. ([ + 0.5 * ])) {y}".to_string(), with(&mut floats(50, 24), &[(7, f64::NAN)]), false
)]
#[case::infinity_in_data(
    "(] F:. ([ + 0.5 * ])) {y}".to_string(), with(&mut floats(50, 25), &[(7, f64::INFINITY)]),
    false
)]
#[case::complex_answer("(] F:. ([: ^. [ - ])) {y}".to_string(), vector(50, 26), false)]
#[case::infinity_made("(] F:. ([: % [ - [)) {y}".to_string(), vector(50, 27), false)]
fn compiled_and_interpreted_agree(#[case] src: String, #[case] y: Array, #[case] compiled: bool) {
    check(&src, std::slice::from_ref(&y), compiled);
}

/// `n` floats cycling through `pattern`.
fn cycle(pattern: &[f64], n: usize) -> Array {
    let v: Vec<f64> = pattern.iter().copied().cycle().take(n).collect();
    Array::new(vec![n], Data::F64(v.into()))
}

const NZ: f64 = -0.0;

#[rstest]
// The two zeros side by side, which the extrema answer by position.
#[case::min_zeros("(] F:. ([ <. ])) {y}", cycle(&[NZ, 0.0, 0.0, NZ, NZ], 2000), true)]
#[case::min_zeros_swapped("(] F:. (] <. [)) {y}", cycle(&[NZ, 0.0, 0.0, NZ, NZ], 2000), true)]
#[case::max_zeros("(] F:. ([ >. ])) {y}", cycle(&[0.0, NZ, NZ, 0.0, NZ], 2000), true)]
#[case::max_zeros_swapped("(] F:. (] >. [)) {y}", cycle(&[0.0, NZ, NZ, 0.0, NZ], 2000), true)]
// A bare primitive step takes the interpreter's typed fold, never the
// compiled one.
#[case::extrema_bare("(] F:. <.) {y}", cycle(&[NZ, 0.0, 1.0, NZ, -1.0], 2000), false)]
#[case::extrema_explicit(
    "(] F.. (4 : '0 >. y <. x')) {y}", cycle(&[NZ, 0.0, NZ, 2.0, -3.0], 2000), true
)]
#[case::abs_zeros("(] F:. ([: | [ <. ])) {y}", cycle(&[NZ, 0.0, -1.5, NZ, 2.0], 2000), true)]
#[case::abs_negated("(] F:. ([: | [: - [)) {y}", cycle(&[NZ, 0.0, -1.5, 3.0], 2000), true)]
#[case::insert_zeros("([ <. ])/ {y}", cycle(&[0.0, NZ, NZ, 0.0], 2001), true)]
#[case::suffix_scan_zeros("([ >. ])/\\. {y}", cycle(&[NZ, 0.0, 0.0], 2000), true)]
// A NaN or an infinity stops the compiled loop; the interpreter answers.
#[case::min_nan(
    "(] F:. ([ <. ])) {y}", with(&mut floats(50, 30), &[(9, f64::NAN)]), false
)]
#[case::max_nan_first(
    "(] F:. ([ >. ])) {y}", with(&mut floats(50, 31), &[(0, f64::NAN)]), false
)]
#[case::abs_nan("(] F:. ([: | [ + ])) {y}", with(&mut floats(50, 32), &[(5, f64::NAN)]), false)]
#[case::min_infinity(
    "(] F:. ([ <. ])) {y}", with(&mut floats(50, 33), &[(4, f64::NEG_INFINITY)]), false
)]
#[case::max_infinity(
    "(] F:. ([ >. ])) {y}", with(&mut floats(50, 34), &[(4, f64::INFINITY)]), false
)]
#[case::abs_infinity(
    "(] F:. ([: | [ - ])) {y}", with(&mut floats(50, 35), &[(4, f64::NEG_INFINITY)]), false
)]
fn extrema_and_magnitude_agree_on_zeros_nan_and_infinities(
    #[case] src: &str,
    #[case] y: Array,
    #[case] compiled: bool,
) {
    check(src, std::slice::from_ref(&y), compiled);
}

#[rstest]
#[case::integers("(] F:. ([ + 2 * ])) {y}", Data::I64((0..100).collect::<Vec<i64>>().into()))]
#[case::booleans("(] F:. ([ + 2 * ])) {y}", Data::Bool(vec![1u8, 0, 1, 1].into()))]
#[case::comparison("(] F:. ([ < ])) {y}", Data::F64(floats(100, 28).into()))]
#[case::constant_answer("(] F:. (4 : '1')) {y}", Data::F64(floats(100, 29).into()))]
fn what_is_not_arithmetic_on_floats_is_interpreted(#[case] src: &str, #[case] d: Data) {
    let n = d.len();
    let y = Array::new(vec![n], d);
    check(src, std::slice::from_ref(&y), false);
}

#[rstest]
fn the_corpus_answers_the_same(#[files("tests/corpus/j/*.txt")] path: PathBuf) {
    let mut differ = Vec::new();
    for e in corpus::read(&path) {
        let off = with_mode(JitMode::Off, || answer(Lang::J, &e.expr, &[]));
        for b in backends() {
            let on = with_mode(JitMode::forcing(b), || answer(Lang::J, &e.expr, &[]));
            if off != on {
                differ.push(format!("{} ({b:?})\n  off: {off}\n  on:  {on}", e.expr));
            }
        }
    }
    assert!(differ.is_empty(), "{}:\n{}", path.display(), differ.join("\n"));
}

/// Without a forced generator, a fold long enough to compile but short of
/// LLVM's threshold takes Cranelift's loop where Cranelift is built in;
/// `force` takes the best generator built in, and once that is LLVM, its
/// cached loop serves the automatic policy too. The step is one no other
/// case uses, since the cache is shared by the whole process.
#[test]
fn the_policy_picks_the_generator() {
    let src = "(] F:. ([ + 0.375 * ])) {y}";
    let y = vector(5000, 40);
    let road = |m: JitMode| {
        let before: Vec<u64> = backends().iter().map(|&b| runs_by(b)).collect();
        with_mode(m, || answer(Lang::J, src, std::slice::from_ref(&y)));
        let after: Vec<u64> = backends().iter().map(|&b| runs_by(b)).collect();
        backends().into_iter().zip(before.iter().zip(&after)).find(|(_, (a, b))| b > a).map(|(b, _)| b)
    };
    let all = backends();
    assert_eq!(road(JitMode::Off), None);
    assert_eq!(road(JitMode::Auto), all.first().copied());
    assert_eq!(road(JitMode::Force), all.last().copied());
    assert_eq!(road(JitMode::Auto), all.last().copied());
}
