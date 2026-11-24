//! A verb lowered to a scalar body: the [`Prog`] both code generators
//! compile.
//!
//! The lowering follows the interpreter's own rules value by value. Every
//! value carries the element types it may have at run time — boolean,
//! integer or float, as the interpreter would type the atom — and the
//! lowering refuses wherever the interpreter's answer would depend on more
//! than those types and the bits of the operands. Integers and booleans are
//! held as the floats they equal: an integer is checked to stay below
//! [`WHOLE_BOUND`] in magnitude, where every arithmetic step on them is
//! exact in double precision and a tolerant comparison of two of them is
//! the exact one, and a zero integer is always the positive zero.

use std::collections::HashMap;

use super::{Bin, Cmp, Ins, Out, Prog, Un, Var, CAP, WHOLE_BOUND};
use crate::array::{Array, Data};
use crate::ir::{Control, ExplicitDef, Expr, Scope};
use crate::verb::{DyadOp, MonadOp, Power, ScalarDyad, ScalarMonad, Tol, Verb, RANK_INF};

/// The largest body lowered, in instructions.
const MAX_INS: usize = 4096;

/// How deeply verbs, definitions and blocks may nest inside one body.
const MAX_DEPTH: usize = 64;

/// The type of an atom, ordered as the interpreter promotes them when it
/// frames cells: a boolean below an integer below a float.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Kind {
    Bool = 0,
    Int = 1,
    Float = 2,
}

impl Kind {
    pub(super) fn code(self) -> f64 {
        self as u8 as f64
    }

    const ALL: [Kind; 3] = [Kind::Bool, Kind::Int, Kind::Float];
}

/// The set of types a value may have at run time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Kinds(u8);

impl Kinds {
    const NONE: Kinds = Kinds(0);

    pub(super) fn one(k: Kind) -> Kinds {
        Kinds(1 << k as u8)
    }

    fn has(self, k: Kind) -> bool {
        self.0 & (1 << k as u8) != 0
    }

    fn iter(self) -> impl Iterator<Item = Kind> {
        Kind::ALL.into_iter().filter(move |&k| self.has(k))
    }

    pub(super) fn single(self) -> Option<Kind> {
        let mut it = self.iter();
        match (it.next(), it.next()) {
            (Some(k), None) => Some(k),
            _ => None,
        }
    }

    fn union(self, o: Kinds) -> Kinds {
        Kinds(self.0 | o.0)
    }
}

/// A lowered atom: the variable holding it, the types it may have, and,
/// where that is more than one, the variable holding its type's code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Val {
    pub(super) v: Var,
    pub(super) k: Kinds,
    pub(super) kv: Option<Var>,
}

/// What a sub-expression stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Term {
    Atom(Val),
    /// The item of a fold, a list of this many floats, read as arguments
    /// 1, 2, ….
    Row(u32),
}

/// Local names and what they are bound to.
type Env = HashMap<String, Term>;

/// What a block of sentences leaves in hand.
#[derive(Clone, Copy, Debug)]
enum Got {
    /// A value on every path.
    Value(Term),
    /// No value on some path: the language's empty result, or nothing.
    Indefinite,
}

/// A key for sharing one computation between its uses inside one block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Un(Un, Val),
    Bin(Bin, Val, Val),
    Cmp(Cmp, Val, Val),
}

/// The result type of a monad over one argument type, None where the
/// interpreter's answer is not one this lowering reproduces.
fn un_kind(op: Un, k: Kind) -> Option<Kind> {
    use Kind::*;
    Some(match op {
        Un::Neg | Un::Inc | Un::Dec | Un::Double => {
            if k == Float {
                Float
            } else {
                Int
            }
        }
        Un::Abs | Un::Square | Un::OneMinus => k,
        // The root of 0 or 1 is itself, and stays boolean.
        Un::Sqrt => {
            if k == Bool {
                Bool
            } else {
                Float
            }
        }
        Un::Halve | Un::Recip | Un::Exp | Un::Ln => Float,
        Un::Floor | Un::Ceil => {
            if k == Float {
                Int
            } else {
                k
            }
        }
    })
}

/// The result type of a dyad over two argument types.
fn bin_kind(op: Bin, a: Kind, b: Kind) -> Option<Kind> {
    use Kind::*;
    let float = a == Float || b == Float;
    Some(match op {
        Bin::Add | Bin::Sub => {
            if float {
                Float
            } else {
                Int
            }
        }
        // `*` `|` `<.` `>.` keep two booleans boolean. A pair of atoms is
        // a pass of one pair, whose extremum settles a tie of the two
        // zeros by the right operand whatever the types, as the compare
        // and select does.
        Bin::Mul | Bin::Residue | Bin::Min | Bin::Max => {
            if float {
                Float
            } else if a == Bool && b == Bool {
                Bool
            } else {
                Int
            }
        }
        Bin::Div => Float,
        // `*.` and `+.` of two booleans are the logical and and or; of
        // anything else, the least common multiple and the greatest common
        // divisor, which are not lowered.
        Bin::And | Bin::Or => {
            if a == Bool && b == Bool {
                Bool
            } else {
                return None;
            }
        }
    })
}

/// Whether an integer result of `op` can be the negative zero, which the
/// interpreter's integers do not have.
fn may_be_negative_zero(op: Option<Un>, bin: Option<Bin>) -> bool {
    matches!(op, Some(Un::Neg | Un::Floor | Un::Ceil)) || matches!(bin, Some(Bin::Mul | Bin::Residue))
}

pub(crate) struct Lower<'a> {
    /// Arguments and constants: run before everything else, so defined on
    /// every path.
    pro: Vec<Ins>,
    /// The block being written.
    code: Vec<Ins>,
    vars: u32,
    size: usize,
    consts: HashMap<u64, Var>,
    args: HashMap<u32, Var>,
    memo: HashMap<Key, Val>,
    resolve: &'a dyn Fn(&str) -> Option<Verb>,
    depth: usize,
    tol: Tol,
}

impl<'a> Lower<'a> {
    pub(super) fn new(resolve: &'a dyn Fn(&str) -> Option<Verb>, tol: Tol) -> Lower<'a> {
        Lower {
            pro: Vec::new(),
            code: Vec::new(),
            vars: 0,
            size: 0,
            consts: HashMap::new(),
            args: HashMap::new(),
            memo: HashMap::new(),
            resolve,
            depth: 0,
            tol,
        }
    }

    /// The program computing `root`, whose type is the answer's.
    pub(super) fn finish(mut self, root: Val) -> Option<Prog> {
        let out = match (root.k.single(), root.kv) {
            (Some(k), _) => Out::Static(k),
            (None, Some(kv)) => Out::Dyn(kv),
            (None, None) => return None,
        };
        let mut code = std::mem::take(&mut self.pro);
        code.append(&mut self.code);
        Some(Prog {
            code,
            vars: self.vars,
            result: root.v,
            out,
            ct: self.tol.ct.to_bits(),
            by_smaller: self.tol.by_smaller,
        })
    }

    fn fresh(&mut self) -> Option<Var> {
        self.size += 1;
        if self.size > MAX_INS {
            return None;
        }
        let v = self.vars;
        self.vars += 1;
        Some(v)
    }

    fn emit(&mut self, i: Ins) -> Option<()> {
        self.size += 1;
        (self.size <= MAX_INS).then(|| self.code.push(i))
    }

    fn enter(&mut self) -> Option<()> {
        self.depth += 1;
        (self.depth <= MAX_DEPTH).then_some(())
    }

    /// Argument k as a variable of the given type.
    pub(super) fn arg(&mut self, k: u32, kind: Kind) -> Option<Val> {
        let v = match self.args.get(&k) {
            Some(&v) => v,
            None => {
                let v = self.fresh()?;
                self.pro.push(Ins::Arg(v, k));
                self.args.insert(k, v);
                v
            }
        };
        Some(Val { v, k: Kinds::one(kind), kv: None })
    }

    /// The variable holding the float `x`.
    fn float_const(&mut self, x: f64) -> Option<Var> {
        let bits = x.to_bits();
        if let Some(&v) = self.consts.get(&bits) {
            return Some(v);
        }
        let v = self.fresh()?;
        self.pro.push(Ins::Const(v, bits));
        self.consts.insert(bits, v);
        Some(v)
    }

    fn known(&mut self, x: f64, k: Kind) -> Option<Val> {
        Some(Val { v: self.float_const(x)?, k: Kinds::one(k), kv: None })
    }

    /// A numeric atom written in the source.
    fn constant(&mut self, a: &Array) -> Option<Term> {
        if a.rank() != 0 || a.is_sparse() {
            return None;
        }
        let (x, k) = match &a.data {
            Data::Bool(b) => (f64::from(*b.first()?), Kind::Bool),
            Data::I64(b) => {
                let n = *b.first()?;
                if n.unsigned_abs() >= WHOLE_BOUND as u64 {
                    return None;
                }
                (n as f64, Kind::Int)
            }
            Data::F64(b) => {
                let x = *b.first()?;
                if !x.is_finite() {
                    return None;
                }
                (x, Kind::Float)
            }
            _ => return None,
        };
        Some(Term::Atom(self.known(x, k)?))
    }

    /// The checks a fresh value of the types `k` needs, its type's code
    /// in `kv` where that is known only at run time: a float must be
    /// finite, and an integer within bounds and never the negative zero.
    fn settle(&mut self, v: Var, k: Kinds, kv: Option<Var>, neg_zero: bool) -> Option<Var> {
        if !k.has(Kind::Int) {
            if k.has(Kind::Float) {
                self.emit(Ins::Finite(v))?;
            }
            return Some(v);
        }
        // Adding a positive zero leaves every value but the negative zero
        // alone, and makes that one positive.
        let normal = if neg_zero {
            let z = self.float_const(0.0)?;
            let n = self.fresh()?;
            self.emit(Ins::Bin(n, Bin::Add, v, z))?;
            n
        } else {
            v
        };
        if !k.has(Kind::Float) {
            self.emit(Ins::Whole(normal))?;
            return Some(normal);
        }
        // An integer or a float: the integer's checks where the type code
        // says it is one, the float's otherwise.
        let kv = kv?;
        let int = self.float_const(Kind::Int.code())?;
        let is_int = self.fresh()?;
        self.emit(Ins::Cmp(is_int, Cmp::Eq, kv, int))?;
        let v = if neg_zero {
            let s = self.fresh()?;
            self.emit(Ins::Select(s, is_int, normal, v))?;
            s
        } else {
            v
        };
        self.emit(Ins::Finite(v))?;
        let z = self.float_const(0.0)?;
        let m = self.fresh()?;
        self.emit(Ins::Select(m, is_int, v, z))?;
        self.emit(Ins::Whole(m))?;
        Some(v)
    }

    /// The variable holding the code of a value's type.
    fn kind_var(&mut self, a: Val) -> Option<Var> {
        match (a.kv, a.k.single()) {
            (Some(kv), _) => Some(kv),
            (None, Some(k)) => self.float_const(k.code()),
            (None, None) => None,
        }
    }

    /// A type code computed from others: `op` over type codes, whose
    /// order is the order types promote in.
    fn kind_op(&mut self, op: Bin, a: Var, b: Var) -> Option<Var> {
        let v = self.fresh()?;
        self.emit(Ins::Bin(v, op, a, b))?;
        Some(v)
    }

    fn un(&mut self, op: Un, a: Term) -> Option<Term> {
        let Term::Atom(a) = a else { return None };
        // APL's floor and ceiling read the tolerance by rules of their own.
        if matches!(op, Un::Floor | Un::Ceil) && !self.tol.is_j() {
            return None;
        }
        let mut k = Kinds::NONE;
        for ka in a.k.iter() {
            k = k.union(Kinds::one(un_kind(op, ka)?));
        }
        let key = Key::Un(op, a);
        if let Some(&v) = self.memo.get(&key) {
            return Some(Term::Atom(v));
        }
        let v = self.fresh()?;
        self.emit(Ins::Un(v, op, a.v))?;
        // Where the type is not known until run time, it is computed the
        // way [`un_kind`] reads it.
        let kv = if k.single().is_none() {
            let ka = self.kind_var(a)?;
            let int = self.float_const(Kind::Int.code())?;
            Some(match op {
                Un::Neg | Un::Inc | Un::Dec | Un::Double => self.kind_op(Bin::Max, ka, int)?,
                Un::Abs | Un::Square | Un::OneMinus => ka,
                Un::Floor | Un::Ceil => self.kind_op(Bin::Min, ka, int)?,
                Un::Sqrt => {
                    let two = self.float_const(2.0)?;
                    let m = self.kind_op(Bin::Min, ka, int)?;
                    self.kind_op(Bin::Mul, m, two)?
                }
                _ => return None,
            })
        } else {
            None
        };
        let v = self.settle(v, k, kv, may_be_negative_zero(Some(op), None))?;
        let val = Val { v, k, kv };
        self.memo.insert(key, val);
        Some(Term::Atom(val))
    }

    fn bin(&mut self, op: Bin, a: Term, b: Term) -> Option<Term> {
        let (Term::Atom(a), Term::Atom(b)) = (a, b) else { return None };
        // APL's extrema settle a tie and a NaN by the left operand, and its
        // residue reads the tolerance by a rule of its own.
        if !matches!(op, Bin::Add | Bin::Sub | Bin::Mul | Bin::Div) && !self.tol.is_j() {
            return None;
        }
        let mut k = Kinds::NONE;
        for ka in a.k.iter() {
            for kb in b.k.iter() {
                k = k.union(Kinds::one(bin_kind(op, ka, kb)?));
            }
        }
        let key = Key::Bin(op, a, b);
        if let Some(&v) = self.memo.get(&key) {
            return Some(Term::Atom(v));
        }
        let v = self.fresh()?;
        self.emit(Ins::Bin(v, op, a.v, b.v))?;
        // Where the type is not known until run time, it is computed the
        // way [`bin_kind`] reads it: the larger type, and at least an
        // integer for a sum or a difference.
        let kv = if k.single().is_none() {
            let (ka, kb) = (self.kind_var(a)?, self.kind_var(b)?);
            let top = self.kind_op(Bin::Max, ka, kb)?;
            Some(match op {
                Bin::Mul | Bin::Residue | Bin::Min | Bin::Max => top,
                Bin::Add | Bin::Sub => {
                    let int = self.float_const(Kind::Int.code())?;
                    self.kind_op(Bin::Max, top, int)?
                }
                _ => return None,
            })
        } else {
            None
        };
        let v = self.settle(v, k, kv, may_be_negative_zero(None, Some(op)))?;
        let val = Val { v, k, kv };
        self.memo.insert(key, val);
        Some(Term::Atom(val))
    }

    fn cmp(&mut self, op: Cmp, a: Term, b: Term) -> Option<Term> {
        let (Term::Atom(a), Term::Atom(b)) = (a, b) else { return None };
        if !self.tol.is_j() {
            return None;
        }
        let key = Key::Cmp(op, a, b);
        if let Some(&v) = self.memo.get(&key) {
            return Some(Term::Atom(v));
        }
        let v = self.fresh()?;
        self.emit(Ins::Cmp(v, op, a.v, b.v))?;
        let val = Val { v, k: Kinds::one(Kind::Bool), kv: None };
        self.memo.insert(key, val);
        Some(Term::Atom(val))
    }

    /// `u y`.
    pub(super) fn monad(&mut self, v: &Verb, y: Term) -> Option<Term> {
        self.enter()?;
        let r = self.monad_inner(v, y);
        self.depth -= 1;
        r
    }

    /// `x u y`.
    pub(super) fn dyad(&mut self, v: &Verb, x: Term, y: Term) -> Option<Term> {
        self.enter()?;
        let r = self.dyad_inner(v, x, y);
        self.depth -= 1;
        r
    }

    fn monad_inner(&mut self, v: &Verb, y: Term) -> Option<Term> {
        match v {
            Verb::Prim(p) => match p.monad {
                MonadOp::Same => Some(y),
                MonadOp::Head => match y {
                    Term::Row(0) => None,
                    Term::Row(_) => Some(Term::Atom(self.arg(1, Kind::Float)?)),
                    a => Some(a),
                },
                MonadOp::Tail => match y {
                    Term::Row(0) => None,
                    Term::Row(k) => Some(Term::Atom(self.arg(k, Kind::Float)?)),
                    a => Some(a),
                },
                MonadOp::Scalar(op) => {
                    use ScalarMonad as M;
                    let u = match op {
                        // The identity on reals.
                        M::Conj => return matches!(y, Term::Atom(_)).then_some(y),
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
                        M::Floor => Un::Floor,
                        M::Ceil => Un::Ceil,
                        _ => return None,
                    };
                    self.un(u, y)
                }
                _ => None,
            },
            Verb::Constant(m) => self.constant(m),
            Verb::Rank(u, r) => {
                if matches!(y, Term::Row(_)) && !whole(r[0]) {
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
            Verb::Agenda(vs, w) => {
                let sel = self.monad(w, y)?;
                self.agenda(vs, sel, None, y)
            }
            Verb::PowerN(u, Power::Times(n)) => self.power(u, *n, None, y),
            Verb::Named(n) if self.depth == 1 => {
                let u = (self.resolve)(n)?;
                self.monad(&u, y)
            }
            _ => None,
        }
    }

    fn dyad_inner(&mut self, v: &Verb, x: Term, y: Term) -> Option<Term> {
        match v {
            Verb::Prim(p) => match p.dyad {
                DyadOp::Left => Some(x),
                DyadOp::Right => Some(y),
                DyadOp::From => {
                    let (Term::Atom(i), Term::Row(k)) = (x, y) else { return None };
                    let f = self.const_value(i.v)?;
                    if f.fract() != 0.0 {
                        return None;
                    }
                    let k = i64::from(k);
                    let i = f as i64;
                    let at = if i < 0 { i + k } else { i };
                    if !(0..k).contains(&at) {
                        return None;
                    }
                    Some(Term::Atom(self.arg(at as u32 + 1, Kind::Float)?))
                }
                DyadOp::Scalar(op) => {
                    use ScalarDyad as D;
                    let c = match op {
                        D::Eq => Some(Cmp::Eq),
                        D::Ne => Some(Cmp::Ne),
                        D::Lt => Some(Cmp::Lt),
                        D::Le => Some(Cmp::Le),
                        D::Gt => Some(Cmp::Gt),
                        D::Ge => Some(Cmp::Ge),
                        _ => None,
                    };
                    if let Some(c) = c {
                        return self.cmp(c, x, y);
                    }
                    let b = match op {
                        D::Add => Bin::Add,
                        D::Sub => Bin::Sub,
                        D::Mul => Bin::Mul,
                        D::DivJ => Bin::Div,
                        D::Min => Bin::Min,
                        D::Max => Bin::Max,
                        D::Residue => Bin::Residue,
                        D::Lcm => Bin::And,
                        D::Gcd => Bin::Or,
                        _ => return None,
                    };
                    self.bin(b, x, y)
                }
                _ => None,
            },
            Verb::Constant(m) => self.constant(m),
            Verb::Rank(u, r) => {
                if matches!(x, Term::Row(_)) && !whole(r[1])
                    || matches!(y, Term::Row(_)) && !whole(r[2])
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
            Verb::Agenda(vs, w) => {
                let sel = self.dyad(w, x, y)?;
                self.agenda(vs, sel, Some(x), y)
            }
            Verb::PowerN(u, Power::Times(n)) => self.power(u, *n, Some(x), y),
            Verb::Named(n) if self.depth == 1 => {
                let u = (self.resolve)(n)?;
                self.dyad(&u, x, y)
            }
            _ => None,
        }
    }

    /// The value of a variable that holds a constant, if it does.
    fn const_value(&self, v: Var) -> Option<f64> {
        self.consts.iter().find(|&(_, &c)| c == v).map(|(&bits, _)| f64::from_bits(bits))
    }

    /// Lower `f` as a block of its own: its instructions are returned
    /// rather than written, and what it learnt about shared computations is
    /// forgotten afterwards, since the block may not run.
    fn nested<R>(&mut self, f: impl FnOnce(&mut Self) -> Option<R>) -> Option<(Vec<Ins>, R)> {
        let saved_code = std::mem::take(&mut self.code);
        let saved_memo = self.memo.clone();
        let r = f(self);
        let code = std::mem::replace(&mut self.code, saved_code);
        self.memo = saved_memo;
        Some((code, r?))
    }

    /// One value standing for whichever of `vals` the path taken made:
    /// each block gets the copies into it appended. None where a value is
    /// a row, which only stands for itself.
    fn join(&mut self, blocks: &mut [Vec<Ins>], vals: &[Val]) -> Option<Val> {
        let first = *vals.first()?;
        if vals.iter().all(|v| *v == first) {
            return Some(first);
        }
        let m = self.fresh()?;
        let k = vals.iter().fold(vals[0].k, |k, v| k.union(v.k));
        let kv = if k.single().is_none() { Some(self.fresh()?) } else { None };
        for (code, val) in blocks.iter_mut().zip(vals) {
            code.push(Ins::Copy(m, val.v));
            if let Some(kv) = kv {
                let src = match (val.kv, val.k.single()) {
                    (Some(src), _) => src,
                    (None, Some(one)) => self.float_const(one.code())?,
                    (None, None) => return None,
                };
                code.push(Ins::Copy(kv, src));
            }
            self.size += 2;
        }
        (self.size <= MAX_INS).then_some(Val { v: m, k, kv })
    }

    /// The environments after several blocks, one of which ran, joined:
    /// a name bound alike on every path keeps its binding, one bound to
    /// different atoms gets a variable of its own, and the rest are
    /// dropped.
    fn join_envs(&mut self, blocks: &mut [Vec<Ins>], envs: &[Env]) -> Option<Env> {
        let mut out = Env::new();
        let first = envs.first()?;
        for (name, t) in first {
            let all: Option<Vec<Term>> = envs.iter().map(|e| e.get(name).copied()).collect();
            let Some(all) = all else { continue };
            if all.iter().all(|u| u == t) {
                out.insert(name.clone(), *t);
                continue;
            }
            let atoms: Option<Vec<Val>> = all
                .iter()
                .map(|u| match u {
                    Term::Atom(v) => Some(*v),
                    Term::Row(_) => None,
                })
                .collect();
            if let Some(atoms) = atoms {
                let v = self.join(blocks, &atoms)?;
                out.insert(name.clone(), Term::Atom(v));
            }
        }
        Some(out)
    }

    /// `u@.v` applied, its selector already computed: every verb of the
    /// gerund is lowered into an arm of its own.
    fn agenda(&mut self, vs: &[Verb], sel: Term, x: Option<Term>, y: Term) -> Option<Term> {
        let Term::Atom(sel) = sel else { return None };
        if vs.is_empty() {
            return None;
        }
        let mut blocks = Vec::with_capacity(vs.len());
        let mut vals = Vec::with_capacity(vs.len());
        for v in vs {
            let (code, t) = self.nested(|l| match x {
                Some(x) => l.dyad(v, x, y),
                None => l.monad(v, y),
            })?;
            let Term::Atom(val) = t else { return None };
            blocks.push(code);
            vals.push(val);
        }
        let joined = self.join(&mut blocks, &vals)?;
        self.emit(Ins::Switch(sel.v, blocks))?;
        Some(Term::Atom(joined))
    }

    /// `u^:n` applied: a loop of n applications.
    fn power(&mut self, u: &Verb, n: u64, x: Option<Term>, y: Term) -> Option<Term> {
        match n {
            0 => return Some(y),
            1 => {
                return match x {
                    Some(x) => self.dyad(u, x, y),
                    None => self.monad(u, y),
                };
            }
            _ => {}
        }
        if n > CAP {
            return None;
        }
        let Term::Atom(_) = y else { return None };
        const ACC: &str = "\0power";
        let mut env = Env::new();
        env.insert(ACC.to_string(), y);
        let count = self.known(n as f64, Kind::Int)?;
        let env = self.counted(&env, count, None, |l, env| {
            let acc = *env.get(ACC)?;
            let next = match x {
                Some(x) => l.dyad(u, x, acc)?,
                None => l.monad(u, acc)?,
            };
            env.insert(ACC.to_string(), next);
            Some(())
        })?;
        env.get(ACC).copied()
    }

    /// An explicit definition whose body is sentences and the control
    /// structures `if.`, `while.` and `for.`.
    fn explicit(&mut self, d: &ExplicitDef, x: Option<Term>, y: Term) -> Option<Term> {
        if !d.pure
            || d.result.is_some()
            || d.axis.is_some()
            || !d.labels.is_empty()
            || !d.enclosing.is_empty()
            || d.body.is_empty()
        {
            return None;
        }
        let mut env = Env::new();
        env.insert(d.right.clone(), y);
        match (&d.left, x) {
            (Some(l), Some(x)) => {
                env.insert(l.clone(), x);
            }
            (None, None) => {}
            _ => return None,
        }
        match self.block(&d.body, &mut env)? {
            Got::Value(t) => Some(t),
            Got::Indefinite => None,
        }
    }

    /// A block of sentences: what it leaves in hand.
    fn block(&mut self, stmts: &[Expr], env: &mut Env) -> Option<Got> {
        self.enter()?;
        let mut got = Got::Indefinite;
        for s in stmts {
            got = self.statement(s, env)?;
        }
        self.depth -= 1;
        Some(got)
    }

    fn statement(&mut self, s: &Expr, env: &mut Env) -> Option<Got> {
        match s {
            Expr::Assign { name, value, scope: Scope::Local, .. } => {
                let v = self.expr(value, env)?;
                env.insert(name.clone(), v);
                Some(Got::Value(v))
            }
            Expr::Control(c, _) => self.control(c, env),
            Expr::Assign { .. } => None,
            e => Some(Got::Value(self.expr(e, env)?)),
        }
    }

    fn control(&mut self, c: &Control, env: &mut Env) -> Option<Got> {
        match c {
            Control::If { arms, otherwise } => self.if_arms(arms, otherwise.as_deref(), env),
            Control::While { test, body, body_first: false, until: false } => {
                self.while_loop(test, body, env)?;
                Some(Got::Indefinite)
            }
            Control::For { names, source, body } if names.len() <= 1 => {
                self.for_loop(names.first(), source, body, env)?;
                Some(Got::Indefinite)
            }
            _ => None,
        }
    }

    /// A test block's condition: J's truth of one atom, nonzero.
    fn condition(&mut self, test: &[Expr], env: &mut Env) -> Option<Var> {
        let before = env.clone();
        let Got::Value(Term::Atom(t)) = self.block(test, env)? else { return None };
        // A test that assigns would make the names it binds depend on how
        // many tests ran.
        if *env != before {
            return None;
        }
        Some(t.v)
    }

    /// `if. T do. B elseif. … else. B end.`: each test in turn, the first
    /// that holds choosing its body.
    fn if_arms(
        &mut self,
        arms: &[crate::ir::Branch],
        otherwise: Option<&[Expr]>,
        env: &mut Env,
    ) -> Option<Got> {
        let Some((arm, rest)) = arms.split_first() else {
            return match otherwise {
                Some(body) => self.block(body, env),
                None => Some(Got::Indefinite),
            };
        };
        if arm.fall_through || arm.list {
            return None;
        }
        let cond = self.condition(arm.test.as_deref()?, env)?;
        let mut then_env = env.clone();
        let (then_code, then_got) = self.nested(|l| l.block(&arm.body, &mut then_env))?;
        let mut else_env = env.clone();
        let (else_code, else_got) = self.nested(|l| l.if_arms(rest, otherwise, &mut else_env))?;
        let mut blocks = [then_code, else_code];
        let got = match (then_got, else_got) {
            (Got::Value(Term::Atom(a)), Got::Value(Term::Atom(b))) => {
                Got::Value(Term::Atom(self.join(&mut blocks, &[a, b])?))
            }
            (Got::Value(a), Got::Value(b)) if a == b => Got::Value(a),
            _ => Got::Indefinite,
        };
        *env = self.join_envs(&mut blocks, &[then_env, else_env])?;
        let [then_code, else_code] = blocks;
        self.emit(Ins::If(cond, then_code, else_code))?;
        Some(got)
    }

    /// The loop-carried names of `env`: every atom gets a variable of its
    /// own for the loop, of the types `kinds` allows where it says.
    fn carried(&mut self, env: &Env, kinds: &HashMap<String, Kinds>) -> Option<(Env, Vec<Ins>)> {
        let mut inner = Env::new();
        let mut entry = Vec::new();
        for (name, t) in env {
            let Term::Atom(v) = t else {
                inner.insert(name.clone(), *t);
                continue;
            };
            let k = kinds.get(name).copied().unwrap_or(v.k).union(v.k);
            let m = self.fresh()?;
            let kv = if k.single().is_none() { Some(self.fresh()?) } else { None };
            entry.push(Ins::Copy(m, v.v));
            if let Some(kv) = kv {
                let src = match (v.kv, v.k.single()) {
                    (Some(src), _) => src,
                    (None, Some(one)) => self.float_const(one.code())?,
                    (None, None) => return None,
                };
                entry.push(Ins::Copy(kv, src));
            }
            inner.insert(name.clone(), Term::Atom(Val { v: m, k, kv }));
        }
        self.size += entry.len();
        Some((inner, entry))
    }

    /// Lower a loop whose iteration is `iterate`, run with the carried
    /// names bound as `carried` makes them: until the types every carried
    /// name may have stop growing. Returns the code that sets the carried
    /// variables up, the iteration's code with the copies back appended,
    /// what `iterate` answered, and the environment after the loop.
    #[allow(clippy::type_complexity)]
    fn fixed_point<R>(
        &mut self,
        env: &Env,
        mut iterate: impl FnMut(&mut Self, &mut Env) -> Option<R>,
    ) -> Option<(Vec<Ins>, Vec<Ins>, R, Env)> {
        let mut kinds: HashMap<String, Kinds> = HashMap::new();
        for _ in 0..4 {
            let (inner, entry) = self.carried(env, &kinds)?;
            let mut after = inner.clone();
            let (mut code, r) = self.nested(|l| iterate(l, &mut after))?;
            let mut grew = false;
            for (name, t) in &inner {
                let Term::Atom(m) = t else { continue };
                match after.get(name) {
                    Some(Term::Atom(v)) => {
                        if v.k.union(m.k) != m.k {
                            kinds.insert(name.clone(), v.k.union(m.k));
                            grew = true;
                        }
                    }
                    // A carried name rebound to a row is past this lowering.
                    Some(Term::Row(_)) | None => return None,
                }
            }
            if grew {
                continue;
            }
            // The copies back into the carried variables, as one parallel
            // copy: a source that is itself a carried variable — `a` and
            // `b` swapped — is read into a temporary before any is written.
            let carried_vars: Vec<Var> = inner
                .values()
                .filter_map(|t| match t {
                    Term::Atom(m) => Some(m),
                    Term::Row(_) => None,
                })
                .flat_map(|m| [Some(m.v), m.kv].into_iter().flatten())
                .collect();
            let mut copies: Vec<(Var, Var)> = Vec::new();
            let mut outer = Env::new();
            for (name, t) in &inner {
                let Term::Atom(m) = t else {
                    outer.insert(name.clone(), *t);
                    continue;
                };
                let Some(Term::Atom(v)) = after.get(name) else { return None };
                copies.push((m.v, v.v));
                if let Some(kv) = m.kv {
                    let src = match (v.kv, v.k.single()) {
                        (Some(src), _) => src,
                        (None, Some(one)) => self.float_const(one.code())?,
                        (None, None) => return None,
                    };
                    copies.push((kv, src));
                }
                outer.insert(name.clone(), *t);
            }
            copies.retain(|(d, s)| d != s);
            let mut staged = Vec::with_capacity(copies.len());
            for (d, s) in copies {
                if carried_vars.contains(&s) {
                    let tmp = self.fresh()?;
                    code.push(Ins::Copy(tmp, s));
                    staged.push((d, tmp));
                } else {
                    staged.push((d, s));
                }
            }
            for (d, s) in staged {
                code.push(Ins::Copy(d, s));
            }
            self.size += inner.len() * 4;
            if self.size > MAX_INS {
                return None;
            }
            return Some((entry, code, r, outer));
        }
        None
    }

    /// `while. T do. B end.`.
    fn while_loop(&mut self, test: &[Expr], body: &[Expr], env: &mut Env) -> Option<()> {
        let (entry, code, (test_code, cond), outer) = self.fixed_point(env, |l, env| {
            let (test_code, cond) = l.nested(|l| l.condition(test, env))?;
            // The test's computations are shared with the body, which it
            // dominates.
            l.code.extend(test_code.iter().cloned());
            l.block(body, env)?;
            Some((test_code, cond))
        })?;
        // The test was written at the head of the iteration's code; it is
        // split off again for the loop's own shape.
        let body_code = code[test_code.len()..].to_vec();
        self.code.extend(entry);
        self.emit(Ins::While(test_code, cond, body_code))?;
        *env = outer;
        Some(())
    }

    /// `for. i. n do. B end.` and `for_name. i. n do. B end.`.
    fn for_loop(
        &mut self,
        name: Option<&String>,
        source: &Expr,
        body: &[Expr],
        env: &mut Env,
    ) -> Option<()> {
        let Expr::Monad { verb: Verb::Prim(p), y, .. } = source else { return None };
        if p.monad != MonadOp::IotaJ {
            return None;
        }
        let Term::Atom(count) = self.expr(y, env)? else { return None };
        let outer = self.counted(env, count, name, |l, env| l.block(body, env).map(|_| ()))?;
        *env = outer;
        Some(())
    }

    /// A loop of `count` iterations, the iteration index bound to `name`
    /// and `name_index` where there is a name. The names the loop binds
    /// for the first time are not visible after it, since it may not run.
    fn counted(
        &mut self,
        env: &Env,
        count: Val,
        name: Option<&String>,
        mut iterate: impl FnMut(&mut Self, &mut Env) -> Option<()>,
    ) -> Option<Env> {
        let index = self.fresh()?;
        let iv = Val { v: index, k: Kinds::one(Kind::Int), kv: None };
        let (entry, code, (), outer) = self.fixed_point(env, |l, env| {
            if let Some(n) = name {
                env.insert(n.clone(), Term::Atom(iv));
                env.insert(format!("{n}_index"), Term::Atom(iv));
            }
            iterate(l, env)
        })?;
        self.code.extend(entry);
        self.emit(Ins::Repeat { count: count.v, index, body: code })?;
        Some(outer)
    }

    fn expr(&mut self, e: &Expr, env: &mut Env) -> Option<Term> {
        self.enter()?;
        let r = match e {
            Expr::Const(a, _) => self.constant(a),
            Expr::Name(n, _) => env.get(n).copied(),
            Expr::Monad { verb, y, .. } => {
                let y = self.expr(y, env)?;
                self.monad(verb, y)
            }
            Expr::Dyad { verb, x, y, .. } => {
                let y = self.expr(y, env)?;
                let x = self.expr(x, env)?;
                self.dyad(verb, x, y)
            }
            Expr::Assign { name, value, scope: Scope::Local, .. } => {
                let v = self.expr(value, env)?;
                env.insert(name.clone(), v);
                Some(v)
            }
            Expr::Fused { orig, .. } => self.expr(orig, env),
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
