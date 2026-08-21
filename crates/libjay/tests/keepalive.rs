//! Who keeps what alive.
//!
//! Every zero-copy boundary libjay has is the same shape: something hands
//! over a pointer and a handle that keeps the memory behind it valid, and
//! the two must not be able to come apart. These tests take each of those
//! boundaries and drop the side that looks like the owner — the source
//! string, the producing array, the device, the thread — while the borrow
//! is still live, then read through the borrow.
//!
//! They assert the release too: keeping memory alive forever is the other
//! way to get this wrong, and a leak that nothing notices is a leak that
//! grows.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use jay::array::{Array, Buf, Data, Owner};
use jay::device::Device;
use jay::frontend::{compile, Dialect, Lang};

/// Owns a vector and records its own drop, standing in for whatever a
/// foreign producer — pyarrow, numpy, a C caller — keeps its memory in.
struct Producer {
    values: Vec<f64>,
    dropped: Arc<AtomicBool>,
}

impl Drop for Producer {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

/// An array borrowing memory that `Producer` owns, plus the flag that says
/// whether the producer is still alive.
fn borrowed(values: Vec<f64>) -> (Array, Arc<AtomicBool>) {
    let dropped = Arc::new(AtomicBool::new(false));
    let len = values.len();
    let producer = Arc::new(Producer { values, dropped: dropped.clone() });
    let ptr = producer.values.as_ptr();
    // SAFETY: the producer owns the vector and is moved into the buffer's
    // owner slot, so the elements stay put and unmutated for as long as the
    // buffer (or anything cloned from it) lives.
    let owner: Owner = producer;
    let buf = unsafe { Buf::foreign(ptr, len, owner) };
    (Array::new(vec![len], Data::F64(buf)), dropped)
}

fn run(src: &str, args: &[Array]) -> Array {
    let prog = compile(Lang::J, src, &Dialect::default()).expect("compile");
    prog.run(args, &mut |_| {}).expect("run").expect("a value")
}

// --- the source string -----------------------------------------------------

#[test]
fn a_program_outlives_the_source_it_was_compiled_from() {
    let prog = {
        let src = String::from("(+/ % #) {x}");
        let prog = compile(Lang::J, &src, &Dialect::default()).expect("compile");
        drop(src);
        prog
    };
    let y = Array::from_f64(vec![1.0, 2.0, 3.0, 4.0]);
    let out = prog.run(&[y], &mut |_| {}).expect("run").expect("a value");
    assert_eq!(out.as_f64_slice(), Some(&[2.5][..]));
}

#[test]
fn a_diagnostic_still_quotes_a_source_that_is_gone() {
    let src = String::from("1 2 3 + 4 5");
    let prog = compile(Lang::J, &src, &Dialect::default()).expect("compile");
    drop(src);
    let e = prog.run(&[], &mut |_| {}).expect_err("lengths disagree");
    let rendered = prog.render_error(&e);
    assert!(rendered.contains("1 2 3 + 4 5"), "the caret line lost its source: {rendered}");
}

#[test]
fn a_compile_error_holds_its_own_copy_of_the_source() {
    let (e, src) = {
        let src = String::from("1 + + )");
        (compile(Lang::J, &src, &Dialect::default()).expect_err("a syntax error"), src)
    };
    let rendered = e.render(&src);
    drop(src);
    assert!(!rendered.is_empty());
}

// --- foreign buffers -------------------------------------------------------

#[test]
fn a_borrowed_array_keeps_its_producer_alive_and_lets_it_go() {
    let (y, dropped) = borrowed(vec![1.0, 2.0, 3.0]);
    assert!(y.data.is_foreign());
    assert!(!dropped.load(Ordering::SeqCst));
    assert_eq!(y.as_f64_slice(), Some(&[1.0, 2.0, 3.0][..]));
    drop(y);
    assert!(dropped.load(Ordering::SeqCst), "the producer was never released");
}

#[test]
fn a_run_over_borrowed_data_does_not_outlive_it() {
    let (y, dropped) = borrowed(vec![1.0, 2.0, 3.0, 4.0]);
    let out = run("(+/ % #) {x}", &[y]);
    assert_eq!(out.as_f64_slice(), Some(&[2.5][..]));
    // The result is computed, not borrowed, so the producer is released with
    // the argument the run consumed.
    assert!(dropped.load(Ordering::SeqCst));
}

#[test]
fn an_identity_result_keeps_borrowing_after_its_argument_is_gone() {
    let (y, dropped) = borrowed(vec![5.0, 6.0, 7.0]);
    let prog = compile(Lang::J, "{x}", &Dialect::default()).expect("compile");
    let out = prog.run(&[y], &mut |_| {}).expect("run").expect("a value");
    drop(prog);
    assert!(out.data.is_foreign(), "the identity copied its argument");
    assert!(!dropped.load(Ordering::SeqCst), "the producer went with the argument");
    assert_eq!(out.as_f64_slice(), Some(&[5.0, 6.0, 7.0][..]));
    drop(out);
    assert!(dropped.load(Ordering::SeqCst));
}

#[test]
fn a_cell_of_a_borrowed_array_outlives_the_array() {
    let (y, dropped) = borrowed(vec![1.0, 2.0, 3.0, 4.0]);
    let y = Array::new(vec![2, 2], y.data);
    let row = y.item(1);
    drop(y);
    assert!(row.data.is_foreign());
    assert!(!dropped.load(Ordering::SeqCst), "the row let its memory go");
    assert_eq!(row.as_f64_slice(), Some(&[3.0, 4.0][..]));
    drop(row);
    assert!(dropped.load(Ordering::SeqCst));
}

#[test]
fn a_borrowed_array_reads_on_another_thread_after_this_one_lets_go() {
    let (y, dropped) = borrowed((0..1024).map(|i| i as f64).collect());
    let handle = std::thread::spawn(move || {
        let out = run("+/ {x}", &[y]);
        out.as_f64_slice().expect("floats")[0]
    });
    let total = handle.join().expect("the worker finished");
    assert_eq!(total, (0..1024).map(|i| i as f64).sum::<f64>());
    assert!(dropped.load(Ordering::SeqCst), "the producer outlived the worker");
}

#[test]
fn writing_through_one_holder_never_touches_a_borrowed_block() {
    let (y, _dropped) = borrowed(vec![1.0, 2.0, 3.0]);
    let mut copy = match &y.data {
        Data::F64(b) => b.clone(),
        _ => unreachable!("built as f64"),
    };
    copy.to_mut()[0] = 99.0;
    assert!(!copy.is_foreign(), "the write stayed in foreign memory");
    assert_eq!(y.as_f64_slice(), Some(&[1.0, 2.0, 3.0][..]));
    assert_eq!(&copy[..], &[99.0, 2.0, 3.0]);
}

// --- exporting -------------------------------------------------------------

/// What `libjay-python`'s Arrow export does, with the consumer played by a
/// raw pointer: take the buffer's address, hand a clone of the buffer to the
/// consumer as the thing that keeps the allocation alive, then drop the
/// value the elements came from and read them anyway.
#[test]
fn an_exported_buffer_survives_the_value_it_came_from() {
    let y = Array::from_f64(vec![1.5, 2.5, 3.5]);
    let exported = match &y.data {
        Data::F64(b) => b.clone(),
        _ => unreachable!("built as f64"),
    };
    let (ptr, len) = (exported.as_ptr(), exported.len());
    drop(y);
    // SAFETY: `exported` holds the same refcounted allocation the pointer
    // addresses, and nothing can write to it while it is shared.
    let seen = unsafe { std::slice::from_raw_parts(ptr, len) };
    assert_eq!(seen, &[1.5, 2.5, 3.5]);
    assert_eq!(exported.as_ptr(), ptr, "the export was moved out from under itself");
}

#[test]
fn an_export_of_borrowed_data_holds_the_original_producer() {
    let (y, dropped) = borrowed(vec![4.0, 5.0]);
    let exported = match &y.data {
        Data::F64(b) => b.clone(),
        _ => unreachable!("built as f64"),
    };
    drop(y);
    assert!(!dropped.load(Ordering::SeqCst), "the export lost the memory it points at");
    assert_eq!(&exported[..], &[4.0, 5.0]);
    drop(exported);
    assert!(dropped.load(Ordering::SeqCst));
}

// --- devices ---------------------------------------------------------------

/// Every device test passes on a machine with no adapter: what it checks is
/// covered nowhere else, but there is nothing to check without a GPU.
macro_rules! gpu {
    ($name:literal) => {
        match Device::default_gpu() {
            Some(d) => d,
            None => {
                eprintln!("{}: no GPU adapter on this machine — skipped", $name);
                return;
            }
        }
    };
}

#[test]
fn an_uploaded_array_outlives_the_device_that_made_it() {
    let device = gpu!("an_uploaded_array_outlives_the_device_that_made_it");
    let y = Array::from_f64((0..4096).map(|i| i as f64).collect());
    let resident = device.upload(&y).expect("upload");
    assert!(resident.data.is_foreign(), "an upload leaves an ordinary owned buffer");
    drop(device);
    // The host mirror is inside the array's own owner handle, so it reads
    // with no device involved at all.
    assert_eq!(resident.count(), 4096);
    assert_eq!(resident.as_f64_slice().expect("floats")[4095], 4095.0);
}

#[test]
fn an_uploaded_array_outlives_the_program_that_ran_on_it() {
    let device = gpu!("an_uploaded_array_outlives_the_program_that_ran_on_it");
    let y = Array::from_f64((0..4096).map(|i| i as f64 * 0.5).collect());
    let resident = device.upload(&y).expect("upload");
    {
        let prog = compile(Lang::J, "+/ {x} * {x}", &Dialect::default()).expect("compile");
        let out = prog
            .run_on(&device, std::slice::from_ref(&resident), &mut |_| {})
            .expect("run")
            .expect("a value");
        assert!(out.count() == 1);
    }
    assert!(device.holds(&resident), "the run took the array's residency with it");
    assert_eq!(resident.as_f64_slice().expect("floats")[10], 5.0);
}

#[test]
fn residency_survives_the_device_handle_being_reopened() {
    let device = gpu!("residency_survives_the_device_handle_being_reopened");
    let y = Array::from_f64((0..4096).map(|i| i as f64).collect());
    let resident = device.upload(&y).expect("upload");
    drop(device);
    let again = Device::default_gpu().expect("the adapter is still there");
    assert!(again.holds(&resident), "reopening the adapter lost the upload");
}

#[test]
fn a_slice_of_an_uploaded_array_is_not_mistaken_for_the_upload() {
    let device = gpu!("a_slice_of_an_uploaded_array_is_not_mistaken_for_the_upload");
    let y = Array::from_f64((0..4096).map(|i| i as f64).collect());
    let resident = device.upload(&y).expect("upload");
    let half = Array::new(vec![2, 2048], resident.data.clone()).item(0);
    assert!(half.data.is_foreign(), "the slice copied");
    assert!(!device.holds(&half), "half an array claimed the whole allocation");
    assert_eq!(half.as_f64_slice().expect("floats")[0], 0.0);
}
