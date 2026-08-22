//! Python binding. Exposes compile/run over the core crate; the friendly
//! layer (kernels with defaults, t-strings, the CLI) lives in Python.

mod data;

use std::sync::Arc;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{
    PyBool, PyCapsule, PyComplex, PyComplexMethods, PyFloat, PyInt, PyList, PyString, PyTuple,
};

use jay::fmt::{format_array, FmtOpts};
use jay::{Array, Data, DType, Device, Dialect, Ext, Lang, Precision, Rat};

create_exception!(_jay, JayError, PyException, "A libjay compile or run error.");

fn jay_err(display_src: &str, e: &jay::Error) -> PyErr {
    JayError::new_err(e.render(display_src))
}

fn parse_lang(name: &str) -> PyResult<Lang> {
    Lang::from_name(name)
        .ok_or_else(|| PyTypeError::new_err(format!("unknown language: {name:?}")))
}

/// A compiled program. Immutable and shareable; default values live on the
/// Python side.
#[pyclass(frozen, module = "libjay._jay")]
struct Kernel {
    program: Arc<jay::Program>,
    /// Where this kernel's fused nodes run. None is the plain CPU path,
    /// which is what a kernel that was never deployed has.
    device: Option<Device>,
}

#[pymethods]
impl Kernel {
    #[getter]
    fn params(&self) -> Vec<String> {
        self.program.params.iter().map(|p| p.name.clone()).collect()
    }

    /// A copy of this kernel placed on a device. `where` is "gpu" or "cpu";
    /// `precision` is "f64" (the default) or "f32".
    #[pyo3(signature = (place, precision=None))]
    fn deploy(&self, place: &str, precision: Option<&str>) -> PyResult<Kernel> {
        let device = match place.trim().to_ascii_lowercase().as_str() {
            "cpu" => Device::cpu(),
            "gpu" | "default" => Device::default_gpu().ok_or_else(|| {
                JayError::new_err(
                    "no GPU adapter was found; jay.devices() lists what this machine offers",
                )
            })?,
            other => {
                return Err(JayError::new_err(format!(
                    "unknown device: {other:?} (expected 'gpu' or 'cpu')"
                )))
            }
        };
        let device = match precision {
            None => device,
            Some(p) => device.with_precision(Precision::from_name(p).ok_or_else(|| {
                JayError::new_err(format!(
                    "unknown precision: {p:?} (expected 'f64' or 'f32')"
                ))
            })?),
        };
        Ok(Kernel { program: Arc::clone(&self.program), device: Some(device) })
    }

    /// What this kernel is deployed on, or None for the plain CPU path.
    #[getter]
    fn device(&self) -> Option<(String, String, String, bool, String)> {
        let d = self.device.as_ref()?;
        Some(match d.info() {
            None => ("cpu".into(), String::new(), "CPU".into(), true, "f64".into()),
            Some(i) => (
                i.name.clone(),
                i.backend.clone(),
                i.kind.clone(),
                i.f64,
                d.precision().name().to_string(),
            ),
        })
    }

    /// `value` with its elements resident on this kernel's device.
    fn upload(&self, value: &Bound<'_, PyAny>) -> PyResult<DeviceArray> {
        let device = self
            .device
            .clone()
            .ok_or_else(|| JayError::new_err("this kernel is not deployed on a device"))?;
        let array = py_to_array(value)?;
        let array = device.upload(&array).map_err(|e| JayError::new_err(e.to_string()))?;
        Ok(DeviceArray { array, device, fmt: self.program.fmt })
    }

    /// Run with positional values (one per parameter). `out` receives
    /// explicit output (echo / ⎕←); `inp` answers what the program reads
    /// (⍞, ⎕, 1!:1) with one line per call, or None at the end of the
    /// input. `inp` None is a run with no input source at all. Returns
    /// (result, display) where `display` is the formatted last value when
    /// `want_display` is set.
    #[pyo3(signature = (values, out, want_display, keep_on_device=false, inp=None))]
    fn run(
        &self,
        py: Python<'_>,
        values: Vec<Bound<'_, PyAny>>,
        out: Bound<'_, PyAny>,
        want_display: bool,
        keep_on_device: bool,
        inp: Option<Bound<'_, PyAny>>,
    ) -> PyResult<(Py<PyAny>, Option<String>)> {
        let args: Vec<Array> =
            values.iter().map(py_to_array).collect::<PyResult<_>>()?;
        let mut write_err: Option<PyErr> = None;
        let mut sink = |s: &str| {
            if write_err.is_none() && let Err(e) = out.call1((s,)) {
                write_err = Some(e);
            }
        };
        // The reader's own failure is not the end of the input: it is held
        // here and raised in place of whatever the program made of it.
        let mut read_err: Option<PyErr> = None;
        let has_input = inp.is_some();
        let mut source = || -> Option<String> {
            let f = inp.as_ref()?;
            if read_err.is_some() {
                return None;
            }
            match f.call0() {
                Ok(v) if v.is_none() => None,
                Ok(v) => match v.extract::<String>() {
                    Ok(s) => Some(s),
                    Err(e) => {
                        read_err = Some(e);
                        None
                    }
                },
                Err(e) => {
                    read_err = Some(e);
                    None
                }
            }
        };
        let result = match (&self.device, has_input) {
            (None, false) => self.program.run(&args, &mut sink),
            (None, true) => self.program.run_io(&args, &mut sink, &mut source),
            (Some(d), false) => self.program.run_on(d, &args, &mut sink),
            (Some(d), true) => self.program.run_on_io(d, &args, &mut sink, &mut source),
        };
        if let Some(e) = read_err {
            return Err(e);
        }
        if let Some(e) = write_err {
            return Err(e);
        }
        let result = result.map_err(|e| jay_err(&self.program.display_src, &e))?;
        let display = if want_display {
            result.as_ref().map(|a| format_array(a, &self.program.fmt)).filter(|s| !s.is_empty())
        } else {
            None
        };
        if keep_on_device {
            let device = self
                .device
                .clone()
                .ok_or_else(|| JayError::new_err("this kernel is not deployed on a device"))?;
            let value = match result {
                None => py.None(),
                Some(a) => {
                    let array =
                        device.upload(&a).map_err(|e| JayError::new_err(e.to_string()))?;
                    Py::new(py, DeviceArray { array, device, fmt: self.program.fmt })?.into_any()
                }
            };
            return Ok((value, display));
        }
        let value = match result {
            None => py.None(),
            Some(a) => array_to_py(py, a, self.program.fmt)?,
        };
        Ok((value, display))
    }

    /// Describe what the expression became. With `values` (one per
    /// parameter) the program is also run and every node is annotated with
    /// the shape and dtype it produced; without them, structure only.
    #[pyo3(signature = (values=None))]
    fn explain(&self, values: Option<Vec<Bound<'_, PyAny>>>) -> PyResult<String> {
        let args: Option<Vec<Array>> = match values {
            None => None,
            Some(v) => Some(v.iter().map(py_to_array).collect::<PyResult<_>>()?),
        };
        Ok(match &self.device {
            None => self.program.explain(args.as_deref()),
            Some(d) => self.program.explain_on(d, args.as_deref()),
        })
    }
}

/// An array whose elements are resident on a device.
///
/// It reads as an ordinary value — shape, dtype, `download()` — and it is
/// one: the host copy is there. What it also carries is the device
/// allocation, so passing it to a run on the same device uploads nothing.
#[pyclass(frozen, name = "DeviceArray", module = "libjay._jay")]
struct DeviceArray {
    array: Array,
    device: Device,
    fmt: FmtOpts,
}

#[pymethods]
impl DeviceArray {
    #[getter]
    fn shape(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(PyTuple::new(py, &self.array.shape)?.unbind().into())
    }

    #[getter]
    fn dtype(&self) -> &'static str {
        self.array.dtype().name()
    }

    fn __len__(&self) -> usize {
        self.array.items()
    }

    /// True while this array's buffer is still the device's own.
    #[getter]
    fn resident(&self) -> bool {
        self.device.holds(&self.array)
    }

    /// The value as ordinary Python data.
    fn download(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        array_to_py(py, self.array.clone(), self.fmt)
    }

    fn __repr__(&self) -> String {
        let what = self.device.info().map_or("cpu".to_string(), |i| i.name.clone());
        let shape: Vec<String> = self.array.shape.iter().map(usize::to_string).collect();
        format!(
            "<DeviceArray {} $ {} on {what}>",
            shape.join(" "),
            self.array.dtype().name()
        )
    }
}

/// A non-scalar result: shape, dtype, list conversion, J/APL-style repr.
#[pyclass(frozen, name = "Value", module = "libjay._jay")]
struct Value {
    array: Array,
    fmt: FmtOpts,
}

#[pymethods]
impl Value {
    #[getter]
    fn shape(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(PyTuple::new(py, &self.array.shape)?.unbind().into())
    }

    #[getter]
    fn dtype(&self) -> &'static str {
        self.array.dtype().name()
    }

    fn __len__(&self) -> usize {
        self.array.items()
    }

    fn tolist(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        nested_list(py, &self.array, 0, 0, self.array.count())
    }

    /// How deeply the value nests: 0 for a scalar, 1 for a simple array,
    /// one more than the deepest box for a boxed one (APL `≡`).
    #[getter]
    fn depth(&self) -> i64 {
        depth(&self.array)
    }

    fn __repr__(&self) -> String {
        format_array(&self.array, &self.fmt)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.cast::<Value>() {
            Ok(v) => v.get().array == self.array,
            Err(_) => false,
        }
    }

    /// The Arrow C data interface: a rank-1 numeric result leaves as
    /// ("arrow_schema", "arrow_array") PyCapsules, without copying its
    /// integer or float payload. `requested_schema` is accepted and ignored.
    #[pyo3(signature = (requested_schema=None))]
    fn __arrow_c_array__<'py>(
        &self,
        py: Python<'py>,
        requested_schema: Option<Bound<'py, PyAny>>,
    ) -> PyResult<(Bound<'py, PyCapsule>, Bound<'py, PyCapsule>)> {
        let _ = requested_schema;
        data::export_capsules(py, &self.array)
    }
}

/// Elements [start, end) at nesting depth `axis`, as nested Python lists.
fn nested_list(
    py: Python<'_>,
    a: &Array,
    axis: usize,
    start: usize,
    end: usize,
) -> PyResult<Py<PyAny>> {
    if axis == a.rank() {
        return element_to_py(py, &a.data, start);
    }
    let n = a.shape[axis];
    let stride = (end - start) / n.max(1);
    let items: Vec<Py<PyAny>> = (0..n)
        .map(|i| nested_list(py, a, axis + 1, start + i * stride, start + (i + 1) * stride))
        .collect::<PyResult<_>>()?;
    Ok(PyList::new(py, items)?.unbind().into())
}

/// An arbitrary-precision integer as a Python `int`. Python's own integers
/// are unbounded, so the value crosses whole; the decimal spelling is the
/// only representation both sides already agree on.
fn ext_to_py(py: Python<'_>, v: &Ext) -> PyResult<Py<PyAny>> {
    if let Some(small) = jay::exact::ext_to_i64(v) {
        return Ok(small.into_pyobject(py)?.unbind().into());
    }
    Ok(py.get_type::<PyInt>().call1((v.to_string(),))?.unbind())
}

/// A rational as a `fractions.Fraction`, which is Python's exact ratio.
fn rat_to_py(py: Python<'_>, v: &Rat) -> PyResult<Py<PyAny>> {
    let fraction = py.import("fractions")?.getattr("Fraction")?;
    let num = ext_to_py(py, v.numer())?;
    let den = ext_to_py(py, v.denom())?;
    Ok(fraction.call1((num, den))?.unbind())
}

fn element_to_py(py: Python<'_>, data: &Data, i: usize) -> PyResult<Py<PyAny>> {
    Ok(match data {
        Data::Bool(v) => (v[i] != 0).into_pyobject(py)?.to_owned().unbind().into(),
        Data::I64(v) => v[i].into_pyobject(py)?.unbind().into(),
        Data::Ext(v) => ext_to_py(py, &v[i])?,
        Data::Rat(v) => rat_to_py(py, &v[i])?,
        Data::F64(v) => v[i].into_pyobject(py)?.unbind().into(),
        // Python's own complex; a value whose imaginary part is zero is
        // still complex, as it is in J.
        Data::Complex(v) => {
            PyComplex::from_doubles(py, v[i][0], v[i][1]).unbind().into()
        }
        Data::Char(v) => v[i].to_string().into_pyobject(py)?.unbind().into(),
        // A symbol crosses as the name it stands for. Python has no symbol
        // of its own, and a `str` going the other way stays a character
        // array — `s:` is how one becomes a symbol.
        Data::Symbol(v) => {
            jay::symbol::name(v[i]).as_ref().into_pyobject(py)?.unbind().into()
        }
        // A box converts to whatever its contents convert to: Python
        // holds nested data as nested lists and strings, not as wrappers.
        Data::Box(v) => contents_to_py(py, &v[i])?,
    })
}

/// A whole array as plain Python data: a character vector is a string, a
/// scalar is a number, anything else is a (nested) list.
fn contents_to_py(py: Python<'_>, a: &Array) -> PyResult<Py<PyAny>> {
    if a.rank() == 1 && a.dtype() == DType::Char {
        let s: String = match &a.data {
            Data::Char(v) => v.iter().collect(),
            _ => unreachable!("checked the dtype"),
        };
        return Ok(PyString::new(py, &s).unbind().into());
    }
    nested_list(py, a, 0, 0, a.count())
}

/// The value's depth, as APL's `≡` counts it.
fn depth(a: &Array) -> i64 {
    match &a.data {
        Data::Box(v) => 1 + v.iter().map(depth).max().unwrap_or(0),
        _ => i64::from(a.rank() > 0),
    }
}

fn array_to_py(py: Python<'_>, a: Array, fmt: FmtOpts) -> PyResult<Py<PyAny>> {
    // Everything on this side of the boundary reads elements in row-major
    // order — `tolist`, the repr, the Arrow export — so a value that was
    // computed column-major is laid out once, here, and once only.
    let a = if a.is_row_major() { a } else { a.to_row_major() };
    // Python has no sparse carrier, so a sparse result crosses as the dense
    // array it stands for.
    let a = if a.is_sparse() { a.densified() } else { a };
    if a.rank() == 0 {
        // A scalar box hands back what it holds, at whatever shape that
        // has: `<1 2 3` is the vector 1 2 3, `<'abc'` the string.
        if let Data::Box(v) = &a.data {
            return array_to_py(py, v[0].clone(), fmt);
        }
        return element_to_py(py, &a.data, 0);
    }
    if a.rank() == 1 && let Data::Char(v) = &a.data {
        return Ok(PyString::new(py, &v.iter().collect::<String>()).unbind().into());
    }
    Ok(Py::new(py, Value { array: a, fmt })?.into_any())
}

/// A `fractions.Fraction` as a rational. None for anything else; the
/// import is what keeps a look-alike with `numerator`/`denominator`
/// attributes from passing for one.
fn fraction_to_rat(obj: &Bound<'_, PyAny>) -> PyResult<Option<Rat>> {
    let py = obj.py();
    let fraction = py.import("fractions")?.getattr("Fraction")?;
    if !obj.is_instance(&fraction)? {
        return Ok(None);
    }
    let read = |name: &str| -> PyResult<Ext> {
        let text = obj.getattr(name)?.str()?.to_string_lossy().into_owned();
        text.parse::<Ext>()
            .map_err(|_| JayError::new_err(format!("cannot read {text} as an integer")))
    };
    let (num, den) = (read("numerator")?, read("denominator")?);
    Rat::new(num, den)
        .map(Some)
        .ok_or_else(|| JayError::new_err("a Fraction with a zero denominator is not a number"))
}

fn py_to_array(obj: &Bound<'_, PyAny>) -> PyResult<Array> {
    // bool first: PyBool is a PyInt subclass.
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(Array::scalar_bool(b.is_true()));
    }
    if obj.cast::<PyInt>().is_ok() {
        return match obj.extract::<i64>() {
            Ok(v) => Ok(Array::scalar_i64(v)),
            // Python's integers are unbounded; one that does not fit a
            // machine word arrives as J's extended type rather than as a
            // refusal or a rounding.
            Err(_) => {
                let text = obj.str()?.to_string_lossy().into_owned();
                let v: Ext = text.parse().map_err(|_| {
                    JayError::new_err(format!("cannot read {text} as an integer"))
                })?;
                Ok(Array::new(vec![], Data::Ext(vec![v].into())))
            }
        };
    }
    if let Some(r) = fraction_to_rat(obj)? {
        return Ok(Array::new(vec![], Data::Rat(vec![r].into())));
    }
    if obj.cast::<PyFloat>().is_ok() {
        return Ok(Array::scalar_f64(obj.extract::<f64>()?));
    }
    if let Ok(z) = obj.cast::<PyComplex>() {
        return Ok(Array::new(vec![], Data::Complex(vec![[z.real(), z.imag()]].into())));
    }
    if let Ok(s) = obj.cast::<PyString>() {
        let chars: Vec<char> = s.to_str()?.chars().collect();
        return Ok(if chars.len() == 1 {
            Array::new(vec![], Data::Char(chars.into()))
        } else {
            Array::from_chars(chars)
        });
    }
    if let Ok(v) = obj.cast::<Value>() {
        return Ok(v.get().array.clone());
    }
    // A device array is an array; the residency travels with the buffer.
    if let Ok(v) = obj.cast::<DeviceArray>() {
        return Ok(v.get().array.clone());
    }
    if let Some(imported) = data::try_import(obj) {
        return imported;
    }
    if obj.cast::<PyList>().is_ok() || obj.cast::<PyTuple>().is_ok() {
        return sequence_to_array(obj);
    }
    Err(PyTypeError::new_err(format!(
        "cannot pass a {} to J/APL; supported: bool, int, float, str, nested \
         lists, and anything carrying Arrow data (Polars, pandas, PyArrow) or \
         a buffer (numpy)",
        obj.get_type().name()?
    )))
}

/// Stack a Python sequence into one array: a dense one when the items
/// agree on shape and element type, a vector of boxes when they do not —
/// which is what makes a list of strings, or of unequal lists, an argument
/// rather than an error.
fn sequence_to_array(obj: &Bound<'_, PyAny>) -> PyResult<Array> {
    let mut items = Vec::new();
    for item in obj.try_iter()? {
        items.push(py_to_array(&item?)?);
    }
    if items.is_empty() {
        return Ok(Array::empty(DType::I64));
    }
    let cell_shape = items[0].shape.clone();
    let dense = items[1..].iter().all(|it| it.shape == cell_shape)
        && items[1..]
            .iter()
            .try_fold(items[0].dtype(), |t, it| DType::promote(t, it.dtype()))
            .is_some();
    if !dense {
        let n = items.len();
        return Ok(Array::new(vec![n], Data::Box(items.into())));
    }
    let dtype = items
        .iter()
        .try_fold(items[0].dtype(), |t, it| DType::promote(t, it.dtype()))
        .expect("checked above");
    let mut data = Data::empty(dtype);
    for it in &items {
        let cast = it
            .data
            .cast(dtype)
            .ok_or_else(|| JayError::new_err("mixed characters and numbers in one array"))?;
        data.extend_from(&cast);
    }
    let mut shape = vec![items.len()];
    shape.extend_from_slice(&cell_shape);
    Ok(Array::new(shape, data))
}

/// One dialect setting, by name. `None` keeps the shipped value.
///
/// A name this dialect does not have is a value error here; a name it has
/// but libjay does not implement is refused by the compiler instead, with
/// the diagnostic that says so.
fn setting<T: Copy>(value: Option<&str>, field: &str, choices: &[(&str, T)]) -> PyResult<Option<T>> {
    let Some(value) = value else { return Ok(None) };
    for (name, v) in choices {
        if *name == value {
            return Ok(Some(*v));
        }
    }
    let names: Vec<&str> = choices.iter().map(|(n, _)| *n).collect();
    Err(JayError::new_err(format!(
        "unknown {field}: {value:?} (this dialect setting is one of: {})",
        names.join(", ")
    )))
}

/// The dialect the host asked for. Everything unnamed is the shipped
/// default, which is the APL2/ISO line GNU APL verifies.
#[allow(clippy::too_many_arguments)]
fn dialect_of(
    index_origin: Option<i64>,
    comparison_tolerance: Option<f64>,
    nested_model: Option<&str>,
    first_disclose: Option<&str>,
    index_form: Option<&str>,
    partition: Option<&str>,
    depth_sign: Option<&str>,
    dfn_result: Option<&str>,
    default_arg: Option<&str>,
    complex_order: Option<&str>,
    nested_grade: Option<&str>,
    lookup_left: Option<&str>,
    gcd_rule: Option<&str>,
    trains: Option<bool>,
) -> PyResult<Dialect> {
    use jay::frontend::{
        ComplexOrder, DefaultArg, DepthSign, DfnResult, FirstDisclose, GcdRule, IndexForm,
        LookupLeft, NestedGrade, NestedModel, Partition,
    };
    let d = Dialect::default();
    Ok(Dialect {
        index_origin,
        comparison_tolerance,
        nested_model: setting(
            nested_model,
            "nested_model",
            &[("floating", NestedModel::Floating), ("grounded", NestedModel::Grounded)],
        )?
        .unwrap_or(d.nested_model),
        first_disclose: setting(
            first_disclose,
            "first_disclose",
            &[
                ("up-is-first", FirstDisclose::UpIsFirst),
                ("up-is-mix", FirstDisclose::UpIsMix),
            ],
        )?
        .unwrap_or(d.first_disclose),
        index_form: setting(
            index_form,
            "index_form",
            &[
                ("scalar-per-axis", IndexForm::ScalarPerAxis),
                ("axis-vectors", IndexForm::AxisVectors),
            ],
        )?
        .unwrap_or(d.index_form),
        partition: setting(
            partition,
            "partition",
            &[("flags", Partition::Flags), ("counts", Partition::Counts)],
        )?
        .unwrap_or(d.partition),
        depth_sign: setting(
            depth_sign,
            "depth_sign",
            &[("unsigned", DepthSign::Unsigned), ("signed", DepthSign::Signed)],
        )?
        .unwrap_or(d.depth_sign),
        dfn_result: setting(
            dfn_result,
            "dfn_result",
            &[
                ("last-sentence", DfnResult::LastSentence),
                ("first-non-assignment", DfnResult::FirstNonAssignment),
            ],
        )?
        .unwrap_or(d.dfn_result),
        default_arg: setting(
            default_arg,
            "default_arg",
            &[("eager", DefaultArg::Eager), ("lazy", DefaultArg::Lazy)],
        )?
        .unwrap_or(d.default_arg),
        complex_order: setting(
            complex_order,
            "complex_order",
            &[
                ("real-then-imaginary", ComplexOrder::RealThenImaginary),
                ("magnitude-then-angle", ComplexOrder::MagnitudeThenAngle),
            ],
        )?
        .unwrap_or(d.complex_order),
        nested_grade: setting(
            nested_grade,
            "nested_grade",
            &[("apl2", NestedGrade::Apl2), ("total-order", NestedGrade::TotalOrder)],
        )?
        .unwrap_or(d.nested_grade),
        lookup_left: setting(
            lookup_left,
            "lookup_left",
            &[("any-rank", LookupLeft::AnyRank), ("vector-only", LookupLeft::VectorOnly)],
        )?
        .unwrap_or(d.lookup_left),
        gcd_rule: setting(
            gcd_rule,
            "gcd_rule",
            &[("tolerant", GcdRule::Tolerant), ("exact", GcdRule::Exact)],
        )?
        .unwrap_or(d.gcd_rule),
        trains: trains.unwrap_or(d.trains),
    })
}

#[pyfunction]
#[pyo3(signature = (
    lang,
    source,
    index_origin=None,
    comparison_tolerance=None,
    nested_model=None,
    first_disclose=None,
    index_form=None,
    partition=None,
    depth_sign=None,
    dfn_result=None,
    default_arg=None,
    complex_order=None,
    nested_grade=None,
    lookup_left=None,
    gcd_rule=None,
    trains=None,
))]
#[allow(clippy::too_many_arguments)]
fn compile(
    lang: &str,
    source: &str,
    index_origin: Option<i64>,
    comparison_tolerance: Option<f64>,
    nested_model: Option<&str>,
    first_disclose: Option<&str>,
    index_form: Option<&str>,
    partition: Option<&str>,
    depth_sign: Option<&str>,
    dfn_result: Option<&str>,
    default_arg: Option<&str>,
    complex_order: Option<&str>,
    nested_grade: Option<&str>,
    lookup_left: Option<&str>,
    gcd_rule: Option<&str>,
    trains: Option<bool>,
) -> PyResult<Kernel> {
    let lang = parse_lang(lang)?;
    let dialect = dialect_of(
        index_origin,
        comparison_tolerance,
        nested_model,
        first_disclose,
        index_form,
        partition,
        depth_sign,
        dfn_result,
        default_arg,
        complex_order,
        nested_grade,
        lookup_left,
        gcd_rule,
        trains,
    )?;
    let program = jay::compile(lang, source, &dialect).map_err(|e| jay_err(source, &e))?;
    Ok(Kernel { program: Arc::new(program), device: None })
}

/// Every adapter this machine offers, as (name, backend, kind, has f64).
/// Empty where there is no GPU, which is not an error.
#[pyfunction]
fn devices() -> Vec<(String, String, String, bool)> {
    jay::device::available()
        .into_iter()
        .map(|i| (i.name, i.backend, i.kind, i.f64))
        .collect()
}

#[pyfunction]
#[pyo3(signature = (
    lang,
    parts,
    names,
    index_origin=None,
    comparison_tolerance=None,
    nested_model=None,
    first_disclose=None,
    index_form=None,
    partition=None,
    depth_sign=None,
    dfn_result=None,
    default_arg=None,
    complex_order=None,
    nested_grade=None,
    lookup_left=None,
    gcd_rule=None,
    trains=None,
))]
#[allow(clippy::too_many_arguments)]
fn compile_parts(
    lang: &str,
    parts: Vec<String>,
    names: Vec<String>,
    index_origin: Option<i64>,
    comparison_tolerance: Option<f64>,
    nested_model: Option<&str>,
    first_disclose: Option<&str>,
    index_form: Option<&str>,
    partition: Option<&str>,
    depth_sign: Option<&str>,
    dfn_result: Option<&str>,
    default_arg: Option<&str>,
    complex_order: Option<&str>,
    nested_grade: Option<&str>,
    lookup_left: Option<&str>,
    gcd_rule: Option<&str>,
    trains: Option<bool>,
) -> PyResult<Kernel> {
    let lang = parse_lang(lang)?;
    let dialect = dialect_of(
        index_origin,
        comparison_tolerance,
        nested_model,
        first_disclose,
        index_form,
        partition,
        depth_sign,
        dfn_result,
        default_arg,
        complex_order,
        nested_grade,
        lookup_left,
        gcd_rule,
        trains,
    )?;
    let part_refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
    let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let program = jay::compile_parts(lang, &part_refs, &name_refs, &dialect)
        .map_err(|e| {
            // Reconstruct the display source the same way the core does.
            let display = jay::frontend::SourceParts::from_parts(&part_refs, &name_refs).display;
            jay_err(&display, &e)
        })?;
    Ok(Kernel { program: Arc::new(program), device: None })
}

/// How many times a table that crossed the boundary as columns has since
/// had to be copied into one block. A test asserts this does not move.
#[pyfunction]
fn joins_made() -> u64 {
    jay::joins_made()
}

/// How many times a column-major value has had its rows materialised. A
/// test asserts which verbs make this move and which leave it alone.
#[pyfunction]
fn layouts_made() -> u64 {
    jay::layouts_made()
}

#[pymodule]
fn _jay(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("JayError", m.py().get_type::<JayError>())?;
    m.add_class::<Kernel>()?;
    m.add_class::<Value>()?;
    m.add_class::<DeviceArray>()?;
    m.add_function(wrap_pyfunction!(compile, m)?)?;
    m.add_function(wrap_pyfunction!(compile_parts, m)?)?;
    m.add_function(wrap_pyfunction!(devices, m)?)?;
    m.add_function(wrap_pyfunction!(joins_made, m)?)?;
    m.add_function(wrap_pyfunction!(layouts_made, m)?)?;
    Ok(())
}
