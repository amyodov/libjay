//! The fusion pass against itself: every fused program must compute exactly
//! what the chain it replaced computes, on data of every dtype the kernel
//! claims, and hand back to that chain wherever it does not.

use std::sync::{RwLock, RwLockReadGuard};

use jay::fuse::{fallback_count, is_fused, is_inlined, unfused};
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

/// The kernel's fallback counter is one number for the whole process, so
/// reading it says something only while nothing else in this binary is
/// running a program. Every run here takes this lock — shared for an
/// ordinary run, exclusive around a measurement — which is what makes the
/// count belong to the test that measured it and not to whichever tests
/// happened to run beside it.
static RUNS: RwLock<()> = RwLock::new(());

/// A shared claim, for a test that runs a program without going through
/// [`run`]. A lock another test poisoned by failing still serves: the
/// counter is the only thing it guards.
fn shared_runs() -> RwLockReadGuard<'static, ()> {
    RUNS.read().unwrap_or_else(|e| e.into_inner())
}

fn run(p: &Program, args: &[Array]) -> Result<Option<Array>, String> {
    let _shared = shared_runs();
    run_alone(p, args)
}

/// A run by a caller that holds the lock already.
fn run_alone(p: &Program, args: &[Array]) -> Result<Option<Array>, String> {
    let mut sink = |_: &str| {};
    p.run(args, &mut sink).map_err(|e| p.render_error(&e))
}

/// Run `f` with the fallback counter to ourselves, and report both what it
/// yielded and how far the counter moved. `f` must run its programs with
/// [`run_alone`]: the lock is held exclusively for the whole call.
fn fallbacks_during<T>(f: impl FnOnce() -> T) -> (T, u64) {
    let _exclusive = RUNS.write().unwrap_or_else(|e| e.into_inner());
    let before = fallback_count();
    let out = f();
    (out, fallback_count() - before)
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
    if let (Ok(Some(a)), Ok(Some(b))) = (&fused, &plain)
        && a.rank() == 0 && a.dtype() == DType::F64 && b.dtype() == DType::F64
    {
        let (x, y) = (a.to_f64_vec().unwrap()[0], b.to_f64_vec().unwrap()[0]);
        let ok = x == y || (x - y).abs() <= 1e-12 * y.abs();
        assert!(ok, "fused {x}, unfused {y} on `{src}` at {n} elements");
        return;
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

// ----------------------------------------------------------- the windows

/// Chains with a window stage in them. Each is checked against its own
/// unfused tree, where the windows are a pass of their own over an array
/// the chain then reads back.
const WINDOWS: &[&str] = &[
    // The Bollinger z-score: the program the stage exists for. `s` is the
    // moving sum, `19 }. {x}` the closes aligned on the window's last item.
    "s =. 20 +/\\ {x}\n((20 * 19 }. {x}) - s) % %: 0.001 + (20 * 20 +/\\ *: {x}) - s * s",
    // A moving mean against the item it ends on.
    "(19 }. {x}) - (20 +/\\ {x}) % 20",
    // The moving range, and the item's place inside it.
    "(20 >./\\ {x}) - 20 <./\\ {x}",
    "((19 }. {x}) - 20 <./\\ {x}) % 0.001 + (20 >./\\ {x}) - 20 <./\\ {x}",
    // Windows over a chain the kernel computes for itself.
    "20 +/\\ *: {x} + 1",
    "1 + 5 */\\ 1 + {x} % 100",
    "3 +/\\ {x} <. {w}",
    // Integers and booleans through the same stage.
    "(4 }. {a}) - 5 +/\\ {a}",
    "5 >./\\ {a} * 2",
    "3 +/\\ {p} + {q}",
    // A reduction over the windows, folded into the same pass.
    "+/ 20 +/\\ *: {x}",
    "<./ 5 +/\\ {x} + {w}",
    // A window over a window: the outer one is the stage, the inner one is
    // the pass it has always been.
    "1 + 4 +/\\ 3 +/\\ {x}",
    // A scalar beside the windows, which stands on either axis.
    "2 * 20 +/\\ {x}",
];

/// Chains with a running fold in them.
const SCANS: &[&str] = &[
    "(+/\\ {x}) % 1 + | {x}",
    "(+/\\ *: {x}) - +/\\ {x}",
    ">./\\ {x} - {w}",
    "1 + +/\\ {a} * 2",
    "{x} - +/\\ {x} * 0.001",
    "+/\\ {p} + {q}",
    // APL's scan is the same stage under its own spelling.
    "1 + +\\ 2 × {x}",
];

#[test]
fn every_window_chain_computes_what_it_replaced() {
    for src in WINDOWS {
        assert!(is_fused(&program(Lang::J, src)), "`{src}` did not fuse");
        // Sizes shorter than the window, either side of the parallel
        // threshold, and one that is not a whole number of blocks.
        for n in [1usize, 5, 25, 1_000, 100_000] {
            same(Lang::J, src, n);
        }
    }
}

#[test]
fn every_scan_chain_computes_what_it_replaced() {
    for src in SCANS {
        let lang = if src.contains('×') { Lang::Apl } else { Lang::J };
        assert!(is_fused(&program(lang, src)), "`{src}` did not fuse");
        for n in [1usize, 2, 25, 1_000, 100_000] {
            same(lang, src, n);
        }
    }
}

/// A window is folded from the items it covers and nothing else, so a huge
/// value in one window leaves the next window exactly as it was. A running
/// accumulator differenced pairwise — the cheap way to move a sum along —
/// would carry that value into every window after it.
#[test]
fn a_window_carries_no_error_from_the_windows_before_it() {
    let n = 10_000;
    let mut v = vec![1.0f64; n];
    v[0] = 1e17;
    let p = program(Lang::J, "1 + 4 +/\\ {x}");
    let args = vec![Array::from_f64(v)];
    let got = run(&p, &args).unwrap().unwrap().to_f64_vec().unwrap();
    assert_eq!(got[0], 1e17 + 4.0);
    // Every window past the first covers four ones exactly.
    for (i, &g) in got.iter().enumerate().skip(1) {
        assert_eq!(g, 5.0, "window {i} carried the first item's error");
    }
    assert_eq!(run(&p, &args), run(&unfused(&p), &args));
}

/// Where the shapes do not prove that the other inputs are aligned on the
/// window's last item, the kernel hands the chain back and the chain says
/// what it would have said.
#[test]
fn a_chain_the_window_cannot_align_hands_itself_back() {
    let cases: &[&str] = &[
        // One item too many on the left: a length error, and the same one.
        "(18 }. {x}) - 20 +/\\ {x}",
        // The whole argument against its own windows.
        "{x} - 20 +/\\ {x}",
        // Two window lengths in one chain: two wide axes, so neither is a
        // stage and the sentence runs as it was written.
        "(20 +/\\ {x}) - 10 +/\\ 11 }. {x}",
    ];
    for src in cases {
        let p = program(Lang::J, src);
        let args: Vec<Array> = p.params.iter().map(|s| data(&s.name, 1_000)).collect();
        assert_eq!(run(&p, &args), run(&unfused(&p), &args), "`{src}`");
    }
}

#[test]
fn a_window_the_axis_cannot_hold_hands_the_chain_back() {
    // No window of twenty items fits three of them: J's answer is an empty
    // result whose shape the verb decides, which is the chain's business.
    let p = program(Lang::J, "1 + 20 +/\\ {x}");
    let plain = unfused(&p);
    let ((), declines) = fallbacks_during(|| {
        for n in [1usize, 3, 19] {
            let args = vec![data("x", n)];
            assert_eq!(run_alone(&p, &args), run_alone(&plain, &args), "at {n} items");
        }
    });
    assert!(declines > 0, "the kernel did not decline the short axis");
}

#[test]
fn the_edges_of_the_window_length_hold() {
    // One item per window is the argument itself; the whole axis in one
    // window is one item.
    for (src, n) in [("1 + 1 +/\\ {x}", 64usize), ("1 + 64 +/\\ {x}", 64), ("1 + 63 +/\\ {x}", 64)] {
        let p = program(Lang::J, src);
        assert!(is_fused(&p), "`{src}` did not fuse");
        let args = vec![data("x", n)];
        assert_eq!(run(&p, &args), run(&unfused(&p), &args), "`{src}` at {n}");
    }
    // A window longer than the kernel takes is left to the pass it was.
    let long = format!("1 + {} +/\\ {{x}}", jay::fuse::MAX_WINDOW + 1);
    let p = program(Lang::J, &long);
    assert!(!is_fused(&p), "a window past the limit was absorbed");
}

/// The window leaves of a chain all stand on the same axis, so an
/// elementwise tree over them is as valid as one over plain columns. These
/// are generated the same way the elementwise fuzz generates its own.
#[test]
fn random_window_chains_agree_with_the_interpreter() {
    let windowed: &[&str] = &[
        "(19 }. {x})",
        "(20 +/\\ {x})",
        "(20 >./\\ {x})",
        "(20 <./\\ {w})",
        "(20 +/\\ *: {w})",
        "(19 }. {a})",
        "(20 +/\\ {a})",
        "(20 <./\\ {b})",
        "2",
        "0.5",
    ];
    let mut rng = Rng::new(20_260_822);
    let mut fused_any = 0;
    for i in 0..300 {
        let body = expr_of(&mut rng, 3, windowed);
        let src = if i % 4 == 0 { format!("+/ {body}") } else { body };
        let p = program(Lang::J, &src);
        if is_fused(&p) {
            fused_any += 1;
        }
        let args: Vec<Array> = p.params.iter().map(|s| data(&s.name, 117)).collect();
        match (run(&p, &args), run(&unfused(&p), &args)) {
            (Ok(Some(f)), Ok(Some(u))) => {
                assert!(identical(&f, &u), "`{src}`\n  fused {f:?}\n  plain {u:?}")
            }
            (f, u) => assert_eq!(f, u, "`{src}`"),
        }
    }
    assert!(fused_any > 200, "only {fused_any} of 300 random window chains fused");
}

#[test]
fn random_scan_chains_agree_with_the_interpreter() {
    let scanned: &[&str] = &["{x}", "(+/\\ {x})", "(>./\\ {w})", "(+/\\ {a})", "{a}", "2", "0.5"];
    let mut rng = Rng::new(20_260_823);
    for i in 0..300 {
        let body = expr_of(&mut rng, 3, scanned);
        let src = if i % 4 == 0 { format!("+/ {body}") } else { body };
        let p = program(Lang::J, &src);
        let args: Vec<Array> = p.params.iter().map(|s| data(&s.name, 117)).collect();
        match (run(&p, &args), run(&unfused(&p), &args)) {
            (Ok(Some(f)), Ok(Some(u))) => {
                assert!(identical(&f, &u), "`{src}`\n  fused {f:?}\n  plain {u:?}")
            }
            (f, u) => assert_eq!(f, u, "`{src}`"),
        }
    }
}

// ------------------------------------------------- across the sentences

/// Every program here names a value that nothing needs as an array, so the
/// pass moves it into the sentences that read it. Each is checked against
/// the sentences it was compiled from, which is what `unfused` returns.
const NAMED: &[&str] = &[
    // The standard deviation, in the spelling this pass exists for: the
    // deviations are a whole column, and no one wants them.
    "d =. {x} - (+/ {x}) % # {x}\n%: (+/ d * d) % # d",
    // The same, with the mean named too, so the moves are transitive.
    "m =. (+/ {x}) % # {x}\nd =. {x} - m\n%: (+/ d * d) % # d",
    // One use, and the sentence that reads it needs no array either.
    "d =. {x} * 2\n+/ d + 1",
    "d =. {x} + 1\n# d",
    // Two names, read by one sentence.
    "u =. {x} + 1\nv2 =. {w} * 2\n+/ u * v2",
    // One name, read by two sentences.
    "d =. {x} + 1\ns =. +/ d * d\ns + +/ d * {w}",
    // A name read twice inside one kernel, and once inside another.
    "d =. {a} + 1\n(+/ d * d) + +/ d * {b}",
    // Integers and booleans through the same machinery.
    "d =. {a} * {b}\n+/ d + d",
    "d =. {x} > 0.5\n+/ d + d",
    "d =. {p} + {q}\n+/ d * d",
    // Work worth doing once: the kernel keeps the exponentials of a block
    // in a slot rather than computing them for each use.
    "d =. ^ {x}\n+/ d * d",
    "d =. ^ {x} - {w}\n(+/ d) % # d",
    // A chain that leaves i64 mid-way, so the kernel declines and the
    // sentences it was made from run instead.
    "d =. {big} * {big}\n+/ d + d",
    // A reduction of its own is a scalar already: it stays a sentence, and
    // the sentence that reads it splats it into the kernel.
    "s =. +/ {x} * {w}\nd =. {x} - s\n+/ d * d",
    // An assignment the pass cannot move, in the same program as one it can.
    "z =. {x} + 1\ny2 =. 3 }. z\n(+/ z * z) + # y2",
    // APL, whose assignment is the same edge under another spelling.
    "d ← {x} - (+/ {x}) ÷ ≢ {x}\n(+/ d × d) ÷ ≢ d",
];

#[test]
fn a_named_value_computes_what_the_sentences_computed() {
    for src in NAMED {
        let lang = if src.contains('←') { Lang::Apl } else { Lang::J };
        for n in [1usize, 2, 3, 1_000, 100_000] {
            same(lang, src, n);
        }
    }
}

#[test]
fn the_two_spellings_of_the_standard_deviation_now_fuse_alike() {
    let named = program(Lang::J, "d =. {x} - (+/ {x}) % # {x}\n%: (+/ d * d) % # d");
    assert!(is_inlined(&named), "the named deviations were still materialised");
    let args = vec![data("x", 100_000)];
    let one = program(
        Lang::J,
        "%: (+/ ({x} - (+/ {x}) % # {x}) * {x} - (+/ {x}) % # {x}) % # {x}",
    );
    // Both are now one pass for the mean and one fused map-reduce over it —
    // the same kernel over the same scalars, so they agree to the last bit
    // rather than merely to the float contract the two folds are held to.
    assert_eq!(run(&named, &args), run(&one, &args));
    same(Lang::J, "d =. {x} - (+/ {x}) % # {x}\n%: (+/ d * d) % # d", 100_000);
}

/// The programs the pass must leave exactly as they are, each with the
/// reason it must: the value would have to be materialised anyway.
#[test]
fn a_use_a_kernel_cannot_take_keeps_the_name() {
    let cases: &[&str] = &[
        // Displayed: the array itself is the point.
        "d =. {x} + 1\nd",
        // Printed on the way past.
        "d =. {x} + 1\necho d\n+/ d * d",
        // Read by a verb no kernel covers.
        "d =. {x} + 1\n|. d",
        "d =. {x} + 1\n3 }. d",
        // Read once inside a kernel and once as a whole array.
        "d =. {x} + 1\n(+/ d * d) , d",
        // Assigned again later, so the copies would not all mean the same.
        "d =. {x} + 1\ns =. +/ d * d\nd =. 5\ns",
        // Defined in terms of itself.
        "d =. {x} + 1\nd =. d + 1\n+/ d * d",
        // A single verb: moving it would move a whole pass, not remove one.
        "d =. 3 }. {x}\n+/ d * d",
        // Nothing reads it at all.
        "d =. {x} + 1\n+/ {x} * {x}",
    ];
    for src in cases {
        assert!(!is_inlined(&program(Lang::J, src)), "`{src}` moved a name it must not");
        for n in [3usize, 1_000] {
            same(Lang::J, src, n);
        }
    }
}

#[test]
fn an_assignment_as_the_last_sentence_still_yields_nothing() {
    for src in ["d =. {x} + 1\n+/ d * d\ne =. {x} * 2", "d =. {x} + 1\ne =. +/ d * d"] {
        let p = program(Lang::J, src);
        let args = vec![data("x", 1_000)];
        assert_eq!(run(&p, &args), Ok(None), "`{src}`");
        assert_eq!(run(&p, &args), run(&unfused(&p), &args));
    }
}

#[test]
fn output_between_the_sentences_happens_once_and_in_place() {
    let src = "d =. {x} + 1\necho 2 + 2\n+/ d * d";
    let p = program(Lang::J, src);
    assert!(is_inlined(&p), "an unrelated output must not block the move");
    let _shared = shared_runs();
    let args = vec![data("x", 1_000)];
    let mut fused = String::new();
    let a = p.run(&args, &mut |s| fused.push_str(s)).expect("run");
    let mut plain = String::new();
    let b = unfused(&p).run(&args, &mut |s| plain.push_str(s)).expect("run");
    assert_eq!(fused, plain);
    assert_eq!(fused.matches('\n').count(), 1, "{fused:?}");
    assert_eq!(a, b);
}

#[test]
fn the_error_a_named_value_raised_is_raised_where_it_was() {
    // The sentence that assigned `d` is gone, but the tally that replaces
    // it reaches the same type error, before the sentence that reads it.
    let src = "d =. {c} + 1\necho 5\n+/ d * d";
    let p = program(Lang::J, src);
    let _shared = shared_runs();
    let args = vec![data("c", 100)];
    let mut fused = String::new();
    let a = p.run(&args, &mut |s| fused.push_str(s)).map_err(|e| p.render_error(&e));
    let plain_p = unfused(&p);
    let mut plain = String::new();
    let b = plain_p.run(&args, &mut |s| plain.push_str(s)).map_err(|e| plain_p.render_error(&e));
    assert!(a.is_err(), "the type error survived the move");
    assert_eq!(a, b);
    assert_eq!(fused, plain, "the error moved past an output");
    assert!(fused.is_empty(), "the error must come first: {fused:?}");
}

#[test]
fn a_length_error_in_a_named_value_reads_the_same() {
    let src = "d =. {w} * {x}\n+/ d * d";
    let p = program(Lang::J, src);
    let args = vec![data("w", 4), data("x", 3)];
    let fused = run(&p, &args);
    assert_eq!(fused, run(&unfused(&p), &args));
    let text = fused.unwrap_err();
    assert!(text.contains("length error"), "{text}");
    // The caret still points into the sentence that wrote the value.
    assert!(text.contains("\n       ^"), "{text}");
}

#[test]
fn a_named_map_matches_the_unfused_one_bit_for_bit() {
    let src = "d =. {x} * {w}\nd + d % {v}";
    let p = program(Lang::J, src);
    assert!(is_inlined(&p));
    for n in [1_000usize, 300_000] {
        let args: Vec<Array> = p.params.iter().map(|s| data(&s.name, n)).collect();
        let f = run(&p, &args).unwrap().unwrap();
        let u = run(&unfused(&p), &args).unwrap().unwrap();
        assert!(identical(&f, &u), "at {n} elements");
    }
}

// ------------------------------------------------------------ the tally

#[test]
fn a_tally_over_a_chain_counts_without_computing_it() {
    for src in ["# {x} - {w}", "# 2 * {x}", "1 + # {x} * {w}", "(+/ {x} * {w}) % # {x} - {w}"] {
        assert!(is_fused(&program(Lang::J, src)), "`{src}` did not fuse");
        for n in [1usize, 3, 1_000, 100_000] {
            same(Lang::J, src, n);
        }
    }
    // A chain the kernel declines still gets its count from the chain.
    let p = program(Lang::J, "# {m} * {x}");
    let m = Array::new(vec![3, 4], Data::F64((0..12).map(|i| i as f64).collect()));
    let args = vec![m, Array::from_f64(vec![1.0, 2.0, 3.0])];
    assert_eq!(run(&p, &args), run(&unfused(&p), &args));
    assert_eq!(run(&p, &args).unwrap().unwrap(), Array::scalar_i64(3));
    // And a chain that would fail reports what it fails with.
    let p = program(Lang::J, "# 1 + 2 * {c}");
    let args = vec![data("c", 10)];
    let got = run(&p, &args);
    assert_eq!(got, run(&unfused(&p), &args));
    assert!(got.unwrap_err().contains("character and numeric"));
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
    let (got, declines) = fallbacks_during(|| run_alone(&p, &args).unwrap().unwrap());
    assert!(declines > 0, "the kernel did not decline");
    assert_eq!(got.dtype(), DType::I64);
    assert_eq!(got, run(&unfused(&p), &args).unwrap().unwrap());

    // The same rule where the integers are exact and f64's would not be:
    // the sum of two large integers, then a division.
    let src = "({big} + {big}) % 2";
    let p = program(Lang::J, src);
    assert!(is_fused(&p));
    let args = vec![data("big", 1_000)];
    let (got, declines) = fallbacks_during(|| run_alone(&p, &args).unwrap().unwrap());
    assert!(declines > 0, "the kernel did not decline");
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
    let (got, declines) = fallbacks_during(|| run_alone(&p, &args).unwrap().unwrap());
    assert!(declines > 0, "the kernel did not decline");
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
    let (got, declines) = fallbacks_during(|| run_alone(&p, &args).unwrap().unwrap());
    assert!(declines > 0, "the kernel did not decline");
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
    // The two fold in different groupings, which §5.9 licenses, and the
    // grouping depends on the pool size — so the bound has to hold at
    // every LIBJAY_THREADS. Four million additions regrouped stay well
    // inside a relative 1e-10; a real semantic difference would not.
    assert!((a - b).abs() <= 1e-10 * b.abs(), "fused {a}, unfused {b}");
}

// ------------------------------------------------------------- the fuzz

/// The verbs the kernel claims, in J. Monads first, then dyads; the
/// generator below builds expressions out of nothing else, so every one of
/// them is checked against the interpreter on every dtype.
const MONADS: &[&str] = &["+", "-", "|", "*", "%", "<.", ">.", ">:", "<:", "+:", "-:", "*:", "-.", "^"];
const DYADS: &[&str] = &["+", "-", "*", "%", "<.", ">.", "|", "=", "~:", "<", "<:", ">", ">:"];
const LEAVES: &[&str] = &["{x}", "{w}", "{v}", "{a}", "{b}", "{p}", "{q}", "2", "_3", "0.5", "0"];

/// The same, plus a name assigned by an earlier sentence.
const NAMED_LEAVES: &[&str] = &["{x}", "{w}", "{a}", "{p}", "2", "0.5", "dd", "dd", "dd"];

fn expr_of(rng: &mut Rng, depth: u32, leaves: &[&str]) -> String {
    if depth == 0 || rng.next() % 5 == 0 {
        return leaves[(rng.next() % leaves.len() as u64) as usize].to_string();
    }
    if rng.next() % 3 == 0 {
        let op = MONADS[(rng.next() % MONADS.len() as u64) as usize];
        return format!("({op} {})", expr_of(rng, depth - 1, leaves));
    }
    let op = DYADS[(rng.next() % DYADS.len() as u64) as usize];
    format!("({} {op} {})", expr_of(rng, depth - 1, leaves), expr_of(rng, depth - 1, leaves))
}

fn expr(rng: &mut Rng, depth: u32) -> String {
    expr_of(rng, depth, LEAVES)
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
    let (mut fused_any, mut ran) = (0, 0);
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
        let (fused, declines) = fallbacks_during(|| run_alone(&p, &args));
        if is_fused(&p) && declines == 0 {
            ran += 1;
        }
        match (fused, run(&unfused(&p), &args)) {
            (Ok(Some(f)), Ok(Some(u))) => {
                assert!(identical(&f, &u), "`{src}`\n  fused {f:?}\n  plain {u:?}")
            }
            (f, u) => assert_eq!(f, u, "`{src}`"),
        }
    }
    // The acceptance rate: how many of the chains the pass fused were run by
    // the kernel rather than handed back at run time. `--nocapture` prints
    // it; a change to the type rules is what moves it. The rest decline for
    // one reason — an integer-typed STEP along a float path, which f64
    // cannot hold exactly past 53 bits. An integer LEAF is not such a step:
    // the unfused chain widens it once too, so the kernel takes it.
    println!("random chains: {fused_any} of 400 fused, {ran} run by the kernel");
    assert!(fused_any > 250, "only {fused_any} of 400 random chains fused");
    assert!(ran > 60, "only {ran} of {fused_any} fused chains reached the kernel");
}

#[test]
fn random_named_programs_agree_with_the_interpreter() {
    let mut rng = Rng::new(20_260_821);
    let mut moved = 0;
    let mut ran = 0;
    for i in 0..400 {
        let body = expr_of(&mut rng, 3, NAMED_LEAVES);
        if !body.contains("dd") {
            continue;
        }
        ran += 1;
        let def = expr(&mut rng, 3);
        // Every third one is reduced, and every fifth counts the name
        // rather than reading it.
        let src = match i % 5 {
            0 => format!("dd =. {def}\n+/ {body}"),
            1 => format!("dd =. {def}\n(# dd) + {body}"),
            _ => format!("dd =. {def}\n{body}"),
        };
        let p = program(Lang::J, &src);
        if is_inlined(&p) {
            moved += 1;
        }
        let args: Vec<Array> = p.params.iter().map(|s| data(&s.name, 97)).collect();
        match (run(&p, &args), run(&unfused(&p), &args)) {
            (Ok(Some(f)), Ok(Some(u))) => {
                assert!(identical(&f, &u), "`{src}`\n  fused {f:?}\n  plain {u:?}")
            }
            (f, u) => assert_eq!(f, u, "`{src}`"),
        }
    }
    assert!(moved * 2 > ran, "only {moved} of {ran} named programs moved the name");
}

#[test]
fn integer_chains_are_exact_however_they_split() {
    let p = program(Lang::J, "+/ {a} * {b} + 1");
    let args = vec![data("a", 200_000), data("b", 200_000)];
    assert_eq!(run(&p, &args), run(&unfused(&p), &args));
}


