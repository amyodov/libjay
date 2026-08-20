//! Fusing chains of elementwise verbs into one blockwise pass.
//!
//! A chain like `+/ w * x` runs one pass per verb: the product is written to
//! memory in full and read back to be reduced. This pass finds maximal
//! subtrees of elementwise primitives at compile time and replaces them with
//! [`Expr::Fused`], which evaluates the whole chain a block at a time — the
//! block stays in cache, so the arrays at the leaves are read once and the
//! result is written once.
//!
//! The kernel is a postfix program over a small stack of block buffers. It
//! covers only what it can compute exactly as the unfused pipeline would;
//! everything else — a shape that needs broadcasting, a dtype the chain
//! would narrow, an integer overflow — declines at run time and the original
//! subtree, kept inside the node, evaluates instead. Fusion therefore cannot
//! change a result or an error message.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::array::{Array, Data};
use crate::dtype::DType;
use crate::ir::{Expr, Program};
use crate::par;
use crate::verb::{DyadOp, MonadOp, ScalarDyad, ScalarMonad, Verb};

/// Elements a block buffer holds.
///
/// The working set is `slots` buffers of this size — two or three for the
/// benchmark kernels — so 8,192 f64 is 128 to 192 KB and stays inside a
/// 256 KB L2. The value is not delicate: measured at 2,048 / 4,096 / 8,192 /
/// 16,384 / 32,768 on `+/ w * x` and `+/ ^ x` over 20M rows, the whole range
/// lands within a few per cent of the best, because what the kernel is
/// really bounded by is streaming the leaves in from memory once.
pub const BLOCK: usize = 8_192;

/// One step of a kernel: postfix, so operands are already on the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instr {
    /// Push input `k`.
    Load(usize),
    /// Replace the top of the stack.
    Monad(ScalarMonad),
    /// Replace the top two, left below right.
    Dyad(ScalarDyad),
}

/// A fused elementwise chain, optionally ending in a reduction.
#[derive(Clone, Debug)]
pub struct FusedKernel {
    code: Vec<Instr>,
    /// Block buffers one evaluation needs at once.
    slots: usize,
    /// The absorbed reduction, applied to the mapped values as a whole.
    reduce: Option<ScalarDyad>,
}

impl FusedKernel {
    pub fn code(&self) -> &[Instr] {
        &self.code
    }

    pub fn reduce(&self) -> Option<ScalarDyad> {
        self.reduce
    }
}

/// How often a fused node has handed its work back to the original subtree.
/// A counter rather than a log: the fallback is correct, only slower, and
/// what a caller wants to know is whether it is happening at all.
static FALLBACKS: AtomicU64 = AtomicU64::new(0);

/// Number of fallbacks since the process started.
pub fn fallback_count() -> u64 {
    FALLBACKS.load(Ordering::Relaxed)
}

fn note_fallback() {
    FALLBACKS.fetch_add(1, Ordering::Relaxed);
}

// ------------------------------------------------------------- the op set
//
// A verb may join a kernel only if it cannot fail on numeric data: the
// kernel reports no errors of its own, so anything that could raise one
// (APL's `÷` by zero, `%:` and `^.` of a negative, `^`'s zero to a negative
// power, APL's `~` off 0/1) stays outside and breaks the chain there.

/// The elementwise monad this verb performs, if the kernel covers it.
fn fusable_monad(v: &Verb) -> Option<ScalarMonad> {
    use ScalarMonad::*;
    let Verb::Prim(p) = v else { return None };
    let MonadOp::Scalar(op) = p.monad else { return None };
    matches!(
        op,
        Conj | Neg | Abs | Signum | Recip | Floor | Ceil | Inc | Dec | Double | Halve | Square
            | OneMinus | Exp
    )
    .then_some(op)
}

/// The elementwise dyad this verb performs, if the kernel covers it.
fn fusable_dyad(v: &Verb) -> Option<ScalarDyad> {
    use ScalarDyad::*;
    let Verb::Prim(p) = v else { return None };
    let DyadOp::Scalar(op) = p.dyad else { return None };
    matches!(op, Add | Sub | Mul | DivJ | Min | Max | Residue | Eq | Ne | Lt | Le | Gt | Ge)
        .then_some(op)
}

/// The reduction this verb performs over the leading axis, if the kernel can
/// absorb it: an associative arithmetic primitive, applied at full rank.
/// APL's `+/` is the same thing under a rank wrapper.
fn absorbable_reduce(v: &Verb) -> Option<ScalarDyad> {
    use ScalarDyad::*;
    let inner = match v {
        Verb::Reduce(u) => u,
        // The wrapper applies the reduction to cells of rank >= 1; over the
        // rank-1 argument this kernel insists on, that is the whole array.
        Verb::Rank(u, r) if r[0] >= 1 => match &**u {
            Verb::Reduce(inner) => inner,
            _ => return None,
        },
        _ => return None,
    };
    let Verb::Prim(p) = &**inner else { return None };
    let DyadOp::Scalar(op) = p.dyad else { return None };
    matches!(op, Add | Mul | Min | Max).then_some(op)
}

// ------------------------------------------------------------- the pass

/// The chain as a tree, before it becomes postfix code.
enum Node {
    /// A subtree the kernel does not cover: an input, with its index.
    Leaf(usize),
    Monad(ScalarMonad, Box<Node>),
    Dyad(ScalarDyad, Box<Node>, Box<Node>),
}

/// Build the chain rooted at `e`, collecting the subtrees that feed it.
///
/// Inputs are numbered in the order the evaluator would reach them — a
/// dyad's right argument first — so that a fused node evaluates its leaves
/// exactly when and where the unfused tree does.
fn chain<'a>(e: &'a Expr, leaves: &mut Vec<&'a Expr>) -> Node {
    match e {
        Expr::Monad { verb, y, .. } => match fusable_monad(verb) {
            Some(op) => Node::Monad(op, Box::new(chain(y, leaves))),
            None => leaf(e, leaves),
        },
        Expr::Dyad { verb, x, y, .. } => match fusable_dyad(verb) {
            Some(op) => {
                let ry = chain(y, leaves);
                let rx = chain(x, leaves);
                Node::Dyad(op, Box::new(rx), Box::new(ry))
            }
            None => leaf(e, leaves),
        },
        _ => leaf(e, leaves),
    }
}

fn leaf<'a>(e: &'a Expr, leaves: &mut Vec<&'a Expr>) -> Node {
    leaves.push(e);
    Node::Leaf(leaves.len() - 1)
}

fn ops(n: &Node) -> usize {
    match n {
        Node::Leaf(_) => 0,
        Node::Monad(_, y) => 1 + ops(y),
        Node::Dyad(_, x, y) => 1 + ops(x) + ops(y),
    }
}

/// Postfix code for the chain: a dyad's left operand is pushed first.
fn emit(n: &Node, code: &mut Vec<Instr>) {
    match n {
        Node::Leaf(i) => code.push(Instr::Load(*i)),
        Node::Monad(op, y) => {
            emit(y, code);
            code.push(Instr::Monad(*op));
        }
        Node::Dyad(op, x, y) => {
            emit(x, code);
            emit(y, code);
            code.push(Instr::Dyad(*op));
        }
    }
}

/// Block buffers the postfix program needs at once.
///
/// Only a computed value holds one — an input is read where it lies — and
/// the buffer being written is allocated before the operands are released,
/// so the peak is the live count at some operation plus one.
fn slots(code: &[Instr]) -> usize {
    let mut stack: Vec<bool> = Vec::new();
    let mut live = 0usize;
    let mut max = 1usize;
    for ins in code {
        let operands = match ins {
            Instr::Load(_) => {
                stack.push(false);
                continue;
            }
            Instr::Monad(_) => 1,
            Instr::Dyad(_) => 2,
        };
        max = max.max(live + 1);
        for _ in 0..operands {
            if stack.pop().unwrap_or(false) {
                live -= 1;
            }
        }
        live += 1;
        stack.push(true);
    }
    max
}

/// Is this subtree free of effects?
///
/// A fused node evaluates its leaves in the order the unfused tree would,
/// so an effect in one would still happen exactly once — but a node that
/// can fall back is easier to be sure of when nothing inside it can act on
/// the world, and a chain with `echo` in it is not the kind worth fusing.
fn replayable(e: &Expr) -> bool {
    match e {
        Expr::Const(..) | Expr::Param(..) | Expr::Name(..) => true,
        Expr::Assign { .. } | Expr::PrintPass { .. } => false,
        Expr::Monad { verb, y, .. } => verb.is_pure() && replayable(y),
        Expr::Dyad { verb, x, y, .. } => verb.is_pure() && replayable(x) && replayable(y),
        Expr::Fused { inputs, .. } => inputs.iter().all(replayable),
    }
}

/// Fuse every chain in a compiled program's sentences.
pub fn pass(stmts: &mut Vec<Expr>) {
    let taken = std::mem::take(stmts);
    *stmts = taken.into_iter().map(fuse_expr).collect();
}

fn fuse_expr(e: Expr) -> Expr {
    if let Some(f) = try_fuse(&e) {
        return f;
    }
    match e {
        Expr::Assign { name, value, span } => {
            Expr::Assign { name, value: Box::new(fuse_expr(*value)), span }
        }
        Expr::Monad { verb, y, span } => Expr::Monad { verb, y: Box::new(fuse_expr(*y)), span },
        Expr::Dyad { verb, x, y, span } => Expr::Dyad {
            verb,
            x: Box::new(fuse_expr(*x)),
            y: Box::new(fuse_expr(*y)),
            span,
        },
        Expr::PrintPass { value, span } => {
            Expr::PrintPass { value: Box::new(fuse_expr(*value)), span }
        }
        other => other,
    }
}

/// The fused node for the chain rooted at `e`, if there is one worth making.
fn try_fuse(e: &Expr) -> Option<Expr> {
    let (root, reduce) = match e {
        Expr::Monad { verb, y, .. } => match absorbable_reduce(verb) {
            Some(op) => (&**y, Some(op)),
            None => (e, None),
        },
        _ => (e, None),
    };
    let mut leaves = Vec::new();
    let node = chain(root, &mut leaves);
    // One elementwise verb on its own already runs as one pass; fusing it
    // would only add a layer. A reduction to absorb makes one verb enough.
    let least = if reduce.is_some() { 1 } else { 2 };
    if ops(&node) < least || !leaves.iter().all(|l| replayable(l)) {
        return None;
    }
    let mut code = Vec::new();
    emit(&node, &mut code);
    let kernel = FusedKernel { slots: slots(&code), code, reduce };
    let inputs = leaves.into_iter().map(|l| fuse_expr(l.clone())).collect();
    Some(Expr::Fused {
        kernel,
        inputs,
        orig: Box::new(e.clone()),
        span: e.span(),
    })
}

/// The chain a fused node came from, with its leaves replaced by the values
/// already computed for them.
///
/// This is what runs when the kernel declines. Rebuilding the tree costs a
/// handful of small allocations and saves evaluating the leaves a second
/// time, which for a leaf like `19 }. {close}` is a whole array.
pub(crate) fn fallback_tree(k: &FusedKernel, orig: &Expr, values: &[Array]) -> Expr {
    let mut next = 0;
    let tree = match orig {
        // An absorbed reduction sits above the chain; only the chain's own
        // leaves were evaluated.
        Expr::Monad { verb, y, span } if k.reduce.is_some() => Expr::Monad {
            verb: verb.clone(),
            y: Box::new(substitute(y, values, &mut next)),
            span: *span,
        },
        e => substitute(e, values, &mut next),
    };
    debug_assert_eq!(next, values.len(), "the fallback found different leaves");
    tree
}

/// Walk the chain exactly as [`chain`] walked it, so the leaves take their
/// values in the order they were numbered in.
fn substitute(e: &Expr, values: &[Array], next: &mut usize) -> Expr {
    match e {
        Expr::Monad { verb, y, span } if fusable_monad(verb).is_some() => Expr::Monad {
            verb: verb.clone(),
            y: Box::new(substitute(y, values, next)),
            span: *span,
        },
        Expr::Dyad { verb, x, y, span } if fusable_dyad(verb).is_some() => {
            let ry = substitute(y, values, next);
            let rx = substitute(x, values, next);
            Expr::Dyad { verb: verb.clone(), x: Box::new(rx), y: Box::new(ry), span: *span }
        }
        leaf => {
            let v = values[*next].clone();
            *next += 1;
            Expr::Const(v, leaf.span())
        }
    }
}

/// Does any sentence of this program run a fused kernel?
pub fn is_fused(p: &Program) -> bool {
    fn any(e: &Expr) -> bool {
        match e {
            Expr::Fused { .. } => true,
            Expr::Const(..) | Expr::Param(..) | Expr::Name(..) => false,
            Expr::Assign { value, .. } | Expr::PrintPass { value, .. } => any(value),
            Expr::Monad { y, .. } => any(y),
            Expr::Dyad { x, y, .. } => any(x) || any(y),
        }
    }
    p.stmts.iter().any(any)
}

/// The same program with every fused node replaced by the subtree it came
/// from. The two must compute the same thing; tests hold them to it.
pub fn unfused(p: &Program) -> Program {
    fn strip(e: &Expr) -> Expr {
        match e {
            Expr::Fused { orig, .. } => strip(orig),
            Expr::Assign { name, value, span } => {
                Expr::Assign { name: name.clone(), value: Box::new(strip(value)), span: *span }
            }
            Expr::PrintPass { value, span } => {
                Expr::PrintPass { value: Box::new(strip(value)), span: *span }
            }
            Expr::Monad { verb, y, span } => {
                Expr::Monad { verb: verb.clone(), y: Box::new(strip(y)), span: *span }
            }
            Expr::Dyad { verb, x, y, span } => Expr::Dyad {
                verb: verb.clone(),
                x: Box::new(strip(x)),
                y: Box::new(strip(y)),
                span: *span,
            },
            other => other.clone(),
        }
    }
    let mut out = p.clone();
    out.stmts = p.stmts.iter().map(strip).collect();
    out
}

// ------------------------------------------------------------- dtype rules

/// The dtype the unfused pipeline gives this monad's result. None where it
/// depends on the values (`<.` of a float is an integer only if every
/// rounded value fits one), which the kernel declines rather than guess.
fn monad_type(op: ScalarMonad, a: DType) -> Option<DType> {
    use DType::*;
    use ScalarMonad::*;
    Some(match op {
        Recip | Halve | Exp => F64,
        // Identity and magnitude keep a boolean boolean.
        Conj | Abs | OneMinus => a,
        Neg | Signum | Inc | Dec | Double | Square => match a {
            Bool | I64 => I64,
            other => other,
        },
        Floor | Ceil => match a {
            Bool | I64 => I64,
            _ => return None,
        },
        _ => return None,
    })
}

/// The dtype the unfused pipeline gives this dyad's result, on the path
/// where no integer step overflows (one that does falls back).
fn dyad_type(op: ScalarDyad, a: DType, b: DType) -> Option<DType> {
    use ScalarDyad::*;
    match op {
        Eq | Ne | Lt | Le | Gt | Ge => Some(DType::Bool),
        DivJ => Some(DType::F64),
        Add | Sub | Mul | Min | Max | Residue => match DType::promote(a, b)? {
            DType::Bool => Some(DType::I64),
            DType::Char => None,
            t => Some(t),
        },
        _ => None,
    }
}

/// The type the kernel computes in, and the dtype of its mapped result.
///
/// Every value in the program is computed in one type, so it must be one
/// that holds them all: integers when nothing in the chain leaves them,
/// floats otherwise. That leaves one case the kernel cannot serve — a chain
/// that computes an integer somewhere along a float path, as
/// `(x > 0) + (y > 0)` or `({a} + {b}) % 2` do. Its unfused pipeline holds
/// those steps in i64, exactly, past where f64 stops being exact, and its
/// result may be an integer array. Rather than compute them in the wrong
/// type, the kernel declines and the chain runs.
///
/// A boolean is not such a case: a comparison yields 0 and 1, which f64
/// holds exactly, and only the dtype of a result made from one has to be
/// narrowed at the end.
///
/// This is the kernel's main blind spot — a random chain over mixed integer
/// and float arguments declines about half the time — and the way out is a
/// stack whose entries carry their own type rather than one type per
/// kernel. Nothing measured so far needs it.
fn working_type(k: &FusedKernel, inputs: &[Array]) -> Option<(DType, DType)> {
    let mut stack: Vec<DType> = Vec::with_capacity(k.slots);
    let mut float = false;
    let mut integer_step = false;
    for ins in &k.code {
        let t = match ins {
            Instr::Load(i) => inputs[*i].dtype(),
            Instr::Monad(op) => monad_type(*op, stack.pop()?)?,
            Instr::Dyad(op) => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                dyad_type(*op, a, b)?
            }
        };
        if t == DType::Char {
            return None;
        }
        float |= t == DType::F64;
        // An argument's own values are exact in either type; a step's are
        // not, once they are integers wider than f64's 53 bits.
        integer_step |= t == DType::I64 && !matches!(ins, Instr::Load(_));
        stack.push(t);
    }
    let root = stack.pop()?;
    let working = if float { DType::F64 } else { DType::I64 };
    if working == DType::F64 && integer_step {
        return None;
    }
    Some((working, root))
}

// ------------------------------------------------------------- execution

/// One input, in the working type: either the values themselves or one
/// value repeated, which is how a rank-0 argument reaches every element.
struct Loaded<'a, T> {
    data: &'a [T],
    splat: bool,
}

impl<T> Loaded<'_, T> {
    #[inline]
    fn block(&self, start: usize, len: usize) -> &[T] {
        if self.splat {
            &self.data[..len]
        } else {
            &self.data[start..start + len]
        }
    }
}

/// What a stack entry refers to: an input, or a block buffer.
#[derive(Clone, Copy)]
enum Slot {
    Input(usize),
    Block(usize),
}

/// Block buffer `d` for writing, plus read-only access to the others.
fn split_slots<'s, T>(
    scratch: &'s mut [T],
    w: usize,
    d: usize,
) -> (&'s mut [T], impl Fn(usize) -> &'s [T]) {
    let (lo, hi) = scratch.split_at_mut(d * w);
    let (dst, hi) = hi.split_at_mut(w);
    let lo: &[T] = lo;
    let hi: &[T] = hi;
    (dst, move |i: usize| {
        if i < d {
            &lo[i * w..(i + 1) * w]
        } else {
            &hi[(i - d - 1) * w..(i - d) * w]
        }
    })
}

/// Run the kernel's map over elements `start .. start + len`.
///
/// `out`, when given, receives the last instruction's result directly and
/// the returned index means nothing; otherwise the result stays in the
/// block buffer that index names. None means an integer step left i64 and
/// the caller must fall back.
#[allow(clippy::too_many_arguments)]
fn exec_block<T, M, D>(
    code: &[Instr],
    srcs: &[Loaded<'_, T>],
    start: usize,
    len: usize,
    scratch: &mut [T],
    w: usize,
    free: &mut Vec<usize>,
    stack: &mut Vec<Slot>,
    out: Option<&mut [T]>,
    mon: &M,
    dya: &D,
) -> Option<usize>
where
    T: Copy,
    M: Fn(ScalarMonad, &[T], &mut [T]) -> bool,
    D: Fn(ScalarDyad, &[T], &[T], &mut [T]) -> bool,
{
    stack.clear();
    free.clear();
    let nslots = scratch.len() / w;
    free.extend((0..nslots).rev());
    let last = code.len() - 1;
    let head = if out.is_some() { last } else { code.len() };
    for ins in &code[..head] {
        match ins {
            Instr::Load(k) => stack.push(Slot::Input(*k)),
            Instr::Monad(op) => {
                let a = stack.pop()?;
                let d = free.pop()?;
                let (dst, get) = split_slots(scratch, w, d);
                let av = match a {
                    Slot::Input(k) => srcs[k].block(start, len),
                    Slot::Block(i) => &get(i)[..len],
                };
                if !mon(*op, av, &mut dst[..len]) {
                    return None;
                }
                if let Slot::Block(i) = a {
                    free.push(i);
                }
                stack.push(Slot::Block(d));
            }
            Instr::Dyad(op) => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                let d = free.pop()?;
                let (dst, get) = split_slots(scratch, w, d);
                let av = match a {
                    Slot::Input(k) => srcs[k].block(start, len),
                    Slot::Block(i) => &get(i)[..len],
                };
                let bv = match b {
                    Slot::Input(k) => srcs[k].block(start, len),
                    Slot::Block(i) => &get(i)[..len],
                };
                if !dya(*op, av, bv, &mut dst[..len]) {
                    return None;
                }
                for s in [a, b] {
                    if let Slot::Block(i) = s {
                        free.push(i);
                    }
                }
                stack.push(Slot::Block(d));
            }
        }
    }
    let Some(dst) = out else {
        return match stack.pop()? {
            Slot::Block(i) => Some(i),
            // Every kernel ends in an operation, so the result is a buffer.
            Slot::Input(_) => None,
        };
    };
    // The last instruction writes the caller's buffer instead of a block.
    let dst = &mut dst[..len];
    let view = |s: Slot| match s {
        Slot::Input(k) => srcs[k].block(start, len),
        Slot::Block(i) => &scratch[i * w..i * w + len],
    };
    let ok = match code[last] {
        Instr::Monad(op) => {
            let a = view(stack.pop()?);
            mon(op, a, dst)
        }
        Instr::Dyad(op) => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            dya(op, view(a), view(b), dst)
        }
        Instr::Load(_) => return None,
    };
    ok.then_some(usize::MAX)
}

/// The whole mapped result, one block at a time. None on integer overflow.
fn map_pass<T, M, D>(
    k: &FusedKernel,
    srcs: &[Loaded<'_, T>],
    n: usize,
    mon: M,
    dya: D,
) -> Option<Vec<T>>
where
    T: Copy + Default + Send + Sync,
    M: Fn(ScalarMonad, &[T], &mut [T]) -> bool + Sync + Send,
    D: Fn(ScalarDyad, &[T], &[T], &mut [T]) -> bool + Sync + Send,
{
    let (out, ok) = par::fill(n, |start, part: &mut [T]| {
        let w = BLOCK.min(part.len()).max(1);
        let mut scratch = vec![T::default(); k.slots * w];
        let mut free = Vec::with_capacity(k.slots);
        let mut stack = Vec::with_capacity(k.slots);
        for (b, chunk) in part.chunks_mut(w).enumerate() {
            let len = chunk.len();
            let ok = exec_block(
                &k.code,
                srcs,
                start + b * w,
                len,
                &mut scratch,
                w,
                &mut free,
                &mut stack,
                Some(chunk),
                &mon,
                &dya,
            );
            if ok.is_none() {
                return false;
            }
        }
        true
    });
    ok.then_some(out)
}

/// Fold the mapped values of `lo .. hi` right to left, block by block.
#[allow(clippy::too_many_arguments)]
fn fold_range<T, M, D, S>(
    k: &FusedKernel,
    srcs: &[Loaded<'_, T>],
    lo: usize,
    hi: usize,
    mon: &M,
    dya: &D,
    step: &S,
) -> Option<T>
where
    T: Copy + Default,
    M: Fn(ScalarMonad, &[T], &mut [T]) -> bool,
    D: Fn(ScalarDyad, &[T], &[T], &mut [T]) -> bool,
    S: Fn(T, T) -> Option<T>,
{
    let w = BLOCK.min(hi - lo).max(1);
    let mut scratch = vec![T::default(); k.slots * w];
    let mut free = Vec::with_capacity(k.slots);
    let mut stack = Vec::with_capacity(k.slots);
    let mut acc: Option<T> = None;
    // Blocks run backwards and the accumulator carries across them, so the
    // fold is the insert's own right-to-left order over the whole range.
    for b in (0..(hi - lo).div_ceil(w)).rev() {
        let start = lo + b * w;
        let len = (hi - start).min(w);
        let slot = exec_block(
            &k.code, srcs, start, len, &mut scratch, w, &mut free, &mut stack, None, mon, dya,
        )?;
        for &v in scratch[slot * w..slot * w + len].iter().rev() {
            acc = Some(match acc {
                None => v,
                Some(a) => step(v, a)?,
            });
        }
    }
    acc
}

/// The mapped values folded into one. None on integer overflow.
fn reduce_pass<T, M, D, S>(
    k: &FusedKernel,
    srcs: &[Loaded<'_, T>],
    n: usize,
    mon: M,
    dya: D,
    step: S,
) -> Option<T>
where
    T: Copy + Default + Send + Sync,
    M: Fn(ScalarMonad, &[T], &mut [T]) -> bool + Sync + Send,
    D: Fn(ScalarDyad, &[T], &[T], &mut [T]) -> bool + Sync + Send,
    S: Fn(T, T) -> Option<T> + Sync + Send,
{
    let chunks = par::chunks(n, n * k.code.len());
    if chunks < 2 {
        return fold_range(k, srcs, 0, n, &mon, &dya, &step);
    }
    let per = n.div_ceil(chunks);
    let parts = par::map_indexed(n.div_ceil(per), |c| {
        fold_range(k, srcs, c * per, ((c + 1) * per).min(n), &mon, &dya, &step)
    });
    // The chunks combine right to left, the order they were folded in. That
    // regroups an associative float fold, which is the §5.9 contract; only
    // associative operations are absorbed.
    let mut it = parts.into_iter().rev();
    let mut acc = it.next()??;
    for part in it {
        acc = step(part?, acc)?;
    }
    Some(acc)
}

// ------------------------------------------------------------ the kernels
//
// Each pass picks its operation before the loop and then runs one plain
// loop over slices, which is the shape the compiler vectorises. Nothing in
// here is hand-written SIMD, and nothing may become it.

macro_rules! each {
    ($a:expr, $dst:expr, $f:expr) => {{
        let f = $f;
        for (slot, &x) in $dst.iter_mut().zip($a) {
            *slot = f(x);
        }
        return true;
    }};
}

macro_rules! zip {
    ($a:expr, $b:expr, $dst:expr, $f:expr) => {{
        let f = $f;
        for ((slot, &x), &y) in $dst.iter_mut().zip($a).zip($b) {
            *slot = f(x, y);
        }
        return true;
    }};
}

fn monad_f64(op: ScalarMonad, a: &[f64], dst: &mut [f64]) -> bool {
    use ScalarMonad::*;
    match op {
        Conj => each!(a, dst, |x: f64| x),
        Neg => each!(a, dst, |x: f64| -x),
        Abs => each!(a, dst, f64::abs),
        Signum => each!(a, dst, |x: f64| if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            0.0
        }),
        // `% 0` is infinity, the J rule the unfused monad follows.
        Recip => each!(a, dst, |x: f64| if x == 0.0 { f64::INFINITY } else { 1.0 / x }),
        // Reached only through an integer chain, where they are the
        // identity: rounding a float narrows its dtype, which is declined.
        Floor => each!(a, dst, f64::floor),
        Ceil => each!(a, dst, f64::ceil),
        Inc => each!(a, dst, |x: f64| x + 1.0),
        Dec => each!(a, dst, |x: f64| x - 1.0),
        Double => each!(a, dst, |x: f64| x + x),
        Halve => each!(a, dst, |x: f64| x / 2.0),
        Square => each!(a, dst, |x: f64| x * x),
        OneMinus => each!(a, dst, |x: f64| 1.0 - x),
        Exp => each!(a, dst, f64::exp),
        _ => false,
    }
}

fn dyad_f64(op: ScalarDyad, a: &[f64], b: &[f64], dst: &mut [f64]) -> bool {
    use ScalarDyad::*;
    match op {
        Add => zip!(a, b, dst, |x: f64, y: f64| x + y),
        Sub => zip!(a, b, dst, |x: f64, y: f64| x - y),
        Mul => zip!(a, b, dst, |x: f64, y: f64| x * y),
        Min => zip!(a, b, dst, f64::min),
        Max => zip!(a, b, dst, f64::max),
        DivJ => zip!(a, b, dst, |x: f64, y: f64| if y == 0.0 {
            if x == 0.0 { 0.0 } else { f64::INFINITY.copysign(x) }
        } else {
            x / y
        }),
        Residue => zip!(a, b, dst, |x: f64, y: f64| if x == 0.0 {
            y
        } else {
            y - x * (y / x).floor()
        }),
        // A comparison is a number here, as it is in J: the boolean only
        // shows in the dtype of a result, which the caller narrows.
        Eq => zip!(a, b, dst, |x: f64, y: f64| (x == y) as u8 as f64),
        Ne => zip!(a, b, dst, |x: f64, y: f64| (x != y) as u8 as f64),
        Lt => zip!(a, b, dst, |x: f64, y: f64| (x < y) as u8 as f64),
        Le => zip!(a, b, dst, |x: f64, y: f64| (x <= y) as u8 as f64),
        Gt => zip!(a, b, dst, |x: f64, y: f64| (x > y) as u8 as f64),
        Ge => zip!(a, b, dst, |x: f64, y: f64| (x >= y) as u8 as f64),
        _ => false,
    }
}

/// Integer passes fold overflow into a flag instead of branching out of the
/// loop: the whole evaluation is thrown away and redone unfused either way.
macro_rules! each_over {
    ($a:expr, $dst:expr, $f:expr) => {{
        let f = $f;
        let mut over = false;
        for (slot, &x) in $dst.iter_mut().zip($a) {
            let (v, o) = f(x);
            *slot = v;
            over |= o;
        }
        return !over;
    }};
}

macro_rules! zip_over {
    ($a:expr, $b:expr, $dst:expr, $f:expr) => {{
        let f = $f;
        let mut over = false;
        for ((slot, &x), &y) in $dst.iter_mut().zip($a).zip($b) {
            let (v, o) = f(x, y);
            *slot = v;
            over |= o;
        }
        return !over;
    }};
}

fn monad_i64(op: ScalarMonad, a: &[i64], dst: &mut [i64]) -> bool {
    use ScalarMonad::*;
    match op {
        Conj | Floor | Ceil => each!(a, dst, |x: i64| x),
        Neg => each_over!(a, dst, i64::overflowing_neg),
        Abs => each_over!(a, dst, i64::overflowing_abs),
        Signum => each!(a, dst, i64::signum),
        Inc => each_over!(a, dst, |x: i64| x.overflowing_add(1)),
        Dec => each_over!(a, dst, |x: i64| x.overflowing_sub(1)),
        Double => each_over!(a, dst, |x: i64| x.overflowing_add(x)),
        Square => each_over!(a, dst, |x: i64| x.overflowing_mul(x)),
        OneMinus => each_over!(a, dst, |x: i64| 1i64.overflowing_sub(x)),
        _ => false,
    }
}

fn dyad_i64(op: ScalarDyad, a: &[i64], b: &[i64], dst: &mut [i64]) -> bool {
    use ScalarDyad::*;
    match op {
        Add => zip_over!(a, b, dst, i64::overflowing_add),
        Sub => zip_over!(a, b, dst, i64::overflowing_sub),
        Mul => zip_over!(a, b, dst, i64::overflowing_mul),
        Min => zip!(a, b, dst, i64::min),
        Max => zip!(a, b, dst, i64::max),
        Residue => zip!(a, b, dst, |x: i64, y: i64| if x == 0 {
            y
        } else {
            // wrapping_rem: i64::MIN % -1 is mathematically 0.
            let mut r = y.wrapping_rem(x);
            if r != 0 && (r < 0) != (x < 0) {
                r += x;
            }
            r
        }),
        Eq => zip!(a, b, dst, |x: i64, y: i64| (x == y) as i64),
        Ne => zip!(a, b, dst, |x: i64, y: i64| (x != y) as i64),
        Lt => zip!(a, b, dst, |x: i64, y: i64| (x < y) as i64),
        Le => zip!(a, b, dst, |x: i64, y: i64| (x <= y) as i64),
        Gt => zip!(a, b, dst, |x: i64, y: i64| (x > y) as i64),
        Ge => zip!(a, b, dst, |x: i64, y: i64| (x >= y) as i64),
        _ => false,
    }
}

/// One fold step of an absorbed reduction. None on integer overflow.
fn step_i64(op: ScalarDyad, a: i64, b: i64) -> Option<i64> {
    use ScalarDyad::*;
    match op {
        Add => a.checked_add(b),
        Mul => a.checked_mul(b),
        Min => Some(a.min(b)),
        Max => Some(a.max(b)),
        _ => None,
    }
}

fn step_f64(op: ScalarDyad, a: f64, b: f64) -> Option<f64> {
    use ScalarDyad::*;
    match op {
        Add => Some(a + b),
        Mul => Some(a * b),
        Min => Some(a.min(b)),
        Max => Some(a.max(b)),
        _ => None,
    }
}

// ------------------------------------------------------------- the driver

/// Elements of `a` as the working type, or None when the array's own buffer
/// already is that. A rank-0 argument becomes one block of the repeated
/// value, which is how it reaches every element without an index test.
fn to_f64(a: &Array, w: usize) -> Option<Vec<f64>> {
    if a.rank() == 0 {
        let v = match &a.data {
            Data::Bool(d) => d[0] as f64,
            Data::I64(d) => d[0] as f64,
            Data::F64(d) => d[0],
            Data::Char(_) => return Some(Vec::new()),
        };
        return Some(vec![v; w]);
    }
    match &a.data {
        Data::F64(_) => None,
        Data::I64(d) => Some(par::map(d, |&x| x as f64)),
        Data::Bool(d) => Some(par::map(d, |&x| x as f64)),
        Data::Char(_) => Some(Vec::new()),
    }
}

fn to_i64(a: &Array, w: usize) -> Option<Vec<i64>> {
    if a.rank() == 0 {
        let v = match &a.data {
            Data::Bool(d) => d[0] as i64,
            Data::I64(d) => d[0],
            _ => return Some(Vec::new()),
        };
        return Some(vec![v; w]);
    }
    match &a.data {
        Data::I64(_) => None,
        Data::Bool(d) => Some(par::map(d, |&x| x as i64)),
        // The working type is integer only when no input is a float.
        _ => Some(Vec::new()),
    }
}

/// The shape every element of the result has: identical for all non-scalar
/// inputs, since anything else needs the agreement machinery.
fn common_shape(inputs: &[Array]) -> Option<Option<Vec<usize>>> {
    let mut shape: Option<&Vec<usize>> = None;
    for a in inputs {
        if a.rank() == 0 {
            continue;
        }
        match shape {
            None => shape = Some(&a.shape),
            Some(s) if *s == a.shape => {}
            Some(_) => return None,
        }
    }
    Some(shape.cloned())
}

/// Run a fused node. None means the kernel declined and the caller must
/// evaluate the original subtree, which is always allowed to be slower and
/// never allowed to differ.
pub(crate) fn run(k: &FusedKernel, inputs: &[Array]) -> Option<Array> {
    let reducing = k.reduce.is_some();
    // Every input a scalar: no work worth blocking, and a reduction would
    // need the leading axis a scalar has not got.
    let shape = common_shape(inputs)??;
    let n: usize = shape.iter().product();
    if n == 0 {
        return None;
    }
    if reducing && (shape.len() != 1 || n < 2) {
        // A one-item reduction yields the item itself, dtype and all, and a
        // higher-rank one folds cells rather than elements.
        return None;
    }
    let (working, root) = working_type(k, inputs)?;
    let w = BLOCK.min(n).max(1);

    let data = if working == DType::F64 {
        let owned: Vec<Option<Vec<f64>>> = inputs.iter().map(|a| to_f64(a, w)).collect();
        let srcs: Vec<Loaded<f64>> = inputs
            .iter()
            .zip(&owned)
            .map(|(a, o)| match o {
                Some(v) => Loaded { data: v, splat: a.rank() == 0 },
                None => Loaded { data: a.as_f64_slice().unwrap_or(&[]), splat: false },
            })
            .collect();
        match k.reduce {
            None => {
                let out = map_pass(k, &srcs, n, monad_f64, dyad_f64)?;
                float_result(out, root)
            }
            Some(op) => {
                let v =
                    reduce_pass(k, &srcs, n, monad_f64, dyad_f64, |a, b| step_f64(op, a, b))?;
                // A comparison at the root maps to exact 0 and 1, which the
                // fold keeps exact; the reduction of booleans is integer.
                match root {
                    DType::F64 => Data::F64(vec![v].into()),
                    _ => Data::I64(vec![v as i64].into()),
                }
            }
        }
    } else {
        let owned: Vec<Option<Vec<i64>>> = inputs.iter().map(|a| to_i64(a, w)).collect();
        let srcs: Vec<Loaded<i64>> = inputs
            .iter()
            .zip(&owned)
            .map(|(a, o)| match o {
                Some(v) => Loaded { data: v, splat: a.rank() == 0 },
                None => Loaded { data: a.as_i64_slice().unwrap_or(&[]), splat: false },
            })
            .collect();
        match k.reduce {
            None => {
                let out = map_pass(k, &srcs, n, monad_i64, dyad_i64)?;
                int_result(out, root)
            }
            Some(op) => {
                let v =
                    reduce_pass(k, &srcs, n, monad_i64, dyad_i64, |a, b| step_i64(op, a, b))?;
                Data::I64(vec![v].into())
            }
        }
    };
    Some(Array::new(if reducing { Vec::new() } else { shape }, data))
}

/// The mapped block values as the array the unfused chain would build. A
/// comparison at the root costs one narrowing pass, since the kernel
/// computes 0 and 1 in its working type and a boolean array holds bytes.
fn float_result(out: Vec<f64>, root: DType) -> Data {
    match root {
        DType::Bool => Data::Bool(par::map(&out, |&v| (v != 0.0) as u8).into()),
        _ => Data::F64(out.into()),
    }
}

fn int_result(out: Vec<i64>, root: DType) -> Data {
    match root {
        DType::Bool => Data::Bool(par::map(&out, |&v| (v != 0) as u8).into()),
        _ => Data::I64(out.into()),
    }
}

/// Evaluate a fused node from its already-evaluated inputs, or report that
/// the original subtree must run instead.
pub(crate) fn eval(k: &FusedKernel, inputs: &[Array]) -> Option<Array> {
    let r = run(k, inputs);
    if r.is_none() {
        note_fallback();
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{compile, Dialect, Lang};

    fn program(src: &str) -> Program {
        compile(Lang::J, src, &Dialect::default()).expect("compile")
    }

    #[test]
    fn a_chain_of_two_scalar_verbs_fuses() {
        assert!(is_fused(&program("1 + 2 * {x}")));
        assert!(is_fused(&program("+/ {w} * {x}")));
        assert!(is_fused(&program("+/ ^ {x}")));
    }

    #[test]
    fn one_verb_on_its_own_is_left_alone() {
        assert!(!is_fused(&program("2 * {x}")));
        assert!(!is_fused(&program("+/ {x}")));
        assert!(!is_fused(&program("{x}")));
    }

    #[test]
    fn a_verb_the_kernel_does_not_cover_breaks_the_chain() {
        // `%:` can fail elementwise, so it stays outside; the chain under it
        // still fuses.
        assert!(!is_fused(&program("%: 2 * {x}")));
        assert!(is_fused(&program("%: 1 + 2 * {x}")));
    }

    #[test]
    fn an_effect_in_a_leaf_keeps_the_chain_unfused() {
        assert!(!is_fused(&program("1 + 2 * echo {x}")));
    }

    #[test]
    fn the_postfix_program_pushes_the_left_operand_first() {
        let p = program("{w} - {x} - 1");
        let Expr::Fused { kernel, .. } = &p.stmts[0] else { panic!("not fused") };
        assert_eq!(
            kernel.code(),
            [
                Instr::Load(2),
                Instr::Load(1),
                Instr::Load(0),
                Instr::Dyad(ScalarDyad::Sub),
                Instr::Dyad(ScalarDyad::Sub),
            ]
        );
        // One buffer holds the inner difference, one takes the outer one.
        assert_eq!(kernel.slots, 2);
    }
}
