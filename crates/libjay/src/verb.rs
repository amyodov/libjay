//! Verbs and the rank machinery: the language-agnostic execution core.
//!
//! A `Verb` is a semantic object — a primitive or a combination of verbs —
//! applied monadically or dyadically to arrays. Frontends lower J/APL syntax
//! to `Verb` trees; nothing in here knows any surface syntax.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::array::{Array, Buf, Data, Layout, NearInt};
use crate::complex::{self as cx, Cx};
use crate::dtype::DType;
use crate::error::{Error, ErrorKind, Result, Span};
use crate::exact::{self, Ext, Rat};
use crate::fmt::FmtOpts;
use crate::frontend::{ComplexOrder, NestedGrade, Rules};
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

    /// Whose rule this is. A scalar verb is handed the tolerance and
    /// nothing else about the dialect, and two rules below need to know
    /// which one they are under: J reads a magnitude below the tolerance
    /// as zero, and J's equality is total across the box boundary where
    /// APL's reaches inside the box instead.
    #[inline(always)]
    pub fn is_j(self) -> bool {
        self.by_smaller
    }

    /// Whether the tolerance reads this magnitude as zero.
    ///
    /// J's signum does: `* 1e_15` is 0 and `* 6e_14` is 1, the threshold
    /// being the tolerance itself. APL's `×` is exact there. With `!.0` the
    /// tolerance is zero, so the rule falls away with it.
    #[inline(always)]
    pub fn is_zero(self, y: f64) -> bool {
        self.is_j() && y.abs() < self.ct
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
    ///
    /// The two references read "just under" differently, and both were
    /// probed. J scales the gap by the magnitude, so `<. 99.999999999995`
    /// is 100 and `<. _1e_14` is `_1`. GNU APL shifts by the tolerance
    /// itself, so `⌊99.999999999995` is 99 — the gap of 5e¯12 is larger
    /// than `⎕CT` however big the value is — while `⌊¯1E¯13` is 0.
    #[inline(always)]
    pub fn floor(self, y: f64) -> f64 {
        if self.is_j() {
            let c = y.ceil();
            if self.eq(y, c) { c } else { y.floor() }
        } else {
            (y + self.ct).floor()
        }
    }

    /// `>. y`: the ceiling, with a value just over an integer counting as
    /// that integer. The two readings are [`Tol::floor`]'s, mirrored.
    #[inline(always)]
    pub fn ceil(self, y: f64) -> f64 {
        if self.is_j() {
            let f = y.floor();
            if self.eq(y, f) { f } else { y.ceil() }
        } else {
            (y - self.ct).ceil()
        }
    }

    /// `x | y`: the remainder of y on division by x, with the quotient read
    /// tolerantly. Both references round the quotient before subtracting,
    /// which is what makes `0.1|0.3` zero rather than a rounding error, and
    /// each rounds it its own way.
    ///
    /// J takes the tolerant floor of the quotient and then answers an exact
    /// zero whenever the product is tolerantly the dividend: `2 | 1e_14` is
    /// `1e_14` (the quotient is nowhere near an integer) while
    /// `2 | 4 + 1e_14` is 0 (the product 4 is tolerantly the dividend).
    ///
    /// GNU APL reads the remainder against the MODULUS instead: a remainder
    /// within `⎕CT` of the modulus's magnitude is zero, so `2|1E¯14` is 0
    /// where J keeps the `1e_14`. A remainder that rounding has pushed out
    /// of `[0, x)` comes back into range.
    #[inline]
    pub fn residue(self, x: f64, y: f64) -> f64 {
        // An infinite DIVIDEND has no residue at all under any nonzero
        // modulus: jconsole refuses `2 | _`, `0.5 | _`, `_1 | _` and `_ | _`
        // alike with a NaN error, and the NaN made here is what
        // [`Tol::made_nan`] turns into that refusal. A zero modulus is the
        // exception, because it never divides: `0 | _` is `_`.
        if self.is_j() && y.is_infinite() && x != 0.0 {
            return f64::NAN;
        }
        // An infinite modulus leaves a value of its own sign alone and
        // sends the other one to that infinity, which is the limit both
        // references answer with; the general formula cannot reach it,
        // because it runs into `inf * 0`.
        if x.is_infinite() {
            return if y == 0.0 || (y > 0.0) == (x > 0.0) { y } else { x };
        }
        if x == 0.0 {
            return y;
        }
        if self.is_j() {
            let p = x * self.floor(y / x);
            return if self.eq(y, p) { 0.0 } else { y - p };
        }
        // GNU APL counts the quotient as its ceiling when the gap to it is
        // within `⎕CT` either outright or relative to the magnitude: the
        // first is what makes `1|¯1E¯14` zero, the second what makes
        // `1E¯15|1` zero, where the quotient is 1e15 and the gap 0.1.
        let q = y / x;
        let c = q.ceil();
        let gap = c - q;
        let k = if gap <= self.ct || gap < self.ct * q.abs().max(c.abs()) { c } else { q.floor() };
        let r = y - x * k;
        if r.abs() < self.ct * x.abs() {
            0.0
        } else if r != 0.0 && (r < 0.0) != (x < 0.0) {
            r + x
        } else {
            r
        }
    }

    /// `x * y`, with J's rule that a zero factor wins.
    ///
    /// J defines `0 * _` as 0 where IEEE arithmetic has no value for it, and
    /// the rule is the factor's, not the product's: `0 * _.` is 0 too, and
    /// `*/ 0 , _` is 0. It is also what gives `j. _` its value, because a
    /// complex product is four real ones and `_ * 0j1` is `0j_` only when
    /// each of them follows this rule. APL never meets the case — GNU APL
    /// refuses an infinite operand to `×` outright — so the rule is J's
    /// alone and a finite pair is untouched, negative zero included.
    #[inline(always)]
    pub fn mul(self, x: f64, y: f64) -> f64 {
        if self.is_j() && (x == 0.0 || y == 0.0) && !(x.is_finite() && y.is_finite()) {
            return 0.0;
        }
        x * y
    }

    /// Whether a result must be refused because the arithmetic MADE this
    /// NaN: J answers `_ - _`, `_ % _`, `2 | _`, `0 ^. 0` and `! __` with a
    /// NaN error, while a NaN the program itself wrote travels on unrefused
    /// (`_. + 1` is `_.`). Distinguishing the two is exactly the operand
    /// test below. APL never reaches a NaN with a value of its own, so the
    /// rule stays J's.
    #[inline(always)]
    pub fn made_nan(self, r: f64, x: f64, y: f64) -> bool {
        self.is_j() && r.is_nan() && !x.is_nan() && !y.is_nan()
    }
}

/// One infinity or NaN in J's own spelling, for a diagnostic that has the
/// value and not the text the user wrote.
pub(crate) fn j_number(v: f64) -> String {
    if v.is_nan() {
        "_.".to_string()
    } else if v == f64::INFINITY {
        "_".to_string()
    } else if v == f64::NEG_INFINITY {
        "__".to_string()
    } else {
        format!("{v}")
    }
}

/// The effect-free half of the execution context. Copyable, so a path that
/// runs cells on other threads can carry it there; neither the output sink
/// nor the input source can go along, which is what keeps those paths pure
/// by construction.
#[derive(Clone, Copy, Debug)]
pub struct EvalCfg {
    pub agreement: Agreement,
    pub fmt: FmtOpts,
    /// Comparison tolerance in force; it starts as the dialect's and `u!.n`
    /// overrides it inside the verb it is attached to.
    pub tol: Tol,
    /// The dialect's settings, resolved once at compile time. A rule that
    /// only bites at run time reads it from here rather than deducing it.
    pub rules: Rules,
}

impl EvalCfg {
    /// Run `f` with a context whose sink is never reached, and whose names
    /// are empty. Only a verb that [`Verb::is_pure`] accepted is given one
    /// of these, and an explicit definition — the only thing that reads
    /// names — is never pure.
    /// The near-integer admission counts, lengths and indices are read
    /// with here. It is the language's, not the dialect's: no setting
    /// moves it in either reference.
    pub(crate) fn near(self) -> NearInt {
        NearInt::of(self.rules.lang)
    }

    pub(crate) fn pure<R>(self, f: impl FnOnce(&mut Ctx<'_>) -> R) -> R {
        let mut sink = |_: &str| debug_assert!(false, "a pure verb wrote to the output sink");
        let mut env = Env::new(Vec::new());
        f(&mut Ctx { cfg: self, out: &mut sink, inp: None, env: &mut env, device: None })
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
        if let Some(frame) = self.frames.last() && let Some(v) = frame.get(name) {
            return Some(v.clone());
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

    pub fn undefine(&mut self, name: &str) {
        self.verbs.remove(name);
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

/// A run's source of input: one line per call, with no line terminator,
/// and `None` once the input has ended.
///
/// `None` in place of the closure is a run the host attached no input to at
/// all, which is a different thing from a source that has run out: the
/// first is a wiring mistake in the embedding, the second is the program
/// asking for more than it was given, and the two say so differently.
pub type InputFn<'a> = Option<&'a mut dyn FnMut() -> Option<String>>;

/// Lend an input source to a shorter-lived context. A `&mut` inside an
/// `Option` does not reborrow on its own, so the borrow is taken apart and
/// put back.
pub fn reborrow_input<'s, 'a: 's>(inp: &'s mut InputFn<'a>) -> InputFn<'s> {
    match inp {
        Some(f) => Some(&mut **f),
        None => None,
    }
}

/// Execution context threaded through evaluation.
pub struct Ctx<'a> {
    pub cfg: EvalCfg,
    /// Sink for explicit output (`echo`, `⎕←`, `⍞←`). stdout by default per
    /// the sandbox contract; the host may redirect.
    pub out: &'a mut dyn FnMut(&str),
    /// Source for explicit input (`⍞`, `⎕`, J's `1!:1 ]1`). stdin by
    /// default per the sandbox contract; the host may redirect, and a host
    /// that attaches none makes every read a diagnostic.
    pub inp: InputFn<'a>,
    /// The names the program has bound so far.
    pub env: &'a mut Env,
    /// Where the run was placed. None is the CPU, which is also what every
    /// path that cannot use a device does; only a fused node reads it.
    pub device: Option<&'a crate::device::Device>,
}

/// How deep one application may sit inside another before libjay stops.
///
/// Every level costs stack frames — in the expression walk, in the rank
/// machinery, in a verb's own tree — and a string is the interface, so a
/// pathological one must come back as a diagnostic rather than take the
/// host process down with it. The count is per THREAD, which is what a
/// stack belongs to: a cell handed to another worker starts from zero on a
/// stack of its own.
const MAX_NESTING: usize = 400;

thread_local! {
    static NESTING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Report a tree already known to be too deep to walk.
pub(crate) fn check_nesting(depth: usize, span: Span) -> Result<()> {
    if depth > MAX_NESTING {
        return Err(Error::new(
            ErrorKind::Limit,
            format!("this program nests more than {MAX_NESTING} applications deep"),
            Some(span),
        ));
    }
    Ok(())
}

/// One level of nesting, released when it goes out of scope.
pub(crate) struct Nesting;

impl Nesting {
    /// Claim a level, or report that the program nests too deeply.
    pub(crate) fn enter(span: Span) -> Result<Nesting> {
        let depth = NESTING.with(|c| {
            let d = c.get() + 1;
            c.set(d);
            d
        });
        if depth > MAX_NESTING {
            NESTING.with(|c| c.set(c.get() - 1));
            return Err(Error::new(
                ErrorKind::Limit,
                format!("this program nests more than {MAX_NESTING} applications deep"),
                Some(span),
            ));
        }
        Ok(Nesting)
    }
}

impl Drop for Nesting {
    fn drop(&mut self) {
        NESTING.with(|c| c.set(c.get().saturating_sub(1)));
    }
}

impl Ctx<'_> {
    /// Run `f` in this context with the comparison tolerance replaced.
    fn with_tol<R>(&mut self, tol: Tol, f: impl FnOnce(&mut Ctx<'_>) -> R) -> R {
        let cfg = EvalCfg { tol, ..self.cfg };
        f(&mut Ctx {
            cfg,
            out: &mut *self.out,
            inp: reborrow_input(&mut self.inp),
            env: &mut *self.env,
            device: self.device,
        })
    }

    /// One line of input, without its terminator.
    ///
    /// Both ways of having no line are errors rather than empty strings: a
    /// program that asks for input reaches for something the host has to
    /// have supplied, and an empty line is a line.
    pub(crate) fn read_line(&mut self, span: Span) -> Result<String> {
        let Some(read) = self.inp.as_deref_mut() else {
            return Err(Error::new(
                ErrorKind::Value,
                "this expression reads input, and this run has no input source attached",
                Some(span),
            )
            .note("attach one with Program::run_io (Rust), input= (Python), or jay_run_io (C)"));
        };
        read().ok_or_else(|| {
            Error::new(ErrorKind::Value, "the input has ended: there is no line to read", Some(span))
        })
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
    /// Each item raveled into a row of a table (J `,.`). The answer never
    /// has a rank below two, so an atom becomes a one-by-one table.
    RavelItems,
    /// Reverse the axes (J `|:`, APL `⍉`).
    TransposeAxes,
    /// `{ y`: catalogue — one element from each item of y, in every
    /// combination, each combination boxed.
    Catalogue,
    /// J `5!:1`: the atomic representation of the entity a boxed name
    /// stands for, boxed. A noun stands for itself, so its representation
    /// is the pair `('0'; <value)`.
    AtomicRep,
    /// `e. y`: raze-in — for every element of y, which items of the raze
    /// of y it holds.
    RazeIn,
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
    /// J `1!:1 y`: one line from the input source as a character vector,
    /// the terminator dropped. `y` names the stream: 1 is stdin, which the
    /// sandbox opens, and everything else is a file, which it does not.
    ReadStream,
    /// J `3!:0 y`: the code J gives the argument's element type.
    TypeCode,
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
    Depth {
        /// Negate the depth of an array whose items differ in depth or in
        /// shape, as the Dyalog line does.
        signed: bool,
    },
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
    /// J `s:`: the argument's text as interned symbols. A character list
    /// is cut on its own leading delimiter; a character table gives one
    /// name per row; a boxed argument gives one name per box.
    Symbols,
    /// J `$.`: the argument in sparse form — every axis sparse, zero the
    /// sparse element. A scalar has no axis to store along and stays dense.
    Sparse,
    /// J `;:`: J's own tokeniser over a character list, one box per word.
    Words,
    /// APL `⊆` (Dyalog): nest — enclose y unless it is already nested, or
    /// a simple scalar, which cannot be enclosed any further.
    Nest,
    /// J `L.`: the boxing level — 0 for anything unboxed, one more than the
    /// deepest content otherwise.
    LevelOf,
    /// J `{::`: y's box structure with every leaf replaced by the path that
    /// fetches it — a boxed list holding one index per level descended.
    MapPaths,
    /// J `p.`: the roots of the polynomial whose ascending coefficients y
    /// holds, as the boxed pair `multiplier ; roots`; a boxed argument of
    /// that form converts back to coefficients.
    PolyRoots,
    /// J `p..`: the derivative of the polynomial y's ascending coefficients
    /// describe, again as coefficients.
    PolyDeriv,
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
    Execute { apl: bool },
    /// Present in the language, not implemented: named feature.
    NotYet(&'static str),
    /// No monadic meaning exists for this primitive in its language.
    None,
}

/// Dyadic meaning of a primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DyadOp {
    Scalar(ScalarDyad),
    /// x $ y / x ⍴ y: lay out shape x, reusing y — its ITEMS in J, its
    /// ravel in APL.
    Reshape,
    /// x {. y / x ↑ y: per-axis take, negative from the end, overtake fills.
    Take,
    /// x }. y / x ↓ y: per-axis drop, negative from the end.
    Drop,
    /// y (APL `⊢`).
    Right,
    /// x (APL `⊣`).
    Left,
    /// `x |. y`: rotate axis k of y left by `x[k]` (negative rotates right).
    Rotate,
    /// `x ⌽ y` and `x ⊖ y`: rotate ONE axis of y — the last one when
    /// `last`, the leading one otherwise — by one amount per vector along
    /// it. APL's left argument is a whole array shaped like y with that
    /// axis removed, not J's one amount per axis.
    RotateApl { last: bool },
    /// Catenate along the LEADING axis (J `,`, APL `⍪`).
    AppendLeading,
    /// Catenate along the LAST axis (APL `,`).
    AppendLast,
    /// x i. y / x ⍳ y: the index in x's items of each cell of y, or
    /// `origin + #items(x)` when absent. `vector_left` is the Dyalog
    /// dialect's rule that the left argument must be a vector.
    IndexOf { origin: i64, vector_left: bool },
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
    /// `x # y` (J), `x/y` and `x⌿y` (APL): item i of y repeated `x[i]` times.
    /// A one-element x applies to every item.
    Copy,
    /// `x #. y` / `x ⊥ y`: mixed-radix decode. A scalar x is the base for
    /// every digit; otherwise x and y have the same length.
    Decode,
    /// `x #: y` / `x ⊤ y`: mixed-radix encode. The digits become the LEADING
    /// axis of the result, which is what makes one operation serve J's
    /// per-atom `#:` (right rank 0) and APL's `⊤` (right rank infinite).
    Encode,
    /// `x ⍋ y` and `x ⍒ y`: the items of y graded by where each of their
    /// characters sits in the collating array x.
    CollateGrade { down: bool, origin: i64 },
    /// `x |: y`: y with the named axes moved to the end. A boxed x groups
    /// axes to be run together, which is the diagonal.
    TransposeJ,
    /// `x ⍉ y`: x says, for each axis of y, which axis of the result it
    /// becomes; a repeated destination runs those axes together.
    TransposeApl,
    /// `x ⊥ y` on arguments of rank 2 and above: the inner product `+.×`
    /// over the LAST axis of x and the LEADING axis of y.
    DecodeApl,
    /// `x ⊤ y` where x has rank 2 or more: x's LEADING axis is the radix,
    /// and its remaining axes frame the result along with y's.
    EncodeApl,
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
    IntervalIndex { offset: i64, closed: bool },
    /// J `x i: y`: where each cell of y LAST sits among the items of x.
    IndexOfLast { origin: i64 },
    /// J `x %. y` / APL `x ⌹ y`: the least-squares solution of `y a = x`.
    MatrixDivide,
    /// APL `x ⊂ y`: partitioned enclose — a 1 in x opens a partition, a 0
    /// continues it, and a leading run of 0s drops those items.
    PartitionEnclose,
    /// Dyalog's partitioned enclose: the left argument counts the
    /// partitions to open before each item, rather than flagging where
    /// one begins.
    PartitionCounts,
    /// APL `x ⌷ y`: one scalar index per axis of y.
    Squad {
        origin: i64,
        /// Read the index as one item per LEADING axis, so fewer items
        /// than the rank take the trailing axes whole (the Dyalog line).
        /// Otherwise there is one item per axis, all of them named.
        leading: bool,
    },
    /// One bracket slot of APL indexing: axis `axis` of y selected by x.
    /// `rank`, when it is not zero, is the number of slots the brackets
    /// held, checked by the slot that sees the whole array.
    SelectAxis { axis: usize, rank: usize, origin: i64 },
    /// J `x {:: y`: follow the path x into y, opening a level a step.
    Fetch,
    /// J `x p. y`: the polynomial with ascending coefficients x at y. A
    /// boxed x is the `multiplier ; roots` form of the same polynomial.
    PolyEval,
    /// J `x p.. y`: the integral of the polynomial y's coefficients
    /// describe, with x as the constant term.
    PolyIntegral,
    /// APL `x ⍕ y`: format by specification — one width and precision per
    /// column of the last axis, or one pair for the whole argument.
    FormatSpec,
    /// J `x ": y`: format by specification — one `w j d` complex value per
    /// column of the last axis, or one for the whole argument. A negative
    /// width asks for the exponential form; a value that does not fit its
    /// field is written as asterisks.
    FormatSpecJ,
    /// J `x ". y`: the numbers a line of text spells, with x standing in
    /// for every word that is not one.
    ParseNumbers,
    /// J `x ;: y`: the sequential machine x describes, run over y.
    SequentialMachine,
    /// J `x m b. y`: the boolean function whose truth table `m` numbers,
    /// on two bits for `m` below 16 and on every bit of two integers for
    /// `m` from 16 to 31.
    TruthTable(u8),
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
    /// J `x s:`: the numbered symbol forms. 4 gives the names as a padded
    /// character table, 5 gives them as boxes.
    SymbolForm,
    /// APL `x ⊃ y`: pick — follow the path x into y, opening a level a step.
    Pick { origin: i64 },
    /// APL `x \ y` and `x ⍀ y`: expand — a 1 in x takes the next item of y,
    /// a 0 puts a fill in its place.
    Expand,
    /// J `x 1!:2 y`: write x, formatted as it displays and followed by a
    /// newline, to the stream y; the value is x. Stream 2 is stdout, which
    /// the sandbox opens, and everything else is a file, which it does not.
    WriteStream,
    /// J `x $.`: the numbered sparse forms. `_1` gives the shape, the
    /// sparse axes and the sparse element boxed; 0 converts between the two
    /// storage kinds; 1 makes a new sparse array from a shape; 2 to 5 and 7
    /// ask about the argument; 8 drops the stored entries that hold the
    /// sparse element.
    SparseForm,
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Power {
    /// Exactly `n` applications; 0 is the identity.
    Times(u64),
    /// Iterate until a result matches the one before it (J `u^:_`).
    Converge,
    /// A list of counts: one answer per count, framed (`u^:(0 1 2)`). A
    /// boxed count is spelled this way too — `u^:(<n)` is `u^:(i.n)`.
    Each(Vec<u64>),
    /// Every result on the way to convergence, framed (`u^:a:`).
    ConvergeTrace,
}

/// Iterations `Power::Converge` allows before giving up.
const CONVERGE_LIMIT: usize = 1 << 20;

/// The results `u M.` has already computed, keyed by the arguments that
/// produced them. Shared by every clone of the derived verb, which is what
/// makes the cache survive from one application to the next.
pub type MemoCache = Arc<std::sync::Mutex<HashMap<Vec<u64>, Array>>>;

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
    /// APL `f/` and `f⌿`: the same insert monadically, and the N-WISE
    /// REDUCTION dyadically — `n f/ y` folds each window of n items along
    /// the leading axis. J's `u/` is the table dyadically, so the two
    /// spellings cannot share a node.
    NWise(Box<Verb>),
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
    /// J `u}`: the same amend, with the indices computed rather than
    /// written — `u} y` is `(u y)} y` and `x u} y` is `x (x u y)} y`.
    AmendVerb(Box<Verb>),
    /// J `|.!.f`: shift instead of rotate, the vacated positions taking the
    /// fill f.
    ShiftFill(Array),
    /// J `u M.`: u, with the results it has already computed kept and
    /// returned again for the same arguments. The cache belongs to this
    /// derived verb, so it lives exactly as long as the program does.
    Memo(Box<Verb>, MemoCache),
    /// J `u L: n` and `u S: n`: apply u to every subarray at boxing level
    /// n or below. `L:` puts each result back where its operand was; `S:`
    /// spreads them into the items of one array.
    Level { u: Box<Verb>, level: i64, spread: bool },
    /// J `u b.`: answers questions about u rather than applying it. `0` asks
    /// for its three ranks.
    Characteristics(Box<Verb>),
    /// APL `f⍛g` (before): g's LEFT argument is prepared by f — monad
    /// `(f y) g y`, dyad `(f x) g y`. The mirror of [`Verb::Beside`].
    Before(Box<Verb>, Box<Verb>),
    /// APL `f OP` and `f OP g`: a dfn that mentions `⍺⍺` or `⍵⍵` is an
    /// OPERATOR, and this is that operator with its operands supplied. They
    /// are bound under those two names for as long as the body runs.
    UserDerived { def: Box<Verb>, alpha: Box<Verb>, omega: Option<Box<Verb>> },
    /// APL `f⌸` (key, Dyalog): the major cells are grouped by value, and f
    /// is applied to each key and the group that shares it. Monadically the
    /// group is the positions the key occupies; dyadically it is the items
    /// of the right argument at those positions.
    KeyPairs(Box<Verb>),
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
    /// what `obverse` answers with; applying the verb applies u.
    WithObverse(Box<Verb>, Box<Verb>),
    /// J `m@.v`: agenda — v's value at the arguments picks which of the
    /// gerund's verbs to apply.
    Agenda(Vec<Verb>, Box<Verb>),
    /// J `u :: v`: adverse — apply u, and if the language refuses it, apply
    /// v to the same arguments instead. A gap in libjay is not an error the
    /// program may handle, and goes straight through.
    Adverse(Box<Verb>, Box<Verb>),
    /// J `m H. n`: the generalised hypergeometric function, summed as a
    /// series over the numerator parameters m and the denominator ones n.
    Hypergeometric { num: Vec<crate::complex::Cx>, den: Vec<crate::complex::Cx> },
    /// APL `f∘g` (beside): monad `f (g y)`, dyad `x f (g y)`. g prepares the
    /// right argument and the left one arrives untouched, which is what
    /// separates it from `⍥` (this crate's [`Verb::Compose`]).
    Beside(Box<Verb>, Box<Verb>),
    /// APL `f⌺w` (Dyalog's stencil): f applied to the window of `w` cells
    /// centred on each cell of y in turn, the edges filled. One size per
    /// leading axis; the axes past them travel with the cell.
    Stencil(Box<Verb>, Vec<i64>),
    /// J `` m`:n `` for the two forms that are not a train: `0` applies
    /// every verb of the gerund to the arguments and frames the answers,
    /// `3` inserts the verbs between the items of y, cycling through them
    /// left to right and folding right to left. `` `:6 `` is a train and is
    /// built at parse time, so it never reaches here.
    Evoke(Vec<Verb>, i64),
    /// J `u . v` and APL `f.g`: the inner product, of which `+/ . *` and
    /// `+.×` are the matrix product. Dyadically each cell of x at v's
    /// dyadic LEFT rank — 1 where that rank is smaller — meets the whole
    /// of y under v, and u folds what comes back. Monadically, which is
    /// J's alone, it is the determinant by minors down the first column:
    /// `-/ . *` is the determinant proper.
    InnerProduct { u: Box<Verb>, v: Box<Verb>, apl: bool },
}

impl Verb {
    /// [monadic, dyadic-left, dyadic-right] ranks governing cell iteration.
    pub fn ranks(&self) -> [i64; 3] {
        match self {
            Verb::Prim(p) => p.ranks,
            Verb::Rank(_, r) => *r,
            // `x u\ y` and `x u\. y` take one width per application, so the
            // left cell is an atom: a list of widths frames the result, as
            // in J, and an empty list of them frames nothing.
            Verb::Windowed(_, WindowKind::Prefix | WindowKind::Suffix) => {
                [RANK_INF, 0, RANK_INF]
            }
            Verb::Each(..) => [0, 0, 0],
            Verb::Fit(v, _) => v.ranks(),
            // Amend reads the whole argument, and the rest run their own
            // verb over the argument as a whole.
            Verb::Amend(_)
            | Verb::AmendVerb(_)
            | Verb::ShiftFill(_)
            | Verb::Level { .. }
            | Verb::Characteristics(_)
            | Verb::UserDerived { .. }
            | Verb::KeyPairs(_)
            | Verb::Key(_)
            | Verb::Cut(..)
            | Verb::PowerV(..)
            | Verb::PowerUntil(..)
            | Verb::AlongAxis(..) => [RANK_INF, RANK_INF, RANK_INF],
            Verb::Memo(v, _) => v.ranks(),
            Verb::WithObverse(v, _) | Verb::Adverse(v, _) => v.ranks(),
            Verb::Beside(..) => [RANK_INF, RANK_INF, RANK_INF],
            // The series is summed for one value at a time.
            Verb::Hypergeometric { .. } => [0, 0, 0],
            // The determinant is over a table; the dyad reads both
            // arguments whole and takes their cells itself.
            Verb::InnerProduct { .. } => [2, RANK_INF, RANK_INF],
            _ => [RANK_INF, RANK_INF, RANK_INF],
        }
    }

    /// Name for diagnostics, e.g. `+/"1`.
    pub fn name(&self) -> String {
        match self {
            Verb::Prim(p) => p.name.to_string(),
            Verb::Rank(v, r) => format!("{}\"{}", v.name(), rank_str(*r)),
            Verb::Reduce(v) | Verb::NWise(v) => format!("{}/", v.name()),
            Verb::Windowed(v, WindowKind::Suffix) => format!("{}\\.", v.name()),
            Verb::Windowed(v, _) => format!("{}\\", v.name()),
            Verb::Commute(v) => format!("{}~", v.name()),
            Verb::PowerN(v, Power::Converge) => format!("{}^:_", v.name()),
            Verb::PowerN(v, Power::Times(n)) => format!("{}^:{n}", v.name()),
            Verb::PowerN(v, Power::Each(_)) => format!("{}^:n", v.name()),
            Verb::PowerN(v, Power::ConvergeTrace) => format!("{}^:a:", v.name()),
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
            Verb::AmendVerb(v) => format!("({}}})", v.name()),
            Verb::ShiftFill(_) => "|.!.n".to_string(),
            Verb::Characteristics(v) => format!("{} b.", v.name()),
            Verb::Before(f, g) => format!("({}⍛{})", f.name(), g.name()),
            Verb::KeyPairs(v) => format!("{}⌸", v.name()),
            Verb::UserDerived { def, alpha, omega } => match omega {
                Some(g) => format!("({} {} {})", alpha.name(), def.name(), g.name()),
                None => format!("({} {})", alpha.name(), def.name()),
            },
            Verb::Memo(v, _) => format!("{} M.", v.name()),
            Verb::Level { u, level, spread } => {
                format!("{} {} {level}", u.name(), if *spread { "S:" } else { "L:" })
            }
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
            Verb::Hypergeometric { num, den } => {
                format!("({} H. {})", cx_list(num), cx_list(den))
            }
            Verb::Agenda(vs, w) => {
                let names: Vec<String> = vs.iter().map(Verb::name).collect();
                format!("({}@.{})", names.join("`"), w.name())
            }
            Verb::Evoke(vs, n) => {
                let names: Vec<String> = vs.iter().map(Verb::name).collect();
                format!("({}`:{n})", names.join("`"))
            }
            Verb::Stencil(u, w) => {
                let sizes: Vec<String> = w.iter().map(i64::to_string).collect();
                format!("({}⌺{})", u.name(), sizes.join(" "))
            }
            Verb::InnerProduct { u, v, .. } => format!("({} . {})", u.name(), v.name()),
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
                        | MonadOp::GradeUp { .. }
                        | MonadOp::GradeDown { .. }
                        | MonadOp::EncodeBits
                ) || matches!(
                    p.dyad,
                    DyadOp::Scalar(
                        ScalarDyad::Eq
                            | ScalarDyad::Ne
                            | ScalarDyad::Lt
                            | ScalarDyad::Le
                            | ScalarDyad::Gt
                            | ScalarDyad::Ge
                            | ScalarDyad::Residue
                            | ScalarDyad::Gcd
                            | ScalarDyad::Lcm
                    ) | DyadOp::Match
                        | DyadOp::GradeSelect { .. }
                        | DyadOp::Encode
                        | DyadOp::EncodeApl
                        | DyadOp::NotMatch
                        | DyadOp::MemberJ
                        | DyadOp::MemberApl
                        | DyadOp::IndexOf { .. }
                        | DyadOp::IndexOfLast { .. }
                )
            }
            Verb::Rank(v, _)
            | Verb::Reduce(v)
            | Verb::NWise(v)
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
            Verb::Amend(_)
            | Verb::AmendVerb(_)
            | Verb::ShiftFill(_)
            | Verb::Characteristics(_)
            | Verb::Explicit(_)
            | Verb::SelfRef
            | Verb::Named(_)
            | Verb::Hypergeometric { .. } => false,
            Verb::Memo(v, _) | Verb::Level { u: v, .. } => v.uses_tolerance(),
            Verb::WithObverse(v, _) => v.uses_tolerance(),
            Verb::Adverse(v, w) | Verb::Beside(v, w) | Verb::Before(v, w) => {
                v.uses_tolerance() || w.uses_tolerance()
            }
            Verb::KeyPairs(v) => v.uses_tolerance(),
            Verb::UserDerived { def, alpha, omega } => {
                def.uses_tolerance()
                    || alpha.uses_tolerance()
                    || omega.as_ref().is_some_and(|g| g.uses_tolerance())
            }
            Verb::Agenda(vs, w) => {
                w.uses_tolerance() || vs.iter().any(Verb::uses_tolerance)
            }
            Verb::Evoke(vs, _) => vs.iter().any(Verb::uses_tolerance),
            Verb::Stencil(u, _) => u.uses_tolerance(),
            Verb::InnerProduct { u, v, .. } => u.uses_tolerance() || v.uses_tolerance(),
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
                !matches!(
                    p.monad,
                    MonadOp::Echo | MonadOp::Roll { .. } | MonadOp::ReadStream
                ) && !matches!(p.dyad, DyadOp::Deal { .. } | DyadOp::WriteStream)
            }
            Verb::Rank(v, _)
            | Verb::Reduce(v)
            | Verb::NWise(v)
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
            Verb::Hypergeometric { .. } => true,
            Verb::PowerV(v, w) | Verb::PowerUntil(v, w) => v.is_pure() && w.is_pure(),
            Verb::WithObverse(v, _) => v.is_pure(),
            Verb::Adverse(v, w) | Verb::Beside(v, w) | Verb::Before(v, w) => {
                v.is_pure() && w.is_pure()
            }
            Verb::KeyPairs(v) => v.is_pure(),
            // The body reads and writes the program's names, exactly as a
            // definition called any other way does.
            Verb::UserDerived { .. } => false,
            Verb::Agenda(vs, w) => w.is_pure() && vs.iter().all(Verb::is_pure),
            Verb::Evoke(vs, _) => vs.iter().all(Verb::is_pure),
            Verb::Stencil(u, _) => u.is_pure(),
            Verb::InnerProduct { u, v, .. } => u.is_pure() && v.is_pure(),
            Verb::Amend(_) | Verb::ShiftFill(_) | Verb::Characteristics(_) => true,
            Verb::AmendVerb(v) | Verb::Level { u: v, .. } => v.is_pure(),
            // A memo answers from its cache, so the verb inside it must be
            // pure for the cache to be an optimisation rather than a change
            // of meaning; running the cells in any order is then safe too.
            Verb::Memo(v, _) => v.is_pure(),
            // An explicit definition reads and writes the program's names,
            // so its cells can never be run out of order on other threads —
            // whatever its body does. `ExplicitDef::pure` records whether
            // the body itself has an effect; this is the stronger question.
            Verb::Explicit(_) | Verb::SelfRef | Verb::Named(_) => false,
        }
    }

    /// Whether this verb reads a sparse argument in its stored form.
    ///
    /// The set is small on purpose: `$.` itself, the two verbs that ask
    /// about an array rather than about its elements, and the three that
    /// draw it. Everything else is handed the dense expansion, which is the
    /// same value — the storage kind is not visible in the answer, only in
    /// how long it took to get there.
    fn monad_reads_sparse(&self) -> bool {
        let Verb::Prim(p) = self else { return false };
        matches!(
            p.monad,
            MonadOp::Sparse
                | MonadOp::ShapeOf
                | MonadOp::Tally
                | MonadOp::TypeCode
                | MonadOp::Format
                | MonadOp::Echo
        )
    }

    /// Whether this verb reads a sparse RIGHT argument in its stored form.
    /// A sparse left argument is always expanded: no dyad reads one.
    fn dyad_reads_sparse(&self) -> bool {
        matches!(self, Verb::Prim(p) if p.dyad == DyadOp::SparseForm)
    }

    /// Full monadic application including rank/frame machinery.
    ///
    /// This is one of the two places a column-major argument is dealt with:
    /// the verbs that read one natively get it as it lies, and every other
    /// verb gets the rows it assumes, materialised once here.
    pub fn monad(&self, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
        let _depth = Nesting::enter(span)?;
        // A verb that does not read the stored form gets the array every
        // position of it materialised, which is the same value.
        let dense;
        let y = if y.is_sparse() && !self.monad_reads_sparse() {
            dense = y.densified();
            &dense
        } else {
            y
        };
        if y.is_row_major() {
            return self.monad_rows(y, ctx, span);
        }
        match self.monad_columns(y, ctx, span) {
            Some(r) => r,
            None => self.monad_rows(&y.to_row_major(), ctx, span),
        }
    }

    /// Monadic application to an argument whose buffer is row-major, which
    /// is what everything below assumes.
    fn monad_rows(&self, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
        debug_assert!(y.is_row_major());
        match self {
            Verb::Prim(p) => {
                // Scalar verbs have cell rank 0: the cells are the elements,
                // so the whole buffer is one elementwise pass.
                if let MonadOp::Scalar(op) = p.monad {
                    return scalar_monad(op, y, ctx.cfg, span);
                }
                // A MIXED SIMPLE array is already simple, so opening it
                // changes nothing — and its cells could not be framed back
                // into one array if the rank machinery took them apart.
                if p.monad == MonadOp::Open && is_mixed_simple(y) {
                    return Ok(y.clone());
                }
                let frame_rank = y.rank() - effective_rank(p.ranks[0], y.rank());
                if frame_rank == 0 {
                    return monad_op(p, y, ctx, span);
                }
                let frame = y.shape[..frame_rank].to_vec();
                let n: usize = frame.iter().product();
                if n == 0 {
                    let cell = fill_cell(y, frame_rank, self.is_pure());
                    return Ok(empty_frame(&frame, y.dtype(), cell, ctx, |cell, c| {
                        monad_op(p, cell, c, span)
                    }));
                }
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
                // A reduction over vector cells is every row of the buffer
                // folded in place, without an array per cell.
                if let Some(a) = reduce_vector_cells(v, y, frame_rank) {
                    return Ok(a);
                }
                let frame = y.shape[..frame_rank].to_vec();
                let n: usize = frame.iter().product();
                if n == 0 {
                    let cell = fill_cell(y, frame_rank, self.is_pure());
                    return Ok(empty_frame(&frame, y.dtype(), cell, ctx, |cell, c| {
                        v.monad(cell, c, span)
                    }));
                }
                let cells = each_cell(n, y.count(), self.is_pure(), ctx, |i, c| {
                    v.monad(&y.cell_at(frame_rank, i), c, span)
                })?;
                assemble(&frame, cells, span)
            }
            Verb::Reduce(v) | Verb::NWise(v) => reduce(v, y, ctx, span),
            Verb::Windowed(v, kind) => {
                runs(v, y, *kind == WindowKind::Suffix, ctx, span)
            }
            Verb::Commute(v) => v.dyad(y, y, ctx, span),
            Verb::PowerN(v, p) => power(v, p.clone(), None, y, ctx, span),
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
                from_index(m, y, ctx.cfg.near(), span)
            }
            // `u} y` computes the indices first: it is `(u y)} y`.
            Verb::AmendVerb(u) => {
                let m = u.monad(y, ctx, span)?;
                Verb::Amend(m).monad(y, ctx, span)
            }
            // The monad shifts by one, the fill taking the place the
            // first item left: `|.!.f y` is `_1 |.!.f y`.
            Verb::ShiftFill(fill) => {
                shift_fill(&Array::scalar_i64(-1), y, fill, ctx.cfg.near(), span)
            }
            Verb::Memo(u, cache) => memoised(u, cache, None, y, ctx, span),
            Verb::Characteristics(u) => characteristics(u, y, span),
            Verb::Before(f, g) => {
                let l = f.monad(y, ctx, span)?;
                g.dyad(&l, y, ctx, span)
            }
            Verb::KeyPairs(u) => key_pairs(u, y, None, ctx, span),
            Verb::UserDerived { def, alpha, omega } => {
                with_operands(alpha, omega.as_deref(), ctx, |c| def.monad(y, c, span))
            }
            Verb::Level { u, level, spread } => {
                at_level(u, *level, *spread, y, ctx, span)
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
            Verb::Hypergeometric { num, den } => hypergeometric(num, den, y, span),
            Verb::Agenda(vs, w) => {
                agenda_pick(vs, w, None, y, ctx, span)?.monad(y, ctx, span)
            }
            Verb::Evoke(vs, n) => evoke(vs, *n, None, y, ctx, span),
            Verb::Stencil(u, w) => stencil(u, w, y, ctx, span),
            Verb::InnerProduct { u, v, apl } => determinant(u, v, *apl, y, ctx, span),
        }
    }

    /// Monadic application to a column-major argument, for the verbs that
    /// read one where it lies. None means this verb is not one of them and
    /// the caller must materialise the rows first.
    ///
    /// Every arm here either reads the buffer in an order it chooses (the
    /// folds), reads it elementwise (order cannot matter), or answers from
    /// the shape alone. Nothing else may be added without the same argument
    /// holding for it.
    fn monad_columns(&self, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Option<Result<Array>> {
        debug_assert!(!y.is_row_major());
        match self {
            Verb::Prim(p) => match p.monad {
                // Elementwise: every element is read and written where it
                // lies, so the answer carries the argument's own layout.
                MonadOp::Scalar(op) => Some(scalar_monad(op, y, ctx.cfg, span)),
                // The shape is the logical one whatever the buffer does.
                MonadOp::ShapeOf | MonadOp::Tally => Some(monad_op(p, y, ctx, span)),
                // Reversing the axes of a column-major buffer is reading the
                // same buffer as a row-major one of the reversed shape: the
                // transpose that costs nothing.
                MonadOp::TransposeAxes => Some(Ok(transpose_axes(y))),
                _ => None,
            },
            // `u/ y` folds the leading axis, and in this layout the leading
            // axis is what each contiguous run holds.
            Verb::Reduce(v) | Verb::NWise(v) => reduce_columns(v, y).map(Ok),
            // `u/"1 y` folds each row across the columns, which is one
            // elementwise pass per column and no transpose at all.
            Verb::Rank(v, r) => {
                if y.rank() != effective_rank(r[0], y.rank()) + 1 {
                    return None;
                }
                reduce_rows_columns(v, y).map(Ok)
            }
            _ => None,
        }
    }

    /// Full dyadic application including rank/frame/agreement machinery.
    ///
    /// The other place a column-major argument is dealt with: an
    /// elementwise verb over arguments that agree exactly reads the buffers
    /// as they lie and keeps the layout, and everything else is given rows.
    pub fn dyad(&self, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
        let _depth = Nesting::enter(span)?;
        // As in `monad`: only `x $. y` reads a sparse argument as it lies,
        // and even there the left one names a form and is always dense.
        let (dense_x, dense_y);
        let x = if x.is_sparse() {
            dense_x = x.densified();
            &dense_x
        } else {
            x
        };
        let y = if y.is_sparse() && !self.dyad_reads_sparse() {
            dense_y = y.densified();
            &dense_y
        } else {
            y
        };
        if x.is_row_major() && y.is_row_major() {
            return self.dyad_rows(x, y, ctx, span);
        }
        if let Some(layout) = self.elementwise_layout(x, y) {
            return Ok(self.dyad_rows(x, y, ctx, span)?.with_layout(layout));
        }
        self.dyad_rows(&x.to_row_major(), &y.to_row_major(), ctx, span)
    }

    /// The layout a dyadic result keeps when its arguments are not both
    /// row-major: an elementwise primitive over a scalar and an array, or
    /// over two arrays of one shape and one layout, computes each element
    /// from the elements at its own index and nothing else.
    fn elementwise_layout(&self, x: &Array, y: &Array) -> Option<Layout> {
        let Verb::Prim(p) = self else { return None };
        if !matches!(p.dyad, DyadOp::Scalar(_)) {
            return None;
        }
        if x.rank() == 0 {
            return Some(y.layout());
        }
        if y.rank() == 0 {
            return Some(x.layout());
        }
        (x.shape == y.shape && x.layout() == y.layout()).then(|| x.layout())
    }

    /// Dyadic application proper: reached with row-major arguments, or with
    /// arguments whose layout the verb above has established it is
    /// indifferent to.
    fn dyad_rows(&self, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
        match self {
            Verb::Prim(_) | Verb::Rank(_, _) | Verb::Each(..) => {
                self.dyad_ranked(x, y, ctx, span)
            }
            // `x u\ y` and `x u\. y` need the frame machinery: their left
            // cell is an atom.
            Verb::Windowed(_, WindowKind::Prefix | WindowKind::Suffix) => {
                self.dyad_ranked(x, y, ctx, span)
            }
            Verb::Windowed(_, WindowKind::Scan) => {
                Err(Error::not_yet("dyadic scan (x f\\ y)", span))
            }
            Verb::Commute(v) => v.dyad(y, x, ctx, span),
            Verb::PowerN(v, p) => power(v, p.clone(), Some(x), y, ctx, span),
            // `x u/ y` is the table: every cell of x against every cell of y.
            Verb::Reduce(v) => table(v, x, y, ctx, span),
            // `n f/ y` is APL's n-wise reduction, a different function.
            Verb::NWise(v) => nwise(v, x, y, ctx, span),
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
            Verb::Amend(m) => amend(m, x, y, ctx.cfg.near(), span),
            // `x u} y` is `x (x u y)} y`: u names the places to amend.
            Verb::AmendVerb(u) => {
                let m = u.dyad(x, y, ctx, span)?;
                amend(&m, x, y, ctx.cfg.near(), span)
            }
            Verb::ShiftFill(fill) => shift_fill(x, y, fill, ctx.cfg.near(), span),
            Verb::Memo(u, cache) => memoised(u, cache, Some(x), y, ctx, span),
            Verb::Characteristics(_) => {
                Err(Error::domain("u b. has no dyadic meaning", span))
            }
            Verb::Before(f, g) => {
                let l = f.monad(x, ctx, span)?;
                g.dyad(&l, y, ctx, span)
            }
            Verb::KeyPairs(u) => key_pairs(u, x, Some(y), ctx, span),
            Verb::UserDerived { def, alpha, omega } => {
                with_operands(alpha, omega.as_deref(), ctx, |c| def.dyad(x, y, c, span))
            }
            Verb::Level { u, level, spread } => {
                at_level_dyad(u, *level, *spread, x, y, ctx, span)
            }
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
            Verb::Hypergeometric { .. } => {
                Err(Error::domain("m H. n has no dyadic meaning", span))
            }
            Verb::Agenda(vs, w) => {
                agenda_pick(vs, w, Some(x), y, ctx, span)?.dyad(x, y, ctx, span)
            }
            Verb::Evoke(vs, n) => evoke(vs, *n, Some(x), y, ctx, span),
            Verb::InnerProduct { u, v, apl } => inner_product(u, v, *apl, x, y, ctx, span),
            Verb::Stencil(..) => {
                Err(Error::domain("f⌺w has no dyadic meaning", span))
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
        if p.n == 0 {
            let right = fill_cell(y, fyl, self.is_pure());
            let cell = fill_cell(x, fxl, self.is_pure()).filter(|_| right.is_some());
            return Ok(empty_frame(&p.frame, y.dtype(), cell, ctx, |left, c| {
                let right = right.as_ref().expect("a left fill cell comes with a right one");
                self.dyad_cell(left, right, c, span)
            }));
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
            // The one dyad that writes: it needs the sink, and the
            // dispatcher below it is the pure half of the evaluator.
            Verb::Prim(p) if p.dyad == DyadOp::WriteStream => {
                stream_number(y, 2, "1!:2 writes", span)?;
                (ctx.out)(&format!("{}\n", crate::fmt::format_array(x, &ctx.cfg.fmt)));
                Ok(x.clone())
            }
            Verb::Prim(p) => dyad_op(p, x, y, ctx.cfg, span),
            Verb::Rank(v, _) => v.dyad(x, y, ctx, span),
            // The infix takes runs of x items; the outfix leaves them out.
            Verb::Windowed(v, WindowKind::Suffix) => outfix(v, x, y, ctx, span),
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
        (Data::Symbol(a), Data::Symbol(b)) => a.push(b[i]),
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

/// The largest fill cell worth building to learn a shape from.
const FILL_CELL_LIMIT: usize = 1 << 20;

/// A cell to learn a shape from where the application had none to run.
///
/// An argument whose own frame is not empty still HAS cells — a dyad frames
/// over both arguments, and only one of them need be the empty one — so its
/// first cell stands in as it is, and only an argument with no cells at all
/// is stood in for by a cell of fills.
///
/// `None` where the verb is not pure — running it to learn a shape would be
/// running it for its effects — or where the cell is too large to be worth
/// building.
fn fill_cell(y: &Array, frame_rank: usize, pure: bool) -> Option<Array> {
    if !pure {
        return None;
    }
    if y.shape[..frame_rank].iter().all(|&d| d != 0) {
        return Some(y.cell_at(frame_rank, 0));
    }
    let shape = y.shape[frame_rank..].to_vec();
    let n: usize = shape.iter().product();
    if n > FILL_CELL_LIMIT {
        return None;
    }
    // A nested argument that remembers its items fills with the prototype,
    // so that mixing an empty keeps the axes its items had.
    let fill = y.proto().cloned();
    let mut data = Data::empty(y.dtype());
    for _ in 0..n {
        push_gap(&mut data, &fill);
    }
    Some(Array::new(shape, data))
}

/// The result of a cell-by-cell application that has no cells to frame.
///
/// The frame says how many cells there would have been, not what shape one
/// would have had, so an empty of the frame's shape alone drops whatever
/// axes the cells carried: `(,"1) i. 0 3` is a 0 by 3 table, not a list.
/// The missing axes come from running the verb once on a cell of fills and
/// keeping the shape of the answer, which is J's own rule. A verb that
/// refuses the fill cell, or a cell there was no point building, leaves the
/// frame standing on its own, holding the argument's type.
fn empty_frame(
    frame: &[usize],
    dtype: DType,
    cell: Option<Array>,
    ctx: &mut Ctx<'_>,
    run: impl FnOnce(&Array, &mut Ctx<'_>) -> Result<Array>,
) -> Array {
    let mut shape = frame.to_vec();
    if let Some(cell) = cell
        && let Ok(answer) = run(&cell, ctx)
    {
        shape.extend_from_slice(&answer.shape);
        return Array::new(shape, Data::empty(answer.dtype()));
    }
    Array::new(shape, Data::empty(dtype))
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
            // APL extends any frame of ONE cell, whatever its rank, not
            // only a scalar one: `(1 1⍴5)+1 2 3` is `6 7 8`. A rank-0 frame
            // — a true scalar — always gives way to the other side, and
            // between two one-cell frames that are not scalars the answer
            // keeps the RIGHT one: `(1 1⍴5)+,3` is a one-item VECTOR, while
            // `(1 1⍴5)+3` keeps the 1 by 1 table.
            let one = |f: &[usize]| f.iter().product::<usize>() == 1;
            if fx.is_empty() || (one(fx) && !fy.is_empty()) {
                let n: usize = fy.iter().product();
                return Ok(Pairing { frame: fy.to_vec(), n, x_div: n.max(1), y_div: 1 });
            }
            if fy.is_empty() || one(fy) {
                let n: usize = fx.iter().product();
                return Ok(Pairing { frame: fx.to_vec(), n, x_div: 1, y_div: n.max(1) });
            }
            let axis = (0..common).find(|&i| fx[i] != fy[i]).unwrap_or(common);
            Err(frame_mismatch(xs, ys, fx, fy, axis, span))
        }
    }
}

// ------------------------------------------------------------- assembly

/// Frame results that need not share a depth, which is how APL collects the
/// values of an application between items: `,\1 2 3` puts the simple scalar
/// `1` beside two enclosed vectors. A simple scalar cannot be nested, so it
/// is enclosed here to take its place among the others; anything already
/// alike goes straight to [`assemble`]. J refuses such a mixture instead,
/// and reaches [`assemble`] directly.
fn assemble_items(frame: &[usize], mut cells: Vec<Array>, span: Span) -> Result<Array> {
    let boxes = cells.iter().filter(|c| c.dtype() == DType::Box).count();
    if boxes > 0 && boxes < cells.len() {
        for c in &mut cells {
            if c.dtype() != DType::Box {
                *c = boxed_elements(c);
            }
        }
    }
    assemble(frame, cells, span)
}

/// The same array with every element held as its own value, so that it can
/// be framed beside cells whose elements are nested.
fn boxed_elements(a: &Array) -> Array {
    let row = a.to_row_major();
    let held: Vec<Array> = (0..row.count()).map(|i| atom(&row, i)).collect();
    Array::new(row.shape.clone(), Data::Box(held.into()))
}

/// Frame the results of a cell-by-cell application into one array.
///
/// The cells arrive as their verb left them, and a verb may leave a
/// column-major one — `|:` flips the layout flag rather than moving the
/// buffer. Framing splices the buffers end to end, so every cell is made
/// row-major first; an already row-major one costs a refcount bump.
fn assemble(frame: &[usize], cells: Vec<Array>, span: Span) -> Result<Array> {
    if cells.is_empty() {
        // Nothing to take a cell shape from. J runs the verb on a fill cell
        // to learn the shape; we yield an empty array of the frame's shape.
        return Ok(Array::new(frame.to_vec(), Data::empty(DType::I64)));
    }
    let cells: Vec<Array> =
        if cells.iter().all(Array::is_row_major) {
            cells
        } else {
            cells.iter().map(Array::to_row_major).collect()
        };
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
    debug_assert!(a.is_row_major(), "an atom out of a column-major buffer");
    Array::new(Vec::new(), a.data.slice(i, i + 1))
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
///
/// What comes out is row-major. A box is filled with a RESULT, and a result
/// carries whatever layout its verb left — `|:&.>` boxes column-major
/// matrices — while everything downstream of an open reads a value the way
/// a verb's argument is read.
fn open_cell(y: &Array) -> Array {
    match &y.data {
        Data::Box(v) if !v.is_empty() => v[0].to_row_major(),
        _ => y.clone(),
    }
}

/// `↑ y` (APL): the first element, disclosed. An empty argument has none,
/// so its fill stands in.
fn first(y: &Array) -> Array {
    if y.count() == 0 {
        // A nested empty that remembers its items answers with the
        // prototype: `↑0⍴⊂2 3⍴9` is the 2 by 3 table of zeros.
        if let Some(p) = y.proto() {
            return p.clone();
        }
        let mut d = Data::empty(y.dtype());
        d.push_fill();
        return open_cell(&Array::new(Vec::new(), d));
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

/// Whether every item of `y`, at every level, has its siblings' depth. A
/// simple array is uniform, and so is `1 2∘.⍴3 4`, whose items are of one
/// depth and different lengths; `1(2(3 4))` and `(1 2),⊂3 4` are not.
fn uniform(y: &Array) -> bool {
    let Data::Box(v) = &y.data else { return true };
    let Some(head) = v.first() else { return true };
    let d = depth(head);
    v.iter().all(|b| depth(b) == d && uniform(b))
}

/// Every leaf array inside `a`, in ravel order.
///
/// A leaf comes out row-major: a box may hold whatever layout the verb that
/// filled it left behind, and a caller that reads the ravel would otherwise
/// read a column-major buffer as rows.
fn leaves(a: &Array, out: &mut Vec<Array>) {
    let a = a.to_row_major();
    match &a.data {
        Data::Box(v) => {
            for b in v.iter() {
                leaves(b, out);
            }
        }
        _ => out.push(a),
    }
}

/// `∊ y` (APL): every leaf element as one vector. Leaves that share no one
/// type make a MIXED SIMPLE vector, as catenating them would.
fn enlist(y: &Array, _span: Span) -> Result<Array> {
    let mut parts = Vec::new();
    leaves(y, &mut parts);
    // An empty leaf contributes no elements, so it does not decide the
    // type either.
    let mut dt = None;
    let mut mixing = false;
    for p in parts.iter().filter(|p| p.count() > 0) {
        dt = Some(match dt {
            None => p.dtype(),
            Some(t) => match DType::promote(t, p.dtype()) {
                Some(t) => t,
                None => {
                    mixing = true;
                    break;
                }
            },
        });
    }
    if mixing {
        let mut cells: Vec<Array> = Vec::new();
        for p in &parts {
            let p = p.to_row_major();
            cells.extend((0..p.count()).map(|i| atom(&p, i)));
        }
        return Ok(Array::new(vec![cells.len()], Data::Box(cells.into())));
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

/// `a` with every element enclosed, where `other` is boxed and `a` is not.
/// The shape is kept, so only the depth changes.
fn nest_like(a: &Array, other: &Array) -> Array {
    if a.dtype() == DType::Box || other.dtype() != DType::Box {
        return a.clone();
    }
    let cells: Vec<Array> = (0..a.count()).map(|i| atom(a, i)).collect();
    Array::new(a.shape.clone(), Data::Box(cells.into()))
}

/// Every ELEMENT of `a` as a rank-0 box, keeping the shape: APL's mixed
/// simple form, which is how libjay holds a value whose elements share no
/// one type. Enclosing a simple scalar is no change at all in APL, so the
/// form says nothing the value did not already say. An already boxed array
/// is left alone.
fn spread_scalars(a: &Array) -> Array {
    if a.dtype() == DType::Box {
        return a.clone();
    }
    let a = a.to_row_major();
    let cells: Vec<Array> = (0..a.count()).map(|i| atom(&a, i)).collect();
    Array::new(a.shape.clone(), Data::Box(cells.into()))
}

/// True where every element of `a` is a simple scalar: the shape libjay
/// holds a mixed simple array in, whether or not the types still differ.
fn holds_scalar_boxes(a: &Array) -> bool {
    match a.as_boxes() {
        Some(items) => {
            !items.is_empty() && items.iter().all(|b| b.rank() == 0 && b.dtype() != DType::Box)
        }
        None => false,
    }
}

/// The way back out of [`spread_scalars`]: a boxed array whose every
/// element is a simple scalar and where one type covers them all is that
/// simple array, and in APL always was. Anything else is returned as it is.
///
/// This runs over every APL result, which is what keeps the form canonical:
/// `2↓1 2,'ab'` is the character vector `ab`, not two boxed characters.
fn tightened_mixed(a: Array) -> Array {
    let common = match a.as_boxes() {
        Some(items) if holds_scalar_boxes(&a) => {
            let mut t = items[0].dtype();
            let mut ok = true;
            for b in &items[1..] {
                match DType::promote(t, b.dtype()) {
                    Some(next) => t = next,
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            ok.then_some(t)
        }
        _ => None,
    };
    let Some(common) = common else { return a };
    let mut data = Data::empty(common);
    for b in a.as_boxes().expect("checked above") {
        match b.data.cast(common) {
            Some(widened) => push_elem(&mut data, &widened, 0),
            None => return a.clone(),
        }
    }
    Array::new(a.shape.clone(), data)
}

/// A pair put in one form, where one of them is held as boxed scalars and
/// the other is not: the simple one is spread into rank-0 boxes so the two
/// compare and join element for element. APL only — J's `<2` is a value of
/// its own and never the same as `2`.
fn align_mixed(x: &Array, y: &Array, apl: bool) -> (Array, Array) {
    if apl && holds_scalar_boxes(x) && y.dtype() != DType::Box {
        return (x.clone(), spread_scalars(y));
    }
    if apl && holds_scalar_boxes(y) && x.dtype() != DType::Box {
        return (spread_scalars(x), y.clone());
    }
    (x.clone(), y.clone())
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
    // A strand of one kind stays a plain array; one that mixes characters
    // with numbers becomes APL's MIXED SIMPLE array, which libjay keeps as
    // boxed scalars. Its depth is 1 and it displays without borders,
    // because a box holding a simple scalar is a scalar in APL.
    if item.dtype() != DType::Box
        && y.dtype() != DType::Box
        && DType::promote(item.dtype(), y.dtype()).is_some()
    {
        return catenate(&one(&item), y, true, false, span);
    }
    let head = if item.dtype() == DType::Box { item } else { Array::boxed(item) };
    catenate(&one(&head), &box_items(y), true, false, span)
}

// -------------------------------------------------- elementwise operations

fn char_arith(span: Span) -> Error {
    Error::new(ErrorKind::Type, "cannot do arithmetic on characters", Some(span))
}

fn symbol_arith(span: Span) -> Error {
    Error::new(
        ErrorKind::Type,
        "cannot do arithmetic on symbols; `5 s:` gives their names back",
        Some(span),
    )
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
        DType::Symbol => symbol_arith(span),
        _ => char_arith(span),
    }
}

/// Borrow numeric data as i64, widening a boolean buffer into `tmp`.
///
/// The widening is a pass over the whole buffer, so it takes the thread
/// pool on the sizes that are worth splitting; the values are the same
/// whichever way it runs.
fn borrow_i64<'a>(d: &'a Data, tmp: &'a mut Vec<i64>) -> &'a [i64] {
    match d {
        Data::I64(v) => v,
        Data::Bool(v) => {
            *tmp = par::map(v, |&b| b as i64);
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
            *tmp = par::map(v, |&x| x as f64);
            &tmp[..]
        }
        Data::Bool(v) => {
            *tmp = par::map(v, |&x| x as f64);
            &tmp[..]
        }
        Data::Ext(v) => {
            *tmp = par::map(v, exact::ext_to_f64);
            &tmp[..]
        }
        Data::Rat(v) => {
            *tmp = par::map(v, Rat::to_f64);
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
            *tmp = par::map(v, |x| [exact::ext_to_f64(x), 0.0]);
            &tmp[..]
        }
        Data::Rat(v) => {
            *tmp = par::map(v, |x| [x.to_f64(), 0.0]);
            &tmp[..]
        }
        Data::F64(v) => {
            *tmp = par::map(v, |&x| [x, 0.0]);
            &tmp[..]
        }
        Data::I64(v) => {
            *tmp = par::map(v, |&x| [x as f64, 0.0]);
            &tmp[..]
        }
        Data::Bool(v) => {
            *tmp = v.iter().map(|&x| [x as f64, 0.0]).collect();
            &tmp[..]
        }
        _ => &[],
    }
}

/// One element of a narrow buffer, read as the type a pass computes in.
///
/// This is what lets a pass over operands of two different types run
/// without a widened copy of either: the promotion happens where the
/// element is read, inside the chunk, so the only buffer the pass touches
/// besides its arguments is its own result. Promotion and then the
/// operation is exactly what the widened copy would have fed it, so the
/// answers are identical either way.
pub(crate) trait Widen<T>: Copy + Send + Sync {
    fn widen(self) -> T;
}

macro_rules! widens {
    ($($from:ty => $to:ty : |$v:ident| $e:expr;)*) => {
        $(impl Widen<$to> for $from {
            #[inline(always)]
            fn widen(self) -> $to {
                let $v = self;
                $e
            }
        })*
    };
}

widens! {
    u8 => i64: |v| v as i64;
    i64 => i64: |v| v;
    u8 => f64: |v| v as f64;
    i64 => f64: |v| v as f64;
    f64 => f64: |v| v;
    u8 => Cx: |v| [v as f64, 0.0];
    i64 => Cx: |v| [v as f64, 0.0];
    f64 => Cx: |v| [v, 0.0];
    Cx => Cx: |v| v;
}

/// Bind `$s` to the buffer behind one numeric operand of an integer pass,
/// in the buffer's own element type, and evaluate `$body` with it.
macro_rules! i64_source {
    ($d:expr, $tmp:ident, $s:ident, $body:expr) => {
        match $d {
            Data::I64(v) => {
                let $s: &[i64] = v;
                $body
            }
            Data::Bool(v) => {
                let $s: &[u8] = v;
                $body
            }
            other => {
                let $s: &[i64] = borrow_i64(other, &mut $tmp);
                $body
            }
        }
    };
}

/// The same for a float pass. The exact types have no fixed-width buffer to
/// read element by element, so they keep the widened copy.
macro_rules! f64_source {
    ($d:expr, $tmp:ident, $s:ident, $body:expr) => {
        match $d {
            Data::F64(v) => {
                let $s: &[f64] = v;
                $body
            }
            Data::I64(v) => {
                let $s: &[i64] = v;
                $body
            }
            Data::Bool(v) => {
                let $s: &[u8] = v;
                $body
            }
            other => {
                let $s: &[f64] = borrow_f64(other, &mut $tmp);
                $body
            }
        }
    };
}

/// The same for a complex pass.
macro_rules! cx_source {
    ($d:expr, $tmp:ident, $s:ident, $body:expr) => {
        match $d {
            Data::Complex(v) => {
                let $s: &[Cx] = v;
                $body
            }
            Data::F64(v) => {
                let $s: &[f64] = v;
                $body
            }
            Data::I64(v) => {
                let $s: &[i64] = v;
                $body
            }
            Data::Bool(v) => {
                let $s: &[u8] = v;
                $body
            }
            other => {
                let $s: &[Cx] = borrow_cx(other, &mut $tmp);
                $body
            }
        }
    };
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
    if a == DType::Symbol || b == DType::Symbol {
        return Err(symbol_arith(span));
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
///
/// The two sides carry their own element types, so a pass over operands of
/// different widths reads each buffer as it lies and promotes inside `f`.
#[allow(clippy::too_many_arguments)]
#[inline]
fn zip_chunk<A, B, U, F>(
    xs: &[A],
    xoff: usize,
    xdiv: usize,
    ys: &[B],
    yoff: usize,
    ydiv: usize,
    start: usize,
    out: &mut [U],
    mut f: F,
) -> bool
where
    A: Copy,
    B: Copy,
    F: FnMut(A, B, &mut U) -> bool,
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

/// `! y` under the dialect's rule for an argument the gamma function cannot
/// reach at all. jconsole answers `_` wherever its own gamma overflows —
/// `! _`, `! 1e308` and `! _1e20` are each `_` — and refuses `! __` alone,
/// which is the NaN this leaves standing. The APL caller refuses every
/// non-finite answer and so needs no rule of its own.
fn factorial_as(y: f64, tol: Tol) -> f64 {
    let r = factorial(y);
    if tol.is_j() && r.is_nan() && !y.is_nan() && y != f64::NEG_INFINITY {
        return f64::INFINITY;
    }
    r
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

/// `x ! y` where an operand is infinite, which the gamma quotient reaches
/// only as a NaN. jconsole answers most of these and refuses the rest, and
/// the table below is what nineteen probes of it say, entry by entry: an
/// infinite LEFT argument gives 0 unless the right one sits on a pole of
/// the gamma function; an infinite RIGHT one is read off the left's sign;
/// and of the four infinite pairs only `__ ! _` has a value. None is a NaN
/// the caller then refuses.
fn binomial_at_infinity(x: f64, y: f64) -> Option<f64> {
    if x.is_infinite() && y.is_infinite() {
        return (x < 0.0 && y > 0.0).then_some(0.0);
    }
    if x.is_infinite() {
        // `_ ! _1` and `_ ! _2` have none; `_ ! _2.5` is 0, because only a
        // whole negative right argument is a pole.
        return (!(y < 0.0 && y.fract() == 0.0)).then_some(0.0);
    }
    if x > 0.0 {
        Some(f64::INFINITY)
    } else if x == 0.0 {
        Some(1.0)
    } else {
        Some(0.0)
    }
}

/// `x ! y` on the reals.
fn binomial(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        // A NaN the program wrote travels: `_ ! _.` is `_.`, not a value
        // read off the table below.
        return f64::NAN;
    }
    if x.is_infinite() || y.is_infinite() {
        return binomial_at_infinity(x, y).unwrap_or(f64::NAN);
    }
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
fn f64_op(op: ScalarDyad, a: f64, b: f64, tol: Tol, span: Span) -> Result<f64> {
    use ScalarDyad::*;
    let r = match op {
        Add => a + b,
        Sub => a - b,
        Mul => tol.mul(a, b),
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
            } else if a == 0.0 && b < 0.0 && !tol.is_j() {
                // GNU APL refuses `0⋆¯1`: it is a division by zero under
                // another name, and its `÷0` is refused too. J answers the
                // infinity, as its `% 0` does.
                return Err(Error::domain("zero has no negative power", span));
            } else if a < 0.0 && b.is_infinite() {
                // A negative base under an infinite exponent alternates in
                // sign for ever. jconsole answers only where the magnitude
                // falls to zero and the sign stops mattering — `_2 ^ __` is
                // 0 — and refuses the rest, `_1 ^ _` and `_2 ^ _` alike.
                if a.abs() != 1.0 && (a.abs() > 1.0) == (b < 0.0) {
                    0.0
                } else {
                    return Err(Error::domain(
                        "a negative base has no infinite power: the sign alternates",
                        span,
                    ));
                }
            } else {
                a.powf(b)
            }
        }
        Residue => tol.residue(a, b),
        Log => {
            if a < 0.0 || b < 0.0 {
                return Err(Error::not_yet("complex numbers", span));
            }
            let r = b.ln() / a.ln();
            // GNU APL has no infinite logarithm: `1⍟2`, `2⍟0` and `1⍟0` are
            // all DOMAIN ERROR. The two it does define where the ratio is a
            // NaN — `0⍟0` and `1⍟1` — are 1, each of them a base raised to
            // the first power. J keeps the infinity (`1 ^. 2` is `_`) and
            // refuses only the NaN, which the check below the match does.
            if !tol.is_j() && !r.is_finite() {
                if r.is_nan() {
                    return Ok(1.0);
                }
                return Err(Error::domain("this logarithm has no value", span));
            }
            r
        }
        Root => {
            if b < 0.0 {
                return Err(Error::not_yet("complex numbers", span));
            }
            b.powf(1.0 / a)
        }
        // `?`, not `return`: `1 o. _` is a NaN the arithmetic made, and
        // jconsole refuses it (as a limit error) rather than answering.
        Circle => {
            let r = circle(a, b, span)?;
            // GNU APL refuses a circle function with no value where J
            // continues it: `¯7○1` is artanh at its pole, an infinity in J
            // and a DOMAIN ERROR there.
            if !tol.is_j() && !r.is_finite() && a.is_finite() && b.is_finite() {
                return Err(Error::domain("this circle function has no value", span));
            }
            r
        }
        Binomial => binomial(a, b),
        _ => return Err(Error::internal("non-arithmetic op in the float path")),
    };
    if tol.made_nan(r, a, b) {
        return Err(nan_error(op, a, b, span));
    }
    Ok(r)
}

/// The diagnostic for arithmetic with no value, naming the pair that has
/// none: "NaN error: `_ - _` has no value".
#[cold]
fn nan_error(op: ScalarDyad, a: f64, b: f64, span: Span) -> Error {
    Error::nan(
        format!(
            "`{} {} {}` has no value",
            j_number(a),
            crate::fuse::dyad_name(op),
            j_number(b)
        ),
        span,
    )
}

/// Which of a real pair's operations has no real answer, so the whole pass
/// runs in the complex domain instead. Only the four operations that can
/// leave the reals are asked.
#[inline]
fn escapes_reals(op: ScalarDyad, a: f64, b: f64) -> bool {
    use ScalarDyad::*;
    match op {
        // An integer exponent keeps a negative base real (`_1 ^ 2` is 1).
        // An INFINITE one is neither integer nor fractional: `fract` is a
        // NaN there, and the pair belongs to the real path, which answers
        // `_2 ^ __` with 0 and refuses the rest.
        Pow => a < 0.0 && b.is_finite() && b.fract() != 0.0,
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
fn dyad_cx_chunk_body<A: Widen<Cx>, B: Widen<Cx>>(
    op: ScalarDyad,
    xs: &[A],
    xoff: usize,
    xdiv: usize,
    ys: &[B],
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
                *slot = $step(a.widen(), b.widen());
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
        match cx_op(op, a.widen(), b.widen(), span) {
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
    /// One chunk of a complex pass, compiled per CPU feature level. Either
    /// operand may be narrower than complex, and is promoted as it is read.
    #[allow(clippy::too_many_arguments)]
    fn dyad_cx_chunk[A: Widen<Cx>, B: Widen<Cx>](
        op: ScalarDyad,
        xs: &[A],
        xoff: usize,
        xdiv: usize,
        ys: &[B],
        yoff: usize,
        ydiv: usize,
        start: usize,
        out: &mut [Cx],
        span: Span,
    ) -> Result<()> = dyad_cx_chunk_body;
}

#[allow(clippy::too_many_arguments)]
fn dyad_cx<A: Widen<Cx>, B: Widen<Cx>>(
    op: ScalarDyad,
    xs: &[A],
    xoff: usize,
    xdiv: usize,
    ys: &[B],
    yoff: usize,
    ydiv: usize,
    n: usize,
    span: Span,
) -> Result<Vec<Cx>> {
    par::try_fill(n, |start, part| {
        dyad_cx_chunk(op, xs, xoff, xdiv, ys, yoff, ydiv, start, part, span)
    })
}

/// One complex pass over two buffers.
///
/// An operand that is not complex already is read in its own type and
/// promoted element by element, so the pass allocates nothing but its
/// result. Only the exact types, which have no fixed-width buffer, are
/// widened into one first — and a pass with no complex operand at all (`j.`
/// of two reals, a power that leaves the reals) with them, since promoting
/// two whole buffers is what such a pass is for.
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
    macro_rules! pass {
        ($xs:expr, $ys:expr) => {
            Data::Complex(dyad_cx(op, $xs, xoff, xdiv, $ys, yoff, ydiv, n, span)?.into())
        };
    }
    Ok(match (x, y) {
        (Data::Complex(a), _) => {
            let xs: &[Cx] = a;
            cx_source!(y, ty, ys, pass!(xs, ys))
        }
        (_, Data::Complex(b)) => {
            let ys: &[Cx] = b;
            cx_source!(x, tx, xs, pass!(xs, ys))
        }
        _ => pass!(borrow_cx(x, &mut tx), borrow_cx(y, &mut ty)),
    })
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
fn dyad_i64_chunk_body<A: Widen<i64>, B: Widen<i64>>(
    op: ScalarDyad,
    xs: &[A],
    xoff: usize,
    xdiv: usize,
    ys: &[B],
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
                let (v, o) = i64::$m(a.widen(), b.widen());
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
                *slot = $step(a.widen(), b.widen());
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
            match i64_op(op, a.widen(), b.widen()) {
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
    fn dyad_i64_chunk[A: Widen<i64>, B: Widen<i64>](
        op: ScalarDyad,
        xs: &[A],
        xoff: usize,
        xdiv: usize,
        ys: &[B],
        yoff: usize,
        ydiv: usize,
        start: usize,
        out: &mut [i64],
    ) -> bool = dyad_i64_chunk_body;
}

/// One elementwise integer pass. None means it left i64 anywhere.
#[allow(clippy::too_many_arguments)]
fn dyad_i64<A: Widen<i64>, B: Widen<i64>>(
    op: ScalarDyad,
    xs: &[A],
    xoff: usize,
    xdiv: usize,
    ys: &[B],
    yoff: usize,
    ydiv: usize,
    n: usize,
) -> Option<Vec<i64>> {
    let (out, ok) = par::fill(n, |start, part| {
        dyad_i64_chunk(op, xs, xoff, xdiv, ys, yoff, ydiv, start, part)
    });
    ok.then_some(out)
}

/// One elementwise integer pass over two buffers, each read in its own
/// element type. None means it left i64 anywhere.
#[allow(clippy::too_many_arguments)]
fn int_dyad_data(
    op: ScalarDyad,
    x: &Data,
    xoff: usize,
    xdiv: usize,
    y: &Data,
    yoff: usize,
    ydiv: usize,
    n: usize,
) -> Option<Data> {
    let (mut tx, mut ty) = (Vec::new(), Vec::new());
    let out = i64_source!(x, tx, xs, {
        i64_source!(y, ty, ys, dyad_i64(op, xs, xoff, xdiv, ys, yoff, ydiv, n))
    })?;
    Some(Data::I64(out.into()))
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn dyad_f64_chunk_body<A: Widen<f64>, B: Widen<f64>>(
    op: ScalarDyad,
    xs: &[A],
    xoff: usize,
    xdiv: usize,
    ys: &[B],
    yoff: usize,
    ydiv: usize,
    start: usize,
    out: &mut [f64],
    tol: Tol,
    span: Span,
) -> Result<()> {
    use ScalarDyad::*;
    // The arithmetic that cannot fail is picked before the loop, so the
    // compiler sees one operation per pass instead of a match per element.
    macro_rules! plain {
        ($step:expr) => {{
            zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, out, |a, b, slot: &mut f64| {
                *slot = $step(a.widen(), b.widen());
                true
            });
            return Ok(());
        }};
    }
    // The arithmetic that cannot fail runs in the plain loop; under J's
    // rules a NaN in what it wrote means the pass has to be redone one pair
    // at a time, because only there are both operands in hand to tell a NaN
    // the arithmetic MADE from one the program wrote. The scan itself
    // vectorises and finds nothing on ordinary data, so the fast path keeps
    // its speed and the slow one keeps the rule.
    macro_rules! plain_checked {
        ($step:expr) => {{
            zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, out, |a, b, slot: &mut f64| {
                *slot = $step(a.widen(), b.widen());
                true
            });
            if !(tol.is_j() && out.iter().any(|v| v.is_nan())) {
                return Ok(());
            }
        }};
    }
    match op {
        Add => plain_checked!(|a: f64, b: f64| a + b),
        Sub => plain_checked!(|a: f64, b: f64| a - b),
        Mul => plain_checked!(|a: f64, b: f64| a * b),
        Min => plain!(f64::min),
        Max => plain!(f64::max),
        _ => {}
    }
    let mut err = None;
    zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, out, |a, b, slot: &mut f64| {
        match f64_op(op, a.widen(), b.widen(), tol, span) {
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
    /// One chunk of a float pass, compiled per CPU feature level. Either
    /// operand may be an integer or a boolean buffer, promoted as it is read.
    #[allow(clippy::too_many_arguments)]
    fn dyad_f64_chunk[A: Widen<f64>, B: Widen<f64>](
        op: ScalarDyad,
        xs: &[A],
        xoff: usize,
        xdiv: usize,
        ys: &[B],
        yoff: usize,
        ydiv: usize,
        start: usize,
        out: &mut [f64],
        tol: Tol,
        span: Span,
    ) -> Result<()> = dyad_f64_chunk_body;
}

#[allow(clippy::too_many_arguments)]
fn dyad_f64<A: Widen<f64>, B: Widen<f64>>(
    op: ScalarDyad,
    xs: &[A],
    xoff: usize,
    xdiv: usize,
    ys: &[B],
    yoff: usize,
    ydiv: usize,
    n: usize,
    tol: Tol,
    span: Span,
) -> Result<Vec<f64>> {
    par::try_fill(n, |start, part| {
        dyad_f64_chunk(op, xs, xoff, xdiv, ys, yoff, ydiv, start, part, tol, span)
    })
}

/// One float pass over two buffers, each read in its own element type.
#[allow(clippy::too_many_arguments)]
fn float_dyad_data(
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
    let (mut tx, mut ty) = (Vec::new(), Vec::new());
    let out = f64_source!(x, tx, xs, {
        f64_source!(y, ty, ys, dyad_f64(op, xs, xoff, xdiv, ys, yoff, ydiv, n, tol, span)?)
    });
    Ok(Data::F64(out.into()))
}

/// Whether two element types have nothing in common to compare: a
/// character against a number, or a box against either. Two numeric types
/// always meet somewhere, however far apart the widths are.
fn crossed_types(a: DType, b: DType) -> bool {
    let class = |d: DType| match d {
        DType::Box => 3,
        DType::Symbol => 2,
        DType::Char => 1,
        _ => 0,
    };
    class(a) != class(b)
}

/// `x <. y` and `x >. y` over symbols: the smaller or larger NAME of the
/// pair, which is the only arithmetic a symbol has.
#[allow(clippy::too_many_arguments)]
fn symbol_min_max(
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
    let (Data::Symbol(a), Data::Symbol(b)) = (x, y) else {
        return Err(symbol_arith(span));
    };
    let down = op == ScalarDyad::Min;
    let (out, _) = par::fill(n, |start, part: &mut [crate::symbol::Id]| {
        zip_chunk(a, xoff, xdiv, b, yoff, ydiv, start, part, |p, q, slot| {
            *slot = if crate::symbol::cmp(p, q).is_le() == down { p } else { q };
            true
        })
    });
    Ok(Data::Symbol(out.into()))
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
    // Equality is TOTAL across a character and a number in both
    // references: `'a' = 1` is 0. It is total across the BOX boundary in J
    // too — `(<1) = 1` is 0 — but not in APL, where a scalar verb reaches
    // inside the box instead, so that case falls through to the diagnostic
    // below rather than answering 0.
    let boxed = dx == DType::Box || dy == DType::Box;
    if equality && crossed_types(dx, dy) && (!boxed || tol.is_j()) {
        let unequal = op == Ne;
        return Ok(Data::Bool(vec![u8::from(unequal); n].into()));
    }
    if boxed {
        // Boxes have no order — J refuses `<` on them — but they do have
        // equality, which compares their contents.
        if !equality {
            return Err(box_arith(span));
        }
        let (Data::Box(a), Data::Box(b)) = (x, y) else {
            // Only APL reaches here: its scalar verbs pervade into a
            // nested argument, which is a promise rather than a refusal.
            return Err(Error::not_yet("a scalar function inside a nested array", span));
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
    if dx == DType::Symbol || dy == DType::Symbol {
        // Equality across the boundary answered above; anything else here
        // is an ordering that has nothing to order against.
        if dx != dy {
            return Err(Error::new(
                ErrorKind::Type,
                "cannot compare a symbol with data that is not a symbol",
                Some(span),
            ));
        }
        let (Data::Symbol(a), Data::Symbol(b)) = (x, y) else {
            return Err(Error::internal("symbol comparison on non-symbol data"));
        };
        // Ordering reads the names; equality is index against index.
        let (out, _) = par::fill(n, |start, part: &mut [u8]| {
            zip_chunk(a, xoff, xdiv, b, yoff, ydiv, start, part, |p, q, slot| {
                *slot = u8::from(match op {
                    Eq => p == q,
                    Ne => p != q,
                    _ => {
                        let o = crate::symbol::cmp(p, q);
                        match op {
                            Lt => o.is_lt(),
                            Le => o.is_le(),
                            Gt => o.is_gt(),
                            _ => o.is_ge(),
                        }
                    }
                });
                true
            })
        });
        return Ok(Data::Bool(out.into()));
    }
    if dx == DType::Char || dy == DType::Char {
        if dx != dy {
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
    if DType::promote(dx, dy).is_some_and(DType::is_exact)
        && let Some(d) = exact_compare_data(op, x, xoff, xdiv, y, yoff, ydiv, n)
    {
        return Ok(d);
    }
    if dx == DType::Complex || dy == DType::Complex {
        if !equality {
            return Err(no_complex_order(span));
        }
        let (mut tx, mut ty) = (Vec::new(), Vec::new());
        let out = cx_source!(x, tx, xs, {
            cx_source!(y, ty, ys, {
                par::fill(n, |start, part: &mut [u8]| {
                    zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, part, |a, b, slot| {
                        let e = tol.eq_cx(a.widen(), b.widen());
                        *slot = if op == Eq { e as u8 } else { !e as u8 };
                        true
                    })
                })
                .0
            })
        });
        return Ok(Data::Bool(out.into()));
    }
    // Floats compare with the dialect's tolerance; integers are exact
    // whatever it is, so the integer pass below is untouched by it.
    let out = if DType::promote(dx, dy) == Some(DType::F64) {
        let (mut tx, mut ty) = (Vec::<f64>::new(), Vec::<f64>::new());
        f64_source!(x, tx, xs, {
            f64_source!(y, ty, ys, {
                par::fill(n, |start, part: &mut [u8]| {
                    zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, part, |a, b, slot| {
                        *slot = tol_cmp(op, a.widen(), b.widen(), tol) as u8;
                        true
                    })
                })
                .0
            })
        })
    } else {
        let (mut tx, mut ty) = (Vec::<i64>::new(), Vec::<i64>::new());
        i64_source!(x, tx, xs, {
            i64_source!(y, ty, ys, {
                par::fill(n, |start, part: &mut [u8]| {
                    zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, start, part, |a, b, slot| {
                        let (a, b): (i64, i64) = (a.widen(), b.widen());
                        *slot = cmp_result(op, Some(i64::cmp(&a, &b))) as u8;
                        true
                    })
                })
                .0
            })
        })
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

/// Two floats ordered under a tolerance: values that are tolerantly equal
/// tie, which is what leaves them in their original order in a stable sort.
/// A NaN ties with everything, which keeps the sort total.
#[inline]
pub(crate) fn tol_ord(a: f64, b: f64, tol: Tol) -> std::cmp::Ordering {
    use std::cmp::Ordering::Equal;
    if tol.ct != 0.0 && tol.eq(a, b) {
        return Equal;
    }
    a.partial_cmp(&b).unwrap_or(Equal)
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
///
/// GNU APL parts company here when one side is zero: `¯3∨0` and `0∨¯3` are
/// both `¯3` there, the other argument returned unchanged with its sign,
/// where J answers `3`. [`signed_gcd_i128`] is the APL reading.
fn gcd_i128(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// GNU APL's GCD: the magnitude, except that a zero argument hands back
/// the other one untouched, sign and all. Only whole numbers keep the sign
/// — `¯3.5∨0` is `3.5` in GNU, so the real path below stays nonnegative.
fn signed_gcd_i128(a: i128, b: i128) -> i128 {
    match (a, b) {
        (0, _) => b,
        (_, 0) => a,
        _ => gcd_i128(a, b),
    }
}

/// A finite float as `p / 10^s`, read off the shortest decimal that prints
/// back as this value — which is the number the user wrote and the number
/// both references show.
///
/// A value needing more than [`WRITTEN_DIGITS`] significant digits is not a
/// number anyone wrote: it is the residue of an arithmetic that missed, and
/// reading it as a decimal turns a rounding error into a divisor.
fn decimal_parts(v: f64) -> Option<(i128, u32)> {
    if !v.is_finite() {
        return None;
    }
    let text = format!("{v:e}");
    let (mantissa, exponent) = text.split_once('e')?;
    let exponent: i32 = exponent.parse().ok()?;
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.trim_start_matches('-').len() + fraction.len() > WRITTEN_DIGITS {
        return None;
    }
    let mut digits: i128 = format!("{whole}{fraction}").parse().ok()?;
    let mut scale = fraction.len() as i32 - exponent;
    // A negative scale is a whole number with trailing zeros; fold them in
    // so every value arrives as `p / 10^s` with s at least zero.
    while scale < 0 {
        digits = digits.checked_mul(10)?;
        scale += 1;
    }
    // Beyond this the products below leave i128, and the Euclid fallback
    // takes over.
    (scale <= 34).then_some((digits, scale as u32))
}

/// How many significant digits a decimal the user typed may need. Twelve
/// leaves every written constant intact and rejects the rounding residues:
/// `0.1+0.2` prints back as seventeen digits, `1.0000000000001` as fourteen.
const WRITTEN_DIGITS: usize = 12;

/// The GCD of two reals read as the decimals they are printed as: `1.23`
/// and `4.56` are 123 and 456 hundredths, so their GCD is three hundredths.
/// That is the value both references print — theirs is the Euclid grind
/// that rounds to it, and a binary Euclid of our own cannot reach either.
fn gcd_decimal(a: f64, b: f64) -> Option<f64> {
    let (pa, sa) = decimal_parts(a)?;
    let (pb, sb) = decimal_parts(b)?;
    let scale = sa.max(sb);
    let lift = |p: i128, s: u32| 10i128.checked_pow(scale - s).and_then(|k| p.checked_mul(k));
    let g = gcd_i128(lift(pa, sa)?, lift(pb, sb)?);
    // Dividing through a decimal string keeps the one rounding the value
    // itself carries, where a multiply by 10^s of its own would add another.
    format!("{g}e-{scale}").parse().ok()
}

/// The real GCD, by Euclid on the values themselves. Floats cannot reach an
/// exact zero remainder, so a remainder is taken to be zero once it is
/// within the comparison tolerance of the LARGER argument — the scale the
/// whole division sequence was measured against — or of the divisor, which
/// is the same step seen from the other end. That is what makes
/// `0.1 +. 0.2` answer `0.1` and `0.3 +. 0.1+0.2` answer `0.3` rather than
/// grinding down to a rounding error.
fn gcd_f64(a: f64, b: f64, tol: Tol) -> Option<f64> {
    let (mut a, mut b) = (a.abs(), b.abs());
    if !a.is_finite() || !b.is_finite() {
        return None;
    }
    let eps = tol.ct * a.max(b);
    // Euclid on reals converges as fast as it does on integers; the bound
    // is a guard, not the usual exit.
    for _ in 0..1000 {
        if b == 0.0 {
            return Some(a);
        }
        if a == 0.0 {
            return Some(b);
        }
        // The quotient's floor is TOLERANT, as J's `<.` is: a quotient a
        // rounding error below an integer is that integer, and the step
        // then lands on a remainder of zero instead of on the divisor. What
        // is left can only fall just outside [0, b), so it is clamped.
        let q = a / b;
        let mut k = q.floor();
        if tol.eq(q, k + 1.0) {
            k += 1.0;
        }
        let mut r = a - b * k;
        if r <= eps || tol.eq(r, b) {
            r = 0.0;
        }
        a = b;
        b = r;
    }
    Some(a)
}

/// The real LCM/GCD pass: Euclid on the values, which is what J answers for
/// a pair that is not whole. An infinite operand has no answer, and both
/// references refuse it.
#[allow(clippy::too_many_arguments)]
fn real_lcm_gcd(
    op: ScalarDyad,
    xs: &[f64],
    xoff: usize,
    xdiv: usize,
    ys: &[f64],
    yoff: usize,
    ydiv: usize,
    n: usize,
    tol: Tol,
    gnu: bool,
    span: Span,
) -> Result<Data> {
    let mut out = vec![0.0f64; n];
    let mut ok = true;
    // GNU APL reads an operand within `⎕CT` of a whole number as that
    // number before anything else: `1.0000000000001∧5` is 5 there, not the
    // 5e13 the unrounded value grinds out. J does no such thing —
    // `1.0000000000001 +. 1` is `9.99e_14` in jconsole.
    let whole = |v: f64| {
        let w = v.round();
        if gnu && tol.eq(v, w) { w } else { v }
    };
    // And an operand no larger than `⎕CT` beside the other one is zero,
    // which leaves the other one: `1E¯13∨1` is 1 in GNU, not `1E¯13`.
    let vanishes = |v: f64, other: f64| gnu && v != 0.0 && v.abs() <= tol.ct * other.abs();
    zip_chunk(xs, xoff, xdiv, ys, yoff, ydiv, 0, &mut out, |a, b, slot| {
        let (a, b) = (whole(a), whole(b));
        let (a, b) = (if vanishes(a, b) { 0.0 } else { a }, if vanishes(b, a) { 0.0 } else { b });
        let Some(g) = gcd_decimal(a, b).or_else(|| gcd_f64(a, b, tol)) else {
            ok = false;
            return false;
        };
        *slot = if op == ScalarDyad::Gcd {
            g
        } else if g == 0.0 {
            0.0
        } else {
            a / g * b
        };
        true
    });
    if !ok {
        return Err(Error::domain("LCM/GCD needs finite values", span));
    }
    Ok(Data::F64(out.into()))
}

/// LCM/GCD over two buffers. Two booleans stay boolean, where the pair is
/// exactly logical and (LCM) / or (GCD); integers give integers; the real
/// GCD of fractions runs the same Euclid on the values themselves.
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
    tol: Tol,
    rules: Rules,
    span: Span,
) -> Result<Data> {
    // GNU APL's GCD rounds its arguments and keeps a whole one's sign
    // beside a zero; J's and Dyalog's do neither.
    let gnu = rules.lang == crate::Lang::Apl
        && rules.gcd_rule == crate::frontend::GcdRule::Tolerant;
    let t = arith_type(x.dtype(), y.dtype(), span)?;
    if t == DType::Complex {
        // The Gaussian-integer versions, which is what both references give.
        return complex_dyad_data(op, x, xoff, xdiv, y, yoff, ydiv, n, span);
    }
    if t.is_exact()
        && let Some(d) = exact_dyad_data(op, t, x, xoff, xdiv, y, yoff, ydiv, n, span)?
    {
        return Ok(d);
    }
    let both_bool = x.dtype() == DType::Bool && y.dtype() == DType::Bool;
    let float = t == DType::F64;
    let (xs, ys) = if float {
        let (mut tx, mut ty) = (Vec::new(), Vec::new());
        let xf = borrow_f64(x, &mut tx);
        let yf = borrow_f64(y, &mut ty);
        let integral = |v: &[f64]| v.iter().all(|&a| a.fract() == 0.0 && fits_i64(a));
        if !integral(xf) || !integral(yf) {
            return real_lcm_gcd(op, xf, xoff, xdiv, yf, yoff, ydiv, n, tol, gnu, span);
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
            let g = if gnu { signed_gcd_i128(a, b) } else { gcd_i128(a, b) };
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
        Data::F64(_) | Data::Complex(_) | Data::Char(_) | Data::Symbol(_) | Data::Box(_) => {
            return None;
        }
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
        return Some(Array::new(shape, Data::Ext(out.into())).with_layout(y.layout()));
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
    Some(Array::new(shape, exact_data(y.dtype(), out)).with_layout(y.layout()))
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
        Data::Complex(_) | Data::Char(_) | Data::Symbol(_) | Data::Box(_) => {
            return Err(Error::domain(
                format!("x: needs real numbers, not {} data", y.dtype().name()),
                span,
            ));
        }
    };
    Ok(Array::new(y.shape.clone(), data).with_layout(y.layout()))
}

/// `_1 x: y`: an exact value back as a machine number — an extended integer
/// as an integer where it fits, a rational as a float.
fn from_exact(y: &Array) -> Array {
    let shape = y.shape.clone();
    match &y.data {
        Data::Ext(v) => match v.iter().map(exact::ext_to_i64).collect::<Option<Vec<i64>>>() {
            Some(out) => Array::new(shape, Data::I64(out.into())).with_layout(y.layout()),
            None => Array::new(shape, Data::F64(v.iter().map(exact::ext_to_f64).collect()))
                .with_layout(y.layout()),
        },
        Data::Rat(v) => Array::new(shape, Data::F64(v.iter().map(Rat::to_f64).collect()))
            .with_layout(y.layout()),
        _ => y.clone(),
    }
}

/// `x x: y`: the exact form named by x.
fn exact_form(x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    match one_whole(x, "the form x: converts to", near, span)? {
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
    rules: Rules,
    span: Span,
) -> Result<Data> {
    use ScalarDyad::*;
    if x.dtype() == DType::Symbol || y.dtype() == DType::Symbol {
        match op {
            // Comparison takes the path below, which knows symbols.
            Eq | Ne | Lt | Le | Gt | Ge => {}
            // `<.` and `>.` are the smaller and the larger of two names,
            // and a name has an order, so they answer a symbol.
            Min | Max => {
                return symbol_min_max(op, x, xoff, xdiv, y, yoff, ydiv, n, span);
            }
            _ => return Err(symbol_arith(span)),
        }
    }
    if matches!(op, Eq | Ne | Lt | Le | Gt | Ge) {
        return compare_data(op, x, xoff, xdiv, y, yoff, ydiv, n, tol, span);
    }
    if matches!(op, Lcm | Gcd) {
        return lcm_gcd_data(op, x, xoff, xdiv, y, yoff, ydiv, n, tol, rules, span);
    }
    let t = arith_type(x.dtype(), y.dtype(), span)?;
    if t.is_exact()
        && let Some(d) = exact_dyad_data(op, t, x, xoff, xdiv, y, yoff, ydiv, n, span)?
    {
        return Ok(d);
    }
    // No exact answer above: widen, exactly as an integer overflow does.
    if t == DType::I64 && !matches!(op, DivJ | DivApl | Log | Root | Circle) {
        // Binomial reaches this path: a whole pair has a whole answer, and
        // the i64 step declines (None) exactly where J widens to float.
        if let Some(d) = int_dyad_data(op, x, xoff, xdiv, y, yoff, ydiv, n) {
            return Ok(d);
        }
        // Integer overflow (or a fractional result): J widens to float.
    }
    if t == DType::Complex
        || matches!(op, MakeComplex | PolarBy)
        || pass_leaves_reals(op, x, xoff, xdiv, y, yoff, ydiv, n)
    {
        let data = complex_dyad_data(op, x, xoff, xdiv, y, yoff, ydiv, n, span)?;
        // GNU APL has no infinite logarithm in the complex domain either:
        // `¯1⍟0` is a DOMAIN ERROR there, exactly as `2⍟0` is on the reals,
        // and the real path above already refuses that one.
        if op == Log
            && rules.lang == crate::Lang::Apl
            && let Data::Complex(v) = &data
            && v.iter().any(|z| !z[0].is_finite() || !z[1].is_finite())
        {
            return Err(Error::domain("this logarithm has no value", span));
        }
        if op == Circle && circle_reads_a_part(x, xoff, xdiv, n) && let Data::Complex(v) = &data {
            return Ok(Data::F64(v.iter().map(|z| z[0]).collect()));
        }
        return Ok(data);
    }
    float_dyad_data(op, x, xoff, xdiv, y, yoff, ydiv, n, tol, span)
}

/// Elementwise dyadic application of a scalar operation to whole arrays.
/// Frame the results of a pervading scalar function. Cells that all came
/// back simple scalars make a simple array again — `(1 2)+(3 4)` is a plain
/// vector — and anything else is enclosed, which is what keeps the nesting.
fn frame_pervaded(frame: Vec<usize>, cells: Vec<Array>, span: Span) -> Result<Array> {
    if cells.iter().all(|c| c.rank() == 0 && c.dtype() != DType::Box) {
        return assemble(&frame, cells, span);
    }
    let boxes: Vec<Array> = cells.into_iter().collect();
    Ok(Array::new(frame, Data::Box(boxes.into())))
}

/// APL's scalar functions PERVADE a nested argument: they descend through
/// the boxes, item by item, and apply to the simple values at the bottom.
/// The two sides agree by the ordinary scalar rule at every level, so a
/// scalar spreads over a nested array's items as it does over a simple
/// array's elements. J has no such rule — a box there is a type error.
fn pervade_dyad(
    op: ScalarDyad,
    x: &Array,
    y: &Array,
    cfg: EvalCfg,
    span: Span,
) -> Result<Array> {
    let p = agree(&x.shape, &y.shape, &x.shape, &y.shape, cfg.agreement, span)?;
    if p.n == 0 {
        return Ok(Array::new(p.frame, Data::empty(DType::Box)));
    }
    let (xr, yr) = (x.to_row_major(), y.to_row_major());
    let mut cells = Vec::with_capacity(p.n);
    for i in 0..p.n {
        let a = open_cell(&atom(&xr, i / p.x_div));
        let b = open_cell(&atom(&yr, i / p.y_div));
        cells.push(scalar_dyad(op, &a, &b, cfg, span)?);
    }
    frame_pervaded(p.frame, cells, span)
}

/// The monadic half of [`pervade_dyad`].
fn pervade_monad(op: ScalarMonad, y: &Array, cfg: EvalCfg, span: Span) -> Result<Array> {
    if y.count() == 0 {
        return Ok(Array::new(y.shape.clone(), Data::empty(DType::Box)));
    }
    let yr = y.to_row_major();
    let mut cells = Vec::with_capacity(y.count());
    for i in 0..y.count() {
        let a = open_cell(&atom(&yr, i));
        cells.push(scalar_monad(op, &a, cfg, span)?);
    }
    frame_pervaded(y.shape.clone(), cells, span)
}

/// The same array with its complex values read as the reals they are, or
/// `None` when one of them really is complex. A value that is not complex
/// at all needs no reading and answers for itself.
fn as_real(a: &Array) -> Option<Array> {
    if a.dtype() != DType::Complex {
        return Some(a.clone());
    }
    let real: Option<Vec<f64>> = a.to_f64_vec();
    Some(Array::new(a.shape.clone(), Data::F64(real?.into())))
}

fn scalar_dyad(
    op: ScalarDyad,
    x: &Array,
    y: &Array,
    cfg: EvalCfg,
    span: Span,
) -> Result<Array> {
    if cfg.rules.lang == crate::Lang::Apl
        && (x.dtype() == DType::Box || y.dtype() == DType::Box)
    {
        return pervade_dyad(op, x, y, cfg, span);
    }
    // A complex value with no imaginary part is ordered by the real it
    // displays as: J answers `1 <. j. 0` with 0 and `3j0 < 4` with 1, while
    // `3!:0 j. 0` still reports the complex type. Only the ordering verbs
    // read a value that way — arithmetic keeps the complex type through its
    // answer, which is why the demotion sits here and not in the maker.
    if matches!(op, ScalarDyad::Min | ScalarDyad::Max | ScalarDyad::Lt
        | ScalarDyad::Le | ScalarDyad::Gt | ScalarDyad::Ge)
        && (x.dtype() == DType::Complex || y.dtype() == DType::Complex)
        && let (Some(a), Some(b)) = (as_real(x), as_real(y))
    {
        return scalar_dyad(op, &a, &b, cfg, span);
    }
    let p = agree(&x.shape, &y.shape, &x.shape, &y.shape, cfg.agreement, span)?;
    // Nothing to apply the verb to: `'a' + ''` is an empty, not a type
    // error, because no pair of elements was ever formed. The agreement
    // above still holds — `1 2 3 + ''` is a length error either way.
    if p.n == 0 {
        return Ok(Array::new(p.frame, Data::empty(empty_result_type(x, y))));
    }
    let data = scalar_dyad_data(
        op,
        &x.data,
        0,
        p.x_div,
        &y.data,
        0,
        p.y_div,
        p.n,
        cfg.tol,
        cfg.rules,
        span,
    )?;
    Ok(Array::new(p.frame, data))
}

/// The element type of an empty answer. A numeric operand names it; with
/// none, the numbers an arithmetic result would have held.
fn empty_result_type(x: &Array, y: &Array) -> DType {
    for a in [x, y] {
        if a.dtype().is_numeric() {
            return a.dtype();
        }
    }
    DType::I64
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
            Data::I64(v) => par::any(v, |&x| x < 0),
            Data::F64(v) => par::any(v, |&x| x < 0.0),
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
    Ok(Array::new(y.shape.clone(), data).with_layout(y.layout()))
}

/// Elementwise monadic application to a whole array.
fn scalar_monad(op: ScalarMonad, y: &Array, cfg: EvalCfg, span: Span) -> Result<Array> {
    use ScalarMonad::*;
    if cfg.rules.lang == crate::Lang::Apl && y.dtype() == DType::Box {
        return pervade_monad(op, y, cfg, span);
    }
    let tol = cfg.tol;
    let d = &y.data;
    // An empty argument has no element for the verb to run on, so its type
    // never comes up: `%: ''` is an empty, not a type error.
    if y.count() == 0 && !d.dtype().is_numeric() {
        return Ok(Array::new(y.shape.clone(), Data::empty(DType::I64)));
    }
    if d.dtype() == DType::Complex || monad_leaves_reals(op, d) {
        return complex_monad(op, y, span);
    }
    if d.dtype().is_exact() && let Some(a) = exact_monad(op, y) {
        return Ok(a);
    }
    // No exact answer above: the float pass below takes over.
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
            // NaN has no sign here; it yields 0, and so does anything the
            // dialect's tolerance reads as zero.
            Data::F64(v) => Data::F64(
                par::map(v, |&x| {
                    if tol.is_zero(x) {
                        0.0
                    } else if x > 0.0 {
                        1.0
                    } else if x < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                })
                .into(),
            ),
            _ => return Err(wrong_type(d.dtype(), span)),
        },
        Recip => {
            // `% 0` is infinity in J. GNU APL has no such value: `÷0` is a
            // DOMAIN ERROR, as its dyadic `2÷0` already is here, and the
            // monad has to refuse the same pair the dyad does — including
            // through `¨`, `/` and `\`, which all arrive at this one step.
            let v = as_f64(d, &mut tmp, span)?;
            if !tol.is_j() && par::any(v, |&x| x == 0.0) {
                return Err(Error::domain("zero has no reciprocal", span));
            }
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
            let out = par::map(v, |&x| factorial_as(x, tol));
            if tol.is_j() {
                // The one factorial J refuses. Everything else its gamma
                // cannot reach it answers with `_`, which `factorial_as`
                // has already done.
                if v.iter().zip(&out).any(|(&x, &r)| tol.made_nan(r, x, 0.0)) {
                    return Err(Error::nan("`! __` has no value", span));
                }
            } else if par::any(&out, |v: &f64| !v.is_finite()) {
                // GNU APL refuses every factorial without a value: `!¯3`
                // and `!¯1` sit on a pole of the gamma function, `!171` has
                // overflowed it. J answers all three with `_`.
                return Err(Error::domain("this factorial has no value", span));
            }
            Data::F64(out.into())
        }
        Ln => {
            // As with `Sqrt`: a negative value is already on the complex path.
            let v = as_f64(d, &mut tmp, span)?;
            // ln(0) is negative infinity, which is what J prints as __. GNU
            // APL has no such value and refuses `⍟0`, exactly as it
            // refuses `÷0`.
            if !tol.is_j() && par::any(v, |&x| x == 0.0) {
                return Err(Error::domain("zero has no logarithm", span));
            }
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
    Ok(Array::new(y.shape.clone(), data).with_layout(y.layout()))
}

// -------------------------------------------------- structural operations

/// Reverse the axes.
///
/// Nothing moves: reversing every axis is exactly what reading the same
/// buffer in the other layout does, so this is a reversed shape, the same
/// buffer, and the flag flipped. Whatever reads the result either knows
/// both layouts or is handed the rows, materialised once and only if some
/// verb really needs them.
fn transpose_axes(y: &Array) -> Array {
    if y.rank() < 2 {
        return y.clone();
    }
    let out_shape: Vec<usize> = y.shape.iter().rev().copied().collect();
    let flipped = match y.layout() {
        Layout::RowMajor => Layout::ColMajor,
        Layout::ColMajor => Layout::RowMajor,
    };
    Array::new(out_shape, y.data.clone()).with_layout(flipped)
}

/// J `i.`: an ascending sequence laid out in shape |y|, running backwards
/// along every axis whose given length was negative.
fn iota_j(y: &Array, near: NearInt, span: Span) -> Result<Array> {
    if y.rank() > 1 {
        return Err(Error::new(
            ErrorKind::Rank,
            "index generator needs a scalar or vector argument",
            Some(span),
        ));
    }
    let dims = y
        .to_i64_vec_near(near)
        .ok_or_else(|| Error::domain("index generator needs integer lengths", span))?;
    let shape: Vec<usize> = dims.iter().map(|d| d.unsigned_abs() as usize).collect();
    let n = crate::limits::elements(&shape, span)?;
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
fn rotate(x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    let counts = axis_counts(x, "rotate", near, span)?;
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
            // The amount is reduced modulo the axis BEFORE the coordinate
            // is added to it: a rotate of 9223372036854775806 is a legal
            // sentence, and adding it to a coordinate first overflows.
            let s = counts.get(k).copied().unwrap_or(0).rem_euclid(len);
            idx += (coord[k] as i64 + s).rem_euclid(len) as usize * st[k];
        }
        push_elem(&mut data, &y.data, idx);
        odometer(&mut coord, &y.shape);
    }
    Ok(Array::new(y.shape.clone(), data))
}

/// `x ⌽ y` and `x ⊖ y`: rotate one axis of y, by one amount per vector
/// along it.
///
/// APL's left argument is not J's one amount per axis. Exactly one axis
/// moves — the last for `⌽`, the leading one for `⊖`, the named one for
/// `⌽[k]` — and x holds one amount for each vector along it, so `⍴x` must
/// be `⍴y` with that axis removed. A scalar (or a one-item vector, which
/// GNU APL accepts as one) rotates every vector by the same amount.
/// Anything else is a conformability error: a rank error where the ranks
/// disagree and a length error where only the lengths do.
fn rotate_apl(x: &Array, y: &Array, last: bool, near: NearInt, span: Span) -> Result<Array> {
    let scalar_like = x.rank() == 0 || (x.rank() == 1 && x.count() == 1);
    // A scalar has no axis to rotate, so it is its own answer — but only
    // for a left argument that could have rotated something.
    if y.rank() == 0 {
        return if scalar_like {
            Ok(y.clone())
        } else {
            Err(Error::new(
                ErrorKind::Rank,
                format!(
                    "rotate has a rank-{} left argument for a scalar, which needs a scalar",
                    x.rank()
                ),
                Some(span),
            ))
        };
    }
    let axis = if last { y.rank() - 1 } else { 0 };
    let want: Vec<usize> =
        y.shape.iter().enumerate().filter(|&(k, _)| k != axis).map(|(_, &n)| n).collect();
    if !scalar_like {
        if x.rank() != want.len() {
            return Err(Error::new(
                ErrorKind::Rank,
                format!(
                    "rotate has a rank-{} left argument for axis {axis} of {}, which needs rank {}",
                    x.rank(),
                    show_shape(&y.shape),
                    want.len()
                ),
                Some(span),
            ));
        }
        if x.shape != want {
            return Err(Error::new(
                ErrorKind::Length,
                format!(
                    "rotate has a {} left argument for axis {axis} of {}, which needs {}",
                    show_shape(&x.shape),
                    show_shape(&y.shape),
                    show_shape(&want)
                ),
                Some(span),
            ));
        }
    }
    let counts = x
        .to_i64_vec_near(near)
        .ok_or_else(|| Error::domain("rotate needs integer lengths", span))?;
    let len = y.shape[axis] as i64;
    let n = y.count();
    if n == 0 {
        return Ok(y.clone());
    }
    let st = strides(&y.shape);
    let r = y.rank();
    let mut data = Data::empty(y.dtype());
    let mut coord = vec![0usize; r];
    for _ in 0..n {
        // Which vector this element sits on, in the order x holds them.
        let mut which = 0usize;
        for (k, &c) in coord.iter().enumerate() {
            if k != axis {
                which = which * y.shape[k] + c;
            }
        }
        let s = if scalar_like { counts[0] } else { counts[which] };
        // Reduced modulo the axis before the coordinate joins it: the
        // amount may be any i64 the program can write.
        let s = s.rem_euclid(len);
        let mut idx = 0usize;
        for (k, &c) in coord.iter().enumerate() {
            let c = if k == axis { (c as i64 + s).rem_euclid(len) as usize } else { c };
            idx += c * st[k];
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
        // A symbol IS its table index, so the index is the key.
        Data::Symbol(v) => v[i] as u64,
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
        Data::Symbol(v) => v[i] as u64,
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

/// Which ordering a grade puts whole arrays in when its items are boxed —
/// J's total array ordering, or the APL2 rule GNU APL implements. The two
/// disagree at every step, so a comparison says which one it is answering
/// for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tao {
    J,
    Apl2,
    /// Dyalog's total array ordering.
    Dyalog,
}

impl Tao {
    fn of(rules: Rules) -> Tao {
        match rules.lang {
            crate::Lang::J => Tao::J,
            crate::Lang::Apl => match rules.nested_grade {
                NestedGrade::Apl2 => Tao::Apl2,
                NestedGrade::TotalOrder => Tao::Dyalog,
            },
        }
    }

    /// The type class compared before the atoms: J puts numeric first,
    /// then symbol, then character, then boxed; APL2 puts character first,
    /// then numeric, then nested. APL has no symbols of its own, so a
    /// symbol that reaches an APL grade sorts with the characters it is
    /// made of names of.
    fn class(self, dt: DType) -> u8 {
        match self {
            Tao::J => match dt {
                DType::Symbol => 1,
                DType::Char => 2,
                DType::Box => 3,
                _ => 0,
            },
            Tao::Apl2 => match dt {
                DType::Char | DType::Symbol => 0,
                DType::Box => 2,
                _ => 1,
            },
            // Dyalog puts every number before every character. A nested
            // value is never placed by its own type here: an array with
            // atoms is decided by them, and an atomless one by the item it
            // would have held (`proto_item`), so the box arm is reached
            // only for an empty that has forgotten its prototype.
            Tao::Dyalog => match dt {
                DType::Char | DType::Symbol => 2,
                DType::Box => 1,
                _ => 0,
            },
        }
    }
}

/// A grade's comparator: which total ordering it puts whole arrays in, and
/// the tolerance the numbers inside it are read with.
///
/// APL's `⍋` and `⍒` compare under `⎕CT` — `⍋1.0000000000001 1` is `1 2` in
/// GNU APL, the two keys equal and left in the order they came — while J's
/// grade is exact whatever the comparison tolerance is: jconsole answers
/// `/: 1 1.0000000000001 1` with `0 2 1`.
#[derive(Clone, Copy, Debug)]
struct Grading {
    tao: Tao,
    tol: Tol,
}

impl Grading {
    fn of(rules: Rules, tol: Tol) -> Grading {
        let tao = Tao::of(rules);
        Grading { tao, tol: if tao == Tao::J { Tol::EXACT } else { tol } }
    }

    fn class(self, dt: DType) -> u8 {
        self.tao.class(dt)
    }
}

/// Order two whole arrays, which is how a grade compares boxed items.
///
/// J compares the type class first — and an EMPTY array has no atoms to
/// take a class from, so it takes the lowest one whatever its type, which
/// is why `/: (<''),(<<1)` puts the empty character list first and two
/// empties of different types tie. Then the rank, then the shape read with
/// the LAST axis most significant, then the atoms in row-major order.
///
/// APL2 compares the rank first, then the shape read from the FIRST axis,
/// then the atoms, where a character precedes a number precedes a nested
/// value; two arrays with no atoms are separated by their types instead.
///
/// Both are exact — a grade never reads the comparison tolerance — and a
/// NaN ties with everything, which keeps the sort total.
fn cmp_items_total(x: &Array, y: &Array, ord: Grading) -> std::cmp::Ordering {
    use std::cmp::Ordering::Equal;
    match ord.tao {
        Tao::Dyalog => cmp_items_dyalog(x, y, ord),
        Tao::J => {
            let class = |a: &Array| if a.count() == 0 { 0 } else { ord.class(a.dtype()) };
            class(x)
                .cmp(&class(y))
                .then_with(|| x.rank().cmp(&y.rank()))
                .then_with(|| x.shape.iter().rev().cmp(y.shape.iter().rev()))
                .then_with(|| cmp_atoms(x, y, ord))
        }
        Tao::Apl2 => x
            .rank()
            .cmp(&y.rank())
            .then_with(|| x.shape.iter().cmp(y.shape.iter()))
            .then_with(|| cmp_atoms(x, y, ord))
            .then_with(|| {
                if x.count() == 0 {
                    ord.class(x.dtype()).cmp(&ord.class(y.dtype()))
                } else {
                    Equal
                }
            }),
    }
}

/// Two whole arrays in Dyalog's total array ordering.
///
/// The shapes are brought together rather than compared: the lower rank
/// gains leading 1s, and each axis is taken to the longer of the two, so
/// the arrays are read position by position over the shape that covers
/// both. A position one array has and the other does not answers at once —
/// what is not there sorts below every value there is — and a position
/// both hold compares its atoms, which recurses where an atom is nested.
/// Only arrays with no atoms to separate them reach the type (numbers,
/// then nested values, then characters) and then the shape, which is read
/// with the LAST axis most significant.
///
/// Derived from the recorded Dyalog answers in
/// `crates/libjay/tests/snapshots/apl/grade.snap`, which is what pins it.
fn cmp_items_dyalog(x: &Array, y: &Array, ord: Grading) -> std::cmp::Ordering {
    use std::cmp::Ordering::{Equal, Greater, Less};
    // Two simple scalars are the bottom of the recursion; everything else
    // is read as an array of atoms.
    if x.rank() == 0 && y.rank() == 0 && x.dtype() != DType::Box && y.dtype() != DType::Box {
        return cmp_atoms(x, y, ord);
    }
    let rank = x.rank().max(y.rank());
    let extend = |a: &Array| -> Vec<usize> {
        let mut s = vec![1usize; rank - a.rank()];
        s.extend_from_slice(&a.shape);
        s
    };
    let (sx, sy) = (extend(x), extend(y));
    let common: Vec<usize> = (0..rank).map(|k| sx[k].max(sy[k])).collect();
    let (xr, yr) = (x.to_row_major(), y.to_row_major());
    let (dx, dy) = (xr.row_major_data(), yr.row_major_data());
    let (stx, sty) = (strides(&sx), strides(&sy));
    let mut order = Equal;
    if !common.contains(&0) {
        let mut coord = vec![0usize; rank];
        loop {
            let inside = |s: &[usize]| (0..rank).all(|k| coord[k] < s[k]);
            let at = |st: &[usize]| -> usize { (0..rank).map(|k| coord[k] * st[k]).sum() };
            let here = match (inside(&sx), inside(&sy)) {
                (true, true) => {
                    cmp_items_dyalog(&atom_array(dx, at(&stx)), &atom_array(dy, at(&sty)), ord)
                }
                // What is not there is below what is.
                (true, false) => Greater,
                (false, true) => Less,
                (false, false) => Equal,
            };
            if here != Equal {
                order = here;
                break;
            }
            // The odometer wraps to all zeros when the last position is
            // done, and every position either decides or holds equal
            // atoms, so this walks no further than the shorter array.
            odometer(&mut coord, &common);
            if coord.iter().all(|&c| c == 0) {
                break;
            }
        }
    }
    if order != Equal {
        return order;
    }
    // Nothing was there to compare, so the arrays are separated by the item
    // they WOULD have held and then by their shape, last axis first.
    match (proto_item(x), proto_item(y)) {
        // The prototypes are values like any other, and are compared under
        // the same tolerance the atoms would have been.
        (Some(px), Some(py)) => cmp_items_dyalog(&px, &py, ord),
        _ => ord.class(x.dtype()).cmp(&ord.class(y.dtype())),
    }
    .then_with(|| x.shape.iter().rev().cmp(y.shape.iter().rev()))
}

/// The item an atomless array would have held, as an array of its own: a
/// nested empty's remembered prototype, and for a simple one the fill its
/// type implies — a zero, or a blank. `None` where there is nothing to say,
/// which is a nested empty that has forgotten (and an array with atoms,
/// which is never separated this way).
fn proto_item(a: &Array) -> Option<Array> {
    if let Some(p) = a.proto() {
        return Some(p.clone());
    }
    match a.dtype() {
        DType::Box => None,
        dt => Some(Array::new(vec![], fill_data(dt, 1))),
    }
}

/// Element `i` of a buffer as an array of its own: a box gives up its
/// contents, anything else is a simple scalar.
fn atom_array(d: &Data, i: usize) -> Array {
    match d {
        Data::Box(v) => v[i].clone(),
        _ => {
            let mut one = Data::empty(d.dtype());
            push_elem(&mut one, d, i);
            Array::new(vec![], one)
        }
    }
}

/// The atoms of two arrays of the same shape, in row-major order. A boxed
/// atom is compared by its contents, which is where the ordering recurses.
fn cmp_atoms(x: &Array, y: &Array, ord: Grading) -> std::cmp::Ordering {
    use std::cmp::Ordering::Equal;
    let n = x.count();
    if n == 0 {
        return Equal;
    }
    let (xr, yr) = (x.to_row_major(), y.to_row_major());
    let (dx, dy) = (xr.row_major_data(), yr.row_major_data());
    if matches!(dx, Data::Box(_)) || matches!(dy, Data::Box(_)) {
        return (0..n)
            .map(|i| cmp_items_total(&atom_array(dx, i), &atom_array(dy, i), ord))
            .find(|o| *o != Equal)
            .unwrap_or(Equal);
    }
    // Neither side is boxed, so one class covers all of each side's atoms.
    let classes = ord.class(dx.dtype()).cmp(&ord.class(dy.dtype()));
    if classes != Equal {
        return classes;
    }
    match (dx, dy) {
        (Data::Char(a), Data::Char(b)) => a[..n].cmp(&b[..n]),
        _ => cmp_numbers(dx, dy, n, ord.tol),
    }
}

/// Two numeric buffers, `n` elements each, compared in order. The widening
/// is the one `arrays_match` uses, so `1r2` and `0.5` compare where they
/// belong however each is spelled.
fn cmp_numbers(dx: &Data, dy: &Data, n: usize, tol: Tol) -> std::cmp::Ordering {
    use std::cmp::Ordering::Equal;
    let seek = |f: &dyn Fn(usize) -> std::cmp::Ordering| {
        (0..n).map(f).find(|o| *o != Equal).unwrap_or(Equal)
    };
    match DType::promote(dx.dtype(), dy.dtype()) {
        Some(DType::Complex) => {
            let (mut ta, mut tb) = (Vec::new(), Vec::new());
            let (a, b) = (borrow_cx(dx, &mut ta), borrow_cx(dy, &mut tb));
            seek(&|k| tol_ord(a[k][0], b[k][0], tol).then_with(|| tol_ord(a[k][1], b[k][1], tol)))
        }
        Some(DType::F64) => {
            let (mut ta, mut tb) = (Vec::new(), Vec::new());
            let (a, b) = (borrow_f64(dx, &mut ta), borrow_f64(dy, &mut tb));
            seek(&|k| tol_ord(a[k], b[k], tol))
        }
        Some(t) if t.is_exact() => match (to_rat_vec(dx), to_rat_vec(dy)) {
            (Some(a), Some(b)) => seek(&|k| a[k].cmp(&b[k])),
            _ => Equal,
        },
        // Characters and boxes never reach here: the classes agreed.
        None => Equal,
        Some(_) => {
            let (mut ta, mut tb) = (Vec::new(), Vec::new());
            let (a, b) = (borrow_i64(dx, &mut ta), borrow_i64(dy, &mut tb));
            seek(&|k| a[k].cmp(&b[k]))
        }
    }
}

/// Compare items `i` and `j` (of `m` elements each) elementwise, left to
/// right. Characters order by codepoint; a NaN compares equal to anything,
/// which keeps the sort total.
fn cmp_items(d: &Data, i: usize, j: usize, m: usize, ord: Grading) -> std::cmp::Ordering {
    use std::cmp::Ordering::Equal;
    let (a, b) = (i * m, j * m);
    let ord = |k: usize| match d {
        Data::Bool(v) => v[a + k].cmp(&v[b + k]),
        Data::I64(v) => v[a + k].cmp(&v[b + k]),
        Data::F64(v) => tol_ord(v[a + k], v[b + k], ord.tol),
        // Grading a complex array orders it by real part then imaginary,
        // which is the order J's `/:` puts it in and the dialect's
        // `ComplexOrder::RealThenImaginary`; `check_gradable` has already
        // refused the other reading. The ordering VERBS still refuse
        // complex outright: a grade is a permutation, not a claim about
        // size.
        Data::Complex(v) => tol_ord(v[a + k][0], v[b + k][0], ord.tol)
            .then_with(|| tol_ord(v[a + k][1], v[b + k][1], ord.tol)),
        Data::Char(v) => v[a + k].cmp(&v[b + k]),
        // Symbols order by the NAME behind the index, not by the order
        // the two names happened to be interned in.
        Data::Symbol(v) => crate::symbol::cmp(v[a + k], v[b + k]),
        // The exact types order by value, however they are spelled: `2r4`
        // grades exactly where `1r2` does.
        Data::Ext(v) => v[a + k].cmp(&v[b + k]),
        Data::Rat(v) => v[a + k].cmp(&v[b + k]),
        // A boxed element is a whole array: the ordering of the language
        // being graded in decides between two of them.
        Data::Box(v) => cmp_items_total(&v[a + k], &v[b + k], ord),
    };
    (0..m).map(ord).find(|o| *o != Equal).unwrap_or(Equal)
}

/// The stable permutation that sorts the items of `y`.
fn grade_order(y: &Array, down: bool, ord: Grading) -> Vec<usize> {
    if y.rank() == 0 {
        return vec![0];
    }
    let n = y.items();
    let m = y.item_size();
    let mut idx: Vec<usize> = (0..n).collect();
    // A stable sort leaves equal items in their original order, which is
    // what both languages promise, ascending and descending alike.
    if down {
        idx.sort_by(|&a, &b| cmp_items(&y.data, b, a, m, ord));
    } else {
        idx.sort_by(|&a, &b| cmp_items(&y.data, a, b, m, ord));
    }
    idx
}

/// `x ⍋ y` and `x ⍒ y`: every character of y is keyed by where it first
/// occurs in the collating array x — the coordinate read with the LAST axis
/// most significant, and one past the end for a character x does not hold —
/// and the items of y are ordered by those keys read left to right.
fn collate_grade(x: &Array, y: &Array, down: bool, origin: i64, span: Span) -> Result<Array> {
    let chars_of = |a: &Array| -> Result<Vec<char>> {
        match a.row_major_data() {
            Data::Char(v) => Ok(v.as_slice().to_vec()),
            _ => Err(Error::domain("a collating grade takes characters", span)),
        }
    };
    let (xs, ys) = (chars_of(x)?, chars_of(y)?);
    let xshape = if x.rank() == 0 { vec![1] } else { x.shape.clone() };
    let width = xshape.len();
    // The key of a character: its first coordinate in x, reversed so the
    // last axis decides first. A character x does not hold sorts after
    // every one it does.
    let absent: Vec<usize> = xshape.iter().rev().copied().collect();
    let mut keys: std::collections::HashMap<char, Vec<usize>> =
        std::collections::HashMap::new();
    let xst = strides(&xshape);
    for (i, &c) in xs.iter().enumerate() {
        keys.entry(c).or_insert_with(|| {
            (0..width).map(|a| (i / xst[a]) % xshape[a]).rev().collect()
        });
    }
    let key_of = |c: char| keys.get(&c).unwrap_or(&absent).clone();
    let n = if y.rank() == 0 { 1 } else { y.items() };
    let m = if n == 0 { 0 } else { ys.len() / n };
    let item_keys: Vec<Vec<usize>> = (0..n)
        .map(|i| ys[i * m..(i + 1) * m].iter().flat_map(|&c| key_of(c)).collect())
        .collect();
    let mut idx: Vec<usize> = (0..n).collect();
    if down {
        idx.sort_by(|&a, &b| item_keys[b].cmp(&item_keys[a]));
    } else {
        idx.sort_by(|&a, &b| item_keys[a].cmp(&item_keys[b]));
    }
    Ok(Array::from_i64(idx.into_iter().map(|i| origin + i as i64).collect()))
}

/// `5!:1 <'name'`: the atomic representation of what the name stands for.
/// A verb answers with the representation of the verb, a value with the
/// noun pair; either way the answer is boxed, as the reference has it.
fn atomic_rep(y: &Array, ctx: &Ctx<'_>, span: Span) -> Result<Array> {
    let name = match y.as_boxes() {
        Some([b]) if y.rank() == 0 => crate::gerund::text_of(b),
        _ => None,
    };
    let Some(name) = name else {
        return Err(Error::domain("5!:1 takes a boxed name", span));
    };
    if let Some(v) = ctx.env.verb(&name) {
        let ar = crate::gerund::verb_ar(v).ok_or_else(|| {
            Error::not_yet(
                format!("the atomic representation of {}", v.name()),
                span,
            )
        })?;
        return Ok(Array::boxed(ar.to_array()));
    }
    match ctx.env.get(&name) {
        Some(a) => Ok(Array::boxed(crate::gerund::Ar::Noun(a).to_array())),
        None => Err(Error::new(
            ErrorKind::Value,
            format!("undefined name: {name}"),
            Some(span),
        )),
    }
}

/// `{ y`: the catalogue — every way of taking one element from each item
/// of y. The shapes of the items, opened, make the result's shape, and each
/// element of it is the boxed vector of one choice from each.
fn catalogue(y: &Array, span: Span) -> Result<Array> {
    let items = if y.rank() == 0 { vec![y.clone()] } else { y.cells(1) };
    // A boxed item stands for its contents; a simple one for itself.
    let opened: Vec<Array> = items
        .iter()
        .map(|it| match it.as_boxes() {
            Some(bs) if it.rank() == 0 => bs[0].clone(),
            _ => it.clone(),
        })
        .collect();
    let mut shape: Vec<usize> = Vec::new();
    for o in &opened {
        shape.extend_from_slice(&o.shape);
    }
    let total: usize = shape.iter().product();
    let mut out = Vec::with_capacity(total);
    let mut coord = vec![0usize; shape.len()];
    for _ in 0..total {
        let mut at = 0usize;
        let mut picks = Vec::with_capacity(opened.len());
        for o in &opened {
            let st = strides(&o.shape);
            let idx: usize = (0..o.rank()).map(|a| coord[at + a] * st[a]).sum();
            at += o.rank();
            let mut data = Data::empty(o.dtype());
            push_elem(&mut data, o.row_major_data(), idx);
            picks.push(Array::new(vec![], data));
        }
        out.push(assemble(&[picks.len()], picks, span)?);
        odometer(&mut coord, &shape);
    }
    Ok(Array::new(shape, Data::Box(out.into())))
}

/// `e. y`: for every element of y, which items of the raze of y it holds —
/// so the answer is shaped `($y), #items of the raze`.
fn raze_in(y: &Array, tol: Tol, span: Span) -> Result<Array> {
    let all = raze(y, span)?;
    let n = if all.rank() == 0 { 1 } else { all.items() };
    let elements: Vec<Array> = (0..y.count())
        .map(|i| {
            let mut data = Data::empty(y.dtype());
            push_elem(&mut data, y.row_major_data(), i);
            let one = Array::new(vec![], data);
            match one.as_boxes() {
                Some(bs) => bs[0].clone(),
                None => one,
            }
        })
        .collect();
    let mut out = Vec::with_capacity(elements.len() * n);
    for e in &elements {
        let row = member_j(&all, e, tol);
        out.extend_from_slice(row.to_i64_vec().unwrap_or_default().as_slice());
    }
    let mut shape = y.shape.clone();
    shape.push(n);
    Ok(Array::new(shape, Data::Bool(out.into_iter().map(|v| v as u8).collect::<Vec<u8>>().into())))
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

/// What a grade refuses, and the dialect setting it reads.
///
/// A grade has to be total over complex values, and the dialect says in
/// which order; only one of the two readings is implemented.
fn check_gradable(y: &Array, rules: Rules, span: Span) -> Result<()> {
    if y.dtype() == DType::Complex && rules.complex_order != ComplexOrder::RealThenImaginary {
        return Err(Error::not_yet("grading complex values by magnitude and angle", span));
    }
    Ok(())
}

/// `x /: y` is `(/: y) { x`: the grade of y is an index into x, so the two
/// lengths need not agree — a shorter key selects fewer items, and only an
/// index past the end of x is an error.
fn grade_select(
    x: &Array,
    y: &Array,
    down: bool,
    rules: Rules,
    tol: Tol,
    span: Span,
) -> Result<Array> {
    check_gradable(y, rules, span)?;
    let order = grade_order(y, down, Grading::of(rules, tol));
    // An atom is ONE item, so the only index it answers is the first: J
    // reads `5 /: 1` as 5 and refuses `5 /: 1 2 3`, where a lenient reading
    // would hand the atom back for any key at all.
    if x.rank() == 0 {
        return match order.iter().find(|&&i| i > 0) {
            None => Ok(x.clone()),
            Some(&past) => Err(Error::domain(
                format!("index {past} is out of range: the argument has 1 item"),
                span,
            )),
        };
    }
    if let Some(&past) = order.iter().find(|&&i| i >= x.items()) {
        return Err(Error::domain(
            format!("index {past} is out of range: the argument has {} items", x.items()),
            span,
        ));
    }
    Ok(select_items(x, &order))
}

/// Whole-array equality: same shape and same values. Characters never equal
/// numbers; `1` equals `1.0`; NaN equals nothing.
pub(crate) fn arrays_match(x: &Array, y: &Array, tol: Tol) -> bool {
    if x.shape != y.shape {
        return false;
    }
    // The comparison is element against element in buffer order, so two
    // values laid out differently are compared in the one order.
    if x.layout() != y.layout() {
        return arrays_match(&x.to_row_major(), &y.to_row_major(), tol);
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
        // Two symbols are the same symbol exactly when they carry the same
        // table index, which is the whole point of interning them.
        Some(DType::Symbol) => match (&x.data, &y.data) {
            (Data::Symbol(a), Data::Symbol(b)) => a.as_slice() == b.as_slice(),
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
        // `⊂5` is `5` in APL, so a box holding a simple scalar compares as
        // that scalar: `1 2 3 ∊ (1 2)(3)` finds the 3.
        let opened = |a: &Array, i: usize| -> Array {
            let e = atom(a, i);
            match e.as_boxes() {
                Some([b]) if b.rank() == 0 && b.dtype() != DType::Box => b.clone(),
                _ => e,
            }
        };
        let out: Vec<u8> = (0..n)
            .map(|i| {
                let e = opened(x, i);
                u8::from((0..y.count()).any(|j| arrays_match(&e, &opened(y, j), tol)))
            })
            .collect();
        return Array::new(x.shape.clone(), Data::Bool(out.into()));
    }
    if x.dtype() != y.dtype()
        && [x.dtype(), y.dtype()].iter().any(|&d| matches!(d, DType::Char | DType::Symbol))
    {
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
///
/// `vector_left` is the Dyalog reading, where the lookup table is a vector
/// and nothing else; without it the items of a left argument of any rank
/// are searched, which is what J and the APL2 line do.
fn index_of(
    x: &Array,
    y: &Array,
    origin: i64,
    vector_left: bool,
    tol: Tol,
    span: Span,
) -> Result<Array> {
    if vector_left && x.rank() != 1 {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("⍳ looks up in a vector, and its left argument has rank {}", x.rank()),
            Some(span),
        ));
    }
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
    Ok(Array::new(frame, Data::I64(out.into())))
}

/// `x { y` for one index atom: the rank machinery supplies the framing.
fn from_index(x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    // A boxed index is J's index specification, which reaches several axes
    // at once; a plain one selects an item.
    if let Some(spec) = x.as_boxes().and_then(<[Array]>::first) {
        let spec = index_spec(spec, y, near, span)?;
        return Ok(select_spec(&spec, y));
    }
    let idx = x
        .to_i64_vec_near(near)
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
pub(crate) fn catenate(
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
            format!(
                "cannot catenate: left shape {}, right shape {}",
                show_shape(&xa.shape),
                show_shape(&ya.shape)
            ),
            Some(span),
        ));
    }
    let (xa, ya) = if ragged {
        let fit = |a: &Array| -> Result<Array> {
            let mut to = want.clone();
            to[axis] = a.shape[axis] as i64;
            // The lengths are ours, not the program's: no float
            // reaches the near-integer admission on this path.
            take(&Array::from_i64(to), a, false, false, NearInt::J, span)
        };
        (fit(&xa)?, fit(&ya)?)
    } else {
        (xa, ya)
    };
    // APL2 catenates a nested array to a simple one by enclosing the
    // simple side's items: `(1 2),⊂3 4` is a three-item nested vector. J
    // refuses the mixture, and its `fill` rule is what tells them apart.
    let (xa, ya) = if !fill && (xa.dtype() == DType::Box) != (ya.dtype() == DType::Box) {
        (nest_like(&xa, &ya), nest_like(&ya, &xa))
    } else {
        (xa, ya)
    };
    // And where two SIMPLE arrays share no type, APL builds a mixed simple
    // one rather than refusing: `1 2,'ab'` is a four-element vector of two
    // numbers and two characters, depth 1. J has no such value.
    let mixing = !fill
        && xa.dtype() != DType::Box
        && ya.dtype() != DType::Box
        && DType::promote(xa.dtype(), ya.dtype()).is_none();
    let (xa, ya) =
        if mixing { (spread_scalars(&xa), spread_scalars(&ya)) } else { (xa, ya) };
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

/// `x # y` / `x / y`: item i of y appears x[i] times.
///
/// A scalar x applies to every item, and a SCALAR y is extended to as many
/// items as x has counts — a one-item vector is not, which is why
/// `1 0 1 # 5` is `5 5` and `1 0 1 # ,5` is a length error. A negative
/// count is APL's: it contributes that many fills. J has no such reading
/// and refuses it.
fn copy_items(x: &Array, y: &Array, apl: bool, near: NearInt, span: Span) -> Result<Array> {
    let counts = x
        .to_i64_vec_near(near)
        .ok_or_else(|| Error::domain("replication counts must be integers", span))?;
    if !apl && counts.iter().any(|&c| c < 0) {
        return Err(Error::domain("replication counts must be nonnegative", span));
    }
    // A scalar right argument stands in for every count, and in APL so does
    // an argument of ONE item along the axis: `2 0 1/,5` is `5 5 5`, where
    // J's `#` calls the same pair a length error.
    let one_item = apl && x.rank() > 0 && y.rank() > 0 && y.items() == 1 && counts.len() != 1;
    let scalar_y = y.rank() == 0 || one_item;
    let m = y.item_size();
    let n = if x.rank() == 0 || !scalar_y { y.items() } else { counts.len() };
    let per = if x.rank() == 0 { vec![counts[0]; n] } else { counts };
    if per.len() != n {
        return Err(Error::new(
            ErrorKind::Length,
            format!("{} replication count(s) for {n} item(s)", per.len()),
            Some(span),
        ));
    }
    // Items, not elements: an item of zero elements still costs a trip
    // round the loop, so the ceiling applies to whichever is larger.
    let items: u128 = per.iter().map(|&c| c.unsigned_abs() as u128).sum();
    let total = crate::limits::count(items * m.max(1) as u128, span)? / m.max(1);
    let fill = if apl { prototype_of(y) } else { None };
    let mut data = Data::empty(y.dtype());
    for (i, &c) in per.iter().enumerate() {
        // A scalar y stands in for every count.
        let src = if scalar_y { 0 } else { i };
        for _ in 0..c.unsigned_abs() {
            for k in 0..m {
                if c < 0 {
                    push_gap(&mut data, &fill);
                } else {
                    push_elem(&mut data, &y.data, src * m + k);
                }
            }
        }
    }
    // A scalar argument has one item, so replicating it yields a vector; an
    // extended one-item argument keeps the shape it already had.
    let mut shape = if y.rank() == 0 { vec![1] } else { y.shape.clone() };
    shape[0] = total;
    Ok(keep_proto(Array::new(shape, data), y, apl))
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
    // A sparse array's display is a table of lines whatever its own rank
    // is: one line per stored entry.
    if y.is_sparse() {
        let text = crate::fmt::format_array(y, opts);
        let lines: Vec<&str> = text.lines().collect();
        let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let mut chars: Vec<char> = Vec::with_capacity(lines.len() * width);
        for line in &lines {
            chars.extend(line.chars());
            chars.resize(chars.len() + width - line.chars().count(), ' ');
        }
        return Array::new(vec![lines.len(), width], Data::Char(chars.into()));
    }
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
    !matches!(a.dtype(), DType::F64 | DType::Rat | DType::Char | DType::Symbol)
}

/// The decode of exact digits in exact radices, accumulated in the exact
/// types. Whole numbers keep every digit — a 19-digit integer decoded
/// through f64 loses its last two — and rational digits give a rational
/// answer, which is what J reports for `#. 1r2 1r3`. `None` hands the pass
/// back to the float path, which also reports the length errors.
fn decode_exact(x: Option<&Array>, y: &Array) -> Option<Array> {
    let yr = y.to_row_major();
    let digits = to_rat_vec(&yr.data)?;
    let two = Rat::from_int(Ext::from(2));
    let mut digits = digits;
    let radix: Vec<Rat> = match x {
        None => vec![two; digits.len()],
        Some(x) => {
            let r = to_rat_vec(&x.to_row_major().data)?;
            // An ATOM of digits is the digit in every position: J reads
            // `2 7 1 8 #. 123x` as four 123s. A one-item LIST is not an
            // atom and does not spread, which is why `1 2 3 #. ,5` is a
            // length error where `1 2 3 #. 5` is 50.
            if y.rank() == 0 && r.len() != 1 {
                digits = vec![digits[0].clone(); r.len()];
            }
            match r.len() {
                1 => vec![r[0].clone(); digits.len()],
                n if n == digits.len() => r,
                _ => return None,
            }
        }
    };
    let mut acc = Rat::from_int(Ext::from(0));
    for (d, b) in digits.iter().zip(&radix) {
        acc = acc.mul(b).add(d);
    }
    let exact_in = |a: &Array| matches!(a.dtype(), DType::Ext | DType::Rat);
    if exact_in(y) || x.is_some_and(exact_in) {
        return Some(Array::new(Vec::new(), exact_data(DType::Ext, vec![acc])));
    }
    // Plain integers in, a plain integer out — but only while it fits; the
    // float path widens beyond that, as both references do.
    let whole = acc.to_int()?;
    Some(Array::scalar_i64(exact::ext_to_i64(&whole)?))
}

/// `x #. y` / `x ⊥ y`: the digits y read in the radices x. A scalar x is the
/// radix of every position; otherwise the two have the same length.
fn decode(x: Option<&Array>, y: &Array, tol: Tol, span: Span) -> Result<Array> {
    if let Some(exact) = decode_exact(x, y) {
        return Ok(exact);
    }
    let mut digits = digits_of(y, "decode", span)?;
    let radix: Vec<f64> = match x {
        None => vec![2.0; digits.len()],
        Some(x) => {
            let r = digits_of(x, "decode", span)?;
            // An atom of digits fills every position the radices name;
            // `(i. 0) #. 5` is the empty sum, 0.
            if y.rank() == 0 && r.len() != 1 {
                digits = vec![digits[0]; r.len()];
            }
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
        // The dialect's product, so that an infinite radix meets the same
        // zero-factor rule `*` does: `_ #. 2` is 2, because the running
        // total is still zero when the infinity multiplies it.
        acc = tol.mul(acc, *b) + d;
    }
    let integral = is_integral(y) && x.is_none_or(is_integral);
    Ok(Array::new(vec![], narrow(vec![acc], integral)))
}

/// `x ⊥ y` on arguments of rank 2 and above: the inner product `+.×` over
/// the LAST axis of x and the LEADING axis of y. A scalar x is the radix
/// for every digit, as it is for a vector argument.
fn decode_apl(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let mut digits = digits_of(y, "decode", span)?;
    let radices = digits_of(x, "decode", span)?;
    // The digit axis is y's leading one; a scalar y has one digit. The
    // frames are the counts of the axes the digit axis leaves over, and a
    // count is a product of axis lengths rather than a division: an axis of
    // length zero on either side leaves no elements to divide by.
    let mut k = if y.rank() == 0 { 1 } else { y.shape[0] };
    let mut n: usize = if y.rank() == 0 { 1 } else { y.shape[1..].iter().product() };
    let (rows, width) = match x.rank() {
        0 => (1usize, 0usize),
        r => (x.shape[..r - 1].iter().product(), x.shape[r - 1]),
    };
    // A SINGLE digit stands in every position the radices name, whatever
    // rank it is written at: `1 2 3⊥5`, `1 2 3⊥,5` and `1 2 3⊥1 1⍴5` are
    // all 50. That is APL2's single extension, and it is why only a digit
    // axis of some OTHER length is a length error.
    if y.count() == 1 && width > 1 && width != k {
        digits = vec![digits[0]; width];
        k = width;
        n = 1;
    }
    // A single radix spreads the same way (`(,2)⊥1 2 3` is 11), and an
    // empty axis on either side weighs nothing at all: the answer is the
    // empty sum, which is what `1 2⊥''` and `(⍳0)⊥5` both report.
    if width > 1 && k != 0 && width != k {
        return Err(Error::new(
            ErrorKind::Length,
            format!("{width} radices for {k} digits"),
            Some(span),
        ));
    }
    // A radix axis of length zero weighs nothing: every answer is the empty
    // sum, whatever the digits are. Only a SCALAR x spreads its one radix
    // over all k digits.
    let per_row = if x.rank() > 0 && width == 0 { 0 } else { k };
    let mut out = vec![0.0f64; rows * n];
    for i in 0..rows {
        for j in 0..n {
            let mut acc = 0.0f64;
            for d in 0..per_row {
                let b = if width <= 1 { radices[i * width] } else { radices[i * width + d] };
                acc = acc * b + digits[d * n + j];
            }
            out[i * n + j] = acc;
        }
    }
    let mut shape: Vec<usize> = if x.rank() == 0 {
        Vec::new()
    } else {
        x.shape[..x.rank() - 1].to_vec()
    };
    if y.rank() > 0 {
        shape.extend_from_slice(&y.shape[1..]);
    }
    let integral = is_integral(y) && is_integral(x);
    Ok(Array::new(shape, narrow(out, integral)))
}

/// `x ⊤ y` where x has rank 2 or more: x's LEADING axis is the radix and
/// its remaining axes frame the answer, so the result is shaped `(⍴x), ⍴y`.
fn encode_apl(x: &Array, y: &Array, tol: Tol, span: Span) -> Result<Array> {
    let radices = digits_of(x, "encode", span)?;
    let values = digits_of(y, "encode", span)?;
    let k = if x.rank() == 0 { 1 } else { x.shape[0] };
    let frames = if k == 0 { 0 } else { radices.len() / k };
    let n = values.len();
    let mut out = vec![0.0f64; k * frames * n];
    let mut radix = vec![0.0f64; k];
    let mut cell = vec![0.0f64; k];
    for p in 0..frames {
        for (i, r) in radix.iter_mut().enumerate() {
            *r = radices[i * frames + p];
        }
        for (j, &v) in values.iter().enumerate() {
            encode_one(&radix, v, &mut cell, tol);
            for i in 0..k {
                out[(i * frames + p) * n + j] = cell[i];
            }
        }
    }
    let mut shape = x.shape.clone();
    shape.extend_from_slice(&y.shape);
    Ok(Array::new(shape, narrow(out, is_integral(x) && is_integral(y))))
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
///
/// Each digit is a residue, and it is taken with the dialect's tolerance as
/// `|` itself is: `2 2 #: 4 - 1e_14` is `0 0` in jconsole, not the `1 2` an
/// exact quotient leaves.
fn encode_one(radix: &[f64], v: f64, out: &mut [f64], tol: Tol) {
    let mut rem = v;
    for i in (0..radix.len()).rev() {
        let b = radix[i];
        if b == 0.0 {
            out[i] = rem;
            rem = 0.0;
        } else {
            let r = tol.residue(b, rem);
            out[i] = r;
            rem = (rem - r) / b;
        }
    }
}

/// `x #: y` / `x ⊤ y`: the digits become the LEADING axis, so the result has
/// shape `(#x), $y`. J applies this per atom of y (right rank 0) and APL to
/// the whole of it (right rank infinite); the operation itself is the same.
fn encode(x: &Array, y: &Array, tol: Tol, span: Span) -> Result<Array> {
    let radix = digits_of(x, "encode", span)?;
    let values = digits_of(y, "encode", span)?;
    let k = radix.len();
    let n = values.len();
    let mut out = vec![0.0f64; k * n];
    let mut cell = vec![0.0f64; k];
    for (j, &v) in values.iter().enumerate() {
        encode_one(&radix, v, &mut cell, tol);
        // Each digit is a residue, so a digit with no value is refused
        // where the residue itself would be: `5 #: _` has none.
        if cell.iter().any(|&d| tol.made_nan(d, v, 0.0)) {
            return Err(Error::nan(
                format!("`{}` has no digits in this base", j_number(v)),
                span,
            ));
        }
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
fn encode_bits(y: &Array, tol: Tol, span: Span) -> Result<Array> {
    let values = digits_of(y, "encode", span)?;
    let k = bit_width(&values, span)?;
    let radix = vec![2.0; k];
    let mut out = vec![0.0f64; values.len() * k];
    for (j, &v) in values.iter().enumerate() {
        encode_one(&radix, v, &mut out[j * k..(j + 1) * k], tol);
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

/// One application of the APL kind: between two ITEMS.
///
/// APL hands a function the contents of an item, not the item, and puts a
/// result that is not a simple scalar back under an enclosure so it can take
/// one place in the array being built. J leaves its boxes shut instead,
/// which is where the two languages part on `∘.⌽` and on `,/`.
fn item_dyad(u: &Verb, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let r = u.dyad(&open_cell(x), &open_cell(y), ctx, span)?;
    Ok(enclose(&r, Enclose::ExceptSimpleScalar))
}

/// `x ∘.u y` (APL): u between every element of x and every element of y.
///
/// The elements are atoms whatever u's rank — `1 2∘.,3 4` is a 2-by-2 table
/// of pairs, not one catenation — and each is disclosed on the way in, so
/// `¯1 0 1∘.⌽⊂m` rotates the matrix rather than the enclosure holding it.
/// The result of each application is enclosed again unless it is already a
/// simple scalar.
fn outer_product(u: &Verb, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let mut frame = x.shape.clone();
    frame.extend_from_slice(&y.shape);
    let (nx, ny) = (x.count(), y.count());
    let n = nx * ny;
    if n == 0 {
        return assemble(&frame, Vec::new(), span);
    }
    let (xr, yr) = (x.to_row_major(), y.to_row_major());
    let cells = each_cell(n, nx.max(ny).max(n), u.is_pure(), ctx, |i, c| {
        item_dyad(u, &atom(&xr, i / ny), &atom(&yr, i % ny), c, span)
    })?;
    assemble_items(&frame, cells, span)
}

/// `x u/ y`: u applied to every pair of cells, x's frame before y's.
///
/// The cells are the ones u's own ranks ask for, which is why `1 2 3 +/ 10 20`
/// is a 3-by-2 table (atoms both sides) while `x ,/ y` is a single catenation
/// (`,` takes its arguments whole). APL spells the same table `∘.u` and reads
/// it by items instead, so that is where its dyad goes.
fn table(u: &Verb, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    if ctx.cfg.rules.lang == crate::Lang::Apl {
        return outer_product(u, x, y, ctx, span);
    }
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

/// The same verb with a different index origin — APL's `f⍠('IO' n)`.
///
/// The origin is a dialect setting, resolved into the primitives when the
/// program is compiled, so overriding it for one application means deriving
/// the verb again with the other value. None where the verb has no origin
/// to change, which is what makes `⎕IO` not one of its options.
pub(crate) fn with_origin(v: &Verb, origin: i64) -> Option<Verb> {
    match v {
        Verb::Prim(p) => {
            let mut out = *p;
            let mut changed = false;
            out.monad = match p.monad {
                MonadOp::GradeUp { .. } => {
                    changed = true;
                    MonadOp::GradeUp { origin }
                }
                MonadOp::GradeDown { .. } => {
                    changed = true;
                    MonadOp::GradeDown { origin }
                }
                MonadOp::IotaApl { .. } => {
                    changed = true;
                    MonadOp::IotaApl { origin }
                }
                MonadOp::Indices { boxed_coords, .. } => {
                    changed = true;
                    MonadOp::Indices { origin, boxed_coords }
                }
                MonadOp::Roll { fixed, float_at_zero, .. } => {
                    changed = true;
                    MonadOp::Roll { origin, fixed, float_at_zero }
                }
                other => other,
            };
            out.dyad = match p.dyad {
                DyadOp::IndexOf { vector_left, .. } => {
                    changed = true;
                    DyadOp::IndexOf { origin, vector_left }
                }
                DyadOp::IndexOfLast { .. } => {
                    changed = true;
                    DyadOp::IndexOfLast { origin }
                }
                DyadOp::CollateGrade { down, .. } => {
                    changed = true;
                    DyadOp::CollateGrade { down, origin }
                }
                DyadOp::Squad { leading, .. } => {
                    changed = true;
                    DyadOp::Squad { origin, leading }
                }
                DyadOp::Pick { .. } => {
                    changed = true;
                    DyadOp::Pick { origin }
                }
                DyadOp::SelectAxis { axis, rank, .. } => {
                    changed = true;
                    DyadOp::SelectAxis { axis, rank, origin }
                }
                DyadOp::Deal { fixed, .. } => {
                    changed = true;
                    DyadOp::Deal { origin, fixed }
                }
                other => other,
            };
            changed.then_some(Verb::Prim(out))
        }
        Verb::Rank(u, r) => Some(Verb::Rank(Box::new(with_origin(u, origin)?), *r)),
        Verb::Reduce(u) => Some(Verb::Reduce(Box::new(with_origin(u, origin)?))),
        Verb::NWise(u) => Some(Verb::NWise(Box::new(with_origin(u, origin)?))),
        Verb::Windowed(u, k) => Some(Verb::Windowed(Box::new(with_origin(u, origin)?), *k)),
        Verb::Commute(u) => Some(Verb::Commute(Box::new(with_origin(u, origin)?))),
        Verb::Each(u, e) => Some(Verb::Each(Box::new(with_origin(u, origin)?), *e)),
        Verb::Fit(u, n) => Some(Verb::Fit(Box::new(with_origin(u, origin)?), *n)),
        Verb::AlongAxis(u, k) => Some(Verb::AlongAxis(Box::new(with_origin(u, origin)?), *k)),
        _ => None,
    }
}

// ------------------------------------------------------- inner product

/// The scalar operation a bare primitive performs dyadically, for the fast
/// paths that recognise `+` and `*` rather than applying them.
fn scalar_dyad_of(v: &Verb) -> Option<ScalarDyad> {
    match v {
        Verb::Prim(p) => match p.dyad {
            DyadOp::Scalar(op) => Some(op),
            _ => None,
        },
        _ => None,
    }
}

/// True where the verb folds a list with one scalar operation, which is
/// what `+/` and `∧/` are and what the matrix product's fast path needs.
fn folds_with(u: &Verb, op: ScalarDyad) -> bool {
    matches!(u, Verb::Reduce(inner) if scalar_dyad_of(inner) == Some(op))
}

/// `x u . v y`: the inner product.
///
/// x is taken in cells at v's dyadic left rank, or at rank 1 where that is
/// smaller — the rule that makes `+/ . *` a matrix product and leaves a
/// whole-argument v (`,`, `,:`) reading the whole of x. Each cell meets the
/// WHOLE of y under v, and u folds what comes back.
fn inner_product(
    u: &Verb,
    v: &Verb,
    apl: bool,
    x: &Array,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    if let Some(a) = matrix_product(u, v, x, y, span) {
        return Ok(a);
    }
    // APL pairs each row of x with each COLUMN of y, which parts from J's
    // reading exactly where v does not apply to atoms.
    if apl && scalar_dyad_of(v).is_none() {
        return apl_inner_product(u, v, x, y, ctx, span);
    }
    if !apl {
        return inner_cells(u, v, false, x, y, ctx, span);
    }
    // A scalar v pairs one element of the row with one element of the
    // column, which is the leading-axis pairing J spells out and APL's own
    // conformability rule — about whole applications — does not describe.
    // The definition asks for that pairing, so the inner application runs
    // under it and the caller's rule is put back afterwards.
    let saved = ctx.cfg.agreement;
    ctx.cfg.agreement = Agreement::LeadingPrefix;
    let out = inner_cells(u, v, true, x, y, ctx, span);
    ctx.cfg.agreement = saved;
    out
}

/// Every element enclosed once more, which is what an each does to the
/// values it brings back. A simple array is all simple scalars and cannot
/// be nested any further, so it is returned as it stands.
fn enclose_elements(a: &Array) -> Array {
    if a.dtype() == DType::Box { boxed_elements(a) } else { a.clone() }
}

/// The fold that closes the cells of an inner product.
///
/// APL's definition is `f/¨ (⊂[last]x) ∘.g (⊂[first]y)`: the each is part of
/// it, so what the fold makes of one pair is enclosed unless it is already a
/// simple scalar. `1 2+.×3 4` is a number either way; `1 2,.+3 4` is an
/// enclosed vector, and only APL says so.
fn inner_fold(
    u: &Verb,
    apl: bool,
    inner: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    let folded = u.monad(inner, ctx, span)?;
    Ok(if apl { enclose_elements(&folded) } else { folded })
}

/// The inner product by the cell machinery: x's cells at v's dyadic left
/// rank, or at rank 1 where that is smaller, each against the whole of y.
fn inner_cells(
    u: &Verb,
    v: &Verb,
    apl: bool,
    x: &Array,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    let cell_rank = effective_rank(v.ranks()[1].max(1), x.rank());
    let frame_rank = x.rank() - cell_rank;
    if frame_rank == 0 {
        let inner = v.dyad(x, y, ctx, span)?;
        return inner_fold(u, apl, &inner, ctx, span);
    }
    let frame = x.shape[..frame_rank].to_vec();
    let n: usize = frame.iter().product();
    if n == 0 {
        return assemble(&frame, Vec::new(), span);
    }
    let work = x.count().max(y.count());
    let pure = u.is_pure() && v.is_pure();
    let cells = each_cell(n, work, pure, ctx, |i, c| {
        let inner = v.dyad(&x.cell_at(frame_rank, i), y, c, span)?;
        inner_fold(u, apl, &inner, c, span)
    })?;
    assemble(&frame, cells, span)
}

/// APL's `f.g` where g is not a scalar function: every vector along x's
/// LAST axis meets every vector along y's FIRST axis, and f folds each
/// result. With a scalar g this is the same as J's reading, which is the
/// path that runs it.
fn apl_inner_product(
    u: &Verb,
    v: &Verb,
    x: &Array,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    // A scalar argument stands for as many copies of itself as the other
    // side's shared axis asks for; two scalars share an axis of one.
    let k = match (x.rank(), y.rank()) {
        (0, 0) => 1,
        (0, _) => y.shape[0],
        _ => x.shape[x.rank() - 1],
    };
    if x.rank() > 0 && y.rank() > 0 && x.shape[x.rank() - 1] != y.shape[0] {
        return Err(Error::new(
            ErrorKind::Length,
            format!("inner product over {} and {} elements", x.shape[x.rank() - 1], y.shape[0]),
            Some(span),
        ));
    }
    let lead: &[usize] = if x.rank() > 0 { &x.shape[..x.rank() - 1] } else { &[] };
    let trail: &[usize] = if y.rank() > 0 { &y.shape[1..] } else { &[] };
    let rows: usize = lead.iter().product();
    let cols: usize = trail.iter().product();
    let mut frame = lead.to_vec();
    frame.extend_from_slice(trail);
    let n = rows * cols;
    if n == 0 {
        return assemble(&frame, Vec::new(), span);
    }
    let vector = |d: &Data, at: &dyn Fn(usize) -> usize| {
        let mut out = Data::empty(d.dtype());
        for t in 0..k {
            out.push_from(d, at(t));
        }
        Array::new(vec![k], out)
    };
    let pure = u.is_pure() && v.is_pure();
    let cells = each_cell(n, x.count().max(y.count()), pure, ctx, |i, c| {
        let (r, col) = (i / cols, i % cols);
        let left = vector(&x.data, &|t| if x.rank() > 0 { r * k + t } else { 0 });
        let right = vector(&y.data, &|t| if y.rank() > 0 { t * cols + col } else { 0 });
        let inner = v.dyad(&left, &right, c, span)?;
        inner_fold(u, true, &inner, c, span)
    })?;
    assemble(&frame, cells, span)
}

/// `+/ . *` (APL `+.×`) over real machine numbers: the matrix product, run
/// as a blocked pass over the two buffers instead of by the cell machinery.
/// The shape rule is the general one — x's last axis pairs with y's first —
/// so an argument of any rank comes through here. None sends the
/// application back to the general path.
fn matrix_product(u: &Verb, v: &Verb, x: &Array, y: &Array, span: Span) -> Option<Array> {
    if !folds_with(u, ScalarDyad::Add) || scalar_dyad_of(v) != Some(ScalarDyad::Mul) {
        return None;
    }
    if x.rank() == 0 || y.rank() == 0 {
        return None;
    }
    let k = x.shape[x.rank() - 1];
    if k != y.shape[0] {
        return None;
    }
    let rows: usize = x.shape[..x.rank() - 1].iter().product();
    let cols: usize = y.shape[1..].iter().product();
    let mut shape = x.shape[..x.rank() - 1].to_vec();
    shape.extend_from_slice(&y.shape[1..]);
    if crate::limits::elements(&shape, span).is_err() {
        return None;
    }
    let whole = matches!(x.dtype(), DType::Bool | DType::I64)
        && matches!(y.dtype(), DType::Bool | DType::I64);
    if whole
        && let (Some(xs), Some(ys)) = (x.to_i64_vec(), y.to_i64_vec())
        && let Some(out) = matmul_whole(&xs, &ys, rows, k, cols)
    {
        return Some(Array::new(shape, Data::I64(out.into())));
    }
    let (xs, ys) = (x.to_f64_vec()?, y.to_f64_vec()?);
    let out = par::fill_rows(rows, cols, rows * k * cols, |r0, part| {
        matmul_f64(&xs, &ys, k, cols, r0, part);
    });
    Some(Array::new(shape, Data::F64(out.into())))
}

/// Elements a block of the matrix product's inner axis covers at once: the
/// slice of y one pass over the output rows reuses. 128 rows of a 1000-wide
/// table is a megabyte, which is what a second-level cache holds.
const MATMUL_BLOCK: usize = 128;

#[inline(always)]
fn matmul_f64_body(xs: &[f64], ys: &[f64], k: usize, n: usize, r0: usize, out: &mut [f64]) {
    if n == 0 {
        return;
    }
    let rows = out.len() / n;
    for k0 in (0..k).step_by(MATMUL_BLOCK) {
        let k1 = (k0 + MATMUL_BLOCK).min(k);
        for r in 0..rows {
            let left = &xs[(r0 + r) * k..(r0 + r + 1) * k];
            let dst = &mut out[r * n..(r + 1) * n];
            for (t, &a) in left.iter().enumerate().take(k1).skip(k0) {
                let row = &ys[t * n..(t + 1) * n];
                for (o, &b) in dst.iter_mut().zip(row) {
                    *o += a * b;
                }
            }
        }
    }
}

multiversioned! {
    /// One block of output rows of a float matrix product. `out` is the
    /// block, `r0` the row it starts at; the accumulator is the output
    /// itself, which arrives zeroed.
    fn matmul_f64(
        xs: &[f64],
        ys: &[f64],
        k: usize,
        n: usize,
        r0: usize,
        out: &mut [f64],
    ) -> () = matmul_f64_body;
}

#[inline(always)]
fn matmul_i64_body(xs: &[i64], ys: &[i64], k: usize, n: usize, r0: usize, out: &mut [i64]) {
    if n == 0 {
        return;
    }
    let rows = out.len() / n;
    for k0 in (0..k).step_by(MATMUL_BLOCK) {
        let k1 = (k0 + MATMUL_BLOCK).min(k);
        for r in 0..rows {
            let left = &xs[(r0 + r) * k..(r0 + r + 1) * k];
            let dst = &mut out[r * n..(r + 1) * n];
            for (t, &a) in left.iter().enumerate().take(k1).skip(k0) {
                let row = &ys[t * n..(t + 1) * n];
                for (o, &b) in dst.iter_mut().zip(row) {
                    *o = o.wrapping_add(a.wrapping_mul(b));
                }
            }
        }
    }
}

multiversioned! {
    /// One block of output rows of an integer matrix product. Reached only
    /// where the values cannot overflow, so wrapping arithmetic is exact
    /// arithmetic here and the loop vectorises.
    fn matmul_i64(
        xs: &[i64],
        ys: &[i64],
        k: usize,
        n: usize,
        r0: usize,
        out: &mut [i64],
    ) -> () = matmul_i64_body;
}

/// The same product over integers. None where a product or a sum leaves
/// i64, which sends the whole pass to floats, as every other integer
/// primitive does.
fn matmul_whole(xs: &[i64], ys: &[i64], rows: usize, k: usize, n: usize) -> Option<Vec<i64>> {
    // A bound on the largest partial sum decides once, for the whole pass,
    // whether the plain loop can overflow at all. Where it cannot, the
    // vectorised kernel runs; where it might, the checked loop does, and
    // leaving i64 anywhere sends the whole product to floats.
    let bound = |v: &[i64]| v.iter().map(|&a| (a as i128).abs()).max().unwrap_or(0);
    if bound(xs).saturating_mul(bound(ys)).saturating_mul(k as i128) <= i64::MAX as i128 {
        return Some(par::fill_rows(rows, n, rows * k * n, |r0, part| {
            matmul_i64(xs, ys, k, n, r0, part);
        }));
    }
    let mut out = vec![0i64; rows * n];
    for r in 0..rows {
        let left = &xs[r * k..(r + 1) * k];
        let dst = &mut out[r * n..(r + 1) * n];
        for (t, &a) in left.iter().enumerate() {
            for (o, &b) in dst.iter_mut().zip(&ys[t * n..(t + 1) * n]) {
                *o = a.checked_mul(b).and_then(|p| o.checked_add(p))?;
            }
        }
    }
    Some(out)
}

/// Rows a determinant by minors is computed for at most. The recursion is
/// memoised on the set of rows still in play, so the cost is `2^n` cells
/// rather than `n!` — but it is still exponential, and past this the
/// message names the limit instead of running out of memory.
const DETERMINANT_MINORS_MAX: usize = 16;

/// `u . v y`: the determinant by minors down the FIRST column — for each
/// row in turn, that row's leading element under v with the determinant of
/// the table the row and the column leave behind, all folded by u. With no
/// columns left the value is v's identity element; with no rows left it is
/// u over nothing.
fn determinant(
    u: &Verb,
    v: &Verb,
    apl: bool,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    if apl {
        return Err(Error::domain("an inner product has no monadic meaning in APL", span));
    }
    // The determinant is of a table, so an argument of higher rank frames
    // one answer per 2-cell. Nothing above applies the rank machinery for
    // this verb: its dyad reads both arguments whole.
    if y.rank() > 2 {
        let frame = y.shape[..y.rank() - 2].to_vec();
        let n: usize = frame.iter().product();
        let pure = u.is_pure() && v.is_pure();
        let cells = each_cell(n, y.count(), pure, ctx, |i, c| {
            determinant(u, v, apl, &y.cell_at(y.rank() - 2, i), c, span)
        })?;
        return assemble(&frame, cells, span);
    }
    let rows = y.items();
    let cols = y.item_size();
    if folds_with(u, ScalarDyad::Sub)
        && scalar_dyad_of(v) == Some(ScalarDyad::Mul)
        && rows == cols
        && rows >= 3
        && matches!(y.dtype(), DType::Bool | DType::I64 | DType::F64)
        && let Some(values) = y.to_f64_vec()
    {
        return Ok(Array::scalar_f64(determinant_lu(values, rows)));
    }
    if rows > DETERMINANT_MINORS_MAX {
        return Err(Error::not_yet(
            format!(
                "a determinant of more than {DETERMINANT_MINORS_MAX} rows by minors \
                 (only -/ . * over machine numbers has a direct method)"
            ),
            span,
        ));
    }
    let mut seen: HashMap<u64, Array> = HashMap::new();
    let all = if rows == 64 { u64::MAX } else { (1u64 << rows) - 1 };
    minors(u, v, y, cols, rows, all, &mut seen, ctx, span)
}

/// One node of the expansion: the determinant of the table `left` still
/// names rows of, with the leading columns the recursion has consumed
/// already dropped.
#[allow(clippy::too_many_arguments)]
fn minors(
    u: &Verb,
    v: &Verb,
    y: &Array,
    cols: usize,
    rows: usize,
    left: u64,
    seen: &mut HashMap<u64, Array>,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    if let Some(a) = seen.get(&left) {
        return Ok(a.clone());
    }
    // One row and one column go at every step, so how many rows are left
    // says which column this node starts at.
    let column = rows - left.count_ones() as usize;
    let value = if column >= cols {
        let data = reduce_identity(v, 1, ctx.cfg.rules.lang).ok_or_else(|| {
            Error::not_yet(
                format!("the identity element of {} (a determinant with no columns)", v.name()),
                span,
            )
        })?;
        Array::new(Vec::new(), data)
    } else if left == 0 {
        u.monad(&Array::new(vec![0], Data::empty(DType::I64)), ctx, span)?
    } else {
        let mut terms = Vec::with_capacity(left.count_ones() as usize);
        for r in 0..rows {
            if left & (1 << r) == 0 {
                continue;
            }
            let minor = minors(u, v, y, cols, rows, left & !(1 << r), seen, ctx, span)?;
            let head = Array::new(Vec::new(), y.data.slice(r * cols + column, r * cols + column + 1));
            terms.push(v.dyad(&head, &minor, ctx, span)?);
        }
        let n = terms.len();
        u.monad(&assemble(&[n], terms, span)?, ctx, span)?
    };
    seen.insert(left, value.clone());
    Ok(value)
}

/// `-/ . * y` over machine numbers: the determinant by Gaussian
/// elimination with partial pivoting, which is how the reference computes
/// it from three rows up — and why its answer there is a float even where
/// every element is whole.
fn determinant_lu(mut a: Vec<f64>, n: usize) -> f64 {
    let mut det = 1.0f64;
    for c in 0..n {
        let mut pivot = c;
        for r in c + 1..n {
            if a[r * n + c].abs() > a[pivot * n + c].abs() {
                pivot = r;
            }
        }
        if a[pivot * n + c] == 0.0 {
            return 0.0;
        }
        if pivot != c {
            for j in 0..n {
                a.swap(c * n + j, pivot * n + j);
            }
            det = -det;
        }
        let head = a[c * n + c];
        det *= head;
        for r in c + 1..n {
            let factor = a[r * n + c] / head;
            if factor == 0.0 {
                continue;
            }
            for j in c..n {
                a[r * n + j] -= factor * a[c * n + j];
            }
        }
    }
    det
}

/// Monadic meaning of a primitive, applied to one cell.
fn monad_op(p: &Prim, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let apl = ctx.cfg.rules.lang == crate::Lang::Apl;
    let out = monad_op_inner(p, y, ctx, span);
    if apl { out.map(tightened_mixed) } else { out }
}

/// Every APL result passes through [`tightened_mixed`] on the way out, so
/// the mixed simple form never outlives the mixture that called for it.
fn monad_op_inner(p: &Prim, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    match p.monad {
        MonadOp::Scalar(op) => scalar_monad(op, y, ctx.cfg, span),
        MonadOp::ShapeOf => {
            Ok(carry_exact(Array::from_i64(y.shape.iter().map(|&n| n as i64).collect()), y))
        }
        MonadOp::Tally => Ok(carry_exact(Array::scalar_i64(y.items() as i64), y)),
        MonadOp::Ravel => Ok(Array::new(vec![y.count()], y.data.clone())),
        // `,. y` is one row per item, the item raveled along it. An atom is
        // one item whose ravel is one element, so it becomes a 1-by-1
        // table: `$ ,. 5` is `1 1`, which is where `,"_1` alone would stop
        // one axis short.
        MonadOp::RavelItems => {
            let (items, width) = if y.rank() == 0 {
                (1usize, 1usize)
            } else {
                (y.shape[0], y.shape[1..].iter().product::<usize>())
            };
            Ok(Array::new(vec![items, width], y.to_row_major().data))
        }
        MonadOp::TransposeAxes => Ok(transpose_axes(y)),
        MonadOp::Head => Ok(head(y)),
        MonadOp::Behead => behead(y, span),
        MonadOp::Tail => Ok(tail(y)),
        MonadOp::Curtail => Ok(curtail(y)),
        MonadOp::Reverse => Ok(reverse(y)),
        // Monadic `∪` stays nub over ITEMS at any rank, which is a
        // recorded divergence from GNU APL's vectors-only monad.
        MonadOp::Nub => Ok(nub(y, ctx.cfg.tol)),
        MonadOp::GradeUp { origin } | MonadOp::GradeDown { origin } => {
            check_gradable(y, ctx.cfg.rules, span)?;
            // APL grades the ITEMS of an array, so a scalar has none to
            // grade; J answers with the one-item permutation.
            if ctx.cfg.rules.lang == crate::Lang::Apl && y.rank() == 0 {
                return Err(Error::domain("a grade needs an array, not a scalar", span));
            }
            let down = matches!(p.monad, MonadOp::GradeDown { .. });
            let order = grade_order(y, down, Grading::of(ctx.cfg.rules, ctx.cfg.tol));
            Ok(Array::from_i64(order.iter().map(|&i| origin + i as i64).collect()))
        }
        MonadOp::IotaJ => iota_j(y, ctx.cfg.near(), span),
        MonadOp::IotaApl { origin } => iota_apl(y, origin, ctx.cfg.near(), span),
        MonadOp::Echo => {
            (ctx.out)(&format!("{}\n", crate::fmt::format_array(y, &ctx.cfg.fmt)));
            Ok(Array::empty(DType::I64))
        }
        MonadOp::ReadStream => {
            stream_number(y, 1, "1!:1 reads", span)?;
            let line = ctx.read_line(span)?;
            Ok(Array::from_chars(line.chars().collect()))
        }
        MonadOp::TypeCode => Ok(Array::scalar_i64(type_code(y))),
        MonadOp::Sparse => crate::sparse::sparsify(y, span),
        MonadOp::Same => Ok(y.clone()),
        MonadOp::Format => Ok(format_chars(y, &ctx.cfg.fmt)),
        MonadOp::DecodeBits => decode(None, y, ctx.cfg.tol, span).map(|r| carry_exact(r, y)),
        MonadOp::EncodeBits => encode_bits(y, ctx.cfg.tol, span).map(|r| carry_exact(r, y)),
        MonadOp::Itemize => {
            let mut shape = vec![1usize];
            shape.extend_from_slice(&y.shape);
            Ok(Array::new(shape, y.data.clone()))
        }
        MonadOp::TableOf => Ok(table_of(y)),
        MonadOp::Enclose(rule) => Ok(enclose(y, rule)),
        MonadOp::Open => Ok(open_cell(y)),
        MonadOp::Raze => raze(y, span),
        MonadOp::Catalogue => catalogue(y, span),
        MonadOp::AtomicRep => atomic_rep(y, ctx, span),
        MonadOp::RazeIn => raze_in(y, ctx.cfg.tol, span),
        MonadOp::First => Ok(first(y)),
        MonadOp::Enlist => enlist(y, span),
        MonadOp::Depth { signed } => {
            let d = depth(y);
            Ok(Array::scalar_i64(if signed && d > 1 && !uniform(y) { -d } else { d }))
        }
        MonadOp::Indices { origin, boxed_coords } => {
            where_indices(y, origin, boxed_coords, ctx.cfg.near(), span)
        }
        MonadOp::Steps => steps(y, span),
        MonadOp::ToExact => to_exact(y, span),
        MonadOp::NthPrime => {
            let n = y
                .to_i64_vec_near(ctx.cfg.near())
                .ok_or_else(|| Error::domain("the prime index must be an integer", span))?;
            let v = n.first().copied().unwrap_or(0);
            Ok(carry_exact(Array::scalar_i64(nth_prime(v, span)?), y))
        }
        MonadOp::PrimeFactors => {
            let n = y
                .to_i64_vec_near(ctx.cfg.near())
                .ok_or_else(|| Error::domain("prime factors need an integer", span))?;
            let v = n.first().copied().unwrap_or(0);
            Ok(carry_exact(Array::from_i64(prime_factors(v, span)?), y))
        }
        MonadOp::MatrixInverse => matrix_inverse(y, span),
        MonadOp::Roll { origin, fixed, float_at_zero } => {
            roll(y, origin, fixed, float_at_zero, ctx.cfg.near(), span)
        }
        MonadOp::ComplexParts { polar } => complex_parts(y, polar, span),
        MonadOp::SelfClassify => Ok(self_classify(y, ctx.cfg.tol)),
        MonadOp::NubSieve => Ok(nub_sieve(y, ctx.cfg.tol, ctx.cfg.rules.lang)),
        MonadOp::Unicode { pass_chars } => unicode(y, pass_chars, ctx.cfg.near(), span),
        MonadOp::Symbols => to_symbols(y, span),
        MonadOp::Words => words(y, span),
        MonadOp::LevelOf => Ok(Array::scalar_i64(boxing_level(y))),
        MonadOp::MapPaths => Ok(map_paths(y)),
        MonadOp::Nest => Ok(nest(y)),
        MonadOp::PolyRoots => poly_roots(y, span),
        MonadOp::PolyDeriv => poly_deriv(y, span),
        MonadOp::AnagramIndex => anagram_index(y, ctx.cfg.rules, span),
        MonadOp::CycleForm => cycle_form(y, ctx.cfg.near(), span),
        MonadOp::Split => Ok(split_items(y)),
        MonadOp::Execute { apl } => execute(y, apl, ctx, span),
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

fn axis_counts(x: &Array, what: &str, near: NearInt, span: Span) -> Result<Vec<i64>> {
    if x.rank() > 1 {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("{what} needs a scalar or vector left argument"),
            Some(span),
        ));
    }
    // An empty left argument asks for no axes at all, whatever type it
    // happens to carry: `'' $ y` is y's first item, not a type error.
    if x.count() == 0 {
        return Ok(Vec::new());
    }
    x.to_i64_vec_near(near)
        .ok_or_else(|| Error::domain(format!("{what} needs integer lengths"), span))
}

/// `x $ y` and `x ⍴ y` are not the same verb.
///
/// J lays out ITEMS: the result's shape is x followed by the shape of an
/// item of y, and the items are reused cyclically, so `$ 3 $ i. 3 4` is
/// `3 4` and `'' $ y` is y's first item. APL lays out ELEMENTS: the shape
/// is exactly x and y's ravel is reused. The two agree on every vector y,
/// which is why the difference shows only above rank 1.
///
/// An empty y parts them too: J refuses to invent items it was not given,
/// and APL fills with the type's fill element.
fn reshape(
    x: &Array,
    y: &Array,
    by_items: bool,
    apl: bool,
    near: NearInt,
    span: Span,
) -> Result<Array> {
    let dims = axis_counts(x, "reshape", near, span)?;
    if dims.iter().any(|&d| d < 0) {
        return Err(Error::domain("reshape lengths must be nonnegative", span));
    }
    let mut shape: Vec<usize> = dims.iter().map(|&d| d as usize).collect();
    // An item of a scalar is the scalar itself, and a scalar has one item.
    let (unit, src) = if by_items {
        let item_shape = if y.rank() == 0 { &[][..] } else { &y.shape[1..] };
        shape.extend_from_slice(item_shape);
        (item_shape.iter().product::<usize>(), y.items().max(usize::from(y.rank() == 0)))
    } else {
        (1, y.count())
    };
    let n = crate::limits::elements(&shape, span)?;
    let mut data = Data::empty(y.dtype());
    if n > 0 && src == 0 {
        if by_items {
            return Err(Error::new(ErrorKind::Length, "reshape of an empty array", Some(span)));
        }
        let fill = if apl { prototype_of(y) } else { None };
        let mut data = Data::empty(y.dtype());
        for _ in 0..n {
            push_gap(&mut data, &fill);
        }
        return Ok(Array::new(shape, data));
    }
    // Element i of the result is element `i % unit` of item
    // `(i / unit) % src`; with `unit` 1 that is the plain cyclic ravel.
    // Below `unit * src` the item index never wraps and that element is
    // element i itself, so a result the argument's own elements cover is a
    // change of shape and nothing else: the buffer comes through shared.
    if y.is_row_major() && n <= unit.saturating_mul(src) && n <= y.data.len() {
        return Ok(keep_proto(Array::new(shape, y.data.slice(0, n)), y, apl));
    }
    for i in 0..n {
        push_elem(&mut data, &y.data, (i / unit) % src * unit + i % unit);
    }
    Ok(keep_proto(Array::new(shape, data), y, apl))
}

/// A take or drop that only touches the leading axis moves a run of whole
/// items, which is a slice of the buffer rather than an element-by-element
/// walk. `keep` is the items to end up with, `from` the first of them.
fn leading_run(y: &Array, counts: &[i64], drop: bool) -> Option<Array> {
    if y.rank() == 0 || counts.is_empty() {
        return None;
    }
    // The fast path holds only while every count after the first leaves its
    // axis alone. A drop of nothing is a zero; a take of everything is the
    // axis's own length, since a take of zero empties the axis instead.
    let trailing_untouched = counts[1..].iter().enumerate().all(|(a, &c)| {
        if drop { c == 0 } else { c.unsigned_abs() as usize == y.shape[a + 1] }
    });
    if !trailing_untouched {
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

/// A count list the argument's rank cannot take. APL wants exactly one
/// count per axis; J takes fewer and leaves the rest of the axes whole, but
/// neither language takes more, and only a SCALAR right argument stretches
/// to whatever rank the list asks for.
fn count_rank(verb: &str, counts: usize, rank: usize, span: Span) -> Error {
    Error::new(
        ErrorKind::Length,
        format!("{counts} {verb} counts for a rank-{rank} argument"),
        Some(span),
    )
}

fn take(
    x: &Array,
    y: &Array,
    prototype_fill: bool,
    apl: bool,
    near: NearInt,
    span: Span,
) -> Result<Array> {
    let counts = axis_counts(x, "take", near, span)?;
    // APL overtakes a nested array with the PROTOTYPE of its first item —
    // that item's shape, with a zero for every number and a blank for every
    // character. J fills with the empty box instead.
    let fill = if prototype_fill { prototype_of(y) } else { None };
    let promoted;
    // A scalar right argument is treated as a one-item array of whatever
    // rank the count list asks for: `1 2 {. 5` is a 1 by 2 table.
    let base = if y.rank() == 0 {
        promoted = Array::new(vec![1; counts.len()], y.data.clone());
        &promoted
    } else {
        y
    };
    // J's take, unlike its drop, wants at least one count.
    let wrong = if apl {
        counts.len() != base.rank()
    } else {
        counts.len() > base.rank() || (counts.is_empty() && base.rank() > 0)
    };
    if wrong {
        return Err(count_rank("take", counts.len(), base.rank(), span));
    }
    if let Some(run) = leading_run(base, &counts, false) {
        return Ok(keep_proto(run, base, prototype_fill));
    }
    let mut out_shape = base.shape.clone();
    for (a, &k) in counts.iter().enumerate() {
        out_shape[a] = k.unsigned_abs() as usize;
    }
    let n = crate::limits::elements(&out_shape, span)?;
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
        } else if let (Data::Box(v), Some(p)) = (&mut data, &fill) {
            v.push(p.clone());
        } else {
            data.push_fill();
        }
        odometer(&mut coord, &out_shape);
    }
    Ok(keep_proto(Array::new(out_shape, data), base, prototype_fill))
}

/// APL's prototype of a nested array: the first item's own shape, with a
/// zero where it holds a number and a blank where it holds a character,
/// and the same done to each of its items where it is nested itself.
fn prototype_of(y: &Array) -> Option<Array> {
    fn zeroed(a: &Array) -> Array {
        if let Some(items) = a.as_boxes() {
            let inner: Vec<Array> = items.iter().map(zeroed).collect();
            return Array::new(a.shape.clone(), Data::Box(inner.into()));
        }
        let dtype = match a.dtype() {
            DType::Char | DType::Symbol => a.dtype(),
            _ => DType::I64,
        };
        Array::new(a.shape.clone(), fill_data(dtype, a.count()))
    }
    match y.as_boxes()?.first() {
        Some(first) => Some(zeroed(first)),
        // No item to take one from: an empty nested array remembers what
        // its items looked like, and that is already a prototype.
        None => y.proto().cloned(),
    }
}

/// An empty nested result remembers the prototype of the array it was made
/// from, so that a later fill, reshape or `↑` can answer with it rather
/// than with a bare empty box. A simple array's type already says what its
/// fills are, and J fills a nested one with the empty box whatever it held,
/// so only APL sets this.
fn keep_proto(out: Array, src: &Array, apl: bool) -> Array {
    if !apl || out.count() > 0 || out.dtype() != DType::Box {
        return out;
    }
    match prototype_of(src) {
        Some(p) => out.with_proto(p),
        None => out,
    }
}

/// Write one element of fill: the prototype where the caller worked one out
/// and the array is nested, and the type's own fill otherwise.
fn push_gap(data: &mut Data, fill: &Option<Array>) {
    match (data, fill) {
        (Data::Box(v), Some(p)) => v.push(p.clone()),
        (d, _) => d.push_fill(),
    }
}

fn drop_(x: &Array, y: &Array, apl: bool, near: NearInt, span: Span) -> Result<Array> {
    let counts = axis_counts(x, "drop", near, span)?;
    let promoted;
    let base = if y.rank() == 0 {
        promoted = Array::new(vec![1; counts.len()], y.data.clone());
        &promoted
    } else {
        y
    };
    let wrong =
        if apl { counts.len() != base.rank() } else { counts.len() > base.rank() };
    if wrong {
        return Err(count_rank("drop", counts.len(), base.rank(), span));
    }
    if let Some(run) = leading_run(base, &counts, true) {
        return Ok(keep_proto(run, base, apl));
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
    Ok(keep_proto(Array::new(out_shape, data), base, apl))
}

/// Dyadic meaning of a primitive, applied to one pair of cells.
fn dyad_op(p: &Prim, x: &Array, y: &Array, cfg: EvalCfg, span: Span) -> Result<Array> {
    let apl = cfg.rules.lang == crate::Lang::Apl;
    let out = dyad_op_inner(p, x, y, cfg, span);
    if apl { out.map(tightened_mixed) } else { out }
}

/// Every APL result passes through [`tightened_mixed`] on the way out, so
/// the mixed simple form never outlives the mixture that called for it.
fn dyad_op_inner(p: &Prim, x: &Array, y: &Array, cfg: EvalCfg, span: Span) -> Result<Array> {
    let tol = cfg.tol;
    let apl = cfg.rules.lang == crate::Lang::Apl;
    match p.dyad {
        // Reached only when a scalar verb is given non-zero cell ranks; the
        // cells then agree among themselves.
        DyadOp::Scalar(op) => scalar_dyad(op, x, y, cfg, span),
        DyadOp::Reshape => {
            let apl = cfg.rules.lang == crate::Lang::Apl;
            reshape(x, y, cfg.agreement == Agreement::LeadingPrefix, apl, cfg.near(), span)
        }
        DyadOp::Take => {
            let apl = cfg.rules.lang == crate::Lang::Apl;
            take(x, y, cfg.agreement == Agreement::ExactOrScalar, apl, cfg.near(), span)
        }
        DyadOp::Drop => drop_(x, y, cfg.rules.lang == crate::Lang::Apl, cfg.near(), span),
        DyadOp::Right => Ok(y.clone()),
        DyadOp::Left => Ok(x.clone()),
        DyadOp::Rotate => rotate(x, y, cfg.near(), span),
        DyadOp::RotateApl { last } => rotate_apl(x, y, last, cfg.near(), span),
        // Only J fills a ragged catenation; APL's conformability rule
        // refuses it, as the reference does.
        DyadOp::AppendLeading => {
            catenate(x, y, true, cfg.agreement == Agreement::LeadingPrefix, span)
        }
        DyadOp::AppendLast => {
            catenate(x, y, false, cfg.agreement == Agreement::LeadingPrefix, span)
        }
        DyadOp::IndexOf { origin, vector_left } => {
            let (x, y) = align_mixed(x, y, apl);
            index_of(&x, &y, origin, vector_left, tol, span)
        }
        DyadOp::MemberJ => Ok(member_j(x, y, tol)),
        DyadOp::MemberApl => {
            let (x, y) = align_mixed(x, y, apl);
            Ok(member_apl(&x, &y, tol))
        }
        DyadOp::From => from_index(x, y, cfg.near(), span),
        DyadOp::Match => {
            // APL tells an empty CHARACTER array from an empty numeric one
            // — their prototypes differ — where J's `-:` reads only the
            // shape once there is nothing left to compare.
            let empties_differ = cfg.rules.lang == crate::Lang::Apl
                && x.count() == 0
                && y.count() == 0
                && (x.dtype() == DType::Char) != (y.dtype() == DType::Char);
            Ok(Array::scalar_bool(!empties_differ && arrays_match(x, y, tol)))
        }
        DyadOp::NotMatch => Ok(Array::scalar_bool(!arrays_match(x, y, tol))),
        DyadOp::GradeSelect { down } => grade_select(x, y, down, cfg.rules, cfg.tol, span),
        DyadOp::Copy => {
            copy_items(x, y, cfg.agreement == Agreement::ExactOrScalar, cfg.near(), span)
        }
        DyadOp::CollateGrade { down, origin } => collate_grade(x, y, down, origin, span),
        DyadOp::TransposeJ => transpose_j(x, y, cfg.near(), span),
        DyadOp::TransposeApl => transpose_apl(x, y, cfg.rules.origin, cfg.near(), span),
        DyadOp::DecodeApl => decode_apl(x, y, span).map(|r| carry_exact2(r, x, y)),
        DyadOp::EncodeApl => encode_apl(x, y, cfg.tol, span).map(|r| carry_exact2(r, x, y)),
        DyadOp::Decode => decode(Some(x), y, cfg.tol, span).map(|r| carry_exact2(r, x, y)),
        DyadOp::Encode => encode(x, y, cfg.tol, span).map(|r| carry_exact2(r, x, y)),
        DyadOp::Laminate => laminate(x, y, span),
        DyadOp::Link => link(x, y, span),
        DyadOp::Strand => strand(x, y, span),
        DyadOp::IntervalIndex { offset, closed } => {
            interval_index(x, y, offset, closed, tol, Grading::of(cfg.rules, tol), span)
        }
        DyadOp::IndexOfLast { origin } => Ok(index_of_last(x, y, origin, tol)),
        DyadOp::MatrixDivide => matrix_divide(x, y, span),
        DyadOp::PartitionEnclose => partition_enclose(x, y, cfg.near(), span),
        DyadOp::PartitionCounts => partition_counts(x, y, cfg.near(), span),
        DyadOp::Squad { origin, leading } => squad(x, y, origin, leading, cfg.near(), span),
        DyadOp::SelectAxis { axis, rank, origin } => {
            select_axis(x, y, axis, rank, origin, cfg.near(), span)
        }
        DyadOp::Fetch => fetch(x, y, cfg.near(), span),
        DyadOp::PolyEval => poly_eval(x, y, span),
        DyadOp::PolyIntegral => poly_integral(x, y, span),
        DyadOp::TruthTable(m) => truth_table(m, x, y, span),
        DyadOp::FormatSpec => format_spec(x, y, &cfg.fmt, span),
        DyadOp::FormatSpecJ => format_spec_j(x, y, &cfg.fmt, span),
        DyadOp::ParseNumbers => parse_numbers(x, y, span),
        DyadOp::SequentialMachine => sequential_machine(x, y, span),
        DyadOp::Deal { origin, fixed } => deal(x, y, origin, fixed, cfg.near(), span),
        DyadOp::ExactForm => exact_form(x, y, cfg.near(), span),
        DyadOp::Boolean(op) => bool_dyad(op, x, y, cfg, span),
        DyadOp::Less => {
            set_rank(cfg, "without", x, y, span)?;
            let (x, y) = align_mixed(x, y, apl);
            Ok(set_less(&x, &y, tol))
        }
        DyadOp::Union => {
            set_rank(cfg, "union", x, y, span)?;
            let (x, y) = align_mixed(x, y, apl);
            union_items(&x, &y, tol, span)
        }
        DyadOp::Intersect => {
            set_rank(cfg, "intersection", x, y, span)?;
            let (x, y) = align_mixed(x, y, apl);
            Ok(intersect_items(&x, &y, tol))
        }
        DyadOp::AnagramFrom => anagram_from(x, y, cfg.near(), span),
        DyadOp::Permute => permute(x, y, cfg.near(), span),
        DyadOp::FindSeq => {
            let (x, y) = align_mixed(x, y, apl);
            find_seq(&x, &y, tol, apl, span)
        }
        DyadOp::UnicodeForm => unicode_form(x, y, cfg.near(), span),
        DyadOp::SymbolForm => symbol_form(x, y, span),
        DyadOp::SparseForm => sparse_form(x, y, cfg.near(), span),
        DyadOp::PrimeMeta => prime_meta(x, y, cfg.near(), span).map(|r| carry_exact2(r, x, y)),
        DyadOp::PrimeExponents => {
            prime_exponents(x, y, cfg.near(), span).map(|r| carry_exact2(r, x, y))
        }
        DyadOp::Pick { origin } => pick(x, y, origin, cfg.near(), span),
        DyadOp::Expand => expand(x, y, cfg.rules.lang == crate::Lang::Apl, cfg.near(), span),
        // Writing needs the output sink, which this dispatcher does not
        // carry; `dyad_cell` takes it before the call gets here.
        DyadOp::WriteStream => Err(Error::internal("1!:2 reached the pure dyad dispatcher")),
        DyadOp::NotYet(what) => Err(Error::not_yet(what, span)),
        DyadOp::None => Err(Error::domain(format!("{} has no dyadic meaning", p.name), span)),
    }
}

// ------------------------------------------------------------- reduction

/// The extreme APL reduces an empty `⌈` or `⌊` to.
///
/// The language has no infinity in its identities, and the reference does
/// not answer the exact largest double either: `⌈/⍬` is this number to
/// every digit GNU APL will show of it, and arithmetic on the answer
/// confirms the rest. J's identities are the infinities and stay so.
const APL_EXTREME: f64 = 1.7976e308;

/// The neutral cell of a reduction over no items, if the verb has one.
///
/// The values are the ones the references produce — both of them, for every
/// verb both spell (`x %: y` is J's alone). Where a table entry is
/// conventional rather than algebraic (a comparison has no true identity)
/// J and GNU APL still agree on it, so libjay follows. `⌊` and `⌈` are the
/// one place the two references part: J's neutral cells are the infinities
/// and APL's are the extremes of the representable range, so the table
/// reads the language.
fn reduce_identity(v: &Verb, n: usize, lang: crate::Lang) -> Option<Data> {
    let Verb::Prim(p) = v else { return None };
    let DyadOp::Scalar(op) = p.dyad else { return None };
    let ints = |k: i64| Data::I64(vec![k; n].into());
    let bits = |k: u8| Data::Bool(vec![k; n].into());
    let extreme =
        |sign: f64| Data::F64(vec![sign * if lang == crate::Lang::Apl { APL_EXTREME } else { f64::INFINITY }; n].into());
    Some(match op {
        ScalarDyad::Add | ScalarDyad::Sub | ScalarDyad::Gcd | ScalarDyad::Residue => ints(0),
        ScalarDyad::Mul
        | ScalarDyad::DivJ
        | ScalarDyad::DivApl
        | ScalarDyad::Pow
        | ScalarDyad::Lcm
        | ScalarDyad::Root
        | ScalarDyad::Binomial => ints(1),
        ScalarDyad::Min => extreme(1.0),
        ScalarDyad::Max => extreme(-1.0),
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
fn fold_range_body<S, T, F>(
    v: &[S],
    m: usize,
    lo: usize,
    hi: usize,
    j0: usize,
    acc: &mut [T],
    step: &F,
) -> bool
where
    S: Widen<T>,
    T: Copy,
    F: Fn(T, T) -> (T, bool),
{
    let w = acc.len();
    let base = (hi - 1) * m + j0;
    for (slot, &x) in acc.iter_mut().zip(&v[base..base + w]) {
        *slot = x.widen();
    }
    // Overflow is folded into a flag rather than breaking the loop: the
    // whole reduction is redone by the general path either way.
    let mut over = false;
    for i in (lo..hi - 1).rev() {
        let row = &v[i * m + j0..i * m + j0 + w];
        for (slot, &x) in acc.iter_mut().zip(row) {
            let (r, o) = step(x.widen(), *slot);
            *slot = r;
            over |= o;
        }
    }
    !over
}

multiversioned! {
    #[allow(clippy::too_many_arguments)]
    fn fold_range_vectorised[S: Widen<T>, T: Copy, F: Fn(T, T) -> (T, bool)](
        v: &[S],
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
///
/// The buffer is read in its own element type and promoted into the
/// accumulator's where each element is read, so a narrower argument costs
/// no widened copy.
#[allow(clippy::too_many_arguments)]
#[inline]
fn fold_range<S, T, F>(
    v: &[S],
    m: usize,
    lo: usize,
    hi: usize,
    j0: usize,
    acc: &mut [T],
    step: &F,
) -> bool
where
    S: Widen<T>,
    T: Copy,
    F: Fn(T, T) -> (T, bool),
{
    if acc.len() < VECTOR_COLUMNS {
        fold_range_body(v, m, lo, hi, j0, acc, step)
    } else {
        fold_range_vectorised(v, m, lo, hi, j0, acc, step)
    }
}

/// Independent accumulators an associative fold over a flat run keeps in
/// flight at once.
///
/// One accumulator makes the fold a chain of dependent steps — a float add
/// is four cycles on this class of machine, and nothing else can start
/// until it retires — so the loop waits on latency and leaves both the
/// pipeline and the vector registers idle. Lanes break the chain into
/// independent ones and give the autovectoriser a shape it can widen: lane
/// `j` takes every eighth element, which is a contiguous vector load.
/// Eight is two AVX2 registers of f64 and four of the complex pair.
const FOLD_LANES: usize = 8;

/// Elements below which a flat fold keeps its plain single accumulator.
///
/// Below this the lanes cost more to set up and combine than the width
/// gives back, and a short fold keeps exactly the rounding it always had.
const MIN_LANE_WORK: usize = 8 * FOLD_LANES;

/// Fold a flat run right to left with [`FOLD_LANES`] accumulators, the
/// lanes combined right to left at the end and the leading remainder folded
/// into the result last — so the fold is a regrouping of the sequential one,
/// which only an associative step may take (§5.9).
#[inline(always)]
fn fold_lanes_body<S, T, F>(v: &[S], step: &F) -> Option<T>
where
    S: Widen<T>,
    T: Copy,
    F: Fn(T, T) -> (T, bool),
{
    let n = v.len();
    let mut over = false;
    if n < MIN_LANE_WORK {
        let mut acc = v[n - 1].widen();
        for &x in v[..n - 1].iter().rev() {
            let (r, o) = step(x.widen(), acc);
            acc = r;
            over |= o;
        }
        return (!over).then_some(acc);
    }
    // The lanes cover a whole number of rows at the end of the run; `head`
    // is what is left over at the front.
    let rows = n / FOLD_LANES;
    let head = n - rows * FOLD_LANES;
    let last = head + (rows - 1) * FOLD_LANES;
    let mut acc = [v[last].widen(); FOLD_LANES];
    for (slot, &x) in acc.iter_mut().zip(&v[last..last + FOLD_LANES]) {
        *slot = x.widen();
    }
    for r in (0..rows - 1).rev() {
        let row = &v[head + r * FOLD_LANES..head + (r + 1) * FOLD_LANES];
        for (slot, &x) in acc.iter_mut().zip(row) {
            let (r, o) = step(x.widen(), *slot);
            *slot = r;
            over |= o;
        }
    }
    let mut a = acc[FOLD_LANES - 1];
    for &x in acc[..FOLD_LANES - 1].iter().rev() {
        let (r, o) = step(x, a);
        a = r;
        over |= o;
    }
    for &x in v[..head].iter().rev() {
        let (r, o) = step(x.widen(), a);
        a = r;
        over |= o;
    }
    (!over).then_some(a)
}

multiversioned! {
    fn fold_lanes_vectorised[S: Widen<T>, T: Copy, F: Fn(T, T) -> (T, bool)](
        v: &[S],
        step: &F,
    ) -> Option<T> = fold_lanes_body;
}

/// A flat run folded with lanes where they pay and with one accumulator
/// where they do not.
#[inline]
fn fold_lanes<S, T, F>(v: &[S], step: &F) -> Option<T>
where
    S: Widen<T>,
    T: Copy,
    F: Fn(T, T) -> (T, bool),
{
    if v.len() < MIN_LANE_WORK {
        fold_lanes_body(v, step)
    } else {
        fold_lanes_vectorised(v, step)
    }
}

/// Fold `n` single-element items, right to left. Associative steps fold in
/// chunks on several threads, and in lanes within a chunk.
fn fold_flat<S, T, F>(v: &[S], n: usize, assoc: bool, step: &F) -> Option<T>
where
    S: Widen<T>,
    T: Copy + Send + Sync,
    F: Fn(T, T) -> (T, bool) + Sync + Send,
{
    if assoc {
        return par::try_fold_chunks(
            &v[..n],
            |part| fold_lanes(part, step),
            |a, b| {
                let (r, o) = step(a, b);
                (!o).then_some(r)
            },
        );
    }
    let mut acc = v[n - 1].widen();
    let mut over = false;
    for &x in v[..n - 1].iter().rev() {
        let (r, o) = step(x.widen(), acc);
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
fn fold_items<S, T, F>(v: &[S], n: usize, m: usize, assoc: bool, step: F) -> Option<Vec<T>>
where
    S: Widen<T>,
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

/// One step of a blockwise float fold, scan or window.
///
/// A NaN abandons the block, exactly as an integer overflow does, and the
/// general path redoes the fold one pair at a time. That is where the
/// dialect's rules live — J's `*/ 0 , _` is 0 and its `+/ _ , __` is
/// refused, and each of those is an IEEE NaN — so the blockwise form never
/// has to carry them, and never answers differently from the plain one. An
/// infinity is an ordinary value and stays in the block. Ordinary data
/// takes this road once per fold and finds nothing.
#[inline(always)]
fn block_f64(r: f64) -> (f64, bool) {
    (r, r.is_nan())
}

/// The integer fold, over any buffer whose elements are integers once read:
/// an `i64` one, or a boolean one promoted where it is read.
fn fold_i64<S: Widen<i64>>(op: ScalarDyad, v: &[S], n: usize, m: usize) -> Option<Vec<i64>> {
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
        Add => fold_items(v, n, m, assoc, |a: f64, b: f64| block_f64(a + b)),
        Sub => fold_items(v, n, m, assoc, |a: f64, b: f64| block_f64(a - b)),
        Mul => fold_items(v, n, m, assoc, |a: f64, b: f64| block_f64(a * b)),
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
        // general path would produce. The promotion happens where the fold
        // reads the element, so the boolean buffer is folded where it lies.
        Data::Bool(v) => Some(Data::I64(fold_i64(op, v.as_slice(), n, m)?.into())),
        // A bignum has no blockwise form: the exact types fold, scan and
        // window through the general path, one step at a time.
        Data::Ext(_) | Data::Rat(_) | Data::Char(_) | Data::Symbol(_) | Data::Box(_) => None,
    }
}

/// Fold each run of `m` consecutive elements into one, right to left.
///
/// This is the reduction of a vector cell, done for every cell of the frame
/// at once. Each run is folded on its own, in the order the insert has, so
/// no step is regrouped and any operation at all is safe here.
#[inline(always)]
fn fold_runs_body<S, T, F>(v: &[S], start: usize, m: usize, out: &mut [T], step: &F) -> bool
where
    S: Widen<T>,
    T: Copy,
    F: Fn(T, T) -> (T, bool),
{
    let mut over = false;
    for (k, slot) in out.iter_mut().enumerate() {
        let run = &v[(start + k) * m..(start + k + 1) * m];
        let mut acc = run[m - 1].widen();
        for &x in run[..m - 1].iter().rev() {
            let (r, o) = step(x.widen(), acc);
            acc = r;
            over |= o;
        }
        *slot = acc;
    }
    !over
}

multiversioned! {
    fn fold_runs_vectorised[S: Widen<T>, T: Copy, F: Fn(T, T) -> (T, bool)](
        v: &[S],
        start: usize,
        m: usize,
        out: &mut [T],
        step: &F,
    ) -> bool = fold_runs_body;
}

/// One output per run of `m`, in parallel over the runs. None when a step
/// left the element type: the general path then runs and knows how to widen.
fn fold_runs<S, T, F>(v: &[S], n: usize, m: usize, step: F) -> Option<Vec<T>>
where
    S: Widen<T>,
    T: Copy + Default + Send + Sync,
    F: Fn(T, T) -> (T, bool) + Sync + Send,
{
    // A run is the loop a vector clone would widen, so a short run takes the
    // baseline compilation — the rule `VECTOR_COLUMNS` carries for the fold
    // across an item's columns, which is the same loop seen sideways.
    let wide = m >= VECTOR_COLUMNS;
    let (out, ok) = par::fill_wide(n, n * m, |start, part: &mut [T]| {
        if wide {
            fold_runs_vectorised(v, start, m, part, &step)
        } else {
            fold_runs_body(v, start, m, part, &step)
        }
    });
    ok.then_some(out)
}

fn fold_runs_data(op: ScalarDyad, d: &Data, n: usize, m: usize) -> Option<Data> {
    use ScalarDyad::*;
    match d {
        Data::F64(v) => Some(Data::F64(
            match op {
                Add => fold_runs(v, n, m, |a: f64, b: f64| block_f64(a + b)),
                Sub => fold_runs(v, n, m, |a: f64, b: f64| block_f64(a - b)),
                Mul => fold_runs(v, n, m, |a: f64, b: f64| block_f64(a * b)),
                Min => fold_runs(v, n, m, |a: f64, b: f64| (a.min(b), false)),
                Max => fold_runs(v, n, m, |a: f64, b: f64| (a.max(b), false)),
                _ => None,
            }?
            .into(),
        )),
        Data::I64(v) => Some(Data::I64(fold_runs_i64(op, v.as_slice(), n, m)?.into())),
        // Min and Max have no complex meaning; the general path reports it.
        Data::Complex(v) => Some(Data::Complex(
            match op {
                Add => fold_runs(v, n, m, |a: Cx, b: Cx| (cx::add(a, b), false)),
                Sub => fold_runs(v, n, m, |a: Cx, b: Cx| (cx::sub(a, b), false)),
                Mul => fold_runs(v, n, m, |a: Cx, b: Cx| (cx::mul(a, b), false)),
                _ => None,
            }?
            .into(),
        )),
        // Booleans reduce as integers, and are promoted where they are read.
        Data::Bool(v) => Some(Data::I64(fold_runs_i64(op, v.as_slice(), n, m)?.into())),
        Data::Ext(_) | Data::Rat(_) | Data::Char(_) | Data::Symbol(_) | Data::Box(_) => None,
    }
}

/// The row fold's integer arm, over an `i64` buffer or a boolean one.
fn fold_runs_i64<S: Widen<i64>>(op: ScalarDyad, v: &[S], n: usize, m: usize) -> Option<Vec<i64>> {
    use ScalarDyad::*;
    match op {
        Add => fold_runs(v, n, m, i64::overflowing_add),
        Sub => fold_runs(v, n, m, i64::overflowing_sub),
        Mul => fold_runs(v, n, m, i64::overflowing_mul),
        Min => fold_runs(v, n, m, |a: i64, b: i64| (a.min(b), false)),
        Max => fold_runs(v, n, m, |a: i64, b: i64| (a.max(b), false)),
        _ => None,
    }
}

// ------------------------------------------------- folds over the columns
//
// A column-major buffer holds each column of the matrix contiguously, so
// the two reductions a table is asked for are both cheaper here than they
// are over rows: the leading-axis fold is one flat fold per column, and the
// row fold is one pass that reads the columns side by side. Neither
// regroups anything the row-major path does not already regroup, and
// neither materialises the transpose.

/// The `runs` runs of `len` elements a buffer holds, as slices.
///
/// A buffer that arrived as parts — one per column of an imported table —
/// hands its parts back, so reading a table column by column never makes
/// the join and never copies. Any other buffer is cut into runs, which for
/// an owned or borrowed one is free as well.
fn run_slices<T: Clone>(b: &Buf<T>, runs: usize, len: usize) -> Vec<&[T]> {
    if let Some(parts) = b.parts() && parts.len() == runs && parts.iter().all(|p| p.len() == len) {
        return parts.iter().map(Buf::as_slice).collect();
    }
    let flat = b.as_slice();
    (0..runs).map(|c| &flat[c * len..(c + 1) * len]).collect()
}

/// Fold each of `runs` contiguous runs of `len` elements into one value,
/// right to left.
///
/// A long run takes the flat fold, which keeps several accumulators in
/// flight and splits itself across threads; a short one is a run like any
/// other and takes the run fold, which parallelises across the runs
/// instead. Both fold in the insert's own order, up to the regrouping an
/// associative float fold is already allowed (§5.9).
fn fold_columns<S, T, F>(cols: &[&[S]], len: usize, assoc: bool, step: F) -> Option<Vec<T>>
where
    S: Widen<T>,
    T: Copy + Default + Send + Sync,
    F: Fn(T, T) -> (T, bool) + Sync + Send,
{
    // A column long enough to split takes the threads for itself, one
    // column at a time; a shorter one is folded whole and the split is
    // across the columns. Either way each column is folded by the flat
    // fold, which keeps its lanes and its contracted regrouping.
    if par::worth_it(len) {
        let mut out = Vec::with_capacity(cols.len());
        for c in cols {
            out.push(fold_flat(c, len, assoc, &step)?);
        }
        return Some(out);
    }
    let (out, ok) = par::fill_wide(cols.len(), cols.len() * len, |start, part: &mut [T]| {
        let mut ok = true;
        for (k, slot) in part.iter_mut().enumerate() {
            match fold_flat(cols[start + k], len, assoc, &step) {
                Some(v) => *slot = v,
                None => ok = false,
            }
        }
        ok
    });
    ok.then_some(out)
}

/// Fold every column of a column-major buffer, one value per column.
fn fold_columns_data(op: ScalarDyad, d: &Data, runs: usize, len: usize) -> Option<Data> {
    use ScalarDyad::*;
    if !matches!(op, Add | Sub | Mul | Min | Max) {
        return None;
    }
    let assoc = is_associative(op);
    macro_rules! by {
        ($v:expr, $add:expr, $sub:expr, $mul:expr, $min:expr, $max:expr) => {{
            let cols = run_slices($v, runs, len);
            match op {
                Add => fold_columns(&cols, len, assoc, $add),
                Sub => fold_columns(&cols, len, assoc, $sub),
                Mul => fold_columns(&cols, len, assoc, $mul),
                Min => fold_columns(&cols, len, assoc, $min),
                Max => fold_columns(&cols, len, assoc, $max),
                _ => None,
            }?
        }};
    }
    match d {
        Data::F64(v) => Some(Data::F64(
            by!(
                v,
                |a: f64, b: f64| block_f64(a + b),
                |a: f64, b: f64| block_f64(a - b),
                |a: f64, b: f64| block_f64(a * b),
                |a: f64, b: f64| (a.min(b), false),
                |a: f64, b: f64| (a.max(b), false)
            )
            .into(),
        )),
        Data::I64(v) => Some(Data::I64(
            by!(
                v,
                i64::overflowing_add,
                i64::overflowing_sub,
                i64::overflowing_mul,
                |a: i64, b: i64| (a.min(b), false),
                |a: i64, b: i64| (a.max(b), false)
            )
            .into(),
        )),
        Data::Complex(v) => {
            if !matches!(op, Add | Sub | Mul) {
                return None;
            }
            Some(Data::Complex(
                by!(
                    v,
                    |a: Cx, b: Cx| (cx::add(a, b), false),
                    |a: Cx, b: Cx| (cx::sub(a, b), false),
                    |a: Cx, b: Cx| (cx::mul(a, b), false),
                    |_: Cx, _: Cx| unreachable!("refused above"),
                    |_: Cx, _: Cx| unreachable!("refused above")
                )
                .into(),
            ))
        }
        // Booleans reduce as integers, which is what promotion says the
        // general path would produce; the promotion happens where the fold
        // reads the element, so the columns are folded where they lie.
        Data::Bool(v) => Some(Data::I64(
            by!(
                v,
                i64::overflowing_add,
                i64::overflowing_sub,
                i64::overflowing_mul,
                |a: i64, b: i64| (a.min(b), false),
                |a: i64, b: i64| (a.max(b), false)
            )
            .into(),
        )),
        Data::Ext(_) | Data::Rat(_) | Data::Char(_) | Data::Symbol(_) | Data::Box(_) => None,
    }
}

/// `u/ y` over a column-major argument: the leading axis is what each
/// contiguous run holds, so every run folds where it lies and no transpose
/// is made. None means the verb, the type or the shape is not one this
/// covers.
fn reduce_columns(v: &Verb, y: &Array) -> Option<Array> {
    let Verb::Prim(p) = v else { return None };
    let DyadOp::Scalar(op) = p.dyad else { return None };
    if !y.dtype().is_numeric() {
        return None;
    }
    let n = y.shape[0];
    let m: usize = y.shape[1..].iter().product();
    // An empty leading axis reduces to the operation's identity, which the
    // general path knows and this one does not.
    if n == 0 || m == 0 {
        return None;
    }
    let shape = y.shape[1..].to_vec();
    // One item reduces to that item, type and all: the insert never runs.
    // The trailing axes lie column-major, which is what the result keeps.
    if n == 1 {
        return Some(Array::col_major(shape, y.data.clone()));
    }
    let data = fold_columns_data(op, &y.data, m, n)?;
    Some(Array::col_major(shape, data))
}

/// Fold the rows of a column-major matrix: one pass that reads the columns
/// side by side, each row folded right to left in the insert's own order.
fn fold_across<S, T, F>(cols: &[&[S]], rows: usize, step: F) -> Option<Vec<T>>
where
    S: Widen<T>,
    T: Copy + Default + Send + Sync,
    F: Fn(T, T) -> (T, bool) + Sync + Send,
{
    let (last, rest) = cols.split_last()?;
    let (out, ok) = par::fill(rows, |start, part: &mut [T]| {
        let mut over = false;
        for (k, slot) in part.iter_mut().enumerate() {
            let i = start + k;
            let mut acc = last[i].widen();
            for c in rest.iter().rev() {
                let (r, o) = step(c[i].widen(), acc);
                acc = r;
                over |= o;
            }
            *slot = acc;
        }
        !over
    });
    ok.then_some(out)
}

fn fold_across_data(op: ScalarDyad, d: &Data, rows: usize, cols: usize) -> Option<Data> {
    use ScalarDyad::*;
    if !matches!(op, Add | Sub | Mul | Min | Max) {
        return None;
    }
    macro_rules! by {
        ($v:expr, $add:expr, $sub:expr, $mul:expr, $min:expr, $max:expr) => {{
            let parts = run_slices($v, cols, rows);
            match op {
                Add => fold_across(&parts, rows, $add),
                Sub => fold_across(&parts, rows, $sub),
                Mul => fold_across(&parts, rows, $mul),
                Min => fold_across(&parts, rows, $min),
                Max => fold_across(&parts, rows, $max),
                _ => None,
            }?
        }};
    }
    match d {
        Data::F64(v) => Some(Data::F64(
            by!(
                v,
                |a: f64, b: f64| block_f64(a + b),
                |a: f64, b: f64| block_f64(a - b),
                |a: f64, b: f64| block_f64(a * b),
                |a: f64, b: f64| (a.min(b), false),
                |a: f64, b: f64| (a.max(b), false)
            )
            .into(),
        )),
        Data::I64(v) => Some(Data::I64(
            by!(
                v,
                i64::overflowing_add,
                i64::overflowing_sub,
                i64::overflowing_mul,
                |a: i64, b: i64| (a.min(b), false),
                |a: i64, b: i64| (a.max(b), false)
            )
            .into(),
        )),
        Data::Complex(v) => {
            if !matches!(op, Add | Sub | Mul) {
                return None;
            }
            Some(Data::Complex(
                by!(
                    v,
                    |a: Cx, b: Cx| (cx::add(a, b), false),
                    |a: Cx, b: Cx| (cx::sub(a, b), false),
                    |a: Cx, b: Cx| (cx::mul(a, b), false),
                    |_: Cx, _: Cx| unreachable!("refused above"),
                    |_: Cx, _: Cx| unreachable!("refused above")
                )
                .into(),
            ))
        }
        // Read as integers where each element is read, as everywhere else.
        Data::Bool(v) => Some(Data::I64(
            by!(
                v,
                i64::overflowing_add,
                i64::overflowing_sub,
                i64::overflowing_mul,
                |a: i64, b: i64| (a.min(b), false),
                |a: i64, b: i64| (a.max(b), false)
            )
            .into(),
        )),
        Data::Ext(_) | Data::Rat(_) | Data::Char(_) | Data::Symbol(_) | Data::Box(_) => None,
    }
}

/// `u/"1 y` over a column-major matrix: every row folded across the
/// columns, without the transpose the row-major path would need first.
fn reduce_rows_columns(u: &Verb, y: &Array) -> Option<Array> {
    let Verb::Reduce(inner) = u else { return None };
    let Verb::Prim(p) = &**inner else { return None };
    let DyadOp::Scalar(op) = p.dyad else { return None };
    // Only a matrix: at higher rank the cells this folds are not the runs
    // the buffer holds.
    if y.rank() != 2 || !y.dtype().is_numeric() {
        return None;
    }
    let (rows, cols) = (y.shape[0], y.shape[1]);
    // An empty cell reduces to the operation's identity, which the general
    // path knows and this one does not.
    if rows == 0 || cols == 0 {
        return None;
    }
    if cols == 1 {
        // A cell of one element reduces to that element, type and all.
        return Some(Array::new(vec![rows], y.data.clone()));
    }
    let data = fold_across_data(op, &y.data, rows, cols)?;
    Some(Array::new(vec![rows], data))
}

/// `u/"1 y` and its like: a reduction whose cells are vectors, answered by
/// folding every cell out of the one buffer.
///
/// The rank machinery would build an array per cell, reduce it, and frame
/// the results — three allocations for every row of a matrix. This produces
/// exactly what that produces, and reads the buffer once. None means the
/// shape, the verb or the type is not one this covers, and the general path
/// runs instead.
fn reduce_vector_cells(u: &Verb, y: &Array, frame_rank: usize) -> Option<Array> {
    let Verb::Reduce(inner) = u else { return None };
    let Verb::Prim(p) = &**inner else { return None };
    let DyadOp::Scalar(op) = p.dyad else { return None };
    // The cell is a vector, so its reduction is a scalar and the result has
    // the frame's own shape.
    if y.rank() != frame_rank + 1 || !y.dtype().is_numeric() {
        return None;
    }
    let m = y.shape[frame_rank];
    // An empty cell reduces to the operation's identity, which the general
    // path knows and this one does not.
    if m == 0 {
        return None;
    }
    use ScalarDyad::{Add, Max, Min, Mul, Sub};
    if !matches!(op, Add | Sub | Mul | Min | Max) {
        return None;
    }
    let frame = y.shape[..frame_rank].to_vec();
    if m == 1 {
        // A cell of one element reduces to that element, type and all: the
        // insert never runs, so nothing widens.
        return Some(Array::new(frame, y.data.clone()));
    }
    let n: usize = frame.iter().product();
    let data = fold_runs_data(op, &y.data, n, m)?;
    Some(Array::new(frame, data))
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
        // Catenation's identity is the empty LIST, whatever shape the cells
        // that were not there would have had: `,/ i. 0 3` is `i. 0`.
        if matches!(v, Verb::Prim(p) if matches!(p.dyad, DyadOp::AppendLeading | DyadOp::AppendLast))
        {
            return Ok(Array::new(vec![0], Data::empty(y.dtype())));
        }
        return match reduce_identity(v, m, ctx.cfg.rules.lang) {
            Some(d) => Ok(Array::new(cell_shape, d)),
            None => Err(Error::domain(
                format!("empty reduction has no identity for {}", v.name()),
                span,
            )),
        };
    }
    if y.dtype().is_numeric() && let Verb::Prim(p) = v && let DyadOp::Scalar(op) = p.dyad {
        // The typed fold covers the arithmetic reductions and runs
        // in parallel wherever the fold order allows; it declines
        // (integer overflow, an operation with its own type rules)
        // by returning None, and then the general fold below runs.
        if let Some(d) = reduce_typed(op, y.row_major_data(), n, m) {
            return Ok(Array::new(cell_shape, d));
        }
        // Fold over the raw buffer, one whole item per step, without
        // materialising item arrays.
        let mut acc = y.data.slice((n - 1) * m, n * m);
        for i in (0..n - 1).rev() {
            acc =
                scalar_dyad_data(
                    op,
                    &y.data,
                    i * m,
                    1,
                    &acc,
                    0,
                    1,
                    m,
                    ctx.cfg.tol,
                    ctx.cfg.rules,
                    span,
                )?;
        }
        return Ok(Array::new(cell_shape, acc));
    }
    if ctx.cfg.rules.lang == crate::Lang::Apl {
        return item_fold(v, y, ctx, span);
    }
    let mut acc = y.item(n - 1);
    for i in (0..n - 1).rev() {
        acc = v.dyad(&y.item(i), &acc, ctx, span)?;
    }
    Ok(acc)
}

/// The same insert read by items, which is what APL's `f/` and `f⌿` are.
///
/// J folds whole cells: `,/ 2 3$i.6` catenates the two rows. APL folds the
/// ELEMENTS along the reduced axis and leaves the other axes as the frame,
/// so `,⌿2 3⍴⍳6` pairs the columns and answers three two-element vectors.
/// Each element is disclosed on the way in and the fold's value is enclosed
/// on the way out, which is why `,/1 2 3` is an enclosed vector rather than
/// a bare one. The arithmetic reductions never reach here: folding atoms
/// and folding cells agree for a scalar function, and the typed path above
/// keeps them.
fn item_fold(v: &Verb, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let n = y.items();
    let frame = y.shape[1..].to_vec();
    let m: usize = frame.iter().product();
    if m == 0 {
        return assemble(&frame, Vec::new(), span);
    }
    let base = y.to_row_major();
    let cells = each_cell(m, base.count(), v.is_pure(), ctx, |p, c| {
        let mut acc = open_cell(&atom(&base, (n - 1) * m + p));
        for i in (0..n - 1).rev() {
            acc = v.dyad(&open_cell(&atom(&base, i * m + p)), &acc, c, span)?;
        }
        Ok(enclose(&acc, Enclose::ExceptSimpleScalar))
    })?;
    assemble_items(&frame, cells, span)
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
fn scan_flat_body<S, T, F>(v: &[S], n: usize, m: usize, back: bool, step: F) -> Option<Vec<T>>
where
    S: Widen<T>,
    T: Copy + Default,
    F: Fn(T, T) -> (T, bool),
{
    if m == 1 {
        // One element per item is the shape a time series has, and it is
        // the one worth keeping the accumulator in a register for.
        let mut out = vec![T::default(); n];
        let mut over = false;
        if back {
            let mut acc = v[n - 1].widen();
            out[n - 1] = acc;
            for (slot, &x) in out[..n - 1].iter_mut().zip(&v[..n - 1]).rev() {
                let (r, o) = step(x.widen(), acc);
                acc = r;
                over |= o;
                *slot = acc;
            }
        } else {
            let mut acc = v[0].widen();
            out[0] = acc;
            for (slot, &x) in out[1..n].iter_mut().zip(&v[1..n]) {
                let (r, o) = step(acc, x.widen());
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
        for (slot, &x) in acc.iter_mut().zip(&v[(n - 1) * m..n * m]) {
            *slot = x.widen();
        }
        out[(n - 1) * m..n * m].copy_from_slice(&acc);
        for i in (0..n - 1).rev() {
            for (j, slot) in acc.iter_mut().enumerate() {
                let (r, o) = step(v[i * m + j].widen(), *slot);
                *slot = r;
                over |= o;
            }
            out[i * m..i * m + m].copy_from_slice(&acc);
        }
    } else {
        for (slot, &x) in acc.iter_mut().zip(&v[..m]) {
            *slot = x.widen();
        }
        out[..m].copy_from_slice(&acc);
        for i in 1..n {
            for (j, slot) in acc.iter_mut().enumerate() {
                let (r, o) = step(*slot, v[i * m + j].widen());
                *slot = r;
                over |= o;
            }
            out[i * m..i * m + m].copy_from_slice(&acc);
        }
    }
    (!over).then_some(out)
}

multiversioned! {
    fn scan_flat_vectorised[S: Widen<T>, T: Copy + Default, F: Fn(T, T) -> (T, bool)](
        v: &[S],
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
fn scan_flat<S, T, F>(v: &[S], n: usize, m: usize, back: bool, step: F) -> Option<Vec<T>>
where
    S: Widen<T>,
    T: Copy + Default,
    F: Fn(T, T) -> (T, bool),
{
    if m < VECTOR_COLUMNS {
        scan_flat_body(v, n, m, back, step)
    } else {
        scan_flat_vectorised(v, n, m, back, step)
    }
}

fn scan_i64<S: Widen<i64>>(
    op: ScalarDyad,
    v: &[S],
    n: usize,
    m: usize,
    back: bool,
) -> Option<Vec<i64>> {
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

fn scan_f64<S: Widen<f64>>(
    op: ScalarDyad,
    v: &[S],
    n: usize,
    m: usize,
    back: bool,
) -> Option<Vec<f64>> {
    use ScalarDyad::*;
    match op {
        Add => scan_flat(v, n, m, back, |a: f64, b: f64| block_f64(a + b)),
        Sub => scan_flat(v, n, m, back, |a: f64, b: f64| block_f64(a - b)),
        Mul => scan_flat(v, n, m, back, |a: f64, b: f64| block_f64(a * b)),
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
    // An integer buffer and a boolean one both scan as integers, each read
    // in its own type; the float retry reads the same buffer again rather
    // than a widened copy of it.
    fn ints<S: Widen<i64> + Widen<f64>>(
        op: ScalarDyad,
        v: &[S],
        n: usize,
        m: usize,
        back: bool,
    ) -> Data {
        match scan_i64(op, v, n, m, back) {
            Some(out) => Data::I64(out.into()),
            None => Data::F64(
                scan_f64(op, v, n, m, back).expect("the float scan cannot overflow").into(),
            ),
        }
    }
    match d {
        Data::F64(v) => Some(Data::F64(scan_f64(op, v.as_slice(), n, m, back)?.into())),
        Data::Complex(v) => Some(Data::Complex(scan_cx(op, v, n, m, back)?.into())),
        Data::I64(v) => Some(ints(op, v.as_slice(), n, m, back)),
        Data::Bool(v) => Some(ints(op, v.as_slice(), n, m, back)),
        // A bignum has no blockwise form: the exact types fold, scan and
        // window through the general path, one step at a time.
        Data::Ext(_) | Data::Rat(_) | Data::Char(_) | Data::Symbol(_) | Data::Box(_) => None,
    }
}

/// The constant `c` of an affine step `x u y = x + c * y`, when the verb is
/// exactly that tree and `c` is a scalar noun written in the source.
///
/// The two spellings of a first-order recurrence are `[ + c * ]` and its
/// mirror `(c * ]) + [`. The match is on the tree, so a verb that computes
/// the same thing another way is not one of them and folds the general way.
fn affine_step(u: &Verb) -> Option<&Array> {
    // The ranks are part of the match: arithmetic pairs atoms and `[` and
    // `]` take whole arguments, and a verb wearing any other rank is a
    // different verb.
    fn prim(v: &Verb, want: DyadOp, ranks: [i64; 3]) -> bool {
        matches!(v, Verb::Prim(p) if p.dyad == want && p.ranks == ranks)
    }
    const ATOMS: [i64; 3] = [0, 0, 0];
    const WHOLE: [i64; 3] = [RANK_INF; 3];
    // `c * ]`: the accumulator scaled by the constant, and nothing else.
    fn scaled(v: &Verb) -> Option<&Array> {
        let Verb::NounFork(c, g, h) = v else { return None };
        let noun = c.rank() == 0
            && matches!(c.dtype(), DType::Bool | DType::I64 | DType::F64 | DType::Complex);
        let tree = prim(g, DyadOp::Scalar(ScalarDyad::Mul), ATOMS)
            && prim(h, DyadOp::Right, WHOLE);
        (noun && tree).then_some(c)
    }
    let Verb::Fork(f, g, h) = u else { return None };
    if !prim(g, DyadOp::Scalar(ScalarDyad::Add), ATOMS) {
        return None;
    }
    if prim(f, DyadOp::Left, WHOLE) {
        scaled(h)
    } else if prim(h, DyadOp::Left, WHOLE) {
        scaled(f)
    } else {
        None
    }
}

/// The arithmetic a running affine fold needs of its element type, and the
/// test that a power of the constant is still a number.
struct Ring<T> {
    add: fn(T, T) -> T,
    mul: fn(T, T) -> T,
    one: T,
    finite: fn(T) -> bool,
}

/// A running affine fold: `out[k] = v[k] + c * out[k+1]` backwards, and
/// forwards the same series carried the only way one pass can carry it —
/// the k-th prefix is the sum of `c^i * v[i]`, so the power of `c` runs
/// along with it. None when a power leaves the finite range, which is the
/// one case that sum and the fold it stands for do not agree on.
fn affine_flat<T>(v: &[T], c: T, n: usize, m: usize, back: bool, r: &Ring<T>) -> Option<Vec<T>>
where
    T: Copy + Default,
{
    let (add, mul) = (r.add, r.mul);
    let mut out = vec![T::default(); n * m];
    if back {
        out[(n - 1) * m..].copy_from_slice(&v[(n - 1) * m..n * m]);
        for i in (0..n - 1).rev() {
            for j in 0..m {
                out[i * m + j] = add(v[i * m + j], mul(c, out[(i + 1) * m + j]));
            }
        }
    } else {
        out[..m].copy_from_slice(&v[..m]);
        let mut pow = r.one;
        for i in 1..n {
            pow = mul(pow, c);
            if !(r.finite)(pow) {
                return None;
            }
            for j in 0..m {
                out[i * m + j] = add(out[(i - 1) * m + j], mul(pow, v[i * m + j]));
            }
        }
    }
    Some(out)
}

/// `u/\ y` and `u/\. y` over an affine step, in one pass instead of one
/// fold per run.
///
/// Backwards this is the insert's own order — the steps are the steps the
/// general path takes, in the same order, so the answer is the same to the
/// last bit. Forwards it is the same series regrouped, which rounds as the
/// blocked window fold rounds and not as the insert would. None when the
/// types are not the ones that carry it: two integers fold exactly and are
/// left alone, as are the exact types.
fn affine_scan(c: &Array, y: &Array, back: bool) -> Option<Data> {
    let (n, m) = (y.items(), y.item_size());
    let machine = |t: DType| matches!(t, DType::Bool | DType::I64 | DType::F64 | DType::Complex);
    if n == 0 || !machine(c.dtype()) || !machine(y.dtype()) {
        return None;
    }
    match DType::promote(c.dtype(), y.dtype())? {
        DType::F64 => {
            let (mut tc, mut tv) = (Vec::new(), Vec::new());
            let k = *borrow_f64(&c.data, &mut tc).first()?;
            let v = borrow_f64(y.row_major_data(), &mut tv);
            let r = Ring { add: |a, b| a + b, mul: |a, b| a * b, one: 1.0, finite: f64::is_finite };
            Some(Data::F64(affine_flat(v, k, n, m, back, &r)?.into()))
        }
        DType::Complex => {
            let (mut tc, mut tv) = (Vec::new(), Vec::new());
            let k = *borrow_cx(&c.data, &mut tc).first()?;
            let v = borrow_cx(y.row_major_data(), &mut tv);
            let finite = |z: Cx| z[0].is_finite() && z[1].is_finite();
            let r = Ring { add: cx::add, mul: cx::mul, one: [1.0, 0.0], finite };
            Some(Data::Complex(affine_flat(v, k, n, m, back, &r)?.into()))
        }
        _ => None,
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
fn window_fold<S, T, F>(v: &[S], n: usize, m: usize, w: usize, step: F) -> Option<Vec<T>>
where
    S: Widen<T>,
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
        for (slot, &x) in pre[..m].iter_mut().zip(&v[bs * m..bs * m + m]) {
            *slot = x.widen();
        }
        for i in 1..be - bs {
            let (o, p) = (i * m, (i - 1) * m);
            for j in 0..m {
                let (r, f) = step(pre[p + j], v[(bs + i) * m + j].widen());
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
        for (slot, &x) in suf[last * m..last * m + m]
            .iter_mut()
            .zip(&v[(be - 1) * m..be * m])
        {
            *slot = x.widen();
        }
        for i in (0..last).rev() {
            let (o, p) = (i * m, (i + 1) * m);
            for j in 0..m {
                let (r, f) = step(v[(bs + i) * m + j].widen(), suf[p + j]);
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
fn window_fold_flat<S, T, F>(v: &[S], n: usize, w: usize, step: F) -> Option<Vec<T>>
where
    S: Widen<T>,
    T: Copy + Default + Send + Sync,
    F: Fn(T, T) -> (T, bool) + Sync + Send,
{
    let (out, ok) = par::fill(n - w + 1, |lo, part: &mut [T]| {
        window_fold_range(v, n, w, lo, part, &step)
    });
    ok.then_some(out)
}

#[inline(always)]
fn window_fold_range_body<S, T, F>(
    v: &[S],
    n: usize,
    w: usize,
    lo: usize,
    out: &mut [T],
    step: &F,
) -> bool
where
    S: Widen<T>,
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
        let mut acc = block[0].widen();
        pre[0] = acc;
        for (slot, &x) in pre[1..lb].iter_mut().zip(&block[1..]) {
            let (r, o) = step(acc, x.widen());
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
        let mut acc = block[lb - 1].widen();
        suf[lb - 1] = acc;
        for (slot, &x) in suf[..lb - 1].iter_mut().zip(&block[..lb - 1]).rev() {
            let (r, o) = step(x.widen(), acc);
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
    fn window_fold_range[S: Widen<T>, T: Copy + Default, F: Fn(T, T) -> (T, bool)](
        v: &[S],
        n: usize,
        w: usize,
        lo: usize,
        out: &mut [T],
        step: &F,
    ) -> bool = window_fold_range_body;
}

/// The windows of `w` items of `v` that begin at `lo` and after, folded into
/// `out` — one item per window, `out.len()` of them.
///
/// The fused kernel folds the windows of a block it computed itself, and
/// calls this to do it: the blocking is counted from `v`'s own start, so a
/// caller whose buffer starts on a multiple of `w` groups every window
/// exactly as the pass over the whole argument groups it. False when a step
/// left the element type.
pub(crate) fn windows_into<S, T, F>(v: &[S], w: usize, lo: usize, out: &mut [T], step: &F) -> bool
where
    S: Widen<T>,
    T: Copy + Default,
    F: Fn(T, T) -> (T, bool),
{
    window_fold_range(v, v.len(), w, lo, out, step)
}

fn window_i64<S: Widen<i64>>(
    op: ScalarDyad,
    v: &[S],
    n: usize,
    m: usize,
    w: usize,
) -> Option<Vec<i64>> {
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

fn window_f64<S: Widen<f64>>(
    op: ScalarDyad,
    v: &[S],
    n: usize,
    m: usize,
    w: usize,
) -> Option<Vec<f64>> {
    use ScalarDyad::*;
    match op {
        Add => window_fold(v, n, m, w, |a: f64, b: f64| block_f64(a + b)),
        Mul => window_fold(v, n, m, w, |a: f64, b: f64| block_f64(a * b)),
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
    // As in the scan: integers and booleans window as integers, each read in
    // its own type, and the float retry rereads the same buffer.
    fn ints<S: Widen<i64> + Widen<f64>>(
        op: ScalarDyad,
        v: &[S],
        n: usize,
        m: usize,
        w: usize,
    ) -> Data {
        match window_i64(op, v, n, m, w) {
            Some(out) => Data::I64(out.into()),
            None => {
                Data::F64(window_f64(op, v, n, m, w).expect("the float fold cannot overflow").into())
            }
        }
    }
    match d {
        Data::F64(v) => Some(Data::F64(window_f64(op, v.as_slice(), n, m, w)?.into())),
        Data::Complex(v) => Some(Data::Complex(window_cx(op, v, n, m, w)?.into())),
        Data::I64(v) => Some(ints(op, v.as_slice(), n, m, w)),
        Data::Bool(v) => Some(ints(op, v.as_slice(), n, m, w)),
        // A bignum has no blockwise form: the exact types fold, scan and
        // window through the general path, one step at a time.
        Data::Ext(_) | Data::Rat(_) | Data::Char(_) | Data::Symbol(_) | Data::Box(_) => None,
    }
}

/// `u\ y` and `u\. y`: the verb applied to every prefix, or to every suffix.
fn runs(u: &Verb, y: &Array, back: bool, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let promoted = as_items(y);
    let base = promoted.as_ref().unwrap_or(y);
    let n = base.items();
    let m = base.item_size();
    // No items, so no runs: the answer's shape cannot come from the cells.
    // APL's scan keeps the shape it was given, whatever the function is.
    // J's takes the shape of the verb applied to the one run an empty
    // argument has, which is the argument itself: `,/\ i.0 3` is a 0 by 0
    // table where `+/\ i.0 3` is 0 by 3.
    if n == 0 {
        if ctx.cfg.rules.lang == crate::Lang::Apl {
            return Ok(Array::new(base.shape.clone(), Data::empty(base.dtype())));
        }
        let cell = u.is_pure().then(|| base.clone());
        return Ok(empty_frame(&[0], base.dtype(), cell, ctx, |cell, c| u.monad(cell, c, span)));
    }
    if n > 0 && base.dtype().is_numeric() && let Some(op) = folded_op(u) {
        // Folding from the right is the insert's own order, so it holds
        // for any step; folding from the left needs associativity.
        if (back || is_associative(op))
            && let Some(d) = scan_typed(op, base.row_major_data(), n, m, back)
        {
            return Ok(Array::new(base.shape.clone(), d));
        }
    }
    if n > 0 && let Verb::Reduce(inner) = u {
        if base.dtype().is_numeric()
            && let Some(c) = affine_step(inner)
            && let Some(d) = affine_scan(c, base, back)
        {
            return Ok(Array::new(base.shape.clone(), d));
        }
        // Suffix k is item k folded with suffix k+1, because right to left
        // is the insert's own order: one step per item, whatever the verb.
        // Prefixes have no such relation — prefix k and prefix k+1 share
        // their tail, not their head — so only this direction is a running
        // fold in general, and it is the direction `|. u/\. |. y` reverses
        // twice to reach.
        if back && u.is_pure() {
            let mut acc = base.item(n - 1);
            let mut cells = Vec::with_capacity(n);
            cells.push(acc.clone());
            for i in (0..n - 1).rev() {
                acc = inner.dyad(&base.item(i), &acc, ctx, span)?;
                cells.push(acc.clone());
            }
            cells.reverse();
            return assemble(&[n], cells, span);
        }
    }
    let apl = ctx.cfg.rules.lang == crate::Lang::Apl;
    let cells = each_cell(n, n * m, u.is_pure(), ctx, |i, c| {
        let part = if back { section(base, i, n) } else { section(base, 0, i + 1) };
        u.monad(&part, c, span)
    })?;
    if apl { assemble_items(&[n], cells, span) } else { assemble(&[n], cells, span) }
}

/// The result of a window longer than the argument holds no items, but it
/// still has the shape of one: J learns that shape by running the verb on a
/// window of fills, and so does this. A verb that fails on fills, or a
/// window too large to build, leaves the result a plain empty vector.
fn empty_windows(u: &Verb, y: &Array, w: usize, ctx: &mut Ctx<'_>, span: Span) -> Array {
    let m = y.item_size();
    if u.is_pure() && let Some(cells) = w.checked_mul(m).filter(|&s| s <= 1 << 20) {
        let mut shape = y.shape.clone();
        shape[0] = w;
        let probe = Array::new(shape, fill_data(y.dtype(), cells));
        if let Ok(cell) = u.monad(&probe, ctx, span) {
            let mut shape = vec![0usize];
            shape.extend_from_slice(&cell.shape);
            return Array::new(shape, Data::empty(cell.dtype()));
        }
    }
    Array::new(vec![0], Data::empty(DType::I64))
}

/// The window size: one integer atom.
fn window_size(x: &Array, near: NearInt, span: Span) -> Result<i64> {
    let v = x
        .to_i64_vec_near(near)
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
    let k = window_size(x, ctx.cfg.near(), span)?;
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
    if w > 0 && base.dtype().is_numeric()
        && let Some(op) = folded_op(u) && let Some(d) = window_typed(op, &base.data, n, m, w)
    {
        let mut shape = base.shape.clone();
        shape[0] = count;
        return Ok(Array::new(shape, d));
    }
    let work = count.saturating_mul(w).saturating_mul(m);
    let cells = each_cell(count, work, u.is_pure(), ctx, |i, c| {
        u.monad(&section(base, i, i + w), c, span)
    })?;
    assemble(&[count], cells, span)
}

/// `n f/ y` (APL): the reduce of every window of n items along the leading
/// axis. `f/` itself decides what folding a window means, so the operand's
/// own rules — the identity of an empty fold, the enclosure APL's insert
/// puts round a non-scalar value — carry over unchanged.
///
/// n is one integer. A positive one takes the overlapping windows in order;
/// a negative one takes the same windows with their items REVERSED, which
/// only shows on a fold that is not commutative (`¯2-/1 2 3` is `1 1` where
/// `2-/1 2 3` is `¯1 ¯1`); zero takes the `1+≢y` empty windows, so the
/// answer is that many copies of the operand's identity. The axis loses
/// `|n|-1` items, so `|n|` may reach `1+≢y` — one item further and there is
/// no such window, which is an error rather than a shorter answer.
fn nwise(f: &Verb, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    // How many numbers there are is settled before what they are: a left
    // argument of two is a length error whatever it holds, which is what
    // keeps `1 1+/2 3` from reading as a compress.
    if x.count() != 1 {
        return Err(Error::new(
            ErrorKind::Length,
            "the window size must be a single number",
            Some(span),
        ));
    }
    let k = window_size(x, ctx.cfg.near(), span)?;
    let promoted = as_items(y);
    // A rank-0 argument has no axis to window. One item is what `≢` counts
    // it as, and a window of one leaves it exactly as it was, rank included;
    // any other window has to make the axis the argument never had.
    if promoted.is_some() && k.unsigned_abs() == 1 {
        return Ok(y.clone());
    }
    let base = promoted.as_ref().unwrap_or(y).to_row_major();
    let base = &base;
    let n = base.items();
    let m = base.item_size();
    let w = k.unsigned_abs() as usize;
    if w > n + 1 {
        return Err(Error::domain(
            format!("a window of {w} does not fit an axis of {n}"),
            span,
        ));
    }
    let count = n + 1 - w;
    let fold = Verb::Reduce(Box::new(f.clone()));
    if count == 0 {
        return Ok(empty_windows(&fold, base, w, ctx, span));
    }
    // The blockwise fold the infix already has. It runs over whole items at
    // full rank, which is what folding the elements along the axis comes to
    // for the arithmetic operands it covers, and those are all commutative,
    // so a reversed window folds to the same value.
    if w > 0
        && base.dtype().is_numeric()
        && let Some(op) = scalar_dyad_of(f)
        && let Some(d) = window_typed(op, base.row_major_data(), n, m, w)
    {
        let mut shape = base.shape.clone();
        shape[0] = count;
        return Ok(Array::new(shape, d));
    }
    let work = count.saturating_mul(w.max(1)).saturating_mul(m);
    let cells = each_cell(count, work, f.is_pure(), ctx, |i, c| {
        let win = section(base, i, i + w);
        let win = if k < 0 { reverse(&win) } else { win };
        fold.monad(&win, c, span)
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
        // One answer per count. The counts are taken in the order given and
        // the walk is shared: the applications are counted from 0 upwards
        // and an answer is kept wherever a count asks for it.
        Power::Each(ref counts) => {
            let mut acc = y.clone();
            let mut done = 0u64;
            let mut order: Vec<usize> = (0..counts.len()).collect();
            order.sort_by_key(|&i| counts[i]);
            let mut cells: Vec<Option<Array>> = vec![None; counts.len()];
            for i in order {
                while done < counts[i] {
                    acc = step(&acc, ctx)?;
                    done += 1;
                }
                cells[i] = Some(acc.clone());
            }
            let cells: Vec<Array> = cells.into_iter().map(|c| c.expect("every count filled")).collect();
            assemble(&[cells.len()], cells, span)
        }
        Power::ConvergeTrace => {
            let mut acc = y.clone();
            let mut cells = vec![acc.clone()];
            for _ in 0..CONVERGE_LIMIT {
                let next = step(&acc, ctx)?;
                if arrays_match(&next, &acc, ctx.cfg.tol) {
                    return assemble(&[cells.len()], cells, span);
                }
                cells.push(next.clone());
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
        .to_i64_vec_near(ctx.cfg.near())
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
fn where_indices(y: &Array, origin: i64, boxed: bool, near: NearInt, span: Span) -> Result<Array> {
    let counts = y
        .to_i64_vec_near(near)
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
fn interval_index(
    x: &Array,
    y: &Array,
    offset: i64,
    closed: bool,
    tol: Tol,
    ord: Grading,
    span: Span,
) -> Result<Array> {
    // Characters, symbols and boxes have an order of their own, and no
    // tolerance: the bounds are searched by that order instead of by value.
    if !x.dtype().is_numeric() || !y.dtype().is_numeric() {
        return ordered_interval_index(x, y, offset, closed, ord, span);
    }
    let bounds = x
        .to_f64_vec()
        .ok_or_else(|| Error::domain("interval index needs numeric bounds", span))?;
    let vals = y
        .to_f64_vec()
        .ok_or_else(|| Error::domain("interval index needs numeric values", span))?;
    let out: Vec<i64> = vals
        .iter()
        .map(|&v| {
            // APL counts a bound EQUAL to the value, J does not: `1 3 5⍸3`
            // is 2 where `1 3 5 I. 3` is 1.
            let count =
                bounds.iter().filter(|&&b| if closed { !tol.lt(v, b) } else { tol.lt(b, v) });
            offset + count.count() as i64
        })
        .collect();
    Ok(Array::new(y.shape.clone(), Data::I64(out.into())))
}

/// [`interval_index`] over the element types that are ordered but not
/// numeric. Both sides must be the same type — a character bound has
/// nothing to say about where a symbol falls.
fn ordered_interval_index(
    x: &Array,
    y: &Array,
    offset: i64,
    closed: bool,
    ord: Grading,
    span: Span,
) -> Result<Array> {
    let (xr, yr) = (x.to_row_major(), y.to_row_major());
    let (bounds, vals) = (&xr.data, &yr.data);
    let cmp = |i: usize, j: usize| -> Option<std::cmp::Ordering> {
        match (bounds, vals) {
            (Data::Char(p), Data::Char(q)) => Some(p[i].cmp(&q[j])),
            (Data::Symbol(p), Data::Symbol(q)) => Some(crate::symbol::cmp(p[i], q[j])),
            // J orders boxed values against each other by the same total
            // order `/:` grades them with, so `I.` can search among them.
            // APL2 gives its nested values no such order, and GNU APL's own
            // is an extension libjay does not follow: see divergences.txt.
            (Data::Box(p), Data::Box(q)) if ord.tao == Tao::J => {
                Some(cmp_items_total(&p[i], &q[j], ord))
            }
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(y.count());
    for j in 0..y.count() {
        let mut count = 0i64;
        for i in 0..x.count() {
            let ord = cmp(i, j).ok_or_else(|| {
                Error::domain(
                    format!(
                        "interval index compares {} bounds with {} values",
                        x.dtype().name(),
                        y.dtype().name()
                    ),
                    span,
                )
            })?;
            // APL counts a bound EQUAL to the value, J does not.
            count += i64::from(if closed { ord.is_le() } else { ord.is_lt() });
        }
        out.push(offset + count);
    }
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
    near: NearInt,
    span: Span,
) -> Result<Array> {
    let bounds = y
        .to_i64_vec_near(near)
        .ok_or_else(|| Error::domain("roll needs whole numbers", span))?;
    if bounds.iter().any(|&b| b < 0) {
        return Err(Error::domain("roll needs non-negative numbers", span));
    }
    if !float_at_zero && bounds.contains(&0) {
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
fn deal(
    x: &Array,
    y: &Array,
    origin: i64,
    fixed: bool,
    near: NearInt,
    span: Span,
) -> Result<Array> {
    let want = one_whole(x, "the count dealt", near, span)?;
    let from = one_whole(y, "the range dealt from", near, span)?;
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
fn one_whole(a: &Array, what: &str, near: NearInt, span: Span) -> Result<i64> {
    let v = a
        .to_i64_vec_near(near)
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
fn squad(x: &Array, y: &Array, origin: i64, leading: bool, near: NearInt, span: Span) -> Result<Array> {
    if x.rank() > 1 {
        return Err(Error::new(
            ErrorKind::Rank,
            "the index of ⌷ must be a scalar or a vector",
            Some(span),
        ));
    }
    // One item of x per axis of y — per LEADING axis where the dialect
    // reads it that way, so a shorter index leaves the trailing axes
    // whole. An item is a scalar, which drops its axis, or an enclosed
    // vector, which keeps it and selects that many.
    let items: Vec<Array> = if x.rank() == 0 { vec![x.clone()] } else { x.cells(1) };
    let named = items.len();
    if named > y.rank() || (!leading && named != y.rank()) {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("{} index(es) for an argument of rank {}", named, y.rank()),
            Some(span),
        ));
    }
    let mut specs = Vec::with_capacity(items.len());
    let mut shape = Vec::new();
    for (k, item) in items.iter().enumerate() {
        let spec = match item.as_boxes() {
            Some(bs) if item.rank() == 0 => bs[0].clone(),
            _ => item.clone(),
        };
        let idx = spec
            .to_i64_vec_near(near)
            .ok_or_else(|| Error::domain("index must be an integer", span))?;
        for &i in &idx {
            let j = i - origin;
            if j < 0 || j as usize >= y.shape[k] {
                return Err(Error::domain(
                    format!("index {i} is out of range on axis {k}"),
                    span,
                ));
            }
        }
        shape.extend_from_slice(&spec.shape);
        specs.push((spec.shape.clone(), idx));
    }
    // An index shorter than the rank names the leading axes only; every
    // trailing axis comes through whole.
    for k in named..y.rank() {
        let n = y.shape[k];
        shape.push(n);
        specs.push((vec![n], (0..n as i64).map(|i| i + origin).collect()));
    }
    let y = y.to_row_major();
    let st = strides(&y.shape);
    let total: usize = shape.iter().product();
    let mut data = Data::empty(y.dtype());
    let mut coord = vec![0usize; shape.len()];
    for _ in 0..total {
        let mut at = 0usize;
        let mut used = 0usize;
        for (k, (sshape, idx)) in specs.iter().enumerate() {
            let sst = strides(sshape);
            let pick: usize = (0..sshape.len()).map(|a| coord[used + a] * sst[a]).sum();
            used += sshape.len();
            at += (idx[pick] - origin) as usize * st[k];
        }
        push_elem(&mut data, y.row_major_data(), at);
        odometer(&mut coord, &shape);
    }
    Ok(Array::new(shape, data))
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
    near: NearInt,
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
        .to_i64_vec_near(near)
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
fn amend(m: &Array, x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    if y.rank() == 0 {
        return Err(Error::new(ErrorKind::Rank, "cannot amend a scalar", Some(span)));
    }
    // A boxed m is J's index specification, the same one `{` reads.
    if let Some(spec) = m.as_boxes().and_then(<[Array]>::first) {
        let spec = index_spec(spec, y, near, span)?;
        return amend_spec(&spec, x, y, span);
    }
    let idx = m
        .to_i64_vec_near(near)
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
fn fetch(x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    let steps: Vec<Array> = match x.as_boxes() {
        Some(bs) => bs.to_vec(),
        None => vec![x.clone()],
    };
    let mut cur = y.clone();
    for step in steps {
        // An empty step selects the level whole, which is how a path
        // reaches into a boxed scalar; `a:` spells it and holds characters.
        let idx = if step.count() == 0 {
            Vec::new()
        } else {
            step.to_i64_vec_near(near)
                .ok_or_else(|| Error::domain("a fetch path holds integers", span))?
        };
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
/// `x⊂y` in the Dyalog line: a partitioned enclose.
///
/// Each item of x says how many partitions to open before the item of y
/// beside it, so a count above one leaves an empty partition behind and a
/// leading zero drops the items ahead of the first partition. The answer
/// is a VECTOR of partitions however deep y is: rank 2 and above splits
/// the last axis and every partition keeps the axes ahead of it.
fn partition_counts(x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    if y.rank() == 0 {
        return Err(Error::new(
            ErrorKind::Rank,
            "partitioned enclose needs an array to partition",
            Some(span),
        ));
    }
    let counts = x
        .to_i64_vec_near(near)
        .ok_or_else(|| Error::domain("partition counts must be integers", span))?;
    if counts.iter().any(|&c| c < 0) {
        return Err(Error::domain("partition counts must not be negative", span));
    }
    let last = y.shape[y.rank() - 1];
    // A scalar count applies to every item; a vector shorter than the
    // axis is padded with zeros, so its items stay in the partition
    // already open. More counts than items is a length error.
    if counts.len() > last {
        return Err(Error::new(
            ErrorKind::Length,
            format!("{} count(s) for {} item(s)", counts.len(), last),
            Some(span),
        ));
    }
    let at = |i: usize| -> i64 {
        if x.rank() == 0 {
            counts.first().copied().unwrap_or(0)
        } else {
            counts.get(i).copied().unwrap_or(0)
        }
    };
    // Each partition is a contiguous run of the last axis: where it
    // starts, and how many items it holds.
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for i in 0..last {
        for _ in 0..at(i) {
            groups.push((i, 0));
        }
        if let Some(g) = groups.last_mut() {
            g.1 += 1;
        }
    }
    let y = y.to_row_major();
    let rows = if last == 0 { 0 } else { y.count() / last };
    let lead = &y.shape[..y.rank() - 1];
    let parts: Vec<Array> = groups
        .iter()
        .map(|&(start, len)| {
            let mut d = Data::empty(y.dtype());
            for r in 0..rows {
                for c in start..start + len {
                    push_elem(&mut d, y.row_major_data(), r * last + c);
                }
            }
            let mut shape = lead.to_vec();
            shape.push(len);
            Array::new(shape, d)
        })
        .collect();
    Ok(Array::new(vec![parts.len()], Data::Box(parts.into())))
}

fn partition_enclose(x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    // Rank 2 and above partitions the LAST axis, once per cross section,
    // so the axes ahead of it frame the answer.
    if y.rank() > 1 {
        let last = y.shape[y.rank() - 1];
        let rows = y.count() / last.max(1);
        let mut cells: Vec<Array> = Vec::new();
        let mut width = None;
        for r in 0..rows {
            let row = Array::new(vec![last], y.data.slice(r * last, (r + 1) * last));
            let parts = partition_enclose(x, &row, near, span)?;
            let n = parts.count();
            if *width.get_or_insert(n) != n {
                return Err(Error::internal("partitions of unequal count"));
            }
            match parts.data {
                Data::Box(v) => cells.extend(v.as_slice().iter().cloned()),
                _ => return Err(Error::internal("a partition is boxed")),
            }
        }
        let mut shape = y.shape[..y.rank() - 1].to_vec();
        shape.push(width.unwrap_or(0));
        return Ok(Array::new(shape, Data::Box(cells.into())));
    }
    if y.rank() == 0 {
        return Err(Error::new(
            ErrorKind::Rank,
            "partitioned enclose needs an array to partition",
            Some(span),
        ));
    }
    let mut flags = x
        .to_i64_vec_near(near)
        .ok_or_else(|| Error::domain("partition flags must be integers", span))?;
    if flags.iter().any(|&f| f < 0) {
        return Err(Error::domain("partition flags must not be negative", span));
    }
    // A SINGLE flag is the flag of every item, so `1⊂1 2 3` opens one
    // partition over the whole vector and `0⊂1 2 3` opens none. Only the
    // one-flag case extends: two flags for three items stays a length
    // error, since there is no reading that makes them fit.
    if flags.len() == 1 && y.shape[0] != 1 {
        flags = vec![flags[0]; y.shape[0]];
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
    let groups = group_positions(&keys, ctx.cfg.tol);
    let items = if y.rank() == 0 { Array::new(vec![1], y.data.clone()) } else { y.clone() };
    let mut cells = Vec::with_capacity(groups.len());
    for (_, at) in &groups {
        cells.push(u.monad(&select_items(&items, at), ctx, span)?);
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
        let Some(x) = x else {
            return u.monad(&reverse_all_axes(y), ctx, span);
        };
        let (origin, size) = rectangle(x, span)?;
        let origin = origin.unwrap_or_else(|| vec![0; size.len()]);
        return u.monad(&subarray(y, &origin, &size, span)?, ctx, span);
    }
    if mode.abs() == 3 {
        let Some(x) = x else {
            return Err(Error::not_yet("monadic tessellation (u;.3 y)", span));
        };
        return tessellate(u, x, y, mode < 0, ctx, span);
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
            // A fret is a flag, and only 0 and 1 are flags: `2 u;.1 y` is
            // a domain error, as the reference has it.
            if let Some(&bad) = flags.iter().find(|&&f| f != 0 && f != 1) {
                return Err(Error::domain(format!("{bad} is not a fret: a fret is 0 or 1"), span));
            }
            // A scalar fret marks every item, which is the whole of
            // `1 u;.2 y`: one interval per item.
            if x.rank() == 0 {
                vec![flags[0] != 0; n]
            } else {
                if flags.len() != n {
                    return Err(Error::new(
                        ErrorKind::Length,
                        format!("{} fret(s) for {n} item(s)", flags.len()),
                        Some(span),
                    ));
                }
                flags.iter().map(|&f| f != 0).collect()
            }
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
    // No frets, so no intervals: the one interval an empty argument offers
    // is the empty itself, and the verb applied to it says what shape the
    // pieces would have had.
    if ranges.is_empty() {
        let cell = u.is_pure().then(|| section(&items, 0, 0));
        return Ok(empty_frame(&[0], items.dtype(), cell, ctx, |cell, c| u.monad(cell, c, span)));
    }
    let mut cells = Vec::with_capacity(ranges.len());
    for (s, e) in &ranges {
        cells.push(u.monad(&section(&items, *s, *e), ctx, span)?);
    }
    assemble(&[ranges.len()], cells, span)
}

/// The left argument of `;.0` and `;.3`: one row of origins (or movements)
/// and one of sizes. A single vector gives only the sizes.
fn rectangle(x: &Array, span: Span) -> Result<(Option<Vec<i64>>, Vec<i64>)> {
    let values = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("a cut rectangle is whole numbers", span))?;
    match x.rank() {
        0 | 1 => Ok((None, values)),
        2 if x.shape[0] == 2 => {
            let n = x.shape[1];
            Ok((Some(values[..n].to_vec()), values[n..].to_vec()))
        }
        _ => Err(Error::new(
            ErrorKind::Rank,
            "a cut rectangle is a vector of sizes, or two rows of origins and sizes",
            Some(span),
        )),
    }
}

/// The block of `y` that starts at `origin` and runs `size` along each of
/// the leading axes, the rest of them taken whole. A negative size runs the
/// same distance and reverses that axis.
fn subarray(y: &Array, origin: &[i64], size: &[i64], span: Span) -> Result<Array> {
    if origin.len() > y.rank() {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("a cut of {} axis/axes into a rank-{} value", origin.len(), y.rank()),
            Some(span),
        ));
    }
    let r = y.rank();
    let st = strides(&y.shape);
    let mut shape = y.shape.clone();
    let mut start = vec![0i64; r];
    let mut step = vec![1i64; r];
    for k in 0..origin.len() {
        // The magnitude is measured in u128 so that a size of i64::MIN — a
        // number the program is free to write — is compared rather than
        // negated, and the axis check runs before anything is cast down.
        let want = u128::from(size[k].unsigned_abs());
        let from = if origin[k] < 0 { origin[k] + y.shape[k] as i64 } else { origin[k] };
        if from < 0 || u128::from(from.unsigned_abs()) + want > y.shape[k] as u128 {
            return Err(Error::domain(
                format!("a cut of {want} from {from} leaves axis {k} of {}", y.shape[k]),
                span,
            ));
        }
        let len = want as usize;
        shape[k] = len;
        if size[k] < 0 {
            start[k] = from + len as i64 - 1;
            step[k] = -1;
        } else {
            start[k] = from;
        }
    }
    Ok(gather(y, &shape, &start, &step, &st))
}

/// The elements of `y` at `start + step × coordinate`, shaped `shape`.
fn gather(y: &Array, shape: &[usize], start: &[i64], step: &[i64], st: &[usize]) -> Array {
    let n: usize = shape.iter().product();
    let mut data = Data::empty(y.dtype());
    let mut coord = vec![0usize; shape.len()];
    for _ in 0..n {
        let idx: usize = (0..shape.len())
            .map(|k| (start[k] + step[k] * coord[k] as i64) as usize * st[k])
            .sum();
        push_elem(&mut data, &y.data, idx);
        odometer(&mut coord, shape);
    }
    Array::new(shape.to_vec(), data)
}

/// `x u;.3 y` and `x u;._3 y`: u over every block of the given size, moved
/// by the given step along each axis. `;.3` keeps the short blocks at the
/// far edge; `;._3` takes only the complete ones.
fn tessellate(
    u: &Verb,
    x: &Array,
    y: &Array,
    complete: bool,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    // A single vector gives the sizes; the blocks then move one at a time.
    let (movement, size) = rectangle(x, span)?;
    // A negative size reverses its axis, which is well defined only where
    // the movement is written out: given a bare vector of sizes the
    // reference answers with something the magnitude plays no part in, and
    // libjay will not guess at it.
    if size.iter().any(|&s| s < 0) && movement.is_none() {
        return Err(Error::not_yet(
            "a negative block size without a movement row (x u;.3 y)",
            span,
        ));
    }
    let movement = movement.unwrap_or_else(|| vec![1; size.len()]);
    if size.len() > y.rank() {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("a tessellation of {} axis/axes into a rank-{} value", size.len(), y.rank()),
            Some(span),
        ));
    }
    // The block size and the step are the program's own numbers and may be
    // any i64, so how many blocks fit is counted in i128: `size` has no
    // negation at i64::MIN and `len + step` overflows for a large step.
    let mut frame = Vec::with_capacity(size.len());
    for k in 0..size.len() {
        let (len, step) = (i128::from(y.shape[k] as i64), i128::from(movement[k]));
        let block = i128::from(size[k]).abs();
        if step <= 0 {
            return Err(Error::domain("a tessellation moves by a positive step", span));
        }
        let count = if complete {
            if len < block { 0 } else { (len - block) / step + 1 }
        } else {
            (len + step - 1) / step
        };
        frame.push(count as usize);
    }
    let total: usize = frame.iter().product();
    let mut cells = Vec::with_capacity(total);
    let mut coord = vec![0usize; frame.len()];
    for _ in 0..total {
        let origin: Vec<i64> = (0..frame.len()).map(|k| coord[k] as i64 * movement[k]).collect();
        // A block at the far edge is cut short by what is left of the axis;
        // a negative size keeps its sign, which reverses that axis.
        let block: Vec<i64> = (0..frame.len())
            .map(|k| {
                let left = i128::from(y.shape[k] as i64 - origin[k]);
                let len = i128::from(size[k]).abs().min(left) as i64;
                if size[k] < 0 { -len } else { len }
            })
            .collect();
        cells.push(u.monad(&subarray(y, &origin, &block, span)?, ctx, span)?);
        odometer(&mut coord, &frame);
    }
    assemble(&frame, cells, span)
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

/// `x |: y` and `x ⍉ y`: y with each of its axes sent where the left
/// argument says. Several axes sharing a destination are run together,
/// which is the diagonal, and the result is as long there as the shortest
/// of them.
fn transpose_to(y: &Array, dest: &[usize], span: Span) -> Result<Array> {
    let rank_out = dest.iter().copied().max().map_or(0, |m| m + 1);
    let mut out_shape = vec![usize::MAX; rank_out];
    for (a, &d) in dest.iter().enumerate() {
        out_shape[d] = out_shape[d].min(y.shape[a]);
    }
    if out_shape.contains(&usize::MAX) {
        return Err(Error::new(
            ErrorKind::Domain,
            "a transpose must name every axis of the result",
            Some(span),
        ));
    }
    let y = y.to_row_major();
    let st = strides(&y.shape);
    let n: usize = out_shape.iter().product();
    let mut data = Data::empty(y.dtype());
    let mut coord = vec![0usize; rank_out];
    for _ in 0..n {
        let idx: usize = dest.iter().enumerate().map(|(a, &d)| coord[d] * st[a]).sum();
        push_elem(&mut data, &y.data, idx);
        odometer(&mut coord, &out_shape);
    }
    Ok(Array::new(out_shape, data))
}

/// `x ⍉ y`: x names, for each axis of y in turn, the axis of the result it
/// becomes. Two axes given the same destination are run together.
fn transpose_apl(x: &Array, y: &Array, io: i64, near: NearInt, span: Span) -> Result<Array> {
    let axes = x
        .to_i64_vec_near(near)
        .ok_or_else(|| Error::domain("a transpose is given whole numbers", span))?;
    if axes.len() != y.rank() {
        return Err(Error::new(
            ErrorKind::Length,
            format!("{} axes for a rank-{} value", axes.len(), y.rank()),
            Some(span),
        ));
    }
    let mut dest = Vec::with_capacity(axes.len());
    for a in axes {
        let d = a - io;
        if d < 0 || d as usize >= y.rank() {
            return Err(Error::new(
                ErrorKind::Domain,
                format!("axis {a} is outside a rank-{} value", y.rank()),
                Some(span),
            ));
        }
        dest.push(d as usize);
    }
    transpose_to(y, &dest, span)
}

/// `x |: y`: x names the axes to move to the END, in the order given; the
/// rest keep their order in front. A boxed x groups axes, and the axes of
/// one group are run together — the diagonal.
fn transpose_j(x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    let groups: Vec<Vec<i64>> = match x.as_boxes() {
        Some(bs) => bs
            .iter()
            .map(|b| {
                b.to_i64_vec_near(near).ok_or_else(|| {
                    Error::domain("a transpose is given whole numbers", span)
                })
            })
            .collect::<Result<Vec<_>>>()?,
        None => x
            .to_i64_vec_near(near)
            .ok_or_else(|| Error::domain("a transpose is given whole numbers", span))?
            .into_iter()
            .map(|a| vec![a])
            .collect(),
    };
    let r = y.rank();
    // Which group each axis belongs to; an axis named twice is an error, as
    // it is in J.
    let mut group_of = vec![None; r];
    for (g, axes) in groups.iter().enumerate() {
        for &a in axes {
            let k = if a < 0 { a + r as i64 } else { a };
            if k < 0 || k as usize >= r {
                return Err(Error::new(
                    ErrorKind::Domain,
                    format!("axis {a} is outside a rank-{r} value"),
                    Some(span),
                ));
            }
            if group_of[k as usize].is_some() {
                return Err(Error::new(
                    ErrorKind::Domain,
                    format!("axis {a} is named twice in a transpose"),
                    Some(span),
                ));
            }
            group_of[k as usize] = Some(g);
        }
    }
    let leading = group_of.iter().filter(|g| g.is_none()).count();
    let mut dest = vec![0usize; r];
    let mut next = 0;
    for a in 0..r {
        match group_of[a] {
            None => {
                dest[a] = next;
                next += 1;
            }
            Some(g) => dest[a] = leading + g,
        }
    }
    transpose_to(y, &dest, span)
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

// ------------------------------------------------ index specifications

/// What a J index specification picks out of an array.
struct Spec {
    /// How many leading axes of the argument the specification indexes.
    width: usize,
    /// One coordinate vector per selected cell, in result order.
    cells: Vec<Vec<usize>>,
    /// The shape the specification contributes; the argument's remaining
    /// axes follow it.
    shape: Vec<usize>,
}

/// One index against an axis of `len` elements, counting a negative one
/// from the end.
fn axis_position(v: i64, len: usize, span: Span) -> Result<usize> {
    let p = if v < 0 { v + len as i64 } else { v };
    if p < 0 || p >= len as i64 {
        return Err(Error::domain(
            format!("index {v} is out of range: the axis has {len} element(s)"),
            span,
        ));
    }
    Ok(p as usize)
}

/// J's index specification: what a BOXED left argument of `{` or `m}` says.
///
/// `<A` with a simple `A` reads A's last axis as one index per leading axis
/// of y, the axes ahead of it framing the result — so `(<1 2) { y` is one
/// element and `(<2 2$…) { y` is two of them. `<(c0;c1;…)` gives one
/// component per leading axis instead: a simple component's atoms are that
/// axis's indices, a scalar one dropping the axis from the result, and a
/// BOXED component is the complement — every index of the axis except the
/// ones it holds, which is what `a:` (the empty box) uses to mean "all".
fn index_spec(content: &Array, y: &Array, near: NearInt, span: Span) -> Result<Spec> {
    let too_deep = |n: usize| {
        Error::new(
            ErrorKind::Rank,
            format!("an index specification of {n} axis/axes into a rank-{} value", y.rank()),
            Some(span),
        )
    };
    if let Some(items) = content.as_boxes() {
        if items.len() > y.rank() {
            return Err(too_deep(items.len()));
        }
        let mut per_axis: Vec<Vec<usize>> = Vec::with_capacity(items.len());
        let mut shape: Vec<usize> = Vec::new();
        for (k, c) in items.iter().enumerate() {
            let len = y.shape[k];
            if c.as_boxes().is_some() {
                let inner = open_cell(c);
                let excluded = inner.to_i64_vec_near(near).ok_or_else(|| {
                    Error::domain("an index complement holds integers", span)
                })?;
                let mut dropped = vec![false; len];
                for v in excluded {
                    dropped[axis_position(v, len, span)?] = true;
                }
                let kept: Vec<usize> = (0..len).filter(|i| !dropped[*i]).collect();
                shape.push(kept.len());
                per_axis.push(kept);
            } else {
                let idx = c
                    .to_i64_vec_near(near)
                    .ok_or_else(|| Error::domain("an index holds integers", span))?;
                let mut positions = Vec::with_capacity(idx.len());
                for v in idx {
                    positions.push(axis_position(v, len, span)?);
                }
                shape.extend_from_slice(&c.shape);
                per_axis.push(positions);
            }
        }
        // The components run as an odometer, the last one fastest.
        let mut cells: Vec<Vec<usize>> = vec![Vec::new()];
        for positions in &per_axis {
            let mut next = Vec::with_capacity(cells.len() * positions.len());
            for prefix in &cells {
                for &p in positions {
                    let mut cell = prefix.clone();
                    cell.push(p);
                    next.push(cell);
                }
            }
            cells = next;
        }
        return Ok(Spec { width: per_axis.len(), cells, shape });
    }
    let idx = content
        .to_i64_vec_near(near)
        .ok_or_else(|| Error::domain("an index specification holds integers", span))?;
    let rank = content.rank();
    let width = if rank == 0 { 1 } else { content.shape[rank - 1] };
    if width > y.rank() {
        return Err(too_deep(width));
    }
    let shape: Vec<usize> = if rank == 0 { Vec::new() } else { content.shape[..rank - 1].to_vec() };
    let count: usize = shape.iter().product();
    let mut cells: Vec<Vec<usize>> = Vec::new();
    if width == 0 {
        cells.resize(count, Vec::new());
    } else {
        for chunk in idx.chunks(width) {
            let mut cell = Vec::with_capacity(width);
            for (k, &v) in chunk.iter().enumerate() {
                cell.push(axis_position(v, y.shape[k], span)?);
            }
            cells.push(cell);
        }
    }
    Ok(Spec { width, cells, shape })
}

/// The offset of a cell's first element, given the argument's strides.
fn spec_offset(st: &[usize], cell: &[usize]) -> usize {
    cell.iter().enumerate().map(|(k, &p)| p * st[k]).sum()
}

/// `(<spec) { y`: the cells the specification names, in its own order.
fn select_spec(spec: &Spec, y: &Array) -> Array {
    let st = strides(&y.shape);
    let size: usize = y.shape[spec.width..].iter().product();
    let mut data = Data::empty(y.dtype());
    for cell in &spec.cells {
        let base = spec_offset(&st, cell);
        for e in 0..size {
            push_elem(&mut data, &y.data, base + e);
        }
    }
    let mut shape = spec.shape.clone();
    shape.extend_from_slice(&y.shape[spec.width..]);
    Array::new(shape, data)
}

/// `x (<spec)} y`: y with the cells the specification names replaced by x,
/// which is either one cell spread over all of them or one cell each.
fn amend_spec(spec: &Spec, x: &Array, y: &Array, span: Span) -> Result<Array> {
    let size: usize = y.shape[spec.width..].iter().product();
    let per_cell = if x.count() == size {
        false
    } else if x.count() == size * spec.cells.len() {
        true
    } else {
        return Err(Error::new(
            ErrorKind::Length,
            format!(
                "cannot amend {} cell(s) of {size} element(s) each with {} element(s)",
                spec.cells.len(),
                x.count()
            ),
            Some(span),
        ));
    };
    let mismatch = || {
        Error::new(
            ErrorKind::Type,
            "the replacement and the argument hold different kinds of value",
            Some(span),
        )
    };
    let t = DType::promote(x.dtype(), y.dtype()).ok_or_else(mismatch)?;
    let (Some(src), Some(base)) = (x.data.cast(t), y.data.cast(t)) else {
        return Err(mismatch());
    };
    let st = strides(&y.shape);
    let mut plan: Vec<Option<usize>> = vec![None; y.count()];
    for (n, cell) in spec.cells.iter().enumerate() {
        let at = spec_offset(&st, cell);
        for e in 0..size {
            plan[at + e] = Some(if per_cell { n * size + e } else { e });
        }
    }
    let mut data = Data::empty(t);
    for (i, slot) in plan.iter().enumerate() {
        match slot {
            Some(n) => push_elem(&mut data, &src, *n),
            None => push_elem(&mut data, &base, i),
        }
    }
    Ok(Array::new(y.shape.clone(), data))
}

// -------------------------------------------------------------- the map

/// J monadic `{::`: y's box structure with every leaf replaced by the path
/// that fetches it.
///
/// A path is a boxed list holding one index per level descended — the
/// coordinate vector within that level's array, empty where the level is a
/// boxed scalar. An unboxed y is one leaf, itself, and its path is empty.
fn map_paths(y: &Array) -> Array {
    fn coord_of(shape: &[usize], mut i: usize) -> Array {
        let mut out = vec![0i64; shape.len()];
        for k in (0..shape.len()).rev() {
            out[k] = (i % shape[k]) as i64;
            i /= shape[k];
        }
        Array::from_i64(out)
    }
    fn go(y: &Array, prefix: &[Array]) -> Array {
        let Some(boxes) = y.as_boxes() else {
            if prefix.is_empty() {
                return Array::new(vec![0], Data::I64(Vec::new().into()));
            }
            return Array::new(vec![prefix.len()], Data::Box(prefix.to_vec().into()));
        };
        let cells: Vec<Array> = boxes
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let mut path = prefix.to_vec();
                path.push(coord_of(&y.shape, i));
                go(b, &path)
            })
            .collect();
        Array::new(y.shape.clone(), Data::Box(cells.into()))
    }
    go(y, &[])
}

// ------------------------------------------------------- fill and shift

/// `x |.!.f y`: shift along each axis instead of rotating, so an item moved
/// past an end is dropped and the place it left takes the fill f.
fn shift_fill(
    x: &Array,
    y: &Array,
    fill: &Array,
    near: NearInt,
    span: Span,
) -> Result<Array> {
    let counts = axis_counts(x, "shift", near, span)?;
    if y.rank() == 0 {
        return Ok(y.clone());
    }
    if counts.len() > y.rank() {
        return Err(Error::new(
            ErrorKind::Length,
            format!("shift has {} amounts for an argument of rank {}", counts.len(), y.rank()),
            Some(span),
        ));
    }
    if fill.count() != 1 {
        return Err(Error::new(ErrorKind::Length, "a fill is one atom", Some(span)));
    }
    let mismatch = || {
        Error::new(ErrorKind::Type, "the fill and the argument differ in kind", Some(span))
    };
    let t = DType::promote(y.dtype(), fill.dtype()).ok_or_else(mismatch)?;
    let (Some(base), Some(f)) = (y.data.cast(t), fill.data.cast(t)) else {
        return Err(mismatch());
    };
    let st = strides(&y.shape);
    let r = y.rank();
    let mut data = Data::empty(t);
    let mut coord = vec![0usize; r];
    for _ in 0..y.count() {
        let mut idx = 0usize;
        let mut vacated = false;
        for k in 0..r {
            // Saturating: an amount that cannot be added to the coordinate
            // has carried the item past the end of the axis by any measure,
            // which is what a shift vacates.
            let from = (coord[k] as i64).saturating_add(counts.get(k).copied().unwrap_or(0));
            if from < 0 || from >= y.shape[k] as i64 {
                vacated = true;
                break;
            }
            idx += from as usize * st[k];
        }
        if vacated {
            push_elem(&mut data, &f, 0);
        } else {
            push_elem(&mut data, &base, idx);
        }
        odometer(&mut coord, &y.shape);
    }
    Ok(Array::new(y.shape.clone(), data))
}

// ---------------------------------------------------------------- memo

/// An exact key for one array, appended to `out`. False where the value has
/// no cheap key — an exact number — and the memo must simply not cache it.
fn memo_key(a: &Array, out: &mut Vec<u64>) -> bool {
    out.push(a.rank() as u64);
    out.extend(a.shape.iter().map(|&n| n as u64));
    out.push(a.dtype() as u64);
    match &a.data {
        Data::Ext(_) | Data::Rat(_) => false,
        Data::Box(items) => items.iter().all(|item| memo_key(item, out)),
        d => {
            for i in 0..d.len() {
                out.push(elem_key(d, i));
            }
            true
        }
    }
}

/// `u M.`: u's answer for these arguments, computed once and kept.
fn memoised(
    u: &Verb,
    cache: &MemoCache,
    x: Option<&Array>,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    let apply = |ctx: &mut Ctx<'_>| match x {
        Some(x) => u.dyad(x, y, ctx, span),
        None => u.monad(y, ctx, span),
    };
    let mut key = vec![u64::from(x.is_some())];
    let keyed = x.is_none_or(|x| memo_key(x, &mut key)) && memo_key(y, &mut key);
    if !keyed {
        return apply(ctx);
    }
    if let Ok(map) = cache.lock() && let Some(hit) = map.get(&key) {
        return Ok(hit.clone());
    }
    let out = apply(ctx)?;
    if let Ok(mut map) = cache.lock() {
        map.insert(key, out.clone());
    }
    Ok(out)
}

// ----------------------------------------------------- levels and spread

/// `u L: n y` and `u S: n y`: u over every subarray at boxing level n or
/// below. `L:` puts each answer back where its operand was; `S:` collects
/// them into the items of one array.
fn at_level(
    u: &Verb,
    level: i64,
    spread: bool,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    // A negative level counts down from the argument's own top.
    let n = if level < 0 { (boxing_level(y) + level).max(0) } else { level };
    if !spread {
        return map_level(u, n, y, ctx, span);
    }
    let mut cells = Vec::new();
    collect_level(u, n, y, ctx, span, &mut cells)?;
    let count = cells.len();
    assemble(&[count], cells, span)
}

fn map_level(u: &Verb, n: i64, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let Some(boxes) = y.as_boxes().filter(|_| boxing_level(y) > n) else {
        return u.monad(y, ctx, span);
    };
    let boxes = boxes.to_vec();
    let mut cells = Vec::with_capacity(boxes.len());
    for b in &boxes {
        cells.push(map_level(u, n, b, ctx, span)?);
    }
    Ok(Array::new(y.shape.clone(), Data::Box(cells.into())))
}

/// `x u L: n y` and `x u S: n y`: both arguments are descended together
/// until each has reached level n, and u is applied to the pair. A side
/// that has already reached its level is held while the other descends, so
/// an unboxed left argument reaches every leaf of the right one.
fn at_level_dyad(
    u: &Verb,
    level: i64,
    spread: bool,
    x: &Array,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    // A negative level counts down from each argument's own top, so the
    // two sides can stop at different depths.
    let depth = |a: &Array| if level < 0 { (boxing_level(a) + level).max(0) } else { level };
    let (nx, ny) = (depth(x), depth(y));
    if !spread {
        return map_level_dyad(u, nx, ny, x, y, ctx, span);
    }
    let mut cells = Vec::new();
    collect_level_dyad(u, nx, ny, x, y, ctx, span, &mut cells)?;
    let count = cells.len();
    assemble(&[count], cells, span)
}

/// The boxes to descend into on each side, and the shape the answer takes.
struct LevelPairs {
    left: Vec<Array>,
    right: Vec<Array>,
    shape: Vec<usize>,
}

/// One step of the descent. `None` where neither side has any box left,
/// which is where u applies.
fn level_pairs(
    nx: i64,
    ny: i64,
    x: &Array,
    y: &Array,
    span: Span,
) -> Result<Option<LevelPairs>> {
    let bx = x.as_boxes().filter(|_| boxing_level(x) > nx);
    let by = y.as_boxes().filter(|_| boxing_level(y) > ny);
    Ok(match (bx, by) {
        (None, None) => None,
        (Some(bx), None) => {
            let n = bx.len();
            Some(LevelPairs {
                left: bx.to_vec(),
                right: vec![y.clone(); n],
                shape: x.shape.clone(),
            })
        }
        (None, Some(by)) => {
            let n = by.len();
            Some(LevelPairs {
                left: vec![x.clone(); n],
                right: by.to_vec(),
                shape: y.shape.clone(),
            })
        }
        (Some(bx), Some(by)) => {
            if x.shape != y.shape {
                return Err(Error::new(
                    ErrorKind::Length,
                    format!(
                        "the levels do not agree: left shape {}, right shape {}",
                        show_shape(&x.shape),
                        show_shape(&y.shape)
                    ),
                    Some(span),
                ));
            }
            Some(LevelPairs { left: bx.to_vec(), right: by.to_vec(), shape: x.shape.clone() })
        }
    })
}

fn map_level_dyad(
    u: &Verb,
    nx: i64,
    ny: i64,
    x: &Array,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    let Some(step) = level_pairs(nx, ny, x, y, span)? else {
        return u.dyad(x, y, ctx, span);
    };
    let mut cells = Vec::with_capacity(step.left.len());
    for (a, b) in step.left.iter().zip(step.right.iter()) {
        cells.push(map_level_dyad(u, nx, ny, a, b, ctx, span)?);
    }
    Ok(Array::new(step.shape, Data::Box(cells.into())))
}

#[allow(clippy::too_many_arguments)]
fn collect_level_dyad(
    u: &Verb,
    nx: i64,
    ny: i64,
    x: &Array,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
    out: &mut Vec<Array>,
) -> Result<()> {
    let Some(step) = level_pairs(nx, ny, x, y, span)? else {
        out.push(u.dyad(x, y, ctx, span)?);
        return Ok(());
    };
    for (a, b) in step.left.iter().zip(step.right.iter()) {
        collect_level_dyad(u, nx, ny, a, b, ctx, span, out)?;
    }
    Ok(())
}

fn collect_level(
    u: &Verb,
    n: i64,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
    out: &mut Vec<Array>,
) -> Result<()> {
    let Some(boxes) = y.as_boxes().filter(|_| boxing_level(y) > n) else {
        out.push(u.monad(y, ctx, span)?);
        return Ok(());
    };
    let boxes = boxes.to_vec();
    for b in &boxes {
        collect_level(u, n, b, ctx, span, out)?;
    }
    Ok(())
}

// --------------------------------------------------------- polynomials

/// The ascending coefficients of a polynomial argument, as complex values.
fn poly_coeffs(y: &Array, span: Span) -> Result<Vec<Cx>> {
    let c = y
        .data
        .cast(DType::Complex)
        .ok_or_else(|| Error::domain("a polynomial's coefficients are numbers", span))?;
    match c {
        Data::Complex(v) => Ok(v.as_slice().to_vec()),
        _ => Err(Error::internal("coefficients did not cast to complex")),
    }
}

// --------------------------------------------------- hypergeometric series

/// Terms the series is allowed before it is called divergent.
const HYPERGEOMETRIC_TERMS: usize = 1 << 16;

/// A parameter list, for a derived verb's name.
fn cx_list(v: &[Cx]) -> String {
    v.iter()
        .map(|z| if z[1] == 0.0 { format!("{}", z[0]) } else { format!("{}j{}", z[0], z[1]) })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `(m H. n) y`: the generalised hypergeometric function, summed term by
/// term from the ratio between neighbours —
/// `t[k+1] = t[k] × (Π(m+k) ÷ Π(n+k)) × y ÷ (k+1)`.
///
/// A parameter on both sides contributes the same factor to each product,
/// so the pairs are cancelled first: that is what makes `0 H. 0` the
/// exponential rather than a term of `0÷0`.
fn hypergeometric(num: &[Cx], den: &[Cx], y: &Array, span: Span) -> Result<Array> {
    let (num, den) = cancel_parameters(num, den);
    let at = poly_coeffs(y, span)?;
    let mut out = Vec::with_capacity(at.len());
    for z in &at {
        out.push(hypergeometric_at(&num, &den, *z, span)?);
    }
    let mut a = complex_or_real(out);
    a.shape = y.shape.clone();
    Ok(a)
}

/// The parameters left once every value common to both lists is dropped
/// from each, one occurrence at a time.
fn cancel_parameters(num: &[Cx], den: &[Cx]) -> (Vec<Cx>, Vec<Cx>) {
    let mut left: Vec<Cx> = Vec::with_capacity(num.len());
    let mut right: Vec<Cx> = den.to_vec();
    for a in num {
        match right.iter().position(|b| b == a) {
            Some(i) => {
                right.remove(i);
            }
            None => left.push(*a),
        }
    }
    (left, right)
}

fn hypergeometric_at(num: &[Cx], den: &[Cx], z: Cx, span: Span) -> Result<Cx> {
    // Wholly real arguments are summed in real arithmetic, where dividing
    // by a zero parameter gives the infinity J answers with; the complex
    // quotient would make that same division a NaN in both parts.
    let real = |v: &[Cx]| v.iter().all(|c| c[1] == 0.0);
    if z[1] == 0.0 && real(num) && real(den) {
        let n: Vec<f64> = num.iter().map(|c| c[0]).collect();
        let d: Vec<f64> = den.iter().map(|c| c[0]).collect();
        return Ok([hypergeometric_real(&n, &d, z[0], span)?, 0.0]);
    }
    let mut sum = cx::ONE;
    let mut term = cx::ONE;
    for k in 0..HYPERGEOMETRIC_TERMS {
        let kk = [k as f64, 0.0];
        let mut ratio = z;
        for a in num {
            ratio = cx::mul(ratio, cx::add(*a, kk));
        }
        for b in den {
            ratio = cx::div(ratio, cx::add(*b, kk));
        }
        term = cx::div(cx::mul(term, ratio), [k as f64 + 1.0, 0.0]);
        if !term[0].is_finite() || !term[1].is_finite() {
            // A zero denominator parameter, or a term past the range of a
            // double: the sum is the infinity (or NaN) the term became.
            return Ok(term);
        }
        let before = sum;
        sum = cx::add(sum, term);
        // The series has converged once a term no longer moves the sum.
        if sum == before {
            return Ok(sum);
        }
    }
    Err(Error::domain(
        format!("the hypergeometric series did not converge within {HYPERGEOMETRIC_TERMS} terms"),
        span,
    ))
}

fn hypergeometric_real(num: &[f64], den: &[f64], z: f64, span: Span) -> Result<f64> {
    let mut sum = 1.0f64;
    let mut term = 1.0f64;
    for k in 0..HYPERGEOMETRIC_TERMS {
        let kk = k as f64;
        let mut ratio = z;
        for a in num {
            ratio *= a + kk;
        }
        for b in den {
            ratio /= b + kk;
        }
        term = term * ratio / (kk + 1.0);
        if !term.is_finite() {
            return Ok(term);
        }
        let before = sum;
        sum += term;
        if sum == before {
            return Ok(sum);
        }
    }
    Err(Error::domain(
        format!("the hypergeometric series did not converge within {HYPERGEOMETRIC_TERMS} terms"),
        span,
    ))
}

/// A complex vector as an array, real where every imaginary part is zero.
fn complex_or_real(values: Vec<Cx>) -> Array {
    if values.iter().all(|z| z[1] == 0.0) {
        return Array::from_f64(values.iter().map(|z| z[0]).collect());
    }
    Array::new(vec![values.len()], Data::Complex(values.into()))
}

/// `x p. y`: the polynomial with ascending coefficients x, at y — Horner's
/// rule, or the product over the roots when x is the boxed root form.
fn poly_eval(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let at = poly_coeffs(y, span)?;
    let at = at.first().copied().unwrap_or(cx::ZERO);
    let value = match x.as_boxes() {
        Some(parts) => {
            if parts.len() != 2 {
                return Err(Error::domain(
                    "the root form of a polynomial is `multiplier ; roots`",
                    span,
                ));
            }
            let multiplier = poly_coeffs(&parts[0], span)?;
            let mut v = multiplier.first().copied().unwrap_or(cx::ONE);
            for r in poly_coeffs(&parts[1], span)? {
                v = cx::mul(v, cx::sub(at, r));
            }
            v
        }
        None => {
            let c = poly_coeffs(x, span)?;
            let mut v = cx::ZERO;
            for &k in c.iter().rev() {
                v = cx::add(cx::mul(v, at), k);
            }
            v
        }
    };
    Ok(scalar_complex_or_real(value))
}

fn scalar_complex_or_real(z: Cx) -> Array {
    if z[1] == 0.0 {
        return Array::scalar_f64(z[0]);
    }
    Array::new(vec![], Data::Complex(vec![z].into()))
}

/// `p. y`: the roots of the polynomial whose ascending coefficients y holds,
/// as `multiplier ; roots`; a y already in that form converts back to
/// coefficients.
fn poly_roots(y: &Array, span: Span) -> Result<Array> {
    if let Some(parts) = y.as_boxes() {
        if parts.len() != 2 {
            return Err(Error::domain(
                "the root form of a polynomial is `multiplier ; roots`",
                span,
            ));
        }
        let multiplier = poly_coeffs(&parts[0], span)?;
        let multiplier = multiplier.first().copied().unwrap_or(cx::ONE);
        // Multiply out `m × (x-r0) × (x-r1) × …`, ascending.
        let mut coeffs = vec![multiplier];
        for r in poly_coeffs(&parts[1], span)? {
            let mut next = vec![cx::ZERO; coeffs.len() + 1];
            for (k, &c) in coeffs.iter().enumerate() {
                next[k + 1] = cx::add(next[k + 1], c);
                next[k] = cx::sub(next[k], cx::mul(c, r));
            }
            coeffs = next;
        }
        return Ok(complex_or_real(coeffs));
    }
    let mut c = poly_coeffs(y, span)?;
    while c.len() > 1 && c[c.len() - 1] == cx::ZERO {
        c.pop();
    }
    // The ZERO polynomial has no leading coefficient to divide by and every
    // number for a root: J answers `0 ; ''`, a zero multiplier and no roots
    // at all. Only a non-zero constant has no root form.
    if c.iter().all(|&k| k == cx::ZERO) {
        let pair = vec![Array::scalar_i64(0), Array::new(vec![0], Data::empty(DType::I64))];
        return Ok(Array::new(vec![2], Data::Box(pair.into())));
    }
    if c.len() < 2 {
        return Err(Error::domain("a polynomial's roots need a coefficient of x", span));
    }
    let lead = c[c.len() - 1];
    let monic: Vec<Cx> = c.iter().map(|&k| cx::div(k, lead)).collect();
    let roots = durand_kerner(&monic);
    let pair = vec![scalar_complex_or_real(lead), complex_or_real(roots)];
    Ok(Array::new(vec![2], Data::Box(pair.into())))
}

/// The roots of a monic polynomial, by the Durand–Kerner iteration: every
/// root is refined against all the others at once, from spread-out starting
/// points, until none of them moves.
///
/// The answer is ordered by descending real part, then descending
/// imaginary part, which is a stable order the iteration itself has none of.
fn durand_kerner(monic: &[Cx]) -> Vec<Cx> {
    let d = monic.len() - 1;
    let seed = [0.4, 0.9];
    let mut z: Vec<Cx> = Vec::with_capacity(d);
    let mut p = cx::ONE;
    for _ in 0..d {
        z.push(p);
        p = cx::mul(p, seed);
    }
    let value = |monic: &[Cx], at: Cx| {
        let mut v = cx::ZERO;
        for &k in monic.iter().rev() {
            v = cx::add(cx::mul(v, at), k);
        }
        v
    };
    for _ in 0..500 {
        let mut moved: f64 = 0.0;
        for i in 0..d {
            let mut denom = cx::ONE;
            for j in 0..d {
                if i != j {
                    denom = cx::mul(denom, cx::sub(z[i], z[j]));
                }
            }
            if denom == cx::ZERO {
                continue;
            }
            let step = cx::div(value(monic, z[i]), denom);
            z[i] = cx::sub(z[i], step);
                moved = moved.max(step[0].hypot(step[1]));
        }
        if moved < 1e-15 {
            break;
        }
    }
    let mut z = polished_repeats(monic, z);
    // A root within rounding of the real axis is a real root.
    for r in &mut z {
        if r[1].abs() < 1e-9 {
            r[1] = 0.0;
        }
        if r[0].abs() < 1e-12 {
            r[0] = 0.0;
        }
    }
    // Order is the one J answers in: the largest magnitude first, then the
    // largest real part, then the largest imaginary part — so ¯3 comes
    // before 2, and a conjugate pair keeps the positive half in front. The
    // keys are coarsened first, because two members of a pair agree only
    // to rounding and the sort needs a total order to stand on.
    let coarse = |v: f64| -> f64 {
        if v == 0.0 || !v.is_finite() { v } else { format!("{v:.11e}").parse().unwrap_or(v) }
    };
    let mut keyed: Vec<([f64; 3], Cx)> =
        z.into_iter().map(|r| ([coarse(cx::abs(r)), coarse(r[0]), coarse(r[1])], r)).collect();
    keyed.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    keyed.into_iter().map(|(_, r)| r).collect()
}

/// A repeated root, put back where it belongs.
///
/// Durand–Kerner reaches a root of multiplicity m only to about the m-th
/// root of the machine epsilon, so a double root of `1 2 1` comes out as
/// two complex values 1e¯8 either side of ¯1: complex noise where the
/// answer is a pair of exact reals. The straddle is symmetric, so the
/// group's CENTRE carries the accuracy its members lack. Roots within reach
/// of one another are gathered and every member of a group moves to the
/// group's centre.
///
/// Reach is a guess, and a wrong one merges two roots that are merely
/// close. So the answer is kept only when the polynomial rebuilt from it
/// fits the coefficients at least as well as the raw roots do, and the
/// widest reach that passes that test is the one taken.
fn polished_repeats(monic: &[Cx], z: Vec<Cx>) -> Vec<Cx> {
    let d = z.len();
    if d < 2 {
        return z;
    }
    let raw = coefficient_error(monic, &z);
    let scale = monic.iter().map(|&k| cx::abs(k)).fold(1.0f64, f64::max);
    let allowed = raw.max(1e-13 * scale);
    for reach in [1e-3, 1e-4, 1e-5, 1e-6, 1e-7] {
        // Single linkage: a chain of near neighbours is one group, which
        // is what a triple root's three points around the true value are.
        let mut group: Vec<usize> = (0..d).collect();
        for i in 0..d {
            for j in 0..i {
                let apart = cx::abs(cx::sub(z[i], z[j]));
                let span = reach * (1.0 + cx::abs(z[i]).max(cx::abs(z[j])));
                if apart <= span {
                    let (a, b) = (group[i], group[j]);
                    let (keep, drop) = (a.min(b), a.max(b));
                    for g in &mut group {
                        if *g == drop {
                            *g = keep;
                        }
                    }
                }
            }
        }
        let mut centre = vec![cx::ZERO; d];
        let mut size = vec![0usize; d];
        for i in 0..d {
            centre[group[i]] = cx::add(centre[group[i]], z[i]);
            size[group[i]] += 1;
        }
        if size.iter().all(|&n| n < 2) {
            return z;
        }
        let mut settled: Vec<Option<Cx>> = vec![None; d];
        for g in 0..d {
            if size[g] == 0 {
                continue;
            }
            let start = cx::div(centre[g], cx::from_real(size[g] as f64));
            // Near a root of multiplicity m the polynomial's own value is
            // lost to cancellation — it reads as zero over a whole ball —
            // so refining against it can go no further. The m-1st
            // DERIVATIVE has the same root simply, with none of that
            // cancellation, and Newton on it lands exactly: `1 3 3 1`'s
            // second derivative is `6 6`, whose one root is ¯1.
            settled[g] = Some(if size[g] < 2 {
                start
            } else {
                newton_at(&nth_derivative(monic, size[g] - 1), start)
            });
        }
        let out: Vec<Cx> = (0..d).map(|i| settled[group[i]].unwrap_or(z[i])).collect();
        if out.iter().all(|r| r[0].is_finite() && r[1].is_finite())
            && coefficient_error(monic, &out) <= allowed
        {
            return out;
        }
    }
    z
}

/// Newton's method from `start`, on the coefficients as given.
fn newton_at(poly: &[Cx], start: Cx) -> Cx {
    let mut z = start;
    for _ in 0..40 {
        let (mut p, mut slope) = (cx::ZERO, cx::ZERO);
        for &k in poly.iter().rev() {
            slope = cx::add(cx::mul(slope, z), p);
            p = cx::add(cx::mul(p, z), k);
        }
        if slope == cx::ZERO {
            break;
        }
        let step = cx::div(p, slope);
        let next = cx::sub(z, step);
        if !next[0].is_finite() || !next[1].is_finite() {
            break;
        }
        z = next;
        if cx::abs(step) <= 1e-17 * (1.0 + cx::abs(z)) {
            break;
        }
    }
    z
}

/// The `k`-th derivative of a polynomial's ascending coefficients.
fn nth_derivative(c: &[Cx], k: usize) -> Vec<Cx> {
    let mut out = c.to_vec();
    for _ in 0..k {
        if out.len() < 2 {
            return vec![cx::ZERO];
        }
        out = out
            .iter()
            .enumerate()
            .skip(1)
            .map(|(i, &v)| cx::mul(v, cx::from_real(i as f64)))
            .collect();
    }
    out
}

/// How far the monic polynomial rebuilt from `roots` sits from the one the
/// coefficients describe: the largest coefficient difference, relative to
/// the coefficient it belongs to.
fn coefficient_error(monic: &[Cx], roots: &[Cx]) -> f64 {
    let mut built = vec![cx::ONE];
    for &r in roots {
        let mut next = vec![cx::ZERO; built.len() + 1];
        for (k, &c) in built.iter().enumerate() {
            next[k + 1] = cx::add(next[k + 1], c);
            next[k] = cx::sub(next[k], cx::mul(c, r));
        }
        built = next;
    }
    let mut worst: f64 = 0.0;
    for (k, &want) in monic.iter().enumerate() {
        let got = built.get(k).copied().unwrap_or(cx::ZERO);
        worst = worst.max(cx::abs(cx::sub(got, want)) / (1.0 + cx::abs(want)));
    }
    worst
}

/// `p.. y`: the derivative of the polynomial y's ascending coefficients
/// describe, again as coefficients.
fn poly_deriv(y: &Array, span: Span) -> Result<Array> {
    let c = poly_coeffs(y, span)?;
    if c.len() < 2 {
        return Ok(Array::from_i64(vec![0]));
    }
    let out: Vec<Cx> =
        c.iter().enumerate().skip(1).map(|(k, &v)| cx::mul(v, cx::from_real(k as f64))).collect();
    Ok(narrow_numbers(complex_or_real(out)))
}

/// `x p.. y`: the integral of y's coefficients, with x as the constant term.
fn poly_integral(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let c = poly_coeffs(y, span)?;
    let k = poly_coeffs(x, span)?;
    let mut out = vec![k.first().copied().unwrap_or(cx::ZERO)];
    for (i, &v) in c.iter().enumerate() {
        out.push(cx::div(v, cx::from_real((i + 1) as f64)));
    }
    Ok(narrow_numbers(complex_or_real(out)))
}

/// A float array whose values are all whole, as integers. Polynomial
/// coefficients are computed in floats and mostly come out whole; J prints
/// and types them as integers, so libjay narrows them back.
fn narrow_numbers(a: Array) -> Array {
    let Data::F64(v) = &a.data else { return a };
    if v.iter().any(|x| !x.is_finite() || x.fract() != 0.0 || x.abs() > 9e15) {
        return a;
    }
    let values: Vec<i64> = v.iter().map(|&x| x as i64).collect();
    Array::new(a.shape, Data::I64(values.into()))
}

/// `u b. n`: what u is, rather than what it does. Only `0`, the three
/// ranks, is answered; the rest of J's characteristics reach into the
/// representation of a verb, which libjay does not publish.
fn characteristics(u: &Verb, y: &Array, span: Span) -> Result<Array> {
    let which = y.to_i64_vec().and_then(|v| v.first().copied());
    let chars = |s: String| Ok(Array::from_chars(s.chars().collect()));
    match which {
        Some(0) => {
            let ranks = u.ranks();
            Ok(Array::from_f64(
                ranks
                    .iter()
                    .map(|&r| if r == RANK_INF { f64::INFINITY } else { r as f64 })
                    .collect(),
            ))
        }
        // `u b. _1` and `u b. 1` answer with a spelling, not a verb: the
        // obverse, and the verb that yields the identity element of a
        // reduction over no items.
        Some(-1) => match obverse(u) {
            Some(v) => chars(v.name()),
            None => Err(Error::not_yet(
                format!("the obverse of {} (no inverse is known)", u.name()),
                span,
            )),
        },
        // `b.` is J's conjunction and has no APL spelling, so the identity
        // asked for here is always J's.
        Some(1) => match reduce_identity(u, 1, crate::Lang::J).as_ref().map(identity_spelling) {
            Some(s) => chars(s),
            None => Err(Error::not_yet(
                format!("the identity function of {} (u b. 1)", u.name()),
                span,
            )),
        },
        _ => Err(Error::not_yet("a verb characteristic other than 0, 1 and _1", span)),
    }
}

/// J spells an identity function as the neutral cell reshaped to the frame
/// of the argument: `+ b. 1` is `0 $~ }.@$`.
fn identity_spelling(d: &Data) -> String {
    let one = Array::new(Vec::new(), d.slice(0, 1));
    let text = crate::fmt::format_array(&one, &crate::fmt::FmtOpts::J);
    format!("{} $~ }}.@$", text.trim())
}

/// Run `f` with `⍺⍺` and `⍵⍵` naming the operands a user-written operator
/// was given, and with whatever they named before put back afterwards.
fn with_operands<R>(
    alpha: &Verb,
    omega: Option<&Verb>,
    ctx: &mut Ctx<'_>,
    f: impl FnOnce(&mut Ctx<'_>) -> Result<R>,
) -> Result<R> {
    let saved = (ctx.env.verb("⍺⍺").cloned(), ctx.env.verb("⍵⍵").cloned());
    ctx.env.define("⍺⍺".to_string(), alpha.clone());
    if let Some(g) = omega {
        ctx.env.define("⍵⍵".to_string(), g.clone());
    }
    let out = f(ctx);
    match saved.0 {
        Some(v) => ctx.env.define("⍺⍺".to_string(), v),
        None => ctx.env.undefine("⍺⍺"),
    }
    match saved.1 {
        Some(v) => ctx.env.define("⍵⍵".to_string(), v),
        None => ctx.env.undefine("⍵⍵"),
    }
    out
}

/// True for APL's MIXED SIMPLE array: every element is a simple scalar,
/// and no one type holds all of them. libjay keeps such an array as boxed
/// scalars, but its depth is 1 and nothing may open it further.
fn is_mixed_simple(a: &Array) -> bool {
    let Some(items) = a.as_boxes() else { return false };
    if items.is_empty() || items.iter().any(|b| b.rank() != 0 || b.dtype() == DType::Box) {
        return false;
    }
    let mut common = Some(items[0].dtype());
    for b in &items[1..] {
        common = common.and_then(|t| DType::promote(t, b.dtype()));
    }
    common.is_none()
}

/// APL `⊆ y` (Dyalog): nest — y enclosed, unless it already is nested or
/// is a simple scalar, neither of which enclosing changes.
fn nest(y: &Array) -> Array {
    if y.dtype() == DType::Box || y.rank() == 0 {
        return y.clone();
    }
    Array::boxed(y.clone())
}

/// APL `f⌸ y` and `x f⌸ y` (Dyalog's key): the distinct major cells of the
/// left argument, in first-occurrence order, each paired with what shares
/// it — the positions it occupies, or the right argument's items there.
fn key_pairs(
    u: &Verb,
    keys: &Array,
    values: Option<&Array>,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    let base = if keys.rank() == 0 { Array::new(vec![1], keys.data.clone()) } else { keys.clone() };
    let n = base.items();
    if let Some(v) = values && v.items() != n {
        return Err(Error::new(
            ErrorKind::Length,
            format!("{n} key(s) for {} item(s)", v.items()),
            Some(span),
        ));
    }
    let groups = group_positions(&base, ctx.cfg.tol);
    let origin = ctx.cfg.rules.origin;
    let mut cells = Vec::with_capacity(groups.len());
    for (first, at) in &groups {
        let key = item_or_self(&base, *first);
        let group = match values {
            Some(v) => select_items(v, at),
            None => Array::from_i64(at.iter().map(|&i| origin + i as i64).collect()),
        };
        // A dfn that never names `⍺` has no dyadic valence; the key is
        // then of no use to it and the group is all it is given.
        let monadic = matches!(u, Verb::Explicit(d) if d.left.is_none());
        cells.push(if monadic {
            u.monad(&group, ctx, span)?
        } else {
            u.dyad(&key, &group, ctx, span)?
        });
    }
    let count = cells.len();
    assemble(&[count], cells, span)
}

/// The distinct items of `y`, each as (its first position, every position
/// it holds), in first-occurrence order.
fn group_positions(y: &Array, tol: Tol) -> Vec<(usize, Vec<usize>)> {
    let n = y.items();
    let m = y.item_size();
    // Exact equality is an equivalence a hash stands in for, so the groups
    // come out of one pass. Tolerant equality is not one, and neither a box
    // nor an exact number has a cheap key: those are compared by content,
    // each item against the distinct ones already found.
    let hashable = match y.dtype() {
        DType::Box | DType::Ext | DType::Rat => false,
        DType::F64 | DType::Complex => tol.ct == 0.0,
        _ => true,
    };
    if hashable {
        return if m == 1 {
            group_by_key(n, |i| elem_key(&y.data, i))
        } else {
            group_by_key(n, |i| (0..m).map(|k| elem_key(&y.data, i * m + k)).collect::<Vec<u64>>())
        };
    }
    let mut keys: Vec<Array> = Vec::new();
    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    for i in 0..n {
        let item = y.item(i);
        match keys.iter().position(|k| arrays_match(k, &item, tol)) {
            Some(at) => groups[at].1.push(i),
            None => {
                keys.push(item);
                groups.push((i, vec![i]));
            }
        }
    }
    groups
}

/// The positions `0 .. n`, grouped by the key each of them has, in the
/// order the keys first appear: one hash lookup per position, not one
/// comparison per position per group.
fn group_by_key<K, F>(n: usize, key: F) -> Vec<(usize, Vec<usize>)>
where
    K: Eq + std::hash::Hash,
    F: Fn(usize) -> K,
{
    use std::collections::hash_map::Entry;
    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut at: HashMap<K, usize, KeyHash> =
        HashMap::with_capacity_and_hasher(n.min(1 << 16), KeyHash);
    for i in 0..n {
        match at.entry(key(i)) {
            Entry::Occupied(e) => groups[*e.get()].1.push(i),
            Entry::Vacant(e) => {
                e.insert(groups.len());
                groups.push((i, vec![i]));
            }
        }
    }
    groups
}

/// The hasher the grouping uses. Its keys are [`elem_key`] values, which
/// already spread a value across the whole of a `u64`, so mixing them costs
/// a multiply where the default hasher runs a block cipher over them.
/// Nothing here is exposed to a chosen key, which is what that default is
/// for.
#[derive(Clone, Copy, Default)]
struct KeyHash;

impl std::hash::BuildHasher for KeyHash {
    type Hasher = KeyHasher;
    fn build_hasher(&self) -> KeyHasher {
        KeyHasher(0)
    }
}

struct KeyHasher(u64);

impl std::hash::Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        let mut x = self.0;
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^ (x >> 29)
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
}

/// APL `x ⍕ y`: format by specification. `x` is one width-and-precision
/// pair per column of y's last axis, one pair for all of them, or a lone
/// precision, which takes the width the values need plus a separating
/// blank. A value that does not fit its width is a domain error, as the
/// reference has it.
fn format_spec(x: &Array, y: &Array, fmt: &FmtOpts, span: Span) -> Result<Array> {
    let spec = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("a format specification is whole numbers", span))?;
    if y.dtype() == DType::Box {
        return Err(Error::not_yet("format by specification of a nested array", span));
    }
    let cols = if y.rank() == 0 { 1 } else { y.shape[y.rank() - 1] };
    let rows = y.count() / cols.max(1);
    // One number is a precision alone; pairs are width and precision.
    let pairs: Vec<(Option<i64>, i64)> = match spec.len() {
        1 => vec![(None, spec[0]); cols],
        2 => vec![(Some(spec[0]), spec[1]); cols],
        n if n == 2 * cols => spec.chunks(2).map(|c| (Some(c[0]), c[1])).collect(),
        n => {
            return Err(Error::new(
                ErrorKind::Length,
                format!("{n} specification value(s) for {cols} column(s)"),
                Some(span),
            ));
        }
    };
    if pairs.iter().any(|&(w, p)| w.is_some_and(|w| w < 0) || p < 0) {
        return Err(Error::domain("a format width and precision are nonnegative", span));
    }
    // A width and a precision are lengths, and a written number is free to
    // ask for more characters than any machine holds. The ceiling applies
    // here as it does to a shape.
    for &(w, p) in &pairs {
        crate::limits::count(w.unwrap_or(0) as u128, span)?;
        crate::limits::count(p as u128, span)?;
    }
    let numbers = y.to_f64_vec();
    let text = |i: usize, p: i64| -> String {
        match (&y.data, &numbers) {
            (Data::Char(v), _) => v[i].to_string(),
            (_, Some(v)) => {
                let s = format!("{:.*}", p as usize, v[i]);
                if v[i] < 0.0 { format!("{}{}", fmt.neg, &s[1..]) } else { s }
            }
            _ => String::new(),
        }
    };
    if y.dtype() != DType::Char && numbers.is_none() {
        return Err(Error::domain("format by specification takes numbers or characters", span));
    }
    // A width the caller did not give is the widest value plus a blank.
    let widths: Vec<usize> = pairs
        .iter()
        .enumerate()
        .map(|(c, &(w, p))| match w {
            Some(w) => w as usize,
            None => {
                (0..rows).map(|r| text(r * cols + c, p).chars().count()).max().unwrap_or(0) + 1
            }
        })
        .collect();
    let line = crate::limits::count(widths.iter().map(|&w| w as u128).sum(), span)?;
    let total = crate::limits::count(rows as u128 * line as u128, span)?;
    let mut out: Vec<char> = Vec::with_capacity(total);
    for r in 0..rows {
        for c in 0..cols {
            let s = text(r * cols + c, pairs[c].1);
            let len = s.chars().count();
            if len > widths[c] {
                return Err(Error::domain(
                    format!("{s} does not fit a field {} wide", widths[c]),
                    span,
                ));
            }
            out.extend(std::iter::repeat_n(' ', widths[c] - len));
            out.extend(s.chars());
        }
    }
    let mut shape = if y.rank() == 0 { Vec::new() } else { y.shape[..y.rank() - 1].to_vec() };
    shape.push(line);
    Ok(Array::new(shape, Data::Char(out.into())))
}

/// J `x ;: y`: the sequential machine.
///
/// x is the boxed description `f ; s ; m ; ijrd`, of which `m` and `ijrd`
/// may be left off. `s` is the transition table, shaped `p q 2`: at state
/// `r` and input class `c`, `s[r;c;0]` is the state to go to and
/// `s[r;c;1]` the output code — 0 nothing, 1 start a word here, 2 end a
/// word and start another, 3 end a word, 6 stop. `m` maps an input element
/// to its class, indexed by the character's codepoint; with none, a
/// numeric argument IS the classes. `ijrd` is the starting position, the
/// starting word (`_1` for none), the starting state and what to do with
/// the end of the input: a class to make one last transition with, or `_1`
/// to end the word in hand. `f` picks the answer: 0 the boxed words, 1
/// their elements catenated, 2 each word's position and length, 3 the
/// table position that ended it, 4 both, 5 the whole trace.
fn sequential_machine(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let Some(parts) = x.as_boxes() else {
        return Err(Error::domain("a sequential machine is a boxed description", span));
    };
    if x.rank() > 1 || !(2..=4).contains(&parts.len()) {
        return Err(Error::domain(
            "a sequential machine is 2 to 4 boxes: f ; s ; m ; ijrd",
            span,
        ));
    }
    let whole = |a: &Array, what: &str| -> Result<Vec<i64>> {
        a.to_i64_vec().ok_or_else(|| Error::domain(format!("{what} is whole numbers"), span))
    };
    let form = *whole(&parts[0], "a sequential machine's result form")?
        .first()
        .ok_or_else(|| Error::domain("a sequential machine needs a result form", span))?;
    if !(0..=5).contains(&form) {
        return Err(Error::domain(format!("{form} is not a result form of 0 to 5"), span));
    }
    let table = &parts[1];
    if table.rank() != 3 || table.shape[2] != 2 {
        return Err(Error::new(
            ErrorKind::Rank,
            "a sequential machine's transition table is shaped p q 2",
            Some(span),
        ));
    }
    let (states, classes) = (table.shape[0], table.shape[1]);
    let entries = whole(table, "a transition table")?;
    let map = parts.get(2).filter(|a| a.count() > 0);
    let start = match parts.get(3) {
        Some(a) => whole(a, "a sequential machine's starting values")?,
        None => Vec::new(),
    };
    let start = if start.is_empty() { vec![0, -1, 0, -1] } else { start };
    if start.len() != 4 {
        return Err(Error::new(
            ErrorKind::Length,
            "a sequential machine starts from four values: i j r d",
            Some(span),
        ));
    }
    let (mut i, mut word, mut state, ending) = (start[0], start[1], start[2], start[3]);
    let n = y.count() as i64;

    // The class of the element at `at`: read through the map where there
    // is one, and the element itself where there is not.
    let codes: Option<Vec<i64>> = match map {
        Some(m) => Some(whole(m, "a sequential machine's map")?),
        None => None,
    };
    let values: Vec<i64> = match (&y.data, &codes) {
        (Data::Char(v), Some(_)) => v.as_slice().iter().map(|&c| c as i64).collect(),
        (_, None) => y
            .to_i64_vec()
            .ok_or_else(|| Error::domain("a sequential machine over characters needs a map", span))?,
        _ => {
            return Err(Error::not_yet(
                "a sequential machine's map over a numeric argument (x's third box)",
                span,
            ));
        }
    };
    let class_at = |at: i64| -> Result<i64> {
        let raw = values[at as usize];
        let Some(m) = &codes else { return Ok(raw) };
        if raw < 0 || raw as usize >= m.len() {
            return Err(Error::new(
                ErrorKind::Domain,
                format!("{raw} is outside a map of {} entries", m.len()),
                Some(span),
            ));
        }
        Ok(m[raw as usize])
    };

    let mut trace: Vec<i64> = Vec::new();
    let mut words: Vec<(i64, i64, i64)> = Vec::new();
    let mut emit = |word: i64, at: i64, place: i64| -> Result<()> {
        if word < 0 {
            return Err(Error::new(
                ErrorKind::Domain,
                "a sequential machine ended a word before one had begun",
                Some(span),
            ));
        }
        words.push((word, at - word, place));
        Ok(())
    };
    loop {
        let class = if i < n {
            class_at(i)?
        } else if ending >= 0 {
            ending
        } else {
            // The input is spent and the end asks for no transition: what
            // is in hand is the last word. The reference gives it the table
            // position class 0 in the state reached would have.
            if word >= 0 {
                emit(word, i, classes as i64 * state)?;
            }
            break;
        };
        if state < 0 || state as usize >= states || class < 0 || class as usize >= classes {
            return Err(Error::new(
                ErrorKind::Domain,
                format!(
                    "state {state} and class {class} are outside a {states} by {classes} table"
                ),
                Some(span),
            ));
        }
        let at = (state as usize * classes + class as usize) * 2;
        let (next, code) = (entries[at], entries[at + 1]);
        trace.extend_from_slice(&[i, word, state, class, next, code]);
        let place = class + classes as i64 * state;
        state = next;
        match code {
            0 => {}
            1 => word = i,
            2 => {
                emit(word, i, place)?;
                word = i;
            }
            3 => {
                emit(word, i, place)?;
                word = -1;
            }
            4 | 5 => {
                return Err(Error::not_yet(
                    "a sequential machine's vector output (codes 4 and 5)",
                    span,
                ));
            }
            6 => break,
            other => {
                return Err(Error::domain(
                    format!("{other} is not a sequential machine output code"),
                    span,
                ));
            }
        }
        if i >= n {
            break;
        }
        i += 1;
    }
    Ok(sequential_result(form, &words, &trace, y))
}

/// The answer a sequential machine's result form asks for, out of the words
/// it marked off and the trace it left.
fn sequential_result(form: i64, words: &[(i64, i64, i64)], trace: &[i64], y: &Array) -> Array {
    let piece = |&(at, len, _): &(i64, i64, i64)| {
        Array::new(vec![len as usize], y.data.slice(at as usize, (at + len) as usize))
    };
    match form {
        0 => Array::new(
            vec![words.len()],
            Data::Box(words.iter().map(piece).collect::<Vec<_>>().into()),
        ),
        1 => {
            let mut data = Data::empty(y.dtype());
            for w in words {
                data.extend_from(&piece(w).data);
            }
            let n = data.len();
            Array::new(vec![n], data)
        }
        2 => Array::new(
            vec![words.len(), 2],
            Data::I64(words.iter().flat_map(|&(at, len, _)| [at, len]).collect::<Vec<_>>().into()),
        ),
        3 => Array::from_i64(words.iter().map(|&(_, _, place)| place).collect()),
        4 => Array::new(
            vec![words.len(), 3],
            Data::I64(
                words
                    .iter()
                    .flat_map(|&(at, len, place)| [at, len, place])
                    .collect::<Vec<_>>()
                    .into(),
            ),
        ),
        _ => Array::new(vec![trace.len() / 6, 6], Data::I64(trace.to_vec().into())),
    }
}

/// J `x ". y`: the numbers the characters of y spell, with x standing in
/// for every blank-separated word that is not a number. y arrives as one
/// line — the verb's right rank is 1 — so a character matrix is read a row
/// at a time and the rows are framed back together.
fn parse_numbers(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let Data::Char(text) = &y.data else {
        return Err(Error::domain("reading numbers from text needs characters", span));
    };
    if x.count() != 1 {
        return Err(Error::new(
            ErrorKind::Rank,
            "the stand-in for an unreadable word is one value",
            Some(span),
        ));
    }
    let line: String = text.as_slice().iter().collect();
    crate::frontend::j::numbers_from_text(&line, x)
        .ok_or_else(|| Error::domain("the stand-in for an unreadable word is a number", span))
}

/// One field of J's `x ": y`, without its padding: `w j d` says how wide
/// the field is and how many digits follow the point, and a NEGATIVE width
/// asks for the exponential form instead of the fixed one.
fn format_field(value: f64, precision: usize, exponential: bool, neg: char) -> String {
    let sign = |s: String| match s.strip_prefix('-') {
        // A value that rounds to nothing keeps no sign, as the reference
        // has it: `5j2 ": _0.001` is ` 0.00`.
        Some(rest) if rest.bytes().all(|b| !b.is_ascii_digit() || b == b'0') => rest.to_string(),
        Some(rest) => format!("{neg}{rest}"),
        None => s,
    };
    if !exponential {
        return sign(format!("{value:.precision$}"));
    }
    // `1.500e3`, `1.234e_4`: the mantissa to the asked-for precision, then
    // the exponent written as J writes an integer.
    let text = format!("{value:.precision$e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((text.as_str(), "0"));
    let exponent = match exponent.strip_prefix('-') {
        Some(rest) => format!("{neg}{rest}"),
        None => exponent.to_string(),
    };
    format!("{}e{exponent}", sign(mantissa.to_string()))
}

/// J `x ": y`: format by specification.
///
/// x is one complex `w j d` per column of y's last axis, or one for all of
/// them: `w` is the field width and `d` the digits after the point. A width
/// of zero takes whatever the column needs, with a blank between it and the
/// column before. A value too wide for its field is written as asterisks
/// rather than refused, which is what the reference does.
fn format_spec_j(x: &Array, y: &Array, fmt: &FmtOpts, span: Span) -> Result<Array> {
    let Some(spec) = x.to_complex_vec() else {
        return Err(Error::domain("a format specification is numbers", span));
    };
    if y.dtype() == DType::Box {
        return Err(Error::domain("format by specification takes numbers", span));
    }
    let Some(values) = y.to_f64_vec() else {
        return Err(Error::domain("format by specification takes numbers", span));
    };
    let cols = if y.rank() == 0 { 1 } else { y.shape[y.rank() - 1] };
    let rows = if cols == 0 { 0 } else { y.count() / cols };
    let fields: Vec<[f64; 2]> = match spec.len() {
        1 => vec![spec[0]; cols],
        n if n == cols => spec,
        n => {
            return Err(Error::new(
                ErrorKind::Length,
                format!("{n} specification value(s) for {cols} column(s)"),
                Some(span),
            ));
        }
    };
    // A width and a digit count are lengths, and a written number is free
    // to ask for more characters than any machine holds. The ceiling
    // applies here as it does to a shape: refuse the request instead of
    // handing the product to an allocator.
    for &[w, d] in &fields {
        crate::limits::count(w.abs() as u128, span)?;
        crate::limits::count(d.max(0.0) as u128, span)?;
    }
    let text = |r: usize, c: usize| {
        let [w, d] = fields[c];
        // Only a column of automatic width renders every digit asked for.
        // Where the width is given, a digit count that reaches it already
        // overflows the field — the point and the digits alone are wider —
        // so rendering past that point cannot change the answer.
        let digits = if w == 0.0 { d.max(0.0) } else { d.max(0.0).min(w.abs()) };
        format_field(values[r * cols + c], digits as usize, w < 0.0, fmt.neg)
    };
    // A width of zero is the widest value in the column, and a blank
    // between it and whatever stands to its left.
    let widths: Vec<usize> = (0..cols)
        .map(|c| {
            let w = fields[c][0];
            if w != 0.0 {
                return w.abs() as usize;
            }
            let wide = (0..rows).map(|r| text(r, c).chars().count()).max().unwrap_or(0);
            wide + usize::from(c > 0)
        })
        .collect();
    let line = crate::limits::count(widths.iter().map(|&w| w as u128).sum(), span)?;
    let total = crate::limits::count(rows as u128 * line as u128, span)?;
    let mut out: Vec<char> = Vec::with_capacity(total);
    for r in 0..rows {
        for c in 0..cols {
            let s = text(r, c);
            // The exponential form is written from the LEFT, one column of
            // sign in front of it; the fixed one is right-justified.
            let (lead, body) = match (fields[c][0] < 0.0, s.strip_prefix(fmt.neg)) {
                (false, _) => (String::new(), s.as_str()),
                (true, Some(rest)) => (fmt.neg.to_string(), rest),
                (true, None) => (" ".to_string(), s.as_str()),
            };
            let len = lead.chars().count() + body.chars().count();
            if len > widths[c] {
                out.extend(std::iter::repeat_n('*', widths[c]));
                continue;
            }
            if fields[c][0] < 0.0 {
                out.extend(lead.chars());
                out.extend(body.chars());
                out.extend(std::iter::repeat_n(' ', widths[c] - len));
            } else {
                out.extend(std::iter::repeat_n(' ', widths[c] - len));
                out.extend(body.chars());
            }
        }
    }
    let mut shape = if y.rank() == 0 { Vec::new() } else { y.shape[..y.rank() - 1].to_vec() };
    shape.push(line);
    Ok(Array::new(shape, Data::Char(out.into())))
}

/// APL `⍳ y`: the indices of an array whose shape is y. One length gives
/// the plain counting vector; two or more give an array of that shape whose
/// elements are the boxed coordinate vectors.
fn iota_apl(y: &Array, origin: i64, near: NearInt, span: Span) -> Result<Array> {
    if y.rank() > 1 {
        return Err(Error::new(
            ErrorKind::Rank,
            "the index generator takes a shape, which is a scalar or a vector",
            Some(span),
        ));
    }
    let dims = y
        .to_i64_vec_near(near)
        .ok_or_else(|| Error::domain("index generator needs an integer argument", span))?;
    if dims.iter().any(|&n| n < 0) {
        return Err(Error::domain("index generator needs nonnegative lengths", span));
    }
    if dims.len() <= 1 {
        let n = dims.first().copied().unwrap_or(0);
        crate::limits::count(n as u128, span)?;
        return Ok(Array::from_i64((0..n).map(|i| origin + i).collect()));
    }
    let shape: Vec<usize> = dims.iter().map(|&n| n as usize).collect();
    let total = crate::limits::elements(&shape, span)?;
    let mut cells = Vec::with_capacity(total);
    let mut coord = vec![0usize; shape.len()];
    for _ in 0..total {
        cells.push(Array::from_i64(coord.iter().map(|&c| origin + c as i64).collect()));
        odometer(&mut coord, &shape);
    }
    Ok(Array::new(shape, Data::Box(cells.into())))
}

/// J carries an argument's exactness into the verbs that answer with
/// counts and digits: `$`, `#`, `#.`, `#:`, `p:` and `q:` of an extended or
/// rational argument answer with extended integers, not machine ones. The
/// values are the same either way; only the type differs, and J's own
/// `3!:0` reports it.
fn carry_exact(result: Array, y: &Array) -> Array {
    if !matches!(y.dtype(), DType::Ext | DType::Rat) {
        return result;
    }
    match result.data.cast(DType::Ext) {
        Some(data) => Array::new(result.shape, data),
        None => result,
    }
}

fn carry_exact2(result: Array, x: &Array, y: &Array) -> Array {
    let widened = carry_exact(result, x);
    carry_exact(widened, y)
}

/// `m b.`: one of the sixteen boolean functions of two bits, and — sixteen
/// higher — the same function applied to every bit of a pair of integers.
fn truth_table(m: u8, x: &Array, y: &Array, span: Span) -> Result<Array> {
    let table = m & 15;
    let bit = |a: i64, b: i64| ((table >> (3 - (2 * a + b))) & 1) as i64;
    let xs = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("a boolean function takes integers", span))?;
    let ys = y
        .to_i64_vec()
        .ok_or_else(|| Error::domain("a boolean function takes integers", span))?;
    let (a, b) = (xs.first().copied().unwrap_or(0), ys.first().copied().unwrap_or(0));
    if m < 16 {
        if !(0..=1).contains(&a) || !(0..=1).contains(&b) {
            return Err(Error::domain(
                format!("{m} b. takes 0 and 1; {m} b. + 16 is the same function on every bit"),
                span,
            ));
        }
        return Ok(Array::scalar_bool(bit(a, b) != 0));
    }
    let mut out = 0i64;
    for k in 0..64 {
        if bit((a >> k) & 1, (b >> k) & 1) != 0 {
            out |= 1i64 << k;
        }
    }
    Ok(Array::scalar_i64(out))
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
    near: NearInt,
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
    // The positions below are row-major offsets into both buffers, so a
    // column-major one is laid out before it is read or written.
    if !base.is_row_major() || !value.is_row_major() {
        let (b, v) = (base.to_row_major(), value.to_row_major());
        return amend_at(&b, slots, &v, origin, near, span);
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
        let Some(values) = idx.to_i64_vec_near(near) else {
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
        .to_i64_vec_near(ctx.cfg.near())
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

/// `` m`:0 `` and `` m`:3 ``, the two evoke-gerund forms that are not a
/// train. `0` applies every verb of the gerund to the arguments and frames
/// the answers; `3` inserts the verbs between the items of y, taking them
/// left to right and cycling, and folds right to left as insert does.
fn evoke(
    vs: &[Verb],
    form: i64,
    x: Option<&Array>,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    if vs.is_empty() {
        return Err(Error::domain("an evoked gerund is empty", span));
    }
    if form == 0 {
        let mut cells = Vec::with_capacity(vs.len());
        for v in vs {
            cells.push(match x {
                None => v.monad(y, ctx, span)?,
                Some(x) => v.dyad(x, y, ctx, span)?,
            });
        }
        return assemble(&[vs.len()], cells, span);
    }
    if x.is_some() {
        return Err(Error::domain("m`:3 has no dyadic meaning", span));
    }
    let items = if y.rank() == 0 { vec![y.clone()] } else { y.cells(1) };
    let Some((last, rest)) = items.split_last() else {
        return Err(Error::domain("m`:3 needs an argument with items", span));
    };
    let mut acc = last.clone();
    for (i, item) in rest.iter().enumerate().rev() {
        acc = vs[i % vs.len()].dyad(item, &acc, ctx, span)?;
    }
    Ok(acc)
}

/// `(f⌺w) y` (Dyalog's stencil): the window of `w` cells centred on each
/// cell of y, with the edges filled, and f applied to each. There is one
/// size per leading axis of y and the axes past them travel whole, so the
/// answer is framed by the axes the windows moved along.
fn stencil(u: &Verb, w: &[i64], y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    if w.len() > y.rank() {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("a stencil of {} axis/axes into a rank-{} value", w.len(), y.rank()),
            Some(span),
        ));
    }
    if w.iter().any(|&n| n <= 0) {
        return Err(Error::domain("a stencil window is a positive size", span));
    }
    let y = y.to_row_major();
    let k = w.len();
    let st = strides(&y.shape);
    let frame: Vec<usize> = y.shape[..k].to_vec();
    // The window's own shape: the sizes, then whatever the cell carries.
    let mut wshape: Vec<usize> = w.iter().map(|&n| n as usize).collect();
    wshape.extend_from_slice(&y.shape[k..]);
    let inner: usize = y.shape[k..].iter().product();
    let total: usize = frame.iter().product();
    let mut cells = Vec::with_capacity(total);
    let mut at = vec![0usize; frame.len()];
    let mut coord = vec![0usize; k];
    for _ in 0..total {
        let mut data = Data::empty(y.dtype());
        coord.iter_mut().for_each(|c| *c = 0);
        let count: usize = w.iter().map(|&n| n as usize).product();
        for _ in 0..count {
            let mut base = 0usize;
            let mut inside = true;
            for a in 0..k {
                let off = at[a] as i64 + coord[a] as i64 - (w[a] - 1) / 2;
                if off < 0 || off >= y.shape[a] as i64 {
                    inside = false;
                    break;
                }
                base += off as usize * st[a];
            }
            for j in 0..inner {
                if inside {
                    push_elem(&mut data, &y.data, base + j);
                } else {
                    data.push_fill();
                }
            }
            odometer(&mut coord, &wshape[..k]);
        }
        cells.push(u.monad(&Array::new(wshape.clone(), data), ctx, span)?);
        odometer(&mut at, &frame);
    }
    assemble(&frame, cells, span)
}

/// `x u\. y`: u applied to y with every run of x consecutive items removed.
/// A run of x items has `1 + (#y) - x` places to sit, and that is how many
/// results there are.
fn outfix(u: &Verb, x: &Array, y: &Array, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let k = one_int(x, "an outfix width", ctx.cfg.near(), span)?;
    let n = y.items() as i64;
    let list = as_list(y);
    // A positive width leaves out every run of x consecutive items, so
    // there are `1 + n - x` of them and none at all once x is longer than
    // the argument. A negative one leaves out NON-OVERLAPPING runs, the
    // last of them short where the length does not divide.
    // The widths are the program's own numbers, so the arithmetic that
    // turns one into a list of starts runs in i128: `_9223372036854775808`
    // has no negation in i64, and `n + step` overflows for a large step.
    let starts: Vec<i64> = if k < 0 {
        let step = i128::from(k.unsigned_abs());
        let count = (i128::from(n) + step - 1) / step;
        (0..count).map(|i| (i * step) as i64).collect()
    } else {
        (0..=(n - k)).collect()
    };
    let width = k.unsigned_abs() as usize;
    // J holds the operand to its own domain over the WHOLE argument, not
    // only over the pieces it folds: `_2 +/\. 'ab'` is a domain error
    // although every piece left behind is empty, and so is `4 +/\. 'abc'`,
    // which has no piece at all. Only an argument of one item or none
    // escapes it, and then only where no piece is folded. Numeric data
    // never fails the check, so the probe is spent on characters and boxes
    // alone -- and on nothing at all when the operand is not pure, since a
    // verb that writes must not write twice.
    if !list.dtype().is_numeric()
        && u.is_pure()
        && n >= 1
        && (n >= 2 || !starts.is_empty())
    {
        // The question is whether the operand has a MEANING for this data,
        // and a fold of one item answers nothing: `+/ ,'a'` is that one
        // character, applying `+` to nothing. So an argument of one item is
        // asked with that item twice, which is the smallest fold that
        // really applies the operand.
        let probe =
            if n == 1 { select_items(&list, &[0, 0]) } else { list.clone() };
        u.monad(&probe, ctx, span)?;
    }
    // A width longer than the argument leaves no place for the run to sit.
    // The one run an empty argument has is the argument itself, and that is
    // the cell whose shape the answer keeps.
    if starts.is_empty() {
        let cell = u.is_pure().then(|| select_items(&list, &[]));
        return Ok(empty_frame(&[0], list.dtype(), cell, ctx, |cell, c| u.monad(cell, c, span)));
    }
    let mut cells = Vec::with_capacity(starts.len());
    for start in starts {
        let start = start as usize;
        let keep: Vec<usize> =
            (0..n as usize).filter(|&i| i < start || i >= start + width).collect();
        cells.push(u.monad(&select_items(&list, &keep), ctx, span)?);
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
            // Every one of these is its own inverse, whichever language
            // spelled it: the verb itself is the answer, so no name is
            // looked up (an APL glyph has no entry in J's table).
            if matches!(
                p.monad,
                MonadOp::Scalar(SM::Conj | SM::Neg | SM::Recip | SM::OneMinus)
                    | MonadOp::Reverse
                    | MonadOp::TransposeAxes
            ) {
                return Some(v.clone());
            }
            // `j. y` turns y a quarter turn about the origin; turning it
            // back is a quarter turn the other way, which is `-@j.`.
            if matches!(p.monad, MonadOp::Scalar(SM::Imaginary)) {
                return Some(Verb::Atop(Box::new(swap("-")?), Box::new(swap("j.")?)));
            }
            // `x # y` undone with the same x is the expansion: the items
            // come back where the ones stand and a fill takes every place a
            // zero left. It has no monadic meaning, since `# y` counts and
            // a count says nothing about what was counted.
            if p.dyad == DyadOp::Copy {
                return Some(Verb::Prim(Prim {
                    name: "#^:_1",
                    monad: MonadOp::None,
                    dyad: DyadOp::Expand,
                    ranks: [RANK_INF, 1, RANK_INF],
                }));
            }
            let by_monad: Option<&'static str> = match p.monad {
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
        // Adding or multiplying is undone by taking the noun off the
        // RIGHT, whichever side it was bonded to: `2&+` is undone by `-&2`
        // and not by `2&-`.
        (SD::Add, _) => Some(Verb::BondRight(Box::new(named("-")?), n.clone())),
        (SD::Mul, _) => Some(Verb::BondRight(Box::new(named("%")?), n.clone())),
        (SD::Sub, false) => bond("+", n),
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

/// `~: y` / `≠ y`: 1 where a value has not been seen before.
///
/// The two languages count different things. J's sieve runs over ITEMS and
/// answers one bit per item, so a matrix gives a vector. APL's runs over
/// the ELEMENTS in ravel order and keeps the argument's own shape, so a
/// matrix gives a matrix and a scalar gives a scalar.
fn nub_sieve(y: &Array, tol: Tol, lang: crate::Lang) -> Array {
    let by_element = lang == crate::Lang::Apl;
    let n = if by_element {
        y.count()
    } else if y.rank() == 0 {
        1
    } else {
        y.items()
    };
    let mut seen: Vec<Array> = Vec::new();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let cell = if by_element {
            Array::new(Vec::new(), y.data.slice(i, i + 1))
        } else {
            item_or_self(y, i)
        };
        let fresh = !seen.iter().any(|s| arrays_match(s, &cell, tol));
        if fresh {
            seen.push(cell);
        }
        out.push(fresh as u8);
    }
    let shape = if by_element { y.shape.clone() } else { vec![n] };
    Array::new(shape, Data::Bool(out.into()))
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

/// APL's set functions read their arguments as lists and refuse anything
/// deeper: `1 2∩2 3⍴⍳6` is a RANK ERROR where J's `-.` and `~.` would work
/// on the items of a table.
fn set_rank(cfg: EvalCfg, what: &str, x: &Array, y: &Array, span: Span) -> Result<()> {
    if cfg.rules.lang == crate::Lang::Apl && (x.rank() > 1 || y.rank() > 1) {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("{what} takes vectors, not rank {} and rank {}", x.rank(), y.rank()),
            Some(span),
        ));
    }
    Ok(())
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
/// The answer is shaped like y, and the search runs over all of y's axes at
/// once, so a table is looked for inside a table. A pattern that would run
/// off an edge matches nowhere; an EMPTY pattern matches everywhere, being
/// a run of no elements.
///
/// The two languages align the pattern differently: J wants the two ranks
/// to agree, counting a scalar pattern as a one-element list, while APL
/// pads the pattern with leading axes of one and takes any rank up to y's.
fn find_seq(x: &Array, y: &Array, tol: Tol, apl: bool, span: Span) -> Result<Array> {
    let (xr, yr) = (x.rank(), y.rank());
    // J reads an atom as a one-item list on BOTH sides, so a pattern of
    // one atom has exactly one place to sit in an argument of one atom:
    // `0 E. 5` is 0 and `1 E. 1` is 1, both of them scalars.
    if !apl && xr == 0 && yr == 0 {
        let hit = arrays_match(x, y, tol);
        return Ok(Array::new(Vec::new(), Data::Bool(vec![u8::from(hit)].into())));
    }
    if apl && xr > yr {
        // A pattern with more axes than the argument fits nowhere in it.
        return Ok(Array::new(y.shape.clone(), Data::Bool(vec![0u8; y.count()].into())));
    }
    if !apl && xr.max(1) != yr {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("a rank-{xr} pattern in a rank-{yr} argument"),
            Some(span),
        ));
    }
    let mut pattern = vec![1usize; yr];
    pattern[yr - xr..].copy_from_slice(&x.shape);
    let n = y.count();
    let mut out = vec![0u8; n];
    let (xrm, yrm) = (x.to_row_major(), y.to_row_major());
    let yst = strides(&y.shape);
    let cells: usize = pattern.iter().product();
    let mut at = vec![0usize; yr];
    for slot in out.iter_mut() {
        if (0..yr).all(|a| at[a] + pattern[a] <= y.shape[a]) {
            let mut off = vec![0usize; yr];
            let mut hit = true;
            for k in 0..cells {
                let i: usize = (0..yr).map(|a| (at[a] + off[a]) * yst[a]).sum();
                if !arrays_match(&atom(&xrm, k), &atom(&yrm, i), tol) {
                    hit = false;
                    break;
                }
                odometer(&mut off, &pattern);
            }
            *slot = hit as u8;
        }
        odometer(&mut at, &y.shape);
    }
    Ok(Array::new(y.shape.clone(), Data::Bool(out.into())))
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
fn item_ranks(y: &Array, rules: Rules, span: Span) -> Result<Vec<usize>> {
    check_gradable(y, rules, span)?;
    if !y.dtype().is_numeric() {
        return Err(Error::domain("an anagram index needs numbers", span));
    }
    let order = grade_order(&as_list(y), false, Grading::of(rules, rules.tol()));
    let mut ranks = vec![0usize; order.len()];
    for (place, &i) in order.iter().enumerate() {
        ranks[i] = place;
    }
    Ok(ranks)
}

/// `A. y`: where the permutation y's items rank as stands in the
/// lexicographic list of the permutations of that length.
fn anagram_index(y: &Array, rules: Rules, span: Span) -> Result<Array> {
    let ranks = item_ranks(y, rules, span)?;
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
fn anagram_from(x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    let idx = x
        .to_i64_vec_near(near)
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
/// is a permutation and answers its cycles. A list shorter than the
/// permutation it names stands for one over `1 + >./ y` items, so
/// `C. 3 4 2` is the cycles of `0 1 3 4 2`.
fn cycle_form(y: &Array, near: NearInt, span: Span) -> Result<Array> {
    if y.dtype() == DType::Box {
        let perm = cycles_to_direct(y, near, span)?;
        return Ok(Array::from_i64(perm.iter().map(|&i| i as i64).collect()));
    }
    let n = permutation_span(y, near, span)?;
    let perm = direct_permutation_of(y, n, near, span)?;
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

/// A direct permutation of `n` items, from a list that may be shorter than
/// one. A short list is J's ABBREVIATED permutation: the items it never
/// mentions come first, in ascending order, and the list itself is the
/// tail. `3 4 2` over five items is `0 1 3 4 2`; `2` over five is the same
/// permutation again, and `2 3` over four is the identity.
///
/// `n` is the count the context supplies — the length of the argument being
/// permuted, or for `C. y` one past the largest index the list names.
fn direct_permutation_of(y: &Array, n: usize, near: NearInt, span: Span) -> Result<Vec<usize>> {
    let v = y
        .to_i64_vec_near(near)
        .ok_or_else(|| Error::domain("a permutation is a list of integers", span))?;
    let mut seen = vec![false; n];
    let mut tail = Vec::with_capacity(v.len());
    for &i in &v {
        let k = usize::try_from(i).ok().filter(|&k| k < n && !seen[k]).ok_or_else(|| {
            Error::domain(format!("{i} does not belong to a permutation of {n} items"), span)
        })?;
        seen[k] = true;
        tail.push(k);
    }
    let mut out: Vec<usize> = (0..n).filter(|&k| !seen[k]).collect();
    out.append(&mut tail);
    Ok(out)
}

/// How many items a permutation list stands for on its own: one past the
/// largest index it names, and never fewer than the indices it has.
fn permutation_span(y: &Array, near: NearInt, span: Span) -> Result<usize> {
    let v = y
        .to_i64_vec_near(near)
        .ok_or_else(|| Error::domain("a permutation is a list of integers", span))?;
    let top = v.iter().copied().max().unwrap_or(-1).saturating_add(1).max(0) as u128;
    Ok(crate::limits::count(top, span)?.max(v.len()))
}

/// The direct permutation a boxed list of cycles stands for. Its length is
/// one past the largest element any cycle mentions; everything unmentioned
/// stays where it is.
fn cycles_to_direct(y: &Array, near: NearInt, span: Span) -> Result<Vec<usize>> {
    let boxes = y.as_boxes().ok_or_else(|| Error::internal("cycles from a simple array"))?;
    let mut cycles: Vec<Vec<usize>> = Vec::new();
    let mut top = 0usize;
    for b in boxes {
        let v = b
            .to_i64_vec_near(near)
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
/// is a direct permutation of y's items, abbreviated where it is shorter
/// than y — the items it never names come first, in ascending order. An
/// atom is such a list of one, so `2 C. i.5` and `3 4 2 C. i.5` are the
/// same permutation.
fn permute(x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    let ys = as_list(y);
    let n = ys.items();
    if x.dtype() != DType::Box {
        let perm = direct_permutation_of(&as_list(x), n, near, span)?;
        return Ok(select_items(&ys, &perm));
    }
    let mut perm = cycles_to_direct(x, near, span)?;
    if perm.len() > n {
        return Err(Error::new(
            ErrorKind::Length,
            format!("a permutation of {} items applied to {n}", perm.len()),
            Some(span),
        ));
    }
    // Cycles name only what moves: everything else stays put.
    perm.extend(perm.len()..n);
    Ok(select_items(&ys, &perm))
}

// ------------------------------------------------------- text and structure

/// `u: y` and `⎕UCS`: characters and their codepoints. `pass_chars` is J's
/// monad, which answers characters with themselves; APL's `⎕UCS` converts
/// in both directions.
fn unicode(y: &Array, pass_chars: bool, near: NearInt, span: Span) -> Result<Array> {
    if y.dtype() == DType::Char {
        if pass_chars {
            return Ok(y.clone());
        }
        return Ok(chars_to_codes(y));
    }
    codes_to_chars(y, near, span)
}

fn chars_to_codes(y: &Array) -> Array {
    let Data::Char(v) = &y.data else { return y.clone() };
    Array::new(y.shape.clone(), Data::I64(v.iter().map(|&c| c as i64).collect()))
}

fn codes_to_chars(y: &Array, near: NearInt, span: Span) -> Result<Array> {
    let v = y
        .to_i64_vec_near(near)
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
fn unicode_form(x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    let form = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("a conversion form is an integer", span))?
        .first()
        .copied()
        .unwrap_or(0);
    match form {
        3 if y.dtype() == DType::Char => Ok(chars_to_codes(y)),
        3 => Err(Error::domain("form 3 converts characters to codepoints", span)),
        10 => codes_to_chars(y, near, span),
        n => Err(Error::not_yet(format!("the byte-oriented unicode form ({n} u:)"), span)),
    }
}

/// `s: y`: the argument's text, interned.
///
/// A character list carries its own delimiter in its first position, so
/// the two names of a list that begins with a backtick are what stands
/// between the backticks, and `s: 'a b'` is the one name `" b"`; the empty
/// list has no delimiter and no names. A character table gives one name per
/// row, trailing blanks trimmed, and its leading axes are the result's
/// shape. A boxed argument gives one name per box, the characters taken
/// exactly as they stand — a box is where a name with a trailing blank
/// comes from.
fn to_symbols(y: &Array, span: Span) -> Result<Array> {
    if let Some(boxes) = y.as_boxes() {
        let mut ids = Vec::with_capacity(boxes.len());
        for b in boxes {
            if b.rank() > 1 {
                return Err(Error::new(
                    ErrorKind::Rank,
                    "a boxed symbol name is a character list",
                    Some(span),
                ));
            }
            let row_major = b.to_row_major();
            let Data::Char(v) = &row_major.data else {
                if b.count() == 0 {
                    ids.push(crate::symbol::EMPTY);
                    continue;
                }
                return Err(Error::domain("a symbol is made from characters", span));
            };
            ids.push(crate::symbol::intern(&v.as_slice().iter().collect::<String>()));
        }
        return Ok(Array::new(y.shape.clone(), Data::Symbol(ids.into())));
    }
    let row_major = y.to_row_major();
    let Data::Char(v) = &row_major.data else {
        return Err(Error::domain(
            format!("s: makes symbols from characters, not {} data", y.dtype().name()),
            span,
        ));
    };
    let chars = v.as_slice();
    if y.rank() >= 2 {
        let width = y.shape[y.rank() - 1];
        let mut ids = Vec::with_capacity(chars.len() / width.max(1));
        for row in chars.chunks(width) {
            let name: String = row.iter().collect();
            ids.push(crate::symbol::intern(name.trim_end_matches(' ')));
        }
        return Ok(Array::new(y.shape[..y.rank() - 1].to_vec(), Data::Symbol(ids.into())));
    }
    let Some((&delim, rest)) = chars.split_first() else {
        return Ok(Array::new(vec![0], Data::empty(DType::Symbol)));
    };
    let mut ids = Vec::new();
    let mut name = String::new();
    for &c in rest {
        if c == delim {
            ids.push(crate::symbol::intern(&name));
            name.clear();
        } else {
            name.push(c);
        }
    }
    ids.push(crate::symbol::intern(&name));
    Ok(Array::new(vec![ids.len()], Data::Symbol(ids.into())))
}

/// `x s: y`: the numbered symbol forms. 4 lays the names out as a character
/// table, blank-padded to the longest, and 5 boxes them one apiece. The
/// remaining numbers J defines report on its own symbol table — how many
/// slots it holds, which are in use, how it hashes them — and describe an
/// interpreter's internals rather than the language.
fn symbol_form(x: &Array, y: &Array, span: Span) -> Result<Array> {
    let form = x
        .to_i64_vec()
        .ok_or_else(|| Error::domain("a symbol form is an integer", span))?
        .first()
        .copied()
        .unwrap_or(0);
    if !matches!(form, 4 | 5) {
        return Err(Error::not_yet(format!("the symbol-table form ({form} s:)"), span));
    }
    let row_major = y.to_row_major();
    let Data::Symbol(ids) = &row_major.data else {
        return Err(Error::domain(
            format!("{form} s: reads symbols, not {} data", y.dtype().name()),
            span,
        ));
    };
    let names = crate::symbol::names(ids.as_slice());
    if form == 5 {
        let boxes: Vec<Array> =
            names.iter().map(|n| Array::from_chars(n.chars().collect())).collect();
        return Ok(Array::new(y.shape.clone(), Data::Box(boxes.into())));
    }
    let width = names.iter().map(|n| n.chars().count()).max().unwrap_or(0);
    let mut out: Vec<char> = Vec::with_capacity(names.len() * width);
    for n in &names {
        out.extend(n.chars());
        out.resize(out.len() + width - n.chars().count(), ' ');
    }
    let mut shape = y.shape.clone();
    shape.push(width);
    Ok(Array::new(shape, Data::Char(out.into())))
}

/// `x $. y`: the numbered sparse forms.
///
/// `0` moves between the two storage kinds in whichever direction the
/// argument is not already in, and `1` builds a new sparse array from a
/// shape. The rest ask about a sparse argument: `_1` its shape, sparse axes
/// and sparse element boxed, `2` the sparse axes, `3` the sparse element,
/// `4` the stored index rows, `5` the stored cells, `7` how many entries
/// are stored, and `8` the same array with the entries that hold the sparse
/// element dropped. `2` also answers a dense argument, which has all of its
/// axes conceptually sparse; the others refuse one.
fn sparse_form(x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    if x.rank() != 0 {
        return Err(Error::new(ErrorKind::Rank, "a sparse form is one atom", Some(span)));
    }
    let form = x
        .to_i64_vec_near(near)
        .and_then(|v| v.first().copied())
        .ok_or_else(|| Error::domain("a sparse form is an integer", span))?;
    match form {
        0 if y.is_sparse() => return Ok(y.densified()),
        0 => return crate::sparse::sparsify(y, span),
        1 => return crate::sparse::create(y, span),
        2 => {
            let axes: Vec<i64> = match y.sparse_parts() {
                Some(s) => s.axes.iter().map(|&k| k as i64).collect(),
                None => (0..y.rank() as i64).collect(),
            };
            return Ok(Array::from_i64(axes));
        }
        _ => {}
    }
    let Some(s) = y.sparse_parts() else {
        return Err(Error::domain(
            format!("{form} $. reads a sparse array, and this one is dense"),
            span,
        ));
    };
    match form {
        -1 => Ok(crate::sparse::attributes(y, s)),
        3 => Ok(crate::sparse::fill_of(s)),
        4 => Ok(crate::sparse::indices_of(s)),
        5 => Ok(crate::sparse::values_of(y, s)),
        7 => Ok(Array::scalar_i64(s.entries as i64)),
        8 => Ok(crate::sparse::compress(y, s)),
        _ => Err(Error::domain(format!("{form} is not a sparse form"), span)),
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
fn pick(x: &Array, y: &Array, origin: i64, near: NearInt, span: Span) -> Result<Array> {
    let xs = as_list(x);
    let mut cur = y.clone();
    for i in 0..xs.items() {
        let step = open_cell(&item_or_self(&xs, i));
        let idx = step
            .to_i64_vec_near(near)
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
fn prime_meta(x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    let form = one_int(x, "a prime query", near, span)?;
    let n = one_int(y, "a prime query", near, span)?;
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
fn prime_exponents(x: &Array, y: &Array, near: NearInt, span: Span) -> Result<Array> {
    let n = one_int(y, "prime exponents", near, span)?;
    let count = x.to_f64_vec().and_then(|v| v.first().copied()).unwrap_or(0.0);
    let (ps, es) = factor_table(n, span)?;
    if count == f64::NEG_INFINITY {
        let k = ps.len();
        let mut all = ps;
        all.extend(es);
        return Ok(Array::new(vec![2, k], Data::I64(all.into())));
    }
    let want = one_int(x, "prime exponents", near, span)?;
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
fn one_int(a: &Array, what: &str, near: NearInt, span: Span) -> Result<i64> {
    a.to_i64_vec_near(near)
        .and_then(|v| v.first().copied())
        .ok_or_else(|| Error::domain(format!("{what} needs an integer"), span))
}

/// `x \\ y`: expand. Every 1 in x takes the next item of y; every 0 leaves
/// a fill in its place — the type's own fill, or, for a nested argument in
/// APL, the prototype of its first item.
fn expand(x: &Array, y: &Array, apl: bool, near: NearInt, span: Span) -> Result<Array> {
    let mask = x
        .to_i64_vec_near(near)
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
    let fill = if apl { prototype_of(&ys) } else { None };
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
                push_gap(&mut data, &fill);
            }
        }
    }
    let mut shape = ys.shape.clone();
    if shape.is_empty() {
        shape.push(mask.len());
    } else {
        shape[0] = mask.len();
    }
    Ok(keep_proto(Array::new(shape, data), &ys, apl))
}

/// `". y` and `⍎ y`: the characters of y as a program of this language,
/// compiled now and run here.
///
/// The nested program shares the caller's names and its output sink, which
/// is what makes `". 'a =. 3'` assign in the scope the sentence stands in.
/// It reaches nothing the caller could not reach: the sandbox contract is
/// about what a primitive may touch, and evaluation touches nothing new.
fn execute(y: &Array, apl: bool, ctx: &mut Ctx<'_>, span: Span) -> Result<Array> {
    let Data::Char(v) = &y.data else {
        return Err(Error::domain("execute reads a character list", span));
    };
    let src: String = v.iter().collect();
    execute_source(&src, apl, ctx, span)
}

/// [`execute`] over source that is already text: APL's `⎕` reads a line and
/// runs it, which is execute over a string nobody boxed into an array.
pub(crate) fn execute_source(
    src: &str,
    apl: bool,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    let lang = if apl { crate::Lang::Apl } else { crate::Lang::J };
    // The nested program runs under the dialect the caller was compiled
    // with — every setting of it, not the index origin alone.
    let dialect = ctx.cfg.rules.dialect();
    let nested = crate::compile(lang, src, &dialect).map_err(|e| nested_error(e, src, span))?;
    if !nested.params.is_empty() {
        return Err(Error::domain(
            "an executed string cannot take host data: `{name}` has nothing to bind to",
            span,
        ));
    }
    let mut rec = None;
    let (value, _) = crate::ir::run_block(&nested.stmts, None, ctx, &mut rec)
        .map_err(|e| nested_error(e, src, span))?;
    value.ok_or_else(|| Error::domain("the executed string yielded no value", span))
}

/// The stream number a J file foreign was given, checked against the one
/// the sandbox opens for that direction.
///
/// J numbers its streams and its open files alike, so a number that is not
/// the standard one is a file handle; a boxed argument is a file NAME. Both
/// are the filesystem, which the sandbox closes.
fn stream_number(y: &Array, open: i64, what: &str, span: Span) -> Result<()> {
    let closed = || {
        Err(Error::sandbox(
            format!("{what} the standard stream {open} only; a file is outside the program"),
            span,
        ))
    };
    if matches!(y.data, Data::Box(_)) {
        return closed();
    }
    match y.to_i64_vec().as_deref() {
        Some([n]) if *n == open => Ok(()),
        Some([_]) => closed(),
        _ => Err(Error::domain(format!("{what} one stream number"), span)),
    }
}

/// `3!:0 y`: the code J gives y's element type. The numbers are J's own,
/// and libjay's element types line up with them one for one.
/// J's code for the argument's element type. A sparse array has a code of
/// its own for every element type that can be stored sparsely, one factor
/// of 1024 above the dense one.
fn type_code(y: &Array) -> i64 {
    if y.is_sparse() {
        return 1024 * dense_type_code(y);
    }
    dense_type_code(y)
}

fn dense_type_code(y: &Array) -> i64 {
    match y.dtype() {
        DType::Bool => 1,
        DType::Char => 2,
        DType::I64 => 4,
        DType::F64 => 8,
        DType::Complex => 16,
        DType::Box => 32,
        DType::Ext => 64,
        DType::Rat => 128,
        DType::Symbol => 65536,
    }
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
                cfg: EvalCfg {
                    agreement: $agreement,
                    fmt: FmtOpts::J,
                    tol: Tol::J,
                    // The agreement names the language here, so the rules
                    // a verb reads are that language's shipped dialect.
                    rules: crate::frontend::Dialect::default()
                        .rules(if $agreement == Agreement::ExactOrScalar {
                            crate::Lang::Apl
                        } else {
                            crate::Lang::J
                        })
                        .expect("the shipped dialect is implemented"),
                },
                out: &mut sink,
                inp: None,
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

    /// The elements in reading order, whatever layout the result kept.
    fn ints(a: &Array) -> Vec<i64> {
        a.to_row_major().as_i64_slice().expect("integer result").to_vec()
    }

    fn floats(a: &Array) -> Vec<f64> {
        a.to_row_major().as_f64_slice().expect("float result").to_vec()
    }

    fn bools(a: &Array) -> Vec<u8> {
        match &a.to_row_major().data {
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
        let bits = Array::new(vec![3], Data::Bool(vec![1, 0, 1].into()));
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
        let bits = Array::new(vec![2], Data::Bool(vec![0, 1].into()));
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
        // A vector of lengths asks for an array of index vectors, one per
        // cell of the result.
        let r = iota_apl(1).monad(&Array::from_i64(vec![2, 3]), &mut c, sp()).unwrap();
        assert_eq!(r.shape, vec![2, 3]);
        assert_eq!(ints(&r.as_boxes().expect("boxed")[4]), vec![2, 2]);
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
        // More counts than the argument has axes: a length error, as both
        // references answer. Only a scalar right argument stretches.
        let e = head_v()
            .dyad(&Array::from_i64(vec![1, 1]), &Array::from_i64(vec![1, 2]), &mut c, sp())
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Length);
        let r = head_v()
            .dyad(&Array::from_i64(vec![1, 2]), &Array::scalar_i64(5), &mut c, sp())
            .unwrap();
        assert_eq!(r.shape, vec![1, 2]);
        assert_eq!(ints(&r), vec![5, 0]);
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
            if let Some(first) = s.split_whitespace().next() && let Ok(n) = first.parse::<i64>() {
                seen.push(n);
            }
        };
        let mut env = Env::new(Vec::new());
        let mut c = Ctx {
            cfg: EvalCfg {
                agreement: Agreement::LeadingPrefix,
                fmt: FmtOpts::J,
                tol: Tol::J,
                rules: Rules::default(),
            },
            out: &mut sink,
            inp: None,
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
