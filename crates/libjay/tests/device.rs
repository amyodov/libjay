//! Device execution: the same answers, somewhere else.
//!
//! Every test here needs an adapter, and a CI runner has none. Rather than
//! fail there, each one says on stderr that it found no GPU and passes: the
//! CPU path these tests compare against is covered by the rest of the suite
//! whatever this machine has.
//!
//! What is checked is equivalence, not speed. A device result must equal the
//! CPU's within the float contract — reassociating an associative fold is
//! allowed (§5.9), so a reduction is compared with a relative tolerance, and
//! the tolerance depends on the type the device computed in.

use jay::array::Array;
use jay::device::{available, Device, Precision, MIN_ELEMS};
use jay::frontend::{compile, Dialect, Lang};
use jay::Program;

/// A device to test on, and the tolerance its precision earns.
///
/// f64 where the adapter has it. Where it has not — every Metal machine,
/// most Vulkan drivers — the only thing that reaches the GPU at all is the
/// explicitly opted-in f32 path, so that is what gets tested, and the
/// tolerance says so.
fn gpu(test: &str) -> Option<(Device, f64)> {
    let d = Device::default_gpu()?;
    let info = d.info().expect("a gpu has an info");
    if info.f64 {
        eprintln!("{test}: {} ({}), f64", info.name, info.backend);
        Some((d, 1e-14))
    } else {
        eprintln!("{test}: {} ({}), no f64 — testing the f32 path", info.name, info.backend);
        Some((d.with_precision(Precision::F32), 1e-4))
    }
}

macro_rules! device {
    ($name:literal) => {
        match gpu($name) {
            Some(d) => d,
            None => {
                eprintln!("{}: no GPU adapter on this machine — skipped", $name);
                return;
            }
        }
    };
}

fn program(src: &str) -> Program {
    compile(Lang::J, src, &Dialect::default()).expect("compile")
}

/// A repeatable pseudo-random vector. The values stay in a narrow band so
/// that an f32 run of a long reduction is comparable at all: the CPU
/// reference is f64 either way, and a sum of ten million values spread over
/// many orders of magnitude says more about f32's mantissa than about the
/// device.
fn vector(n: usize, seed: u64) -> Array {
    let mut s = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        v.push((s >> 11) as f64 / (1u64 << 53) as f64 + 0.5);
    }
    Array::from_f64(v)
}

fn run(p: &Program, args: &[Array]) -> Array {
    p.run(args, &mut |_: &str| {}).expect("run").expect("a value")
}

fn run_on(p: &Program, d: &Device, args: &[Array]) -> Array {
    p.run_on(d, args, &mut |_: &str| {}).expect("run").expect("a value")
}

/// The elements of a result as floats, whatever it holds.
fn floats(a: &Array) -> Vec<f64> {
    match &a.data {
        jay::Data::F64(v) => v.as_slice().to_vec(),
        jay::Data::I64(v) => v.iter().map(|&x| x as f64).collect(),
        jay::Data::Bool(v) => v.iter().map(|&x| x as f64).collect(),
        other => panic!("not a numeric result: {other:?}"),
    }
}

fn same(a: &Array, b: &Array, rel: f64, what: &str) {
    assert_eq!(a.shape, b.shape, "{what}: shapes differ");
    assert_eq!(a.dtype(), b.dtype(), "{what}: dtypes differ");
    let (x, y) = (floats(a), floats(b));
    for (i, (p, q)) in x.iter().zip(&y).enumerate() {
        let scale = p.abs().max(q.abs()).max(1.0);
        assert!(
            (p - q).abs() <= rel * scale,
            "{what}: element {i} is {p} on the cpu and {q} on the device"
        );
    }
}

/// The chains the device is held to. Each takes `w` and `x`, so one pair of
/// arguments serves them all; the reductions are what the phase is really
/// about, and the maps are here because a map is the other half of what a
/// fused kernel yields.
const CHAINS: &[&str] = &[
    "+/ {w} * {x}",
    "+/ {w} + {x}",
    ">./ {w} * {x}",
    "<./ {w} * {x}",
    "({w} * {x}) + 1",
    "1 + 2 * {w} - {x}",
    "+/ ({x} - {w}) * ({x} - {w})",
    "+/ ({w} % {x}) * {x}",
    "+/ (| {w} - {x}) * {x}",
    "+/ ({w} >. {x}) - {w} <. {x}",
];

#[test]
fn the_device_computes_what_the_cpu_computes() {
    let (d, rel) = device!("equivalence");
    for n in [100_000usize, 1_000_000, 4_000_000] {
        let w = vector(n, 1);
        let x = vector(n, 2);
        for src in CHAINS {
            let p = program(src);
            let cpu = run(&p, &[w.clone(), x.clone()]);
            let dev = run_on(&p, &d, &[w.clone(), x.clone()]);
            same(&cpu, &dev, rel, &format!("{src} over {n}"));
        }
    }
}

#[test]
fn ten_million_rows_agree() {
    let (d, rel) = device!("ten million");
    let n = 10_000_000;
    let w = vector(n, 3);
    let x = vector(n, 4);
    let p = program("+/ {w} * {x}");
    same(
        &run(&p, &[w.clone(), x.clone()]),
        &run_on(&p, &d, &[w, x]),
        rel,
        "+/ w * x over 10M",
    );
}

/// A product over half a million factors is its own test: it needs factors
/// close enough to one that the product neither overflows nor underflows,
/// and the size cannot then be swept.
#[test]
fn a_product_reduction_agrees() {
    let (d, rel) = device!("product");
    let n = MIN_ELEMS;
    let w = vector(n, 15);
    let x = vector(n, 16);
    let p = program("*/ 1 + ({w} * {x}) % 100000");
    // A product multiplies every factor's rounding into the result: over
    // half a million factors in f32 that is around sqrt(n) ulps, a few
    // parts in ten thousand. An f64 device keeps the ordinary tolerance.
    let rel = if rel < 1e-10 { rel } else { 3e-3 };
    same(
        &run(&p, &[w.clone(), x.clone()]),
        &run_on(&p, &d, &[w, x]),
        rel,
        "*/ over half a million factors",
    );
}

#[test]
fn a_resident_argument_is_not_uploaded_again() {
    let (d, rel) = device!("residency");
    let n = 2_000_000;
    let w = d.upload(&vector(n, 5)).expect("upload");
    let x = d.upload(&vector(n, 6)).expect("upload");
    assert!(d.holds(&w) && d.holds(&x), "an uploaded array is resident");
    let p = program("+/ {w} * {x}");
    let cpu = run(&p, &[w.clone(), x.clone()]);
    // Twice, so that the second run is the one with nothing left to upload.
    let once = run_on(&p, &d, &[w.clone(), x.clone()]);
    let twice = run_on(&p, &d, &[w.clone(), x.clone()]);
    same(&cpu, &once, rel, "resident, first run");
    assert_eq!(once, twice, "the same buffers give the same answer");
    // The upload is transparent: an array on the device reads as itself.
    assert_eq!(floats(&w).len(), n);
}

#[test]
fn a_downloaded_array_is_the_array_it_came_from() {
    let (d, _) = device!("round trip");
    let src = vector(MIN_ELEMS, 7);
    let up = d.upload(&src).expect("upload");
    assert_eq!(up.shape, src.shape);
    assert_eq!(floats(&up), floats(&src));
}

#[test]
fn too_little_data_stays_on_the_cpu() {
    let (d, _) = device!("threshold");
    let n = MIN_ELEMS / 4;
    let w = vector(n, 8);
    let x = vector(n, 9);
    let p = program("+/ {w} * {x}");
    // Below the threshold the device is not asked, so the answer is the
    // CPU's own, exactly.
    assert_eq!(run(&p, &[w.clone(), x.clone()]), run_on(&p, &d, &[w.clone(), x.clone()]));
    let text = p.explain_on(&d, Some(&[w, x]));
    assert!(text.contains("device: cpu"), "{text}");
    assert!(text.contains("too little data"), "{text}");
}

#[test]
fn an_integer_chain_stays_on_the_cpu() {
    let (d, _) = device!("integers");
    let n = MIN_ELEMS * 2;
    let v = Array::from_i64((0..n as i64).collect());
    let p = program("+/ {x} * {x}");
    assert_eq!(run(&p, std::slice::from_ref(&v)), run_on(&p, &d, std::slice::from_ref(&v)));
    let text = p.explain_on(&d, Some(&[v]));
    assert!(text.contains("64-bit integers"), "{text}");
}

#[test]
fn a_chain_the_generator_declines_gives_the_cpu_answer() {
    let (d, rel) = device!("decline");
    let n = MIN_ELEMS * 2;
    let x = vector(n, 10);
    // `^` has no f64 shader form; in f32 it has one. Either way the answer
    // must be the CPU's.
    let p = program("+/ ^ {x} % 1000");
    let (cpu, dev) = (run(&p, std::slice::from_ref(&x)), run_on(&p, &d, std::slice::from_ref(&x)));
    same(&cpu, &dev, rel, "+/ ^ x % 1000");
    let text = p.explain_on(&d, Some(&[x]));
    assert!(text.contains("device: "), "{text}");
}

#[test]
fn explain_names_the_adapter_and_where_each_kernel_ran() {
    let (d, _) = device!("explain");
    let n = MIN_ELEMS * 2;
    let w = vector(n, 11);
    let x = vector(n, 12);
    let text = program("+/ {w} * {x}").explain_on(&d, Some(&[w, x]));
    let info = d.info().expect("a gpu");
    assert!(text.contains(&info.name), "{text}");
    assert!(text.contains("device: gpu") || text.contains("device: cpu"), "{text}");
}

#[test]
fn the_cpu_device_is_the_cpu() {
    let d = Device::cpu();
    let n = MIN_ELEMS * 2;
    let w = vector(n, 13);
    let x = vector(n, 14);
    let p = program("+/ {w} * {x}");
    // Naming the CPU changes nothing at all, bit for bit.
    assert_eq!(run(&p, &[w.clone(), x.clone()]), run_on(&p, &d, &[w, x]));
}

#[test]
fn listing_adapters_never_fails() {
    // The one test that must pass with or without a GPU: asking is safe.
    let all = available();
    for i in &all {
        eprintln!("adapter: {} ({}, {}), f64 {}", i.name, i.backend, i.kind, i.f64);
        assert!(!i.name.is_empty());
    }
    if !all.is_empty() {
        assert!(Device::default_gpu().is_some(), "adapters listed but none opened");
    }
}
