//! Exercises the C entry points from Rust: same functions, same ABI, same
//! ownership rules a C caller has to follow.
//!
//! The crate is built as cdylib + staticlib only (an rlib would collide with
//! the core crate's libjay.rlib), so there is nothing to `use` — the module
//! is included by path and compiled into this test binary.

#[path = "../src/lib.rs"]
mod capi;

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;

use capi::*;

// --- helpers ---------------------------------------------------------------

/// Take an error's rendered message and free the error.
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

fn compile(src: &str, lang: &str, index_origin: i32) -> Result<*mut jay_program, String> {
    let src = CString::new(src).unwrap();
    let lang = CString::new(lang).unwrap();
    let mut err: *mut jay_error = ptr::null_mut();
    let prog = unsafe { jay_compile(src.as_ptr(), lang.as_ptr(), index_origin, &mut err) };
    if prog.is_null() {
        Err(take_message(err))
    } else {
        assert!(err.is_null(), "error set alongside a successful compile");
        Ok(prog)
    }
}

fn compile_ok(src: &str, lang: &str) -> *mut jay_program {
    compile(src, lang, -1).unwrap_or_else(|e| panic!("compiling {src:?} failed: {e}"))
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
        assert!(!out.is_null());
        Ok(out)
    } else {
        assert!(out.is_null(), "result handed out alongside a failure");
        Err(take_message(err))
    }
}

fn run_ok(prog: *mut jay_program, args: &[jay_value]) -> *mut jay_result {
    run(prog, args).unwrap_or_else(|e| panic!("run failed: {e}"))
}

fn result_f64(r: *mut jay_result) -> Vec<f64> {
    unsafe {
        assert_eq!(jay_result_dtype(r), JAY_F64);
        let n: usize = shape_of(r).iter().map(|&d| d as usize).product();
        std::slice::from_raw_parts(jay_result_data(r) as *const f64, n).to_vec()
    }
}

fn result_i64(r: *mut jay_result) -> Vec<i64> {
    unsafe {
        assert_eq!(jay_result_dtype(r), JAY_I64);
        let n: usize = shape_of(r).iter().map(|&d| d as usize).product();
        std::slice::from_raw_parts(jay_result_data(r) as *const i64, n).to_vec()
    }
}

/// A complex result's elements: two doubles each, real then imaginary.
fn result_complex(r: *mut jay_result) -> Vec<[f64; 2]> {
    unsafe {
        assert_eq!(jay_result_dtype(r), JAY_COMPLEX);
        let n: usize = shape_of(r).iter().map(|&d| d as usize).product();
        std::slice::from_raw_parts(jay_result_data(r) as *const [f64; 2], n).to_vec()
    }
}

fn shape_of(r: *mut jay_result) -> Vec<u64> {
    unsafe {
        let rank = jay_result_rank(r);
        assert!(rank >= 0);
        let p = jay_result_shape(r);
        assert!(!p.is_null());
        std::slice::from_raw_parts(p, rank as usize).to_vec()
    }
}

fn formatted(r: *mut jay_result) -> String {
    unsafe {
        let s = jay_result_format(r);
        assert!(!s.is_null());
        let text = CStr::from_ptr(s).to_string_lossy().into_owned();
        jay_string_free(s);
        text
    }
}

/// Appends every chunk it is handed to the `String` behind `userdata`.
unsafe extern "C" fn collect(text: *const c_char, len: usize, userdata: *mut c_void) {
    let bytes = unsafe { std::slice::from_raw_parts(text as *const u8, len) };
    let out = unsafe { &mut *(userdata as *mut String) };
    out.push_str(&String::from_utf8_lossy(bytes));
}

// --- tests -----------------------------------------------------------------

#[test]
fn j_mean_fork_over_a_float_vector() {
    let prog = compile_ok("(+/ % #) {x}", "j");
    assert_eq!(unsafe { jay_program_param_count(prog) }, 1);

    let data = [1.0f64, 2.0, 3.0, 4.0];
    let shape = [data.len() as u64];
    let arg = value(JAY_F64, &shape, data.as_ptr() as *const c_void);
    let r = run_ok(prog, &[arg]);

    assert_eq!(unsafe { jay_result_is_empty(r) }, 0);
    assert_eq!(unsafe { jay_result_rank(r) }, 0, "the mean is a scalar");
    assert_eq!(result_f64(r), vec![2.5]);
    assert_eq!(formatted(r), "2.5");

    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }
}

#[test]
fn parameters_are_named_and_positional() {
    let prog = compile_ok("{a} - {b}", "j");
    assert_eq!(unsafe { jay_program_param_count(prog) }, 2);
    let name = |i| unsafe { CStr::from_ptr(jay_program_param_name(prog, i)) }.to_str().unwrap();
    assert_eq!(name(0), "a");
    assert_eq!(name(1), "b");
    assert!(unsafe { jay_program_param_name(prog, 2) }.is_null(), "out of range must be NULL");

    let (a, b) = ([10i64, 20, 30], [1i64, 2, 3]);
    let shape = [3u64];
    let args = [
        value(JAY_I64, &shape, a.as_ptr() as *const c_void),
        value(JAY_I64, &shape, b.as_ptr() as *const c_void),
    ];
    let r = run_ok(prog, &args);
    assert_eq!(result_i64(r), vec![9, 18, 27]);
    assert_eq!(shape_of(r), vec![3]);

    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }
}

#[test]
fn wrong_argument_count_is_reported_not_ignored() {
    let prog = compile_ok("{a} - {b}", "j");
    let msg = run(prog, &[]).unwrap_err();
    assert!(msg.contains("expected 2 argument(s), got 0"), "{msg}");
    unsafe { jay_program_free(prog) };
}

#[test]
fn apl_reduces_and_honours_the_index_origin() {
    let prog = compile_ok("+/{v}", "apl");
    let data = [1i64, 2, 3, 4];
    let shape = [4u64];
    let r = run_ok(prog, &[value(JAY_I64, &shape, data.as_ptr() as *const c_void)]);
    assert_eq!(result_i64(r), vec![10]);
    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }

    // -1 asks for the language default, which is APL's ⎕IO of 1.
    let default = compile("⍳4", "apl", -1).unwrap();
    let r = run_ok(default, &[]);
    assert_eq!(result_i64(r), vec![1, 2, 3, 4]);
    unsafe {
        jay_result_free(r);
        jay_program_free(default);
    }

    let origin_zero = compile("⍳4", "apl", 0).unwrap();
    let r = run_ok(origin_zero, &[]);
    assert_eq!(result_i64(r), vec![0, 1, 2, 3]);
    unsafe {
        jay_result_free(r);
        jay_program_free(origin_zero);
    }
}

#[test]
fn boolean_and_character_data_cross_both_ways() {
    let prog = compile_ok("#{s}", "j");
    let text: Vec<u32> = "hello".chars().map(|c| c as u32).collect();
    let shape = [text.len() as u64];
    let r = run_ok(prog, &[value(JAY_CHAR, &shape, text.as_ptr() as *const c_void)]);
    assert_eq!(result_i64(r), vec![5]);
    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }

    // Characters come back as UTF-32 codepoints.
    let prog = compile_ok("'jay'", "j");
    let r = run_ok(prog, &[]);
    assert_eq!(unsafe { jay_result_dtype(r) }, JAY_CHAR);
    let cps = unsafe { std::slice::from_raw_parts(jay_result_data(r) as *const u32, 3) };
    assert_eq!(cps, [b'j' as u32, b'a' as u32, b'y' as u32]);
    assert_eq!(formatted(r), "jay");
    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }

    // Booleans are one byte per element, 0 or 1.
    let prog = compile_ok("{a} > 2", "j");
    let data = [1u8, 0, 1];
    let shape = [3u64];
    let r = run_ok(prog, &[value(JAY_BOOL, &shape, data.as_ptr() as *const c_void)]);
    assert_eq!(unsafe { jay_result_dtype(r) }, JAY_BOOL);
    let bits = unsafe { std::slice::from_raw_parts(jay_result_data(r) as *const u8, 3) };
    assert_eq!(bits, [0, 0, 0]);
    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }
}

#[test]
fn rank_two_results_carry_their_shape() {
    let prog = compile_ok("2 3 $ i. 6", "j");
    let r = run_ok(prog, &[]);
    assert_eq!(unsafe { jay_result_rank(r) }, 2);
    assert_eq!(shape_of(r), vec![2, 3]);
    assert_eq!(result_i64(r), vec![0, 1, 2, 3, 4, 5]);
    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }
}

#[test]
fn a_compile_error_points_at_the_source() {
    let msg = compile("(1 + 2", "j", -1).unwrap_err();
    assert!(msg.contains('^'), "no caret line in {msg:?}");
    assert!(msg.contains("(1 + 2"), "no source line in {msg:?}");
}

#[test]
fn a_runtime_error_points_at_the_source() {
    let prog = compile_ok("1 2 3 + 1 2", "j");
    let msg = run(prog, &[]).unwrap_err();
    assert!(msg.contains('^'), "no caret line in {msg:?}");
    assert!(msg.contains("length error"), "{msg}");
    unsafe { jay_program_free(prog) };
}

#[test]
fn a_symbol_result_is_refused_by_name() {
    // A symbol is an index into a table this ABI cannot hand over; the
    // refusal names the conversion that does cross.
    let prog = compile_ok("s: ;: 'a b'", "j");
    let msg = run(prog, &[]).unwrap_err();
    assert!(msg.contains("symbol results are not in the C ABI yet"), "{msg}");
    assert!(msg.contains("5 s:"), "{msg}");
    unsafe { jay_program_free(prog) };
}

#[test]
fn an_unknown_language_is_named() {
    let msg = compile("1 + 1", "k", -1).unwrap_err();
    assert!(msg.contains("unknown language: k"), "{msg}");
}

#[test]
fn bad_input_data_is_reported_at_the_boundary() {
    let prog = compile_ok("+/{v}", "j");
    let shape = [2u64];
    let bogus = [7u8, 0];
    let msg = run(prog, &[value(JAY_BOOL, &shape, bogus.as_ptr() as *const c_void)]).unwrap_err();
    assert!(msg.contains("must contain only 0 and 1"), "{msg}");

    let msg = run(prog, &[value(99, &shape, bogus.as_ptr() as *const c_void)]).unwrap_err();
    assert!(msg.contains("unknown dtype tag 99"), "{msg}");

    let msg = run(prog, &[value(JAY_I64, &shape, ptr::null())]).unwrap_err();
    assert!(msg.contains("data is NULL"), "{msg}");

    unsafe { jay_program_free(prog) };
}

#[test]
fn echo_goes_to_the_write_callback() {
    let prog = compile_ok("echo 1 2 3", "j");
    let mut sink = String::new();
    let mut out: *mut jay_result = ptr::null_mut();
    let mut err: *mut jay_error = ptr::null_mut();
    let rc = unsafe {
        jay_run(
            prog,
            ptr::null(),
            0,
            Some(collect),
            &mut sink as *mut String as *mut c_void,
            &mut out,
            &mut err,
        )
    };
    assert_eq!(rc, 0, "{}", take_message(err));
    assert_eq!(sink, "1 2 3\n");
    unsafe {
        jay_result_free(out);
        jay_program_free(prog);
    }
}

#[test]
fn a_program_ending_in_an_assignment_yields_nothing() {
    let prog = compile_ok("a =. 1 2 3", "j");
    let r = run_ok(prog, &[]);
    assert_eq!(unsafe { jay_result_is_empty(r) }, 1);
    assert_eq!(unsafe { jay_result_dtype(r) }, 0);
    assert_eq!(unsafe { jay_result_rank(r) }, -1);
    assert!(unsafe { jay_result_shape(r) }.is_null());
    assert!(unsafe { jay_result_data(r) }.is_null());
    assert!(unsafe { jay_result_format(r) }.is_null());
    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }
}

#[test]
fn a_program_can_be_run_more_than_once() {
    let prog = compile_ok("+/{v}", "j");
    let shape = [3u64];
    for (data, want) in [([1i64, 2, 3], 6i64), ([10, 20, 30], 60)] {
        let r = run_ok(prog, &[value(JAY_I64, &shape, data.as_ptr() as *const c_void)]);
        assert_eq!(result_i64(r), vec![want]);
        unsafe { jay_result_free(r) };
    }
    unsafe { jay_program_free(prog) };
}

#[test]
fn a_discarded_result_still_runs() {
    let prog = compile_ok("1 + 1", "j");
    let mut err: *mut jay_error = ptr::null_mut();
    let rc = unsafe {
        jay_run(prog, ptr::null(), 0, Some(collect), ptr::null_mut(), ptr::null_mut(), &mut err)
    };
    assert_eq!(rc, 0);
    assert!(err.is_null());
    unsafe { jay_program_free(prog) };
}

#[test]
fn every_entry_point_tolerates_null() {
    unsafe {
        assert!(jay_compile(ptr::null(), ptr::null(), -1, ptr::null_mut()).is_null());
        // A NULL source with an error slot still explains itself.
        let mut err: *mut jay_error = ptr::null_mut();
        assert!(jay_compile(ptr::null(), c"j".as_ptr(), -1, &mut err).is_null());
        assert!(take_message(err).contains("source_utf8 is NULL"));

        jay_program_free(ptr::null_mut());
        assert_eq!(jay_program_param_count(ptr::null()), 0);
        assert!(jay_program_param_name(ptr::null(), 0).is_null());

        assert_ne!(
            jay_run(ptr::null(), ptr::null(), 0, None, ptr::null_mut(), ptr::null_mut(), ptr::null_mut()),
            0
        );

        assert_eq!(jay_result_is_empty(ptr::null()), 1);
        assert_eq!(jay_result_dtype(ptr::null()), 0);
        assert_eq!(jay_result_rank(ptr::null()), -1);
        assert!(jay_result_shape(ptr::null()).is_null());
        assert!(jay_result_data(ptr::null()).is_null());
        assert!(jay_result_format(ptr::null()).is_null());
        jay_result_free(ptr::null_mut());

        assert!(jay_error_message(ptr::null()).is_null());
        jay_error_free(ptr::null_mut());
        jay_string_free(ptr::null_mut());
    }
}

#[test]
fn version_is_a_readable_string() {
    let v = unsafe { CStr::from_ptr(jay_version()) }.to_str().unwrap();
    assert_eq!(v, env!("CARGO_PKG_VERSION"));
    assert!(v.starts_with(char::is_numeric), "{v}");
}

/// Complex data crosses in and out as interleaved doubles — the layout C's
/// `double _Complex` and numpy's `complex128` both use, so a caller passes
/// its own buffer straight through.
#[test]
fn complex_data_crosses_both_ways() {
    let prog = compile_ok("{z} * {z}", "j");
    let data: [[f64; 2]; 3] = [[1.0, 1.0], [3.0, 4.0], [0.0, 1.0]];
    let shape = [data.len() as u64];
    let arg = value(JAY_COMPLEX, &shape, data.as_ptr() as *const c_void);
    let r = run_ok(prog, &[arg]);

    assert_eq!(unsafe { jay_result_dtype(r) }, JAY_COMPLEX);
    assert_eq!(shape_of(r), vec![3]);
    assert_eq!(result_complex(r), vec![[0.0, 2.0], [-7.0, 24.0], [-1.0, 0.0]]);
    // The display demotes an exactly-real element and nothing else.
    assert_eq!(formatted(r), "0j2 _7j24 _1");

    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }
}

/// A real argument whose answer is not real comes back as JAY_COMPLEX, so a
/// C caller has to read the dtype rather than assume the one it passed in.
#[test]
fn a_real_argument_can_produce_a_complex_result() {
    let prog = compile_ok("%: {x}", "j");
    let data = [-4.0f64, 9.0];
    let shape = [data.len() as u64];
    let arg = value(JAY_F64, &shape, data.as_ptr() as *const c_void);
    let r = run_ok(prog, &[arg]);

    assert_eq!(unsafe { jay_result_dtype(r) }, JAY_COMPLEX);
    assert_eq!(result_complex(r), vec![[0.0, 2.0], [3.0, 0.0]]);

    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }
}

// --- the input half of stdio ----------------------------------------------

/// A list of lines behind `userdata`, handed out one per call under the
/// protocol on `jay_read_fn`: the length when the buffer is too small,
/// otherwise the bytes; negative at the end.
unsafe extern "C" fn feed(buf: *mut c_char, cap: usize, userdata: *mut c_void) -> i32 {
    let lines = unsafe { &mut *(userdata as *mut Vec<String>) };
    if lines.is_empty() {
        return -1;
    }
    let bytes = lines[0].clone().into_bytes();
    if bytes.len() > cap {
        // The line stays where it is: libjay will grow its buffer and ask
        // for the same line again.
        return bytes.len() as i32;
    }
    lines.remove(0);
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, bytes.len()) };
    bytes.len() as i32
}

fn run_io_ok(prog: *mut jay_program, lines: &[&str]) -> (*mut jay_result, String) {
    let mut lines: Vec<String> = lines.iter().map(|s| (*s).to_string()).collect();
    let mut sink = String::new();
    let mut out: *mut jay_result = ptr::null_mut();
    let mut err: *mut jay_error = ptr::null_mut();
    let rc = unsafe {
        jay_run_io(
            prog,
            ptr::null(),
            0,
            Some(collect),
            &mut sink as *mut String as *mut c_void,
            Some(feed),
            &mut lines as *mut Vec<String> as *mut c_void,
            &mut out,
            &mut err,
        )
    };
    assert_eq!(rc, 0, "{}", take_message(err));
    (out, sink)
}

#[test]
fn a_read_callback_answers_what_the_program_reads() {
    let prog = compile_ok("⍞", "apl");
    let (r, _) = run_io_ok(prog, &["hello"]);
    assert_eq!(formatted(r), "hello");
    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }

    // The evaluated form runs the line it read.
    let prog = compile_ok("⎕", "apl");
    let (r, _) = run_io_ok(prog, &["2+2"]);
    assert_eq!(result_i64(r), vec![4]);
    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }

    // J reads through the same source, and writes through the sink.
    let prog = compile_ok("(1!:1 ]1) 1!:2 ]2", "j");
    let (r, written) = run_io_ok(prog, &["through"]);
    assert_eq!(formatted(r), "through");
    assert_eq!(written, "through\n");
    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }
}

#[test]
fn a_line_longer_than_the_buffer_arrives_whole() {
    let long = "x".repeat(9000);
    let prog = compile_ok("≢⍞", "apl");
    let (r, _) = run_io_ok(prog, &[&long]);
    assert_eq!(result_i64(r), vec![9000]);
    unsafe {
        jay_result_free(r);
        jay_program_free(prog);
    }
}

#[test]
fn the_end_of_the_input_is_reported_not_guessed() {
    let prog = compile_ok("⍞", "apl");
    let mut lines: Vec<String> = Vec::new();
    let mut out: *mut jay_result = ptr::null_mut();
    let mut err: *mut jay_error = ptr::null_mut();
    let rc = unsafe {
        jay_run_io(
            prog,
            ptr::null(),
            0,
            None,
            ptr::null_mut(),
            Some(feed),
            &mut lines as *mut Vec<String> as *mut c_void,
            &mut out,
            &mut err,
        )
    };
    assert_ne!(rc, 0);
    assert!(take_message(err).contains("the input has ended"));
    unsafe { jay_program_free(prog) };
}

/// `jay_run` is the signature that was always there, and it stays the run
/// with no input source at all — a different diagnostic from an exhausted
/// one, and the reason the new spelling is a new function.
#[test]
fn the_old_run_has_no_input_source() {
    let prog = compile_ok("⍞", "apl");
    let mut out: *mut jay_result = ptr::null_mut();
    let mut err: *mut jay_error = ptr::null_mut();
    let rc = unsafe {
        jay_run(prog, ptr::null(), 0, None, ptr::null_mut(), &mut out, &mut err)
    };
    assert_ne!(rc, 0);
    assert!(take_message(err).contains("no input source attached"));
    unsafe { jay_program_free(prog) };
}
