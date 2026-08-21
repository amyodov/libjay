//! The data boundary: Arrow and numpy input, Arrow output.
//!
//! Contiguous `i64` and `f64` blocks cross into libjay without a copy;
//! everything else is either widened with a copy or refused with an error
//! that names the offending column and the fix. Nothing is guessed: nulls
//! and disagreeing column types stop the call.

use std::ffi::{c_void, CStr};
use std::ptr::NonNull;
use std::sync::Arc;

use arrow_array::cast::AsArray;
use arrow_array::ffi::{from_ffi, to_ffi};
use arrow_array::ffi_stream::FFI_ArrowArrayStream;
use arrow_array::types::{
    Date32Type, Float32Type, Int16Type, Int32Type, Int8Type, Time32MillisecondType,
    Time32SecondType, UInt16Type, UInt32Type, UInt64Type, UInt8Type,
};
use arrow_array::{
    Array as ArrowArray, ArrayRef, BooleanArray, Float64Array, Int64Array, StructArray, make_array,
};
use arrow_buffer::{Buffer, ScalarBuffer};
use arrow_data::ffi::FFI_ArrowArray;
use arrow_schema::ffi::FFI_ArrowSchema;
use arrow_schema::{ArrowError, DataType, Field, TimeUnit};
use pyo3::prelude::*;
use pyo3::types::{PyCapsule, PyCapsuleMethods};

use jay::{Array, Buf, DType, Data, Owner};

use crate::JayError;

fn arrow_err(e: ArrowError) -> PyErr {
    JayError::new_err(format!("Arrow data could not be read: {e}"))
}

/// "column 'close'" when the producer named it, "the input" when it did not.
fn describe(name: &str) -> String {
    if name.is_empty() { "the input".to_string() } else { format!("column '{name}'") }
}

// ---------------------------------------------------------------- importing

/// Import an object that carries Arrow data or a numpy memory block.
///
/// `None` means "not one of those" and the caller falls through to its own
/// handling; `Some(Err(..))` means the object was recognised and refused.
pub fn try_import(obj: &Bound<'_, PyAny>) -> Option<PyResult<Array>> {
    if obj.hasattr("__arrow_c_array__").unwrap_or(false) {
        return Some(import_arrow_array(obj));
    }
    if obj.hasattr("__arrow_c_stream__").unwrap_or(false) {
        return Some(import_arrow_stream(obj));
    }
    import_numpy(obj)
}

/// The names the Arrow PyCapsule interface gives its capsules.
const ARROW_SCHEMA: &CStr = c"arrow_schema";
const ARROW_ARRAY: &CStr = c"arrow_array";
const ARROW_STREAM: &CStr = c"arrow_array_stream";

/// Refuse a capsule that is not the one the Arrow PyCapsule interface says
/// it should be.
///
/// The name is the only thing that says what the pointer inside points at,
/// and what happens next is to move a struct out of it. A producer that
/// hands back the pair in the wrong order, or a capsule from some other
/// library, must be an error here and not a write through the wrong type.
fn check_capsule(capsule: &Bound<'_, PyCapsule>, expected: &CStr) -> PyResult<()> {
    // SAFETY: the name is copied out of the capsule here and now. Nothing
    // between the read and the copy runs Python code, which is the only
    // thing that could rename the capsule out from under it.
    let got = capsule.name()?.map(|n| unsafe { n.as_cstr() }.to_string_lossy().into_owned());
    let want = expected.to_string_lossy();
    if got.as_deref() == Some(want.as_ref()) {
        return Ok(());
    }
    Err(JayError::new_err(match got {
        Some(n) => format!("expected an Arrow '{want}' capsule, got '{n}'"),
        None => format!("expected an Arrow '{want}' capsule, got an unnamed one"),
    }))
}

/// The pointer a checked capsule holds, or null.
///
/// `pointer_checked` reports a capsule with nothing in it as a Python
/// exception; the callers here name the method that produced the capsule
/// instead, so they take the null and say so themselves.
fn capsule_pointer(capsule: &Bound<'_, PyCapsule>, name: &CStr) -> *mut c_void {
    capsule.pointer_checked(Some(name)).map_or(std::ptr::null_mut(), |p| p.as_ptr())
}

/// A single Arrow value through the PyCapsule array interface.
fn import_arrow_array(obj: &Bound<'_, PyAny>) -> PyResult<Array> {
    let pair = obj.call_method0("__arrow_c_array__")?;
    let (schema_capsule, array_capsule) =
        pair.extract::<(Bound<'_, PyCapsule>, Bound<'_, PyCapsule>)>()?;
    check_capsule(&schema_capsule, ARROW_SCHEMA)?;
    check_capsule(&array_capsule, ARROW_ARRAY)?;
    let schema_ptr = capsule_pointer(&schema_capsule, ARROW_SCHEMA) as *mut FFI_ArrowSchema;
    let array_ptr = capsule_pointer(&array_capsule, ARROW_ARRAY) as *mut FFI_ArrowArray;
    if schema_ptr.is_null() || array_ptr.is_null() {
        return Err(JayError::new_err("__arrow_c_array__ returned empty capsules"));
    }
    // SAFETY: the Arrow PyCapsule interface guarantees these point at a live
    // FFI_ArrowSchema / FFI_ArrowArray. Moving the structs out and leaving
    // released ones behind is how a consumer takes ownership: the capsules'
    // own destructors then see `release == NULL` and do nothing.
    let (schema, ffi_array) = unsafe {
        (
            std::ptr::replace(schema_ptr, FFI_ArrowSchema::empty()),
            std::ptr::replace(array_ptr, FFI_ArrowArray::empty()),
        )
    };
    let field = Field::try_from(&schema).map_err(arrow_err)?;
    // SAFETY: `ffi_array` was produced against `schema` by the exporter and
    // is now owned by us.
    let data = unsafe { from_ffi(ffi_array, &schema) }.map_err(arrow_err)?;
    assemble(obj, &field, vec![make_array(data)])
}

/// A table, or a chunked single column, through the PyCapsule stream
/// interface.
fn import_arrow_stream(obj: &Bound<'_, PyAny>) -> PyResult<Array> {
    let capsule = obj.call_method0("__arrow_c_stream__")?.extract::<Bound<'_, PyCapsule>>()?;
    check_capsule(&capsule, ARROW_STREAM)?;
    let stream_ptr = capsule_pointer(&capsule, ARROW_STREAM) as *mut FFI_ArrowArrayStream;
    if stream_ptr.is_null() {
        return Err(JayError::new_err("__arrow_c_stream__ returned an empty capsule"));
    }
    // SAFETY: as in `import_arrow_array`; we take ownership of the stream and
    // leave a released one in the capsule. Dropping `stream` releases it.
    let mut stream = unsafe { std::ptr::replace(stream_ptr, FFI_ArrowArrayStream::empty()) };
    if stream.release.is_none() {
        return Err(JayError::new_err("the Arrow stream was already released"));
    }
    // A live stream must carry both readers; the interface says so, and the
    // calls below would otherwise be made through a NULL pointer.
    if stream.get_schema.is_none() || stream.get_next.is_none() {
        return Err(JayError::new_err("the Arrow stream has no reader callbacks"));
    }
    let schema = stream_schema(&mut stream)?;
    let field = Field::try_from(&schema).map_err(arrow_err)?;
    let chunks = stream_chunks(&mut stream, &schema)?;
    assemble(obj, &field, chunks)
}

/// The C stream interface, driven directly: arrow-rs's RecordBatchReader
/// insists on a struct schema, but a single column (a Series) arrives with
/// its own element type at the top.
fn stream_schema(stream: &mut FFI_ArrowArrayStream) -> PyResult<FFI_ArrowSchema> {
    let get = stream.get_schema.expect("checked by the caller");
    let mut schema = FFI_ArrowSchema::empty();
    // SAFETY: `stream` is live and unreleased; `get_schema` fills the empty
    // schema we own and hand it its own pointer, as the interface requires.
    let code = unsafe { get(stream, &mut schema) };
    if code != 0 {
        return Err(stream_error(stream, code));
    }
    Ok(schema)
}

fn stream_chunks(
    stream: &mut FFI_ArrowArrayStream,
    schema: &FFI_ArrowSchema,
) -> PyResult<Vec<ArrayRef>> {
    let get = stream.get_next.expect("checked by the caller");
    let mut out = Vec::new();
    loop {
        let mut array = FFI_ArrowArray::empty();
        // SAFETY: as in `stream_schema`.
        let code = unsafe { get(stream, &mut array) };
        if code != 0 {
            return Err(stream_error(stream, code));
        }
        if array.is_released() {
            return Ok(out);
        }
        // SAFETY: the stream produced `array` against `schema` and we own it.
        let data = unsafe { from_ffi(array, schema) }.map_err(arrow_err)?;
        out.push(make_array(data));
    }
}

fn stream_error(stream: &mut FFI_ArrowArrayStream, code: i32) -> PyErr {
    let detail = stream.get_last_error.and_then(|f| {
        // SAFETY: the stream is live; the string it returns belongs to it and
        // is copied here before anything else touches the stream.
        let text = unsafe { f(stream) };
        if text.is_null() {
            None
        } else {
            Some(unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned())
        }
    });
    match detail {
        Some(d) => JayError::new_err(format!("the Arrow stream failed: {d}")),
        None => JayError::new_err(format!("the Arrow stream failed with code {code}")),
    }
}

/// Imported chunks as one libjay array. A struct (a table) of N columns and
/// M rows becomes shape [M, N] — rows leading, matching the
/// DataFrame-to-matrix contract; anything else is one column of shape [M].
fn assemble(obj: &Bound<'_, PyAny>, field: &Field, chunks: Vec<ArrayRef>) -> PyResult<Array> {
    let rows: usize = chunks.iter().map(|c| c.len()).sum();
    // A struct of exactly `re` and `im` is one complex column, not a table
    // of two: it is the single-array form of the paired-column convention.
    let complex_struct = matches!(field.data_type(), DataType::Struct(f) if is_complex_pair(f));
    let DataType::Struct(fields) = field.data_type().clone().to_owned() else {
        let mut parts = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            parts.push(column_data(chunk, field.name(), obj)?);
        }
        let data = concat_chunks(parts, field.data_type(), field.name())?;
        return Ok(Array::new(vec![rows], data));
    };
    if complex_struct {
        let mut parts = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            parts.push(column_data(chunk, field.name(), obj)?);
        }
        let data = concat_chunks(parts, field.data_type(), field.name())?;
        return Ok(Array::new(vec![rows], data));
    }
    let cols = fields.len();
    if cols == 0 {
        return Ok(Array::new(vec![rows, 0], Data::empty(DType::I64)));
    }
    let tables: Vec<StructArray> =
        chunks.into_iter().map(|c| StructArray::from(c.to_data())).collect();
    let mut columns: Vec<Column> = Vec::with_capacity(cols);
    for (j, f) in fields.iter().enumerate() {
        let mut parts = Vec::with_capacity(tables.len());
        for table in &tables {
            parts.push(column_data(table.column(j), f.name(), obj)?);
        }
        columns.push(Column {
            name: f.name().clone(),
            arrow: type_name(f.data_type()),
            data: concat_chunks(parts, f.data_type(), f.name())?,
        });
    }
    let mut columns = pair_complex_columns(columns);
    check_agreement(&columns)?;
    if columns.len() == 1 {
        let c = columns.pop().expect("one column");
        return Ok(Array::new(vec![rows], c.data));
    }
    let cols = columns.len();
    let datas: Vec<Data> = columns.into_iter().map(|c| c.data).collect();
    // The table crosses as it lies: shape [rows, cols] rows-leading, which
    // is the contract, over a buffer that is the columns end to end, which
    // is what Arrow handed us. Nothing is copied and nothing is woven —
    // the runtime knows this layout and folds it where it lies.
    let joined = Data::join(&datas, rows)
        .ok_or_else(|| JayError::new_err("columns disagree on type or length"))?;
    Ok(Array::col_major(vec![rows, cols], joined))
}

/// Exactly two `Float64` children named `re` and `im`, in that order.
fn is_complex_pair(fields: &arrow_schema::Fields) -> bool {
    fields.len() == 2
        && fields[0].name() == "re"
        && fields[1].name() == "im"
        && fields[0].data_type() == &DataType::Float64
        && fields[1].data_type() == &DataType::Float64
}

/// One imported column: its name, the Arrow type it arrived as (for the
/// diagnostic), and its elements.
struct Column {
    name: String,
    arrow: String,
    data: Data,
}

/// Two adjacent float columns named `x_re` and `x_im` are one complex
/// column `x` — the paired-column convention a table carries complex data
/// in, since Arrow has no complex type of its own.
fn pair_complex_columns(columns: Vec<Column>) -> Vec<Column> {
    let mut out: Vec<Column> = Vec::with_capacity(columns.len());
    let mut it = columns.into_iter().peekable();
    while let Some(column) = it.next() {
        let stem = column.name.strip_suffix("_re").map(str::to_string);
        let joined = stem.and_then(|stem| {
            let next_is_pair = it
                .peek()
                .is_some_and(|c| c.name == format!("{stem}_im") && c.data.dtype() == DType::F64);
            if !next_is_pair || column.data.dtype() != DType::F64 {
                return None;
            }
            let imag = it.next().expect("peeked");
            let (Data::F64(re), Data::F64(im)) = (&column.data, &imag.data) else {
                return None;
            };
            let pairs: Vec<[f64; 2]> = re.iter().zip(im.iter()).map(|(&a, &b)| [a, b]).collect();
            Some(Column {
                name: stem,
                arrow: "a _re/_im column pair".to_string(),
                data: Data::Complex(pairs.into()),
            })
        });
        out.push(joined.unwrap_or(column));
    }
    out
}


fn concat_chunks(mut chunks: Vec<Data>, dt: &DataType, name: &str) -> PyResult<Data> {
    if chunks.len() == 1 {
        return Ok(chunks.pop().expect("one chunk"));
    }
    let dtype = chunks.first().map(Data::dtype).unwrap_or(DType::I64);
    let mut out = Data::empty(dtype);
    for chunk in &chunks {
        if !out.extend_from(chunk) {
            return Err(JayError::new_err(format!(
                "{} arrives in chunks of different types ({}); rechunk it first",
                describe(name),
                type_name(dt)
            )));
        }
    }
    Ok(out)
}


/// All columns must land on one element type. Widening one of them behind
/// the user's back can silently change values, so this refuses instead.
fn check_agreement(columns: &[Column]) -> PyResult<()> {
    let widest = columns
        .iter()
        .map(|c| c.data.dtype())
        .max_by_key(|d| match d {
            DType::Bool => 0,
            DType::I64 => 1,
            DType::Ext => 2,
            DType::Rat => 3,
            DType::F64 => 4,
            DType::Complex => 5,
            DType::Char => 6,
            DType::Box => 7,
        })
        .expect("at least one column");
    let Some(odd) = columns.iter().position(|c| c.data.dtype() != widest) else {
        return Ok(());
    };
    let target = arrow_name_for(widest);
    let wide = columns.iter().position(|c| c.data.dtype() == widest).expect("widest exists");
    Err(JayError::new_err(format!(
        "columns disagree: '{}' is {} but '{}' is {}; cast explicitly \
         (e.g. {}.cast({})) — automatic promotion can silently damage values \
         above 2^53",
        columns[odd].name,
        columns[odd].arrow,
        columns[wide].name,
        columns[wide].arrow,
        columns[odd].name,
        target,
    )))
}

fn arrow_name_for(dtype: DType) -> &'static str {
    match dtype {
        DType::Bool => "Boolean",
        DType::I64 => "Int64",
        DType::F64 => "Float64",
        DType::Complex => "a struct of re/im, or a _re/_im column pair",
        DType::Ext => "an arbitrary-precision integer column",
        DType::Rat => "a rational column",
        DType::Char => "Utf8",
        DType::Box => "a nested column",
    }
}

/// One Arrow column reduced to libjay data. Zero-copy for the 64-bit types.
fn column_data(array: &ArrayRef, name: &str, source: &Bound<'_, PyAny>) -> PyResult<Data> {
    if array.null_count() > 0 {
        return Err(JayError::new_err(format!(
            "{} contains {} null(s); J has no representation for missing \
             values — fill or filter them first (e.g. fill_null/drop_nulls)",
            describe(name),
            array.null_count()
        )));
    }
    let dt = array.data_type().clone();
    Ok(match dt {
        // Physically i64: reinterpreting is reading, not converting. A
        // timestamp difference is therefore plain integer arithmetic.
        DataType::Int64
        | DataType::Timestamp(_, _)
        | DataType::Date64
        | DataType::Duration(_)
        | DataType::Time64(_) => {
            let arr = reinterpret::<Int64Array>(array, DataType::Int64)?;
            Data::I64(borrow_arrow(arr.values().clone(), source))
        }
        DataType::Float64 => {
            let arr = reinterpret::<Float64Array>(array, DataType::Float64)?;
            Data::F64(borrow_arrow(arr.values().clone(), source))
        }
        DataType::Int8 => widen_i64(array.as_primitive::<Int8Type>().values()),
        DataType::Int16 => widen_i64(array.as_primitive::<Int16Type>().values()),
        DataType::Int32 => widen_i64(array.as_primitive::<Int32Type>().values()),
        DataType::Date32 => widen_i64(array.as_primitive::<Date32Type>().values()),
        DataType::Time32(TimeUnit::Second) => {
            widen_i64(array.as_primitive::<Time32SecondType>().values())
        }
        DataType::Time32(TimeUnit::Millisecond) => {
            widen_i64(array.as_primitive::<Time32MillisecondType>().values())
        }
        DataType::UInt8 => widen_i64(array.as_primitive::<UInt8Type>().values()),
        DataType::UInt16 => widen_i64(array.as_primitive::<UInt16Type>().values()),
        DataType::UInt32 => widen_i64(array.as_primitive::<UInt32Type>().values()),
        DataType::UInt64 => {
            let values = array.as_primitive::<UInt64Type>().values();
            let mut out = Vec::with_capacity(values.len());
            for &v in values.iter() {
                if v > i64::MAX as u64 {
                    return Err(too_big(name, v));
                }
                out.push(v as i64);
            }
            Data::I64(out.into())
        }
        DataType::Float32 => Data::F64(
            array.as_primitive::<Float32Type>().values().iter().map(|&v| v as f64).collect(),
        ),
        DataType::Boolean => {
            Data::Bool(array.as_boolean().values().iter().map(|v| v as u8).collect())
        }
        // Arrow's Null type is a column of nothing but nulls, and it
        // reports no null count of its own: the type is the missing value.
        DataType::Null => {
            return Err(JayError::new_err(format!(
                "{} holds nothing but nulls; J has no representation for \
                 missing values — fill or filter them first (e.g. \
                 fill_null/drop_nulls)",
                describe(name)
            )));
        }
        DataType::Decimal128(_, _) | DataType::Decimal256(_, _) => {
            return Err(JayError::new_err(format!(
                "{} is {}; decimal columns are not supported yet",
                describe(name),
                type_name(&dt)
            )));
        }
        // A complex column: two Float64 children, real then imaginary.
        DataType::Struct(ref fields) if is_complex_pair(fields) => {
            let st = StructArray::from(array.to_data());
            let re = reinterpret::<Float64Array>(&st.column(0).clone(), DataType::Float64)?;
            let im = reinterpret::<Float64Array>(&st.column(1).clone(), DataType::Float64)?;
            if re.null_count() > 0 || im.null_count() > 0 {
                return Err(JayError::new_err(format!(
                    "{} has nulls in its re/im children; fill or filter them first",
                    describe(name)
                )));
            }
            Data::Complex(
                re.values().iter().zip(im.values().iter()).map(|(&a, &b)| [a, b]).collect(),
            )
        }
        DataType::Utf8
        | DataType::LargeUtf8
        | DataType::Utf8View
        | DataType::Binary
        | DataType::LargeBinary
        | DataType::BinaryView
        | DataType::List(_)
        | DataType::LargeList(_)
        | DataType::ListView(_)
        | DataType::FixedSizeList(_, _)
        | DataType::Struct(_)
        | DataType::Map(_, _)
        | DataType::Union(_, _)
        | DataType::Dictionary(_, _) => {
            return Err(JayError::new_err(format!(
                "{} is {}; J has no boxed/nested arrays at the data boundary \
                 yet — select the numeric columns, or encode it as numbers first",
                describe(name),
                type_name(&dt)
            )));
        }
        other => {
            return Err(JayError::new_err(format!(
                "{} is {}; that Arrow type is not supported yet",
                describe(name),
                type_name(&other)
            )));
        }
    })
}

fn too_big(name: &str, value: u64) -> PyErr {
    JayError::new_err(format!(
        "{} holds {value}, which does not fit a 64-bit signed integer; \
         J has no unsigned 64-bit type — rescale or cast the column first",
        describe(name)
    ))
}

fn widen_i64<T: Copy + Into<i64>>(values: &[T]) -> Data {
    Data::I64(values.iter().map(|&v| v.into()).collect())
}

/// Re-read a column under a physically identical Arrow type. Free: the
/// buffers are shared, only the type label changes.
fn reinterpret<A: From<arrow_data::ArrayData>>(
    array: &ArrayRef,
    as_type: DataType,
) -> PyResult<A> {
    let data = array.to_data().into_builder().data_type(as_type).build().map_err(arrow_err)?;
    Ok(A::from(data))
}

/// Borrow an Arrow value buffer, keeping the buffer and the Python object
/// that produced it alive for as long as the borrow lasts.
fn borrow_arrow<T: arrow_buffer::ArrowNativeType + 'static>(
    values: ScalarBuffer<T>,
    source: &Bound<'_, PyAny>,
) -> Buf<T> {
    let ptr = values.as_ptr();
    let len = values.len();
    let owner: Owner = Arc::new((values, source.clone().unbind()));
    // SAFETY: the pointer addresses the Arrow buffer's heap allocation, which
    // the ScalarBuffer inside `owner` keeps alive (moving the ScalarBuffer
    // does not move the data). Arrow buffers are immutable once exported.
    unsafe { Buf::foreign(ptr, len, owner) }
}

fn type_name(dt: &DataType) -> String {
    match dt {
        DataType::Boolean => "bool".to_string(),
        DataType::Int8 => "int8".to_string(),
        DataType::Int16 => "int16".to_string(),
        DataType::Int32 => "int32".to_string(),
        DataType::Int64 => "int64".to_string(),
        DataType::UInt8 => "uint8".to_string(),
        DataType::UInt16 => "uint16".to_string(),
        DataType::UInt32 => "uint32".to_string(),
        DataType::UInt64 => "uint64".to_string(),
        DataType::Float32 => "float32".to_string(),
        DataType::Float64 => "float64".to_string(),
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => "a string column".to_string(),
        DataType::Binary | DataType::LargeBinary | DataType::BinaryView => {
            "a binary column".to_string()
        }
        DataType::Timestamp(unit, _) => format!("timestamp[{}]", unit_name(unit)),
        DataType::Duration(unit) => format!("duration[{}]", unit_name(unit)),
        DataType::Time32(unit) | DataType::Time64(unit) => format!("time[{}]", unit_name(unit)),
        DataType::Date32 | DataType::Date64 => "date".to_string(),
        DataType::Decimal128(p, s) | DataType::Decimal256(p, s) => format!("decimal({p},{s})"),
        DataType::List(_) | DataType::LargeList(_) | DataType::ListView(_) => {
            "a list column".to_string()
        }
        DataType::Struct(_) => "a struct column".to_string(),
        other => format!("{other:?}"),
    }
}

fn unit_name(unit: &TimeUnit) -> &'static str {
    match unit {
        TimeUnit::Second => "s",
        TimeUnit::Millisecond => "ms",
        TimeUnit::Microsecond => "us",
        TimeUnit::Nanosecond => "ns",
    }
}

// ------------------------------------------------- the numpy array interface

/// numpy and friends, through `__array_interface__`: a raw pointer, a shape,
/// strides and a dtype string.
///
/// This is the pointer-level protocol reachable from a limited-API (abi3)
/// build; CPython's buffer protocol only entered the limited API in 3.11 and
/// libjay's floor is 3.10.
fn import_numpy(obj: &Bound<'_, PyAny>) -> Option<PyResult<Array>> {
    let iface = obj.getattr("__array_interface__").ok()?;
    Some(read_array_interface(obj, &iface))
}

fn read_array_interface(obj: &Bound<'_, PyAny>, iface: &Bound<'_, PyAny>) -> PyResult<Array> {
    let shape: Vec<usize> = iface.get_item("shape")?.extract()?;
    let typestr: String = iface.get_item("typestr")?.extract()?;
    let count: usize = shape.iter().product();

    let (kind, width) = parse_typestr(&typestr)?;
    let mut column_major = false;
    let strides = iface.get_item("strides").ok().filter(|s| !s.is_none());
    if let Some(strides) = strides {
        let strides: Vec<isize> = strides.extract()?;
        // A Fortran-ordered block — numpy's `.T` of a C-ordered one, and
        // what a column store produces — is contiguous too, in the other
        // order. libjay carries that order rather than refusing it.
        if strides != c_strides(&shape, width) {
            if strides == f_strides(&shape, width) && shape.len() > 1 {
                column_major = true;
            } else {
                return Err(not_contiguous());
            }
        }
    }

    let data = iface.get_item("data")?;
    let Ok((address, _readonly)) = data.extract::<(usize, bool)>() else {
        return Err(JayError::new_err(
            "this array exposes its memory as a buffer object rather than an \
             address; copy it first (e.g. numpy's .copy())",
        ));
    };
    if count > 0 && address == 0 {
        return Err(JayError::new_err("array reports a null data pointer"));
    }
    // A block need not be aligned for its own element type: `np.frombuffer`
    // over an offset buffer produces exactly that, and the array interface
    // reports no strides for it because it really is contiguous. Reading one
    // as a slice would be undefined, so it is refused here rather than
    // trusted. Every element type this reads has alignment equal to its
    // width, except complex128, which is a pair of doubles.
    let align = match (kind, width) {
        (b'c', 16) => 8,
        (b'i' | b'u' | b'f', w @ (2 | 4 | 8)) => w,
        _ => 1,
    };
    if count > 0 && align > 1 && address % align != 0 {
        return Err(JayError::new_err(
            "array's memory is not aligned for its element type (a view into \
             an offset buffer?); make an aligned copy first, e.g. numpy's \
             .copy()",
        ));
    }

    // SAFETY (all arms): `address` is the start of `count` contiguous,
    // `width`-byte elements — the array interface's guarantee, with
    // contiguity and alignment checked above. numpy keeps that memory alive
    // for as long as the object exists, and the object is either held in
    // `owner` (borrowed arms) or alive for the whole call (copied arms).
    let data = match (kind, width) {
        (b'i', 8) => Data::I64(unsafe { borrow_block(obj, address as *const i64, count) }),
        (b'f', 8) => Data::F64(unsafe { borrow_block(obj, address as *const f64, count) }),
        // complex128 is two contiguous doubles per element, which is
        // exactly `Data::Complex`'s own layout — so it borrows.
        (b'c', 16) => {
            Data::Complex(unsafe { borrow_block(obj, address as *const [f64; 2], count) })
        }
        (b'i', 4) => Data::I64(unsafe { widen::<i32>(address, count) }),
        (b'i', 2) => Data::I64(unsafe { widen::<i16>(address, count) }),
        (b'i', 1) => Data::I64(unsafe { widen::<i8>(address, count) }),
        (b'u', 4) => Data::I64(unsafe { widen::<u32>(address, count) }),
        (b'u', 2) => Data::I64(unsafe { widen::<u16>(address, count) }),
        (b'u', 1) => Data::I64(unsafe { widen::<u8>(address, count) }),
        (b'f', 4) => Data::F64(
            unsafe { read::<f32>(address, count) }.iter().map(|&v| v as f64).collect(),
        ),
        (b'b', 1) => Data::Bool(
            unsafe { read::<u8>(address, count) }.iter().map(|&v| u8::from(v != 0)).collect(),
        ),
        (b'u', 8) => {
            let mut out = Vec::with_capacity(count);
            for &v in unsafe { read::<u64>(address, count) } {
                if v > i64::MAX as u64 {
                    return Err(too_big("", v));
                }
                out.push(v as i64);
            }
            Data::I64(out.into())
        }
        _ => {
            return Err(JayError::new_err(format!(
                "cannot read an array of dtype '{typestr}' as array data; \
                 that element type is not supported yet (supported: bool, \
                 8/16/32/64-bit integers, 32/64-bit floats, complex128)"
            )));
        }
    };
    Ok(if column_major { Array::col_major(shape, data) } else { Array::new(shape, data) })
}

fn not_contiguous() -> PyErr {
    JayError::new_err(
        "array is not contiguous (a transposed or sliced view?); make it \
         contiguous first, e.g. numpy's .copy()",
    )
}

/// Byte strides a C-contiguous array of this shape would have.
fn c_strides(shape: &[usize], width: usize) -> Vec<isize> {
    let mut out = vec![0isize; shape.len()];
    let mut step = width as isize;
    for i in (0..shape.len()).rev() {
        out[i] = step;
        step *= shape[i] as isize;
    }
    out
}

/// Byte strides a Fortran-contiguous array of this shape would have.
fn f_strides(shape: &[usize], width: usize) -> Vec<isize> {
    let mut out = vec![0isize; shape.len()];
    let mut step = width as isize;
    for (slot, &len) in out.iter_mut().zip(shape) {
        *slot = step;
        step *= len as isize;
    }
    out
}

/// `('<i8', ...)` → the dtype kind and its width in bytes. Byte order must
/// match the host's; `|` means "not applicable" (single-byte types).
fn parse_typestr(typestr: &str) -> PyResult<(u8, usize)> {
    let b = typestr.as_bytes();
    // numpy's object dtype is '|O', with no width: the block holds pointers
    // to Python objects rather than numbers, so there is nothing here to
    // read as array data.
    if b.get(1) == Some(&b'O') {
        return Err(JayError::new_err(
            "this array's dtype is object: its memory holds pointers to \
             Python objects, not numbers — convert it to a numeric dtype \
             first (e.g. numpy's .astype('int64') or .astype('float64'))",
        ));
    }
    if b.len() < 3 {
        return Err(JayError::new_err(format!("unreadable dtype '{typestr}'")));
    }
    let native = if cfg!(target_endian = "little") { b'<' } else { b'>' };
    if b[0] != native && b[0] != b'|' && b[0] != b'=' {
        return Err(JayError::new_err(format!(
            "array dtype '{typestr}' has the wrong byte order; \
             byte-swapped data is not supported yet"
        )));
    }
    let width: usize = typestr[2..].parse().unwrap_or(0);
    Ok((b[1], width))
}

/// # Safety
/// `address` must start `count` initialised, aligned `T`.
unsafe fn read<'a, T>(address: usize, count: usize) -> &'a [T] {
    if count == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(address as *const T, count) }
}

/// # Safety
/// As [`read`].
unsafe fn widen<T: Copy + Into<i64>>(address: usize, count: usize) -> Buf<i64> {
    unsafe { read::<T>(address, count) }.iter().map(|&v| v.into()).collect()
}

/// Borrow a numpy block, keeping the source object alive alongside it.
///
/// # Safety
///
/// `ptr` must start `len` initialised, aligned, contiguous `T` whose memory
/// `obj` keeps alive.
unsafe fn borrow_block<T: Send + Sync + 'static>(
    obj: &Bound<'_, PyAny>,
    ptr: *const T,
    len: usize,
) -> Buf<T> {
    let owner: Owner = Arc::new(obj.clone().unbind());
    // SAFETY: numpy ties the block's lifetime to the array object (or to its
    // `base`, which the array itself keeps alive); `owner` holds a strong
    // reference to that object. Python can still write into the block in
    // place — that is what zero-copy costs, and it is the documented
    // contract.
    unsafe { Buf::foreign(ptr, len, owner) }
}

// ---------------------------------------------------------------- exporting

/// Keeps a libjay buffer alive under Arrow's allocation contract.
struct Exported<T>(#[allow(dead_code)] Buf<T>);

// `Buf` holds an opaque owner, so it is not automatically unwind-safe; it is
// immutable once exported, which is what the marker actually asks for.
impl<T> std::panic::RefUnwindSafe for Exported<T> {}

/// Hand a libjay buffer to Arrow without copying it.
fn export_buffer<T: Clone + Send + Sync + 'static>(buf: Buf<T>) -> Buffer {
    let bytes = std::mem::size_of_val(&buf[..]);
    if bytes == 0 {
        return Buffer::from(Vec::<u8>::new());
    }
    let ptr = NonNull::new(buf.as_ptr() as *mut u8).expect("a non-empty buffer has a pointer");
    let owner = Arc::new(Exported(buf));
    // SAFETY: `ptr`/`bytes` describe the buffer we simultaneously move into
    // `owner`, so the memory outlives the Arrow Buffer; the region is
    // immutable and aligned for T, hence for u8.
    unsafe { Buffer::from_custom_allocation(ptr, bytes, owner) }
}

/// Export a rank-1 numeric result as ("arrow_schema", "arrow_array")
/// PyCapsules — the consumer side of `polars.Series(v)`/`pyarrow.array(v)`.
pub fn export_capsules<'py>(
    py: Python<'py>,
    array: &Array,
) -> PyResult<(Bound<'py, PyCapsule>, Bound<'py, PyCapsule>)> {
    if array.dtype().is_exact() {
        return Err(JayError::new_err(format!(
            "Arrow has no carrier for {} values; use .tolist() for exact \
             Python objects, or convert with `_1 x:` for machine numbers",
            array.dtype().name()
        )));
    }
    if array.rank() != 1 || matches!(array.dtype(), DType::Char | DType::Box) {
        return Err(JayError::new_err(format!(
            "cannot export a rank-{} {} result through Arrow yet; use \
             .tolist(), or ravel the result first",
            array.rank(),
            array.dtype().name()
        )));
    }
    let len = array.count();
    let arrow: ArrayRef = match array.data.clone() {
        Data::I64(b) => Arc::new(Int64Array::new(ScalarBuffer::new(export_buffer(b), 0, len), None)),
        Data::F64(b) => {
            Arc::new(Float64Array::new(ScalarBuffer::new(export_buffer(b), 0, len), None))
        }
        // Arrow booleans are bit-packed; ours are one byte each.
        Data::Bool(b) => Arc::new(BooleanArray::from(b.iter().map(|&v| v != 0).collect::<Vec<_>>())),
        // Arrow has no complex type, so a complex result leaves as a struct
        // of `re` and `im` — the single-array form of the paired-column
        // convention. Splitting the interleaved buffer costs one copy;
        // Arrow's layout gives no way to avoid it.
        Data::Complex(b) => {
            let re: Float64Array = b.iter().map(|z| z[0]).collect::<Vec<f64>>().into();
            let im: Float64Array = b.iter().map(|z| z[1]).collect::<Vec<f64>>().into();
            Arc::new(
                StructArray::try_new(
                    vec![
                        Field::new("re", DataType::Float64, false),
                        Field::new("im", DataType::Float64, false),
                    ]
                    .into(),
                    vec![Arc::new(re) as ArrayRef, Arc::new(im) as ArrayRef],
                    None,
                )
                .map_err(arrow_err)?,
            )
        }
        Data::Ext(_) | Data::Rat(_) | Data::Char(_) | Data::Box(_) => {
            unreachable!("refused above")
        }
    };
    let (ffi_array, ffi_schema) = to_ffi(&arrow.to_data()).map_err(arrow_err)?;
    let schema_capsule = PyCapsule::new_with_value(py, ffi_schema, ARROW_SCHEMA)?;
    let array_capsule = PyCapsule::new_with_value(py, ffi_array, ARROW_ARRAY)?;
    Ok((schema_capsule, array_capsule))
}
