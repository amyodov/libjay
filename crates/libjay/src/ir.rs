//! The language-agnostic program representation and its evaluator.

use std::collections::HashMap;
use std::sync::Arc;

use crate::array::{Array, Data};
use crate::error::{Error, ErrorKind, Result, Span};
use crate::fmt::{format_array, FmtOpts};
use crate::frontend::{ControlStrictness, Lang, Rules};
use crate::fuse::FusedKernel;
use crate::verb::{arrays_match, Agreement, Ctx, Env, EvalCfg, Tol, Verb};

/// Where an assignment puts its name. The two differ only inside an
/// explicit definition, which is the only thing that has a local frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// J `=.`, APL's default inside a definition: the running definition's
    /// own frame, discarded when it returns.
    Local,
    /// J `=:`: the program's names, visible to everything that runs later.
    Global,
    /// APL `⍺←`: the local frame, but only where the name has no value yet.
    /// A left argument that was supplied keeps the value it arrived with.
    LocalDefault,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Const(Array, Span),
    /// A bound parameter, by position in `Program::params`.
    Param(usize, Span),
    /// A name assigned earlier in the same program.
    Name(String, Span),
    /// Yields the assigned value in expression position; a whole sentence
    /// that is an assignment displays nothing at the top level.
    Assign { name: String, value: Box<Expr>, scope: Scope, span: Span },
    /// APL `A[i;j]←v`: the named value with the part the brackets select
    /// replaced. The name is read, a copy is written, and the copy takes
    /// the name's place. An elided slot selects its whole axis.
    AmendIndex {
        name: String,
        slots: Vec<Option<Expr>>,
        value: Box<Expr>,
        origin: i64,
        scope: Scope,
        span: Span,
    },
    /// A control-flow sentence (J's control words, APL's `:If` family).
    /// Its value is the value of the last sentence the branch it chose
    /// executed. J holds one to an explicit definition's body, as the
    /// reference does; APL's stands outside a definition too.
    Control(Box<Control>, Span),
    Monad { verb: Verb, y: Box<Expr>, span: Span },
    Dyad { verb: Verb, x: Box<Expr>, y: Box<Expr>, span: Span },
    /// APL `⎕← expr` and `⍞← expr`: print, pass the value through. `bare`
    /// is the `⍞←` form, which writes the characters and nothing else;
    /// `⎕←` ends the line.
    PrintPass { value: Box<Expr>, bare: bool, span: Span },
    /// APL `⍞` and `⎕` standing where a value belongs: one line of input.
    /// `eval` is the `⎕` form, which runs the line as APL rather than
    /// taking its characters.
    Input { eval: bool, span: Span },
    /// A chain of elementwise verbs evaluated in one blockwise pass (see
    /// [`crate::fuse`]). `inputs` are the subtrees the chain reads; `orig`
    /// is the chain itself, which runs whenever the kernel declines.
    Fused { kernel: FusedKernel, inputs: Vec<Expr>, orig: Box<Expr>, span: Span },
    /// A marker the fusion pass leaves when it has rewritten the program
    /// across sentence boundaries: it does nothing and yields nothing, and
    /// carries the sentences the program was compiled from so that
    /// [`crate::fuse::unfused`] can rebuild them.
    Elided { orig: Vec<Expr>, span: Span },
    /// A sentence that named a verb (J `mean =. +/ % #`). The frontend has
    /// already substituted the verb into the later sentences that use the
    /// name, so nothing runs here; the node is kept so that the sentence
    /// still yields no value, and so that [`Program::explain`] can show it.
    VerbDef { name: String, verb: Verb, span: Span },
    /// A sentence that named an adverb or a conjunction (J `m =. /`). A
    /// modifier is applied when the sentence holding it is parsed, so this
    /// node carries only what the name stands for; like
    /// [`Expr::VerbDef`] it runs nothing and yields nothing.
    ModDef { name: String, spelling: String, conjunction: bool, span: Span },
}

/// A control-flow sentence. Every body is a block: a list of sentences whose
/// value is the last one's.
#[derive(Clone, Debug)]
pub enum Control {
    /// `if. T do. B elseif. T do. B else. B end.`, and APL's `:If` family.
    /// The arms are tested in order; `otherwise` is the `else.` body.
    If { arms: Vec<Branch>, otherwise: Option<Vec<Expr>> },
    /// `while.` and `whilst.`, APL's `:While` and `:Repeat`. `body_first`
    /// runs the body once before the first test; `until` inverts the test.
    While { test: Vec<Expr>, body: Vec<Expr>, body_first: bool, until: bool },
    /// `for. y do. B end.` / `for_i.` / `:For i :In y`. One name binds each
    /// item and `<name>_index` its position; several — APL's `:For a b :In
    /// y` — take the item apart and bind one of its own items each. No name
    /// binds nothing, which is `for.` without a suffix.
    For { names: Vec<String>, source: Box<Expr>, body: Vec<Expr> },
    /// `select. T case. S do. B end.` and `:Select`. A case with no test is
    /// the default (`case. do.`, `:Else`); `fall_through` is `fcase.`.
    Select { subject: Box<Expr>, cases: Vec<Branch> },
    /// `try. B catch. B end.`. The catch block runs on a language error;
    /// a gap in libjay itself is never caught.
    Try { body: Vec<Expr>, catch: Vec<Expr> },
    /// `return.` / `:Return`: leave the definition with the value in hand.
    Return,
    /// `break.` / `:Leave`: leave the innermost loop.
    Break,
    /// APL `→ e`: continue at the line e names. An empty value falls
    /// through to the next line; anything that is not a line of this
    /// definition — `→0` above all — leaves it.
    Branch(Box<Expr>),
    /// `continue.` / `:Continue`: start the innermost loop's next iteration.
    Continue,
    /// A dfn's guard, `cond:expr`: the body is the dfn's answer when the
    /// condition holds, and the definition returns there.
    ///
    /// It is not an `:If` with one arm, because it reads its condition
    /// more strictly: Dyalog wants exactly one 0 or 1, and refuses `2`,
    /// `1 1`, `⍬` and a character alike.
    Guard { test: Vec<Expr>, body: Vec<Expr> },
}

/// One arm of an `if.` or `select.`: a test (absent for the default arm) and
/// the body to run when it holds.
#[derive(Clone, Debug)]
pub struct Branch {
    pub test: Option<Vec<Expr>>,
    pub body: Vec<Expr>,
    /// `fcase.`: run the next arm's body too, without testing it.
    pub fall_through: bool,
    /// APL `:CaseList`: the test yields a LIST of candidates and the arm is
    /// taken where the subject matches any one of its items, rather than
    /// the list as a whole.
    pub list: bool,
}

/// The right-argument name a NILADIC APL definition carries. No sentence
/// can write it, so the body cannot read the argument it never gets, and a
/// definition wearing it is called by naming it rather than applying it.
pub const NILADIC: &str = "(no argument)";

/// An explicit definition: J's `3 : '…'`, `4 : '…'` and `{{ … }}`, APL's
/// `{…}` and `∇`-defined functions.
#[derive(Debug)]
pub struct ExplicitDef {
    /// How the definition names itself in diagnostics and `explain`.
    pub name: String,
    /// The names the arguments arrive under: `(left, right)`. A definition
    /// with no left name has no dyadic valence.
    pub left: Option<String>,
    pub right: String,
    /// True where a left name is part of the definition's valence rather
    /// than a name the body may or may not read: J's `4 : '…'` and a `{{ }}`
    /// that mentions `x` are dyads and nothing else, while an APL dfn that
    /// names `⍺` still runs monadically and finds `⍺` undefined.
    pub dyad_only: bool,
    /// True where a left argument the definition has NO name for is simply
    /// dropped rather than refused: a dfn is ambivalent whatever its body
    /// mentions, so `3 {⍵×2} 5` is 10, while a `∇`-definition or a J
    /// `3 : '…'` refuses the argument it cannot bind.
    pub spare_left: bool,
    /// The name the result is read from when the body does not yield one
    /// (an APL `∇`-definition's `Z←`); None means the body's own value.
    pub result: Option<String>,
    /// Names the header declares local (APL's `;name` list).
    pub locals: Vec<String>,
    pub body: Vec<Expr>,
    /// The value a body that ran nothing yields; None makes that an error.
    pub empty: Option<Array>,
    /// APL's branch labels: each label with the body statement it names.
    /// A label's value is its line number, which is one more than its
    /// position here, and `→` takes one of those numbers.
    pub labels: Vec<(String, usize)>,
    /// The dfns this one is written INSIDE, outermost first, by
    /// [`ExplicitDef::id`]. A dfn's body reads the names its enclosing
    /// dfns made local — `{a←10 ⋄ {a+⍵} ⍵} 5` is 15 — and this is what
    /// tells a running body which frames on the stack are its own
    /// lexical parents rather than an unrelated caller's. Empty for
    /// everything that is not a nested dfn.
    pub enclosing: Vec<u64>,
    /// This definition's identity among the dfns of one compilation, so
    /// that a nested one can name it in `enclosing`. Zero where nothing
    /// is nested inside.
    pub id: u64,
    /// True when running the body can have no effect beyond its result.
    pub pure: bool,
}

impl Expr {
    /// How deeply this tree nests, counted WITHOUT recursing — the point
    /// of the measurement is that walking such a tree is what runs out of
    /// stack, so the measurement itself must not.
    pub(crate) fn depth(&self) -> usize {
        let mut deepest = 0usize;
        let mut stack: Vec<(&Expr, usize)> = vec![(self, 1)];
        while let Some((e, d)) = stack.pop() {
            deepest = deepest.max(d);
            let kids: Vec<&Expr> = match e {
                Expr::Const(..)
                | Expr::Param(..)
                | Expr::Name(..)
                | Expr::Control(..)
                | Expr::VerbDef { .. }
                | Expr::Input { .. }
                | Expr::ModDef { .. } => Vec::new(),
                Expr::Assign { value, .. } | Expr::PrintPass { value, .. } => vec![value],
                Expr::AmendIndex { slots, value, .. } => {
                    slots.iter().flatten().chain(std::iter::once(&**value)).collect()
                }
                Expr::Monad { y, .. } => vec![y],
                Expr::Dyad { x, y, .. } => vec![x, y],
                Expr::Fused { inputs, orig, .. } => {
                    inputs.iter().chain(std::iter::once(&**orig)).collect()
                }
                Expr::Elided { orig, .. } => orig.iter().collect(),
            };
            stack.extend(kids.into_iter().map(|c| (c, d + 1)));
        }
        deepest
    }

    pub fn span(&self) -> Span {
        match self {
            Expr::Const(_, s) | Expr::Param(_, s) | Expr::Name(_, s) => *s,
            Expr::Control(_, s) => *s,
            Expr::AmendIndex { span, .. } | Expr::Input { span, .. } => *span,
            Expr::Assign { span, .. }
            | Expr::Monad { span, .. }
            | Expr::Dyad { span, .. }
            | Expr::PrintPass { span, .. }
            | Expr::Fused { span, .. }
            | Expr::Elided { span, .. }
            | Expr::VerbDef { span, .. }
            | Expr::ModDef { span, .. } => *span,
        }
    }

    /// Widen (or move) the source this node points at. A parenthesised
    /// expression uses it to take in its own brackets, so that a caret
    /// under it underlines something balanced.
    pub fn set_span(&mut self, to: Span) {
        match self {
            Expr::Const(_, s) | Expr::Param(_, s) | Expr::Name(_, s) => *s = to,
            Expr::Control(_, s) => *s = to,
            Expr::AmendIndex { span, .. } | Expr::Input { span, .. } => *span = to,
            Expr::Assign { span, .. }
            | Expr::Monad { span, .. }
            | Expr::Dyad { span, .. }
            | Expr::PrintPass { span, .. }
            | Expr::Fused { span, .. }
            | Expr::Elided { span, .. }
            | Expr::VerbDef { span, .. }
            | Expr::ModDef { span, .. } => *span = to,
        }
    }

    /// Sentences whose top level is an assignment, explicit output, or the
    /// pass's record of what the program was yield no value to the
    /// sequence.
    fn is_silent(&self) -> bool {
        matches!(
            self,
            Expr::Assign { .. }
                | Expr::AmendIndex { .. }
                | Expr::PrintPass { .. }
                | Expr::Elided { .. }
                | Expr::VerbDef { .. }
                | Expr::ModDef { .. }
        )
    }
}

#[derive(Clone, Debug)]
pub struct ParamSpec {
    pub name: String,
}

/// A compiled program: immutable, reusable, holds no data bindings.
#[derive(Clone, Debug)]
pub struct Program {
    pub stmts: Vec<Expr>,
    pub params: Vec<ParamSpec>,
    /// The source as the user would recognise it (interpolations shown as
    /// `{name}`); all spans point into this string.
    pub display_src: String,
    pub agreement: Agreement,
    pub fmt: FmtOpts,
    /// The dialect this program was compiled under, resolved.
    pub rules: Rules,
}

/// What an instrumented run saw at one node.
#[derive(Clone, Debug)]
pub(crate) struct Note {
    pub shape: Vec<usize>,
    pub dtype: crate::dtype::DType,
    /// How the value's buffer was laid out — worth saying only when it was
    /// not the row-major order everything assumes.
    pub layout: crate::array::Layout,
    /// For a fused node: whether the kernel itself produced the value, and
    /// the reason it declined when it did not.
    pub kernel_ran: Option<bool>,
    pub decline: Option<crate::fuse::Decline>,
    /// For a fused node in a run that was given a device: where the
    /// arithmetic happened.
    pub placement: crate::device::Placement,
}

/// Notes from one run, keyed by the address of the node in the tree that
/// ran. Explaining borrows the same `Program`, so the addresses still name
/// the same nodes; nothing outside this crate ever sees them.
pub(crate) type Trace = HashMap<usize, Note>;

pub(crate) fn key(e: &Expr) -> usize {
    std::ptr::from_ref(e) as usize
}

impl Program {
    /// Execute with one value per parameter, in `params` order.
    /// Returns None when the last sentence yields no value.
    ///
    /// The run has no input source: an expression that reads one — APL's
    /// `⍞` and `⎕`, J's `1!:1` — says so rather than reading anything.
    /// [`Program::run_io`] is the same run with a source attached.
    pub fn run(&self, args: &[Array], out: &mut dyn FnMut(&str)) -> Result<Option<Array>> {
        self.exec(args, out, None, &mut None, None)
    }

    /// Execute with both halves of the sandbox's stdio wired: `out` takes
    /// the program's output, `inp` answers its reads with one line at a
    /// time (no terminator) and None once the input has ended.
    pub fn run_io(
        &self,
        args: &[Array],
        out: &mut dyn FnMut(&str),
        inp: &mut dyn FnMut() -> Option<String>,
    ) -> Result<Option<Array>> {
        self.exec(args, out, Some(inp), &mut None, None)
    }

    /// [`Program::run_io`] with the fused kernels placed on `device`.
    pub fn run_on_io(
        &self,
        device: &crate::device::Device,
        args: &[Array],
        out: &mut dyn FnMut(&str),
        inp: &mut dyn FnMut() -> Option<String>,
    ) -> Result<Option<Array>> {
        self.exec(args, out, Some(inp), &mut None, Some(device))
    }

    /// Execute with the fused kernels placed on `device`.
    ///
    /// Placement is not binding: the program, its data and its diagnostics
    /// are the same whatever device is named here. What a device will not
    /// take runs on the CPU, and `explain` says which and why.
    pub fn run_on(
        &self,
        device: &crate::device::Device,
        args: &[Array],
        out: &mut dyn FnMut(&str),
    ) -> Result<Option<Array>> {
        self.exec(args, out, None, &mut None, Some(device))
    }

    /// Execute and record every node's result shape and dtype. The trace is
    /// returned even when a sentence fails, so that a partial explanation
    /// still shows what did run.
    pub(crate) fn trace(
        &self,
        args: &[Array],
        out: &mut dyn FnMut(&str),
        device: Option<&crate::device::Device>,
    ) -> (Result<Option<Array>>, Trace) {
        let mut rec = Some(Trace::new());
        let r = self.exec(args, out, None, &mut rec, device);
        (r, rec.expect("the recorder stays in place"))
    }

    fn exec(
        &self,
        args: &[Array],
        out: &mut dyn FnMut(&str),
        inp: crate::verb::InputFn<'_>,
        rec: &mut Option<Trace>,
        device: Option<&crate::device::Device>,
    ) -> Result<Option<Array>> {
        if args.len() != self.params.len() {
            let names: Vec<&str> = self.params.iter().map(|p| p.name.as_str()).collect();
            let wanted = if names.is_empty() {
                "no arguments".to_string()
            } else {
                format!("one value for each of {}", names.join(", "))
            };
            return Err(Error::new(
                ErrorKind::Value,
                format!("this program takes {wanted}, and was given {}", args.len()),
                None,
            ));
        }
        let cfg = EvalCfg {
            agreement: self.agreement,
            fmt: self.fmt,
            tol: self.rules.tol(),
            rules: self.rules,
        };
        let mut env = Env::new(args.to_vec());
        let mut inp = inp;
        let inp = crate::verb::reborrow_input(&mut inp);
        let mut ctx = Ctx { cfg, out, inp, env: &mut env, device };
        let mut last = None;
        for stmt in &self.stmts {
            // A loop contains its own `:Leave`, and there is no definition
            // out here for a `:Return` or a `→` to leave.
            let (v, flow) = eval_stmt(stmt, &mut ctx, rec)?;
            if flow != Flow::Normal {
                return Err(Error::new(
                    ErrorKind::Domain,
                    "this control word leaves a definition, and there is none here",
                    None,
                ));
            }
            last = if stmt.is_silent() { None } else { v };
        }
        Ok(last)
    }

    pub fn render_error(&self, e: &Error) -> String {
        e.render(&self.display_src)
    }

    /// What this expression became, as text: one section per sentence,
    /// giving the structure the frontend and the fusion pass produced.
    ///
    /// With one value per parameter (or none, for a program that takes
    /// none) the program is also run, and every node is annotated with the
    /// shape and dtype it produced — a fused node with whether its kernel
    /// ran, and why not when it did not. The run is the ordinary one, so it
    /// has the ordinary effects; output it makes is discarded here, and an
    /// error stops the annotations and is reported at the end.
    pub fn explain(&self, args: Option<&[Array]>) -> String {
        crate::explain::explain(self, args, None)
    }

    /// [`Program::explain`], with the run placed on `device`: every fused
    /// node then also says where its arithmetic happened, and why it was
    /// not the device when it was not.
    pub fn explain_on(
        &self,
        device: &crate::device::Device,
        args: Option<&[Array]>,
    ) -> String {
        crate::explain::explain(self, args, Some(device))
    }
}

/// Why a block stopped. `Normal` is falling off the end of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flow {
    Normal,
    Return,
    Break,
    Continue,
    /// APL `→`: continue at this statement of the definition's body.
    Goto(usize),
}

/// Run a block of sentences: the value is the last sentence's, and an
/// assignment yields the value it assigned (the top level is the one place
/// that discards it, and `Program::exec` applies that rule itself).
pub(crate) fn run_block(
    stmts: &[Expr],
    last: Option<Array>,
    ctx: &mut Ctx<'_>,
    rec: &mut Option<Trace>,
) -> Result<(Option<Array>, Flow)> {
    let mut last = last;
    for stmt in stmts {
        let (v, flow) = eval_stmt(stmt, ctx, rec)?;
        // `return.` and its relatives produce nothing of their own: the
        // value in hand is what the definition hands back.
        if let Some(v) = v {
            last = Some(v);
        }
        if flow != Flow::Normal {
            return Ok((last, flow));
        }
    }
    Ok((last, Flow::Normal))
}

/// One sentence, control words included. The value of a control sentence is
/// the value of the last sentence the branch it chose ran.
fn eval_stmt(
    e: &Expr,
    ctx: &mut Ctx<'_>,
    rec: &mut Option<Trace>,
) -> Result<(Option<Array>, Flow)> {
    let Expr::Control(c, span) = e else {
        return Ok((Some(eval(e, ctx, rec)?), Flow::Normal));
    };
    let (v, flow) = eval_control(c, *span, ctx, rec)?;
    // A branch that ran and produced nothing yields whatever the language
    // gives an untaken branch: J's empty `i. 0 0`, and nothing at all in
    // APL, where a function with no result is an error. A branch that left
    // early yields nothing either way, so the value in hand survives.
    let v = match (v, flow) {
        (Some(v), _) => Some(v),
        (None, Flow::Normal) => ctx.env.current_def().and_then(|d| d.empty.clone()),
        (None, _) => None,
    };
    if let (Some(t), Some(v)) = (rec.as_mut(), v.as_ref()) {
        t.insert(
            key(e),
            Note {
                shape: v.shape.clone(),
                dtype: v.dtype(),
                layout: v.layout(),
                kernel_ran: None,
                decline: None,
                placement: crate::device::Placement::Default,
            },
        );
    }
    Ok((v, flow))
}

/// The value of a branch that executed nothing: J's `i. 0 0`.
pub(crate) fn empty_result() -> Array {
    Array::new(vec![0, 0], Data::I64(Vec::new().into()))
}

/// J's truth: an empty condition is true, and otherwise the first atom
/// decides. Characters count by their code point, as the reference does.
fn is_true(a: &Array, span: Span) -> Result<bool> {
    if a.is_sparse() {
        return is_true(&a.densified(), span);
    }
    if a.count() == 0 {
        return Ok(true);
    }
    match &a.data {
        Data::I64(v) => Ok(v.as_slice()[0] != 0),
        Data::F64(v) => Ok(v.as_slice()[0] != 0.0),
        Data::Bool(v) => Ok(v.as_slice()[0] != 0),
        Data::Char(v) => Ok(v.as_slice()[0] as u32 != 0),
        Data::Complex(v) => Ok(v.as_slice()[0] != crate::complex::ZERO),
        Data::Ext(v) => Ok(v.as_slice()[0] != crate::exact::Ext::default()),
        Data::Rat(v) => Ok(!v.as_slice()[0].is_zero()),
        Data::Box(_) => Err(Error::domain("a condition must be numeric, not boxed", span)),
        Data::Symbol(_) => {
            Err(Error::domain("a condition must be numeric, not a symbol", span))
        }
    }
}

/// Bind one iteration of a `for.` loop. One name takes the item whole and
/// `<name>_index` its position; several take the item apart, one of its own
/// items each, and the item has to have exactly that many.
fn bind_for_names(
    names: &[String],
    item: &Array,
    i: usize,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<()> {
    if names.len() == 1 {
        ctx.env.assign(names[0].clone(), item.clone(), Scope::Local);
        ctx.env.assign(
            format!("{}_index", names[0]),
            Array::scalar_i64(i as i64),
            Scope::Local,
        );
        return Ok(());
    }
    let have = if item.rank() == 0 { 1 } else { item.shape[0] };
    if have != names.len() {
        return Err(Error::new(
            ErrorKind::Length,
            format!("{} names bind an item of {have}", names.len()),
            Some(span),
        ));
    }
    for (k, name) in names.iter().enumerate() {
        let part = if item.rank() == 0 { item.clone() } else { item.item(k) };
        ctx.env.assign(name.clone(), crate::verb::open_cell(&part), Scope::Local);
    }
    Ok(())
}

/// Whether a control structure's condition holds.
///
/// The lenient reading is the one both languages ship: an empty condition
/// is true and otherwise the first atom decides. Dyalog reads a condition
/// strictly instead — one element and no more — so `:If 1 1` is an error
/// there where it takes the first here.
fn condition_holds(a: &Array, ctx: &Ctx<'_>, span: Span) -> Result<bool> {
    if ctx.cfg.rules.control_strictness == ControlStrictness::Strict && a.count() != 1 {
        return Err(Error::domain("a condition must be a single value", span));
    }
    is_true(a, span)
}

/// Whether a `:CaseList` arm's list holds the subject. The comparison is
/// the one `:Case` makes, item by item.
fn any_item_matches(subject: &Array, list: &Array, tol: Tol) -> bool {
    if list.rank() == 0 {
        return arrays_match(subject, &crate::verb::open_cell(list), tol);
    }
    (0..list.shape[0])
        .any(|i| arrays_match(subject, &crate::verb::open_cell(&list.item(i)), tol))
}

/// Whether a dfn's guard holds. Its condition is read strictly: exactly
/// one element, and that element 0 or 1. `{2:1 ⋄ 0}`, `{1 1:1 ⋄ 0}`,
/// `{⍬:1 ⋄ 0}` and `{'x':1 ⋄ 0}` are all refused, where a control
/// structure's `:If` takes the first element of whatever it is given.
fn guard_holds(a: &Array, span: Span) -> Result<bool> {
    if a.is_sparse() {
        return guard_holds(&a.densified(), span);
    }
    let refuse = || Error::domain("a guard's condition must be a single 0 or 1", span);
    if a.count() != 1 {
        return Err(refuse());
    }
    match a.to_i64_vec().as_deref() {
        Some([0]) => Ok(false),
        Some([1]) => Ok(true),
        _ => Err(refuse()),
    }
}

fn eval_control(
    c: &Control,
    span: Span,
    ctx: &mut Ctx<'_>,
    rec: &mut Option<Trace>,
) -> Result<(Option<Array>, Flow)> {
    match c {
        Control::Return => Ok((None, Flow::Return)),
        // `→ e`: an empty target falls through, a line number of this
        // definition jumps to it, and anything else leaves.
        Control::Branch(target) => {
            let to = eval(target, ctx, rec)?;
            if to.count() == 0 {
                return Ok((None, Flow::Normal));
            }
            let line = to
                .to_i64_vec()
                .and_then(|v| v.first().copied())
                .ok_or_else(|| Error::domain("a branch target is a line number", span))?;
            let lines = ctx.env.current_def().map_or(0, |d| d.body.len() as i64);
            if line >= 1 && line <= lines {
                return Ok((None, Flow::Goto(line as usize - 1)));
            }
            Ok((None, Flow::Return))
        }
        Control::Break => Ok((None, Flow::Break)),
        Control::Continue => Ok((None, Flow::Continue)),
        Control::Guard { test, body } => {
            let (t, flow) = run_block(test, None, ctx, rec)?;
            if flow != Flow::Normal {
                return Ok((t, flow));
            }
            let Some(v) = &t else {
                return Err(Error::domain("a guard's condition produced no value", span));
            };
            if guard_holds(v, span)? {
                return run_block(body, None, ctx, rec);
            }
            Ok((None, Flow::Normal))
        }
        Control::If { arms, otherwise } => {
            for arm in arms {
                let test = arm.test.as_deref().unwrap_or(&[]);
                let (t, flow) = run_block(test, None, ctx, rec)?;
                if flow != Flow::Normal {
                    return Ok((t, flow));
                }
                let taken = match &t {
                    Some(v) => condition_holds(v, ctx, span)?,
                    None => true,
                };
                if taken {
                    return run_block(&arm.body, None, ctx, rec);
                }
            }
            match otherwise {
                Some(body) => run_block(body, None, ctx, rec),
                None => Ok((None, Flow::Normal)),
            }
        }
        Control::While { test, body, body_first, until } => {
            let mut last = None;
            let mut first = *body_first;
            loop {
                if !first {
                    let (t, flow) = run_block(test, None, ctx, rec)?;
                    if flow != Flow::Normal {
                        return Ok((t, flow));
                    }
                    let mut go = match &t {
                        Some(v) => condition_holds(v, ctx, span)?,
                        None => false,
                    };
                    if *until {
                        go = !go;
                    }
                    if !go {
                        return Ok((last, Flow::Normal));
                    }
                }
                first = false;
                let (v, flow) = run_block(body, last, ctx, rec)?;
                last = v;
                match flow {
                    Flow::Normal | Flow::Continue => {}
                    Flow::Break => return Ok((last, Flow::Normal)),
                    // A branch out of a loop leaves the loop, and the
                    // definition's own statement list takes it from there.
                    other => return Ok((last, other)),
                }
            }
        }
        Control::For { names, source, body } => {
            let src = eval(source, ctx, rec)?;
            let n = if src.rank() == 0 { 1 } else { src.shape[0] };
            let mut last = None;
            for i in 0..n {
                if !names.is_empty() {
                    let item = if src.rank() == 0 { src.clone() } else { src.item(i) };
                    // APL binds the item's CONTENTS — `:For p :In (1 2)(3 4)`
                    // gives `p` a pair of numbers, not an enclosure of one —
                    // where J leaves its boxes shut.
                    let item = if ctx.cfg.rules.lang == Lang::Apl {
                        crate::verb::open_cell(&item)
                    } else {
                        item
                    };
                    bind_for_names(names, &item, i, ctx, span)?;
                }
                let (v, flow) = run_block(body, last, ctx, rec)?;
                last = v;
                match flow {
                    Flow::Normal | Flow::Continue => {}
                    Flow::Break => return Ok((last, Flow::Normal)),
                    // A branch out of a loop leaves the loop, and the
                    // definition's own statement list takes it from there.
                    other => return Ok((last, other)),
                }
            }
            Ok((last, Flow::Normal))
        }
        Control::Select { subject, cases } => {
            let subject = eval(subject, ctx, rec)?;
            let tol = ctx.cfg.tol;
            let mut running = false;
            let mut last = None;
            for case in cases {
                if !running {
                    match &case.test {
                        None => running = true,
                        Some(test) => {
                            let (t, flow) = run_block(test, None, ctx, rec)?;
                            if flow != Flow::Normal {
                                return Ok((t, flow));
                            }
                            // The reference compares with match (`-:`), not
                            // membership: `case. 1 2` takes the list 1 2.
                            // `:CaseList 1 2` is the membership arm, and
                            // takes either.
                            running = t.is_some_and(|v| {
                                if case.list {
                                    any_item_matches(&subject, &v, tol)
                                } else {
                                    arrays_match(&subject, &v, tol)
                                }
                            });
                        }
                    }
                }
                if running {
                    let (v, flow) = run_block(&case.body, last, ctx, rec)?;
                    last = v;
                    if flow != Flow::Normal {
                        return Ok((last, flow));
                    }
                    if !case.fall_through {
                        return Ok((last, Flow::Normal));
                    }
                    // `fcase.` runs the next body without testing it.
                    running = true;
                }
            }
            Ok((last, Flow::Normal))
        }
        Control::Try { body, catch } => {
            // The catch block answers for the languages' own errors. A gap
            // in libjay is not one of them: swallowing a "not supported
            // yet" would turn a promise into a wrong answer.
            match run_block(body, None, ctx, rec) {
                Ok(r) => Ok(r),
                Err(e) if matches!(e.kind, ErrorKind::NotYet | ErrorKind::Internal) => Err(e),
                Err(_) => run_block(catch, None, ctx, rec),
            }
        }
    }
}

/// Apply an explicit definition. `x` is None for a monadic application.
pub(crate) fn call_explicit(
    def: &Arc<ExplicitDef>,
    x: Option<&Array>,
    y: &Array,
    ctx: &mut Ctx<'_>,
    span: Span,
) -> Result<Array> {
    if x.is_some() && def.left.is_none() && !def.spare_left {
        return Err(Error::new(
            ErrorKind::Domain,
            format!("{} has no dyadic definition", def.name),
            Some(span),
        ));
    }
    if x.is_none() && def.dyad_only {
        return Err(Error::new(
            ErrorKind::Domain,
            format!(
                "{} has no monadic definition: it names {}",
                def.name,
                def.left.as_deref().unwrap_or("a left argument")
            ),
            Some(span),
        ));
    }
    let mut frame: HashMap<String, Array> = HashMap::new();
    frame.insert(def.right.clone(), y.clone());
    if let (Some(name), Some(v)) = (&def.left, x) {
        frame.insert(name.clone(), v.clone());
    }
    // A label's value is its line number, which is what `→` takes.
    for (label, at) in &def.labels {
        frame.insert(label.clone(), Array::scalar_i64(*at as i64 + 1));
    }
    ctx.env.enter(frame, Arc::clone(def), span)?;
    let mut rec = None;
    let out = run_body(&def.body, ctx, &mut rec);
    let frame = ctx.env.leave();
    let value = out?;
    // An APL `∇`-definition names its result; the body's own value is not
    // it, and a definition that never assigned the name has no result.
    if let Some(name) = &def.result {
        return frame.get(name).cloned().ok_or_else(|| {
            Error::new(
                ErrorKind::Value,
                format!("{} did not set its result {name}", def.name),
                Some(span),
            )
        });
    }
    match value {
        Some(v) => Ok(v),
        None => def.empty.clone().ok_or_else(|| {
            Error::new(
                ErrorKind::Value,
                format!("{} produced no result", def.name),
                Some(span),
            )
        }),
    }
}

/// How many statements a branching definition may run before libjay stops
/// it. A `→` loop has no other bound, and an unbounded one would hang.
const BRANCH_LIMIT: usize = 1 << 22;

/// A definition's body, statement by statement, with `→` free to move the
/// place it runs from. The value is the last statement that produced one.
fn run_body(
    stmts: &[Expr],
    ctx: &mut Ctx<'_>,
    rec: &mut Option<Trace>,
) -> Result<Option<Array>> {
    let mut last = None;
    let mut at = 0usize;
    let mut steps = 0usize;
    while at < stmts.len() {
        steps += 1;
        if steps > BRANCH_LIMIT {
            return Err(Error::new(
                ErrorKind::Domain,
                format!("a definition branched more than {BRANCH_LIMIT} times"),
                Some(stmts[at].span()),
            )
            .note("a loop written with → needs a branch that leaves it"));
        }
        let (v, flow) = eval_stmt(&stmts[at], ctx, rec)?;
        if let Some(v) = v {
            last = Some(v);
        }
        match flow {
            Flow::Normal => at += 1,
            Flow::Goto(to) => at = to,
            _ => break,
        }
    }
    Ok(last)
}

/// A noun expression's value where the whole of it can be settled now:
/// constants combined by pure verbs, with no name, no bound parameter and
/// no control flow anywhere in it. Modifiers that capture a noun operand
/// use this, so a written-out `(<a:;1)}` is as good as a literal.
pub(crate) fn fold_const(e: &Expr, cfg: EvalCfg) -> Option<Array> {
    fn closed(e: &Expr) -> bool {
        match e {
            Expr::Const(..) => true,
            Expr::Monad { verb, y, .. } => verb.is_pure() && closed(y),
            Expr::Dyad { verb, x, y, .. } => verb.is_pure() && closed(x) && closed(y),
            _ => false,
        }
    }
    if !closed(e) {
        return None;
    }
    cfg.pure(|ctx| eval(e, ctx, &mut None).ok())
}

fn eval(e: &Expr, ctx: &mut Ctx<'_>, rec: &mut Option<Trace>) -> Result<Array> {
    // The walk is recursive, so a deeply nested sentence would run out of
    // stack; the ceiling turns that into a diagnostic.
    let _depth = crate::verb::Nesting::enter(e.span())?;
    let v = eval_node(e, ctx, rec)?;
    if let Some(t) = rec.as_mut() {
        // A fused node has already left what it knows about its kernel.
        let (kernel_ran, decline, placement) = t.get(&key(e)).map_or(
            (None, None, crate::device::Placement::Default),
            |n| (n.kernel_ran, n.decline, n.placement.clone()),
        );
        t.insert(
            key(e),
            Note {
                shape: v.shape.clone(),
                dtype: v.dtype(),
                layout: v.layout(),
                kernel_ran,
                decline,
                placement,
            },
        );
    }
    Ok(v)
}

fn eval_node(e: &Expr, ctx: &mut Ctx<'_>, rec: &mut Option<Trace>) -> Result<Array> {
    match e {
        Expr::Const(a, _) => Ok(a.clone()),
        Expr::Param(i, _) => ctx.env.arg(*i),
        Expr::Name(n, span) => ctx.env.get(n).ok_or_else(|| {
            Error::new(ErrorKind::Value, format!("undefined name: {n}"), Some(*span))
        }),
        Expr::Assign { name, value, scope, .. } => {
            let v = eval(value, ctx, rec)?;
            ctx.env.assign(name.clone(), v.clone(), *scope);
            Ok(v)
        }
        Expr::AmendIndex { name, slots, value, origin, scope, span } => {
            let base = ctx.env.get(name).ok_or_else(|| {
                Error::new(ErrorKind::Value, format!("undefined name: {name}"), Some(*span))
            })?;
            // The sentence reads right to left, so the value comes first.
            let v = eval(value, ctx, rec)?;
            let mut idx = Vec::with_capacity(slots.len());
            for slot in slots {
                idx.push(match slot {
                    Some(e) => Some(eval(e, ctx, rec)?),
                    None => None,
                });
            }
            // Amending a sparse array writes into its dense expansion; the
            // stored form is not preserved across the write.
            let out = crate::verb::amend_at(
                &base.densified(),
                &idx,
                &v.densified(),
                *origin,
                ctx.cfg.near(),
                *span,
            )?;
            ctx.env.assign(name.clone(), out.clone(), *scope);
            Ok(out)
        }
        // A control sentence is run by `eval_stmt`, which is the only place
        // its signal has anywhere to go.
        Expr::Control(..) => {
            Err(Error::internal("a control sentence appeared in expression position"))
        }
        Expr::Monad { verb, y, span } => {
            let vy = eval(y, ctx, rec)?;
            verb.monad(&vy, ctx, *span)
        }
        Expr::Dyad { verb, x, y, span } => {
            // The right argument evaluates first: both languages read
            // sentences right to left, and inline assignments rely on it.
            let vy = eval(y, ctx, rec)?;
            let vx = eval(x, ctx, rec)?;
            verb.dyad(&vx, &vy, ctx, *span)
        }
        Expr::PrintPass { value, bare, .. } => {
            let v = eval(value, ctx, rec)?;
            let text = format_array(&v, &ctx.cfg.fmt);
            (ctx.out)(&text);
            // `⍞←` writes the characters and nothing else, so that several
            // of them build one line; `⎕←` ends the line it wrote.
            if !bare {
                (ctx.out)("\n");
            }
            Ok(v)
        }
        // `⍞` takes the line as characters; `⎕` runs it as APL, through the
        // same machinery `⍎` uses, over the names the program already has.
        Expr::Input { eval: run_it, span } => {
            let line = ctx.read_line(*span)?;
            if !run_it {
                return Ok(Array::from_chars(line.chars().collect()));
            }
            crate::verb::execute_source(&line, true, ctx, *span)
        }
        Expr::Fused { kernel, inputs, orig, .. } => {
            let mut vals = Vec::with_capacity(inputs.len());
            for e in inputs {
                // A fused kernel reads flat buffers, so a sparse leaf is
                // expanded before the chain sees it.
                vals.push(eval(e, ctx, rec)?.densified());
            }
            let (ran, placement) = crate::fuse::eval_on(ctx.device, kernel, &vals);
            if let Some(t) = rec.as_mut() {
                let decline =
                    if ran.is_none() { crate::fuse::decline_reason(kernel, &vals) } else { None };
                // Shape and dtype arrive from the wrapper above; only the
                // kernel's own story is recorded here.
                t.insert(
                    key(e),
                    Note {
                        shape: Vec::new(),
                        dtype: crate::dtype::DType::I64,
                        layout: crate::array::Layout::RowMajor,
                        kernel_ran: Some(ran.is_some()),
                        decline,
                        placement,
                    },
                );
            }
            match ran {
                Some(a) => Ok(a),
                // The kernel does not cover this data. The chain it came
                // from does, including whatever error it raises; it runs
                // over the values just computed, not over the leaves again.
                None => {
                    let tree = crate::fuse::fallback_tree(kernel, orig, &vals);
                    // The fallback tree is temporary, so its nodes are not
                    // ones an explanation can name: it runs unrecorded.
                    let v = eval(&tree, ctx, &mut None)?;
                    Ok(crate::fuse::fallback_finish(kernel, v))
                }
            }
        }
        // Naming a verb records it so that a definition can call itself by
        // name; the sentence is silent, so the value is never read.
        Expr::VerbDef { name, verb, .. } => {
            ctx.env.define(name.clone(), verb.clone());
            Ok(Array::scalar_i64(0))
        }
        // A record of what the program was, and a named modifier, which the
        // parser has already applied everywhere it is used: silent
        // sentences whose value is never read.
        Expr::Elided { .. } | Expr::ModDef { .. } => Ok(Array::scalar_i64(0)),
    }
}
