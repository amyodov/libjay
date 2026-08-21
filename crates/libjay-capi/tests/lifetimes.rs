//! Ownership across the C boundary: what a caller may free, and when.
//!
//! The ABI's promise is that a `jay_result` is self-contained — its
//! elements, its shape and the source its diagnostics quote all belong to
//! it — so a C caller can free the program and its own input buffers the
//! moment the call returns. These tests free everything the ABI does not
//! own, as early as the header allows, and then read the result.
//!
//! The second half feeds `jay_run` descriptors a C caller could plausibly
//! get wrong. Each one must come back as a reported error: the boundary
//! validates what it can and refuses the rest, rather than trusting a
//! number into a raw pointer read.

#[path = "../src/lib.rs"]
mod capi;

use std::ffi::{CStr, CString, c_int, c_void};
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

fn compile_ok(src: &str, lang: &str) -> *mut jay_program {
    let src = CString::new(src).unwrap();
    let lang = CString::new(lang).unwrap();
    let mut err: *mut jay_error = ptr::null_mut();
    let prog = unsafe { jay_compile(src.as_ptr(), lang.as_ptr(), -1, &mut err) };
    assert!(!prog.is_null(), "compile failed: {}", take_message(err));
    prog
}

fn value(dtype: i32, shape: &[u64], data: *const c_void) -> jay_value {
    jay_value { dtype, rank: shape.len() as i32, shape: shape.as_ptr(), data }
}

fn run(prog: *mut jay_program, args: &[jay_value]) -> Result<*mut jay_result, String> {
    let mut out: *mut jay_result = ptr::null_mut();
    let mut err: *mut jay_error = ptr::null_mut();
    let rc = unsafe {
        jay_run(prog, args.as_ptr(), args.len(), None, ptr::null_mut(), &mut out, &mut err)
    };
    if rc == 0 {
        assert!(err.is_null());
        Ok(out)
    } else {
        assert!(out.is_null(), "result handed out alongside a failure");
        Err(take_message(err))
    }
}

fn run_ok(prog: *mut jay_program, args: &[jay_value]) -> *mut jay_result {
    run(prog, args).unwrap_or_else(|e| panic!("run failed: {e}"))
}

fn run_err(prog: *mut jay_program, args: &[jay_value]) -> String {
    match run(prog, args) {
        Ok(r) => {
            unsafe { jay_result_free(r) };
            panic!("the descriptor was accepted");
        }
        Err(e) => e,
    }
}

fn elements_i64(r: *mut jay_result, n: usize) -> Vec<i64> {
    unsafe { std::slice::from_raw_parts(jay_result_data(r) as *const i64, n).to_vec() }
}

// --- ownership -------------------------------------------------------------

#[test]
fn a_result_owns_its_elements_after_the_caller_reuses_the_input() {
    let prog = compile_ok("2 * {v}", "j");
    let shape = [4u64];
    let mut input = vec![1i64, 2, 3, 4];
    let r = run_ok(prog, &[value(JAY_I64, &shape, input.as_ptr() as *const c_void)]);

    // The header says the descriptor is borrowed for the call only, so the
    // caller is free to scribble on its buffer the instant it returns.
    input.iter_mut().for_each(|x| *x = -1);
    input.clear();
    input.shrink_to_fit();
    drop(input);

    assert_eq!(elements_i64(r, 4), vec![2, 4, 6, 8]);
    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }
}

#[test]
fn a_result_outlives_the_program_that_produced_it() {
    let prog = compile_ok("3 4 $ i. 12", "j");
    let r = run_ok(prog, &[]);
    unsafe { jay_program_free(prog) };

    assert_eq!(unsafe { jay_result_rank(r) }, 2);
    let shape = unsafe { std::slice::from_raw_parts(jay_result_shape(r), 2) };
    assert_eq!(shape, [3, 4]);
    assert_eq!(elements_i64(r, 12), (0..12).collect::<Vec<i64>>());

    // Formatting reads the display options the program carried, which the
    // result copied rather than borrowed.
    let text = unsafe { jay_result_format(r) };
    assert!(!text.is_null());
    let text = unsafe { CStr::from_ptr(text) };
    assert!(text.to_string_lossy().contains("0 1  2  3"), "{text:?}");
    unsafe {
        jay_string_free(text.as_ptr() as *mut _);
        jay_result_free(r);
    }
}

#[test]
fn a_character_result_keeps_its_codepoints_for_its_whole_life() {
    let prog = compile_ok("'libjay'", "j");
    let r = run_ok(prog, &[]);
    unsafe { jay_program_free(prog) };
    let cps = unsafe { std::slice::from_raw_parts(jay_result_data(r) as *const u32, 6) };
    assert_eq!(cps, [b'l' as u32, b'i' as u32, b'b' as u32, b'j' as u32, b'a' as u32, b'y' as u32]);
    // Asking twice hands back the same cached block, not a fresh one.
    assert_eq!(unsafe { jay_result_data(r) }, cps.as_ptr() as *const c_void);
    unsafe { jay_result_free(r) };
}

#[test]
fn an_error_quotes_a_source_the_caller_has_already_freed() {
    let err = {
        let src = CString::new("(1 + 2").unwrap();
        let lang = CString::new("j").unwrap();
        let mut err: *mut jay_error = ptr::null_mut();
        let prog = unsafe { jay_compile(src.as_ptr(), lang.as_ptr(), -1, &mut err) };
        assert!(prog.is_null());
        drop(src);
        drop(lang);
        err
    };
    let msg = take_message(err);
    assert!(msg.contains("(1 + 2"), "the error lost its source line: {msg}");
    assert!(msg.contains('^'), "no caret line in {msg:?}");
}

#[test]
fn a_runtime_error_outlives_the_program_it_came_from() {
    let prog = compile_ok("1 2 3 + 1 2", "j");
    let mut out: *mut jay_result = ptr::null_mut();
    let mut err: *mut jay_error = ptr::null_mut();
    let rc =
        unsafe { jay_run(prog, ptr::null(), 0, None, ptr::null_mut(), &mut out, &mut err) };
    assert_ne!(rc, 0);
    unsafe { jay_program_free(prog) };
    let msg = take_message(err);
    assert!(msg.contains("1 2 3 + 1 2"), "{msg}");
}

#[test]
fn parameter_names_stay_put_for_the_life_of_the_program() {
    let prog = compile_ok("{alpha} + {omega}", "j");
    let first = unsafe { jay_program_param_name(prog, 0) };
    let mut prior = Vec::new();
    for _ in 0..4 {
        prior.push(unsafe { jay_program_param_name(prog, 0) });
    }
    assert!(prior.iter().all(|&p| p == first), "a name moved between calls");
    assert_eq!(unsafe { CStr::from_ptr(first) }.to_str().unwrap(), "alpha");
    unsafe { jay_program_free(prog) };
}

#[test]
fn many_results_from_one_program_are_independent() {
    let prog = compile_ok("{v} + 1", "j");
    let shape = [3u64];
    let a = [1i64, 2, 3];
    let b = [10i64, 20, 30];
    let ra = run_ok(prog, &[value(JAY_I64, &shape, a.as_ptr() as *const c_void)]);
    let rb = run_ok(prog, &[value(JAY_I64, &shape, b.as_ptr() as *const c_void)]);
    // Freeing one must not disturb the other.
    unsafe { jay_result_free(ra) };
    assert_eq!(elements_i64(rb, 3), vec![11, 21, 31]);
    unsafe {
        jay_result_free(rb);
        jay_program_free(prog);
    }
}

// --- descriptors a caller could get wrong ----------------------------------

#[test]
fn a_negative_rank_is_refused_rather_than_read() {
    let prog = compile_ok("+/{v}", "j");
    let shape = [3u64];
    let data = [1i64, 2, 3];
    let mut d = value(JAY_I64, &shape, data.as_ptr() as *const c_void);
    d.rank = -1;
    let msg = run_err(prog, &[d]);
    assert!(msg.contains("rank is negative"), "{msg}");
    unsafe { jay_program_free(prog) };
}

#[test]
fn a_null_shape_at_nonzero_rank_is_refused() {
    let prog = compile_ok("+/{v}", "j");
    let data = [1i64, 2, 3];
    let d = jay_value {
        dtype: JAY_I64,
        rank: 1,
        shape: ptr::null(),
        data: data.as_ptr() as *const c_void,
    };
    let msg = run_err(prog, &[d]);
    assert!(msg.contains("shape is NULL"), "{msg}");
    unsafe { jay_program_free(prog) };
}

#[test]
fn an_element_count_that_overflows_is_refused_before_any_read() {
    let prog = compile_ok("+/{v}", "j");
    let huge = [u64::MAX / 2, 4];
    let data = [1i64];
    let msg = run_err(prog, &[value(JAY_I64, &huge, data.as_ptr() as *const c_void)]);
    assert!(msg.contains("overflows"), "{msg}");

    // An axis that does not fit a usize at all is caught first.
    if usize::BITS < 64 {
        let wide = [u64::MAX];
        let msg = run_err(prog, &[value(JAY_I64, &wide, data.as_ptr() as *const c_void)]);
        assert!(msg.contains("does not fit"), "{msg}");
    }
    unsafe { jay_program_free(prog) };
}

#[test]
fn a_codepoint_that_is_not_a_character_is_refused() {
    let prog = compile_ok("#{s}", "j");
    let shape = [2u64];
    // 0xD800 is a lone surrogate: a valid u32, never a `char`.
    let bogus = [b'a' as u32, 0xD800];
    let msg = run_err(prog, &[value(JAY_CHAR, &shape, bogus.as_ptr() as *const c_void)]);
    assert!(msg.contains("not a Unicode codepoint"), "{msg}");
    assert!(msg.contains("element 1"), "the offending element is not named: {msg}");
    unsafe { jay_program_free(prog) };
}

#[test]
fn an_empty_array_may_pass_a_null_data_pointer() {
    let prog = compile_ok("#{v}", "j");
    let shape = [0u64];
    let r = run_ok(prog, &[value(JAY_I64, &shape, ptr::null())]);
    assert_eq!(elements_i64(r, 1), vec![0]);
    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }
}

#[test]
fn a_scalar_needs_its_one_element() {
    let prog = compile_ok("{v} + 1", "j");
    let msg = run_err(prog, &[value(JAY_I64, &[], ptr::null())]);
    assert!(msg.contains("data is NULL"), "a rank-0 descriptor still has one element: {msg}");
    let one = [41i64];
    let r = run_ok(prog, &[value(JAY_I64, &[], one.as_ptr() as *const c_void)]);
    assert_eq!(unsafe { jay_result_rank(r) }, 0);
    assert_eq!(elements_i64(r, 1), vec![42]);
    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }
}

#[test]
fn a_result_the_abi_cannot_describe_is_an_error_not_a_null_pointer() {
    for (src, want) in [("<1 2 3", "boxed"), ("3x + 4", "extended-precision"), ("1r3", "rational")]
    {
        let prog = compile_ok(src, "j");
        let msg = run_err(prog, &[]);
        assert!(msg.contains(want), "{src}: {msg}");
        unsafe { jay_program_free(prog) };
    }
}

#[test]
fn args_may_be_null_only_when_there_are_none() {
    let prog = compile_ok("+/{v}", "j");
    let mut out: *mut jay_result = ptr::null_mut();
    let mut err: *mut jay_error = ptr::null_mut();
    let rc: c_int =
        unsafe { jay_run(prog, ptr::null(), 3, None, ptr::null_mut(), &mut out, &mut err) };
    assert_ne!(rc, 0);
    assert!(out.is_null());
    assert!(take_message(err).contains("args is NULL"));
    unsafe { jay_program_free(prog) };
}
