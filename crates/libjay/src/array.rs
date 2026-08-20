//! Dense multidimensional array: a shape and a flat row-major buffer.
//!
//! Storage is owned `Vec`s for now; buffer sharing / views arrive with the
//! Arrow boundary and the parallel runtime.

use crate::dtype::DType;

#[derive(Clone, Debug, PartialEq)]
pub enum Data {
    Bool(Vec<u8>),
    I64(Vec<i64>),
    F64(Vec<f64>),
    Char(Vec<char>),
}

impl Data {
    pub fn dtype(&self) -> DType {
        match self {
            Data::Bool(_) => DType::Bool,
            Data::I64(_) => DType::I64,
            Data::F64(_) => DType::F64,
            Data::Char(_) => DType::Char,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Data::Bool(v) => v.len(),
            Data::I64(v) => v.len(),
            Data::F64(v) => v.len(),
            Data::Char(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn slice(&self, start: usize, end: usize) -> Data {
        match self {
            Data::Bool(v) => Data::Bool(v[start..end].to_vec()),
            Data::I64(v) => Data::I64(v[start..end].to_vec()),
            Data::F64(v) => Data::F64(v[start..end].to_vec()),
            Data::Char(v) => Data::Char(v[start..end].to_vec()),
        }
    }

    pub fn empty(dtype: DType) -> Data {
        match dtype {
            DType::Bool => Data::Bool(Vec::new()),
            DType::I64 => Data::I64(Vec::new()),
            DType::F64 => Data::F64(Vec::new()),
            DType::Char => Data::Char(Vec::new()),
        }
    }

    /// The fill element used by overtaking and framing.
    pub fn push_fill(&mut self) {
        match self {
            Data::Bool(v) => v.push(0),
            Data::I64(v) => v.push(0),
            Data::F64(v) => v.push(0.0),
            Data::Char(v) => v.push(' '),
        }
    }

    pub fn extend_from(&mut self, other: &Data) -> bool {
        match (self, other) {
            (Data::Bool(a), Data::Bool(b)) => a.extend_from_slice(b),
            (Data::I64(a), Data::I64(b)) => a.extend_from_slice(b),
            (Data::F64(a), Data::F64(b)) => a.extend_from_slice(b),
            (Data::Char(a), Data::Char(b)) => a.extend_from_slice(b),
            _ => return false,
        }
        true
    }

    /// Widen to `to`. Returns None for unsupported conversions.
    pub fn cast(&self, to: DType) -> Option<Data> {
        if self.dtype() == to {
            return Some(self.clone());
        }
        match (self, to) {
            (Data::Bool(v), DType::I64) => Some(Data::I64(v.iter().map(|&x| x as i64).collect())),
            (Data::Bool(v), DType::F64) => Some(Data::F64(v.iter().map(|&x| x as f64).collect())),
            (Data::I64(v), DType::F64) => Some(Data::F64(v.iter().map(|&x| x as f64).collect())),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Array {
    pub shape: Vec<usize>,
    pub data: Data,
}

impl Array {
    pub fn new(shape: Vec<usize>, data: Data) -> Array {
        debug_assert_eq!(shape.iter().product::<usize>(), data.len());
        Array { shape, data }
    }

    pub fn scalar_i64(v: i64) -> Array {
        Array { shape: vec![], data: Data::I64(vec![v]) }
    }

    pub fn scalar_f64(v: f64) -> Array {
        Array { shape: vec![], data: Data::F64(vec![v]) }
    }

    pub fn scalar_bool(v: bool) -> Array {
        Array { shape: vec![], data: Data::Bool(vec![v as u8]) }
    }

    pub fn from_i64(values: Vec<i64>) -> Array {
        Array { shape: vec![values.len()], data: Data::I64(values) }
    }

    pub fn from_f64(values: Vec<f64>) -> Array {
        Array { shape: vec![values.len()], data: Data::F64(values) }
    }

    pub fn from_chars(values: Vec<char>) -> Array {
        Array { shape: vec![values.len()], data: Data::Char(values) }
    }

    pub fn empty(dtype: DType) -> Array {
        Array { shape: vec![0], data: Data::empty(dtype) }
    }

    pub fn dtype(&self) -> DType {
        self.data.dtype()
    }

    pub fn rank(&self) -> usize {
        self.shape.len()
    }

    /// Total number of elements.
    pub fn count(&self) -> usize {
        self.shape.iter().product()
    }

    /// Number of items (major cells): leading axis length, 1 for a scalar.
    pub fn items(&self) -> usize {
        self.shape.first().copied().unwrap_or(1)
    }

    /// Elements per item.
    pub fn item_size(&self) -> usize {
        self.shape.iter().skip(1).product()
    }

    pub fn cast(&self, to: DType) -> Option<Array> {
        Some(Array { shape: self.shape.clone(), data: self.data.cast(to)? })
    }

    /// Split into cells: the trailing `cell_rank` axes form the cell shape,
    /// the leading axes form the frame. Returns owned copies.
    pub fn cells(&self, frame_rank: usize) -> Vec<Array> {
        debug_assert!(frame_rank <= self.rank());
        let cell_shape: Vec<usize> = self.shape[frame_rank..].to_vec();
        let cell_size: usize = cell_shape.iter().product();
        let n: usize = self.shape[..frame_rank].iter().product();
        (0..n)
            .map(|i| Array {
                shape: cell_shape.clone(),
                data: self.data.slice(i * cell_size, (i + 1) * cell_size),
            })
            .collect()
    }

    /// One cell without materialising all of them.
    pub fn cell_at(&self, frame_rank: usize, index: usize) -> Array {
        let cell_shape: Vec<usize> = self.shape[frame_rank..].to_vec();
        let cell_size: usize = cell_shape.iter().product();
        Array {
            shape: cell_shape,
            data: self.data.slice(index * cell_size, (index + 1) * cell_size),
        }
    }

    /// Item `i` (major cell along the leading axis).
    pub fn item(&self, i: usize) -> Array {
        debug_assert!(self.rank() >= 1);
        self.cell_at(1, i)
    }

    pub fn as_i64_slice(&self) -> Option<&[i64]> {
        match &self.data {
            Data::I64(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_f64_slice(&self) -> Option<&[f64]> {
        match &self.data {
            Data::F64(v) => Some(v),
            _ => None,
        }
    }

    /// Numeric contents widened to f64. None for character data.
    pub fn to_f64_vec(&self) -> Option<Vec<f64>> {
        match &self.data {
            Data::Bool(v) => Some(v.iter().map(|&x| x as f64).collect()),
            Data::I64(v) => Some(v.iter().map(|&x| x as f64).collect()),
            Data::F64(v) => Some(v.clone()),
            Data::Char(_) => None,
        }
    }

    /// Numeric contents as i64 if exactly representable.
    pub fn to_i64_vec(&self) -> Option<Vec<i64>> {
        match &self.data {
            Data::Bool(v) => Some(v.iter().map(|&x| x as i64).collect()),
            Data::I64(v) => Some(v.clone()),
            Data::F64(v) => {
                let mut out = Vec::with_capacity(v.len());
                for &x in v {
                    if x.fract() != 0.0 || x.abs() >= i64::MAX as f64 {
                        return None;
                    }
                    out.push(x as i64);
                }
                Some(out)
            }
            Data::Char(_) => None,
        }
    }
}
