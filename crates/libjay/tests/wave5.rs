//! End-to-end tests for the fifth coverage wave: the map, J's index
//! specifications, amend with a verb operand, tessellation and the
//! rectangle cut, the fill shift, fix, memo, the levels, the polynomial
//! verbs and the boolean functions; APL's index generator on a shape,
//! mixed simple arrays, prototype fills, the Dyalog operators, the branch
//! and the niladic definition.
//!
//! The differential evidence is in tests/corpus/{j,apl}/wave5.txt. This
//! file carries what no oracle covers: the rules only Dyalog states (`⊆`,
//! `⍛`, `f⍤g`, `⌸`, dfn operators), the exact text of the new diagnostics,
//! the types J reports and libjay now matches, and the gaps this wave
//! leaves named.

use jay::{compile, Array, DType, Data, Dialect, ErrorKind, Lang};

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

fn i64s(shape: &[usize], values: &[i64]) -> Array {
    Array::new(shape.to_vec(), Data::I64(values.to_vec().into()))
}

fn f64s(shape: &[usize], values: &[f64]) -> Array {
    Array::new(shape.to_vec(), Data::F64(values.to_vec().into()))
}

fn text(shape: &[usize], s: &str) -> Array {
    Array::new(shape.to_vec(), Data::Char(s.chars().collect()))
}

fn boxes(shape: &[usize], items: Vec<Array>) -> Array {
    Array::new(shape.to_vec(), Data::Box(items.into()))
}

fn bools(shape: &[usize], values: &[u8]) -> Array {
    Array::new(shape.to_vec(), Data::Bool(values.to_vec().into()))
}

/// The values of a numeric array, as floats, for comparisons with a
/// tolerance.
fn floats(a: &Array) -> Vec<f64> {
    a.to_f64_vec().unwrap_or_else(|| panic!("expected numbers, got {:?}", a.dtype()))
}

fn close(got: &[f64], want: &[f64], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: {got:?} vs {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < 1e-9, "{what}: {got:?} vs {want:?}");
    }
}

// --- J: the map -----------------------------------------------------------

#[test]
fn the_map_replaces_every_leaf_by_its_path() {
    // A path is a boxed list of one index per level descended.
    assert_eq!(
        val(Lang::J, "{:: 1;2;3"),
        boxes(
            &[3],
            (0..3)
                .map(|i| boxes(&[1], vec![Array::from_i64(vec![i])]))
                .collect()
        )
    );
    // An unboxed argument is one leaf, itself, and its path is empty.
    assert_eq!(val(Lang::J, "{:: 1 2 3"), Array::empty(DType::I64));
    // A boxed scalar contributes an empty index: nothing chooses among the
    // one thing it holds.
    assert_eq!(
        val(Lang::J, "{:: <1 2 3"),
        boxes(&[], vec![boxes(&[1], vec![Array::empty(DType::I64)])])
    );
    // The index within a rank-2 array is the whole coordinate vector.
    let paths = val(Lang::J, "{:: 2 2$1;2;3;4");
    assert_eq!(paths.shape, vec![2, 2]);
    let last = &paths.as_boxes().expect("boxed")[3];
    assert_eq!(*last, boxes(&[1], vec![i64s(&[2], &[1, 1])]));
    // What the map answers is what fetch takes: the path to the first
    // leaf of `(1;2);3` is `0;0`, and fetching with it gives that leaf.
    let map = val(Lang::J, "{:: (1;2);3");
    let first = &map.as_boxes().expect("boxed")[0].as_boxes().expect("boxed")[0];
    assert_eq!(
        *first,
        boxes(&[2], vec![Array::from_i64(vec![0]), Array::from_i64(vec![0])])
    );
    assert_eq!(val(Lang::J, "(0;0) {:: (1;2);3"), Array::scalar_bool(true));
}

// --- J: index specifications ----------------------------------------------

#[test]
fn a_boxed_index_reaches_several_axes_at_once() {
    assert_eq!(val(Lang::J, "(<1 2) { i.3 3"), Array::scalar_i64(5));
    assert_eq!(val(Lang::J, "(<0 1;2) { i.3 3"), i64s(&[2], &[2, 5]));
    // The empty box selects a whole axis, being the complement of nothing.
    assert_eq!(val(Lang::J, "(<a:;1) { i.3 3"), i64s(&[3], &[1, 4, 7]));
    // A component that is itself boxed is the complement: every index of
    // the axis but the ones it holds.
    assert_eq!(val(Lang::J, "(<(<<1)) { i.3 3"), i64s(&[2, 3], &[0, 1, 2, 6, 7, 8]));
    // One box less selects that index instead of all the others.
    assert_eq!(val(Lang::J, "(<(<1)) { i.3 3"), i64s(&[3], &[3, 4, 5]));
    // A rank-2 component's last axis is one index vector per cell.
    assert_eq!(val(Lang::J, "$ (<(2 2$0 1 1 2)) { i.3 3"), i64s(&[1], &[2]));
    // Out of range and too deep are named separately.
    assert_eq!(err(Lang::J, "(<9) { i.3 3").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "(<0 0 0 0) { i.3 3").kind, ErrorKind::Rank);
    assert!(err(Lang::J, "(<0 0 0 0) { i.3 3").msg.contains("index specification"));
}

#[test]
fn amend_takes_the_same_specification() {
    assert_eq!(val(Lang::J, "99 (<1;2)} i.3 3"), i64s(&[3, 3], &[0, 1, 2, 3, 4, 99, 6, 7, 8]));
    assert_eq!(
        val(Lang::J, "99 (<a:;1)} i.3 3"),
        i64s(&[3, 3], &[0, 99, 2, 3, 99, 5, 6, 99, 8])
    );
    // One replacement per selected cell, or one spread over all of them.
    assert_eq!(
        val(Lang::J, "10 20 30 (<a:;1)} i.3 3"),
        i64s(&[3, 3], &[0, 10, 2, 3, 20, 5, 6, 30, 8])
    );
    let e = err(Lang::J, "10 20 (<a:;1)} i.3 3");
    assert_eq!(e.kind, ErrorKind::Length);
    assert!(e.msg.contains("cell"), "{}", e.msg);
}

#[test]
fn amend_with_a_verb_operand_computes_the_indices() {
    // `u} y` is `(u y)} y`, and `x u} y` is `x (x u y)} y`.
    assert_eq!(val(Lang::J, "(<.@-:@#)} i.5"), Array::scalar_i64(2));
    assert_eq!(val(Lang::J, "10 20 (i.@#@[)} 0 1 2 3"), i64s(&[4], &[10, 20, 2, 3]));
    // The indices u computes have to be indices.
    assert_eq!(err(Lang::J, "99 +} i.5").kind, ErrorKind::Domain);
}

// --- J: cutting -----------------------------------------------------------

#[test]
fn tessellation_moves_a_block_over_the_argument() {
    // `;._3` takes only the whole blocks, `;.3` the short edge ones too.
    assert_eq!(val(Lang::J, "$ 2 2 <;._3 i.4 4"), i64s(&[2], &[3, 3]));
    assert_eq!(val(Lang::J, "$ 2 2 <;.3 i.4 4"), i64s(&[2], &[4, 4]));
    // Two rows are the movement and the size.
    assert_eq!(val(Lang::J, "$ (2 2,:2 2) <;.3 i.4 4"), i64s(&[2], &[2, 2]));
    assert_eq!(val(Lang::J, "2 2 (+/@,);._3 i.3 3"), i64s(&[2, 2], &[8, 12, 20, 24]));
    // Only the axes the sizes cover are cut.
    assert_eq!(val(Lang::J, "$ 2 2 <;.3 i.3 4 5"), i64s(&[2], &[3, 4]));
    // With the movement row left out a negative size does not measure the
    // block: it runs to the end of its axis and comes back reversed, so
    // the magnitude plays no part.
    assert_eq!(val(Lang::J, "_2 <;.3 i.5"), val(Lang::J, "_3 <;.3 i.5"));
    assert_eq!(val(Lang::J, "$ _2 <;._3 i.5"), i64s(&[1], &[1]));
    // A positive step is required.
    assert_eq!(err(Lang::J, "(0 0,:2 2) <;.3 i.4 4").kind, ErrorKind::Domain);
}

#[test]
fn the_rectangle_cut_takes_one_block() {
    assert_eq!(val(Lang::J, "(1 1,:2 2) <;.0 i.4 4"), boxes(&[], vec![i64s(&[2, 2], &[5, 6, 9, 10])]));
    // A negative size reverses that axis.
    assert_eq!(
        val(Lang::J, "(1 1,:_2 2) ];.0 i.4 4"),
        i64s(&[2, 2], &[9, 10, 5, 6])
    );
    // A block that runs off the end is a domain error, not a short block.
    assert_eq!(err(Lang::J, "(3 3,:2 2) <;.0 i.4 4").kind, ErrorKind::Domain);
}

#[test]
fn the_fill_shift_drops_what_moves_past_an_end() {
    assert_eq!(val(Lang::J, "1 |.!.0 i.5"), i64s(&[5], &[1, 2, 3, 4, 0]));
    assert_eq!(val(Lang::J, "_1 |.!.' ' 'abcde'"), text(&[5], " abcd"));
    // The monad shifts by one: `|.!.f y` is `_1 |.!.f y`.
    assert_eq!(val(Lang::J, "|.!.0 i.5"), i64s(&[5], &[0, 0, 1, 2, 3]));
    // The fill and the argument have to be of one kind.
    assert_eq!(err(Lang::J, "1 |.!.0 'abc'").kind, ErrorKind::Type);
}

// --- J: fix, memo and the levels ------------------------------------------

#[test]
fn fix_is_the_verb_itself() {
    // Names are substituted where they are used, so a fixed verb is the
    // verb: `f.` changes what it means to nobody.
    assert_eq!(val(Lang::J, "(+/ % #) f. 1 2 3"), Array::scalar_f64(2.0));
    assert_eq!(val(Lang::J, "mean =. +/ % #\nmean f. 1 2 3 4"), Array::scalar_f64(2.5));
}

#[test]
fn memo_answers_the_same_arguments_from_its_cache() {
    // The cache belongs to the derived verb, so it survives from one
    // application to the next within the program.
    assert_eq!(val(Lang::J, "f =. *: M.\n(f 5) + f 5"), Array::scalar_i64(50));
    assert_eq!(val(Lang::J, "(+/ M.) i. 5"), Array::scalar_i64(10));
    assert_eq!(val(Lang::J, "2 (+ M.) 3"), Array::scalar_i64(5));
    // Different arguments are different keys.
    assert_eq!(val(Lang::J, "f =. *: M.\n(f 3) , f 4"), i64s(&[2], &[9, 16]));
}

#[test]
fn the_levels_apply_a_verb_at_a_boxing_depth() {
    // `L: 0` reaches the leaves and puts each answer back in its box.
    assert_eq!(
        val(Lang::J, "# L: 0 (1 2;3 4 5)"),
        boxes(&[2], vec![Array::scalar_i64(2), Array::scalar_i64(3)])
    );
    // A level at or above the argument's own applies to the whole of it.
    assert_eq!(val(Lang::J, "# L: 1 (1 2;3 4)"), Array::scalar_i64(2));
    // `S:` spreads the answers into the items of one array instead.
    assert_eq!(val(Lang::J, "# S: 0 ((1 2;3 4);5)"), i64s(&[3], &[2, 2, 1]));
    // A negative level counts down from the argument's own top.
    assert_eq!(
        val(Lang::J, "# L: _1 (1 2;3 4)"),
        boxes(&[2], vec![Array::scalar_i64(2), Array::scalar_i64(2)])
    );
    // The dyadic level landed in wave 6; tests/wave6.rs carries it.
    assert_eq!(
        val(Lang::J, "2 (+ L: 0) (1;2)"),
        boxes(&[2], vec![Array::scalar_i64(3), Array::scalar_i64(4)])
    );
}

// --- J: the polynomial verbs ----------------------------------------------

#[test]
fn a_polynomial_evaluates_at_its_argument() {
    assert_eq!(val(Lang::J, "2 3 p. 5"), Array::scalar_f64(17.0));
    close(&floats(&val(Lang::J, "1 2 3 p. 0 1 2")), &[1.0, 6.0, 17.0], "Horner");
    // The boxed form is `multiplier ; roots`.
    assert_eq!(val(Lang::J, "(2;1 0.5) p. 3"), Array::scalar_f64(10.0));
    let e = err(Lang::J, "(1;2;3) p. 4");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("multiplier"), "{}", e.msg);
}

#[test]
fn the_monad_converts_between_coefficients_and_roots() {
    let roots = val(Lang::J, "p. 1 _3 2");
    let parts = roots.as_boxes().expect("boxed");
    assert_eq!(parts.len(), 2);
    close(&floats(&parts[0]), &[2.0], "multiplier");
    close(&floats(&parts[1]), &[1.0, 0.5], "roots");
    // Complex roots come out in J's order: descending by real part, and
    // within a conjugate pair the positive imaginary part first.
    let cx = val(Lang::J, "p. 1 2 3");
    let pair = &cx.as_boxes().expect("boxed")[1];
    assert_eq!(pair.dtype(), DType::Complex);
    // The boxed form converts back to coefficients, exactly.
    close(&floats(&val(Lang::J, "p. 2;1 0.5")), &[1.0, -3.0, 2.0], "coefficients");
    close(&floats(&val(Lang::J, "p. p. 1 _3 2")), &[1.0, -3.0, 2.0], "round trip");
    // A constant has no roots to find.
    assert_eq!(err(Lang::J, "p. 5").kind, ErrorKind::Domain);
}

/// A repeated root is exact, and stays real. Durand–Kerner reaches a root
/// of multiplicity m only to about the m-th root of the machine epsilon,
/// so `p. 1 2 1` used to answer a pair of complex values 1e¯9 either side
/// of ¯1 where jconsole answers `_1 _1`.
#[test]
fn a_repeated_root_comes_out_exact() {
    for (src, want) in [
        ("p. 1 2 1", vec![-1.0, -1.0]),
        ("p. 1 _2 1", vec![1.0, 1.0]),
        ("p. 4 4 1", vec![-2.0, -2.0]),
        ("p. 1 3 3 1", vec![-1.0, -1.0, -1.0]),
        ("p. 1 4 6 4 1", vec![-1.0; 4]),
        ("p. 1 5 10 10 5 1", vec![-1.0; 5]),
        ("p. 1 0 _2 0 1", vec![1.0, 1.0, -1.0, -1.0]),
        ("p. 0 1 2 1", vec![-1.0, -1.0, 0.0]),
    ] {
        let pair = &val(Lang::J, src).as_boxes().expect("boxed")[1].clone();
        assert_ne!(pair.dtype(), DType::Complex, "{src} kept an imaginary residue");
        assert_eq!(floats(pair), want, "{src}");
    }
    // The coefficients come back from the roots unchanged.
    close(&floats(&val(Lang::J, "p. p. 1 3 3 1")), &[1.0, 3.0, 3.0, 1.0], "round trip");
}

/// Root order is J's: the largest magnitude first, then the largest real
/// part, then the largest imaginary part. `¯3` comes before `2`, and a
/// conjugate pair keeps its positive half in front.
#[test]
fn roots_come_out_in_js_order() {
    for (src, want) in [
        ("p. _6 1 1", vec![-3.0, 2.0]),
        ("p. 6 11 6 1", vec![-3.0, -2.0, -1.0]),
        ("p. 24 _50 35 _10 1", vec![4.0, 3.0, 2.0, 1.0]),
        ("p. 0 6 5 1", vec![-3.0, -2.0, 0.0]),
        ("p. _4 0 1", vec![2.0, -2.0]),
        ("p. 0 _1 0 1", vec![1.0, -1.0, 0.0]),
    ] {
        let pair = &val(Lang::J, src).as_boxes().expect("boxed")[1].clone();
        close(&floats(pair), &want, src);
    }
    // x⁴ = 1: four roots of one magnitude, ordered by real then imaginary.
    let quartic = val(Lang::J, "p. _1 0 0 0 1");
    let roots = &quartic.as_boxes().expect("boxed")[1];
    assert_eq!(jay::fmt::format_array(roots, &jay::fmt::FmtOpts::J), "1 0j1 0j_1 _1");
}

#[test]
fn the_derivative_and_the_integral_are_coefficient_vectors() {
    assert_eq!(val(Lang::J, "p.. 1 2 3"), i64s(&[2], &[2, 6]));
    assert_eq!(val(Lang::J, "p.. 5"), i64s(&[1], &[0]));
    // The left argument of the integral is the constant term.
    assert_eq!(val(Lang::J, "_1 p.. 1 2 3"), i64s(&[4], &[-1, 1, 1, 1]));
    close(&floats(&val(Lang::J, "0 p.. 0 1")), &[0.0, 0.0, 0.5], "integral");
    // Differentiating the integral gives the coefficients back.
    assert_eq!(val(Lang::J, "p.. 0 p.. 1 2 3"), i64s(&[3], &[1, 2, 3]));
}

// --- J: the boolean functions ---------------------------------------------

#[test]
fn the_boolean_functions_are_numbered_by_their_truth_table() {
    // 0 to 15 are the functions of two bits; 16 higher is the same
    // function on every bit of a pair of integers.
    assert_eq!(val(Lang::J, "1 (1 b.) 1"), Array::scalar_bool(true));
    assert_eq!(val(Lang::J, "1 (1 b.) 0"), Array::scalar_bool(false));
    assert_eq!(val(Lang::J, "1 (6 b.) 0"), Array::scalar_bool(true));
    assert_eq!(val(Lang::J, "5 (17 b.) 3"), Array::scalar_i64(1));
    assert_eq!(val(Lang::J, "5 (23 b.) 3"), Array::scalar_i64(7));
    assert_eq!(val(Lang::J, "5 (22 b.) 3"), Array::scalar_i64(6));
    assert_eq!(val(Lang::J, "_5 (17 b.) 3"), Array::scalar_i64(3));
    // Below 16 the arguments have to be bits, and the message says where
    // the other sixteen are.
    let e = err(Lang::J, "5 (1 b.) 3");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("16"), "{}", e.msg);
    // A verb operand asks after the verb: `0` is its three ranks.
    assert_eq!(val(Lang::J, "+ b. 0"), f64s(&[3], &[0.0, 0.0, 0.0]));
    assert_eq!(
        val(Lang::J, "(+/) b. 0"),
        f64s(&[3], &[f64::INFINITY, f64::INFINITY, f64::INFINITY])
    );
    let e = err(Lang::J, "+ b. 2");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("characteristic"), "{}", e.msg);
}

// --- J: the exact types carry through the counting verbs ------------------

#[test]
fn a_count_of_an_exact_argument_is_exact_too() {
    // J reports these as extended, and libjay now matches: same values,
    // same type, which is what `3!:0` sees there.
    for src in ["# 1x 2x 3x", "$ 1x 2x", "#. 1x 1x", "#: 5x", "p: 3x", "q: 12x"] {
        assert_eq!(val(Lang::J, src).dtype(), DType::Ext, "{src}");
    }
    // A machine argument still answers with machine numbers.
    assert_eq!(val(Lang::J, "# 1 2 3").dtype(), DType::I64);
    assert_eq!(val(Lang::J, "$ 1 2").dtype(), DType::I64);
    // A rational argument counts as exact as well.
    assert_eq!(val(Lang::J, "# 1r2 3r4").dtype(), DType::Ext);
    // The values are unchanged.
    assert_eq!(val(Lang::J, "_1 x: q: 12x"), i64s(&[3], &[2, 2, 3]));
}

// --- APL: the index generator on a shape ----------------------------------

#[test]
fn the_index_generator_on_a_shape_gives_coordinate_vectors() {
    let a = val(Lang::Apl, "⍳2 3");
    assert_eq!(a.shape, vec![2, 3]);
    assert_eq!(a.as_boxes().expect("boxed")[4], i64s(&[2], &[2, 2]));
    // One length is the plain counting vector, whatever its rank.
    assert_eq!(val(Lang::Apl, "⍳,3"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(val(Lang::Apl, "⍳3"), i64s(&[3], &[1, 2, 3]));
    // A shape is a scalar or a vector; nothing deeper.
    assert_eq!(err(Lang::Apl, "⍳2 2⍴2").kind, ErrorKind::Rank);
    assert_eq!(err(Lang::Apl, "⍳¯1").kind, ErrorKind::Domain);
}

// --- APL: mixed simple arrays ---------------------------------------------

#[test]
fn a_mixed_simple_array_has_depth_one() {
    let a = val(Lang::Apl, "1 'a'");
    assert_eq!(a, boxes(&[2], vec![Array::scalar_i64(1), text(&[], "a")]));
    assert_eq!(val(Lang::Apl, "≡1 'a'"), Array::scalar_i64(1));
    assert_eq!(val(Lang::Apl, "⍴1 'a'"), i64s(&[1], &[2]));
    // Disclosing one changes nothing: every element is already a scalar.
    assert_eq!(val(Lang::Apl, "⊃1 'a'"), a);
    // It displays like a plain array, without a nested display's spacing.
    assert_eq!(val(Lang::Apl, "⍕1 'a'"), text(&[3], "1 a"));
}

/// Catenate, union, intersection, without, find, member and index-of all
/// BUILD a mixed simple array where the two sides share no one type, and
/// read one element for element where they do. libjay used to refuse the
/// pair outright, though the value model already held the answer.
#[test]
fn the_set_and_join_verbs_build_mixed_simple_arrays() {
    let mixed = boxes(
        &[4],
        vec![Array::scalar_i64(1), Array::scalar_i64(2), text(&[], "a"), text(&[], "b")],
    );
    assert_eq!(val(Lang::Apl, "1 2,'ab'"), mixed);
    assert_eq!(val(Lang::Apl, "≡1 2,'ab'"), Array::scalar_i64(1));
    assert_eq!(val(Lang::Apl, "≢1 2,'ab'"), Array::scalar_i64(4));
    assert_eq!(val(Lang::Apl, "1 2∪'ab'"), mixed);
    assert_eq!(val(Lang::Apl, "1 2⍪'a'"), val(Lang::Apl, "1 2,'a'"));
    // A comparison against a mixed array reads element for element, and
    // the answer tightens back to a simple array wherever it can.
    assert_eq!(val(Lang::Apl, "'a'=1 2,'ab'"), bools(&[4], &[0, 0, 1, 0]));
    assert_eq!(val(Lang::Apl, "(1 'a')∩'abc'"), text(&[1], "a"));
    assert_eq!(val(Lang::Apl, "1 2 3∩'a' 2"), i64s(&[1], &[2]));
    assert_eq!(val(Lang::Apl, "+/1 2 3∩'a' 2"), Array::scalar_i64(2));
    assert_eq!(val(Lang::Apl, "'a'⍷(1 'a')"), bools(&[2], &[0, 1]));
    assert_eq!(val(Lang::Apl, "'abc'⍳1 'a'"), i64s(&[2], &[4, 1]));
    assert_eq!(val(Lang::Apl, "(1 'a')⍳'a'"), Array::scalar_i64(2));
    // Arithmetic over one still has no meaning.
    assert_eq!(err(Lang::Apl, "1+1 2,'ab'").kind, ErrorKind::Type);
    // J has no such value and says so.
    assert_eq!(err(Lang::J, "1 2 , 'ab'").kind, ErrorKind::Type);
}

/// A run of characters beside each other in a mixed VECTOR is text and
/// prints as text: `1 2,'ab'` shows as `1 2 ab`, not `1 2 a b`. Higher
/// ranks align in columns instead, and there each character is a column.
#[test]
fn a_run_of_characters_in_a_mixed_vector_prints_as_text() {
    assert_eq!(val(Lang::Apl, "⍕1 2,'ab'"), text(&[6], "1 2 ab"));
    assert_eq!(val(Lang::Apl, "⍕1,'ab',2,'cd'"), text(&[9], "1 ab 2 cd"));
    assert_eq!(val(Lang::Apl, "⍕1 2,'a'"), text(&[5], "1 2 a"));
    let table = jay::fmt::format_array(&val(Lang::Apl, "2 3⍴1 2,'abcd'"), &jay::fmt::FmtOpts::APL);
    assert_eq!(table, "1 2 a\nb c d");
}

#[test]
fn overtaking_a_nested_array_fills_with_the_first_items_prototype() {
    // The prototype is that item's shape with a zero for every number and
    // a blank for every character.
    assert_eq!(
        val(Lang::Apl, "3↑⊂1 2"),
        boxes(&[3], vec![i64s(&[2], &[1, 2]), i64s(&[2], &[0, 0]), i64s(&[2], &[0, 0])])
    );
    assert_eq!(
        val(Lang::Apl, "2↑⊂'ab'"),
        boxes(&[2], vec![text(&[2], "ab"), text(&[2], "  ")])
    );
    // A prototype of a nested item is nested to the same depth.
    let deep = val(Lang::Apl, "2↑⊂(1 2)(3 4)");
    let fill = &deep.as_boxes().expect("boxed")[1];
    assert_eq!(*fill, boxes(&[2], vec![i64s(&[2], &[0, 0]), i64s(&[2], &[0, 0])]));
}

#[test]
fn catenating_nested_to_simple_encloses_the_simple_side() {
    assert_eq!(val(Lang::Apl, "⍴(1 2),⊂3 4"), i64s(&[1], &[3]));
    assert_eq!(val(Lang::Apl, "≡(1 2),⊂3 4"), Array::scalar_i64(2));
    assert_eq!(
        val(Lang::Apl, "(1 2),⊂3 4"),
        boxes(&[3], vec![Array::scalar_i64(1), Array::scalar_i64(2), i64s(&[2], &[3, 4])])
    );
    // J keeps refusing the mixture, which is what its reference does.
    assert_eq!(err(Lang::J, "1 2 , <3 4").kind, ErrorKind::Type);
}

// --- APL: the Dyalog operators, which GNU APL has none of -----------------

#[test]
fn nest_encloses_only_what_is_not_already_nested() {
    assert_eq!(val(Lang::Apl, "⊆1 2 3"), boxes(&[], vec![i64s(&[3], &[1, 2, 3])]));
    assert_eq!(val(Lang::Apl, "⊆5"), Array::scalar_i64(5));
    assert_eq!(val(Lang::Apl, "⊆⊂1 2"), boxes(&[], vec![i64s(&[2], &[1, 2])]));
    // The dyad is the partition GNU APL spells `⊂`.
    assert_eq!(
        val(Lang::Apl, "1 1 0 2 2⊆'abcde'"),
        boxes(&[2], vec![text(&[2], "ab"), text(&[2], "de")])
    );
}

#[test]
fn before_prepares_the_left_argument() {
    // `f⍛g` is the mirror of `f∘g`: monad `(f y) g y`, dyad `(f x) g y`.
    assert_eq!(val(Lang::Apl, "+⍛×2"), Array::scalar_i64(4));
    assert_eq!(val(Lang::Apl, "3(+⍛×)4"), Array::scalar_i64(12));
    assert_eq!(val(Lang::Apl, "¯1(×⍛+)5"), Array::scalar_i64(4));
}

#[test]
fn a_function_operand_makes_the_rank_operator_an_atop() {
    // `f⍤g`: monad `f g y`, dyad `f (x g y)`.
    assert_eq!(val(Lang::Apl, "2(+⍤×)3"), Array::scalar_i64(6));
    assert_eq!(val(Lang::Apl, "(-⍤×)2 3"), i64s(&[2], &[-1, -1]));
    // A value operand is still the rank specification.
    assert_eq!(val(Lang::Apl, "(⍴⍤1)2 3⍴⍳6"), i64s(&[2, 1], &[3, 3]));
}

#[test]
fn key_pairs_each_distinct_cell_with_what_shares_it() {
    // The operand takes the key on the left and the group on the right;
    // the group is the positions monadically.
    assert_eq!(val(Lang::Apl, "{⍺,≢⍵}⌸1 1 2"), i64s(&[2, 2], &[1, 2, 2, 1]));
    assert_eq!(val(Lang::Apl, "{⍺}⌸'aabc'"), text(&[3], "abc"));
    // Dyadically the group is the right argument's items at those places.
    assert_eq!(val(Lang::Apl, "1 1 2{+/⍵}⌸10 20 30"), i64s(&[2], &[30, 30]));
    // The keys and the values have to line up.
    assert_eq!(err(Lang::Apl, "1 1 2{+/⍵}⌸10 20").kind, ErrorKind::Length);
}

#[test]
fn a_dfn_that_names_an_operand_is_an_operator() {
    assert_eq!(val(Lang::Apl, "+{⍺⍺/⍵}1 2 3"), Array::scalar_i64(6));
    assert_eq!(val(Lang::Apl, "TWICE←{⍺⍺ ⍺⍺ ⍵} ⋄ -TWICE 5"), Array::scalar_i64(5));
    assert_eq!(val(Lang::Apl, "BOTH←{(⍺⍺ ⍵),⍵⍵ ⍵} ⋄ -BOTH+ 3"), i64s(&[2], &[-3, 3]));
    // The operand names are gone once the derived function has run.
    let e = err(Lang::Apl, "{⍺⍺ ⍵}5");
    assert_eq!(e.kind, ErrorKind::Parse);
    assert!(e.msg.contains("⍺⍺ needs an operand"), "{}", e.msg);
}

// --- APL: format by specification -----------------------------------------

#[test]
fn format_by_specification_lays_out_columns() {
    assert_eq!(val(Lang::Apl, "6 2⍕1.5 2.25"), text(&[12], "  1.50  2.25"));
    // One number alone is the precision; the width is what the values need
    // plus a blank between them.
    assert_eq!(val(Lang::Apl, "2⍕1.5 2.25"), text(&[10], " 1.50 2.25"));
    // One pair per column of the last axis.
    assert_eq!(
        val(Lang::Apl, "6 2 8 3⍕2 2⍴1.5 2.25 3 4"),
        text(&[2, 14], "  1.50   2.250  3.00   4.000")
    );
    // A value that does not fit its field is a domain error, as it is in
    // the reference, and a nested argument is a named gap.
    assert_eq!(err(Lang::Apl, "6 2⍕123456789").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::Apl, "¯6 2⍕1.5").kind, ErrorKind::Domain);
    let e = err(Lang::Apl, "6 2⍕(1 2)(3 4)");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("nested"), "{}", e.msg);
    assert_eq!(err(Lang::Apl, "1 2 3⍕1.5 2.5").kind, ErrorKind::Length);
}

// --- what this wave leaves named ------------------------------------------

#[test]
fn the_gaps_this_wave_leaves_name_themselves() {
    let cases: &[(Lang, &str, &str)] = &[
        (Lang::J, "1 s: s: <'abc'", "symbol-table form"),
        (Lang::J, "$. 'ab'", "sparse"),
        (Lang::Apl, "1(=⍠('ZZ' 2))2", "variant option ZZ"),
    ];
    for (lang, src, what) in cases {
        let e = err(*lang, src);
        assert_eq!(e.kind, ErrorKind::NotYet, "{src}: {}", e.msg);
        assert!(e.msg.contains(what), "{src}: {}", e.msg);
    }
    // A gap the sandbox holds open permanently is not a promise: `T.`
    // starts J's own threads, which libjay will not open.
    let threads = err(Lang::J, "(+ T. 0) 1");
    assert_eq!(threads.kind, ErrorKind::Sandbox);
    assert!(threads.msg.contains("T. starts J's own threads"), "{}", threads.msg);
}
