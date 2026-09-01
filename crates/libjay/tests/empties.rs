//! What an argument with no elements does to a check, and what a cycle does
//! to an allocation.
//!
//! Two rules meet here. A check that applies PER ELEMENT has nothing to
//! apply to when the argument is empty, and the reference answers the empty
//! instead of refusing — but only where no cell is ever computed, so the
//! cases that go on refusing are pinned beside the ones that no longer do.
//! And a size the program asks for is checked before it is allocated:
//! `(<9223372036854775806) C. 1 2 3` is an error, not a capacity overflow.
//!
//! The corpora in tests/corpus/{j,apl}/edges.txt carry the breadth; this
//! file states the rules and compares answers on data.

use jay::{compile, Array, Dialect, Error, ErrorKind, Lang};

fn run(lang: Lang, src: &str) -> Result<Option<Array>, Error> {
    let program = compile(lang, src, &Dialect::default())?;
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink)
}

fn val(lang: Lang, src: &str) -> Array {
    run(lang, src)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn j(src: &str) -> Array {
    val(Lang::J, src)
}

fn apl(src: &str) -> Array {
    val(Lang::Apl, src)
}

fn err(lang: Lang, src: &str) -> Error {
    match run(lang, src) {
        Err(e) => e,
        Ok(v) => panic!("{src:?} answered {v:?} where a refusal was expected"),
    }
}

/// An empty answer, whatever type the empty carries. Neither reference
/// pins the type of an empty it prints as nothing, so neither does this.
fn assert_empty(lang: Lang, src: &str) {
    let a = val(lang, src);
    assert_eq!(a.count(), 0, "{src:?} answered {a:?}");
}

/// Every one of these asks for a permutation of more items than libjay will
/// ever hold. The answer is a diagnostic — the reference's is an index
/// error for the dyad and a limit error for the monad — and never a panic
/// on the way to allocating it.
#[test]
fn a_cycle_is_checked_before_its_permutation_is_allocated() {
    for src in [
        "(<9223372036854775806) C. 1 2 3",
        "(<9223372036854775806 1) C. 1 2 3",
        "(<9223372036854775806) C. (i.0)",
        "(<1000000) C. 1 2 3",
        "((<2 1);(<9223372036854775806)) C. 1 2 3",
        "C. (<9223372036854775806)",
        "C. 9223372036854775806",
        "(<4611686018427387904) C. 'abc'",
    ] {
        let e = err(Lang::J, src);
        assert!(
            matches!(e.kind, ErrorKind::Domain | ErrorKind::Limit),
            "{src:?} gave {:?}: {}",
            e.kind,
            e.msg
        );
    }
}

/// An element of a cycle is an index into the argument, and a negative one
/// counts back from the end. A cycle of one item moves nothing, which is
/// why `(<_1) C. 1 2 3` is the argument itself.
#[test]
fn a_cycle_counts_back_from_the_end() {
    assert_eq!(j("(<_1 0) C. 1 2 3"), j("3 2 1"));
    assert_eq!(j("(<0 _1) C. 1 2 3"), j("3 2 1"));
    assert_eq!(j("(<_1 _2) C. 1 2 3"), j("1 3 2"));
    assert_eq!(j("(<_1) C. 1 2 3"), j("1 2 3"));
    assert_eq!(j("(<2 1) C. 1 2 3"), j("1 3 2"));
    assert_eq!(err(Lang::J, "(<_4) C. 1 2 3").kind, ErrorKind::Domain);
    // The monad has no argument to count back through.
    assert_eq!(err(Lang::J, "C. (<_1)").kind, ErrorKind::Domain);
}

/// An outfix piece of one item applies nothing at all, so the operand's
/// domain never comes up — `2 %/\. 'abc'` is `ca`. The five folds J has
/// special code for are the exception: they type the whole argument first
/// and refuse the same characters.
#[test]
fn an_outfix_piece_of_one_item_applies_nothing() {
    for verb in ["-", "%", "!", "<", "=", "|", "^", ",", "*.", "-.", ">:"] {
        let src = format!("2 {verb}/\\. 'abc'");
        assert_eq!(j(&src), j("'ca'"), "{src}");
    }
    for verb in ["+", "*", "<.", ">.", "+."] {
        let src = format!("2 {verb}/\\. 'abc'");
        assert!(run(Lang::J, &src).is_err(), "{src} answered");
    }
    // A piece of two items really does fold, and characters really are
    // refused there.
    assert_eq!(j("3 -/\\. 'abcd'"), j("'da'"));
    assert!(run(Lang::J, "2 -/\\. 'abcd'").is_err());
    // A piece with no item at all is the fold's identity, whatever the
    // argument held.
    assert_eq!(j("_2 %/\\. 'ab'"), j(",1"));
    assert_eq!(j("2 %/\\. 'ab'"), j(",1"));
}

/// An operand with no elements takes the other side's type rather than
/// clashing with it, wherever J frames or catenates the two.
#[test]
fn an_empty_operand_takes_the_other_sides_type() {
    assert_eq!(j("(0$'a') , 1 2 3"), j("1 2 3"));
    assert_eq!(j("1 2 3 , (0$'a')"), j("1 2 3"));
    assert_eq!(j("(0 0$'a') , 1 2 3"), j("1 3$1 2 3"));
    assert_eq!(j("(0$<0) , 1 2 3"), j("1 2 3"));
    assert_eq!(j("'abc' , (0$<0)"), j("'abc'"));
    // Framing fills the empty row out with the RESULT's fill, not its own.
    assert_eq!(j("(0$'a') ,: 1 2 3"), j("2 3$0 0 0 1 2 3"));
    assert_eq!(j("(0 0$0) , 'ab'"), j("1 2$'ab'"));
    // The table applies `,` to no cell at all, and the same rule answers.
    assert_eq!(j("(0 0$0) ,/ 'ab'"), j("1 2$'ab'"));
    // A rank gap of any width is one item of the answer, filled out.
    assert_eq!(j("1 2 3 , (2 1 3$1)"), j("3 1 3$1 2 3 1 1 1 1 1 1"));
    assert_eq!(j("$ (2 1 3$1) , 1 2 3 4 5"), j("3 1 5"));
    // Two non-empty types still clash.
    assert_eq!(err(Lang::J, "1 2 , 'ab'").kind, ErrorKind::Type);
}

/// A check that reads the argument element by element has nothing to read
/// when it is empty, and answers the empty instead.
#[test]
fn a_per_element_check_vanishes_with_no_elements() {
    for src in [
        "0.5 A. i.0",
        "0.9 A. i.0",
        "_1 A. i.0",
        "_0.5 A. i.0",
        ";: (0$1 2 3)",
        ";: (i.0)",
        ";: (0 0$0)",
        "\". i.0",
        "0.5 \". i.0",
        "(0$0) <;.1 (i.0)",
        "0.5 /: i.0",
        "0.5 \\: i.0",
    ] {
        assert_empty(Lang::J, src);
    }
    // Words answers boxes, and an anagram answers the items it was given.
    assert_eq!(j(";: (0$1 2 3)"), j(";: ''"));
    assert_eq!(j("0.5 A. i.0"), j("i.0"));
    // The RANGE of an anagram index is still a range, and a character is
    // still no index at all.
    assert_eq!(err(Lang::J, "1.5 A. i.0").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "7 A. i.0").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "'a' A. i.0").kind, ErrorKind::Domain);
    // With items to permute, a fractional index is no index.
    assert_eq!(err(Lang::J, "0.5 A. ,1").kind, ErrorKind::Domain);
    // And the integral goes on reading its argument strictly.
    assert_eq!(err(Lang::J, "1 p.. (0$'a')").kind, ErrorKind::Domain);
}

/// A boxed polynomial argument is the root form, and J lets the multiplier
/// go unsaid. An empty list of roots is no root, not a type to refuse.
#[test]
fn a_boxed_polynomial_is_its_roots() {
    // Exact roots multiply out exactly, in the extended type: J reports 64
    // for all four of these.
    assert_eq!(j("p. (<1 2)"), j("2 _3 1x"));
    assert_eq!(j("p. (1;2 3)"), j("6 _5 1x"));
    assert_eq!(j("p.. (<1 2 3)"), j("11 _12 3x"));
    assert_eq!(j("p. (<i.0)"), j(",1x"));
    // An integer list, not the boolean one `,0` spells.
    assert_eq!(j("p.. (<i.0)"), j(",0 + 0"));
    assert_eq!(j("p.. (0$'a')"), j(",0 + 0"));
    assert_eq!(j("(1;0$'a') p. 4"), j("1.0"));
    assert_eq!(j("(<i.0) p. 3"), j("1.0"));
}

/// An empty fret list marks nothing, and J reads that as the whole argument
/// in one piece. An argument with no item of its own still has no piece.
#[test]
fn an_empty_fret_list_is_the_whole_argument() {
    for mode in ["1", "2", "_1"] {
        let src = format!("(0$0) <;.{mode} 'abc'");
        assert_eq!(j(&src), j(",<'abc'"), "{src}");
    }
    assert_eq!(j("(0$0) <;.1 (2 3$'a')"), j(",<2 3$'a'"));
    assert_empty(Lang::J, "(0$0) <;.1 (i.0)");
    // A fret list of a higher rank names axes, and an empty one names none.
    assert_empty(Lang::J, "(0 3$0) <;.1 'abc'");
    // Frets that are there are read as always.
    assert_empty(Lang::J, "0 0 0 <;.1 'abc'");
    assert_eq!(err(Lang::J, "1 0 <;.1 'abc'").kind, ErrorKind::Length);
    // A boxed left argument is J's per-axis form: one box of frets per
    // leading axis, and a scalar in it marks every item.
    assert_eq!(j("(<1) <;.1 'abc'"), j("<\"1 (3 1$'abc')"));
    // A box holding no fret at all leaves its axis in one piece.
    assert_eq!(j("(<0$0) <;.1 'abc'"), j(",<'abc'"));
}

/// With nothing to weigh, write or partition, APL never reads the argument
/// that says HOW, so its type is never refused.
#[test]
fn apl_reads_no_radix_and_no_flag_for_an_empty() {
    assert_eq!(apl("'a'⊥(0⍴0)"), apl("0"));
    assert_eq!(apl("'ab'⊥(0⍴0)"), apl("0"));
    assert_eq!(apl("(⊂1 2)⊥(0⍴0)"), apl("0"));
    assert_empty(Lang::Apl, "'a'⊥(0 0⍴0)");
    assert_empty(Lang::Apl, "'a'⊤(0⍴0)");
    assert_eq!(apl("⍴'ab'⊤(0⍴0)"), apl("2 0"));
    assert_empty(Lang::Apl, "(0⍴⊂⍳3)⊂(0⍴0)");
    // Where there ARE items, every check stands.
    assert_eq!(err(Lang::Apl, "'a'⊥1 2").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::Apl, "'a'⊤1 2").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::Apl, "(0⍴0.5)⊂1 2 3").kind, ErrorKind::Length);
    assert_eq!(err(Lang::Apl, "1 0⊂(0⍴0)").kind, ErrorKind::Length);
}
