//! The fusion pass against itself: every fused program must compute exactly
//! what the chain it replaced computes, on data of every dtype the kernel
//! claims, and hand back to that chain wherever it does not.

use jay::fuse::{fallback_count, is_fused, unfused};
use jay::{compile, Array, Data, DType, Dialect, Lang, Program};

// ---------------------------------------------------------------- fixtures

/// A deterministic value stream: splitmix64, so the same parameter name
/// gives the same data in every run and on every machine.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x2545_f491_4f6c_dd1d)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn seed_of(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// The data bound to a parameter. The name decides the dtype, so one table
/// of expressions can exercise floats, integers, booleans and characters.
fn data(name: &str, n: usize) -> Array {
    let mut rng = Rng::new(seed_of(name));
    match name {
        // Floats around zero, with a few exact halves and zeros in reach of
        // the comparisons and of `%`'s zero rules.
        "x" | "w" | "v" => {
            Array::from_f64((0..n).map(|_| (rng.unit() * 8.0 - 4.0).round() / 4.0).collect())
        }
        // Small integers, including zero (which `|` and `%` treat specially).
        "a" | "b" => Array::from_i64((0..n).map(|_| (rng.next() % 21) as i64 - 10).collect()),
        // Integers big enough that a product or a sum leaves i64.
        "big" => Array::from_i64(
            (0..n).map(|_| i64::MAX / 3 - (rng.next() % 1_000_000) as i64).collect(),
        ),
        "p" | "q" => Array::new(
            vec![n],
            Data::Bool((0..n).map(|_| (rng.next() % 2) as u8).collect()),
        ),
        "c" => Array::from_chars((0..n).map(|_| (b'a' + (rng.next() % 26) as u8) as char).collect()),
        other => panic!("no data for parameter {other}"),
    }
}

fn program(lang: Lang, src: &str) -> Program {
    compile(lang, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile failed:\n{}", e.render(src)))
}

fn run(p: &Program, args: &[Array]) -> Result<Option<Array>, String> {
    let mut sink = |_: &str| {};
    p.run(args, &mut sink).map_err(|e| p.render_error(&e))
}

/// Run `src` fused and unfused over the same generated data.
fn both(lang: Lang, src: &str, n: usize) -> (Result<Option<Array>, String>, Result<Option<Array>, String>) {
    let p = program(lang, src);
    let args: Vec<Array> = p.params.iter().map(|s| data(&s.name, n)).collect();
    let plain = unfused(&p);
    (run(&p, &args), run(&plain, &args))
}

/// The fused and unfused programs must agree element for element, dtype
/// included; `Array`'s equality compares both.
///
/// The one thing allowed to move is a float reduction: a fused fold blocks
/// the vector differently from an unfused one, and regrouping an
/// associative float fold is the §5.9 contract. That shows only as a scalar
/// (this kernel absorbs a reduction only over a vector), so a rank-0 float
/// pair is compared to 1e-12 relative and everything else exactly.
fn same(lang: Lang, src: &str, n: usize) {
    let (fused, plain) = both(lang, src, n);
    if let (Ok(Some(a)), Ok(Some(b))) = (&fused, &plain) {
        if a.rank() == 0 && a.dtype() == DType::F64 && b.dtype() == DType::F64 {
            let (x, y) = (a.to_f64_vec().unwrap()[0], b.to_f64_vec().unwrap()[0]);
            let ok = x == y || (x - y).abs() <= 1e-12 * y.abs();
            assert!(ok, "fused {x}, unfused {y} on `{src}` at {n} elements");
            return;
        }
    }
    assert_eq!(fused, plain, "fused and unfused disagree on `{src}` at {n} elements");
}

// ------------------------------------------------------- the equivalence

/// Every expression here fuses; each is checked against its own unfused
/// tree. The sizes straddle `par::MIN_WORK`, so both the sequential and the
/// chunked paths run.
const CHAINS: &[&str] = &[
    "(2 * {x}) + 1",
    "1 + 2 * 3 - {x}",
    "{w} * {x} + {w}",
    "| {x} - {w}",
    "*: {x} - {w}",
    "{x} <. {w} >. 0",
    ">: <: {x} + 1",
    "- - {x}",
    "{x} + {x} + {x} + {x}",
    "*: {x} % {w}",
    "+: -: {x} + 1",
    "%: 1 + *: {x}",
    "0.5 + {x} * {w} - {v}",
    // Comparisons, at the root and inside the chain.
    "-. {x} > {w}",
    "({x} > {w}) * {v}",
    "1 + {x} ~: {w}",
    // Integers: the same chains over i64 and boolean data.
    "(2 * {a}) + 1",
    "{a} * {b} + 1",
    "| {a} - {b}",
    "3 | {a} + 1",
    "{a} <. {b} >. 0",
    "-. {a} > {b}",
    "1 + {p} * 2",
    "{p} + {q} * 2",
    "-. {p} + {q}",
    "<. 1 + {a}",
    // Reductions absorbed into the kernel.
    "+/ {w} * {x}",
    "+/ ^ {x}",
    "+/ {x} > 0.5",
    "+/ *: {x} - {w}",
    "*/ 1 + {x} % 100",
    "<./ {x} + {w}",
    ">./ {x} * {w}",
    "+/ {a} * {b}",
    "+/ -. {a} > {b}",
    "*/ 1 + 0 * {a}",
    "%: (+/ ({x} - (+/ {x}) % # {x}) * {x} - (+/ {x}) % # {x}) % # {x}",
];

#[test]
fn every_chain_computes_what_it_replaced() {
    for src in CHAINS {
        assert!(is_fused(&program(Lang::J, src)), "`{src}` did not fuse");
        for n in [1usize, 2, 3, 1_000, 100_000] {
            same(Lang::J, src, n);
        }
    }
}

#[test]
fn apl_chains_fuse_through_their_own_spelling() {
    // APL's `+/` is a rank-wrapped reduction and its verbs are the same
    // primitives, so the same kernels must come out.
    for src in ["1 + 2 × {x}", "+/ 2 × {x}", "+/ {w} × {x}", "{x} ⌈ {w} ⌊ 0", "+/ {x} > 0.5"] {
        assert!(is_fused(&program(Lang::Apl, src)), "`{src}` did not fuse");
        for n in [3usize, 1_000, 100_000] {
            same(Lang::Apl, src, n);
        }
    }
}

#[test]
fn a_sequence_fuses_every_sentence() {
    let src = "d =. {x} - (+/ {x}) % # {x}\n%: (+/ d * d) % # d";
    assert!(is_fused(&program(Lang::J, src)));
    for n in [1_000usize, 100_000] {
        same(Lang::J, src, n);
    }
}

// ----------------------------------------------------------- the dtypes

#[test]
fn an_absorbed_reduction_keeps_the_unfused_dtype() {
    let p = program(Lang::J, "+/ {x} > 0.5");
    let args = vec![data("x", 1_000)];
    let got = run(&p, &args).unwrap().unwrap();
    // Booleans reduce as integers, fused or not.
    assert_eq!(got.dtype(), DType::I64);
    assert_eq!(got.shape, Vec::<usize>::new());
    assert_eq!(got, run(&unfused(&p), &args).unwrap().unwrap());
}

#[test]
fn a_comparison_at_the_root_still_yields_booleans() {
    let p = program(Lang::J, "-. {x} > {w}");
    let args = vec![data("x", 500), data("w", 500)];
    let got = run(&p, &args).unwrap().unwrap();
    assert_eq!(got.dtype(), DType::Bool);
    assert_eq!(got, run(&unfused(&p), &args).unwrap().unwrap());
}

#[test]
fn an_integer_step_on_a_float_path_falls_back() {
    // Both comparisons are float work, their sum is an integer: no single
    // working type holds the chain, so the kernel declines.
    let src = "({x} > 0.5) + {w} > 0.5";
    let p = program(Lang::J, src);
    assert!(is_fused(&p));
    let args = vec![data("x", 1_000), data("w", 1_000)];
    let before = fallback_count();
    let got = run(&p, &args).unwrap().unwrap();
    assert!(fallback_count() > before, "the kernel did not decline");
    assert_eq!(got.dtype(), DType::I64);
    assert_eq!(got, run(&unfused(&p), &args).unwrap().unwrap());

    // The same rule where the integers are exact and f64's would not be:
    // the sum of two large integers, then a division.
    let src = "({big} + {big}) % 2";
    let p = program(Lang::J, src);
    assert!(is_fused(&p));
    let args = vec![data("big", 1_000)];
    let before = fallback_count();
    let got = run(&p, &args).unwrap().unwrap();
    assert!(fallback_count() > before, "the kernel did not decline");
    assert_eq!(got, run(&unfused(&p), &args).unwrap().unwrap());
}

// ---------------------------------------------------------- the fallbacks

#[test]
fn integer_overflow_mid_chain_falls_back_to_the_unfused_chain() {
    // `big * big` leaves i64 and J widens the whole thing to float.
    let src = "1 + {big} * {big}";
    let p = program(Lang::J, src);
    assert!(is_fused(&p));
    let args = vec![data("big", 1_000)];
    let before = fallback_count();
    let got = run(&p, &args).unwrap().unwrap();
    assert!(fallback_count() > before, "the kernel did not decline");
    assert_eq!(got.dtype(), DType::F64);
    assert_eq!(got, run(&unfused(&p), &args).unwrap().unwrap());
}

#[test]
fn a_sum_that_overflows_falls_back_too() {
    let src = "+/ {big} + 1";
    let p = program(Lang::J, src);
    assert!(is_fused(&p));
    let args = vec![data("big", 10_000)];
    assert_eq!(run(&p, &args), run(&unfused(&p), &args));
}

#[test]
fn a_frame_that_needs_broadcasting_falls_back() {
    // A 3x4 matrix against a 3-vector agrees by leading prefix, which the
    // kernel's identical-shapes rule does not cover.
    let src = "1 + {m} * {x}";
    let p = program(Lang::J, src);
    assert!(is_fused(&p));
    let m = Array::new(vec![3, 4], Data::F64((0..12).map(|i| i as f64).collect()));
    let x = Array::from_f64(vec![10.0, 20.0, 30.0]);
    let args = vec![m, x];
    let before = fallback_count();
    let got = run(&p, &args).unwrap().unwrap();
    assert!(fallback_count() > before, "the kernel did not decline");
    assert_eq!(got, run(&unfused(&p), &args).unwrap().unwrap());
    assert_eq!(got.shape, vec![3, 4]);
}

#[test]
fn a_higher_rank_argument_still_reduces_correctly() {
    // The absorbed reduction only covers a vector; a matrix folds cells.
    let src = "+/ {m} * 2";
    let p = program(Lang::J, src);
    let m = Array::new(vec![3, 4], Data::F64((0..12).map(|i| i as f64).collect()));
    let args = vec![m];
    assert_eq!(run(&p, &args), run(&unfused(&p), &args));
    assert_eq!(run(&p, &args).unwrap().unwrap().shape, vec![4]);
}

#[test]
fn character_data_reports_the_type_error_the_chain_reports() {
    let src = "1 + 2 * {c}";
    let p = program(Lang::J, src);
    assert!(is_fused(&p));
    let args = vec![data("c", 100)];
    let fused = run(&p, &args);
    assert_eq!(fused, run(&unfused(&p), &args));
    assert!(fused.unwrap_err().contains("character and numeric"));
}

#[test]
fn a_length_mismatch_reports_the_same_error_at_the_same_place() {
    let src = "1 + {w} * {x}";
    let p = program(Lang::J, src);
    let args = vec![data("w", 4), data("x", 3)];
    let fused = run(&p, &args);
    let plain = run(&unfused(&p), &args);
    assert_eq!(fused, plain);
    let text = fused.unwrap_err();
    assert!(text.contains("length error"), "{text}");
    // The caret points at the verb that could not agree, not at the chain.
    assert!(text.contains("\n      ^"), "{text}");
}

#[test]
fn an_effect_in_the_chain_keeps_its_output_once() {
    let src = "1 + 2 * echo {x}";
    let p = program(Lang::J, src);
    assert!(!is_fused(&p), "a chain with output in it must not fuse");
    let args = vec![data("x", 3)];
    let mut out = String::new();
    p.run(&args, &mut |s| out.push_str(s)).expect("run");
    assert_eq!(out.matches('\n').count(), 1, "echo ran twice: {out:?}");
}

#[test]
fn a_verb_that_can_fail_elementwise_stays_out_of_the_kernel() {
    // APL's `÷` by zero is a domain error, so it never joins a kernel and
    // the error survives unchanged.
    let src = "1 + 2 ÷ {a}";
    let p = program(Lang::Apl, src);
    let args = vec![data("a", 100)];
    let fused = run(&p, &args);
    assert_eq!(fused, run(&unfused(&p), &args));
    assert!(fused.unwrap_err().contains("division by zero"));
}

// ------------------------------------------------------------- precision

#[test]
fn a_fused_map_matches_the_unfused_one_bit_for_bit() {
    // Same operations in the same order per element, so no rounding moves.
    for n in [1_000usize, 300_000] {
        same(Lang::J, "({x} * {w}) + {x} % {w}", n);
    }
}

#[test]
fn a_fused_reduce_matches_the_unfused_one_when_neither_splits() {
    // Below the parallel threshold both fold right to left over the whole
    // vector, so the sums are identical to the last bit.
    let p = program(Lang::J, "+/ {x} * {w}");
    let args = vec![data("x", 40_000), data("w", 40_000)];
    assert_eq!(run(&p, &args), run(&unfused(&p), &args));
}

#[test]
fn a_split_reduce_agrees_to_the_float_contract() {
    let p = program(Lang::J, "+/ ^ {x}");
    let args = vec![data("x", 4_000_000)];
    let a = run(&p, &args).unwrap().unwrap().to_f64_vec().unwrap()[0];
    let b = run(&unfused(&p), &args).unwrap().unwrap().to_f64_vec().unwrap()[0];
    assert!((a - b).abs() <= 1e-12 * b.abs(), "fused {a}, unfused {b}");
}

// ------------------------------------------------------------- the fuzz

/// The verbs the kernel claims, in J. Monads first, then dyads; the
/// generator below builds expressions out of nothing else, so every one of
/// them is checked against the interpreter on every dtype.
const MONADS: &[&str] = &["+", "-", "|", "*", "%", "<.", ">.", ">:", "<:", "+:", "-:", "*:", "-.", "^"];
const DYADS: &[&str] = &["+", "-", "*", "%", "<.", ">.", "|", "=", "~:", "<", "<:", ">", ">:"];
const LEAVES: &[&str] = &["{x}", "{w}", "{v}", "{a}", "{b}", "{p}", "{q}", "2", "_3", "0.5", "0"];

fn expr(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 || rng.next() % 5 == 0 {
        return LEAVES[(rng.next() % LEAVES.len() as u64) as usize].to_string();
    }
    if rng.next() % 3 == 0 {
        let op = MONADS[(rng.next() % MONADS.len() as u64) as usize];
        return format!("({op} {})", expr(rng, depth - 1));
    }
    let op = DYADS[(rng.next() % DYADS.len() as u64) as usize];
    format!("({} {op} {})", expr(rng, depth - 1), expr(rng, depth - 1))
}

/// Bit-for-bit array equality, with NaN equal to itself: a fused map does
/// the same arithmetic in the same order, so nothing may move at all.
fn identical(a: &Array, b: &Array) -> bool {
    if a.shape != b.shape || a.dtype() != b.dtype() {
        return false;
    }
    match (&a.data, &b.data) {
        (Data::F64(p), Data::F64(q)) => p.iter().zip(q.iter()).all(|(&x, &y)| x.to_bits() == y.to_bits()),
        _ => a == b,
    }
}

#[test]
fn random_chains_agree_with_the_interpreter() {
    let mut rng = Rng::new(20_260_820);
    let mut fused_any = 0;
    for i in 0..400 {
        let body = expr(&mut rng, 4);
        // Every fourth one is reduced, which exercises the absorbed fold
        // over whatever the map produced.
        let src = if i % 4 == 0 { format!("+/ {body}") } else { body };
        let p = program(Lang::J, &src);
        if is_fused(&p) {
            fused_any += 1;
        }
        // 97 elements: below the parallel threshold, so even a fold runs
        // right to left over the whole vector in both programs.
        let args: Vec<Array> = p.params.iter().map(|s| data(&s.name, 97)).collect();
        match (run(&p, &args), run(&unfused(&p), &args)) {
            (Ok(Some(f)), Ok(Some(u))) => {
                assert!(identical(&f, &u), "`{src}`\n  fused {f:?}\n  plain {u:?}")
            }
            (f, u) => assert_eq!(f, u, "`{src}`"),
        }
    }
    assert!(fused_any > 250, "only {fused_any} of 400 random chains fused");
}

#[test]
fn integer_chains_are_exact_however_they_split() {
    let p = program(Lang::J, "+/ {a} * {b} + 1");
    let args = vec![data("a", 200_000), data("b", 200_000)];
    assert_eq!(run(&p, &args), run(&unfused(&p), &args));
}
