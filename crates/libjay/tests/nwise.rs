//! N-wise reduction: `n f/ y`, the dyadic case of a `/`-derived function.
//!
//! tests/corpus/apl/nwise.txt carries the breadth against GNU APL. This file
//! states the rules — how long a window may be, what a negative or zero n
//! means, which axis each spelling takes, what makes the left argument legal,
//! and the one thing a reader is likeliest to get wrong: `/` is the reduce
//! operator or the replicate function according to what stands to its LEFT,
//! never according to the left argument's values.

use jay::fmt::{format_array, FmtOpts};
use jay::{compile, Array, Data, Dialect, Error, ErrorKind, Lang};
use rstest::rstest;

fn run_in(src: &str, dialect: &Dialect) -> Result<Option<Array>, Error> {
    let program = compile(Lang::Apl, src, dialect)?;
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink)
}

fn run(src: &str) -> Result<Option<Array>, Error> {
    run_in(src, &Dialect::default())
}

fn apl(src: &str) -> Array {
    run(src)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn shown(src: &str) -> String {
    format_array(&apl(src), &FmtOpts::APL).trim_end().to_string()
}

fn err(src: &str) -> Error {
    match run(src) {
        Err(e) => e,
        Ok(v) => panic!("{src:?} was expected to fail; it answered {v:?}"),
    }
}

fn i64s(shape: &[usize], values: &[i64]) -> Array {
    Array::new(shape.to_vec(), Data::I64(values.to_vec().into()))
}

/// The windows overlap and keep their order, and the operand folds each one.
/// The reduced axis loses `n-1` items, so the answer is one item shorter per
/// step of the window, and it is still an axis: `2+/1 2 3` is a two-item
/// vector, not the three-item scalar extension of `2+1 2 3`.
#[rstest]
#[case("2+/1 2 3", &[2], &[3, 5])]
#[case("2×/1 2 3", &[2], &[2, 6])]
#[case("2-/1 2 3", &[2], &[-1, -1])]
#[case("3+/⍳5", &[3], &[6, 9, 12])]
#[case("3-/1 2 3 4 5", &[3], &[2, 3, 4])]
#[case("2⌈/1 2 3 4", &[3], &[2, 3, 4])]
fn a_window_of_n_items_is_folded_at_every_position(
    #[case] src: &str,
    #[case] shape: &[usize],
    #[case] want: &[i64],
) {
    assert_eq!(apl(src), i64s(shape, want));
}

/// A window may be as long as the axis, and one longer than that: the last
/// leaves no positions at all and answers an empty. Beyond it there is no
/// window to speak of, and asking for one is a domain error rather than a
/// shorter answer. A scalar counts as one item, as `≢` counts it.
#[rstest]
#[case("5+/1 2 3 4 5", &[1], &[15])]
#[case("1+/1 2 3", &[3], &[1, 2, 3])]
#[case("6+/1 2 3 4 5", &[0], &[])]
#[case("2+/,1", &[0], &[])]
#[case("1+/⍬", &[0], &[])]
#[case("1+/5", &[], &[5])]
#[case("2+/5", &[0], &[])]
fn the_window_may_reach_one_past_the_end_of_the_axis(
    #[case] src: &str,
    #[case] shape: &[usize],
    #[case] want: &[i64],
) {
    assert_eq!(apl(src), i64s(shape, want));
}

#[rstest]
#[case("7+/1 2 3 4 5")]
#[case("2+/⍬")]
#[case("4+/1 2")]
#[case("5+/2 3⍴⍳6")]
#[case("2+⌿0 3⍴⍳0")]
fn a_window_longer_than_that_is_a_domain_error(#[case] src: &str) {
    let e = err(src);
    assert_eq!(e.kind, ErrorKind::Domain, "{src:?}: {}", e.msg);
}

/// A negative n takes the same windows with their items reversed. It shows
/// only where the fold is not commutative — `¯2+/` and `2+/` agree, `¯2-/`
/// and `2-/` do not — and `¯0` is `0`.
#[rstest]
#[case("¯2+/1 2 3 4 5", &[4], &[3, 5, 7, 9])]
#[case("¯2-/1 2 3", &[2], &[1, 1])]
#[case("¯1+/1 2 3", &[3], &[1, 2, 3])]
#[case("¯3+/1 2 3 4 5", &[3], &[6, 9, 12])]
#[case("¯5+/1 2 3 4 5", &[1], &[15])]
#[case("¯6+/1 2 3 4 5", &[0], &[])]
fn a_negative_n_reverses_each_window(
    #[case] src: &str,
    #[case] shape: &[usize],
    #[case] want: &[i64],
) {
    assert_eq!(apl(src), i64s(shape, want));
}

/// Zero takes the `1+≢y` empty windows between and around the items, so the
/// answer is that many copies of what `f/` gives an empty argument — the
/// operand's identity, and a domain error where it has none.
#[rstest]
#[case("0+/1 2 3 4 5", &[6], &[0, 0, 0, 0, 0, 0])]
#[case("0×/1 2 3", &[4], &[1, 1, 1, 1])]
#[case("0-/1 2 3", &[4], &[0, 0, 0, 0])]
#[case("0∨/1 2 3", &[4], &[0, 0, 0, 0])]
#[case("0∧/1 2 3", &[4], &[1, 1, 1, 1])]
#[case("0+/⍬", &[1], &[0])]
#[case("0+/5", &[2], &[0, 0])]
#[case("¯0+/1 2 3", &[4], &[0, 0, 0, 0])]
#[case("0+/'abc'", &[4], &[0, 0, 0, 0])]
fn a_zero_window_answers_the_operands_identity_once_per_gap(
    #[case] src: &str,
    #[case] shape: &[usize],
    #[case] want: &[i64],
) {
    // Every one of these identities is 0 or 1, which is boolean storage.
    let want: Vec<u8> = want.iter().map(|&k| k as u8).collect();
    assert_eq!(
        apl(src),
        Array::new(shape.to_vec(), Data::Bool(want.into()))
    );
}

/// `/` windows the LAST axis and `⌿` the leading one, which is the same
/// divergence from J the monadic reduce already has; a bracket axis replaces
/// either glyph's choice with the axis it names, counted from `⎕IO`. The
/// reduced axis stays in place, so the answer has the argument's rank.
#[rstest]
#[case("2+/2 3⍴⍳6", &[2, 2], &[3, 5, 9, 11])]
#[case("2+⌿2 3⍴⍳6", &[1, 3], &[5, 7, 9])]
#[case("2+/[1]2 3⍴⍳6", &[1, 3], &[5, 7, 9])]
#[case("2+/[2]2 3⍴⍳6", &[2, 2], &[3, 5, 9, 11])]
#[case("2+⌿[2]2 3⍴⍳6", &[2, 2], &[3, 5, 9, 11])]
#[case("2+/2 2⍴⍳4", &[2, 1], &[3, 7])]
#[case("2(+⌿)1 2 3", &[2], &[3, 5])]
#[case("2+/[2]2 3 4⍴⍳24", &[2, 2, 4], &[6, 8, 10, 12, 14, 16, 18, 20, 30, 32, 34, 36, 38, 40, 42, 44])]
fn each_spelling_windows_its_own_axis(
    #[case] src: &str,
    #[case] shape: &[usize],
    #[case] want: &[i64],
) {
    assert_eq!(apl(src), i64s(shape, want));
}

/// A bracket axis is counted from `⎕IO`, so the same axis is `[1]` in the
/// shipped origin and `[0]` in the zero one. The windows themselves do not
/// move: `⎕IO` numbers axes, not items.
#[rstest]
#[case("2+/[0]2 3⍴⍳6", &[1, 3], &[3, 5, 7])]
#[case("2+/[1]2 3⍴⍳6", &[2, 2], &[1, 3, 7, 9])]
#[case("2+/1 2 3", &[2], &[3, 5])]
fn a_bracket_axis_counts_from_the_index_origin(
    #[case] src: &str,
    #[case] shape: &[usize],
    #[case] want: &[i64],
) {
    let zero = Dialect { index_origin: Some(0), ..Dialect::default() };
    let got = run_in(src, &zero)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"));
    assert_eq!(got, i64s(shape, want));
}

/// The operand decides what a window folds to, and APL's insert folds the
/// ELEMENTS along the axis and encloses a value that is not a simple scalar.
/// So `2,/` builds the pairs as boxes, and `2,/'abcd'` builds strings.
#[rstest]
#[case("2,/1 2 3", " 1 2  2 3")]
#[case("¯2,/1 2 3", " 2 1  3 2")]
#[case("3,/1 2 3 4 5", " 1 2 3  2 3 4  3 4 5")]
#[case("2,/'abcd'", " ab  bc  cd")]
#[case("2+/(1 2)(3 4)(5 6)", " 4 6  8 10")]
fn the_operand_decides_what_a_window_folds_to(#[case] src: &str, #[case] want: &str) {
    assert_eq!(
        shown(src).split_whitespace().collect::<Vec<_>>(),
        want.split_whitespace().collect::<Vec<_>>(),
        "{src:?} showed {:?}",
        shown(src)
    );
}

/// n is ONE number, whatever shape holds it: a scalar, a one-item vector, a
/// one-item matrix and an enclosed scalar all say the same thing. More than
/// one number is a length error — it is not a frame of several windows, as
/// J's infix takes it — and a number that is not an integer, or not a number
/// at all, is a domain error.
#[rstest]
#[case("(,2)+/1 2 3")]
#[case("(1 1⍴2)+/1 2 3")]
#[case("(⊂2)+/1 2 3")]
fn n_is_one_number_however_it_is_shaped(#[case] src: &str) {
    assert_eq!(apl(src), i64s(&[2], &[3, 5]));
}

#[rstest]
#[case("1 1+/2 3", ErrorKind::Length)]
#[case("2 2+/1 2 3 4", ErrorKind::Length)]
#[case("⍬+/1 2 3", ErrorKind::Length)]
#[case("'ab'+/1 2 3", ErrorKind::Length)]
#[case("'a'+/1 2 3", ErrorKind::Domain)]
#[case("2.5+/1 2 3", ErrorKind::Domain)]
fn anything_else_on_the_left_is_refused(#[case] src: &str, #[case] kind: ErrorKind) {
    let e = err(src);
    assert_eq!(e.kind, kind, "{src:?}: {}", e.msg);
}

/// The reading of `/` is settled by the token to its LEFT and nothing else:
/// after a function it is the reduce operator, after a value it is replicate.
/// The left ARGUMENT never enters into it, so `1 1/2 3` is a replicate that
/// repeats each item once and `1 1+/2 3` is an n-wise reduction whose n is
/// two numbers — a length error, not a compress.
#[rstest]
#[case("1 1/2 3", &[2], &[2, 3])]
#[case("2 2/2 3", &[4], &[2, 2, 3, 3])]
#[case("1 0 1/⍳3", &[2], &[1, 3])]
#[case("0 1/2 3", &[1], &[3])]
#[case("2/1 2 3", &[6], &[1, 1, 2, 2, 3, 3])]
fn slash_after_a_value_is_still_replicate(
    #[case] src: &str,
    #[case] shape: &[usize],
    #[case] want: &[i64],
) {
    assert_eq!(apl(src), i64s(shape, want));
}

#[rstest]
#[case("2+/1 2 3", &[2], &[3, 5])]
#[case("2+⌿2 3⍴⍳6", &[1, 3], &[5, 7, 9])]
#[case("2(+/)1 2 3", &[2], &[3, 5])]
#[case("2{⍺+⍵}/1 2 3", &[2], &[3, 5])]
fn slash_after_a_function_is_the_n_wise_reduction(
    #[case] src: &str,
    #[case] shape: &[usize],
    #[case] want: &[i64],
) {
    assert_eq!(apl(src), i64s(shape, want));
}

#[rstest]
#[case("1 1+/2 3")]
#[case("1 0 1+/⍳3")]
fn a_boolean_left_argument_does_not_turn_a_reduction_into_a_compress(#[case] src: &str) {
    let e = err(src);
    assert_eq!(e.kind, ErrorKind::Length, "{src:?}: {}", e.msg);
}

/// J's `x u\ y` is the same computation for a positive window, and libjay
/// runs the two through one blockwise fold. The rest of J's infix is a
/// different function: a negative left argument chops the argument into
/// non-overlapping blocks there and reverses each window here.
#[rstest]
#[case("2 +/\\ 1 2 3", "2+/1 2 3")]
#[case("3 +/\\ 1 2 3 4 5", "3+/1 2 3 4 5")]
#[case("2 <./\\ 1 2 3 4", "2⌊/1 2 3 4")]
fn a_positive_window_is_what_js_infix_computes(#[case] jsrc: &str, #[case] aplsrc: &str) {
    let program = compile(Lang::J, jsrc, &Dialect::default()).expect("J compiles");
    let mut sink = |_: &str| {};
    let got = program.run(&[], &mut sink).expect("J runs").expect("J answers");
    assert_eq!(got, apl(aplsrc));
}

/// The blockwise window kernel is the same one the infix uses, so a run long
/// enough to be split into blocks must answer what the one-window-at-a-time
/// path answers.
#[test]
fn a_long_argument_windows_blockwise_to_the_same_values() {
    let got = apl("3+/⍳10000");
    let want: Vec<i64> = (1..=9998).map(|i: i64| 3 * i + 3).collect();
    assert_eq!(got, i64s(&[9998], &want));
    assert_eq!(apl("2⌈/⍳10000"), i64s(&[9999], &(2..=10000).collect::<Vec<i64>>()));
}
