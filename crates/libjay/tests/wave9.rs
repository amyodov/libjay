//! Wave 9: the inner product, APL's variant operator, the sequential
//! machine, and format-and-parse by specification.
//!
//! The corpora in tests/corpus/{j,apl}/wave9.txt carry the breadth. This
//! file states one rule per assertion — the shape rule the inner product
//! follows, the two readings of a non-scalar operand, the determinant's
//! base cases, and the one feature of the four that no oracle covers:
//! `⍠`, which GNU APL rejects outright.

use jay::{compile, Array, Data, Dialect, Error, ErrorKind, Lang};

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
        Ok(v) => panic!("{src:?} was expected to fail; it answered {v:?}"),
    }
}

fn i64s(shape: &[usize], values: &[i64]) -> Array {
    Array::new(shape.to_vec(), Data::I64(values.to_vec().into()))
}

fn chars(shape: &[usize], text: &str) -> Array {
    Array::new(shape.to_vec(), Data::Char(text.chars().collect::<Vec<_>>().into()))
}

// --- the inner product: the shape rule -----------------------------------

/// x's LAST axis pairs with y's FIRST, and what is left of each becomes the
/// shape of the answer. That is one rule for both languages and every rank.
#[test]
fn the_inner_product_pairs_the_last_axis_with_the_first() {
    assert_eq!(j("(i.2 3) +/ . * i.3 2"), i64s(&[2, 2], &[10, 13, 28, 40]));
    assert_eq!(apl("(2 3⍴⍳6)+.×3 2⍴⍳6"), i64s(&[2, 2], &[22, 28, 49, 64]));
    assert_eq!(j("$ (i.2 3 4) +/ . * i. 4 5"), i64s(&[3], &[2, 3, 5]));
    assert_eq!(apl("⍴(2 3 4⍴⍳24)+.×4 5⍴⍳20"), i64s(&[3], &[2, 3, 5]));
    // A vector on either side leaves nothing of its own behind.
    assert_eq!(j("1 2 3 +/ . * 1 2 3"), Array::scalar_i64(14));
    assert_eq!(j("(i.3 3) +/ . * i.3"), i64s(&[3], &[5, 14, 23]));
    assert_eq!(j("(i.3) +/ . * i.3 3"), i64s(&[3], &[15, 18, 21]));
}

/// A scalar stands for as many copies of itself as the shared axis wants.
#[test]
fn a_scalar_argument_is_extended_to_the_shared_axis() {
    assert_eq!(j("2 +/ . * i.3 3"), i64s(&[3], &[18, 24, 30]));
    assert_eq!(j("(i.2 2) +/ . * 5"), i64s(&[2], &[5, 25]));
    assert_eq!(apl("2+.×3 4"), Array::scalar_i64(14));
    assert_eq!(apl("(2 3⍴⍳6)+.×2"), i64s(&[2], &[12, 30]));
}

/// Axes that do not pair are refused, with both shapes named. Both
/// references call it a length error; libjay says shape where whole shapes
/// disagree and length where two lengths do, which is the rule it follows
/// everywhere else.
#[test]
fn a_mismatched_shared_axis_is_refused() {
    for src in ["(i.2 3) *./ . = i.2 3", "(i.3 2) +/ . * i.3 2"] {
        let e = err(Lang::J, src);
        assert!(matches!(e.kind, ErrorKind::Shape | ErrorKind::Length), "{src}: {:?}", e.kind);
    }
    for src in ["(⍳3)+.×2 3⍴⍳6", "(2 3⍴⍳6)∧.=2 3⍴⍳6", "(2 2⍴1 2 3 4)+.,3 2⍴⍳6"] {
        let e = err(Lang::Apl, src);
        assert!(matches!(e.kind, ErrorKind::Shape | ErrorKind::Length), "{src}: {:?}", e.kind);
    }
}

/// An empty shared axis is the empty fold, which is what `u` answers with
/// no items: zero for `+/`, and the answer keeps the frame.
#[test]
fn an_empty_shared_axis_folds_over_nothing() {
    assert_eq!(j("(2 0 $ 0) +/ . * (0 3 $ 0)"), i64s(&[2, 3], &[0, 0, 0, 0, 0, 0]));
    assert_eq!(apl("(⍳0)+.×⍳0"), Array::scalar_i64(0));
}

/// Whole numbers in, whole numbers out — the fast path answers in the type
/// the general one would.
#[test]
fn a_whole_matrix_product_stays_whole() {
    assert_eq!(j("3!:0 (i.2 3) +/ . * i.3 2"), Array::scalar_i64(4));
    assert_eq!(j("(i.2 3) +/ . * i.3 2").dtype(), jay::DType::I64);
    // A product that leaves i64 goes to floats, as every other integer
    // primitive does.
    let big = j("(2 2 $ 4e18) +/ . * 2 2 $ 4e18");
    assert_eq!(big.dtype(), jay::DType::F64);
}

/// The two languages read a non-scalar `v` differently, and both are here.
/// J hands the whole of y to v once per row of x; APL pairs each row with
/// each column, both as vectors.
#[test]
fn a_non_scalar_operand_parts_the_two_languages() {
    // J: `x , y` overtakes both to three columns and catenates, and `+/`
    // folds the five rows that makes.
    assert_eq!(j("(i.2 3) +/ . , i.3 2"), i64s(&[3], &[9, 14, 7]));
    // APL: row `,` column is a four-element vector, and `+/` folds it.
    assert_eq!(apl("(2 2⍴1 2 3 4)+.,2 2⍴1 2 3 4"), i64s(&[2, 2], &[7, 9, 11, 13]));
}

/// `u` is applied MONADICALLY to what `v` made, so it need not fold at all.
#[test]
fn u_is_applied_monadically_and_need_not_reduce() {
    assert_eq!(j("$ (i.2 3) <. . * i.3 2"), i64s(&[3], &[2, 3, 2]));
    assert_eq!(j("$ (i.2 3) ,/ . * i.3 2"), i64s(&[2], &[2, 6]));
}

/// APL gives the inner product no monadic valence at all.
#[test]
fn an_apl_inner_product_is_dyadic_only() {
    assert_eq!(err(Lang::Apl, "+.×⍳3").kind, ErrorKind::Domain);
}

// --- the determinant ------------------------------------------------------

/// `u . v y` expands down the FIRST column: each row's leading element
/// under v with the determinant of what the row and the column leave, all
/// folded by u. `-/ . *` is the determinant proper.
#[test]
fn the_determinant_expands_down_the_first_column() {
    assert_eq!(j("-/ . * 2 2 $ 1 2 3 4"), Array::scalar_i64(-2));
    assert_eq!(j("-/ . * 3 3 $ 2 0 1 1 3 2 1 1 1"), Array::scalar_f64(0.0));
    // The same expansion with `+` in place of `-` adds the minors instead.
    assert_eq!(j("+/ . * 3 3 $ 2 0 1 1 3 2 1 1 1"), Array::scalar_i64(14));
    assert_eq!(j("-/ . + 2 2 $ 1 2 3 4"), Array::scalar_i64(0));
    assert_eq!(j("*/ . + 2 2 $ 1 2 3 4"), Array::scalar_i64(25));
}

/// The base cases: ONE column left is u applied to that column, no columns
/// at all is v's identity element, and no rows left is u over nothing. The
/// identity elements are boolean, as both references report them.
#[test]
fn the_determinant_bottoms_out_at_the_identity_elements() {
    assert_eq!(j("-/ . * 0 0 $ 0"), Array::scalar_bool(true));
    assert_eq!(j("-/ . + 0 0 $ 0"), Array::scalar_bool(false));
    assert_eq!(j("-/ . * 2 0 $ 0"), Array::scalar_bool(true));
    assert_eq!(j("-/ . * 0 2 $ 0"), Array::scalar_bool(false));
    assert_eq!(j("-/ . * 1 1 $ 7"), Array::scalar_i64(7));
    // A single column is the fold of that column; an argument of rank 1 or
    // 0 is read as one.
    assert_eq!(j("-/ . * 2 1 $ 5 6"), Array::scalar_i64(-1));
    assert_eq!(j("-/ . * 1 2 3"), Array::scalar_i64(2));
    assert_eq!(j("-/ . * 5"), Array::scalar_i64(5));
    // The base case carries the column's own VALUES, so a u that is not an
    // insert sees them rather than a vector of identity elements.
    assert_eq!(j("*: . > 1 2"), i64s(&[2], &[1, 4]));
    assert_eq!(j("$ . > 1 2"), i64s(&[1], &[2]));
    assert_eq!(
        j("< . > 'ab'"),
        Array::new(Vec::new(), Data::Box(vec![chars(&[2], "ab")].into()))
    );
    assert_eq!(j("*: . > 2 2 $ 1 2 3 4"), i64s(&[2, 1], &[0, 0]));
}

/// The verb's monadic rank is 2, so an argument of higher rank gives one
/// determinant per table.
#[test]
fn the_determinant_frames_a_higher_rank_argument() {
    assert_eq!(j("-/ . * i. 2 2 2"), i64s(&[2], &[-2, -2]));
}

/// The exact types keep their exactness: an extended argument is expanded
/// by minors rather than eliminated in floats.
#[test]
fn an_exact_determinant_stays_exact() {
    let exact = j("-/ . * 3 3 $ 1x 2 3 4 5 6 7 8 10");
    assert_eq!(exact.dtype(), jay::DType::Ext);
    assert_eq!(j("_1 x: -/ . * 3 3 $ 1x 2 3 4 5 6 7 8 10"), Array::scalar_i64(-3));
}

/// Expansion by minors is exponential, so a large table with no direct
/// method names the limit rather than running for ever.
#[test]
fn a_large_determinant_by_minors_names_the_limit() {
    let e = err(Lang::J, "+/ . * i. 17 17");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("determinant"), "{}", e.msg);
}

// --- APL's variant --------------------------------------------------------

/// `f⍠B` overrides one dialect setting for one application. A bare number
/// is the principal option, which is the comparison tolerance.
#[test]
fn a_variant_overrides_the_comparison_tolerance() {
    assert_eq!(apl("1 = 1+1E¯14"), Array::scalar_bool(true));
    assert_eq!(apl("1 (=⍠0) 1+1E¯14"), Array::scalar_bool(false));
    assert_eq!(apl("1 (=⍠('CT' 0)) 1+1E¯14"), Array::scalar_bool(false));
    assert_eq!(apl("1 (=⍠1E¯10) 1+1E¯14"), Array::scalar_bool(true));
    // The dialect's own setting is untouched by the application that
    // overrode it.
    assert_eq!(apl("Z←1(=⍠0)1+1E¯14 ⋄ 1=1+1E¯14"), Array::scalar_bool(true));
}

/// `⍠('IO' n)` derives the verb again with the other index origin, in both
/// valences and under an operator.
#[test]
fn a_variant_overrides_the_index_origin() {
    assert_eq!(apl("⍳5"), i64s(&[5], &[1, 2, 3, 4, 5]));
    assert_eq!(apl("(⍳⍠('IO' 0))5"), i64s(&[5], &[0, 1, 2, 3, 4]));
    assert_eq!(apl("(⍋⍠('IO' 0))3 1 2"), i64s(&[3], &[1, 2, 0]));
    assert_eq!(apl("(⍸⍠('IO' 0))0 1 0 1"), i64s(&[2], &[1, 3]));
    assert_eq!(apl("2 3(⍳⍠('IO' 0))3"), Array::scalar_i64(1));
    // Several options at once, applied left to right.
    assert_eq!(apl("(⍳⍠('IO' 0)('IO' 1))5"), i64s(&[5], &[1, 2, 3, 4, 5]));
}

/// A setting the verb does not have is not one of its options, and a
/// variant that is not settled when the program is compiled is a named gap.
#[test]
fn a_variant_refuses_what_is_not_an_option() {
    assert_eq!(err(Lang::Apl, "1(+⍠0)2").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::Apl, "(⍳⍠('IO' 2))5").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::Apl, "(+⍠('IO' 0))1 2").kind, ErrorKind::Domain);
    let named = err(Lang::Apl, "(⍳⍠('ZZ' 0))5");
    assert_eq!(named.kind, ErrorKind::NotYet);
    assert!(named.msg.contains("ZZ"), "{}", named.msg);
    let computed = err(Lang::Apl, "1(=⍠A)2");
    assert_eq!(computed.kind, ErrorKind::NotYet);
    assert!(computed.msg.contains("computed variant option"), "{}", computed.msg);
}

// --- the sequential machine ----------------------------------------------

/// The classic word-splitting machine: state 1 is between words, state 0 is
/// inside one, and code 3 ends a word.
const WORDS: &str =
    "(0;(2 2 2 $ 0 0 1 3 0 1 1 0);(' ' = a.);0 _1 1 _1) ;: ";

#[test]
fn the_sequential_machine_marks_off_words() {
    let boxes = j(&format!("{WORDS}'  ab cd  ef  '"));
    assert_eq!(boxes.dtype(), jay::DType::Box);
    assert_eq!(boxes.shape, vec![3]);
    assert_eq!(j(&format!("> {WORDS}'  ab cd  ef  '")), chars(&[3, 2], "abcdef"));
    // The end of the input ends the word in hand.
    assert_eq!(j(&format!("> {WORDS}'  ab cd  ef'")), chars(&[3, 2], "abcdef"));
}

/// Each result form answers a different question about the same run.
#[test]
fn the_result_form_picks_what_the_machine_answers() {
    let with = |f: i64| j(&format!("({f};(2 2 2 $ 0 0 1 3 0 1 1 0);(' ' = a.);0 _1 1 _1) ;: '  ab cd  ef  '"));
    assert_eq!(with(1), chars(&[6], "abcdef"));
    assert_eq!(with(2), i64s(&[3, 2], &[2, 2, 5, 2, 9, 2]));
    assert_eq!(with(3), i64s(&[3], &[1, 1, 1]));
    assert_eq!(with(4), i64s(&[3, 3], &[2, 2, 1, 5, 2, 1, 9, 2, 1]));
    // The trace is one row per transition: i, the word's start, the state,
    // the class, and the table entry those two chose.
    let trace = with(5);
    assert_eq!(trace.shape, vec![13, 6]);
    assert_eq!(trace.cell_at(1, 4), i64s(&[6], &[4, 2, 0, 1, 1, 3]));
}

/// The fourth box says where to start and what the end of the input means:
/// `_1` ends the word in hand, a class makes one last transition instead.
#[test]
fn the_starting_values_say_what_the_end_of_the_input_does() {
    let ending = |d: &str| {
        j(&format!(
            "# (0;(2 2 2 $ 0 0 1 3 0 1 1 0);(' ' = a.);0 _1 1 {d}) ;: '  ab cd  ef'",
        ))
    };
    assert_eq!(ending("_1"), Array::scalar_i64(3));
    // Class 0 from state 0 is "still inside a word", so nothing is emitted.
    assert_eq!(ending("0"), Array::scalar_i64(2));
    // Class 1 is a blank, which ends it.
    assert_eq!(ending("1"), Array::scalar_i64(3));
    // The starting position skips what is in front of it.
    assert_eq!(
        j(&format!("> {WORDS}'  ab cd  ef  '")),
        j("> (0;(2 2 2 $ 0 0 1 3 0 1 1 0);(' ' = a.);2 _1 1 _1) ;: '  ab cd  ef  '")
    );
}

/// What the machine refuses, and the one output code libjay has not
/// reached.
#[test]
fn the_sequential_machine_names_what_it_will_not_do() {
    // A word ended before one began.
    assert_eq!(
        err(Lang::J, "(0;(2 2 2 $ 0 0 1 3 0 1 1 0);(' ' = a.)) ;: 'ab cd'").kind,
        ErrorKind::Domain
    );
    // A left argument that is not the boxed description.
    assert_eq!(err(Lang::J, "2 ;: 'a b'").kind, ErrorKind::Domain);
    // A table of the wrong shape.
    assert_eq!(err(Lang::J, "(0;(2 2 $ 0);(' ' = a.)) ;: 'a b'").kind, ErrorKind::Rank);
    let vector = err(
        Lang::J,
        "(0;(2 2 2 $ 0 0 1 4 0 1 1 0);(' ' = a.);0 _1 1 _1) ;: 'a b'",
    );
    assert_eq!(vector.kind, ErrorKind::NotYet);
    assert!(vector.msg.contains("vector output"), "{}", vector.msg);
}

// --- format and parse by specification -----------------------------------

/// `w j d` is a field `w` wide with `d` digits after the point; a width of
/// zero is what the column needs, with a blank in front of every column but
/// the first.
#[test]
fn format_by_specification_lays_out_one_field_per_column() {
    assert_eq!(j("5j2 \": 1.5"), chars(&[5], " 1.50"));
    assert_eq!(j("(5j2, 8j3) \": 2 2 $ 1.5 2.25 3 4"), chars(&[2, 13], " 1.50   2.250 3.00   4.000"));
    assert_eq!(j("0 \": 2 2 $ 1 22 333 4"), chars(&[2, 6], "  1 22333  4"));
    assert_eq!(j("$ 0 \": 1 2 3"), i64s(&[1], &[5]));
    // Half goes to even, which is what the reference's printer does.
    assert_eq!(j("2j0 \": 1.5 2.5 3.5"), chars(&[6], " 2 2 4"));
}

/// A negative width asks for the exponential form, written from the left
/// behind one column of sign; a value too wide for its field is asterisks
/// rather than a refusal.
#[test]
fn a_field_that_does_not_fit_is_written_as_asterisks() {
    assert_eq!(j("_9j2 \": 1.5"), chars(&[9], " 1.50e0  "));
    assert_eq!(j("_12j3 \": _1500.25"), chars(&[12], "_1.500e3    "));
    assert_eq!(j("_12j3 \": 0.00012345"), chars(&[12], " 1.234e_4   "));
    assert_eq!(j("_6j2 \": 1.5"), chars(&[6], "******"));
    assert_eq!(j("5j2 \": 12345.678"), chars(&[5], "*****"));
    // A value that rounds to nothing keeps no sign.
    assert_eq!(j("5j2 \": _0.001"), chars(&[5], " 0.00"));
}

/// Characters have no format by specification, and one specification per
/// column is the only count other than one.
#[test]
fn format_by_specification_refuses_what_it_cannot_lay_out() {
    assert_eq!(err(Lang::J, "5 \": 'abc'").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "5j2 \": < 1 2").kind, ErrorKind::Domain);
    assert_eq!(
        err(Lang::J, "(5j2 , 8j3 , 4j1) \": 2 2 $ 1.5 2.25 3 4").kind,
        ErrorKind::Length
    );
}

/// `x ". y` reads the numbers a line spells, with x standing in for a word
/// that is not one. One word gives a scalar, as reading the line as a noun
/// would.
#[test]
fn reading_numbers_out_of_text_stands_in_for_what_it_cannot_read() {
    assert_eq!(j("0 \". '1 2 3'"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(j("_1 \". '1 2 x 3'"), i64s(&[4], &[1, 2, -1, 3]));
    assert_eq!(j("0 \". 'abc'"), Array::scalar_bool(false));
    assert_eq!(j("99 \". 'oops'"), Array::scalar_i64(99));
    assert_eq!(j("$ 0 \". '1'"), i64s(&[0], &[]));
    assert_eq!(j("$ 0 \". ''"), i64s(&[1], &[0]));
    // The whole J numeric vocabulary is read, not just the digits.
    assert_eq!(j("0 \". '_3.5e2'"), Array::scalar_f64(-350.0));
    // A matrix is read a row at a time, and the rows are framed with fills.
    assert_eq!(j("0 \". 2 3 $ '1 234 5'"), i64s(&[2, 2], &[1, 2, 34, 0]));
    // The stand-in is one value.
    assert_eq!(err(Lang::J, "1 2 3 \". 'a b'").kind, ErrorKind::Rank);
}
