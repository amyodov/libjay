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
use crate::ir::{key, Control, Expr, ExplicitDef, Program, Trace};
use crate::verb::{Enclose, Power, Verb, WindowKind, RANK_INF};

/// Width of one level of the outline.
const STEP: usize = 2;

pub(crate) fn explain(
    p: &Program,
    args: Option<&[Array]>,
    device: Option<&crate::device::Device>,
) -> String {
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
        let (result, t) = p.trace(a, &mut sink, device);
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
    if let Some(d) = device {
        let _ = writeln!(
            out,
            "device: {}",
            match d.info() {
                None => "cpu".to_string(),
                Some(i) => format!("{} ({}, {}), computing in {}", i.name, i.backend, i.kind,
                    d.precision().name()),
            }
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
            verb_lines(verb, depth + 1, p, tr, out);
            let _ = writeln!(out, "{pad}  y:");
            expr_lines(y, depth + 2, p, tr, out);
        }
        Expr::Dyad { verb, x, y, .. } => {
            let _ = writeln!(out, "{pad}dyad {}{}", verb.name(), note(e, tr));
            verb_lines(verb, depth + 1, p, tr, out);
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
            verb_lines(verb, depth + 1, p, tr, out);
        }
        Expr::AmendIndex { name, slots, value, .. } => {
            let shown: Vec<String> = slots
                .iter()
                .map(|s| if s.is_some() { "i".to_string() } else { String::new() })
                .collect();
            let _ =
                writeln!(out, "{pad}amend {name}[{}]{}", shown.join(";"), note(e, tr));
            for slot in slots.iter().flatten() {
                let _ = writeln!(out, "{pad}  index:");
                expr_lines(slot, depth + 2, p, tr, out);
            }
            let _ = writeln!(out, "{pad}  value:");
            expr_lines(value, depth + 2, p, tr, out);
        }
        Expr::Control(c, _) => control_lines(c, depth, p, tr, out),
    }
}

/// A control sentence: one line naming it, then its tests and bodies, each
/// body a block of sentences one level further in.
fn control_lines(c: &Control, depth: usize, p: &Program, tr: &Trace, out: &mut String) {
    let pad = " ".repeat(depth * STEP);
    let block = |label: &str, stmts: &[Expr], out: &mut String| {
        let _ = writeln!(out, "{pad}  {label}:");
        for s in stmts {
            expr_lines(s, depth + 2, p, tr, out);
        }
    };
    match c {
        Control::If { arms, otherwise } => {
            let _ = writeln!(out, "{pad}if — {} arm(s)", arms.len());
            for (i, arm) in arms.iter().enumerate() {
                if let Some(test) = &arm.test {
                    block(&format!("test {}", i + 1), test, out);
                }
                block(&format!("body {}", i + 1), &arm.body, out);
            }
            if let Some(body) = otherwise {
                block("else", body, out);
            }
        }
        Control::While { test, body, body_first, until } => {
            let kind = match (body_first, until) {
                (false, false) => "while",
                (true, false) => "while, body first",
                (false, true) => "until",
                (true, true) => "repeat until",
            };
            let _ = writeln!(out, "{pad}{kind}");
            block("test", test, out);
            block("body", body, out);
        }
        Control::For { name, source, body } => {
            let _ = writeln!(
                out,
                "{pad}for over items{}",
                name.as_ref().map_or(String::new(), |n| format!(", item in {n}, index in {n}_index"))
            );
            block("source", std::slice::from_ref(source), out);
            block("body", body, out);
        }
        Control::Select { subject, cases } => {
            let _ = writeln!(out, "{pad}select — {} case(s), matched with -:", cases.len());
            block("subject", std::slice::from_ref(subject), out);
            for (i, case) in cases.iter().enumerate() {
                match &case.test {
                    Some(test) => block(&format!("case {}", i + 1), test, out),
                    None => {
                        let _ = writeln!(out, "{pad}  case {} (default):", i + 1);
                    }
                }
                block(
                    &format!("body {}{}", i + 1, if case.fall_through { " (falls through)" } else { "" }),
                    &case.body,
                    out,
                );
            }
        }
        Control::Try { body, catch } => {
            let _ = writeln!(out, "{pad}try");
            block("body", body, out);
            block("catch", catch, out);
        }
        Control::Return => {
            let _ = writeln!(out, "{pad}return");
        }
        Control::Break => {
            let _ = writeln!(out, "{pad}break");
        }
        Control::Continue => {
            let _ = writeln!(out, "{pad}continue");
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

/// Whether the kernel itself produced the value, and why not when it did
/// not; and, for a run that named a device, where the arithmetic happened.
fn kernel_note(e: &Expr, tr: &Trace) -> String {
    let Some(n) = tr.get(&key(e)) else { return String::new() };
    let Some(ran) = n.kernel_ran else { return String::new() };
    let mut s = if ran {
        "  [kernel ran".to_string()
    } else {
        format!(
            "  [kernel declined: {}",
            n.decline.map_or("the chain ran instead", fuse::Decline::reason)
        )
    };
    if n.placement != crate::device::Placement::Default {
        s.push_str("; ");
        s.push_str(&n.placement.to_string());
    }
    s.push(']');
    s
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

fn verb_lines(v: &Verb, depth: usize, p: &Program, tr: &Trace, out: &mut String) {
    let pad = " ".repeat(depth * STEP);
    let head = |out: &mut String, what: &str| {
        let _ = writeln!(out, "{pad}{what}  ranks {}", ranks(v.ranks()));
    };
    match v {
        Verb::Prim(p) => head(out, &format!("{} primitive", p.name)),
        Verb::Rank(u, r) => {
            head(out, &format!("rank \"{}", ranks(*r)));
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Reduce(u) => {
            head(out, "reduce (insert between items)");
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Fit(u, n) => {
            head(out, &format!("fit !.{n} (comparison tolerance)"));
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Amend(m) => head(out, &format!("amend at {} index(es)", m.count())),
        Verb::Key(u) => {
            head(out, "key / oblique");
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Cut(u, n) => {
            head(out, &format!("cut ;.{n}"));
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::PowerV(u, v) => {
            head(out, "power, count from a verb");
            verb_lines(u, depth + 1, p, tr, out);
            verb_lines(v, depth + 1, p, tr, out);
        }
        Verb::PowerUntil(u, v) => {
            head(out, "power, until a test holds");
            verb_lines(u, depth + 1, p, tr, out);
            verb_lines(v, depth + 1, p, tr, out);
        }
        Verb::AlongAxis(u, k) => {
            head(out, &format!("along axis {k}"));
            verb_lines(u, depth + 1, p, tr, out);
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
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Commute(u) => {
            head(out, "commute (swap the arguments)");
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::PowerN(u, n) => {
            head(
                out,
                &match n {
                    Power::Converge => "power (to convergence)".to_string(),
                    Power::Times(k) => format!("power ({k} times)"),
                },
            );
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Fork(f, g, h) => {
            head(out, "fork (f y) g (h y)");
            verb_lines(f, depth + 1, p, tr, out);
            verb_lines(g, depth + 1, p, tr, out);
            verb_lines(h, depth + 1, p, tr, out);
        }
        Verb::NounFork(a, g, h) => {
            head(out, &format!("fork with the noun {}", brief(a)));
            verb_lines(g, depth + 1, p, tr, out);
            verb_lines(h, depth + 1, p, tr, out);
        }
        Verb::Hook(f, g) => {
            head(out, "hook y f (g y)");
            verb_lines(f, depth + 1, p, tr, out);
            verb_lines(g, depth + 1, p, tr, out);
        }
        Verb::Atop(f, g) => {
            head(out, "atop f (g y)");
            verb_lines(f, depth + 1, p, tr, out);
            verb_lines(g, depth + 1, p, tr, out);
        }
        Verb::Compose(f, g) => {
            head(out, "compose (g x) f (g y)");
            verb_lines(f, depth + 1, p, tr, out);
            verb_lines(g, depth + 1, p, tr, out);
        }
        Verb::BondLeft(a, u) => {
            head(out, &format!("bond, left argument {}", brief(a)));
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::BondRight(u, a) => {
            head(out, &format!("bond, right argument {}", brief(a)));
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Explicit(d) => {
            head(out, &format!("explicit definition {}", d.name));
            explicit_lines(d, depth + 1, p, tr, out);
        }
        Verb::SelfRef => head(out, "self-reference (the definition it stands in)"),
        Verb::Named(n) => head(out, &format!("verb named {n}, resolved when it is applied")),
        Verb::Each(u, kind) => {
            head(
                out,
                match kind {
                    Enclose::Always => "each (open, apply, box again)",
                    Enclose::ExceptSimpleScalar => "each (open, apply, enclose again)",
                },
            );
            verb_lines(u, depth + 1, p, tr, out);
        }
    }
}

/// An explicit definition's shape: how it takes its arguments and how many
/// sentences its body holds. The body itself is a program of its own, so it
/// is summarised rather than unfolded.
fn explicit_lines(d: &ExplicitDef, depth: usize, p: &Program, tr: &Trace, out: &mut String) {
    let pad = " ".repeat(depth * STEP);
    let args = match &d.left {
        Some(x) => format!("{x} and {}", d.right),
        None => d.right.clone(),
    };
    let _ = writeln!(out, "{pad}arguments {args}; body of {} sentence(s)", d.body.len());
    if let Some(z) = &d.result {
        let _ = writeln!(out, "{pad}result read from {z}");
    }
    if !d.locals.is_empty() {
        let _ = writeln!(out, "{pad}declared local: {}", d.locals.join(", "));
    }
    // The body is a program of its own; it is shown the same way, one
    // level further in. A run's notes belong to the sentences the program
    // itself holds, so the body's nodes carry none.
    for (i, stmt) in d.body.iter().enumerate() {
        let _ = writeln!(out, "{pad}sentence {}:", i + 1);
        expr_lines(stmt, depth + 1, p, tr, out);
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
