//! The language-agnostic program representation and its evaluator.

use std::collections::HashMap;
use std::sync::Arc;

use crate::array::{Array, Data};
use crate::error::{Error, ErrorKind, Result, Span};
use crate::fmt::{format_array, FmtOpts};
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
    /// executed. Only an explicit definition's body holds one: neither
    /// language allows a control word outside a definition.
    Control(Box<Control>, Span),
    Monad { verb: Verb, y: Box<Expr>, span: Span },
    Dyad { verb: Verb, x: Box<Expr>, y: Box<Expr>, span: Span },
    /// APL `⎕← expr`: print, pass the value through.
    PrintPass { value: Box<Expr>, span: Span },
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
    /// `for. y do. B end.` / `for_i.` / `:For i :In y`. `name` binds each
    /// item and `<name>_index` its position.
    For { name: Option<String>, source: Box<Expr>, body: Vec<Expr> },
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
    /// `continue.` / `:Continue`: start the innermost loop's next iteration.
    Continue,
}

/// One arm of an `if.` or `select.`: a test (absent for the default arm) and
/// the body to run when it holds.
#[derive(Clone, Debug)]
pub struct Branch {
    pub test: Option<Vec<Expr>>,
    pub body: Vec<Expr>,
    /// `fcase.`: run the next arm's body too, without testing it.
    pub fall_through: bool,
}

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
    /// The name the result is read from when the body does not yield one
    /// (an APL `∇`-definition's `Z←`); None means the body's own value.
    pub result: Option<String>,
    /// Names the header declares local (APL's `;name` list).
    pub locals: Vec<String>,
    pub body: Vec<Expr>,
    /// The value a body that ran nothing yields; None makes that an error.
    pub empty: Option<Array>,
    /// True when running the body can have no effect beyond its result.
    pub pure: bool,
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Const(_, s) | Expr::Param(_, s) | Expr::Name(_, s) => *s,
            Expr::Control(_, s) => *s,
            Expr::AmendIndex { span, .. } => *span,
            Expr::Assign { span, .. }
            | Expr::Monad { span, .. }
            | Expr::Dyad { span, .. }
            | Expr::PrintPass { span, .. }
            | Expr::Fused { span, .. }
            | Expr::Elided { span, .. }
            | Expr::VerbDef { span, .. } => *span,
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
    /// The dialect's comparison tolerance.
    pub tol: Tol,
}

/// What an instrumented run saw at one node.
#[derive(Clone, Debug)]
pub(crate) struct Note {
    pub shape: Vec<usize>,
    pub dtype: crate::dtype::DType,
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
    pub fn run(&self, args: &[Array], out: &mut dyn FnMut(&str)) -> Result<Option<Array>> {
        self.exec(args, out, &mut None, None)
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
        self.exec(args, out, &mut None, Some(device))
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
        let r = self.exec(args, out, &mut rec, device);
        (r, rec.expect("the recorder stays in place"))
    }

    fn exec(
        &self,
        args: &[Array],
        out: &mut dyn FnMut(&str),
        rec: &mut Option<Trace>,
        device: Option<&crate::device::Device>,
    ) -> Result<Option<Array>> {
        if args.len() != self.params.len() {
            return Err(Error::internal(format!(
                "expected {} argument(s), got {}",
                self.params.len(),
                args.len()
            )));
        }
        let cfg = EvalCfg { agreement: self.agreement, fmt: self.fmt, tol: self.tol };
        let mut env = Env::new(args.to_vec());
        let mut ctx = Ctx { cfg, out, env: &mut env, device };
        let mut last = None;
        for stmt in &self.stmts {
            // A control word cannot reach the top level in either language,
            // so a loop signal here would have nowhere to go.
            let (v, flow) = eval_stmt(stmt, &mut ctx, rec)?;
            if flow != Flow::Normal {
                return Err(Error::internal("a control signal escaped to the top level"));
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
}

/// Run a block of sentences: the value is the last sentence's, and an
/// assignment yields the value it assigned (the top level is the one place
/// that discards it, and `Program::exec` applies that rule itself).
fn run_block(
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
        Control::Break => Ok((None, Flow::Break)),
        Control::Continue => Ok((None, Flow::Continue)),
        Control::If { arms, otherwise } => {
            for arm in arms {
                let test = arm.test.as_deref().unwrap_or(&[]);
                let (t, flow) = run_block(test, None, ctx, rec)?;
                if flow != Flow::Normal {
                    return Ok((t, flow));
                }
                let taken = match &t {
                    Some(v) => is_true(v, span)?,
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
                        Some(v) => is_true(v, span)?,
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
                    Flow::Return => return Ok((last, Flow::Return)),
                }
            }
        }
        Control::For { name, source, body } => {
            let src = eval(source, ctx, rec)?;
            let n = if src.rank() == 0 { 1 } else { src.shape[0] };
            let mut last = None;
            for i in 0..n {
                if let Some(name) = name {
                    let item = if src.rank() == 0 { src.clone() } else { src.item(i) };
                    ctx.env.assign(name.clone(), item, Scope::Local);
                    ctx.env.assign(
                        format!("{name}_index"),
                        Array::scalar_i64(i as i64),
                        Scope::Local,
                    );
                }
                let (v, flow) = run_block(body, last, ctx, rec)?;
                last = v;
                match flow {
                    Flow::Normal | Flow::Continue => {}
                    Flow::Break => return Ok((last, Flow::Normal)),
                    Flow::Return => return Ok((last, Flow::Return)),
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
                            running = t.is_some_and(|v| arrays_match(&subject, &v, tol));
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
    if x.is_some() && def.left.is_none() {
        return Err(Error::new(
            ErrorKind::Domain,
            format!("{} has no dyadic definition", def.name),
            Some(span),
        ));
    }
    let mut frame: HashMap<String, Array> = HashMap::new();
    frame.insert(def.right.clone(), y.clone());
    if let (Some(name), Some(v)) = (&def.left, x) {
        frame.insert(name.clone(), v.clone());
    }
    ctx.env.enter(frame, Arc::clone(def), span)?;
    let mut rec = None;
    let out = run_block(&def.body, None, ctx, &mut rec);
    let frame = ctx.env.leave();
    let (value, _) = out?;
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

fn eval(e: &Expr, ctx: &mut Ctx<'_>, rec: &mut Option<Trace>) -> Result<Array> {
    let v = eval_node(e, ctx, rec)?;
    if let Some(t) = rec.as_mut() {
        // A fused node has already left what it knows about its kernel.
        let (kernel_ran, decline, placement) = t.get(&key(e)).map_or(
            (None, None, crate::device::Placement::Default),
            |n| (n.kernel_ran, n.decline, n.placement.clone()),
        );
        t.insert(
            key(e),
            Note { shape: v.shape.clone(), dtype: v.dtype(), kernel_ran, decline, placement },
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
            let out = crate::verb::amend_at(&base, &idx, &v, *origin, *span)?;
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
        Expr::PrintPass { value, .. } => {
            let v = eval(value, ctx, rec)?;
            let text = format_array(&v, &ctx.cfg.fmt);
            (ctx.out)(&text);
            (ctx.out)("\n");
            Ok(v)
        }
        Expr::Fused { kernel, inputs, orig, .. } => {
            let mut vals = Vec::with_capacity(inputs.len());
            for e in inputs {
                vals.push(eval(e, ctx, rec)?);
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
        // A record of what the program was: a silent sentence whose value
        // is never read.
        Expr::Elided { .. } => Ok(Array::scalar_i64(0)),
    }
}
