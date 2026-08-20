//! The CPU feature levels against each other.
//!
//! One artifact carries several compilations of every hot loop, and which
//! one runs is a runtime decision. That is only allowed to change how fast
//! a program is. This suite pins that down: every level this machine can
//! run computes the same thing, on the same data, for a program of each
//! shape the dispatch covers — an elementwise chain, an unfused pass, a
//! reduction, a scan, a moving window, a fused standard deviation.
//!
//! Elementwise results must be identical bit for bit: vectorising
//! `dst[i] = f(a[i])` reorders nothing. A float reduction is compared to
//! 1e-12 relative instead, since the §5.9 contract already allows an
//! associative float fold to be regrouped.

use jay::simd::{available, detected, level, set_level, Level};
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

/// The data bound to a parameter. The name decides the dtype and the shape:
/// `m` is a matrix of narrow items and `mw` one of wide items, which are the
/// two shapes the typed reduction splits on.
fn data(name: &str, n: usize) -> Array {
    let mut rng = Rng::new(seed_of(name));
    match name {
        "x" | "w" | "v" => {
            Array::from_f64((0..n).map(|_| (rng.unit() * 8.0 - 4.0).round() / 4.0).collect())
        }
        "a" | "b" => Array::from_i64((0..n).map(|_| (rng.next() % 21) as i64 - 10).collect()),
        "m" | "mw" => {
            let cols = if name == "m" { 8 } else { 512 };
            let vals: Vec<f64> = (0..n).map(|_| (rng.unit() * 8.0 - 4.0).round() / 4.0).collect();
            Array::new(vec![n / cols, cols], Data::F64(vals.into()))
        }
        other => panic!("no data for parameter {other}"),
    }
}

fn program(lang: Lang, src: &str) -> Program {
    compile(lang, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile failed:\n{}", e.render(src)))
}

fn run(p: &Program, args: &[Array]) -> Array {
    let mut sink = |_: &str| {};
    p.run(args, &mut sink)
        .unwrap_or_else(|e| panic!("run failed:\n{}", p.render_error(&e)))
        .expect("the program yielded no value")
}

/// What two levels are allowed to differ by: nothing at all, except for a
/// float scalar, which a reduction may have regrouped.
fn agree(base: &Array, got: &Array, at: Level, src: &str, n: usize) {
    if base.rank() == 0 && base.dtype() == DType::F64 && got.dtype() == DType::F64 {
        let (x, y) = (got.to_f64_vec().unwrap()[0], base.to_f64_vec().unwrap()[0]);
        let ok = x == y || (x - y).abs() <= 1e-12 * y.abs();
        assert!(ok, "{}: {x}, baseline {y}, on `{src}` at {n} elements", at.name());
        return;
    }
    assert_eq!(base, got, "{} differs from baseline on `{src}` at {n} elements", at.name());
}

/// The programs every level must agree on. Between them they reach each
/// loop the dispatch covers: the fused block kernels, the unfused
/// elementwise passes, the typed reduction over both item widths, the scan
/// and the moving window.
const PROGRAMS: &[(Lang, &str)] = &[
    // Fused elementwise chains, float and integer.
    (Lang::J, "(2 * {x}) + 1"),
    (Lang::J, "{w} * {x} + {v}"),
    (Lang::J, "| {x} - {w}"),
    (Lang::J, "*: {x} % {w}"),
    (Lang::J, "(2 * {a}) + 1"),
    (Lang::J, "{a} * {b} + 1"),
    (Lang::J, "-. {x} > {w}"),
    // One verb on its own does not fuse: these are the unfused passes.
    (Lang::J, "{x} * {w}"),
    (Lang::J, "{a} + {b}"),
    (Lang::J, "{x} > 0.5"),
    (Lang::J, "^ {x}"),
    (Lang::J, "- {a}"),
    // Reductions, absorbed into a kernel and standing alone.
    (Lang::J, "+/ {w} * {x}"),
    (Lang::J, "+/ ^ {x}"),
    (Lang::J, "+/ {x} > 0.5"),
    (Lang::J, "+/ {x}"),
    (Lang::J, "+/ {a}"),
    (Lang::J, ">./ {x}"),
    // The typed reduction over items: narrow and wide.
    (Lang::J, "+/ {m}"),
    (Lang::J, "+/ {mw}"),
    // A named value moved into the kernels that read it.
    (Lang::J, "d =. {x} - (+/ {x}) % # {x}\n%: (+/ d * d) % # d"),
    // Scans and moving windows.
    (Lang::J, "+/\\ {x}"),
    (Lang::J, "+/\\ {m}"),
    (Lang::J, "*/\\ {a}"),
    (Lang::J, "20 +/\\ {x}"),
    (Lang::J, "20 >./\\ {x}"),
    (Lang::J, "20 +/\\ {a}"),
    // The other frontend reaches the same runtime.
    (Lang::Apl, "+/ {x} × {w}"),
];

/// Sizes on both sides of the threshold that splits a pass across threads,
/// each divisible by both matrix widths.
const SIZES: &[usize] = &[1_024, 131_072];

#[test]
fn every_level_computes_the_same_values() {
    // One test, not one per program: the level is process-wide, so tests
    // that set it cannot run beside each other.
    for &(lang, src) in PROGRAMS {
        let p = program(lang, src);
        for &n in SIZES {
            let args: Vec<Array> = p.params.iter().map(|s| data(&s.name, n)).collect();
            let mut base: Option<Array> = None;
            for l in available() {
                assert_eq!(set_level(l), l, "level {} would not take effect", l.name());
                assert_eq!(level(), l);
                let got = run(&p, &args);
                match &base {
                    None => base = Some(got),
                    Some(b) => agree(b, &got, l, src, n),
                }
            }
        }
    }
    set_level(detected());
}

#[test]
fn the_baseline_is_always_available_and_the_machines_level_is_the_last() {
    let all = available();
    assert_eq!(all.first().copied(), Some(Level::Baseline));
    assert_eq!(all.last().copied(), Some(detected()));
}
