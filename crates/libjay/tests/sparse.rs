//! Sparse arrays: J's `$.`, a storage kind that holds only the positions
//! differing from one repeated element.
//!
//! The corpus in tests/corpus/j/sparse.txt carries the breadth — every
//! spelling there is checked against jconsole. This file states the rules a
//! single displayed expression cannot show: that the stored form really is
//! smaller than the array it stands for, that expanding it is exact at every
//! position, and where a sparse value stops — the amend, the boundaries and
//! the verbs that read its elements.

use rstest::rstest;

use jay::{compile, Array, DType, Data, Dialect, Error, ErrorKind, Lang};

fn run(src: &str) -> Result<Option<Array>, Error> {
    let program = compile(Lang::J, src, &Dialect::default())?;
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink)
}

fn j(src: &str) -> Array {
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

fn show(src: &str) -> String {
    jay::fmt::format_array(&j(src), &jay::fmt::FmtOpts::J)
}

// --- the stored form ------------------------------------------------------

#[rstest]
// Every axis is sparse and the entries are the non-zero positions, so the
// buffer holds one element per stored entry and not one per position.
#[case("$. 0 0 3 0 5", 5, 2)]
#[case("$. 0 0 0 0 0", 5, 0)]
#[case("$. 1 2 3", 3, 3)]
#[case("$. 3 4 $ 0 0 1 0 0 2 0 0 0 0 0 3", 12, 3)]
#[case("$. 2 2 2 $ 0 0 0 7 0 0 0 9", 8, 2)]
fn a_sparse_array_stores_only_what_differs_from_its_element(
    #[case] src: &str,
    #[case] positions: usize,
    #[case] stored: usize,
) {
    let a = j(src);
    assert!(a.is_sparse(), "{src} is not sparse");
    assert_eq!(a.shape.iter().product::<usize>(), positions);
    assert_eq!(a.data.len(), stored, "{src} stores the wrong number of elements");
    assert_eq!(a.sparse_parts().expect("sparse").entries, stored);
}

#[rstest]
#[case("0 0 3 0 5")]
#[case("1 2 3")]
#[case("0 0 0")]
#[case("i. 2 3")]
#[case("3 4 $ 0 0 1 0 0 2 0 0 0 0 0 3")]
#[case("2 2 2 $ 0 0 0 7 0 0 0 9")]
#[case("0 1.5 0 0 _2.25")]
#[case("0 3j4 0 1j_2")]
#[case("(1 0 1 0 = 1)")]
fn expanding_the_stored_form_gives_back_every_position(#[case] src: &str) {
    let dense = j(src);
    let there_and_back = j(&format!("0 $. $. {src}"));
    assert!(!there_and_back.is_sparse());
    assert_eq!(there_and_back, dense, "{src} did not survive the round trip");
    assert_eq!(there_and_back.dtype(), dense.dtype());
}

#[test]
fn a_created_sparse_array_holds_nothing_however_large_it_is() {
    let a = j("1 $. 1000 1000");
    assert!(a.is_sparse());
    assert_eq!(a.shape, vec![1000, 1000]);
    assert_eq!(a.data.len(), 0, "a new sparse array stored something");
    // The value it stands for is a million zeros, and expanding it says so.
    let dense = j("0 $. 1 $. 1000 1000");
    assert_eq!(dense.count(), 1_000_000);
    assert_eq!(dense.to_f64_vec().expect("floats").iter().sum::<f64>(), 0.0);
}

#[test]
fn the_sparse_element_is_what_every_unstored_position_holds() {
    let a = j("0 $. 1 $. (2 3) ; 0 1 ; 5");
    assert_eq!(a.shape, vec![2, 3]);
    assert_eq!(a.to_i64_vec().expect("integers"), vec![5; 6]);
}

// --- what the storage kind is visible in ----------------------------------

#[rstest]
// The dense code times 1024, which is the code J gives each sparse type.
#[case("$. 1 2 3", 4096)]
#[case("$. (1 0 1 0 = 1)", 1024)]
#[case("$. 0 1.5 0", 8192)]
#[case("$. 0 3j4 0", 16384)]
#[case("1 $. 3 4", 8192)]
#[case("0 $. $. 1 2 3", 4)]
fn the_type_foreign_reports_the_storage_kind(#[case] src: &str, #[case] code: i64) {
    assert_eq!(j(&format!("3!:0 ({src})")), Array::scalar_i64(code), "{src}");
}

#[rstest]
#[case("$. 0 0 3 0 5", "2 | 3\n4 | 5")]
// Positions and values each align in their own column.
#[case("$. 0 1.5 0 0 _2.25", "1 |   1.5\n4 | _2.25")]
#[case("$. 12 3 $ (34 $ 0) , 7 8", "11 1 | 7\n11 2 | 8")]
// Nothing stored is nothing to show, whatever the sparse element is.
#[case("$. 0 0 0", "")]
#[case("1 $. (3 4) ; 0 1 ; 5", "")]
fn the_display_shows_what_is_stored(#[case] src: &str, #[case] want: &str) {
    assert_eq!(show(src), want);
}

#[test]
fn format_gives_the_sparse_display_as_a_table_of_lines() {
    let a = j("\": $. 0 0 3 0 5");
    assert_eq!(a.dtype(), DType::Char);
    assert_eq!(a.shape, vec![2, 5]);
    assert_eq!(show("\": $. 0 0 3 0 5"), "2 | 3\n4 | 5");
}

// --- where a sparse value stops -------------------------------------------

#[rstest]
// A verb that reads elements is given every position, so its answer is the
// dense array's — including the storage kind of the result.
#[case("+/ $. 0 0 3 0 5", "8")]
#[case("|. $. 0 0 3 0 5", "5 0 3 0 0")]
#[case("($. 0 0 3 0 5) + 1", "1 1 4 1 6")]
#[case("<. / $. 0 0 3 0 5", "0")]
fn a_verb_that_reads_the_elements_gets_the_dense_array(#[case] src: &str, #[case] want: &str) {
    assert_eq!(show(src), want);
    assert!(!j(src).is_sparse(), "{src} stayed sparse");
}

#[rstest]
// Matching compares values, so the two storage kinds of one array match.
#[case("($. 0 0 3 0 5) -: 0 0 3 0 5", 1)]
#[case("(0 0 3 0 5) -: $. 0 0 3 0 5", 1)]
#[case("($. 0 0 3 0 5) -: $. 0 0 3 0 5", 1)]
#[case("($. 0 0 3 0 5) -: 0 0 3 0 6", 0)]
fn a_sparse_array_matches_the_dense_one_it_stands_for(#[case] src: &str, #[case] want: u8) {
    assert_eq!(j(src), Array::new(vec![], Data::Bool(vec![want].into())), "{src}");
}

#[test]
fn amending_a_sparse_array_writes_into_its_dense_expansion() {
    let a = j("s =. $. 0 0 3 0 5\ns =. 9 (1) } s\ns");
    assert!(!a.is_sparse());
    assert_eq!(a.to_i64_vec().expect("integers"), vec![0, 9, 3, 0, 5]);
}

#[test]
fn a_fused_chain_reads_the_dense_expansion() {
    // The chain is one the fusion pass takes; a sparse leaf must reach it
    // as a flat buffer and not as its stored entries.
    let a = j("s =. $. 0 0 3 0 5\n+/ 2 * s + 1");
    assert_eq!(a, Array::scalar_i64(26));
}

// --- the refusals ---------------------------------------------------------

#[rstest]
// J has a type code for sparse characters and sparse boxes and makes
// neither; the exact types have no sparse form at all.
#[case("$. 'abc'", ErrorKind::NotYet, "sparse array of character")]
#[case("$. (1 ; 2)", ErrorKind::NotYet, "sparse array of boxed")]
#[case("$. s: 'abc'", ErrorKind::NotYet, "sparse array of symbol")]
#[case("$. 1 2x", ErrorKind::Domain, "extended has no sparse form")]
#[case("$. 1r2 1r3", ErrorKind::Domain, "rational has no sparse form")]
// The forms that ask about the storage need something stored.
#[case("4 $. 1 2 3", ErrorKind::Domain, "reads a sparse array")]
#[case("6 $. $. 1 0 2", ErrorKind::Domain, "not a sparse form")]
#[case("(0 1) $. i. 3 4", ErrorKind::Rank, "one atom")]
// Building one: the shape and the axes both have to make sense.
#[case("1 $. _1 3", ErrorKind::Domain, "length cannot be negative")]
#[case("1 $. (2 3) ; 5", ErrorKind::Domain, "not an axis")]
#[case("1 $. (2 3) ; 0 0", ErrorKind::Domain, "names one twice")]
#[case("1 $. i. 0", ErrorKind::Length, "at least one axis")]
#[case("1 $. (3 4) ; 0 1 ; 0 ; 9", ErrorKind::Length, "made from a shape")]
fn the_refusals_say_which_rule_was_broken(
    #[case] src: &str,
    #[case] kind: ErrorKind,
    #[case] what: &str,
) {
    let e = err(src);
    assert_eq!(e.kind, kind, "{src}: {}", e.msg);
    assert!(e.msg.contains(what), "{src}: {}", e.msg);
}

#[test]
fn a_shape_past_the_element_ceiling_is_refused_where_it_is_asked_for() {
    // Nothing is allocated to make it, but every other verb would expand
    // it, so the ceiling applies at the request rather than at the first
    // use of the value.
    let e = err("1 $. 1000000 1000000 1000000");
    assert_eq!(e.kind, ErrorKind::Limit);
}
