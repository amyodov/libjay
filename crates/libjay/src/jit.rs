//! Native code for the step of a fold, compiled at run time with Cranelift.
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
//! The compiled loop answers exactly what the interpreter answers, bit for
//! bit: every operation is the one [`crate::scalar`] defines for the
//! primitive, which the interpreter uses too, on the same operands in the
//! same order. The arithmetic is emitted as the IEEE instruction the
//! definition consists of, `<.` `>.` and `|` as the same compare and
//! select, and `^` and `^.` call the definitions themselves. Everywhere
//! the language's own rules
//! differ from plain IEEE arithmetic — a NaN, an infinity, `0 % 0` — the
//! value involved is not finite, so the loop checks every value it makes
//! and stops at the first that is not. The caller then runs the
//! interpreter over the whole fold, which is free to answer or refuse as
//! it always did. Only float data takes this road; the integer types keep
//! their overflow rules in the interpreter.
//!
//! `LIBJAY_JIT` chooses the policy once per process: `0` (or `off`) never
//! compiles, `force` compiles every step it can lower whatever the fold's
//! length, and anything else — the default — compiles where the fold is
//! long enough for the compilation to pay. [`with_mode`] overrides it on
//! one thread, which is how the differential tests run both roads in one
//! process. Compiled loops are cached by the expression they compute and
//! live as long as the process.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use cranelift_codegen::ir::condcodes::FloatCC;
use cranelift_codegen::ir::{types, AbiParam, InstBuilder, MemFlags, Signature, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};

use crate::array::{Array, Data};
use crate::ir::{ExplicitDef, Expr, Scope};
use crate::scalar;
use crate::verb::{DyadOp, MonadOp, ScalarDyad, ScalarMonad, Verb, RANK_INF};

/// When the fold's step is compiled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JitMode {
    /// Never: every step is interpreted.
    Off,
    /// Where the fold is long enough for compiling to pay for itself.
    Auto,
    /// Wherever the step can be lowered, however short the fold.
    Force,
}

thread_local! {
    static OVERRIDE: Cell<Option<JitMode>> = const { Cell::new(None) };
    static RUNS: Cell<u64> = const { Cell::new(0) };
}

/// How many folds this thread has answered with compiled code: what a
/// test reads to know that the compiled road was the one taken.
pub fn runs() -> u64 {
    RUNS.with(Cell::get)
}

/// The policy `LIBJAY_JIT` names, read once per process.
fn process_mode() -> JitMode {
    static MODE: OnceLock<JitMode> = OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("LIBJAY_JIT") {
        Ok(v) => match v.trim().to_ascii_lowercase().as_str() {
            "0" | "off" | "false" | "no" => JitMode::Off,
            "force" | "always" => JitMode::Force,
            _ => JitMode::Auto,
        },
        Err(_) => JitMode::Auto,
    })
}

/// The policy in force on this thread.
pub fn mode() -> JitMode {
    OVERRIDE.with(Cell::get).unwrap_or_else(process_mode)
}

/// Run `f` with the policy set to `m` on this thread, whatever
/// `LIBJAY_JIT` says. The previous setting is restored afterwards.
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

/// Fewest steps for which a loop is compiled under [`JitMode::Auto`].
/// Compiling one costs a few hundred microseconds, which is a few hundred
/// interpreted steps of the cheapest train.
const COMPILE_STEPS: usize = 1024;

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

#[derive(Clone, Copy)]
struct Kernel(Raw);

fn cache() -> &'static Mutex<HashMap<Step, Option<Kernel>>> {
    static CACHE: OnceLock<Mutex<HashMap<Step, Option<Kernel>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Compile a step's loop. None where the host has no Cranelift backend or
/// the compilation fails for any reason.
fn compile(step: &Step) -> Option<Kernel> {
    std::panic::catch_unwind(|| compile_inner(step)).ok().flatten()
}

fn compile_inner(step: &Step) -> Option<Kernel> {
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").ok()?;
    flags.set("use_colocated_libcalls", "false").ok()?;
    flags.set("is_pic", "false").ok()?;
    let isa = cranelift_native::builder().ok()?.finish(settings::Flags::new(flags)).ok()?;
    let ptr = isa.pointer_type();
    let call_conv = isa.default_call_conv();
    let mut module = JITModule::new(JITBuilder::with_isa(isa, default_libcall_names()));

    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(ptr)); // xs
    sig.params.push(AbiParam::new(types::I64)); // xstride
    sig.params.push(AbiParam::new(types::I64)); // n
    sig.params.push(AbiParam::new(types::F64)); // seed
    sig.params.push(AbiParam::new(ptr)); // out
    sig.params.push(AbiParam::new(types::I64)); // ostride
    sig.params.push(AbiParam::new(ptr)); // last
    sig.returns.push(AbiParam::new(types::I64));

    let id = module.declare_function("fold", Linkage::Local, &sig).ok()?;
    let mut ctx = module.make_context();
    ctx.func.signature = sig;
    let mut fctx = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let unary = {
            let mut s = Signature::new(call_conv);
            s.params.push(AbiParam::new(types::F64));
            s.returns.push(AbiParam::new(types::F64));
            b.import_signature(s)
        };

        let entry = b.create_block();
        let head = b.create_block();
        let body = b.create_block();
        let exit = b.create_block();
        b.append_block_params_for_function_params(entry);
        // head(i, acc, xp, op)
        let hi = b.append_block_param(head, types::I64);
        let hacc = b.append_block_param(head, types::F64);
        let hxp = b.append_block_param(head, ptr);
        let hop = b.append_block_param(head, ptr);
        // exit(i, acc)
        let ei = b.append_block_param(exit, types::I64);
        let eacc = b.append_block_param(exit, types::F64);

        b.switch_to_block(entry);
        let p = b.block_params(entry).to_vec();
        let (xs, xstride, n, seed, out, ostride, last) = (p[0], p[1], p[2], p[3], p[4], p[5], p[6]);
        let xstep = b.ins().ishl_imm(xstride, 3);
        let ostep = b.ins().ishl_imm(ostride, 3);
        let zero = b.ins().iconst(types::I64, 0);
        b.ins().jump(head, &[zero.into(), seed.into(), xs.into(), out.into()]);

        b.switch_to_block(head);
        let more = b.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::SignedLessThan, hi, n);
        b.ins().brif(more, body, &[], exit, &[hi.into(), hacc.into()]);

        b.switch_to_block(body);
        let inf = b.ins().f64const(f64::INFINITY);
        let mut vals: Vec<Value> = Vec::with_capacity(step.nodes.len());
        let mut ok: Option<Value> = None;
        let finite = |b: &mut FunctionBuilder, v: Value, ok: &mut Option<Value>| {
            let m = b.ins().fabs(v);
            let f = b.ins().fcmp(FloatCC::LessThan, m, inf);
            *ok = Some(match *ok {
                None => f,
                Some(o) => b.ins().band(o, f),
            });
        };
        for node in &step.nodes {
            let v = match *node {
                Node::X(k) => {
                    let v = b.ins().load(types::F64, MemFlags::trusted(), hxp, (k * 8) as i32);
                    finite(&mut b, v, &mut ok);
                    v
                }
                Node::Y => hacc,
                Node::Const(bits) => b.ins().f64const(f64::from_bits(bits)),
                Node::Un(op, a) => {
                    let a = vals[a as usize];
                    let v = match op {
                        Un::Neg => b.ins().fneg(a),
                        Un::Abs => {
                            // `scalar::abs`: the negative of a value below
                            // zero and the value itself otherwise.
                            let z = b.ins().f64const(0.0);
                            let below = b.ins().fcmp(FloatCC::LessThan, a, z);
                            let neg = b.ins().fneg(a);
                            b.ins().select(below, neg, a)
                        }
                        Un::Inc => {
                            let one = b.ins().f64const(1.0);
                            b.ins().fadd(a, one)
                        }
                        Un::Dec => {
                            let one = b.ins().f64const(1.0);
                            b.ins().fsub(a, one)
                        }
                        Un::Double => b.ins().fadd(a, a),
                        Un::Halve => {
                            let two = b.ins().f64const(2.0);
                            b.ins().fdiv(a, two)
                        }
                        Un::Square => b.ins().fmul(a, a),
                        Un::OneMinus => {
                            let one = b.ins().f64const(1.0);
                            b.ins().fsub(one, a)
                        }
                        Un::Recip => {
                            let one = b.ins().f64const(1.0);
                            b.ins().fdiv(one, a)
                        }
                        Un::Sqrt => b.ins().sqrt(a),
                        Un::Exp | Un::Ln => {
                            let f: extern "C" fn(f64) -> f64 =
                                if op == Un::Exp { scalar::exp } else { scalar::ln };
                            let callee = b.ins().iconst(ptr, f as usize as i64);
                            let call = b.ins().call_indirect(unary, callee, &[a]);
                            b.inst_results(call)[0]
                        }
                    };
                    finite(&mut b, v, &mut ok);
                    v
                }
                Node::Bin(op, l, r) => {
                    let (l, r) = (vals[l as usize], vals[r as usize]);
                    let v = match op {
                        Bin::Add => b.ins().fadd(l, r),
                        Bin::Sub => b.ins().fsub(l, r),
                        Bin::Mul => b.ins().fmul(l, r),
                        // `scalar::div` differs from IEEE only at `0 % 0`,
                        // whose NaN stops the loop.
                        Bin::Div => b.ins().fdiv(l, r),
                        // `scalar::min` and `scalar::max`: the left operand
                        // where the strict comparison picks it, otherwise
                        // the right, which is how the interpreter reads one
                        // pair of atoms. A NaN operand stops the loop
                        // before the answer is used.
                        Bin::Min => {
                            let lt = b.ins().fcmp(FloatCC::LessThan, l, r);
                            b.ins().select(lt, l, r)
                        }
                        Bin::Max => {
                            let gt = b.ins().fcmp(FloatCC::GreaterThan, l, r);
                            b.ins().select(gt, l, r)
                        }
                    };
                    finite(&mut b, v, &mut ok);
                    v
                }
            };
            vals.push(v);
        }
        let new = vals[step.root as usize];
        let cont = b.create_block();
        match ok {
            Some(ok) => {
                b.ins().brif(ok, cont, &[], exit, &[hi.into(), hacc.into()]);
            }
            None => {
                b.ins().jump(cont, &[]);
            }
        }
        b.switch_to_block(cont);
        if step.store {
            b.ins().store(MemFlags::trusted(), new, hop, 0);
        }
        let i1 = b.ins().iadd_imm(hi, 1);
        let xp1 = b.ins().iadd(hxp, xstep);
        let op1 = b.ins().iadd(hop, ostep);
        b.ins().jump(head, &[i1.into(), new.into(), xp1.into(), op1.into()]);

        b.switch_to_block(exit);
        b.ins().store(MemFlags::trusted(), eacc, last, 0);
        b.ins().return_(&[ei]);

        b.seal_all_blocks();
        b.finalize();
    }
    module.define_function(id, &mut ctx).ok()?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().ok()?;
    let code = module.get_finalized_function(id);
    // The code lives as long as the module's memory, which is never freed:
    // the cache hands the loop out for the rest of the process.
    std::mem::forget(module);
    // SAFETY: the function was declared with exactly this signature, under
    // the host's default calling convention, which `extern "C"` is.
    Some(Kernel(unsafe { std::mem::transmute::<*const u8, Raw>(code) }))
}

/// The compiled loop for `x v y` over items of `width` floats, under the
/// policy in force for a fold of `steps` steps.
fn kernel(
    v: &Verb,
    width: usize,
    store: bool,
    steps: usize,
    resolve: &dyn Fn(&str) -> Option<Verb>,
) -> Option<Kernel> {
    let m = mode();
    if m == JitMode::Off || (m == JitMode::Auto && steps < USE_STEPS) {
        return None;
    }
    let step = lower(v, width, store, resolve)?;
    let mut cache = cache().lock().ok()?;
    if let Some(k) = cache.get(&step) {
        return *k;
    }
    if (m == JitMode::Auto && steps < COMPILE_STEPS) || cache.len() >= MAX_KERNELS {
        return None;
    }
    let k = compile(&step);
    cache.insert(step, k);
    k
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
    let k = kernel(v, width, out.is_some(), n, resolve)?;
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
    RUNS.with(|r| r.set(r.get() + 1));
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
        let k = compile(&step).expect("the host has a backend");
        let xs = [1.0, 0.0, 2.0];
        let mut last = 0.0;
        // 1 % 4, then 0 % 0.25, then 2 % 0: the third is not finite.
        let taken = unsafe { (k.0)(xs.as_ptr(), 1, 3, 4.0, std::ptr::null_mut(), 0, &mut last) };
        assert_eq!(taken, 2);
        assert_eq!(last, 0.0);
        let _ = no_names("");
    }
}
