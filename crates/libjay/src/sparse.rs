//! Sparse arrays: the storage kind behind J's `$.`.
//!
//! A sparse array has the shape of the array it stands for and holds only
//! the positions that differ from one repeated element — the SPARSE
//! ELEMENT, which is zero for anything [`sparsify`] makes. Some of the axes
//! are stored sparsely and the rest are dense, so one stored entry is a
//! whole cell over the dense axes: an index row naming the entry's position
//! along the sparse axes, and that cell's elements. When every axis is
//! sparse — what `$. y` always produces — the cell is a single element and
//! the array is the familiar list of coordinates and values.
//!
//! The shape and the stored values live on the [`Array`] itself: `shape` is
//! the LOGICAL shape and `data` is the stored cells end to end, so an array
//! of sparse doubles reports the same dtype and formats its values through
//! the same code a dense one does. Everything else is in [`Sparse`], which
//! the array carries behind an `Arc`.
//!
//! Only `$.` itself, the display and `":` read the stored form. Every other
//! verb receives [`Array::densified`], which is semantically exact and says
//! nothing about how fast it is; the caveat is written down in
//! docs/status.md.

use std::sync::Arc;

use crate::array::{Array, Data};
use crate::complex::Cx;
use crate::dtype::DType;
use crate::error::{Error, ErrorKind, Result, Span};
use crate::exact::{Ext, Rat};

/// What a sparse array holds besides its shape and its stored cells.
#[derive(Clone, Debug, PartialEq)]
pub struct Sparse {
    /// The axes stored sparsely, ascending and distinct. Every other axis
    /// of the shape is dense and forms the stored cell.
    pub axes: Vec<usize>,
    /// One row per stored entry, `axes.len()` columns, row-major: entry
    /// `e`'s position along sparse axis `axes[j]` is `indices[e * k + j]`.
    pub indices: Vec<usize>,
    /// The element every position not named by `indices` holds. Exactly one
    /// element, of the array's own dtype.
    pub fill: Data,
    /// Stored entries. It is not derivable from the buffers when there are
    /// no sparse axes at all, or when a dense axis has length zero.
    pub entries: usize,
}

impl Sparse {
    /// The shape of one stored cell: the lengths of the axes that are not
    /// sparse, in axis order.
    pub fn cell_shape(&self, shape: &[usize]) -> Vec<usize> {
        shape
            .iter()
            .enumerate()
            .filter(|(k, _)| !self.axes.contains(k))
            .map(|(_, &n)| n)
            .collect()
    }

    /// Elements in one stored cell.
    pub fn cell_size(&self, shape: &[usize]) -> usize {
        self.cell_shape(shape).iter().product()
    }
}

/// Where each stored entry and each element of its cell lands in the dense
/// buffer. Worked out once per expansion, then read by whichever element
/// type the array holds.
struct Plan {
    /// The offset of entry `e`'s cell origin.
    bases: Vec<usize>,
    /// The offset of each element within a cell, relative to that origin.
    cell: Vec<usize>,
}

fn plan(shape: &[usize], s: &Sparse) -> Plan {
    let rank = shape.len();
    let mut strides = vec![1usize; rank];
    for k in (0..rank.saturating_sub(1)).rev() {
        strides[k] = strides[k + 1] * shape[k + 1];
    }
    let k = s.axes.len();
    let bases = (0..s.entries)
        .map(|e| {
            (0..k).map(|j| s.indices[e * k + j] * strides[s.axes[j]]).sum::<usize>()
        })
        .collect();
    // The dense axes, walked as an odometer, give the offsets inside a cell
    // in the order the cell's own elements are stored.
    let dense: Vec<usize> = (0..rank).filter(|k| !s.axes.contains(k)).collect();
    let mut cell = Vec::with_capacity(s.cell_size(shape));
    let mut coord = vec![0usize; dense.len()];
    let cells = s.cell_size(shape);
    for _ in 0..cells {
        cell.push(coord.iter().zip(&dense).map(|(&c, &ax)| c * strides[ax]).sum());
        let mut j = dense.len();
        while j > 0 {
            j -= 1;
            coord[j] += 1;
            if coord[j] < shape[dense[j]] {
                break;
            }
            coord[j] = 0;
        }
    }
    Plan { bases, cell }
}

/// The dense buffer of an array whose stored cells are `values`.
fn expand<T: Clone>(values: &[T], fill: &T, count: usize, p: &Plan) -> Vec<T> {
    let mut out = vec![fill.clone(); count];
    let width = p.cell.len();
    for (e, &base) in p.bases.iter().enumerate() {
        for (c, &off) in p.cell.iter().enumerate() {
            out[base + off] = values[e * width + c].clone();
        }
    }
    out
}

/// The array `a`, which must be sparse, with every position materialised.
pub(crate) fn densify(a: &Array, s: &Sparse) -> Array {
    let count: usize = a.shape.iter().product();
    let p = plan(&a.shape, s);
    macro_rules! by {
        ($($variant:ident),*) => {
            match (&a.data, &s.fill) {
                $((Data::$variant(v), Data::$variant(f)) => {
                    Data::$variant(expand(v, &f[0], count, &p).into())
                })*
                // A sparse array is built in one place and always carries a
                // fill of its own dtype.
                _ => Data::empty(a.dtype()),
            }
        };
    }
    let data = by!(Bool, I64, Ext, Rat, F64, Complex, Char, Symbol, Box);
    Array::new(a.shape.clone(), data)
}

/// The positions of `a` that differ from zero, in ravel order.
fn nonzero(a: &Array) -> Vec<usize> {
    fn of<T: PartialEq>(v: &[T], zero: T) -> Vec<usize> {
        v.iter().enumerate().filter(|(_, x)| **x != zero).map(|(i, _)| i).collect()
    }
    match &a.data {
        Data::Bool(v) => of(v, 0),
        Data::I64(v) => of(v, 0),
        Data::F64(v) => of(v, 0.0),
        Data::Complex(v) => of(v, crate::complex::ZERO),
        _ => Vec::new(),
    }
}

/// One zero of this dtype, as the sparse element of a converted array.
fn zero_of(dtype: DType) -> Data {
    match dtype {
        DType::Bool => Data::Bool(vec![0u8].into()),
        DType::I64 => Data::I64(vec![0i64].into()),
        DType::F64 => Data::F64(vec![0.0f64].into()),
        DType::Complex => Data::Complex(vec![crate::complex::ZERO].into()),
        DType::Ext => Data::Ext(vec![Ext::default()].into()),
        DType::Rat => Data::Rat(vec![Rat::zero()].into()),
        DType::Char => Data::Char(vec![' '].into()),
        DType::Symbol => Data::Symbol(vec![crate::symbol::EMPTY].into()),
        DType::Box => Data::Box(vec![Array::box_fill()].into()),
    }
}

/// The element types that can be stored sparsely. J has a code for sparse
/// characters and sparse boxes and refuses to make either; the exact types
/// have no sparse form at all.
fn check_storable(a: &Array, span: Span) -> Result<()> {
    match a.dtype() {
        DType::Bool | DType::I64 | DType::F64 | DType::Complex => Ok(()),
        DType::Char | DType::Box | DType::Symbol => Err(Error::not_yet(
            format!("a sparse array of {}", a.dtype().name()),
            span,
        )),
        DType::Ext | DType::Rat => Err(Error::domain(
            format!("{} has no sparse form", a.dtype().name()),
            span,
        )),
    }
}

/// `$. y`: the dense array `y` in sparse form, every axis sparse and zero
/// the sparse element. A sparse argument comes back unchanged, and a scalar
/// stays dense — there is no axis to store it along.
pub fn sparsify(y: &Array, span: Span) -> Result<Array> {
    if y.is_sparse() {
        return Ok(y.clone());
    }
    let y = y.to_row_major();
    if y.rank() == 0 {
        return Ok(y);
    }
    check_storable(&y, span)?;
    let rank = y.rank();
    let mut strides = vec![1usize; rank];
    for k in (0..rank - 1).rev() {
        strides[k] = strides[k + 1] * y.shape[k + 1];
    }
    let at = nonzero(&y);
    let mut indices = Vec::with_capacity(at.len() * rank);
    let mut values = Data::empty(y.dtype());
    for &i in &at {
        let mut rest = i;
        for &stride in &strides {
            indices.push(rest / stride);
            rest %= stride;
        }
        values.push_from(&y.data, i);
    }
    let s = Sparse {
        axes: (0..rank).collect(),
        indices,
        fill: zero_of(y.dtype()),
        entries: at.len(),
    };
    Ok(Array::sparse(y.shape.clone(), values, s))
}

/// `1 $. y`: a new sparse array with nothing stored in it. `y` is the
/// shape, or a boxed `shape ; axes`, or a boxed `shape ; axes ; element`.
/// Left to itself the whole shape is sparse and the element is a float
/// zero, which is what J's own bare form gives.
pub fn create(y: &Array, span: Span) -> Result<Array> {
    let parts: Vec<Array> = match y.as_boxes() {
        Some(b) if y.rank() <= 1 => b.iter().map(|a| a.densified()).collect(),
        _ => vec![y.densified()],
    };
    if parts.is_empty() || parts.len() > 3 {
        return Err(Error::new(
            ErrorKind::Length,
            "a sparse array is made from a shape, or a shape and its sparse axes, or those and the element that fills it",
            Some(span),
        ));
    }
    let shape = axis_lengths(&parts[0], span)?;
    let rank = shape.len();
    let axes = match parts.get(1) {
        None => (0..rank).collect(),
        Some(a) => sparse_axes(a, rank, span)?,
    };
    let fill = match parts.get(2) {
        // J's own default is a floating-point zero, so a bare `1 $. shape`
        // is a sparse array of doubles.
        None => Data::F64(vec![0.0].into()),
        Some(a) => {
            if a.rank() != 0 {
                return Err(Error::new(
                    ErrorKind::Rank,
                    "the element that fills a sparse array is one atom",
                    Some(span),
                ));
            }
            a.data.slice(0, 1)
        }
    };
    let empty = Array::new(vec![0], fill.slice(0, 0));
    check_storable(&empty, span)?;
    // The dense expansion is what every other verb will ask for, so a shape
    // too large to hold is refused here rather than at the first use.
    crate::limits::elements(&shape, span)?;
    let s = Sparse { axes, indices: Vec::new(), fill, entries: 0 };
    Ok(Array::sparse(shape, Data::empty(empty.dtype()), s))
}

/// A shape argument: an atom or a list of non-negative axis lengths.
fn axis_lengths(a: &Array, span: Span) -> Result<Vec<usize>> {
    if a.rank() > 1 {
        return Err(Error::new(ErrorKind::Rank, "a shape is a list, not a table", Some(span)));
    }
    let Some(v) = a.to_i64_vec() else {
        return Err(Error::domain("a shape is made of integers", span));
    };
    if v.is_empty() {
        return Err(Error::new(
            ErrorKind::Length,
            "a sparse array needs at least one axis",
            Some(span),
        ));
    }
    let mut shape = Vec::with_capacity(v.len());
    for n in v {
        if n < 0 {
            return Err(Error::domain("an axis length cannot be negative", span));
        }
        shape.push(n as usize);
    }
    Ok(shape)
}

/// The sparse-axis list: distinct axes of the shape, put in ascending order.
fn sparse_axes(a: &Array, rank: usize, span: Span) -> Result<Vec<usize>> {
    if a.rank() > 1 {
        return Err(Error::new(
            ErrorKind::Rank,
            "the sparse axes are a list, not a table",
            Some(span),
        ));
    }
    let Some(v) = a.to_i64_vec() else {
        return Err(Error::domain("the sparse axes are integers", span));
    };
    let mut axes: Vec<usize> = Vec::with_capacity(v.len());
    for k in v {
        if k < 0 || k as usize >= rank || axes.contains(&(k as usize)) {
            return Err(Error::new(
                ErrorKind::Domain,
                format!("{k} is not an axis of a rank-{rank} array, or names one twice"),
                Some(span),
            ));
        }
        axes.push(k as usize);
    }
    axes.sort_unstable();
    Ok(axes)
}

/// `8 $. y`: the same array with every stored entry whose cell is entirely
/// the sparse element dropped. Amending a stored position back to the fill
/// leaves the entry behind; this is what removes it.
pub fn compress(a: &Array, s: &Sparse) -> Array {
    let width = s.cell_size(&a.shape);
    let k = s.axes.len();
    let keep: Vec<usize> = (0..s.entries)
        .filter(|&e| (0..width).any(|c| !same_as_fill(&a.data, e * width + c, &s.fill)))
        .collect();
    let mut indices = Vec::with_capacity(keep.len() * k);
    let mut values = Data::empty(a.dtype());
    for &e in &keep {
        indices.extend_from_slice(&s.indices[e * k..(e + 1) * k]);
        for c in 0..width {
            values.push_from(&a.data, e * width + c);
        }
    }
    let out = Sparse { axes: s.axes.clone(), indices, fill: s.fill.clone(), entries: keep.len() };
    Array::sparse(a.shape.clone(), values, out)
}

/// Whether element `i` of `data` is the sparse element `fill` holds.
fn same_as_fill(data: &Data, i: usize, fill: &Data) -> bool {
    fn at<T: Clone + PartialEq>(v: &[T], i: usize, f: &[T]) -> bool {
        v[i] == f[0]
    }
    match (data, fill) {
        (Data::Bool(v), Data::Bool(f)) => at(v, i, f),
        (Data::I64(v), Data::I64(f)) => at(v, i, f),
        (Data::F64(v), Data::F64(f)) => at(v, i, f),
        (Data::Complex(v), Data::Complex(f)) => at::<Cx>(v, i, f),
        _ => false,
    }
}

/// The stored cells as an ordinary array: one leading axis of entries, then
/// the cell's own shape. This is `5 $. y`.
pub fn values_of(a: &Array, s: &Sparse) -> Array {
    let mut shape = vec![s.entries];
    shape.extend(s.cell_shape(&a.shape));
    Array::new(shape, a.data.clone())
}

/// The stored index rows as an integer table: one row per entry, one column
/// per sparse axis. This is `4 $. y`.
pub fn indices_of(s: &Sparse) -> Array {
    let values: Vec<i64> = s.indices.iter().map(|&i| i as i64).collect();
    Array::new(vec![s.entries, s.axes.len()], Data::I64(values.into()))
}

/// The shape, the sparse axes and the sparse element, each boxed. This is
/// `_1 $. y`.
pub fn attributes(a: &Array, s: &Sparse) -> Array {
    let shape = Array::from_i64(a.shape.iter().map(|&n| n as i64).collect());
    let axes = Array::from_i64(s.axes.iter().map(|&k| k as i64).collect());
    let fill = Array::new(vec![], s.fill.clone());
    Array::new(vec![3], Data::Box(vec![shape, axes, fill].into()))
}

/// The sparse element on its own, as an atom. This is `3 $. y`.
pub fn fill_of(s: &Sparse) -> Array {
    Array::new(vec![], s.fill.clone())
}

/// A sparse array carried behind an `Arc`, which is what the array itself
/// holds so that cloning a sparse value stays a refcount bump.
pub(crate) type Handle = Arc<Sparse>;
