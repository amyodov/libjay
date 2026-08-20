//! Boxed (J) and nested (APL) arrays, end to end in both languages: the
//! values, the structure the generic machinery gives them for free, the
//! display, and the diagnostics where a box is not allowed.
//!
//! Every meaning here was taken from the reference interpreters first; the
//! differential corpora in oracle.rs and oracle_apl.rs keep it that way.

use jay::fmt::{format_array, FmtOpts};
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

fn err(lang: Lang, src: &str) -> jay::Error {
    let program = match compile(lang, src, &Dialect::default()) {
        Err(e) => return e,
        Ok(p) => p,
    };
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink).expect_err("expected an error")
}

/// The J session's rendering of a program's value.
fn shown(lang: Lang, src: &str) -> String {
    let fmt = if lang == Lang::J { FmtOpts::J } else { FmtOpts::APL };
    format_array(&val(lang, src), &fmt)
}

fn i64s(shape: &[usize], values: &[i64]) -> Array {
    Array::new(shape.to_vec(), Data::I64(values.to_vec().into()))
}

fn bits(shape: &[usize], values: &[u8]) -> Array {
    Array::new(shape.to_vec(), Data::Bool(values.to_vec().into()))
}

fn text(shape: &[usize], s: &str) -> Array {
    Array::new(shape.to_vec(), Data::Char(s.chars().collect()))
}

/// A boxed array of the given shape.
fn boxes(shape: &[usize], items: Vec<Array>) -> Array {
    Array::new(shape.to_vec(), Data::Box(items.into()))
}

fn scalar_box(a: Array) -> Array {
    boxes(&[], vec![a])
}

/// `1;2 3;'abc'`, the worked example of the J dictionary's box entry.
fn sample() -> Array {
    boxes(
        &[3],
        vec![Array::scalar_i64(1), i64s(&[2], &[2, 3]), text(&[3], "abc")],
    )
}

// --- boxing and opening -------------------------------------------------

#[test]
fn j_box_and_open_are_inverses() {
    // `<` takes the whole argument, whatever its rank, into one box.
    assert_eq!(val(Lang::J, "< 5"), scalar_box(Array::scalar_i64(5)));
    assert_eq!(val(Lang::J, "$ < 5"), i64s(&[0], &[]));
    assert_eq!(val(Lang::J, "# < 5"), Array::scalar_i64(1));
    assert_eq!(val(Lang::J, "< i. 2 3"), scalar_box(val(Lang::J, "i. 2 3")));
    assert_eq!(val(Lang::J, "> < i. 2 3"), val(Lang::J, "i. 2 3"));
    // A box holding a box holds it whole.
    assert_eq!(val(Lang::J, "> < < 5"), val(Lang::J, "< 5"));
    // Opening something that is not boxed yields it unchanged.
    assert_eq!(val(Lang::J, "> 5"), Array::scalar_i64(5));
    assert_eq!(val(Lang::J, "> 'abc'"), text(&[3], "abc"));
}

#[test]
fn j_open_frames_the_contents_with_fill() {
    // Cells of different shapes assemble exactly as the rank machinery
    // frames any other unequal results.
    assert_eq!(val(Lang::J, "> 1;2 3"), i64s(&[2, 2], &[1, 0, 2, 3]));
    assert_eq!(val(Lang::J, "> <\"0 i. 2 3"), val(Lang::J, "i. 2 3"));
    // Character and numeric contents cannot share one array.
    let e = err(Lang::J, "> 1;'ab'");
    assert_eq!(e.kind, ErrorKind::Type);
}

#[test]
fn apl_enclose_leaves_a_simple_scalar_alone() {
    // APL's rule, and the one thing `⊂` does not share with J's `<`.
    assert_eq!(val(Lang::Apl, "⊂5"), Array::scalar_i64(5));
    assert_eq!(val(Lang::Apl, "≡⊂5"), Array::scalar_i64(0));
    assert_eq!(val(Lang::Apl, "⊂1 2 3"), scalar_box(i64s(&[3], &[1, 2, 3])));
    assert_eq!(val(Lang::Apl, "≡⊂1 2 3"), Array::scalar_i64(2));
    assert_eq!(val(Lang::Apl, "⍴⊂1 2 3"), i64s(&[0], &[]));
    // `⊂` of something already boxed boxes it again.
    assert_eq!(val(Lang::Apl, "≡⊂⊂1 2"), Array::scalar_i64(3));
}

#[test]
fn apl_disclose_mixes_the_items() {
    // GNU APL is APL2-flavoured: `⊃` discloses (J's `>`) and `↑` is first.
    assert_eq!(val(Lang::Apl, "⊃'ab' 'cd'"), text(&[2, 2], "abcd"));
    assert_eq!(val(Lang::Apl, "⊃(1 2)(3 4 5)"), i64s(&[2, 3], &[1, 2, 0, 3, 4, 5]));
    assert_eq!(val(Lang::Apl, "⊃3"), Array::scalar_i64(3));
    assert_eq!(val(Lang::Apl, "⊃⊂1 2"), i64s(&[2], &[1, 2]));
    assert_eq!(val(Lang::Apl, "⊃'abc'"), text(&[3], "abc"));
}

#[test]
fn apl_first_takes_one_element() {
    assert_eq!(val(Lang::Apl, "↑(1 2)(3 4)"), i64s(&[2], &[1, 2]));
    assert_eq!(val(Lang::Apl, "↑1 2 3"), Array::scalar_i64(1));
    // First runs over the ravel, so a matrix gives its first element.
    assert_eq!(val(Lang::Apl, "↑2 3⍴⍳6"), Array::scalar_i64(1));
    // No elements: the type's own fill stands in.
    assert_eq!(val(Lang::Apl, "↑⍳0"), Array::scalar_i64(0));
    assert_eq!(val(Lang::Apl, "↑''"), text(&[], " "));
}

// --- link and raze ------------------------------------------------------

#[test]
fn j_link_builds_a_boxed_list() {
    // `;` is right-associative and a boxed right argument is joined as it
    // is, so a chain of links is one flat list of boxes.
    assert_eq!(val(Lang::J, "1;2;3"), boxes(&[3], vec![
        Array::scalar_i64(1),
        Array::scalar_i64(2),
        Array::scalar_i64(3),
    ]));
    assert_eq!(val(Lang::J, "$ 1;2;3"), i64s(&[1], &[3]));
    assert_eq!(val(Lang::J, "1;2 3;'abc'"), sample());
    // A boxed LEFT argument is boxed again, which is what makes the
    // grouping visible: `(1;2);3` has two items, not three.
    assert_eq!(val(Lang::J, "$ (1;2);3"), i64s(&[1], &[2]));
    assert_eq!(
        val(Lang::J, "(1;2);3"),
        boxes(&[2], vec![val(Lang::J, "1;2"), Array::scalar_i64(3)])
    );
    // `1;<2` links two boxes, since the right one is already boxed.
    assert_eq!(val(Lang::J, "1;<2"), val(Lang::J, "1;2"));
    assert_eq!(val(Lang::J, "$ 'ab';'cde'"), i64s(&[1], &[2]));
}

#[test]
fn j_raze_catenates_the_opened_items() {
    assert_eq!(val(Lang::J, "; 1;2 3;4"), i64s(&[4], &[1, 2, 3, 4]));
    assert_eq!(val(Lang::J, "; 'ab';'cde'"), text(&[5], "abcde"));
    // A scalar spreads over the common item shape, as catenation does.
    assert_eq!(
        val(Lang::J, "; 1;(i. 2 3)"),
        i64s(&[3, 3], &[1, 1, 1, 0, 1, 2, 3, 4, 5])
    );
    // Items of unequal shape are padded with fill, which plain catenation
    // refuses to do.
    assert_eq!(
        val(Lang::J, "; (<i. 2 2),(<i. 2 3)"),
        i64s(&[4, 3], &[0, 1, 0, 2, 3, 0, 0, 1, 2, 3, 4, 5])
    );
    // An unboxed argument razes to its own ravel.
    assert_eq!(val(Lang::J, "; i. 2 3"), i64s(&[6], &[0, 1, 2, 3, 4, 5]));
    assert_eq!(val(Lang::J, "; 1"), i64s(&[1], &[1]));
    // Mixed contents have no common type.
    assert_eq!(err(Lang::J, ";1;2 3;'ab'").kind, ErrorKind::Type);
}

// --- each ---------------------------------------------------------------

#[test]
fn j_each_opens_applies_and_boxes_again() {
    assert_eq!(val(Lang::J, "# &.> 'ab';'cde'"), val(Lang::J, "2;3"));
    assert_eq!(val(Lang::J, "1 + &.> 1;2;3"), val(Lang::J, "2;3;4"));
    assert_eq!(val(Lang::J, "+/ &.> (<1 2 3),(<4 5)"), val(Lang::J, "6;9"));
    // The dyad pairs boxes at rank 0, so a scalar left argument extends.
    assert_eq!(
        val(Lang::J, "(1;2) ,&.> 3;4"),
        boxes(&[2], vec![i64s(&[2], &[1, 3]), i64s(&[2], &[2, 4])])
    );
    assert_eq!(
        val(Lang::J, "1 ,&.> 1;2"),
        boxes(&[2], vec![i64s(&[2], &[1, 1]), i64s(&[2], &[1, 2])])
    );
    // J always boxes the result again, even a bare number.
    assert_eq!(val(Lang::J, "+ &.> 1;2"), val(Lang::J, "1;2"));
    // `&.` over anything else still needs verb inverses.
    let e = err(Lang::J, "+ &. - 1");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("under"), "{}", e.msg);
}

#[test]
fn apl_each_keeps_simple_scalars_simple() {
    // A scalar result needs no enclosure, so `2×¨1 2 3` stays flat.
    assert_eq!(val(Lang::Apl, "2×¨1 2 3"), i64s(&[3], &[2, 4, 6]));
    assert_eq!(val(Lang::Apl, "≢¨'ab' 'cde'"), i64s(&[2], &[2, 3]));
    assert_eq!(val(Lang::Apl, "+/¨(1 2)(3 4)"), i64s(&[2], &[3, 7]));
    // A non-scalar result does need one.
    assert_eq!(
        val(Lang::Apl, "⍴¨'ab' 'cde'"),
        boxes(&[2], vec![i64s(&[1], &[2]), i64s(&[1], &[3])])
    );
    assert_eq!(
        val(Lang::Apl, "1+¨(1 2)(3 4)"),
        boxes(&[2], vec![i64s(&[2], &[2, 3]), i64s(&[2], &[4, 5])])
    );
    assert_eq!(
        val(Lang::Apl, "(1 2)+¨(1 2)(3 4)"),
        boxes(&[2], vec![i64s(&[2], &[2, 3]), i64s(&[2], &[5, 6])])
    );
    // `¨` is an operator, so it wants a function on its left.
    assert_eq!(err(Lang::Apl, "1 2¨3").kind, ErrorKind::Parse);
}

// --- vector notation ----------------------------------------------------

#[test]
fn apl_stranding_makes_vectors() {
    // Numbers written next to each other are still one simple vector.
    assert_eq!(val(Lang::Apl, "1 2 3"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(val(Lang::Apl, "≡1 2 3"), Array::scalar_i64(1));
    // Anything else juxtaposed is an item of its own.
    assert_eq!(
        val(Lang::Apl, "(1 2)(3 4)"),
        boxes(&[2], vec![i64s(&[2], &[1, 2]), i64s(&[2], &[3, 4])])
    );
    assert_eq!(val(Lang::Apl, "⍴(1 2)(3 4)"), i64s(&[1], &[2]));
    assert_eq!(val(Lang::Apl, "≡(1 2)(3 4)"), Array::scalar_i64(2));
    assert_eq!(
        val(Lang::Apl, "'ab' 'cd'"),
        boxes(&[2], vec![text(&[2], "ab"), text(&[2], "cd")])
    );
    // A numeric literal run spreads into separate items; a string does not.
    assert_eq!(val(Lang::Apl, "⍴1 2 (3 4)"), i64s(&[1], &[3]));
    assert_eq!(val(Lang::Apl, "⍴'ab' 1 2"), i64s(&[1], &[3]));
    assert_eq!(val(Lang::Apl, "⍴1 (2 3)"), i64s(&[1], &[2]));
    // A name contributes one item whatever its shape.
    assert_eq!(val(Lang::Apl, "A←1 2 ⋄ ⍴A (3 4)"), i64s(&[1], &[2]));
    // A strand is one operand, so a function to its left takes all of it.
    assert_eq!(val(Lang::Apl, "≢(1 2)(3 4)(5 6)"), Array::scalar_i64(3));
    // Simple scalars of different types would need a mixed simple array.
    let e = err(Lang::Apl, "1 'a'");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("mixing characters and numbers"), "{}", e.msg);
}

// --- enlist, depth, mix, split -----------------------------------------

#[test]
fn apl_enlist_depth_and_split() {
    assert_eq!(val(Lang::Apl, "∊(1 2)(3 4 5)"), i64s(&[5], &[1, 2, 3, 4, 5]));
    assert_eq!(val(Lang::Apl, "∊'ab' 'cd'"), text(&[4], "abcd"));
    assert_eq!(val(Lang::Apl, "∊2 3⍴⍳6"), i64s(&[6], &[1, 2, 3, 4, 5, 6]));
    // Enlist always yields a vector, even from a scalar.
    assert_eq!(val(Lang::Apl, "∊5"), i64s(&[1], &[5]));
    assert_eq!(val(Lang::Apl, "∊(1 2)((3 4)(5 6))"), i64s(&[6], &[1, 2, 3, 4, 5, 6]));

    assert_eq!(val(Lang::Apl, "≡1"), Array::scalar_i64(0));
    assert_eq!(val(Lang::Apl, "≡'abc'"), Array::scalar_i64(1));
    assert_eq!(val(Lang::Apl, "≡'ab' 'cd'"), Array::scalar_i64(2));
    assert_eq!(val(Lang::Apl, "≡1(2(3 4))"), Array::scalar_i64(3));

    // Split is what GNU APL itself has no monadic `↓` for.
    let e = err(Lang::Apl, "↓2 3⍴⍳6");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("split"), "{}", e.msg);
}

// --- the structural verbs over boxes ------------------------------------

#[test]
fn j_structure_works_on_boxes() {
    let a = "1;2 3;'abc'";
    assert_eq!(val(Lang::J, &format!("$ {a}")), i64s(&[1], &[3]));
    assert_eq!(val(Lang::J, &format!("# {a}")), Array::scalar_i64(3));
    assert_eq!(val(Lang::J, &format!("{{. {a}")), scalar_box(Array::scalar_i64(1)));
    assert_eq!(val(Lang::J, &format!("{{: {a}")), scalar_box(text(&[3], "abc")));
    assert_eq!(val(Lang::J, &format!("}}. {a}")), val(Lang::J, "(2 3);'abc'"));
    assert_eq!(val(Lang::J, &format!("}}: {a}")), val(Lang::J, "1;2 3"));
    assert_eq!(val(Lang::J, &format!("1 {{ {a}")), scalar_box(i64s(&[2], &[2, 3])));
    assert_eq!(val(Lang::J, &format!("|. {a}")), val(Lang::J, "'abc';(2 3);1"));
    assert_eq!(val(Lang::J, &format!("0 1 0 # {a}")), val(Lang::J, ", <2 3"));
    assert_eq!(val(Lang::J, &format!("2 {{. {a}")), val(Lang::J, "1;2 3"));
    assert_eq!(val(Lang::J, &format!("_1 }}. {a}")), val(Lang::J, "1;2 3"));
    assert_eq!(val(Lang::J, &format!("({a}) , <'zz'")), val(Lang::J, "1;2 3;'abc';'zz'"));
    assert_eq!(val(Lang::J, &format!("$ ,: {a}")), i64s(&[2], &[1, 3]));
    assert_eq!(val(Lang::J, &format!("$ 2 3 $ {a}")), i64s(&[2], &[2, 3]));
    assert_eq!(val(Lang::J, "$ |: 2 3 $ 1;2;3;4;5;6"), i64s(&[2], &[3, 2]));
    // Reshape reuses the ravel cyclically, boxes included.
    assert_eq!(val(Lang::J, "2 2 $ 1;2;3"), val(Lang::J, "2 2 $ 1;2;3;1"));
    // Match, nub, index-of and membership all compare contents.
    assert_eq!(val(Lang::J, "(1;2) -: (1;2)"), Array::scalar_bool(true));
    assert_eq!(val(Lang::J, "(1;2) -: (1;3)"), Array::scalar_bool(false));
    assert_eq!(val(Lang::J, "~. 1;2;1;2"), val(Lang::J, "1;2"));
    assert_eq!(val(Lang::J, "(1;2;3) i. <2"), Array::scalar_i64(1));
    assert_eq!(val(Lang::J, "(1;2;3) e. <2"), bits(&[3], &[0, 1, 0]));
    assert_eq!(val(Lang::J, "1 e. 1;2"), Array::scalar_bool(false));
    // Equality compares boxes; ordering has no meaning for them.
    assert_eq!(val(Lang::J, "(<1) = (<1)"), Array::scalar_bool(true));
    assert_eq!(val(Lang::J, "(1;2) = (1;2)"), bits(&[2], &[1, 1]));
    assert_eq!(err(Lang::J, "(<1) < (<2)").kind, ErrorKind::Type);
    // Two empty arrays match whatever their element types, so the boxed
    // fill and an empty list are the same value.
    assert_eq!(val(Lang::J, "'' -: i. 0"), Array::scalar_bool(true));
}

#[test]
fn apl_structure_works_on_nested() {
    assert_eq!(val(Lang::Apl, "⍴2 2⍴(1 2)(3 4)(5 6)(7 8)"), i64s(&[2], &[2, 2]));
    assert_eq!(val(Lang::Apl, "∪(1 2)(3 4)(1 2)"), val(Lang::Apl, "(1 2)(3 4)"));
    assert_eq!(val(Lang::Apl, ",(1 2)(3 4)"), val(Lang::Apl, "(1 2)(3 4)"));
    assert_eq!(val(Lang::Apl, "⌽(1 2)(3 4)"), val(Lang::Apl, "(3 4)(1 2)"));
    assert_eq!(val(Lang::Apl, "2↑(1 2)(3 4)(5 6)"), val(Lang::Apl, "(1 2)(3 4)"));
    assert_eq!(val(Lang::Apl, "1↓(1 2)(3 4)(5 6)"), val(Lang::Apl, "(3 4)(5 6)"));
    assert_eq!(val(Lang::Apl, "(1 2)(3 4)⍳⊂3 4"), Array::scalar_i64(2));
    assert_eq!(val(Lang::Apl, "'ab' 'cd'∊'ab' 'xy'"), bits(&[2], &[1, 0]));
    // `∊` is per element, and an element of a nested array is a whole
    // array, so a bare number is not one of them.
    assert_eq!(val(Lang::Apl, "(1 2)∊(1 2)(3 4)"), bits(&[2], &[0, 0]));
    assert_eq!(val(Lang::Apl, "(⊂1 2),⊂3 4"), val(Lang::Apl, "(1 2)(3 4)"));
}

// --- fills --------------------------------------------------------------

#[test]
fn overtaking_a_boxed_array_fills_with_the_empty_box() {
    // J's `a:` is a box holding an empty numeric list.
    let ace = scalar_box(Array::empty(jay::DType::I64));
    assert_eq!(
        val(Lang::J, "4 {. 1;2 3"),
        boxes(&[4], vec![
            Array::scalar_i64(1),
            i64s(&[2], &[2, 3]),
            Array::empty(jay::DType::I64),
            Array::empty(jay::DType::I64),
        ])
    );
    assert_eq!(val(Lang::J, "1 {. 3 {. 2 1 $ <'x'"), val(Lang::J, "1 1 $ <'x'"));
    assert_eq!(val(Lang::J, "_1 {. 3 {. 2 1 $ <'x'"), Array::new(vec![1, 1], ace.data));
    // The fill is the same value `<''` and `<i.0` denote.
    assert_eq!(val(Lang::J, "(<'') -: <i. 0"), Array::scalar_bool(true));
}

// --- display ------------------------------------------------------------

#[test]
fn j_draws_the_classic_box_table() {
    assert_eq!(shown(Lang::J, "<5"), "+-+\n|5|\n+-+");
    assert_eq!(shown(Lang::J, "1;2 3;'abc'"), "+-+---+---+\n|1|2 3|abc|\n+-+---+---+");
    // A cell's contents keep their own layout; shorter cells are padded
    // below and to the right.
    assert_eq!(
        shown(Lang::J, "1;(2 2 $ 1 2 3 4)"),
        "+-+---+\n|1|1 2|\n| |3 4|\n+-+---+"
    );
    // A nested box draws inside its cell.
    assert_eq!(shown(Lang::J, "<<5"), "+---+\n|+-+|\n||5||\n|+-+|\n+---+");
    // A matrix of boxes fences every row.
    assert_eq!(shown(Lang::J, "2 2 $ 1;2;3;4"), "+-+-+\n|1|2|\n+-+-+\n|3|4|\n+-+-+");
    // Rank 3 separates planes with a blank line, as numbers do.
    assert_eq!(
        shown(Lang::J, "<\"0 i. 2 2 2"),
        "+-+-+\n|0|1|\n+-+-+\n|2|3|\n+-+-+\n\n+-+-+\n|4|5|\n+-+-+\n|6|7|\n+-+-+"
    );
    // An empty box is a cell of width zero.
    assert_eq!(shown(Lang::J, "<''"), "++\n||\n++");
    assert_eq!(shown(Lang::J, "0 $ <1"), "");
    // `":` hands the same drawing back as characters.
    assert_eq!(val(Lang::J, "$ \": 1;2 3"), i64s(&[2], &[3, 7]));
    assert_eq!(val(Lang::J, "$ \": <5"), i64s(&[2], &[3, 3]));
    assert_eq!(val(Lang::J, "$ \": <\"0 i. 2 2 2"), i64s(&[3], &[2, 5, 5]));
}

#[test]
fn apl_spaces_a_nested_display() {
    // GNU APL's own nested display is spaced more widely; this is the
    // approximation recorded in docs/coverage.md.
    assert_eq!(shown(Lang::Apl, "(1 2)(3 4)"), " 1 2 3 4 ");
    assert_eq!(shown(Lang::Apl, "'ab' 'cd'"), " ab cd ");
    assert_eq!(shown(Lang::Apl, "⊂1 2"), " 1 2 ");
    assert_eq!(shown(Lang::Apl, "2 2⍴(1 2)(3 4)(5 6)(7 8)"), " 1 2 3 4 \n 5 6 7 8 ");
    // `⍕` of a nested vector is one line, so it stays a character vector.
    assert_eq!(val(Lang::Apl, "⍴⍕(1 2)(3 4)"), i64s(&[1], &[9]));
    assert_eq!(val(Lang::Apl, "⍴⍕⊂1 2"), i64s(&[1], &[5]));
}

// --- what a box may not do ----------------------------------------------

#[test]
fn arithmetic_on_boxes_says_to_open_them() {
    for (lang, src) in [
        (Lang::J, "1 + <1"),
        (Lang::J, "(<1) + 1"),
        (Lang::J, "- <1"),
        (Lang::J, "+/ 1;2;3"),
        (Lang::Apl, "1+⊂1 2"),
        (Lang::Apl, "-⊂1 2"),
    ] {
        let e = err(lang, src);
        assert_eq!(e.kind, ErrorKind::Type, "{src}");
        assert!(e.msg.contains("boxed"), "{src}: {}", e.msg);
        assert!(e.msg.contains("open them first"), "{src}: {}", e.msg);
    }
    // A verb that does not do arithmetic still folds over boxes.
    assert_eq!(val(Lang::J, ",/ 1;2;3"), val(Lang::J, "1;2;3"));
}

#[test]
fn grading_boxes_is_named_as_missing() {
    for (lang, src) in [(Lang::J, "/: 'b';'a'"), (Lang::J, "\\: 1;2"), (Lang::Apl, "⍋(1 2)(3 4)")] {
        let e = err(lang, src);
        assert_eq!(e.kind, ErrorKind::NotYet, "{src}");
        assert!(e.msg.contains("grading boxed arrays"), "{src}: {}", e.msg);
    }
    // Sorting boxed items BY a key that is not boxed works.
    assert_eq!(val(Lang::J, "('ab';'c';'d') /: 3 1 2"), val(Lang::J, "'c';'d';'ab'"));
}

#[test]
fn mixing_boxed_and_unboxed_is_refused_rather_than_guessed() {
    for (lang, src) in [(Lang::J, "(1;2) , 5"), (Lang::J, "1 , <2"), (Lang::Apl, "1 2,⊂3 4")] {
        let e = err(lang, src);
        assert_eq!(e.kind, ErrorKind::Type, "{src}");
        assert!(e.msg.contains("boxed"), "{src}: {}", e.msg);
    }
    // Encoding, decoding and the index generator want numbers.
    assert_eq!(err(Lang::J, "#: <5").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "i. 1;2").kind, ErrorKind::Domain);
}
