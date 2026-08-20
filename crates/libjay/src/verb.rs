//! Verbs and the rank machinery: the language-agnostic execution core.
//!
//! A `Verb` is a semantic object — a primitive or a combination of verbs —
//! applied monadically or dyadically to arrays. Frontends lower J/APL syntax
//! to `Verb` trees; nothing in here knows any surface syntax.

use std::collections::HashSet;

use crate::array::{Array, Data};
use crate::dtype::DType;
use crate::error::{Error, ErrorKind, Result, Span};
use crate::fmt::FmtOpts;
use crate::par;

/// Infinite rank (applies to the argument as a whole).
pub const RANK_INF: i64 = i64::MAX;

/// How dyadic frames must agree. A property of the source language,
/// fixed per compiled program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agreement {
    /// J: the shorter frame must be a prefix of the longer.
    LeadingPrefix,
    /// APL scalar conformability: equal frames, or one of them empty.
    ExactOrScalar,
}

/// The effect-free half of the execution context. Copyable, so a path that
/// runs cells on other threads can carry it there; the output sink cannot
/// go along, which is what keeps those paths pure by construction.
#[derive(Clone, Copy, Debug)]
pub struct EvalCfg {
    pub agreement: Agreement,
    pub fmt: FmtOpts,
}

impl EvalCfg {
    /// Run `f` with a context whose sink is never reached. Only a verb that
    /// [`Verb::is_pure`] accepted is given one of these.
    fn pure<R>(self, f: impl FnOnce(&mut Ctx<'_>) -> R) -> R {
        let mut sink = |_: &str| debug_assert!(false, "a pure verb wrote to the output sink");
        f(&mut Ctx { cfg: self, out: &mut sink })
    }
}

/// Execution context threaded through evaluation.
pub struct Ctx<'a> {
    pub cfg: EvalCfg,
    /// Sink for explicit output (`echo`, `⎕←`). stdout by default per the
    /// sandbox contract; the host may redirect.
    pub out: &'a mut dyn FnMut(&str),
}

/// Elementwise monadic operations (cell rank 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarMonad {
    /// Identity on reals (J `+`, APL `+`).
    Conj,
    Neg,
    Signum,
    Recip,
    Sqrt,
    Exp,
    Abs,
    Floor,
    Ceil,
    /// APL `~`: logical negation; the argument must be 0 or 1.
    Not,
    /// J `-.`: `1 - y` on any number (a superset of logical negation).
    OneMinus,
    /// `y + 1` (J `>:`).
    Inc,
    /// `y - 1` (J `<:`).
    Dec,
    /// `y + y` (J `+:`).
    Double,
    /// `y % 2` (J `-:`); always float.
    Halve,
    /// `y * y` (J `*:`).
    Square,
    /// Natural logarithm (J `^.`, APL `⍟`); always float.
    Ln,
    /// `pi * y` (J/APL monadic `o.` / `○`); always float.
    Pi,
    /// `! y`: factorial, i.e. the gamma function at y+1. Always float, as in
    /// J; a negative integer is a pole and yields a signed infinity.
    Factorial,
}

/// Elementwise dyadic operations (cell ranks 0 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarDyad {
    Add,
    Sub,
    Mul,
    /// J `%`: result is float; `0 % 0` is 0, `n % 0` is signed infinity.
    DivJ,
    /// APL `÷`: result is float; `0 ÷ 0` is 1, `n ÷ 0` is a domain error.
    DivApl,
    Min,
    Max,
    Pow,
    /// `x | y`: y modulo x, sign following x; `0 | y` is y.
    Residue,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Least common multiple (J `*.`, APL `∧`); logical and on booleans.
    Lcm,
    /// Greatest common divisor (J `+.`, APL `∨`); logical or on booleans.
    Gcd,
    /// `x ^. y` / `x ⍟ y`: logarithm of y to base x; always float.
    Log,
    /// `x %: y`: the x-th root of y; always float.
    Root,
    /// `k o. y` / `k ○ y`: the circle function selected by the integer k —
    /// the trigonometric, hyperbolic and inverse families, plus the two
    /// Pythagorean forms at 0 and 4. Always float.
    Circle,
    /// `x ! y`: the number of ways to choose x things from y — J's argument
    /// order. Defined for every real pair through the gamma function.
    Binomial,
}

/// How a value is put into a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enclose {
    /// J `<`: every value becomes a box.
    Always,
    /// APL `⊂`: a simple scalar is its own enclosure, so `⊂5` is `5`.
    ExceptSimpleScalar,
}

/// Monadic meaning of a primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonadOp {
    Scalar(ScalarMonad),
    /// Shape as an integer vector (J `$`, APL `⍴`).
    ShapeOf,
    /// Item count as a scalar (J `#`, APL `≢`).
    Tally,
    /// All elements as a vector (J/APL `,`).
    Ravel,
    /// Reverse the axes (J `|:`, APL `⍉`).
    TransposeAxes,
    /// First item (J `{.`).
    Head,
    /// All but the first item (J `}.`).
    Behead,
    /// Last item (J `{:`); a cell of fills when there are no items.
    Tail,
    /// All but the last item (J `}:`).
    Curtail,
    /// Reverse the items, i.e. along the leading axis (J `|.`, APL `⊖`).
    Reverse,
    /// Distinct items in first-occurrence order (J `~.`, APL `∪`).
    Nub,
    /// The stable permutation that sorts the items ascending (J `/:`, APL `⍋`).
    GradeUp { origin: i64 },
    /// The stable permutation that sorts the items descending (J `\:`, APL `⍒`).
    GradeDown { origin: i64 },
    /// J `i.`: integers 0.. filling shape |y|, reversed along negative axes.
    IotaJ,
    /// APL `⍳` on a scalar: origin .. origin+y-1.
    IotaApl { origin: i64 },
    /// Print the formatted argument, yield an empty array (J `echo`).
    Echo,
    /// The argument itself (APL `⊢`).
    Same,
    /// J `":` / APL `⍕`: the argument as the characters that display it.
    /// A rank-0 argument gives a character vector, a rank-r one a character
    /// array of rank r (the display's lines, padded to one width).
    Format,
    /// J `#.` / APL monadic base-2 decode: a vector of digits as one number.
    DecodeBits,
    /// J `#:`: base-2 encode. The width comes from the largest magnitude in
    /// the whole argument, so the verb has infinite rank; the digits become
    /// a new trailing axis.
    EncodeBits,
    /// J `,:`: a leading axis of one (shape `2 3` becomes `1 2 3`).
    Itemize,
    /// APL `⍪`: the argument as a matrix — one row per item, that item's
    /// elements ravelled. A scalar becomes 1×1, a vector n×1.
    TableOf,
    /// J `<` / APL `⊂`: the argument as one box.
    Enclose(Enclose),
    /// J `>` / APL `⊃`: open a box (rank 0, so the frame reassembles the
    /// contents, filling where their shapes differ). A non-box opens to
    /// itself.
    Open,
    /// J `;`: raze — the items of the opened boxes, catenated.
    Raze,
    /// APL `↑`: the first element, disclosed; the type's fill when there
    /// is none.
    First,
    /// APL `∊`: enlist — every leaf element, in ravel order, as a vector.
    Enlist,
    /// APL `≡`: depth — 0 for a simple scalar, 1 for a simple array, one
    /// more than the deepest content for a box.
    Depth,
    /// Present in the language, not implemented: named feature.
    NotYet(&'static str),
    /// No monadic meaning exists for this primitive in its language.
    None,
}

/// Dyadic meaning of a primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DyadOp {
    Scalar(ScalarDyad),
    /// x $ y / x ⍴ y: reuse y's ravel cyclically to fill shape x.
    Reshape,
    /// x {. y / x ↑ y: per-axis take, negative from the end, overtake fills.
    Take,
    /// x }. y / x ↓ y: per-axis drop, negative from the end.
    Drop,
    /// y (APL `⊢`).
    Right,
    /// x (APL `⊣`).
    Left,
    /// x |. y: rotate axis k of y left by x[k] (negative rotates right).
    Rotate,
    /// Catenate along the LEADING axis (J `,`, APL `⍪`).
    AppendLeading,
    /// Catenate along the LAST axis (APL `,`).
    AppendLast,
    /// x i. y / x ⍳ y: the index in x's items of each cell of y, or
    /// `origin + #items(x)` when absent.
    IndexOf { origin: i64 },
    /// x e. y: is each cell of x, shaped like y's items, an item of y?
    MemberJ,
    /// x ∊ y: does each ELEMENT of x occur anywhere in y?
    MemberApl,
    /// x { y: each integer atom of x selects an item of y (negative from
    /// the end).
    From,
    /// x -: y / x ≡ y: same shape and same values; never a shape error.
    Match,
    /// The negation of `Match` (APL `≢`).
    NotMatch,
    /// x /: y and x \: y: x's items reordered by the grade of y's items.
    GradeSelect { down: bool },
    /// `x # y` (J), `x/y` and `x⌿y` (APL): item i of y repeated x[i] times.
    /// A one-element x applies to every item.
    Copy,
    /// `x #. y` / `x ⊥ y`: mixed-radix decode. A scalar x is the base for
    /// every digit; otherwise x and y have the same length.
    Decode,
    /// `x #: y` / `x ⊤ y`: mixed-radix encode. The digits become the LEADING
    /// axis of the result, which is what makes one operation serve J's
    /// per-atom `#:` (right rank 0) and APL's `⊤` (right rank infinite).
    Encode,
    /// `x ,: y`: the two arguments as the items of a new leading axis.
    Laminate,
    /// J `;`: link — `(<x)` before y, which is taken as it is when it is
    /// already boxed and boxed when it is not.
    Link,
    /// APL vector notation: x is one more item in front of the strand y.
    Strand,
    NotYet(&'static str),
    None,
}

/// A primitive verb: a name for diagnostics, both valence meanings, and
/// J-style ranks [monadic, dyadic-left, dyadic-right].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prim {
    pub name: &'static str,
    pub monad: MonadOp,
    pub dyad: DyadOp,
    pub ranks: [i64; 3],
}

/// Which windowed application a [`Verb::Windowed`] performs. One variant
/// covers all three because the work is the same: the verb is applied to a
/// run of consecutive items, and only the choice of runs differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    /// J `u\`: the monad applies u to every prefix, the dyad `x u\ y` to
    /// every window of x items.
    Prefix,
    /// J `u\.`: the monad applies u to every suffix; the dyad (outfix) is
    /// not implemented.
    Suffix,
    /// APL `f\` and `f⍀`: the monad is the scan, which is the prefix
    /// application. APL has no dyadic scan — `x\y` is expand, a function of
    /// its own — so the dyad reports that instead.
    Scan,
}

/// How many times a [`Verb::PowerN`] applies its verb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    /// Exactly `n` applications; 0 is the identity.
    Times(u64),
    /// Iterate until a result matches the one before it (J `u^:_`).
    Converge,
}

/// Iterations `Power::Converge` allows before giving up.
const CONVERGE_LIMIT: usize = 1 << 20;

/// A verb: primitive or derived. Language-agnostic; frontends decide which
/// combinations their syntax produces (e.g. APL `+/` becomes
/// `Rank(Reduce(+), [1,1,1])` — reduce the last axis).
#[derive(Clone, Debug)]
pub enum Verb {
    Prim(Prim),
    /// Apply the verb to cells of the given ranks (J `"`, APL `⍤`).
    Rank(Box<Verb>, [i64; 3]),
    /// Insert the verb between items, folding right to left (J `/`, APL `⌿`).
    Reduce(Box<Verb>),
    /// Apply the verb to runs of consecutive items (J `\` and `\.`, APL
    /// `\` and `⍀`). The valence chooses the runs; see [`WindowKind`].
    Windowed(Box<Verb>, WindowKind),
    /// J `u~`, APL `u⍨`: monad `u~ y` = `y u y`; dyad `x u~ y` = `y u x`.
    Commute(Box<Verb>),
    /// J `u^:n`, APL `u⍣n`: apply the verb n times, or to convergence.
    PowerN(Box<Verb>, Power),
    /// (f g h) y = (f y) g (h y);  x (f g h) y = (x f y) g (x h y).
    Fork(Box<Verb>, Box<Verb>, Box<Verb>),
    /// (n g h) y = n g (h y);  x (n g h) y = n g (x h y).
    NounFork(Array, Box<Verb>, Box<Verb>),
    /// (f g) y = y f (g y);  x (f g) y = x f (g y).  (J hook)
    Hook(Box<Verb>, Box<Verb>),
    /// f@:g / [: f g:  monad f (g y);  dyad f (x g y).
    Atop(Box<Verb>, Box<Verb>),
    /// f&:g:  monad f (g y);  dyad (g x) f (g y). J's `&` is this wrapped in
    /// [`Verb::Rank`] at g's monadic rank; `&:` is this on its own.
    Compose(Box<Verb>, Box<Verb>),
    /// `m&v`: the noun bonded as the left argument — monad `m v y`. J gives
    /// a bond no dyadic valence at all.
    BondLeft(Array, Box<Verb>),
    /// `u&n`: the noun bonded as the right argument — monad `y u n`.
    BondRight(Box<Verb>, Array),
    /// J `u&.>` and APL `u¨`: open each box, apply u, put the result back
    /// in a box. Cell rank 0 on every side, so the frames pair as usual.
    Each(Box<Verb>, Enclose),
}

impl Verb {
    /// [monadic, dyadic-left, dyadic-right] ranks governing cell iteration.
    pub fn ranks(&self) -> [i64; 3] {
        match self {
            Verb::Prim(p) => p.ranks,
            Verb::Rank(_, r) => *r,
            // `x u\ y` takes one window size per application, so the left
            // cell is an atom: a list of sizes frames the result, as in J.
            Verb::Windowed(_, WindowKind::Prefix) => [RANK_INF, 0, RANK_INF],
            Verb::Each(..) => [0, 0, 0],
            _ => [RANK_INF, RANK_INF, RANK_INF],
        }
    }

    /// Name for diagnostics, e.g. `+/"1`.
    pub fn name(&self) -> String {
        match self {
            Verb::Prim(p) => p.name.to_string(),
            Verb::Rank(v, r) => format!("{}\"{}", v.name(), rank_str(*r)),
            Verb::Reduce(v) => format!("{}/", v.name()),
            Verb::Windowed(v, WindowKind::Suffix) => format!("{}\\.", v.name()),
            Verb::Windowed(v, _) => format!("{}\\", v.name()),
            Verb::Commute(v) => format!("{}~", v.name()),
            Verb::PowerN(v, Power::Converge) => format!("{}^:_", v.name()),
            Verb::PowerN(v, Power::Times(n)) => format!("{}^:{n}", v.name()),
            Verb::Fork(f, g, h) => format!("({} {} {})", f.name(), g.name(), h.name()),
            Verb::NounFork(_, g, h) => format!("(n {} {})", g.name(), h.name()),
            Verb::Hook(f, g) => format!("({} {})", f.name(), g.name()),
            Verb::Atop(f, g) => format!("({}@:{})", f.name(), g.name()),
            Verb::Compose(f, g) => format!("({}&:{})", f.name(), g.name()),
            Verb::BondLeft(_, v) => format!("(n&{})", v.name()),
            Verb::BondRight(v, _) => format!("({}&n)", v.name()),
            Verb::Each(v, Enclose::Always) => format!("({}&.>)", v.name()),
            Verb::Each(v, _) => format!("({}¨)", v.name()),
        }
    }

    /// True when applying this verb does nothing beyond producing its
    /// result. Output (`echo`, `⎕←`) is the only effect a verb can have, and
    /// only a pure verb may have its cells run out of order on several
    /// threads. Deliberately conservative: a new effect must be added here.
    pub fn is_pure(&self) -> bool {
        match self {
            Verb::Prim(p) => !matches!(p.monad, MonadOp::Echo),
            Verb::Rank(v, _)
            | Verb::Reduce(v)
            | Verb::Windowed(v, _)
            | Verb::Commute(v)
            | Verb::PowerN(v, _) => v.is_pure(),
            Verb::Fork(f, g, h) => f.is_pure() && g.is_pure() && h.is_pure(),
            Verb::NounFork(_, g, h)
            | Verb::Hook(g, h)
            | Verb::Atop(g, h)
            | Verb::Compose(g, h) => g.is_pure() && h.is_pure(),
            Verb::BondLeft(_, v) | Verb::BondRight(v, _) | Verb::Each(v, _) => v.is_pure(),
        }
    }

    /// Full monadic application including rank/frame machinery.
    pub fn monad(&self, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
        match self {
            Verb::Prim(p) => {
                // Scalar verbs have cell rank 0: the cells are the elements,
                // so the whole buffer is one elementwise pass.
                if let MonadOp::Scalar(op) = p.monad {
                    return scalar_monad(op, y, span);
                }
                let frame_rank = y.rank() - effective_rank(p.ranks[0], y.rank());
                if frame_rank == 0 {
                    return monad_op(p, y, ctx, span);
                }
                let frame = y.shape[..frame_rank].to_vec();
                let n: usize = frame.iter().product();
                let cells = each_cell(n, y.count(), self.is_pure(), ctx, |i, c| {
                    monad_op(p, &y.cell_at(frame_rank, i), c, span)
                })?;
                assemble(&frame, cells, span)
            }
            Verb::Rank(v, r) => {
                let frame_rank = y.rank() - effective_rank(r[0], y.rank());
                if frame_rank == 0 {
                    // The inner verb applies its own rank machinery to the
                    // whole argument; that is what `"` means.
                    return v.monad(y, ctx, span);
                }
                let frame = y.shape[..frame_rank].to_vec();
                let n: usize = frame.iter().product();
                let cells = each_cell(n, y.count(), self.is_pure(), ctx, |i, c| {
                    v.monad(&y.cell_at(frame_rank, i), c, span)
                })?;
                assemble(&frame, cells, span)
            }
            Verb::Reduce(v) => reduce(v, y, ctx, span),
            Verb::Windowed(v, kind) => {
                runs(v, y, *kind == WindowKind::Suffix, ctx, span)
            }
            Verb::Commute(v) => v.dyad(y, y, ctx, span),
            Verb::PowerN(v, p) => power(v, *p, None, y, ctx, span),
            Verb::Fork(f, g, h) => {
                let l = f.monad(y, ctx, span)?;
                let r = h.monad(y, ctx, span)?;
                g.dyad(&l, &r, ctx, span)
            }
            Verb::NounFork(n, g, h) => {
                let r = h.monad(y, ctx, span)?;
                g.dyad(n, &r, ctx, span)
            }
            Verb::Hook(f, g) => {
                let r = g.monad(y, ctx, span)?;
                f.dyad(y, &r, ctx, span)
            }
            Verb::Atop(f, g) | Verb::Compose(f, g) => {
                let r = g.monad(y, ctx, span)?;
                f.monad(&r, ctx, span)
            }
            Verb::BondLeft(m, v) => v.dyad(m, y, ctx, span),
            Verb::BondRight(v, n) => v.dyad(y, n, ctx, span),
            Verb::Each(u, rule) => {
                let n = y.count();
                let cells = each_cell(n, n, self.is_pure(), ctx, |i, c| {
                    let opened = open_cell(&atom(y, i));
                    Ok(enclose(&u.monad(&opened, c, span)?, *rule))
                })?;
                assemble(&y.shape, cells, span)
            }
        }
    }

    /// Full dyadic application including rank/frame/agreement machinery.
    pub fn dyad(&self, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
        match self {
            Verb::Prim(_) | Verb::Rank(_, _) | Verb::Each(..) => {
                self.dyad_ranked(x, y, ctx, span)
            }
            // `x u\ y` needs the frame machinery: its left cell is an atom.
            Verb::Windowed(_, WindowKind::Prefix) => self.dyad_ranked(x, y, ctx, span),
            Verb::Windowed(_, WindowKind::Suffix) => {
                Err(Error::not_yet("outfix (x u\\. y)", span))
            }
            Verb::Windowed(_, WindowKind::Scan) => {
                Err(Error::not_yet("dyadic scan (x f\\ y)", span))
            }
            Verb::Commute(v) => v.dyad(y, x, ctx, span),
            Verb::PowerN(v, p) => power(v, *p, Some(x), y, ctx, span),
            // `x u/ y` is the table: every cell of x against every cell of y.
            Verb::Reduce(v) => table(v, x, y, ctx, span),
            Verb::Fork(f, g, h) => {
                let l = f.dyad(x, y, ctx, span)?;
                let r = h.dyad(x, y, ctx, span)?;
                g.dyad(&l, &r, ctx, span)
            }
            Verb::NounFork(n, g, h) => {
                let r = h.dyad(x, y, ctx, span)?;
                g.dyad(n, &r, ctx, span)
            }
            Verb::Hook(f, g) => {
                let r = g.monad(y, ctx, span)?;
                f.dyad(x, &r, ctx, span)
            }
            Verb::Atop(f, g) => {
                let r = g.dyad(x, y, ctx, span)?;
                f.monad(&r, ctx, span)
            }
            Verb::Compose(f, g) => {
                let l = g.monad(x, ctx, span)?;
                let r = g.monad(y, ctx, span)?;
                f.dyad(&l, &r, ctx, span)
            }
            // J gives a bond one valence only.
            Verb::BondLeft(..) | Verb::BondRight(..) => {
                Err(Error::domain(format!("{} has no dyadic meaning", self.name()), span))
            }
        }
    }

    /// Dyadic application for the verbs that carry cell ranks.
    fn dyad_ranked(&self, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
        let ranks = self.ranks();
        let er_l = effective_rank(ranks[1], x.rank());
        let er_r = effective_rank(ranks[2], y.rank());
        if er_l == 0 && er_r == 0 {
            // Both cells are elements: run the flat elementwise path instead
            // of materialising one Array per element.
            if let Some(op) = self.scalar_dyad_op() {
                return scalar_dyad(op, x, y, ctx.cfg.agreement, span);
            }
        }
        let fxl = x.rank() - er_l;
        let fyl = y.rank() - er_r;
        let p = agree(&x.shape[..fxl], &y.shape[..fyl], &x.shape, &y.shape, ctx.cfg.agreement, span)?;
        if p.frame.is_empty() {
            return self.dyad_cell(x, y, ctx, span);
        }
        let work = x.count().max(y.count());
        let cells = each_cell(p.n, work, self.is_pure(), ctx, |i, c| {
            let xc = x.cell_at(fxl, i / p.x_div);
            let yc = y.cell_at(fyl, i / p.y_div);
            self.dyad_cell(&xc, &yc, c, span)
        })?;
        assemble(&p.frame, cells, span)
    }

    /// The meaning applied to one pair of cells by `dyad_ranked`.
    fn dyad_cell(&self, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
        match self {
            Verb::Prim(p) => dyad_op(p, x, y, ctx.cfg.agreement, span),
            Verb::Rank(v, _) => v.dyad(x, y, ctx, span),
            Verb::Windowed(v, _) => infix(v, x, y, ctx, span),
            Verb::Each(u, rule) => {
                let r = u.dyad(&open_cell(x), &open_cell(y), ctx, span)?;
                Ok(enclose(&r, *rule))
            }
            _ => Err(Error::internal("dyad_cell on a verb without cell ranks")),
        }
    }

    /// The elementwise dyadic operation this verb performs on element cells,
    /// if it performs one.
    fn scalar_dyad_op(&self) -> Option<ScalarDyad> {
        match self {
            Verb::Prim(p) => match p.dyad {
                DyadOp::Scalar(op) => Some(op),
                _ => None,
            },
            Verb::Rank(v, _) => v.scalar_dyad_op(),
            _ => None,
        }
    }
}

/// Effective cell rank: nonnegative rank clamps to the argument's rank;
/// negative rank means "leave |r| frame axes" (at least rank 0 cells).
pub fn effective_rank(r: i64, arg_rank: usize) -> usize {
    if r >= 0 {
        (r as usize).min(arg_rank)
    } else {
        arg_rank.saturating_sub(r.unsigned_abs() as usize)
    }
}

/// Apply `f` to the `n` cells of a frame, in index order.
///
/// Cells are independent, so a pure verb runs them on several threads and
/// the results are framed afterwards; an impure one keeps the caller's
/// context, and with it the order its output appears in. `work` is the
/// number of elements the whole application touches, which decides whether
/// splitting is worth it. Either way the first failing cell in index order
/// supplies the error.
fn each_cell<F>(
    n: usize,
    work: usize,
    pure: bool,
    ctx: &mut Ctx<'_>,
    f: F,
) -> Result<Vec<Array>>
where
    F: Fn(usize, &mut Ctx<'_>) -> Result<Array> + Sync + Send,
{
    if pure && n > 1 && par::worth_it(work) {
        let cfg = ctx.cfg;
        return par::map_indexed(n, |i| cfg.pure(|c| f(i, c))).into_iter().collect();
    }
    (0..n).map(|i| f(i, ctx)).collect()
}

// ---------------------------------------------------------------- naming

fn one_rank(r: i64) -> String {
    if r == RANK_INF { "_".to_string() } else { r.to_string() }
}

/// The rank list as `"` writes it: one number when all three agree,
/// otherwise monadic, dyadic-left, dyadic-right.
fn rank_str(r: [i64; 3]) -> String {
    if r[0] == r[1] && r[1] == r[2] {
        one_rank(r[0])
    } else {
        format!("{} {} {}", one_rank(r[0]), one_rank(r[1]), one_rank(r[2]))
    }
}

/// A shape as it appears in diagnostics.
fn show_shape(shape: &[usize]) -> String {
    if shape.is_empty() {
        return "(scalar)".to_string();
    }
    shape.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" ")
}

// ------------------------------------------------------------- indexing

/// Row-major strides for `shape`.
fn strides(shape: &[usize]) -> Vec<usize> {
    let mut s = vec![1usize; shape.len()];
    for k in (0..shape.len().saturating_sub(1)).rev() {
        s[k] = s[k + 1] * shape[k + 1];
    }
    s
}

/// Step `coord` to the next position in row-major order within `shape`.
fn odometer(coord: &mut [usize], shape: &[usize]) {
    for k in (0..coord.len()).rev() {
        coord[k] += 1;
        if coord[k] < shape[k] {
            return;
        }
        coord[k] = 0;
    }
}

/// Append element `i` of `src` to `dst`. Both must have the same dtype.
fn push_elem(dst: &mut Data, src: &Data, i: usize) {
    match (dst, src) {
        (Data::Bool(a), Data::Bool(b)) => a.push(b[i]),
        (Data::I64(a), Data::I64(b)) => a.push(b[i]),
        (Data::F64(a), Data::F64(b)) => a.push(b[i]),
        (Data::Char(a), Data::Char(b)) => a.push(b[i]),
        (Data::Box(a), Data::Box(b)) => a.push(b[i].clone()),
        _ => debug_assert!(false, "push_elem across dtypes"),
    }
}

/// `n` fill elements of the given type.
fn fill_data(dtype: DType, n: usize) -> Data {
    let mut d = Data::empty(dtype);
    for _ in 0..n {
        d.push_fill();
    }
    d
}

// ------------------------------------------------------------ agreement

/// How result cells map back to argument cells: result cell `i` uses left
/// cell `i / x_div` and right cell `i / y_div`.
struct Pairing {
    frame: Vec<usize>,
    n: usize,
    x_div: usize,
    y_div: usize,
}

fn frame_mismatch(
    xs: &[usize],
    ys: &[usize],
    fx: &[usize],
    fy: &[usize],
    axis: usize,
    span: Span,
) -> Error {
    // 1-D against 1-D is a length error in both languages; anything else is
    // reported as a shape error.
    let kind = if fx.len() == 1 && fy.len() == 1 { ErrorKind::Length } else { ErrorKind::Shape };
    let note = if axis < fx.len() && axis < fy.len() {
        format!("frames first differ at axis {axis}: {} vs {}", fx[axis], fy[axis])
    } else {
        format!(
            "frames have different numbers of axes: {} vs {}, diverging at axis {axis}",
            fx.len(),
            fy.len()
        )
    };
    Error::new(
        kind,
        format!(
            "arguments do not agree: left shape {}, right shape {}",
            show_shape(xs),
            show_shape(ys)
        ),
        Some(span),
    )
    .note(note)
}

/// Check frame agreement and build the cell pairing. `xs`/`ys` are the full
/// argument shapes, used only for diagnostics.
fn agree(
    fx: &[usize],
    fy: &[usize],
    xs: &[usize],
    ys: &[usize],
    mode: Agreement,
    span: Span,
) -> Result<Pairing> {
    let common = fx.len().min(fy.len());
    match mode {
        Agreement::LeadingPrefix => {
            for i in 0..common {
                if fx[i] != fy[i] {
                    return Err(frame_mismatch(xs, ys, fx, fy, i, span));
                }
            }
            let (long, short) = if fx.len() >= fy.len() { (fx, fy) } else { (fy, fx) };
            let n: usize = long.iter().product();
            let surplus: usize = long[short.len()..].iter().product();
            let (x_div, y_div) =
                if fx.len() >= fy.len() { (1, surplus.max(1)) } else { (surplus.max(1), 1) };
            Ok(Pairing { frame: long.to_vec(), n, x_div, y_div })
        }
        Agreement::ExactOrScalar => {
            if fx == fy {
                let n: usize = fx.iter().product();
                return Ok(Pairing { frame: fx.to_vec(), n, x_div: 1, y_div: 1 });
            }
            if fx.is_empty() {
                let n: usize = fy.iter().product();
                return Ok(Pairing { frame: fy.to_vec(), n, x_div: n.max(1), y_div: 1 });
            }
            if fy.is_empty() {
                let n: usize = fx.iter().product();
                return Ok(Pairing { frame: fx.to_vec(), n, x_div: 1, y_div: n.max(1) });
            }
            let axis = (0..common).find(|&i| fx[i] != fy[i]).unwrap_or(common);
            Err(frame_mismatch(xs, ys, fx, fy, axis, span))
        }
    }
}

// ------------------------------------------------------------- assembly

/// Frame the results of a cell-by-cell application into one array.
fn assemble(frame: &[usize], cells: Vec<Array>, span: Span) -> Result<Array> {
    if cells.is_empty() {
        // Nothing to take a cell shape from. J runs the verb on a fill cell
        // to learn the shape; we yield an empty array of the frame's shape.
        return Ok(Array { shape: frame.to_vec(), data: Data::empty(DType::I64) });
    }
    let mut dt = cells[0].dtype();
    for c in &cells[1..] {
        dt = DType::promote(dt, c.dtype()).ok_or_else(|| {
            let boxed = dt == DType::Box || c.dtype() == DType::Box;
            let what = if boxed {
                "cannot frame boxed and unboxed results into one array"
            } else {
                "cannot frame character and numeric results into one array"
            };
            Error::new(ErrorKind::Type, what, Some(span))
        })?;
    }
    let widen = |c: &Array| -> Result<Data> {
        c.data.cast(dt).ok_or_else(|| Error::internal("unsupported widening while framing"))
    };

    if cells[1..].iter().all(|c| c.shape == cells[0].shape) {
        let mut data = Data::empty(dt);
        for c in &cells {
            if c.dtype() == dt {
                data.extend_from(&c.data);
            } else {
                data.extend_from(&widen(c)?);
            }
        }
        let mut shape = frame.to_vec();
        shape.extend_from_slice(&cells[0].shape);
        return Ok(Array::new(shape, data));
    }

    // Unequal cell shapes: pad every cell out to the per-axis maximum,
    // aligning lower-rank cells at the trailing axes.
    let crank = cells.iter().map(|c| c.rank()).max().unwrap_or(0);
    let padded: Vec<Vec<usize>> = cells
        .iter()
        .map(|c| {
            let mut s = vec![1usize; crank - c.rank()];
            s.extend_from_slice(&c.shape);
            s
        })
        .collect();
    let mut common = vec![0usize; crank];
    for s in &padded {
        for k in 0..crank {
            common[k] = common[k].max(s[k]);
        }
    }
    let cell_n: usize = common.iter().product();
    let mut data = Data::empty(dt);
    for (c, ps) in cells.iter().zip(&padded) {
        let cd = if c.dtype() == dt { c.data.clone() } else { widen(c)? };
        let st = strides(ps);
        let mut coord = vec![0usize; crank];
        for _ in 0..cell_n {
            let mut idx = 0usize;
            let mut inside = true;
            for k in 0..crank {
                if coord[k] >= ps[k] {
                    inside = false;
                    break;
                }
                idx += coord[k] * st[k];
            }
            if inside {
                push_elem(&mut data, &cd, idx);
            } else {
                data.push_fill();
            }
            odometer(&mut coord, &common);
        }
    }
    let mut shape = frame.to_vec();
    shape.extend_from_slice(&common);
    Ok(Array::new(shape, data))
}

// ------------------------------------------------------------------ boxes

/// Element `i` of `a` as a rank-0 array — the cell an operation of rank 0
/// sees.
fn atom(a: &Array, i: usize) -> Array {
    Array { shape: Vec::new(), data: a.data.slice(i, i + 1) }
}

/// `< y` / `⊂ y`.
fn enclose(y: &Array, rule: Enclose) -> Array {
    if rule == Enclose::ExceptSimpleScalar && y.rank() == 0 && y.dtype() != DType::Box {
        return y.clone();
    }
    Array::boxed(y.clone())
}

/// One rank-0 cell opened: a box gives up its contents, anything else is
/// its own contents already.
fn open_cell(y: &Array) -> Array {
    match &y.data {
        Data::Box(v) if !v.is_empty() => v[0].clone(),
        _ => y.clone(),
    }
}

/// `↑ y` (APL): the first element, disclosed. An empty argument has none,
/// so its fill stands in.
fn first(y: &Array) -> Array {
    if y.count() == 0 {
        let mut d = Data::empty(y.dtype());
        d.push_fill();
        return open_cell(&Array { shape: Vec::new(), data: d });
    }
    open_cell(&atom(y, 0))
}

/// `≡ y` (APL).
fn depth(y: &Array) -> i64 {
    match &y.data {
        Data::Box(v) => 1 + v.iter().map(depth).max().unwrap_or(0),
        _ => i64::from(y.rank() > 0),
    }
}

/// Every leaf array inside `a`, in ravel order.
fn leaves(a: &Array, out: &mut Vec<Array>) {
    match &a.data {
        Data::Box(v) => {
            for b in v.iter() {
                leaves(b, out);
            }
        }
        _ => out.push(a.clone()),
    }
}

/// `∊ y` (APL): every leaf element as one vector.
fn enlist(y: &Array, span: Span) -> Result<Array> {
    let mut parts = Vec::new();
    leaves(y, &mut parts);
    // An empty leaf contributes no elements, so it does not decide the
    // type either.
    let mut dt = None;
    for p in parts.iter().filter(|p| p.count() > 0) {
        dt = Some(match dt {
            None => p.dtype(),
            Some(t) => DType::promote(t, p.dtype()).ok_or_else(|| {
                Error::new(
                    ErrorKind::Type,
                    "cannot enlist character and numeric data into one vector",
                    Some(span),
                )
            })?,
        });
    }
    let dt = dt.unwrap_or(DType::I64);
    let mut data = Data::empty(dt);
    for p in &parts {
        let cast = p.data.cast(dt).ok_or_else(|| Error::internal("unsupported widening in enlist"))?;
        data.extend_from(&cast);
    }
    Ok(Array::new(vec![data.len()], data))
}

/// A scalar repeated over `shape` — how a catenation spreads an atom.
fn spread(a: &Array, shape: &[usize]) -> Array {
    let n: usize = shape.iter().product();
    let mut data = Data::empty(a.dtype());
    for _ in 0..n {
        push_elem(&mut data, &a.data, 0);
    }
    Array::new(shape.to_vec(), data)
}

/// Per-axis maximum of two cell shapes, aligned at their trailing axes —
/// the same alignment framing uses.
fn wider_shape(a: &[usize], b: &[usize]) -> Vec<usize> {
    let r = a.len().max(b.len());
    let pad = |s: &[usize]| {
        let mut v = vec![1usize; r - s.len()];
        v.extend_from_slice(s);
        v
    };
    let (pa, pb) = (pad(a), pad(b));
    (0..r).map(|k| pa[k].max(pb[k])).collect()
}

/// `; y` (J): the items of the opened boxes, one after another. A scalar
/// among them spreads over the common item shape, as catenation does; the
/// rest are padded with fill, which is what makes raze accept items that
/// plain catenation would refuse.
fn raze(y: &Array, span: Span) -> Result<Array> {
    let opened: Vec<Array> = (0..y.count()).map(|i| open_cell(&atom(y, i))).collect();
    let mut common: Option<Vec<usize>> = None;
    for a in opened.iter().filter(|a| a.rank() > 0) {
        common = Some(match common {
            None => a.shape[1..].to_vec(),
            Some(c) => wider_shape(&c, &a.shape[1..]),
        });
    }
    let common = common.unwrap_or_default();
    let mut cells: Vec<Array> = Vec::new();
    for a in &opened {
        if a.rank() == 0 {
            cells.push(spread(a, &common));
            continue;
        }
        for i in 0..a.items() {
            cells.push(a.item(i));
        }
    }
    if cells.is_empty() {
        return Ok(Array::new(vec![0], Data::empty(DType::I64)));
    }
    let n = cells.len();
    assemble(&[n], cells, span)
}

/// `x ; y` (J): x boxed, then y — which joins as it is when it is already
/// boxed and boxed when it is not.
fn link(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let head = Array::boxed(x.clone());
    let tail = if y.dtype() == DType::Box { y.clone() } else { Array::boxed(y.clone()) };
    catenate(&head, &tail, true, span)
}

/// Every item of `y` boxed; an already boxed array is left alone.
fn box_items(y: &Array) -> Array {
    if y.dtype() == DType::Box {
        return y.clone();
    }
    let n = y.items();
    let boxes: Vec<Array> = (0..n).map(|i| item_or_self(y, i)).collect();
    Array::new(vec![n], Data::Box(boxes.into()))
}

/// APL vector notation: `x` becomes one more item in front of the strand
/// `y`. Simple scalars stay simple, so `1 2 3` is a plain integer vector
/// and only a strand holding something else becomes nested.
fn strand(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let item = enclose(x, Enclose::ExceptSimpleScalar);
    let one = |a: &Array| Array::new(vec![1], a.data.clone());
    if item.dtype() != DType::Box && y.dtype() != DType::Box {
        if DType::promote(item.dtype(), y.dtype()).is_none() {
            return Err(Error::not_yet(
                "a vector mixing characters and numbers (simple mixed arrays)",
                span,
            ));
        }
        return catenate(&one(&item), y, true, span);
    }
    let head = if item.dtype() == DType::Box { item } else { Array::boxed(item) };
    catenate(&one(&head), &box_items(y), true, span)
}

// -------------------------------------------------- elementwise operations

fn char_arith(span: Span) -> Error {
    Error::new(ErrorKind::Type, "cannot do arithmetic on characters", Some(span))
}

fn box_arith(span: Span) -> Error {
    Error::new(
        ErrorKind::Type,
        "cannot do arithmetic on boxed values; open them first (J `>`, APL `⊃`)",
        Some(span),
    )
}

/// The complaint an operation makes about an element type it cannot work
/// on at all.
fn wrong_type(d: DType, span: Span) -> Error {
    match d {
        DType::Box => box_arith(span),
        _ => char_arith(span),
    }
}

/// Borrow numeric data as i64, widening a boolean buffer into `tmp`.
fn borrow_i64<'a>(d: &'a Data, tmp: &'a mut Vec<i64>) -> &'a [i64] {
    match d {
        Data::I64(v) => v,
        Data::Bool(v) => {
            *tmp = v.iter().map(|&b| b as i64).collect();
            &tmp[..]
        }
        // Callers exclude character data before reaching here.
        _ => &[],
    }
}

/// Borrow numeric data as f64, widening into `tmp` when needed.
fn borrow_f64<'a>(d: &'a Data, tmp: &'a mut Vec<f64>) -> &'a [f64] {
    match d {
        Data::F64(v) => v,
        Data::I64(v) => {
            *tmp = v.iter().map(|&x| x as f64).collect();
            &tmp[..]
        }
        Data::Bool(v) => {
            *tmp = v.iter().map(|&x| x as f64).collect();
            &tmp[..]
        }
        _ => &[],
    }
}

/// Numeric data as f64, borrowed when it already is that.
fn as_f64<'a>(d: &'a Data, tmp: &'a mut Vec<f64>, span: Span) -> Result<&'a [f64]> {
    if !d.dtype().is_numeric() {
        return Err(wrong_type(d.dtype(), span));
    }
    Ok(borrow_f64(d, tmp))
}

/// The type an arithmetic pair computes in. Booleans count as integers.
fn arith_type(a: DType, b: DType, span: Span) -> Result<DType> {
    if a == DType::Box || b == DType::Box {
        return Err(box_arith(span));
    }
    match DType::promote(a, b) {
        Some(DType::Char) => Err(char_arith(span)),
        None => Err(Error::new(
            ErrorKind::Type,
            "cannot mix character and numeric data",
            Some(span),
        )),
        Some(DType::Bool) => Ok(DType::I64),
        Some(t) => Ok(t),
    }
}

/// Apply `f` to the argument pair behind every element of one output chunk.
/// Element `start + k` of the result pairs `xs[xoff + (start+k)/xdiv]` with
/// `ys[yoff + (start+k)/ydiv]`, so broadcasting and folding both run without
/// materialising cells.
///
/// The two shapes that carry the work — one element per element, and one
/// element spread over a whole chunk — become plain loops over slices, which
/// is what lets the compiler vectorise the pass; anything else keeps the
/// general index arithmetic. `f` returns false to abandon the chunk.
#[allow(clippy::too_many_arguments)]
#[inline]
fn zip_chunk<T, U, F>(
    xs: &[T],
    xoff: usize,
    xdiv: usize,
    ys: &[T],
    yoff: usize,
    ydiv: usize,
    start: usize,
    out: &mut [U],
    mut f: F,
) -> bool
where
    T: Copy,
    F: FnMut(T, T, &mut U) -> bool,
{
    let len = out.len();
    if len == 0 {
        return true;
    }
    let last = start + len - 1;
    let one_x = xdiv > 1 && start / xdiv == last / xdiv;
    let one_y = ydiv > 1 && start / ydiv == last / ydiv;
    if xdiv == 1 && ydiv == 1 {
        let xc = &xs[xoff + start..xoff + start + len];
        let yc = &ys[yoff + start..yoff + start + len];
        for ((slot, &a), &b) in out.iter_mut().zip(xc).zip(yc) {
            if !f(a, b, slot) {
                return false;
            }
        }
    } else if xdiv == 1 && one_y {
        let b = ys[yoff + start / ydiv];
        let xc = &xs[xoff + start..xoff + start + len];
        for (slot, &a) in out.iter_mut().zip(xc) {
            if !f(a, b, slot) {
                return false;
            }
        }
    } else if one_x && ydiv == 1 {
        let a = xs[xoff + start / xdiv];
        let yc = &ys[yoff + start..yoff + start + len];
        for (slot, &b) in out.iter_mut().zip(yc) {
            if !f(a, b, slot) {
                return false;
            }
        }
    } else {
        for (k, slot) in out.iter_mut().enumerate() {
            let i = start + k;
            if !f(xs[xoff + i / xdiv], ys[yoff + i / ydiv], slot) {
                return false;
            }
        }
    }
    true
}

// ------------------------------------------------- factorial and binomial

/// Lanczos coefficients for g = 7, the published nine-term series.
const LANCZOS: [f64; 9] = [
    0.999_999_999_999_809_9,
    676.520_368_121_885_1,
    -1_259.139_216_722_402_8,
    771.323_428_777_653_1,
    -176.615_029_162_140_6,
    12.507_343_278_686_905,
    -0.138_571_095_265_720_12,
    9.984_369_578_019_572e-6,
    1.505_632_735_149_311_6e-7,
];

/// The gamma function on the reals, by the Lanczos approximation (relative
/// error below 1e-13 over the range that stays finite). Poles are left to
/// the callers, which know the sign the limit approaches from.
fn gamma(x: f64) -> f64 {
    use std::f64::consts::PI;
    if x < 0.5 {
        // Reflection carries the negative half onto the positive one.
        return PI / ((PI * x).sin() * gamma(1.0 - x));
    }
    let z = x - 1.0;
    let mut a = LANCZOS[0];
    for (i, &c) in LANCZOS.iter().enumerate().skip(1) {
        a += c / (z + i as f64);
    }
    let t = z + 7.5;
    (2.0 * PI).sqrt() * t.powf(z + 0.5) * (-t).exp() * a
}

/// `! y`: gamma(y+1). Integers up to 20! are exact in f64 and every
/// factorial is one in J, which is why this never returns an integer.
fn factorial(y: f64) -> f64 {
    if y.fract() == 0.0 && y.abs() < 1e17 {
        let n = y as i64;
        if n < 0 {
            // A pole: the limit alternates sign as the argument walks left.
            return if n % 2 == -1 { f64::INFINITY } else { f64::NEG_INFINITY };
        }
        if n > 170 {
            return f64::INFINITY;
        }
        let mut c = 1.0f64;
        for i in 2..=n {
            c *= i as f64;
        }
        return c;
    }
    gamma(y + 1.0)
}

/// The largest left argument the product form of the binomial is taken for;
/// beyond it the gamma quotient is both faster and accurate enough.
const BINOMIAL_PRODUCT_LIMIT: i64 = 4096;

/// `x ! y` for a nonnegative whole x: the falling factorial over `x!`, one
/// factor at a time so that no partial product overflows more than the
/// result does.
fn binomial_product(x: i64, y: f64) -> f64 {
    let mut c = 1.0f64;
    for i in 1..=x {
        c = c * (y - i as f64 + 1.0) / i as f64;
        if c == 0.0 {
            break;
        }
    }
    c
}

/// The two whole-number cases J answers with an exact integer: a
/// nonnegative x, and a negative x against a y at least as negative (the
/// upper-negation identity). None when the value leaves i64.
fn binomial_i64(x: i64, y: i64) -> Option<i64> {
    if x < 0 {
        // C(y, x) is zero for a negative x unless y is negative too and no
        // greater, where C(y,x) = (-1)^(y-x) C(-x-1, -y-1).
        if y >= 0 || y < x {
            return Some(0);
        }
        let v = binomial_exact(-y - 1, -x - 1)?;
        return if (y - x) % 2 == 0 { Some(v) } else { v.checked_neg() };
    }
    binomial_exact(x, y)
}

/// `x ! y` in exact integers for a nonnegative whole x. Every partial value
/// is itself a binomial coefficient, so the division is always exact.
fn binomial_exact(x: i64, y: i64) -> Option<i64> {
    if x > BINOMIAL_PRODUCT_LIMIT {
        return None;
    }
    let mut c: i128 = 1;
    for i in 1..=x as i128 {
        c = c.checked_mul(y as i128 - i + 1)? / i;
        if c == 0 {
            break;
        }
    }
    i64::try_from(c).ok()
}

/// `x ! y` on the reals.
fn binomial(x: f64, y: f64) -> f64 {
    if x.fract() == 0.0 && x.abs() < 1e17 {
        let xi = x as i64;
        if xi < 0 {
            if y.fract() == 0.0 && y < 0.0 && y >= x {
                let sign = if (y as i64 - xi) % 2 == 0 { 1.0 } else { -1.0 };
                return sign * binomial_product(-y as i64 - 1, -x - 1.0);
            }
            return 0.0;
        }
        if xi <= BINOMIAL_PRODUCT_LIMIT {
            return binomial_product(xi, y);
        }
    }
    gamma(y + 1.0) / (gamma(x + 1.0) * gamma(y - x + 1.0))
}

/// One integer step. None means the result left i64 — an overflow, or a
/// value that is not an integer — and the whole pass is redone in f64.
#[inline]
fn i64_op(op: ScalarDyad, a: i64, b: i64) -> Option<i64> {
    use ScalarDyad::*;
    Some(match op {
        Add => a.checked_add(b)?,
        Sub => a.checked_sub(b)?,
        Mul => a.checked_mul(b)?,
        Min => a.min(b),
        Max => a.max(b),
        Residue => {
            if a == 0 {
                b
            } else {
                // wrapping_rem: i64::MIN % -1 is mathematically 0.
                let mut r = b.wrapping_rem(a);
                if r != 0 && (r < 0) != (a < 0) {
                    r += a;
                }
                r
            }
        }
        Pow => {
            if b < 0 {
                return None;
            }
            a.checked_pow(u32::try_from(b).ok()?)?
        }
        Binomial => binomial_i64(a, b)?,
        _ => return None,
    })
}

/// One float step.
#[inline]
fn f64_op(op: ScalarDyad, a: f64, b: f64, span: Span) -> Result<f64> {
    use ScalarDyad::*;
    Ok(match op {
        Add => a + b,
        Sub => a - b,
        Mul => a * b,
        Min => a.min(b),
        Max => a.max(b),
        DivJ => {
            if b == 0.0 {
                if a == 0.0 { 0.0 } else { f64::INFINITY.copysign(a) }
            } else {
                a / b
            }
        }
        DivApl => {
            if b == 0.0 {
                if a == 0.0 {
                    1.0
                } else {
                    return Err(Error::domain("division by zero", span));
                }
            } else {
                a / b
            }
        }
        Pow => {
            if a == 0.0 && b == 0.0 {
                1.0
            } else {
                a.powf(b)
            }
        }
        Residue => {
            if a == 0.0 {
                b
            } else {
                b - a * (b / a).floor()
            }
        }
        Log => {
            if a < 0.0 || b < 0.0 {
                return Err(Error::not_yet("complex numbers", span));
            }
            b.ln() / a.ln()
        }
        Root => {
            if b < 0.0 {
                return Err(Error::not_yet("complex numbers", span));
            }
            b.powf(1.0 / a)
        }
        Circle => return circle(a, b, span),
        Binomial => binomial(a, b),
        _ => return Err(Error::internal("non-arithmetic op in the float path")),
    })
}

/// `k o. y`: the circle function k applied to y.
///
/// The table is J's and APL's alike (they share it): 1 2 3 are sine, cosine
/// and tangent, 5 6 7 their hyperbolic counterparts, a negative k inverts the
/// function at |k|, and 0 and 4 are the two Pythagorean forms. The functions
/// that would leave the reals report the same "complex numbers" gap as `%:`
/// of a negative number; the k values that only mean something for a complex
/// argument (8..12 and their negatives) are named as their own gap.
#[inline]
fn circle(k: f64, y: f64, span: Span) -> Result<f64> {
    if k.fract() != 0.0 {
        return Err(Error::domain("the circle function needs an integer left argument", span));
    }
    let complex = || Error::not_yet("complex numbers", span);
    Ok(match k as i64 {
        0 => {
            if y.abs() > 1.0 {
                return Err(complex());
            }
            (1.0 - y * y).max(0.0).sqrt()
        }
        1 => y.sin(),
        2 => y.cos(),
        3 => y.tan(),
        4 => (1.0 + y * y).sqrt(),
        5 => y.sinh(),
        6 => y.cosh(),
        7 => y.tanh(),
        -1 => {
            if y.abs() > 1.0 {
                return Err(complex());
            }
            y.asin()
        }
        -2 => {
            if y.abs() > 1.0 {
                return Err(complex());
            }
            y.acos()
        }
        -3 => y.atan(),
        -4 => {
            if y.abs() < 1.0 {
                return Err(complex());
            }
            // The sign follows y: `_4 o. _2` is `_1.73205`, not `1.73205`.
            y.signum() * (y * y - 1.0).max(0.0).sqrt()
        }
        -5 => y.asinh(),
        -6 => {
            if y < 1.0 {
                return Err(complex());
            }
            y.acosh()
        }
        -7 => {
            if y.abs() > 1.0 {
                return Err(complex());
            }
            y.atanh()
        }
        other => {
            return Err(Error::not_yet(
                match other {
                    8 | -8 => "circle function 8 (complex)",
                    9 | -9 => "circle function 9 (complex)",
                    10 | -10 => "circle function 10 (complex)",
                    11 | -11 => "circle function 11 (complex)",
                    12 | -12 => "circle function 12 (complex)",
                    _ => "that circle function",
                },
                span,
            ));
        }
    })
}

/// One chunk of an integer pass. False means the chunk left i64 and the
/// caller redoes the whole operation in f64.
#[allow(clippy::too_many_arguments)]
fn dyad_i64_chunk(
    op: ScalarDyad,
    xs: &[i64],
    xoff: usize,
    xdiv: usize,
    ys: &[i64],
    yoff: usize,
    ydiv: usize,
    start: usize,
    out: &mut [i64],
) -> bool {
    use ScalarDyad::*;
    // The overflow of the three growing operations is folded into a flag
    // rather than breaking the loop: that keeps the pass branch-free, and an
    // overflowing chunk is thrown away and redone in f64 in any case.
    macro_rules! overflowing {
        ($m:ident) => {{
            let mut over = false;
            zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, out, |a, b, slot: &mut i64| {
                let (v, o) = i64::$m(a, b);
                *slot = v;
                over |= o;
                true
            });
            !over
        }};
    }
    macro_rules! plain {
        ($step:expr) => {{
            zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, out, |a, b, slot: &mut i64| {
                *slot = $step(a, b);
                true
            })
        }};
    }
    match op {
        Add => overflowing!(overflowing_add),
        Sub => overflowing!(overflowing_sub),
        Mul => overflowing!(overflowing_mul),
        Min => plain!(i64::min),
        Max => plain!(i64::max),
        _ => zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, out, |a, b, slot: &mut i64| {
            match i64_op(op, a, b) {
                Some(v) => {
                    *slot = v;
                    true
                }
                None => false,
            }
        }),
    }
}

/// One elementwise integer pass. None means it left i64 anywhere.
#[allow(clippy::too_many_arguments)]
fn dyad_i64(
    op: ScalarDyad,
    xs: &[i64],
    xoff: usize,
    xdiv: usize,
    ys: &[i64],
    yoff: usize,
    ydiv: usize,
    n: usize,
) -> Option<Vec<i64>> {
    let (out, ok) = par::fill(n, |start, part| {
        dyad_i64_chunk(op, xs, xoff, xdiv, ys, yoff, ydiv, start, part)
    });
    ok.then_some(out)
}

#[allow(clippy::too_many_arguments)]
fn dyad_f64_chunk(
    op: ScalarDyad,
    xs: &[f64],
    xoff: usize,
    xdiv: usize,
    ys: &[f64],
    yoff: usize,
    ydiv: usize,
    start: usize,
    out: &mut [f64],
    span: Span,
) -> Result<()> {
    use ScalarDyad::*;
    // The arithmetic that cannot fail is picked before the loop, so the
    // compiler sees one operation per pass instead of a match per element.
    macro_rules! plain {
        ($step:expr) => {{
            zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, out, |a, b, slot: &mut f64| {
                *slot = $step(a, b);
                true
            });
            return Ok(());
        }};
    }
    match op {
        Add => plain!(|a: f64, b: f64| a + b),
        Sub => plain!(|a: f64, b: f64| a - b),
        Mul => plain!(|a: f64, b: f64| a * b),
        Min => plain!(f64::min),
        Max => plain!(f64::max),
        _ => {}
    }
    let mut err = None;
    zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, out, |a, b, slot: &mut f64| {
        match f64_op(op, a, b, span) {
            Ok(v) => {
                *slot = v;
                true
            }
            Err(e) => {
                err = Some(e);
                false
            }
        }
    });
    match err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

#[allow(clippy::too_many_arguments)]
fn dyad_f64(
    op: ScalarDyad,
    xs: &[f64],
    xoff: usize,
    xdiv: usize,
    ys: &[f64],
    yoff: usize,
    ydiv: usize,
    n: usize,
    span: Span,
) -> Result<Vec<f64>> {
    par::try_fill(n, |start, part| {
        dyad_f64_chunk(op, xs, xoff, xdiv, ys, yoff, ydiv, start, part, span)
    })
}

#[allow(clippy::too_many_arguments)]
fn compare_data(
    op: ScalarDyad,
    x: &Data,
    xoff: usize,
    xdiv: usize,
    y: &Data,
    yoff: usize,
    ydiv: usize,
    n: usize,
    span: Span,
) -> Result<Data> {
    use ScalarDyad::*;
    let (dx, dy) = (x.dtype(), y.dtype());
    let equality = matches!(op, Eq | Ne);
    if dx == DType::Box || dy == DType::Box {
        // Boxes have no order — J refuses `<` on them — but they do have
        // equality, which compares their contents.
        if !equality {
            return Err(box_arith(span));
        }
        let (Data::Box(a), Data::Box(b)) = (x, y) else {
            return Err(Error::new(
                ErrorKind::Type,
                "cannot compare boxed and unboxed values",
                Some(span),
            ));
        };
        let (out, _) = par::fill(n, |start, part: &mut [u8]| {
            for (k, slot) in part.iter_mut().enumerate() {
                let i = start + k;
                let e = arrays_match(&a[xoff + i / xdiv], &b[yoff + i / ydiv]);
                *slot = u8::from(if op == Eq { e } else { !e });
            }
            true
        });
        return Ok(Data::Bool(out.into()));
    }
    if dx == DType::Char || dy == DType::Char {
        if dx != dy {
            // J compares mixed types as unequal rather than failing; libjay
            // reports it, per the "refuse rather than guess" rule.
            return Err(Error::new(
                ErrorKind::Type,
                "cannot compare character and numeric data",
                Some(span),
            ));
        }
        if !equality {
            return Err(Error::new(
                ErrorKind::Type,
                "cannot order character data; only equality applies",
                Some(span),
            ));
        }
        let (Data::Char(a), Data::Char(b)) = (x, y) else {
            return Err(Error::internal("character comparison on non-character data"));
        };
        let (out, _) = par::fill(n, |start, part: &mut [u8]| {
            zip_chunk(a, xoff, xdiv, b, yoff, ydiv, start, part, |p, q, slot| {
                let e = p == q;
                *slot = if op == Eq { e as u8 } else { !e as u8 };
                true
            })
        });
        return Ok(Data::Bool(out.into()));
    }
    // Floats compare exactly. J's comparison tolerance (!:) is a known
    // divergence, to revisit together with the rest of the tolerance rules.
    let out = if DType::promote(dx, dy) == Some(DType::F64) {
        let (mut tx, mut ty) = (Vec::new(), Vec::new());
        let xs = borrow_f64(x, &mut tx);
        let ys = borrow_f64(y, &mut ty);
        par::fill(n, |start, part: &mut [u8]| {
            zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, part, |a, b, slot| {
                *slot = cmp_result(op, a.partial_cmp(&b)) as u8;
                true
            })
        })
        .0
    } else {
        let (mut tx, mut ty) = (Vec::new(), Vec::new());
        let xs = borrow_i64(x, &mut tx);
        let ys = borrow_i64(y, &mut ty);
        par::fill(n, |start, part: &mut [u8]| {
            zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, part, |a, b, slot| {
                *slot = cmp_result(op, Some(i64::cmp(&a, &b))) as u8;
                true
            })
        })
        .0
    };
    Ok(Data::Bool(out.into()))
}

/// Turn an ordering (None for NaN) into a comparison result.
fn cmp_result(op: ScalarDyad, ord: Option<std::cmp::Ordering>) -> bool {
    use std::cmp::Ordering::*;
    use ScalarDyad::*;
    match ord {
        None => matches!(op, Ne),
        Some(o) => match op {
            Eq => o == Equal,
            Ne => o != Equal,
            Lt => o == Less,
            Le => o != Greater,
            Gt => o == Greater,
            Ge => o != Less,
            _ => false,
        },
    }
}

/// Greatest common divisor, always nonnegative; `gcd(0, 0)` is 0.
fn gcd_i128(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// LCM/GCD over two buffers. Two booleans stay boolean, where the pair is
/// exactly logical and (LCM) / or (GCD); integers give integers; floats are
/// accepted only when every value is integral.
#[allow(clippy::too_many_arguments)]
fn lcm_gcd_data(
    op: ScalarDyad,
    x: &Data,
    xoff: usize,
    xdiv: usize,
    y: &Data,
    yoff: usize,
    ydiv: usize,
    n: usize,
    span: Span,
) -> Result<Data> {
    let t = arith_type(x.dtype(), y.dtype(), span)?;
    let both_bool = x.dtype() == DType::Bool && y.dtype() == DType::Bool;
    let float = t == DType::F64;
    let (xs, ys) = if float {
        let (mut tx, mut ty) = (Vec::new(), Vec::new());
        let xf = borrow_f64(x, &mut tx);
        let yf = borrow_f64(y, &mut ty);
        let integral = |v: &[f64]| v.iter().all(|&a| a.fract() == 0.0 && fits_i64(a));
        if !integral(xf) || !integral(yf) {
            return Err(Error::not_yet("LCM/GCD on floats", span));
        }
        (
            xf.iter().map(|&a| a as i64).collect::<Vec<_>>(),
            yf.iter().map(|&a| a as i64).collect::<Vec<_>>(),
        )
    } else {
        let (mut tx, mut ty) = (Vec::new(), Vec::new());
        (borrow_i64(x, &mut tx).to_vec(), borrow_i64(y, &mut ty).to_vec())
    };
    // The chunk flag carries "every value fits an i64", so the whole pass
    // widens to float exactly when the sequential one would.
    let (out, fits) = par::fill(n, |start, part: &mut [i128]| {
        let mut fits = true;
        zip_chunk(&xs, xoff, xdiv, &ys, yoff, ydiv, start, part, |a, b, slot| {
            let (a, b) = (a as i128, b as i128);
            let g = gcd_i128(a, b);
            let v = if op == ScalarDyad::Gcd {
                g
            } else if g == 0 {
                0
            } else {
                a / g * b
            };
            fits &= i64::try_from(v).is_ok();
            *slot = v;
            true
        });
        fits
    });
    if !fits || float {
        return Ok(Data::F64(par::map(&out, |&v| v as f64).into()));
    }
    if both_bool {
        return Ok(Data::Bool(par::map(&out, |&v| v as u8).into()));
    }
    Ok(Data::I64(par::map(&out, |&v| v as i64).into()))
}

/// One elementwise dyadic pass over two buffers. Element `i` of the result
/// pairs `x[xoff + i / xdiv]` with `y[yoff + i / ydiv]`, so broadcasting and
/// folding both run without materialising cells.
#[allow(clippy::too_many_arguments)]
fn scalar_dyad_data(
    op: ScalarDyad,
    x: &Data,
    xoff: usize,
    xdiv: usize,
    y: &Data,
    yoff: usize,
    ydiv: usize,
    n: usize,
    span: Span,
) -> Result<Data> {
    use ScalarDyad::*;
    if matches!(op, Eq | Ne | Lt | Le | Gt | Ge) {
        return compare_data(op, x, xoff, xdiv, y, yoff, ydiv, n, span);
    }
    if matches!(op, Lcm | Gcd) {
        return lcm_gcd_data(op, x, xoff, xdiv, y, yoff, ydiv, n, span);
    }
    let t = arith_type(x.dtype(), y.dtype(), span)?;
    if t == DType::I64 && !matches!(op, DivJ | DivApl | Log | Root | Circle) {
        // Binomial reaches this path: a whole pair has a whole answer, and
        // the i64 step declines (None) exactly where J widens to float.
        let (mut tx, mut ty) = (Vec::new(), Vec::new());
        let xs = borrow_i64(x, &mut tx);
        let ys = borrow_i64(y, &mut ty);
        if let Some(v) = dyad_i64(op, xs, xoff, xdiv, ys, yoff, ydiv, n) {
            return Ok(Data::I64(v.into()));
        }
        // Integer overflow (or a fractional result): J widens to float.
    }
    let (mut tx, mut ty) = (Vec::new(), Vec::new());
    let xs = borrow_f64(x, &mut tx);
    let ys = borrow_f64(y, &mut ty);
    Ok(Data::F64(dyad_f64(op, xs, xoff, xdiv, ys, yoff, ydiv, n, span)?.into()))
}

/// Elementwise dyadic application of a scalar operation to whole arrays.
fn scalar_dyad(
    op: ScalarDyad,
    x: &Array,
    y: &Array,
    mode: Agreement,
    span: Span,
) -> Result<Array> {
    let p = agree(&x.shape, &y.shape, &x.shape, &y.shape, mode, span)?;
    let data = scalar_dyad_data(op, &x.data, 0, p.x_div, &y.data, 0, p.y_div, p.n, span)?;
    Ok(Array::new(p.frame, data))
}

/// Is `v` exactly representable as an i64?
fn fits_i64(v: f64) -> bool {
    v.is_finite() && v >= i64::MIN as f64 && v < i64::MAX as f64
}

/// Elementwise monadic application to a whole array.
fn scalar_monad(op: ScalarMonad, y: &Array, span: Span) -> Result<Array> {
    use ScalarMonad::*;
    let d = &y.data;
    // The float-only operations borrow float data as it lies; anything else
    // is widened once into `tmp` first.
    let mut tmp = Vec::new();
    let data = match op {
        Conj => match d {
            // Identity on reals; conjugation matters once complex arrives.
            Data::Char(_) | Data::Box(_) => return Err(wrong_type(d.dtype(), span)),
            _ => d.clone(),
        },
        Neg => match d {
            Data::Bool(v) => Data::I64(par::map(v, |&b| -(b as i64)).into()),
            Data::I64(v) => match par::try_map(v, i64::checked_neg) {
                Some(out) => Data::I64(out.into()),
                None => Data::F64(par::map(v, |&x| -(x as f64)).into()),
            },
            Data::F64(v) => Data::F64(par::map(v, |&x| -x).into()),
            Data::Char(_) | Data::Box(_) => return Err(wrong_type(d.dtype(), span)),
        },
        Signum => match d {
            Data::Bool(v) => Data::I64(par::map(v, |&b| b as i64).into()),
            Data::I64(v) => Data::I64(par::map(v, |&x| x.signum()).into()),
            // NaN has no sign here; it yields 0.
            Data::F64(v) => Data::F64(
                par::map(v, |&x| if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 }).into(),
            ),
            Data::Char(_) | Data::Box(_) => return Err(wrong_type(d.dtype(), span)),
        },
        Recip => {
            // 1 % 0 is infinity, the J rule. APL's ÷0 is a domain error; a
            // ScalarMonad cannot tell the two languages apart, so the APL
            // divergence is left to revisit when monadic ops carry a dialect.
            let v = as_f64(d, &mut tmp, span)?;
            Data::F64(par::map(v, |&x| if x == 0.0 { f64::INFINITY } else { 1.0 / x }).into())
        }
        Sqrt => {
            let v = as_f64(d, &mut tmp, span)?;
            if v.iter().any(|&x| x < 0.0) {
                return Err(Error::not_yet("complex numbers", span));
            }
            Data::F64(par::map(v, |&x| x.sqrt()).into())
        }
        Exp => {
            let v = as_f64(d, &mut tmp, span)?;
            Data::F64(par::map(v, |&x| x.exp()).into())
        }
        Abs => match d {
            Data::Bool(_) => d.clone(),
            Data::I64(v) => match par::try_map(v, i64::checked_abs) {
                Some(out) => Data::I64(out.into()),
                None => Data::F64(par::map(v, |&x| (x as f64).abs()).into()),
            },
            Data::F64(v) => Data::F64(par::map(v, |&x| x.abs()).into()),
            Data::Char(_) | Data::Box(_) => return Err(wrong_type(d.dtype(), span)),
        },
        Floor | Ceil => match d {
            Data::Bool(v) => Data::I64(par::map(v, |&b| b as i64).into()),
            Data::I64(_) => d.clone(),
            Data::F64(v) => {
                let round = |x: f64| if op == Floor { x.floor() } else { x.ceil() };
                // Integer when every rounded value is one, as in J.
                match par::try_map(v, |x| {
                    let r = round(x);
                    fits_i64(r).then_some(r as i64)
                }) {
                    Some(out) => Data::I64(out.into()),
                    None => Data::F64(par::map(v, |&x| round(x)).into()),
                }
            }
            Data::Char(_) | Data::Box(_) => return Err(wrong_type(d.dtype(), span)),
        },
        Inc | Dec => {
            let step = if op == Inc { 1i64 } else { -1 };
            match d {
                Data::Bool(v) => Data::I64(par::map(v, |&b| b as i64 + step).into()),
                Data::I64(v) => match par::try_map(v, |x: i64| x.checked_add(step)) {
                    Some(out) => Data::I64(out.into()),
                    None => Data::F64(par::map(v, |&x| x as f64 + step as f64).into()),
                },
                Data::F64(v) => Data::F64(par::map(v, |&x| x + step as f64).into()),
                Data::Char(_) | Data::Box(_) => return Err(wrong_type(d.dtype(), span)),
            }
        }
        Double | Square => match d {
            Data::Bool(v) => {
                Data::I64(par::map(v, |&b| if op == Double { 2 * b as i64 } else { b as i64 }).into())
            }
            Data::I64(v) => {
                let f = |x: i64| if op == Double { x.checked_mul(2) } else { x.checked_mul(x) };
                match par::try_map(v, f) {
                    Some(out) => Data::I64(out.into()),
                    None => Data::F64(
                        par::map(v, |&x| {
                            let x = x as f64;
                            if op == Double { x + x } else { x * x }
                        })
                        .into(),
                    ),
                }
            }
            Data::F64(v) => {
                Data::F64(par::map(v, |&x| if op == Double { x + x } else { x * x }).into())
            }
            Data::Char(_) | Data::Box(_) => return Err(wrong_type(d.dtype(), span)),
        },
        Halve => {
            let v = as_f64(d, &mut tmp, span)?;
            Data::F64(par::map(v, |&x| x / 2.0).into())
        }
        Pi => {
            let v = as_f64(d, &mut tmp, span)?;
            Data::F64(par::map(v, |&x| std::f64::consts::PI * x).into())
        }
        Factorial => {
            let v = as_f64(d, &mut tmp, span)?;
            Data::F64(par::map(v, |&x| factorial(x)).into())
        }
        Ln => {
            let v = as_f64(d, &mut tmp, span)?;
            if v.iter().any(|&x| x < 0.0) {
                return Err(Error::not_yet("complex numbers", span));
            }
            // ln(0) is negative infinity, which is what J prints as __.
            Data::F64(par::map(v, |&x| x.ln()).into())
        }
        OneMinus => match d {
            Data::Bool(v) => Data::Bool(par::map(v, |&b| 1 - b).into()),
            Data::I64(v) => match par::try_map(v, |x: i64| 1i64.checked_sub(x)) {
                Some(out) => Data::I64(out.into()),
                None => Data::F64(par::map(v, |&x| 1.0 - x as f64).into()),
            },
            Data::F64(v) => Data::F64(par::map(v, |&x| 1.0 - x).into()),
            Data::Char(_) | Data::Box(_) => return Err(wrong_type(d.dtype(), span)),
        },
        Not => {
            let bad = || Error::domain("logical negation needs values of 0 or 1", span);
            match d {
                Data::Bool(v) => Data::Bool(par::map(v, |&b| 1 - b).into()),
                Data::I64(v) => {
                    let out = par::try_map(v, |x: i64| match x {
                        0 => Some(1u8),
                        1 => Some(0u8),
                        _ => None,
                    })
                    .ok_or_else(bad)?;
                    Data::Bool(out.into())
                }
                Data::F64(v) => {
                    let out = par::try_map(v, |x: f64| {
                        if x == 0.0 {
                            Some(1u8)
                        } else if x == 1.0 {
                            Some(0u8)
                        } else {
                            None
                        }
                    })
                    .ok_or_else(bad)?;
                    Data::Bool(out.into())
                }
                Data::Char(_) | Data::Box(_) => return Err(bad()),
            }
        }
    };
    Ok(Array { shape: y.shape.clone(), data })
}

// -------------------------------------------------- structural operations

/// Reverse the axes.
fn transpose_axes(y: &Array) -> Array {
    if y.rank() < 2 {
        return y.clone();
    }
    let out_shape: Vec<usize> = y.shape.iter().rev().copied().collect();
    let src_strides = strides(&y.shape);
    let r = y.rank();
    let n = y.count();
    let mut data = Data::empty(y.dtype());
    let mut coord = vec![0usize; r];
    for _ in 0..n {
        // Output coordinate k indexes source axis r-1-k.
        let idx: usize = (0..r).map(|k| coord[k] * src_strides[r - 1 - k]).sum();
        push_elem(&mut data, &y.data, idx);
        odometer(&mut coord, &out_shape);
    }
    Array::new(out_shape, data)
}

/// J `i.`: an ascending sequence laid out in shape |y|, running backwards
/// along every axis whose given length was negative.
fn iota_j(y: &Array, span: Span) -> Result<Array> {
    if y.rank() > 1 {
        return Err(Error::new(
            ErrorKind::Rank,
            "index generator needs a scalar or vector argument",
            Some(span),
        ));
    }
    let dims = y
        .to_i64_vec()
        .ok_or_else(|| Error::domain("index generator needs integer lengths", span))?;
    let shape: Vec<usize> = dims.iter().map(|d| d.unsigned_abs() as usize).collect();
    let n: usize = shape.iter().product();
    let st = strides(&shape);
    let mut out = Vec::with_capacity(n);
    let mut coord = vec![0usize; shape.len()];
    for _ in 0..n {
        let mut v = 0usize;
        for k in 0..shape.len() {
            let c = if dims[k] < 0 { shape[k] - 1 - coord[k] } else { coord[k] };
            v += c * st[k];
        }
        out.push(v as i64);
        odometer(&mut coord, &shape);
    }
    Ok(Array::new(shape, Data::I64(out.into())))
}

/// The first item, or a cell of fills when there are no items.
fn head(y: &Array) -> Array {
    if y.rank() == 0 {
        return y.clone();
    }
    if y.items() == 0 {
        let cell_shape = y.shape[1..].to_vec();
        let n: usize = cell_shape.iter().product();
        return Array::new(cell_shape, fill_data(y.dtype(), n));
    }
    y.item(0)
}

fn behead(y: &Array, span: Span) -> Result<Array> {
    if y.rank() == 0 {
        return Err(Error::domain("cannot drop the first item of a scalar", span));
    }
    if y.items() == 0 {
        return Ok(y.clone());
    }
    let m = y.item_size();
    let mut shape = y.shape.clone();
    shape[0] -= 1;
    Ok(Array::new(shape, y.data.slice(m, y.count())))
}

/// The last item, or a cell of fills when there are no items.
fn tail(y: &Array) -> Array {
    if y.rank() == 0 {
        return y.clone();
    }
    let n = y.items();
    if n == 0 {
        let cell_shape = y.shape[1..].to_vec();
        let m: usize = cell_shape.iter().product();
        return Array::new(cell_shape, fill_data(y.dtype(), m));
    }
    y.item(n - 1)
}

/// All items but the last. A scalar has one item, so it curtails to empty.
fn curtail(y: &Array) -> Array {
    if y.rank() == 0 {
        return Array::empty(y.dtype());
    }
    let n = y.items();
    if n == 0 {
        return y.clone();
    }
    let m = y.item_size();
    let mut shape = y.shape.clone();
    shape[0] = n - 1;
    Array::new(shape, y.data.slice(0, (n - 1) * m))
}

/// Reverse the items (the leading axis).
fn reverse(y: &Array) -> Array {
    if y.rank() == 0 {
        return y.clone();
    }
    let n = y.items();
    let m = y.item_size();
    let mut data = Data::empty(y.dtype());
    for i in (0..n).rev() {
        for k in 0..m {
            push_elem(&mut data, &y.data, i * m + k);
        }
    }
    Array::new(y.shape.clone(), data)
}

/// `x |. y`: rotate axis k of y left by `x[k]`, cyclically; a negative
/// amount rotates right. A scalar argument has nothing to rotate.
fn rotate(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let counts = axis_counts(x, "rotate", span)?;
    if y.rank() == 0 {
        return Ok(y.clone());
    }
    if counts.len() > y.rank() {
        return Err(Error::new(
            ErrorKind::Length,
            format!(
                "rotate has {} amounts for an argument of rank {}",
                counts.len(),
                y.rank()
            ),
            Some(span),
        ));
    }
    let st = strides(&y.shape);
    let n = y.count();
    let r = y.rank();
    let mut data = Data::empty(y.dtype());
    let mut coord = vec![0usize; r];
    for _ in 0..n {
        let mut idx = 0usize;
        for k in 0..r {
            // No axis is empty here: an empty axis makes n zero.
            let len = y.shape[k] as i64;
            let s = counts.get(k).copied().unwrap_or(0);
            idx += (coord[k] as i64 + s).rem_euclid(len) as usize * st[k];
        }
        push_elem(&mut data, &y.data, idx);
        odometer(&mut coord, &y.shape);
    }
    Ok(Array::new(y.shape.clone(), data))
}

/// A key identifying one element exactly, for equality by hashing. Only
/// comparable within one dtype; the two zeros share a key.
fn elem_key(d: &Data, i: usize) -> u64 {
    match d {
        Data::Bool(v) => v[i] as u64,
        Data::I64(v) => v[i] as u64,
        Data::F64(v) => {
            let x = v[i];
            if x == 0.0 { 0 } else { x.to_bits() }
        }
        Data::Char(v) => v[i] as u64,
        // Boxes have no cheap key; their callers compare them by content.
        Data::Box(_) => 0,
    }
}

/// A key comparable across the numeric dtypes: numbers by their float value,
/// characters by codepoint. Callers keep the two kinds apart.
fn num_key(d: &Data, i: usize) -> u64 {
    match d {
        Data::Bool(v) => (v[i] as f64).to_bits(),
        Data::I64(v) => (v[i] as f64).to_bits(),
        Data::F64(v) => {
            let x = v[i];
            if x == 0.0 { 0.0f64.to_bits() } else { x.to_bits() }
        }
        Data::Char(v) => v[i] as u64,
        // As in `elem_key`: never reached for boxed data.
        Data::Box(_) => 0,
    }
}

/// Distinct items, in the order of their first occurrence.
fn nub(y: &Array) -> Array {
    if y.rank() == 0 {
        return Array::new(vec![1], y.data.clone());
    }
    let n = y.items();
    let m = y.item_size();
    let mut keep = Vec::new();
    if y.dtype() == DType::Box {
        // Boxed items are compared by content, one against the ones kept
        // so far: there is no key to hash.
        for i in 0..n {
            if !keep.iter().any(|&j| arrays_match(&y.item(i), &y.item(j))) {
                keep.push(i);
            }
        }
    } else {
        let mut seen: HashSet<Vec<u64>> = HashSet::with_capacity(n);
        for i in 0..n {
            let key: Vec<u64> = (0..m).map(|k| elem_key(&y.data, i * m + k)).collect();
            if seen.insert(key) {
                keep.push(i);
            }
        }
    }
    let mut data = Data::empty(y.dtype());
    for &i in &keep {
        for k in 0..m {
            push_elem(&mut data, &y.data, i * m + k);
        }
    }
    let mut shape = y.shape.clone();
    shape[0] = keep.len();
    Array::new(shape, data)
}

/// Compare items `i` and `j` (of `m` elements each) elementwise, left to
/// right. Characters order by codepoint; a NaN compares equal to anything,
/// which keeps the sort total.
fn cmp_items(d: &Data, i: usize, j: usize, m: usize) -> std::cmp::Ordering {
    use std::cmp::Ordering::Equal;
    let (a, b) = (i * m, j * m);
    let ord = |k: usize| match d {
        Data::Bool(v) => v[a + k].cmp(&v[b + k]),
        Data::I64(v) => v[a + k].cmp(&v[b + k]),
        Data::F64(v) => v[a + k].partial_cmp(&v[b + k]).unwrap_or(Equal),
        Data::Char(v) => v[a + k].cmp(&v[b + k]),
        // Grading a boxed array is refused before it gets here.
        Data::Box(_) => Equal,
    };
    (0..m).map(ord).find(|o| *o != Equal).unwrap_or(Equal)
}

/// The stable permutation that sorts the items of `y`.
fn grade_order(y: &Array, down: bool) -> Vec<usize> {
    if y.rank() == 0 {
        return vec![0];
    }
    let n = y.items();
    let m = y.item_size();
    let mut idx: Vec<usize> = (0..n).collect();
    // A stable sort leaves equal items in their original order, which is
    // what both languages promise, ascending and descending alike.
    if down {
        idx.sort_by(|&a, &b| cmp_items(&y.data, b, a, m));
    } else {
        idx.sort_by(|&a, &b| cmp_items(&y.data, a, b, m));
    }
    idx
}

/// Select items of `y` in the given order.
fn select_items(y: &Array, order: &[usize]) -> Array {
    let m = y.item_size();
    let mut data = Data::empty(y.dtype());
    for &i in order {
        for k in 0..m {
            push_elem(&mut data, &y.data, i * m + k);
        }
    }
    let mut shape = y.shape.clone();
    shape[0] = order.len();
    Array::new(shape, data)
}

/// `x /: y` / `x \: y`: x's items reordered by the grade of y's items.
/// Ordering boxes needs J's total array ordering, which libjay does not
/// implement yet; sorting boxed items BY something else works.
fn no_grading_boxes(y: &Array, span: Span) -> Result<()> {
    if y.dtype() == DType::Box {
        return Err(Error::not_yet("grading boxed arrays (the total array ordering)", span));
    }
    Ok(())
}

fn grade_select(x: &Array, y: &Array, down: bool, span: Span) -> Result<Array> {
    no_grading_boxes(y, span)?;
    let order = grade_order(y, down);
    if x.items() != order.len() {
        return Err(Error::new(
            ErrorKind::Length,
            format!(
                "sorting {} items by a key of {} items",
                x.items(),
                order.len()
            ),
            Some(span),
        ));
    }
    if x.rank() == 0 {
        return Ok(x.clone());
    }
    Ok(select_items(x, &order))
}

/// Whole-array equality: same shape and same values. Characters never equal
/// numbers; `1` equals `1.0`; NaN equals nothing.
fn arrays_match(x: &Array, y: &Array) -> bool {
    if x.shape != y.shape {
        return false;
    }
    // Two empty arrays of the same shape match whatever their types are,
    // which is what both references answer for `'' -: i. 0`.
    if x.count() == 0 {
        return true;
    }
    if let (Data::Box(a), Data::Box(b)) = (&x.data, &y.data) {
        return a.iter().zip(b.iter()).all(|(p, q)| arrays_match(p, q));
    }
    let (dx, dy) = (x.dtype(), y.dtype());
    match DType::promote(dx, dy) {
        None => false,
        Some(DType::Char) => match (&x.data, &y.data) {
            (Data::Char(a), Data::Char(b)) => a.as_slice() == b.as_slice(),
            _ => false,
        },
        Some(DType::F64) => {
            let (mut ta, mut tb) = (Vec::new(), Vec::new());
            let a = borrow_f64(&x.data, &mut ta);
            let b = borrow_f64(&y.data, &mut tb);
            a.iter().zip(b).all(|(p, q)| p == q)
        }
        Some(_) => {
            let (mut ta, mut tb) = (Vec::new(), Vec::new());
            let a = borrow_i64(&x.data, &mut ta);
            let b = borrow_i64(&y.data, &mut tb);
            a.iter().zip(b).all(|(p, q)| p == q)
        }
    }
}

/// Item `i` of `a`, treating a scalar as an array of one item.
fn item_or_self(a: &Array, i: usize) -> Array {
    if a.rank() == 0 { a.clone() } else { a.item(i) }
}

/// `x e. y`: for every cell of x shaped like an item of y, is it an item
/// of y? A cell of the wrong shape simply is not one, as in J.
fn member_j(x: &Array, y: &Array) -> Array {
    let cell_rank = y.rank().saturating_sub(1).min(x.rank());
    let frame_rank = x.rank() - cell_rank;
    let frame: Vec<usize> = x.shape[..frame_rank].to_vec();
    let nf: usize = frame.iter().product();
    let items = y.items();
    let mut out = Vec::with_capacity(nf);
    for i in 0..nf {
        let cell = x.cell_at(frame_rank, i);
        out.push((0..items).any(|j| arrays_match(&cell, &item_or_self(y, j))) as u8);
    }
    Array::new(frame, Data::Bool(out.into()))
}

/// `x ∊ y`: for every element of x, does that value occur anywhere in y?
fn member_apl(x: &Array, y: &Array) -> Array {
    let n = x.count();
    if x.dtype() == DType::Box || y.dtype() == DType::Box {
        // The elements are whole arrays, so they are compared by content;
        // a box never equals a plain number or character.
        let out: Vec<u8> = (0..n)
            .map(|i| {
                let e = atom(x, i);
                u8::from((0..y.count()).any(|j| arrays_match(&e, &atom(y, j))))
            })
            .collect();
        return Array::new(x.shape.clone(), Data::Bool(out.into()));
    }
    if (x.dtype() == DType::Char) != (y.dtype() == DType::Char) {
        return Array::new(x.shape.clone(), Data::Bool(vec![0u8; n].into()));
    }
    let seen: HashSet<u64> = (0..y.count()).map(|i| num_key(&y.data, i)).collect();
    let out: Vec<u8> =
        (0..n).map(|i| seen.contains(&num_key(&x.data, i)) as u8).collect();
    Array::new(x.shape.clone(), Data::Bool(out.into()))
}

/// `x i. y` / `x ⍳ y`: where each cell of y sits among the items of x.
fn index_of(x: &Array, y: &Array, origin: i64) -> Array {
    let cell_rank = x.rank().saturating_sub(1).min(y.rank());
    let frame_rank = y.rank() - cell_rank;
    let frame: Vec<usize> = y.shape[..frame_rank].to_vec();
    let nf: usize = frame.iter().product();
    let items = x.items();
    let mut out = Vec::with_capacity(nf);
    for i in 0..nf {
        let cell = y.cell_at(frame_rank, i);
        let at = (0..items)
            .find(|&j| arrays_match(&cell, &item_or_self(x, j)))
            .unwrap_or(items);
        out.push(origin + at as i64);
    }
    Array::new(frame, Data::I64(out.into()))
}

/// `x { y` for one index atom: the rank machinery supplies the framing.
fn from_index(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let idx = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("index must be an integer", span))?;
    let Some(&i) = idx.first() else {
        return Err(Error::internal("from_index with no index"));
    };
    let n = y.items() as i64;
    let k = if i < 0 { i + n } else { i };
    if k < 0 || k >= n {
        return Err(Error::domain(
            format!("index {i} is out of range: the argument has {n} items"),
            span,
        ));
    }
    Ok(item_or_self(y, k as usize))
}

/// Bring `a` up to `rank` axes for catenation along `axis`. A scalar spreads
/// over one cross section of the other argument; one missing axis becomes a
/// length-1 axis at `axis`.
fn cat_promote(a: &Array, other: &Array, rank: usize, axis: usize, span: Span) -> Result<Array> {
    if a.rank() == rank {
        return Ok(a.clone());
    }
    if a.rank() == 0 {
        let mut shape =
            if other.rank() == rank { other.shape.clone() } else { vec![1usize; rank] };
        shape[axis] = 1;
        let n: usize = shape.iter().product();
        let mut data = Data::empty(a.dtype());
        for _ in 0..n {
            push_elem(&mut data, &a.data, 0);
        }
        return Ok(Array::new(shape, data));
    }
    if a.rank() + 1 == rank {
        let mut shape = a.shape.clone();
        shape.insert(axis, 1);
        return Ok(Array::new(shape, a.data.clone()));
    }
    Err(Error::new(
        ErrorKind::Rank,
        format!("cannot catenate rank {} with rank {}", a.rank(), other.rank()),
        Some(span),
    ))
}

/// Catenate along the leading or the last axis.
fn catenate(x: &Array, y: &Array, leading: bool, span: Span) -> Result<Array> {
    let rank = x.rank().max(y.rank()).max(1);
    let axis = if leading { 0 } else { rank - 1 };
    let xa = cat_promote(x, y, rank, axis, span)?;
    let ya = cat_promote(y, x, rank, axis, span)?;
    for k in 0..rank {
        if k != axis && xa.shape[k] != ya.shape[k] {
            return Err(Error::not_yet("catenate with fill", span));
        }
    }
    let dt = DType::promote(xa.dtype(), ya.dtype()).ok_or_else(|| {
        let boxed = xa.dtype() == DType::Box || ya.dtype() == DType::Box;
        let what = if boxed {
            "cannot catenate boxed and unboxed data; box the other side first"
        } else {
            "cannot catenate character and numeric data"
        };
        Error::new(ErrorKind::Type, what, Some(span))
    })?;
    let widen = |a: &Array| -> Result<Data> {
        if a.dtype() == dt {
            Ok(a.data.clone())
        } else {
            a.data.cast(dt).ok_or_else(|| Error::internal("unsupported widening in catenate"))
        }
    };
    let xd = widen(&xa)?;
    let yd = widen(&ya)?;
    let outer: usize = xa.shape[..axis].iter().product();
    let ix: usize = xa.shape[axis..].iter().product();
    let iy: usize = ya.shape[axis..].iter().product();
    let mut data = Data::empty(dt);
    for o in 0..outer {
        for k in 0..ix {
            push_elem(&mut data, &xd, o * ix + k);
        }
        for k in 0..iy {
            push_elem(&mut data, &yd, o * iy + k);
        }
    }
    let mut shape = xa.shape.clone();
    shape[axis] = xa.shape[axis] + ya.shape[axis];
    Ok(Array::new(shape, data))
}

/// `x # y`: item i of y appears x[i] times. Only a scalar x is extended to
/// every item — a one-element vector is a length error, as in J.
fn copy_items(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let counts = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("replication counts must be integers", span))?;
    if counts.iter().any(|&c| c < 0) {
        return Err(Error::domain("replication counts must be nonnegative", span));
    }
    let n = y.items();
    let m = y.item_size();
    let per = if x.rank() == 0 { vec![counts[0]; n] } else { counts };
    if per.len() != n {
        return Err(Error::new(
            ErrorKind::Length,
            format!("{} replication count(s) for {n} item(s)", per.len()),
            Some(span),
        ));
    }
    let total: usize = per.iter().map(|&c| c as usize).sum();
    let mut data = Data::empty(y.dtype());
    for (i, &c) in per.iter().enumerate() {
        for _ in 0..c {
            for k in 0..m {
                push_elem(&mut data, &y.data, i * m + k);
            }
        }
    }
    // A scalar argument has one item, so replicating it yields a vector.
    let mut shape = if y.rank() == 0 { vec![1] } else { y.shape.clone() };
    shape[0] = total;
    Ok(Array::new(shape, data))
}

/// `": y` / `⍕ y`: the argument as the characters that display it.
///
/// Characters are already their own display, so they pass through unchanged.
/// Anything else is laid out exactly as the session would print it: a rank-0
/// or rank-1 argument gives one character vector, and a higher-rank one gives
/// the display's lines as the rows of a character array of the same rank —
/// column widths span the whole argument, so every line has one width and the
/// planes stay aligned with each other.
fn format_chars(y: &Array, opts: &FmtOpts) -> Array {
    if y.dtype() == DType::Char {
        return y.clone();
    }
    // An empty argument has nothing to lay out; J keeps its shape.
    if y.count() == 0 {
        return Array::new(y.shape.clone(), Data::empty(DType::Char));
    }
    let text = crate::fmt::format_array(y, opts);
    if y.dtype() == DType::Box {
        // A fenced box (J) takes several lines per row of cells, so the
        // display's own rows and columns become the last two axes of the
        // result. A spaced one (APL) still prints one line per row, and
        // keeps the plain rule below.
        let lines = text.lines().filter(|l| !l.is_empty()).count();
        let rows: usize =
            if y.rank() == 0 { 1 } else { y.shape[..y.rank() - 1].iter().product() };
        if lines != rows {
            return text_planes(&text, &y.shape[..y.rank().saturating_sub(2)]);
        }
    }
    if y.rank() < 2 {
        let chars: Vec<char> = text.chars().collect();
        return Array::new(vec![chars.len()], Data::Char(chars.into()));
    }
    // The blank lines are the plane separators, which the array does not
    // carry: its own shape already says where the planes are.
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let mut chars: Vec<char> = Vec::with_capacity(lines.len() * width);
    for line in &lines {
        chars.extend(line.chars());
        chars.resize(chars.len() + width - line.chars().count(), ' ');
    }
    // One line per row of the display: the argument's shape with its last
    // axis replaced by the line width.
    let mut shape = y.shape[..y.rank() - 1].to_vec();
    shape.push(width);
    debug_assert_eq!(lines.len(), shape[..shape.len() - 1].iter().product::<usize>());
    Array::new(shape, Data::Char(chars.into()))
}

/// A multi-line display as a character array: the frame, then the lines of
/// one plane, then their common width.
fn text_planes(text: &str, frame: &[usize]) -> Array {
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let planes: usize = frame.iter().product::<usize>().max(1);
    let per = lines.len() / planes;
    let mut chars: Vec<char> = Vec::with_capacity(lines.len() * width);
    for line in &lines {
        chars.extend(line.chars());
        chars.resize(chars.len() + width - line.chars().count(), ' ');
    }
    let mut shape = frame.to_vec();
    shape.push(per);
    shape.push(width);
    Array::new(shape, Data::Char(chars.into()))
}

/// Numeric data as f64, refusing characters.
fn digits_of(a: &Array, what: &str, span: Span) -> Result<Vec<f64>> {
    a.to_f64_vec().ok_or_else(|| Error::domain(format!("{what} needs numeric data"), span))
}

/// Narrow a finished digit or value buffer back to integers when the inputs
/// were whole and nothing left the exact range, which is what both languages
/// do with integer arguments.
fn narrow(values: Vec<f64>, integral: bool) -> Data {
    if integral && values.iter().all(|&v| v.fract() == 0.0 && fits_i64(v)) {
        return Data::I64(values.iter().map(|&v| v as i64).collect::<Vec<_>>().into());
    }
    Data::F64(values.into())
}

/// True when the array holds whole numbers only.
fn is_integral(a: &Array) -> bool {
    !matches!(a.dtype(), DType::F64 | DType::Char)
}

/// `x #. y` / `x ⊥ y`: the digits y read in the radices x. A scalar x is the
/// radix of every position; otherwise the two have the same length.
fn decode(x: Option<&Array>, y: &Array, span: Span) -> Result<Array> {
    let digits = digits_of(y, "decode", span)?;
    let radix: Vec<f64> = match x {
        None => vec![2.0; digits.len()],
        Some(x) => {
            let r = digits_of(x, "decode", span)?;
            match r.len() {
                1 => vec![r[0]; digits.len()],
                n if n == digits.len() => r,
                n => {
                    return Err(Error::new(
                        ErrorKind::Length,
                        format!("{n} radices for {} digits", digits.len()),
                        Some(span),
                    ));
                }
            }
        }
    };
    let mut acc = 0.0f64;
    for (d, b) in digits.iter().zip(&radix) {
        acc = acc * b + d;
    }
    let integral = is_integral(y) && x.is_none_or(is_integral);
    Ok(Array::new(vec![], narrow(vec![acc], integral)))
}

/// The number of binary digits `#: y` uses: enough for the largest magnitude
/// in the whole argument, and never fewer than one.
fn bit_width(values: &[f64], span: Span) -> Result<usize> {
    // Nothing to encode needs no digits at all: `$ #: i. 0` is `0 0`.
    if values.is_empty() {
        return Ok(0);
    }
    let mut m = 0.0f64;
    for &v in values {
        if !v.is_finite() {
            return Err(Error::domain("cannot encode an infinite value", span));
        }
        m = m.max(v.abs());
    }
    let whole = m.floor();
    if whole >= 1e15 {
        return Err(Error::domain("the value is too large to encode in binary", span));
    }
    let mut w = 1usize;
    let mut n = whole as i64;
    while n > 1 {
        n /= 2;
        w += 1;
    }
    Ok(w)
}

/// One value written in the radices `radix`, most significant first. A radix
/// of 0 takes whatever is left, which is how both languages spell "and the
/// rest".
fn encode_one(radix: &[f64], v: f64, out: &mut [f64]) {
    let mut rem = v;
    for i in (0..radix.len()).rev() {
        let b = radix[i];
        if b == 0.0 {
            out[i] = rem;
            rem = 0.0;
        } else {
            let r = rem - b * (rem / b).floor();
            out[i] = r;
            rem = (rem - r) / b;
        }
    }
}

/// `x #: y` / `x ⊤ y`: the digits become the LEADING axis, so the result has
/// shape `(#x), $y`. J applies this per atom of y (right rank 0) and APL to
/// the whole of it (right rank infinite); the operation itself is the same.
fn encode(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let radix = digits_of(x, "encode", span)?;
    let values = digits_of(y, "encode", span)?;
    let k = radix.len();
    let n = values.len();
    let mut out = vec![0.0f64; k * n];
    let mut cell = vec![0.0f64; k];
    for (j, &v) in values.iter().enumerate() {
        encode_one(&radix, v, &mut cell);
        for i in 0..k {
            out[i * n + j] = cell[i];
        }
    }
    // The digit axis is x's own shape: a scalar radix adds no axis at all,
    // which is why `2 #: 5` is a scalar and `2 2 #: 5` is a two-element list.
    let mut shape = if x.rank() == 0 { Vec::new() } else { vec![k] };
    shape.extend_from_slice(&y.shape);
    Ok(Array::new(shape, narrow(out, is_integral(x) && is_integral(y))))
}

/// `#: y`: base-2 encode of the whole argument, the digits trailing.
fn encode_bits(y: &Array, span: Span) -> Result<Array> {
    let values = digits_of(y, "encode", span)?;
    let k = bit_width(&values, span)?;
    let radix = vec![2.0; k];
    let mut out = vec![0.0f64; values.len() * k];
    for (j, &v) in values.iter().enumerate() {
        encode_one(&radix, v, &mut out[j * k..(j + 1) * k]);
    }
    let mut shape = y.shape.clone();
    shape.push(k);
    Ok(Array::new(shape, narrow(out, is_integral(y))))
}

/// `x ,: y`: the two arguments as the items of a new leading axis. A scalar
/// spreads over the other argument's shape, and two scalars become
/// one-element lists (`1 ,: 2` has shape 2 1); otherwise the framing
/// machinery's own fill brings the two cells to a common shape.
fn laminate(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let spread = |a: &Array, other: &Array| -> Array {
        if a.rank() != 0 {
            return a.clone();
        }
        let shape = if other.rank() == 0 { vec![1] } else { other.shape.clone() };
        let n: usize = shape.iter().product();
        let mut data = Data::empty(a.dtype());
        for _ in 0..n {
            push_elem(&mut data, &a.data, 0);
        }
        Array::new(shape, data)
    };
    assemble(&[2], vec![spread(x, y), spread(y, x)], span)
}

/// `⍪ y`: one row per item, holding that item's elements.
fn table_of(y: &Array) -> Array {
    let shape = match y.rank() {
        0 => vec![1, 1],
        _ => vec![y.items(), y.item_size()],
    };
    Array::new(shape, y.data.clone())
}

/// `x u/ y`: u applied to every pair of cells, x's frame before y's.
///
/// The cells are the ones u's own ranks ask for, which is why `1 2 3 +/ 10 20`
/// is a 3-by-2 table (atoms both sides) while `x ,/ y` is a single catenation
/// (`,` takes its arguments whole).
fn table(u: &Verb, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let ranks = u.ranks();
    let fxl = x.rank() - effective_rank(ranks[1], x.rank());
    let fyl = y.rank() - effective_rank(ranks[2], y.rank());
    let mut frame = x.shape[..fxl].to_vec();
    frame.extend_from_slice(&y.shape[..fyl]);
    let nx: usize = x.shape[..fxl].iter().product();
    let ny: usize = y.shape[..fyl].iter().product();
    let n = nx * ny;
    if n == 0 {
        return assemble(&frame, Vec::new(), span);
    }
    if frame.is_empty() {
        return u.dyad(x, y, ctx, span);
    }
    let work = x.count().max(y.count()).max(n);
    let cells = each_cell(n, work, u.is_pure(), ctx, |i, c| {
        u.dyad(&x.cell_at(fxl, i / ny), &y.cell_at(fyl, i % ny), c, span)
    })?;
    assemble(&frame, cells, span)
}

/// Monadic meaning of a primitive, applied to one cell.
fn monad_op(p: &Prim, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    match p.monad {
        MonadOp::Scalar(op) => scalar_monad(op, y, span),
        MonadOp::ShapeOf => Ok(Array::from_i64(y.shape.iter().map(|&n| n as i64).collect())),
        MonadOp::Tally => Ok(Array::scalar_i64(y.items() as i64)),
        MonadOp::Ravel => Ok(Array::new(vec![y.count()], y.data.clone())),
        MonadOp::TransposeAxes => Ok(transpose_axes(y)),
        MonadOp::Head => Ok(head(y)),
        MonadOp::Behead => behead(y, span),
        MonadOp::Tail => Ok(tail(y)),
        MonadOp::Curtail => Ok(curtail(y)),
        MonadOp::Reverse => Ok(reverse(y)),
        MonadOp::Nub => Ok(nub(y)),
        MonadOp::GradeUp { origin } | MonadOp::GradeDown { origin } => {
            no_grading_boxes(y, span)?;
            let down = matches!(p.monad, MonadOp::GradeDown { .. });
            let order = grade_order(y, down);
            Ok(Array::from_i64(order.iter().map(|&i| origin + i as i64).collect()))
        }
        MonadOp::IotaJ => iota_j(y, span),
        MonadOp::IotaApl { origin } => {
            if y.rank() != 0 {
                return Err(Error::domain("index generator needs a scalar argument", span));
            }
            let n = y
                .to_i64_vec()
                .ok_or_else(|| Error::domain("index generator needs an integer argument", span))?
                [0];
            if n < 0 {
                return Err(Error::domain("index generator needs a nonnegative count", span));
            }
            Ok(Array::from_i64((0..n).map(|i| origin + i).collect()))
        }
        MonadOp::Echo => {
            (ctx.out)(&format!("{}\n", crate::fmt::format_array(y, &ctx.cfg.fmt)));
            Ok(Array::empty(DType::I64))
        }
        MonadOp::Same => Ok(y.clone()),
        MonadOp::Format => Ok(format_chars(y, &ctx.cfg.fmt)),
        MonadOp::DecodeBits => decode(None, y, span),
        MonadOp::EncodeBits => encode_bits(y, span),
        MonadOp::Itemize => {
            let mut shape = vec![1usize];
            shape.extend_from_slice(&y.shape);
            Ok(Array::new(shape, y.data.clone()))
        }
        MonadOp::TableOf => Ok(table_of(y)),
        MonadOp::Enclose(rule) => Ok(enclose(y, rule)),
        MonadOp::Open => Ok(open_cell(y)),
        MonadOp::Raze => raze(y, span),
        MonadOp::First => Ok(first(y)),
        MonadOp::Enlist => enlist(y, span),
        MonadOp::Depth => Ok(Array::scalar_i64(depth(y))),
        MonadOp::NotYet(what) => Err(Error::not_yet(what, span)),
        MonadOp::None => {
            Err(Error::domain(format!("{} has no monadic meaning", p.name), span))
        }
    }
}

/// Left argument of reshape/take/drop: a scalar or vector of integers.
fn axis_counts(x: &Array, what: &str, span: Span) -> Result<Vec<i64>> {
    if x.rank() > 1 {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("{what} needs a scalar or vector left argument"),
            Some(span),
        ));
    }
    x.to_i64_vec()
        .ok_or_else(|| Error::domain(format!("{what} needs integer lengths"), span))
}

fn reshape(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let dims = axis_counts(x, "reshape", span)?;
    if dims.iter().any(|&d| d < 0) {
        return Err(Error::domain("reshape lengths must be nonnegative", span));
    }
    let shape: Vec<usize> = dims.iter().map(|&d| d as usize).collect();
    let n: usize = shape.iter().product();
    let src = y.count();
    if n > 0 && src == 0 {
        return Err(Error::new(
            ErrorKind::Length,
            "reshape of an empty array",
            Some(span),
        ));
    }
    let mut data = Data::empty(y.dtype());
    for i in 0..n {
        push_elem(&mut data, &y.data, i % src);
    }
    Ok(Array::new(shape, data))
}

/// A take or drop that only touches the leading axis moves a run of whole
/// items, which is a slice of the buffer rather than an element-by-element
/// walk. `keep` is the items to end up with, `from` the first of them.
fn leading_run(y: &Array, counts: &[i64], drop: bool) -> Option<Array> {
    if y.rank() == 0 || counts.is_empty() || counts[1..].iter().any(|&c| c != 0) {
        return None;
    }
    let n = y.items();
    let k = counts[0];
    let a = k.unsigned_abs() as usize;
    let (lo, keep) = if drop {
        let a = a.min(n);
        if k >= 0 { (a, n - a) } else { (0, n - a) }
    } else {
        // An overtake has to produce fills, which is not a slice.
        if a > n {
            return None;
        }
        if k >= 0 { (0, a) } else { (n - a, a) }
    };
    Some(section(y, lo, lo + keep))
}

fn take(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let counts = axis_counts(x, "take", span)?;
    let promoted;
    // A scalar right argument is treated as a one-item vector.
    let base = if y.rank() == 0 {
        promoted = Array::new(vec![1], y.data.clone());
        &promoted
    } else {
        y
    };
    if counts.len() > base.rank() {
        return Err(Error::not_yet("take with more axes than the rank", span));
    }
    if let Some(run) = leading_run(base, &counts, false) {
        return Ok(run);
    }
    let mut out_shape = base.shape.clone();
    for (a, &k) in counts.iter().enumerate() {
        out_shape[a] = k.unsigned_abs() as usize;
    }
    let n: usize = out_shape.iter().product();
    let st = strides(&base.shape);
    let mut data = Data::empty(base.dtype());
    let mut coord = vec![0usize; out_shape.len()];
    for _ in 0..n {
        let mut idx = 0usize;
        let mut inside = true;
        for a in 0..out_shape.len() {
            let len = base.shape[a] as i64;
            let c = coord[a] as i64;
            // Positive takes from the front and overtakes at the back;
            // negative takes from the back and overtakes at the front.
            let s = match counts.get(a) {
                Some(&k) if k < 0 => c + len - k.unsigned_abs() as i64,
                _ => c,
            };
            if s < 0 || s >= len {
                inside = false;
                break;
            }
            idx += s as usize * st[a];
        }
        if inside {
            push_elem(&mut data, &base.data, idx);
        } else {
            data.push_fill();
        }
        odometer(&mut coord, &out_shape);
    }
    Ok(Array::new(out_shape, data))
}

fn drop_(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let counts = axis_counts(x, "drop", span)?;
    let promoted;
    let base = if y.rank() == 0 {
        promoted = Array::new(vec![1], y.data.clone());
        &promoted
    } else {
        y
    };
    if counts.len() > base.rank() {
        return Err(Error::not_yet("drop with more axes than the rank", span));
    }
    if let Some(run) = leading_run(base, &counts, true) {
        return Ok(run);
    }
    let mut out_shape = base.shape.clone();
    let mut offset = vec![0usize; base.rank()];
    for (a, &k) in counts.iter().enumerate() {
        let len = base.shape[a];
        let d = (k.unsigned_abs() as usize).min(len);
        out_shape[a] = len - d;
        if k > 0 {
            offset[a] = d;
        }
    }
    let n: usize = out_shape.iter().product();
    let st = strides(&base.shape);
    let mut data = Data::empty(base.dtype());
    let mut coord = vec![0usize; out_shape.len()];
    for _ in 0..n {
        let idx: usize = (0..out_shape.len()).map(|a| (coord[a] + offset[a]) * st[a]).sum();
        push_elem(&mut data, &base.data, idx);
        odometer(&mut coord, &out_shape);
    }
    Ok(Array::new(out_shape, data))
}

/// Dyadic meaning of a primitive, applied to one pair of cells.
fn dyad_op(p: &Prim, x: &Array, y: &Array, mode: Agreement, span: Span) -> Result<Array> {
    match p.dyad {
        // Reached only when a scalar verb is given non-zero cell ranks; the
        // cells then agree among themselves.
        DyadOp::Scalar(op) => scalar_dyad(op, x, y, mode, span),
        DyadOp::Reshape => reshape(x, y, span),
        DyadOp::Take => take(x, y, span),
        DyadOp::Drop => drop_(x, y, span),
        DyadOp::Right => Ok(y.clone()),
        DyadOp::Left => Ok(x.clone()),
        DyadOp::Rotate => rotate(x, y, span),
        DyadOp::AppendLeading => catenate(x, y, true, span),
        DyadOp::AppendLast => catenate(x, y, false, span),
        DyadOp::IndexOf { origin } => Ok(index_of(x, y, origin)),
        DyadOp::MemberJ => Ok(member_j(x, y)),
        DyadOp::MemberApl => Ok(member_apl(x, y)),
        DyadOp::From => from_index(x, y, span),
        DyadOp::Match => Ok(Array::scalar_bool(arrays_match(x, y))),
        DyadOp::NotMatch => Ok(Array::scalar_bool(!arrays_match(x, y))),
        DyadOp::GradeSelect { down } => grade_select(x, y, down, span),
        DyadOp::Copy => copy_items(x, y, span),
        DyadOp::Decode => decode(Some(x), y, span),
        DyadOp::Encode => encode(x, y, span),
        DyadOp::Laminate => laminate(x, y, span),
        DyadOp::Link => link(x, y, span),
        DyadOp::Strand => strand(x, y, span),
        DyadOp::NotYet(what) => Err(Error::not_yet(what, span)),
        DyadOp::None => Err(Error::domain(format!("{} has no dyadic meaning", p.name), span)),
    }
}

// ------------------------------------------------------------- reduction

/// The neutral cell of a reduction over no items, if the verb has one.
///
/// The values are the ones the references produce — both of them, for every
/// verb both spell (`x %: y` is J's alone). Where a table entry is
/// conventional rather than algebraic (a comparison has no true identity)
/// J and GNU APL still agree on it, so libjay follows. The two exceptions
/// are `⌊` and `⌈`: J's neutral cells are the infinities and GNU APL's are
/// the largest representable magnitudes — libjay takes J's, and the
/// difference is recorded in docs/coverage.md.
fn reduce_identity(v: &Verb, n: usize) -> Option<Data> {
    let Verb::Prim(p) = v else { return None };
    let DyadOp::Scalar(op) = p.dyad else { return None };
    let ints = |k: i64| Data::I64(vec![k; n].into());
    let bits = |k: u8| Data::Bool(vec![k; n].into());
    Some(match op {
        ScalarDyad::Add | ScalarDyad::Sub | ScalarDyad::Gcd | ScalarDyad::Residue => ints(0),
        ScalarDyad::Mul
        | ScalarDyad::DivJ
        | ScalarDyad::DivApl
        | ScalarDyad::Pow
        | ScalarDyad::Lcm
        | ScalarDyad::Root
        | ScalarDyad::Binomial => ints(1),
        ScalarDyad::Min => Data::F64(vec![f64::INFINITY; n].into()),
        ScalarDyad::Max => Data::F64(vec![f64::NEG_INFINITY; n].into()),
        ScalarDyad::Eq | ScalarDyad::Le | ScalarDyad::Ge => bits(1),
        ScalarDyad::Ne | ScalarDyad::Lt | ScalarDyad::Gt => bits(0),
        // Logarithm and the circle functions have none: both references
        // refuse an empty reduction of them.
        ScalarDyad::Log | ScalarDyad::Circle => return None,
    })
}

/// Of the operations the typed fold covers, the ones whose reduction may be
/// regrouped: folding the items in chunks and combining the chunks gives the
/// same result, exactly for integers and to within the tolerance the float
/// contract allows (§5.9). LCM and GCD associate too but reduce through the
/// general path, which carries their type rules.
fn is_associative(op: ScalarDyad) -> bool {
    use ScalarDyad::*;
    matches!(op, Add | Mul | Min | Max)
}

/// Fold items `lo .. hi` into `acc`, right to left, taking only the columns
/// that start at `j0` — `acc.len()` of them. False when a step left the
/// element type; the accumulator is then meaningless.
#[inline]
fn fold_range<T, F>(v: &[T], m: usize, lo: usize, hi: usize, j0: usize, acc: &mut [T], step: &F) -> bool
where
    T: Copy,
    F: Fn(T, T) -> (T, bool),
{
    let w = acc.len();
    let base = (hi - 1) * m + j0;
    acc.copy_from_slice(&v[base..base + w]);
    // Overflow is folded into a flag rather than breaking the loop: the
    // whole reduction is redone by the general path either way.
    let mut over = false;
    for i in (lo..hi - 1).rev() {
        let row = &v[i * m + j0..i * m + j0 + w];
        for (slot, &x) in acc.iter_mut().zip(row) {
            let (r, o) = step(x, *slot);
            *slot = r;
            over |= o;
        }
    }
    !over
}

/// Fold `n` single-element items, right to left. Associative steps fold in
/// chunks on several threads.
fn fold_flat<T, F>(v: &[T], n: usize, assoc: bool, step: &F) -> Option<T>
where
    T: Copy + Send + Sync,
    F: Fn(T, T) -> (T, bool) + Sync + Send,
{
    if assoc && par::worth_it(n) {
        return par::try_fold_chunks(&v[..n], |a, b| {
            let (r, o) = step(a, b);
            (!o).then_some(r)
        });
    }
    let mut acc = v[n - 1];
    let mut over = false;
    for &x in v[..n - 1].iter().rev() {
        let (r, o) = step(x, acc);
        acc = r;
        over |= o;
    }
    (!over).then_some(acc)
}

/// Fold the `n` items of a flat buffer into one item of `m` elements, right
/// to left. None when a step left the element type (integer overflow): the
/// caller then re-folds through the general path, which knows how to widen.
///
/// Three shapes, each yielding what one sequential pass would:
/// * a wide item splits into ranges of columns, and every element folds its
///   own column in order, so any step at all is safe;
/// * a one-element item folds in a register;
/// * a narrow item splits into chunks of items, which regroups the fold and
///   is taken only for an associative step.
fn fold_items<T, F>(v: &[T], n: usize, m: usize, assoc: bool, step: F) -> Option<Vec<T>>
where
    T: Copy + Default + Send + Sync,
    F: Fn(T, T) -> (T, bool) + Sync + Send,
{
    if m >= par::WIDE_ITEM {
        let (out, ok) = par::fill_wide(m, n * m, |j0, acc: &mut [T]| {
            fold_range(v, m, 0, n, j0, acc, &step)
        });
        return ok.then_some(out);
    }
    if m == 1 {
        return fold_flat(v, n, assoc, &step).map(|x| vec![x]);
    }
    let chunks = if assoc { par::chunks(n, n * m) } else { 1 };
    if chunks < 2 {
        let mut acc = vec![T::default(); m];
        return fold_range(v, m, 0, n, 0, &mut acc, &step).then_some(acc);
    }
    let per = n.div_ceil(chunks);
    let parts = par::map_indexed(n.div_ceil(per), |c| {
        let mut acc = vec![T::default(); m];
        let ok = fold_range(v, m, c * per, ((c + 1) * per).min(n), 0, &mut acc, &step);
        ok.then_some(acc)
    });
    // The chunk results combine right to left, the order the chunks
    // themselves were folded in.
    let mut it = parts.into_iter().rev();
    let mut acc = it.next()??;
    for part in it {
        let part = part?;
        let mut over = false;
        for (slot, &x) in acc.iter_mut().zip(&part) {
            let (r, o) = step(x, *slot);
            *slot = r;
            over |= o;
        }
        if over {
            return None;
        }
    }
    Some(acc)
}

fn fold_i64(op: ScalarDyad, v: &[i64], n: usize, m: usize) -> Option<Vec<i64>> {
    use ScalarDyad::*;
    let assoc = is_associative(op);
    match op {
        Add => fold_items(v, n, m, assoc, i64::overflowing_add),
        Sub => fold_items(v, n, m, assoc, i64::overflowing_sub),
        Mul => fold_items(v, n, m, assoc, i64::overflowing_mul),
        Min => fold_items(v, n, m, assoc, |a: i64, b: i64| (a.min(b), false)),
        Max => fold_items(v, n, m, assoc, |a: i64, b: i64| (a.max(b), false)),
        _ => None,
    }
}

fn fold_f64(op: ScalarDyad, v: &[f64], n: usize, m: usize) -> Option<Vec<f64>> {
    use ScalarDyad::*;
    let assoc = is_associative(op);
    match op {
        Add => fold_items(v, n, m, assoc, |a: f64, b: f64| (a + b, false)),
        Sub => fold_items(v, n, m, assoc, |a: f64, b: f64| (a - b, false)),
        Mul => fold_items(v, n, m, assoc, |a: f64, b: f64| (a * b, false)),
        Min => fold_items(v, n, m, assoc, |a: f64, b: f64| (a.min(b), false)),
        Max => fold_items(v, n, m, assoc, |a: f64, b: f64| (a.max(b), false)),
        _ => None,
    }
}

/// Reduce a numeric buffer with one of the arithmetic operations, without
/// an intermediate array per step. None means this path does not apply and
/// the general fold must run.
fn reduce_typed(op: ScalarDyad, d: &Data, n: usize, m: usize) -> Option<Data> {
    use ScalarDyad::*;
    // The rest — comparisons, LCM/GCD, the float-only divisions — decide
    // their result type by rules the general path already carries.
    if !matches!(op, Add | Sub | Mul | Min | Max) {
        return None;
    }
    match d {
        Data::F64(v) => Some(Data::F64(fold_f64(op, v, n, m)?.into())),
        Data::I64(v) => Some(Data::I64(fold_i64(op, v, n, m)?.into())),
        // Booleans reduce as integers, which is what promotion says the
        // general path would produce; widen once and fold.
        Data::Bool(v) => {
            let widened = par::map(v, |&b| b as i64);
            Some(Data::I64(fold_i64(op, &widened, n, m)?.into()))
        }
        Data::Char(_) | Data::Box(_) => None,
    }
}

/// Insert `v` between the items of `y`, folding right to left.
fn reduce(v: &Verb, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    if y.rank() == 0 {
        return Ok(y.clone());
    }
    let n = y.items();
    if n == 1 {
        return Ok(y.item(0));
    }
    let cell_shape = y.shape[1..].to_vec();
    let m: usize = cell_shape.iter().product();
    if n == 0 {
        return match reduce_identity(v, m) {
            Some(d) => Ok(Array::new(cell_shape, d)),
            None => Err(Error::domain(
                format!("empty reduction has no identity for {}", v.name()),
                span,
            )),
        };
    }
    if y.dtype().is_numeric() {
        if let Verb::Prim(p) = v {
            if let DyadOp::Scalar(op) = p.dyad {
                // The typed fold covers the arithmetic reductions and runs
                // in parallel wherever the fold order allows; it declines
                // (integer overflow, an operation with its own type rules)
                // by returning None, and then the general fold below runs.
                if let Some(d) = reduce_typed(op, &y.data, n, m) {
                    return Ok(Array::new(cell_shape, d));
                }
                // Fold over the raw buffer, one whole item per step, without
                // materialising item arrays.
                let mut acc = y.data.slice((n - 1) * m, n * m);
                for i in (0..n - 1).rev() {
                    acc = scalar_dyad_data(op, &y.data, i * m, 1, &acc, 0, 1, m, span)?;
                }
                return Ok(Array::new(cell_shape, acc));
            }
        }
    }
    let mut acc = y.item(n - 1);
    for i in (0..n - 1).rev() {
        acc = v.dyad(&y.item(i), &acc, ctx, span)?;
    }
    Ok(acc)
}

// ------------------------------------------------- windows, scans, power

/// The elementwise operation a windowed verb folds with, when the verb is
/// exactly a reduction by a scalar primitive. The fast paths below apply
/// only then: they fold whole items at full rank, which is what `u/` does
/// and what any other spelling (a rank wrapper, a train) does not.
fn folded_op(u: &Verb) -> Option<ScalarDyad> {
    let Verb::Reduce(inner) = u else { return None };
    let Verb::Prim(p) = &**inner else { return None };
    match p.dyad {
        DyadOp::Scalar(op) => Some(op),
        _ => None,
    }
}

/// Items `lo .. hi` of `y`, sharing its buffer where the buffer allows.
fn section(y: &Array, lo: usize, hi: usize) -> Array {
    let m = y.item_size();
    let mut shape = y.shape.clone();
    shape[0] = hi - lo;
    Array::new(shape, y.data.slice(lo * m, hi * m))
}

/// `y` with a leading axis: a scalar is one item, which is how both
/// languages count the items of a rank-0 argument.
fn as_items(y: &Array) -> Option<Array> {
    (y.rank() == 0).then(|| Array::new(vec![1], y.data.clone()))
}

/// Running fold over `n` items of `m` elements each, one output item per
/// step. Backward is exactly the insert's right-to-left order, so it holds
/// for any step; forward is the left-to-right order, which agrees with the
/// insert only when the step is associative. None when a step left the
/// element type.
fn scan_flat<T, F>(v: &[T], n: usize, m: usize, back: bool, step: F) -> Option<Vec<T>>
where
    T: Copy + Default,
    F: Fn(T, T) -> (T, bool),
{
    if m == 1 {
        // One element per item is the shape a time series has, and it is
        // the one worth keeping the accumulator in a register for.
        let mut out = vec![T::default(); n];
        let mut over = false;
        if back {
            let mut acc = v[n - 1];
            out[n - 1] = acc;
            for (slot, &x) in out[..n - 1].iter_mut().zip(&v[..n - 1]).rev() {
                let (r, o) = step(x, acc);
                acc = r;
                over |= o;
                *slot = acc;
            }
        } else {
            let mut acc = v[0];
            out[0] = acc;
            for (slot, &x) in out[1..n].iter_mut().zip(&v[1..n]) {
                let (r, o) = step(acc, x);
                acc = r;
                over |= o;
                *slot = acc;
            }
        }
        return (!over).then_some(out);
    }
    let mut out = vec![T::default(); n * m];
    let mut acc = vec![T::default(); m];
    let mut over = false;
    if back {
        acc.copy_from_slice(&v[(n - 1) * m..n * m]);
        out[(n - 1) * m..n * m].copy_from_slice(&acc);
        for i in (0..n - 1).rev() {
            for (j, slot) in acc.iter_mut().enumerate() {
                let (r, o) = step(v[i * m + j], *slot);
                *slot = r;
                over |= o;
            }
            out[i * m..i * m + m].copy_from_slice(&acc);
        }
    } else {
        acc.copy_from_slice(&v[..m]);
        out[..m].copy_from_slice(&acc);
        for i in 1..n {
            for (j, slot) in acc.iter_mut().enumerate() {
                let (r, o) = step(*slot, v[i * m + j]);
                *slot = r;
                over |= o;
            }
            out[i * m..i * m + m].copy_from_slice(&acc);
        }
    }
    (!over).then_some(out)
}

fn scan_i64(op: ScalarDyad, v: &[i64], n: usize, m: usize, back: bool) -> Option<Vec<i64>> {
    use ScalarDyad::*;
    match op {
        Add => scan_flat(v, n, m, back, i64::overflowing_add),
        Sub => scan_flat(v, n, m, back, i64::overflowing_sub),
        Mul => scan_flat(v, n, m, back, i64::overflowing_mul),
        Min => scan_flat(v, n, m, back, |a: i64, b: i64| (a.min(b), false)),
        Max => scan_flat(v, n, m, back, |a: i64, b: i64| (a.max(b), false)),
        _ => None,
    }
}

fn scan_f64(op: ScalarDyad, v: &[f64], n: usize, m: usize, back: bool) -> Option<Vec<f64>> {
    use ScalarDyad::*;
    match op {
        Add => scan_flat(v, n, m, back, |a: f64, b: f64| (a + b, false)),
        Sub => scan_flat(v, n, m, back, |a: f64, b: f64| (a - b, false)),
        Mul => scan_flat(v, n, m, back, |a: f64, b: f64| (a * b, false)),
        Min => scan_flat(v, n, m, back, |a: f64, b: f64| (a.min(b), false)),
        Max => scan_flat(v, n, m, back, |a: f64, b: f64| (a.max(b), false)),
        _ => None,
    }
}

/// The scan of a numeric buffer in one pass. None means this path does not
/// apply. Integer overflow anywhere widens the whole result to float, which
/// is what the per-prefix reduction would also produce.
fn scan_typed(op: ScalarDyad, d: &Data, n: usize, m: usize, back: bool) -> Option<Data> {
    use ScalarDyad::*;
    if !matches!(op, Add | Sub | Mul | Min | Max) {
        return None;
    }
    let widened = |v: &[i64]| {
        let f: Vec<f64> = v.iter().map(|&x| x as f64).collect();
        Data::F64(scan_f64(op, &f, n, m, back).expect("the float scan cannot overflow").into())
    };
    let ints = |v: &[i64]| match scan_i64(op, v, n, m, back) {
        Some(out) => Data::I64(out.into()),
        None => widened(v),
    };
    match d {
        Data::F64(v) => Some(Data::F64(scan_f64(op, v, n, m, back)?.into())),
        Data::I64(v) => Some(ints(v)),
        Data::Bool(v) => Some(ints(&v.iter().map(|&b| b as i64).collect::<Vec<_>>())),
        Data::Char(_) | Data::Box(_) => None,
    }
}

/// Fold every window of `w` consecutive items into one item.
///
/// The items are cut into blocks of `w`. Within a block the running folds
/// from its start and from its end are computed once each, and then every
/// window is either one whole block or one block's suffix combined with the
/// next block's prefix. That is two steps per element with no accumulator
/// running longer than `w` of them, so the float error of a window is the
/// error of computing that window on its own — a cumulative sum over the
/// whole argument, differenced, would instead carry the drift of the entire
/// series into every window.
///
/// `step` has to be associative: the grouping is not the insert's own. The
/// float reassociation is the §5.9 contract, the same one reduction takes.
/// None when a step left the element type.
fn window_fold<T, F>(v: &[T], n: usize, m: usize, w: usize, step: F) -> Option<Vec<T>>
where
    T: Copy + Default + Send + Sync,
    F: Fn(T, T) -> (T, bool) + Sync + Send,
{
    debug_assert!(w >= 1 && n >= w);
    if m == 1 {
        return window_fold_flat(v, n, w, step);
    }
    let count = n - w + 1;
    let mut out = vec![T::default(); count * m];
    // Prefix folds of the current block, suffix folds of it and of the one
    // before: `w` items each, whatever the length of the argument.
    let mut pre = vec![T::default(); w * m];
    let mut suf = vec![T::default(); w * m];
    let mut prev = vec![T::default(); w * m];
    let mut over = false;
    for b in 0..n.div_ceil(w) {
        let bs = b * w;
        let be = ((b + 1) * w).min(n);
        pre[..m].copy_from_slice(&v[bs * m..bs * m + m]);
        for i in 1..be - bs {
            let (o, p) = (i * m, (i - 1) * m);
            for j in 0..m {
                let (r, f) = step(pre[p + j], v[(bs + i) * m + j]);
                pre[o + j] = r;
                over |= f;
            }
        }
        // Every window whose last item is in this block; its first item is
        // either this block's start or somewhere in the block before.
        for e in bs.max(w - 1)..be {
            let i = e + 1 - w;
            let (oo, po) = (i * m, (e - bs) * m);
            if i == bs {
                out[oo..oo + m].copy_from_slice(&pre[po..po + m]);
            } else {
                let so = (i + w - bs) * m;
                for j in 0..m {
                    let (r, f) = step(prev[so + j], pre[po + j]);
                    out[oo + j] = r;
                    over |= f;
                }
            }
        }
        let last = be - 1 - bs;
        suf[last * m..last * m + m].copy_from_slice(&v[(be - 1) * m..be * m]);
        for i in (0..last).rev() {
            let (o, p) = (i * m, (i + 1) * m);
            for j in 0..m {
                let (r, f) = step(v[(bs + i) * m + j], suf[p + j]);
                suf[o + j] = r;
                over |= f;
            }
        }
        std::mem::swap(&mut prev, &mut suf);
    }
    (!over).then_some(out)
}

/// [`window_fold`] for one element per item — a plain time series, and the
/// shape worth writing the loops out for: each of the three runs over a
/// block is a walk over one slice, so the accumulator stays in a register
/// and nothing is bounds-checked per element.
///
/// A range of the output depends only on the blocks its own windows lie in,
/// so the output splits across threads with nothing shared: a chunk starting
/// at `lo` starts at the block holding item `lo`, and the first window it
/// writes begins in that same block.
fn window_fold_flat<T, F>(v: &[T], n: usize, w: usize, step: F) -> Option<Vec<T>>
where
    T: Copy + Default + Send + Sync,
    F: Fn(T, T) -> (T, bool) + Sync + Send,
{
    let (out, ok) = par::fill(n - w + 1, |lo, part: &mut [T]| {
        window_fold_range(v, n, w, lo, part, &step)
    });
    ok.then_some(out)
}

/// The windows `lo .. lo + out.len()`. False when a step left the type.
fn window_fold_range<T, F>(v: &[T], n: usize, w: usize, lo: usize, out: &mut [T], step: &F) -> bool
where
    T: Copy + Default,
    F: Fn(T, T) -> (T, bool),
{
    if out.is_empty() {
        return true;
    }
    let hi = lo + out.len();
    let mut pre = vec![T::default(); w];
    let mut suf = vec![T::default(); w];
    let mut prev = vec![T::default(); w];
    let mut over = false;
    let mut bs = lo / w * w;
    // The last item any window of this chunk needs is `hi + w - 2`.
    while bs < n && bs <= hi + w - 2 {
        let block = &v[bs..(bs + w).min(n)];
        let lb = block.len();
        let mut acc = block[0];
        pre[0] = acc;
        for (slot, &x) in pre[1..lb].iter_mut().zip(&block[1..]) {
            let (r, o) = step(acc, x);
            acc = r;
            over |= o;
            *slot = acc;
        }
        // Every window of this chunk whose last item is in this block. Its
        // first item is this block's start, or is in the block before —
        // which is never the case in the first block a chunk touches, since
        // that block holds item `lo` and no window here starts earlier.
        for e in bs.max(lo + w - 1)..(bs + lb).min(hi + w - 1) {
            let i = e + 1 - w;
            out[i - lo] = if i == bs {
                pre[e - bs]
            } else {
                let (r, o) = step(prev[i + w - bs], pre[e - bs]);
                over |= o;
                r
            };
        }
        let mut acc = block[lb - 1];
        suf[lb - 1] = acc;
        for (slot, &x) in suf[..lb - 1].iter_mut().zip(&block[..lb - 1]).rev() {
            let (r, o) = step(x, acc);
            acc = r;
            over |= o;
            *slot = acc;
        }
        std::mem::swap(&mut prev, &mut suf);
        bs += w;
    }
    !over
}

fn window_i64(op: ScalarDyad, v: &[i64], n: usize, m: usize, w: usize) -> Option<Vec<i64>> {
    use ScalarDyad::*;
    match op {
        Add => window_fold(v, n, m, w, i64::overflowing_add),
        Mul => window_fold(v, n, m, w, i64::overflowing_mul),
        Min => window_fold(v, n, m, w, |a: i64, b: i64| (a.min(b), false)),
        Max => window_fold(v, n, m, w, |a: i64, b: i64| (a.max(b), false)),
        _ => None,
    }
}

fn window_f64(op: ScalarDyad, v: &[f64], n: usize, m: usize, w: usize) -> Option<Vec<f64>> {
    use ScalarDyad::*;
    match op {
        Add => window_fold(v, n, m, w, |a: f64, b: f64| (a + b, false)),
        Mul => window_fold(v, n, m, w, |a: f64, b: f64| (a * b, false)),
        Min => window_fold(v, n, m, w, |a: f64, b: f64| (a.min(b), false)),
        Max => window_fold(v, n, m, w, |a: f64, b: f64| (a.max(b), false)),
        _ => None,
    }
}

/// Moving windows over a numeric buffer in two passes. None means this path
/// does not apply: only the associative arithmetic can be regrouped into
/// blocks, so subtraction and every non-scalar verb go the general way.
fn window_typed(op: ScalarDyad, d: &Data, n: usize, m: usize, w: usize) -> Option<Data> {
    use ScalarDyad::*;
    if !matches!(op, Add | Mul | Min | Max) {
        return None;
    }
    let widened = |v: &[i64]| {
        let f: Vec<f64> = v.iter().map(|&x| x as f64).collect();
        Data::F64(window_f64(op, &f, n, m, w).expect("the float fold cannot overflow").into())
    };
    let ints = |v: &[i64]| match window_i64(op, v, n, m, w) {
        Some(out) => Data::I64(out.into()),
        None => widened(v),
    };
    match d {
        Data::F64(v) => Some(Data::F64(window_f64(op, v, n, m, w)?.into())),
        Data::I64(v) => Some(ints(v)),
        Data::Bool(v) => Some(ints(&v.iter().map(|&b| b as i64).collect::<Vec<_>>())),
        Data::Char(_) | Data::Box(_) => None,
    }
}

/// `u\ y` and `u\. y`: the verb applied to every prefix, or to every suffix.
fn runs(u: &Verb, y: &Array, back: bool, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let promoted = as_items(y);
    let base = promoted.as_ref().unwrap_or(y);
    let n = base.items();
    let m = base.item_size();
    if n > 0 && base.dtype().is_numeric() {
        if let Some(op) = folded_op(u) {
            // Folding from the right is the insert's own order, so it holds
            // for any step; folding from the left needs associativity.
            if back || is_associative(op) {
                if let Some(d) = scan_typed(op, &base.data, n, m, back) {
                    return Ok(Array::new(base.shape.clone(), d));
                }
            }
        }
    }
    let cells = each_cell(n, n * m, u.is_pure(), ctx, |i, c| {
        let part = if back { section(base, i, n) } else { section(base, 0, i + 1) };
        u.monad(&part, c, span)
    })?;
    assemble(&[n], cells, span)
}

/// The result of a window longer than the argument holds no items, but it
/// still has the shape of one: J learns that shape by running the verb on a
/// window of fills, and so does this. A verb that fails on fills, or a
/// window too large to build, leaves the result a plain empty vector.
fn empty_windows(u: &Verb, y: &Array, w: usize, ctx: &mut Ctx<'_>, span: Span) -> Array {
    let m = y.item_size();
    if u.is_pure() {
        if let Some(cells) = w.checked_mul(m).filter(|&s| s <= 1 << 20) {
            let mut shape = y.shape.clone();
            shape[0] = w;
            let probe = Array::new(shape, fill_data(y.dtype(), cells));
            if let Ok(cell) = u.monad(&probe, ctx, span) {
                let mut shape = vec![0usize];
                shape.extend_from_slice(&cell.shape);
                return Array::new(shape, Data::empty(cell.dtype()));
            }
        }
    }
    Array::new(vec![0], Data::empty(DType::I64))
}

/// The window size: one integer atom.
fn window_size(x: &Array, span: Span) -> Result<i64> {
    let v = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("the window size must be an integer", span))?;
    match v.as_slice() {
        [k] => Ok(*k),
        _ => Err(Error::new(
            ErrorKind::Length,
            "the window size must be a single number",
            Some(span),
        )),
    }
}

/// `x u\ y`: the verb applied to runs of x items.
///
/// A positive x takes the overlapping windows of that length, of which there
/// are none when the argument is shorter; a negative one takes the
/// non-overlapping chunks of |x| items, the last of them short; and zero
/// takes the n+1 empty runs between and around the items, which is what J
/// does with it.
fn infix(u: &Verb, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let k = window_size(x, span)?;
    let promoted = as_items(y);
    let base = promoted.as_ref().unwrap_or(y);
    let n = base.items();
    let m = base.item_size();
    if k < 0 {
        let w = k.unsigned_abs() as usize;
        let count = n.div_ceil(w);
        let cells = each_cell(count, n * m, u.is_pure(), ctx, |i, c| {
            u.monad(&section(base, i * w, ((i + 1) * w).min(n)), c, span)
        })?;
        return assemble(&[count], cells, span);
    }
    let w = k as usize;
    if n < w {
        return Ok(empty_windows(u, base, w, ctx, span));
    }
    let count = n - w + 1;
    if w > 0 && base.dtype().is_numeric() {
        if let Some(op) = folded_op(u) {
            if let Some(d) = window_typed(op, &base.data, n, m, w) {
                let mut shape = base.shape.clone();
                shape[0] = count;
                return Ok(Array::new(shape, d));
            }
        }
    }
    let work = count.saturating_mul(w).saturating_mul(m);
    let cells = each_cell(count, work, u.is_pure(), ctx, |i, c| {
        u.monad(&section(base, i, i + w), c, span)
    })?;
    assemble(&[count], cells, span)
}

/// `u^:n y` and `x u^:n y`: n applications of the verb, or iteration until
/// the result stops changing.
fn power(
    u: &Verb,
    p: Power,
    x: Option<&Array>,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    let step = |acc: &Array, c: &mut Ctx<'_>| match x {
        Some(x) => u.dyad(x, acc, c, span),
        None => u.monad(acc, c, span),
    };
    match p {
        Power::Times(n) => {
            let mut acc = y.clone();
            for _ in 0..n {
                acc = step(&acc, ctx)?;
            }
            Ok(acc)
        }
        Power::Converge => {
            let mut acc = y.clone();
            for _ in 0..CONVERGE_LIMIT {
                let next = step(&acc, ctx)?;
                if arrays_match(&next, &acc) {
                    return Ok(next);
                }
                acc = next;
            }
            Err(Error::domain("the iteration did not converge", span))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A context bound to a discarding output sink.
    macro_rules! ctx {
        ($name:ident, $agreement:expr) => {
            let mut sink = |_: &str| {};
            #[allow(unused_mut)]
            let mut $name = Ctx {
                cfg: EvalCfg { agreement: $agreement, fmt: FmtOpts::J },
                out: &mut sink,
            };
        };
        ($name:ident) => {
            ctx!($name, Agreement::LeadingPrefix);
        };
    }

    fn scalar_prim(name: &'static str, monad: MonadOp, dyad: DyadOp) -> Verb {
        Verb::Prim(Prim { name, monad, dyad, ranks: [0, 0, 0] })
    }

    fn inf_prim(name: &'static str, monad: MonadOp, dyad: DyadOp) -> Verb {
        Verb::Prim(Prim { name, monad, dyad, ranks: [RANK_INF, RANK_INF, RANK_INF] })
    }

    fn plus() -> Verb {
        scalar_prim("+", MonadOp::Scalar(ScalarMonad::Conj), DyadOp::Scalar(ScalarDyad::Add))
    }
    fn minus() -> Verb {
        scalar_prim("-", MonadOp::Scalar(ScalarMonad::Neg), DyadOp::Scalar(ScalarDyad::Sub))
    }
    fn times() -> Verb {
        scalar_prim("*", MonadOp::Scalar(ScalarMonad::Signum), DyadOp::Scalar(ScalarDyad::Mul))
    }
    fn pct() -> Verb {
        scalar_prim("%", MonadOp::Scalar(ScalarMonad::Recip), DyadOp::Scalar(ScalarDyad::DivJ))
    }
    fn div_apl() -> Verb {
        scalar_prim("÷", MonadOp::Scalar(ScalarMonad::Recip), DyadOp::Scalar(ScalarDyad::DivApl))
    }
    fn floor_v() -> Verb {
        scalar_prim("<.", MonadOp::Scalar(ScalarMonad::Floor), DyadOp::Scalar(ScalarDyad::Min))
    }
    fn ceil_v() -> Verb {
        scalar_prim(">.", MonadOp::Scalar(ScalarMonad::Ceil), DyadOp::Scalar(ScalarDyad::Max))
    }
    fn pow_v() -> Verb {
        scalar_prim("^", MonadOp::Scalar(ScalarMonad::Exp), DyadOp::Scalar(ScalarDyad::Pow))
    }
    fn residue_v() -> Verb {
        scalar_prim("|", MonadOp::Scalar(ScalarMonad::Abs), DyadOp::Scalar(ScalarDyad::Residue))
    }
    fn eq_v() -> Verb {
        scalar_prim("=", MonadOp::None, DyadOp::Scalar(ScalarDyad::Eq))
    }
    fn lt_v() -> Verb {
        scalar_prim("<", MonadOp::None, DyadOp::Scalar(ScalarDyad::Lt))
    }
    fn not_v() -> Verb {
        scalar_prim("-.", MonadOp::Scalar(ScalarMonad::Not), DyadOp::None)
    }
    fn sqrt_v() -> Verb {
        scalar_prim("%:", MonadOp::Scalar(ScalarMonad::Sqrt), DyadOp::NotYet("dyadic root"))
    }
    fn dollar() -> Verb {
        inf_prim("$", MonadOp::ShapeOf, DyadOp::Reshape)
    }
    fn pound() -> Verb {
        inf_prim("#", MonadOp::Tally, DyadOp::NotYet("copy"))
    }
    fn comma() -> Verb {
        inf_prim(",", MonadOp::Ravel, DyadOp::NotYet("append"))
    }
    fn transpose_v() -> Verb {
        inf_prim("|:", MonadOp::TransposeAxes, DyadOp::NotYet("dyadic transpose"))
    }
    fn head_v() -> Verb {
        inf_prim("{.", MonadOp::Head, DyadOp::Take)
    }
    fn behead_v() -> Verb {
        inf_prim("}.", MonadOp::Behead, DyadOp::Drop)
    }
    fn iota() -> Verb {
        inf_prim("i.", MonadOp::IotaJ, DyadOp::NotYet("index of"))
    }
    fn iota_apl(origin: i64) -> Verb {
        inf_prim("⍳", MonadOp::IotaApl { origin }, DyadOp::NotYet("index of"))
    }
    fn right_v() -> Verb {
        inf_prim("]", MonadOp::Same, DyadOp::Right)
    }
    fn echo_v() -> Verb {
        inf_prim("echo", MonadOp::Echo, DyadOp::None)
    }

    fn b(v: Verb) -> Box<Verb> {
        Box::new(v)
    }

    fn mat(rows: usize, cols: usize, v: Vec<i64>) -> Array {
        Array::new(vec![rows, cols], Data::I64(v.into()))
    }

    fn ints(a: &Array) -> Vec<i64> {
        a.as_i64_slice().expect("integer result").to_vec()
    }

    fn floats(a: &Array) -> Vec<f64> {
        a.as_f64_slice().expect("float result").to_vec()
    }

    fn bools(a: &Array) -> Vec<u8> {
        match &a.data {
            Data::Bool(v) => v.to_vec(),
            other => panic!("expected boolean result, got {other:?}"),
        }
    }

    fn sp() -> Span {
        Span::new(0, 1)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9 || (a.is_infinite() && b.is_infinite() && a.signum() == b.signum())
    }

    // ------------------------------------------------------------- naming

    #[test]
    fn names_of_primitives_and_derived_verbs() {
        assert_eq!(plus().name(), "+");
        assert_eq!(Verb::Rank(b(plus()), [1, 1, 1]).name(), "+\"1");
        assert_eq!(Verb::Rank(b(plus()), [0, 1, RANK_INF]).name(), "+\"0 1 _");
        assert_eq!(Verb::Rank(b(plus()), [RANK_INF; 3]).name(), "+\"_");
        assert_eq!(Verb::Reduce(b(plus())).name(), "+/");
        assert_eq!(Verb::Rank(b(Verb::Reduce(b(plus()))), [1, 1, 1]).name(), "+/\"1");
        assert_eq!(Verb::Fork(b(plus()), b(minus()), b(times())).name(), "(+ - *)");
        assert_eq!(
            Verb::NounFork(Array::scalar_i64(1), b(plus()), b(minus())).name(),
            "(n + -)"
        );
        assert_eq!(Verb::Hook(b(plus()), b(minus())).name(), "(+ -)");
        assert_eq!(Verb::Atop(b(plus()), b(minus())).name(), "(+@:-)");
        assert_eq!(Verb::Compose(b(plus()), b(minus())).name(), "(+&:-)");
        assert_eq!(Verb::BondLeft(Array::scalar_i64(1), b(plus())).name(), "(n&+)");
        assert_eq!(Verb::BondRight(b(plus()), Array::scalar_i64(1)).name(), "(+&n)");
    }

    #[test]
    fn composition_applies_the_right_verb_to_both_arguments() {
        ctx!(c);
        let v = Verb::Compose(b(plus()), b(times()));
        // Monadically an atop; dyadically the right verb runs on each side.
        let r = v.monad(&Array::from_i64(vec![-2, 0, 3]), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![-1, 0, 1]);
        let r = v
            .dyad(&Array::scalar_i64(-5), &Array::scalar_i64(7), &mut c, sp())
            .unwrap();
        assert_eq!(ints(&r), vec![0]);
        // A bond has a monadic valence only.
        let bond = Verb::BondLeft(Array::scalar_i64(10), b(minus()));
        let r = bond.monad(&Array::from_i64(vec![1, 2]), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![9, 8]);
        let e = bond
            .dyad(&Array::scalar_i64(1), &Array::scalar_i64(2), &mut c, sp())
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Domain);
        let bond = Verb::BondRight(b(minus()), Array::scalar_i64(10));
        let r = bond.monad(&Array::from_i64(vec![1, 2]), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![-9, -8]);
    }

    // ------------------------------------------------- rank and agreement

    #[test]
    fn scalar_monad_covers_the_whole_buffer() {
        ctx!(c);
        let r = minus().monad(&mat(2, 3, vec![1, 2, 3, 4, 5, 6]), &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![2, 3]);
        assert_eq!(ints(&r), vec![-1, -2, -3, -4, -5, -6]);
    }

    #[test]
    fn leading_prefix_agreement_broadcasts_per_row() {
        ctx!(c);
        let x = mat(2, 3, vec![1, 2, 3, 4, 5, 6]);
        let y = Array::from_i64(vec![10, 20]);
        let r = plus().dyad(&x, &y, &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![2, 3]);
        assert_eq!(ints(&r), vec![11, 12, 13, 24, 25, 26]);
        // and the same pairing with the operands swapped
        let r = plus().dyad(&y, &x, &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![11, 12, 13, 24, 25, 26]);
    }

    #[test]
    fn exact_or_scalar_rejects_a_prefix_frame() {
        ctx!(c, Agreement::ExactOrScalar);
        let x = mat(2, 3, vec![1, 2, 3, 4, 5, 6]);
        let y = Array::from_i64(vec![10, 20]);
        let e = plus().dyad(&x, &y, &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Shape);
        assert!(e.msg.contains("2 3"), "{}", e.msg);
        assert!(e.msg.contains("right shape 2"), "{}", e.msg);
    }

    #[test]
    fn exact_or_scalar_accepts_equal_frames_and_scalars() {
        ctx!(c, Agreement::ExactOrScalar);
        let x = mat(2, 3, vec![1, 2, 3, 4, 5, 6]);
        let r = plus().dyad(&x, &x, &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![2, 4, 6, 8, 10, 12]);
        let r = plus().dyad(&Array::scalar_i64(10), &x, &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![2, 3]);
        assert_eq!(ints(&r), vec![11, 12, 13, 14, 15, 16]);
        let r = plus().dyad(&x, &Array::scalar_i64(10), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![11, 12, 13, 14, 15, 16]);
    }

    #[test]
    fn vector_length_mismatch_is_a_length_error() {
        ctx!(c);
        let e = plus()
            .dyad(&Array::from_i64(vec![1, 2, 3]), &Array::from_i64(vec![1, 2, 3, 4, 5]), &mut c, sp())
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Length);
        assert!(e.msg.contains("left shape 3"), "{}", e.msg);
        assert!(e.msg.contains("right shape 5"), "{}", e.msg);
        assert!(e.notes[0].contains("axis 0"), "{:?}", e.notes);
    }

    #[test]
    fn diverging_matrix_frames_name_the_axis() {
        ctx!(c);
        let e = plus()
            .dyad(&mat(2, 3, vec![0; 6]), &mat(2, 4, vec![0; 8]), &mut c, sp())
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Shape);
        assert!(e.notes[0].contains("axis 1"), "{:?}", e.notes);
    }

    #[test]
    fn dyadic_rank_pairs_rows_with_the_whole_right_argument() {
        ctx!(c);
        // Left cells are rows, the right argument is one cell for all of them.
        let v = Verb::Rank(b(plus()), [0, 1, 1]);
        let r = v
            .dyad(&mat(2, 3, vec![1, 2, 3, 4, 5, 6]), &Array::from_i64(vec![10, 20, 30]), &mut c, sp())
            .unwrap();
        assert_eq!(r.shape, vec![2, 3]);
        assert_eq!(ints(&r), vec![11, 22, 33, 14, 25, 36]);
    }

    #[test]
    fn surplus_frame_axes_repeat_the_shorter_frames_cells() {
        ctx!(c);
        // Left cells are scalars (frame 2 2), right cells are rows (frame 2):
        // each right row serves the two left cells sharing its index.
        let v = Verb::Rank(b(head_v()), [0, 0, 1]);
        let x = mat(2, 2, vec![1, 1, 2, 2]);
        let y = mat(2, 3, vec![1, 2, 3, 4, 5, 6]);
        let r = v.dyad(&x, &y, &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![2, 2, 2]);
        assert_eq!(ints(&r), vec![1, 0, 1, 0, 4, 5, 4, 5]);
    }

    #[test]
    fn an_empty_frame_pairs_its_single_cell_with_every_other_cell() {
        ctx!(c, Agreement::ExactOrScalar);
        // Right cell rank 1 leaves an empty right frame; the left frame is 2.
        let v = Verb::Rank(b(head_v()), [0, 0, 1]);
        let x = Array::from_i64(vec![1, 2]);
        let y = Array::from_i64(vec![7, 8, 9]);
        let r = v.dyad(&x, &y, &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![2, 2]);
        assert_eq!(ints(&r), vec![7, 0, 7, 8]);
    }

    #[test]
    fn negative_rank_leaves_frame_axes() {
        ctx!(c);
        // Rank _1 on a matrix leaves one frame axis: shape of each row.
        let v = Verb::Rank(b(dollar()), [-1, -1, -1]);
        let r = v.monad(&mat(2, 3, vec![1, 2, 3, 4, 5, 6]), &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![2, 1]);
        assert_eq!(ints(&r), vec![3, 3]);
    }

    #[test]
    fn effective_rank_clamps_and_counts_back() {
        assert_eq!(effective_rank(0, 3), 0);
        assert_eq!(effective_rank(2, 1), 1);
        assert_eq!(effective_rank(RANK_INF, 4), 4);
        assert_eq!(effective_rank(-1, 3), 2);
        assert_eq!(effective_rank(-5, 3), 0);
    }

    // ---------------------------------------------------------- reduction

    #[test]
    fn reduction_folds_right_to_left() {
        ctx!(c);
        // -/ 1 2 3 is 1-(2-3), not (1-2)-3.
        let r = Verb::Reduce(b(minus()))
            .monad(&Array::from_i64(vec![1, 2, 3]), &mut c, sp())
            .unwrap();
        assert!(r.shape.is_empty());
        assert_eq!(ints(&r), vec![2]);
    }

    #[test]
    fn reduction_of_one_item_and_of_a_scalar() {
        ctx!(c);
        let r = Verb::Reduce(b(plus()))
            .monad(&Array::from_i64(vec![7]), &mut c, sp())
            .unwrap();
        assert!(r.shape.is_empty());
        assert_eq!(ints(&r), vec![7]);
        let r = Verb::Reduce(b(plus())).monad(&Array::scalar_i64(7), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![7]);
    }

    #[test]
    fn reduction_runs_along_the_leading_axis() {
        ctx!(c);
        let r = Verb::Reduce(b(plus()))
            .monad(&mat(2, 3, vec![1, 2, 3, 4, 5, 6]), &mut c, sp())
            .unwrap();
        assert_eq!(r.shape, vec![3]);
        assert_eq!(ints(&r), vec![5, 7, 9]);
    }

    #[test]
    fn rank_wrapped_reduction_sums_the_last_axis() {
        ctx!(c);
        let v = Verb::Rank(b(Verb::Reduce(b(plus()))), [1, 1, 1]);
        let r = v.monad(&mat(2, 3, vec![1, 2, 3, 4, 5, 6]), &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![2]);
        assert_eq!(ints(&r), vec![6, 15]);
    }

    #[test]
    fn empty_reduction_uses_the_identity_cell() {
        ctx!(c);
        let empty = Array::new(vec![0, 2], Data::I64(vec![].into()));
        let r = Verb::Reduce(b(plus())).monad(&empty, &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![2]);
        assert_eq!(ints(&r), vec![0, 0]);
        let r = Verb::Reduce(b(times())).monad(&empty, &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![1, 1]);
        let r = Verb::Reduce(b(floor_v())).monad(&empty, &mut c, sp()).unwrap();
        assert!(floats(&r).iter().all(|&x| x == f64::INFINITY));
        let r = Verb::Reduce(b(ceil_v())).monad(&empty, &mut c, sp()).unwrap();
        assert!(floats(&r).iter().all(|&x| x == f64::NEG_INFINITY));
        // Subtraction and division have identities too, and a comparison
        // has the conventional one both references print.
        let r = Verb::Reduce(b(minus())).monad(&empty, &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![0, 0]);
        let r = Verb::Reduce(b(pct())).monad(&empty, &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![1, 1]);
        let r = Verb::Reduce(b(eq_v())).monad(&empty, &mut c, sp()).unwrap();
        assert_eq!(bools(&r), vec![1, 1]);
        // An empty vector reduces to a scalar identity.
        let r = Verb::Reduce(b(plus()))
            .monad(&Array::empty(DType::I64), &mut c, sp())
            .unwrap();
        assert!(r.shape.is_empty());
        assert_eq!(ints(&r), vec![0]);
    }

    #[test]
    fn empty_reduction_without_an_identity_is_a_domain_error() {
        ctx!(c);
        // A derived verb has no identity cell at all; among the primitives
        // only the logarithm and the circle functions are left without one,
        // which is what both references do.
        let v = Verb::Hook(b(plus()), b(minus()));
        let e = Verb::Reduce(b(v)).monad(&Array::empty(DType::I64), &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Domain);
        assert!(e.msg.contains("identity"), "{}", e.msg);
    }

    #[test]
    fn reduction_with_a_non_primitive_verb_uses_the_general_fold() {
        ctx!(c);
        // The hook x (+ -) y is x + (-y), so this folds as 1-(2-3).
        let v = Verb::Reduce(b(Verb::Hook(b(plus()), b(minus()))));
        let r = v.monad(&Array::from_i64(vec![1, 2, 3]), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![2]);
    }

    #[test]
    fn dyadic_reduction_is_the_table() {
        ctx!(c);
        // `x u/ y` is the table (outer product), not a windowed reduction —
        // the windows are `x u\ y`.
        let v = Verb::Reduce(b(plus()));
        let r = v
            .dyad(&Array::scalar_i64(2), &Array::from_i64(vec![1, 2, 3]), &mut c, sp())
            .unwrap();
        assert_eq!(r.shape, vec![3]);
        assert_eq!(ints(&r), vec![3, 4, 5]);
        // The cells are the ones the inner verb's ranks ask for, so a scalar
        // verb pairs every atom of x with every atom of y.
        let r = v
            .dyad(&Array::from_i64(vec![1, 2, 3]), &Array::from_i64(vec![10, 20]), &mut c, sp())
            .unwrap();
        assert_eq!(r.shape, vec![3, 2]);
        assert_eq!(ints(&r), vec![11, 21, 12, 22, 13, 23]);
        // An infinite-rank verb takes both arguments whole: one application.
        let cat = Verb::Reduce(b(inf_prim(",", MonadOp::Ravel, DyadOp::AppendLeading)));
        let r = cat
            .dyad(&Array::from_i64(vec![1, 2]), &Array::from_i64(vec![3, 4]), &mut c, sp())
            .unwrap();
        assert_eq!(r.shape, vec![4]);
        assert_eq!(ints(&r), vec![1, 2, 3, 4]);
    }

    // --------------------------------------------------------- arithmetic

    #[test]
    fn integer_overflow_promotes_the_whole_result_to_float() {
        ctx!(c);
        let r = plus()
            .dyad(&Array::from_i64(vec![1, i64::MAX]), &Array::scalar_i64(1), &mut c, sp())
            .unwrap();
        assert_eq!(r.dtype(), DType::F64);
        let v = floats(&r);
        assert!(close(v[0], 2.0));
        assert!(close(v[1], i64::MAX as f64 + 1.0));
        // Without overflow the result stays integral.
        let r = plus()
            .dyad(&Array::from_i64(vec![1, 2]), &Array::scalar_i64(1), &mut c, sp())
            .unwrap();
        assert_eq!(r.dtype(), DType::I64);
    }

    #[test]
    fn reduction_overflow_promotes_too() {
        ctx!(c);
        let r = Verb::Reduce(b(plus()))
            .monad(&Array::from_i64(vec![i64::MAX, i64::MAX]), &mut c, sp())
            .unwrap();
        assert_eq!(r.dtype(), DType::F64);
        assert!(close(floats(&r)[0], 2.0 * i64::MAX as f64));
    }

    #[test]
    fn booleans_widen_to_integers_in_arithmetic() {
        ctx!(c);
        let bits = Array { shape: vec![3], data: Data::Bool(vec![1, 0, 1].into()) };
        let r = plus().dyad(&bits, &bits, &mut c, sp()).unwrap();
        assert_eq!(r.dtype(), DType::I64);
        assert_eq!(ints(&r), vec![2, 0, 2]);
    }

    #[test]
    fn j_division_is_float_and_survives_zero() {
        ctx!(c);
        let r = pct()
            .dyad(&Array::from_i64(vec![1, -1, 0, 6]), &Array::from_i64(vec![0, 0, 0, 4]), &mut c, sp())
            .unwrap();
        let v = floats(&r);
        assert_eq!(v[0], f64::INFINITY);
        assert_eq!(v[1], f64::NEG_INFINITY);
        assert_eq!(v[2], 0.0);
        assert!(close(v[3], 1.5));
    }

    #[test]
    fn apl_division_by_zero_is_a_domain_error_except_zero_by_zero() {
        ctx!(c, Agreement::ExactOrScalar);
        let r = div_apl()
            .dyad(&Array::scalar_i64(0), &Array::scalar_i64(0), &mut c, sp())
            .unwrap();
        assert!(close(floats(&r)[0], 1.0));
        let e = div_apl()
            .dyad(&Array::scalar_i64(1), &Array::scalar_i64(0), &mut c, sp())
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Domain);
        assert!(e.msg.contains("division by zero"), "{}", e.msg);
        let r = div_apl()
            .dyad(&Array::scalar_i64(6), &Array::scalar_i64(4), &mut c, sp())
            .unwrap();
        assert!(close(floats(&r)[0], 1.5));
    }

    #[test]
    fn reciprocal_of_zero_is_infinite() {
        ctx!(c);
        let r = pct().monad(&Array::from_i64(vec![0, 2]), &mut c, sp()).unwrap();
        let v = floats(&r);
        assert_eq!(v[0], f64::INFINITY);
        assert!(close(v[1], 0.5));
    }

    #[test]
    fn residue_takes_the_sign_of_the_left_argument() {
        ctx!(c);
        let x = Array::from_i64(vec![3, 3, -3, -3, 0]);
        let y = Array::from_i64(vec![5, -5, 5, -5, 5]);
        let r = residue_v().dyad(&x, &y, &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![2, 1, -1, -2, 5]);
        // Floats use the same rule via the floor of the quotient.
        let r = residue_v()
            .dyad(&Array::from_f64(vec![2.5]), &Array::from_f64(vec![7.0]), &mut c, sp())
            .unwrap();
        assert!(close(floats(&r)[0], 2.0));
    }

    #[test]
    fn power_stays_integral_when_it_can() {
        ctx!(c);
        let r = pow_v()
            .dyad(&Array::from_i64(vec![2, 0, 5]), &Array::from_i64(vec![10, 0, 1]), &mut c, sp())
            .unwrap();
        assert_eq!(r.dtype(), DType::I64);
        assert_eq!(ints(&r), vec![1024, 1, 5]);
        // A negative exponent forces the float path for the whole result.
        let r = pow_v()
            .dyad(&Array::from_i64(vec![2, 4]), &Array::from_i64(vec![-1, 2]), &mut c, sp())
            .unwrap();
        assert_eq!(r.dtype(), DType::F64);
        assert!(close(floats(&r)[0], 0.5));
        assert!(close(floats(&r)[1], 16.0));
        // Overflow does the same.
        let r = pow_v()
            .dyad(&Array::scalar_i64(10), &Array::scalar_i64(30), &mut c, sp())
            .unwrap();
        assert_eq!(r.dtype(), DType::F64);
    }

    #[test]
    fn comparisons_yield_booleans() {
        ctx!(c);
        let r = lt_v()
            .dyad(&Array::from_i64(vec![1, 2, 3]), &Array::scalar_i64(2), &mut c, sp())
            .unwrap();
        assert_eq!(bools(&r), vec![1, 0, 0]);
        let r = eq_v()
            .dyad(&Array::from_f64(vec![1.0, 2.0]), &Array::from_i64(vec![1, 3]), &mut c, sp())
            .unwrap();
        assert_eq!(bools(&r), vec![1, 0]);
    }

    #[test]
    fn characters_compare_but_do_not_add() {
        ctx!(c);
        let a = Array::from_chars(vec!['a', 'b']);
        let bb = Array::from_chars(vec!['a', 'c']);
        assert_eq!(bools(&eq_v().dyad(&a, &bb, &mut c, sp()).unwrap()), vec![1, 0]);
        let e = plus().dyad(&a, &bb, &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Type);
        assert!(e.msg.contains("characters"), "{}", e.msg);
        let e = lt_v().dyad(&a, &bb, &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Type);
        let e = plus().dyad(&a, &Array::from_i64(vec![1, 2]), &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Type);
        assert!(e.msg.contains("character"), "{}", e.msg);
        let e = plus().monad(&a, &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Type);
    }

    #[test]
    fn floor_and_ceiling_return_integers_when_they_fit() {
        ctx!(c);
        let r = floor_v().monad(&Array::from_f64(vec![1.5, -1.5]), &mut c, sp()).unwrap();
        assert_eq!(r.dtype(), DType::I64);
        assert_eq!(ints(&r), vec![1, -2]);
        let r = ceil_v().monad(&Array::from_f64(vec![1.5, -1.5]), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![2, -1]);
        // Values outside the integer range stay floating.
        let r = floor_v().monad(&Array::from_f64(vec![1e30]), &mut c, sp()).unwrap();
        assert_eq!(r.dtype(), DType::F64);
        // Integers pass through unchanged.
        let r = floor_v().monad(&Array::from_i64(vec![3]), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![3]);
    }

    #[test]
    fn logical_negation_needs_zero_or_one() {
        ctx!(c);
        let r = not_v().monad(&Array::from_i64(vec![0, 1]), &mut c, sp()).unwrap();
        assert_eq!(bools(&r), vec![1, 0]);
        let e = not_v().monad(&Array::from_i64(vec![2]), &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Domain);
    }

    #[test]
    fn signum_abs_and_negation_pick_their_types() {
        ctx!(c);
        let r = times().monad(&Array::from_i64(vec![-3, 0, 9]), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![-1, 0, 1]);
        let r = times().monad(&Array::from_f64(vec![-3.0, 0.0, 9.0]), &mut c, sp()).unwrap();
        assert_eq!(floats(&r), vec![-1.0, 0.0, 1.0]);
        let r = residue_v().monad(&Array::from_i64(vec![-3, 3]), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![3, 3]);
        let bits = Array { shape: vec![2], data: Data::Bool(vec![0, 1].into()) };
        let r = minus().monad(&bits, &mut c, sp()).unwrap();
        assert_eq!(r.dtype(), DType::I64);
        assert_eq!(ints(&r), vec![0, -1]);
    }

    #[test]
    fn square_root_of_a_negative_number_awaits_complex_numbers() {
        ctx!(c);
        let r = sqrt_v().monad(&Array::from_i64(vec![9]), &mut c, sp()).unwrap();
        assert!(close(floats(&r)[0], 3.0));
        let e = sqrt_v().monad(&Array::from_i64(vec![-1]), &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("complex"), "{}", e.msg);
    }

    // --------------------------------------------------------- structural

    #[test]
    fn shape_tally_and_ravel() {
        ctx!(c);
        let m = mat(2, 3, vec![1, 2, 3, 4, 5, 6]);
        let r = dollar().monad(&m, &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![2]);
        assert_eq!(ints(&r), vec![2, 3]);
        let r = pound().monad(&m, &mut c, sp()).unwrap();
        assert!(r.shape.is_empty());
        assert_eq!(ints(&r), vec![2]);
        // A scalar has one item and no axes.
        let r = pound().monad(&Array::scalar_i64(5), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![1]);
        let r = comma().monad(&m, &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![6]);
        assert_eq!(ints(&r), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn transpose_reverses_the_axes() {
        ctx!(c);
        let r = transpose_v().monad(&mat(2, 3, vec![1, 2, 3, 4, 5, 6]), &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![3, 2]);
        assert_eq!(ints(&r), vec![1, 4, 2, 5, 3, 6]);
        // Rank 3: 2 by 1 by 3 becomes 3 by 1 by 2.
        let a = Array::new(vec![2, 1, 3], Data::I64(vec![1, 2, 3, 4, 5, 6].into()));
        let r = transpose_v().monad(&a, &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![3, 1, 2]);
        assert_eq!(ints(&r), vec![1, 4, 2, 5, 3, 6]);
        // Vectors and scalars are unchanged.
        let v = Array::from_i64(vec![1, 2]);
        assert_eq!(transpose_v().monad(&v, &mut c, sp()).unwrap(), v);
    }

    #[test]
    fn head_and_behead() {
        ctx!(c);
        let m = mat(2, 3, vec![1, 2, 3, 4, 5, 6]);
        let r = head_v().monad(&m, &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![3]);
        assert_eq!(ints(&r), vec![1, 2, 3]);
        let r = behead_v().monad(&m, &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![1, 3]);
        assert_eq!(ints(&r), vec![4, 5, 6]);
        // The head of an empty array is a cell of fills.
        let e = Array::new(vec![0, 2], Data::I64(vec![].into()));
        let r = head_v().monad(&e, &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![2]);
        assert_eq!(ints(&r), vec![0, 0]);
        assert_eq!(behead_v().monad(&e, &mut c, sp()).unwrap(), e);
        assert_eq!(head_v().monad(&Array::scalar_i64(5), &mut c, sp()).unwrap().shape, Vec::<usize>::new());
        let err = behead_v().monad(&Array::scalar_i64(5), &mut c, sp()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Domain);
    }

    #[test]
    fn iota_fills_a_shape_and_reverses_negative_axes() {
        ctx!(c);
        let r = iota().monad(&Array::from_i64(vec![2, 3]), &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![2, 3]);
        assert_eq!(ints(&r), vec![0, 1, 2, 3, 4, 5]);
        // A scalar argument gives one axis.
        let r = iota().monad(&Array::scalar_i64(3), &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![3]);
        assert_eq!(ints(&r), vec![0, 1, 2]);
        // Negative lengths run the axis backwards.
        let r = iota().monad(&Array::scalar_i64(-3), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![2, 1, 0]);
        let r = iota().monad(&Array::from_i64(vec![2, -3]), &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![2, 3]);
        assert_eq!(ints(&r), vec![2, 1, 0, 5, 4, 3]);
        let r = iota().monad(&Array::from_i64(vec![-2, 3]), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![3, 4, 5, 0, 1, 2]);
        // Zero lengths give an empty result of that shape.
        let r = iota().monad(&Array::scalar_i64(0), &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![0]);
        assert!(ints(&r).is_empty());
        // Non-integers and matrices are refused.
        let e = iota().monad(&Array::from_f64(vec![1.5]), &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Domain);
        let e = iota().monad(&mat(1, 1, vec![1]), &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Rank);
    }

    #[test]
    fn apl_iota_starts_at_the_index_origin() {
        ctx!(c, Agreement::ExactOrScalar);
        let r = iota_apl(1).monad(&Array::scalar_i64(3), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![1, 2, 3]);
        let r = iota_apl(0).monad(&Array::scalar_i64(3), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![0, 1, 2]);
        let e = iota_apl(1).monad(&Array::scalar_i64(-1), &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Domain);
        let e = iota_apl(1).monad(&Array::from_i64(vec![2, 3]), &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Domain);
    }

    #[test]
    fn reshape_cycles_the_ravel() {
        ctx!(c);
        let r = dollar()
            .dyad(&Array::from_i64(vec![2, 3]), &Array::from_i64(vec![1, 2]), &mut c, sp())
            .unwrap();
        assert_eq!(r.shape, vec![2, 3]);
        assert_eq!(ints(&r), vec![1, 2, 1, 2, 1, 2]);
        // A scalar left argument reshapes to a vector.
        let r = dollar()
            .dyad(&Array::scalar_i64(3), &Array::from_i64(vec![7]), &mut c, sp())
            .unwrap();
        assert_eq!(r.shape, vec![3]);
        assert_eq!(ints(&r), vec![7, 7, 7]);
        // Reshaping down keeps the leading elements, and the type is y's.
        let r = dollar()
            .dyad(&Array::scalar_i64(2), &Array::from_chars(vec!['a', 'b', 'c']), &mut c, sp())
            .unwrap();
        assert_eq!(r.dtype(), DType::Char);
        // An empty right argument cannot fill a non-empty shape.
        let e = dollar()
            .dyad(&Array::scalar_i64(2), &Array::empty(DType::I64), &mut c, sp())
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Length);
        assert!(e.msg.contains("empty"), "{}", e.msg);
        // but an empty shape is fine.
        let r = dollar()
            .dyad(&Array::scalar_i64(0), &Array::empty(DType::I64), &mut c, sp())
            .unwrap();
        assert_eq!(r.shape, vec![0]);
        let e = dollar()
            .dyad(&Array::scalar_i64(-1), &Array::from_i64(vec![1]), &mut c, sp())
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Domain);
    }

    #[test]
    fn take_from_both_ends_and_beyond() {
        ctx!(c);
        let v = Array::from_i64(vec![1, 2, 3, 4]);
        let take = |x: Array, y: &Array, c: &mut Ctx<'_>| head_v().dyad(&x, y, c, sp()).unwrap();
        assert_eq!(ints(&take(Array::scalar_i64(2), &v, &mut c)), vec![1, 2]);
        assert_eq!(ints(&take(Array::scalar_i64(-2), &v, &mut c)), vec![3, 4]);
        // Overtaking pads at the back for a positive count,
        let short = Array::from_i64(vec![1, 2, 3]);
        assert_eq!(ints(&take(Array::scalar_i64(6), &short, &mut c)), vec![1, 2, 3, 0, 0, 0]);
        // and at the front for a negative one.
        assert_eq!(ints(&take(Array::scalar_i64(-6), &short, &mut c)), vec![0, 0, 0, 1, 2, 3]);
        // A scalar right argument is treated as a one-item vector.
        let r = take(Array::scalar_i64(2), &Array::scalar_i64(5), &mut c);
        assert_eq!(r.shape, vec![2]);
        assert_eq!(ints(&r), vec![5, 0]);
        // Per-axis on a matrix.
        let m = mat(2, 3, vec![1, 2, 3, 4, 5, 6]);
        let r = take(Array::scalar_i64(1), &m, &mut c);
        assert_eq!(r.shape, vec![1, 3]);
        assert_eq!(ints(&r), vec![1, 2, 3]);
        let r = take(Array::scalar_i64(-1), &m, &mut c);
        assert_eq!(ints(&r), vec![4, 5, 6]);
        let r = take(Array::from_i64(vec![2, 2]), &m, &mut c);
        assert_eq!(r.shape, vec![2, 2]);
        assert_eq!(ints(&r), vec![1, 2, 4, 5]);
        let r = take(Array::from_i64(vec![3, -2]), &m, &mut c);
        assert_eq!(r.shape, vec![3, 2]);
        assert_eq!(ints(&r), vec![2, 3, 5, 6, 0, 0]);
        // Character fills are spaces.
        let r = head_v()
            .dyad(&Array::scalar_i64(3), &Array::from_chars(vec!['a']), &mut c, sp())
            .unwrap();
        assert_eq!(r.data, Data::Char(vec!['a', ' ', ' '].into()));
        let e = head_v()
            .dyad(&Array::from_i64(vec![1, 1]), &Array::from_i64(vec![1, 2]), &mut c, sp())
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::NotYet);
    }

    #[test]
    fn drop_from_both_ends_and_beyond() {
        ctx!(c);
        let v = Array::from_i64(vec![1, 2, 3]);
        let drop = |x: Array, y: &Array, c: &mut Ctx<'_>| behead_v().dyad(&x, y, c, sp()).unwrap();
        assert_eq!(ints(&drop(Array::scalar_i64(1), &v, &mut c)), vec![2, 3]);
        assert_eq!(ints(&drop(Array::scalar_i64(-1), &v, &mut c)), vec![1, 2]);
        // Dropping more than there is empties the axis.
        let r = drop(Array::scalar_i64(5), &v, &mut c);
        assert_eq!(r.shape, vec![0]);
        assert!(ints(&r).is_empty());
        let m = mat(2, 3, vec![1, 2, 3, 4, 5, 6]);
        let r = drop(Array::scalar_i64(1), &m, &mut c);
        assert_eq!(r.shape, vec![1, 3]);
        assert_eq!(ints(&r), vec![4, 5, 6]);
        let r = drop(Array::from_i64(vec![0, -1]), &m, &mut c);
        assert_eq!(r.shape, vec![2, 2]);
        assert_eq!(ints(&r), vec![1, 2, 4, 5]);
    }

    // ------------------------------------------------------------ framing

    #[test]
    fn cells_of_unequal_shapes_are_padded_with_fills() {
        ctx!(c);
        // i."0 ] 1 2 3: cells of length 1, 2 and 3 frame into a 3 by 3 table.
        let v = Verb::Rank(b(iota()), [0, 0, 0]);
        let r = v.monad(&Array::from_i64(vec![1, 2, 3]), &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![3, 3]);
        assert_eq!(ints(&r), vec![0, 0, 0, 0, 1, 0, 0, 1, 2]);
    }

    #[test]
    fn framing_aligns_lower_rank_cells_at_the_trailing_axes() {
        let cells = vec![Array::from_i64(vec![1, 2]), mat(2, 2, vec![1, 2, 3, 4])];
        let r = assemble(&[2], cells, sp()).unwrap();
        assert_eq!(r.shape, vec![2, 2, 2]);
        assert_eq!(ints(&r), vec![1, 2, 0, 0, 1, 2, 3, 4]);
    }

    #[test]
    fn framing_promotes_cell_types() {
        let cells = vec![Array::from_i64(vec![1]), Array::from_f64(vec![2.5])];
        let r = assemble(&[2], cells, sp()).unwrap();
        assert_eq!(r.dtype(), DType::F64);
        assert_eq!(floats(&r), vec![1.0, 2.5]);
        // Characters and numbers cannot share a result.
        let cells = vec![Array::from_i64(vec![1]), Array::from_chars(vec!['a'])];
        let e = assemble(&[2], cells, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Type);
    }

    #[test]
    fn framing_over_an_empty_frame_yields_an_empty_result() {
        let r = assemble(&[0], Vec::new(), sp()).unwrap();
        assert_eq!(r.shape, vec![0]);
        assert_eq!(r.count(), 0);
    }

    // ------------------------------------------------------------- trains

    #[test]
    fn fork_applies_both_tines() {
        ctx!(c);
        // (+/ % #) is the mean.
        let v = Verb::Fork(b(Verb::Reduce(b(plus()))), b(pct()), b(pound()));
        let r = v.monad(&Array::from_i64(vec![1, 2, 3, 4]), &mut c, sp()).unwrap();
        assert!(close(floats(&r)[0], 2.5));
        // Dyadically both tines see both arguments: (x-y) + (x+y) = 2x.
        let v = Verb::Fork(b(minus()), b(plus()), b(plus()));
        let r = v
            .dyad(&Array::from_i64(vec![5]), &Array::from_i64(vec![3]), &mut c, sp())
            .unwrap();
        assert_eq!(ints(&r), vec![10]);
    }

    #[test]
    fn noun_fork_supplies_a_constant_left_argument() {
        ctx!(c);
        let v = Verb::NounFork(Array::scalar_i64(10), b(minus()), b(right_v()));
        let r = v.monad(&Array::from_i64(vec![1, 2]), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![9, 8]);
        let r = v
            .dyad(&Array::scalar_i64(0), &Array::from_i64(vec![1, 2]), &mut c, sp())
            .unwrap();
        assert_eq!(ints(&r), vec![9, 8]);
    }

    #[test]
    fn hook_reuses_its_right_argument() {
        ctx!(c);
        // y + (-y) is zero.
        let v = Verb::Hook(b(plus()), b(minus()));
        let r = v.monad(&Array::from_i64(vec![1, 2]), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![0, 0]);
        // x + (-y)
        let r = v
            .dyad(&Array::from_i64(vec![10]), &Array::from_i64(vec![3]), &mut c, sp())
            .unwrap();
        assert_eq!(ints(&r), vec![7]);
    }

    #[test]
    fn atop_composes() {
        ctx!(c);
        let v = Verb::Atop(b(minus()), b(plus()));
        let r = v.monad(&Array::from_i64(vec![1, 2]), &mut c, sp()).unwrap();
        assert_eq!(ints(&r), vec![-1, -2]);
        let r = v
            .dyad(&Array::from_i64(vec![1]), &Array::from_i64(vec![2]), &mut c, sp())
            .unwrap();
        assert_eq!(ints(&r), vec![-3]);
    }

    #[test]
    fn trains_apply_to_the_whole_argument() {
        // No train iterates cells of its own.
        assert_eq!(Verb::Hook(b(plus()), b(minus())).ranks(), [RANK_INF; 3]);
        assert_eq!(Verb::Reduce(b(plus())).ranks(), [RANK_INF; 3]);
    }

    // ------------------------------------------------------- missing cases

    #[test]
    fn absent_and_unwritten_meanings_are_reported_differently() {
        ctx!(c);
        let e = eq_v().monad(&Array::scalar_i64(1), &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Domain);
        assert!(e.msg.contains("no monadic meaning"), "{}", e.msg);
        let e = not_v()
            .dyad(&Array::scalar_i64(1), &Array::scalar_i64(1), &mut c, sp())
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Domain);
        assert!(e.msg.contains("no dyadic meaning"), "{}", e.msg);
        let e = pound()
            .dyad(&Array::scalar_i64(1), &Array::scalar_i64(1), &mut c, sp())
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("copy"), "{}", e.msg);
        // Echo's output formatting belongs to fmt; only its result is checked.
        let _ = echo_v();
    }

    // ----------------------------------------------------- parallel paths
    //
    // Every case here runs the same application twice, on a pool of one
    // thread and on a pool of four, and compares the two: the sequential
    // result is the contract, and the argument sizes are chosen to be over
    // the threshold so the parallel path is really taken.

    /// The result of `f` under one thread and under four.
    fn seq_par<T: Send>(f: impl Fn() -> T + Sync + Send) -> (T, T) {
        (par::with_threads(1, &f), par::with_threads(4, &f))
    }

    /// A deterministic spread of values, positive and negative.
    fn noise(n: usize) -> Vec<f64> {
        let mut x = 0x2545_f491_4f6c_dd1du64;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 11) as f64 / (1u64 << 53) as f64 - 0.5
            })
            .collect()
    }

    fn f64_mat(rows: usize, cols: usize) -> Array {
        Array::new(vec![rows, cols], Data::F64(noise(rows * cols).into()))
    }

    /// Above `par::MIN_WORK`, so anything elementwise splits.
    const BIG: usize = 200_000;

    #[test]
    fn an_elementwise_dyad_splits_into_the_same_result() {
        let x = Array::from_f64(noise(BIG));
        let y = Array::from_f64(noise(BIG).iter().map(|v| v + 0.25).collect());
        let (one, many) = seq_par(|| {
            ctx!(c);
            times().dyad(&x, &y, &mut c, sp()).unwrap()
        });
        assert_eq!(floats(&one), floats(&many));
        // A scalar left argument takes the broadcasting shape of the loop.
        let (one, many) = seq_par(|| {
            ctx!(c);
            plus().dyad(&Array::scalar_f64(0.5), &y, &mut c, sp()).unwrap()
        });
        assert_eq!(floats(&one), floats(&many));
    }

    #[test]
    fn an_elementwise_dyad_that_overflows_widens_the_same_way() {
        // One pair overflows i64, so the whole pass is redone in floats
        // however the chunks fell.
        let mut v = vec![1i64; BIG];
        v[BIG - 3] = i64::MAX;
        let x = Array::from_i64(v);
        let (one, many) = seq_par(|| {
            ctx!(c);
            plus().dyad(&x, &x, &mut c, sp()).unwrap()
        });
        assert_eq!(one.dtype(), DType::F64);
        assert_eq!(floats(&one), floats(&many));
    }

    #[test]
    fn an_elementwise_monad_splits_into_the_same_result() {
        let y = Array::from_f64(noise(BIG));
        for v in [minus(), sqrt_v(), floor_v(), pct()] {
            let (one, many) = seq_par(|| {
                ctx!(c);
                v.monad(&Array::from_f64(y.as_f64_slice().unwrap().iter().map(|x| x.abs()).collect()), &mut c, sp())
                    .unwrap()
            });
            assert_eq!(one.data, many.data, "{}", v.name());
        }
    }

    #[test]
    fn monadic_cells_run_in_parallel_and_frame_in_order() {
        // 400 cells of 512 elements: over the threshold, and every cell
        // yields a different value, so a misplaced cell would show.
        let y = f64_mat(400, 512);
        let v = Verb::Rank(b(Verb::Reduce(b(plus()))), [1, 1, 1]);
        let (one, many) = seq_par(|| {
            ctx!(c);
            v.monad(&y, &mut c, sp()).unwrap()
        });
        assert_eq!(one.shape, vec![400]);
        assert_eq!(floats(&one), floats(&many));
    }

    #[test]
    fn dyadic_cells_run_in_parallel_and_frame_in_order() {
        let x = f64_mat(400, 512);
        let y = f64_mat(400, 512);
        // Rank 1: the frame is the rows, and each row pair is one cell.
        let v = Verb::Rank(b(plus()), [1, 1, 1]);
        let (one, many) = seq_par(|| {
            ctx!(c);
            v.dyad(&x, &y, &mut c, sp()).unwrap()
        });
        assert_eq!(one.shape, vec![400, 512]);
        assert_eq!(floats(&one), floats(&many));
    }

    #[test]
    fn a_verb_that_writes_output_is_not_pure() {
        assert!(plus().is_pure());
        assert!(Verb::Rank(b(Verb::Reduce(b(plus()))), [1, 1, 1]).is_pure());
        assert!(!echo_v().is_pure());
        assert!(!Verb::Rank(b(Verb::Atop(b(echo_v()), b(plus()))), [1, 1, 1]).is_pure());
    }

    #[test]
    fn an_impure_verb_keeps_its_cells_in_order() {
        // Enough elements to pass the threshold; the cells must still be
        // written one after another, in index order.
        let y = Array::new(vec![16, 8192], Data::I64((0..16 * 8192).collect::<Vec<i64>>().into()));
        let v = Verb::Rank(b(Verb::Atop(b(echo_v()), b(head_v()))), [1, 1, 1]);
        let mut seen: Vec<i64> = Vec::new();
        let mut sink = |s: &str| {
            if let Some(first) = s.split_whitespace().next() {
                if let Ok(n) = first.parse::<i64>() {
                    seen.push(n);
                }
            }
        };
        let mut c = Ctx {
            cfg: EvalCfg { agreement: Agreement::LeadingPrefix, fmt: FmtOpts::J },
            out: &mut sink,
        };
        v.monad(&y, &mut c, sp()).unwrap();
        assert_eq!(seen, (0..16).map(|i| i * 8192).collect::<Vec<i64>>());
    }

    #[test]
    fn a_wide_item_reduce_folds_every_column_in_order() {
        // item_size over par::WIDE_ITEM: each output element folds its own
        // column, so even a non-associative fold matches exactly.
        let y = f64_mat(300, 512);
        for v in [plus(), minus(), floor_v()] {
            let (one, many) = seq_par(|| {
                ctx!(c);
                Verb::Reduce(b(v.clone())).monad(&y, &mut c, sp()).unwrap()
            });
            assert_eq!(one.shape, vec![512]);
            assert_eq!(floats(&one), floats(&many), "{}", v.name());
        }
    }

    #[test]
    fn a_wide_item_integer_reduce_is_exact() {
        let n = 300;
        let m = 512;
        let y = Array::new(
            vec![n, m],
            Data::I64((0..(n * m) as i64).map(|i| i % 977 - 400).collect::<Vec<i64>>().into()),
        );
        let (one, many) = seq_par(|| {
            ctx!(c);
            Verb::Reduce(b(minus())).monad(&y, &mut c, sp()).unwrap()
        });
        assert_eq!(ints(&one), ints(&many));
    }

    #[test]
    fn a_narrow_item_reduce_chunks_the_items() {
        // item_size under par::WIDE_ITEM and an associative verb: the items
        // are chunked, which reassociates a float sum (§5.9) but not an
        // integer one.
        let y = f64_mat(300_000, 8);
        let (one, many) = seq_par(|| {
            ctx!(c);
            Verb::Reduce(b(plus())).monad(&y, &mut c, sp()).unwrap()
        });
        assert_eq!(one.shape, vec![8]);
        for (p, q) in floats(&one).iter().zip(floats(&many)) {
            assert!((p - q).abs() <= 1e-12 * p.abs().max(1.0), "{p} vs {q}");
        }
        let ints_y = Array::new(
            vec![300_000, 8],
            Data::I64((0..300_000 * 8).map(|i| (i % 101) as i64 - 50).collect::<Vec<i64>>().into()),
        );
        let (one, many) = seq_par(|| {
            ctx!(c);
            Verb::Reduce(b(plus())).monad(&ints_y, &mut c, sp()).unwrap()
        });
        assert_eq!(ints(&one), ints(&many));
    }

    #[test]
    fn a_vector_reduce_folds_the_flat_buffer() {
        let y = Array::from_f64(noise(BIG * 4));
        let (one, many) = seq_par(|| {
            ctx!(c);
            Verb::Reduce(b(plus())).monad(&y, &mut c, sp()).unwrap()
        });
        let (p, q) = (floats(&one)[0], floats(&many)[0]);
        assert!((p - q).abs() <= 1e-12 * p.abs().max(1.0), "{p} vs {q}");

        // Integers are exact, and a non-associative fold is not regrouped
        // at all, so it matches to the bit.
        let ints_y = Array::from_i64((0..BIG as i64 * 4).map(|i| i % 1009 - 500).collect());
        for v in [plus(), minus(), ceil_v()] {
            let (one, many) = seq_par(|| {
                ctx!(c);
                Verb::Reduce(b(v.clone())).monad(&ints_y, &mut c, sp()).unwrap()
            });
            assert_eq!(ints(&one), ints(&many), "{}", v.name());
        }
    }

    #[test]
    fn a_reduce_that_overflows_falls_back_to_the_sequential_widening() {
        let mut v: Vec<i64> = vec![1; BIG];
        v[7] = i64::MAX;
        let y = Array::from_i64(v);
        let (one, many) = seq_par(|| {
            ctx!(c);
            Verb::Reduce(b(plus())).monad(&y, &mut c, sp()).unwrap()
        });
        assert_eq!(one.dtype(), DType::F64);
        assert_eq!(floats(&one), floats(&many));
    }

    #[test]
    fn a_boolean_reduce_matches_the_sequential_promotion() {
        let n = BIG;
        let y = Array::new(
            vec![n],
            Data::Bool((0..n).map(|i| (i % 3 == 0) as u8).collect::<Vec<u8>>().into()),
        );
        let (one, many) = seq_par(|| {
            ctx!(c);
            Verb::Reduce(b(plus())).monad(&y, &mut c, sp()).unwrap()
        });
        assert_eq!(one.dtype(), DType::I64);
        assert_eq!(ints(&one), ints(&many));
        assert_eq!(ints(&one)[0], n.div_ceil(3) as i64);
    }
}
