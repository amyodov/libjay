//! Scans, moving windows, suffixes, commute, power, the circle functions
//! and replicate: one test per family, both languages, values hand-derived
//! or checked against the reference J. The differential corpus itself lives
//! in tests/oracle.rs; these tests pin the meanings down in place.

use jay::{compile, Array, Data, Dialect, ErrorKind, Lang};

fn run(lang: Lang, src: &str) -> Option<Array> {
    run_dialect(lang, src, &Dialect::default())
}

fn run_dialect(lang: Lang, src: &str, dialect: &Dialect) -> Option<Array> {
    let program = compile(lang, src, dialect)
        .unwrap_or_else(|e| panic!("compile failed:\n{}", e.render(src)));
    let mut sink = |_: &str| {};
    program
        .run(&[], &mut sink)
        .unwrap_or_else(|e| panic!("run of {src:?} failed:\n{}", program.render_error(&e)))
}

fn val(lang: Lang, src: &str) -> Array {
    run(lang, src).unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

/// The same, with `⎕IO` set to 0, so APL's `⍳` counts the way J's `i.` does.
fn val0(src: &str) -> Array {
    run_dialect(Lang::Apl, src, &Dialect { index_origin: Some(0) })
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

fn f64s(shape: &[usize], values: &[f64]) -> Array {
    Array::new(shape.to_vec(), Data::F64(values.to_vec().into()))
}

fn text(shape: &[usize], s: &str) -> Array {
    Array::new(shape.to_vec(), Data::Char(s.chars().collect()))
}

/// A one-number result.
fn num(lang: Lang, src: &str) -> f64 {
    let a = val(lang, src);
    let v = a.to_f64_vec().unwrap_or_else(|| panic!("{src:?} is not numeric"));
    assert_eq!(v.len(), 1, "{src:?} yielded {} numbers", v.len());
    v[0]
}

fn close(a: f64, b: f64, what: &str) {
    let ok = (a - b).abs() <= 1e-12 + 1e-12 * a.abs().max(b.abs())
        || (a.is_infinite() && a == b);
    assert!(ok, "{what}: got {a}, want {b}");
}

// --- scans --------------------------------------------------------------

#[test]
fn j_scans_apply_the_verb_to_every_prefix() {
    assert_eq!(val(Lang::J, "+/\\ 1 2 3"), i64s(&[3], &[1, 3, 6]));
    assert_eq!(val(Lang::J, "*/\\ 1 2 3 4"), i64s(&[4], &[1, 2, 6, 24]));
    assert_eq!(val(Lang::J, "<./\\ 3 1 4 1 5"), i64s(&[5], &[3, 1, 1, 1, 1]));
    assert_eq!(val(Lang::J, ">./\\ 3 1 4 1 5"), i64s(&[5], &[3, 3, 4, 4, 5]));
    // The prefix reduction folds right to left, so subtraction alternates:
    // -/1 2 3 4 is 1-(2-(3-4)) = _2, not ((1-2)-3)-4.
    assert_eq!(val(Lang::J, "-/\\ 1 2 3 4"), i64s(&[4], &[1, -1, 2, -2]));
    // A scan runs over items, so a matrix accumulates rows.
    assert_eq!(val(Lang::J, "+/\\ i. 2 3"), i64s(&[2, 3], &[0, 1, 2, 3, 5, 7]));
    // The verb need not be a reduction.
    assert_eq!(val(Lang::J, "#\\ 1 2 3"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(val(Lang::J, "(+/ % #)\\ 1 2 3"), f64s(&[3], &[1.0, 1.5, 2.0]));
    // Edges: nothing to scan, and a scalar as its own single item.
    assert_eq!(val(Lang::J, "+/\\ i. 0"), i64s(&[0], &[]));
    assert_eq!(val(Lang::J, "+/\\ 5"), i64s(&[1], &[5]));
}

#[test]
fn j_suffix_scans_apply_the_verb_to_every_suffix() {
    assert_eq!(val(Lang::J, "+/\\. 1 2 3"), i64s(&[3], &[6, 5, 3]));
    // Suffix i is item i folded against suffix i+1, which is the insert's
    // own order: -/1 2 3 4 = _2, -/2 3 4 = 3, -/3 4 = _1, -/4 = 4.
    assert_eq!(val(Lang::J, "-/\\. 1 2 3 4"), i64s(&[4], &[-2, 3, -1, 4]));
    assert_eq!(val(Lang::J, "+/\\. i. 2 3"), i64s(&[2, 3], &[3, 5, 7, 3, 4, 5]));
    assert_eq!(val(Lang::J, "+/\\. i. 0"), i64s(&[0], &[]));
    // The outfix (the dyad of `\.`) leaves out every run of x items.
    assert_eq!(val(Lang::J, "2 +/\\. 1 2 3"), i64s(&[2], &[3, 1]));
    assert_eq!(val(Lang::J, "2 +/\\. i. 5"), i64s(&[4], &[9, 7, 5, 3]));
    assert_eq!(val(Lang::J, "3 -/\\. i. 5"), i64s(&[3], &[-1, -4, -1]));
}

#[test]
fn apl_scans_follow_the_axis_of_their_glyph() {
    // `\` scans the last axis, `⍀` the leading one — the same divergence
    // reduce has between `/` and `⌿`.
    assert_eq!(val(Lang::Apl, "+\\1 2 3"), i64s(&[3], &[1, 3, 6]));
    assert_eq!(val(Lang::Apl, "+⍀1 2 3"), i64s(&[3], &[1, 3, 6]));
    assert_eq!(
        val(Lang::Apl, "+\\2 3⍴⍳6"),
        i64s(&[2, 3], &[1, 3, 6, 4, 9, 15]),
        "the last axis accumulates within each row"
    );
    assert_eq!(
        val(Lang::Apl, "+⍀2 3⍴⍳6"),
        i64s(&[2, 3], &[1, 2, 3, 5, 7, 9]),
        "the leading axis accumulates down the rows"
    );
}

/// APL's scan is defined as the reduce of each prefix, not as a left fold.
/// The two differ for a verb that does not associate: the third element of
/// `-\1 2 3` is `-/1 2 3`, which APL and J alike evaluate right to left as
/// `1-(2-3)` = 2 — a left fold would give `(1-2)-3` = ¯4.
#[test]
fn apl_scan_of_a_non_associative_function_reduces_each_prefix() {
    assert_eq!(val(Lang::Apl, "-\\1 2 3"), i64s(&[3], &[1, -1, 2]));
    assert_eq!(val(Lang::Apl, "-\\1 2 3 4"), i64s(&[4], &[1, -1, 2, -2]));
    // Division is not associative either: 1, 1÷2, 1÷(2÷3).
    assert_eq!(val(Lang::Apl, "÷\\1 2 3"), f64s(&[3], &[1.0, 0.5, 1.5]));
    // J spells the same thing `-/\`, and gets the same answer.
    assert_eq!(val(Lang::J, "-/\\ 1 2 3"), val(Lang::Apl, "-\\1 2 3"));
    // APL has no dyadic scan; `x\y` after a value is expand, a function of
    // its own: every 1 takes an item, every 0 leaves a fill.
    assert_eq!(val(Lang::Apl, "1 0 1\\1 2"), i64s(&[3], &[1, 0, 2]));
    assert_eq!(val(Lang::Apl, "1 0 1⍀1 2"), i64s(&[3], &[1, 0, 2]));
    assert_eq!(val(Lang::Apl, "1 0 0 1\\'ab'"), text(&[4], "a  b"));
}

// --- moving windows -----------------------------------------------------

#[test]
fn j_positive_windows_are_overlapping_runs() {
    assert_eq!(val(Lang::J, "3 +/\\ 1 2 3 4 5"), i64s(&[3], &[6, 9, 12]));
    assert_eq!(val(Lang::J, "2 +/\\ 1 2 3 4 5"), i64s(&[4], &[3, 5, 7, 9]));
    assert_eq!(val(Lang::J, "1 +/\\ 1 2 3"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(val(Lang::J, "2 <./\\ 3 1 4 1 5"), i64s(&[4], &[1, 1, 1, 1]));
    assert_eq!(val(Lang::J, "3 >./\\ 3 1 4 1 5"), i64s(&[3], &[4, 4, 5]));
    assert_eq!(val(Lang::J, "3 */\\ 1 2 3 4 5"), i64s(&[3], &[6, 24, 60]));
    // A window of items, not of elements: rows of a matrix.
    assert_eq!(val(Lang::J, "3 +/\\ i. 4 3"), i64s(&[2, 3], &[9, 12, 15, 18, 21, 24]));
    // A verb that is not a reduction takes the general path.
    assert_eq!(val(Lang::J, "3 #\\ 1 2 3 4 5"), i64s(&[3], &[3, 3, 3]));
    assert_eq!(val(Lang::J, "3 (+/ % #)\\ 1 2 3 4 5"), f64s(&[3], &[2.0, 3.0, 4.0]));
    // So does a verb that does not associate.
    assert_eq!(val(Lang::J, "2 %/\\ 1 2 3 4"), f64s(&[3], &[0.5, 2.0 / 3.0, 0.75]));
}

#[test]
fn j_window_edges_follow_the_reference() {
    // A window longer than the argument yields no windows at all, but it
    // keeps the shape of one: `+/` of a 2x3 window is a 3-vector.
    assert_eq!(val(Lang::J, "5 +/\\ 1 2 3 4").shape, vec![0]);
    assert_eq!(val(Lang::J, "9 +/\\ 1 2 3").shape, vec![0]);
    assert_eq!(val(Lang::J, "3 +/\\ i. 2 3").shape, vec![0, 3]);
    // Zero takes the n+1 empty runs, each reduced to the verb's identity.
    assert_eq!(val(Lang::J, "0 +/\\ 1 2 3 4 5"), i64s(&[6], &[0, 0, 0, 0, 0, 0]));
    assert_eq!(val(Lang::J, "0 <./\\ 1 2 3"), f64s(&[4], &[f64::INFINITY; 4]));
    // A negative window takes non-overlapping chunks, the last one short.
    assert_eq!(val(Lang::J, "_2 +/\\ 1 2 3 4 5"), i64s(&[3], &[3, 7, 5]));
    assert_eq!(val(Lang::J, "_3 +/\\ 1 2 3 4 5 6 7"), i64s(&[3], &[6, 15, 7]));
    assert_eq!(val(Lang::J, "_9 +/\\ 1 2 3"), i64s(&[1], &[6]));
    assert_eq!(val(Lang::J, "_2 +/\\ i. 4 3"), i64s(&[2, 3], &[3, 5, 7, 15, 17, 19]));
    // A list of window sizes frames the result, padding the short rows: the
    // derived verb takes one size per application.
    assert_eq!(val(Lang::J, "2 3 4 -/\\ 1 2 3"), i64s(&[3, 2], &[-1, -1, 2, 0, 0, 0]));
    // A scalar argument is one item.
    assert_eq!(val(Lang::J, "1 +/\\ 5"), i64s(&[1], &[5]));
    assert_eq!(val(Lang::J, "2 +/\\ 5").shape, vec![0]);
}

/// The moving-sum fast path cuts the argument into blocks of the window
/// length and combines one block's suffix with the next block's prefix, so
/// every window but the aligned ones straddles a boundary. This walks the
/// window over that grid for a range of sizes.
#[test]
fn moving_sums_agree_with_per_window_sums_across_block_boundaries() {
    for n in [1usize, 2, 3, 5, 12, 40, 41] {
        for w in 1..=n.min(13) {
            let src = format!("{w} +/\\ i. {n}");
            let want: Vec<i64> = (0..=n - w).map(|i| (i..i + w).sum::<usize>() as i64).collect();
            assert_eq!(val(Lang::J, &src), i64s(&[want.len()], &want), "{src}");
        }
    }
}

/// Above the parallel threshold the windows are split across threads, at
/// chunk boundaries that have nothing to do with the block grid. The sums
/// of consecutive integers are known in closed form, so this checks every
/// one of them.
#[test]
fn moving_sums_split_across_threads_stay_in_order() {
    let n = 200_000usize;
    for w in [1usize, 7, 64] {
        let got = val(Lang::J, &format!("{w} +/\\ i. {n}"));
        assert_eq!(got.shape, vec![n - w + 1]);
        let values = got.as_i64_slice().expect("integer sums");
        let base = (w * (w - 1) / 2) as i64;
        for (i, &v) in values.iter().enumerate() {
            assert_eq!(v, w as i64 * i as i64 + base, "window {i} of {w}");
        }
    }
}

/// `[: u ]` computes what `u` does but is not the shape the fast paths look
/// for, so it is the same answer down the general per-window path.
#[test]
fn the_window_fast_path_agrees_with_the_general_one() {
    let y = "3 1 4 1 5 9 2 6 5 3 5 8 9 7 9";
    for w in 1..=6 {
        for verb in ["+/", "<./", ">./", "*/"] {
            let fast = val(Lang::J, &format!("{w} {verb}\\ {y}"));
            let general = val(Lang::J, &format!("{w} ([: {verb} ])\\ {y}"));
            assert_eq!(fast, general, "{w} {verb}\\ {y}");
        }
    }
    // Prefixes too, where the fast path runs one accumulator.
    for verb in ["+/", "<./", ">./", "*/", "-/"] {
        let fast = val(Lang::J, &format!("{verb}\\ {y}"));
        let general = val(Lang::J, &format!("([: {verb} ])\\ {y}"));
        assert_eq!(fast, general, "{verb}\\ {y}");
        let fast = val(Lang::J, &format!("{verb}\\. {y}"));
        let general = val(Lang::J, &format!("([: {verb} ])\\. {y}"));
        assert_eq!(fast, general, "{verb}\\. {y}");
    }
}

#[test]
fn moving_sums_of_floats_stay_within_the_windows_own_error() {
    // Every window sum is computed from at most `w` additions, so its error
    // does not grow with the length of the series.
    let got = val(Lang::J, "3 +/\\ 0.5 1.5 2.5 3.5 4.5");
    assert_eq!(got, f64s(&[3], &[4.5, 7.5, 10.5]));
    let got = val(Lang::J, "2 +/\\ 1 2 3.5");
    assert_eq!(got, f64s(&[2], &[3.0, 5.5]));
}

#[test]
fn window_sizes_must_be_one_whole_number() {
    let e = err(Lang::J, "1.5 +/\\ 1 2 3");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("window size"), "{}", e.msg);
}

// --- commute ------------------------------------------------------------

#[test]
fn commute_doubles_the_argument_or_swaps_the_two() {
    assert_eq!(val(Lang::J, "+~ 3"), i64s(&[], &[6]));
    assert_eq!(val(Lang::J, "-~ 1 2 3"), i64s(&[3], &[0, 0, 0]));
    assert_eq!(val(Lang::J, "2 -~ 5"), i64s(&[], &[3]));
    assert_eq!(val(Lang::J, "3 %~ 12"), f64s(&[], &[4.0]));
    assert_eq!(val(Lang::J, "2 3 +~ 4 5"), i64s(&[2], &[6, 8]));
    // APL spells it `⍨`, and means the same.
    assert_eq!(val(Lang::Apl, "2-⍨5"), i64s(&[], &[3]));
    assert_eq!(val(Lang::Apl, "×⍨3"), i64s(&[], &[9]));
    assert_eq!(val(Lang::Apl, "2÷⍨8"), f64s(&[], &[4.0]));
}

// --- power --------------------------------------------------------------

#[test]
fn power_applies_the_verb_a_fixed_number_of_times() {
    assert_eq!(val(Lang::J, "+:^:3 (1)"), i64s(&[], &[8]));
    // Zero applications is the identity on the right argument.
    assert_eq!(val(Lang::J, "+:^:0 (5)"), i64s(&[], &[5]));
    assert_eq!(val(Lang::J, "+:^:2 (1 2 3)"), i64s(&[3], &[4, 8, 12]));
    // The dyad binds the left argument to every application.
    assert_eq!(val(Lang::J, "2 +^:3 (5)"), i64s(&[], &[11]));
    assert_eq!(val(Lang::J, "2 *^:4 (1)"), i64s(&[], &[16]));
    // APL spells it `⍣`.
    assert_eq!(val(Lang::Apl, "2×⍣3⊢1"), i64s(&[], &[8]));
    assert_eq!(val(Lang::Apl, "2×⍣0⊢7"), i64s(&[], &[7]));
}

#[test]
fn power_to_convergence_stops_when_the_result_stops_changing() {
    // Repeated square roots converge to 1 from either side.
    close(num(Lang::J, "%:^:_ (100)"), 1.0, "%:^:_ 100");
    close(num(Lang::J, "%:^:_ (0.5)"), 1.0, "%:^:_ 0.5");
    close(num(Lang::J, "%:^:_ (1e300)"), 1.0, "%:^:_ 1e300");
    // Fixed points reached at once.
    assert_eq!(val(Lang::J, "%:^:_ (1)"), f64s(&[], &[1.0]));
    assert_eq!(val(Lang::J, "%:^:_ (0)"), f64s(&[], &[0.0]));
    // Doubling runs away, but infinity is a fixed point of doubling.
    assert!(num(Lang::J, "+:^:_ (1)").is_infinite());
    // Flooring converges after one step.
    assert_eq!(val(Lang::J, "<.^:_ (2.5)"), i64s(&[], &[2]));
}

#[test]
fn power_operands_that_are_not_a_count_are_named() {
    for (src, msg) in [
        // A verb operand is the count, so a count that is not a whole
        // number is what the diagnostic names.
        ("+ ^: (% & 2) 5", "count must be an integer"),
        ("(+/ % #) ^: _1 (5)", "obverse"),
        ("+ ^: 1.5 (5)", "whole number"),
    ] {
        let e = err(Lang::J, src);
        assert!(e.msg.contains(msg), "{src}: {}", e.msg);
    }
}

// --- circle functions ---------------------------------------------------

#[test]
fn the_monadic_circle_function_multiplies_by_pi() {
    close(num(Lang::J, "o. 1"), std::f64::consts::PI, "o. 1");
    close(num(Lang::Apl, "○1"), std::f64::consts::PI, "○1");
    assert_eq!(
        val(Lang::J, "o. 0 1 2"),
        f64s(&[3], &[0.0, std::f64::consts::PI, 2.0 * std::f64::consts::PI])
    );
}

#[test]
fn the_circle_table_matches_the_standard_library() {
    let y = 0.5f64;
    let cases: [(&str, f64); 14] = [
        ("0", (1.0 - y * y).sqrt()),
        ("1", y.sin()),
        ("2", y.cos()),
        ("3", y.tan()),
        ("4", (1.0 + y * y).sqrt()),
        ("5", y.sinh()),
        ("6", y.cosh()),
        ("7", y.tanh()),
        ("_1", y.asin()),
        ("_2", y.acos()),
        ("_3", y.atan()),
        ("_5", y.asinh()),
        ("_7", y.atanh()),
        // The two families that need |y| >= 1 are checked at y = 2.
        ("_6", 2.0f64.acosh()),
    ];
    for (k, want) in cases {
        let arg = if k == "_6" { "2" } else { "0.5" };
        close(num(Lang::J, &format!("{k} o. {arg}")), want, k);
        let apl_k = k.replace('_', "¯");
        let apl = format!("{apl_k}○{arg}");
        close(num(Lang::Apl, &apl), want, &apl);
    }
    // `_4 o. y` keeps the sign of y: it is not simply sqrt(y*y-1).
    close(num(Lang::J, "_4 o. 2"), 3.0f64.sqrt(), "_4 o. 2");
    close(num(Lang::J, "_4 o. _2"), -(3.0f64.sqrt()), "_4 o. _2");
    // The left argument is elementwise, like any scalar dyad.
    assert_eq!(
        val(Lang::J, "1 2 3 o. 1"),
        f64s(&[3], &[1.0f64.sin(), 1.0f64.cos(), 1.0f64.tan()])
    );
}

#[test]
fn circle_functions_outside_the_reals_answer_in_complex() {
    // A real argument whose answer is not real turns the whole pass complex.
    for src in ["_1 o. 2", "_2 o. 2", "0 o. 2", "_4 o. 0.5", "_6 o. 0.5", "_7 o. 2", "8 o. 3"] {
        assert_eq!(val(Lang::J, src).dtype(), jay::DType::Complex, "{src}");
    }
    // 9 to 12 read the parts of a number that may well be real.
    assert_eq!(val(Lang::J, "9 o. 1"), f64s(&[], &[1.0]));
    assert_eq!(val(Lang::J, "11 o. 1"), f64s(&[], &[0.0]));
    assert_eq!(val(Lang::J, "10 o. _3"), f64s(&[], &[3.0]));
    // A fractional k selects nothing, and neither does one off the table.
    assert_eq!(err(Lang::J, "1.5 o. 1").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "13 o. 1").kind, ErrorKind::Domain);
}

// --- replicate ----------------------------------------------------------

#[test]
fn j_replicate_repeats_items() {
    assert_eq!(val(Lang::J, "1 0 1 # 1 2 3"), i64s(&[2], &[1, 3]));
    assert_eq!(val(Lang::J, "2 0 1 # 1 2 3"), i64s(&[3], &[1, 1, 3]));
    assert_eq!(val(Lang::J, "2 # 1 2 3"), i64s(&[6], &[1, 1, 2, 2, 3, 3]));
    assert_eq!(val(Lang::J, "0 # 1 2 3"), i64s(&[0], &[]));
    assert_eq!(val(Lang::J, "1 0 1 # 'abc'"), text(&[2], "ac"));
    // The items of a matrix are its rows.
    assert_eq!(val(Lang::J, "1 0 # i. 2 3"), i64s(&[1, 3], &[0, 1, 2]));
    assert_eq!(val(Lang::J, "2 # i. 2 3"), i64s(&[4, 3], &[0, 1, 2, 0, 1, 2, 3, 4, 5, 3, 4, 5]));
    // A scalar argument is one item, so replicating it makes a vector.
    assert_eq!(val(Lang::J, "2 # 5"), i64s(&[2], &[5, 5]));
    // Only a scalar count extends; a one-element vector is a length error.
    assert_eq!(err(Lang::J, "1 0 1 # 1 2").kind, ErrorKind::Length);
    assert_eq!(err(Lang::J, "_1 # 1 2 3").kind, ErrorKind::Domain);
}

#[test]
fn apl_replicate_is_the_same_operation_on_another_axis() {
    assert_eq!(val(Lang::Apl, "1 0 1/1 2 3"), i64s(&[2], &[1, 3]));
    assert_eq!(val(Lang::Apl, "2/1 2 3"), i64s(&[6], &[1, 1, 2, 2, 3, 3]));
    assert_eq!(val(Lang::Apl, "1 0 1⌿1 2 3"), i64s(&[2], &[1, 3]));
    // `/` counts along the LAST axis: three counts for three columns.
    assert_eq!(val(Lang::Apl, "1 0 1/2 3⍴⍳6"), i64s(&[2, 2], &[1, 3, 4, 6]));
    // `⌿` counts along the leading one: two counts for two rows.
    assert_eq!(val(Lang::Apl, "1 0⌿2 3⍴⍳6"), i64s(&[1, 3], &[1, 2, 3]));
    assert_eq!(val(Lang::Apl, "2⌿2 3⍴⍳6"), i64s(&[4, 3], &[1, 2, 3, 1, 2, 3, 4, 5, 6, 4, 5, 6]));
    // A name to the left of `/` is a value, so `/` there is replicate.
    assert_eq!(val(Lang::Apl, "m←1 0 1 ⋄ m/1 2 3"), i64s(&[2], &[1, 3]));
}

/// Same counts, same matrix, different axis: the divergence the shared IR
/// carries as one primitive at two ranks.
#[test]
fn divergence_replicate_axis() {
    let apl_last = val(Lang::Apl, "1 0 1/2 3⍴⍳6");
    assert_eq!(apl_last, i64s(&[2, 2], &[1, 3, 4, 6]));
    // The leading-axis reading of the same counts has the wrong length,
    // in APL as in J, because the matrix has two items and not three.
    assert_eq!(err(Lang::Apl, "1 0 1⌿2 3⍴⍳6").kind, ErrorKind::Length);
    assert_eq!(err(Lang::J, "1 0 1 # i. 2 3").kind, ErrorKind::Length);
    // J's `#` is the leading-axis one, so it agrees with `⌿` on the very
    // same matrix (`⎕IO←0` so that `⍳6` counts the way `i. 6` does).
    assert_eq!(val(Lang::J, "1 0 # i. 2 3"), val0("1 0⌿2 3⍴⍳6"));
}
