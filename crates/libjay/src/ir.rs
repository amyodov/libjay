//! The language-agnostic program representation and its evaluator.

use std::collections::HashMap;

use crate::array::Array;
use crate::error::{Error, ErrorKind, Result, Span};
use crate::fmt::{format_array, FmtOpts};
use crate::fuse::FusedKernel;
use crate::verb::{Agreement, Ctx, EvalCfg, Verb};

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
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Const(_, s) | Expr::Param(_, s) | Expr::Name(_, s) => *s,
            Expr::Assign { span, .. }
            | Expr::Monad { span, .. }
            | Expr::Dyad { span, .. }
            | Expr::PrintPass { span, .. }
            | Expr::Fused { span, .. } => *span,
        }
    }

    /// Sentences whose top level is an assignment (or explicit output)
    /// yield no value to the sequence.
    fn is_silent(&self) -> bool {
        matches!(self, Expr::Assign { .. } | Expr::PrintPass { .. })
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
}

impl Program {
    /// Execute with one value per parameter, in `params` order.
    /// Returns None when the last sentence yields no value.
    pub fn run(&self, args: &[Array], out: &mut dyn FnMut(&str)) -> Result<Option<Array>> {
        if args.len() != self.params.len() {
            return Err(Error::internal(format!(
                "expected {} argument(s), got {}",
                self.params.len(),
                args.len()
            )));
        }
        let cfg = EvalCfg { agreement: self.agreement, fmt: self.fmt };
        let mut ctx = Ctx { cfg, out };
        let mut env: HashMap<String, Array> = HashMap::new();
        let mut last = None;
        for stmt in &self.stmts {
            let v = eval(stmt, args, &mut env, &mut ctx)?;
            last = if stmt.is_silent() { None } else { Some(v) };
        }
        Ok(last)
    }

    pub fn render_error(&self, e: &Error) -> String {
        e.render(&self.display_src)
    }
}

fn eval(
    e: &Expr,
    args: &[Array],
    env: &mut HashMap<String, Array>,
    ctx: &mut Ctx<'_>,
) -> Result<Array> {
    match e {
        Expr::Const(a, _) => Ok(a.clone()),
        Expr::Param(i, _) => Ok(args[*i].clone()),
        Expr::Name(n, span) => env.get(n).cloned().ok_or_else(|| {
            Error::new(ErrorKind::Value, format!("undefined name: {n}"), Some(*span))
        }),
        Expr::Assign { name, value, .. } => {
            let v = eval(value, args, env, ctx)?;
            env.insert(name.clone(), v.clone());
            Ok(v)
        }
        Expr::Monad { verb, y, span } => {
            let vy = eval(y, args, env, ctx)?;
            verb.monad(&vy, ctx, *span)
        }
        Expr::Dyad { verb, x, y, span } => {
            // The right argument evaluates first: both languages read
            // sentences right to left, and inline assignments rely on it.
            let vy = eval(y, args, env, ctx)?;
            let vx = eval(x, args, env, ctx)?;
            verb.dyad(&vx, &vy, ctx, *span)
        }
        Expr::PrintPass { value, .. } => {
            let v = eval(value, args, env, ctx)?;
            let text = format_array(&v, &ctx.cfg.fmt);
            (ctx.out)(&text);
            (ctx.out)("\n");
            Ok(v)
        }
        Expr::Fused { kernel, inputs, orig, .. } => {
            let mut vals = Vec::with_capacity(inputs.len());
            for e in inputs {
                vals.push(eval(e, args, env, ctx)?);
            }
            match crate::fuse::eval(kernel, &vals) {
                Some(a) => Ok(a),
                // The kernel does not cover this data. The chain it came
                // from does, including whatever error it raises; it runs
                // over the values just computed, not over the leaves again.
                None => eval(&crate::fuse::fallback_tree(kernel, orig, &vals), args, env, ctx),
            }
        }
    }
}
