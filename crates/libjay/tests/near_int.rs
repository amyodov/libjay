//! A float that is merely NEAR a whole number, where a count or an index
//! is wanted.
//!
//! Both references round such a float rather than refusing it — `⍳2-1E¯14`
//! is `1 2`, `(2-1e_14) {. 1 2 3` is `1 2` — and this is not the
//! comparison tolerance tests/tolerance.rs covers: `⎕CT←0` and `9!:19 (0)`
//! leave the admission exactly where it was, and neither reference lets a
//! program move it at all.
//!
//! The two admissions differ in shape, and each was measured against its
//! own reference:
//!
//! - J's is RELATIVE. A value within `2^-44` of a whole number's magnitude
//!   reads as that whole number, so the window widens with the count and
//!   closes completely at zero: `i. 2+1.1e_13` answers, `i. 2+1.2e_13`
//!   does not, and `i. 1e_14` is a domain error.
//! - GNU APL's is ABSOLUTE, `1e¯10` at every magnitude: `⍳2+9E¯11` and
//!   `⍳1000+9.9E¯11` both answer, `⍳1000000+1E¯9` does not, and `⍳1E¯11` is
//!   the empty vector because 1e¯11 reads as 0.
//!
//! The two cross over at about 1760: below it APL admits what J refuses,
//! above it J admits what APL refuses.
//!
//! The breadth is in tests/corpus/{j,apl}/tolerance.txt; this file states
//! one rule per case.

use jay::{compile, Array, Dialect, Error, Lang};
use rstest::rstest;

fn run(lang: Lang, src: &str, dialect: &Dialect) -> Result<Option<Array>, Error> {
    let program = compile(lang, src, dialect)?;
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink)
}

fn val(lang: Lang, src: &str, dialect: &Dialect) -> Array {
    run(lang, src, dialect)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn refused(lang: Lang, src: &str) -> Error {
    match run(lang, src, &Dialect::default()) {
        Err(e) => e,
        Ok(v) => panic!("{src:?} was expected to fail; it answered {v:?}"),
    }
}

fn ints(lang: Lang, src: &str) -> Vec<i64> {
    let a = val(lang, src, &Dialect::default());
    a.to_i64_vec().unwrap_or_else(|| panic!("{src:?} is not whole numbers: {a:?}"))
}

fn j(src: &str) -> Vec<i64> {
    ints(Lang::J, src)
}

fn apl(src: &str) -> Vec<i64> {
    ints(Lang::Apl, src)
}

// --- the window each reference admits ------------------------------------

/// J: `|x - n| ≤ 2^-44 × max(|x|, |n|)`. At 2 that is 1.1368e¯13, and the
/// pair of cases either side of it is the measurement.
#[rstest]
#[case("# i. 2 - 1e_14", 2)]
#[case("# i. 2 + 1e_14", 2)]
#[case("# i. 2 + 1e_13", 2)]
#[case("# i. 2 + 1.1e_13", 2)]
#[case("# i. 2 - 1.1e_13", 2)]
// The window grows with the magnitude, which is what makes it relative:
// 1.1e¯12 is admitted at 20 and refused at 2.
#[case("# i. 20 + 1.1e_12", 20)]
#[case("# i. 200 + 1.1e_11", 200)]
#[case("# i. 2000 + 1e_10", 2000)]
#[case("# i. 1000000 + 5e_8", 1000000)]
fn j_admits_a_float_within_a_relative_window(#[case] src: &str, #[case] want: i64) {
    assert_eq!(j(src), vec![want]);
}

/// One step outside the window and the same sentence is a domain error, at
/// every magnitude.
#[rstest]
#[case("i. 2 + 1.2e_13")]
#[case("i. 2 - 1.2e_13")]
#[case("i. 2 + 1e_12")]
#[case("i. 2 + 9e_11")]
#[case("i. 20 + 1.2e_12")]
#[case("i. 200 + 1.2e_11")]
#[case("i. 2000 + 2e_10")]
#[case("i. 1000000 + 6e_8")]
#[case("i. 2.5")]
fn j_refuses_a_float_outside_it(#[case] src: &str) {
    refused(Lang::J, src);
}

/// GNU APL: `|x - n| < 1e¯10`, the same width whatever the magnitude.
#[rstest]
#[case("⍴⍳2-1E¯14", 2)]
#[case("⍴⍳2+9E¯11", 2)]
#[case("⍴⍳2-9E¯11", 2)]
#[case("⍴⍳20+9.9E¯11", 20)]
#[case("⍴⍳1000+9.9E¯11", 1000)]
#[case("⍴⍳1000000+1E¯11", 1000000)]
fn apl_admits_a_float_within_an_absolute_window(#[case] src: &str, #[case] want: i64) {
    assert_eq!(apl(src), vec![want]);
}

/// A tenth of a nanosecond out is a domain error even at 2, and being large
/// buys nothing.
#[rstest]
#[case("⍳2+1E¯10")]
#[case("⍳2-1E¯10")]
#[case("⍳1000+1E¯9")]
#[case("⍳1000000+1E¯9")]
#[case("⍳100000+1E¯6")]
#[case("⍳2.5")]
fn apl_refuses_a_float_outside_it(#[case] src: &str) {
    refused(Lang::Apl, src);
}

/// The two rules genuinely part company, in both directions. `2+9E¯11` is
/// inside APL's absolute window and far outside J's relative one;
/// `1000000+1e¯9` is the other way about. And `1E¯11` reads as 0 in APL,
/// where J's window has no width at all beside zero.
#[test]
fn the_two_windows_are_not_the_same_window() {
    assert_eq!(apl("⍴⍳2+9E¯11"), vec![2]);
    refused(Lang::J, "i. 2 + 9e_11");

    assert_eq!(j("# i. 1000000 + 1e_9"), vec![1000000]);
    refused(Lang::Apl, "⍳1000000+1E¯9");

    assert_eq!(apl("⍴⍳1E¯11"), vec![0]);
    refused(Lang::J, "i. 1e_14");
}

// --- it is not the comparison tolerance ----------------------------------

/// Setting the comparison tolerance to zero leaves the admission untouched
/// in both languages, which is how the references behave and why this is
/// not a tolerance rule wearing a different hat. With the tolerance off,
/// `(2+1e_13) = 2` is 0 in jconsole and `i. 2+1e_13` still counts to 2.
#[test]
fn no_setting_moves_it() {
    let exact = Dialect { comparison_tolerance: Some(0.0), ..Dialect::default() };
    assert_eq!(val(Lang::Apl, "⍴⍳2+9E¯11", &exact).to_i64_vec(), Some(vec![2]));
    assert_eq!(val(Lang::J, "# i. 2 + 1e_13", &exact).to_i64_vec(), Some(vec![2]));
    assert_eq!(val(Lang::J, "(2 + 1e_13) = 2", &exact).to_i64_vec(), Some(vec![0]));
}

/// Nor does a wide tolerance widen it: `⎕CT←1E¯5` leaves `⍳2+1E¯8` a
/// domain error.
#[test]
fn a_wide_tolerance_does_not_widen_it() {
    let wide = Dialect { comparison_tolerance: Some(1e-5), ..Dialect::default() };
    assert!(run(Lang::Apl, "⍳2+1E¯8", &wide).is_err());
}

/// And the comparison itself still sees the difference: the admission
/// rounds an argument, it does not make `2+9E¯11` equal 2.
#[test]
fn the_comparison_still_sees_the_difference() {
    assert_eq!(apl("(2+9E¯11)=2"), vec![0]);
}

// --- the whole family goes through it ------------------------------------

/// Every J verb that reads a count, a length or an index takes the near
/// integer, not just `i.`.
#[rstest]
#[case("(2 - 1e_14) {. 1 2 3", &[1, 2])]
#[case("(2 + 1e_13) }. 1 2 3 4", &[3, 4])]
#[case("(_2 + 1e_13) {. 1 2 3", &[2, 3])]
#[case("(2 + 1e_13) $ 5", &[5, 5])]
#[case("(1 + 1e_14) |. 1 2 3", &[2, 3, 1])]
#[case("(2 + 1e_13) # 1 2", &[1, 1, 2, 2])]
#[case("(1 + 1e_14) { 1 2 3", &[2])]
#[case("q: 6 + 1e_13", &[2, 3])]
#[case("p: 5 + 1e_13", &[13])]
#[case("(2 + 1e_13) A. 1 2 3", &[2, 1, 3])]
#[case("(1 + 1e_14) |.!.0 ] 1 2 3", &[2, 3, 0])]
#[case("I. 2 3 4 + 1e_13", &[0, 0, 1, 1, 1, 2, 2, 2, 2])]
#[case("(>: ^: (2 + 1e_13)) 3", &[5])]
#[case("(+`- @. 1.00000000000001) 5", &[-5])]
#[case("(2 + 1e_13) {\"1 i. 3 3", &[2, 5, 8])]
fn the_j_family_reads_the_near_integer(#[case] src: &str, #[case] want: &[i64]) {
    assert_eq!(j(src), want.to_vec());
}

/// It is not universal, though: J reads an operand SELECTOR exactly, and
/// the reference refuses these where it took the counts above.
#[rstest]
#[case("3 u: 65 + 1e_13")]
#[case("s: 65 + 1e_13")]
#[case("(2 + 1e_13) b. 3")]
fn a_j_operand_selector_is_still_read_exactly(#[case] src: &str) {
    refused(Lang::J, src);
}

/// And every APL one.
#[rstest]
#[case("(2-1E¯14)↑1 2 3", &[1, 2])]
#[case("(2+9E¯11)↓1 2 3 4", &[3, 4])]
#[case("(¯2+9E¯11)↑1 2 3", &[2, 3])]
#[case("(2+9E¯11)⍴5", &[5, 5])]
#[case("(1+9E¯11)⌽1 2 3", &[2, 3, 1])]
#[case("(2+9E¯11)/1 2", &[1, 1, 2, 2])]
#[case("(⍳5)[2+9E¯11]", &[2])]
#[case("(2+9E¯11)⊃(1 2)(3 4)", &[3, 4])]
#[case("(2+9E¯11)⌷1 2 3", &[2])]
#[case("(1 0 1+9E¯11)\\1 2", &[1, 0, 2])]
#[case("(1+9E¯11)⊖2 2⍴⍳4", &[3, 4, 1, 2])]
#[case("⎕UCS ⎕UCS 65+9E¯11", &[65])]
fn the_apl_family_reads_the_near_integer(#[case] src: &str, #[case] want: &[i64]) {
    assert_eq!(apl(src), want.to_vec());
}

/// A float that is nowhere near a whole number is still refused, and the
/// diagnostic names what was wanted rather than blaming a tolerance.
#[test]
fn a_float_that_is_no_count_still_says_so() {
    let e = refused(Lang::Apl, "⍳2.5");
    assert!(e.msg.contains("integer"), "{}", e.msg);
    let e = refused(Lang::J, "2.5 $ 5");
    assert!(e.msg.contains("integer"), "{}", e.msg);
}
