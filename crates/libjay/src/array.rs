//! Dense multidimensional array: a shape, a flat buffer and the [`Layout`]
//! that says how one indexes the other.
//!
//! Buffers are either owned or borrowed from foreign memory (Arrow, the
//! Python buffer protocol) through [`Buf`], which is what makes the data
//! boundary zero-copy.

use std::any::Any;
use std::ops::Deref;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

use crate::complex::Cx;
use crate::dtype::DType;
use crate::exact::{Ext, Rat};

/// Anything that keeps a foreign buffer's memory alive: the importing side
/// stores its release guards here and the buffer outlives nothing else.
pub type Owner = Arc<dyn Any + Send + Sync>;

/// How many joined buffers have been joined — that is, how many times a set
/// of columns that crossed the boundary without a copy has since had to be
/// copied into one block.
///
/// It exists so that a test can assert a copy did not happen: the number is
/// process-wide and only ever grows.
static JOINS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many times a column-major array has had its rows materialised —
/// [`Array::to_row_major`] doing real work. Process-wide, only ever grows,
/// and here so that a test can say which verbs need the rows and which do
/// not.
static LAYOUTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The count of joins made since the process started. See `JOINS`.
pub fn joins_made() -> u64 {
    JOINS.load(Ordering::Relaxed)
}

/// The count of row-major materialisations since the process started. See
/// `LAYOUTS`.
pub fn layouts_made() -> u64 {
    LAYOUTS.load(Ordering::Relaxed)
}

/// A flat element buffer, owned or borrowed.
///
/// A borrowed (foreign) buffer points into memory owned by someone else — an
/// Arrow C data interface import, a Python buffer — and holds an `Owner`
/// handle that keeps that memory alive for at least as long as the buffer.
/// An owned buffer is refcounted, and [`Buf::slice`] of one is a window over
/// the same allocation rather than a copy. However the buffer was made,
/// cloning is a refcount bump and mutation copies first if the memory is
/// shared, foreign or a window ([`Buf::to_mut`]), so a `Buf` behaves as a
/// private value however cheaply it was cloned.
pub struct Buf<T> {
    repr: Repr<T>,
}

enum Repr<T> {
    /// The whole of a refcounted `Vec`.
    Owned(Arc<Vec<T>>),
    /// The window `[off, off + len)` of a refcounted `Vec`, which is what
    /// taking a cell or a section out of an owned array gives: a view over
    /// the same allocation, never a copy. Writing to one copies first, as
    /// writing to a shared whole does.
    Slice { buf: Arc<Vec<T>>, off: usize, len: usize },
    Foreign { ptr: *const T, len: usize, owner: Owner },
    /// Several buffers end to end, joined only if someone asks for the flat
    /// slice. This is how a table of columns arrives: each column keeps
    /// borrowing its own memory, and a reader that wants the columns takes
    /// them ([`Buf::parts`]) rather than the join. The join, once made, is
    /// kept and shared with every clone, so no buffer is ever built twice.
    ///
    /// `join` is how to make it. A plain element type takes the parallel
    /// copy, which is what keeps the join from costing more than the weave
    /// it replaced; a heap-backed one takes the sequential clone.
    Cols {
        parts: Vec<Buf<T>>,
        len: usize,
        flat: Arc<OnceLock<Arc<Vec<T>>>>,
        join: fn(&[Buf<T>], usize) -> Vec<T>,
    },
}

// SAFETY: no variant hands out aliased mutable access; a join of parts is
// made once behind a `OnceLock` and never written again. A foreign buffer
// is read-only for its whole life and its `owner` keeps the memory alive; an
// owned buffer shares its `Vec` through an `Arc` and only ever mutates it
// through `Arc::make_mut`, which copies unless this buffer is the sole
// holder; a window over part of one becomes a `Vec` of its own before any
// write, so it never mutates the allocation it shares. So `Buf` is exactly
// as shareable as the `&[T]` it derefs to —
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

    /// True while the buffer still borrows foreign memory. A join of
    /// buffers borrows while any of its parts does and the join has not
    /// been made.
    pub fn is_foreign(&self) -> bool {
        match &self.repr {
            Repr::Foreign { .. } => true,
            Repr::Cols { parts, flat, .. } => {
                flat.get().is_none() && parts.iter().any(Buf::is_foreign)
            }
            _ => false,
        }
    }

    /// Elements the buffer holds, without joining a set of parts.
    pub fn len(&self) -> usize {
        match &self.repr {
            Repr::Owned(v) => v.len(),
            Repr::Slice { len, .. } | Repr::Foreign { len, .. } | Repr::Cols { len, .. } => *len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True once a joined buffer has had its join made: the copy the
    /// boundary avoided has since been paid for.
    pub fn is_joined(&self) -> bool {
        matches!(&self.repr, Repr::Cols { flat, .. } if flat.get().is_some())
    }

    /// The parts of a buffer that was made by joining several, in order —
    /// None for every other buffer. A reader that can work part by part
    /// (a column at a time) takes this and never makes the join.
    pub fn parts(&self) -> Option<&[Buf<T>]> {
        match &self.repr {
            Repr::Cols { parts, .. } => Some(parts),
            _ => None,
        }
    }

    /// The handle keeping this buffer's memory alive, for a borrowed
    /// buffer.
    ///
    /// What the handle holds is the importing side's business, and a reader
    /// that recognises one of its own can act on it: a device upload leaves
    /// the device allocation in here, which is how an array carries its
    /// location without becoming a different kind of array.
    pub fn owner(&self) -> Option<&Owner> {
        match &self.repr {
            Repr::Foreign { owner, .. } => Some(owner),
            _ => None,
        }
    }
}

/// Join the parts one element at a time. Any element type at all, and the
/// only choice for the heap-backed ones.
fn join_sequential<T: Clone>(parts: &[Buf<T>], len: usize) -> Vec<T> {
    let mut v = Vec::with_capacity(len);
    for part in parts {
        v.extend_from_slice(part.as_slice());
    }
    v
}

/// Join the parts on the thread pool: each chunk of the result copies from
/// whichever parts cover it. A fresh block of this size costs more to fault
/// in than to fill, and that cost only comes down by spreading the writes.
fn join_parallel<T: Copy + Default + Send + Sync>(parts: &[Buf<T>], len: usize) -> Vec<T> {
    let slices: Vec<&[T]> = parts.iter().map(Buf::as_slice).collect();
    let (out, ok) = crate::par::fill(len, |start, dst: &mut [T]| {
        let mut at = 0;
        let mut written = 0;
        for s in &slices {
            let (from, to) = (at, at + s.len());
            at = to;
            let lo = start.max(from);
            let hi = (start + dst.len()).min(to);
            if lo < hi {
                dst[lo - start..hi - start].copy_from_slice(&s[lo - from..hi - from]);
                written += hi - lo;
            }
        }
        written == dst.len()
    });
    debug_assert!(ok, "the parts do not cover the join");
    out
}

impl<T: Clone> Buf<T> {
    /// One buffer holding `parts` end to end. Nothing is copied here: the
    /// parts are joined when — and only when — a caller asks for the flat
    /// slice, and then one element at a time.
    pub fn join(parts: Vec<Buf<T>>) -> Buf<T> {
        Buf::joined(parts, join_sequential)
    }

    fn joined(parts: Vec<Buf<T>>, join: fn(&[Buf<T>], usize) -> Vec<T>) -> Buf<T> {
        let len = parts.iter().map(Buf::len).sum();
        Buf { repr: Repr::Cols { parts, len, flat: Arc::new(OnceLock::new()), join } }
    }

    pub fn as_slice(&self) -> &[T] {
        match &self.repr {
            Repr::Owned(v) => v,
            Repr::Slice { buf, off, len } => &buf[*off..*off + *len],
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
            // The join a set of parts was put off making. It is made once
            // and kept, so a buffer asked for its flat form twice pays for
            // it once.
            Repr::Cols { parts, len, flat, join } => flat.get_or_init(|| {
                JOINS.fetch_add(1, Ordering::Relaxed);
                Arc::new(join(parts, *len))
            }),
        }
    }

    /// The buffer as a uniquely owned `Vec`, copying once if it is foreign or
    /// shared with another holder. Subsequent calls on the same buffer are
    /// free until it is cloned again.
    pub fn to_mut(&mut self) -> &mut Vec<T> {
        // A window over part of a `Vec` becomes a `Vec` of its own first:
        // what the caller writes — a change of length included — must not
        // reach the other windows over the same allocation.
        if !matches!(self.repr, Repr::Owned(_)) {
            self.repr = Repr::Owned(Arc::new(self.as_slice().to_vec()));
        }
        match &mut self.repr {
            Repr::Owned(v) => Arc::make_mut(v),
            _ => unreachable!("just converted to a whole owned buffer"),
        }
    }

    /// The contents as a `Vec`, moving it out when this buffer is the sole
    /// holder of a whole one and copying otherwise.
    pub fn into_vec(self) -> Vec<T> {
        match self.repr {
            Repr::Owned(v) => Arc::try_unwrap(v).unwrap_or_else(|v| v.as_slice().to_vec()),
            Repr::Slice { ref buf, off, len } => buf[off..off + len].to_vec(),
            Repr::Foreign { .. } | Repr::Cols { .. } => self.as_slice().to_vec(),
        }
    }

    pub fn push(&mut self, value: T) {
        self.to_mut().push(value);
    }

    pub fn extend_from_slice(&mut self, other: &[T]) {
        self.to_mut().extend_from_slice(other);
    }

    /// Elements `[start, end)`, as a view: no element is copied, whatever
    /// the buffer is. A foreign slice keeps borrowing and shares the same
    /// owner; an owned one is a window over the same refcounted `Vec`, so
    /// it holds that whole allocation alive for as long as it lives.
    pub fn slice(&self, start: usize, end: usize) -> Buf<T> {
        match &self.repr {
            Repr::Owned(v) => {
                assert!(start <= end && end <= v.len(), "slice out of range");
                if start == 0 && end == v.len() {
                    return Buf { repr: Repr::Owned(Arc::clone(v)) };
                }
                Buf { repr: Repr::Slice { buf: Arc::clone(v), off: start, len: end - start } }
            }
            Repr::Slice { buf, off, len } => {
                assert!(start <= end && end <= *len, "slice out of range");
                let repr =
                    Repr::Slice { buf: Arc::clone(buf), off: off + start, len: end - start };
                Buf { repr }
            }
            Repr::Foreign { ptr, len, owner } => {
                assert!(start <= end && end <= *len, "slice out of range");
                // SAFETY: `start <= len` keeps the offset inside the same
                // allocation; the new buffer holds a clone of the owner.
                unsafe { Buf::foreign(ptr.add(start), end - start, owner.clone()) }
            }
            // A range inside one part is that part's own slice, so taking a
            // column out of a joined table copies nothing. Anything else
            // crosses a seam and has to read the join.
            Repr::Cols { parts, len, flat, .. } => {
                assert!(start <= end && end <= *len, "slice out of range");
                if flat.get().is_none() {
                    let mut at = 0;
                    for part in parts {
                        let stop = at + part.len();
                        if start >= at && end <= stop {
                            return part.slice(start - at, end - at);
                        }
                        at = stop;
                    }
                }
                let whole = Arc::clone(self.flat_arc());
                if start == 0 && end == whole.len() {
                    return Buf { repr: Repr::Owned(whole) };
                }
                Buf { repr: Repr::Slice { buf: whole, off: start, len: end - start } }
            }
        }
    }

    /// The join, made if it was not made yet. Only a joined buffer has one.
    fn flat_arc(&self) -> &Arc<Vec<T>> {
        self.as_slice();
        match &self.repr {
            Repr::Cols { flat, .. } => flat.get().expect("just initialised"),
            _ => unreachable!("only a joined buffer is asked for its join"),
        }
    }
}

impl<T: Copy + Default + Send + Sync> Buf<T> {
    /// [`Buf::join`] for a plain element type: the join, if it is ever
    /// made, is made on the thread pool.
    pub fn join_fast(parts: Vec<Buf<T>>) -> Buf<T> {
        Buf::joined(parts, join_parallel)
    }
}

impl<T: Clone> Deref for Buf<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

/// Cloning never copies elements: every shape of buffer is a refcount bump,
/// and the copy happens later, in [`Buf::to_mut`], only if someone writes
/// while the memory is still shared.
impl<T: Clone> Clone for Buf<T> {
    fn clone(&self) -> Buf<T> {
        match &self.repr {
            Repr::Owned(v) => Buf { repr: Repr::Owned(Arc::clone(v)) },
            Repr::Slice { buf, off, len } => {
                Buf { repr: Repr::Slice { buf: Arc::clone(buf), off: *off, len: *len } }
            }
            Repr::Foreign { ptr, len, owner } => {
                // SAFETY: same pointer, same owner, same guarantees.
                unsafe { Buf::foreign(*ptr, *len, owner.clone()) }
            }
            // The parts are refcount bumps, and the join is shared with
            // every other holder: made at most once however many clones ask
            // for it.
            Repr::Cols { parts, len, flat, join } => Buf {
                repr: Repr::Cols {
                    parts: parts.clone(),
                    len: *len,
                    flat: Arc::clone(flat),
                    join: *join,
                },
            },
        }
    }
}

impl<T> Default for Buf<T> {
    fn default() -> Buf<T> {
        Buf::new()
    }
}

impl<T: Clone + std::fmt::Debug> std::fmt::Debug for Buf<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_slice(), f)
    }
}

impl<T: Clone + PartialEq> PartialEq for Buf<T> {
    fn eq(&self, other: &Buf<T>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T> From<Vec<T>> for Buf<T> {
    fn from(v: Vec<T>) -> Buf<T> {
        Buf::from_vec(v)
    }
}

impl<'a, T: Clone> IntoIterator for &'a Buf<T> {
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
    /// Arbitrary-precision integers. Like boxes, these are heap-backed
    /// pointers rather than machine words: never foreign, never fused,
    /// never vectorised.
    Ext(Buf<Ext>),
    /// Exact ratios, each in lowest terms. Heap-backed, as `Ext` is.
    Rat(Buf<Rat>),
    F64(Buf<f64>),
    /// Complex numbers, interleaved `[re, im]` — the layout numpy, C and a
    /// pair of Arrow float columns all share.
    Complex(Buf<Cx>),
    Char(Buf<char>),
    /// Symbols: every element is an index into the process-wide symbol
    /// table (see [`crate::symbol`]), so the buffer is as flat and as
    /// cheap to copy as one of integers and the names live once each.
    Symbol(Buf<crate::symbol::Id>),
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
            Data::Ext(_) => DType::Ext,
            Data::Rat(_) => DType::Rat,
            Data::F64(_) => DType::F64,
            Data::Complex(_) => DType::Complex,
            Data::Char(_) => DType::Char,
            Data::Symbol(_) => DType::Symbol,
            Data::Box(_) => DType::Box,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Data::Bool(v) => v.len(),
            Data::I64(v) => v.len(),
            Data::Ext(v) => v.len(),
            Data::Rat(v) => v.len(),
            Data::F64(v) => v.len(),
            Data::Complex(v) => v.len(),
            Data::Char(v) => v.len(),
            Data::Symbol(v) => v.len(),
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
            Data::Ext(v) => v.is_foreign(),
            Data::Rat(v) => v.is_foreign(),
            Data::F64(v) => v.is_foreign(),
            Data::Complex(v) => v.is_foreign(),
            Data::Char(v) => v.is_foreign(),
            Data::Symbol(v) => v.is_foreign(),
            Data::Box(v) => v.is_foreign(),
        }
    }

    /// The handle keeping this payload's memory alive, for a borrowed one.
    /// See [`Buf::owner`].
    pub fn owner(&self) -> Option<&Owner> {
        match self {
            Data::Bool(v) => v.owner(),
            Data::I64(v) => v.owner(),
            Data::Ext(v) => v.owner(),
            Data::Rat(v) => v.owner(),
            Data::F64(v) => v.owner(),
            Data::Complex(v) => v.owner(),
            Data::Char(v) => v.owner(),
            Data::Symbol(v) => v.owner(),
            Data::Box(v) => v.owner(),
        }
    }

    pub fn slice(&self, start: usize, end: usize) -> Data {
        match self {
            Data::Bool(v) => Data::Bool(v.slice(start, end)),
            Data::I64(v) => Data::I64(v.slice(start, end)),
            Data::Ext(v) => Data::Ext(v.slice(start, end)),
            Data::Rat(v) => Data::Rat(v.slice(start, end)),
            Data::F64(v) => Data::F64(v.slice(start, end)),
            Data::Complex(v) => Data::Complex(v.slice(start, end)),
            Data::Char(v) => Data::Char(v.slice(start, end)),
            Data::Symbol(v) => Data::Symbol(v.slice(start, end)),
            Data::Box(v) => Data::Box(v.slice(start, end)),
        }
    }

    pub fn empty(dtype: DType) -> Data {
        match dtype {
            DType::Bool => Data::Bool(Buf::new()),
            DType::I64 => Data::I64(Buf::new()),
            DType::Ext => Data::Ext(Buf::new()),
            DType::Rat => Data::Rat(Buf::new()),
            DType::F64 => Data::F64(Buf::new()),
            DType::Complex => Data::Complex(Buf::new()),
            DType::Char => Data::Char(Buf::new()),
            DType::Symbol => Data::Symbol(Buf::new()),
            DType::Box => Data::Box(Buf::new()),
        }
    }

    /// The fill element used by overtaking and framing. The boxed fill is
    /// J's `a:`, a box holding an empty numeric list.
    pub fn push_fill(&mut self) {
        match self {
            Data::Bool(v) => v.push(0),
            Data::I64(v) => v.push(0),
            Data::Ext(v) => v.push(Ext::default()),
            Data::Rat(v) => v.push(Rat::zero()),
            Data::F64(v) => v.push(0.0),
            Data::Complex(v) => v.push(crate::complex::ZERO),
            Data::Char(v) => v.push(' '),
            Data::Symbol(v) => v.push(crate::symbol::EMPTY),
            Data::Box(v) => v.push(Array::box_fill()),
        }
    }

    /// Append element `i` of `src`, which must hold the same type. Nothing
    /// happens if the types disagree; every caller has checked already.
    pub fn push_from(&mut self, src: &Data, i: usize) {
        match (self, src) {
            (Data::Bool(a), Data::Bool(b)) => a.push(b[i]),
            (Data::I64(a), Data::I64(b)) => a.push(b[i]),
            (Data::Ext(a), Data::Ext(b)) => a.push(b[i].clone()),
            (Data::Rat(a), Data::Rat(b)) => a.push(b[i].clone()),
            (Data::F64(a), Data::F64(b)) => a.push(b[i]),
            (Data::Complex(a), Data::Complex(b)) => a.push(b[i]),
            (Data::Char(a), Data::Char(b)) => a.push(b[i]),
            (Data::Symbol(a), Data::Symbol(b)) => a.push(b[i]),
            (Data::Box(a), Data::Box(b)) => a.push(b[i].clone()),
            _ => {}
        }
    }

    pub fn extend_from(&mut self, other: &Data) -> bool {
        match (self, other) {
            (Data::Bool(a), Data::Bool(b)) => a.extend_from_slice(b),
            (Data::I64(a), Data::I64(b)) => a.extend_from_slice(b),
            (Data::Ext(a), Data::Ext(b)) => a.extend_from_slice(b),
            (Data::Rat(a), Data::Rat(b)) => a.extend_from_slice(b),
            (Data::F64(a), Data::F64(b)) => a.extend_from_slice(b),
            (Data::Complex(a), Data::Complex(b)) => a.extend_from_slice(b),
            (Data::Char(a), Data::Char(b)) => a.extend_from_slice(b),
            (Data::Symbol(a), Data::Symbol(b)) => a.extend_from_slice(b),
            (Data::Box(a), Data::Box(b)) => a.extend_from_slice(b),
            _ => return false,
        }
        true
    }

    /// The columns end to end, as the flat buffer of a [`Layout::ColMajor`]
    /// array of shape `[rows, columns.len()]`.
    ///
    /// Nothing is copied: each column keeps borrowing whatever memory it
    /// arrived in, and the buffer joins them only if some reader asks for
    /// the flat slice. This is the table boundary that does no work.
    ///
    /// None on the same disagreements [`Data::interleave`] refuses.
    pub fn join(columns: &[Data], rows: usize) -> Option<Data> {
        let first = columns.first()?;
        if columns.iter().any(|c| c.dtype() != first.dtype() || c.len() < rows) {
            return None;
        }
        macro_rules! by {
            ($variant:ident, $join:expr) => {{
                let mut parts = Vec::with_capacity(columns.len());
                for c in columns {
                    let Data::$variant(v) = c else { return None };
                    // A column longer than the table contributes its first
                    // `rows` elements, as the weave takes them.
                    parts.push(if v.len() == rows { v.clone() } else { v.slice(0, rows) });
                }
                Some(Data::$variant($join(parts)))
            }};
        }
        match first.dtype() {
            DType::Bool => by!(Bool, Buf::join_fast),
            DType::I64 => by!(I64, Buf::join_fast),
            DType::F64 => by!(F64, Buf::join_fast),
            DType::Complex => by!(Complex, Buf::join_fast),
            DType::Char => by!(Char, Buf::join),
            DType::Symbol => by!(Symbol, Buf::join_fast),
            DType::Ext => by!(Ext, Buf::join),
            DType::Rat => by!(Rat, Buf::join),
            DType::Box => by!(Box, Buf::join),
        }
    }

    /// The `cols` runs of `rows` elements this buffer holds, each as a
    /// buffer of its own: the columns of a [`Layout::ColMajor`] array.
    /// Slicing a joined buffer at a seam copies nothing, so a table that
    /// arrived as columns is read back as the columns it arrived as.
    pub fn columns(&self, rows: usize, cols: usize) -> Vec<Data> {
        (0..cols).map(|j| self.slice(j * rows, (j + 1) * rows)).collect()
    }

    /// Weave column-major buffers into one row-major block of shape
    /// `[rows, columns.len()]`.
    ///
    /// This is the table boundary: a DataFrame arrives as one buffer per
    /// column and libjay works rows-leading, so the elements have to be
    /// woven once. The weave reads every column in order and writes its
    /// result straight through, split across threads at the sizes that pay.
    ///
    /// None when the columns disagree on element type, when one is shorter
    /// than `rows`, or when there are no columns at all — the importing
    /// side has already reported that.
    pub fn interleave(columns: &[Data], rows: usize) -> Option<Data> {
        let cols = columns.len();
        let first = columns.first()?;
        if columns.iter().any(|c| c.dtype() != first.dtype() || c.len() < rows) {
            return None;
        }

        /// One row of the output takes one element from each column, so a
        /// chunk of the output is a run of whole rows plus, at either end,
        /// the part of a row the neighbouring chunk does not hold.
        fn weave<T: Copy + Default + Send + Sync>(columns: &[&[T]], rows: usize) -> Vec<T> {
            let cols = columns.len();
            let (out, _) = crate::par::fill(rows * cols, |start, part: &mut [T]| {
                let mut rest = &mut part[..];
                let mut at = start;
                // The tail of a row that began in the chunk before this one.
                let lead = ((cols - at % cols) % cols).min(rest.len());
                if lead > 0 {
                    let (head, tail) = rest.split_at_mut(lead);
                    let r = at / cols;
                    for (k, slot) in head.iter_mut().enumerate() {
                        *slot = columns[at % cols + k][r];
                    }
                    at += lead;
                    rest = tail;
                }
                let whole = rest.len() / cols;
                let (body, tail) = rest.split_at_mut(whole * cols);
                let r0 = at / cols;
                for (k, row) in body.chunks_exact_mut(cols).enumerate() {
                    for (slot, col) in row.iter_mut().zip(columns) {
                        *slot = col[r0 + k];
                    }
                }
                // The head of a row the next chunk finishes.
                let r = r0 + whole;
                for (c, slot) in tail.iter_mut().enumerate() {
                    *slot = columns[c][r];
                }
                true
            });
            out
        }

        /// The same weave for the heap-backed types, which are neither
        /// `Copy` nor worth a thread: Arrow carries none of them, so this
        /// only ever runs on data libjay built itself.
        fn weave_cloned<T: Clone>(columns: &[&[T]], rows: usize) -> Vec<T> {
            let mut out = Vec::with_capacity(rows * columns.len());
            for r in 0..rows {
                for c in columns {
                    out.push(c[r].clone());
                }
            }
            out
        }

        macro_rules! by {
            ($variant:ident, $weave:ident) => {{
                let mut s = Vec::with_capacity(cols);
                for c in columns {
                    let Data::$variant(v) = c else { return None };
                    s.push(v.as_slice());
                }
                Some(Data::$variant($weave(&s, rows).into()))
            }};
        }
        match first.dtype() {
            DType::Bool => by!(Bool, weave),
            DType::I64 => by!(I64, weave),
            DType::F64 => by!(F64, weave),
            DType::Complex => by!(Complex, weave),
            DType::Char => by!(Char, weave),
            DType::Symbol => by!(Symbol, weave),
            DType::Ext => by!(Ext, weave_cloned),
            DType::Rat => by!(Rat, weave_cloned),
            DType::Box => by!(Box, weave_cloned),
        }
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
            (Data::Bool(v), DType::Ext) => Some(Data::Ext(v.iter().map(|&x| Ext::from(x)).collect())),
            (Data::I64(v), DType::Ext) => Some(Data::Ext(v.iter().map(|&x| Ext::from(x)).collect())),
            (Data::Bool(v), DType::Rat) => {
                Some(Data::Rat(v.iter().map(|&x| Rat::from_int(Ext::from(x))).collect()))
            }
            (Data::I64(v), DType::Rat) => {
                Some(Data::Rat(v.iter().map(|&x| Rat::from_int(Ext::from(x))).collect()))
            }
            (Data::Ext(v), DType::Rat) => {
                Some(Data::Rat(v.iter().map(|x| Rat::from_int(x.clone())).collect()))
            }
            (Data::Ext(v), DType::F64) => {
                Some(Data::F64(v.iter().map(crate::exact::ext_to_f64).collect()))
            }
            (Data::Rat(v), DType::F64) => Some(Data::F64(v.iter().map(Rat::to_f64).collect())),
            (Data::Bool(v), DType::Complex) => {
                Some(Data::Complex(v.iter().map(|&x| [x as f64, 0.0]).collect()))
            }
            (Data::I64(v), DType::Complex) => {
                Some(Data::Complex(v.iter().map(|&x| [x as f64, 0.0]).collect()))
            }
            (Data::Ext(v), DType::Complex) => {
                Some(Data::Complex(v.iter().map(|x| [crate::exact::ext_to_f64(x), 0.0]).collect()))
            }
            (Data::Rat(v), DType::Complex) => {
                Some(Data::Complex(v.iter().map(|x| [x.to_f64(), 0.0]).collect()))
            }
            (Data::F64(v), DType::Complex) => {
                Some(Data::Complex(v.iter().map(|&x| [x, 0.0]).collect()))
            }
            _ => None,
        }
    }
}

/// How an array's shape indexes its flat buffer.
///
/// The shape is always the LOGICAL one — rows leading, the contract every
/// frontend and every diagnostic reads by — and the layout says only where
/// element `(i0 … ik)` sits in the buffer. Rank 0 and rank 1 have one
/// possible answer and are always [`Layout::RowMajor`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// The last axis varies fastest: offset `((i0 * s1) + i1) * s2 + …`.
    /// Everything in the runtime reads this unless it has asked.
    #[default]
    RowMajor,
    /// The FIRST axis varies fastest, which for a matrix means each column
    /// is contiguous — the layout a table of Arrow columns already has, and
    /// the layout `|:` produces by flipping this flag instead of moving
    /// 160 MB.
    ColMajor,
}

/// An array: a logical shape, a flat buffer, and the layout joining them.
///
/// `data` is the buffer as it lies. A reader that indexes it must either
/// honour [`Array::layout`] or take [`Array::to_row_major`] first; the
/// runtime's rule is that a value reaching a verb has already been made
/// row-major unless that verb asked for the other one.
///
/// A SPARSE array is the one exception to "the buffer holds every element":
/// `shape` is still the logical shape, but `data` holds only the stored
/// cells and [`crate::sparse::Sparse`] says where they sit. Only `$.`, the
/// display and `":` read that form; every other reader takes
/// [`Array::densified`] first.
#[derive(Clone, Debug)]
pub struct Array {
    pub shape: Vec<usize>,
    pub data: Data,
    layout: Layout,
    sparse: Option<crate::sparse::Handle>,
    proto: Option<std::sync::Arc<Array>>,
}

/// Two arrays are equal when they hold the same elements at the same
/// indices, whatever buffer order — or storage kind — each of them keeps.
impl PartialEq for Array {
    fn eq(&self, other: &Array) -> bool {
        if self.shape != other.shape {
            return false;
        }
        if self.sparse.is_some() || other.sparse.is_some() {
            let (a, b) = (self.densified(), other.densified());
            return a.to_row_major().data == b.to_row_major().data;
        }
        if self.layout == other.layout {
            return self.data == other.data;
        }
        self.to_row_major().data == other.to_row_major().data
    }
}

impl Array {
    pub fn new(shape: Vec<usize>, data: Data) -> Array {
        debug_assert_eq!(shape.iter().product::<usize>(), data.len());
        Array { shape, data, layout: Layout::RowMajor, sparse: None, proto: None }
    }

    /// A sparse array: the logical `shape`, the stored cells, and the
    /// description of where they sit. `data` holds `entries` cells and not
    /// one element per position, so this is the only constructor that does
    /// not tie the buffer's length to the shape.
    pub fn sparse(shape: Vec<usize>, data: Data, sparse: crate::sparse::Sparse) -> Array {
        Array { shape, data, layout: Layout::RowMajor, sparse: Some(std::sync::Arc::new(sparse)), proto: None }
    }

    /// True while the array holds only its stored cells.
    pub fn is_sparse(&self) -> bool {
        self.sparse.is_some()
    }

    /// The item an array with no items would have held — APL's prototype.
    ///
    /// A simple array's type says what its fills look like, so nothing has
    /// to be remembered; a nested one does, since an empty buffer of boxes
    /// no longer says whether its items were pairs of numbers or of
    /// characters. `0⍴⊂2 3⍴9` is such an array, and `↑` of it answers the
    /// 2 by 3 table of zeros this holds. Only the operations that make an
    /// empty out of a nested array set it, and only APL reads it.
    pub fn proto(&self) -> Option<&Array> {
        self.proto.as_deref()
    }

    /// The same array, remembering what its items looked like.
    pub fn with_proto(mut self, proto: Array) -> Array {
        self.proto = Some(std::sync::Arc::new(proto));
        self
    }

    /// How this array is stored sparsely, or None for a dense one.
    pub fn sparse_parts(&self) -> Option<&crate::sparse::Sparse> {
        self.sparse.as_deref()
    }

    /// This array with every position materialised. A dense array is a
    /// refcount bump; a sparse one is expanded here and nowhere else.
    pub fn densified(&self) -> Array {
        match &self.sparse {
            None => self.clone(),
            Some(s) => crate::sparse::densify(self, s),
        }
    }

    /// An array whose buffer holds its first axis fastest — the columns of
    /// a matrix, end to end. Rank 0 and 1 have only one layout and take it.
    pub fn col_major(shape: Vec<usize>, data: Data) -> Array {
        debug_assert_eq!(shape.iter().product::<usize>(), data.len());
        let layout = if shape.len() < 2 { Layout::RowMajor } else { Layout::ColMajor };
        Array { shape, data, layout, sparse: None, proto: None }
    }

    /// The same buffer read the other way round. The caller is asserting
    /// that the buffer really is in `layout` order for this shape.
    pub fn with_layout(mut self, layout: Layout) -> Array {
        self.layout = if self.shape.len() < 2 { Layout::RowMajor } else { layout };
        self
    }

    /// How this array's buffer is ordered.
    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn is_row_major(&self) -> bool {
        self.layout == Layout::RowMajor
    }

    /// The flat buffer, for a reader that indexes it row-major. Debug builds
    /// refuse a buffer that is not in that order, which is what keeps a
    /// column-major table from being read as if it were rows.
    pub fn row_major_data(&self) -> &Data {
        debug_assert!(self.is_row_major(), "a column-major buffer read as row-major");
        &self.data
    }

    /// This array with its elements in row-major order, materialising them
    /// once if they are not. Already row-major: a refcount bump.
    pub fn to_row_major(&self) -> Array {
        if self.is_row_major() {
            return self.clone();
        }
        LAYOUTS.fetch_add(1, Ordering::Relaxed);
        Array::new(self.shape.clone(), self.transposed_data())
    }

    /// The buffer's elements in row-major order for this array's shape.
    fn transposed_data(&self) -> Data {
        let rows = self.shape[0];
        let rest: usize = self.shape[1..].iter().product();
        // A matrix is the weave the table boundary used to do eagerly: the
        // columns are already contiguous, and it runs on the pool.
        if self.rank() == 2
            && let Some(d) = Data::interleave(&self.data.columns(rows, rest), rows)
        {
            return d;
        }
        // Higher rank: the first axis varies fastest, so reading the source
        // at the transposed offset gives the row-major order.
        let n = self.count();
        let mut out = Data::empty(self.dtype());
        let mut coord = vec![0usize; self.rank()];
        for _ in 0..n {
            let mut idx = 0;
            let mut stride = 1;
            for (k, &len) in self.shape.iter().enumerate() {
                idx += coord[k] * stride;
                stride *= len;
            }
            out.push_from(&self.data, idx);
            let mut k = self.rank();
            while k > 0 {
                k -= 1;
                coord[k] += 1;
                if coord[k] < self.shape[k] {
                    break;
                }
                coord[k] = 0;
            }
        }
        out
    }

    pub fn scalar_i64(v: i64) -> Array {
        Array::new(vec![], Data::I64(vec![v].into()))
    }

    pub fn scalar_f64(v: f64) -> Array {
        Array::new(vec![], Data::F64(vec![v].into()))
    }

    pub fn scalar_bool(v: bool) -> Array {
        Array::new(vec![], Data::Bool(vec![v as u8].into()))
    }

    pub fn from_i64(values: Vec<i64>) -> Array {
        Array::new(vec![values.len()], Data::I64(values.into()))
    }

    pub fn from_f64(values: Vec<f64>) -> Array {
        Array::new(vec![values.len()], Data::F64(values.into()))
    }

    pub fn from_chars(values: Vec<char>) -> Array {
        Array::new(vec![values.len()], Data::Char(values.into()))
    }

    pub fn empty(dtype: DType) -> Array {
        Array::new(vec![0], Data::empty(dtype))
    }

    /// `y` as a scalar box (J `<`).
    pub fn boxed(value: Array) -> Array {
        Array::new(vec![], Data::Box(vec![value].into()))
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

    /// Widen the elements. A cast reads and writes the buffer as it lies,
    /// so the layout comes through untouched.
    pub fn cast(&self, to: DType) -> Option<Array> {
        if self.is_sparse() {
            return self.densified().cast(to);
        }
        Some(Array {
            shape: self.shape.clone(),
            data: self.data.cast(to)?,
            layout: self.layout,
            sparse: None,
            proto: self.proto.clone(),
        })
    }

    /// Split into cells: the trailing `cell_rank` axes form the cell shape,
    /// the leading axes form the frame.
    pub fn cells(&self, frame_rank: usize) -> Vec<Array> {
        debug_assert!(frame_rank <= self.rank());
        debug_assert!(self.is_row_major(), "cells of a column-major buffer");
        let cell_shape: Vec<usize> = self.shape[frame_rank..].to_vec();
        let cell_size: usize = cell_shape.iter().product();
        let n: usize = self.shape[..frame_rank].iter().product();
        (0..n)
            .map(|i| {
                Array::new(cell_shape.clone(), self.data.slice(i * cell_size, (i + 1) * cell_size))
            })
            .collect()
    }

    /// One cell without materialising all of them.
    pub fn cell_at(&self, frame_rank: usize, index: usize) -> Array {
        debug_assert!(self.is_row_major(), "a cell of a column-major buffer");
        let cell_shape: Vec<usize> = self.shape[frame_rank..].to_vec();
        let cell_size: usize = cell_shape.iter().product();
        Array::new(cell_shape, self.data.slice(index * cell_size, (index + 1) * cell_size))
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

    /// The extended integers, if the array holds them.
    pub fn as_ext_slice(&self) -> Option<&[Ext]> {
        match &self.data {
            Data::Ext(v) => Some(v),
            _ => None,
        }
    }

    /// The rationals, if the array holds them.
    pub fn as_rat_slice(&self) -> Option<&[Rat]> {
        match &self.data {
            Data::Rat(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_complex_slice(&self) -> Option<&[Cx]> {
        match &self.data {
            Data::Complex(v) => Some(v),
            _ => None,
        }
    }

    /// Numeric contents widened to complex. None for character or boxed data.
    pub fn to_complex_vec(&self) -> Option<Vec<Cx>> {
        match &self.data {
            Data::Bool(v) => Some(v.iter().map(|&x| [x as f64, 0.0]).collect()),
            Data::I64(v) => Some(v.iter().map(|&x| [x as f64, 0.0]).collect()),
            Data::Ext(v) => Some(v.iter().map(|x| [crate::exact::ext_to_f64(x), 0.0]).collect()),
            Data::Rat(v) => Some(v.iter().map(|x| [x.to_f64(), 0.0]).collect()),
            Data::F64(v) => Some(v.iter().map(|&x| [x, 0.0]).collect()),
            Data::Complex(v) => Some(v.to_vec()),
            Data::Char(_) | Data::Symbol(_) | Data::Box(_) => None,
        }
    }

    /// Numeric contents widened to f64. None for character data.
    pub fn to_f64_vec(&self) -> Option<Vec<f64>> {
        match &self.data {
            Data::Bool(v) => Some(v.iter().map(|&x| x as f64).collect()),
            Data::I64(v) => Some(v.iter().map(|&x| x as f64).collect()),
            Data::Ext(v) => Some(v.iter().map(crate::exact::ext_to_f64).collect()),
            Data::Rat(v) => Some(v.iter().map(Rat::to_f64).collect()),
            Data::F64(v) => Some(v.to_vec()),
            // A complex value is not a real one, even when its imaginary
            // part is zero: the caller wants a real and must ask for it.
            Data::Complex(_) | Data::Char(_) | Data::Symbol(_) | Data::Box(_) => None,
        }
    }

    /// Numeric contents as i64 if exactly representable.
    pub fn to_i64_vec(&self) -> Option<Vec<i64>> {
        match &self.data {
            Data::Bool(v) => Some(v.iter().map(|&x| x as i64).collect()),
            Data::I64(v) => Some(v.to_vec()),
            // An exact value converts only when it really is a machine
            // integer; anything else is a refusal, not a rounding.
            Data::Ext(v) => v.iter().map(crate::exact::ext_to_i64).collect(),
            Data::Rat(v) => {
                v.iter().map(|x| x.to_int().as_ref().and_then(crate::exact::ext_to_i64)).collect()
            }
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
            Data::Complex(_) | Data::Char(_) | Data::Symbol(_) | Data::Box(_) => None,
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
