//! The GNU APL manual crawl of 2026-08-28: the features it found missing.
//!
//! The breadth is in tests/corpus/apl/doc-crawl.txt, which is recorded
//! against the oracle. This file states one rule per assertion — the rules
//! that are easy to get subtly wrong and that a recorded answer alone does
//! not explain.

use jay::{compile, Array, Data, Dialect, Error, ErrorKind, Lang};

fn run(src: &str) -> Result<Option<Array>, Error> {
    let program = compile(Lang::Apl, src, &Dialect::default())?;
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink)
}

fn apl(src: &str) -> Array {
    run(src)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
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

fn scalar(v: i64) -> Array {
    Array::scalar_i64(v)
}

/// The boolean scalar a comparison answers with.
fn bit(v: u8) -> Array {
    Array::new(Vec::new(), Data::Bool(vec![v].into()))
}

// ------------------------------------------------- the bit-wise functions

/// `⊤` with a logical glyph after it is one function, whatever stands
/// between the two glyphs, and it works on every bit at once.
#[rstest::rstest]
#[case("12 ⊤∧ 10", 8)]
#[case("12 ⊤∨ 10", 14)]
#[case("12 ⊤⍲ 10", -9)]
#[case("12 ⊤⍱ 10", -15)]
#[case("12 ⊤= 10", -7)]
#[case("12 ⊤≠ 10", 6)]
#[case("12 ⊤ ∧ 10", 8)]
#[case("12⊤∧10", 8)]
#[case("12 (⊤∧) 10", 8)]
#[case("⊤⍱ 5", -6)]
#[case("⊤∧ 5", 5)]
#[case("⊤∨ 3.0", 3)]
#[case("⊤∧/ 3 5 7", 1)]
fn the_bitwise_family(#[case] src: &str, #[case] want: i64) {
    assert_eq!(apl(src), scalar(want), "{src}");
}

/// `12 ⊤≠ 10` used to parse as `12 ⊤ (≠10)` and answer 1. It is the one
/// spelling of the six whose second glyph HAS a monadic meaning, so it is
/// the one that was silently wrong rather than refused.
#[test]
fn the_exclusive_or_is_not_an_encode_of_a_nub_sieve() {
    assert_eq!(apl("12 ⊤≠ 10"), scalar(6));
}

/// The arguments are whole numbers that fit in 64 bits. A value merely
/// near a whole number is admitted under the comparison tolerance.
#[rstest::rstest]
#[case("⊤∧ 3.0000000000001", Some(3))]
#[case("⊤∧ 3.000000000001", None)]
#[case("⊤∧ 1e18", Some(1_000_000_000_000_000_000))]
#[case("⊤∧ 1e19", None)]
#[case("12 ⊤∧ 10.5", None)]
#[case("'a' ⊤∧ 3", None)]
fn a_bitwise_argument_is_a_whole_64_bit_number(#[case] src: &str, #[case] want: Option<i64>) {
    match want {
        Some(v) => assert_eq!(apl(src), scalar(v), "{src}"),
        None => assert_eq!(err(src).kind, ErrorKind::Domain, "{src}"),
    }
}

/// Three of the six have a monadic meaning and three do not.
#[rstest::rstest]
#[case("⊤⍲ 5")]
#[case("⊤= 5")]
#[case("⊤≠ 5")]
fn half_the_family_has_no_monad(#[case] src: &str) {
    assert!(err(src).msg.contains("no monadic meaning"), "{src}");
}

// ----------------------------------------------------- ⊤ with a width

/// `A⊤[N]B` is an encode to N copies of A, and N is a count rather than an
/// axis: `⎕IO` does not move it and `[0]` is the automatic width.
#[rstest::rstest]
#[case("2⊤[4]13", &[4], &[1, 1, 0, 1])]
#[case("2⊤[0]13", &[4], &[1, 1, 0, 1])]
#[case("2⊤[5]¯13", &[5], &[1, 0, 0, 1, 1])]
#[case("2⊤[0]¯13", &[5], &[1, 0, 0, 1, 1])]
#[case("16⊤[2]255", &[2], &[15, 15])]
#[case("10⊤[3]4321", &[3], &[3, 2, 1])]
#[case("10⊤[0]4321", &[4], &[4, 3, 2, 1])]
#[case("10⊤[0]¯5", &[2], &[9, 5])]
fn encode_to_a_width(#[case] src: &str, #[case] shape: &[usize], #[case] want: &[i64]) {
    assert_eq!(apl(src), i64s(shape, want), "{src}");
}

/// The automatic width reaches the largest value, one digit more when any
/// value is negative — the encoding is then two's complement — so an exact
/// power of the radix loses its leading digit, as the reference has it.
#[rstest::rstest]
#[case("⍴2⊤[0]1", 0)]
#[case("⍴2⊤[0]2", 1)]
#[case("⍴2⊤[0]3", 2)]
#[case("⍴2⊤[0]4", 2)]
#[case("⍴2⊤[0]¯1", 1)]
#[case("⍴2⊤[0]¯4", 3)]
#[case("⍴10⊤[0]¯50", 3)]
fn the_automatic_width(#[case] src: &str, #[case] want: i64) {
    assert_eq!(apl(src), i64s(&[1], &[want]), "{src}");
}

/// `⊤[N]` repeats ONE radix, so a left argument holding more is refused.
#[rstest::rstest]
#[case("2 3⊤[4]13", ErrorKind::Length)]
#[case("(2 2⍴2)⊤[2]13", ErrorKind::Rank)]
#[case("2.5⊤[3]13", ErrorKind::Domain)]
#[case("2⊤[0]0.5", ErrorKind::Domain)]
fn a_width_encode_takes_one_radix(#[case] src: &str, #[case] kind: ErrorKind) {
    assert_eq!(err(src).kind, kind, "{src}");
}

// ------------------------------------------------------- total ordering

/// GNU APL's comparisons are total: characters order by codepoint, a
/// character stands below every number, and a complex value orders by its
/// real part and then its imaginary one.
#[rstest::rstest]
#[case("'b'<'c'", 1)]
#[case("'q'≥'r'", 0)]
#[case("'a'<1", 1)]
#[case("1<'a'", 0)]
#[case("2>'x'", 1)]
#[case("'a'<1J2", 1)]
#[case("1J2<1J3", 1)]
#[case("1J1<1J1", 0)]
#[case("1J1<2J0", 1)]
fn comparisons_are_total(#[case] src: &str, #[case] want: u8) {
    assert_eq!(apl(src), bit(want), "{src}");
}

/// `⌈` and `⌊` are not comparisons and keep the numeric domain.
#[rstest::rstest]
#[case("'b'⌈'c'")]
#[case("'b'⌊3")]
#[case("3J4⌈1J2")]
fn the_extrema_stay_numeric(#[case] src: &str) {
    let kind = err(src).kind;
    assert!(matches!(kind, ErrorKind::Type | ErrorKind::Domain), "{src}: {kind:?}");
}

// ------------------------------------------------------ the ⊢ selection

/// `A⊢[M]B` takes B where M is 1 and A where it is 0; the three agree by
/// the ordinary scalar rule, and the two sides need not share a type.
#[test]
fn the_selection_function() {
    assert_eq!(apl("1 2 3⊢[1 0 1]4 5 6"), i64s(&[3], &[4, 2, 6]));
    assert_eq!(apl("5⊢[0]6"), scalar(5));
    assert_eq!(apl("5⊢[1]6"), scalar(6));
    assert_eq!(apl("(1 2)⊢[1](3 4)"), i64s(&[2], &[3, 4]));
    assert_eq!(apl("⍴'Q'⊢[1 0 1](1 2 3)"), i64s(&[1], &[3]));
    assert_eq!(err("(1 2)⊢[2](3 4)").kind, ErrorKind::Domain);
    assert_eq!(err("'Q'⊢[1 0 1 0](1 2 3)").kind, ErrorKind::Length);
}

// -------------------------------------------------- the matrix product

/// `A∘B` between two values is `+.×`, with a vector read as a row on the
/// left and a column on the right — so the answer is always a matrix.
#[test]
fn the_matrix_product() {
    assert_eq!(apl("(1 2 3)∘(3 2⍴⍳6)"), i64s(&[1, 2], &[22, 28]));
    assert_eq!(apl("(2 2⍴⍳4)∘(1 2)"), i64s(&[2, 1], &[5, 11]));
    assert_eq!(apl("(1 2)∘(3 4)"), i64s(&[1, 1], &[11]));
    // A mismatched inner length is padded with zeros rather than refused.
    assert_eq!(apl("(2 3⍴⍳6)∘(2 2⍴⍳4)"), i64s(&[2, 2], &[7, 10, 19, 28]));
    // A scalar operand makes it the element-wise product, at the other
    // argument's own shape.
    assert_eq!(apl("(1 2)∘5"), i64s(&[2], &[5, 10]));
    assert_eq!(apl("5∘6"), scalar(30));
    assert_eq!(err("(2 2 2⍴⍳8)∘(2 2⍴⍳4)").kind, ErrorKind::Rank);
    assert_eq!(err("'ab'∘(2 2⍴⍳4)").kind, ErrorKind::Domain);
}

// ------------------------------------------- dyadic ⍳ over a table

/// A left argument of rank 2 or more is searched element by element, and
/// each answer is the enclosed coordinate vector that finds it.
#[test]
fn a_lookup_in_a_table_answers_coordinates() {
    assert_eq!(apl("(2 2⍴⍳4)⍳3"), Array::new(vec![], Data::Box(vec![i64s(&[2], &[2, 1])].into())));
    assert_eq!(apl("≡(2 2⍴⍳4)⍳3"), scalar(2));
    assert_eq!(apl("⍴(2 3⍴⍳6)⍳4 9"), i64s(&[1], &[2]));
    // Absent is the enclosed empty vector, not a count one past the end.
    assert_eq!(
        apl("(2 2⍴⍳4)⍳9"),
        Array::new(vec![], Data::Box(vec![Array::empty(jay::DType::I64)].into()))
    );
    // A vector on the left keeps the ordinary reading.
    assert_eq!(apl("1 2 3⍳2"), scalar(2));
}

// --------------------------------------------------- lexical additions

/// `$ff` is a hexadecimal integer and one scalar, so a run of them strands
/// exactly as a run of decimal numbers does.
#[test]
fn hexadecimal_literals() {
    assert_eq!(apl("$ff"), scalar(255));
    assert_eq!(apl("$FF"), scalar(255));
    assert_eq!(apl("1+$10"), scalar(17));
    assert_eq!(apl("$10 $20"), i64s(&[2], &[16, 32]));
    assert_eq!(apl("2 $10"), i64s(&[2], &[2, 16]));
    assert_eq!(apl("⍴$1f"), Array::empty(jay::DType::I64));
    assert!(run("$g").is_err());
}

/// A double-quoted string is always a vector, where `'Q'` is a scalar, and
/// it reads the C escapes. An unknown escape keeps its backslash.
#[test]
fn double_quoted_strings() {
    assert_eq!(apl("⍴\"Q\""), i64s(&[1], &[1]));
    assert_eq!(apl("\"ab\"≡'ab'"), bit(1));
    assert_eq!(apl("⍴\"a\\nb\""), i64s(&[1], &[3]));
    assert_eq!(apl("⎕UCS \"x\\ty\""), i64s(&[3], &[120, 9, 121]));
    assert_eq!(apl("⍴\"one\" \"two\""), i64s(&[1], &[2]));
    assert_eq!(apl("⎕UCS \"a\\qb\""), i64s(&[4], &[97, 92, 113, 98]));
    assert!(run("\"unterminated").is_err());
}

// ---------------------------------------------------- the conditional

/// `test →→ body ←→ otherwise ←←`. The test is read as strictly as a dfn
/// guard's, and a conditional with no clause to run shows nothing.
#[test]
fn the_conditional() {
    assert_eq!(apl("(1=1) →→ 7 ←←"), scalar(7));
    assert_eq!(apl("(2>1) →→ 'yes' ←→ 'no' ←←"), Array::from_chars("yes".chars().collect()));
    assert_eq!(apl("(2<1) →→ 'yes' ←→ 'no' ←←"), Array::from_chars("no".chars().collect()));
    assert_eq!(apl("X←9 ⋄ (X>5) →→ X×2 ←→ X÷2 ←←"), scalar(18));
    assert_eq!(err("2 →→ 3 ←←").kind, ErrorKind::Domain);
    assert!(run("(1=1) →→ 7").is_err());
}

// -------------------------------------------- labels and the branch

/// A label names its own line number, which is what `→` takes.
#[test]
fn a_label_counts_the_line_it_stands_on() {
    let src = "∇Z←C1\nZ←0\nL:Z←Z+1\n→(Z<4)/L\n∇\nC1";
    assert_eq!(apl(src), scalar(4));
}

/// `A→B` branches A lines on from the line it stands on when B holds; a
/// step past the end of the body ends the definition.
#[rstest::rstest]
#[case("∇Z←C5\nZ←0\nL:Z←Z+1\n¯1→Z<4\n∇\nC5", 4)]
#[case("∇Z←C7 X\nZ←1\n2→X\nZ←2\n∇\nC7 0", 2)]
#[case("∇Z←C7 X\nZ←1\n2→X\nZ←2\n∇\nC7 1", 1)]
#[case("∇Z←C9\nZ←1\n2→⍬\nZ←2\n∇\nC9", 2)]
fn the_relative_branch(#[case] src: &str, #[case] want: i64) {
    assert_eq!(apl(src), scalar(want), "{src}");
}

// ------------------------------------------ stranding a specification

/// Specification binds tighter than a strand item, so an assignment's
/// value becomes one item of the vector around it.
#[test]
fn a_specification_is_one_strand_item() {
    assert_eq!(apl("⍴3 V←1 2"), i64s(&[1], &[2]));
    assert_eq!(apl("⍴3 4 V←1 2"), i64s(&[1], &[3]));
    assert_eq!(apl("1+V←5"), scalar(6));
}

// ------------------------------------------------------------- ⎕CC

/// The classes that are sets anyone can state are answered; the four GNU
/// APL states as its own glyph repertoire are named gaps.
#[test]
fn character_classes() {
    assert_eq!(apl("⍴⎕CC 1"), i64s(&[1], &[10]));
    assert_eq!(apl("⍴⎕CC 2"), i64s(&[1], &[26]));
    assert_eq!(apl("⍴⎕CC 4"), i64s(&[1], &[128]));
    assert_eq!(apl("⍴⎕CC 48"), i64s(&[1], &[48]));
    assert_eq!(apl("'5'∊⎕CC 1"), bit(1));
    assert_eq!(apl("≡⎕CC 1 2"), scalar(2));
    assert_eq!(err("⎕CC 11").kind, ErrorKind::Domain);
    assert_eq!(err("⎕CC 5").kind, ErrorKind::NotYet);
    assert_eq!(err("1 ⎕CC '7'").kind, ErrorKind::Domain);
}

// ---------------------------------------------------- the other dialect

/// Only the ordering is a dialect choice; under the Dyalog line the same
/// comparisons are refused.
#[test]
fn the_dyalog_line_keeps_the_narrow_order() {
    let p = compile(Lang::Apl, "'b'<'c'", &Dialect::dyalog()).expect("compiles");
    let mut sink = |_: &str| {};
    assert!(p.run(&[], &mut sink).is_err());
}
