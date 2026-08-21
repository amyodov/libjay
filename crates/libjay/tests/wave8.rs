//! Ordering whole arrays: what `/:` and `\:` do to a boxed argument, and
//! what `⍋` and `⍒` do to a nested one.
//!
//! The two languages order differently, and neither ordering is the other
//! read backwards, so both are pinned here rule by rule. Everything an
//! oracle answers is also in tests/corpus/{j,apl}/grade.txt; this file
//! states each rule as one assertion, and carries the dialect setting that
//! names Dyalog's total array ordering as a gap.

use jay::frontend::NestedGrade;
use jay::{compile, Array, Data, Dialect, ErrorKind, Lang};

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

fn i64s(values: &[i64]) -> Array {
    Array::new(vec![values.len()], Data::I64(values.to_vec().into()))
}

/// A J grade, which counts from zero.
fn j(src: &str) -> Array {
    val(Lang::J, src)
}

/// An APL grade, which counts from `⎕IO`.
fn apl(src: &str) -> Array {
    val(Lang::Apl, src)
}

// --- J: the total array ordering ----------------------------------------

/// The type class comes first: numeric, then character, then boxed. It
/// beats the rank, the shape and the contents alike.
#[test]
fn j_orders_by_type_class_before_anything_else() {
    assert_eq!(j("/: 1;'a';(1 2);(<<3)"), i64s(&[0, 2, 1, 3]));
    assert_eq!(j("/: 'b';1;'a';2"), i64s(&[1, 3, 2, 0]));
    // A one-atom character list still sorts after a three-atom numeric one.
    assert_eq!(j("/: (<'ab'),(<1 2 3)"), i64s(&[1, 0]));
    // The class of a box is the box's, not its contents'.
    assert_eq!(j("/: (<a:),(<1)"), i64s(&[1, 0]));
    assert_eq!(j("/: (<a:),(<'')"), i64s(&[1, 0]));
    // Complex values are numeric, ordered by real part then imaginary.
    assert_eq!(j("/: (<1j1),(<'a')"), i64s(&[0, 1]));
    assert_eq!(j("/: (<1j2),(<1j1)"), i64s(&[1, 0]));
}

/// Within a class, the rank, ascending — whatever the shapes say.
#[test]
fn j_orders_by_rank_before_shape() {
    assert_eq!(j("/: (i.0);1"), i64s(&[1, 0]));
    assert_eq!(j("/: (1 1$1);(1 2)"), i64s(&[1, 0]));
    assert_eq!(j("/: (2 2 2$0);(3 3$0)"), i64s(&[1, 0]));
    assert_eq!(j("/: (<i.0),(<2 2$1)"), i64s(&[0, 1]));
}

/// Then the shape, read with the LAST axis most significant. This is where
/// the rule most often surprises: `2 3` sorts after `3 2`.
#[test]
fn j_reads_the_shape_from_the_last_axis() {
    assert_eq!(j("/: (2 3$0);(3 2$0)"), i64s(&[1, 0]));
    assert_eq!(j("/: (1 6$0);(6 1$0)"), i64s(&[1, 0]));
    assert_eq!(j("/: (0 3$0);(3 0$0)"), i64s(&[1, 0]));
    assert_eq!(j("/: (2 1 3$0);(1 2 3$0)"), i64s(&[0, 1]));
    // The element COUNT does not decide: 4 atoms sort after 6 of them.
    assert_eq!(j("/: (1 4$0);(2 3$0)"), i64s(&[1, 0]));
}

/// Then the atoms, in row-major order; a boxed atom recurses.
#[test]
fn j_compares_the_atoms_last_and_recurses_through_boxes() {
    assert_eq!(j("/: 'abc';'abd'"), i64s(&[0, 1]));
    assert_eq!(j("/: (<2 2$0 1 2 3),(<2 2$0 1 2 4)"), i64s(&[0, 1]));
    assert_eq!(j("/: (<<1 2),(<<1 3)"), i64s(&[0, 1]));
    // One box deeper, the same rules apply from the top again.
    assert_eq!(j("/: (<<1 2),(<<1)"), i64s(&[1, 0]));
    assert_eq!(j("/: (<<'a'),(<<1)"), i64s(&[1, 0]));
    assert_eq!(j("/: (<<1),(<<<1)"), i64s(&[0, 1]));
    assert_eq!(j("/: (<1;2),(<'a';'b')"), i64s(&[0, 1]));
    // The comparison is exact, never tolerant, and a NaN ties with all.
    assert_eq!(j("/: (<1),(<1+1e_14)"), i64s(&[0, 1]));
    assert_eq!(j("/: (<1+1e_14),(<1)"), i64s(&[1, 0]));
    assert_eq!(j("/: (<1r3),(<0.3333333333)"), i64s(&[1, 0]));
    assert_eq!(j("/: (<_.),(<1)"), i64s(&[0, 1]));
    assert_eq!(j("/: (<1),(<_.)"), i64s(&[0, 1]));
}

/// An EMPTY array has no atoms to take a class from, so it takes the
/// lowest one whatever its type. Its rank and shape still decide.
#[test]
fn j_gives_an_empty_array_the_lowest_class() {
    // Two empties of different types tie, and a stable sort keeps them.
    assert_eq!(j("/: (<0$'a'),(<i.0)"), i64s(&[0, 1]));
    assert_eq!(j("/: (<i.0),(<0$'a')"), i64s(&[0, 1]));
    assert_eq!(j("/: (<0$0),(<0$a:),(<0$'a')"), i64s(&[0, 1, 2]));
    // An empty character list sorts below a box and below a numeric list,
    // but a numeric SCALAR still outranks it: rank 0 before rank 1.
    assert_eq!(j("/: (<''),(<<1)"), i64s(&[0, 1]));
    assert_eq!(j("/: (<''),(<1 2)"), i64s(&[0, 1]));
    assert_eq!(j("/: (<''),(<1)"), i64s(&[1, 0]));
    assert_eq!(j("/: (<''),(<'a')"), i64s(&[0, 1]));
    // Which leaves one order over the three, not a cycle.
    assert_eq!(j("/: (<'a'),(<1 2),(<'')"), i64s(&[2, 1, 0]));
}

/// A grade is a stable permutation, ascending and descending alike, and
/// `\:` is not `|. /:` — equal items keep their order in both.
#[test]
fn j_grades_down_stably() {
    assert_eq!(j("/: (<'a'),(<'a'),(<'b')"), i64s(&[0, 1, 2]));
    assert_eq!(j("\\: (<'a'),(<'a'),(<'b')"), i64s(&[2, 0, 1]));
    assert_eq!(j("\\: (<1),(<1),(<2)"), i64s(&[2, 0, 1]));
    assert_eq!(j("\\: 1;'a';(1 2);(<<3)"), i64s(&[3, 1, 2, 0]));
}

/// The ordering reaches whatever is built on a grade: the sort idiom, the
/// dyad that selects with one, and a boxed array of rank 2, whose ROWS are
/// the items being ordered.
#[test]
fn j_sorts_and_selects_with_the_same_ordering() {
    assert_eq!(j("/:~ 'b';'aa';1"), j("1;'b';'aa'"));
    assert_eq!(j("\\:~ 'b';'aa';1"), j("'aa';'b';1"));
    assert_eq!(j("(1 2 3) /: ('b';'a';'c')"), i64s(&[2, 1, 3]));
    assert_eq!(j("(1 2 3) \\: ('b';'a';'c')"), i64s(&[3, 1, 2]));
    assert_eq!(j("'abc' /: ('b';'a';'c')"), val(Lang::J, "'bac'"));
    assert_eq!(j("/: 2 2$(<1),(<2),(<0),(<3)"), i64s(&[1, 0]));
    assert_eq!(j("/: 2 2$(<1),(<2),(<1),(<1)"), i64s(&[1, 0]));
    // A boxed scalar has one item, so its grade is the one permutation.
    assert_eq!(j("/: <1"), i64s(&[0]));
    assert_eq!(j("\\: <1"), i64s(&[0]));
}

/// The ordering is the one `/:` reports, and nothing else changed hands:
/// `~.`, `e.` and `i.` still compare boxes by MATCH, which is tolerant
/// where the ordering is exact.
#[test]
fn j_leaves_the_matching_verbs_alone() {
    assert_eq!(j("~. 'a';1;'a';1"), j("'a';1"));
    assert_eq!(j("(<1) e. 1;'a'"), Array::scalar_bool(true));
    assert_eq!(j("(1;'a') i. <'a'"), Array::scalar_i64(1));
    // Tolerantly equal boxes match; the ordering keeps them apart.
    assert_eq!(j("(<1) e. ,<1+1e_14"), Array::scalar_bool(true));
    assert_eq!(j("/: (<1+1e_14),(<1)"), i64s(&[1, 0]));
}

// --- APL: the APL2 ordering ---------------------------------------------

/// APL2 reads the rank first — before the type and before the shape.
#[test]
fn apl_orders_by_rank_first() {
    assert_eq!(apl("⍋(1)(1 2)"), i64s(&[1, 2]));
    assert_eq!(apl("⍋(1 1⍴1)(1 2)"), i64s(&[2, 1]));
    assert_eq!(apl("⍋(2 2 2⍴0)(3 3⍴0)"), i64s(&[2, 1]));
    // A nested scalar outranks a simple vector, which is the plainest
    // place J and APL2 part company.
    assert_eq!(apl("⍋(⊂1 2)(1 2)"), i64s(&[1, 2]));
    assert_eq!(apl("⍋(2 2⍴'ab')('')"), i64s(&[2, 1]));
}

/// Then the shape, read from the FIRST axis — J reads it from the last.
#[test]
fn apl_reads_the_shape_from_the_first_axis() {
    assert_eq!(apl("⍋(1 4⍴0)(2 3⍴0)"), i64s(&[1, 2]));
    assert_eq!(apl("⍋(2 3⍴1)(3 2⍴0)"), i64s(&[1, 2]));
    assert_eq!(apl("⍋(2 0⍴0)(0 2⍴0)"), i64s(&[2, 1]));
    assert_eq!(apl("⍋(1 2)(1 2 3)"), i64s(&[1, 2]));
    // The shape beats the type: a longer character list sorts after a
    // shorter numeric one, though characters come first.
    assert_eq!(apl("⍋('abc')(1 2)"), i64s(&[2, 1]));
}

/// Then the atoms: a character precedes a number precedes a nested value.
#[test]
fn apl_puts_characters_before_numbers_before_nested_values() {
    assert_eq!(apl("⍋1 'a'"), i64s(&[2, 1]));
    assert_eq!(apl("⍋('a')(1)"), i64s(&[1, 2]));
    assert_eq!(apl("⍋('ab')(1 2)"), i64s(&[1, 2]));
    assert_eq!(apl("⍋(⊂1 2)(3)"), i64s(&[2, 1]));
    assert_eq!(apl("⍋(⊂1 2)('a')"), i64s(&[2, 1]));
    assert_eq!(apl("⍋(1 'a')(1 1)"), i64s(&[1, 2]));
    assert_eq!(apl("⍋(⊂1 2)(⊂'ab')"), i64s(&[2, 1]));
    // One whole ordering over five items of five different shapes.
    assert_eq!(apl("⍋(1 2)('ab')(⊂1 2)(1)('a')"), i64s(&[5, 4, 3, 2, 1]));
    assert_eq!(apl("⍒(1 2)('ab')(⊂1 2)(1)('a')"), i64s(&[1, 2, 3, 4, 5]));
}

/// Two arrays with no atoms are separated by their types instead — which
/// is where APL2 and J differ again: J ties them.
#[test]
fn apl_separates_two_empties_by_type() {
    assert_eq!(apl("⍋('')(⍳0)"), i64s(&[1, 2]));
    assert_eq!(apl("⍋(⍳0)('')"), i64s(&[2, 1]));
    assert_eq!(apl("⍋(0⍴⊂1 2)(⍳0)"), i64s(&[2, 1]));
    assert_eq!(apl("⍋(0⍴⊂1 2)('')"), i64s(&[2, 1]));
    // And one level down, inside two enclosures of the same shape.
    assert_eq!(apl("⍋(⊂'')(⊂⍳0)"), i64s(&[1, 2]));
}

/// A nested grade is stable, and it reaches the rows of a nested matrix
/// and the sort idiom built on it.
#[test]
fn apl_grades_nested_rows_stably() {
    assert_eq!(apl("⍋(1 2)(3 4)(1 2)"), i64s(&[1, 3, 2]));
    assert_eq!(apl("⍒(1 2)(3 4)(1 2)"), i64s(&[2, 1, 3]));
    assert_eq!(apl("⍋3 2⍴(1 2)(3 4)(0 5)(1 1)(2 2)(3 3)"), i64s(&[2, 1, 3]));
    assert_eq!(
        apl("⊃((1 2)(3 4)(0 5))[⍋(1 2)(3 4)(0 5)]"),
        apl("3 2⍴0 5 1 2 3 4")
    );
    // A grade orders items, and an enclosed array is one scalar.
    let e = compile(Lang::Apl, "⍋⊂1 2", &Dialect::default())
        .expect("it compiles")
        .run(&[], &mut |_: &str| {})
        .expect_err("a scalar has no items to order");
    assert_eq!(e.kind, ErrorKind::Domain);
}

/// Dyalog's total array ordering is a different comparator at every step —
/// it compares the atoms first, pads the shorter array with an item below
/// every type, extends a lower rank with leading 1s, and puts numbers
/// before characters. libjay implements the APL2 rule its oracle answers
/// with; asking for the other one is refused by name rather than answered
/// with this dialect's ordering.
#[test]
fn the_other_lineages_nested_ordering_is_a_named_gap() {
    assert_eq!(Dialect::default().nested_grade, NestedGrade::Apl2);
    let dyalog = Dialect { nested_grade: NestedGrade::TotalOrder, ..Dialect::default() };
    let e = compile(Lang::Apl, "⍋(1 2)(3 4)", &dyalog).expect_err("the other reading is a gap");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("total array ordering"), "{}", e.msg);
    assert!(e.msg.contains("not supported yet"), "{}", e.msg);
    // The setting is the APL lineages'; J's ordering is not a choice, and
    // a dialect that asks for the other reading is refused whichever
    // language it is resolved against, as every other APL setting is.
    let e = compile(Lang::J, "/: 'b';'a'", &dyalog).expect_err("the setting is read for J too");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert_eq!(j("/: 'b';'a'"), i64s(&[1, 0]));
}
