//! The language-agnostic program representation and its evaluator.

use std::collections::HashMap;

use crate::array::Array;
use crate::error::{Error, ErrorKind, Result, Span};
use crate::fmt::{format_array, FmtOpts};
use crate::fuse::FusedKernel;
use crate::verb::{Agreement, Ctx, EvalCfg, Tol, Verb};

#[derive(Clone, Debug)]
pub enum Expr {
    Const(Array, Span),
    /// A bound parameter, by position in `Program::params`.
    Param(usize, Span),
    /// A name assigned earlier in the same program.
    Name(String, Span),
    /// Yields the assigned value in expression position; a whole sentence
    /// that is an assignment displays nothing.
    Assign { name: String, value: Box<Expr>, span: Span },
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

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Const(_, s) | Expr::Param(_, s) | Expr::Name(_, s) => *s,
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
        self.exec(args, out, &mut None)
    }

    /// Execute and record every node's result shape and dtype. The trace is
    /// returned even when a sentence fails, so that a partial explanation
    /// still shows what did run.
    pub(crate) fn trace(
        &self,
        args: &[Array],
        out: &mut dyn FnMut(&str),
    ) -> (Result<Option<Array>>, Trace) {
        let mut rec = Some(Trace::new());
        let r = self.exec(args, out, &mut rec);
        (r, rec.expect("the recorder stays in place"))
    }

    fn exec(
        &self,
        args: &[Array],
        out: &mut dyn FnMut(&str),
        rec: &mut Option<Trace>,
    ) -> Result<Option<Array>> {
        if args.len() != self.params.len() {
            return Err(Error::internal(format!(
                "expected {} argument(s), got {}",
                self.params.len(),
                args.len()
            )));
        }
        let cfg = EvalCfg { agreement: self.agreement, fmt: self.fmt, tol: self.tol };
        let mut ctx = Ctx { cfg, out };
        let mut env: HashMap<String, Array> = HashMap::new();
        let mut last = None;
        for stmt in &self.stmts {
            let v = eval(stmt, args, &mut env, &mut ctx, rec)?;
            last = if stmt.is_silent() { None } else { Some(v) };
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
        crate::explain::explain(self, args)
    }
}

fn eval(
    e: &Expr,
    args: &[Array],
    env: &mut HashMap<String, Array>,
    ctx: &mut Ctx<'_>,
    rec: &mut Option<Trace>,
) -> Result<Array> {
    let v = eval_node(e, args, env, ctx, rec)?;
    if let Some(t) = rec.as_mut() {
        // A fused node has already left what it knows about its kernel.
        let (kernel_ran, decline) =
            t.get(&key(e)).map_or((None, None), |n| (n.kernel_ran, n.decline));
        t.insert(
            key(e),
            Note { shape: v.shape.clone(), dtype: v.dtype(), kernel_ran, decline },
        );
    }
    Ok(v)
}

fn eval_node(
    e: &Expr,
    args: &[Array],
    env: &mut HashMap<String, Array>,
    ctx: &mut Ctx<'_>,
    rec: &mut Option<Trace>,
) -> Result<Array> {
    match e {
        Expr::Const(a, _) => Ok(a.clone()),
        Expr::Param(i, _) => Ok(args[*i].clone()),
        Expr::Name(n, span) => env.get(n).cloned().ok_or_else(|| {
            Error::new(ErrorKind::Value, format!("undefined name: {n}"), Some(*span))
        }),
        Expr::Assign { name, value, .. } => {
            let v = eval(value, args, env, ctx, rec)?;
            env.insert(name.clone(), v.clone());
            Ok(v)
        }
        Expr::Monad { verb, y, span } => {
            let vy = eval(y, args, env, ctx, rec)?;
            verb.monad(&vy, ctx, *span)
        }
        Expr::Dyad { verb, x, y, span } => {
            // The right argument evaluates first: both languages read
            // sentences right to left, and inline assignments rely on it.
            let vy = eval(y, args, env, ctx, rec)?;
            let vx = eval(x, args, env, ctx, rec)?;
            verb.dyad(&vx, &vy, ctx, *span)
        }
        Expr::PrintPass { value, .. } => {
            let v = eval(value, args, env, ctx, rec)?;
            let text = format_array(&v, &ctx.cfg.fmt);
            (ctx.out)(&text);
            (ctx.out)("\n");
            Ok(v)
        }
        Expr::Fused { kernel, inputs, orig, .. } => {
            let mut vals = Vec::with_capacity(inputs.len());
            for e in inputs {
                vals.push(eval(e, args, env, ctx, rec)?);
            }
            let ran = crate::fuse::eval(kernel, &vals);
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
                    let v = eval(&tree, args, env, ctx, &mut None)?;
                    Ok(crate::fuse::fallback_finish(kernel, v))
                }
            }
        }
        // A record of what the program was, and a verb the frontend has
        // already substituted: both sentences are silent, so neither value
        // is ever read.
        Expr::Elided { .. } | Expr::VerbDef { .. } => Ok(Array::scalar_i64(0)),
    }
}
