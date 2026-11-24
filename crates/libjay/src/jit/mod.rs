//! Native code for the step of a fold, compiled at run time.
//!
//! A fold whose step is a verb the interpreter has no typed loop for — a
//! train such as `([ + 0.5 * ])` or an explicit definition such as
//! `4 : 'y + ({. x) * ({: x) - y'` — is interpreted one step at a time,
//! and each step costs a dispatch through every node of the verb. When the
//! step computes one float from the item and the running value with
//! nothing but arithmetic, this module lowers it to a scalar expression,
//! compiles the whole loop over the items to machine code, and runs that
//! instead.
//!
//! Two code generators compile the same lowered step to the same loop,
//! each behind a Cargo feature: Cranelift (`cranelift`, the default), which
//! compiles in a fraction of a millisecond, and LLVM (`llvm`), which takes
//! a few milliseconds and optimises harder. With neither, every step is
//! interpreted.
//!
//! The compiled loop answers exactly what the interpreter answers, bit for
//! bit, whichever generator made it: every operation is the one
//! [`crate::scalar`] defines for the primitive, which the interpreter uses
//! too, on the same operands in the same order. The arithmetic is emitted
//! as the IEEE instruction the definition consists of, with no
//! reassociation and no fused multiply-add; `<.` `>.` and `|` are the same
//! compare and select, and `^` and `^.` call the definitions themselves.
//! Everywhere the language's own rules differ from plain IEEE arithmetic —
//! a NaN, an infinity, `0 % 0` — the value involved is not finite, so the
//! loop checks every value it makes and stops at the first that is not.
//! The caller then runs the interpreter over the whole fold, which is free
//! to answer or refuse as it always did. Only float data takes this road;
//! the integer types keep their overflow rules in the interpreter.
//!
//! The same policy governs a second kind of compiled code, LLVM's alone:
//! the element-wise map of a fused float kernel, in `map.rs`. [`map_runs`]
//! counts the kernels it answered.
//!
//! `LIBJAY_JIT` chooses the policy once per process: `0` (or `off`) never
//! compiles; `force` compiles every step it can lower whatever the fold's
//! length, with the best generator built in; `cranelift` and `llvm` do the
//! same with that generator alone; anything else — the default — compiles
//! with Cranelift where the fold is long enough for the compilation to
//! pay, and with LLVM where it is long enough for LLVM's longer
//! compilation to pay too. [`with_mode`] overrides the policy on one
//! thread, which is how the differential tests run every road in one
//! process. Compiled loops are cached by the expression they compute and
//! the generator that compiled them, and live as long as the process.

#[cfg(feature = "cranelift")]
mod clif;
#[cfg(feature = "llvm")]
mod llvm;
pub(crate) mod map;

pub use map::map_runs;

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::array::{Array, Data};
use crate::ir::{ExplicitDef, Expr, Scope};
use crate::verb::{DyadOp, MonadOp, ScalarDyad, ScalarMonad, Verb, RANK_INF};

/// A code generator for the fold loop.
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

/// When the fold's step is compiled, and by which generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JitMode {
    /// Never: every step is interpreted.
    Off,
    /// Where the fold is long enough for compiling to pay for itself.
    Auto,
    /// Wherever the step can be lowered, however short the fold, with the
    /// best generator built in: LLVM where it is, otherwise Cranelift.
    Force,
    /// Wherever the step can be lowered, with Cranelift alone. Nothing is
    /// compiled where Cranelift is not built in.
    Cranelift,
    /// Wherever the step can be lowered, with LLVM alone. Nothing is
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

/// How many folds this thread has answered with compiled code, by either
/// generator: what a test reads to know that the compiled road was the one
/// taken.
pub fn runs() -> u64 {
    RUNS.with(|r| r[0].get() + r[1].get())
}

/// How many folds this thread has answered with code generator `b` made.
pub fn runs_by(b: Backend) -> u64 {
    RUNS.with(|r| r[b as usize].get())
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

/// Fewest steps for which an already compiled loop is used under
/// [`JitMode::Auto`]. Below it, lowering the verb costs about what the
/// interpreted steps do.
const USE_STEPS: usize = 16;

/// Fewest steps for which Cranelift compiles a loop under
/// [`JitMode::Auto`]. Compiling one costs a few hundred microseconds, which
/// is a few hundred interpreted steps of the cheapest train.
const COMPILE_STEPS: usize = 1024;

/// Fewest steps for which LLVM compiles a loop under [`JitMode::Auto`]
/// where Cranelift is built in too. LLVM compiles a fold step in 6–9 ms
/// against Cranelift's half millisecond, and its loop is faster by 0 to
/// about 2 ns a step (measured over five million floats: a cusum gained
/// 1.2–1.9 ns, a clamped bucket about 0.25, KAMA up to 1.4). The extra
/// compilation is repaid after roughly four million steps on the steps
/// that gain most, so that is where LLVM starts; once compiled, its loop
/// also serves shorter folds of the same step.
const LLVM_STEPS: usize = 1 << 22;

/// Fewest steps for which LLVM compiles a loop under [`JitMode::Auto`]
/// where it is the only generator. Its compilation costs 6–9 ms, which is
/// five or six thousand interpreted steps of about 1.4 µs.
const LLVM_ALONE_STEPS: usize = 8192;

/// The most loops one process compiles. Each is a few hundred bytes of
/// code that is never freed; past this, steps are interpreted.
const MAX_KERNELS: usize = 1024;

/// The largest step lowered, in expression nodes.
const MAX_NODES: usize = 512;

/// How deeply verbs and definitions may nest inside one step.
const MAX_DEPTH: usize = 64;

// ------------------------------------------------------------- the step

/// One operation of a lowered step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Node {
    /// Element k of the item (an atom item is element 0).
    X(u32),
    /// The running value.
    Y,
    /// A constant, as the bits of the float it widens to.
    Const(u64),
    Un(Un, u32),
    Bin(Bin, u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Un {
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Bin {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
}

/// A lowered step: the nodes in evaluation order and which one answers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Step {
    nodes: Vec<Node>,
    root: u32,
    /// Whether the loop writes every running value out.
    store: bool,
}

/// What a sub-expression stands for while a verb is lowered.
#[derive(Clone, Copy, Debug)]
enum Val {
    /// One float, made by the node with this index.
    Atom(u32),
    /// The whole item, a list of this many floats.
    Row(u32),
}

struct Lower<'a> {
    nodes: Vec<Node>,
    memo: HashMap<Node, u32>,
    resolve: &'a dyn Fn(&str) -> Option<Verb>,
    depth: usize,
}

impl Lower<'_> {
    fn push(&mut self, n: Node) -> Option<u32> {
        if let Some(&i) = self.memo.get(&n) {
            return Some(i);
        }
        if self.nodes.len() >= MAX_NODES {
            return None;
        }
        let i = self.nodes.len() as u32;
        self.nodes.push(n);
        self.memo.insert(n, i);
        Some(i)
    }

    fn is_const(&self, i: u32) -> bool {
        matches!(self.nodes[i as usize], Node::Const(_))
    }

    /// A numeric atom written in the source, as a float node. A value that
    /// does not widen to a float exactly is left to the interpreter.
    fn constant(&mut self, a: &Array) -> Option<Val> {
        if a.rank() != 0 || a.is_sparse() {
            return None;
        }
        let v = match &a.data {
            Data::Bool(b) => f64::from(*b.first()?),
            Data::I64(b) => {
                let k = *b.first()?;
                if k.unsigned_abs() > (1u64 << 53) {
                    return None;
                }
                k as f64
            }
            Data::F64(b) => *b.first()?,
            _ => return None,
        };
        if !v.is_finite() {
            return None;
        }
        Some(Val::Atom(self.push(Node::Const(v.to_bits()))?))
    }

    fn un(&mut self, op: Un, a: Val) -> Option<Val> {
        let Val::Atom(a) = a else { return None };
        // A verb of constants alone is computed in the constants' own type,
        // which may not be a float; the interpreter keeps it.
        if self.is_const(a) {
            return None;
        }
        Some(Val::Atom(self.push(Node::Un(op, a))?))
    }

    fn bin(&mut self, op: Bin, a: Val, b: Val) -> Option<Val> {
        let (Val::Atom(a), Val::Atom(b)) = (a, b) else { return None };
        if self.is_const(a) && self.is_const(b) {
            return None;
        }
        Some(Val::Atom(self.push(Node::Bin(op, a, b))?))
    }

    fn enter(&mut self) -> Option<()> {
        self.depth += 1;
        (self.depth <= MAX_DEPTH).then_some(())
    }

    /// `u y`.
    fn monad(&mut self, v: &Verb, y: Val) -> Option<Val> {
        self.enter()?;
        let r = self.monad_inner(v, y);
        self.depth -= 1;
        r
    }

    /// `x u y`.
    fn dyad(&mut self, v: &Verb, x: Val, y: Val) -> Option<Val> {
        self.enter()?;
        let r = self.dyad_inner(v, x, y);
        self.depth -= 1;
        r
    }

    fn monad_inner(&mut self, v: &Verb, y: Val) -> Option<Val> {
        match v {
            Verb::Prim(p) => match p.monad {
                MonadOp::Same => Some(y),
                MonadOp::Head => match y {
                    Val::Row(0) => None,
                    Val::Row(_) => Some(Val::Atom(self.push(Node::X(0))?)),
                    a => Some(a),
                },
                MonadOp::Tail => match y {
                    Val::Row(0) => None,
                    Val::Row(k) => Some(Val::Atom(self.push(Node::X(k - 1))?)),
                    a => Some(a),
                },
                MonadOp::Scalar(op) => {
                    use ScalarMonad as M;
                    let u = match op {
                        // The identity on reals.
                        M::Conj => return matches!(y, Val::Atom(_)).then_some(y),
                        M::Neg => Un::Neg,
                        M::Abs => Un::Abs,
                        M::Inc => Un::Inc,
                        M::Dec => Un::Dec,
                        M::Double => Un::Double,
                        M::Halve => Un::Halve,
                        M::Square => Un::Square,
                        M::OneMinus => Un::OneMinus,
                        M::Recip => Un::Recip,
                        M::Sqrt => Un::Sqrt,
                        M::Exp => Un::Exp,
                        M::Ln => Un::Ln,
                        _ => return None,
                    };
                    self.un(u, y)
                }
                _ => None,
            },
            Verb::Rank(u, r) => {
                if matches!(y, Val::Row(_)) && !whole(r[0]) {
                    return None;
                }
                self.monad(u, y)
            }
            Verb::Fork(f, g, h) => {
                let l = self.monad(f, y)?;
                let r = self.monad(h, y)?;
                self.dyad(g, l, r)
            }
            Verb::NounFork(n, g, h) => {
                let r = self.monad(h, y)?;
                let l = self.constant(n)?;
                self.dyad(g, l, r)
            }
            Verb::Hook(f, g) => {
                let r = self.monad(g, y)?;
                self.dyad(f, y, r)
            }
            Verb::Atop(f, g, _) | Verb::Compose(f, g) | Verb::Beside(f, g) => {
                let r = self.monad(g, y)?;
                self.monad(f, r)
            }
            Verb::BondLeft(m, u) => {
                let l = self.constant(m)?;
                self.dyad(u, l, y)
            }
            Verb::BondRight(u, m) => {
                let r = self.constant(m)?;
                self.dyad(u, y, r)
            }
            Verb::Commute(u) => self.dyad(u, y, y),
            Verb::WithObverse(u, _) => self.monad(u, y),
            Verb::Ambivalent(u, _) => self.monad(u, y),
            Verb::Explicit(d) => self.explicit(d, None, y),
            _ => None,
        }
    }

    fn dyad_inner(&mut self, v: &Verb, x: Val, y: Val) -> Option<Val> {
        match v {
            Verb::Prim(p) => match p.dyad {
                DyadOp::Left => Some(x),
                DyadOp::Right => Some(y),
                DyadOp::From => {
                    let (Val::Atom(i), Val::Row(k)) = (x, y) else { return None };
                    let Node::Const(bits) = self.nodes[i as usize] else { return None };
                    let f = f64::from_bits(bits);
                    if f.fract() != 0.0 {
                        return None;
                    }
                    let k = i64::from(k);
                    let i = f as i64;
                    let at = if i < 0 { i + k } else { i };
                    if !(0..k).contains(&at) {
                        return None;
                    }
                    Some(Val::Atom(self.push(Node::X(at as u32))?))
                }
                DyadOp::Scalar(op) => {
                    use ScalarDyad as D;
                    let b = match op {
                        D::Add => Bin::Add,
                        D::Sub => Bin::Sub,
                        D::Mul => Bin::Mul,
                        D::DivJ => Bin::Div,
                        D::Min => Bin::Min,
                        D::Max => Bin::Max,
                        _ => return None,
                    };
                    self.bin(b, x, y)
                }
                _ => None,
            },
            Verb::Rank(u, r) => {
                if matches!(x, Val::Row(_)) && !whole(r[1])
                    || matches!(y, Val::Row(_)) && !whole(r[2])
                {
                    return None;
                }
                self.dyad(u, x, y)
            }
            Verb::Fork(f, g, h) => {
                let l = self.dyad(f, x, y)?;
                let r = self.dyad(h, x, y)?;
                self.dyad(g, l, r)
            }
            Verb::NounFork(n, g, h) => {
                let r = self.dyad(h, x, y)?;
                let l = self.constant(n)?;
                self.dyad(g, l, r)
            }
            Verb::Hook(f, g) => {
                let r = self.monad(g, y)?;
                self.dyad(f, x, r)
            }
            Verb::Atop(f, g, _) => {
                let r = self.dyad(g, x, y)?;
                self.monad(f, r)
            }
            Verb::Compose(f, g) => {
                let l = self.monad(g, x)?;
                let r = self.monad(g, y)?;
                self.dyad(f, l, r)
            }
            Verb::Beside(f, g) => {
                let r = self.monad(g, y)?;
                self.dyad(f, x, r)
            }
            Verb::Commute(u) => self.dyad(u, y, x),
            Verb::WithObverse(u, _) => self.dyad(u, x, y),
            Verb::Ambivalent(_, u) => self.dyad(u, x, y),
            Verb::Explicit(d) => self.explicit(d, Some(x), y),
            Verb::Named(n) if self.depth == 1 => {
                let u = (self.resolve)(n)?;
                self.dyad(&u, x, y)
            }
            _ => None,
        }
    }

    /// An explicit definition whose body is a run of sentences, each an
    /// expression or a local assignment, the last an expression.
    fn explicit(&mut self, d: &ExplicitDef, x: Option<Val>, y: Val) -> Option<Val> {
        if !d.pure
            || d.result.is_some()
            || d.axis.is_some()
            || !d.labels.is_empty()
            || !d.enclosing.is_empty()
            || d.body.is_empty()
        {
            return None;
        }
        let mut names: Vec<(&str, Val)> = vec![(d.right.as_str(), y)];
        match (&d.left, x) {
            (Some(l), Some(x)) => names.push((l.as_str(), x)),
            (None, None) => {}
            _ => return None,
        }
        let (last, init) = d.body.split_last()?;
        for e in init {
            match e {
                Expr::Assign { name, value, scope: Scope::Local, .. } => {
                    let v = self.expr(value, &names)?;
                    names.push((name.as_str(), v));
                }
                _ => return None,
            }
        }
        match last {
            Expr::Assign { .. } => None,
            e => self.expr(e, &names),
        }
    }

    fn expr(&mut self, e: &Expr, names: &[(&str, Val)]) -> Option<Val> {
        self.enter()?;
        let r = match e {
            Expr::Const(a, _) => self.constant(a),
            Expr::Name(n, _) => names.iter().rev().find(|(k, _)| k == n).map(|&(_, v)| v),
            Expr::Monad { verb, y, .. } => {
                let y = self.expr(y, names)?;
                self.monad(verb, y)
            }
            Expr::Dyad { verb, x, y, .. } => {
                let y = self.expr(y, names)?;
                let x = self.expr(x, names)?;
                self.dyad(verb, x, y)
            }
            Expr::Fused { orig, .. } => self.expr(orig, names),
            _ => None,
        };
        self.depth -= 1;
        r
    }
}

/// Whether a cell rank takes a list whole.
fn whole(r: i64) -> bool {
    r >= 1 || r == RANK_INF
}

/// Lower `x v y` where x is an item of `width` floats (0 for an atom) and y
/// the running float. None where any part of the verb is not arithmetic on
/// floats this module compiles.
fn lower(
    v: &Verb,
    width: usize,
    store: bool,
    resolve: &dyn Fn(&str) -> Option<Verb>,
) -> Option<Step> {
    let mut l = Lower { nodes: Vec::new(), memo: HashMap::new(), resolve, depth: 0 };
    let x = match width {
        0 => Val::Atom(l.push(Node::X(0))?),
        k => Val::Row(u32::try_from(k).ok()?),
    };
    let y = Val::Atom(l.push(Node::Y)?);
    let Val::Atom(root) = l.dyad(v, x, y)? else { return None };
    // A step that answers a constant answers it in the constant's own type.
    if l.is_const(root) {
        return None;
    }
    Some(Step { nodes: l.nodes, root, store })
}

// ------------------------------------------------------------- the loop

/// A compiled fold loop:
///
/// `steps(xs, xstride, n, seed, out, ostride, last)` takes up to n steps.
/// Step i reads the item at `xs + i*xstride` (in elements) and the running
/// value, and where the loop stores, writes the new running value to
/// `out + i*ostride`. It stops before the first step that makes a value
/// that is not finite, writes the running value reached to `last`, and
/// answers the number of steps taken.
type Raw = unsafe extern "C" fn(*const f64, i64, i64, f64, *mut f64, i64, *mut f64) -> i64;

/// A compiled loop, by whichever generator.
#[derive(Clone, Copy)]
struct Kernel(Raw);

/// Every loop compiled so far, by the step and the generator, None where
/// the compilation failed.
type Cache = HashMap<(Step, Backend), Option<Kernel>>;

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Compile a step's loop with generator `b`. None where `b` is not built
/// in, the host has no backend for it, or the compilation fails.
fn compile(step: &Step, b: Backend) -> Option<Kernel> {
    match b {
        #[cfg(feature = "cranelift")]
        Backend::Cranelift => clif::compile(step),
        #[cfg(feature = "llvm")]
        Backend::Llvm => llvm::compile(step),
        #[allow(unreachable_patterns)]
        _ => {
            let _ = step;
            None
        }
    }
}

/// The loop for `step` from generator `b`, compiled now if it is not cached
/// and `may_compile` allows it.
fn cached_or_compiled(
    cache: &mut Cache,
    step: &Step,
    b: Backend,
    may_compile: bool,
) -> Option<Kernel> {
    if let Some(k) = cache.get(&(step.clone(), b)) {
        return *k;
    }
    if !may_compile || !b.available() || cache.len() >= MAX_KERNELS {
        return None;
    }
    let k = compile(step, b);
    cache.insert((step.clone(), b), k);
    k
}

/// The compiled loop for `x v y` over items of `width` floats, under the
/// policy in force for a fold of `steps` steps, and the generator that
/// made it.
///
/// Under [`JitMode::Auto`], with both generators built in, a fold of
/// [`LLVM_STEPS`] steps or more is compiled by LLVM and a shorter one of
/// [`COMPILE_STEPS`] or more by Cranelift; a loop LLVM has already
/// compiled serves any fold of [`USE_STEPS`] or more. With one generator
/// built in, it compiles from its own threshold.
fn kernel(
    v: &Verb,
    width: usize,
    store: bool,
    steps: usize,
    resolve: &dyn Fn(&str) -> Option<Verb>,
) -> Option<(Kernel, Backend)> {
    let m = mode();
    if m == JitMode::Off || (m == JitMode::Auto && steps < USE_STEPS) {
        return None;
    }
    let step = lower(v, width, store, resolve)?;
    let mut cache = cache().lock().ok()?;
    let forced = match m {
        JitMode::Off => return None,
        JitMode::Force => backends().pop(),
        JitMode::Cranelift => Some(Backend::Cranelift),
        JitMode::Llvm => Some(Backend::Llvm),
        JitMode::Auto => None,
    };
    if let Some(b) = forced {
        return cached_or_compiled(&mut cache, &step, b, true).map(|k| (k, b));
    }
    let (cl, ll) = (Backend::Cranelift, Backend::Llvm);
    let llvm_from = if cl.available() { LLVM_STEPS } else { LLVM_ALONE_STEPS };
    let from = |b: Backend| if b == ll { llvm_from } else { COMPILE_STEPS };
    let (want, other) = if !cl.available() || steps >= llvm_from { (ll, cl) } else { (cl, ll) };
    // LLVM's loop, once compiled, serves a fold of any length.
    if want == cl
        && let Some(Some(k)) = cache.get(&(step.clone(), ll))
    {
        return Some((*k, ll));
    }
    if let Some(k) = cached_or_compiled(&mut cache, &step, want, steps >= from(want)) {
        return Some((k, want));
    }
    cached_or_compiled(&mut cache, &step, other, steps >= from(other)).map(|k| (k, other))
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
/// given. None where no loop runs or it stopped at a value that is not
/// finite; the caller then interprets the fold from the start.
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
    resolve: &dyn Fn(&str) -> Option<Verb>,
) -> Option<f64> {
    if n == 0 || !seed.is_finite() {
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
    let (k, backend) = kernel(v, width, out.is_some(), n, resolve)?;
    let xs = ys[first * w..].as_ptr();
    let (op, ostride) = match out {
        Some((o, s)) if s < 0 => (o[n - 1..].as_mut_ptr(), s),
        Some((o, s)) => (o.as_mut_ptr(), s),
        None => (std::ptr::null_mut(), 0),
    };
    let mut last = seed;
    // SAFETY: the loop reads items `first`, `first + dir`, … n of them,
    // all checked to lie inside `ys` above, and writes n floats to `out`
    // along its stride, which was checked to hold them.
    let taken = unsafe { (k.0)(xs, dir * w as i64, n as i64, seed, op, ostride, &mut last) };
    if taken != n as i64 {
        return None;
    }
    RUNS.with(|r| r[backend as usize].set(r[backend as usize].get() + 1));
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
            if m == JitMode::Auto && items < USE_STEPS {
                return None;
            }
            lower(v, width, multiple, &|_| None)?;
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
        let last = run(v, ys, width, at(from), n, step, seed, Some((&mut buf, 1)), &|_| None)?;
        kept.extend_from_slice(&buf);
        last
    } else {
        run(v, ys, width, at(from), n, step, seed, None, &|_| None)?
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
    let last = run(v, ys, 0, n - 2, n - 1, -1, ys[n - 1], None, resolve)?;
    Some(Array::scalar_f64(last))
}

/// `v/\. y` over a float list: each suffix folded, carried from the back.
/// None leaves it to the interpreter.
pub(crate) fn suffix_scan(
    v: &Verb,
    y: &Array,
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
    run(v, ys, 0, n - 2, n - 1, -1, ys[n - 1], Some((head, -1)), resolve)?;
    Some(Array::new(vec![n], Data::F64(buf.into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_names(_: &str) -> Option<Verb> {
        None
    }

    #[test]
    fn a_loop_stops_at_the_first_value_that_is_not_finite() {
        let step = Step {
            nodes: vec![Node::X(0), Node::Y, Node::Bin(Bin::Div, 0, 1)],
            root: 2,
            store: false,
        };
        for b in backends() {
            let k = compile(&step, b).expect("the host has a backend");
            let xs = [1.0, 0.0, 2.0];
            let mut last = 0.0;
            // 1 % 4, then 0 % 0.25, then 2 % 0: the third is not finite.
            let taken =
                unsafe { (k.0)(xs.as_ptr(), 1, 3, 4.0, std::ptr::null_mut(), 0, &mut last) };
            assert_eq!(taken, 2, "{b:?}");
            assert_eq!(last, 0.0, "{b:?}");
        }
        let _ = no_names("");
    }

    /// `x * y + x`, stored at every step, against the same arithmetic in
    /// Rust: a generator that fused the product and the sum into one
    /// multiply-add would round once where Rust rounds twice.
    #[test]
    fn a_product_and_a_sum_stay_two_roundings() {
        let step = Step {
            nodes: vec![Node::X(0), Node::Y, Node::Bin(Bin::Mul, 0, 1), Node::Bin(Bin::Add, 2, 0)],
            root: 3,
            store: true,
        };
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
            let k = compile(&step, b).expect("the host has a backend");
            let mut out = vec![0.0; xs.len()];
            let mut last = 0.0;
            let n = xs.len() as i64;
            let taken = unsafe { (k.0)(xs.as_ptr(), 1, n, 0.3, out.as_mut_ptr(), 1, &mut last) };
            assert_eq!(taken, n, "{b:?}");
            let got: Vec<u64> = out.iter().map(|v| v.to_bits()).collect();
            assert!(got == want, "{b:?}: the loop rounds differently from Rust");
        }
    }
}
