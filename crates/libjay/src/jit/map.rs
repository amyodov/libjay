//! Native code for a fused element-wise float map, compiled at run time.
//!
//! A fused kernel ([`crate::fuse`]) whose working type is f64 and whose
//! program is arithmetic alone — no window, no running fold, no comparison —
//! computes `out[i] = f(in0[i], in1[i], …)` for every item. This module
//! lowers that program to one loop over a range of items and compiles it
//! with LLVM, which vectorises it. The block interpreter stays the
//! definition: the loop is used only where it answers exactly what the
//! interpreter answers, bit for bit.
//!
//! The arithmetic is the IEEE operation [`crate::scalar`] defines for each
//! primitive, emitted with no reassociation and no fused multiply-add;
//! `<.` `>.` `|` are its compare and select and `^` calls its definition.
//! Where the languages part from IEEE arithmetic the loop does not stop:
//! it accumulates, without branching, a flag that some item is not to be
//! trusted, and the caller then hands the whole range to the interpreter,
//! which answers or declines as it always did. The flag is raised where
//!
//! * the value written is not finite, which covers every NaN the
//!   interpreter would have declined on (a NaN survives every operation
//!   the loop performs except an extremum) and `0 % 0`, which IEEE makes a
//!   NaN and J a zero;
//! * an extremum sees a NaN or the two zeros, the pairs the interpreter's
//!   extrema decline;
//! * a reciprocal is not finite, which covers APL's refusal of `÷ 0`.
//!
//! Every other value the loop computes is the value the interpreter
//! computes, an infinity included, so an untouched flag means the same
//! answer. The flag is conservative where an infinity reaches the result,
//! which the interpreter would have kept.
//!
//! The policy is [`super::mode`]: `0` never compiles a map; `cranelift`
//! does not either, Cranelift having no map loop; `llvm` and `force`
//! compile every map LLVM is built in for; the automatic policy compiles
//! where the work — items times instructions — is large enough to repay
//! LLVM's compilation, and uses a map already compiled for less.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::{mode, Backend, Bin, JitMode, Un};
use crate::array::{Array, Data};
use crate::fuse::{FusedKernel, Instr, Yield};
use crate::verb::{ScalarDyad, ScalarMonad};

/// Fewest units of work — items times instructions — for which the
/// automatic policy compiles a map. Compiling one costs 15–22 ms. Measured
/// over five and twenty million floats, the compiled loop saves about
/// 0.3–0.45 ns a unit of work on one thread and 0.04–0.08 ns on a full
/// pool, where memory bandwidth bounds both roads; at 2^27 units one run
/// repays the compilation on one thread and about half of it on a pool,
/// and every later run of the cached loop is profit.
const COMPILE_WORK: usize = 1 << 27;

/// Fewest units of work for which an already compiled map is used under
/// the automatic policy. Below it a call costs about what it saves.
const USE_WORK: usize = 1 << 14;

/// The most maps one process compiles; past this, the interpreter runs.
const MAX_MAPS: usize = 1024;

/// How one input reaches the loop.
#[cfg_attr(not(feature = "llvm"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Input {
    /// A buffer of floats, read item by item.
    F64,
    /// A buffer of integers, each widened to the float it rounds to.
    I64,
    /// A buffer of booleans, one byte each, widened to 0 or 1.
    Bool,
    /// One float, the same for every item.
    Splat,
}

/// One instruction of a map program, postfix.
#[cfg_attr(not(feature = "llvm"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Op {
    /// Push input k.
    Load(u32),
    Un(Un),
    Bin(Bin),
    /// Keep the top of the stack as let k.
    Store(u32),
    /// Push let k.
    Let(u32),
}

/// A map program and the kinds of its inputs: what a compiled loop is
/// cached by.
#[cfg_attr(not(feature = "llvm"), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Map {
    pub(super) ops: Vec<Op>,
    pub(super) inputs: Vec<Input>,
}

/// A compiled map:
///
/// `map(ptrs, splats, n, out)` computes items `0 .. n`. Input k is read from
/// `ptrs[k]`, as its [`Input`] says, or is `splats[k]` where it is a splat.
/// Item i of the result is written to `out[i]`. The answer is nonzero where
/// some item is not to be trusted, and then `out` holds nothing of use.
pub(super) type MapRaw = unsafe extern "C" fn(*const *const u8, *const f64, i64, *mut f64) -> i64;

thread_local! {
    static MAP_RUNS: Cell<u64> = const { Cell::new(0) };
}

/// How many fused kernels this thread has had answered entirely by a
/// compiled map: what a test reads to know which road was taken.
pub fn map_runs() -> u64 {
    MAP_RUNS.with(Cell::get)
}

/// Count one fused kernel answered entirely by compiled code.
pub(crate) fn note_map_run() {
    MAP_RUNS.with(|c| c.set(c.get() + 1));
}

type Cache = HashMap<Map, Option<MapRaw>>;

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn un(op: ScalarMonad) -> Option<Option<Un>> {
    use ScalarMonad::*;
    Some(Some(match op {
        // The identity on real numbers emits nothing.
        Conj => return Some(None),
        Neg => Un::Neg,
        Abs => Un::Abs,
        Recip => Un::Recip,
        Inc => Un::Inc,
        Dec => Un::Dec,
        Double => Un::Double,
        Halve => Un::Halve,
        Square => Un::Square,
        OneMinus => Un::OneMinus,
        Exp => Un::Exp,
        // The sign reads the dialect's tolerance, and rounding a float
        // narrows its type: the interpreter keeps both.
        _ => return None,
    }))
}

fn bin(op: ScalarDyad) -> Option<Bin> {
    use ScalarDyad::*;
    Some(match op {
        Add => Bin::Add,
        Sub => Bin::Sub,
        Mul => Bin::Mul,
        DivJ => Bin::Div,
        Min => Bin::Min,
        Max => Bin::Max,
        // The residue and the comparisons read the dialect's tolerance.
        _ => return None,
    })
}

/// A kernel's program as a map, where it is one.
fn lower(code: &[Instr]) -> Option<Vec<Op>> {
    let mut ops = Vec::with_capacity(code.len());
    for ins in code {
        match *ins {
            Instr::Load(j) => ops.push(Op::Load(u32::try_from(j).ok()?)),
            Instr::Monad(m) => {
                if let Some(u) = un(m)? {
                    ops.push(Op::Un(u));
                }
            }
            Instr::Dyad(d) => ops.push(Op::Bin(bin(d)?)),
            Instr::Store(j) => ops.push(Op::Store(u32::try_from(j).ok()?)),
            Instr::Let(j) => ops.push(Op::Let(u32::try_from(j).ok()?)),
            Instr::Window(..) | Instr::Scan(_) => return None,
        }
    }
    Some(ops)
}

/// One input's values, as the loop reads them.
#[derive(Clone, Copy)]
enum Col<'a> {
    F64(&'a [f64]),
    I64(&'a [i64]),
    Bool(&'a [u8]),
    Splat(f64),
}

/// A compiled map bound to a fused kernel's inputs.
pub(crate) struct Bound<'a> {
    raw: MapRaw,
    cols: Vec<Col<'a>>,
}

impl Bound<'_> {
    /// Compute result items `start .. start + out.len()` into `out`. False
    /// where some item is not to be trusted; `out` then holds nothing of
    /// use and the caller computes the range another way.
    pub(crate) fn fill(&self, start: usize, out: &mut [f64]) -> bool {
        let n = out.len();
        if n == 0 {
            return true;
        }
        let mut ptrs: Vec<*const u8> = Vec::with_capacity(self.cols.len());
        let mut splats: Vec<f64> = Vec::with_capacity(self.cols.len());
        for c in &self.cols {
            let (p, s) = match *c {
                Col::F64(v) if v.len() >= start + n => (v[start..].as_ptr().cast(), 0.0),
                Col::I64(v) if v.len() >= start + n => (v[start..].as_ptr().cast(), 0.0),
                Col::Bool(v) if v.len() >= start + n => (v[start..].as_ptr(), 0.0),
                Col::Splat(x) => (std::ptr::null(), x),
                _ => return false,
            };
            ptrs.push(p);
            splats.push(s);
        }
        // SAFETY: every buffer the loop reads holds the n items from
        // `start`, checked above; `out` holds n floats and is the caller's
        // alone.
        unsafe { (self.raw)(ptrs.as_ptr(), splats.as_ptr(), n as i64, out.as_mut_ptr()) == 0 }
    }
}

/// The compiled map for kernel `k` over `inputs`, a float kernel of `n`
/// result items, under the policy in force; None where the interpreter
/// runs it.
pub(crate) fn bind<'a>(k: &FusedKernel, inputs: &'a [Array], n: usize) -> Option<Bound<'a>> {
    if !Backend::Llvm.available() {
        return None;
    }
    let m = mode();
    let (use_it, compile_it) = match m {
        JitMode::Off | JitMode::Cranelift => return None,
        JitMode::Force | JitMode::Llvm => (true, true),
        JitMode::Auto => {
            let work = n.saturating_mul(k.code().len());
            (work >= USE_WORK, work >= COMPILE_WORK)
        }
    };
    if !use_it || !matches!(k.yields(), Yield::Values | Yield::Reduce(_)) {
        return None;
    }
    let ops = lower(k.code())?;
    let mut kinds = Vec::with_capacity(inputs.len());
    let mut cols = Vec::with_capacity(inputs.len());
    for a in inputs {
        if a.is_sparse() {
            return None;
        }
        let (kind, col) = if a.rank() == 0 {
            let v = match &a.data {
                Data::F64(d) => *d.first()?,
                Data::I64(d) => *d.first()? as f64,
                Data::Bool(d) => f64::from(*d.first()?),
                _ => return None,
            };
            (Input::Splat, Col::Splat(v))
        } else {
            match &a.data {
                Data::F64(d) => (Input::F64, Col::F64(d)),
                Data::I64(d) => (Input::I64, Col::I64(d)),
                Data::Bool(d) => (Input::Bool, Col::Bool(d)),
                _ => return None,
            }
        };
        kinds.push(kind);
        cols.push(col);
    }
    let map = Map { ops, inputs: kinds };
    let raw = compiled(&map, compile_it)?;
    Some(Bound { raw, cols })
}

/// The loop for `map`, from the cache or compiled now where `may_compile`.
fn compiled(map: &Map, may_compile: bool) -> Option<MapRaw> {
    let mut cache = cache().lock().ok()?;
    if let Some(r) = cache.get(map) {
        return *r;
    }
    if !may_compile || cache.len() >= MAX_MAPS {
        return None;
    }
    let r = compile(map);
    cache.insert(map.clone(), r);
    r
}

#[cfg(feature = "llvm")]
fn compile(map: &Map) -> Option<MapRaw> {
    super::llvm::compile_map(map)
}

#[cfg(not(feature = "llvm"))]
fn compile(_: &Map) -> Option<MapRaw> {
    None
}
