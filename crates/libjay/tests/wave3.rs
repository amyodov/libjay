//! End-to-end tests for the third coverage wave: comparison tolerance, the
//! index-producing verbs, key and cut, amend and fetch, APL indexing and
//! partitioned enclose, matrix division, power with a function operand, and
//! roll and deal.
//!
//! Every value here was read off the reference interpreter first — jconsole
//! for J, GNU APL for APL — and the places where the two disagree are
//! asserted separately rather than averaged.

use jay::{compile, Array, Data, Dialect, ErrorKind, Lang};

fn run_dialect(lang: Lang, src: &str, dialect: &Dialect) -> Option<Array> {
    let program = compile(lang, src, dialect)
        .unwrap_or_else(|e| panic!("compile of {src:?} failed:\n{}", e.render(src)));
    let mut sink = |_: &str| {};
    program
        .run(&[], &mut sink)
        .unwrap_or_else(|e| panic!("run of {src:?} failed:\n{}", program.render_error(&e)))
}

fn val(lang: Lang, src: &str) -> Array {
    run_dialect(lang, src, &Dialect::default())
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

/// The same, with `⎕IO` set to 0 (APL only).
fn val0(src: &str) -> Array {
    run_dialect(Lang::Apl, src, &Dialect { index_origin: Some(0), ..Dialect::default() })
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn err(lang: Lang, src: &str) -> jay::Error {
    let program = match compile(lang, src, &Dialect::default()) {
        Err(e) => return e,
        Ok(p) => p,
    };
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink).expect_err("expected an error")
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

/// The whole numbers a result holds, whatever it stores them as.
fn ints(a: &Array) -> Vec<i64> {
    a.to_i64_vec().unwrap_or_else(|| panic!("expected whole numbers, got {a:?}"))
}

fn floats(a: &Array) -> Vec<f64> {
    a.to_f64_vec().unwrap_or_else(|| panic!("expected numbers, got {a:?}"))
}

/// Every value within `eps` of the one expected.
fn close(a: &Array, want: &[f64], eps: f64, what: &str) {
    let got = floats(a);
    assert_eq!(got.len(), want.len(), "{what}: {got:?} vs {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() <= eps, "{what}: {got:?} vs {want:?}");
    }
}

/// The contents of a boxed result, one string per box.
fn boxed_text(a: &Array) -> Vec<String> {
    a.as_boxes()
        .unwrap_or_else(|| panic!("expected boxes, got {a:?}"))
        .iter()
        .map(|b| match &b.data {
            Data::Char(v) => v.as_slice().iter().collect(),
            other => panic!("expected characters in a box, got {other:?}"),
        })
        .collect()
}

// --- comparison tolerance -----------------------------------------------

/// J's default tolerance is 2^-44, relative to the SMALLER magnitude and
/// strict: `1 = 1 + 2^_44` is 0 on the reference, `1 = 1 + 2^_50` is 1.
#[test]
fn j_compares_floats_with_its_default_tolerance() {
    for (src, want) in [
        ("1 = 1 + 2^_50", 1),
        ("1 = 1 + 2^_44", 0),
        ("1 = 1 + 2^_45", 1),
        ("1 = 1 - 2^_44", 0),
        ("1 = 1 - 2^_45", 1),
        ("2 = 2 + 2^_43", 0),
        ("2 = 2 + 2^_44", 1),
        ("4 = 4 + 2^_42", 0),
        ("1e10 = 1e10 + 1e_5", 1),
        ("1e10 = 1e10 + 1e_3", 0),
        // A comparison with zero is exact: nothing is near it relatively.
        ("1e_20 = 0", 0),
        ("0 = 1e_300", 0),
        // Equal infinities are equal; unequal ones are not.
        ("_ = _", 1),
        ("__ = _", 0),
        ("_ = 1e300", 0),
    ] {
        assert_eq!(ints(&val(Lang::J, src)), vec![want], "{src}");
    }
}

/// GNU APL's `⎕CT` is 1E¯13, relative to the LARGER magnitude — a genuinely
/// different rule from J's, and the reference for each is its own.
#[test]
fn apl_compares_floats_with_quad_ct() {
    for (src, want) in [
        ("1=1+1E¯15", 1),
        ("1=1+1E¯13", 1),
        ("1=1+1E¯12", 0),
        ("1=1+2E¯13", 0),
        ("0=1E¯20", 0),
        ("1<1+1E¯15", 0),
        ("1≤1+1E¯15", 1),
    ] {
        assert_eq!(ints(&val(Lang::Apl, src)), vec![want], "{src}");
    }
}

#[test]
fn the_ordering_comparisons_follow_the_tolerance() {
    assert_eq!(ints(&val(Lang::J, "1 < 1 + 2^_50")), vec![0]);
    assert_eq!(ints(&val(Lang::J, "1 <: 1 - 2^_50")), vec![1]);
    assert_eq!(ints(&val(Lang::J, "1 > 1 - 2^_50")), vec![0]);
    assert_eq!(ints(&val(Lang::J, "1 >: 1 + 2^_50")), vec![1]);
}

/// Match, nub, membership and index-of all use the same tolerance, and so
/// do the two roundings.
#[test]
fn the_searches_and_roundings_use_the_tolerance() {
    assert_eq!(ints(&val(Lang::J, "1 -: 1 + 2^_50")), vec![1]);
    assert_eq!(ints(&val(Lang::J, "~. 1 , 1 + 2^_50")), vec![1]);
    assert_eq!(ints(&val(Lang::J, "(1 + 2^_50) e. 1 2 3")), vec![1]);
    assert_eq!(ints(&val(Lang::J, "(1 2 3) i. 1 + 2^_50")), vec![0]);
    assert_eq!(ints(&val(Lang::J, "<. 2.9999999999999")), vec![3]);
    assert_eq!(ints(&val(Lang::J, ">. 3.0000000000001")), vec![3]);
    assert_eq!(ints(&val(Lang::Apl, "∪1,1+1E¯15")), vec![1]);
    assert_eq!(ints(&val(Lang::Apl, "(1+1E¯15)∊1 2")), vec![1]);
    assert_eq!(ints(&val(Lang::Apl, "1 2⍳1+1E¯15")), vec![1]);
    assert_eq!(ints(&val(Lang::Apl, "⌊2.9999999999999")), vec![3]);
    assert_eq!(ints(&val(Lang::Apl, "⌈3.0000000000001")), vec![3]);
}

/// Boxes compare their contents, so the tolerance reaches inside them.
#[test]
fn boxed_equality_is_tolerant_too() {
    assert_eq!(ints(&val(Lang::J, "(<1) = (<1 + 2^_50)")), vec![1]);
}

/// A fused chain must answer as the same chain unfused does: the kernel
/// carries the tolerance the program was compiled with.
#[test]
fn a_fused_comparison_is_tolerant() {
    // Two verbs, so the chain fuses; the sum counts the tolerant equalities.
    assert_eq!(ints(&val(Lang::J, "+/ (1 + 2^_50) = 1 1 1.5")), vec![2]);
}

/// `u!.n` replaces the tolerance for the verbs whose meaning uses one.
#[test]
fn the_fit_conjunction_sets_the_tolerance() {
    for (src, want) in [
        ("1 =!.0 (1 + 2^_50)", 0),
        ("1 <!.0 (1 + 2^_50)", 1),
        ("1 ~:!.0 (1 + 2^_50)", 1),
        ("1 >:!.0 (1 + 2^_50)", 0),
        ("1 <:!.0 (1 + 2^_50)", 1),
        ("1 >!.0 (1 + 2^_50)", 0),
        ("1 -:!.0 (1 + 2^_50)", 0),
        ("1 e.!.0 (1 , 1 + 2^_50)", 1),
    ] {
        assert_eq!(ints(&val(Lang::J, src)), vec![want], "{src}");
    }
    // Exactly compared, the two values are distinct and both survive.
    assert_eq!(val(Lang::J, "(~.!.0) 1 , 1 + 2^_50").count(), 2);
    assert_eq!(ints(&val(Lang::J, "(1 2 3) i.!.0 (1 + 2^_50)")), vec![3]);
    assert_eq!(ints(&val(Lang::J, "(<.!.0) 2.9999999999999")), vec![2]);
}

#[test]
fn fit_on_a_verb_without_a_tolerance_names_the_other_meaning() {
    let e = err(Lang::J, "4 {.!.0 (1 2)");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("fill specification"), "{}", e.msg);
}

#[test]
fn a_tolerance_beyond_the_references_limit_is_refused() {
    let e = err(Lang::J, "1 =!.(1) 5");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("tolerance"), "{}", e.msg);
}

// --- indices, interval index, steps -------------------------------------

#[test]
fn j_indices_repeats_each_index_by_its_count() {
    assert_eq!(ints(&val(Lang::J, "I. 0 1 0 2")), vec![1, 3, 3]);
    assert_eq!(ints(&val(Lang::J, "I. 1 0 1")), vec![0, 2]);
    assert!(ints(&val(Lang::J, "I. 0 0 0")).is_empty());
    assert_eq!(ints(&val(Lang::J, "I. 2 3")), vec![0, 0, 1, 1, 1]);
    assert_eq!(ints(&val(Lang::J, "I. i.5")), vec![1, 2, 2, 3, 3, 3, 4, 4, 4, 4]);
    // Rank 1, so a table frames the vector answers and fills the short ones.
    assert_eq!(val(Lang::J, "I. 2 3 $ 1 0 1 0 1 0"), i64s(&[2, 2], &[0, 2, 1, 0]));
}

#[test]
fn apl_where_counts_from_the_index_origin() {
    assert_eq!(ints(&val(Lang::Apl, "⍸1 0 1 1")), vec![1, 3, 4]);
    assert_eq!(ints(&val(Lang::Apl, "⍸0 1 0 2")), vec![2, 4, 4]);
    assert_eq!(ints(&val(Lang::Apl, "⍸2 3")), vec![1, 1, 2, 2, 2]);
    assert_eq!(ints(&val0("⍸1 0 1 1")), vec![0, 2, 3]);
    // A table answers with one boxed coordinate vector per occurrence.
    let a = val(Lang::Apl, "⍸2 2⍴1 0 0 1");
    let boxes = a.as_boxes().expect("boxed coordinates");
    assert_eq!(boxes.len(), 2);
    assert_eq!(ints(&boxes[0]), vec![1, 1]);
    assert_eq!(ints(&boxes[1]), vec![2, 2]);
}

#[test]
fn indices_refuses_anything_but_a_count() {
    for src in ["I. 0 1 0 2.5", "I. _1 2"] {
        assert_eq!(err(Lang::J, src).kind, ErrorKind::Domain, "{src}");
    }
    assert_eq!(err(Lang::Apl, "⍸1 0 1.5").kind, ErrorKind::Domain);
}

/// The interval index counts the bounds strictly below the value. J starts
/// at 0; GNU APL adds `⎕IO - 1`, so its answer moves with the origin.
#[test]
fn the_interval_index_counts_the_bounds_below() {
    assert_eq!(ints(&val(Lang::J, "1 2 3 I. 0 1 2 3 4 5")), vec![0, 0, 1, 2, 3, 3]);
    assert_eq!(ints(&val(Lang::J, "0 10 20 I. 5 15 25")), vec![1, 2, 3]);
    assert_eq!(ints(&val(Lang::Apl, "0 10 20⍸5 15 25")), vec![1, 2, 3]);
    assert_eq!(ints(&val0("0 10 20⍸5 15 25")), vec![0, 1, 2]);
}

#[test]
fn j_steps_runs_from_minus_y_to_y() {
    assert_eq!(ints(&val(Lang::J, "i: 3")), vec![-3, -2, -1, 0, 1, 2, 3]);
    assert_eq!(ints(&val(Lang::J, "i: 2")), vec![-2, -1, 0, 1, 2]);
    assert_eq!(ints(&val(Lang::J, "i: 0")), vec![0]);
    assert_eq!(ints(&val(Lang::J, "i: _3")), vec![3, 2, 1, 0, -1, -2, -3]);
    close(
        &val(Lang::J, "i: 2.5"),
        &[-2.5, -1.5, -0.5, 0.5, 1.5, 2.5],
        1e-12,
        "i: 2.5",
    );
    close(&val(Lang::J, "i: 0.5"), &[-0.5, 0.5], 1e-12, "i: 0.5");
}

#[test]
fn j_index_of_last_finds_the_last_occurrence() {
    assert_eq!(ints(&val(Lang::J, "1 2 3 2 1 i: 2")), vec![3]);
    assert_eq!(ints(&val(Lang::J, "1 2 3 2 1 i. 2")), vec![1]);
    // Absent is the item count, as `i.` reports it.
    assert_eq!(ints(&val(Lang::J, "1 2 3 2 1 i: 5")), vec![5]);
    assert_eq!(ints(&val(Lang::J, "'abcba' i: 'b'")), vec![3]);
    assert_eq!(ints(&val(Lang::J, "2 i: 3")), vec![1]);
}

// --- key and oblique ----------------------------------------------------

#[test]
fn j_key_groups_by_first_appearance() {
    assert_eq!(ints(&val(Lang::J, "1 2 1 +//. 10 20 30")), vec![40, 20]);
    assert_eq!(ints(&val(Lang::J, "1 1 2 2 #/. 'abcd'")), vec![2, 2]);
    assert_eq!(ints(&val(Lang::J, "1 2 1 {./. 10 20 30")), vec![10, 20]);
    assert_eq!(boxed_text(&val(Lang::J, "(1 2 1) </. 'abc'")), vec!["ac", "b"]);
    let a = val(Lang::J, "1 2 1 </. 10 20 30");
    let boxes = a.as_boxes().expect("boxes");
    assert_eq!(ints(&boxes[0]), vec![10, 30]);
    assert_eq!(ints(&boxes[1]), vec![20]);
}

/// The groups come out in the order their keys first appear, whatever the
/// keys are made of and however many of them there are.
#[test]
fn j_key_groups_whole_columns_in_first_appearance_order() {
    let v = val(Lang::J, "1 2 1 2 +//. 10.5 20.25 30.5 40.25");
    assert_eq!(v.to_f64_vec().expect("floats"), vec![41.0, 60.5]);
    assert_eq!(ints(&val(Lang::J, "'aab' #/. 1 2 3")), vec![2, 1]);
    // 200 items over three keys, the third of them seen first.
    let counts = val(Lang::J, "(3 | 2 + i. 200) #/. i. 200");
    assert_eq!(ints(&counts), vec![67, 67, 66]);
    let sums = val(Lang::J, "(3 | 2 + i. 200) +//. i. 200");
    assert_eq!(ints(&sums)[0], (0..200).step_by(3).sum::<i64>());
    // Rows as keys: two columns compared as one item.
    assert_eq!(ints(&val(Lang::J, "(2 2 $ 0 1 0 1) +//. 10 20")), vec![30]);
}

#[test]
fn j_oblique_runs_over_the_anti_diagonals() {
    assert_eq!(ints(&val(Lang::J, "+//. i. 3 3")), vec![0, 4, 12, 12, 8]);
    let a = val(Lang::J, "</. i. 3 3");
    let boxes = a.as_boxes().expect("boxes");
    assert_eq!(boxes.len(), 5);
    assert_eq!(ints(&boxes[2]), vec![2, 4, 6]);
    assert_eq!(boxed_text(&val(Lang::J, "</. 'abc' ,: 'def'")), vec!["a", "bd", "ce", "f"]);
}

#[test]
fn key_needs_one_key_per_item() {
    let e = err(Lang::J, "1 2 +//. 10 20 30");
    assert_eq!(e.kind, ErrorKind::Length);
}

// --- cut ----------------------------------------------------------------

#[test]
fn j_cut_splits_on_the_frets_a_boolean_marks() {
    assert_eq!(boxed_text(&val(Lang::J, "1 0 0 1 0 <;.1 'abcde'")), vec!["abc", "de"]);
    assert_eq!(boxed_text(&val(Lang::J, "1 0 0 1 0 <;._1 'abcde'")), vec!["bc", "e"]);
    assert_eq!(boxed_text(&val(Lang::J, "0 0 1 0 1 <;.2 'abcde'")), vec!["abc", "de"]);
    assert_eq!(boxed_text(&val(Lang::J, "0 0 1 0 1 <;._2 'abcde'")), vec!["ab", "d"]);
    // What comes before the first opening fret is dropped.
    assert_eq!(boxed_text(&val(Lang::J, "0 1 0 1 0 <;.1 'abcde'")), vec!["bc", "de"]);
    assert_eq!(ints(&val(Lang::J, "1 0 0 1 0 #;.1 'abcde'")), vec![3, 2]);
    assert_eq!(ints(&val(Lang::J, "1 0 0 1 0 +/;.1 ] 1 2 3 4 5")), vec![6, 9]);
}

#[test]
fn j_monadic_cut_takes_the_fret_from_the_argument() {
    assert_eq!(boxed_text(&val(Lang::J, "<;._2 'a,b,c,'")), vec!["a", "b", "c"]);
    assert_eq!(boxed_text(&val(Lang::J, "<;._1 ',a,b,c'")), vec!["a", "b", "c"]);
    // An empty field between two frets is a field.
    assert_eq!(boxed_text(&val(Lang::J, "<;._2 'a,b,,c,'")), vec!["a", "b", "", "c"]);
    // The last item is the fret whether or not it looks like a delimiter.
    assert_eq!(boxed_text(&val(Lang::J, "<;._2 'abc'")), vec!["ab"]);
}

#[test]
fn cut_zero_reverses_every_axis() {
    assert_eq!(boxed_text(&val(Lang::J, "<;.0 'abcde'")), vec!["edcba"]);
    let a = val(Lang::J, "] ;.0 i. 2 3");
    assert_eq!(a, i64s(&[2, 3], &[5, 4, 3, 2, 1, 0]));
}

#[test]
fn the_cut_modes_libjay_lacks_are_named() {
    for src in ["<;.4 'abc'", "<;._4 'abc'"] {
        let e = err(Lang::J, src);
        assert_eq!(e.kind, ErrorKind::NotYet, "{src}");
        assert!(e.msg.contains("cut"), "{src}: {}", e.msg);
    }
}

// --- amend and fetch ----------------------------------------------------

#[test]
fn j_amend_replaces_the_items_at_the_indices() {
    assert_eq!(ints(&val(Lang::J, "99 (1)} 10 20 30")), vec![10, 99, 30]);
    assert_eq!(ints(&val(Lang::J, "99 (0 2)} 10 20 30")), vec![99, 20, 99]);
    assert_eq!(ints(&val(Lang::J, "99 (_1)} 10 20 30")), vec![10, 20, 99]);
    assert_eq!(ints(&val(Lang::J, "(100 200) 0 2} 10 20 30")), vec![100, 20, 200]);
    assert_eq!(ints(&val(Lang::J, "(1 2) (0 1)} 10 20 30")), vec![1, 2, 30]);
    assert_eq!(val(Lang::J, "'x' 1} 'abc'"), text(&[3], "axc"));
    // An item of a table is a row, so the replacement is a row.
    assert_eq!(val(Lang::J, "(1 2) 0} i. 2 2"), i64s(&[2, 2], &[1, 2, 2, 3]));
    // The result holds both kinds of value, so it takes the wider type.
    close(&val(Lang::J, "1.5 (0)} 10 20 30"), &[1.5, 20.0, 30.0], 1e-12, "float amend");
    // Boxes and numbers do not mix, and neither does either with text.
    assert_eq!(err(Lang::J, "(<9) (0)} 10 20 30").kind, ErrorKind::Type);
    assert_eq!(err(Lang::J, "99 (0)} 'abc'").kind, ErrorKind::Type);
}

#[test]
fn amend_refuses_an_index_off_the_end_and_a_wrong_shape() {
    assert_eq!(err(Lang::J, "99 (5)} 10 20 30").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "(1 2 3) (0)} 10 20 30").kind, ErrorKind::Length);
}

#[test]
fn a_noun_amend_selects_when_it_is_given_one_argument() {
    assert_eq!(ints(&val(Lang::J, "1} 10 20 30")), vec![20]);
}

#[test]
fn j_fetch_opens_one_level_a_step() {
    assert_eq!(val(Lang::J, "1 {:: 'abc' ; 'de' ; 'f'"), text(&[2], "de"));
    assert_eq!(ints(&val(Lang::J, "(0;1) {:: (1 2 3);(4 5 6)")), vec![2]);
    assert_eq!(ints(&val(Lang::J, "_1 {:: 1;2;3")), vec![3]);
    // One level only: an inner box survives the step.
    let a = val(Lang::J, "0 {:: <<1 2 3");
    assert_eq!(ints(&a.as_boxes().expect("a box")[0]), vec![1, 2, 3]);
}

#[test]
fn a_fetch_path_longer_than_the_rank_is_refused() {
    assert_eq!(err(Lang::J, "1 0 {:: (1 2 3);(4 5 6)").kind, ErrorKind::Length);
}

// --- APL indexing -------------------------------------------------------

#[test]
fn apl_bracket_indexing_takes_one_slot_per_axis() {
    assert_eq!(ints(&val(Lang::Apl, "(⍳5)[2]")), vec![2]);
    assert_eq!(ints(&val(Lang::Apl, "(⍳5)[2 3]")), vec![2, 3]);
    assert_eq!(ints(&val(Lang::Apl, "A←3 3⍴⍳9 ⋄ A[2;3]")), vec![6]);
    assert_eq!(ints(&val(Lang::Apl, "A←3 3⍴⍳9 ⋄ A[2;]")), vec![4, 5, 6]);
    assert_eq!(ints(&val(Lang::Apl, "A←3 3⍴⍳9 ⋄ A[;2]")), vec![2, 5, 8]);
    assert_eq!(
        val(Lang::Apl, "A←3 3⍴⍳9 ⋄ A[1 2;2 3]"),
        i64s(&[2, 2], &[2, 3, 5, 6])
    );
    assert_eq!(val(Lang::Apl, "'abcde'[2 4]"), text(&[2], "bd"));
    assert_eq!(ints(&val(Lang::Apl, "+/(⍳5)[2 3]")), vec![5]);
    // A scalar slot drops its axis; an all-elided bracket is the array.
    assert_eq!(val(Lang::Apl, "A←3 3⍴⍳9 ⋄ A[;]"), i64s(&[3, 3], &[1, 2, 3, 4, 5, 6, 7, 8, 9]));
}

#[test]
fn bracket_indices_move_with_the_index_origin() {
    assert_eq!(val0("'abcde'[0]"), text(&[], "a"));
    assert_eq!(val0("'abcde'[2]"), text(&[], "c"));
    assert_eq!(err(Lang::Apl, "(⍳5)[0]").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::Apl, "(⍳5)[6]").kind, ErrorKind::Domain);
}

#[test]
fn a_bracket_needs_one_slot_per_axis() {
    let e = err(Lang::Apl, "A←3 3⍴⍳9 ⋄ A[2]");
    assert_eq!(e.kind, ErrorKind::Rank);
}

/// GNU APL's `⌷` is APL2's: a simple vector of one scalar index per axis,
/// and nothing else. Dyalog's enclosed-index form is a rank error there.
#[test]
fn apl_squad_takes_one_scalar_index_per_axis() {
    assert_eq!(ints(&val(Lang::Apl, "2⌷⍳5")), vec![2]);
    assert_eq!(ints(&val(Lang::Apl, "2 3⌷3 3⍴⍳9")), vec![6]);
    assert_eq!(val(Lang::Apl, "2⌷'abcde'"), text(&[], "b"));
    assert_eq!(ints(&val0("2⌷⍳5")), vec![2]);
    assert_eq!(err(Lang::Apl, "2⌷3 3⍴⍳9").kind, ErrorKind::Rank);
    assert_eq!(err(Lang::Apl, "1 2 3⌷3 3⍴⍳9").kind, ErrorKind::Rank);
}

#[test]
fn apl_axis_specification_picks_the_axis() {
    assert_eq!(ints(&val(Lang::Apl, "+/[1]2 3⍴⍳6")), vec![5, 7, 9]);
    assert_eq!(ints(&val(Lang::Apl, "+/[2]2 3⍴⍳6")), vec![6, 15]);
    assert_eq!(ints(&val(Lang::Apl, "+⌿[1]2 3⍴⍳6")), vec![5, 7, 9]);
    assert_eq!(val(Lang::Apl, "⌽[1]2 3⍴⍳6"), i64s(&[2, 3], &[4, 5, 6, 1, 2, 3]));
    assert_eq!(val(Lang::Apl, "⌽[2]2 3⍴⍳6"), i64s(&[2, 3], &[3, 2, 1, 6, 5, 4]));
    assert_eq!(val(Lang::Apl, "+\\[1]2 3⍴⍳6"), i64s(&[2, 3], &[1, 2, 3, 5, 7, 9]));
    assert_eq!(val(Lang::Apl, "+\\[2]2 3⍴⍳6"), i64s(&[2, 3], &[1, 3, 6, 4, 9, 15]));
    // The axis is named in ⎕IO origin.
    // `⍳6` starts at 0 here, so the sums do too.
    assert_eq!(ints(&val0("+/[0]2 3⍴⍳6")), vec![3, 5, 7]);
}

#[test]
fn the_axis_forms_libjay_lacks_are_named() {
    for src in [",[1]2 3⍴⍳6", "⊂[1]2 3⍴⍳6"] {
        let e = err(Lang::Apl, src);
        assert_eq!(e.kind, ErrorKind::NotYet, "{src}");
        assert!(e.msg.contains("axis specification"), "{src}: {}", e.msg);
    }
}

// --- partitioned enclose -------------------------------------------------

/// APL2's rule, which GNU APL follows: a partition opens where the left
/// argument rises, and an item flagged zero is dropped rather than joined.
#[test]
fn apl_partitioned_enclose_opens_where_the_flags_rise() {
    assert_eq!(boxed_text(&val(Lang::Apl, "1 1 0 1⊂'abcd'")), vec!["ab", "d"]);
    assert_eq!(boxed_text(&val(Lang::Apl, "1 0 0 1⊂'abcd'")), vec!["a", "d"]);
    assert_eq!(boxed_text(&val(Lang::Apl, "0 1 0 1⊂'abcd'")), vec!["b", "d"]);
    assert_eq!(boxed_text(&val(Lang::Apl, "1 1 1 1⊂'abcd'")), vec!["abcd"]);
    assert_eq!(boxed_text(&val(Lang::Apl, "1 2 0 1⊂'abcd'")), vec!["a", "b", "d"]);
    assert_eq!(boxed_text(&val(Lang::Apl, "2 1 0 1⊂'abcd'")), vec!["ab", "d"]);
    assert_eq!(boxed_text(&val(Lang::Apl, "1 1 2 2⊂'abcd'")), vec!["ab", "cd"]);
    assert_eq!(val(Lang::Apl, "0 0 0 0⊂'abcd'").shape, vec![0]);
    let a = val(Lang::Apl, "1 0 1 0⊂⍳4");
    let boxes = a.as_boxes().expect("boxes");
    assert_eq!(ints(&boxes[0]), vec![1]);
    assert_eq!(ints(&boxes[1]), vec![3]);
}

#[test]
fn partitioned_enclose_checks_its_flags() {
    assert_eq!(err(Lang::Apl, "1 0 1⊂'abcd'").kind, ErrorKind::Length);
    assert_eq!(err(Lang::Apl, "¯1 0 1 0⊂'abcd'").kind, ErrorKind::Domain);
}

// --- matrix division -----------------------------------------------------

#[test]
fn the_matrix_inverse_is_the_inverse_where_there_is_one() {
    close(&val(Lang::J, "%. 2 2 $ 1 2 3 4"), &[-2.0, 1.0, 1.5, -0.5], 1e-9, "%. 2x2");
    close(&val(Lang::Apl, "⌹2 2⍴1 2 3 4"), &[-2.0, 1.0, 1.5, -0.5], 1e-9, "⌹2x2");
    close(&val(Lang::J, "%. 2 2 $ 4 7 2 6"), &[0.6, -0.7, -0.2, 0.4], 1e-9, "%. 2x2 b");
    close(
        &val(Lang::J, "%. 3 3 $ 2 0 0 0 3 0 0 0 4"),
        &[0.5, 0.0, 0.0, 0.0, 1.0 / 3.0, 0.0, 0.0, 0.0, 0.25],
        1e-9,
        "diagonal",
    );
    // A vector is a column, so its pseudo-inverse is `y % +/ y*y`.
    close(&val(Lang::J, "%. 1 2 3"), &[1.0 / 14.0, 2.0 / 14.0, 3.0 / 14.0], 1e-9, "%. vector");
    close(&val(Lang::J, "%. 0.5"), &[2.0], 1e-12, "%. scalar");
    close(&val(Lang::Apl, "⌹0.5"), &[2.0], 1e-12, "⌹ scalar");
}

#[test]
fn matrix_divide_solves_the_least_squares_system() {
    close(&val(Lang::J, "(1 2) %. 2 2 $ 1 2 3 4"), &[0.0, 0.5], 1e-9, "exact 2x2");
    close(&val(Lang::Apl, "(1 2)⌹2 2⍴1 2 3 4"), &[0.0, 0.5], 1e-9, "APL exact");
    close(
        &val(Lang::J, "(1 0 0) %. 3 2 $ 1 1 1 2 1 3"),
        &[4.0 / 3.0, -0.5],
        1e-9,
        "overdetermined",
    );
    close(&val(Lang::J, "(1 2 3) %. 3 1 $ 1 1 1"), &[2.0], 1e-9, "one unknown");
    close(
        &val(Lang::J, "(1 2 3) %. 3 3 $ 2 0 0 0 3 0 0 0 4"),
        &[0.5, 2.0 / 3.0, 0.75],
        1e-9,
        "diagonal solve",
    );
}

#[test]
fn a_singular_or_wide_system_is_refused() {
    assert_eq!(err(Lang::J, "%. 2 2 $ 1 2 2 4").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::Apl, "⌹2 2⍴1 2 2 4").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "%. 2 3 $ i. 6").kind, ErrorKind::Length);
    assert_eq!(err(Lang::Apl, "⌹2 3⍴⍳6").kind, ErrorKind::Length);
}

// --- power with a function operand ---------------------------------------

/// J's `u^:v` asks `v` how many times to apply `u`; the while loop is that
/// verb under `^:_`. APL's `f⍣g` iterates until `new g old` holds.
#[test]
fn power_takes_its_count_from_a_verb() {
    assert_eq!(ints(&val(Lang::J, "(>:^:(2&>)) 1")), vec![2]);
    assert_eq!(ints(&val(Lang::J, "(>:^:(2&>)) 5")), vec![5]);
    assert_eq!(ints(&val(Lang::J, "({.^:(1<#)^:_) 1 2 3")), vec![1]);
    close(&val(Lang::J, "(%:^:(1&<)^:_) 1e10"), &[1.0], 1e-9, "repeated root");
}

#[test]
fn apl_power_iterates_until_the_test_holds() {
    assert_eq!(ints(&val(Lang::Apl, "(⌊⍣≡)2.5")), vec![2]);
    assert_eq!(ints(&val(Lang::Apl, "(⍴⍣≡)2 3⍴⍳6")), vec![1]);
    assert_eq!(ints(&val(Lang::Apl, "(⌽⍣2)1 2 3")), vec![1, 2, 3]);
    assert_eq!(ints(&val(Lang::Apl, "(⌽⍣3)1 2 3")), vec![3, 2, 1]);
}

#[test]
fn a_count_that_is_not_a_whole_number_is_named() {
    let e = err(Lang::J, "(>:^:(% & 2)) 5");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("count"), "{}", e.msg);
}

// --- primes --------------------------------------------------------------

#[test]
fn j_primes_and_factors() {
    assert_eq!(ints(&val(Lang::J, "p: 0")), vec![2]);
    assert_eq!(ints(&val(Lang::J, "p: i. 10")), vec![2, 3, 5, 7, 11, 13, 17, 19, 23, 29]);
    assert_eq!(ints(&val(Lang::J, "p: 100")), vec![547]);
    assert_eq!(ints(&val(Lang::J, "p: 1000")), vec![7927]);
    assert_eq!(ints(&val(Lang::J, "q: 12")), vec![2, 2, 3]);
    assert_eq!(ints(&val(Lang::J, "q: 97")), vec![97]);
    assert_eq!(ints(&val(Lang::J, "q: 1000000007")), vec![1_000_000_007]);
    assert!(ints(&val(Lang::J, "q: 1")).is_empty());
}

#[test]
fn primes_refuse_what_they_have_no_answer_for() {
    assert_eq!(err(Lang::J, "p: _1").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "q: 0").kind, ErrorKind::Domain);
    // A left argument that names no query at all is refused by name.
    assert_eq!(err(Lang::J, "5 p: 10").kind, ErrorKind::Domain);
}

// --- roll and deal -------------------------------------------------------
//
// libjay's generator is its own — neither reference publishes the stream it
// draws from — so these check the contract, not the numbers: the range, the
// distinctness of a deal, and that `?.` restarts from its fixed seed while
// `?` does not have to.

#[test]
fn a_roll_stays_inside_its_bound() {
    for _ in 0..20 {
        let v = ints(&val(Lang::J, "? 10 10 10"));
        assert_eq!(v.len(), 3);
        assert!(v.iter().all(|&x| (0..10).contains(&x)), "{v:?}");
    }
    let v = ints(&val(Lang::Apl, "?10 10 10"));
    assert!(v.iter().all(|&x| (1..=10).contains(&x)), "{v:?}");
    let v = ints(&val0("?10 10 10"));
    assert!(v.iter().all(|&x| (0..10).contains(&x)), "{v:?}");
}

#[test]
fn j_rolls_a_float_for_a_zero_bound_and_apl_refuses_one() {
    let v = floats(&val(Lang::J, "? 0"));
    assert!((0.0..1.0).contains(&v[0]), "{v:?}");
    assert_eq!(err(Lang::Apl, "?0").kind, ErrorKind::Domain);
}

#[test]
fn a_deal_draws_distinct_values() {
    let mut v = ints(&val(Lang::J, "5 ? 5"));
    v.sort_unstable();
    assert_eq!(v, vec![0, 1, 2, 3, 4]);
    let mut v = ints(&val(Lang::Apl, "10?10"));
    v.sort_unstable();
    assert_eq!(v, (1..=10).collect::<Vec<i64>>());
    let v = ints(&val(Lang::J, "6 ? 10"));
    assert_eq!(v.len(), 6);
    assert!(v.iter().all(|&x| (0..10).contains(&x)), "{v:?}");
    assert!(val(Lang::J, "0 ? 5").shape == vec![0]);
}

#[test]
fn dealing_more_than_there_is_is_refused() {
    assert_eq!(err(Lang::J, "10 ? 5").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::Apl, "11?10").kind, ErrorKind::Domain);
}

/// `?.` restarts from a fixed seed on every invocation, so the same
/// sentence always answers the same way — which is the property the
/// reference has, whatever the numbers themselves are.
#[test]
fn the_fixed_seed_roll_repeats() {
    let a = ints(&val(Lang::J, "?. 5 # 100"));
    let b = ints(&val(Lang::J, "?. 5 # 100"));
    assert_eq!(a, b);
    assert_eq!(a.len(), 5);
    assert!(a.iter().all(|&x| (0..100).contains(&x)), "{a:?}");
    // Two of five equal would be a coincidence; five of five is a stuck
    // generator, which is what this rules out.
    assert!(a.iter().any(|&x| x != a[0]), "{a:?}");
    assert_eq!(ints(&val(Lang::J, "6 ?. 10")), ints(&val(Lang::J, "6 ?. 10")));
}

// --- what the wave still lacks -------------------------------------------

#[test]
fn the_gaps_this_wave_leaves_name_themselves() {
    let cases: &[(Lang, &str, &str)] = &[
        (Lang::J, "2 s: s: <'a'", "symbol-table form"),
        (Lang::Apl, "(⍳3)∘×2", "∘ with a value operand"),
    ];
    for (lang, src, what) in cases {
        let e = err(*lang, src);
        assert_eq!(e.kind, ErrorKind::NotYet, "{src}: {}", e.msg);
        assert!(e.msg.contains(what), "{src}: {}", e.msg);
    }
}

#[test]
fn booleans_stay_booleans_through_the_new_verbs() {
    assert_eq!(val(Lang::J, "1 0 0 1 0 = 1 0 0 1 0"), bits(&[5], &[1, 1, 1, 1, 1]));
}
