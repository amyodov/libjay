//! Stable C ABI for libjay.
//!
//! The surface mirrors the Rust one: [`jay_compile`] turns a source string
//! into an opaque program, [`jay_run`] executes it with caller-supplied
//! arrays. Every function tolerates NULL handles, every function catches
//! panics, and everything returned by pointer has an explicit free.
//!
//! The C declarations live in `include/jay.h`; that header is the contract
//! and this file must keep matching it.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::slice;

use jay::array::{Array, Buf, Data};
use jay::dtype::DType;
use jay::error::{Error, ErrorKind};
use jay::fmt::{FmtOpts, format_array};
use jay::frontend::{Dialect, Lang, compile};
use jay::ir::Program;

/// Element type tags. Kept as plain integers rather than a Rust enum so that
/// a bogus value from C is a reported error and not undefined behaviour.
pub const JAY_BOOL: i32 = 1;
pub const JAY_I64: i32 = 2;
pub const JAY_F64: i32 = 3;
pub const JAY_CHAR: i32 = 4;
pub const JAY_COMPLEX: i32 = 5;

/// `jay_run` return codes.
const JAY_OK: c_int = 0;
const JAY_ERR: c_int = 1;

/// A compiled program plus its parameter names in NUL-terminated form.
#[allow(non_camel_case_types)]
pub struct jay_program {
    prog: Program,
    names: Vec<CString>,
}

/// One value produced by a run: the array, or nothing when the program's
/// last sentence was silent. Shape and character data are cached in the C
/// representations so borrowed pointers stay valid for the result's life.
#[allow(non_camel_case_types)]
pub struct jay_result {
    value: Option<Array>,
    shape: Vec<u64>,
    chars: Vec<u32>,
    fmt: FmtOpts,
}

/// A failure plus the source it points into, so the caret line can be
/// rendered on demand.
#[allow(non_camel_case_types)]
pub struct jay_error {
    err: Error,
    src: String,
}

/// A borrowed array descriptor. Valid only for the duration of the
/// `jay_run` call it is passed to; libjay copies what it needs.
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct jay_value {
    /// One of the `JAY_*` tags.
    pub dtype: i32,
    pub rank: i32,
    /// `rank` axis lengths, or NULL when `rank` is 0.
    pub shape: *const u64,
    /// Row-major elements: `uint8_t` 0/1, `int64_t`, `double`, a pair of
    /// `double` (real then imaginary) per complex value, or `uint32_t`
    /// codepoints. May be NULL when the array is empty.
    pub data: *const c_void,
}

/// Output sink for `echo` / `⎕←`. The text is UTF-8 and is *not*
/// NUL-terminated; `len` is authoritative.
#[allow(non_camel_case_types)]
pub type jay_write_fn =
    Option<unsafe extern "C" fn(text_utf8: *const c_char, len: usize, userdata: *mut c_void)>;

// ---------------------------------------------------------------------------
// internals
// ---------------------------------------------------------------------------

fn value_error(msg: impl Into<String>) -> Error {
    Error::new(ErrorKind::Value, msg, None)
}

/// Hand `err` a freshly boxed error, or drop it when the caller passed NULL.
fn store_error(slot: *mut *mut jay_error, err: Error, src: String) {
    if slot.is_null() {
        return;
    }
    let boxed = Box::new(jay_error { err, src });
    // SAFETY: `slot` is non-NULL; the caller owns a `jay_error *` there.
    unsafe { *slot = Box::into_raw(boxed) };
}

/// Run `f`, turning a panic into `fallback` plus an "internal panic" error.
/// Used at every extern boundary that reports errors.
fn guarded<T>(slot: *mut *mut jay_error, fallback: T, f: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(_) => {
            store_error(slot, Error::internal("internal panic"), String::new());
            fallback
        }
    }
}

/// Run `f` at a boundary with no error channel.
fn guarded_quiet<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

/// Borrow a NUL-terminated UTF-8 string, naming the argument on failure.
fn borrow_str<'a>(p: *const c_char, what: &str) -> Result<&'a str, Error> {
    if p.is_null() {
        return Err(value_error(format!("{what} is NULL")));
    }
    // SAFETY: non-NULL and, per the header, NUL-terminated and valid for
    // reads for the duration of the call.
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map_err(|_| value_error(format!("{what} is not valid UTF-8")))
}

/// `count` elements at `p`, or an empty slice when `count` is 0.
///
/// # Safety
///
/// `p` must be aligned for `T` and point to `count` initialised elements.
unsafe fn raw_slice<'a, T>(p: *const c_void, count: usize) -> &'a [T] {
    if count == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(p as *const T, count) }
    }
}

/// Copy a borrowed descriptor into an owned array.
///
/// Copying is deliberate for now: zero-copy ingestion needs the caller to
/// promise a lifetime, which this ABI does not yet express.
fn value_to_array(v: &jay_value, index: usize) -> Result<Array, Error> {
    let at = |m: &str| value_error(format!("argument {index}: {m}"));
    if v.rank < 0 {
        return Err(at("rank is negative"));
    }
    let rank = v.rank as usize;
    if rank > 0 && v.shape.is_null() {
        return Err(at("shape is NULL"));
    }
    // SAFETY: `rank` axis lengths at a non-NULL, u64-aligned `shape`.
    let axes: &[u64] = if rank == 0 { &[] } else { unsafe { slice::from_raw_parts(v.shape, rank) } };
    let mut shape = Vec::with_capacity(rank);
    let mut count: usize = 1;
    for &axis in axes {
        let axis = usize::try_from(axis).map_err(|_| at("axis length does not fit in size_t"))?;
        count = count.checked_mul(axis).ok_or_else(|| at("element count overflows size_t"))?;
        shape.push(axis);
    }
    if count > 0 && v.data.is_null() {
        return Err(at("data is NULL but the array is not empty"));
    }
    let data = match v.dtype {
        JAY_BOOL => {
            // SAFETY: `count` bytes at `data`, per the descriptor contract.
            let src: &[u8] = unsafe { raw_slice(v.data, count) };
            if src.iter().any(|&b| b > 1) {
                return Err(at("JAY_BOOL data must contain only 0 and 1"));
            }
            Data::Bool(Buf::from_vec(src.to_vec()))
        }
        // SAFETY (the three below): `count` elements of the tagged type at
        // `data`, aligned, per the descriptor contract.
        JAY_I64 => Data::I64(Buf::from_vec(unsafe { raw_slice::<i64>(v.data, count) }.to_vec())),
        JAY_F64 => Data::F64(Buf::from_vec(unsafe { raw_slice::<f64>(v.data, count) }.to_vec())),
        // Two doubles per element, which is the layout `Data::Complex`
        // already holds, so this is a copy and not a conversion.
        JAY_COMPLEX => {
            Data::Complex(Buf::from_vec(unsafe { raw_slice::<[f64; 2]>(v.data, count) }.to_vec()))
        }
        JAY_CHAR => {
            let src: &[u32] = unsafe { raw_slice(v.data, count) };
            let mut chars = Vec::with_capacity(count);
            for (i, &cp) in src.iter().enumerate() {
                let c = char::from_u32(cp)
                    .ok_or_else(|| at(&format!("element {i} is not a Unicode codepoint: {cp}")))?;
                chars.push(c);
            }
            Data::Char(Buf::from_vec(chars))
        }
        other => return Err(at(&format!("unknown dtype tag {other}"))),
    };
    Ok(Array::new(shape, data))
}

impl jay_result {
    fn new(value: Option<Array>, fmt: FmtOpts) -> jay_result {
        let shape = value
            .as_ref()
            .map(|a| a.shape.iter().map(|&d| d as u64).collect())
            .unwrap_or_default();
        let chars = match value.as_ref().map(|a| &a.data) {
            Some(Data::Char(v)) => v.iter().map(|&c| c as u32).collect(),
            _ => Vec::new(),
        };
        jay_result { value, shape, chars, fmt }
    }
}

fn dtype_tag(d: DType) -> i32 {
    match d {
        DType::Bool => JAY_BOOL,
        DType::I64 => JAY_I64,
        DType::F64 => JAY_F64,
        DType::Char => JAY_CHAR,
        DType::Complex => JAY_COMPLEX,
        // Refused by `jay_run` before a result is built.
        DType::Ext | DType::Rat | DType::Box => 0,
    }
}

/// Hand a Rust string to C as a `jay_string_free`-able buffer. NUL bytes
/// cannot occur in rendered errors or formatted arrays; if one ever does,
/// return NULL rather than truncate.
fn into_c_string(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

// ---------------------------------------------------------------------------
// lifecycle
// ---------------------------------------------------------------------------

/// Compile `source_utf8` in `lang` ("j" or "apl"); `index_origin` sets APL's
/// ⎕IO, or -1 for the language default. NULL on failure, with `*err` set.
///
/// # Safety
///
/// `source_utf8` and `lang` must each be NULL or a NUL-terminated
/// string readable for its whole length; `err` must be NULL or point to a
/// writable `jay_error *`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_compile(
    source_utf8: *const c_char,
    lang: *const c_char,
    index_origin: i32,
    err: *mut *mut jay_error,
) -> *mut jay_program {
    if !err.is_null() {
        // SAFETY: non-NULL out-param owned by the caller.
        unsafe { *err = ptr::null_mut() };
    }
    guarded(err, ptr::null_mut(), || {
        let build = || -> Result<jay_program, (Error, String)> {
            let src = borrow_str(source_utf8, "source_utf8").map_err(|e| (e, String::new()))?;
            let lang_name = borrow_str(lang, "lang").map_err(|e| (e, src.to_string()))?;
            let lang = Lang::from_name(lang_name).ok_or_else(|| {
                (
                    value_error(format!("unknown language: {lang_name}"))
                        .note("supported languages are \"j\" and \"apl\""),
                    src.to_string(),
                )
            })?;
            let dialect = Dialect {
                index_origin: if index_origin < 0 { None } else { Some(index_origin as i64) },
            };
            let prog = compile(lang, src, &dialect).map_err(|e| (e, src.to_string()))?;
            let names = prog
                .params
                .iter()
                .map(|p| CString::new(p.name.as_str()).unwrap_or_default())
                .collect();
            Ok(jay_program { prog, names })
        };
        match build() {
            Ok(p) => Box::into_raw(Box::new(p)),
            Err((e, src)) => {
                store_error(err, e, src);
                ptr::null_mut()
            }
        }
    })
}

/// Release a program. NULL is a no-op.
///
/// # Safety
///
/// `program` must be NULL or a handle from `jay_compile` that has
/// not been freed; it is dangling afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_program_free(program: *mut jay_program) {
    if program.is_null() {
        return;
    }
    guarded_quiet((), || {
        // SAFETY: non-NULL and produced by `jay_compile`, so it is a
        // `Box<jay_program>` the caller has not yet freed.
        drop(unsafe { Box::from_raw(program) });
    });
}

/// Number of parameters the program expects, in the order `jay_run` wants
/// them. 0 for NULL.
///
/// # Safety
///
/// `program` must be NULL or a live handle from `jay_compile`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_program_param_count(program: *const jay_program) -> usize {
    guarded_quiet(0, || match unsafe { program.as_ref() } {
        Some(p) => p.names.len(),
        None => 0,
    })
}

/// Name of parameter `i`, borrowed from the program. NULL when out of range.
///
/// # Safety
///
/// `program` must be NULL or a live handle from `jay_compile`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_program_param_name(program: *const jay_program, i: usize) -> *const c_char {
    guarded_quiet(ptr::null(), || match unsafe { program.as_ref() } {
        Some(p) => p.names.get(i).map(|n| n.as_ptr()).unwrap_or(ptr::null()),
        None => ptr::null(),
    })
}

// ---------------------------------------------------------------------------
// run
// ---------------------------------------------------------------------------

/// Execute a program. `args` holds one value per parameter, in order.
/// `write` NULL sends echo output to stdout; `out` NULL discards the result.
/// Returns 0 on success, nonzero with `*err` set on failure.
///
/// # Safety
///
/// `program` must be NULL or a live handle from `jay_compile`; `args`
/// must point to `nargs` descriptors whose `shape` and `data` are readable and
/// correctly aligned for the whole call; `write` must be NULL or callable with
/// `write_userdata`; `out` and `err` must be NULL or point to writable
/// pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_run(
    program: *const jay_program,
    args: *const jay_value,
    nargs: usize,
    write: jay_write_fn,
    write_userdata: *mut c_void,
    out: *mut *mut jay_result,
    err: *mut *mut jay_error,
) -> c_int {
    if !err.is_null() {
        // SAFETY: non-NULL out-param owned by the caller.
        unsafe { *err = ptr::null_mut() };
    }
    if !out.is_null() {
        // SAFETY: as above.
        unsafe { *out = ptr::null_mut() };
    }
    guarded(err, JAY_ERR, || {
        // SAFETY: `program` is either NULL or a live `jay_compile` handle.
        let Some(program) = (unsafe { program.as_ref() }) else {
            store_error(err, value_error("program is NULL"), String::new());
            return JAY_ERR;
        };
        let src = program.prog.display_src.clone();
        if nargs > 0 && args.is_null() {
            store_error(err, value_error("args is NULL but nargs is not 0"), src);
            return JAY_ERR;
        }
        // SAFETY: `nargs` descriptors at a non-NULL, aligned `args`.
        let descs: &[jay_value] =
            if nargs == 0 { &[] } else { unsafe { slice::from_raw_parts(args, nargs) } };
        if descs.len() != program.prog.params.len() {
            store_error(
                err,
                value_error(format!(
                    "expected {} argument(s), got {}",
                    program.prog.params.len(),
                    descs.len()
                )),
                src,
            );
            return JAY_ERR;
        }
        let mut arrays = Vec::with_capacity(descs.len());
        for (i, d) in descs.iter().enumerate() {
            match value_to_array(d, i) {
                Ok(a) => arrays.push(a),
                Err(e) => {
                    store_error(err, e, src);
                    return JAY_ERR;
                }
            }
        }
        let mut sink = |text: &str| match write {
            // SAFETY: the caller promises a callable matching `jay_write_fn`;
            // the pointer is valid UTF-8 bytes for `len`, for this call only.
            Some(f) => unsafe { f(text.as_ptr() as *const c_char, text.len(), write_userdata) },
            None => {
                let mut stdout = std::io::stdout();
                let _ = stdout.write_all(text.as_bytes());
                let _ = stdout.flush();
            }
        };
        match program.prog.run(&arrays, &mut sink) {
            Ok(value) => {
                // Boxed and exact results have no descriptor in this ABI
                // yet: their elements are arrays and bignums, not numbers.
                let unsupported = match value.as_ref().map(|a| a.dtype()) {
                    Some(DType::Box) => Some("boxed results are not in the C ABI yet"),
                    Some(DType::Ext) => Some(
                        "extended-precision results are not in the C ABI yet; \
                         convert with `_1 x:` first",
                    ),
                    Some(DType::Rat) => Some(
                        "rational results are not in the C ABI yet; \
                         convert with `_1 x:` first",
                    ),
                    _ => None,
                };
                if let Some(what) = unsupported {
                    store_error(err, value_error(what), src);
                    return JAY_ERR;
                }
                if out.is_null() {
                    return JAY_OK;
                }
                let result = Box::new(jay_result::new(value, program.prog.fmt));
                // SAFETY: `out` is non-NULL and owned by the caller.
                unsafe { *out = Box::into_raw(result) };
                JAY_OK
            }
            Err(e) => {
                store_error(err, e, src);
                JAY_ERR
            }
        }
    })
}

// ---------------------------------------------------------------------------
// results
// ---------------------------------------------------------------------------

/// 1 when the program yielded no value (its last sentence was an
/// assignment or `⎕←`), 0 otherwise. 1 for NULL.
///
/// # Safety
///
/// `result` must be NULL or a live handle from `jay_run`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_result_is_empty(result: *const jay_result) -> c_int {
    guarded_quiet(1, || match unsafe { result.as_ref() } {
        Some(r) => c_int::from(r.value.is_none()),
        None => 1,
    })
}

/// The result's element type. 0 for a NULL or valueless result.
///
/// # Safety
///
/// `result` must be NULL or a live handle from `jay_run`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_result_dtype(result: *const jay_result) -> i32 {
    guarded_quiet(0, || match unsafe { result.as_ref() }.and_then(|r| r.value.as_ref()) {
        Some(a) => dtype_tag(a.dtype()),
        None => 0,
    })
}

/// The result's rank (0 for a scalar). -1 for a NULL or valueless result.
///
/// # Safety
///
/// `result` must be NULL or a live handle from `jay_run`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_result_rank(result: *const jay_result) -> i32 {
    guarded_quiet(-1, || match unsafe { result.as_ref() }.and_then(|r| r.value.as_ref()) {
        Some(a) => a.rank() as i32,
        None => -1,
    })
}

/// The result's axis lengths, borrowed for its life; `rank` entries, and
/// non-NULL even at rank 0. NULL for a NULL or valueless result.
///
/// # Safety
///
/// `result` must be NULL or a live handle from `jay_run`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_result_shape(result: *const jay_result) -> *const u64 {
    guarded_quiet(ptr::null(), || match unsafe { result.as_ref() } {
        Some(r) if r.value.is_some() => r.shape.as_ptr(),
        _ => ptr::null(),
    })
}

/// The result's row-major elements, borrowed for its life: `uint8_t` 0/1,
/// `int64_t`, `double`, or `uint32_t` codepoints. NULL when valueless.
///
/// # Safety
///
/// `result` must be NULL or a live handle from `jay_run`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_result_data(result: *const jay_result) -> *const c_void {
    guarded_quiet(ptr::null(), || {
        let Some(r) = (unsafe { result.as_ref() }) else { return ptr::null() };
        match r.value.as_ref().map(|a| &a.data) {
            Some(Data::Bool(v)) => v.as_ptr() as *const c_void,
            Some(Data::I64(v)) => v.as_ptr() as *const c_void,
            Some(Data::F64(v)) => v.as_ptr() as *const c_void,
            Some(Data::Complex(v)) => v.as_ptr() as *const c_void,
            Some(Data::Char(_)) => r.chars.as_ptr() as *const c_void,
            Some(Data::Ext(_) | Data::Rat(_) | Data::Box(_)) => ptr::null(),
            None => ptr::null(),
        }
    })
}

/// The result formatted the way its language displays it, without a
/// trailing newline. Free with `jay_string_free`. NULL when valueless.
///
/// # Safety
///
/// `result` must be NULL or a live handle from `jay_run`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_result_format(result: *const jay_result) -> *mut c_char {
    guarded_quiet(ptr::null_mut(), || {
        match unsafe { result.as_ref() }.and_then(|r| r.value.as_ref().map(|a| (a, r.fmt))) {
            Some((a, fmt)) => into_c_string(format_array(a, &fmt)),
            None => ptr::null_mut(),
        }
    })
}

/// Release a result. NULL is a no-op.
///
/// # Safety
///
/// `result` must be NULL or a handle from `jay_run` that has not
/// been freed; it is dangling afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_result_free(result: *mut jay_result) {
    if result.is_null() {
        return;
    }
    guarded_quiet((), || {
        // SAFETY: non-NULL and produced by `jay_run`, so it is a
        // `Box<jay_result>` the caller has not yet freed.
        drop(unsafe { Box::from_raw(result) });
    });
}

// ---------------------------------------------------------------------------
// errors and strings
// ---------------------------------------------------------------------------

/// The error rendered for display, including the source line and caret when
/// the error has a position. Free with `jay_string_free`. NULL for NULL.
///
/// # Safety
///
/// `err` must be NULL or a live error handed out by this library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_error_message(err: *const jay_error) -> *mut c_char {
    guarded_quiet(ptr::null_mut(), || match unsafe { err.as_ref() } {
        Some(e) => into_c_string(e.err.render(&e.src)),
        None => ptr::null_mut(),
    })
}

/// Release an error. NULL is a no-op.
///
/// # Safety
///
/// `err` must be NULL or an error handed out by this library that has
/// not been freed; it is dangling afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_error_free(err: *mut jay_error) {
    if err.is_null() {
        return;
    }
    guarded_quiet((), || {
        // SAFETY: non-NULL and produced by this library, so it is a
        // `Box<jay_error>` the caller has not yet freed.
        drop(unsafe { Box::from_raw(err) });
    });
}

/// Release a string returned by this library. NULL is a no-op.
///
/// # Safety
///
/// `s` must be NULL or a string handed out by this library that has
/// not been freed; it is dangling afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jay_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    guarded_quiet((), || {
        // SAFETY: non-NULL and produced by `into_c_string`, so it is a
        // `CString` the caller has not yet freed.
        drop(unsafe { CString::from_raw(s) });
    });
}

/// The libjay version, as a static NUL-terminated string. Never NULL.
#[unsafe(no_mangle)]
pub extern "C" fn jay_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}
