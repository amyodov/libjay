//! Dense multidimensional array: a shape and a flat row-major buffer.
//!
//! Buffers are either owned or borrowed from foreign memory (Arrow, the
//! Python buffer protocol) through [`Buf`], which is what makes the data
//! boundary zero-copy.

use std::any::Any;
use std::ops::Deref;
use std::sync::Arc;

use crate::dtype::DType;

/// Anything that keeps a foreign buffer's memory alive: the importing side
/// stores its release guards here and the buffer outlives nothing else.
pub type Owner = Arc<dyn Any + Send + Sync>;

/// A flat element buffer, owned or borrowed.
///
/// A borrowed (foreign) buffer points into memory owned by someone else — an
/// Arrow C data interface import, a Python buffer — and holds an `Owner`
/// handle that keeps that memory alive for at least as long as the buffer.
/// An owned buffer is refcounted. Either way cloning is a refcount bump and
/// mutation copies first if the memory is shared or foreign ([`Buf::to_mut`]),
/// so a `Buf` behaves as a private value however cheaply it was cloned.
pub struct Buf<T> {
    repr: Repr<T>,
}

enum Repr<T> {
    Owned(Arc<Vec<T>>),
    Foreign { ptr: *const T, len: usize, owner: Owner },
}

// SAFETY: neither variant hands out aliased mutable access. A foreign buffer
// is read-only for its whole life and its `owner` keeps the memory alive; an
// owned buffer shares its `Vec` through an `Arc` and only ever mutates it
// through `Arc::make_mut`, which copies unless this buffer is the sole
// holder. So `Buf` is exactly as shareable as the `&[T]` it derefs to —
// which, because an owned buffer is an `Arc<Vec<T>>` that may be dropped or
// read from any thread holding a clone, needs `T: Send + Sync` on both.
unsafe impl<T: Send + Sync> Send for Buf<T> {}
// SAFETY: as above; `&Buf<T>` only ever hands out `&[T]`.
unsafe impl<T: Send + Sync> Sync for Buf<T> {}

impl<T> Buf<T> {
    pub fn new() -> Buf<T> {
        Buf { repr: Repr::Owned(Arc::new(Vec::new())) }
    }

    pub fn from_vec(v: Vec<T>) -> Buf<T> {
        Buf { repr: Repr::Owned(Arc::new(v)) }
    }

    /// Borrow `len` elements at `ptr`, keeping `owner` alive alongside them.
    ///
    /// # Safety
    ///
    /// `ptr` must be aligned for `T` and point to `len` initialised elements
    /// that stay valid, and are not mutated by anyone, for as long as `owner`
    /// is alive. `len == 0` accepts a dangling `ptr`.
    pub unsafe fn foreign(ptr: *const T, len: usize, owner: Owner) -> Buf<T> {
        Buf { repr: Repr::Foreign { ptr, len, owner } }
    }

    /// True while the buffer still borrows foreign memory.
    pub fn is_foreign(&self) -> bool {
        matches!(self.repr, Repr::Foreign { .. })
    }

    pub fn as_slice(&self) -> &[T] {
        match &self.repr {
            Repr::Owned(v) => v,
            Repr::Foreign { ptr, len, .. } => {
                if *len == 0 {
                    &[]
                } else {
                    // SAFETY: the `foreign` contract guarantees `len`
                    // initialised, aligned, immutable elements at `ptr`, kept
                    // alive by the owner this buffer holds.
                    unsafe { std::slice::from_raw_parts(*ptr, *len) }
                }
            }
        }
    }
}

impl<T: Clone> Buf<T> {
    /// The buffer as a uniquely owned `Vec`, copying once if it is foreign or
    /// shared with another holder. Subsequent calls on the same buffer are
    /// free until it is cloned again.
    pub fn to_mut(&mut self) -> &mut Vec<T> {
        if self.is_foreign() {
            self.repr = Repr::Owned(Arc::new(self.as_slice().to_vec()));
        }
        match &mut self.repr {
            Repr::Owned(v) => Arc::make_mut(v),
            Repr::Foreign { .. } => unreachable!("just converted to owned"),
        }
    }

    /// The contents as a `Vec`, moving it out when this buffer is the sole
    /// holder and copying otherwise.
    pub fn into_vec(self) -> Vec<T> {
        match self.repr {
            Repr::Owned(v) => Arc::try_unwrap(v).unwrap_or_else(|v| v.as_slice().to_vec()),
            Repr::Foreign { .. } => self.as_slice().to_vec(),
        }
    }

    pub fn push(&mut self, value: T) {
        self.to_mut().push(value);
    }

    pub fn extend_from_slice(&mut self, other: &[T]) {
        self.to_mut().extend_from_slice(other);
    }

    /// Elements `[start, end)`. Free for a foreign buffer (the slice keeps
    /// borrowing, sharing the same owner), a copy for an owned one.
    pub fn slice(&self, start: usize, end: usize) -> Buf<T> {
        match &self.repr {
            Repr::Owned(v) => Buf::from_vec(v[start..end].to_vec()),
            Repr::Foreign { ptr, len, owner } => {
                assert!(start <= end && end <= *len, "slice out of range");
                // SAFETY: `start <= len` keeps the offset inside the same
                // allocation; the new buffer holds a clone of the owner.
                unsafe { Buf::foreign(ptr.add(start), end - start, owner.clone()) }
            }
        }
    }
}

impl<T> Deref for Buf<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

/// Cloning never copies elements: both variants are a refcount bump, and the
/// copy happens later, in [`Buf::to_mut`], only if someone writes while the
/// memory is still shared.
impl<T: Clone> Clone for Buf<T> {
    fn clone(&self) -> Buf<T> {
        match &self.repr {
            Repr::Owned(v) => Buf { repr: Repr::Owned(Arc::clone(v)) },
            Repr::Foreign { ptr, len, owner } => {
                // SAFETY: same pointer, same owner, same guarantees.
                unsafe { Buf::foreign(*ptr, *len, owner.clone()) }
            }
        }
    }
}

impl<T> Default for Buf<T> {
    fn default() -> Buf<T> {
        Buf::new()
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Buf<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_slice(), f)
    }
}

impl<T: PartialEq> PartialEq for Buf<T> {
    fn eq(&self, other: &Buf<T>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T> From<Vec<T>> for Buf<T> {
    fn from(v: Vec<T>) -> Buf<T> {
        Buf::from_vec(v)
    }
}

impl<'a, T> IntoIterator for &'a Buf<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> std::slice::Iter<'a, T> {
        self.as_slice().iter()
    }
}

impl<T> FromIterator<T> for Buf<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Buf<T> {
        Buf::from_vec(Vec::from_iter(iter))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Data {
    Bool(Buf<u8>),
    I64(Buf<i64>),
    F64(Buf<f64>),
    Char(Buf<char>),
    /// Boxes: every element is a whole array. Foreign memory never holds
    /// these, so a boxed buffer is always owned and cloning it is a
    /// refcount bump like any other.
    Box(Buf<Array>),
}

impl Data {
    pub fn dtype(&self) -> DType {
        match self {
            Data::Bool(_) => DType::Bool,
            Data::I64(_) => DType::I64,
            Data::F64(_) => DType::F64,
            Data::Char(_) => DType::Char,
            Data::Box(_) => DType::Box,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Data::Bool(v) => v.len(),
            Data::I64(v) => v.len(),
            Data::F64(v) => v.len(),
            Data::Char(v) => v.len(),
            Data::Box(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True while the payload still borrows foreign memory.
    pub fn is_foreign(&self) -> bool {
        match self {
            Data::Bool(v) => v.is_foreign(),
            Data::I64(v) => v.is_foreign(),
            Data::F64(v) => v.is_foreign(),
            Data::Char(v) => v.is_foreign(),
            Data::Box(v) => v.is_foreign(),
        }
    }

    pub fn slice(&self, start: usize, end: usize) -> Data {
        match self {
            Data::Bool(v) => Data::Bool(v.slice(start, end)),
            Data::I64(v) => Data::I64(v.slice(start, end)),
            Data::F64(v) => Data::F64(v.slice(start, end)),
            Data::Char(v) => Data::Char(v.slice(start, end)),
            Data::Box(v) => Data::Box(v.slice(start, end)),
        }
    }

    pub fn empty(dtype: DType) -> Data {
        match dtype {
            DType::Bool => Data::Bool(Buf::new()),
            DType::I64 => Data::I64(Buf::new()),
            DType::F64 => Data::F64(Buf::new()),
            DType::Char => Data::Char(Buf::new()),
            DType::Box => Data::Box(Buf::new()),
        }
    }

    /// The fill element used by overtaking and framing. The boxed fill is
    /// J's `a:`, a box holding an empty numeric list.
    pub fn push_fill(&mut self) {
        match self {
            Data::Bool(v) => v.push(0),
            Data::I64(v) => v.push(0),
            Data::F64(v) => v.push(0.0),
            Data::Char(v) => v.push(' '),
            Data::Box(v) => v.push(Array::box_fill()),
        }
    }

    pub fn extend_from(&mut self, other: &Data) -> bool {
        match (self, other) {
            (Data::Bool(a), Data::Bool(b)) => a.extend_from_slice(b),
            (Data::I64(a), Data::I64(b)) => a.extend_from_slice(b),
            (Data::F64(a), Data::F64(b)) => a.extend_from_slice(b),
            (Data::Char(a), Data::Char(b)) => a.extend_from_slice(b),
            (Data::Box(a), Data::Box(b)) => a.extend_from_slice(b),
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
        Array { shape: vec![], data: Data::I64(vec![v].into()) }
    }

    pub fn scalar_f64(v: f64) -> Array {
        Array { shape: vec![], data: Data::F64(vec![v].into()) }
    }

    pub fn scalar_bool(v: bool) -> Array {
        Array { shape: vec![], data: Data::Bool(vec![v as u8].into()) }
    }

    pub fn from_i64(values: Vec<i64>) -> Array {
        Array { shape: vec![values.len()], data: Data::I64(values.into()) }
    }

    pub fn from_f64(values: Vec<f64>) -> Array {
        Array { shape: vec![values.len()], data: Data::F64(values.into()) }
    }

    pub fn from_chars(values: Vec<char>) -> Array {
        Array { shape: vec![values.len()], data: Data::Char(values.into()) }
    }

    pub fn empty(dtype: DType) -> Array {
        Array { shape: vec![0], data: Data::empty(dtype) }
    }

    /// `y` as a scalar box (J `<`).
    pub fn boxed(value: Array) -> Array {
        Array { shape: vec![], data: Data::Box(vec![value].into()) }
    }

    /// The element that fills a boxed array: J's `a:`, a box holding an
    /// empty numeric list.
    pub fn box_fill() -> Array {
        Array::empty(DType::I64)
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
    /// the leading axes form the frame.
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

    /// The boxed elements, if the array holds boxes.
    pub fn as_boxes(&self) -> Option<&[Array]> {
        match &self.data {
            Data::Box(v) => Some(v),
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
            Data::F64(v) => Some(v.to_vec()),
            Data::Char(_) | Data::Box(_) => None,
        }
    }

    /// Numeric contents as i64 if exactly representable.
    pub fn to_i64_vec(&self) -> Option<Vec<i64>> {
        match &self.data {
            Data::Bool(v) => Some(v.iter().map(|&x| x as i64).collect()),
            Data::I64(v) => Some(v.to_vec()),
            Data::F64(v) => {
                let mut out = Vec::with_capacity(v.len());
                for &x in v.iter() {
                    if x.fract() != 0.0 || x.abs() >= i64::MAX as f64 {
                        return None;
                    }
                    out.push(x as i64);
                }
                Some(out)
            }
            Data::Char(_) | Data::Box(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Owns a vector and records its own drop, so a test can assert that a
    /// foreign buffer kept it alive.
    struct Guard {
        values: Vec<i64>,
        dropped: Arc<AtomicBool>,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    fn foreign_buf(values: Vec<i64>, dropped: Arc<AtomicBool>) -> Buf<i64> {
        let guard = Arc::new(Guard { values, dropped });
        let ptr = guard.values.as_ptr();
        let len = guard.values.len();
        // SAFETY: the guard owns the vector, is moved into the buffer's owner
        // slot, and nothing mutates it afterwards.
        unsafe { Buf::foreign(ptr, len, guard) }
    }

    #[test]
    fn owned_buf_derefs_to_its_slice() {
        let b: Buf<i64> = vec![1, 2, 3].into();
        assert!(!b.is_foreign());
        assert_eq!(&b[..], &[1, 2, 3]);
        assert_eq!(b.len(), 3);
        assert_eq!(b.iter().sum::<i64>(), 6);
    }

    #[test]
    fn empty_buf_is_a_valid_empty_slice() {
        let b: Buf<f64> = Buf::new();
        assert_eq!(&b[..], &[] as &[f64]);
        // SAFETY: zero length, so the dangling pointer is never dereferenced.
        let f = unsafe { Buf::<f64>::foreign(std::ptr::null(), 0, Arc::new(())) };
        assert_eq!(&f[..], &[] as &[f64]);
    }

    #[test]
    fn cloning_an_owned_buf_shares_the_same_memory() {
        let b: Buf<i64> = vec![1, 2, 3].into();
        let c = b.clone();
        assert_eq!(b.as_ptr(), c.as_ptr(), "owned clone copied the elements");
        assert_eq!(&c[..], &[1, 2, 3]);
    }

    #[test]
    fn writing_to_a_shared_owned_buf_copies_first() {
        let b: Buf<i64> = vec![1, 2, 3].into();
        let mut c = b.clone();
        c.to_mut()[0] = 99;
        assert_eq!(&b[..], &[1, 2, 3], "the other holder saw the write");
        assert_eq!(&c[..], &[99, 2, 3]);
        assert_ne!(b.as_ptr(), c.as_ptr());
        // Sole holder again: further writes are in place.
        let ptr = c.as_ptr();
        c.to_mut()[1] = 98;
        assert_eq!(c.as_ptr(), ptr, "unshared write copied");
    }

    #[test]
    fn into_vec_moves_when_sole_holder_and_copies_when_shared() {
        let b: Buf<i64> = vec![1, 2, 3].into();
        let ptr = b.as_ptr();
        let v = b.into_vec();
        assert_eq!(v.as_ptr(), ptr, "sole holder copied instead of moving");

        let b: Buf<i64> = vec![1, 2, 3].into();
        let c = b.clone();
        let v = b.into_vec();
        assert_eq!(v, vec![1, 2, 3]);
        assert_eq!(&c[..], &[1, 2, 3]);
    }

    #[test]
    fn foreign_buf_reads_borrowed_memory_and_keeps_the_owner_alive() {
        let dropped = Arc::new(AtomicBool::new(false));
        let b = foreign_buf(vec![10, 20, 30], dropped.clone());
        assert!(b.is_foreign());
        assert_eq!(&b[..], &[10, 20, 30]);
        assert!(!dropped.load(Ordering::SeqCst), "owner dropped while borrowed");
        drop(b);
        assert!(dropped.load(Ordering::SeqCst), "owner leaked after the buffer died");
    }

    #[test]
    fn cloning_a_foreign_buf_shares_the_same_memory() {
        let dropped = Arc::new(AtomicBool::new(false));
        let b = foreign_buf(vec![1, 2, 3], dropped.clone());
        let c = b.clone();
        assert!(c.is_foreign());
        assert_eq!(b.as_ptr(), c.as_ptr());
        drop(b);
        assert!(!dropped.load(Ordering::SeqCst), "owner dropped while a clone lives");
        assert_eq!(&c[..], &[1, 2, 3]);
    }

    #[test]
    fn slicing_a_foreign_buf_keeps_borrowing() {
        let dropped = Arc::new(AtomicBool::new(false));
        let b = foreign_buf(vec![1, 2, 3, 4], dropped.clone());
        let s = b.slice(1, 3);
        assert!(s.is_foreign());
        assert_eq!(&s[..], &[2, 3]);
        drop(b);
        assert_eq!(&s[..], &[2, 3]);
        assert!(!dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn mutating_a_foreign_buf_copies_first() {
        let dropped = Arc::new(AtomicBool::new(false));
        let mut b = foreign_buf(vec![1, 2, 3], dropped.clone());
        b.push(4);
        assert!(!b.is_foreign());
        assert_eq!(&b[..], &[1, 2, 3, 4]);
        // The original memory is untouched and released with the owner.
        drop(b);
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn copy_on_write_leaves_other_holders_alone() {
        let dropped = Arc::new(AtomicBool::new(false));
        let b = foreign_buf(vec![1, 2, 3], dropped.clone());
        let mut c = b.clone();
        c.to_mut()[0] = 99;
        assert_eq!(&b[..], &[1, 2, 3]);
        assert_eq!(&c[..], &[99, 2, 3]);
    }

    #[test]
    fn foreign_data_slices_without_copying() {
        let dropped = Arc::new(AtomicBool::new(false));
        let a = Array::new(vec![2, 2], Data::I64(foreign_buf(vec![1, 2, 3, 4], dropped)));
        assert!(a.data.is_foreign());
        let row = a.item(1);
        assert!(row.data.is_foreign());
        assert_eq!(row.as_i64_slice(), Some(&[3, 4][..]));
    }

    #[test]
    fn foreign_data_extends_by_copying() {
        let dropped = Arc::new(AtomicBool::new(false));
        let mut d = Data::I64(foreign_buf(vec![1, 2], dropped));
        assert!(d.is_foreign());
        assert!(d.extend_from(&Data::I64(vec![3].into())));
        assert!(!d.is_foreign());
        assert_eq!(d, Data::I64(vec![1, 2, 3].into()));
    }
}
