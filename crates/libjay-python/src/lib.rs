//! Python binding. Exposes compile/run over the core crate; the friendly
//! layer (kernels with defaults, t-strings, the CLI) lives in Python.

mod data;

use std::sync::Arc;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyCapsule, PyFloat, PyInt, PyList, PyString, PyTuple};

use jay::fmt::{format_array, FmtOpts};
use jay::{Array, Data, DType, Dialect, Lang};

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
}

#[pymethods]
impl Kernel {
    #[getter]
    fn params(&self) -> Vec<String> {
        self.program.params.iter().map(|p| p.name.clone()).collect()
    }

    /// Run with positional values (one per parameter). `out` receives
    /// explicit output (echo / ⎕←). Returns (result, display) where
    /// `display` is the formatted last value when `want_display` is set.
    fn run(
        &self,
        py: Python<'_>,
        values: Vec<Bound<'_, PyAny>>,
        out: Bound<'_, PyAny>,
        want_display: bool,
    ) -> PyResult<(PyObject, Option<String>)> {
        let args: Vec<Array> =
            values.iter().map(py_to_array).collect::<PyResult<_>>()?;
        let mut write_err: Option<PyErr> = None;
        let mut sink = |s: &str| {
            if write_err.is_none() {
                if let Err(e) = out.call1((s,)) {
                    write_err = Some(e);
                }
            }
        };
        let result = self.program.run(&args, &mut sink);
        if let Some(e) = write_err {
            return Err(e);
        }
        let result = result.map_err(|e| jay_err(&self.program.display_src, &e))?;
        let display = if want_display {
            result.as_ref().map(|a| format_array(a, &self.program.fmt)).filter(|s| !s.is_empty())
        } else {
            None
        };
        let value = match result {
            None => py.None(),
            Some(a) => array_to_py(py, a, self.program.fmt)?,
        };
        Ok((value, display))
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
    fn shape(&self, py: Python<'_>) -> PyResult<PyObject> {
        Ok(PyTuple::new(py, &self.array.shape)?.unbind().into())
    }

    #[getter]
    fn dtype(&self) -> &'static str {
        self.array.dtype().name()
    }

    fn __len__(&self) -> usize {
        self.array.items()
    }

    fn tolist(&self, py: Python<'_>) -> PyResult<PyObject> {
        nested_list(py, &self.array, 0, 0, self.array.count())
    }

    fn __repr__(&self) -> String {
        format_array(&self.array, &self.fmt)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.downcast::<Value>() {
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
) -> PyResult<PyObject> {
    if axis == a.rank() {
        return element_to_py(py, &a.data, start);
    }
    let n = a.shape[axis];
    let stride = (end - start) / n.max(1);
    let items: Vec<PyObject> = (0..n)
        .map(|i| nested_list(py, a, axis + 1, start + i * stride, start + (i + 1) * stride))
        .collect::<PyResult<_>>()?;
    Ok(PyList::new(py, items)?.unbind().into())
}

fn element_to_py(py: Python<'_>, data: &Data, i: usize) -> PyResult<PyObject> {
    Ok(match data {
        Data::Bool(v) => (v[i] != 0).into_pyobject(py)?.to_owned().unbind().into(),
        Data::I64(v) => v[i].into_pyobject(py)?.unbind().into(),
        Data::F64(v) => v[i].into_pyobject(py)?.unbind().into(),
        Data::Char(v) => v[i].to_string().into_pyobject(py)?.unbind().into(),
    })
}

fn array_to_py(py: Python<'_>, a: Array, fmt: FmtOpts) -> PyResult<PyObject> {
    if a.rank() == 0 {
        return element_to_py(py, &a.data, 0);
    }
    if a.rank() == 1 {
        if let Data::Char(v) = &a.data {
            return Ok(PyString::new(py, &v.iter().collect::<String>()).unbind().into());
        }
    }
    Ok(Py::new(py, Value { array: a, fmt })?.into_any())
}

fn py_to_array(obj: &Bound<'_, PyAny>) -> PyResult<Array> {
    // bool first: PyBool is a PyInt subclass.
    if let Ok(b) = obj.downcast::<PyBool>() {
        return Ok(Array::scalar_bool(b.is_true()));
    }
    if obj.downcast::<PyInt>().is_ok() {
        return match obj.extract::<i64>() {
            Ok(v) => Ok(Array::scalar_i64(v)),
            Err(_) => Err(JayError::new_err(
                "integer does not fit 64 bits; \
                 extended-precision integers are not supported yet",
            )),
        };
    }
    if obj.downcast::<PyFloat>().is_ok() {
        return Ok(Array::scalar_f64(obj.extract::<f64>()?));
    }
    if let Ok(s) = obj.downcast::<PyString>() {
        let chars: Vec<char> = s.to_str()?.chars().collect();
        return Ok(if chars.len() == 1 {
            Array::new(vec![], Data::Char(chars.into()))
        } else {
            Array::from_chars(chars)
        });
    }
    if let Ok(v) = obj.downcast::<Value>() {
        return Ok(v.get().array.clone());
    }
    if let Some(imported) = data::try_import(obj) {
        return imported;
    }
    if obj.downcast::<PyList>().is_ok() || obj.downcast::<PyTuple>().is_ok() {
        return sequence_to_array(obj);
    }
    Err(PyTypeError::new_err(format!(
        "cannot pass a {} to J/APL; supported: bool, int, float, str, nested \
         lists, and anything carrying Arrow data (Polars, pandas, PyArrow) or \
         a buffer (numpy)",
        obj.get_type().name()?
    )))
}

/// Stack a Python sequence of equally-shaped values into one array.
fn sequence_to_array(obj: &Bound<'_, PyAny>) -> PyResult<Array> {
    let mut items = Vec::new();
    for item in obj.try_iter()? {
        items.push(py_to_array(&item?)?);
    }
    if items.is_empty() {
        return Ok(Array::empty(DType::I64));
    }
    let cell_shape = items[0].shape.clone();
    let mut dtype = items[0].dtype();
    for it in &items[1..] {
        if it.shape != cell_shape {
            return Err(JayError::new_err(format!(
                "ragged nested list: an item of shape {:?} next to shape {:?}",
                it.shape, cell_shape
            )));
        }
        dtype = DType::promote(dtype, it.dtype()).ok_or_else(|| {
            JayError::new_err("mixed characters and numbers in one array")
        })?;
    }
    let mut data = Data::empty(dtype);
    for it in &items {
        let cast = it.data.cast(dtype).ok_or_else(|| {
            JayError::new_err("mixed characters and numbers in one array")
        })?;
        data.extend_from(&cast);
    }
    let mut shape = vec![items.len()];
    shape.extend_from_slice(&cell_shape);
    Ok(Array::new(shape, data))
}

#[pyfunction]
#[pyo3(signature = (lang, source, index_origin=None))]
fn compile(lang: &str, source: &str, index_origin: Option<i64>) -> PyResult<Kernel> {
    let lang = parse_lang(lang)?;
    let dialect = Dialect { index_origin };
    let program = jay::compile(lang, source, &dialect).map_err(|e| jay_err(source, &e))?;
    Ok(Kernel { program: Arc::new(program) })
}

#[pyfunction]
#[pyo3(signature = (lang, parts, names, index_origin=None))]
fn compile_parts(
    lang: &str,
    parts: Vec<String>,
    names: Vec<String>,
    index_origin: Option<i64>,
) -> PyResult<Kernel> {
    let lang = parse_lang(lang)?;
    let dialect = Dialect { index_origin };
    let part_refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
    let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let program = jay::compile_parts(lang, &part_refs, &name_refs, &dialect)
        .map_err(|e| {
            // Reconstruct the display source the same way the core does.
            let display = jay::frontend::SourceParts::from_parts(&part_refs, &name_refs).display;
            jay_err(&display, &e)
        })?;
    Ok(Kernel { program: Arc::new(program) })
}

#[pymodule]
fn _jay(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("JayError", m.py().get_type::<JayError>())?;
    m.add_class::<Kernel>()?;
    m.add_class::<Value>()?;
    m.add_function(wrap_pyfunction!(compile, m)?)?;
    m.add_function(wrap_pyfunction!(compile_parts, m)?)?;
    Ok(())
}
