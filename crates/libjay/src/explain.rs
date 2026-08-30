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
        Expr::AssignMany { names, value, .. } => {
            let _ = writeln!(out, "{pad}assign ({}){}", names.join(" "), note(e, tr));
            expr_lines(value, depth + 1, p, tr, out);
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
        Expr::PrintPass { value, bare, .. } => {
            let end = if *bare { " (no line break)" } else { "" };
            let _ = writeln!(out, "{pad}print and pass on{end}{}", note(e, tr));
            expr_lines(value, depth + 1, p, tr, out);
        }
        Expr::Input { eval, .. } => {
            let what = if *eval { "reads stdin and runs the line" } else { "reads stdin" };
            let _ = writeln!(out, "{pad}{what}{}", note(e, tr));
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
        Expr::ModDef { name, spelling, conjunction, .. } => {
            let what = if *conjunction { "conjunction" } else { "adverb" };
            let _ = writeln!(
                out,
                "{pad}{what} definition {name} = {spelling}  \
                 [named at parse time; no runtime work]"
            );
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
        Control::Branch(target) => {
            let _ = writeln!(out, "{pad}branch →");
            expr_lines(target, depth + 1, p, tr, out);
        }
        Control::BranchBy { by, test } => {
            let _ = writeln!(out, "{pad}branch → this many lines on, when the test holds");
            expr_lines(by, depth + 1, p, tr, out);
            expr_lines(test, depth + 1, p, tr, out);
        }
        Control::Guard { test, body } => {
            let _ = writeln!(out, "{pad}guard — the dfn's answer when it holds");
            block("test", test, out);
            block("body", body, out);
        }
        Control::Cond { test, body, otherwise } => {
            let _ = writeln!(out, "{pad}conditional →→ … ←→ … ←←");
            block("test", test, out);
            block("body", body, out);
            block("otherwise", otherwise, out);
        }
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
        Control::For { names, source, body } => {
            let bound = match names.as_slice() {
                [] => String::new(),
                [n] => format!(", item in {n}, index in {n}_index"),
                many => format!(", item taken apart into {}", many.join(" ")),
            };
            let _ = writeln!(out, "{pad}for over items{bound}");
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
        Control::Try { body, catch, catcht } => {
            let _ = writeln!(out, "{pad}try");
            block("body", body, out);
            block("catch", catch, out);
            block("catcht", catcht, out);
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
        Control::Label(name) => {
            let _ = writeln!(out, "{pad}label {name}");
        }
        Control::Goto { name, .. } => {
            let _ = writeln!(out, "{pad}goto {name}");
        }
        Control::Throw => {
            let _ = writeln!(out, "{pad}throw");
        }
    }
}

/// `→ 2 3 $ integer` for a node an instrumented run reached; nothing for one
/// it did not.
fn note(e: &Expr, tr: &Trace) -> String {
    match tr.get(&key(e)) {
        None => String::new(),
        Some(n) => {
            let laid = match n.layout {
                crate::array::Layout::RowMajor => "",
                crate::array::Layout::ColMajor => ", column-major",
            };
            format!("  → {}{laid}", shape_dtype(&n.shape, n.dtype))
        }
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
/// A node's span covers the words it was built from, so it is widened to
/// the sentence that holds it: to the line in J, and to the `⋄`-separated
/// part of the line in APL. A span that does not land on the display source
/// yields nothing rather than cutting it: explaining never fails.
fn source_of(p: &Program, e: &Expr) -> String {
    let span = e.span();
    let src = &p.display_src;
    if span.start >= span.end
        || span.end > src.len()
        || !src.is_char_boundary(span.start)
        || !src.is_char_boundary(span.end)
    {
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

/// One operand of a user-written operator: the function's own tree, or the
/// one line an array operand needs.
fn operand_lines(
    o: &crate::verb::Operand,
    depth: usize,
    p: &Program,
    tr: &Trace,
    out: &mut String,
) {
    match o {
        crate::verb::Operand::Func(v) => verb_lines(v, depth, p, tr, out),
        crate::verb::Operand::Value(_) => {
            let _ = writeln!(out, "{}an array operand", " ".repeat(depth * STEP));
        }
    }
}

fn verb_lines(v: &Verb, depth: usize, p: &Program, tr: &Trace, out: &mut String) {
    let pad = " ".repeat(depth * STEP);
    let head = |out: &mut String, what: &str| {
        let _ = writeln!(out, "{pad}{what}  ranks {}", ranks(v.ranks()));
    };
    match v {
        Verb::Prim(p) => {
            let reads = if p.monad == crate::verb::MonadOp::ReadStream { " (reads stdin)" } else { "" };
            head(out, &format!("{} primitive{reads}", p.name))
        }
        Verb::Constant(m) => head(out, &format!("the constant verb answering {}", brief(m))),
        Verb::Rank(u, r) => {
            head(out, &format!("rank \"{}", ranks(*r)));
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Reduce(u) => {
            head(out, "reduce (insert between items)");
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Cycle(vs) => {
            head(out, &format!("gerund ({} verbs, one per piece)", vs.len()));
            for u in vs {
                verb_lines(u, depth + 1, p, tr, out);
            }
        }
        Verb::NWise(u) => {
            head(out, "reduce (insert between items); n-wise with a left argument");
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Fit(u, n) => {
            head(out, &format!("fit !.{n} (comparison tolerance)"));
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Fill(u, f) => {
            head(out, &format!("fit !.{f} (the element a value runs out into)"));
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Amend(m) => head(out, &format!("amend at {} index(es)", m.count())),
        Verb::Choose(m) => head(out, &format!("select between two arguments by a {}-position mask", m.count())),
        Verb::AmendVerb(u) => {
            head(out, "amend at the indices a verb computes");
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::ShiftFill(f) => {
            head(out, &format!("shift, filling with {} atom(s)", f.count()))
        }
        Verb::Memo(u, _) => {
            head(out, "memo M. (answers repeat from a cache)");
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Level { u, level, spread } => {
            head(
                out,
                &format!(
                    "{} at boxing level {level}",
                    if *spread { "spread S:" } else { "level L:" }
                ),
            );
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Before(f, g) => {
            head(out, "before ⍛ (the left argument is prepared)");
            verb_lines(f, depth + 1, p, tr, out);
            verb_lines(g, depth + 1, p, tr, out);
        }
        Verb::InnerProduct { u, v, .. } => {
            head(out, "inner product (u . v; the matrix product for +/ . *)");
            verb_lines(u, depth + 1, p, tr, out);
            verb_lines(v, depth + 1, p, tr, out);
        }
        Verb::UserDerived { def, alpha, omega } => {
            head(out, "a user-written operator with its operands");
            if let Ok(body) = def.pick(alpha, omega.as_ref()) {
                verb_lines(body, depth + 1, p, tr, out);
            }
            operand_lines(alpha, depth + 1, p, tr, out);
            if let Some(g) = omega {
                operand_lines(g, depth + 1, p, tr, out);
            }
        }
        Verb::KeyPairs(u) => {
            head(out, "key ⌸ (each key with what shares it)");
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Characteristics(u) => {
            head(out, "characteristics b. (answers about the verb)");
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Key(u) => {
            head(out, "key / oblique");
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Cut(u, n) => {
            head(out, &format!("cut ;.{n}"));
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::BoundAxis(f, a) => {
            head(out, &format!("an axis bound in the definition: {}", brief(a)));
            verb_lines(f, depth + 1, p, tr, out);
        }
        Verb::Deferred(d) => {
            head(out, &format!("{}, its operand read at each application", d.spelling));
            verb_lines(&d.template, depth + 1, p, tr, out);
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
        Verb::Hypergeometric { .. } => head(out, "hypergeometric H. (a series)"),
        Verb::Beside(f, g) => {
            head(out, "beside ∘ (the right argument is prepared)");
            verb_lines(f, depth + 1, p, tr, out);
            verb_lines(g, depth + 1, p, tr, out);
        }
        Verb::Adverse(u, w) => {
            head(out, "adverse :: (the second verb answers a refusal)");
            verb_lines(u, depth + 1, p, tr, out);
            verb_lines(w, depth + 1, p, tr, out);
        }
        Verb::At { left, right } => {
            head(out, &format!("at @ ({} where {})", left.name(), right.name()));
            if let crate::verb::Operand::Func(f) = left {
                verb_lines(f, depth + 1, p, tr, out);
            }
            if let crate::verb::Operand::Func(g) = right {
                verb_lines(g, depth + 1, p, tr, out);
            }
        }
        Verb::AmendGerund(vs) => {
            head(out, "gerund amend } (the replacement, the indices, the array)");
            for v in vs {
                verb_lines(v, depth + 1, p, tr, out);
            }
        }
        Verb::Ambivalent(u, w) => {
            head(out, "monad-dyad pair : (the monad first, then the dyad)");
            verb_lines(u, depth + 1, p, tr, out);
            verb_lines(w, depth + 1, p, tr, out);
        }
        Verb::WithObverse(u, w) => {
            head(out, "verb with a declared obverse :.");
            verb_lines(u, depth + 1, p, tr, out);
            verb_lines(w, depth + 1, p, tr, out);
        }
        Verb::Agenda(vs, w) => {
            head(out, &format!("agenda @. over {} verbs", vs.len()));
            for u in vs {
                verb_lines(u, depth + 1, p, tr, out);
            }
            verb_lines(w, depth + 1, p, tr, out);
        }
        Verb::Stencil(u, w) => {
            let sizes: Vec<String> = w.iter().map(i64::to_string).collect();
            head(out, &format!("stencil ⌺ over windows of {}", sizes.join(" ")));
            verb_lines(u, depth + 1, p, tr, out);
        }
        Verb::Evoke(vs, n) => {
            let what = if *n == 0 { "applied to the arguments" } else { "inserted between items" };
            head(out, &format!("evoke `:{n} — {} verbs {what}", vs.len()));
            for u in vs {
                verb_lines(u, depth + 1, p, tr, out);
            }
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
                    Power::ConvergeTrace => "power (every result to convergence)".to_string(),
                    Power::Times(k) => format!("power ({k} times)"),
                    Power::Each(ks) => format!("power ({} counts, framed)", ks.len()),
                    Power::Inverse(k) => format!("power (the inverse, {k} times)"),
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
        Verb::UnderRavel(u) => {
            head(out, "under ravel (flatten, apply, put the shape back)");
            verb_lines(u, depth + 1, p, tr, out);
        }
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
