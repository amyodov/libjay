//! Verbs and the rank machinery: the language-agnostic execution core.
//!
//! A `Verb` is a semantic object — a primitive or a combination of verbs —
//! applied monadically or dyadically to arrays. Frontends lower J/APL syntax
//! to `Verb` trees; nothing in here knows any surface syntax.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::array::{Array, Data};
use crate::complex::{self as cx, Cx};
use crate::dtype::DType;
use crate::error::{Error, ErrorKind, Result, Span};
use crate::exact::{self, Ext, Rat};
use crate::fmt::FmtOpts;
use crate::par;
use crate::simd::multiversioned;

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

/// How close two floats have to be to count as equal.
///
/// Both languages compare reals with a relative tolerance: J's `9!:18`
/// comparison tolerance, APL's `⎕CT`. Two values are equal when they differ
/// by less than the tolerance scaled by one of their magnitudes — the
/// smaller one in J, the larger one in APL. Both references answer strictly:
/// a difference exactly at the threshold is not equal. Integers, characters
/// and boxes are unaffected, and an exact bit-for-bit equality (the
/// infinities included) is equality whatever the tolerance is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tol {
    /// Relative tolerance; zero compares exactly.
    pub ct: f64,
    /// Scale by the smaller magnitude (J) rather than the larger (APL).
    pub by_smaller: bool,
}

impl Tol {
    /// No tolerance at all — J's `u!.0`.
    pub const EXACT: Tol = Tol { ct: 0.0, by_smaller: true };
    /// J's default comparison tolerance, 2^-44.
    pub const J: Tol = Tol { ct: 5.684_341_886_080_802e-14, by_smaller: true };
    /// GNU APL's default `⎕CT`.
    pub const APL: Tol = Tol { ct: 1e-13, by_smaller: false };

    /// Tolerant equality.
    #[inline(always)]
    pub fn eq(self, a: f64, b: f64) -> bool {
        if a == b {
            return true;
        }
        // NaN and unequal infinities fail every comparison below, which is
        // what both references answer for them.
        let s = if self.by_smaller {
            a.abs().min(b.abs())
        } else {
            a.abs().max(b.abs())
        };
        (a - b).abs() < self.ct * s
    }

    /// Tolerant `<`: less, and not tolerantly equal.
    #[inline(always)]
    pub fn lt(self, a: f64, b: f64) -> bool {
        a < b && !self.eq(a, b)
    }

    /// Tolerant `<=`: less, or tolerantly equal.
    #[inline(always)]
    pub fn le(self, a: f64, b: f64) -> bool {
        a <= b || self.eq(a, b)
    }

    /// Tolerant equality on complex values: the magnitude of the difference
    /// against the same scale the real comparison uses. J answers
    /// `3j4 = 3.0000000000001j4` with 1, which is this rule on magnitudes.
    #[inline]
    pub fn eq_cx(self, a: Cx, b: Cx) -> bool {
        if a == b {
            return true;
        }
        let (ma, mb) = (cx::abs(a), cx::abs(b));
        let s = if self.by_smaller { ma.min(mb) } else { ma.max(mb) };
        cx::abs(cx::sub(a, b)) < self.ct * s
    }

    /// `<. y`: the largest integer not above y, with a value just under an
    /// integer counting as that integer.
    #[inline(always)]
    pub fn floor(self, y: f64) -> f64 {
        let c = y.ceil();
        if self.eq(y, c) {
            c
        } else {
            y.floor()
        }
    }

    /// `>. y`: the ceiling, with a value just over an integer counting as
    /// that integer.
    #[inline(always)]
    pub fn ceil(self, y: f64) -> f64 {
        let f = y.floor();
        if self.eq(y, f) {
            f
        } else {
            y.ceil()
        }
    }
}

/// The effect-free half of the execution context. Copyable, so a path that
/// runs cells on other threads can carry it there; the output sink cannot
/// go along, which is what keeps those paths pure by construction.
#[derive(Clone, Copy, Debug)]
pub struct EvalCfg {
    pub agreement: Agreement,
    pub fmt: FmtOpts,
    /// Comparison tolerance, from the dialect; `u!.n` overrides it inside
    /// the verb it is attached to.
    pub tol: Tol,
}

impl EvalCfg {
    /// Run `f` with a context whose sink is never reached, and whose names
    /// are empty. Only a verb that [`Verb::is_pure`] accepted is given one
    /// of these, and an explicit definition — the only thing that reads
    /// names — is never pure.
    fn pure<R>(self, f: impl FnOnce(&mut Ctx<'_>) -> R) -> R {
        let mut sink = |_: &str| debug_assert!(false, "a pure verb wrote to the output sink");
        let mut env = Env::new(Vec::new());
        f(&mut Ctx { cfg: self, out: &mut sink, env: &mut env, device: None })
    }
}

/// How deep explicit definitions may call each other before libjay stops
/// them. Recursion that runs away is a program bug; the diagnostic says so
/// rather than letting the process die on a stack overflow.
///
/// The number is set by the machine stack, not by the languages: one level
/// of a definition costs about 24 kB of stack in an unoptimised build, so
/// the guard has to fire well inside the 2 MiB a small thread gets. It can
/// rise when the evaluator's frames shrink.
pub const RECURSION_LIMIT: usize = 64;

/// The names a running program can reach: the values it has assigned, the
/// verbs it has named, and the arguments bound to its parameters.
///
/// An explicit definition runs with a frame of its own on top: J's `=.`
/// writes there and `=:` writes to the globals, and a name is looked for in
/// the frame before the globals. Frames do not nest — a definition called
/// from another sees only its own locals, which is what both references do.
pub struct Env {
    globals: HashMap<String, Array>,
    frames: Vec<HashMap<String, Array>>,
    /// The definitions currently running, innermost last; J's `$:` and
    /// APL's `∇` name the last of them.
    running: Vec<std::sync::Arc<crate::ir::ExplicitDef>>,
    verbs: HashMap<String, Verb>,
    args: Vec<Array>,
}

impl Env {
    pub fn new(args: Vec<Array>) -> Env {
        Env {
            globals: HashMap::new(),
            frames: Vec::new(),
            running: Vec::new(),
            verbs: HashMap::new(),
            args,
        }
    }

    pub fn get(&self, name: &str) -> Option<Array> {
        if let Some(frame) = self.frames.last() {
            if let Some(v) = frame.get(name) {
                return Some(v.clone());
            }
        }
        self.globals.get(name).cloned()
    }

    pub fn assign(&mut self, name: String, value: Array, scope: crate::ir::Scope) {
        if scope == crate::ir::Scope::LocalDefault && self.get(&name).is_some() {
            return;
        }
        let target = match (scope, self.frames.last_mut()) {
            (crate::ir::Scope::Local | crate::ir::Scope::LocalDefault, Some(frame)) => frame,
            _ => &mut self.globals,
        };
        target.insert(name, value);
    }

    pub fn define(&mut self, name: String, verb: Verb) {
        self.verbs.insert(name, verb);
    }

    pub fn verb(&self, name: &str) -> Option<&Verb> {
        self.verbs.get(name)
    }

    pub fn arg(&self, i: usize) -> Result<Array> {
        self.args
            .get(i)
            .cloned()
            .ok_or_else(|| Error::internal("a parameter was read where none is bound"))
    }

    /// Start a definition's frame. Fails rather than overflowing the stack.
    pub fn enter(
        &mut self,
        frame: HashMap<String, Array>,
        def: std::sync::Arc<crate::ir::ExplicitDef>,
        span: Span,
    ) -> Result<()> {
        if self.frames.len() >= RECURSION_LIMIT {
            return Err(Error::new(
                ErrorKind::Domain,
                format!("explicit definitions called each other more than {RECURSION_LIMIT} deep"),
                Some(span),
            )
            .note("a definition that recurses needs a case that stops"));
        }
        self.frames.push(frame);
        self.running.push(def);
        Ok(())
    }

    /// End a definition's frame and hand back the names it assigned.
    pub fn leave(&mut self) -> HashMap<String, Array> {
        self.running.pop();
        self.frames.pop().unwrap_or_default()
    }

    /// The innermost definition now running; `$:` and `∇` name it.
    pub fn current_def(&self) -> Option<std::sync::Arc<crate::ir::ExplicitDef>> {
        self.running.last().cloned()
    }
}

/// Execution context threaded through evaluation.
pub struct Ctx<'a> {
    pub cfg: EvalCfg,
    /// Sink for explicit output (`echo`, `⎕←`). stdout by default per the
    /// sandbox contract; the host may redirect.
    pub out: &'a mut dyn FnMut(&str),
    /// The names the program has bound so far.
    pub env: &'a mut Env,
    /// Where the run was placed. None is the CPU, which is also what every
    /// path that cannot use a device does; only a fused node reads it.
    pub device: Option<&'a crate::device::Device>,
}

impl Ctx<'_> {
    /// Run `f` in this context with the comparison tolerance replaced.
    fn with_tol<R>(&mut self, tol: Tol, f: impl FnOnce(&mut Ctx<'_>) -> R) -> R {
        let cfg = EvalCfg { tol, ..self.cfg };
        f(&mut Ctx { cfg, out: &mut *self.out, env: &mut *self.env, device: self.device })
    }
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
    /// J `j. y`: `0j1 * y`. Always complex.
    Imaginary,
    /// J `r. y`: `^ 0j1 * y`, the unit complex at angle y. Always complex.
    Polar,
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
    /// J `x j. y`: `x + 0j1 * y`. Always complex.
    MakeComplex,
    /// J `x r. y`: `x * ^ 0j1 * y`, i.e. polar coordinates. Always complex.
    PolarBy,
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
    /// J `I.` / APL `⍸`: index `i` repeated `y[i]` times. J applies at
    /// rank 1; APL applies whole, and answers a rank-2-or-higher argument
    /// with one boxed coordinate vector per occurrence.
    Indices { origin: i64, boxed_coords: bool },
    /// J `i:`: the integers from `-y` to `y`, one step apart.
    Steps,
    /// J `x:`: the argument in the exact types — extended when every value
    /// is whole, rational otherwise.
    ToExact,
    /// J `p:`: the y-th prime, counting from zero.
    NthPrime,
    /// J `q:`: y's prime factors, ascending, with multiplicity.
    PrimeFactors,
    /// J `%.` / APL `⌹`: the inverse, or the least-squares pseudo-inverse.
    MatrixInverse,
    /// J `?` / `?.` and APL `?`: roll. Each element of y is replaced by a
    /// random value below it, counted from `origin`. `fixed` restarts the
    /// generator at its fixed seed, which is J's `?.`; `float_at_zero` is
    /// J's `? 0`, a uniform double, where APL refuses a zero.
    Roll { origin: i64, fixed: bool, float_at_zero: bool },
    /// J `+. y` (rectangular) and `*. y` (polar): the two parts of a
    /// complex number as a two-element vector, which becomes a new trailing
    /// axis. A real argument is the pair `y 0` / `|y| 0`.
    ComplexParts { polar: bool },
    /// J `=`: self-classify — one row per distinct item, holding 1 where
    /// that item stands among y's items.
    SelfClassify,
    /// J `~:` / APL `≠`: nub sieve — 1 at each item that has not occurred
    /// before.
    NubSieve,
    /// J `u:` / APL `⎕UCS`: codepoints become characters, characters become
    /// their codepoints. `pass_chars` is J's monad, which answers characters
    /// with themselves rather than converting them.
    Unicode { pass_chars: bool },
    /// J `;:`: J's own tokeniser over a character list, one box per word.
    Words,
    /// J `L.`: the boxing level — 0 for anything unboxed, one more than the
    /// deepest content otherwise.
    LevelOf,
    /// J `A.`: the anagram index of the permutation y's items rank as.
    AnagramIndex,
    /// J `C.`: a direct permutation as its cycles, or a boxed list of
    /// cycles as the direct permutation. The argument's type decides which.
    CycleForm,
    /// APL `↓`: split — each major cell of y enclosed, the leading axis
    /// becoming the shape of the result.
    Split,
    /// J `". y` / APL `⍎ y`: compile the characters of y as a program of
    /// this language and run it here, over the names the caller already
    /// has. Nothing else about the sandbox changes: the nested program can
    /// reach exactly what the outer one can.
    Execute { apl: bool, origin: i64 },
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
    /// J `x I. y` / APL `x ⍸ y`: which interval of the ascending x each cell
    /// of y falls in. The field is what the language adds to the count of
    /// items below it: nothing in J, `⎕IO - 1` in APL.
    IntervalIndex { offset: i64 },
    /// J `x i: y`: where each cell of y LAST sits among the items of x.
    IndexOfLast { origin: i64 },
    /// J `x %. y` / APL `x ⌹ y`: the least-squares solution of `y a = x`.
    MatrixDivide,
    /// APL `x ⊂ y`: partitioned enclose — a 1 in x opens a partition, a 0
    /// continues it, and a leading run of 0s drops those items.
    PartitionEnclose,
    /// APL `x ⌷ y`: one scalar index per axis of y.
    Squad { origin: i64 },
    /// One bracket slot of APL indexing: axis `axis` of y selected by x.
    /// `rank`, when it is not zero, is the number of slots the brackets
    /// held, checked by the slot that sees the whole array.
    SelectAxis { axis: usize, rank: usize, origin: i64 },
    /// J `x {:: y`: follow the path x into y, opening a level a step.
    Fetch,
    /// J `x x: y`: which exact form. 1 is the rational one, 2 the pair of
    /// numerator and denominator, `_1` the conversion back to a machine
    /// number, `_2` the argument unchanged.
    ExactForm,
    /// J `x ? y` / `x ?. y` and APL `x ? y`: deal — x distinct values from
    /// the y below `origin + y`.
    Deal { origin: i64, fixed: bool },
    /// J `+:` and `*:` / APL `⍱` and `⍲`: the two boolean operations that
    /// have no other reading. Both arguments must be 0 or 1.
    Boolean(BoolDyad),
    /// J `x -. y` / APL `x ~ y`: the items of x that are not items of y.
    Less,
    /// APL `x ∪ y`: x's items, then y's items that x does not already have.
    Union,
    /// APL `x ∩ y`: the items of x that y also has, in x's order.
    Intersect,
    /// J `x A. y`: y's items under the x-th permutation of the items, the
    /// permutations counted in lexicographic order.
    AnagramFrom,
    /// J `x C. y`: y's items permuted by x — a direct permutation, or a
    /// boxed list of cycles.
    Permute,
    /// J `x E. y` / APL `x ⍷ y`: 1 at each position of y where a copy of x
    /// begins.
    FindSeq,
    /// J `x u: y`: which conversion — 3 and 4 take characters to
    /// codepoints, 8 and 10 take codepoints to characters.
    UnicodeForm,
    /// J `x p: y`: which fact about primes — `_1` counts the primes below
    /// y, 0 asks whether y is composite, 1 whether it is prime, and `x` of
    /// magnitude 4 steps to the next or previous prime.
    PrimeMeta,
    /// J `x q: y`: the exponents of the first x primes in y, or, for `__`,
    /// the distinct primes over their exponents as a 2-row table.
    PrimeExponents,
    /// APL `x ⊃ y`: pick — follow the path x into y, opening a level a step.
    Pick { origin: i64 },
    /// APL `x \ y` and `x ⍀ y`: expand — a 1 in x takes the next item of y,
    /// a 0 puts a fill in its place.
    Expand,
    NotYet(&'static str),
    None,
}

/// The dyadic operations that read and write booleans and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolDyad {
    /// J `+:`, APL `⍱`: neither.
    Nor,
    /// J `*:`, APL `⍲`: not both.
    Nand,
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
    /// J `u!.n`: apply u with the comparison tolerance replaced by n.
    Fit(Box<Verb>, f64),
    /// J `x m} y`: y with the items at the indices m replaced by x.
    Amend(Array),
    /// J `u/.`: the key dyadically (u over each group of items sharing a
    /// key), the oblique monadically (u over each anti-diagonal).
    Key(Box<Verb>),
    /// J `u;.n`: cut — u over the intervals a fret marks out.
    Cut(Box<Verb>, i64),
    /// J `u^:v`: v's value at the arguments is the number of applications.
    PowerV(Box<Verb>, Box<Verb>),
    /// APL `f⍣g`: apply f until `new g old` holds.
    PowerUntil(Box<Verb>, Box<Verb>),
    /// APL `f[k]`: f along axis k. The axis is brought to the front, f
    /// applies to the leading axis, and a result of the argument's own rank
    /// has the axis put back where it was.
    AlongAxis(Box<Verb>, usize),
    /// An explicit definition: a body of sentences run with the arguments
    /// bound to names. J's `3 : '…'`, `4 : '…'` and `{{ … }}`, APL's `{…}`
    /// and `∇`-defined functions.
    Explicit(Arc<crate::ir::ExplicitDef>),
    /// J `$:`, APL `∇`: the definition lexically containing the reference,
    /// found at run time as the innermost one then running.
    SelfRef,
    /// A verb named earlier in the program, looked up when it is applied so
    /// that a definition can call itself by its own name.
    Named(String),
    /// J `u :. v`: u, with v declared to be its obverse. The declaration is
    /// what [`obverse`] answers with; applying the verb applies u.
    WithObverse(Box<Verb>, Box<Verb>),
    /// J `m@.v`: agenda — v's value at the arguments picks which of the
    /// gerund's verbs to apply.
    Agenda(Vec<Verb>, Box<Verb>),
    /// J `u :: v`: adverse — apply u, and if the language refuses it, apply
    /// v to the same arguments instead. A gap in libjay is not an error the
    /// program may handle, and goes straight through.
    Adverse(Box<Verb>, Box<Verb>),
    /// APL `f∘g` (beside): monad `f (g y)`, dyad `x f (g y)`. g prepares the
    /// right argument and the left one arrives untouched, which is what
    /// separates it from `⍥` (this crate's [`Verb::Compose`]).
    Beside(Box<Verb>, Box<Verb>),
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
            Verb::Fit(v, _) => v.ranks(),
            // Amend reads the whole argument, and the rest run their own
            // verb over the argument as a whole.
            Verb::Amend(_)
            | Verb::Key(_)
            | Verb::Cut(..)
            | Verb::PowerV(..)
            | Verb::PowerUntil(..)
            | Verb::AlongAxis(..) => [RANK_INF, RANK_INF, RANK_INF],
            Verb::WithObverse(v, _) | Verb::Adverse(v, _) => v.ranks(),
            Verb::Beside(..) => [RANK_INF, RANK_INF, RANK_INF],
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
            Verb::Fit(v, n) => format!("{}!.{n}", v.name()),
            Verb::Amend(_) => "(m})".to_string(),
            Verb::Key(v) => format!("{}/.", v.name()),
            Verb::Cut(v, n) => format!("{};.{n}", v.name()),
            Verb::PowerV(v, w) => format!("{}^:{}", v.name(), w.name()),
            Verb::PowerUntil(v, w) => format!("{}⍣{}", v.name(), w.name()),
            Verb::AlongAxis(v, k) => format!("{}[{k}]", v.name()),
            Verb::Explicit(d) => d.name.clone(),
            Verb::SelfRef => "$:".to_string(),
            Verb::Named(n) => n.clone(),
            Verb::WithObverse(v, w) => format!("({}:.{})", v.name(), w.name()),
            Verb::Adverse(v, w) => format!("({}::{})", v.name(), w.name()),
            Verb::Beside(f, g) => format!("({}∘{})", f.name(), g.name()),
            Verb::Agenda(vs, w) => {
                let names: Vec<String> = vs.iter().map(Verb::name).collect();
                format!("({}@.{})", names.join("`"), w.name())
            }
        }
    }

    /// True when the verb's meaning depends on the comparison tolerance —
    /// the comparisons, the searches that use them, and the two roundings.
    /// `u!.n` is only the tolerance conjunction for these; on anything else
    /// J's `!.` specifies a fill instead, which is a separate feature.
    pub fn uses_tolerance(&self) -> bool {
        match self {
            Verb::Prim(p) => {
                matches!(
                    p.monad,
                    MonadOp::Scalar(ScalarMonad::Floor)
                        | MonadOp::Scalar(ScalarMonad::Ceil)
                        | MonadOp::Nub
                ) || matches!(
                    p.dyad,
                    DyadOp::Scalar(
                        ScalarDyad::Eq
                            | ScalarDyad::Ne
                            | ScalarDyad::Lt
                            | ScalarDyad::Le
                            | ScalarDyad::Gt
                            | ScalarDyad::Ge
                    ) | DyadOp::Match
                        | DyadOp::NotMatch
                        | DyadOp::MemberJ
                        | DyadOp::MemberApl
                        | DyadOp::IndexOf { .. }
                        | DyadOp::IndexOfLast { .. }
                )
            }
            Verb::Rank(v, _)
            | Verb::Reduce(v)
            | Verb::Windowed(v, _)
            | Verb::Commute(v)
            | Verb::PowerN(v, _)
            | Verb::BondLeft(_, v)
            | Verb::BondRight(v, _)
            | Verb::Each(v, _)
            | Verb::Fit(v, _)
            | Verb::Key(v)
            | Verb::Cut(v, _)
            | Verb::AlongAxis(v, _) => v.uses_tolerance(),
            Verb::PowerV(v, w) | Verb::PowerUntil(v, w) => {
                v.uses_tolerance() || w.uses_tolerance()
            }
            // An explicit definition's body is a program of its own; `!.`
            // has no reach into it.
            Verb::Amend(_) | Verb::Explicit(_) | Verb::SelfRef | Verb::Named(_) => false,
            Verb::WithObverse(v, _) => v.uses_tolerance(),
            Verb::Adverse(v, w) | Verb::Beside(v, w) => {
                v.uses_tolerance() || w.uses_tolerance()
            }
            Verb::Agenda(vs, w) => {
                w.uses_tolerance() || vs.iter().any(Verb::uses_tolerance)
            }
            Verb::Fork(f, g, h) => {
                f.uses_tolerance() || g.uses_tolerance() || h.uses_tolerance()
            }
            Verb::NounFork(_, g, h)
            | Verb::Hook(g, h)
            | Verb::Atop(g, h)
            | Verb::Compose(g, h) => g.uses_tolerance() || h.uses_tolerance(),
        }
    }

    /// True when applying this verb does nothing beyond producing its
    /// result. Output (`echo`, `⎕←`) is the only effect a verb can have, and
    /// only a pure verb may have its cells run out of order on several
    /// threads. Deliberately conservative: a new effect must be added here.
    pub fn is_pure(&self) -> bool {
        match self {
            // Output and the random source are the two effects a verb can
            // have; both fix the order its cells must run in.
            Verb::Prim(p) => {
                !matches!(p.monad, MonadOp::Echo | MonadOp::Roll { .. })
                    && !matches!(p.dyad, DyadOp::Deal { .. })
            }
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
            Verb::BondLeft(_, v) | Verb::BondRight(v, _) | Verb::Each(v, _) | Verb::Fit(v, _) => {
                v.is_pure()
            }
            Verb::Key(v) | Verb::Cut(v, _) | Verb::AlongAxis(v, _) => v.is_pure(),
            Verb::PowerV(v, w) | Verb::PowerUntil(v, w) => v.is_pure() && w.is_pure(),
            Verb::WithObverse(v, _) => v.is_pure(),
            Verb::Adverse(v, w) | Verb::Beside(v, w) => v.is_pure() && w.is_pure(),
            Verb::Agenda(vs, w) => w.is_pure() && vs.iter().all(Verb::is_pure),
            Verb::Amend(_) => true,
            // An explicit definition reads and writes the program's names,
            // so its cells can never be run out of order on other threads —
            // whatever its body does. `ExplicitDef::pure` records whether
            // the body itself has an effect; this is the stronger question.
            Verb::Explicit(_) | Verb::SelfRef | Verb::Named(_) => false,
        }
    }

    /// Full monadic application including rank/frame machinery.
    pub fn monad(&self, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
        match self {
            Verb::Prim(p) => {
                // Scalar verbs have cell rank 0: the cells are the elements,
                // so the whole buffer is one elementwise pass.
                if let MonadOp::Scalar(op) = p.monad {
                    return scalar_monad(op, y, ctx.cfg.tol, span);
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
            Verb::Fit(v, n) => {
                let tol = Tol { ct: *n, ..ctx.cfg.tol };
                ctx.with_tol(tol, |c| v.monad(y, c, span))
            }
            // `m} y` with one index is J's item selection.
            Verb::Amend(m) => {
                if m.rank() != 0 || y.rank() > 1 {
                    return Err(Error::new(
                        ErrorKind::Rank,
                        "selecting with m} takes one index into a list",
                        Some(span),
                    ));
                }
                from_index(m, y, span)
            }
            Verb::Key(u) => oblique(u, y, ctx, span),
            Verb::Cut(u, n) => cut(u, None, y, *n, ctx, span),
            Verb::PowerV(u, v) => power_v(u, v, None, y, ctx, span),
            Verb::PowerUntil(u, v) => power_until(u, v, y, ctx, span),
            Verb::AlongAxis(u, k) => along_axis(u, None, y, *k, ctx, span),
            Verb::Explicit(d) => crate::ir::call_explicit(d, None, y, ctx, span),
            Verb::SelfRef => {
                let d = self_ref(ctx, span)?;
                crate::ir::call_explicit(&d, None, y, ctx, span)
            }
            Verb::Named(n) => named_verb(ctx, n, span)?.monad(y, ctx, span),
            Verb::WithObverse(v, _) => v.monad(y, ctx, span),
            Verb::Adverse(v, w) => match v.monad(y, ctx, span) {
                Err(e) if e.kind != ErrorKind::NotYet => w.monad(y, ctx, span),
                other => other,
            },
            Verb::Beside(f, g) => {
                let r = g.monad(y, ctx, span)?;
                f.monad(&r, ctx, span)
            }
            Verb::Agenda(vs, w) => {
                agenda_pick(vs, w, None, y, ctx, span)?.monad(y, ctx, span)
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
            // `x u\. y` is the outfix: u over y with each run of x
            // consecutive items left out.
            Verb::Windowed(u, WindowKind::Suffix) => outfix(u, x, y, ctx, span),
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
            Verb::Fit(v, n) => {
                let tol = Tol { ct: *n, ..ctx.cfg.tol };
                ctx.with_tol(tol, |c| v.dyad(x, y, c, span))
            }
            Verb::Amend(m) => amend(m, x, y, span),
            Verb::Key(u) => key(u, x, y, ctx, span),
            Verb::Cut(u, n) => cut(u, Some(x), y, *n, ctx, span),
            Verb::PowerV(u, v) => power_v(u, v, Some(x), y, ctx, span),
            Verb::PowerUntil(..) => {
                Err(Error::not_yet("dyadic power with a function operand (x f⍣g y)", span))
            }
            Verb::AlongAxis(u, k) => along_axis(u, Some(x), y, *k, ctx, span),
            Verb::Explicit(d) => crate::ir::call_explicit(d, Some(x), y, ctx, span),
            Verb::SelfRef => {
                let d = self_ref(ctx, span)?;
                crate::ir::call_explicit(&d, Some(x), y, ctx, span)
            }
            Verb::Named(n) => named_verb(ctx, n, span)?.dyad(x, y, ctx, span),
            Verb::WithObverse(v, _) => v.dyad(x, y, ctx, span),
            Verb::Adverse(v, w) => match v.dyad(x, y, ctx, span) {
                Err(e) if e.kind != ErrorKind::NotYet => w.dyad(x, y, ctx, span),
                other => other,
            },
            Verb::Beside(f, g) => {
                let r = g.monad(y, ctx, span)?;
                f.dyad(x, &r, ctx, span)
            }
            Verb::Agenda(vs, w) => {
                agenda_pick(vs, w, Some(x), y, ctx, span)?.dyad(x, y, ctx, span)
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
                return scalar_dyad(op, x, y, ctx.cfg, span);
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
            Verb::Prim(p) => dyad_op(p, x, y, ctx.cfg, span),
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
/// The definition `$:` or `∇` names: the innermost one now running.
fn self_ref(ctx: &Ctx<'_>, span: Span) -> Result<Arc<crate::ir::ExplicitDef>> {
    ctx.env.current_def().ok_or_else(|| {
        Error::new(
            ErrorKind::Value,
            "self-reference outside an explicit definition",
            Some(span),
        )
    })
}

/// A verb the program named earlier, resolved when it is applied.
fn named_verb(ctx: &Ctx<'_>, name: &str, span: Span) -> Result<Verb> {
    ctx.env.verb(name).cloned().ok_or_else(|| {
        Error::new(ErrorKind::Value, format!("undefined verb: {name}"), Some(span))
    })
}

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
        (Data::Ext(a), Data::Ext(b)) => a.push(b[i].clone()),
        (Data::Rat(a), Data::Rat(b)) => a.push(b[i].clone()),
        (Data::F64(a), Data::F64(b)) => a.push(b[i]),
        (Data::Complex(a), Data::Complex(b)) => a.push(b[i]),
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
    catenate(&head, &tail, true, false, span)
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
        return catenate(&one(&item), y, true, false, span);
    }
    let head = if item.dtype() == DType::Box { item } else { Array::boxed(item) };
    catenate(&one(&head), &box_items(y), true, false, span)
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
        Data::Ext(v) => {
            *tmp = v.iter().map(exact::ext_to_f64).collect();
            &tmp[..]
        }
        Data::Rat(v) => {
            *tmp = v.iter().map(Rat::to_f64).collect();
            &tmp[..]
        }
        _ => &[],
    }
}

/// Borrow numeric data as complex, widening into `tmp` when needed.
fn borrow_cx<'a>(d: &'a Data, tmp: &'a mut Vec<Cx>) -> &'a [Cx] {
    match d {
        Data::Complex(v) => v,
        Data::Ext(v) => {
            *tmp = v.iter().map(|x| [exact::ext_to_f64(x), 0.0]).collect();
            &tmp[..]
        }
        Data::Rat(v) => {
            *tmp = v.iter().map(|x| [x.to_f64(), 0.0]).collect();
            &tmp[..]
        }
        Data::F64(v) => {
            *tmp = v.iter().map(|&x| [x, 0.0]).collect();
            &tmp[..]
        }
        Data::I64(v) => {
            *tmp = v.iter().map(|&x| [x as f64, 0.0]).collect();
            &tmp[..]
        }
        Data::Bool(v) => {
            *tmp = v.iter().map(|&x| [x as f64, 0.0]).collect();
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

/// Which of a real pair's operations has no real answer, so the whole pass
/// runs in the complex domain instead. Only the four operations that can
/// leave the reals are asked.
#[inline]
fn escapes_reals(op: ScalarDyad, a: f64, b: f64) -> bool {
    use ScalarDyad::*;
    match op {
        // An integer exponent keeps a negative base real (`_1 ^ 2` is 1).
        Pow => a < 0.0 && b.fract() != 0.0,
        Log => a < 0.0 || b < 0.0,
        Root => b < 0.0,
        Circle => circle_escapes(a, b),
        _ => false,
    }
}

/// The circle functions with no real answer at a real argument. A
/// non-integer k is a domain error, which the real path reports.
#[inline]
fn circle_escapes(k: f64, y: f64) -> bool {
    if k.fract() != 0.0 {
        return false;
    }
    match k as i64 {
        0 | -1 | -2 | -7 => y.abs() > 1.0,
        -4 => y.abs() < 1.0,
        -6 => y < 1.0,
        // The functions built on the imaginary unit, which no real argument
        // escapes.
        8 | -8 | -11 | -12 => true,
        _ => false,
    }
}

/// `k o. y`: the circle function k applied to a real y.
///
/// The table is J's and APL's alike (they share it): 1 2 3 are sine, cosine
/// and tangent, 5 6 7 their hyperbolic counterparts, a negative k inverts the
/// function at |k|, and 0 and 4 are the two Pythagorean forms. 9 to 12 read
/// the parts of a complex number — real, magnitude, imaginary, phase — and
/// are answered here for the reals they also accept. A pair whose answer
/// leaves the reals never reaches this function: [`escapes_reals`] sends the
/// whole pass to the complex path first.
#[inline]
fn circle(k: f64, y: f64, span: Span) -> Result<f64> {
    if k.fract() != 0.0 {
        return Err(Error::domain("the circle function needs an integer left argument", span));
    }
    let complex = || Error::internal("a circle function left the reals on the real path");
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
        // The parts of a number that happens to be real.
        9 | -9 | -10 => y,
        10 => y.abs(),
        11 => 0.0,
        12 => {
            if y < 0.0 {
                std::f64::consts::PI
            } else {
                0.0
            }
        }
        8 | -8 | -11 | -12 => return Err(complex()),
        _ => {
            return Err(Error::domain(
                "the circle functions run from _12 to 12",
                span,
            ));
        }
    })
}

/// One complex step.
#[inline]
fn cx_op(op: ScalarDyad, a: Cx, b: Cx, span: Span) -> Result<Cx> {
    use ScalarDyad::*;
    Ok(match op {
        Add => cx::add(a, b),
        Sub => cx::sub(a, b),
        Mul => cx::mul(a, b),
        DivJ => cx::div(a, b),
        DivApl => {
            if b == cx::ZERO {
                if a == cx::ZERO {
                    cx::ONE
                } else {
                    return Err(Error::domain("division by zero", span));
                }
            } else {
                cx::div(a, b)
            }
        }
        Pow => cx::pow(a, b),
        Log => cx::log(a, b),
        Root => cx::root(a, b),
        Residue => cx::residue(a, b),
        Lcm => cx::lcm(a, b),
        Gcd => cx::gcd(a, b),
        MakeComplex => cx::add(a, cx::mul(cx::I, b)),
        PolarBy => cx::mul(a, cx::exp(cx::mul(cx::I, b))),
        Circle => {
            if a[1] != 0.0 || a[0].fract() != 0.0 {
                return Err(Error::domain(
                    "the circle function needs an integer left argument",
                    span,
                ));
            }
            cx::circle(a[0] as i64, b).ok_or_else(|| {
                Error::domain("the circle functions run from _12 to 12", span)
            })?
        }
        Min | Max => return Err(no_complex_order(span)),
        Binomial => {
            return Err(Error::not_yet("the binomial function on complex numbers", span));
        }
        Eq | Ne | Lt | Le | Gt | Ge => {
            return Err(Error::internal("a comparison in the complex arithmetic path"));
        }
    })
}

/// The complaint an ordering makes about complex operands. Both references
/// refuse it: complex numbers carry no order, only equality.
fn no_complex_order(span: Span) -> Error {
    Error::new(
        ErrorKind::Domain,
        "complex numbers have no order; only equality (=, ~:) applies to them",
        Some(span),
    )
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn dyad_cx_chunk(
    op: ScalarDyad,
    xs: &[Cx],
    xoff: usize,
    xdiv: usize,
    ys: &[Cx],
    yoff: usize,
    ydiv: usize,
    start: usize,
    out: &mut [Cx],
    span: Span,
) -> Result<()> {
    use ScalarDyad::*;
    // The three steps that cannot fail are picked before the loop, so the
    // pass is one operation per element rather than a match per element.
    macro_rules! plain {
        ($step:expr) => {{
            zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, out, |a, b, slot: &mut Cx| {
                *slot = $step(a, b);
                true
            });
            return Ok(());
        }};
    }
    match op {
        Add => plain!(cx::add),
        Sub => plain!(cx::sub),
        Mul => plain!(cx::mul),
        DivJ => plain!(cx::div),
        _ => {}
    }
    let mut err = None;
    zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, out, |a, b, slot: &mut Cx| {
        match cx_op(op, a, b, span) {
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
fn dyad_cx(
    op: ScalarDyad,
    xs: &[Cx],
    xoff: usize,
    xdiv: usize,
    ys: &[Cx],
    yoff: usize,
    ydiv: usize,
    n: usize,
    span: Span,
) -> Result<Vec<Cx>> {
    par::try_fill(n, |start, part| {
        dyad_cx_chunk(op, xs, xoff, xdiv, ys, yoff, ydiv, start, part, span)
    })
}

/// One complex pass over two buffers, widening both to complex first.
#[allow(clippy::too_many_arguments)]
fn complex_dyad_data(
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
    let (mut tx, mut ty) = (Vec::new(), Vec::new());
    let xs = borrow_cx(x, &mut tx);
    let ys = borrow_cx(y, &mut ty);
    Ok(Data::Complex(dyad_cx(op, xs, xoff, xdiv, ys, yoff, ydiv, n, span)?.into()))
}

/// `9 o.` to `12 o.` read a part of a number — real, magnitude, imaginary,
/// phase — so their answers are real however complex the argument was. J
/// reports them as floats rather than as complex values with a zero
/// imaginary part.
fn circle_reads_a_part(x: &Data, xoff: usize, xdiv: usize, n: usize) -> bool {
    if x.dtype() == DType::Complex {
        // A complex left argument selects nothing; the pass reports it.
        return false;
    }
    let mut tmp = Vec::new();
    let xs = borrow_f64(x, &mut tmp);
    (0..n).all(|i| {
        let k = xs[xoff + i / xdiv];
        k.fract() == 0.0 && (9.0..=12.0).contains(&k)
    })
}

/// Does the real pass hold an argument pair whose answer leaves the reals?
/// One extra scan, and only for the four operations that can.
#[allow(clippy::too_many_arguments)]
fn pass_leaves_reals(
    op: ScalarDyad,
    x: &Data,
    xoff: usize,
    xdiv: usize,
    y: &Data,
    yoff: usize,
    ydiv: usize,
    n: usize,
) -> bool {
    use ScalarDyad::*;
    if !matches!(op, Pow | Log | Root | Circle) {
        return false;
    }
    let (mut tx, mut ty) = (Vec::new(), Vec::new());
    let xs = borrow_f64(x, &mut tx);
    let ys = borrow_f64(y, &mut ty);
    (0..n).any(|i| escapes_reals(op, xs[xoff + i / xdiv], ys[yoff + i / ydiv]))
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn dyad_i64_chunk_body(
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

multiversioned! {
    /// One chunk of an integer pass. False means the chunk left i64 and the
    /// caller redoes the whole operation in f64.
    ///
    /// This is one of the loops compiled per CPU feature level: a chunk is
    /// thousands of elements, so choosing the compilation costs nothing
    /// against the pass it chooses.
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
    ) -> bool = dyad_i64_chunk_body;
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
#[inline(always)]
fn dyad_f64_chunk_body(
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

multiversioned! {
    /// One chunk of a float pass, compiled per CPU feature level.
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
    ) -> Result<()> = dyad_f64_chunk_body;
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
    tol: Tol,
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
                let e = arrays_match(&a[xoff + i / xdiv], &b[yoff + i / ydiv], tol);
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
    if DType::promote(dx, dy).is_some_and(DType::is_exact) {
        if let Some(d) = exact_compare_data(op, x, xoff, xdiv, y, yoff, ydiv, n) {
            return Ok(d);
        }
    }
    if dx == DType::Complex || dy == DType::Complex {
        if !equality {
            return Err(no_complex_order(span));
        }
        let (mut tx, mut ty) = (Vec::new(), Vec::new());
        let xs = borrow_cx(x, &mut tx);
        let ys = borrow_cx(y, &mut ty);
        let (out, _) = par::fill(n, |start, part: &mut [u8]| {
            zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, part, |a, b, slot| {
                let e = tol.eq_cx(a, b);
                *slot = if op == Eq { e as u8 } else { !e as u8 };
                true
            })
        });
        return Ok(Data::Bool(out.into()));
    }
    // Floats compare with the dialect's tolerance; integers are exact
    // whatever it is, so the integer pass below is untouched by it.
    let out = if DType::promote(dx, dy) == Some(DType::F64) {
        let (mut tx, mut ty) = (Vec::new(), Vec::new());
        let xs = borrow_f64(x, &mut tx);
        let ys = borrow_f64(y, &mut ty);
        par::fill(n, |start, part: &mut [u8]| {
            zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, part, |a, b, slot| {
                *slot = tol_cmp(op, a, b, tol) as u8;
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

/// One tolerant float comparison.
#[inline(always)]
pub(crate) fn tol_cmp(op: ScalarDyad, a: f64, b: f64, tol: Tol) -> bool {
    use ScalarDyad::*;
    match op {
        Eq => tol.eq(a, b),
        Ne => !tol.eq(a, b),
        Lt => tol.lt(a, b),
        Le => tol.le(a, b),
        Gt => tol.lt(b, a),
        Ge => tol.le(b, a),
        _ => false,
    }
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
    if t == DType::Complex {
        // The Gaussian-integer versions, which is what both references give.
        return complex_dyad_data(op, x, xoff, xdiv, y, yoff, ydiv, n, span);
    }
    if t.is_exact() {
        if let Some(d) = exact_dyad_data(op, t, x, xoff, xdiv, y, yoff, ydiv, n, span)? {
            return Ok(d);
        }
    }
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

// ------------------------------------------------------- the exact types

/// Numeric data widened to rationals. None for a type above the exact part
/// of the tower, which has no exact reading.
fn to_rat_vec(d: &Data) -> Option<Vec<Rat>> {
    Some(match d {
        Data::Bool(v) => v.iter().map(|&b| Rat::from_int(Ext::from(b))).collect(),
        Data::I64(v) => v.iter().map(|&x| Rat::from_int(Ext::from(x))).collect(),
        Data::Ext(v) => v.iter().map(|x| Rat::from_int(x.clone())).collect(),
        Data::Rat(v) => v.to_vec(),
        Data::F64(_) | Data::Complex(_) | Data::Char(_) | Data::Box(_) => return None,
    })
}

/// The elements one pass really reads, as rationals: indices
/// `off .. off + (n-1)/div`, rebased to zero.
///
/// A fold hands the SAME buffer to every step with a different offset, so
/// converting the whole of it each time would make the fold quadratic. The
/// window is the whole buffer in the ordinary elementwise case, and one
/// element in a fold step.
fn rat_window(d: &Data, off: usize, div: usize, n: usize) -> Option<Vec<Rat>> {
    if n == 0 {
        return Some(Vec::new());
    }
    let end = off + (n - 1) / div + 1;
    if off == 0 && end == d.len() {
        return to_rat_vec(d);
    }
    to_rat_vec(&d.slice(off, end))
}

/// A finished exact pass as data: extended when the arguments were extended
/// AND every answer is whole, rational otherwise.
///
/// That one rule is the whole demotion story. It makes `4x % 2` extended and
/// `1x % 3` rational, and it leaves `1r2 - 1r2` rational even though the
/// answer is zero — a rational never falls back down the tower, which is
/// what the reference reports of it.
fn exact_data(t: DType, out: Vec<Rat>) -> Data {
    if t == DType::Ext && out.iter().all(Rat::is_integer) {
        return Data::Ext(out.iter().map(|r| r.to_int().expect("whole")).collect());
    }
    Data::Rat(out.into())
}

/// The complaint a power too large to hold makes.
fn too_large(span: Span) -> Error {
    Error::domain(
        format!(
            "the exact result needs more than {} bits; use floats for a value this large",
            exact::MAX_BITS
        ),
        span,
    )
}

/// `a ^ b` in the exact types. None when the answer is not exact — a
/// fractional exponent, or zero raised to a negative one.
fn exact_pow(a: &Rat, b: &Rat, span: Span) -> Result<Option<Rat>> {
    let Some(e) = b.to_int().as_ref().and_then(exact::ext_to_i64) else {
        return Ok(None);
    };
    if let Some(v) = a.pow(e) {
        return Ok(Some(v));
    }
    // `pow` declines for two reasons; only one of them is an error.
    if a.is_zero() && e < 0 { Ok(None) } else { Err(too_large(span)) }
}

/// One elementwise dyadic pass in the exact types. `Ok(None)` means the
/// operation has no exact answer for these arguments, and the caller widens
/// to float exactly as it would for a machine integer that overflowed.
#[allow(clippy::too_many_arguments)]
fn exact_dyad_data(
    op: ScalarDyad,
    t: DType,
    x: &Data,
    xoff: usize,
    xdiv: usize,
    y: &Data,
    yoff: usize,
    ydiv: usize,
    n: usize,
    span: Span,
) -> Result<Option<Data>> {
    use ScalarDyad::*;
    let (Some(xs), Some(ys)) = (rat_window(x, xoff, xdiv, n), rat_window(y, yoff, ydiv, n))
    else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let a = &xs[i / xdiv];
        let b = &ys[i / ydiv];
        let v = match op {
            Add => a.add(b),
            Sub => a.sub(b),
            Mul => a.mul(b),
            // A zero divisor is an infinity, which no rational spells.
            DivJ | DivApl => match a.div(b) {
                Some(v) => v,
                None => return Ok(None),
            },
            Min => a.min(b).clone(),
            Max => a.max(b).clone(),
            Residue => exact::rat_residue(a, b),
            Gcd => exact::rat_gcd(a, b),
            Lcm => exact::rat_lcm(a, b),
            Pow => match exact_pow(a, b, span)? {
                Some(v) => v,
                None => return Ok(None),
            },
            Binomial => match (a.to_int(), b.to_int()) {
                (Some(k), Some(m)) => match exact::ext_binomial(&k, &m) {
                    Some(v) => Rat::from_int(v),
                    None => return Ok(None),
                },
                _ => return Ok(None),
            },
            // An exact root exists only between whole numbers: the
            // reference answers `3 %: 8r27` with a float, not with `2r3`.
            Root if t == DType::Ext => {
                let (Some(k), Some(m)) = (a.to_int(), b.to_int()) else {
                    return Ok(None);
                };
                let Some(k) = exact::ext_to_i64(&k).and_then(|k| u32::try_from(k).ok()) else {
                    return Ok(None);
                };
                match exact::exact_root(k, &m) {
                    Some(v) => Rat::from_int(v),
                    None => return Ok(None),
                }
            }
            Root | Log | Circle | MakeComplex | PolarBy => return Ok(None),
            // Comparisons never reach here; `compare_data` takes them.
            Eq | Ne | Lt | Le | Gt | Ge => return Ok(None),
        };
        out.push(v);
    }
    Ok(Some(exact_data(t, out)))
}

/// Elementwise monadic application in the exact types. `Ok(None)` widens to
/// float, as in the dyadic pass.
fn exact_monad(op: ScalarMonad, y: &Array) -> Option<Array> {
    use ScalarMonad::*;
    let v = to_rat_vec(&y.data)?;
    let shape = y.shape.clone();
    // The three that answer with a whole number whatever they were given:
    // `<. 7r2` is the extended 3, not the rational 3.
    if matches!(op, Floor | Ceil | Signum) {
        let out: Vec<Ext> = v
            .iter()
            .map(|r| match op {
                Floor => r.floor(),
                Ceil => r.ceil(),
                _ => r.signum(),
            })
            .collect();
        return Some(Array { shape, data: Data::Ext(out.into()) });
    }
    let two = Rat::from_int(Ext::from(2));
    let mut out = Vec::with_capacity(v.len());
    for r in &v {
        let value = match op {
            Conj => r.clone(),
            Neg => r.neg(),
            Abs => r.abs(),
            Recip => r.recip()?,
            Inc => r.add(&Rat::one()),
            Dec => r.sub(&Rat::one()),
            OneMinus => Rat::one().sub(r),
            Double => r.add(r),
            Halve => r.div(&two).expect("two is not zero"),
            Square => r.mul(r),
            Sqrt => r.sqrt()?,
            Factorial => Rat::from_int(r.to_int().as_ref().and_then(exact::ext_factorial)?),
            // No exact answer: the transcendentals, the two that make a
            // complex value, and logical negation.
            Exp | Ln | Pi | Imaginary | Polar | Not => return None,
            Floor | Ceil | Signum => unreachable!("handled above"),
        };
        out.push(value);
    }
    Some(Array { shape, data: exact_data(y.dtype(), out) })
}

/// `x: y`: the argument in the exact types. Whole values become extended
/// integers; anything else becomes the simplest rational within the
/// dialect's comparison tolerance of it, so `x: 0.1` is `1r10` rather than
/// the binary fraction a double really holds.
fn to_exact(y: &Array, span: Span) -> Result<Array> {
    let data = match &y.data {
        Data::Ext(_) | Data::Rat(_) => return Ok(y.clone()),
        Data::Bool(v) => Data::Ext(v.iter().map(|&b| Ext::from(b)).collect()),
        Data::I64(v) => Data::Ext(v.iter().map(|&x| Ext::from(x)).collect()),
        Data::F64(v) => {
            let mut out = Vec::with_capacity(v.len());
            for &x in v.iter() {
                out.push(exact::f64_to_rat(x).ok_or_else(|| {
                    Error::domain("an infinity has no exact value", span)
                })?);
            }
            exact_data(DType::Ext, out)
        }
        Data::Complex(_) | Data::Char(_) | Data::Box(_) => {
            return Err(Error::domain(
                format!("x: needs real numbers, not {} data", y.dtype().name()),
                span,
            ));
        }
    };
    Ok(Array { shape: y.shape.clone(), data })
}

/// `_1 x: y`: an exact value back as a machine number — an extended integer
/// as an integer where it fits, a rational as a float.
fn from_exact(y: &Array) -> Array {
    let shape = y.shape.clone();
    match &y.data {
        Data::Ext(v) => match v.iter().map(exact::ext_to_i64).collect::<Option<Vec<i64>>>() {
            Some(out) => Array { shape, data: Data::I64(out.into()) },
            None => Array { shape, data: Data::F64(v.iter().map(exact::ext_to_f64).collect()) },
        },
        Data::Rat(v) => Array { shape, data: Data::F64(v.iter().map(Rat::to_f64).collect()) },
        _ => y.clone(),
    }
}

/// `x x: y`: the exact form named by x.
fn exact_form(x: &Array, y: &Array, span: Span) -> Result<Array> {
    match one_whole(x, "the form x: converts to", span)? {
        1 => {
            let e = to_exact(y, span)?;
            e.cast(DType::Rat).ok_or_else(|| Error::internal("an exact value has no rational form"))
        }
        2 => {
            let e = to_exact(y, span)?;
            let v = to_rat_vec(&e.data).ok_or_else(|| Error::internal("x: gave an inexact value"))?;
            let mut out = Vec::with_capacity(2 * v.len());
            for r in &v {
                out.push(r.numer().clone());
                out.push(r.denom().clone());
            }
            let mut shape = y.shape.clone();
            shape.push(2);
            Ok(Array::new(shape, Data::Ext(out.into())))
        }
        -1 => Ok(from_exact(y)),
        // The one that leaves an inexact argument alone.
        -2 => {
            if !y.dtype().is_numeric() {
                return Err(Error::domain(
                    format!("x: needs real numbers, not {} data", y.dtype().name()),
                    span,
                ));
            }
            Ok(y.clone())
        }
        n => Err(Error::domain(
            format!("x: converts to form 1, 2, _1 or _2, not {n}"),
            span,
        )),
    }
}

/// Exact comparison of two exact buffers. No tolerance applies: two exact
/// values are equal when they are the same number, which is why
/// `(10x^30) = 1 + 10x^30` is 0 where the float answer would be 1.
#[allow(clippy::too_many_arguments)]
fn exact_compare_data(
    op: ScalarDyad,
    x: &Data,
    xoff: usize,
    xdiv: usize,
    y: &Data,
    yoff: usize,
    ydiv: usize,
    n: usize,
) -> Option<Data> {
    let (xs, ys) = (rat_window(x, xoff, xdiv, n)?, rat_window(y, yoff, ydiv, n)?);
    let out: Vec<u8> = (0..n)
        .map(|i| {
            let ord = xs[i / xdiv].cmp(&ys[i / ydiv]);
            cmp_result(op, Some(ord)) as u8
        })
        .collect();
    Some(Data::Bool(out.into()))
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
    tol: Tol,
    span: Span,
) -> Result<Data> {
    use ScalarDyad::*;
    if matches!(op, Eq | Ne | Lt | Le | Gt | Ge) {
        return compare_data(op, x, xoff, xdiv, y, yoff, ydiv, n, tol, span);
    }
    if matches!(op, Lcm | Gcd) {
        return lcm_gcd_data(op, x, xoff, xdiv, y, yoff, ydiv, n, span);
    }
    let t = arith_type(x.dtype(), y.dtype(), span)?;
    if t.is_exact() {
        if let Some(d) = exact_dyad_data(op, t, x, xoff, xdiv, y, yoff, ydiv, n, span)? {
            return Ok(d);
        }
        // No exact answer: widen, exactly as an integer overflow does.
    }
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
    if t == DType::Complex
        || matches!(op, MakeComplex | PolarBy)
        || pass_leaves_reals(op, x, xoff, xdiv, y, yoff, ydiv, n)
    {
        let data = complex_dyad_data(op, x, xoff, xdiv, y, yoff, ydiv, n, span)?;
        if op == Circle && circle_reads_a_part(x, xoff, xdiv, n) {
            if let Data::Complex(v) = &data {
                return Ok(Data::F64(v.iter().map(|z| z[0]).collect()));
            }
        }
        return Ok(data);
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
    cfg: EvalCfg,
    span: Span,
) -> Result<Array> {
    let p = agree(&x.shape, &y.shape, &x.shape, &y.shape, cfg.agreement, span)?;
    let data =
        scalar_dyad_data(op, &x.data, 0, p.x_div, &y.data, 0, p.y_div, p.n, cfg.tol, span)?;
    Ok(Array::new(p.frame, data))
}

/// Is `v` exactly representable as an i64?
fn fits_i64(v: f64) -> bool {
    v.is_finite() && v >= i64::MIN as f64 && v < i64::MAX as f64
}

/// Does a real argument have no real answer under this monad?
fn monad_leaves_reals(op: ScalarMonad, d: &Data) -> bool {
    use ScalarMonad::*;
    match op {
        // The two that make a complex number out of a real one.
        Imaginary | Polar => d.dtype().is_numeric(),
        Sqrt | Ln => match d {
            Data::I64(v) => v.iter().any(|&x| x < 0),
            Data::F64(v) => v.iter().any(|&x| x < 0.0),
            Data::Ext(v) => v.iter().any(|x| x.sign() == num_bigint::Sign::Minus),
            Data::Rat(v) => v.iter().any(|x| x < &Rat::zero()),
            _ => false,
        },
        _ => false,
    }
}

/// Elementwise monadic application in the complex domain.
fn complex_monad(op: ScalarMonad, y: &Array, span: Span) -> Result<Array> {
    use ScalarMonad::*;
    let mut tmp = Vec::new();
    let v = borrow_cx(&y.data, &mut tmp);
    if y.count() > 0 && v.is_empty() {
        return Err(wrong_type(y.dtype(), span));
    }
    let data = match op {
        // Magnitude is the one that leaves the complex domain again.
        Abs => Data::F64(par::map(v, |&z| cx::abs(z)).into()),
        Not => return Err(Error::domain("logical negation needs values of 0 or 1", span)),
        Factorial => {
            return Err(Error::not_yet("the factorial of a complex number", span));
        }
        _ => {
            let step: fn(Cx) -> Cx = match op {
                Conj => cx::conj,
                Neg => cx::neg,
                Signum => cx::signum,
                Recip => cx::recip,
                Sqrt => cx::sqrt,
                Exp => cx::exp,
                Ln => cx::ln,
                Floor => cx::floor,
                Ceil => cx::ceil,
                OneMinus => |z| cx::sub(cx::ONE, z),
                Inc => |z| cx::add(z, cx::ONE),
                Dec => |z| cx::sub(z, cx::ONE),
                Double => |z| cx::add(z, z),
                Halve => |z| [z[0] / 2.0, z[1] / 2.0],
                Square => |z| cx::mul(z, z),
                Pi => |z| [std::f64::consts::PI * z[0], std::f64::consts::PI * z[1]],
                Imaginary => |z| cx::mul(cx::I, z),
                Polar => |z| cx::exp(cx::mul(cx::I, z)),
                Abs | Not | Factorial => unreachable!("handled above"),
            };
            Data::Complex(par::map(v, |&z| step(z)).into())
        }
    };
    Ok(Array { shape: y.shape.clone(), data })
}

/// Elementwise monadic application to a whole array.
fn scalar_monad(op: ScalarMonad, y: &Array, tol: Tol, span: Span) -> Result<Array> {
    use ScalarMonad::*;
    let d = &y.data;
    if d.dtype() == DType::Complex || monad_leaves_reals(op, d) {
        return complex_monad(op, y, span);
    }
    if d.dtype().is_exact() {
        if let Some(a) = exact_monad(op, y) {
            return Ok(a);
        }
        // No exact answer: the float pass below takes over.
    }
    // The float-only operations borrow float data as it lies; anything else
    // is widened once into `tmp` first.
    let mut tmp = Vec::new();
    let data = match op {
        // Conjugation is the identity on reals.
        Conj if d.dtype().is_numeric() => d.clone(),
        Conj => return Err(wrong_type(d.dtype(), span)),
        // Both make a complex value out of any argument, so they never
        // reach the real path.
        Imaginary | Polar => return Err(Error::internal("a complex monad on the real path")),
        Neg => match d {
            Data::Bool(v) => Data::I64(par::map(v, |&b| -(b as i64)).into()),
            Data::I64(v) => match par::try_map(v, i64::checked_neg) {
                Some(out) => Data::I64(out.into()),
                None => Data::F64(par::map(v, |&x| -(x as f64)).into()),
            },
            Data::F64(v) => Data::F64(par::map(v, |&x| -x).into()),
            _ => return Err(wrong_type(d.dtype(), span)),
        },
        Signum => match d {
            Data::Bool(v) => Data::I64(par::map(v, |&b| b as i64).into()),
            Data::I64(v) => Data::I64(par::map(v, |&x| x.signum()).into()),
            // NaN has no sign here; it yields 0.
            Data::F64(v) => Data::F64(
                par::map(v, |&x| if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 }).into(),
            ),
            _ => return Err(wrong_type(d.dtype(), span)),
        },
        Recip => {
            // 1 % 0 is infinity, the J rule. APL's ÷0 is a domain error; a
            // ScalarMonad cannot tell the two languages apart, so the APL
            // divergence is left to revisit when monadic ops carry a dialect.
            let v = as_f64(d, &mut tmp, span)?;
            Data::F64(par::map(v, |&x| if x == 0.0 { f64::INFINITY } else { 1.0 / x }).into())
        }
        Sqrt => {
            // A negative value went to the complex path before this point.
            let v = as_f64(d, &mut tmp, span)?;
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
            _ => return Err(wrong_type(d.dtype(), span)),
        },
        Floor | Ceil => match d {
            Data::Bool(v) => Data::I64(par::map(v, |&b| b as i64).into()),
            Data::I64(_) => d.clone(),
            Data::F64(v) => {
                let round = |x: f64| if op == Floor { tol.floor(x) } else { tol.ceil(x) };
                // Integer when every rounded value is one, as in J.
                match par::try_map(v, |x| {
                    let r = round(x);
                    fits_i64(r).then_some(r as i64)
                }) {
                    Some(out) => Data::I64(out.into()),
                    None => Data::F64(par::map(v, |&x| round(x)).into()),
                }
            }
            _ => return Err(wrong_type(d.dtype(), span)),
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
                _ => return Err(wrong_type(d.dtype(), span)),
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
            _ => return Err(wrong_type(d.dtype(), span)),
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
            // As with `Sqrt`: a negative value is already on the complex path.
            let v = as_f64(d, &mut tmp, span)?;
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
            _ => return Err(wrong_type(d.dtype(), span)),
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
                _ => return Err(bad()),
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
    let data = Data::I64(out.into());
    // An extended length generates extended indices, so `*/ >: i. 25x` is
    // the exact factorial rather than the overflowing machine one.
    let data = if y.dtype() == DType::Ext {
        data.cast(DType::Ext).ok_or_else(|| Error::internal("integers have no extended form"))?
    } else {
        data
    };
    Ok(Array::new(shape, data))
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
        Data::Complex(v) => cx_key(v[i]),
        Data::Char(v) => v[i] as u64,
        // Neither a box nor an exact value has a cheap key; their callers
        // compare them by content.
        Data::Ext(_) | Data::Rat(_) | Data::Box(_) => 0,
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
        Data::Complex(v) => cx_key(v[i]),
        Data::Char(v) => v[i] as u64,
        // As in `elem_key`: never reached for boxed or exact data.
        Data::Ext(_) | Data::Rat(_) | Data::Box(_) => 0,
    }
}

/// One key for a complex value; the two parts have to disagree to disagree.
fn cx_key(z: Cx) -> u64 {
    let bits = |x: f64| if x == 0.0 { 0u64 } else { x.to_bits() };
    bits(z[0]) ^ bits(z[1]).rotate_left(32)
}

/// Distinct items, in the order of their first occurrence.
fn nub(y: &Array, tol: Tol) -> Array {
    if y.rank() == 0 {
        return Array::new(vec![1], y.data.clone());
    }
    let n = y.items();
    let m = y.item_size();
    let mut keep = Vec::new();
    if y.dtype() == DType::Box || y.dtype().is_exact() {
        // Boxed and exact items are compared by content, one against the
        // ones kept so far: there is no key to hash.
        for i in 0..n {
            if !keep.iter().any(|&j| arrays_match(&y.item(i), &y.item(j), tol)) {
                keep.push(i);
            }
        }
    } else if y.dtype() == DType::F64 && tol.ct != 0.0 {
        // Tolerant equality is not an equivalence a hash can stand in for:
        // each float item is compared against the ones already kept.
        let mut tv = Vec::new();
        let v = borrow_f64(&y.data, &mut tv);
        for i in 0..n {
            if !keep.iter().any(|&j| (0..m).all(|k| tol.eq(v[i * m + k], v[j * m + k]))) {
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
        // Grading a complex array orders it by real part then imaginary,
        // which is the order J's `/:` puts it in. The ordering VERBS still
        // refuse it; a grade is a permutation, not a claim about size.
        Data::Complex(v) => v[a + k][0]
            .partial_cmp(&v[b + k][0])
            .unwrap_or(Equal)
            .then_with(|| v[a + k][1].partial_cmp(&v[b + k][1]).unwrap_or(Equal)),
        Data::Char(v) => v[a + k].cmp(&v[b + k]),
        // The exact types order by value, however they are spelled: `2r4`
        // grades exactly where `1r2` does.
        Data::Ext(v) => v[a + k].cmp(&v[b + k]),
        Data::Rat(v) => v[a + k].cmp(&v[b + k]),
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
pub(crate) fn arrays_match(x: &Array, y: &Array, tol: Tol) -> bool {
    if x.shape != y.shape {
        return false;
    }
    // Two empty arrays of the same shape match whatever their types are,
    // which is what both references answer for `'' -: i. 0`.
    if x.count() == 0 {
        return true;
    }
    if let (Data::Box(a), Data::Box(b)) = (&x.data, &y.data) {
        return a.iter().zip(b.iter()).all(|(p, q)| arrays_match(p, q, tol));
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
            a.iter().zip(b).all(|(p, q)| tol.eq(*p, *q))
        }
        Some(DType::Complex) => {
            let (mut ta, mut tb) = (Vec::new(), Vec::new());
            let a = borrow_cx(&x.data, &mut ta);
            let b = borrow_cx(&y.data, &mut tb);
            a.iter().zip(b).all(|(p, q)| tol.eq_cx(*p, *q))
        }
        Some(t) if t.is_exact() => match (to_rat_vec(&x.data), to_rat_vec(&y.data)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        },
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
fn member_j(x: &Array, y: &Array, tol: Tol) -> Array {
    let cell_rank = y.rank().saturating_sub(1).min(x.rank());
    let frame_rank = x.rank() - cell_rank;
    let frame: Vec<usize> = x.shape[..frame_rank].to_vec();
    let nf: usize = frame.iter().product();
    let items = y.items();
    let mut out = Vec::with_capacity(nf);
    for i in 0..nf {
        let cell = x.cell_at(frame_rank, i);
        out.push((0..items).any(|j| arrays_match(&cell, &item_or_self(y, j), tol)) as u8);
    }
    Array::new(frame, Data::Bool(out.into()))
}

/// `x ∊ y`: for every element of x, does that value occur anywhere in y?
fn member_apl(x: &Array, y: &Array, tol: Tol) -> Array {
    let n = x.count();
    if x.dtype() == DType::Box
        || y.dtype() == DType::Box
        || x.dtype().is_exact()
        || y.dtype().is_exact()
    {
        // A box's elements are whole arrays and an exact value has no cheap
        // key, so both are compared by content; a box never equals a plain
        // number or character.
        let out: Vec<u8> = (0..n)
            .map(|i| {
                let e = atom(x, i);
                u8::from((0..y.count()).any(|j| arrays_match(&e, &atom(y, j), tol)))
            })
            .collect();
        return Array::new(x.shape.clone(), Data::Bool(out.into()));
    }
    if (x.dtype() == DType::Char) != (y.dtype() == DType::Char) {
        return Array::new(x.shape.clone(), Data::Bool(vec![0u8; n].into()));
    }
    if tol.ct != 0.0
        && (x.dtype() == DType::F64 || y.dtype() == DType::F64)
        && x.dtype() != DType::Char
    {
        // Tolerance rules a hash out; the values are compared directly.
        let (mut tx, mut ty) = (Vec::new(), Vec::new());
        let xs = borrow_f64(&x.data, &mut tx);
        let ys = borrow_f64(&y.data, &mut ty);
        let out: Vec<u8> =
            xs.iter().map(|a| ys.iter().any(|b| tol.eq(*a, *b)) as u8).collect();
        return Array::new(x.shape.clone(), Data::Bool(out.into()));
    }
    let seen: HashSet<u64> = (0..y.count()).map(|i| num_key(&y.data, i)).collect();
    let out: Vec<u8> =
        (0..n).map(|i| seen.contains(&num_key(&x.data, i)) as u8).collect();
    Array::new(x.shape.clone(), Data::Bool(out.into()))
}

/// `x i. y` / `x ⍳ y`: where each cell of y sits among the items of x.
fn index_of(x: &Array, y: &Array, origin: i64, tol: Tol) -> Array {
    let cell_rank = x.rank().saturating_sub(1).min(y.rank());
    let frame_rank = y.rank() - cell_rank;
    let frame: Vec<usize> = y.shape[..frame_rank].to_vec();
    let nf: usize = frame.iter().product();
    let items = x.items();
    let mut out = Vec::with_capacity(nf);
    for i in 0..nf {
        let cell = y.cell_at(frame_rank, i);
        let at = (0..items)
            .find(|&j| arrays_match(&cell, &item_or_self(x, j), tol))
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
fn catenate(
    x: &Array,
    y: &Array,
    leading: bool,
    fill: bool,
    span: Span,
) -> Result<Array> {
    let rank = x.rank().max(y.rank()).max(1);
    let axis = if leading { 0 } else { rank - 1 };
    let xa = cat_promote(x, y, rank, axis, span)?;
    let ya = cat_promote(y, x, rank, axis, span)?;
    // Axes other than the one being joined must agree. J overtakes both
    // sides to the larger length, which fills; APL insists they conform,
    // and the reference refuses the ragged case outright.
    let mut ragged = false;
    let want: Vec<i64> = (0..rank)
        .map(|k| {
            ragged |= k != axis && xa.shape[k] != ya.shape[k];
            xa.shape[k].max(ya.shape[k]) as i64
        })
        .collect();
    if ragged && !fill {
        return Err(Error::new(
            ErrorKind::Length,
            format!("catenating shapes {:?} and {:?}", xa.shape, ya.shape),
            Some(span),
        ));
    }
    let (xa, ya) = if ragged {
        let fit = |a: &Array| -> Result<Array> {
            let mut to = want.clone();
            to[axis] = a.shape[axis] as i64;
            take(&Array::from_i64(to), a, span)
        };
        (fit(&xa)?, fit(&ya)?)
    } else {
        (xa, ya)
    };
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
    !matches!(a.dtype(), DType::F64 | DType::Rat | DType::Char)
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
        MonadOp::Scalar(op) => scalar_monad(op, y, ctx.cfg.tol, span),
        MonadOp::ShapeOf => Ok(Array::from_i64(y.shape.iter().map(|&n| n as i64).collect())),
        MonadOp::Tally => Ok(Array::scalar_i64(y.items() as i64)),
        MonadOp::Ravel => Ok(Array::new(vec![y.count()], y.data.clone())),
        MonadOp::TransposeAxes => Ok(transpose_axes(y)),
        MonadOp::Head => Ok(head(y)),
        MonadOp::Behead => behead(y, span),
        MonadOp::Tail => Ok(tail(y)),
        MonadOp::Curtail => Ok(curtail(y)),
        MonadOp::Reverse => Ok(reverse(y)),
        MonadOp::Nub => Ok(nub(y, ctx.cfg.tol)),
        MonadOp::GradeUp { origin } | MonadOp::GradeDown { origin } => {
            no_grading_boxes(y, span)?;
            let down = matches!(p.monad, MonadOp::GradeDown { .. });
            let order = grade_order(y, down);
            Ok(Array::from_i64(order.iter().map(|&i| origin + i as i64).collect()))
        }
        MonadOp::IotaJ => iota_j(y, span),
        MonadOp::IotaApl { origin } => {
            // A non-scalar argument asks for an array of index vectors, one
            // per cell of the result: a nested array, which is still to come.
            if y.rank() != 0 {
                return Err(Error::not_yet("nested index arrays (⍳ with an array argument)", span));
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
        MonadOp::Indices { origin, boxed_coords } => {
            where_indices(y, origin, boxed_coords, span)
        }
        MonadOp::Steps => steps(y, span),
        MonadOp::ToExact => to_exact(y, span),
        MonadOp::NthPrime => {
            let n = y
                .to_i64_vec()
                .ok_or_else(|| Error::domain("the prime index must be an integer", span))?;
            let v = n.first().copied().unwrap_or(0);
            Ok(Array::scalar_i64(nth_prime(v, span)?))
        }
        MonadOp::PrimeFactors => {
            let n = y
                .to_i64_vec()
                .ok_or_else(|| Error::domain("prime factors need an integer", span))?;
            let v = n.first().copied().unwrap_or(0);
            Ok(Array::from_i64(prime_factors(v, span)?))
        }
        MonadOp::MatrixInverse => matrix_inverse(y, span),
        MonadOp::Roll { origin, fixed, float_at_zero } => {
            roll(y, origin, fixed, float_at_zero, span)
        }
        MonadOp::ComplexParts { polar } => complex_parts(y, polar, span),
        MonadOp::SelfClassify => Ok(self_classify(y, ctx.cfg.tol)),
        MonadOp::NubSieve => Ok(nub_sieve(y, ctx.cfg.tol)),
        MonadOp::Unicode { pass_chars } => unicode(y, pass_chars, span),
        MonadOp::Words => words(y, span),
        MonadOp::LevelOf => Ok(Array::scalar_i64(boxing_level(y))),
        MonadOp::AnagramIndex => anagram_index(y, span),
        MonadOp::CycleForm => cycle_form(y, span),
        MonadOp::Split => Ok(split_items(y)),
        MonadOp::Execute { apl, origin } => execute(y, apl, origin, ctx, span),
        MonadOp::NotYet(what) => Err(Error::not_yet(what, span)),
        MonadOp::None => {
            Err(Error::domain(format!("{} has no monadic meaning", p.name), span))
        }
    }
}

/// Left argument of reshape/take/drop: a scalar or vector of integers.
/// J `+. y` and `*. y` at rank 0: one complex value as its two parts, so
/// the rank machinery turns them into a new trailing axis of length 2.
fn complex_parts(y: &Array, polar: bool, span: Span) -> Result<Array> {
    let Some(v) = y.to_complex_vec() else {
        return Err(wrong_type(y.dtype(), span));
    };
    let z = v.first().copied().unwrap_or(cx::ZERO);
    let pair = if polar { vec![cx::abs(z), cx::arg(z)] } else { vec![z[0], z[1]] };
    Ok(Array::from_f64(pair))
}

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
fn dyad_op(p: &Prim, x: &Array, y: &Array, cfg: EvalCfg, span: Span) -> Result<Array> {
    let tol = cfg.tol;
    match p.dyad {
        // Reached only when a scalar verb is given non-zero cell ranks; the
        // cells then agree among themselves.
        DyadOp::Scalar(op) => scalar_dyad(op, x, y, cfg, span),
        DyadOp::Reshape => reshape(x, y, span),
        DyadOp::Take => take(x, y, span),
        DyadOp::Drop => drop_(x, y, span),
        DyadOp::Right => Ok(y.clone()),
        DyadOp::Left => Ok(x.clone()),
        DyadOp::Rotate => rotate(x, y, span),
        // Only J fills a ragged catenation; APL's conformability rule
        // refuses it, as the reference does.
        DyadOp::AppendLeading => {
            catenate(x, y, true, cfg.agreement == Agreement::LeadingPrefix, span)
        }
        DyadOp::AppendLast => {
            catenate(x, y, false, cfg.agreement == Agreement::LeadingPrefix, span)
        }
        DyadOp::IndexOf { origin } => Ok(index_of(x, y, origin, tol)),
        DyadOp::MemberJ => Ok(member_j(x, y, tol)),
        DyadOp::MemberApl => Ok(member_apl(x, y, tol)),
        DyadOp::From => from_index(x, y, span),
        DyadOp::Match => Ok(Array::scalar_bool(arrays_match(x, y, tol))),
        DyadOp::NotMatch => Ok(Array::scalar_bool(!arrays_match(x, y, tol))),
        DyadOp::GradeSelect { down } => grade_select(x, y, down, span),
        DyadOp::Copy => copy_items(x, y, span),
        DyadOp::Decode => decode(Some(x), y, span),
        DyadOp::Encode => encode(x, y, span),
        DyadOp::Laminate => laminate(x, y, span),
        DyadOp::Link => link(x, y, span),
        DyadOp::Strand => strand(x, y, span),
        DyadOp::IntervalIndex { offset } => interval_index(x, y, offset, tol, span),
        DyadOp::IndexOfLast { origin } => Ok(index_of_last(x, y, origin, tol)),
        DyadOp::MatrixDivide => matrix_divide(x, y, span),
        DyadOp::PartitionEnclose => partition_enclose(x, y, span),
        DyadOp::Squad { origin } => squad(x, y, origin, span),
        DyadOp::SelectAxis { axis, rank, origin } => {
            select_axis(x, y, axis, rank, origin, span)
        }
        DyadOp::Fetch => fetch(x, y, span),
        DyadOp::Deal { origin, fixed } => deal(x, y, origin, fixed, span),
        DyadOp::ExactForm => exact_form(x, y, span),
        DyadOp::Boolean(op) => bool_dyad(op, x, y, cfg, span),
        DyadOp::Less => Ok(set_less(x, y, tol)),
        DyadOp::Union => union_items(x, y, tol, span),
        DyadOp::Intersect => Ok(intersect_items(x, y, tol)),
        DyadOp::AnagramFrom => anagram_from(x, y, span),
        DyadOp::Permute => permute(x, y, span),
        DyadOp::FindSeq => Ok(find_seq(x, y, tol)),
        DyadOp::UnicodeForm => unicode_form(x, y, span),
        DyadOp::PrimeMeta => prime_meta(x, y, span),
        DyadOp::PrimeExponents => prime_exponents(x, y, span),
        DyadOp::Pick { origin } => pick(x, y, origin, span),
        DyadOp::Expand => expand(x, y, span),
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
        // `j.` and `r.` build a complex number out of two reals; neither
        // reference gives them an identity element.
        ScalarDyad::MakeComplex | ScalarDyad::PolarBy => return None,
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

#[inline(always)]
fn fold_range_body<T, F>(
    v: &[T],
    m: usize,
    lo: usize,
    hi: usize,
    j0: usize,
    acc: &mut [T],
    step: &F,
) -> bool
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

multiversioned! {
    #[allow(clippy::too_many_arguments)]
    fn fold_range_vectorised[T: Copy, F: Fn(T, T) -> (T, bool)](
        v: &[T],
        m: usize,
        lo: usize,
        hi: usize,
        j0: usize,
        acc: &mut [T],
        step: &F,
    ) -> bool = fold_range_body;
}

/// Columns per fold below which the baseline compilation wins.
///
/// The only loop a wider vector can widen here is the one across an item's
/// columns, and a loop of a few columns spends more on entering the vector
/// body than the width gives back. Measured on `+/ m` over 20M f64 on one
/// thread: at 4 and 8 columns the AVX2 clone is about 1.5x slower than the
/// baseline one, at 16 columns and above it is 1.2x to 1.6x faster.
const VECTOR_COLUMNS: usize = 16;

/// Fold items `lo .. hi` into `acc`, right to left, taking only the columns
/// that start at `j0` — `acc.len()` of them. False when a step left the
/// element type; the accumulator is then meaningless.
///
/// Wide enough, and this is the reduce that vectorises, so it runs the
/// compilation the CPU is entitled to; narrow, and it runs the baseline one.
/// Either way the fold order is the same: the columns are independent
/// accumulators, not a reassociation of one.
#[allow(clippy::too_many_arguments)]
#[inline]
fn fold_range<T, F>(
    v: &[T],
    m: usize,
    lo: usize,
    hi: usize,
    j0: usize,
    acc: &mut [T],
    step: &F,
) -> bool
where
    T: Copy,
    F: Fn(T, T) -> (T, bool),
{
    if acc.len() < VECTOR_COLUMNS {
        fold_range_body(v, m, lo, hi, j0, acc, step)
    } else {
        fold_range_vectorised(v, m, lo, hi, j0, acc, step)
    }
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

fn fold_cx(op: ScalarDyad, v: &[Cx], n: usize, m: usize) -> Option<Vec<Cx>> {
    use ScalarDyad::*;
    let assoc = is_associative(op);
    match op {
        Add => fold_items(v, n, m, assoc, |a: Cx, b: Cx| (cx::add(a, b), false)),
        Sub => fold_items(v, n, m, assoc, |a: Cx, b: Cx| (cx::sub(a, b), false)),
        Mul => fold_items(v, n, m, assoc, |a: Cx, b: Cx| (cx::mul(a, b), false)),
        // Min and Max have no complex meaning; the general path reports it.
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
        Data::Complex(v) => Some(Data::Complex(fold_cx(op, v, n, m)?.into())),
        Data::I64(v) => Some(Data::I64(fold_i64(op, v, n, m)?.into())),
        // Booleans reduce as integers, which is what promotion says the
        // general path would produce; widen once and fold.
        Data::Bool(v) => {
            let widened = par::map(v, |&b| b as i64);
            Some(Data::I64(fold_i64(op, &widened, n, m)?.into()))
        }
        // A bignum has no blockwise form: the exact types fold, scan and
        // window through the general path, one step at a time.
        Data::Ext(_) | Data::Rat(_) | Data::Char(_) | Data::Box(_) => None,
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
                    acc =
                        scalar_dyad_data(op, &y.data, i * m, 1, &acc, 0, 1, m, ctx.cfg.tol, span)?;
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

#[inline(always)]
fn scan_flat_body<T, F>(v: &[T], n: usize, m: usize, back: bool, step: F) -> Option<Vec<T>>
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

multiversioned! {
    fn scan_flat_vectorised[T: Copy + Default, F: Fn(T, T) -> (T, bool)](
        v: &[T],
        n: usize,
        m: usize,
        back: bool,
        step: F,
    ) -> Option<Vec<T>> = scan_flat_body;
}

/// Running fold over `n` items of `m` elements each, one output item per
/// step. Backward is exactly the insert's right-to-left order, so it holds
/// for any step; forward is the left-to-right order, which agrees with the
/// insert only when the step is associative. None when a step left the
/// element type.
///
/// Only the wide shape has anything to gain from a wider vector, and for
/// the same reason the reduce has: the loop that widens is the one across
/// an item's elements. A scan of one element per item is a chain of
/// dependent steps, which no vector shortens, so it takes the baseline
/// compilation.
fn scan_flat<T, F>(v: &[T], n: usize, m: usize, back: bool, step: F) -> Option<Vec<T>>
where
    T: Copy + Default,
    F: Fn(T, T) -> (T, bool),
{
    if m < VECTOR_COLUMNS {
        scan_flat_body(v, n, m, back, step)
    } else {
        scan_flat_vectorised(v, n, m, back, step)
    }
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

fn scan_cx(op: ScalarDyad, v: &[Cx], n: usize, m: usize, back: bool) -> Option<Vec<Cx>> {
    use ScalarDyad::*;
    match op {
        Add => scan_flat(v, n, m, back, |a: Cx, b: Cx| (cx::add(a, b), false)),
        Sub => scan_flat(v, n, m, back, |a: Cx, b: Cx| (cx::sub(a, b), false)),
        Mul => scan_flat(v, n, m, back, |a: Cx, b: Cx| (cx::mul(a, b), false)),
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
        Data::Complex(v) => Some(Data::Complex(scan_cx(op, v, n, m, back)?.into())),
        Data::I64(v) => Some(ints(v)),
        Data::Bool(v) => Some(ints(&v.iter().map(|&b| b as i64).collect::<Vec<_>>())),
        // A bignum has no blockwise form: the exact types fold, scan and
        // window through the general path, one step at a time.
        Data::Ext(_) | Data::Rat(_) | Data::Char(_) | Data::Box(_) => None,
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

#[inline(always)]
fn window_fold_range_body<T, F>(
    v: &[T],
    n: usize,
    w: usize,
    lo: usize,
    out: &mut [T],
    step: &F,
) -> bool
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

multiversioned! {
    /// The windows `lo .. lo + out.len()`. False when a step left the type.
    /// Compiled per CPU feature level; the prefix and suffix passes it runs
    /// are dependent chains, so what a wider vector reaches here is the
    /// pairing of the two, not the passes themselves.
    fn window_fold_range[T: Copy + Default, F: Fn(T, T) -> (T, bool)](
        v: &[T],
        n: usize,
        w: usize,
        lo: usize,
        out: &mut [T],
        step: &F,
    ) -> bool = window_fold_range_body;
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

fn window_cx(op: ScalarDyad, v: &[Cx], n: usize, m: usize, w: usize) -> Option<Vec<Cx>> {
    use ScalarDyad::*;
    match op {
        Add => window_fold(v, n, m, w, |a: Cx, b: Cx| (cx::add(a, b), false)),
        Mul => window_fold(v, n, m, w, |a: Cx, b: Cx| (cx::mul(a, b), false)),
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
        Data::Complex(v) => Some(Data::Complex(window_cx(op, v, n, m, w)?.into())),
        Data::I64(v) => Some(ints(v)),
        Data::Bool(v) => Some(ints(&v.iter().map(|&b| b as i64).collect::<Vec<_>>())),
        // A bignum has no blockwise form: the exact types fold, scan and
        // window through the general path, one step at a time.
        Data::Ext(_) | Data::Rat(_) | Data::Char(_) | Data::Box(_) => None,
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
                if arrays_match(&next, &acc, ctx.cfg.tol) {
                    return Ok(next);
                }
                acc = next;
            }
            Err(Error::domain("the iteration did not converge", span))
        }
    }
}

/// `u^:v y` and `x u^:v y` (J): the verb `v` says how many times to apply
/// `u`. `(u^:v)^:_` is the while loop the idiom is written with.
fn power_v(
    u: &Verb,
    v: &Verb,
    x: Option<&Array>,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    let count = match x {
        Some(x) => v.dyad(x, y, ctx, span)?,
        None => v.monad(y, ctx, span)?,
    };
    let n = count
        .to_i64_vec()
        .ok_or_else(|| Error::domain("the power count must be an integer", span))?;
    if n.len() != 1 {
        return Err(Error::not_yet("a list of power counts (u^:v with several)", span));
    }
    let n = n[0];
    if n < 0 {
        return Err(Error::not_yet("a negative power (the verb's inverse)", span));
    }
    power(u, Power::Times(n as u64), x, y, ctx, span)
}

/// `f⍣g y` (APL): apply `f` until `new g old` holds.
fn power_until(
    u: &Verb,
    test: &Verb,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    let mut acc = y.clone();
    for _ in 0..CONVERGE_LIMIT {
        let next = u.monad(&acc, ctx, span)?;
        let done = test.dyad(&next, &acc, ctx, span)?;
        let stop = done
            .to_f64_vec()
            .ok_or_else(|| Error::domain("the ⍣ test must answer with numbers", span))?;
        if !stop.is_empty() && stop.iter().all(|&v| v != 0.0) {
            return Ok(next);
        }
        acc = next;
    }
    Err(Error::domain("the iteration did not converge", span))
}

/// `f[k]` (APL): `f` applied along axis `k`.
///
/// The axis is brought to the front, the verb runs on the leading axis, and
/// a result that kept the argument's rank has the axis put back — which is
/// what separates a reduction (rank drops, axes stay in order) from a scan
/// or a reversal (rank kept).
fn along_axis(
    u: &Verb,
    x: Option<&Array>,
    y: &Array,
    k: usize,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    if k >= y.rank().max(1) {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("axis {k} does not exist on an argument of rank {}", y.rank()),
            Some(span),
        ));
    }
    let moved = axis_to_front(y, k);
    let r = moved.rank();
    let out = match x {
        Some(x) => u.dyad(x, &moved, ctx, span)?,
        None => u.monad(&moved, ctx, span)?,
    };
    if out.rank() == r {
        return Ok(front_to_axis(&out, k));
    }
    Ok(out)
}

// ------------------------------------------------- wave 3: search and steps

/// `I. y` (J) / `⍸ y` (APL): index `i` repeated `y[i]` times.
///
/// J applies at rank 1, so a higher-rank argument frames the vector answers;
/// APL applies to the whole argument and answers a rank-2-or-higher one with
/// one boxed coordinate vector per occurrence.
fn where_indices(y: &Array, origin: i64, boxed: bool, span: Span) -> Result<Array> {
    let counts = y
        .to_i64_vec()
        .ok_or_else(|| Error::domain("indices needs non-negative integers", span))?;
    if counts.iter().any(|&c| c < 0) {
        return Err(Error::domain("indices needs non-negative integers", span));
    }
    if !boxed || y.rank() < 2 {
        let mut out = Vec::new();
        for (i, &c) in counts.iter().enumerate() {
            for _ in 0..c {
                out.push(origin + i as i64);
            }
        }
        return Ok(Array::from_i64(out));
    }
    let r = y.rank();
    let mut coord = vec![0usize; r];
    let mut out: Vec<Array> = Vec::new();
    for &c in &counts {
        if c > 0 {
            let point =
                Array::from_i64(coord.iter().map(|&k| origin + k as i64).collect::<Vec<_>>());
            for _ in 0..c {
                out.push(point.clone());
            }
        }
        odometer(&mut coord, &y.shape);
    }
    Ok(Array::new(vec![out.len()], Data::Box(out.into())))
}

/// `x I. y` / `x ⍸ y`: which interval of the ascending `x` each cell of `y`
/// falls in — the number of items of `x` strictly below it.
///
/// `offset` is what the language adds to that count: nothing in J, and
/// `⎕IO - 1` in APL, which is what both references answer.
fn interval_index(x: &Array, y: &Array, offset: i64, tol: Tol, span: Span) -> Result<Array> {
    let bounds = x
        .to_f64_vec()
        .ok_or_else(|| Error::domain("interval index needs numeric bounds", span))?;
    let vals = y
        .to_f64_vec()
        .ok_or_else(|| Error::domain("interval index needs numeric values", span))?;
    let out: Vec<i64> = vals
        .iter()
        .map(|&v| offset + bounds.iter().filter(|&&b| tol.lt(b, v)).count() as i64)
        .collect();
    Ok(Array::new(y.shape.clone(), Data::I64(out.into())))
}

/// `i: y` (J): the integers from `-y` to `y`, one step apart. The count is
/// `1 + <. 2 * | y`, and a negative argument counts down.
fn steps(y: &Array, span: Span) -> Result<Array> {
    let vals = y.to_f64_vec().ok_or_else(|| Error::domain("steps needs a number", span))?;
    let v = match vals.first() {
        Some(&v) if v.is_finite() => v,
        _ => return Err(Error::domain("steps needs a finite number", span)),
    };
    let n = (2.0 * v.abs()).floor();
    if n > 1e7 {
        return Err(Error::domain("steps would produce too many items", span));
    }
    let n = n as i64 + 1;
    let step = if v < 0.0 { -1.0 } else { 1.0 };
    let start = -v;
    if v.fract() == 0.0 {
        let start = start as i64;
        let step = step as i64;
        return Ok(Array::from_i64((0..n).map(|k| start + k * step).collect()));
    }
    Ok(Array::from_f64((0..n).map(|k| start + k as f64 * step).collect()))
}

/// `x i: y`: where each cell of `y` LAST sits among the items of `x`.
fn index_of_last(x: &Array, y: &Array, origin: i64, tol: Tol) -> Array {
    let cell_rank = x.rank().saturating_sub(1).min(y.rank());
    let frame_rank = y.rank() - cell_rank;
    let frame: Vec<usize> = y.shape[..frame_rank].to_vec();
    let nf: usize = frame.iter().product();
    let items = x.items();
    let mut out = Vec::with_capacity(nf);
    for i in 0..nf {
        let cell = y.cell_at(frame_rank, i);
        let at = (0..items)
            .rev()
            .find(|&j| arrays_match(&cell, &item_or_self(x, j), tol))
            .unwrap_or(items);
        out.push(origin + at as i64);
    }
    Array::new(frame, Data::I64(out.into()))
}

// ----------------------------------------------------------- roll and deal

/// `? y` / `?. y`: every element of y replaced by a random value below it.
///
/// The whole argument is one draw, taken in ravel order, which is what
/// makes `?. 5 # 100` five different numbers rather than one repeated.
fn roll(
    y: &Array,
    origin: i64,
    fixed: bool,
    float_at_zero: bool,
    span: Span,
) -> Result<Array> {
    let bounds = y
        .to_i64_vec()
        .ok_or_else(|| Error::domain("roll needs whole numbers", span))?;
    if bounds.iter().any(|&b| b < 0) {
        return Err(Error::domain("roll needs non-negative numbers", span));
    }
    if !float_at_zero && bounds.iter().any(|&b| b == 0) {
        return Err(Error::domain("? 0 has no value: the range is empty", span));
    }
    // A zero anywhere makes the whole answer float, as J's does.
    let any_zero = bounds.contains(&0);
    crate::rng::with(fixed, |g| {
        if any_zero {
            let out: Vec<f64> = bounds
                .iter()
                .map(|&b| {
                    if b == 0 {
                        g.unit()
                    } else {
                        (origin + g.below(b as u64) as i64) as f64
                    }
                })
                .collect();
            return Ok(Array::new(y.shape.clone(), Data::F64(out.into())));
        }
        let out: Vec<i64> =
            bounds.iter().map(|&b| origin + g.below(b as u64) as i64).collect();
        Ok(Array::new(y.shape.clone(), Data::I64(out.into())))
    })
}

/// `x ? y` / `x ?. y`: x distinct values drawn from the y below `origin+y`.
fn deal(x: &Array, y: &Array, origin: i64, fixed: bool, span: Span) -> Result<Array> {
    let want = one_whole(x, "the count dealt", span)?;
    let from = one_whole(y, "the range dealt from", span)?;
    if want < 0 || from < 0 {
        return Err(Error::domain("deal needs non-negative numbers", span));
    }
    if want > from {
        return Err(Error::domain(
            format!("cannot deal {want} distinct value(s) from {from}"),
            span,
        ));
    }
    if want == 0 {
        return Ok(Array::from_i64(Vec::new()));
    }
    let drawn = crate::rng::with(fixed, |g| g.deal(want as usize, from as u64));
    Ok(Array::from_i64(drawn.into_iter().map(|v| v + origin).collect()))
}

/// One whole number from a one-element argument.
fn one_whole(a: &Array, what: &str, span: Span) -> Result<i64> {
    let v = a
        .to_i64_vec()
        .ok_or_else(|| Error::domain(format!("{what} must be a whole number"), span))?;
    match v[..] {
        [n] => Ok(n),
        _ => Err(Error::new(
            ErrorKind::Rank,
            format!("{what} must be one number"),
            Some(span),
        )),
    }
}

// ------------------------------------------------------------------ primes

/// The `n`-th prime, counting from zero (`p: n`).
fn nth_prime(n: i64, span: Span) -> Result<i64> {
    if n < 0 {
        return Err(Error::domain("the prime index must not be negative", span));
    }
    const LIMIT: i64 = 5_000_000;
    if n >= LIMIT {
        return Err(Error::domain(
            format!("prime index {n} is beyond the {LIMIT}th prime"),
            span,
        ));
    }
    // An upper bound for p_n (n counted from zero): n < 6 is tabulated,
    // above that Rosser's bound n(ln n + ln ln n) holds.
    let k = (n + 1) as f64;
    let bound = if n < 6 { 15.0 } else { k * (k.ln() + k.ln().ln()) };
    let bound = bound.ceil() as usize + 1;
    let mut sieve = vec![true; bound + 1];
    sieve[0] = false;
    if bound >= 1 {
        sieve[1] = false;
    }
    let mut p = 2usize;
    while p * p <= bound {
        if sieve[p] {
            let mut q = p * p;
            while q <= bound {
                sieve[q] = false;
                q += p;
            }
        }
        p += 1;
    }
    let mut seen = 0i64;
    for (v, &is_p) in sieve.iter().enumerate() {
        if is_p {
            if seen == n {
                return Ok(v as i64);
            }
            seen += 1;
        }
    }
    Err(Error::internal("the prime sieve was too small"))
}

/// `q: n`: the prime factors of n, ascending, with multiplicity.
fn prime_factors(n: i64, span: Span) -> Result<Vec<i64>> {
    if n < 1 {
        return Err(Error::domain("prime factors need a positive integer", span));
    }
    let mut out = Vec::new();
    let mut m = n;
    let mut d = 2i64;
    while d.saturating_mul(d) <= m {
        while m % d == 0 {
            out.push(d);
            m /= d;
        }
        d += if d == 2 { 1 } else { 2 };
    }
    if m > 1 {
        out.push(m);
    }
    Ok(out)
}

// --------------------------------------------------------- matrix division

/// Least-squares solution of `a x = b` by Householder QR.
///
/// `a` is `m` by `n` in row-major order with `m >= n`, `b` is `m` by `k`.
/// The answer is `n` by `k`. None when `a` has not got full column rank,
/// which both references refuse.
fn lstsq(a: &[f64], m: usize, n: usize, b: &[f64], k: usize) -> Option<Vec<f64>> {
    // Work on copies: the factorisation overwrites both.
    let mut r = a.to_vec();
    let mut c = b.to_vec();
    let at = |i: usize, j: usize, w: usize| i * w + j;
    let scale = a.iter().fold(0.0f64, |acc, v| acc.max(v.abs()));
    if scale == 0.0 {
        return None;
    }
    for j in 0..n {
        // The Householder vector for column j below the diagonal.
        let norm = (j..m).map(|i| r[at(i, j, n)] * r[at(i, j, n)]).sum::<f64>().sqrt();
        if norm <= 1e-13 * scale {
            return None;
        }
        let alpha = if r[at(j, j, n)] > 0.0 { -norm } else { norm };
        let mut v = vec![0.0f64; m];
        for i in j..m {
            v[i] = r[at(i, j, n)];
        }
        v[j] -= alpha;
        let vnorm2: f64 = (j..m).map(|i| v[i] * v[i]).sum();
        if vnorm2 > 0.0 {
            for col in j..n {
                let dot: f64 = (j..m).map(|i| v[i] * r[at(i, col, n)]).sum();
                let f = 2.0 * dot / vnorm2;
                for i in j..m {
                    r[at(i, col, n)] -= f * v[i];
                }
            }
            for col in 0..k {
                let dot: f64 = (j..m).map(|i| v[i] * c[at(i, col, k)]).sum();
                let f = 2.0 * dot / vnorm2;
                for i in j..m {
                    c[at(i, col, k)] -= f * v[i];
                }
            }
        }
    }
    // Back-substitute the upper triangle.
    let mut x = vec![0.0f64; n * k];
    for col in 0..k {
        for i in (0..n).rev() {
            let mut acc = c[at(i, col, k)];
            for j in i + 1..n {
                acc -= r[at(i, j, n)] * x[at(j, col, k)];
            }
            let d = r[at(i, i, n)];
            if d.abs() <= 1e-13 * scale {
                return None;
            }
            x[at(i, col, k)] = acc / d;
        }
    }
    Some(x)
}

/// A numeric argument as an `m` by `n` row-major buffer. Rank 0 is 1 by 1
/// and rank 1 is `m` by 1, which is how both references read them.
fn as_matrix(a: &Array, span: Span) -> Result<(Vec<f64>, usize, usize)> {
    let v = a
        .to_f64_vec()
        .ok_or_else(|| Error::domain("matrix division needs numeric data", span))?;
    match a.rank() {
        0 => Ok((v, 1, 1)),
        1 => {
            let m = a.shape[0];
            Ok((v, m, 1))
        }
        2 => Ok((v, a.shape[0], a.shape[1])),
        _ => Err(Error::new(
            ErrorKind::Rank,
            "matrix division needs an argument of rank 2 or less",
            Some(span),
        )),
    }
}

/// `%. y` / `⌹ y`: the inverse of a square matrix, or the least-squares
/// pseudo-inverse of a taller one. A wider one is refused, as both
/// references refuse it.
fn matrix_inverse(y: &Array, span: Span) -> Result<Array> {
    let (a, m, n) = as_matrix(y, span)?;
    if m < n {
        return Err(Error::new(
            ErrorKind::Length,
            format!("cannot invert a {m} by {n} matrix: it has more columns than rows"),
            Some(span),
        ));
    }
    let mut eye = vec![0.0f64; m * m];
    for i in 0..m {
        eye[i * m + i] = 1.0;
    }
    let x = lstsq(&a, m, n, &eye, m)
        .ok_or_else(|| Error::domain("the matrix is singular", span))?;
    // A rank-2 argument gives the n by m pseudo-inverse; a vector or scalar
    // keeps its own shape, which is what J prints for them.
    let shape = if y.rank() == 2 { vec![n, m] } else { y.shape.clone() };
    Ok(Array::new(shape, Data::F64(x.into())))
}

/// `x %. y` / `x ⌹ y`: the least-squares solution of `y a = x`.
fn matrix_divide(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let (a, m, n) = as_matrix(y, span)?;
    let (b, bm, k) = as_matrix(x, span)?;
    if bm != m {
        return Err(Error::new(
            ErrorKind::Length,
            format!("the system has {m} rows but the right-hand side has {bm}"),
            Some(span),
        ));
    }
    if m < n {
        return Err(Error::new(
            ErrorKind::Length,
            format!("the {m} by {n} system is underdetermined"),
            Some(span),
        ));
    }
    let sol = lstsq(&a, m, n, &b, k)
        .ok_or_else(|| Error::domain("the system is singular", span))?;
    // The right-hand side's own rank decides the answer's: a vector in gives
    // one solution vector, a matrix in gives one column per column.
    let shape = if x.rank() == 2 { vec![n, k] } else { vec![n] };
    Ok(Array::new(shape, Data::F64(sol.into())))
}

// ----------------------------------------------------- indexing and amend

/// `x ⌷ y` (APL2): one scalar index per axis of y.
fn squad(x: &Array, y: &Array, origin: i64, span: Span) -> Result<Array> {
    if x.rank() > 1 {
        return Err(Error::new(
            ErrorKind::Rank,
            "the index of ⌷ must be a scalar or a vector",
            Some(span),
        ));
    }
    let idx = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("index must be an integer", span))?;
    if idx.len() != y.rank() {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("{} index(es) for an argument of rank {}", idx.len(), y.rank()),
            Some(span),
        ));
    }
    let st = strides(&y.shape);
    let mut at = 0usize;
    for (k, &i) in idx.iter().enumerate() {
        let j = i - origin;
        if j < 0 || j as usize >= y.shape[k] {
            return Err(Error::domain(
                format!("index {i} is out of range on axis {k}"),
                span,
            ));
        }
        at += j as usize * st[k];
    }
    Ok(atom(y, at))
}

/// One bracket slot of APL indexing: axis `axis` of `y` selected by `x`.
///
/// A scalar index drops the axis, any other shape splices in. `rank`, when
/// it is not zero, is the number of slots the brackets held: the slot that
/// sees the whole array checks it, and the others have already been applied
/// to a smaller one.
fn select_axis(
    x: &Array,
    y: &Array,
    axis: usize,
    rank: usize,
    origin: i64,
    span: Span,
) -> Result<Array> {
    if rank != 0 && y.rank() != rank {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("{rank} index slot(s) for an argument of rank {}", y.rank()),
            Some(span),
        ));
    }
    if axis >= y.rank() {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("axis {axis} does not exist on an argument of rank {}", y.rank()),
            Some(span),
        ));
    }
    let idx = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("index must be an integer", span))?;
    let len = y.shape[axis];
    let mut picks = Vec::with_capacity(idx.len());
    for &i in &idx {
        let j = i - origin;
        if j < 0 || j as usize >= len {
            return Err(Error::domain(
                format!("index {i} is out of range: axis {axis} has {len} items"),
                span,
            ));
        }
        picks.push(j as usize);
    }
    let mut shape = Vec::with_capacity(y.rank() + x.rank());
    shape.extend_from_slice(&y.shape[..axis]);
    shape.extend_from_slice(&x.shape);
    shape.extend_from_slice(&y.shape[axis + 1..]);
    let outer: usize = y.shape[..axis].iter().product();
    let inner: usize = y.shape[axis + 1..].iter().product();
    let mut data = Data::empty(y.dtype());
    for o in 0..outer {
        for &p in &picks {
            let base = (o * len + p) * inner;
            for e in 0..inner {
                push_elem(&mut data, &y.data, base + e);
            }
        }
    }
    Ok(Array::new(shape, data))
}

/// `x m} y` (J): the items of `y` at the indices `m`, replaced by `x`.
///
/// `x` is either one item, used at every index, or one item per index.
fn amend(m: &Array, x: &Array, y: &Array, span: Span) -> Result<Array> {
    if y.rank() == 0 {
        return Err(Error::new(ErrorKind::Rank, "cannot amend a scalar", Some(span)));
    }
    let idx = m
        .to_i64_vec()
        .ok_or_else(|| Error::domain("amend indices must be integers", span))?;
    let items = y.items() as i64;
    let mut at = Vec::with_capacity(idx.len());
    for &i in &idx {
        let k = if i < 0 { i + items } else { i };
        if k < 0 || k >= items {
            return Err(Error::domain(
                format!("index {i} is out of range: the argument has {items} items"),
                span,
            ));
        }
        at.push(k as usize);
    }
    let cell = y.item_size();
    let per_index = if x.count() == cell {
        false
    } else if x.count() == cell * at.len() {
        true
    } else {
        return Err(Error::new(
            ErrorKind::Length,
            format!(
                "cannot amend {} item(s) of {} element(s) each with {} element(s)",
                at.len(),
                cell,
                x.count()
            ),
            Some(span),
        ));
    };
    // The result holds both kinds of value, so it takes the wider type:
    // amending an integer list with 1.5 gives a float list, as J's does.
    let Some(t) = DType::promote(x.dtype(), y.dtype()) else {
        return Err(Error::new(
            ErrorKind::Type,
            "the replacement and the argument hold different kinds of value",
            Some(span),
        ));
    };
    let (Some(src), Some(base)) = (x.data.cast(t), y.data.cast(t)) else {
        return Err(Error::new(
            ErrorKind::Type,
            "the replacement and the argument hold different kinds of value",
            Some(span),
        ));
    };
    // Rebuild rather than mutate: the buffer may be shared, or foreign.
    let mut data = Data::empty(t);
    let mut plan: Vec<Option<usize>> = vec![None; y.items()];
    for (n, &k) in at.iter().enumerate() {
        plan[k] = Some(if per_index { n } else { 0 });
    }
    for (i, slot) in plan.iter().enumerate() {
        match slot {
            Some(n) => {
                for e in 0..cell {
                    push_elem(&mut data, &src, n * cell + e);
                }
            }
            None => {
                for e in 0..cell {
                    push_elem(&mut data, &base, i * cell + e);
                }
            }
        }
    }
    Ok(Array::new(y.shape.clone(), data))
}

/// `x {:: y` (J): follow the path `x` into `y`, opening one level a step.
///
/// A boxed `x` is one step per box; a simple `x` is a single step, so
/// `1 {:: y` is item 1 of y opened once.
fn fetch(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let steps: Vec<Array> = match x.as_boxes() {
        Some(bs) => bs.to_vec(),
        None => vec![x.clone()],
    };
    let mut cur = y.clone();
    for step in steps {
        let idx = step
            .to_i64_vec()
            .ok_or_else(|| Error::domain("a fetch path holds integers", span))?;
        // A scalar has one item, which is how `{` reads one too.
        let base =
            if cur.rank() == 0 { Array::new(vec![1], cur.data.clone()) } else { cur.clone() };
        if idx.len() > base.rank() {
            return Err(Error::new(
                ErrorKind::Length,
                format!(
                    "a path step of {} index(es) into a value of rank {}",
                    idx.len(),
                    cur.rank()
                ),
                Some(span),
            ));
        }
        let at = cell_index(&base, &idx, span)?;
        cur = open_cell(&base.cell_at(idx.len(), at));
    }
    Ok(cur)
}

/// The cell number a path step names, in the order `cell_at` counts them.
fn cell_index(y: &Array, idx: &[i64], span: Span) -> Result<usize> {
    let mut at = 0usize;
    for (k, &i) in idx.iter().enumerate() {
        let len = y.shape[k] as i64;
        let j = if i < 0 { i + len } else { i };
        if j < 0 || j >= len {
            return Err(Error::domain(
                format!("index {i} is out of range: axis {k} has {len} items"),
                span,
            ));
        }
        at = at * y.shape[k] + j as usize;
    }
    Ok(at)
}

// ------------------------------------------------------ partition, groups

/// `x ⊂ y` (APL2): partitioned enclose.
///
/// A partition opens wherever `x` rises — `x[i] > x[i-1]`, reading `x[-1]`
/// as zero — and an item whose flag is zero is dropped rather than joined
/// to anything. That is what GNU APL answers, and it is what makes
/// `1 1 2 2 ⊂ 'abcd'` two pairs rather than one run.
fn partition_enclose(x: &Array, y: &Array, span: Span) -> Result<Array> {
    if y.rank() != 1 {
        return Err(Error::not_yet("partitioned enclose on a matrix", span));
    }
    let flags = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("partition flags must be integers", span))?;
    if flags.iter().any(|&f| f < 0) {
        return Err(Error::domain("partition flags must not be negative", span));
    }
    if flags.len() != y.shape[0] {
        return Err(Error::new(
            ErrorKind::Length,
            format!("{} flag(s) for {} item(s)", flags.len(), y.shape[0]),
            Some(span),
        ));
    }
    let mut parts: Vec<Array> = Vec::new();
    let mut cur: Option<Data> = None;
    let mut prev = 0i64;
    for (i, &f) in flags.iter().enumerate() {
        if f > prev {
            if let Some(d) = cur.take() {
                parts.push(Array::new(vec![d.len()], d));
            }
            cur = Some(Data::empty(y.dtype()));
        }
        prev = f;
        if f == 0 {
            continue;
        }
        if let Some(d) = cur.as_mut() {
            push_elem(d, &y.data, i);
        }
    }
    if let Some(d) = cur.take() {
        parts.push(Array::new(vec![d.len()], d));
    }
    Ok(Array::new(vec![parts.len()], Data::Box(parts.into())))
}

/// `x u/. y` (J): `u` over each group of items of `y` sharing a key in `x`,
/// the groups in the order their keys first appear.
fn key(u: &Verb, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let keys = if x.rank() == 0 { Array::new(vec![1], x.data.clone()) } else { x.clone() };
    let n = keys.items();
    if n != y.items() && !(y.rank() == 0 && n == 1) {
        return Err(Error::new(
            ErrorKind::Length,
            format!("{n} key(s) for {} item(s)", y.items()),
            Some(span),
        ));
    }
    let tol = ctx.cfg.tol;
    let mut order: Vec<usize> = Vec::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in 0..n {
        let k = keys.item(i);
        match order.iter().position(|&j| arrays_match(&k, &keys.item(j), tol)) {
            Some(g) => groups[g].push(i),
            None => {
                order.push(i);
                groups.push(vec![i]);
            }
        }
    }
    let items = if y.rank() == 0 { Array::new(vec![1], y.data.clone()) } else { y.clone() };
    let mut cells = Vec::with_capacity(groups.len());
    for g in &groups {
        cells.push(u.monad(&select_items(&items, g), ctx, span)?);
    }
    assemble(&[groups.len()], cells, span)
}

/// `u/. y` (J): `u` over each anti-diagonal of a table, starting at the
/// leading corner.
fn oblique(u: &Verb, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    if y.rank() < 2 {
        let items = if y.rank() == 0 { Array::new(vec![1], y.data.clone()) } else { y.clone() };
        let n = items.items();
        let mut cells = Vec::with_capacity(n);
        for i in 0..n {
            cells.push(u.monad(&select_items(&items, &[i]), ctx, span)?);
        }
        return assemble(&[n], cells, span);
    }
    if y.rank() > 2 {
        return Err(Error::not_yet("oblique (u/.) on a rank-3 or higher argument", span));
    }
    let (rows, cols) = (y.shape[0], y.shape[1]);
    let mut cells = Vec::with_capacity(rows + cols - 1);
    for d in 0..rows + cols - 1 {
        let mut data = Data::empty(y.dtype());
        let mut len = 0usize;
        for i in 0..rows {
            if d >= i && d - i < cols {
                push_elem(&mut data, &y.data, i * cols + (d - i));
                len += 1;
            }
        }
        cells.push(u.monad(&Array::new(vec![len], data), ctx, span)?);
    }
    assemble(&[rows + cols - 1], cells, span)
}

// ----------------------------------------------------------------- cutting

/// Where each interval of a cut begins and ends (both inclusive of the
/// start, exclusive of the end).
///
/// `mode` is J's: 1 and -1 have the fret open an interval, 2 and -2 have it
/// close one, and the negative spellings drop the fret itself.
fn cut_ranges(frets: &[bool], mode: i64) -> Vec<(usize, usize)> {
    let n = frets.len();
    let mut out = Vec::new();
    if mode.abs() == 1 {
        let mut start: Option<usize> = None;
        for (i, &fret) in frets.iter().enumerate() {
            if fret {
                if let Some(s) = start {
                    out.push((s, i));
                }
                start = Some(i);
            }
        }
        if let Some(s) = start {
            out.push((s, n));
        }
        if mode < 0 {
            return out.into_iter().map(|(s, e)| (s + 1, e)).collect();
        }
    } else {
        let mut start = 0usize;
        for (i, &fret) in frets.iter().enumerate() {
            if fret {
                out.push((start, i + 1));
                start = i + 1;
            }
        }
        if mode < 0 {
            return out.into_iter().map(|(s, e)| (s, e - 1)).collect();
        }
    }
    out
}

/// `x u;.n y` and `u;.n y` (J).
fn cut(
    u: &Verb,
    x: Option<&Array>,
    y: &Array,
    mode: i64,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    if mode == 0 {
        if x.is_some() {
            return Err(Error::not_yet("dyadic cut with a rectangle (x u;.0 y)", span));
        }
        return u.monad(&reverse_all_axes(y), ctx, span);
    }
    if !matches!(mode, 1 | -1 | 2 | -2) {
        return Err(Error::not_yet(format!("cut (u;.{mode})"), span));
    }
    let items = if y.rank() == 0 { Array::new(vec![1], y.data.clone()) } else { y.clone() };
    let n = items.items();
    let tol = ctx.cfg.tol;
    let frets: Vec<bool> = match x {
        Some(x) => {
            let flags = x
                .to_i64_vec()
                .ok_or_else(|| Error::domain("cut frets must be integers", span))?;
            if flags.len() != n {
                return Err(Error::new(
                    ErrorKind::Length,
                    format!("{} fret(s) for {n} item(s)", flags.len()),
                    Some(span),
                ));
            }
            flags.iter().map(|&f| f != 0).collect()
        }
        None => {
            // The fret is the argument's own first or last item.
            if n == 0 {
                Vec::new()
            } else {
                let at = if mode.abs() == 1 { 0 } else { n - 1 };
                let mark = items.item(at);
                (0..n).map(|i| arrays_match(&items.item(i), &mark, tol)).collect()
            }
        }
    };
    let ranges = cut_ranges(&frets, mode);
    let mut cells = Vec::with_capacity(ranges.len());
    for (s, e) in &ranges {
        cells.push(u.monad(&section(&items, *s, *e), ctx, span)?);
    }
    assemble(&[ranges.len()], cells, span)
}

/// Every axis of `y` reversed — what `u;.0 y` applies its verb to.
fn reverse_all_axes(y: &Array) -> Array {
    if y.rank() == 0 {
        return y.clone();
    }
    let st = strides(&y.shape);
    let n = y.count();
    let r = y.rank();
    let mut data = Data::empty(y.dtype());
    let mut coord = vec![0usize; r];
    for _ in 0..n {
        let idx: usize = (0..r).map(|k| (y.shape[k] - 1 - coord[k]) * st[k]).sum();
        push_elem(&mut data, &y.data, idx);
        odometer(&mut coord, &y.shape);
    }
    Array::new(y.shape.clone(), data)
}

// ------------------------------------------------------------ along an axis

/// `y` with axis `k` moved in front of the others, their order kept.
fn axis_to_front(y: &Array, k: usize) -> Array {
    if k == 0 || y.rank() < 2 {
        return y.clone();
    }
    let r = y.rank();
    let src: Vec<usize> = std::iter::once(k).chain((0..r).filter(|&a| a != k)).collect();
    permute_axes(y, &src)
}

/// `y` with its leading axis moved to position `k`.
fn front_to_axis(y: &Array, k: usize) -> Array {
    if k == 0 || y.rank() < 2 {
        return y.clone();
    }
    let r = y.rank();
    // Output axis a reads source axis: the ones before k shift up by one,
    // k itself is the source's leading axis, the rest keep their place.
    let mut src = Vec::with_capacity(r);
    for a in 0..r {
        src.push(match a.cmp(&k) {
            std::cmp::Ordering::Less => a + 1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => a,
        });
    }
    permute_axes(y, &src)
}

/// `y` with output axis `a` reading source axis `src[a]`.
fn permute_axes(y: &Array, src: &[usize]) -> Array {
    let st = strides(&y.shape);
    let out_shape: Vec<usize> = src.iter().map(|&a| y.shape[a]).collect();
    let n = y.count();
    let mut data = Data::empty(y.dtype());
    let mut coord = vec![0usize; src.len()];
    for _ in 0..n {
        let idx: usize = (0..src.len()).map(|a| coord[a] * st[src[a]]).sum();
        push_elem(&mut data, &y.data, idx);
        odometer(&mut coord, &out_shape);
    }
    Array::new(out_shape, data)
}

/// APL `A[i;j]←v`: `base` with the elements the slots select replaced by
/// `value`. An elided slot takes its whole axis; a scalar slot drops its
/// axis from the shape the value has to match. The base is copied, so the
/// array the name held before is untouched.
pub fn amend_at(
    base: &Array,
    slots: &[Option<Array>],
    value: &Array,
    origin: i64,
    span: Span,
) -> Result<Array> {
    if slots.len() != base.rank() {
        return Err(Error::new(
            ErrorKind::Rank,
            format!(
                "indexed assignment needs one index per axis: {} slot(s) for a rank-{} value",
                slots.len(),
                base.rank()
            ),
            Some(span),
        ));
    }
    // One list of positions per axis, and the shape the value must match.
    let mut axes: Vec<Vec<usize>> = Vec::with_capacity(slots.len());
    let mut selected: Vec<usize> = Vec::new();
    for (k, slot) in slots.iter().enumerate() {
        let len = base.shape[k];
        let Some(idx) = slot else {
            axes.push((0..len).collect());
            selected.push(len);
            continue;
        };
        let Some(values) = idx.to_i64_vec() else {
            return Err(Error::new(
                ErrorKind::Type,
                "an index must be numeric",
                Some(span),
            ));
        };
        let mut positions = Vec::with_capacity(values.len());
        for v in values {
            let p = v - origin;
            if p < 0 || p as usize >= len {
                return Err(Error::new(
                    ErrorKind::Domain,
                    format!("index {v} is outside axis {k}, which has {len} element(s)"),
                    Some(span),
                ));
            }
            positions.push(p as usize);
        }
        // A scalar index drops its axis, as it does when reading.
        if idx.rank() > 0 {
            selected.push(positions.len());
        }
        axes.push(positions);
    }
    let count: usize = axes.iter().map(Vec::len).product();
    if value.rank() != 0 && (value.shape != selected || value.count() != count) {
        return Err(Error::new(
            ErrorKind::Shape,
            format!(
                "indexed assignment needs a scalar or a {} value, not a {} one",
                show_shape(&selected),
                show_shape(&value.shape)
            ),
            Some(span),
        ));
    }
    // The two sides meet at the wider type, so assigning a float into an
    // integer array widens the array rather than truncating the value.
    let dtype = DType::promote(base.dtype(), value.dtype()).ok_or_else(|| {
        Error::new(
            ErrorKind::Type,
            format!(
                "cannot put a {} value into a {} array",
                value.dtype().name(),
                base.dtype().name()
            ),
            Some(span),
        )
    })?;
    let mut out = base.cast(dtype).ok_or_else(|| Error::internal("promotion failed"))?;
    let src = value.cast(dtype).ok_or_else(|| Error::internal("promotion failed"))?;
    let strides = row_major_strides(&base.shape);
    let mut coords = vec![0usize; axes.len()];
    for n in 0..count {
        let mut rest = n;
        for k in (0..axes.len()).rev() {
            let len = axes[k].len();
            coords[k] = axes[k][rest % len];
            rest /= len;
        }
        let at: usize = coords.iter().zip(&strides).map(|(c, s)| c * s).sum();
        let from = if src.rank() == 0 { 0 } else { n };
        put_element(&mut out.data, at, &src.data, from);
    }
    Ok(out)
}

fn row_major_strides(shape: &[usize]) -> Vec<usize> {
    let mut strides = vec![1usize; shape.len()];
    for k in (0..shape.len().saturating_sub(1)).rev() {
        strides[k] = strides[k + 1] * shape[k + 1];
    }
    strides
}

/// Copy one element between two buffers of the same type.
fn put_element(dst: &mut Data, at: usize, src: &Data, from: usize) {
    match (dst, src) {
        (Data::Bool(d), Data::Bool(s)) => d.to_mut()[at] = s.as_slice()[from],
        (Data::I64(d), Data::I64(s)) => d.to_mut()[at] = s.as_slice()[from],
        (Data::Ext(d), Data::Ext(s)) => d.to_mut()[at] = s.as_slice()[from].clone(),
        (Data::Rat(d), Data::Rat(s)) => d.to_mut()[at] = s.as_slice()[from].clone(),
        (Data::F64(d), Data::F64(s)) => d.to_mut()[at] = s.as_slice()[from],
        (Data::Char(d), Data::Char(s)) => d.to_mut()[at] = s.as_slice()[from],
        (Data::Box(d), Data::Box(s)) => d.to_mut()[at] = s.as_slice()[from].clone(),
        // Both sides were cast to one type above.
        _ => debug_assert!(false, "amend across types"),
    }
}

/// Which of an agenda's verbs the selector picks. The selector runs at the
/// same arguments the agenda was given, and its value must be one index.
fn agenda_pick(
    vs: &[Verb],
    w: &Verb,
    x: Option<&Array>,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Verb> {
    let chosen = match x {
        None => w.monad(y, ctx, span)?,
        Some(x) => w.dyad(x, y, ctx, span)?,
    };
    let at = chosen
        .to_i64_vec()
        .and_then(|v| v.first().copied())
        .ok_or_else(|| Error::domain("an agenda index must be an integer", span))?;
    pick_gerund(vs, at, span)
}

/// One verb of a gerund by index, with the diagnostic the out-of-range case
/// deserves.
pub(crate) fn pick_gerund(vs: &[Verb], at: i64, span: Span) -> Result<Verb> {
    usize::try_from(at)
        .ok()
        .and_then(|k| vs.get(k))
        .cloned()
        .ok_or_else(|| {
            Error::domain(
                format!("agenda {at} is out of range: the gerund has {} verbs", vs.len()),
                span,
            )
        })
}

/// `x u\. y`: u applied to y with every run of x consecutive items removed.
/// A run of x items has `1 + (#y) - x` places to sit, and that is how many
/// results there are.
fn outfix(u: &Verb, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let k = one_int(x, "an outfix width", span)?;
    let n = y.items() as i64;
    if k < 0 {
        return Err(Error::not_yet("a negative outfix width (x u\\. y)", span));
    }
    if k > n {
        return Err(Error::new(
            ErrorKind::Length,
            format!("an outfix of {k} items over {n}"),
            Some(span),
        ));
    }
    let mut cells = Vec::with_capacity((n - k + 1) as usize);
    for start in 0..=(n - k) {
        let keep: Vec<usize> = (0..n as usize)
            .filter(|&i| i < start as usize || i >= (start + k) as usize)
            .collect();
        cells.push(u.monad(&select_items(&as_list(y), &keep), ctx, span)?);
    }
    assemble(&[cells.len()], cells, span)
}

// ---------------------------------------------------------------- obverses

/// The verb that undoes this one, where libjay knows of one.
///
/// This is J's obverse table, and it is deliberately a table rather than a
/// search: a verb is here only when its inverse is another verb libjay can
/// already write down. Everything built out of those — the compositions,
/// the bonds, `u^:n` — inverts by inverting its parts, so the table stays
/// small while `&.`, `&.:` and the negative powers reach a long way past
/// it. A verb that is not here has no obverse, and the diagnostic says so
/// by name.
pub(crate) fn obverse(v: &Verb) -> Option<Verb> {
    let swap = |name: &'static str| -> Option<Verb> {
        crate::frontend::j::verb_named(name)
    };
    Some(match v {
        Verb::Prim(p) => {
            use ScalarMonad as SM;
            let by_monad: Option<&'static str> = match p.monad {
                // Every one of these is its own inverse.
                MonadOp::Scalar(SM::Conj | SM::Neg | SM::Recip | SM::OneMinus)
                | MonadOp::Reverse
                | MonadOp::TransposeAxes => Some(p.name),
                MonadOp::Scalar(SM::Exp) => Some("^."),
                MonadOp::Scalar(SM::Ln) => Some("^"),
                MonadOp::Scalar(SM::Sqrt) => Some("*:"),
                MonadOp::Scalar(SM::Square) => Some("%:"),
                MonadOp::Scalar(SM::Double) => Some("-:"),
                MonadOp::Scalar(SM::Halve) => Some("+:"),
                MonadOp::Scalar(SM::Inc) => Some("<:"),
                MonadOp::Scalar(SM::Dec) => Some(">:"),
                MonadOp::Enclose(_) => Some(">"),
                MonadOp::Open => Some("<"),
                MonadOp::DecodeBits => Some("#:"),
                MonadOp::EncodeBits => Some("#."),
                _ => None,
            };
            swap(by_monad?)?
        }
        // An explicit obverse (`u :. v`) is the whole answer.
        Verb::WithObverse(_, w) => (**w).clone(),
        // A composition inverts by inverting its parts, in the other order.
        Verb::Atop(f, g) => {
            Verb::Atop(Box::new(obverse(g)?), Box::new(obverse(f)?))
        }
        Verb::Compose(f, g) | Verb::Beside(f, g) => {
            Verb::Atop(Box::new(obverse(g)?), Box::new(obverse(f)?))
        }
        Verb::Rank(f, r) => Verb::Rank(Box::new(obverse(f)?), *r),
        Verb::Fit(f, n) => Verb::Fit(Box::new(obverse(f)?), *n),
        // `u^:n` undone is `u^:_1` done n times.
        Verb::PowerN(f, Power::Times(n)) => {
            Verb::PowerN(Box::new(obverse(f)?), Power::Times(*n))
        }
        Verb::BondLeft(m, f) => bond_obverse(m, f, true)?,
        Verb::BondRight(f, n) => bond_obverse(n, f, false)?,
        _ => return None,
    })
}

/// The obverse of a bonded arithmetic verb. `left` says which side the noun
/// was bonded to, which is what tells `n - y` (its own inverse) from
/// `y - n` (whose inverse adds).
fn bond_obverse(n: &Array, f: &Verb, left: bool) -> Option<Verb> {
    let Verb::Prim(p) = f else { return None };
    let named = |name: &'static str| crate::frontend::j::verb_named(name);
    let bond = |name: &'static str, arg: &Array| -> Option<Verb> {
        let g = named(name)?;
        Some(if left {
            Verb::BondLeft(arg.clone(), Box::new(g))
        } else {
            Verb::BondRight(Box::new(g), arg.clone())
        })
    };
    use ScalarDyad as SD;
    let DyadOp::Scalar(op) = p.dyad else { return None };
    match (op, left) {
        // `n - y` and `n % y` undo themselves; the other side does not.
        (SD::Sub | SD::DivJ | SD::DivApl, true) => bond(p.name, n),
        (SD::Add, _) => bond("-", n),
        (SD::Sub, false) => bond("+", n),
        (SD::Mul, _) => bond("%", n),
        (SD::DivJ | SD::DivApl, false) => bond("*", n),
        // `y ^ n` is undone by the n-th root; `n ^ y` by the base-n log.
        (SD::Pow, false) => Some(Verb::BondLeft(n.clone(), Box::new(named("%:")?))),
        (SD::Pow, true) => Some(Verb::BondLeft(n.clone(), Box::new(named("^.")?))),
        (SD::Log, true) => Some(Verb::BondLeft(n.clone(), Box::new(named("^")?))),
        (SD::Root, true) => Some(Verb::BondLeft(n.clone(), Box::new(named("^")?))),
        _ => None,
    }
}

// ------------------------------------------------- classification and sets

/// `= y`: one row per distinct item, marking where that item stands. A
/// scalar has one item, so it answers a 1×1 table.
fn self_classify(y: &Array, tol: Tol) -> Array {
    let items = if y.rank() == 0 { 1 } else { y.items() };
    let keys = nub(&as_list(y), tol);
    let rows = keys.items();
    let mut out = Vec::with_capacity(rows * items);
    for i in 0..rows {
        let key = item_or_self(&keys, i);
        for j in 0..items {
            out.push(arrays_match(&key, &item_or_self(y, j), tol) as u8);
        }
    }
    Array::new(vec![rows, items], Data::Bool(out.into()))
}

/// `~: y` / `≠ y`: 1 at each item that has not been seen before.
fn nub_sieve(y: &Array, tol: Tol) -> Array {
    let items = if y.rank() == 0 { 1 } else { y.items() };
    let mut seen: Vec<Array> = Vec::new();
    let mut out = Vec::with_capacity(items);
    for i in 0..items {
        let cell = item_or_self(y, i);
        let fresh = !seen.iter().any(|s| arrays_match(s, &cell, tol));
        if fresh {
            seen.push(cell);
        }
        out.push(fresh as u8);
    }
    Array::new(vec![items], Data::Bool(out.into()))
}

/// A rank-0 argument as the one-item list it behaves as for the set verbs.
fn as_list(y: &Array) -> Array {
    if y.rank() == 0 { Array::new(vec![1], y.data.clone()) } else { y.clone() }
}

/// The values of `y` that an item of shape `item_rank` could match: y's
/// cells of that rank, framed by whatever axes are left. A y with no room
/// for a frame is one such value, which is what lets `(i.3 2) -. 2 3`
/// remove the row rather than nothing.
fn conforming_cells(y: &Array, item_rank: usize) -> Vec<Array> {
    let frame_rank = y.rank().saturating_sub(item_rank);
    let nf: usize = y.shape[..frame_rank].iter().product();
    (0..nf).map(|i| y.cell_at(frame_rank, i)).collect()
}

/// Which items of `y` occur among the values of `x` that could match one.
fn item_marks(y: &Array, x: &Array, tol: Tol) -> Vec<bool> {
    let n = if y.rank() == 0 { 1 } else { y.items() };
    let item_rank = y.rank().saturating_sub(1);
    let against = conforming_cells(x, item_rank);
    (0..n)
        .map(|i| {
            let cell = item_or_self(y, i);
            against.iter().any(|c| arrays_match(&cell, c, tol))
        })
        .collect()
}

/// `x -. y` / `x ~ y`: x's items with the ones y also has removed.
fn set_less(x: &Array, y: &Array, tol: Tol) -> Array {
    let xs = as_list(x);
    let marks = item_marks(&xs, y, tol);
    let keep: Vec<usize> = (0..marks.len()).filter(|&i| !marks[i]).collect();
    select_items(&xs, &keep)
}

/// `x ∩ y`: x's items that y also has, in x's order and with x's repeats.
fn intersect_items(x: &Array, y: &Array, tol: Tol) -> Array {
    let xs = as_list(x);
    let marks = item_marks(&xs, y, tol);
    let keep: Vec<usize> = (0..marks.len()).filter(|&i| marks[i]).collect();
    select_items(&xs, &keep)
}

/// `x ∪ y`: x's items, then the items of y that are new. x keeps whatever
/// repeats it has; APL's union only sieves the right argument.
fn union_items(x: &Array, y: &Array, tol: Tol, span: Span) -> Result<Array> {
    let xs = as_list(x);
    let ys = as_list(y);
    let marks = item_marks(&ys, &xs, tol);
    let mut extra: Vec<usize> = Vec::new();
    for (i, &seen) in marks.iter().enumerate() {
        if seen {
            continue;
        }
        let cell = item_or_self(&ys, i);
        if !extra.iter().any(|&j| arrays_match(&item_or_self(&ys, j), &cell, tol)) {
            extra.push(i);
        }
    }
    catenate(&xs, &select_items(&ys, &extra), true, false, span)
}

/// `x E. y` / `x ⍷ y`: 1 at each position of y where a copy of x begins.
/// A pattern longer than y matches nowhere, and the answer is still shaped
/// like y's items, as both references have it.
fn find_seq(x: &Array, y: &Array, tol: Tol) -> Array {
    let xs = as_list(x);
    let ys = as_list(y);
    let (k, n) = (xs.items(), ys.items());
    let mut out = vec![0u8; n];
    if k > 0 && k <= n {
        for (start, slot) in out.iter_mut().enumerate().take(n - k + 1) {
            let hit = (0..k).all(|d| {
                arrays_match(&item_or_self(&xs, d), &item_or_self(&ys, start + d), tol)
            });
            *slot = hit as u8;
        }
    }
    Array::new(vec![n], Data::Bool(out.into()))
}

/// `+:` and `*:` dyadically, and APL's `⍱` and `⍲`: both arguments must
/// already be booleans, which is the only domain either reference gives
/// them.
fn bool_dyad(op: BoolDyad, x: &Array, y: &Array, cfg: EvalCfg, span: Span) -> Result<Array> {
    let bit = |a: &Array| -> Result<u8> {
        match a.to_i64_vec().as_deref() {
            Some([0]) => Ok(0),
            Some([1]) => Ok(1),
            _ => Err(Error::domain("this verb reads values of 0 or 1", span)),
        }
    };
    let _ = cfg;
    let (a, b) = (bit(x)?, bit(y)?);
    let v = match op {
        BoolDyad::Nor => u8::from(a == 0 && b == 0),
        BoolDyad::Nand => u8::from(a == 0 || b == 0),
    };
    Ok(Array::new(vec![], Data::Bool(vec![v].into())))
}

// ------------------------------------------------------------ permutations

/// The ranks of y's items: the position each would take in a stable sort.
/// This is the permutation `A.` reports the index of, which is why a list
/// that is not itself a permutation still has an anagram index.
fn item_ranks(y: &Array, span: Span) -> Result<Vec<usize>> {
    no_grading_boxes(y, span)?;
    if !y.dtype().is_numeric() {
        return Err(Error::domain("an anagram index needs numbers", span));
    }
    let order = grade_order(&as_list(y), false);
    let mut ranks = vec![0usize; order.len()];
    for (place, &i) in order.iter().enumerate() {
        ranks[i] = place;
    }
    Ok(ranks)
}

/// `A. y`: where the permutation y's items rank as stands in the
/// lexicographic list of the permutations of that length.
fn anagram_index(y: &Array, span: Span) -> Result<Array> {
    let ranks = item_ranks(y, span)?;
    let n = ranks.len();
    let mut index: i128 = 0;
    for i in 0..n {
        let smaller = ranks[i + 1..].iter().filter(|&&r| r < ranks[i]).count() as i128;
        index = index
            .checked_mul((n - i) as i128)
            .and_then(|v| v.checked_add(smaller))
            .ok_or_else(|| Error::not_yet("an anagram index too large for an integer", span))?;
    }
    i64::try_from(index)
        .map(Array::scalar_i64)
        .map_err(|_| Error::not_yet("an anagram index too large for an integer", span))
}

/// `x A. y`: y's items in the order the x-th permutation puts them. A
/// negative x counts back from the last permutation, as J's does.
fn anagram_from(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let idx = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("an anagram index must be an integer", span))?;
    let Some(&want) = idx.first() else {
        return Err(Error::internal("anagram with no index"));
    };
    let ys = as_list(y);
    let n = ys.items();
    let mut total: i128 = 1;
    for k in 1..=n as i128 {
        total = total
            .checked_mul(k)
            .ok_or_else(|| Error::not_yet("permuting more items than an integer counts", span))?;
    }
    let mut at = want as i128;
    if at < 0 {
        at += total;
    }
    if at < 0 || at >= total {
        return Err(Error::domain(
            format!("permutation {want} is out of range: {n} items have {total} of them"),
            span,
        ));
    }
    // The factorial number system, read most significant digit first: each
    // digit picks one of the items still unused.
    let mut pool: Vec<usize> = (0..n).collect();
    let mut order = Vec::with_capacity(n);
    let mut fact = total;
    for i in 0..n {
        fact /= (n - i) as i128;
        let d = (at / fact) as usize;
        at %= fact;
        order.push(pool.remove(d));
    }
    Ok(select_items(&ys, &order))
}

/// `C. y`: the two directions between a direct permutation and its cycles.
/// A boxed argument holds cycles and answers the permutation; anything else
/// is a permutation and answers its cycles.
fn cycle_form(y: &Array, span: Span) -> Result<Array> {
    if y.dtype() == DType::Box {
        let perm = cycles_to_direct(y, span)?;
        return Ok(Array::from_i64(perm.iter().map(|&i| i as i64).collect()));
    }
    let perm = direct_permutation(y, span)?;
    let mut boxes: Vec<Array> = Vec::new();
    let mut done = vec![false; perm.len()];
    for start in 0..perm.len() {
        if done[start] {
            continue;
        }
        let mut cycle = Vec::new();
        let mut at = start;
        while !done[at] {
            done[at] = true;
            cycle.push(at);
            at = perm[at];
        }
        // J writes each cycle starting at its largest element, and lists
        // the cycles in order of those.
        let top = cycle.iter().position(|&v| v == *cycle.iter().max().unwrap()).unwrap();
        cycle.rotate_left(top);
        boxes.push(Array::boxed(Array::from_i64(
            cycle.iter().map(|&i| i as i64).collect(),
        )));
    }
    boxes.sort_by_key(|b| b.as_boxes().map(|s| s[0].to_i64_vec().unwrap()[0]).unwrap_or(0));
    let n = boxes.len();
    let inner: Vec<Array> =
        boxes.into_iter().map(|b| b.as_boxes().unwrap()[0].clone()).collect();
    Ok(Array::new(vec![n], Data::Box(inner.into())))
}

/// A direct permutation's entries, checked to be one.
fn direct_permutation(y: &Array, span: Span) -> Result<Vec<usize>> {
    let v = y
        .to_i64_vec()
        .ok_or_else(|| Error::domain("a permutation is a list of integers", span))?;
    let n = v.len();
    let mut seen = vec![false; n];
    let mut out = Vec::with_capacity(n);
    for &i in &v {
        let k = usize::try_from(i).ok().filter(|&k| k < n && !seen[k]).ok_or_else(|| {
            Error::domain(format!("{i} does not belong to a permutation of {n} items"), span)
        })?;
        seen[k] = true;
        out.push(k);
    }
    Ok(out)
}

/// The direct permutation a boxed list of cycles stands for. Its length is
/// one past the largest element any cycle mentions; everything unmentioned
/// stays where it is.
fn cycles_to_direct(y: &Array, span: Span) -> Result<Vec<usize>> {
    let boxes = y.as_boxes().ok_or_else(|| Error::internal("cycles from a simple array"))?;
    let mut cycles: Vec<Vec<usize>> = Vec::new();
    let mut top = 0usize;
    for b in boxes {
        let v = b
            .to_i64_vec()
            .ok_or_else(|| Error::domain("a cycle is a list of integers", span))?;
        let mut cycle = Vec::with_capacity(v.len());
        for &i in &v {
            let k = usize::try_from(i)
                .map_err(|_| Error::domain(format!("{i} is not an index"), span))?;
            top = top.max(k + 1);
            cycle.push(k);
        }
        cycles.push(cycle);
    }
    let mut perm: Vec<usize> = (0..top).collect();
    for cycle in &cycles {
        for w in 0..cycle.len() {
            // Cycle (a b c) sends a's slot to b's item, b's to c's, c's to a's.
            perm[cycle[w]] = cycle[(w + 1) % cycle.len()];
        }
    }
    Ok(perm)
}

/// `x C. y`: y's items permuted by x. A boxed x holds cycles; a numeric x
/// is a direct permutation, and one shorter than y applies to y's last
/// items with the leading ones brought round to the front — J's extension
/// of a short permutation.
fn permute(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let ys = as_list(y);
    let n = ys.items();
    let cyclic = x.dtype() == DType::Box;
    if !cyclic && x.rank() == 0 {
        return Err(Error::not_yet("permuting by a single atom (x C. y)", span));
    }
    let mut perm =
        if cyclic { cycles_to_direct(x, span)? } else { direct_permutation(&as_list(x), span)? };
    if perm.len() > n {
        return Err(Error::new(
            ErrorKind::Length,
            format!("a permutation of {} items applied to {n}", perm.len()),
            Some(span),
        ));
    }
    if perm.len() < n {
        if cyclic {
            // Cycles name only what moves: everything else stays put.
            perm.extend(perm.len()..n);
        } else {
            // A short direct permutation applies to the items it counts,
            // and the ones past it come round to the front.
            let head: Vec<usize> = (perm.len()..n).collect();
            perm.splice(0..0, head);
        }
    }
    Ok(select_items(&ys, &perm))
}

// ------------------------------------------------------- text and structure

/// `u: y` and `⎕UCS`: characters and their codepoints. `pass_chars` is J's
/// monad, which answers characters with themselves; APL's `⎕UCS` converts
/// in both directions.
fn unicode(y: &Array, pass_chars: bool, span: Span) -> Result<Array> {
    if y.dtype() == DType::Char {
        if pass_chars {
            return Ok(y.clone());
        }
        return Ok(chars_to_codes(y));
    }
    codes_to_chars(y, span)
}

fn chars_to_codes(y: &Array) -> Array {
    let Data::Char(v) = &y.data else { return y.clone() };
    Array::new(y.shape.clone(), Data::I64(v.iter().map(|&c| c as i64).collect()))
}

fn codes_to_chars(y: &Array, span: Span) -> Result<Array> {
    let v = y
        .to_i64_vec()
        .ok_or_else(|| Error::domain("a codepoint must be an integer", span))?;
    let mut out = Vec::with_capacity(v.len());
    for &c in &v {
        let ch = u32::try_from(c).ok().and_then(char::from_u32).ok_or_else(|| {
            Error::domain(format!("{c} is not a Unicode codepoint"), span)
        })?;
        out.push(ch);
    }
    Ok(Array::new(y.shape.clone(), Data::Char(out.into())))
}

/// `x u: y`: 3 asks for codepoints, 10 for the characters they name. The
/// other forms J defines are byte-oriented and are named, not guessed at.
fn unicode_form(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let form = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("a conversion form is an integer", span))?
        .first()
        .copied()
        .unwrap_or(0);
    match form {
        3 if y.dtype() == DType::Char => Ok(chars_to_codes(y)),
        3 => Err(Error::domain("form 3 converts characters to codepoints", span)),
        10 => codes_to_chars(y, span),
        n => Err(Error::not_yet(format!("the byte-oriented unicode form ({n} u:)"), span)),
    }
}

/// `L. y`: how deep the boxing goes. Anything unboxed is level 0.
fn boxing_level(y: &Array) -> i64 {
    match y.as_boxes() {
        None => 0,
        Some(bs) => 1 + bs.iter().map(boxing_level).max().unwrap_or(0),
    }
}

/// `↓ y`: split — the vectors along the last axis, each enclosed, laid out
/// in the shape the remaining axes give. GNU APL has no monadic `↓`; this
/// follows Dyalog's published definition.
fn split_items(y: &Array) -> Array {
    if y.rank() == 0 {
        return Array::boxed(y.clone());
    }
    let last = y.shape[y.rank() - 1];
    let outer: Vec<usize> = y.shape[..y.rank() - 1].to_vec();
    let n: usize = outer.iter().product();
    let mut boxes = Vec::with_capacity(n);
    for i in 0..n {
        let mut data = Data::empty(y.dtype());
        for k in 0..last {
            push_elem(&mut data, &y.data, i * last + k);
        }
        boxes.push(Array::new(vec![last], data));
    }
    Array::new(outer, Data::Box(boxes.into()))
}

/// `x ⊃ y`: pick. Each item of x is one step of a path — a boxed step is a
/// whole coordinate vector, a simple one indexes the items.
fn pick(x: &Array, y: &Array, origin: i64, span: Span) -> Result<Array> {
    let xs = as_list(x);
    let mut cur = y.clone();
    for i in 0..xs.items() {
        let step = open_cell(&item_or_self(&xs, i));
        let idx = step
            .to_i64_vec()
            .ok_or_else(|| Error::domain("a pick path holds integers", span))?;
        let base =
            if cur.rank() == 0 { Array::new(vec![1], cur.data.clone()) } else { cur.clone() };
        if idx.len() > base.rank() {
            return Err(Error::new(
                ErrorKind::Length,
                format!(
                    "a path step of {} index(es) into a value of rank {}",
                    idx.len(),
                    cur.rank()
                ),
                Some(span),
            ));
        }
        let zeroed: Vec<i64> = idx.iter().map(|&v| v - origin).collect();
        let at = cell_index(&base, &zeroed, span)?;
        cur = open_cell(&base.cell_at(idx.len(), at));
    }
    Ok(cur)
}

// ------------------------------------------------------------------ primes

/// `x p: y`: the facts about primes J spells with this conjunction of
/// arguments. Every form here reads one integer and answers about it.
fn prime_meta(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let form = one_int(x, "a prime query", span)?;
    let n = one_int(y, "a prime query", span)?;
    match form {
        // How many primes are below y.
        -1 => Ok(Array::scalar_i64(primes_below(n, span)?)),
        // Whether y is prime, and its negation.
        0 => Ok(Array::scalar_bool(!is_prime(n))),
        1 => Ok(Array::scalar_bool(is_prime(n))),
        // The factorisation as a table, and its top row on its own.
        2 | 3 => {
            let (ps, es) = factor_table(n, span)?;
            let k = ps.len();
            if form == 3 {
                return Ok(Array::from_i64(ps));
            }
            let mut all = ps;
            all.extend(es);
            Ok(Array::new(vec![2, k], Data::I64(all.into())))
        }
        // The neighbouring primes.
        4 => Ok(Array::scalar_i64(next_prime(n, span)?)),
        -4 => Ok(Array::scalar_i64(previous_prime(n, span)?)),
        other => Err(Error::domain(format!("{other} is not a prime query"), span)),
    }
}

/// `x q: y`: the exponents of the primes in y — of the first x of them, or,
/// for `__`, of the ones that actually divide y over a second row.
fn prime_exponents(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let n = one_int(y, "prime exponents", span)?;
    let count = x.to_f64_vec().and_then(|v| v.first().copied()).unwrap_or(0.0);
    let (ps, es) = factor_table(n, span)?;
    if count == f64::NEG_INFINITY {
        let k = ps.len();
        let mut all = ps;
        all.extend(es);
        return Ok(Array::new(vec![2, k], Data::I64(all.into())));
    }
    let want = one_int(x, "prime exponents", span)?;
    if want < 0 {
        return Err(Error::not_yet(format!("the prime exponent form ({want} q:)"), span));
    }
    let mut out = Vec::with_capacity(want as usize);
    for i in 0..want {
        let p = nth_prime(i, span)?;
        out.push(ps.iter().position(|&q| q == p).map_or(0, |at| es[at]));
    }
    Ok(Array::from_i64(out))
}

/// y's distinct prime factors, ascending, and how often each divides it.
fn factor_table(n: i64, span: Span) -> Result<(Vec<i64>, Vec<i64>)> {
    let factors = prime_factors(n, span)?;
    let mut ps: Vec<i64> = Vec::new();
    let mut es: Vec<i64> = Vec::new();
    for f in factors {
        if ps.last() == Some(&f) {
            *es.last_mut().unwrap() += 1;
        } else {
            ps.push(f);
            es.push(1);
        }
    }
    Ok((ps, es))
}

fn is_prime(n: i64) -> bool {
    if n < 2 {
        return false;
    }
    let mut d = 2i64;
    while d.saturating_mul(d) <= n {
        if n % d == 0 {
            return false;
        }
        d += 1;
    }
    true
}

fn primes_below(n: i64, span: Span) -> Result<i64> {
    if n < 0 {
        return Err(Error::domain("counting the primes below a negative number", span));
    }
    Ok((2..n).filter(|&k| is_prime(k)).count() as i64)
}

fn next_prime(n: i64, span: Span) -> Result<i64> {
    let mut k = n.checked_add(1).ok_or_else(|| Error::domain("no next prime", span))?;
    while !is_prime(k) {
        k = k.checked_add(1).ok_or_else(|| Error::domain("no next prime", span))?;
    }
    Ok(k)
}

fn previous_prime(n: i64, span: Span) -> Result<i64> {
    let mut k = n - 1;
    while k >= 2 {
        if is_prime(k) {
            return Ok(k);
        }
        k -= 1;
    }
    Err(Error::domain(format!("there is no prime below {n}"), span))
}

/// One whole number from an argument that has to hold exactly that.
fn one_int(a: &Array, what: &str, span: Span) -> Result<i64> {
    a.to_i64_vec()
        .and_then(|v| v.first().copied())
        .ok_or_else(|| Error::domain(format!("{what} needs an integer"), span))
}

/// `x \\ y`: expand. Every 1 in x takes the next item of y; every 0 leaves
/// the type's fill in its place.
fn expand(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let mask = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("an expansion mask holds 0s and 1s", span))?;
    if mask.iter().any(|&b| b != 0 && b != 1) {
        return Err(Error::domain("an expansion mask holds 0s and 1s", span));
    }
    let ys = as_list(y);
    let taken = mask.iter().filter(|&&b| b == 1).count();
    let n = ys.items();
    // A one-item argument spreads over every slot the mask opens.
    let spread = n == 1 && taken != 1;
    if !spread && taken != n {
        return Err(Error::new(
            ErrorKind::Length,
            format!("an expansion mask taking {taken} item(s) over {n}"),
            Some(span),
        ));
    }
    let m = ys.item_size();
    let mut data = Data::empty(ys.dtype());
    let mut at = 0usize;
    for &b in &mask {
        if b == 1 {
            let from = if spread { 0 } else { at };
            for k in 0..m {
                push_elem(&mut data, &ys.data, from * m + k);
            }
            at += 1;
        } else {
            for _ in 0..m {
                data.push_fill();
            }
        }
    }
    let mut shape = ys.shape.clone();
    if shape.is_empty() {
        shape.push(mask.len());
    } else {
        shape[0] = mask.len();
    }
    Ok(Array::new(shape, data))
}

/// `". y` and `⍎ y`: the characters of y as a program of this language,
/// compiled now and run here.
///
/// The nested program shares the caller's names and its output sink, which
/// is what makes `". 'a =. 3'` assign in the scope the sentence stands in.
/// It reaches nothing the caller could not reach: the sandbox contract is
/// about what a primitive may touch, and evaluation touches nothing new.
fn execute(y: &Array, apl: bool, origin: i64, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let Data::Char(v) = &y.data else {
        return Err(Error::domain("execute reads a character list", span));
    };
    let src: String = v.iter().collect();
    let lang = if apl { crate::Lang::Apl } else { crate::Lang::J };
    let dialect = crate::Dialect { index_origin: apl.then_some(origin) };
    let nested = crate::compile(lang, &src, &dialect).map_err(|e| nested_error(e, &src, span))?;
    if !nested.params.is_empty() {
        return Err(Error::domain(
            "an executed string cannot take host data: `{name}` has nothing to bind to",
            span,
        ));
    }
    let mut rec = None;
    let (value, _) = crate::ir::run_block(&nested.stmts, None, ctx, &mut rec)
        .map_err(|e| nested_error(e, &src, span))?;
    value.ok_or_else(|| Error::domain("the executed string yielded no value", span))
}

/// An error from an executed string, re-pointed at the sentence that ran it.
/// The inner diagnostic still reads in full, as a note, because its spans
/// point into a source the caller never sees.
fn nested_error(e: Error, src: &str, span: Span) -> Error {
    let inner = e.render(src);
    let mut out = Error::new(e.kind, format!("in the executed string: {}", e.msg), Some(span));
    out.notes.push(inner.trim_end().to_string());
    out
}

// ------------------------------------------------------------------- words

/// `;: y`: J's own word rules over a character list, each word a box. A run
/// of numeric literals separated by blanks is one word, which is what makes
/// `'1 2 3'` a single number and `'i.5'` two words.
fn words(y: &Array, span: Span) -> Result<Array> {
    let Data::Char(v) = &y.data else {
        return Err(Error::domain("words reads a character list", span));
    };
    let src: Vec<char> = v.as_slice().to_vec();
    let n = src.len();
    let mut out: Vec<Array> = Vec::new();
    let mut i = 0usize;
    let numeric_start = |k: usize| -> bool {
        k < n && (src[k].is_ascii_digit() || src[k] == '_')
    };
    while i < n {
        let c = src[i];
        if c == ' ' || c == '\t' {
            i += 1;
            continue;
        }
        let start = i;
        if c == '\'' {
            i += 1;
            loop {
                if i >= n {
                    return Err(Error::parse("a word list ends inside a string", span));
                }
                if src[i] == '\'' {
                    i += 1;
                    if i < n && src[i] == '\'' {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
        } else if c.is_ascii_alphabetic() {
            while i < n && (src[i].is_ascii_alphanumeric() || src[i] == '_') {
                i += 1;
            }
            if i < n && (src[i] == '.' || src[i] == ':') {
                i += 1;
            }
            // `NB.` swallows the rest of the line, comment and all.
            if src[start..i].iter().collect::<String>() == "NB." {
                while i < n && src[i] != '\n' {
                    i += 1;
                }
            }
        } else if numeric_start(i) {
            loop {
                while i < n && (src[i].is_ascii_alphanumeric() || src[i] == '.' || src[i] == '_')
                {
                    i += 1;
                }
                // A blank between two numeric literals keeps one word.
                let mut j = i;
                while j < n && src[j] == ' ' {
                    j += 1;
                }
                if j > i && numeric_start(j) {
                    i = j;
                    continue;
                }
                break;
            }
        } else {
            i += 1;
            while i < n && (src[i] == '.' || src[i] == ':') {
                i += 1;
            }
        }
        out.push(Array::from_chars(src[start..i].to_vec()));
    }
    let k = out.len();
    Ok(Array::new(vec![k], Data::Box(out.into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A context bound to a discarding output sink.
    macro_rules! ctx {
        ($name:ident, $agreement:expr) => {
            let mut sink = |_: &str| {};
            let mut env = Env::new(Vec::new());
            #[allow(unused_mut)]
            let mut $name = Ctx {
                cfg: EvalCfg { agreement: $agreement, fmt: FmtOpts::J, tol: Tol::J },
                out: &mut sink,
                env: &mut env,
                device: None,
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
    fn square_root_of_a_negative_number_is_complex() {
        ctx!(c);
        let r = sqrt_v().monad(&Array::from_i64(vec![9]), &mut c, sp()).unwrap();
        assert!(close(floats(&r)[0], 3.0));
        let r = sqrt_v().monad(&Array::from_i64(vec![-4]), &mut c, sp()).unwrap();
        assert_eq!(r.dtype(), DType::Complex);
        assert_eq!(r.as_complex_slice().expect("complex data"), &[[0.0, 2.0]]);
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
        // A vector of lengths asks for an array of index vectors.
        let e = iota_apl(1).monad(&Array::from_i64(vec![2, 3]), &mut c, sp()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::NotYet);
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
        let mut env = Env::new(Vec::new());
        let mut c = Ctx {
            cfg: EvalCfg { agreement: Agreement::LeadingPrefix, fmt: FmtOpts::J, tol: Tol::J },
            out: &mut sink,
            env: &mut env,
            device: None,
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
