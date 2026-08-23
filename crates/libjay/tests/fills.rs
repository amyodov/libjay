//! Fills, prototypes and the shape an empty keeps.
//!
//! Three rules meet here. APL fills a nested argument with the PROTOTYPE of
//! its first item and an empty nested array remembers that prototype, so
//! that `↑` of one has something to answer with. An application with no
//! cells to frame still has the shape a cell would have had, and learns it
//! by running the verb on a cell of fills. And a width written as a number
//! is a length like any other: past the ceiling it is a limit error, never
//! an allocation.
//!
//! The breadth against the references is in tests/corpus/{apl/nested.txt,
//! apl/structural.txt, j/scans.txt, j/edges.txt, j/format.txt}; this file
//! states one rule per case.

use jay::fmt::{format_array, FmtOpts};
use jay::{compile, Array, Dialect, ErrorKind, Lang};
use rstest::rstest;

fn run(lang: Lang, src: &str) -> Result<Option<Array>, jay::Error> {
    let program = compile(lang, src, &Dialect::default())?;
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink)
}

fn val(lang: Lang, src: &str) -> Array {
    run(lang, src)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn shown(lang: Lang, src: &str) -> String {
    let opts = match lang {
        Lang::J => FmtOpts::J,
        Lang::Apl => FmtOpts::APL,
    };
    format_array(&val(lang, src), &opts).trim_end().to_string()
}

fn err(lang: Lang, src: &str) -> jay::Error {
    match run(lang, src) {
        Err(e) => e,
        Ok(v) => panic!("{src:?} was expected to fail; it answered {v:?}"),
    }
}

// --------------------------------------------------- the APL prototype

/// The gap an expansion or a replication leaves in a nested argument holds
/// the first item's shape with a zero for every number and a blank for
/// every character — not the empty box J would use.
// Every `⍴¨` result here is a nested vector of 1- or 2-element shape
// vectors, so each item is itself non-scalar (rank 1) and widens the gap
// beside it by a column, the same as any other numeric vector item would.
#[rstest]
#[case("⍴¨1 0 1\\(1 2)(3 4)", " 2  2  2")]
#[case("≡1 0 1\\('abc')(1 2)", "2")]
#[case("⍴¨1 0 1\\('abc')(1 2)", " 3  3  2")]
#[case("⍴¨¯2/⊂⍳3", " 3  3")]
#[case("⍴¨¯2/(1 2)(3 4 5)", " 2  2  2  2")]
#[case("⍴¨3↑(1 2)(3 4)", " 2  2  2")]
#[case("⍴¨1 0 1\\(2 2⍴⍳4)(3 4)", " 2 2  2 2  2")]
fn a_nested_fill_is_the_prototype_of_the_first_item(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::Apl, src), want);
}

/// The prototype's own elements are zeros and blanks, nested as deeply as
/// the item was.
#[rstest]
#[case("1 0 1\\(1 2)(3 4)", " 1 2  0 0  3 4")]
#[case("¯2/⊂⍳3", " 0 0 0  0 0 0")]
#[case("1 0 1\\((1 2)(3 4))(5 6)", "  1 2  3 4    0 0  0 0   5 6")]
fn the_prototype_zeroes_numbers_and_blanks_characters(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::Apl, src), want);
}

/// An empty nested array remembers what its items looked like, whichever
/// verb emptied it, and `↑` and `⍴` answer from that memory.
#[rstest]
#[case("⍴↑0⍴⊂2 3⍴9", "2 3")]
#[case("⍴↑0/⊂2 3⍴9", "2 3")]
#[case("⍴↑0↑,⊂2 3⍴9", "2 3")]
#[case("⍴↑1↓,⊂2 3⍴9", "2 3")]
#[case("⍴↑0⍴⊂'ab'", "2")]
#[case("⍴⊃0⍴⊂2 3⍴9", "0 2 3")]
#[case("⍴¨3↑0⍴⊂2 2⍴'a'", " 2 2  2 2  2 2")]
fn an_empty_nested_array_keeps_its_prototype(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::Apl, src), want);
}

/// A scan over an empty keeps the argument's type as well as its shape, so
/// a reshape of one fills with blanks and not with zeros.
#[test]
fn an_empty_keeps_the_type_it_was_scanned_from() {
    let a = val(Lang::Apl, "4⍴×⍀''");
    assert_eq!(a.shape, vec![4]);
    assert_eq!(a.dtype(), jay::DType::Char);
    assert_eq!(shown(Lang::Apl, "⍴4⍴×⍀''"), "4");
}

/// A simple empty needs no memory: its type says what its fills are.
#[rstest]
#[case(Lang::Apl, "↑⍬", "0")]
#[case(Lang::J, "{. i. 0 3", "0 0 0")]
#[case(Lang::J, "$ {. i. 0 3", "3")]
fn a_simple_empty_fills_by_its_type(#[case] lang: Lang, #[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(lang, src), want);
}

/// J fills a nested gap with the empty box whatever the argument held, and
/// keeps doing so: the prototype is APL's rule alone.
#[test]
fn j_fills_a_box_with_the_empty_box() {
    assert_eq!(shown(Lang::J, "$ 1 0 1 #^:_1 ] 1;3"), "3");
    assert_eq!(shown(Lang::J, "1 0 1 #^:_1 ] 1 3"), "1 0 3");
    // The gap is `a:`, so its own shape is the empty list's.
    assert_eq!(shown(Lang::J, "$ > 1 { 1 0 1 #^:_1 ] 1;3"), "0");
}

// ------------------------------------------- the shape an empty keeps

/// No cells to frame is not no shape: the verb runs once on a cell of fills
/// and the answer's shape stands in for the cells there were none of.
#[rstest]
#[case("$ (,\"1) i. 0 3", "0 3")]
#[case("$ (,:\"1) i. 0 3", "0 1 3")]
#[case("$ ({.\"1) i. 0 3", "0")]
#[case("$ (2&{.)\"1 i. 0 3", "0 2")]
#[case("$ ,/\\ i. 0 3", "0 0")]
#[case("$ +/\\ i. 0 3", "0 3")]
#[case("$ +/\\. i. 0 3", "0 3")]
#[case("$ 1 >./\\. (0 3 $ 0)", "0 3")]
#[case("$ 2 (,)/\\ i. 0 3", "0 6")]
#[case("$ (,);._2 ''", "0 0")]
fn a_j_application_with_no_cells_keeps_the_cell_shape(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}

/// The same in APL, where replicate and expand work out the axis they
/// change whether or not there are rows to change it in — and where a scan
/// keeps the shape it was given, function or no function.
#[rstest]
#[case("⍴0/0 3⍴0", "0 0")]
#[case("⍴0⌿0 3⍴0", "0 3")]
#[case("⍴1 0 1/0 3⍴0", "0 2")]
#[case("⍴¯2/0 3⍴0", "0 6")]
#[case("⍴÷⍀0 3⍴0", "0 3")]
#[case("⍴+\\0 3⍴0", "0 3")]
#[case("⍴,\\''", "0")]
fn an_apl_application_with_no_cells_keeps_the_cell_shape(
    #[case] src: &str,
    #[case] want: &str,
) {
    assert_eq!(shown(Lang::Apl, src), want);
}

/// The width of an infix or an outfix is one atom, so a list of widths
/// frames the result and an empty list of them frames nothing.
#[rstest]
#[case("$ 2 3 +/\\. 1 2 3", "2 2")]
#[case("$ (0$0) +/\\. 1 2 3", "0 4")]
#[case("$ (0$0) +/\\ 1 2 3", "0 4")]
fn a_list_of_widths_frames_the_infix(#[case] src: &str, #[case] want: &str) {
    assert_eq!(shown(Lang::J, src), want);
}

// ------------------------------------------------- widths are lengths

/// A field width or a digit count is a length: past the element ceiling it
/// is a limit error naming the request, and never an allocation.
#[rstest]
#[case(Lang::J, "9223372036854775806 \": 1")]
#[case(Lang::J, "_9223372036854775806 \": 1")]
#[case(Lang::J, "9223372036854775806j2 \": 1")]
#[case(Lang::J, "1e18 \": 1 2")]
#[case(Lang::J, "9223372036854775806 \": 1 2 3")]
#[case(Lang::J, "1j9223372036854775806 \": 1")]
#[case(Lang::Apl, "9223372036854775806⍕1")]
#[case(Lang::Apl, "1 9223372036854775806⍕1")]
#[case(Lang::Apl, "9223372036854775806 2⍕1")]
fn an_absurd_field_width_is_a_limit_error(#[case] lang: Lang, #[case] src: &str) {
    let e = err(lang, src);
    assert_eq!(e.kind, ErrorKind::Limit, "{src:?}: {}", e.msg);
    assert!(e.msg.contains("ceiling"), "{src:?}: {}", e.msg);
}

/// The widths that fit still format, including the digit counts the check
/// stands next to.
#[rstest]
#[case(Lang::J, "5j2 \": 1 2 3", " 1.00 2.00 3.00")]
#[case(Lang::J, "0j2 \": 1 2", "1.00 2.00")]
#[case(Lang::Apl, "2⍕1.5", " 1.50")]
fn an_ordinary_field_width_still_formats(
    #[case] lang: Lang,
    #[case] src: &str,
    #[case] want: &str,
) {
    assert_eq!(shown(lang, src), want);
}
