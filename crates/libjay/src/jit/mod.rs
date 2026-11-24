//! Native code for scalar verbs, compiled at run time.
//!
//! Two kinds of loop run a verb the interpreter would otherwise apply one
//! atom at a time, each dispatch costing a walk through every node of the
//! verb:
//!
//! * the step of a fold — `x (] F.. v) y` and its relatives, the insert
//!   `v/` and the suffix scan `v/\.` over floats — where the step is a
//!   train such as `([ + 0.5 * ])` or an explicit definition such as
//!   `4 : 'y + ({. x) * ({: x) - y'`;
//! * a rank-0 application `u"0 y` or `x u"0 y` over numbers, where u is a
//!   scalar body: arithmetic, comparisons, an agenda `u0`u1@.v`, a power
//!   `u^:n`, and explicit definitions with `if.`, `while.` and `for.`.
//!
//! This module lowers the verb to a program, a scalar body with blocks
//! and loops, compiles the whole loop around it to machine code, and runs
//! that instead. `lower.rs` does the lowering; `clif.rs` and `llvm.rs`
//! compile the same program, each behind a Cargo feature: Cranelift
//! (`cranelift`, the default), which compiles in a fraction of a
//! millisecond, and LLVM (`llvm`), which takes a few milliseconds and
//! optimises harder. With neither, everything is interpreted.
//!
//! The compiled loop answers exactly what the interpreter answers, bit for
//! bit, whichever generator made it: every operation is the one
//! [`crate::scalar`] or the interpreter's tolerant comparison defines, on
//! the same operands in the same order. The arithmetic is emitted as the
//! IEEE instruction the definition consists of, with no reassociation and
//! no fused multiply-add; `<.` `>.` and `|` are the same compare and
//! select, and `^` and `^.` call the definitions themselves. Everywhere the
//! language's own rules differ from plain IEEE arithmetic — a NaN, an
//! infinity, `0 % 0`, an integer that overflows — the value involved is
//! not finite or out of bounds, so the loop checks every value it makes and
//! stops at the first that is not; so does an agenda index that is not a
//! whole number in range, a loop count that is not, and a `while.` loop
//! that runs past 2^24 iterations. The caller then runs the interpreter
//! over the whole application, which is free to answer, refuse or run for
//! ever as it always did.
//!
//! The same policy governs a third kind of compiled code, LLVM's alone:
//! the element-wise map of a fused float kernel, in `map.rs`. [`map_runs`]
//! counts the kernels it answered.
//!
//! `LIBJAY_JIT` chooses the policy once per process: `0` (or `off`) never
//! compiles; `force` compiles every verb it can lower whatever the length,
//! with the best generator built in; `cranelift` and `llvm` do the same
//! with that generator alone; anything else — the default — compiles with
//! Cranelift where the loop is long enough for the compilation to pay, and
//! with LLVM where it is long enough for LLVM's longer compilation to pay
//! too. [`with_mode`] overrides the policy on one thread, which is how the
//! differential tests run every road in one process. Compiled loops are
//! cached by the program they compute, the loop around it and the
//! generator that compiled them, and live as long as the process.

#[cfg(feature = "cranelift")]
mod clif;
#[cfg(feature = "llvm")]
mod llvm;
mod lower;
pub(crate) mod map;

pub use map::map_runs;

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::array::{Array, Data};
use crate::verb::{Tol, Verb};
use lower::{Kind, Kinds, Lower, Term};

/// A code generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Backend {
    /// Cranelift: compiles in a fraction of a millisecond.
    Cranelift,
    /// LLVM: compiles in a few milliseconds and optimises harder.
    Llvm,
}

impl Backend {
    /// Whether this generator is built into the library.
    pub const fn available(self) -> bool {
        match self {
            Backend::Cranelift => cfg!(feature = "cranelift"),
            Backend::Llvm => cfg!(feature = "llvm"),
        }
    }
}

/// The generators built into the library, the faster-compiling first.
pub fn backends() -> Vec<Backend> {
    [Backend::Cranelift, Backend::Llvm].into_iter().filter(|b| b.available()).collect()
}

/// When a verb is compiled, and by which generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JitMode {
    /// Never: everything is interpreted.
    Off,
    /// Where the loop is long enough for compiling to pay for itself.
    Auto,
    /// Wherever the verb can be lowered, however short the loop, with the
    /// best generator built in: LLVM where it is, otherwise Cranelift.
    Force,
    /// Wherever the verb can be lowered, with Cranelift alone. Nothing is
    /// compiled where Cranelift is not built in.
    Cranelift,
    /// Wherever the verb can be lowered, with LLVM alone. Nothing is
    /// compiled where LLVM is not built in.
    Llvm,
}

impl JitMode {
    /// The mode that forces generator `b`.
    pub fn forcing(b: Backend) -> JitMode {
        match b {
            Backend::Cranelift => JitMode::Cranelift,
            Backend::Llvm => JitMode::Llvm,
        }
    }
}

thread_local! {
    static OVERRIDE: Cell<Option<JitMode>> = const { Cell::new(None) };
    static RUNS: [Cell<u64>; 2] = const { [Cell::new(0), Cell::new(0)] };
}

/// How many folds and rank-0 applications this thread has answered with
/// compiled code, by either generator: what a test reads to know that the
/// compiled road was the one taken.
pub fn runs() -> u64 {
    RUNS.with(|r| r[0].get() + r[1].get())
}

/// How many folds and rank-0 applications this thread has answered with
/// code generator `b` made.
pub fn runs_by(b: Backend) -> u64 {
    RUNS.with(|r| r[b as usize].get())
}

fn note_run(b: Backend) {
    RUNS.with(|r| r[b as usize].set(r[b as usize].get() + 1));
}

/// The policy `LIBJAY_JIT` names, read once per process. A generator that
/// is named but not built in is reported on standard error, once, and the
/// automatic policy runs instead.
fn process_mode() -> JitMode {
    static MODE: OnceLock<JitMode> = OnceLock::new();
    *MODE.get_or_init(|| {
        let Ok(v) = std::env::var("LIBJAY_JIT") else { return JitMode::Auto };
        let name = v.trim().to_ascii_lowercase();
        let m = match name.as_str() {
            "0" | "off" | "false" | "no" => JitMode::Off,
            "force" | "always" => JitMode::Force,
            "cranelift" => JitMode::Cranelift,
            "llvm" => JitMode::Llvm,
            _ => JitMode::Auto,
        };
        let missing = match m {
            JitMode::Cranelift => !Backend::Cranelift.available(),
            JitMode::Llvm => !Backend::Llvm.available(),
            _ => false,
        };
        if missing {
            eprintln!(
                "libjay: LIBJAY_JIT={name}: this build has no {name} code generator \
                 (the `{name}` feature is off); the automatic policy applies"
            );
            return JitMode::Auto;
        }
        m
    })
}

/// The policy in force on this thread.
pub fn mode() -> JitMode {
    OVERRIDE.with(Cell::get).unwrap_or_else(process_mode)
}

/// Run `f` with the policy set to `m` on this thread, whatever
/// `LIBJAY_JIT` says. The previous setting is restored afterwards. Forcing
/// a generator that is not built in compiles nothing.
pub fn with_mode<R>(m: JitMode, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<JitMode>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(OVERRIDE.with(|c| c.replace(Some(m))));
    f()
}

/// Fewest iterations — fold steps or cells — for which an already compiled
/// loop is used under [`JitMode::Auto`]. Below it, lowering the verb costs
/// about what the interpreted iterations do.
const USE_STEPS: usize = 16;

/// Fewest iterations for which Cranelift compiles a loop under
/// [`JitMode::Auto`]. Compiling one costs a few hundred microseconds, which
/// is a few hundred interpreted steps of the cheapest train.
const COMPILE_STEPS: usize = 1024;

/// Fewest iterations for which LLVM compiles a loop under [`JitMode::Auto`]
/// where Cranelift is built in too. LLVM compiles a fold step in 6–9 ms
/// against Cranelift's half millisecond, and its loop is faster by 0 to
/// about 2 ns a step (measured over five million floats: a cusum gained
/// 1.2–1.9 ns, a clamped bucket about 0.25, KAMA up to 1.4). The extra
/// compilation is repaid after roughly four million steps on the steps
/// that gain most, so that is where LLVM starts; once compiled, its loop
/// also serves shorter loops of the same program.
const LLVM_STEPS: usize = 1 << 22;

/// [`LLVM_STEPS`] for a rank-0 application, whose cells cost more than a
/// fold's step. Measured on one thread over five million floats and a
/// million points, LLVM compiles a body in 10–30 ms against Cranelift's
/// 1–6, and its loop saves 7 ns a cell on a piecewise agenda, 13 on six
/// Newton steps and 390 on a Mandelbrot escape count; two million cells
/// repay the extra compilation on all but the lightest.
const LLVM_CELLS: usize = 1 << 21;

/// Fewest iterations for which LLVM compiles a loop under [`JitMode::Auto`]
/// where it is the only generator. Its compilation costs 6–9 ms, which is
/// five or six thousand interpreted steps of about 1.4 µs.
const LLVM_ALONE_STEPS: usize = 8192;

/// The most loops one process compiles with each generator. Each is a few
/// hundred bytes of code that is never freed; past this, verbs are
/// interpreted.
const MAX_KERNELS: usize = 1024;

/// The most iterations a compiled `while.` loop runs for one cell, and the
/// largest count a compiled `u^:n` or `for.` loop takes. Past it the loop
/// stops and the interpreter answers, or runs on for ever, as it would
/// have anyway.
pub(crate) const CAP: u64 = 1 << 24;

/// The bound on an integer the compiled code holds. Below 2^44 every sum,
/// difference and product of two integers is exact in double precision
/// and none overflows, and J's tolerant comparison of two integers is the
/// exact one — `|x - y| >= 1` exceeds the tolerance 2^-44 times either
/// magnitude. A larger integer stops the loop.
pub(crate) const WHOLE_BOUND: f64 = (1u64 << 44) as f64;

// ------------------------------------------------------------ the program

/// A variable of a lowered program: one float.
pub(crate) type Var = u32;

/// A monad of the lowered arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Un {
    Neg,
    Abs,
    Inc,
    Dec,
    Double,
    Halve,
    Square,
    OneMinus,
    Recip,
    Sqrt,
    Exp,
    Ln,
    /// J's tolerant floor: the ceiling where y is tolerantly equal to it,
    /// otherwise the floor.
    Floor,
    /// J's tolerant ceiling, the floor's mirror.
    Ceil,
}

/// A dyad of the lowered arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Bin {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    /// J's `x | y` on finite floats: y where x is zero, otherwise
    /// `y - x * <. y % x`, read as 0 where that product is tolerantly y.
    Residue,
    /// The logical and of two booleans.
    And,
    /// The logical or of two booleans.
    Or,
}

/// A tolerant comparison, answering 1 or 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// One instruction of a lowered program. Every variable a block reads was
/// written on every path to it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Ins {
    /// Argument k, as the loop around the program supplies it.
    Arg(Var, u32),
    /// A constant, as its bits.
    Const(Var, u64),
    Copy(Var, Var),
    Un(Var, Un, Var),
    Bin(Var, Bin, Var, Var),
    Cmp(Var, Cmp, Var, Var),
    /// The second value where the first is nonzero, otherwise the third.
    Select(Var, Var, Var, Var),
    /// Stop unless the value is finite.
    Finite(Var),
    /// Stop unless the value's magnitude is below [`WHOLE_BOUND`].
    Whole(Var),
    /// The first block where the value is nonzero, otherwise the second.
    If(Var, Vec<Ins>, Vec<Ins>),
    /// Block k of n where the value is k or k - n; stop where it is
    /// neither for any k.
    Switch(Var, Vec<Vec<Ins>>),
    /// Run the first block, and while the value it leaves is nonzero, the
    /// second and the first again; stop after [`CAP`] iterations.
    While(Vec<Ins>, Var, Vec<Ins>),
    /// Run `body` `count` times, with `index` 0, 1, … in turn; stop unless
    /// `count` is a whole number from 0 to [`CAP`].
    Repeat { count: Var, index: Var, body: Vec<Ins> },
}

/// The type of a program's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Out {
    /// Always this one.
    Static(Kind),
    /// The code of [`Kind`] this variable holds.
    Dyn(Var),
}

/// A lowered program: what both generators compile.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Prog {
    pub(crate) code: Vec<Ins>,
    pub(crate) vars: u32,
    pub(crate) result: Var,
    pub(crate) out: Out,
    /// The comparison tolerance, as its bits.
    pub(crate) ct: u64,
    /// Whether the tolerance scales by the smaller magnitude (J).
    pub(crate) by_smaller: bool,
}

/// How a loop reads one argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Repr {
    F64,
    I64,
    Bool,
}

impl Repr {
    fn of(a: &Array) -> Option<(Repr, Kind)> {
        if a.is_sparse() || !a.is_row_major() || a.count() == 0 {
            return None;
        }
        Some(match &a.data {
            Data::F64(b) if b.len() == a.count() => (Repr::F64, Kind::Float),
            Data::I64(b) if b.len() == a.count() => (Repr::I64, Kind::Int),
            Data::Bool(b) if b.len() == a.count() => (Repr::Bool, Kind::Bool),
            _ => return None,
        })
    }

    fn ptr(a: &Array) -> *const u8 {
        match &a.data {
            Data::F64(b) => b.as_slice().as_ptr().cast(),
            Data::I64(b) => b.as_slice().as_ptr().cast(),
            Data::Bool(b) => b.as_slice().as_ptr(),
            _ => std::ptr::null(),
        }
    }

    fn size(self) -> usize {
        match self {
            Repr::F64 | Repr::I64 => 8,
            Repr::Bool => 1,
        }
    }
}

/// The loop a program is compiled into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Shape {
    /// A fold: argument 0 is the running value, argument k + 1 element k
    /// of the item; whether every running value is written out.
    Fold { store: bool },
    /// A rank-0 application: argument 0 is y's atom, argument 1 x's.
    Cells { y: Repr, x: Option<Repr> },
}

/// A compiled fold loop:
///
/// `steps(xs, xstride, n, seed, out, ostride, last)` takes up to n steps.
/// Step i reads the item at `xs + i*xstride` (in elements) and the running
/// value, and where the loop stores, writes the new running value to
/// `out + i*ostride`. It stops before the first step that cannot be
/// completed, writes the running value reached to `last`, and answers the
/// number of steps taken.
type Raw = unsafe extern "C" fn(*const f64, i64, i64, f64, *mut f64, i64, *mut f64) -> i64;

/// A compiled rank-0 loop:
///
/// `cells(ys, ystride, xs, xstride, n, out, kind)` computes items `0 .. n`.
/// Item i reads y's atom at element `i*ystride` of `ys` and x's at
/// `i*xstride` of `xs`, each as the loop's [`Shape`] says, and writes the
/// answer to `out[i]`. It stops before the first item that cannot be
/// completed, writes the largest [`Kind`] code of the answers to `kind`,
/// and answers the number of items computed.
type CellsRaw =
    unsafe extern "C" fn(*const u8, i64, *const u8, i64, i64, *mut f64, *mut f64) -> i64;

/// A compiled loop, by whichever generator: its entry point's address,
/// whose signature its [`Shape`] says.
#[derive(Clone, Copy)]
struct Kernel(usize);

/// Every loop compiled so far, None where the compilation failed.
type Cache = HashMap<(Shape, Prog, Backend), Option<Kernel>>;

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Compile a program's loop with generator `b`. None where `b` is not
/// built in, the host has no backend for it, or the compilation fails.
fn compile(shape: Shape, prog: &Prog, b: Backend) -> Option<Kernel> {
    match b {
        #[cfg(feature = "cranelift")]
        Backend::Cranelift => clif::compile(shape, prog).map(Kernel),
        #[cfg(feature = "llvm")]
        Backend::Llvm => llvm::compile(shape, prog).map(Kernel),
        #[allow(unreachable_patterns)]
        _ => {
            let _ = (shape, prog);
            None
        }
    }
}

/// The loop for `prog` from generator `b`, compiled now if it is not
/// cached and `may_compile` allows it.
fn cached_or_compiled(
    cache: &mut Cache,
    shape: Shape,
    prog: &Prog,
    b: Backend,
    may_compile: bool,
) -> Option<Kernel> {
    let key = (shape, prog.clone(), b);
    if let Some(k) = cache.get(&key) {
        return *k;
    }
    if !may_compile || !b.available() || cache.keys().filter(|k| k.2 == b).count() >= MAX_KERNELS {
        return None;
    }
    let k = compile(shape, prog, b);
    cache.insert(key, k);
    k
}

/// The compiled loop for `prog`, under the policy in force for a loop of
/// `steps` iterations, and the generator that made it.
///
/// Under [`JitMode::Auto`], with both generators built in, a loop of
/// [`LLVM_STEPS`] iterations or more ([`LLVM_CELLS`] cells for a rank-0
/// application) is compiled by LLVM and a shorter one of [`COMPILE_STEPS`]
/// or more by Cranelift; a loop LLVM has already
/// compiled serves any loop of [`USE_STEPS`] or more. With one generator
/// built in, it compiles from its own threshold.
fn kernel(shape: Shape, prog: &Prog, steps: usize) -> Option<(Kernel, Backend)> {
    let m = mode();
    let mut cache = cache().lock().ok()?;
    let forced = match m {
        JitMode::Off => return None,
        JitMode::Force => backends().pop(),
        JitMode::Cranelift => Some(Backend::Cranelift),
        JitMode::Llvm => Some(Backend::Llvm),
        JitMode::Auto => None,
    };
    if let Some(b) = forced {
        return cached_or_compiled(&mut cache, shape, prog, b, true).map(|k| (k, b));
    }
    let (cl, ll) = (Backend::Cranelift, Backend::Llvm);
    let llvm_from = match shape {
        _ if !cl.available() => LLVM_ALONE_STEPS,
        Shape::Fold { .. } => LLVM_STEPS,
        Shape::Cells { .. } => LLVM_CELLS,
    };
    let from = |b: Backend| if b == ll { llvm_from } else { COMPILE_STEPS };
    let (want, other) = if !cl.available() || steps >= llvm_from { (ll, cl) } else { (cl, ll) };
    // LLVM's loop, once compiled, serves a loop of any length.
    if want == cl
        && let Some(Some(k)) = cache.get(&(shape, prog.clone(), ll))
    {
        return Some((*k, ll));
    }
    if let Some(k) = cached_or_compiled(&mut cache, shape, prog, want, steps >= from(want)) {
        return Some((k, want));
    }
    cached_or_compiled(&mut cache, shape, prog, other, steps >= from(other)).map(|k| (k, other))
}

/// Whether the policy may compile or use a loop of `steps` iterations.
fn worth_lowering(steps: usize) -> bool {
    match mode() {
        JitMode::Off => false,
        JitMode::Auto => steps >= USE_STEPS,
        _ => true,
    }
}

// ------------------------------------------------------------- the folds

/// Lower `x v y` where x is an item of `width` floats (0 for an atom) and
/// y the running float. None where any part of the verb is not something
/// this module compiles, or the step's answer is not always a float.
fn lower_fold(
    v: &Verb,
    width: usize,
    tol: Tol,
    resolve: &dyn Fn(&str) -> Option<Verb>,
) -> Option<Prog> {
    let mut l = Lower::new(resolve, tol);
    let y = Term::Atom(l.arg(0, Kind::Float)?);
    let x = match width {
        0 => Term::Atom(l.arg(1, Kind::Float)?),
        k => Term::Row(u32::try_from(k).ok()?),
    };
    let Term::Atom(root) = l.dyad(v, x, y)? else { return None };
    // A step that answers a boolean or an integer answers it in that type.
    if root.k != Kinds::one(Kind::Float) {
        return None;
    }
    l.finish(root)
}

/// The floats of a dense row-major float array, or None.
fn floats(a: &Array) -> Option<&[f64]> {
    if a.is_sparse() || !a.is_row_major() {
        return None;
    }
    match &a.data {
        Data::F64(b) if b.len() == a.count() => Some(b.as_slice()),
        _ => None,
    }
}

/// Run `x v acc` over `n` items of `y` from item `first`, stepping by
/// `dir` items (1 or -1), with `width` floats to an item (0 for atoms).
/// Every running value is written to `out` in step order when one is
/// given. None where no loop runs or it stopped; the caller then
/// interprets the fold from the start.
#[allow(clippy::too_many_arguments)]
fn run(
    v: &Verb,
    ys: &[f64],
    width: usize,
    first: usize,
    n: usize,
    dir: i64,
    seed: f64,
    out: Option<(&mut [f64], i64)>,
    tol: Tol,
    resolve: &dyn Fn(&str) -> Option<Verb>,
) -> Option<f64> {
    if n == 0 || !seed.is_finite() || !worth_lowering(n) {
        return None;
    }
    let w = width.max(1);
    // Every item the loop reads lies inside the buffer.
    let lo = if dir > 0 { first } else { first.checked_sub(n - 1)? };
    if (lo + n) * w > ys.len() {
        return None;
    }
    if let Some((o, s)) = &out {
        let need = if *s == 0 { 1 } else { n };
        let start = if *s > 0 { 0 } else { need - 1 };
        if o.len() < need || start >= o.len() {
            return None;
        }
    }
    let prog = lower_fold(v, width, tol, resolve)?;
    let (k, backend) = kernel(Shape::Fold { store: out.is_some() }, &prog, n)?;
    let xs = ys[first * w..].as_ptr();
    let (op, ostride) = match out {
        Some((o, s)) if s < 0 => (o[n - 1..].as_mut_ptr(), s),
        Some((o, s)) => (o.as_mut_ptr(), s),
        None => (std::ptr::null_mut(), 0),
    };
    let mut last = seed;
    // SAFETY: the kernel was compiled for a fold, with [`Raw`]'s signature.
    // The loop reads items `first`, `first + dir`, … n of them, all checked
    // to lie inside `ys` above, and writes n floats to `out` along its
    // stride, which was checked to hold them.
    let taken = unsafe {
        let f = std::mem::transmute::<usize, Raw>(k.0);
        f(xs, dir * w as i64, n as i64, seed, op, ostride, &mut last)
    };
    if taken != n as i64 {
        return None;
    }
    note_run(backend);
    Some(last)
}

/// `x (] F.. v) y` and its three relatives over float data, compiled.
/// `multiple` keeps every running value, `reverse` takes the items from
/// the back. A seed that is an integer or a boolean atom is first stepped
/// by `first` — the interpreter — so that the type of the first running
/// value is the interpreter's to decide. None leaves the fold to the
/// interpreter.
pub(crate) fn fold(
    v: &Verb,
    multiple: bool,
    reverse: bool,
    x: Option<&Array>,
    y: &Array,
    tol: Tol,
    first: &mut dyn FnMut(&Array, &Array) -> Option<Array>,
) -> Option<Array> {
    let m = mode();
    if m == JitMode::Off || !(1..=2).contains(&y.rank()) {
        return None;
    }
    let ys = floats(y)?;
    let items = y.items();
    let width = if y.rank() == 1 { 0 } else { y.shape[1] };
    if width == 0 && y.rank() == 2 {
        return None;
    }
    let at = |i: usize| if reverse { items - 1 - i } else { i };
    let mut kept = Vec::new();
    let (seed, from) = match x {
        Some(a) if a.rank() == 0 && matches!(a.data, Data::F64(_)) => (*floats(a)?.first()?, 0),
        Some(a)
            if a.rank() == 0
                && !a.is_sparse()
                && matches!(a.data, Data::I64(_) | Data::Bool(_))
                && items > 0 =>
        {
            // The first step is taken here only where the rest of the fold
            // would be compiled.
            if !worth_lowering(items) {
                return None;
            }
            lower_fold(v, width, tol, &|_| None)?;
            let stepped = first(&y.cell_at(1, at(0)), a)?;
            if stepped.rank() != 0 {
                return None;
            }
            let s = *floats(&stepped)?.first()?;
            kept.push(s);
            (s, 1)
        }
        Some(_) => return None,
        // With no left argument the first item is the running value, which
        // has to be one float.
        None if width == 0 && items > 0 => (ys[at(0)], 1),
        None => return None,
    };
    if items < from || (items == from && kept.is_empty()) {
        return None;
    }
    let n = items - from;
    let step = if reverse { -1 } else { 1 };
    let last = if n == 0 {
        // The interpreted first step was the only one.
        seed
    } else if multiple {
        let mut buf = vec![0.0; n];
        let last =
            run(v, ys, width, at(from), n, step, seed, Some((&mut buf, 1)), tol, &|_| None)?;
        kept.extend_from_slice(&buf);
        last
    } else {
        run(v, ys, width, at(from), n, step, seed, None, tol, &|_| None)?
    };
    Some(match multiple {
        true => Array::new(vec![kept.len()], Data::F64(kept.into())),
        false => Array::scalar_f64(last),
    })
}

/// `v/ y` over a float list, folding right to left. None leaves it to the
/// interpreter.
pub(crate) fn insert(
    v: &Verb,
    y: &Array,
    tol: Tol,
    resolve: &dyn Fn(&str) -> Option<Verb>,
) -> Option<Array> {
    if mode() == JitMode::Off || y.rank() != 1 {
        return None;
    }
    let ys = floats(y)?;
    let n = ys.len();
    if n < 2 {
        return None;
    }
    let last = run(v, ys, 0, n - 2, n - 1, -1, ys[n - 1], None, tol, resolve)?;
    Some(Array::scalar_f64(last))
}

/// `v/\. y` over a float list: each suffix folded, carried from the back.
/// None leaves it to the interpreter.
pub(crate) fn suffix_scan(
    v: &Verb,
    y: &Array,
    tol: Tol,
    resolve: &dyn Fn(&str) -> Option<Verb>,
) -> Option<Array> {
    if mode() == JitMode::Off || y.rank() != 1 {
        return None;
    }
    let ys = floats(y)?;
    let n = ys.len();
    if n < 2 {
        return None;
    }
    let mut buf = vec![0.0; n];
    buf[n - 1] = ys[n - 1];
    let (head, _) = buf.split_at_mut(n - 1);
    run(v, ys, 0, n - 2, n - 1, -1, ys[n - 1], Some((head, -1)), tol, resolve)?;
    Some(Array::new(vec![n], Data::F64(buf.into())))
}

// ------------------------------------------------------------- the cells

/// One argument of a rank-0 application as the loop reads it: where its
/// atoms start, how it is read, its type, and whether it is one atom
/// spread over every cell.
struct Side<'a> {
    a: &'a Array,
    repr: Repr,
    kind: Kind,
    spread: bool,
}

impl<'a> Side<'a> {
    fn of(a: &'a Array, frame: &[usize]) -> Option<Side<'a>> {
        let (repr, kind) = Repr::of(a)?;
        let spread = if a.shape.as_slice() == frame {
            false
        } else if a.rank() == 0 {
            true
        } else {
            return None;
        };
        Some(Side { a, repr, kind, spread })
    }
}

/// `u"0 y` and `x u"0 y` over numbers, compiled: `v` applied to every
/// atom of y, or to every pair of atoms of x and y, where x and y have the
/// same shape or one of them is an atom. The answer has the cells' frame
/// and the type the interpreter's framing gives them. None leaves the
/// application to the interpreter.
pub(crate) fn cells(
    v: &Verb,
    x: Option<&Array>,
    y: &Array,
    tol: Tol,
    resolve: &dyn Fn(&str) -> Option<Verb>,
) -> Option<Array> {
    if !tol.is_j() {
        return None;
    }
    let frame: &[usize] = match x {
        Some(x) if x.rank() > y.rank() => &x.shape,
        _ => &y.shape,
    };
    let n: usize = frame.iter().product();
    if n == 0 || !worth_lowering(n) {
        return None;
    }
    let ys = Side::of(y, frame)?;
    let xs = match x {
        Some(x) => Some(Side::of(x, frame)?),
        None => None,
    };
    let mut l = Lower::new(resolve, tol);
    let yt = Term::Atom(l.arg(0, ys.kind)?);
    let t = match &xs {
        Some(s) => {
            let xt = Term::Atom(l.arg(1, s.kind)?);
            l.dyad(v, xt, yt)?
        }
        None => l.monad(v, yt)?,
    };
    let Term::Atom(root) = t else { return None };
    let prog = l.finish(root)?;
    let shape = Shape::Cells { y: ys.repr, x: xs.as_ref().map(|s| s.repr) };
    let (k, backend) = kernel(shape, &prog, n)?;
    // SAFETY: the kernel was compiled for this shape, with [`CellsRaw`]'s
    // signature.
    let f = unsafe { std::mem::transmute::<usize, CellsRaw>(k.0) };
    let base = |s: &Side<'_>| (Repr::ptr(s.a), s.repr.size(), if s.spread { 0 } else { 1 });
    let (yp, ysz, ystride) = base(&ys);
    let (xp, xsz, xstride) = match &xs {
        Some(s) => base(s),
        None => (std::ptr::null(), 0, 0),
    };
    let (yp, xp) = (yp as usize, xp as usize);
    let top = AtomicU8::new(0);
    let (out, ok) = crate::par::fill(n, |start, part: &mut [f64]| {
        let mut kind = 0.0;
        // SAFETY: each argument holds n atoms, or one spread over every
        // cell (stride 0), so items `start .. start + part.len()` lie
        // inside it; `part` is this chunk's alone.
        let done = unsafe {
            f(
                (yp + start * ystride as usize * ysz) as *const u8,
                ystride,
                (xp + start * xstride as usize * xsz) as *const u8,
                xstride,
                part.len() as i64,
                part.as_mut_ptr(),
                &mut kind,
            )
        };
        top.fetch_max(kind as u8, Ordering::Relaxed);
        done == part.len() as i64
    });
    if !ok {
        return None;
    }
    let data = match top.load(Ordering::Relaxed) {
        2 => Data::F64(out.into()),
        1 => Data::I64(out.iter().map(|&v| v as i64).collect::<Vec<_>>().into()),
        _ => Data::Bool(out.iter().map(|&v| v as u8).collect::<Vec<_>>().into()),
    };
    note_run(backend);
    Some(Array::new(frame.to_vec(), data))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prog(code: Vec<Ins>, vars: u32, result: Var) -> Prog {
        Prog {
            code,
            vars,
            result,
            out: Out::Static(Kind::Float),
            ct: Tol::J.ct.to_bits(),
            by_smaller: true,
        }
    }

    fn fold_kernel(p: &Prog, store: bool, b: Backend) -> Raw {
        let k = compile(Shape::Fold { store }, p, b).expect("the host has a backend");
        unsafe { std::mem::transmute::<usize, Raw>(k.0) }
    }

    #[test]
    fn a_loop_stops_at_the_first_value_that_is_not_finite() {
        // x % y, the running value.
        let p = prog(
            vec![Ins::Arg(0, 0), Ins::Arg(1, 1), Ins::Finite(1), Ins::Bin(2, Bin::Div, 1, 0), Ins::Finite(2)],
            3,
            2,
        );
        for b in backends() {
            let f = fold_kernel(&p, false, b);
            let xs = [1.0, 0.0, 2.0];
            let mut last = 0.0;
            // 1 % 4, then 0 % 0.25, then 2 % 0: the third is not finite.
            let taken = unsafe { f(xs.as_ptr(), 1, 3, 4.0, std::ptr::null_mut(), 0, &mut last) };
            assert_eq!(taken, 2, "{b:?}");
            assert_eq!(last, 0.0, "{b:?}");
        }
    }

    /// `x * y + x`, stored at every step, against the same arithmetic in
    /// Rust: a generator that fused the product and the sum into one
    /// multiply-add would round once where Rust rounds twice.
    #[test]
    fn a_product_and_a_sum_stay_two_roundings() {
        let p = prog(
            vec![
                Ins::Arg(0, 0),
                Ins::Arg(1, 1),
                Ins::Bin(2, Bin::Mul, 1, 0),
                Ins::Bin(3, Bin::Add, 2, 1),
                Ins::Finite(3),
            ],
            4,
            3,
        );
        let mut s = 0x2545_f491_4f6c_dd1du64;
        let xs: Vec<f64> = (0..4096)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s >> 11) as f64 / (1u64 << 53) as f64 - 0.5
            })
            .collect();
        let mut want = Vec::with_capacity(xs.len());
        let mut fused = Vec::with_capacity(xs.len());
        let (mut acc, mut facc) = (0.3f64, 0.3f64);
        for &x in &xs {
            acc = std::hint::black_box(x * acc) + x;
            want.push(acc.to_bits());
            facc = x.mul_add(facc, x);
            fused.push(facc.to_bits());
        }
        // The data tells the two apart.
        assert!(want != fused);
        for b in backends() {
            let f = fold_kernel(&p, true, b);
            let mut out = vec![0.0; xs.len()];
            let mut last = 0.0;
            let n = xs.len() as i64;
            let taken = unsafe { f(xs.as_ptr(), 1, n, 0.3, out.as_mut_ptr(), 1, &mut last) };
            assert_eq!(taken, n, "{b:?}");
            let got: Vec<u64> = out.iter().map(|v| v.to_bits()).collect();
            assert!(got == want, "{b:?}: the loop rounds differently from Rust");
        }
    }

    /// A `while.` loop that never ends stops at the iteration cap rather
    /// than running for ever.
    #[test]
    fn an_endless_while_loop_stops_at_the_cap() {
        // while. 1 do. end. y
        let p = Prog {
            code: vec![Ins::Arg(0, 0), Ins::Const(1, 1f64.to_bits()), Ins::While(vec![], 1, vec![])],
            vars: 2,
            result: 0,
            out: Out::Static(Kind::Float),
            ct: Tol::J.ct.to_bits(),
            by_smaller: true,
        };
        for b in backends() {
            let k = compile(Shape::Cells { y: Repr::F64, x: None }, &p, b).expect("a backend");
            let f = unsafe { std::mem::transmute::<usize, CellsRaw>(k.0) };
            let ys = [1.5f64];
            let mut out = [0.0];
            let mut kind = 0.0;
            let done = unsafe {
                f(ys.as_ptr().cast(), 1, std::ptr::null(), 0, 1, out.as_mut_ptr(), &mut kind)
            };
            assert_eq!(done, 0, "{b:?}");
        }
    }
}
