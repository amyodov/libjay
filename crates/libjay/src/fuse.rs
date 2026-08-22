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
//!
//! A chain does not have to be written as one sentence. `d =. {x} - m`
//! followed by `+/ d * d` names a value that nothing needs as an array, and
//! the pass moves such a value into the sentences that read it — see
//! `inline_once` for the rules that keep that sound.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::array::{Array, Data, Layout};
use crate::dtype::DType;
use crate::error::Span;
use crate::ir::{Expr, Program, Scope};
use crate::par;
use crate::simd::multiversioned;
use crate::verb::{
    tol_cmp, windows_into, DyadOp, MonadOp, ScalarDyad, ScalarMonad, Tol, Verb, WindowKind,
    RANK_INF,
};

/// Elements a block buffer holds.
///
/// The working set is `slots` buffers of this size — two or three for the
/// benchmark kernels — so 8,192 f64 is 128 to 192 KB and stays inside a
/// 256 KB L2. The value is not delicate: measured at 2,048 / 4,096 / 8,192 /
/// 16,384 / 32,768 on `+/ w * x` and `+/ ^ x` over 20M rows, the whole range
/// lands within a few per cent of the best, because what the kernel is
/// really bounded by is streaming the leaves in from memory once.
pub const BLOCK: usize = 8_192;

/// The largest window a kernel absorbs.
///
/// A block computes the wide axis its own windows need, which is the block
/// plus a halo of about three window lengths, and holds it in the same
/// buffers the arithmetic uses. Past this size the halo is most of the work
/// and the buffers are past any cache worth staying in, so a longer window
/// stays outside the kernel and takes the pass it has always taken.
pub const MAX_WINDOW: usize = 1_024;

/// One step of a kernel: postfix, so operands are already on the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instr {
    /// Push input `k`.
    Load(usize),
    /// Replace the top of the stack.
    Monad(ScalarMonad),
    /// Replace the top two, left below right.
    Dyad(ScalarDyad),
    /// Keep the top of the stack as let `k`, a value the rest of the
    /// program reads more than once. It holds its block buffer until the
    /// block is finished; nothing pops it.
    Store(usize),
    /// Push let `k` again.
    Let(usize),
    /// Fold every window of `k` consecutive items of the top of the stack
    /// into one item. The operand stands on the wide axis and the result on
    /// the kernel's own, which is `k - 1` items shorter.
    Window(ScalarDyad, usize),
    /// Replace the top of the stack with its running fold: item `i` becomes
    /// the fold of items `0 .. i`. Both stand on the same axis.
    Scan(ScalarDyad),
}

/// Which axis a value inside a kernel stands on.
///
/// A kernel that folds windows reads two: the one its result stands on, and
/// the wider one every window step reads, which is `k - 1` items longer.
/// Where a value stands is decided by the chain — everything under a window
/// step is wide — so the two never have to be told apart at run time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dom {
    Result,
    Wide,
}

/// The stages a chain absorbs.
///
/// A chain takes moving windows or running folds, not both, and every
/// window step in one kernel folds windows of the same length: that is what
/// leaves exactly two axes to align, which shapes alone can then decide.
/// Anything else — a second window length, a window inside a window, a
/// running fold beside a window — is read as a leaf and runs as the pass it
/// was.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Plan {
    window: Option<usize>,
    scan: bool,
}

/// What one evaluation of a kernel produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Yield {
    /// The mapped values, as an array of the chain's own shape.
    Values,
    /// The mapped values folded into one by an absorbed reduction.
    Reduce(ScalarDyad),
    /// How many items the mapped values would have — `#` over a chain. The
    /// shapes answer that before any arithmetic runs, so none runs.
    Tally,
}

/// A fused elementwise chain and what is made of its values.
#[derive(Clone, Debug)]
pub struct FusedKernel {
    code: Vec<Instr>,
    /// Block buffers one evaluation needs at once.
    slots: usize,
    yields: Yield,
    /// The input each leaf of the chain reads, in the order the chain
    /// reaches them. Two leaves that are the same subtree share one input,
    /// so this is not the identity, and the fallback needs it to give every
    /// leaf back the value it was given.
    leaves: Vec<usize>,
    /// The axis each input is read on. None is an input the chain reads on
    /// both, which only a scalar can satisfy.
    doms: Vec<Option<Dom>>,
    /// The axis each let stands on, which is the axis every repeat it was
    /// taken for was written on.
    let_doms: Vec<Dom>,
    /// The stages the chain was built with, so that the fallback walks it
    /// exactly as the pass walked it.
    plan: Plan,
    /// The window every window step folds, when the code holds one.
    window: Option<usize>,
    /// Running folds in the code. Each carries an accumulator from block to
    /// block, so a kernel that has any runs its blocks in order.
    scans: usize,
    /// The dialect's comparison tolerance, so that a comparison inside the
    /// kernel answers exactly as the same comparison outside it does.
    tol: Tol,
}

impl FusedKernel {
    pub fn code(&self) -> &[Instr] {
        &self.code
    }

    pub fn yields(&self) -> Yield {
        self.yields
    }

    pub fn reduce(&self) -> Option<ScalarDyad> {
        match self.yields {
            Yield::Reduce(op) => Some(op),
            _ => None,
        }
    }

    /// The comparison tolerance the program was compiled with. A backend
    /// that generates its own code for this kernel needs it, so that a
    /// comparison answers there as it answers everywhere else.
    pub fn tol(&self) -> Tol {
        self.tol
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
        // APL's `f/` is the same fold monadically; only its dyad differs.
        Verb::Reduce(u) | Verb::NWise(u) => u,
        // The wrapper applies the reduction to cells of rank >= 1; over the
        // rank-1 argument this kernel insists on, that is the whole array.
        Verb::Rank(u, r) if r[0] >= 1 => match &**u {
            Verb::Reduce(inner) | Verb::NWise(inner) => inner,
            _ => return None,
        },
        _ => return None,
    };
    let Verb::Prim(p) = &**inner else { return None };
    let DyadOp::Scalar(op) = p.dyad else { return None };
    matches!(op, Add | Mul | Min | Max).then_some(op)
}

/// The moving fold this dyad performs, if the kernel can absorb it: `k u/\ y`
/// over an associative arithmetic `u` and a window the compiler knows the
/// length of. APL's n-wise reduction `n f/ y` is the same fold over the same
/// windows, so it takes the same path. A left argument of more than one
/// number is a frame in J — several window lengths, several results — and
/// stays outside; in APL it is not a window length at all.
fn absorbable_window(e: &Expr) -> Option<(ScalarDyad, usize)> {
    let (op, x) = match e {
        Expr::Dyad { verb: Verb::Windowed(u, WindowKind::Prefix), x, .. } => {
            (absorbable_reduce(u)?, x)
        }
        // `absorbable_reduce` also unwraps the rank wrapper APL's `/` wears;
        // over the rank-1 argument this kernel insists on it changes nothing.
        Expr::Dyad { verb: v @ (Verb::NWise(_) | Verb::Rank(..)), x, .. }
            if is_nwise(v) =>
        {
            (absorbable_reduce(v)?, x)
        }
        _ => return None,
    };
    let Expr::Const(a, _) = &**x else { return None };
    if a.rank() != 0 {
        return None;
    }
    let k = *a.to_i64_vec()?.first()?;
    // A negative left argument reverses each window in APL and cuts the
    // argument into chunks in J, and a zero takes the empty runs between the
    // items: none of the three is a plain moving window.
    (1..=MAX_WINDOW as i64).contains(&k).then_some((op, k as usize))
}

/// Is this verb APL's `f/` or `f⌿` — the one whose dyad is the n-wise
/// reduction — rather than J's `u/`, whose dyad is the table?
fn is_nwise(v: &Verb) -> bool {
    match v {
        Verb::NWise(_) => true,
        Verb::Rank(u, r) => r[0] >= 1 && matches!(&**u, Verb::NWise(_)),
        _ => false,
    }
}

/// The running fold this monad performs, if the kernel can absorb it. J's
/// `u\` and APL's `f\` are the same scan; `u\.` folds from the far end,
/// where an accumulator cannot be handed from one block to the next.
fn absorbable_scan(e: &Expr) -> Option<ScalarDyad> {
    let Expr::Monad { verb, .. } = e else { return None };
    // APL's `f\` scans the last axis, which over the vector this stage
    // insists on is the whole argument — the same wrapper `+/` wears.
    let inner = match verb {
        Verb::Rank(u, r) if r[0] >= 1 => &**u,
        v => v,
    };
    let Verb::Windowed(u, kind) = inner else { return None };
    if *kind == WindowKind::Suffix {
        return None;
    }
    absorbable_reduce(u)
}

/// Is this the tally, applied to the array as a whole? `#"1` and its like
/// count the items of cells instead, which is not what the shape says.
fn is_tally(v: &Verb) -> bool {
    matches!(v, Verb::Prim(p) if p.monad == MonadOp::Tally && p.ranks[0] == RANK_INF)
}

// ------------------------------------------------------------- the pass

/// The chain as a tree, before it becomes postfix code.
#[derive(Clone, PartialEq)]
enum Node {
    /// A subtree the kernel does not cover: an input, with its index.
    Leaf(usize),
    Monad(ScalarMonad, Box<Node>),
    Dyad(ScalarDyad, Box<Node>, Box<Node>),
    /// A moving fold: its operand stands on the wide axis, it on the
    /// kernel's own.
    Window(ScalarDyad, usize, Box<Node>),
    Scan(ScalarDyad, Box<Node>),
}

/// The subtrees a chain reads.
///
/// Inputs are numbered in the order the evaluator would reach them — a
/// dyad's right argument first — so that a fused node evaluates its leaves
/// exactly when and where the unfused tree does. Two leaves that are the
/// same subtree take the same input: nothing inside a chain can assign, so
/// the second writing of `+/ {x}` reads what the first one read, and
/// evaluating it once is what the sentence means either way.
#[derive(Default)]
struct Leaves<'a> {
    inputs: Vec<&'a Expr>,
    /// The input each leaf position reads, in chain order.
    order: Vec<usize>,
    /// The axis each input is read on, None where the chain reads it on
    /// both — which only a scalar can be.
    doms: Vec<Option<Dom>>,
}

impl<'a> Leaves<'a> {
    fn push(&mut self, e: &'a Expr, dom: Dom) -> usize {
        let i = match self.inputs.iter().position(|&p| same(p, e)) {
            Some(i) => i,
            None => {
                self.inputs.push(e);
                self.doms.push(Some(dom));
                self.inputs.len() - 1
            }
        };
        if self.doms[i] != Some(dom) {
            self.doms[i] = None;
        }
        self.order.push(i);
        i
    }
}

/// A name the chain reads through to the value assigned to it, as inlining
/// that assignment would; `hits` counts the uses it absorbed.
struct Inline<'a> {
    name: &'a str,
    def: &'a Expr,
    hits: usize,
}

/// The name a chain reads through, where the pass is moving one.
fn read_through<'a>(e: &Expr, sub: Option<&Inline<'a>>) -> Option<&'a Expr> {
    match (e, sub) {
        (Expr::Name(n, _), Some(s)) if n == s.name => Some(s.def),
        _ => None,
    }
}

/// The stages the chain rooted at `e` may absorb.
///
/// Decided before the chain is built and then consulted by everything that
/// walks it, so the pass, the fallback and the inliner all read the same
/// tree. Window lengths are collected from the positions a window could be
/// absorbed at; where they do not all agree there is more than one wide
/// axis, and none is taken.
fn plan_of(e: &Expr, sub: Option<&Inline<'_>>) -> Plan {
    fn walk(e: &Expr, sub: Option<&Inline<'_>>, inside: bool, ks: &mut Vec<usize>, s: &mut bool) {
        if let Some(def) = read_through(e, sub) {
            return walk(def, sub, inside, ks, s);
        }
        match e {
            Expr::Monad { verb, y, .. } if fusable_monad(verb).is_some() => {
                walk(y, sub, inside, ks, s)
            }
            Expr::Dyad { verb, x, y, .. } if fusable_dyad(verb).is_some() => {
                walk(y, sub, inside, ks, s);
                walk(x, sub, inside, ks, s);
            }
            Expr::Dyad { y, .. } if !inside && absorbable_window(e).is_some() => {
                ks.push(absorbable_window(e).expect("just matched").1);
                walk(y, sub, true, ks, s);
            }
            Expr::Monad { y, .. } if absorbable_scan(e).is_some() => {
                *s = true;
                walk(y, sub, inside, ks, s);
            }
            _ => {}
        }
    }
    let (mut ks, mut scan) = (Vec::new(), false);
    walk(e, sub, false, &mut ks, &mut scan);
    let window = match ks.split_first() {
        Some((k, rest)) if rest.iter().all(|r| r == k) => Some(*k),
        _ => None,
    };
    Plan { window, scan: window.is_none() && scan }
}

/// Build the chain rooted at `e`, collecting the subtrees that feed it.
fn chain<'a>(
    e: &'a Expr,
    lv: &mut Leaves<'a>,
    sub: &mut Option<Inline<'a>>,
    plan: Plan,
    dom: Dom,
) -> Node {
    if read_through(e, sub.as_ref()).is_some() {
        let def = read_through(e, sub.as_ref()).expect("just matched");
        if let Some(s) = sub.as_mut() {
            s.hits += 1;
        }
        return chain(def, lv, sub, plan, dom);
    }
    match e {
        Expr::Monad { verb, y, .. } => {
            if let Some(op) = fusable_monad(verb) {
                return Node::Monad(op, Box::new(chain(y, lv, sub, plan, dom)));
            }
            if plan.scan && let Some(op) = absorbable_scan(e) {
                return Node::Scan(op, Box::new(chain(y, lv, sub, plan, dom)));
            }
            Node::Leaf(lv.push(e, dom))
        }
        Expr::Dyad { verb, x, y, .. } => {
            if let Some(op) = fusable_dyad(verb) {
                let ry = chain(y, lv, sub, plan, dom);
                let rx = chain(x, lv, sub, plan, dom);
                return Node::Dyad(op, Box::new(rx), Box::new(ry));
            }
            // A window inside a window would want a third axis; only the
            // outer one is taken, and the inner reads as the leaf it is.
            if dom == Dom::Result
                && let Some((op, k)) = absorbable_window(e)
                && plan.window == Some(k)
            {
                return Node::Window(op, k, Box::new(chain(y, lv, sub, plan, Dom::Wide)));
            }
            Node::Leaf(lv.push(e, dom))
        }
        _ => Node::Leaf(lv.push(e, dom)),
    }
}

fn ops(n: &Node) -> usize {
    match n {
        Node::Leaf(_) => 0,
        Node::Monad(_, y) | Node::Window(_, _, y) | Node::Scan(_, y) => 1 + ops(y),
        Node::Dyad(_, x, y) => 1 + ops(x) + ops(y),
    }
}

/// Every subtree of the chain that computes something, with the axis it
/// stands on.
fn subtrees<'a>(n: &'a Node, dom: Dom, out: &mut Vec<(&'a Node, Dom)>) {
    if ops(n) == 0 {
        return;
    }
    out.push((n, dom));
    match n {
        Node::Leaf(_) => {}
        Node::Monad(_, y) | Node::Scan(_, y) => subtrees(y, dom, out),
        Node::Window(_, _, y) => subtrees(y, Dom::Wide, out),
        Node::Dyad(_, x, y) => {
            subtrees(x, dom, out);
            subtrees(y, dom, out);
        }
    }
}

/// The values the chain computes more than once, largest first.
///
/// `+/ d * d` over an inlined `d` writes the same arithmetic twice, and a
/// block-at-a-time kernel can do what the assignment did: compute it once
/// and read it twice. Each of these becomes a let — a block buffer of its
/// own, held for the length of the block. Only maximal repeats are taken,
/// so a repeat inside a let is part of that let rather than one more.
///
/// A value written on both axes of a windowed chain is not one value: the
/// two are different lengths and read different items, so a repeat counts
/// only against the repeats on its own axis, and only where the other axis
/// holds none.
fn lets_of(n: &Node) -> Vec<(Node, Dom)> {
    let mut all = Vec::new();
    subtrees(n, Dom::Result, &mut all);
    let mut out = Vec::new();
    fn walk(n: &Node, dom: Dom, all: &[(&Node, Dom)], out: &mut Vec<(Node, Dom)>) {
        let count = |d: Dom| all.iter().filter(|(m, md)| *m == n && *md == d).count();
        if ops(n) >= 1 && count(dom) >= 2 && count(other(dom)) == 0 {
            if !out.iter().any(|(m, _)| m == n) {
                out.push((n.clone(), dom));
            }
            return;
        }
        match n {
            Node::Leaf(_) => {}
            Node::Monad(_, y) | Node::Scan(_, y) => walk(y, dom, all, out),
            Node::Window(_, _, y) => walk(y, Dom::Wide, all, out),
            Node::Dyad(_, x, y) => {
                walk(x, dom, all, out);
                walk(y, dom, all, out);
            }
        }
    }
    walk(n, Dom::Result, &all, &mut out);
    out
}

fn other(d: Dom) -> Dom {
    match d {
        Dom::Result => Dom::Wide,
        Dom::Wide => Dom::Result,
    }
}

/// Postfix code for the chain: the lets first, each into a slot of its own,
/// then the chain that reads them.
fn emit_all(n: &Node, lets: &[(Node, Dom)], code: &mut Vec<Instr>) {
    for (k, (l, _)) in lets.iter().enumerate() {
        // A let is emitted from the lets before it, so it cannot read
        // itself; maximal repeats never nest, so there is nothing else.
        emit(l, &lets[..k], code);
        code.push(Instr::Store(k));
    }
    emit(n, lets, code);
}

/// Postfix code for the chain: a dyad's left operand is pushed first.
fn emit(n: &Node, lets: &[(Node, Dom)], code: &mut Vec<Instr>) {
    if let Some(k) = lets.iter().position(|(l, _)| l == n) {
        code.push(Instr::Let(k));
        return;
    }
    match n {
        Node::Leaf(i) => code.push(Instr::Load(*i)),
        Node::Monad(op, y) => {
            emit(y, lets, code);
            code.push(Instr::Monad(*op));
        }
        Node::Window(op, k, y) => {
            emit(y, lets, code);
            code.push(Instr::Window(*op, *k));
        }
        Node::Scan(op, y) => {
            emit(y, lets, code);
            code.push(Instr::Scan(*op));
        }
        Node::Dyad(op, x, y) => {
            emit(x, lets, code);
            emit(y, lets, code);
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
            // A let holds its buffer for the whole block: it is never
            // released, so the count it added when it was computed stands
            // and reading it takes nothing.
            Instr::Let(_) => {
                stack.push(false);
                continue;
            }
            Instr::Store(_) => {
                stack.pop();
                continue;
            }
            Instr::Monad(_) | Instr::Window(..) | Instr::Scan(_) => 1,
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
        Expr::Assign { .. }
        | Expr::PrintPass { .. }
        | Expr::Input { .. }
        | Expr::Elided { .. }
        | Expr::Control(..)
        | Expr::AmendIndex { .. }
        | Expr::VerbDef { .. }
        | Expr::ModDef { .. } => false,
        Expr::Monad { verb, y, .. } => verb.is_pure() && replayable(y),
        Expr::Dyad { verb, x, y, .. } => verb.is_pure() && replayable(x) && replayable(y),
        Expr::Fused { inputs, .. } => inputs.iter().all(replayable),
    }
}

/// Are these the same computation? Two writings of one subexpression differ
/// in their spans, which are positions in the source and mean nothing to
/// the value, so spans are not compared. Assignments, output and fused
/// nodes are never the same as anything: only leaves of a chain reach here,
/// and a chain holds none of those.
fn same(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Const(p, _), Expr::Const(q, _)) => p == q,
        (Expr::Param(p, _), Expr::Param(q, _)) => p == q,
        (Expr::Name(p, _), Expr::Name(q, _)) => p == q,
        (Expr::Monad { verb: u, y: p, .. }, Expr::Monad { verb: v, y: q, .. }) => {
            same_verb(u, v) && same(p, q)
        }
        (
            Expr::Dyad { verb: u, x: px, y: py, .. },
            Expr::Dyad { verb: v, x: qx, y: qy, .. },
        ) => same_verb(u, v) && same(px, qx) && same(py, qy),
        _ => false,
    }
}

fn same_verb(a: &Verb, b: &Verb) -> bool {
    match (a, b) {
        (Verb::Prim(p), Verb::Prim(q)) => p == q,
        (Verb::Rank(u, r), Verb::Rank(v, s)) => r == s && same_verb(u, v),
        (Verb::Reduce(u), Verb::Reduce(v))
        | (Verb::NWise(u), Verb::NWise(v))
        | (Verb::Commute(u), Verb::Commute(v)) => same_verb(u, v),
        (Verb::Windowed(u, j), Verb::Windowed(v, k)) => j == k && same_verb(u, v),
        (Verb::PowerN(u, m), Verb::PowerN(v, n)) => m == n && same_verb(u, v),
        (Verb::Fork(f, g, h), Verb::Fork(f2, g2, h2)) => {
            same_verb(f, f2) && same_verb(g, g2) && same_verb(h, h2)
        }
        (Verb::NounFork(m, g, h), Verb::NounFork(n, g2, h2)) => {
            m == n && same_verb(g, g2) && same_verb(h, h2)
        }
        (Verb::Hook(g, h), Verb::Hook(g2, h2))
        | (Verb::Atop(g, h), Verb::Atop(g2, h2))
        | (Verb::Compose(g, h), Verb::Compose(g2, h2)) => same_verb(g, g2) && same_verb(h, h2),
        (Verb::BondLeft(m, u), Verb::BondLeft(n, v)) => m == n && same_verb(u, v),
        (Verb::BondRight(u, m), Verb::BondRight(v, n)) => m == n && same_verb(u, v),
        _ => false,
    }
}

/// Optimise a compiled program's sentences: move the values that are only
/// named for the reader into the sentences that read them, then fuse every
/// chain that is left.
pub fn pass(stmts: &mut Vec<Expr>, tol: Tol) {
    let orig = std::mem::take(stmts);
    let mut cur = orig.clone();
    let mut names = 0usize;
    let mut crossed = false;
    // A round elides one assignment, so a chain of them — `m =. ...`,
    // `d =. {x} - m`, `+/ d * d` — takes one round per link.
    for _ in 0..=orig.len() {
        match inline_once(&cur, &mut names, tol) {
            Some(next) => {
                cur = next;
                crossed = true;
            }
            None => break,
        }
    }
    let mut out: Vec<Expr> = cur.into_iter().map(|e| fuse_expr(e, tol)).collect();
    if crossed {
        // What the sentences were, for `unfused` to hold this against.
        out.insert(0, Expr::Elided { orig, span: Span::new(0, 0) });
    }
    *stmts = out;
}

fn fuse_expr(e: Expr, tol: Tol) -> Expr {
    if let Some(f) = try_fuse(&e, tol) {
        return f;
    }
    match e {
        Expr::Assign { name, value, scope, span } => {
            Expr::Assign { name, value: Box::new(fuse_expr(*value, tol)), scope, span }
        }
        Expr::Monad { verb, y, span } => {
            Expr::Monad { verb, y: Box::new(fuse_expr(*y, tol)), span }
        }
        Expr::Dyad { verb, x, y, span } => Expr::Dyad {
            verb,
            x: Box::new(fuse_expr(*x, tol)),
            y: Box::new(fuse_expr(*y, tol)),
            span,
        },
        Expr::PrintPass { value, bare, span } => {
            Expr::PrintPass { value: Box::new(fuse_expr(*value, tol)), bare, span }
        }
        other => other,
    }
}

/// The kernel for the chain rooted at `root`, if it carries at least
/// `least` operations and reads nothing that cannot be replayed.
fn build<'a>(
    root: &'a Expr,
    yields: Yield,
    least: usize,
    sub: &mut Option<Inline<'a>>,
    tol: Tol,
) -> Option<(FusedKernel, Vec<&'a Expr>)> {
    if let Some(s) = sub.as_mut() {
        s.hits = 0;
    }
    let plan = plan_of(root, sub.as_ref());
    let mut lv = Leaves::default();
    let node = chain(root, &mut lv, sub, plan, Dom::Result);
    if ops(&node) < least || !lv.inputs.iter().all(|l| replayable(l)) {
        return None;
    }
    let mut code = Vec::new();
    let lets = lets_of(&node);
    emit_all(&node, &lets, &mut code);
    let window = code.iter().find_map(|i| match i {
        Instr::Window(_, k) => Some(*k),
        _ => None,
    });
    let scans = code.iter().filter(|i| matches!(i, Instr::Scan(_))).count();
    // A running fold hands its accumulator from one block to the next, so
    // its blocks run forwards and in order; an absorbed reduction folds
    // them backwards, which is the insert's own order. A chain that wants
    // both runs as the passes it was written as.
    if scans > 0 && matches!(yields, Yield::Reduce(_)) {
        return None;
    }
    let kernel = FusedKernel {
        slots: slots(&code),
        code,
        yields,
        leaves: lv.order,
        doms: lv.doms,
        let_doms: lets.iter().map(|(_, d)| *d).collect(),
        plan,
        window,
        scans,
        tol,
    };
    Some((kernel, lv.inputs))
}

/// The kernel this node becomes, with the subtree it stands for — the chain
/// itself where a tally reads only its shape, the whole sentence where a
/// reduction sits above it.
///
/// One elementwise verb on its own already runs as one pass; fusing it
/// would only add a layer. A reduction to absorb, or a tally that makes the
/// values unnecessary altogether, makes one verb enough.
fn kernel_at<'a>(
    e: &'a Expr,
    sub: &mut Option<Inline<'a>>,
    tol: Tol,
) -> Option<(FusedKernel, Vec<&'a Expr>, &'a Expr)> {
    if let Expr::Monad { verb, y, .. } = e {
        if is_tally(verb) && let Some((k, l)) = build(y, Yield::Tally, 1, sub, tol) {
            return Some((k, l, e));
        }
        if let Some(op) = absorbable_reduce(verb)
            && let Some((k, l)) = build(y, Yield::Reduce(op), 1, sub, tol)
        {
            return Some((k, l, e));
        }
    }
    let (k, l) = build(e, Yield::Values, 2, sub, tol)?;
    Some((k, l, e))
}

/// The fused node for the chain rooted at `e`, if there is one worth making.
fn try_fuse(e: &Expr, tol: Tol) -> Option<Expr> {
    let (kernel, leaves, orig) = kernel_at(e, &mut None, tol)?;
    let inputs = leaves.into_iter().map(|l| fuse_expr(l.clone(), tol)).collect();
    Some(Expr::Fused {
        kernel,
        inputs,
        orig: Box::new(orig.clone()),
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
    let plan = k.plan;
    let tree = match orig {
        // An absorbed reduction sits above the chain; only the chain's own
        // leaves were evaluated.
        Expr::Monad { verb, y, span } if matches!(k.yields, Yield::Reduce(_)) => Expr::Monad {
            verb: verb.clone(),
            y: Box::new(substitute(y, values, k, &mut next, plan, Dom::Result)),
            span: *span,
        },
        // A tally is not applied at all: the chain alone runs, and the
        // count of what it made is what the node yields.
        Expr::Monad { verb, y, .. } if k.yields == Yield::Tally && is_tally(verb) => {
            substitute(y, values, k, &mut next, plan, Dom::Result)
        }
        e => substitute(e, values, k, &mut next, plan, Dom::Result),
    };
    debug_assert_eq!(next, k.leaves.len(), "the fallback found different leaves");
    tree
}

/// What the kernel would have made of the value its chain produced. A tally
/// skips the chain entirely when it runs, and counts the items of it when
/// the chain has had to run instead.
pub(crate) fn fallback_finish(k: &FusedKernel, v: Array) -> Array {
    match k.yields {
        Yield::Tally => Array::scalar_i64(v.items() as i64),
        _ => v,
    }
}

/// Walk the chain exactly as [`chain`] walked it, so the leaves take their
/// values in the order they were numbered in.
fn substitute(
    e: &Expr,
    values: &[Array],
    k: &FusedKernel,
    next: &mut usize,
    plan: Plan,
    dom: Dom,
) -> Expr {
    match e {
        Expr::Monad { verb, y, span } if fusable_monad(verb).is_some() => Expr::Monad {
            verb: verb.clone(),
            y: Box::new(substitute(y, values, k, next, plan, dom)),
            span: *span,
        },
        Expr::Monad { verb, y, span } if plan.scan && absorbable_scan(e).is_some() => {
            Expr::Monad {
                verb: verb.clone(),
                y: Box::new(substitute(y, values, k, next, plan, dom)),
                span: *span,
            }
        }
        Expr::Dyad { verb, x, y, span } if fusable_dyad(verb).is_some() => {
            let ry = substitute(y, values, k, next, plan, dom);
            let rx = substitute(x, values, k, next, plan, dom);
            Expr::Dyad { verb: verb.clone(), x: Box::new(rx), y: Box::new(ry), span: *span }
        }
        Expr::Dyad { verb, x, y, span }
            if dom == Dom::Result
                && absorbable_window(e).map(|(_, k)| k) == plan.window
                && plan.window.is_some() =>
        {
            Expr::Dyad {
                verb: verb.clone(),
                x: x.clone(),
                y: Box::new(substitute(y, values, k, next, plan, Dom::Wide)),
                span: *span,
            }
        }
        leaf => {
            let v = values[k.leaves[*next]].clone();
            *next += 1;
            Expr::Const(v, leaf.span())
        }
    }
}

// ------------------------------------------- across sentence boundaries
//
// `d =. {x} - m` and then `+/ d * d` is the same computation as the one
// sentence that spells it out, but the assignment writes `d` to memory in
// full and the next sentence reads it back — the traffic the kernel exists
// to remove. Nothing there needs the array: the name is for the reader.
//
// So the pass moves the value into the sentences that read it, and hoists
// the value's own leaves — the mean's `+/ {x}` — into sentences of their
// own first, so that copying the chain does not copy the work. What comes
// out is the two-phase shape a hand-written kernel has: one pass for the
// reductions the chain reads as scalars, one for the map-reduce over them.

/// Names the pass introduces for the values it hoists. `·` starts no name
/// either frontend accepts, so these cannot collide with the program's.
fn hoisted_name(n: &mut usize) -> String {
    *n += 1;
    format!("·{}", *n - 1)
}

/// Elide the first assignment whose value can move into the sentences that
/// read it, and report the sentences that leaves; None when none can.
///
/// The value moves only where the name is pure dataflow:
///
/// - the value is replayable and is a chain, so that moving it moves
///   arithmetic into a kernel rather than moving a whole pass;
/// - no later sentence assigns the name again, or any name the value reads,
///   so every copy means what the original meant;
/// - every use lands inside a kernel, so no copy materialises the value.
///   A tally counts as landing inside one: it reads the chain's shape.
///
/// The assignment's own sentence stays, as the tally of the chain: that
/// reaches every leaf and every rule the kernel has, so whatever the
/// assignment would have raised is raised where it was raised before, and
/// nothing else is computed.
fn inline_once(stmts: &[Expr], names: &mut usize, tol: Tol) -> Option<Vec<Expr>> {
    for (i, stmt) in stmts.iter().enumerate() {
        let Expr::Assign { name, value, span, .. } = stmt else { continue };
        if !inlinable(stmts, i, name, value, tol) {
            continue;
        }
        if let Some(out) = rewrite(stmts, i, name, value, *span, names, tol) {
            return Some(out);
        }
    }
    None
}

fn inlinable(stmts: &[Expr], i: usize, name: &str, value: &Expr, tol: Tol) -> bool {
    if !replayable(value) || mentions(value, name) {
        return false;
    }
    let mut lv = Leaves::default();
    if ops(&chain(value, &mut lv, &mut None, plan_of(value, None), Dom::Result)) < 1 {
        return false;
    }
    let mut guarded = vec![name.to_string()];
    free_names(value, &mut guarded);
    let later = &stmts[i + 1..];
    if later.iter().any(|s| assigns_any(s, &guarded)) {
        return false;
    }
    let mut uses = 0;
    for stmt in later {
        match uses_land(stmt, name, value, tol) {
            Some(n) => uses += n,
            None => return false,
        }
    }
    uses > 0
}

/// How many uses of `name` this sentence would take into a kernel, or None
/// when one of them would have to materialise the value instead.
fn uses_land(e: &Expr, name: &str, def: &Expr, tol: Tol) -> Option<usize> {
    let mut sub = Some(Inline { name, def, hits: 0 });
    if let Some((_, leaves, _)) = kernel_at(e, &mut sub, tol) {
        let mut n = sub.map_or(0, |s| s.hits);
        for l in leaves {
            n += uses_land(l, name, def, tol)?;
        }
        return Some(n);
    }
    match e {
        Expr::Name(n, _) if n == name => None,
        Expr::Const(..) | Expr::Param(..) | Expr::Name(..) => Some(0),
        Expr::Assign { value, .. } | Expr::PrintPass { value, .. } => uses_land(value, name, def, tol),
        Expr::Monad { y, .. } => uses_land(y, name, def, tol),
        Expr::Dyad { x, y, .. } => Some(uses_land(x, name, def, tol)? + uses_land(y, name, def, tol)?),
        Expr::Fused { .. }
        | Expr::Elided { .. }
        | Expr::Input { .. }
        | Expr::Control(..)
        | Expr::AmendIndex { .. }
        | Expr::VerbDef { .. }
        | Expr::ModDef { .. } => None,
    }
}

/// The sentences that replace `stmts`, with the assignment at `i` elided.
fn rewrite(
    stmts: &[Expr],
    i: usize,
    name: &str,
    value: &Expr,
    span: Span,
    names: &mut usize,
    tol: Tol,
) -> Option<Vec<Expr>> {
    let mut lv = Leaves::default();
    let plan = plan_of(value, None);
    chain(value, &mut lv, &mut None, plan, Dom::Result);
    // A leaf that is more than a name or a constant becomes a sentence of
    // its own, evaluated once and where it was evaluated before.
    let mut hoists = Vec::new();
    let mut bound: Vec<Option<String>> = Vec::new();
    for l in &lv.inputs {
        if matches!(l, Expr::Const(..) | Expr::Param(..) | Expr::Name(..)) {
            bound.push(None);
            continue;
        }
        let n = hoisted_name(names);
        hoists.push(Expr::Assign {
            name: n.clone(),
            value: Box::new((*l).clone()),
            scope: Scope::Local,
            span: l.span(),
        });
        bound.push(Some(n));
    }
    let def = with_leaves(value, &lv, &bound, plan, Dom::Result);
    let (kernel, leaves) = build(&def, Yield::Tally, 1, &mut None, tol)?;
    let inputs = leaves.into_iter().map(|l| fuse_expr(l.clone(), tol)).collect();
    let guard = Expr::Assign {
        name: hoisted_name(names),
        value: Box::new(Expr::Fused {
            kernel,
            inputs,
            orig: Box::new(def.clone()),
            span,
        }),
        scope: Scope::Local,
        span,
    };
    let mut out = stmts[..i].to_vec();
    out.extend(hoists);
    out.push(guard);
    out.extend(stmts[i + 1..].iter().map(|s| replace_name(s, name, &def)));
    Some(out)
}

/// The chain with its hoisted leaves replaced by the names they were bound
/// to. Walks exactly as [`chain`] walks, so the leaves are the same ones.
fn with_leaves(e: &Expr, lv: &Leaves<'_>, bound: &[Option<String>], plan: Plan, dom: Dom) -> Expr {
    match e {
        Expr::Monad { verb, y, span } if fusable_monad(verb).is_some() => Expr::Monad {
            verb: verb.clone(),
            y: Box::new(with_leaves(y, lv, bound, plan, dom)),
            span: *span,
        },
        Expr::Monad { verb, y, span } if plan.scan && absorbable_scan(e).is_some() => {
            Expr::Monad {
                verb: verb.clone(),
                y: Box::new(with_leaves(y, lv, bound, plan, dom)),
                span: *span,
            }
        }
        Expr::Dyad { verb, x, y, span } if fusable_dyad(verb).is_some() => Expr::Dyad {
            verb: verb.clone(),
            x: Box::new(with_leaves(x, lv, bound, plan, dom)),
            y: Box::new(with_leaves(y, lv, bound, plan, dom)),
            span: *span,
        },
        Expr::Dyad { verb, x, y, span }
            if dom == Dom::Result
                && absorbable_window(e).map(|(_, k)| k) == plan.window
                && plan.window.is_some() =>
        {
            Expr::Dyad {
                verb: verb.clone(),
                x: x.clone(),
                y: Box::new(with_leaves(y, lv, bound, plan, Dom::Wide)),
                span: *span,
            }
        }
        leaf => {
            let bind = lv
                .inputs
                .iter()
                .position(|&p| same(p, leaf))
                .and_then(|i| bound[i].as_ref());
            match bind {
                Some(n) => Expr::Name(n.clone(), leaf.span()),
                None => leaf.clone(),
            }
        }
    }
}

fn replace_name(e: &Expr, name: &str, def: &Expr) -> Expr {
    match e {
        Expr::Name(n, _) if n == name => def.clone(),
        Expr::Assign { name: a, value, scope, span } => Expr::Assign {
            scope: *scope,
            name: a.clone(),
            value: Box::new(replace_name(value, name, def)),
            span: *span,
        },
        Expr::PrintPass { value, bare, span } => Expr::PrintPass {
            value: Box::new(replace_name(value, name, def)),
            bare: *bare,
            span: *span,
        },
        Expr::Monad { verb, y, span } => Expr::Monad {
            verb: verb.clone(),
            y: Box::new(replace_name(y, name, def)),
            span: *span,
        },
        Expr::Dyad { verb, x, y, span } => Expr::Dyad {
            verb: verb.clone(),
            x: Box::new(replace_name(x, name, def)),
            y: Box::new(replace_name(y, name, def)),
            span: *span,
        },
        other => other.clone(),
    }
}

fn mentions(e: &Expr, name: &str) -> bool {
    let mut names = Vec::new();
    free_names(e, &mut names);
    names.iter().any(|n| n == name)
}

/// Every name this subtree reads.
fn free_names(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Name(n, _) => out.push(n.clone()),
        Expr::Assign { value, .. } | Expr::PrintPass { value, .. } => free_names(value, out),
        Expr::Monad { y, .. } => free_names(y, out),
        Expr::Dyad { x, y, .. } => {
            free_names(x, out);
            free_names(y, out);
        }
        Expr::Fused { inputs, .. } => inputs.iter().for_each(|i| free_names(i, out)),
        Expr::Const(..)
        | Expr::Param(..)
        | Expr::Elided { .. }
        | Expr::Input { .. }
        | Expr::Control(..)
        | Expr::AmendIndex { .. }
        | Expr::VerbDef { .. }
        | Expr::ModDef { .. } => {}
    }
}

/// Does this sentence assign any of these names, at any depth?
fn assigns_any(e: &Expr, names: &[String]) -> bool {
    match e {
        Expr::Assign { name, value, .. } => {
            names.iter().any(|n| n == name) || assigns_any(value, names)
        }
        Expr::PrintPass { value, .. } => assigns_any(value, names),
        Expr::Monad { y, .. } => assigns_any(y, names),
        Expr::Dyad { x, y, .. } => assigns_any(x, names) || assigns_any(y, names),
        Expr::Fused { inputs, .. } => inputs.iter().any(|i| assigns_any(i, names)),
        Expr::Const(..)
        | Expr::Param(..)
        | Expr::Name(..)
        | Expr::Elided { .. }
        | Expr::Input { .. }
        | Expr::Control(..)
        | Expr::AmendIndex { .. }
        | Expr::VerbDef { .. }
        | Expr::ModDef { .. } => false,
    }
}

/// Does any sentence of this program run a fused kernel?
pub fn is_fused(p: &Program) -> bool {
    fn any(e: &Expr) -> bool {
        match e {
            Expr::Fused { .. } => true,
            Expr::Const(..)
            | Expr::Param(..)
            | Expr::Name(..)
            | Expr::Elided { .. }
            | Expr::Input { .. }
            | Expr::Control(..)
            | Expr::AmendIndex { .. }
            | Expr::VerbDef { .. }
            | Expr::ModDef { .. } => false,
            Expr::Assign { value, .. } | Expr::PrintPass { value, .. } => any(value),
            Expr::Monad { y, .. } => any(y),
            Expr::Dyad { x, y, .. } => any(x) || any(y),
        }
    }
    p.stmts.iter().any(any)
}

/// Did the pass move a named value into the sentences that read it?
pub fn is_inlined(p: &Program) -> bool {
    matches!(p.stmts.first(), Some(Expr::Elided { .. }))
}

/// The program as the plain evaluator would run it: the sentences it was
/// compiled from, with every fused node replaced by the subtree it came
/// from. The two must compute the same thing; tests hold them to it.
pub fn unfused(p: &Program) -> Program {
    fn strip(e: &Expr) -> Expr {
        match e {
            Expr::Fused { orig, .. } => strip(orig),
            Expr::Assign { name, value, scope, span } => {
                Expr::Assign {
                    name: name.clone(),
                    value: Box::new(strip(value)),
                    scope: *scope,
                    span: *span,
                }
            }
            Expr::PrintPass { value, bare, span } => {
                Expr::PrintPass { value: Box::new(strip(value)), bare: *bare, span: *span }
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
    // A program the pass rewrote across sentences kept the sentences it
    // rewrote; those, not the rewriting, are what the evaluator would run.
    let stmts = match p.stmts.first() {
        Some(Expr::Elided { orig, .. }) => orig,
        _ => &p.stmts,
    };
    out.stmts = stmts.iter().map(strip).collect();
    out
}

// ------------------------------------------------------------- dtype rules

/// The dtype the unfused pipeline gives this monad's result. None where it
/// depends on the values (`<.` of a float is an integer only if every
/// rounded value fits one), which the kernel declines rather than guess.
fn monad_type(op: ScalarMonad, a: DType) -> Option<DType> {
    use DType::*;
    use ScalarMonad::*;
    // The kernel computes in one real type; complex values are not one of
    // them, so a chain that touches one declines and runs unfused.
    if a == Complex {
        return None;
    }
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
    if a == DType::Complex || b == DType::Complex {
        return None;
    }
    match op {
        Eq | Ne | Lt | Le | Gt | Ge => Some(DType::Bool),
        DivJ => Some(DType::F64),
        Add | Sub | Mul | Min | Max | Residue => match DType::promote(a, b)? {
            DType::Bool => Some(DType::I64),
            DType::Char | DType::Symbol => None,
            t => Some(t),
        },
        _ => None,
    }
}

/// The dtype the unfused pipeline gives a fold over items of this type —
/// a moving window's, or a running one's. Booleans fold as the integers
/// they count as, which is what the windowed and scanning fast paths do.
fn fold_type(op: ScalarDyad, a: DType) -> Option<DType> {
    use ScalarDyad::*;
    if !matches!(op, Add | Mul | Min | Max) {
        return None;
    }
    match a {
        DType::Bool | DType::I64 => Some(DType::I64),
        DType::F64 => Some(DType::F64),
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
pub(crate) fn working_type(k: &FusedKernel, inputs: &[Array]) -> Option<(DType, DType)> {
    let mut stack: Vec<DType> = Vec::with_capacity(k.slots);
    let mut lets: Vec<DType> = Vec::new();
    let mut float = false;
    let mut integer_step = false;
    // The exact types and the complex ones have no blockwise kernel: a
    // fused chain over them declines and the general path evaluates it.
    if inputs.iter().any(|a| a.dtype() == DType::Complex || a.dtype().is_exact()) {
        return None;
    }
    for ins in &k.code {
        let t = match ins {
            Instr::Load(i) => inputs[*i].dtype(),
            Instr::Monad(op) => monad_type(*op, stack.pop()?)?,
            Instr::Window(op, _) | Instr::Scan(op) => fold_type(*op, stack.pop()?)?,
            Instr::Dyad(op) => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                dyad_type(*op, a, b)?
            }
            Instr::Store(k) => {
                let t = stack.pop()?;
                if lets.len() != *k {
                    return None;
                }
                lets.push(t);
                continue;
            }
            // Reading a let is not a step: the value was accounted for
            // where it was computed.
            Instr::Let(k) => {
                let t = *lets.get(*k)?;
                float |= t == DType::F64;
                stack.push(t);
                continue;
            }
        };
        // Only numbers: everything else — characters, boxes — is a type
        // the kernel has no arithmetic for and the chain must handle.
        if !t.is_numeric() {
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

/// Where a value inside a block stands. A repeated scalar stands wherever
/// it is read, so it takes the axis of whatever it is combined with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum On {
    Result,
    Wide,
    Either,
}

fn combine(a: On, b: On) -> On {
    if a == On::Either {
        b
    } else {
        a
    }
}

fn placed(d: Option<Dom>) -> On {
    match d {
        Some(Dom::Result) => On::Result,
        Some(Dom::Wide) => On::Wide,
        None => On::Either,
    }
}

/// One block of work: the result items it writes, and the items of the wide
/// axis its window steps read to write them.
#[derive(Clone, Copy)]
struct Extent {
    start: usize,
    len: usize,
    wide_start: usize,
    wide_len: usize,
}

impl Extent {
    /// The block that writes result items `start .. start + len`.
    ///
    /// The window fold cuts the wide axis into runs of `k` counted from the
    /// axis's own start and joins one run's suffix to the next run's
    /// prefix, so which items a window is folded from, and in what
    /// grouping, depend on where the window lies and never on where a block
    /// boundary fell. This block therefore reads from the start of the run
    /// its first window begins in to the end of the run its last item lies
    /// in: the same arithmetic, item for item, as one pass over the whole
    /// axis.
    fn of(start: usize, len: usize, window: Option<usize>, wide: usize) -> Extent {
        let Some(k) = window else {
            return Extent { start, len, wide_start: start, wide_len: len };
        };
        let lo = start - start % k;
        let hi = ((start + len + k - 2) / k + 1) * k;
        Extent { start, len, wide_start: lo, wide_len: hi.min(wide) - lo }
    }

    fn len_on(&self, dom: On) -> usize {
        match dom {
            On::Wide => self.wide_len,
            _ => self.len,
        }
    }
}

/// One input, in the working type: either the values themselves or one
/// value repeated, which is how a rank-0 argument reaches every element.
#[derive(Clone, Copy)]
struct Loaded<'a, T> {
    data: &'a [T],
    splat: bool,
    /// The axis the chain reads this input on.
    on: On,
    /// The index `data[0]` stands at on that axis. Zero for an argument's
    /// own buffer, and the block's own start for one staged a block at a
    /// time.
    base: usize,
}

impl<T> Loaded<'_, T> {
    #[inline]
    fn block(&self, at: &Extent, dom: On) -> &[T] {
        if self.splat {
            return &self.data[..at.len_on(dom)];
        }
        let (start, len) = read_over(self.on, at);
        &self.data[start - self.base..start - self.base + len]
    }
}

/// The range of its own axis an input standing on `on` is read over for one
/// block.
#[inline]
fn read_over(on: On, at: &Extent) -> (usize, usize) {
    match on {
        On::Wide => (at.wide_start, at.wide_len),
        _ => (at.start, at.len),
    }
}

/// An argument narrower than the working type, read where it lies.
///
/// A whole widened copy of such an argument costs two round trips of the
/// working set — writing 160 MB of freshly faulted pages, then reading them
/// back — where the values themselves are needed one block at a time and a
/// block fits in cache. So the promotion happens at the block, into a
/// staging buffer each thread reuses; the values are what the widened copy
/// would have held, element for element.
#[derive(Clone, Copy)]
enum Narrow<'a> {
    I64(&'a [i64]),
    Bool(&'a [u8]),
}

/// One input as the kernel will read it: its own buffer when that already
/// holds the working type, a narrower buffer to be promoted otherwise.
enum Source<'a, T> {
    Ready(Loaded<'a, T>),
    /// The narrow values, and the axis the chain reads them on.
    Staged(Narrow<'a>, On),
}

/// The staging buffer's element type, filled from a narrow argument.
trait FromNarrow: Copy {
    fn fill(src: Narrow<'_>, at: usize, dst: &mut [Self]);
}

impl FromNarrow for f64 {
    #[inline]
    fn fill(src: Narrow<'_>, at: usize, dst: &mut [f64]) {
        match src {
            Narrow::I64(v) => {
                for (slot, &x) in dst.iter_mut().zip(&v[at..]) {
                    *slot = x as f64;
                }
            }
            Narrow::Bool(v) => {
                for (slot, &x) in dst.iter_mut().zip(&v[at..]) {
                    *slot = x as f64;
                }
            }
        }
    }
}

impl FromNarrow for i64 {
    #[inline]
    fn fill(src: Narrow<'_>, at: usize, dst: &mut [i64]) {
        match src {
            // The working type is integer only when no input is a float.
            Narrow::I64(v) => dst.copy_from_slice(&v[at..at + dst.len()]),
            Narrow::Bool(v) => {
                for (slot, &x) in dst.iter_mut().zip(&v[at..]) {
                    *slot = x as i64;
                }
            }
        }
    }
}

/// The inputs of one run, and how wide a staging block has to be.
struct Sources<'a, T> {
    of: Vec<Source<'a, T>>,
    staged: usize,
}

impl<T: FromNarrow + Default> Sources<'_, T> {
    /// Promote every staged input's block into `stage` and hand the whole
    /// input list, block-local, to `f`.
    ///
    /// `stage` holds one region of `width` per staged input, so the regions
    /// are the same from block to block and a thread faults them once.
    fn with_block<R>(
        &self,
        at: &Extent,
        stage: &mut [T],
        width: usize,
        f: impl FnOnce(&[Loaded<'_, T>]) -> R,
    ) -> R {
        let mut k = 0;
        for s in &self.of {
            let Source::Staged(src, on) = s else { continue };
            let (start, len) = read_over(*on, at);
            T::fill(*src, start, &mut stage[k * width..k * width + len]);
            k += 1;
        }
        let mut k = 0;
        let loaded: Vec<Loaded<'_, T>> = self
            .of
            .iter()
            .map(|s| match s {
                Source::Ready(l) => *l,
                Source::Staged(_, on) => {
                    let (start, len) = read_over(*on, at);
                    let d = &stage[k * width..k * width + len];
                    k += 1;
                    Loaded { data: d, splat: false, on: *on, base: start }
                }
            })
            .collect();
        f(&loaded)
    }
}

/// What a stack entry refers to: an input, or a block buffer and the axis
/// the value in it stands on.
#[derive(Clone, Copy)]
enum Slot {
    Input(usize),
    Block(usize, On),
}

/// The buffers one thread reuses from block to block.
struct Scratch<T> {
    cells: Vec<T>,
    /// Elements one block buffer holds: a block's result items, and the
    /// halo of the wide axis its window steps read around them.
    width: usize,
    free: Vec<usize>,
    stack: Vec<Slot>,
    lets: Vec<usize>,
    /// One accumulator per running fold in the code, carried from block to
    /// block so that the fold is the one the unfused scan performs.
    carry: Vec<Option<T>>,
}

impl<T: Copy + Default> Scratch<T> {
    /// Room for one thread's blocks of `w` result items each.
    ///
    /// A window step reads the run its first window begins in and the run
    /// its last item lies in, so a block of `w` items reads fewer than
    /// `w + 3k` items of the wide axis, and every buffer is that wide.
    fn new(k: &FusedKernel, w: usize) -> Scratch<T> {
        let width = w + 3 * k.window.unwrap_or(0);
        Scratch {
            cells: vec![T::default(); k.slots * width],
            width,
            free: Vec::with_capacity(k.slots),
            stack: Vec::with_capacity(k.slots),
            lets: Vec::new(),
            carry: vec![None; k.scans],
        }
    }
}

/// The leaf loops one working type runs, one block of one instruction at a
/// time. All of a kernel's arithmetic goes through these four.
struct Steps<M, D, W, S> {
    monad: M,
    dyad: D,
    window: W,
    scan: S,
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

/// Run the kernel over one block.
///
/// `out`, when given, receives the last instruction's result directly and
/// the returned index means nothing; otherwise the result stays in the
/// block buffer that index names. None means a step left the working type
/// and the caller must fall back.
fn exec_block<T, M, D, W, S>(
    k: &FusedKernel,
    srcs: &[Loaded<'_, T>],
    at: &Extent,
    sc: &mut Scratch<T>,
    out: Option<&mut [T]>,
    steps: &Steps<M, D, W, S>,
) -> Option<usize>
where
    T: Copy,
    M: Fn(ScalarMonad, &[T], &mut [T]) -> bool,
    D: Fn(ScalarDyad, &[T], &[T], &mut [T]) -> bool,
    W: Fn(ScalarDyad, usize, &[T], usize, &mut [T]) -> bool,
    S: Fn(ScalarDyad, &[T], Option<T>, &mut [T]) -> Option<T>,
{
    let Scratch { cells, width, free, stack, lets, carry } = sc;
    let w = *width;
    stack.clear();
    free.clear();
    lets.clear();
    let nslots = cells.len() / w;
    free.extend((0..nslots).rev());
    let place = |s: &Slot| match s {
        Slot::Input(j) => srcs[*j].on,
        Slot::Block(_, o) => *o,
    };
    let last = k.code.len() - 1;
    let head = if out.is_some() { last } else { k.code.len() };
    let mut scanned = 0usize;
    for ins in &k.code[..head] {
        match ins {
            Instr::Load(j) => stack.push(Slot::Input(*j)),
            Instr::Monad(op) => {
                let a = stack.pop()?;
                let dom = place(&a);
                let len = at.len_on(dom);
                let d = free.pop()?;
                let (dst, get) = split_slots(cells, w, d);
                let av = match a {
                    Slot::Input(j) => srcs[j].block(at, dom),
                    Slot::Block(i, _) => &get(i)[..len],
                };
                if !(steps.monad)(*op, av, &mut dst[..len]) {
                    return None;
                }
                release(free, lets, a);
                stack.push(Slot::Block(d, dom));
            }
            Instr::Scan(op) => {
                let a = stack.pop()?;
                let dom = place(&a);
                let len = at.len_on(dom);
                let d = free.pop()?;
                let (dst, get) = split_slots(cells, w, d);
                let av = match a {
                    Slot::Input(j) => srcs[j].block(at, dom),
                    Slot::Block(i, _) => &get(i)[..len],
                };
                carry[scanned] = Some((steps.scan)(*op, av, carry[scanned], &mut dst[..len])?);
                scanned += 1;
                release(free, lets, a);
                stack.push(Slot::Block(d, dom));
            }
            Instr::Window(op, size) => {
                let a = stack.pop()?;
                let d = free.pop()?;
                let (dst, get) = split_slots(cells, w, d);
                let av = match a {
                    Slot::Input(j) => srcs[j].block(at, On::Wide),
                    Slot::Block(i, _) => &get(i)[..at.wide_len],
                };
                let first = at.start - at.wide_start;
                if !(steps.window)(*op, *size, av, first, &mut dst[..at.len]) {
                    return None;
                }
                release(free, lets, a);
                stack.push(Slot::Block(d, On::Result));
            }
            Instr::Dyad(op) => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                let dom = combine(place(&a), place(&b));
                let len = at.len_on(dom);
                let d = free.pop()?;
                let (dst, get) = split_slots(cells, w, d);
                let av = match a {
                    Slot::Input(j) => srcs[j].block(at, dom),
                    Slot::Block(i, _) => &get(i)[..len],
                };
                let bv = match b {
                    Slot::Input(j) => srcs[j].block(at, dom),
                    Slot::Block(i, _) => &get(i)[..len],
                };
                if !(steps.dyad)(*op, av, bv, &mut dst[..len]) {
                    return None;
                }
                for s in [a, b] {
                    release(free, lets, s);
                }
                stack.push(Slot::Block(d, dom));
            }
            Instr::Store(j) => {
                let Slot::Block(i, _) = stack.pop()? else { return None };
                if lets.len() != *j {
                    return None;
                }
                lets.push(i);
            }
            // A let stands where the pass computed it, which is the one
            // axis every repeat it stands for was written on.
            Instr::Let(j) => {
                stack.push(Slot::Block(*lets.get(*j)?, placed(Some(*k.let_doms.get(*j)?))))
            }
        }
    }
    let Some(dst) = out else {
        return match stack.pop()? {
            Slot::Block(i, _) => Some(i),
            // Every kernel ends in an operation, so the result is a buffer.
            Slot::Input(_) => None,
        };
    };
    // The last instruction writes the caller's buffer instead of a block.
    // The chain's root stands on the result's own axis, whatever its
    // operands stand on.
    let dst = &mut dst[..at.len];
    let view = |s: Slot, dom: On| match s {
        Slot::Input(j) => srcs[j].block(at, dom),
        Slot::Block(i, o) => &cells[i * w..i * w + at.len_on(o)],
    };
    let ok = match k.code[last] {
        Instr::Monad(op) => {
            let a = stack.pop()?;
            (steps.monad)(op, view(a, On::Result), dst)
        }
        Instr::Scan(op) => {
            let a = stack.pop()?;
            match (steps.scan)(op, view(a, On::Result), carry[scanned], dst) {
                Some(c) => {
                    carry[scanned] = Some(c);
                    true
                }
                None => false,
            }
        }
        Instr::Window(op, size) => {
            let a = stack.pop()?;
            let first = at.start - at.wide_start;
            (steps.window)(op, size, view(a, On::Wide), first, dst)
        }
        Instr::Dyad(op) => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            let dom = combine(place(&a), place(&b));
            (steps.dyad)(op, view(a, dom), view(b, dom), dst)
        }
        // A kernel ends in the operation that makes its result.
        Instr::Load(_) | Instr::Store(_) | Instr::Let(_) => return None,
    };
    ok.then_some(usize::MAX)
}

/// Give a block buffer back, unless a let is holding it for the rest of
/// the block.
fn release(free: &mut Vec<usize>, lets: &[usize], s: Slot) {
    if let Slot::Block(i, _) = s
        && !lets.contains(&i)
    {
        free.push(i);
    }
}

/// The whole mapped result, one block at a time. None on integer overflow.
fn map_pass<T, M, D, W, S>(
    k: &FusedKernel,
    srcs: &Sources<'_, T>,
    n: usize,
    wide: usize,
    steps: &Steps<M, D, W, S>,
) -> Option<Vec<T>>
where
    T: FromNarrow + Default + Send + Sync,
    M: Fn(ScalarMonad, &[T], &mut [T]) -> bool + Sync + Send,
    D: Fn(ScalarDyad, &[T], &[T], &mut [T]) -> bool + Sync + Send,
    W: Fn(ScalarDyad, usize, &[T], usize, &mut [T]) -> bool + Sync + Send,
    S: Fn(ScalarDyad, &[T], Option<T>, &mut [T]) -> Option<T> + Sync + Send,
{
    let run = |start: usize, part: &mut [T]| {
        let w = BLOCK.min(part.len()).max(1);
        let mut sc = Scratch::new(k, w);
        let width = sc.width;
        let mut stage = vec![T::default(); srcs.staged * width];
        for (b, chunk) in part.chunks_mut(w).enumerate() {
            let at = Extent::of(start + b * w, chunk.len(), k.window, wide);
            let done = srcs.with_block(&at, &mut stage, width, |loaded| {
                exec_block(k, loaded, &at, &mut sc, Some(chunk), steps).is_some()
            });
            if !done {
                return false;
            }
        }
        true
    };
    if k.scans > 0 {
        // A running fold hands its accumulator to the next block, so the
        // blocks run in one order on one thread. That is the order the
        // unfused scan runs them in, and it rounds where that rounds.
        let mut out = vec![T::default(); n];
        return run(0, &mut out).then_some(out);
    }
    let (out, ok) = par::fill(n, run);
    ok.then_some(out)
}

/// Independent accumulators the fold over a block keeps in flight, and the
/// block length below which one accumulator is cheaper. The reasoning is
/// the one `verb::FOLD_LANES` carries: a single accumulator makes the fold
/// a chain of dependent steps, and only an associative step is ever
/// absorbed here, so the lanes are a regrouping the float contract already
/// allows (§5.9).
const FOLD_LANES: usize = 8;
const MIN_LANE_WORK: usize = 8 * FOLD_LANES;

/// Fold one block of mapped values right to left, in lanes. None when a
/// step left the element type.
#[inline(always)]
fn fold_block_body<T, S>(v: &[T], step: &S) -> Option<T>
where
    T: Copy,
    S: Fn(T, T) -> Option<T>,
{
    let n = v.len();
    if n < MIN_LANE_WORK {
        let mut acc = v[n - 1];
        for &x in v[..n - 1].iter().rev() {
            acc = step(x, acc)?;
        }
        return Some(acc);
    }
    let rows = n / FOLD_LANES;
    let head = n - rows * FOLD_LANES;
    let last = head + (rows - 1) * FOLD_LANES;
    let mut acc = [v[last]; FOLD_LANES];
    acc.copy_from_slice(&v[last..last + FOLD_LANES]);
    for r in (0..rows - 1).rev() {
        let row = &v[head + r * FOLD_LANES..head + (r + 1) * FOLD_LANES];
        for (slot, &x) in acc.iter_mut().zip(row) {
            *slot = step(x, *slot)?;
        }
    }
    let mut a = acc[FOLD_LANES - 1];
    for &x in acc[..FOLD_LANES - 1].iter().rev() {
        a = step(x, a)?;
    }
    for &x in v[..head].iter().rev() {
        a = step(x, a)?;
    }
    Some(a)
}

multiversioned! {
    /// One block's values folded into one, at the CPU's own width.
    fn fold_block[T: Copy, S: Fn(T, T) -> Option<T>](
        v: &[T],
        step: &S,
    ) -> Option<T> = fold_block_body;
}

/// Fold the mapped values of `lo .. hi` right to left, block by block.
fn fold_range<T, M, D, W, C, S>(
    k: &FusedKernel,
    srcs: &Sources<'_, T>,
    lo: usize,
    hi: usize,
    wide: usize,
    steps: &Steps<M, D, W, C>,
    step: &S,
) -> Option<T>
where
    T: FromNarrow + Default,
    M: Fn(ScalarMonad, &[T], &mut [T]) -> bool,
    D: Fn(ScalarDyad, &[T], &[T], &mut [T]) -> bool,
    W: Fn(ScalarDyad, usize, &[T], usize, &mut [T]) -> bool,
    C: Fn(ScalarDyad, &[T], Option<T>, &mut [T]) -> Option<T>,
    S: Fn(T, T) -> Option<T>,
{
    let w = BLOCK.min(hi - lo).max(1);
    let mut sc = Scratch::new(k, w);
    let width = sc.width;
    let mut stage = vec![T::default(); srcs.staged * width];
    let mut acc: Option<T> = None;
    // Blocks run backwards and the accumulator carries across them, so the
    // fold is the insert's own right-to-left order over the whole range.
    // Nothing a block computes depends on the block before it: a running
    // fold, which would, is never absorbed under a reduction.
    for b in (0..(hi - lo).div_ceil(w)).rev() {
        let start = lo + b * w;
        let len = (hi - start).min(w);
        let at = Extent::of(start, len, k.window, wide);
        let slot = srcs
            .with_block(&at, &mut stage, width, |loaded| {
                exec_block(k, loaded, &at, &mut sc, None, steps)
            })?;
        let block = fold_block(&sc.cells[slot * sc.width..slot * sc.width + len], step)?;
        acc = Some(match acc {
            None => block,
            Some(a) => step(block, a)?,
        });
    }
    acc
}

/// The mapped values folded into one. None on integer overflow.
fn reduce_pass<T, M, D, W, C, S>(
    k: &FusedKernel,
    srcs: &Sources<'_, T>,
    n: usize,
    wide: usize,
    steps: &Steps<M, D, W, C>,
    step: S,
) -> Option<T>
where
    T: FromNarrow + Default + Send + Sync,
    M: Fn(ScalarMonad, &[T], &mut [T]) -> bool + Sync + Send,
    D: Fn(ScalarDyad, &[T], &[T], &mut [T]) -> bool + Sync + Send,
    W: Fn(ScalarDyad, usize, &[T], usize, &mut [T]) -> bool + Sync + Send,
    C: Fn(ScalarDyad, &[T], Option<T>, &mut [T]) -> Option<T> + Sync + Send,
    S: Fn(T, T) -> Option<T> + Sync + Send,
{
    let chunks = par::chunks(n, n * k.code.len());
    if chunks < 2 {
        return fold_range(k, srcs, 0, n, wide, steps, &step);
    }
    let per = n.div_ceil(chunks);
    let parts = par::map_indexed(n.div_ceil(per), |c| {
        fold_range(k, srcs, c * per, ((c + 1) * per).min(n), wide, steps, &step)
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
//
// These four are the whole arithmetic of a kernel, so they are also where
// the CPU feature levels are chosen: each is compiled once per level (see
// `simd`) and the call dispatches on what the machine runs. One block of
// one instruction is thousands of elements, so the dispatch costs nothing
// measurable.

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

#[inline(always)]
fn monad_f64_body(op: ScalarMonad, a: &[f64], dst: &mut [f64], tol: Tol) -> bool {
    use ScalarMonad::*;
    match op {
        Conj => each!(a, dst, |x: f64| x),
        Neg => each!(a, dst, |x: f64| -x),
        Abs => each!(a, dst, f64::abs),
        // A magnitude the dialect's tolerance reads as zero has no sign,
        // exactly as unfused.
        Signum => each!(a, dst, |x: f64| if tol.is_zero(x) {
            0.0
        } else if x > 0.0 {
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

#[inline(always)]
fn dyad_f64_body(op: ScalarDyad, a: &[f64], b: &[f64], dst: &mut [f64], tol: Tol) -> bool {
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
        // An infinite modulus leaves a value of its own sign alone and
        // sends the other one to that infinity, exactly as unfused.
        Residue => zip!(a, b, dst, |x: f64, y: f64| if x.is_infinite() {
            if y == 0.0 || (y > 0.0) == (x > 0.0) { y } else { x }
        } else if x == 0.0 {
            y
        } else {
            y - x * (y / x).floor()
        }),
        // A comparison is a number here, as it is in J: the boolean only
        // shows in the dtype of a result, which the caller narrows. Floats
        // compare with the dialect's tolerance, as they do unfused.
        Eq | Ne | Lt | Le | Gt | Ge => {
            zip!(a, b, dst, |x: f64, y: f64| tol_cmp(op, x, y, tol) as u8 as f64)
        }
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

#[inline(always)]
fn monad_i64_body(op: ScalarMonad, a: &[i64], dst: &mut [i64]) -> bool {
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

#[inline(always)]
fn dyad_i64_body(op: ScalarDyad, a: &[i64], b: &[i64], dst: &mut [i64]) -> bool {
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

multiversioned! {
    /// One instruction of a kernel over one block of floats: the monadic
    /// operations. False is unreachable — every operation a kernel holds is
    /// covered — and exists so the two passes have one signature.
    fn monad_f64(
        op: ScalarMonad,
        a: &[f64],
        dst: &mut [f64],
        tol: Tol,
    ) -> bool = monad_f64_body;
}

multiversioned! {
    /// One instruction of a kernel over one block of floats: the dyadic
    /// operations.
    fn dyad_f64(
        op: ScalarDyad,
        a: &[f64],
        b: &[f64],
        dst: &mut [f64],
        tol: Tol,
    ) -> bool = dyad_f64_body;
}

multiversioned! {
    /// One instruction of a kernel over one block of integers: the monadic
    /// operations. False means the block left i64.
    fn monad_i64(op: ScalarMonad, a: &[i64], dst: &mut [i64]) -> bool = monad_i64_body;
}

multiversioned! {
    /// One instruction of a kernel over one block of integers: the dyadic
    /// operations. False means the block left i64.
    fn dyad_i64(op: ScalarDyad, a: &[i64], b: &[i64], dst: &mut [i64]) -> bool = dyad_i64_body;
}

/// One block's running fold, continued from the accumulator the block
/// before it left. None when a step left the element type.
///
/// The accumulator runs the length of the argument, one step per item, in
/// the order the unfused scan takes them: what the fused kernel saves is
/// the traffic around the scan, not the scan.
#[inline(always)]
fn scan_block_body<T, F>(v: &[T], carry: Option<T>, dst: &mut [T], step: &F) -> Option<T>
where
    T: Copy,
    F: Fn(T, T) -> (T, bool),
{
    let mut over = false;
    let (mut acc, from) = match carry {
        Some(a) => (a, 0),
        None => {
            // The first item of a scan is the item itself.
            dst[0] = v[0];
            (v[0], 1)
        }
    };
    for (slot, &x) in dst.iter_mut().zip(v).skip(from) {
        let (r, o) = step(acc, x);
        acc = r;
        over |= o;
        *slot = acc;
    }
    (!over).then_some(acc)
}

multiversioned! {
    /// One block of a running fold. The steps depend on one another, so
    /// what a wider vector reaches here is the loop around them.
    fn scan_block[T: Copy, F: Fn(T, T) -> (T, bool)](
        v: &[T],
        carry: Option<T>,
        dst: &mut [T],
        step: &F,
    ) -> Option<T> = scan_block_body;
}

/// The windows of a block of floats, folded one per result item. The step
/// is chosen before the fold so that the fold itself is one plain loop.
fn window_pass_f64(op: ScalarDyad, k: usize, v: &[f64], first: usize, dst: &mut [f64]) -> bool {
    use ScalarDyad::*;
    match op {
        Add => windows_into(v, k, first, dst, &|a: f64, b: f64| (a + b, false)),
        Mul => windows_into(v, k, first, dst, &|a: f64, b: f64| (a * b, false)),
        Min => windows_into(v, k, first, dst, &|a: f64, b: f64| (a.min(b), false)),
        Max => windows_into(v, k, first, dst, &|a: f64, b: f64| (a.max(b), false)),
        _ => false,
    }
}

fn window_pass_i64(op: ScalarDyad, k: usize, v: &[i64], first: usize, dst: &mut [i64]) -> bool {
    use ScalarDyad::*;
    match op {
        Add => windows_into(v, k, first, dst, &i64::overflowing_add),
        Mul => windows_into(v, k, first, dst, &i64::overflowing_mul),
        Min => windows_into(v, k, first, dst, &|a: i64, b: i64| (a.min(b), false)),
        Max => windows_into(v, k, first, dst, &|a: i64, b: i64| (a.max(b), false)),
        _ => false,
    }
}

fn scan_pass_f64(op: ScalarDyad, v: &[f64], carry: Option<f64>, dst: &mut [f64]) -> Option<f64> {
    use ScalarDyad::*;
    match op {
        Add => scan_block(v, carry, dst, &|a: f64, b: f64| (a + b, false)),
        Mul => scan_block(v, carry, dst, &|a: f64, b: f64| (a * b, false)),
        Min => scan_block(v, carry, dst, &|a: f64, b: f64| (a.min(b), false)),
        Max => scan_block(v, carry, dst, &|a: f64, b: f64| (a.max(b), false)),
        _ => None,
    }
}

fn scan_pass_i64(op: ScalarDyad, v: &[i64], carry: Option<i64>, dst: &mut [i64]) -> Option<i64> {
    use ScalarDyad::*;
    match op {
        Add => scan_block(v, carry, dst, &i64::overflowing_add),
        Mul => scan_block(v, carry, dst, &i64::overflowing_mul),
        Min => scan_block(v, carry, dst, &|a: i64, b: i64| (a.min(b), false)),
        Max => scan_block(v, carry, dst, &|a: i64, b: i64| (a.max(b), false)),
        _ => None,
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

/// One fold step of an absorbed float reduction, for a backend that mapped
/// the values elsewhere and brings its partials back here to combine.
pub(crate) fn step(op: ScalarDyad, a: f64, b: f64) -> Option<f64> {
    step_f64(op, a, b)
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

/// A rank-0 argument as one block of the repeated value, which is how it
/// reaches every element without an index test. None for an argument with
/// items, which is read where it lies.
fn splat_f64(a: &Array, w: usize) -> Option<Vec<f64>> {
    if a.rank() != 0 {
        return None;
    }
    let v = match &a.data {
        Data::Bool(d) => d[0] as f64,
        Data::I64(d) => d[0] as f64,
        Data::F64(d) => d[0],
        Data::Ext(_)
        | Data::Rat(_)
        | Data::Complex(_)
        | Data::Char(_)
        | Data::Symbol(_)
        | Data::Box(_) => {
            return Some(Vec::new());
        }
    };
    Some(vec![v; w])
}

fn splat_i64(a: &Array, w: usize) -> Option<Vec<i64>> {
    if a.rank() != 0 {
        return None;
    }
    let v = match &a.data {
        Data::Bool(d) => d[0] as i64,
        Data::I64(d) => d[0],
        _ => return Some(Vec::new()),
    };
    Some(vec![v; w])
}

/// An argument with items, seen by a float kernel: its own buffer when that
/// is already f64, the narrow values otherwise.
fn narrow_f64(a: &Array) -> Result<&[f64], Narrow<'_>> {
    match &a.data {
        Data::I64(d) => Err(Narrow::I64(d)),
        Data::Bool(d) => Err(Narrow::Bool(d)),
        _ => Ok(a.as_f64_slice().unwrap_or(&[])),
    }
}

fn narrow_i64(a: &Array) -> Result<&[i64], Narrow<'_>> {
    match &a.data {
        Data::Bool(d) => Err(Narrow::Bool(d)),
        _ => Ok(a.as_i64_slice().unwrap_or(&[])),
    }
}

/// The input list one run reads: a repeated scalar from `owned`, an
/// argument's own buffer, or a narrow buffer to be promoted block by block.
fn sources<'a, T>(
    inputs: &'a [Array],
    owned: &'a [Option<Vec<T>>],
    on: &impl Fn(usize) -> On,
    narrow: impl Fn(&'a Array) -> Result<&'a [T], Narrow<'a>>,
) -> Sources<'a, T> {
    let mut staged = 0;
    let of = inputs
        .iter()
        .zip(owned)
        .enumerate()
        .map(|(j, (a, o))| match o {
            Some(v) => Source::Ready(Loaded { data: v, splat: true, on: on(j), base: 0 }),
            None => match narrow(a) {
                Ok(d) => Source::Ready(Loaded { data: d, splat: false, on: on(j), base: 0 }),
                Err(n) => {
                    staged += 1;
                    Source::Staged(n, on(j))
                }
            },
        })
        .collect();
    Sources { of, staged }
}

/// The shape every element of the result has: identical for all non-scalar
/// inputs, since anything else needs the agreement machinery.
pub(crate) fn common_shape(inputs: &[Array]) -> Option<Option<Vec<usize>>> {
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

/// The axes a kernel's inputs stand on: the shape of its result, and the
/// length of the wide axis its window steps read.
///
/// This is the whole of the alignment rule, and it is decided by shapes
/// alone. Where a chain reads an input is settled when the chain is built —
/// everything under a window step is wide — so all that is left at run time
/// is that the inputs on one axis agree with each other, and that the two
/// axes stand `k - 1` items apart. `19 }. y` beside `20 +/\ y` passes
/// because it is 19 items shorter; `18 }. y` beside it does not, and the
/// chain runs and raises the length error it was going to raise. Nothing is
/// shifted or padded here: an input arrives as the items it holds.
struct Axes {
    shape: Vec<usize>,
    /// Items of the wide axis, when there is a window step to read it.
    wide: usize,
}

fn axes(k: &FusedKernel, inputs: &[Array]) -> Option<Axes> {
    let Some(window) = k.window else {
        // Every input a scalar: no work worth blocking, and a reduction
        // would need the leading axis a scalar has not got.
        let shape = common_shape(inputs)??;
        // A running fold folds items, and a block of this kernel is
        // elements: over anything but a vector the two are not the same
        // fold, so a higher-rank argument goes the way it went.
        if k.scans > 0 && shape.len() != 1 {
            return None;
        }
        return Some(Axes { shape, wide: 0 });
    };
    let (mut wide, mut result) = (None, None);
    for (a, dom) in inputs.iter().zip(&k.doms) {
        // A scalar reaches every item of whatever it is combined with, so
        // it stands on either axis and constrains neither.
        if a.rank() == 0 {
            continue;
        }
        // A window folds the items of a vector. An input the chain reads on
        // both axes cannot be two lengths at once.
        let (Some(d), 1) = (dom, a.rank()) else { return None };
        let seen = if *d == Dom::Wide { &mut wide } else { &mut result };
        match seen {
            None => *seen = Some(a.shape[0]),
            Some(m) if *m == a.shape[0] => {}
            Some(_) => return None,
        }
    }
    let wide = wide?;
    if wide < window {
        // No window fits: the result has no items, which the chain builds
        // out of the verb's own answer for an empty argument.
        return None;
    }
    let count = wide - window + 1;
    if result.is_some_and(|m| m != count) {
        return None;
    }
    Some(Axes { shape: vec![count], wide })
}

/// Run a fused node. None means the kernel declined and the caller must
/// evaluate the original subtree, which is always allowed to be slower and
/// never allowed to differ.
pub(crate) fn run(k: &FusedKernel, inputs: &[Array]) -> Option<Array> {
    let reducing = matches!(k.yields, Yield::Reduce(_));
    let Axes { shape, wide } = axes(k, inputs)?;
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
    if k.yields == Yield::Tally {
        // The shapes have already said how many items the chain produces,
        // and the type rules have said it would reach them without an
        // error. There is nothing else a tally wants from the values.
        return Some(Array::scalar_i64(shape[0] as i64));
    }
    // A repeated scalar is one block long, and a block reads its window
    // halo as well as its own items.
    let w = BLOCK.min(n).max(1) + 3 * k.window.unwrap_or(0);
    // The kernel's comparisons carry the tolerance the program was compiled
    // with, so a fused comparison answers as the unfused one does.
    let tol = k.tol;
    let on = |j: usize| placed(k.doms[j]);

    let data = if working == DType::F64 {
        let steps = Steps {
            monad: move |op, a: &[f64], dst: &mut [f64]| monad_f64(op, a, dst, tol),
            dyad: move |op, a: &[f64], b: &[f64], dst: &mut [f64]| dyad_f64(op, a, b, dst, tol),
            window: window_pass_f64,
            scan: scan_pass_f64,
        };
        let owned: Vec<Option<Vec<f64>>> = inputs.iter().map(|a| splat_f64(a, w)).collect();
        let srcs = sources(inputs, &owned, &on, narrow_f64);
        match k.reduce() {
            None => {
                let out = map_pass(k, &srcs, n, wide, &steps)?;
                float_result(out, root)
            }
            Some(op) => {
                let v = reduce_pass(k, &srcs, n, wide, &steps, |a, b| step_f64(op, a, b))?;
                // A comparison at the root maps to exact 0 and 1, which the
                // fold keeps exact; the reduction of booleans is integer.
                match root {
                    DType::F64 => Data::F64(vec![v].into()),
                    _ => Data::I64(vec![v as i64].into()),
                }
            }
        }
    } else {
        let steps = Steps {
            monad: monad_i64,
            dyad: dyad_i64,
            window: window_pass_i64,
            scan: scan_pass_i64,
        };
        let owned: Vec<Option<Vec<i64>>> = inputs.iter().map(|a| splat_i64(a, w)).collect();
        let srcs = sources(inputs, &owned, &on, narrow_i64);
        match k.reduce() {
            None => {
                let out = map_pass(k, &srcs, n, wide, &steps)?;
                int_result(out, root)
            }
            Some(op) => {
                let v = reduce_pass(k, &srcs, n, wide, &steps, |a, b| step_i64(op, a, b))?;
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

// -------------------------------------------------------- describing one
//
// Read-only descriptions of a compiled kernel, for `Program::explain`.
// Nothing here runs a kernel or changes one; the summary is derived from
// the code the pass emitted, and the decline reason re-checks the same
// preconditions `run` checks before it starts.

/// Why a kernel handed its work back to the chain it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decline {
    /// Inputs disagree on shape, or every input is a scalar: broadcasting
    /// and agreement are the chain's business.
    Agreement,
    /// Nothing to compute.
    Empty,
    /// An absorbed reduction wants one axis with at least two items.
    ReduceShape,
    /// One working type cannot hold every step exactly — a chain that
    /// computes integers along a float path, or non-numeric data.
    WorkingType,
    /// The preconditions held, so a step went out of range mid-block:
    /// integer overflow, which the chain redoes in a wider type.
    Overflow,
    /// A window step wants one vector axis longer than the window, and
    /// every other input aligned on the window's last item.
    Window,
}

impl Decline {
    pub fn reason(self) -> &'static str {
        match self {
            Decline::Agreement => "the inputs need agreement or are all scalars",
            Decline::Empty => "there is nothing to compute",
            Decline::ReduceShape => "the reduction needs one axis of two or more items",
            Decline::WorkingType => "no single working type holds every step exactly",
            Decline::Overflow => "an integer step left 64-bit range",
            Decline::Window => "the window does not fit the axis, or the inputs are not aligned with it",
        }
    }
}

/// Why this kernel would decline these inputs, or None if it would run.
///
/// A read-only mirror of the preconditions at the top of `run`: it looks
/// at shapes and dtypes only, never at values, so the one thing it cannot
/// see in advance is an overflow — which is what is left when every
/// precondition holds.
pub fn decline_reason(k: &FusedKernel, inputs: &[Array]) -> Option<Decline> {
    let Some(Axes { shape, .. }) = axes(k, inputs) else {
        return Some(if k.window.is_some() { Decline::Window } else { Decline::Agreement });
    };
    let n: usize = shape.iter().product();
    if n == 0 {
        return Some(Decline::Empty);
    }
    if matches!(k.yields, Yield::Reduce(_)) && (shape.len() != 1 || n < 2) {
        return Some(Decline::ReduceShape);
    }
    if working_type(k, inputs).is_none() {
        return Some(Decline::WorkingType);
    }
    Some(Decline::Overflow)
}

/// What a compiled kernel is made of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// Arithmetic steps: the monads and dyads, not the loads and stores.
    pub ops: usize,
    /// Those steps in the order the kernel performs them.
    pub op_names: Vec<String>,
    /// The reduction folded into the same pass, if there is one.
    pub reduce: Option<&'static str>,
    /// True when the whole chain collapsed to a count of its own items.
    pub tally: bool,
    /// Values the kernel keeps for a second read within one block.
    pub lets: usize,
    /// Subtrees the chain reads.
    pub inputs: usize,
    /// Elements one block buffer holds.
    pub block: usize,
    /// The window every window step folds, when the kernel has one.
    pub window: Option<usize>,
    /// Running folds the kernel carries from block to block.
    pub scans: usize,
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} op{}", self.ops, if self.ops == 1 { "" } else { "s" })?;
        if !self.op_names.is_empty() {
            write!(f, ": {}", self.op_names.join(" "))?;
        }
        if let Some(r) = self.reduce {
            write!(f, "; {r}/ absorbed")?;
        }
        if self.tally {
            write!(f, "; tally only")?;
        }
        if self.lets > 0 {
            write!(f, "; {} let slot{}", self.lets, if self.lets == 1 { "" } else { "s" })?;
        }
        if let Some(k) = self.window {
            write!(f, "; window {k}")?;
        }
        if self.scans > 0 {
            write!(f, "; {} running fold{}", self.scans, if self.scans == 1 { "" } else { "s" })?;
        }
        write!(f, "; block {}", self.block)
    }
}

/// Describe a compiled kernel: what it computes, and with what.
pub fn summary(k: &FusedKernel) -> Summary {
    let mut op_names: Vec<String> = Vec::new();
    let mut lets = 0usize;
    for ins in &k.code {
        match ins {
            Instr::Monad(op) => op_names.push(monad_name(*op).to_string()),
            Instr::Dyad(op) => op_names.push(dyad_name(*op).to_string()),
            Instr::Window(op, k) => op_names.push(format!("{k} {}/\\", dyad_name(*op))),
            Instr::Scan(op) => op_names.push(format!("{}/\\", dyad_name(*op))),
            Instr::Store(_) => lets += 1,
            Instr::Load(_) | Instr::Let(_) => {}
        }
    }
    Summary {
        ops: op_names.len(),
        op_names,
        reduce: k.reduce().map(dyad_name),
        tally: k.yields == Yield::Tally,
        lets,
        inputs: k.leaves.iter().copied().max().map_or(0, |m| m + 1),
        block: BLOCK,
        window: k.window,
        scans: k.scans,
    }
}

/// The names the pass took out of the program: values it moved into the
/// kernels that read them, so no sentence computes them as arrays any more.
pub fn inlined_names(p: &Program) -> Vec<String> {
    let Some(Expr::Elided { orig, .. }) = p.stmts.first() else { return Vec::new() };
    let assigned = |stmts: &[Expr]| -> Vec<String> {
        stmts
            .iter()
            .filter_map(|s| match s {
                Expr::Assign { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    };
    let kept = assigned(&p.stmts);
    assigned(orig).into_iter().filter(|n| !kept.contains(n)).collect()
}

/// J spellings for the elementwise operations a kernel can hold. Only the
/// naming lives here; the meanings are [`crate::verb`]'s.
fn monad_name(op: ScalarMonad) -> &'static str {
    use ScalarMonad::*;
    match op {
        Conj => "+",
        Neg => "-",
        Signum => "*",
        Recip => "%",
        Sqrt => "%:",
        Exp => "^",
        Abs => "|",
        Floor => "<.",
        Ceil => ">.",
        Not => "-.",
        OneMinus => "-.",
        Inc => ">:",
        Dec => "<:",
        Double => "+:",
        Halve => "-:",
        Square => "*:",
        Ln => "^.",
        Pi => "o.",
        Factorial => "!",
        Imaginary => "j.",
        Polar => "r.",
    }
}

fn dyad_name(op: ScalarDyad) -> &'static str {
    use ScalarDyad::*;
    match op {
        Add => "+",
        Sub => "-",
        Mul => "*",
        DivJ | DivApl => "%",
        Min => "<.",
        Max => ">.",
        Pow => "^",
        Residue => "|",
        Eq => "=",
        Ne => "~:",
        Lt => "<",
        Le => "<:",
        Gt => ">",
        Ge => ">:",
        Lcm => "*.",
        Gcd => "+.",
        Log => "^.",
        Root => "%:",
        Circle => "o.",
        Binomial => "!",
        MakeComplex => "j.",
        PolarBy => "r.",
    }
}

/// Evaluate a fused node from its already-evaluated inputs, or report that
/// the original subtree must run instead.
///
/// This is the one place a device gets to run libjay's arithmetic. With a
/// device attached the kernel is offered to it first; everything it will not
/// take comes back here with a reason, and the CPU path runs exactly as it
/// runs with no device in sight. The device therefore cannot change a
/// result's shape, dtype or error — only where the arithmetic happened.
pub(crate) fn eval_on(
    device: Option<&crate::device::Device>,
    k: &FusedKernel,
    inputs: &[Array],
) -> (Option<Array>, crate::device::Placement) {
    use crate::device::Placement;
    let mut placement = Placement::Default;
    // A block kernel reads every input and writes every slot at the same
    // index, so the order the buffers are laid out in cannot reach the
    // result — as long as every non-scalar input is laid out the same way.
    // Then the answer is laid out that way too, and no transpose is made.
    let materialised: Vec<Array>;
    let (inputs, layout) = match kernel_layout(inputs) {
        Some(l) => (inputs, l),
        None => {
            materialised = inputs.iter().map(Array::to_row_major).collect();
            (&materialised[..], Layout::RowMajor)
        }
    };
    // The device is offered row-major work only: uploading a matrix that is
    // faster to fold where it lies would be the wrong trade anyway.
    if layout == Layout::RowMajor && let Some(d) = device.filter(|d| d.is_gpu()) {
        match crate::device::try_run(d, k, inputs) {
            Ok(a) => return (Some(a), Placement::Gpu),
            Err(why) => placement = Placement::Cpu(why),
        }
    }
    let r = run(k, inputs).map(|a| a.with_layout(layout));
    if r.is_none() {
        note_fallback();
    }
    (r, placement)
}

/// The layout a fused kernel's answer keeps, or None when its inputs
/// disagree and the caller must materialise the rows of each.
fn kernel_layout(inputs: &[Array]) -> Option<Layout> {
    let mut found: Option<Layout> = None;
    for a in inputs {
        // A scalar is one value repeated into every block: it has no layout
        // to agree or disagree with.
        if a.rank() == 0 {
            continue;
        }
        match found {
            None => found = Some(a.layout()),
            Some(l) if l == a.layout() => {}
            Some(_) => return None,
        }
    }
    Some(found.unwrap_or_default())
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

    #[test]
    fn a_value_the_chain_reads_twice_becomes_a_let() {
        // What `d =. {x} + 1` then `+/ d * d` comes to once the name has
        // moved into the kernel: the sum is computed once per block.
        let p = program("+/ ({x} + 1) * ({x} + 1)");
        let Expr::Fused { kernel, .. } = &p.stmts[0] else { panic!("not fused") };
        assert_eq!(
            kernel.code(),
            [
                Instr::Load(1),
                Instr::Load(0),
                Instr::Dyad(ScalarDyad::Add),
                Instr::Store(0),
                Instr::Let(0),
                Instr::Let(0),
                Instr::Dyad(ScalarDyad::Mul),
            ]
        );
        // One buffer for the let, one for the product it feeds.
        assert_eq!(kernel.slots, 2);
    }

    #[test]
    fn a_named_value_moves_into_the_sentence_that_reads_it() {
        let p = program("d =. {x} + 1\n+/ d * d");
        assert!(is_inlined(&p));
        // Three sentences: what the program was, the check that stands
        // where the assignment stood, and the sum, which is now the chain
        // of the test above.
        assert_eq!(p.stmts.len(), 3);
        let Expr::Fused { kernel, .. } = &p.stmts[2] else { panic!("the sum did not fuse") };
        assert!(kernel.code().contains(&Instr::Store(0)));
        assert_eq!(unfused(&p).stmts.len(), 2);
    }
}
