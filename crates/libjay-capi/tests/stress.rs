//! Usage stress across the C ABI: long call sequences through the public
//! surface, and the ownership rules kept on every one of them.
//!
//! A C caller does not get destructors. Every handle this file takes it also
//! frees — programs, results, errors, strings — and the point of the loops is
//! that a thousand rounds of compile, run, read, free cost no more resident
//! memory than the first, and that a refused compile in the middle of the
//! sequence leaves the next one answering what it answered before.
//!
//! Resident memory is compared as a RATIO against this process's own
//! baseline, taken after a warm-up: a megabyte figure is a property of the
//! machine, a ratio is a property of the code.

#[path = "../src/lib.rs"]
mod capi;

use std::ffi::{CStr, CString, c_char, c_void};
use std::process::Command;
use std::ptr;

use capi::*;

// --- helpers ---------------------------------------------------------------

fn take_message(err: *mut jay_error) -> String {
    assert!(!err.is_null(), "expected an error");
    unsafe {
        let msg = jay_error_message(err);
        assert!(!msg.is_null());
        let s = CStr::from_ptr(msg).to_string_lossy().into_owned();
        jay_string_free(msg);
        jay_error_free(err);
        s
    }
}

fn compile(src: &str, lang: &str) -> Result<*mut jay_program, String> {
    let src = CString::new(src).unwrap();
    let lang = CString::new(lang).unwrap();
    let mut err: *mut jay_error = ptr::null_mut();
    let prog = unsafe { jay_compile(src.as_ptr(), lang.as_ptr(), -1, &mut err) };
    if prog.is_null() { Err(take_message(err)) } else { Ok(prog) }
}

fn compile_ok(src: &str, lang: &str) -> *mut jay_program {
    compile(src, lang).unwrap_or_else(|e| panic!("compiling {src:?} failed: {e}"))
}

fn run(prog: *mut jay_program, args: &[jay_value]) -> Result<*mut jay_result, String> {
    let mut out: *mut jay_result = ptr::null_mut();
    let mut err: *mut jay_error = ptr::null_mut();
    let rc = unsafe {
        jay_run(prog, args.as_ptr(), args.len(), None, ptr::null_mut(), &mut out, &mut err)
    };
    if rc == 0 { Ok(out) } else { Err(take_message(err)) }
}

fn value(dtype: i32, shape: &[u64], data: *const c_void) -> jay_value {
    jay_value { dtype, rank: shape.len() as i32, shape: shape.as_ptr(), data }
}

/// A result read out as text and freed, which is the whole of what a caller
/// does with one.
fn take_text(r: *mut jay_result) -> String {
    unsafe {
        let s = jay_result_format(r);
        assert!(!s.is_null());
        let text = CStr::from_ptr(s).to_string_lossy().into_owned();
        jay_string_free(s);
        jay_result_free(r);
        text
    }
}

/// Reads the i64 elements out of a result, then frees it.
fn take_i64(r: *mut jay_result) -> Vec<i64> {
    unsafe {
        assert_eq!(jay_result_dtype(r), JAY_I64);
        let rank = jay_result_rank(r);
        assert!(rank >= 0);
        let shape = std::slice::from_raw_parts(jay_result_shape(r), rank as usize);
        let n: usize = shape.iter().map(|&d| d as usize).product();
        let v = std::slice::from_raw_parts(jay_result_data(r) as *const i64, n).to_vec();
        jay_result_free(r);
        v
    }
}

fn rss_kib() -> Option<u64> {
    let pid = std::process::id().to_string();
    let out = Command::new("ps").args(["-o", "rss=", "-p", &pid]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Appends every chunk it is handed to the `String` behind `userdata`.
unsafe extern "C" fn collect(text: *const c_char, len: usize, userdata: *mut c_void) {
    let bytes = unsafe { std::slice::from_raw_parts(text as *const u8, len) };
    let out = unsafe { &mut *(userdata as *mut String) };
    out.push_str(&String::from_utf8_lossy(bytes));
}

// --- the cycle -------------------------------------------------------------

/// One full pass over the surface: compile with a parameter, run it against
/// borrowed data, read the answer two ways, free everything, and be refused
/// twice on the way. The data is borrowed, never copied in by the caller, so
/// a round that retained it would show up as growth.
fn cycle(data: &[i64]) -> (Vec<i64>, String) {
    let shape = [data.len() as u64];

    let prog = compile_ok("s =. {v}\n(+/ s) , (>./ s) , (# s)", "j");
    assert_eq!(unsafe { jay_program_param_count(prog) }, 1);
    let name = unsafe { CStr::from_ptr(jay_program_param_name(prog, 0)) };
    assert_eq!(name.to_str().unwrap(), "v");
    let arg = value(JAY_I64, &shape, data.as_ptr() as *const c_void);
    let nums = take_i64(run(prog, &[arg]).expect("the J program should answer"));
    unsafe { jay_program_free(prog) };

    let apl = compile_ok("S←{v}\n(+/S),(⌈/S),⍴S", "apl");
    let arg = value(JAY_I64, &shape, data.as_ptr() as *const c_void);
    let text = take_text(run(apl, &[arg]).expect("the APL program should answer"));
    unsafe { jay_program_free(apl) };

    // A compile that cannot succeed, and a run that cannot: both must hand
    // back an error the caller frees, and neither may hand back a handle.
    let msg = compile("1 + ", "j").expect_err("an unfinished sentence");
    assert!(!msg.is_empty());
    let bad = compile_ok("1 2 3 + 4 5", "j");
    let msg = run(bad, &[]).expect_err("lengths 3 and 2 do not agree");
    assert!(!msg.is_empty());
    unsafe { jay_program_free(bad) };

    (nums, text)
}

// --- the cases -------------------------------------------------------------

#[test]
fn a_long_call_sequence_holds_its_memory() {
    let data: Vec<i64> = (0..5_000).collect();
    let expected = cycle(&data);
    for _ in 0..20 {
        assert_eq!(cycle(&data), expected, "an answer moved during the warm-up");
    }
    let before = rss_kib();
    for _ in 0..500 {
        assert_eq!(cycle(&data), expected, "an answer moved under repetition");
    }
    let after = rss_kib();
    let (Some(before), Some(after)) = (before, after) else {
        println!("resident memory could not be read here; the answers were still checked");
        return;
    };
    let ceiling = (before as f64 * 1.5) + 16_384.0;
    assert!(
        (after as f64) <= ceiling,
        "resident memory grew from {before} KiB to {after} KiB over 500 call sequences \
         (ceiling {ceiling:.0} KiB)"
    );
}

#[test]
fn one_program_serves_many_runs() {
    // Compiling is the expensive half of the surface, so the shape a caller
    // is meant to use is one program and many runs. Each run gets its own
    // data and its own result, and freeing a result must not touch the
    // program that produced it.
    let prog = compile_ok("s =. {v}\n(+/ s) , (# s)", "j");
    for n in 1..=200u64 {
        let data: Vec<i64> = (0..n as i64).collect();
        let shape = [n];
        let arg = value(JAY_I64, &shape, data.as_ptr() as *const c_void);
        let got = take_i64(run(prog, &[arg]).expect("a run"));
        assert_eq!(got, vec![(0..n as i64).sum::<i64>(), n as i64]);
    }
    unsafe { jay_program_free(prog) };
}

#[test]
fn refusals_in_a_loop_leave_the_surface_working() {
    let good = compile_ok("+/ i. 100", "j");
    let want = take_i64(run(good, &[]).expect("a run"));
    assert_eq!(want, vec![4950]);
    for _ in 0..500 {
        // Every refusal the surface can produce, each freed the way a C
        // caller must free it.
        assert!(!compile("1 + ", "j").unwrap_err().is_empty());
        assert!(!compile("+/ (", "j").unwrap_err().is_empty());
        assert!(!compile("2 + 2", "klingon").unwrap_err().is_empty());
        let bad = compile_ok("1 2 3 + 4 5", "j");
        assert!(!run(bad, &[]).unwrap_err().is_empty());
        unsafe { jay_program_free(bad) };
        let wants_data = compile_ok("+/ {v}", "j");
        assert!(!run(wants_data, &[]).unwrap_err().is_empty(), "no data was given");
        unsafe { jay_program_free(wants_data) };

        assert_eq!(take_i64(run(good, &[]).expect("a run")), want);
    }
    unsafe { jay_program_free(good) };
}

#[test]
fn the_write_callback_survives_a_long_sequence() {
    // A program that prints as well as answers, run many times into the same
    // sink: what the callback is handed must be the same text every round.
    let prog = compile_ok("t =. i. 5\necho 'ok'\n+/ t", "j");
    let mut first: Option<String> = None;
    for _ in 0..500 {
        let mut printed = String::new();
        let mut out: *mut jay_result = ptr::null_mut();
        let mut err: *mut jay_error = ptr::null_mut();
        let rc = unsafe {
            jay_run(
                prog,
                ptr::null(),
                0,
                Some(collect),
                &mut printed as *mut String as *mut c_void,
                &mut out,
                &mut err,
            )
        };
        assert_eq!(rc, 0, "{}", take_message(err));
        assert_eq!(take_i64(out), vec![10]);
        match &first {
            None => first = Some(printed),
            Some(want) => assert_eq!(&printed, want),
        }
    }
    assert_eq!(first.as_deref(), Some("ok\n"));
    unsafe { jay_program_free(prog) };
}
