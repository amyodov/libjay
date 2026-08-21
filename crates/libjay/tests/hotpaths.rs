//! The reduction fast paths against the spellings that do not take them.
//!
//! Two of them are new. A reduction over vector cells (`u/"1 y`) folds every
//! row out of the one buffer instead of building an array per cell; a flat
//! associative fold keeps several accumulators in flight instead of one.
//! Both are only allowed to be faster, so each is compared here against a
//! spelling of the same computation that the fast path does not cover —
//! `u/ |: y` reduces along the leading axis, which is the older path, and a
//! run short enough to stay under the lane threshold keeps one accumulator.
//!
//! The suffix scan, the key and the reshape are here for the same reason.
//! `u/\\.` folds one step per item instead of folding every suffix from
//! scratch, and the reference is the fold of each suffix taken on its own;
//! the key groups by a hash of the item where the same keys as floats are
//! compared one by one; a reshape that keeps the element count hands the
//! buffer through, which shows as the pointer it came in with.
//!
//! Integer, boolean and complex results must be identical; a float
//! reduction is compared to 1e-12 relative, since regrouping an associative
//! float fold is already contracted (§5.9).

use jay::{compile, Array, DType, Data, Dialect, Lang};

/// A deterministic value stream: splitmix64, so every run sees the same data.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn f64s(&mut self, n: usize) -> Vec<f64> {
        (0..n).map(|_| (self.next() >> 11) as f64 / (1u64 << 53) as f64 - 0.5).collect()
    }

    fn i64s(&mut self, n: usize, span: i64) -> Vec<i64> {
        (0..n).map(|_| (self.next() % (2 * span as u64 + 1)) as i64 - span).collect()
    }

    fn bools(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| (self.next() & 1) as u8).collect()
    }
}

fn run(src: &str, args: &[Array]) -> Array {
    let program = compile(Lang::J, src, &Dialect::default()).expect("compile");
    let mut sink = |_: &str| {};
    program.run(args, &mut sink).expect("run").expect("a value")
}

/// Two results agree: exactly for the discrete types, to 1e-12 relative for
/// the float and complex ones.
fn agree(a: &Array, b: &Array) {
    assert_eq!(a.shape, b.shape, "shapes differ");
    assert_eq!(a.dtype(), b.dtype(), "types differ");
    match (&a.data, &b.data) {
        (Data::F64(x), Data::F64(y)) => {
            for (i, (p, q)) in x.iter().zip(y.iter()).enumerate() {
                let scale = p.abs().max(q.abs()).max(1e-300);
                assert!((p - q).abs() <= 1e-12 * scale, "element {i}: {p} vs {q}");
            }
        }
        (Data::Complex(x), Data::Complex(y)) => {
            for (i, (p, q)) in x.iter().zip(y.iter()).enumerate() {
                for k in 0..2 {
                    let scale = p[k].abs().max(q[k].abs()).max(1e-300);
                    assert!((p[k] - q[k]).abs() <= 1e-12 * scale, "element {i}: {p:?} vs {q:?}");
                }
            }
        }
        (x, y) => assert_eq!(x, y, "values differ"),
    }
}

/// Every reduction the row fold covers, over every element type it covers,
/// against the same reduction taken along the leading axis of the transpose.
#[test]
fn a_reduction_over_vector_cells_matches_the_leading_axis_one() {
    let mut rng = Rng(1);
    // Enough rows to reach the parallel split, and both a narrow item and
    // one wide enough for the vector clone.
    for &(rows, cols) in &[(1usize, 1usize), (3, 2), (7, 5), (40_000, 8), (5_000, 17)] {
        let n = rows * cols;
        let cases: Vec<Array> = vec![
            Array::new(vec![rows, cols], Data::F64(rng.f64s(n).into())),
            Array::new(vec![rows, cols], Data::I64(rng.i64s(n, 1_000).into())),
            Array::new(vec![rows, cols], Data::Bool(rng.bools(n).into())),
        ];
        for y in &cases {
            for verb in ["+", "-", "*", ">.", "<."] {
                let fast = run(&format!("{verb}/\"1 {{y}}"), std::slice::from_ref(y));
                let slow = run(&format!("{verb}/ |: {{y}}"), std::slice::from_ref(y));
                agree(&fast, &slow);
            }
        }
        // Complex has no ordering, so only the three arithmetic folds.
        let cx: Vec<[f64; 2]> =
            rng.f64s(2 * n).chunks_exact(2).map(|p| [p[0], p[1]]).collect();
        let z = Array::new(vec![rows, cols], Data::Complex(cx.into()));
        for verb in ["+", "-", "*"] {
            let fast = run(&format!("{verb}/\"1 {{y}}"), std::slice::from_ref(&z));
            let slow = run(&format!("{verb}/ |: {{y}}"), std::slice::from_ref(&z));
            agree(&fast, &slow);
        }
    }
}

/// The fold order is the insert's own, right to left, which a subtraction
/// makes visible: `-/"1` of three columns is `a - (b - c)`.
#[test]
fn the_row_fold_keeps_the_inserts_right_to_left_order() {
    let y = Array::new(vec![2, 3], Data::F64(vec![1.0, 2.0, 4.0, 10.0, 20.0, 40.0].into()));
    let r = run("-/\"1 {y}", std::slice::from_ref(&y));
    assert_eq!(r.shape, vec![2]);
    match &r.data {
        Data::F64(v) => assert_eq!(v.as_slice(), &[1.0 - (2.0 - 4.0), 10.0 - (20.0 - 40.0)]),
        d => panic!("expected floats, got {d:?}"),
    }
}

/// Cells of higher-rank frames reduce to the frame's own shape, and the
/// values are the ones the same rows give as a matrix.
#[test]
fn a_higher_rank_frame_reduces_to_the_frames_shape() {
    let mut rng = Rng(7);
    let v = rng.f64s(24);
    let cube = Array::new(vec![2, 3, 4], Data::F64(v.clone().into()));
    let flat = Array::new(vec![6, 4], Data::F64(v.into()));
    let a = run("+/\"1 {y}", std::slice::from_ref(&cube));
    let b = run("+/\"1 {y}", std::slice::from_ref(&flat));
    assert_eq!(a.shape, vec![2, 3]);
    assert_eq!(b.shape, vec![6]);
    match (&a.data, &b.data) {
        (Data::F64(x), Data::F64(y)) => assert_eq!(x.as_slice(), y.as_slice()),
        _ => panic!("expected floats"),
    }
}

/// What the row fold declines still works, and answers what it always did:
/// an empty cell reduces to the operation's identity, an integer product
/// that leaves i64 widens, and a non-arithmetic fold takes the general path.
#[test]
fn the_cases_the_row_fold_declines_are_unchanged() {
    let empty = Array::new(vec![2, 0], Data::I64(Vec::new().into()));
    let r = run("+/\"1 {y}", std::slice::from_ref(&empty));
    assert_eq!(r.shape, vec![2]);
    assert_eq!(r.data, Data::I64(vec![0, 0].into()));

    let big = Array::new(vec![2, 2], Data::I64(vec![1 << 40, 1 << 40, 2, 3].into()));
    let r = run("*/\"1 {y}", std::slice::from_ref(&big));
    let s = run("*/ |: {y}", std::slice::from_ref(&big));
    agree(&r, &s);
    assert_ne!(r.dtype(), DType::I64, "a product that leaves i64 must widen");

    // A comparison decides its own result type, which the row fold leaves
    // to the general path.
    let y = Array::new(vec![2, 3], Data::I64(vec![1, 2, 3, 6, 5, 4].into()));
    let r = run("</\"1 {y}", std::slice::from_ref(&y));
    let s = run("</ |: {y}", std::slice::from_ref(&y));
    agree(&r, &s);
}

/// A flat associative fold in lanes agrees with the same fold taken one
/// accumulator at a time — exactly for the discrete types, and to the
/// contracted tolerance for floats.
#[test]
fn a_flat_fold_in_lanes_agrees_with_one_accumulator() {
    let mut rng = Rng(11);
    // A short run stays under the lane threshold and a long one crosses it;
    // reducing a one-column matrix along its leading axis is the same fold.
    for n in [3usize, 63, 64, 65, 1_000, 300_000] {
        let f = Array::new(vec![n], Data::F64(rng.f64s(n).into()));
        let i = Array::new(vec![n], Data::I64(rng.i64s(n, 1_000_000).into()));
        for verb in ["+", ">.", "<."] {
            // A one-row matrix has one cell of `n` elements, which the row
            // fold walks with a single accumulator: the same fold, ungrouped.
            for y in [&f, &i] {
                let flat = run(&format!("{verb}/ {{y}}"), std::slice::from_ref(y));
                let row = run(&format!("{verb}/\"1 (1 {n} $ {{y}})"), std::slice::from_ref(y));
                assert_eq!(flat.shape, Vec::<usize>::new());
                assert_eq!(row.shape, vec![1]);
                agree(&flat, &Array::new(Vec::new(), row.data.clone()));
            }
        }
    }
}

// ------------------------------------------------------ the mixed passes

/// The same values one step up the numeric tower — the buffer a mixed pass
/// used to build before it ran.
fn widened(a: &Array, to: DType) -> Array {
    let f = a.to_f64_vec().expect("numeric data");
    match to {
        DType::F64 => Array::from_f64(f),
        DType::Complex => Array::new(
            a.shape.clone(),
            Data::Complex(f.iter().map(|&x| [x, 0.0]).collect::<Vec<_>>().into()),
        ),
        other => panic!("nothing widens to {}", other.name()),
    }
}

/// Bit-for-bit equality, NaN included: promoting an element and then
/// operating is the same arithmetic on the same values as promoting the
/// whole buffer first, so nothing at all may move.
fn identical(a: &Array, b: &Array) {
    assert_eq!(a.shape, b.shape, "shapes differ");
    assert_eq!(a.dtype(), b.dtype(), "types differ");
    match (&a.data, &b.data) {
        (Data::F64(x), Data::F64(y)) => {
            for (i, (p, q)) in x.iter().zip(y.iter()).enumerate() {
                assert_eq!(p.to_bits(), q.to_bits(), "element {i}: {p} vs {q}");
            }
        }
        (Data::Complex(x), Data::Complex(y)) => {
            for (i, (p, q)) in x.iter().zip(y.iter()).enumerate() {
                assert_eq!(
                    [p[0].to_bits(), p[1].to_bits()],
                    [q[0].to_bits(), q[1].to_bits()],
                    "element {i}: {p:?} vs {q:?}"
                );
            }
        }
        (x, y) => assert_eq!(x, y, "values differ"),
    }
}

/// A pass whose two operands have different element types promotes each
/// element where it reads it. What comes out must be exactly what the pass
/// gave when it widened the narrow operand into a buffer of its own first,
/// which is what handing it the widened argument makes it do.
///
/// The sizes straddle `par::MIN_WORK`, so the one-thread and the chunked
/// passes both run, and a scalar operand exercises the repeated-element
/// shape beside the element-per-element one.
#[test]
fn a_mixed_pass_answers_what_the_widened_one_answers() {
    let mut rng = Rng(23);
    for n in [1usize, 97, 100_000] {
        let f = Array::new(vec![n], Data::F64(rng.f64s(n).into()));
        let i = Array::new(vec![n], Data::I64(rng.i64s(n, 1_000_000).into()));
        let b = Array::new(vec![n], Data::Bool(rng.bools(n).into()));
        let cx: Vec<[f64; 2]> = rng.f64s(2 * n).chunks_exact(2).map(|p| [p[0], p[1]]).collect();
        let z = Array::new(vec![n], Data::Complex(cx.into()));
        // Every arithmetic verb the typed passes pick a step for, and the
        // comparisons, which have passes of their own.
        let float_verbs = ["+", "-", "*", "%", "<.", ">.", "|", "=", "~:", "<", ">:"];
        // Complex has no order: only the arithmetic and equality.
        let cx_verbs = ["+", "-", "*", "%", "=", "~:"];
        for (narrow, wide, verbs, up) in [
            (&i, &f, &float_verbs[..], DType::F64),
            (&b, &f, &float_verbs[..], DType::F64),
            (&i, &z, &cx_verbs[..], DType::Complex),
            (&b, &z, &cx_verbs[..], DType::Complex),
            (&f, &z, &cx_verbs[..], DType::Complex),
        ] {
            let up_narrow = widened(narrow, up);
            for verb in verbs {
                for src in [
                    format!("{{a}} {verb} {{y}}"),
                    format!("{{y}} {verb} {{a}}"),
                    // A scalar on the narrow side, which every element reads.
                    format!("(0 {{ {{a}}) {verb} {{y}}"),
                ] {
                    let mixed = run(&src, &[narrow.clone(), wide.clone()]);
                    let plain = run(&src, &[up_narrow.clone(), wide.clone()]);
                    identical(&mixed, &plain);
                }
            }
        }
        // A fused chain loads its narrow arguments a block at a time, and a
        // window reads the block's halo as well.
        for src in ["+/ {a} * {y}", "{a} * 2 + {y}", "20 +/\\ {a} * {y}"] {
            if n < 20 && src.contains("+/\\") {
                continue;
            }
            for narrow in [&i, &b] {
                let mixed = run(src, &[narrow.clone(), f.clone()]);
                let plain = run(src, &[widened(narrow, DType::F64), f.clone()]);
                identical(&mixed, &plain);
            }
        }
    }
}

// ------------------------------------------------- scans, keys and shapes

/// Every suffix folded on its own — `u/ i }. y` for each i — as one array.
/// This is the answer `u/\.` had before it learnt to carry an accumulator,
/// and it is what it must still give.
fn suffixes(verb: &str, y: &Array) -> Array {
    let n = y.shape[0];
    let cells: Vec<Array> = (0..n)
        .map(|i| run(&format!("{verb}/ {i} }}. {{y}}"), std::slice::from_ref(y)))
        .collect();
    let mut out = Vec::with_capacity(n);
    for c in &cells {
        out.extend(c.to_f64_vec().expect("numeric"));
    }
    Array::from_f64(out)
}

/// The running suffix fold, against every suffix folded from scratch.
///
/// Right to left is the insert's own order, so the two are the same
/// operations in the same order and the floats must agree to the last bit —
/// including for the verbs no fast path covers (`-`, `%`) and for the
/// affine step, whose one-pass form is the recurrence itself.
#[test]
fn the_running_suffix_fold_matches_folding_every_suffix() {
    let mut rng = Rng(11);
    for n in [1usize, 2, 5, 40] {
        let y = Array::from_f64(rng.f64s(n).iter().map(|x| x + 1.5).collect());
        for verb in ["+", "-", "*", "%", ">.", "<.", "([ + 0.5 * ])", "((0.25 * ]) + [)"] {
            let fast = run(&format!("{verb}/\\. {{y}}"), std::slice::from_ref(&y));
            assert_eq!(fast.shape, vec![n], "{verb}");
            identical(&fast, &suffixes(verb, &y));
        }
    }
}

/// An affine step the recogniser does not know — the same arithmetic
/// written with a bond and an atop — must give exactly what the recognised
/// spelling gives, backwards. Forwards the one-pass form carries the power
/// of the constant instead of folding each prefix from its tail, which is
/// the same series regrouped and rounds accordingly.
#[test]
fn a_recognised_affine_step_answers_what_the_general_one_answers() {
    let mut rng = Rng(13);
    let y = Array::from_f64(rng.f64s(64).iter().map(|x| x + 1.5).collect());
    let arg = std::slice::from_ref(&y);
    identical(
        &run("([ + 0.5 * ])/\\. {y}", arg),
        &run("([ + (0.5&*)@]) /\\. {y}", arg),
    );
    agree(&run("([ + 0.5 * ])/\\ {y}", arg), &run("([ + (0.5&*)@]) /\\ {y}", arg));
    // A power that leaves the finite range is refused rather than carried:
    // the general fold runs and answers what it always did.
    identical(
        &run("([ + 1e300 * ])/\\ {y}", arg),
        &run("([ + (1e300&*)@]) /\\ {y}", arg),
    );
}

/// The hashed grouping and the compared one, on the same groups: whole
/// numbers as integers take the hash, the same numbers as floats are
/// compared under the tolerance, and both must find the same groups in the
/// same first-occurrence order.
#[test]
fn the_hashed_key_finds_the_groups_the_compared_one_finds() {
    let mut rng = Rng(17);
    for n in [1usize, 3, 500] {
        let k: Vec<i64> = rng.i64s(n, 4);
        let keys = Array::from_i64(k.clone());
        let floats = Array::from_f64(k.iter().map(|&x| x as f64).collect());
        let v = Array::from_f64(rng.f64s(n));
        for verb in ["+/", "#", "{.", "(+/ % #)"] {
            let src = format!("{{k}} {verb}/. {{v}}");
            let fast = run(&src, &[keys.clone(), v.clone()]);
            let slow = run(&src, &[floats.clone(), v.clone()]);
            identical(&fast, &slow);
        }
        // The keys themselves, as items of a table: one row per key, which
        // is the wide shape the hash builds a vector of element keys for.
        let table = Array::new(
            vec![n, 2],
            Data::I64(k.iter().flat_map(|&x| [x, x * 2]).collect::<Vec<_>>().into()),
        );
        let wide = run("{k} +/. {v}", &[table, v.clone()]);
        identical(&wide, &run("{k} +/. {v}", &[keys.clone(), v.clone()]));
    }
}

/// The first element of a buffer, as an address: two arrays that share a
/// buffer report the same one, and a copy never does.
fn addr(a: &Array) -> usize {
    match &a.data {
        Data::F64(v) => v.as_slice().as_ptr() as usize,
        Data::I64(v) => v.as_slice().as_ptr() as usize,
        d => panic!("no address for {d:?}"),
    }
}

/// A reshape the argument's own elements cover is a change of shape and
/// nothing else, so the buffer comes through shared. A reshape that has to
/// go round the ravel again still copies.
#[test]
fn a_reshape_that_keeps_the_elements_shares_the_buffer() {
    let y = Array::from_f64((0..12).map(|i| i as f64).collect());
    let arg = std::slice::from_ref(&y);
    let here = addr(&y);
    for src in ["3 4 $ {y}", "2 3 $ {y}", "2 2 3 $ {y}", "12 $ {y}", ", {y}"] {
        assert_eq!(addr(&run(src, arg)), here, "{src} copied");
    }
    // Longer than the ravel: the fill cycles, and that is a new buffer.
    assert_ne!(addr(&run("4 4 $ {y}", arg)), here);
    // The values are what they always were.
    assert_eq!(run("3 4 $ {y}", arg), run("3 4 $ 12 $ {y}", arg));
    assert_eq!(
        run("4 4 $ {y}", arg).to_f64_vec().expect("floats")[13],
        1.0,
        "the cyclic refill starts over"
    );
}
