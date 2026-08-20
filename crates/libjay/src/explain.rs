//! Rendering a compiled program as the structure it became.
//!
//! What a J or APL sentence compiles to is not what it looks like: a train
//! becomes a fork, a modifier becomes a wrapper around a verb, and a chain
//! of elementwise verbs becomes one blockwise kernel with the reduction
//! above it folded in. [`crate::Program::explain`] prints that structure,
//! one section per sentence, and — when there are values to run — what each
//! node produced.
//!
//! The instrumentation is the evaluator itself. [`crate::ir`]'s `eval`
//! takes an optional recorder and notes every node's result shape and dtype
//! under the node's address; explaining then walks the same tree and reads
//! the notes off. One code path runs both ways, so an explained run cannot
//! mean anything different from a plain one — which a second walker, or a
//! re-evaluation, could not promise.

use std::fmt::Write as _;

use crate::array::Array;
use crate::dtype::DType;
use crate::fmt::format_array;
use crate::fuse;
use crate::ir::{key, Expr, Program, Trace};
use crate::verb::{Enclose, Power, Verb, WindowKind, RANK_INF};

/// Width of one level of the outline.
const STEP: usize = 2;

pub(crate) fn explain(p: &Program, args: Option<&[Array]>) -> String {
    let none: [Array; 0] = [];
    // A program with no parameters has everything it needs already.
    let args = match args {
        Some(a) if a.len() == p.params.len() => Some(a),
        Some(_) => None,
        None if p.params.is_empty() => Some(&none[..]),
        None => None,
    };

    let mut trace = Trace::new();
    let mut failure = None;
    if let Some(a) = args {
        // Output the program makes belongs to the program, not here.
        let mut sink = |_: &str| {};
        let (result, t) = p.trace(a, &mut sink);
        trace = t;
        if let Err(e) = result {
            failure = Some(p.render_error(&e));
        }
    }

    let mut out = String::new();
    let params: Vec<&str> = p.params.iter().map(|s| s.name.as_str()).collect();
    out.push_str("source:\n");
    for line in p.display_src.lines() {
        let _ = writeln!(out, "  {line}");
    }
    if !params.is_empty() {
        let _ = writeln!(out, "parameters: {}", params.join(", "));
    }
    let inlined = fuse::inlined_names(p);
    if !inlined.is_empty() {
        let _ = writeln!(
            out,
            "inlined into kernels: {} (elided; errors guarded by tally)",
            inlined.join(", ")
        );
    }
    if args.is_none() {
        out.push_str("values: none supplied — structure only\n");
    }

    for (i, stmt) in p.stmts.iter().enumerate() {
        let text = source_of(p, stmt);
        let _ = write!(out, "\nsentence {}", i + 1);
        if !text.is_empty() {
            let _ = write!(out, "  |  {text}");
        }
        out.push('\n');
        expr_lines(stmt, 1, p, &trace, &mut out);
    }

    if let Some(e) = failure {
        let _ = write!(out, "\nthe run stopped here:\n{e}");
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

// ------------------------------------------------------------- expressions

fn expr_lines(e: &Expr, depth: usize, p: &Program, tr: &Trace, out: &mut String) {
    let pad = " ".repeat(depth * STEP);
    match e {
        Expr::Const(a, _) => {
            let _ = writeln!(out, "{pad}const {}{}", brief(a), note(e, tr));
        }
        Expr::Param(i, _) => {
            let name = p.params.get(*i).map_or("?", |s| s.name.as_str());
            let _ = writeln!(out, "{pad}{{{name}}}{}", note(e, tr));
        }
        Expr::Name(n, _) => {
            let _ = writeln!(out, "{pad}name {n}{}", note(e, tr));
        }
        Expr::Assign { name, value, .. } => {
            // A name the fusion pass introduced replaces an assignment it
            // elided: what is left of the sentence is the tally that still
            // raises whatever the assignment would have raised.
            let origin = if name.starts_with('·') {
                "  [introduced by the fusion pass; the elided value's errors are guarded here]"
            } else {
                ""
            };
            let _ = writeln!(out, "{pad}assign {name}{}{origin}", note(e, tr));
            expr_lines(value, depth + 1, p, tr, out);
        }
        Expr::Monad { verb, y, .. } => {
            let _ = writeln!(out, "{pad}monad {}{}", verb.name(), note(e, tr));
            verb_lines(verb, depth + 1, out);
            let _ = writeln!(out, "{pad}  y:");
            expr_lines(y, depth + 2, p, tr, out);
        }
        Expr::Dyad { verb, x, y, .. } => {
            let _ = writeln!(out, "{pad}dyad {}{}", verb.name(), note(e, tr));
            verb_lines(verb, depth + 1, out);
            let _ = writeln!(out, "{pad}  x:");
            expr_lines(x, depth + 2, p, tr, out);
            let _ = writeln!(out, "{pad}  y:");
            expr_lines(y, depth + 2, p, tr, out);
        }
        Expr::PrintPass { value, .. } => {
            let _ = writeln!(out, "{pad}print and pass on{}", note(e, tr));
            expr_lines(value, depth + 1, p, tr, out);
        }
        Expr::Fused { kernel, inputs, orig, .. } => {
            let _ = writeln!(
                out,
                "{pad}fused kernel ({}){}{}",
                fuse::summary(kernel),
                note(e, tr),
                kernel_note(e, tr)
            );
            for (k, input) in inputs.iter().enumerate() {
                let _ = writeln!(out, "{pad}  in {k}:");
                expr_lines(input, depth + 2, p, tr, out);
            }
            let _ = writeln!(out, "{pad}  falls back to:");
            expr_lines(orig, depth + 2, p, tr, out);
        }
        Expr::Elided { orig, .. } => {
            let _ = writeln!(out, "{pad}the fusion pass rewrote the sentences below; it was:");
            for stmt in orig {
                let text = source_of(p, stmt);
                if !text.is_empty() {
                    let _ = writeln!(out, "{pad}  {text}");
                }
            }
        }
        Expr::VerbDef { name, verb, .. } => {
            let _ = writeln!(
                out,
                "{pad}verb definition {name} = {}  [named at parse time; no runtime work]",
                verb.name()
            );
            verb_lines(verb, depth + 1, out);
        }
    }
}

/// `→ 2 3 $ integer` for a node an instrumented run reached; nothing for one
/// it did not.
fn note(e: &Expr, tr: &Trace) -> String {
    match tr.get(&key(e)) {
        None => String::new(),
        Some(n) => format!("  → {}", shape_dtype(&n.shape, n.dtype)),
    }
}

/// Whether the kernel itself produced the value, and why not when it did not.
fn kernel_note(e: &Expr, tr: &Trace) -> String {
    match tr.get(&key(e)).and_then(|n| n.kernel_ran.map(|r| (r, n.decline))) {
        None => String::new(),
        Some((true, _)) => "  [kernel ran]".to_string(),
        Some((false, reason)) => format!(
            "  [kernel declined: {}]",
            reason.map_or("the chain ran instead", fuse::Decline::reason)
        ),
    }
}

fn shape_dtype(shape: &[usize], dtype: DType) -> String {
    if shape.is_empty() {
        return format!("scalar {}", dtype.name());
    }
    let axes: Vec<String> = shape.iter().map(usize::to_string).collect();
    format!("{} $ {}", axes.join(" "), dtype.name())
}

/// A constant as one short line: small ones in full, larger ones by shape.
fn brief(a: &Array) -> String {
    if a.rank() <= 1 && a.items() <= 6 && a.dtype() != DType::Box {
        return one_line(&format_array(a, &crate::fmt::FmtOpts::J));
    }
    shape_dtype(&a.shape, a.dtype())
}

fn one_line(s: &str) -> String {
    s.trim_end().replace('\n', " ")
}

/// The sentence this node was compiled from, as the user wrote it.
///
/// A node's span covers what the parser kept, which drops the parentheses
/// around it, so the span is widened to the sentence that holds it: to the
/// line in J, and to the `⋄`-separated part of the line in APL.
fn source_of(p: &Program, e: &Expr) -> String {
    let span = e.span();
    let src = &p.display_src;
    if span.start >= span.end || span.end > src.len() {
        return String::new();
    }
    let mut start = src[..span.start].rfind('\n').map_or(0, |i| i + 1);
    let mut end = src[span.end..].find('\n').map_or(src.len(), |i| span.end + i);
    if let Some(i) = src[start..span.start].rfind('⋄') {
        start += i + '⋄'.len_utf8();
    }
    if let Some(i) = src[span.end..end].find('⋄') {
        end = span.end + i;
    }
    src[start..end].trim().to_string()
}

// ------------------------------------------------------------------- verbs

fn verb_lines(v: &Verb, depth: usize, out: &mut String) {
    let pad = " ".repeat(depth * STEP);
    let head = |out: &mut String, what: &str| {
        let _ = writeln!(out, "{pad}{what}  ranks {}", ranks(v.ranks()));
    };
    match v {
        Verb::Prim(p) => head(out, &format!("{} primitive", p.name)),
        Verb::Rank(u, r) => {
            head(out, &format!("rank \"{}", ranks(*r)));
            verb_lines(u, depth + 1, out);
        }
        Verb::Reduce(u) => {
            head(out, "reduce (insert between items)");
            verb_lines(u, depth + 1, out);
        }
        Verb::Windowed(u, kind) => {
            head(
                out,
                match kind {
                    WindowKind::Prefix => "windowed over prefixes",
                    WindowKind::Suffix => "windowed over suffixes",
                    WindowKind::Scan => "scan",
                },
            );
            verb_lines(u, depth + 1, out);
        }
        Verb::Commute(u) => {
            head(out, "commute (swap the arguments)");
            verb_lines(u, depth + 1, out);
        }
        Verb::PowerN(u, n) => {
            head(
                out,
                &match n {
                    Power::Converge => "power (to convergence)".to_string(),
                    Power::Times(k) => format!("power ({k} times)"),
                },
            );
            verb_lines(u, depth + 1, out);
        }
        Verb::Fork(f, g, h) => {
            head(out, "fork (f y) g (h y)");
            verb_lines(f, depth + 1, out);
            verb_lines(g, depth + 1, out);
            verb_lines(h, depth + 1, out);
        }
        Verb::NounFork(a, g, h) => {
            head(out, &format!("fork with the noun {}", brief(a)));
            verb_lines(g, depth + 1, out);
            verb_lines(h, depth + 1, out);
        }
        Verb::Hook(f, g) => {
            head(out, "hook y f (g y)");
            verb_lines(f, depth + 1, out);
            verb_lines(g, depth + 1, out);
        }
        Verb::Atop(f, g) => {
            head(out, "atop f (g y)");
            verb_lines(f, depth + 1, out);
            verb_lines(g, depth + 1, out);
        }
        Verb::Compose(f, g) => {
            head(out, "compose (g x) f (g y)");
            verb_lines(f, depth + 1, out);
            verb_lines(g, depth + 1, out);
        }
        Verb::BondLeft(a, u) => {
            head(out, &format!("bond, left argument {}", brief(a)));
            verb_lines(u, depth + 1, out);
        }
        Verb::BondRight(u, a) => {
            head(out, &format!("bond, right argument {}", brief(a)));
            verb_lines(u, depth + 1, out);
        }
        Verb::Each(u, kind) => {
            head(
                out,
                match kind {
                    Enclose::Always => "each (open, apply, box again)",
                    Enclose::ExceptSimpleScalar => "each (open, apply, enclose again)",
                },
            );
            verb_lines(u, depth + 1, out);
        }
    }
}

fn ranks(r: [i64; 3]) -> String {
    let one = |n: i64| match n {
        RANK_INF => "_".to_string(),
        n if n == -RANK_INF => "__".to_string(),
        n => n.to_string(),
    };
    format!("{} {} {}", one(r[0]), one(r[1]), one(r[2]))
}
