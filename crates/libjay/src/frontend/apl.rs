//! APL frontend: lexer and parser, lowering to the shared IR.
//!
//! APL sentences read right to left: the rightmost expression is the right
//! argument of the function to its left, and a function is dyadic exactly
//! when an operand ends immediately to its left. Operators bind tighter than
//! that: `f/` and `f⍤r` are folded into derived functions before the
//! sentence is parsed.

use std::collections::HashMap;
use std::sync::Arc;

use crate::array::{Array, Data};
use crate::error::{Error, Result, Span};
use crate::frontend::{
    ControlStrictness, DefaultArg, DepthSign, DfnResult, FirstDisclose, IndexForm, LookupLeft,
    NestedModel, Partition, Rules, Segment, SourceParts,
};
use crate::ir::{Branch, Control, ExplicitDef, Expr, Scope};
use crate::verb::{
    AtopForm, AxisSpec, BitDyad, BitMonad, BoolDyad, DyadOp, Enclose, MonadOp, OpDef,
    Operand, Power, Prim, ScalarDyad, ScalarMonad, Verb, WindowKind,
    RANK_INF,
};

/// Parse an APL program (sentences separated by newlines or `⋄`) into IR
/// statements. `d` is the dialect, resolved: `⎕IO`, `⎕CT` and the
/// lineage settings the parser reads.
pub fn parse(src: &SourceParts, d: Rules) -> Result<Vec<Expr>> {
    let sentences = lex(src, d)?;
    let mut verbs: HashMap<String, Verb> = HashMap::new();
    for name in fixed_names(&sentences, d) {
        verbs.entry(name.clone()).or_insert_with(|| Verb::Named(name.clone()));
    }
    let mut stmts = Vec::with_capacity(sentences.len());
    let mut i = 0usize;
    while i < sentences.len() {
        if matches!(sentences[i].first().map(|t| &t.kind), Some(Tok::Del)) {
            let stmt = parse_tradfn(&sentences, &mut i, d, &mut verbs, &src.display)?;
            stmts.push(stmt);
            continue;
        }
        // A control structure standing on its own, outside any definition:
        // its sentences are the lines of a body with nothing around them.
        if let Some(Tok::Control(w)) = sentences[i].first().map(|t| &t.kind)
            && matches!(*w, "If" | "While" | "Repeat" | "For" | "Select")
        {
            let end = control_block_end(&sentences, i).unwrap_or(sentences.len());
            let mut items = Vec::with_capacity(end - i);
            for line in &sentences[i..end] {
                let mut label = None;
                items.push(to_item(line.clone(), d, &mut verbs, &mut label)?);
            }
            let mut cursor = AplCursor { items: &items, at: 0, d, loops: 0 };
            stmts.push(parse_apl_control(&mut cursor)?);
            i = end;
            continue;
        }
        // `test →→ … ←→ … ←←`: the markers may end a line, so the whole
        // conditional is taken before the sentence parser sees any of it.
        if sentences[i].iter().any(|t| matches!(t.kind, Tok::CondThen)) {
            let stmt = parse_conditional(&sentences, &mut i, d, &mut verbs)?;
            stmts.push(stmt);
            continue;
        }
        let mut sentence = sentences[i].clone();
        i += 1;
        // `⎕FX` defines a function where it stands; the sentence keeps only
        // the name it answers with.
        while let Some(at) = outermost_fx(&sentence) {
            let mut end = at + 1;
            while end < sentence.len() && matches!(sentence[end].kind, Tok::Value(_)) {
                end += 1;
            }
            // Everything to a `⎕FX`'s right is its argument. A token past
            // the run of literals that is not a closing bracket is part of
            // that argument and is not literal text, so the definition is
            // one libjay cannot fix before the program runs — a gap, not a
            // definition with a short body.
            if let Some(t) = sentence.get(end)
                && !matches!(t.kind, Tok::RParen | Tok::RBracket | Tok::RBrace | Tok::Semi)
            {
                return Err(Error::not_yet(
                    "⎕FX on a definition that is not literal text in the program",
                    t.span,
                ));
            }
            let (def, name) = fix_definition(&sentence[at..end], d, &mut verbs)?;
            stmts.push(def);
            let span = Span::merge(sentence[at].span, sentence[end - 1].span);
            sentence.splice(at..end, [Token { kind: Tok::Value(name), span }]);
        }
        if let Some(stmt) = parse_statement(sentence, d, &mut verbs, false)? {
            stmts.push(stmt);
        }
    }
    Ok(stmts)
}

/// `test →→ body ←→ otherwise ←←`, from the sentence the `→→` stands in to
/// the one the `←←` closes.
///
/// The three markers each break a statement, and so does the end of a
/// line, so a clause may hold several statements written either way. The
/// `←→` clause is optional; without it a test that fails yields nothing.
fn parse_conditional(
    sentences: &[Vec<Token>],
    i: &mut usize,
    d: Rules,
    verbs: &mut HashMap<String, Verb>,
) -> Result<Expr> {
    let open = sentences[*i].first().map_or(Span::new(0, 0), |t| t.span);
    let mut parts: [Vec<Vec<Token>>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut phase = 0usize;
    let mut closed = false;
    let mut last = open;
    while *i < sentences.len() && !closed {
        let line = &sentences[*i];
        *i += 1;
        let mut cur: Vec<Token> = Vec::new();
        let mut depth = 0i32;
        for t in line {
            last = t.span;
            match t.kind {
                Tok::LParen | Tok::LBracket | Tok::LBrace => depth += 1,
                Tok::RParen | Tok::RBracket | Tok::RBrace => depth -= 1,
                Tok::CondThen | Tok::CondElse | Tok::CondEnd if depth == 0 => {
                    parts[phase].push(std::mem::take(&mut cur));
                    match t.kind {
                        Tok::CondThen => phase = 1,
                        Tok::CondElse => phase = 2,
                        _ => {
                            closed = true;
                            continue;
                        }
                    }
                    continue;
                }
                _ => {}
            }
            if closed {
                return Err(Error::parse("←← ends the conditional", t.span));
            }
            cur.push(t.clone());
        }
        parts[phase].push(cur);
    }
    if !closed {
        return Err(Error::parse("this conditional has no closing ←←", open));
    }
    let span = Span::merge(open, last);
    let mut block = |toks: &Vec<Vec<Token>>| -> Result<Vec<Expr>> {
        let mut out = Vec::new();
        for stmt in toks {
            if stmt.is_empty() {
                continue;
            }
            if let Some(e) = parse_statement(stmt.clone(), d, verbs, true)? {
                out.push(e);
            }
        }
        Ok(out)
    };
    let test = block(&parts[0])?;
    if test.is_empty() {
        return Err(Error::parse("→→ needs a test on its left", span));
    }
    let body = block(&parts[1])?;
    let otherwise = block(&parts[2])?;
    Ok(Expr::Control(Box::new(Control::Cond { test, body, otherwise }), span))
}

/// The functions the program fixes anywhere in it, by name.
///
/// APL settles a name's class when the line runs; libjay compiles first, so
/// a definition whose body calls a function fixed AFTER it would have
/// nothing to call. The names every `∇` and `⎕FX` in the program gives a
/// function are therefore collected before anything is parsed, and each
/// stands as a verb resolved when it is applied — the definition it names
/// has been fixed by then. A header this cannot read is skipped: parsing
/// the definition itself reports the fault.
fn fixed_names(sentences: &[Vec<Token>], d: Rules) -> Vec<String> {
    let mut out = Vec::new();
    for line in sentences {
        let head = match line.first().map(|t| &t.kind) {
            Some(Tok::Del) => line[1..].to_vec(),
            _ => {
                let Some(at) = line.iter().position(|t| matches!(t.kind, Tok::QuadFx)) else {
                    continue;
                };
                let Some(Tok::Value(a)) = line.get(at + 1).map(|t| &t.kind) else {
                    continue;
                };
                let Data::Char(cs) = &a.data else { continue };
                if a.rank() > 1 {
                    continue;
                }
                let text: String = cs.iter().collect();
                let src = SourceParts::from_parts(&[&text], &[]);
                match lex(&src, d) {
                    Ok(mut lexed) if lexed.len() == 1 => lexed.pop().unwrap_or_default(),
                    _ => continue,
                }
            }
        };
        let Some(first) = head.first() else { continue };
        if let Ok(Header { name, .. }) = parse_header(&head, first.span) {
            out.push(name);
        }
    }
    out
}

/// The sentence after the one that closes the control structure opening at
/// `at`. None where nothing closes it, which the block parser reports.
fn control_block_end(sentences: &[Vec<Token>], at: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (k, line) in sentences.iter().enumerate().skip(at) {
        let Some(Tok::Control(w)) = line.first().map(|t| &t.kind) else { continue };
        match *w {
            "If" | "While" | "Repeat" | "For" | "Select" => depth += 1,
            "EndIf" | "EndWhile" | "EndFor" | "EndSelect" | "Until" | "End" => {
                depth -= 1;
                if depth == 0 {
                    return Some(k + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Where a `⎕FX` stands in a sentence of its own rather than inside a dfn's
/// braces. One inside a body belongs to that body, which is compiled when
/// the definition is, so it is left where it is and named there as a gap.
fn outermost_fx(sentence: &[Token]) -> Option<usize> {
    let mut depth = 0usize;
    for (i, t) in sentence.iter().enumerate() {
        match t.kind {
            Tok::LBrace => depth += 1,
            Tok::RBrace => depth = depth.saturating_sub(1),
            Tok::QuadFx if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// `⎕FX`: fix a definition from its text. `toks` opens with the `⎕FX`
/// itself and runs to the end of the sentence; the lines are a vector of
/// character vectors, one per line of the definition, the first of them the
/// header. The answer is the name the definition gives the function.
///
/// libjay compiles before it runs, so the lines have to be text the
/// compiler can read: a definition assembled while the program runs is a
/// promise, not a refusal. Dyalog's own answer for a definition it cannot
/// fix is the number of the offending line; libjay reports the fault
/// instead, pointing at the line that carries it.
fn fix_definition(
    toks: &[Token],
    d: Rules,
    verbs: &mut HashMap<String, Verb>,
) -> Result<(Expr, Array)> {
    let fx = toks[0].span;
    if toks.len() == 1 {
        return Err(Error::not_yet(
            "⎕FX on a definition that is not literal text in the program",
            fx,
        ));
    }
    let span = Span::merge(fx, toks[toks.len() - 1].span);
    let mut lines: Vec<(Vec<Token>, Span)> = Vec::new();
    // The text `⎕FX` was given, line by line, kept for `⎕CR` to hand back.
    let mut source: Vec<String> = Vec::new();
    for t in &toks[1..] {
        let text = match &t.kind {
            Tok::Value(a) if a.rank() <= 1 => match &a.data {
                Data::Char(cs) => Some(cs.iter().collect::<String>()),
                _ => None,
            },
            _ => None,
        };
        let Some(text) = text else {
            return Err(Error::not_yet(
                "⎕FX on a definition that is not literal text in the program",
                t.span,
            ));
        };
        source.push(text.trim_end().to_string());
        let src = SourceParts::from_parts(&[&text], &[]);
        let mut lexed = lex(&src, d)?;
        if lexed.len() > 1 {
            return Err(Error::not_yet("a ⋄ inside a ⎕FX line", t.span));
        }
        // Every span inside the line indexes the line's own text, so it is
        // brought back to the literal the line came from.
        let mut line = lexed.pop().unwrap_or_default();
        for tok in &mut line {
            tok.span = t.span;
        }
        lines.push((line, t.span));
    }
    let (head, head_span) = lines.remove(0);
    if head.is_empty() {
        return Err(Error::parse("⎕FX starts with the definition's header", head_span));
    }
    let body: Vec<Vec<Token>> = lines.into_iter().map(|(l, _)| l).collect();
    let def = build_tradfn(&head, &body, span, d, verbs, source)?;
    let Expr::VerbDef { name, .. } = &def else {
        return Err(Error::internal("⎕FX did not build a definition"));
    };
    let answer = Array::from_chars(name.chars().collect());
    Ok((def, answer))
}

/// One sentence, with every name known to be a function already a function.
/// None where the sentence held nothing but blanks and a comment.
fn parse_statement(
    sentence: Vec<Token>,
    d: Rules,
    verbs: &mut HashMap<String, Verb>,
    // True where `verbs` is the ENCLOSING program's map rather than this
    // definition's own, so that a function this sentence names must not be
    // registered in it. A dfn body parses against a clone of its own and
    // registers there, which is what lets `G←{⍵×2} ⋄ G ⍵` read `G` as the
    // function it named.
    shared_verbs: bool,
) -> Result<Option<Expr>> {
    // A `⎕FX` that reaches here is inside another definition's body: the
    // one that stands on its own was fixed and rewritten away before the
    // sentence was parsed.
    if let Some(t) = sentence.iter().find(|t| matches!(t.kind, Tok::QuadFx)) {
        return Err(Error::not_yet("⎕FX inside another definition", t.span));
    }
    let sentence = substitute_verbs(sentence, verbs);
    let sentence = fold_dfns(sentence, d, verbs)?;
    // `F←{⍵×2}` names a function: the sentence does no work at run time, and
    // later sentences read `F` as the function itself.
    if let [name, assign, func] = &sentence[..]
        && let (Tok::Name(n), Tok::Assign) = (&name.kind, &assign.kind)
    {
        // A dfn that mentions `⍺⍺` or `⍵⍵` is an operator; naming it
        // keeps it one, waiting for the operands.
        let named = match &func.kind {
            Tok::Func(v) => Some(v.clone()),
            Tok::UserOp { def, omega } => Some(unapplied_op(def.clone(), *omega)),
            _ => None,
        };
        if let Some(v) = named {
            let span = Span::merge(name.span, func.span);
            if !shared_verbs {
                verbs.insert(n.clone(), v.clone());
            }
            return Ok(Some(Expr::VerbDef { name: n.clone(), verb: v, span }));
        }
    }
    let toks = fold_axes(fold_operators(unwrap_lone_operators(sentence), d)?, d)?;
    if toks.is_empty() {
        return Ok(None);
    }
    // `F←+/` and `F←+/÷≢` name a function, derived or tacit. Like the dfn
    // above, the sentence does no work at run time and later sentences read
    // the name as the function itself.
    if let [name, assign, rest @ ..] = &toks[..]
        && let (Tok::Name(n), Tok::Assign) = (&name.kind, &assign.kind)
        && let Some(v) = tine_run(rest, d)?
    {
        let span = Span::merge(name.span, toks[toks.len() - 1].span);
        if !shared_verbs {
            verbs.insert(n.clone(), v.clone());
        }
        return Ok(Some(Expr::VerbDef { name: n.clone(), verb: v, span }));
    }
    // A control word that opens a structure is taken before the sentence
    // is; anything left here continues or closes one that never opened.
    if let Some(t) = toks.iter().find(|t| matches!(t.kind, Tok::Control(_))) {
        let Tok::Control(w) = t.kind else { unreachable!() };
        return Err(Error::parse(format!(":{w} has no matching opening word"), t.span));
    }
    if let Some(t) = toks.iter().find(|t| matches!(t.kind, Tok::Arrow)) {
        return Err(Error::parse(
            "→ branches, and only a line of a ∇ definition may begin with it",
            t.span,
        ));
    }
    let hint = Span::merge(toks[0].span, toks[toks.len() - 1].span);
    parse_range(&toks, 0, toks.len(), hint, d).map(Some)
}

/// Replace every name the program has given a function by that function,
/// except where the name is the target of an assignment.
fn substitute_verbs(mut toks: Vec<Token>, verbs: &HashMap<String, Verb>) -> Vec<Token> {
    for i in 0..toks.len() {
        let Tok::Name(n) = &toks[i].kind else { continue };
        if matches!(toks.get(i + 1).map(|t| &t.kind), Some(Tok::Assign)) {
            continue;
        }
        if let Some(v) = verbs.get(n) {
            // A niladic definition is called by naming it, so its name
            // stands where a value does, not where a function does.
            toks[i].kind = match as_user_op(v) {
                Some((def, omega)) => Tok::UserOp { def, omega },
                None if is_niladic(v) => Tok::Niladic(v.clone()),
                None => Tok::Func(v.clone()),
            };
        }
    }
    toks
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpGlyph {
    /// `/` — reduce along the last axis.
    Slash,
    /// `⌿` — reduce along the leading axis.
    SlashBar,
    /// `\` — scan along the last axis.
    Backslash,
    /// `⍀` — scan along the leading axis.
    BackslashBar,
    /// `⍤` — rank.
    Rank,
    /// `⍨` — commute.
    Commute,
    /// `⍣` — power.
    Power,
    /// `∘.` — outer product; unlike the rest, its operand is on its right.
    JotDot,
    /// `⍥` over: `f⍥g` prepares both arguments with g, then applies f.
    Over,
    /// `⍢` under (Dyalog): `f⍢g` is `g⁻¹ (g x) f (g y)` — over, undone.
    Under,
    /// `⌺` stencil (Dyalog): f over the window centred on each cell.
    Stencil,
    /// `∘` on its own — Dyalog's `f∘g`, which libjay does not have yet.
    Jot,
    /// `¨` — each.
    Each,
    /// `⍛` before: `f⍛g` prepares the LEFT argument with f.
    Before,
    /// `⌸` key (Dyalog): each distinct major cell with what shares it.
    Key,
    /// `@` at (Dyalog): the selected positions changed, the rest left alone.
    At,
    /// `.` between two functions: the inner product, `+.×` above all. The
    /// operand on its right is a function too, as `∘.`'s is.
    Dot,
    /// `⍠` variant: one dialect knob overridden for this application.
    Variant,
}

impl OpGlyph {
    fn glyph(self) -> char {
        match self {
            OpGlyph::Slash => '/',
            OpGlyph::SlashBar => '⌿',
            OpGlyph::Backslash => '\\',
            OpGlyph::BackslashBar => '⍀',
            OpGlyph::Rank => '⍤',
            OpGlyph::Commute => '⍨',
            OpGlyph::Power => '⍣',
            OpGlyph::JotDot | OpGlyph::Jot => '∘',
            OpGlyph::Over => '⍥',
            OpGlyph::Under => '⍢',
            OpGlyph::Stencil => '⌺',
            OpGlyph::Each => '¨',
            OpGlyph::Before => '⍛',
            OpGlyph::Key => '⌸',
            OpGlyph::At => '@',
            OpGlyph::Dot => '.',
            OpGlyph::Variant => '⍠',
        }
    }
}

#[derive(Clone, Debug)]
enum Tok {
    /// A literal array: a character array, or a value built by the lexer.
    Value(Array),
    /// A run of adjacent numeric literals. Apart from `Value` because
    /// vector notation spreads its numbers into separate items while a
    /// string contributes one.
    Nums(Array),
    /// An interpolation hole, by parameter index.
    Param(usize),
    Name(String),
    /// A primitive or derived function.
    Func(Verb),
    /// An operator glyph; gone after `fold_operators`.
    Op(OpGlyph),
    Assign,
    /// `⎕` or `⍞` standing alone: input where a value belongs, output
    /// where `←` follows it. `quote` is the `⍞` form.
    Quad { quote: bool },
    LParen,
    RParen,
    LBracket,
    RBracket,
    /// The separator between index slots inside `[ ]`, and the one between
    /// a `∇`-definition header's name and its locals.
    Semi,
    /// `{` and `}`, a dfn's brackets; gone after `fold_dfns`.
    LBrace,
    RBrace,
    /// A statement break inside a dfn's braces, where `⋄` and a line break
    /// do not end the sentence the dfn belongs to.
    Separator,
    /// The `:` of a dfn's guard, `cond:expr`.
    Colon,
    /// `→` — the branch, which only a `∇` definition's line may open with.
    Arrow,
    /// `→→`, `←→` and `←←`: the three markers of a conditional. The test
    /// stands before the first, the two clauses between them, and the last
    /// closes the whole thing.
    CondThen,
    CondElse,
    CondEnd,
    /// A niladic `∇` definition named where a value belongs: naming it is
    /// what calls it.
    Niladic(Verb),
    /// A dfn that mentions `⍺⍺` or `⍵⍵`: an operator, waiting for its
    /// operands. `omega` says whether it wants one on its right too.
    UserOp { def: Arc<OpDef>, omega: bool },
    /// `∇`: a definition's bracket outside a dfn, a self-reference inside.
    Del,
    /// A control word, `:If` and its family, without the colon.
    Control(&'static str),
    /// `⎕FX`, which fixes a definition from its text. It is not a function
    /// the sentence parser can carry: the definition is built while the
    /// program is compiled, so the whole sentence is rewritten around it.
    QuadFx,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Tok,
    span: Span,
}

/// True for tokens that can end an operand (and so make the function on
/// their right dyadic).
fn is_operand_end(k: &Tok) -> bool {
    matches!(
        k,
        Tok::Value(_)
            | Tok::Nums(_)
            | Tok::Param(_)
            | Tok::Name(_)
            | Tok::Niladic(_)
            // `⍞` and `⎕` are values where nothing assigns to them, so an
            // operand ends at one: `⍞,⍞` is a dyad and `⍞[1]` indexes the
            // line. The `⍞←` form never reaches here — the parser reads
            // the assignment arrow before it looks left.
            | Tok::Quad { .. }
            | Tok::RParen
            | Tok::RBracket
    )
}

/// A user-written operator with no operands yet: the two names stand in
/// for them until it is applied, which is how a NAMED operator survives
/// from the sentence that defined it to the one that uses it.
fn unapplied_op(def: Arc<OpDef>, omega: bool) -> Verb {
    let named = |n: &str| Operand::Func(Box::new(Verb::Named(n.to_string())));
    Verb::UserDerived {
        def,
        alpha: named("⍺⍺"),
        omega: omega.then(|| named("⍵⍵")),
    }
}

/// The definition and right-operand appetite of an operator that is still
/// waiting for its operands.
fn as_user_op(v: &Verb) -> Option<(Arc<OpDef>, bool)> {
    let Verb::UserDerived { def, alpha, omega } = v else { return None };
    // A left operand still standing under its own name is what marks an
    // operator that has not been applied to anything yet.
    let Operand::Func(f) = alpha else { return None };
    match &**f {
        Verb::Named(n) if n == "⍺⍺" => Some((def.clone(), omega.is_some())),
        _ => None,
    }
}

/// True for a `∇` definition that takes no argument.
fn is_niladic(v: &Verb) -> bool {
    matches!(v, Verb::Explicit(d) if d.left.is_none() && d.right == crate::ir::NILADIC)
}

/// The array a literal token holds.
fn literal(k: &Tok) -> Option<&Array> {
    match k {
        Tok::Value(a) | Tok::Nums(a) => Some(a),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Primitive table
// ---------------------------------------------------------------------------

/// The primitive for a function glyph, under the dialect `d`: `⎕IO`
/// parameterises the counting primitives, and the lineage settings decide
/// the glyphs the APL lines read differently (`↑ ⊃ ⊂ ⌷`).
///
/// A glyph whose meaning this dialect does not have is `None` here, as an
/// unknown glyph is — but a dialect that reads one differently is refused
/// by [`Dialect::rules`](crate::Dialect::rules) before a program reaches
/// this table, so `None` here is only ever the unknown glyph.
fn prim_for(ch: char, d: Rules) -> Option<Prim> {
    use DyadOp as D;
    use MonadOp as M;
    use ScalarDyad as SD;
    use ScalarMonad as SM;
    let origin = d.origin;
    let p = match ch {
        '+' => Prim {
            name: "+",
            monad: M::Scalar(SM::Conj),
            dyad: D::Scalar(SD::Add),
            ranks: [0, 0, 0],
        },
        '-' => {
            Prim { name: "-", monad: M::Scalar(SM::Neg), dyad: D::Scalar(SD::Sub), ranks: [0, 0, 0] }
        }
        '×' => Prim {
            name: "×",
            monad: M::Scalar(SM::Signum),
            dyad: D::Scalar(SD::Mul),
            ranks: [0, 0, 0],
        },
        '÷' => Prim {
            name: "÷",
            monad: M::Scalar(SM::Recip),
            dyad: D::Scalar(SD::DivApl),
            ranks: [0, 0, 0],
        },
        '⌈' => Prim {
            name: "⌈",
            monad: M::Scalar(SM::Ceil),
            dyad: D::Scalar(SD::Max),
            ranks: [0, 0, 0],
        },
        '⌊' => Prim {
            name: "⌊",
            monad: M::Scalar(SM::Floor),
            dyad: D::Scalar(SD::Min),
            ranks: [0, 0, 0],
        },
        '*' => {
            Prim { name: "*", monad: M::Scalar(SM::Exp), dyad: D::Scalar(SD::Pow), ranks: [0, 0, 0] }
        }
        '|' => Prim {
            name: "|",
            monad: M::Scalar(SM::Abs),
            dyad: D::Scalar(SD::Residue),
            ranks: [0, 0, 0],
        },
        '=' => Prim { name: "=", monad: M::None, dyad: D::Scalar(SD::Eq), ranks: [0, 0, 0] },
        '≠' => Prim {
            name: "≠",
            monad: M::NubSieve,
            dyad: D::Scalar(SD::Ne),
            ranks: [RANK_INF, 0, 0],
        },
        '<' => Prim { name: "<", monad: M::None, dyad: D::Scalar(SD::Lt), ranks: [0, 0, 0] },
        '≤' => Prim { name: "≤", monad: M::None, dyad: D::Scalar(SD::Le), ranks: [0, 0, 0] },
        '>' => Prim { name: ">", monad: M::None, dyad: D::Scalar(SD::Gt), ranks: [0, 0, 0] },
        '≥' => Prim { name: "≥", monad: M::None, dyad: D::Scalar(SD::Ge), ranks: [0, 0, 0] },
        '⍴' => Prim {
            name: "⍴",
            monad: M::ShapeOf,
            dyad: D::Reshape,
            ranks: [RANK_INF, 1, RANK_INF],
        },
        '⍳' => Prim {
            name: "⍳",
            monad: M::IotaApl { origin },
            dyad: D::IndexOf { origin, major_cells: d.lookup_left == LookupLeft::MajorCells },
            // The monad takes the whole argument: a vector of lengths asks
            // for a nested index array, which is a refusal, not a frame of
            // one index generator per atom.
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '∊' => Prim {
            name: "∊",
            monad: M::Enlist,
            dyad: D::MemberApl,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '∪' => Prim {
            name: "∪",
            monad: M::Nub,
            dyad: D::Union,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '∩' => Prim {
            name: "∩",
            monad: M::None,
            dyad: D::Intersect,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        // `^` is the ASCII spelling of `∧`, which is what a program typed
        // on a keyboard without an APL layout holds. It reads as the same
        // primitive and reports itself under the glyph.
        '∧' | '^' => Prim { name: "∧", monad: M::None, dyad: D::Scalar(SD::Lcm), ranks: [0, 0, 0] },
        '∨' => Prim { name: "∨", monad: M::None, dyad: D::Scalar(SD::Gcd), ranks: [0, 0, 0] },
        '⍱' => Prim {
            name: "⍱",
            monad: M::None,
            dyad: D::Boolean(BoolDyad::Nor),
            ranks: [0, 0, 0],
        },
        '⍲' => Prim {
            name: "⍲",
            monad: M::None,
            dyad: D::Boolean(BoolDyad::Nand),
            ranks: [0, 0, 0],
        },
        '⍟' => Prim {
            name: "⍟",
            monad: M::Scalar(SM::Ln),
            dyad: D::Scalar(SD::Log),
            ranks: [0, 0, 0],
        },
        '~' => Prim {
            name: "~",
            monad: M::Scalar(SM::Not),
            dyad: D::Less,
            ranks: [0, RANK_INF, RANK_INF],
        },
        '≡' => Prim {
            name: "≡",
            monad: M::Depth { signed: d.depth_sign == DepthSign::Signed },
            dyad: D::Match,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '⍋' => Prim {
            name: "⍋",
            monad: M::GradeUp { origin },
            dyad: D::CollateGrade { down: false, origin },
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '⍒' => Prim {
            name: "⍒",
            monad: M::GradeDown { origin },
            dyad: D::CollateGrade { down: true, origin },
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        // `⊖` works on the leading axis; `⌽` is the same primitive applied to
        // rows, which `verb_for` wraps in the rank that does it. The DYAD
        // takes its argument whole in both spellings: APL's rotate reads
        // one amount per vector along the axis it moves, and picking the
        // axis is the primitive's own job.
        '⊖' | '⌽' => Prim {
            name: if ch == '⊖' { "⊖" } else { "⌽" },
            monad: M::Reverse,
            dyad: D::RotateApl { last: ch == '⌽' },
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '⍪' => Prim {
            name: "⍪",
            monad: M::TableOf,
            dyad: D::AppendLeading,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '!' => Prim {
            name: "!",
            monad: M::Scalar(SM::Factorial),
            dyad: D::Scalar(SD::Binomial),
            ranks: [0, 0, 0],
        },
        '⍕' => Prim {
            name: "⍕",
            monad: M::Format,
            dyad: D::FormatSpec,
            ranks: [RANK_INF, 1, RANK_INF],
        },
        // `⊥` and `⊤` have no monadic meaning in APL; J spells those `#.`
        // and `#:`. Both take their arguments whole: `⊥` is the inner
        // product `+.×` over x's last axis and y's leading one, and `⊤`
        // makes x's leading axis the digits, so the result of either is
        // shaped by what is left of both arguments.
        '⊥' => Prim {
            name: "⊥",
            monad: M::None,
            dyad: D::DecodeApl,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '⊤' => Prim {
            name: "⊤",
            monad: M::None,
            dyad: D::EncodeApl,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '⍉' => Prim {
            name: "⍉",
            monad: M::TransposeAxes,
            dyad: D::TransposeApl,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        // `↑` and `⊃` are the lineages' clearest divergence, so the
        // dialect names which reading applies; the dyads agree.
        '↑' => Prim {
            name: "↑",
            monad: match d.first_disclose {
                FirstDisclose::UpIsFirst => M::First,
                FirstDisclose::UpIsMix => M::Open,
            },
            // Mix is the rank-0 application: every item opened, and the
            // results framed into one array. First sees the whole array.
            ranks: match d.first_disclose {
                FirstDisclose::UpIsFirst => [RANK_INF, 1, RANK_INF],
                FirstDisclose::UpIsMix => [0, 1, RANK_INF],
            },
            dyad: D::Take,
        },
        '⊂' => Prim {
            name: "⊂",
            // A floating model cannot nest a simple scalar, so `⊂3` is 3;
            // a grounded one encloses it like anything else.
            monad: match d.nested_model {
                NestedModel::Floating => M::Enclose(Enclose::ExceptSimpleScalar),
                NestedModel::Grounded => return None,
            },
            // The flag reading is the partition; the count reading is
            // Dyalog's partitioned enclose, a different function.
            dyad: match d.partition {
                Partition::Flags => D::PartitionEnclose,
                Partition::Counts => D::PartitionCounts,
            },
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        // Dyalog's `⊆`: nest monadically, and the partition GNU APL spells
        // `⊂` dyadically. GNU APL has neither, so both follow Dyalog.
        '⊆' => Prim {
            name: "⊆",
            monad: M::Nest,
            dyad: D::PartitionEnclose,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        // `⍸` counts from ⎕IO; its dyad is the interval index, which GNU
        // APL answers with the count of bounds below the value plus ⎕IO-1.
        // The Dyalog reading searches the left argument's MAJOR CELLS, so
        // the whole of it reaches the verb rather than one vector at a time.
        '⍸' => Prim {
            name: "⍸",
            monad: M::Indices { origin, boxed_coords: true },
            dyad: D::IntervalIndex { offset: origin - 1, closed: true },
            ranks: match d.lookup_left {
                LookupLeft::AnyRank => [RANK_INF, 1, RANK_INF],
                LookupLeft::MajorCells => [RANK_INF, RANK_INF, RANK_INF],
            },
        },
        // `⌷` indexes with one scalar per axis and has no monadic case in
        // the APL2 reading; the other reads index vectors instead.
        '⌷' => Prim {
            name: "⌷",
            // Monadically `⌷` materialises, which for an array libjay
            // already holds is the array itself, in both readings.
            monad: M::Same,
            dyad: D::Squad { origin, leading: d.index_form == IndexForm::AxisVectors },
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '?' => Prim {
            name: "?",
            monad: M::Roll { origin, fixed: false, float_at_zero: false },
            dyad: D::Deal { origin, fixed: false },
            ranks: [RANK_INF, 0, 0],
        },
        // APL's ⌹ sees the whole argument: a rank of 3 or more is a rank
        // error rather than a run over 2-cells. J's `%.` has rank 2 and
        // does run over them, which is where the two lines differ.
        '⌹' => Prim {
            name: "⌹",
            monad: M::MatrixInverse,
            dyad: D::MatrixDivide,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '⊃' => Prim {
            name: "⊃",
            monad: match d.first_disclose {
                FirstDisclose::UpIsFirst => M::Open,
                FirstDisclose::UpIsMix => M::First,
            },
            dyad: D::Pick { origin },
            ranks: match d.first_disclose {
                FirstDisclose::UpIsFirst => [0, RANK_INF, RANK_INF],
                FirstDisclose::UpIsMix => [RANK_INF, RANK_INF, RANK_INF],
            },
        },
        '↓' => Prim {
            name: "↓",
            monad: M::Split,
            dyad: D::Drop,
            ranks: [RANK_INF, 1, RANK_INF],
        },
        ',' => Prim {
            name: ",",
            monad: M::Ravel,
            dyad: D::AppendLast,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '≢' => Prim {
            name: "≢",
            monad: M::Tally,
            dyad: D::NotMatch,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '⊢' => Prim {
            name: "⊢",
            monad: M::Same,
            dyad: D::Right,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '⊣' => Prim {
            name: "⊣",
            monad: M::Same,
            dyad: D::Left,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '○' => Prim {
            name: "○",
            monad: M::Scalar(SM::Pi),
            dyad: D::Scalar(SD::Circle),
            ranks: [0, 0, 0],
        },
        '⍷' => Prim {
            name: "⍷",
            monad: M::None,
            dyad: D::FindSeq,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '⍎' => Prim {
            name: "⍎",
            monad: M::Execute { apl: true },
            dyad: D::None,
            ranks: [1, RANK_INF, RANK_INF],
        },
        _ => return None,
    };
    Some(p)
}

/// The function a glyph denotes. Every glyph but `⌽` is a bare primitive;
/// `⌽` is `⊖` applied to rows, so it carries the rank that does that: cells
/// of rank 1 on the right, atoms on the left.
/// The bit-wise logical function `⊤` spells with the glyph after it, and
/// how far past the `⊤` that glyph ends.
///
/// GNU APL reads `⊤` followed by one of six logical glyphs as one
/// function: `12 ⊤∧ 10` is 8. None of the six has a monadic meaning of its
/// own, so the pair is never ambiguous with `⊤` applied to a monad, and
/// the interpreter allows blanks between the two glyphs.
fn bitwise_prim(text: &str, after_top: usize) -> Option<(Prim, usize)> {
    let rest = text[after_top..].trim_start_matches([' ', '\t', '\r']);
    let gap = text.len() - after_top - rest.len();
    let ch = rest.chars().next()?;
    let (name, monad, dyad) = match ch {
        '∧' | '^' => ("⊤∧", MonadOp::Bitwise(BitMonad::Integer), BitDyad::And),
        '∨' => ("⊤∨", MonadOp::Bitwise(BitMonad::Integer), BitDyad::Or),
        '⍲' => ("⊤⍲", MonadOp::None, BitDyad::Nand),
        '⍱' => ("⊤⍱", MonadOp::Bitwise(BitMonad::Not), BitDyad::Nor),
        '=' => ("⊤=", MonadOp::None, BitDyad::Eq),
        '≠' => ("⊤≠", MonadOp::None, BitDyad::Ne),
        _ => return None,
    };
    let end = after_top + gap + ch.len_utf8();
    Some((Prim { name, monad, dyad: DyadOp::Bitwise(dyad), ranks: [0, 0, 0] }, end))
}

fn verb_for(ch: char, d: Rules) -> Option<Verb> {
    let p = prim_for(ch, d)?;
    // Monadic `⌽` is `⊖` on rows, which the rank operator supplies. The
    // dyad picks its own axis, so it keeps its whole arguments.
    if ch == '⌽' {
        return Some(Verb::Rank(Box::new(Verb::Prim(p)), [1, RANK_INF, RANK_INF].into()));
    }
    Some(Verb::Prim(p))
}

/// A `⎕`-name: the pure ones libjay answers, and a clear refusal for the
/// ones that would have to reach outside the sandbox.
///
/// Most of them are constants the compiler folds in before the program
/// runs. `⎕IO` and `⎕CT` are the dialect's own settings, readable but not
/// assignable, for that reason. `⎕PP` and `⎕RL` are the two a program may
/// change while it runs, so they become names the run holds rather than
/// values fixed here — [`crate::sysvar`] seeds and applies them.
fn quad_name(name: &str, d: Rules, span: Span) -> Result<Tok> {
    let chars = |s: &str| Tok::Value(Array::from_chars(s.chars().collect()));
    let func = |name, monad, dyad| {
        Tok::Func(Verb::Prim(Prim {
            name,
            monad,
            dyad,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        }))
    };
    Ok(match name {
        "A" => chars("ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
        "D" => chars("0123456789"),
        "AV" => Tok::Value(Array::from_chars(crate::sysvar::atomic_vector())),
        "IO" => Tok::Value(Array::scalar_i64(d.origin)),
        "CT" => Tok::Value(Array::scalar_f64(d.ct)),
        // Read and set while the program runs: they live in the run's own
        // name table, under the spelling written here.
        "PP" | "RL" => Tok::Name(format!("⎕{name}")),
        // The latent expression is what a session runs when it loads a
        // saved workspace. libjay compiles and runs a string and never
        // loads a workspace, so its latent expression is empty and stays
        // empty; reading it is honest, setting it is refused below.
        "LX" => chars(""),
        // The last error's type and message. libjay's APL has no
        // construct that catches an error and carries on, so an error
        // always ends the program: nothing can be read after one, and
        // these keep the values that mean "no error yet" for every
        // program libjay can run.
        "ET" => Tok::Value(Array::from_i64(vec![0, 0])),
        "EM" => Tok::Value(Array::new(vec![3, 0], Data::Char(Vec::new().into()))),
        "FX" => Tok::QuadFx,
        "CC" => func("⎕CC", MonadOp::CharClass, DyadOp::None),
        "NC" => func("⎕NC", MonadOp::NameClass, DyadOp::None),
        "CR" => func("⎕CR", MonadOp::CharRep, DyadOp::CharRep),
        "UCS" => func("⎕UCS", MonadOp::Unicode { pass_chars: false }, DyadOp::None),
        // The ones that would read a clock, a workspace or a file. The
        // sandbox is libjay's own policy, not a queue position, so this
        // is a refusal and not a promise. `⎕SVR` retracts the offer of a
        // shared variable, which is the same surface as `⎕SVO`/`⎕SVQ`:
        // libjay shares no variable with anything, so the whole surface
        // is closed rather than answering that nothing is shared.
        "TS" | "AI" | "TC" | "WA" | "SI" | "LC" | "NL" | "EX" | "FIO" | "NA" | "SH" | "CMD"
        | "MAP" | "SVO" | "SVQ" | "SVR" | "SVC" | "SVE" | "SVS" | "TZ" | "DL" => {
            Err(Error::sandbox(format!("⎕{name} reads outside the program"), span))?
        }
        // A table of one interpreter's own build: how many cores it was
        // configured for, how big its hash table is, what its input line
        // length is. Those are not facts about APL, and libjay has no
        // counterpart to report in their place.
        "SYL" => Err(Error::language(
            "⎕SYL reports one interpreter's build limits, which is not \
             something another implementation can answer",
            span,
        ))?,
        // A page width would have to be a page. libjay's display writes
        // an array in full and never folds a long line, so there is no
        // width to set.
        "PW" => Err(Error::not_yet(
            "⎕PW: libjay's display does not fold a value to a page width",
            span,
        ))?,
        other => Err(Error::not_yet(format!("the system name ⎕{other}"), span))?,
    })
}

/// A glyph the language has and libjay does not, with the name to report
/// it under. These are queue positions, not unknown characters.
/// The Unicode near-twins of five APL glyphs, folded onto the glyph they
/// stand for. Harvested APL is full of them — a page typeset in a
/// mathematical font writes the divides sign, the element sign, the tilde
/// operator, the star operator and the minus sign rather than the APL
/// characters — and the reference reads all five. The list is closed: the
/// asterisk operator, dot minus, the overline and the empty set are near
/// twins too and the reference refuses them, so it refuses them here.
///
/// The fold happens where a glyph is read as a symbol, never inside a
/// character literal, so `'∣'` is still that character.
fn lookalike(ch: char) -> char {
    match ch {
        '\u{2223}' => '|', // DIVIDES for the residue
        '\u{2208}' => '∊', // ELEMENT OF for membership
        '\u{223C}' => '~', // TILDE OPERATOR for not/without
        '\u{22C6}' => '*', // STAR OPERATOR for power
        '\u{2212}' => '-', // MINUS SIGN for subtract
        other => other,
    }
}

fn queued_glyph(ch: char) -> Option<&'static str> {
    Some(match ch {
        '⌶' => "I-beam (⌶)",
        // Dyalog's spawn. Named as a queue position rather than reported as
        // an unknown character; whether libjay's sandbox opens APL's
        // threads is the decision that has not been made, not the parsing.
        '&' => "the spawn operator (f&y)",
        _ => return None,
    })
}

fn op_for(ch: char) -> Option<OpGlyph> {
    match ch {
        '/' => Some(OpGlyph::Slash),
        '⌿' => Some(OpGlyph::SlashBar),
        '\\' => Some(OpGlyph::Backslash),
        '⍀' => Some(OpGlyph::BackslashBar),
        '⍤' => Some(OpGlyph::Rank),
        '⍨' => Some(OpGlyph::Commute),
        '⍣' => Some(OpGlyph::Power),
        '∘' => Some(OpGlyph::Jot),
        '⍥' => Some(OpGlyph::Over),
        '⍢' => Some(OpGlyph::Under),
        '⌺' => Some(OpGlyph::Stencil),
        '@' => Some(OpGlyph::At),
        '¨' => Some(OpGlyph::Each),
        '⍛' => Some(OpGlyph::Before),
        '⌸' => Some(OpGlyph::Key),
        '⍠' => Some(OpGlyph::Variant),
        // A `.` that is not the start of a number and not the tail of `∘.`
        // is the inner-product operator.
        '.' => Some(OpGlyph::Dot),
        _ => None,
    }
}

/// Expand as a function: `x\\y` along the last axis, `x⍀y` along the
/// leading one, matching the two axes replicate distinguishes.
fn expand_verb(leading: bool) -> Verb {
    let p = Prim {
        name: if leading { "⍀" } else { "\\" },
        monad: MonadOp::None,
        dyad: DyadOp::Expand,
        ranks: if leading { [RANK_INF, 1, RANK_INF] } else { [RANK_INF, 1, 1] },
    };
    Verb::Prim(p)
}

/// Replicate as a function: `x/y` along the last axis, `x⌿y` along the
/// leading one. One primitive, applied at the rank that picks the axis —
/// exactly the J/APL divergence the shared IR is built to carry.
fn copy_verb(leading: bool) -> Verb {
    let p = Prim {
        name: if leading { "⌿" } else { "/" },
        monad: MonadOp::None,
        dyad: DyadOp::Copy,
        ranks: if leading { [RANK_INF, 1, RANK_INF] } else { [RANK_INF, 1, 1] },
    };
    Verb::Prim(p)
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

/// Split the source into sentences of tokens. Blank sentences are dropped.
/// Spans are absolute offsets into `SourceParts::display`.
fn lex(src: &SourceParts, d: Rules) -> Result<Vec<Vec<Token>>> {
    let mut out: Vec<Vec<Token>> = Vec::new();
    let mut cur: Vec<Token> = Vec::new();
    // A comment runs to the end of a line, which may be a later segment.
    let mut in_comment = false;
    let mut braces = 0usize;
    for seg in &src.segments {
        match seg {
            Segment::Text { text, offset } => {
                lex_text(text, *offset, d, &mut out, &mut cur, &mut in_comment, &mut braces)?;
            }
            Segment::Param { index, offset, len } => {
                if !in_comment {
                    cur.push(Token {
                        kind: Tok::Param(*index),
                        span: Span::new(*offset, offset + len),
                    });
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn lex_text(
    text: &str,
    offset: usize,
    d: Rules,
    out: &mut Vec<Vec<Token>>,
    cur: &mut Vec<Token>,
    in_comment: &mut bool,
    braces: &mut usize,
) -> Result<()> {
    let mut i = 0usize;
    while i < text.len() {
        let ch = text[i..].chars().next().unwrap();
        let clen = ch.len_utf8();
        if *in_comment {
            if ch == '\n' {
                *in_comment = false;
                end_sentence(out, cur);
            }
            i += clen;
            continue;
        }
        match ch {
            // Inside a dfn's braces neither a line break nor `⋄` ends the
            // sentence the dfn belongs to; both separate its statements.
            '\n' | '⋄' => {
                if *braces > 0 {
                    cur.push(Token {
                        kind: Tok::Separator,
                        span: Span::new(offset + i, offset + i + clen),
                    });
                } else {
                    end_sentence(out, cur);
                }
                i += clen;
            }
            ' ' | '\t' | '\r' => i += clen,
            '⍝' => {
                *in_comment = true;
                i += clen;
            }
            '\'' => {
                let (arr, next) = lex_string(text, i, offset)?;
                cur.push(Token {
                    kind: Tok::Value(arr),
                    span: Span::new(offset + i, offset + next),
                });
                i = next;
            }
            '"' => {
                let (arr, next) = lex_escaped_string(text, i, offset)?;
                cur.push(Token {
                    kind: Tok::Value(arr),
                    span: Span::new(offset + i, offset + next),
                });
                i = next;
            }
            // `$ff` is a hexadecimal number, in either case of letter.
            '$' => {
                let (arr, next) = lex_hex(text, i, offset)?;
                cur.push(Token {
                    kind: Tok::Nums(arr),
                    span: Span::new(offset + i, offset + next),
                });
                i = next;
            }
            '{' => {
                *braces += 1;
                cur.push(Token { kind: Tok::LBrace, span: Span::new(offset + i, offset + i + 1) });
                i += 1;
            }
            '}' => {
                *braces = braces.saturating_sub(1);
                cur.push(Token { kind: Tok::RBrace, span: Span::new(offset + i, offset + i + 1) });
                i += 1;
            }
            '∇' => {
                cur.push(Token { kind: Tok::Del, span: Span::new(offset + i, offset + i + clen) });
                i += clen;
            }
            // `⍺⍺` and `⍵⍵` are one name each: a dfn operator's operands.
            '⍺' | '⍵' => {
                let mut end = i + clen;
                if text[end..].starts_with(ch) {
                    end += clen;
                }
                cur.push(Token {
                    kind: Tok::Name(text[i..end].to_string()),
                    span: Span::new(offset + i, offset + end),
                });
                i = end;
            }
            // `:If` and its family are one word; a bare `:` is a dfn guard.
            // A dfn has no control words at all, so inside braces the `:`
            // is a guard however its condition's answer is spelled —
            // `a>10:a` names no control word and never could.
            ':' if *braces > 0 => {
                cur.push(Token {
                    kind: Tok::Colon,
                    span: Span::new(offset + i, offset + i + 1),
                });
                i += 1;
            }
            // `L:` at the head of a line is a label: the name stands for
            // that line's number and `→` branches to it. A control word
            // never follows a name, so the two never compete.
            ':' if is_label_head(cur) => {
                cur.push(Token { kind: Tok::Colon, span: Span::new(offset + i, offset + i + 1) });
                i += 1;
            }
            ':' => {
                let mut j = i + 1;
                while let Some(c) = text[j..].chars().next() {
                    if c.is_ascii_alphabetic() {
                        j += c.len_utf8();
                    } else {
                        break;
                    }
                }
                let span = Span::new(offset + i, offset + j);
                match control_word(&text[i + 1..j]) {
                    Some(word) => cur.push(Token { kind: Tok::Control(word), span }),
                    None if j > i + 1 => {
                        return Err(Error::parse(
                            format!("unknown control word: {}", &text[i..j]),
                            span,
                        ));
                    }
                    None => cur.push(Token {
                        kind: Tok::Colon,
                        span: Span::new(offset + i, offset + i + 1),
                    }),
                }
                i = j;
            }
            '→' => {
                let two = text[i + clen..].starts_with('→');
                let end = if two { i + 2 * clen } else { i + clen };
                cur.push(Token {
                    kind: if two { Tok::CondThen } else { Tok::Arrow },
                    span: Span::new(offset + i, offset + end),
                });
                i = end;
            }
            // `⍬` is the empty numeric vector, written as a constant.
            '⍬' => {
                cur.push(Token {
                    kind: Tok::Value(Array::empty(crate::dtype::DType::I64)),
                    span: Span::new(offset + i, offset + i + clen),
                });
                i += clen;
            }
            '(' => {
                cur.push(Token { kind: Tok::LParen, span: Span::new(offset + i, offset + i + 1) });
                i += 1;
            }
            ')' => {
                cur.push(Token { kind: Tok::RParen, span: Span::new(offset + i, offset + i + 1) });
                i += 1;
            }
            '[' => {
                cur.push(Token {
                    kind: Tok::LBracket,
                    span: Span::new(offset + i, offset + i + 1),
                });
                i += 1;
            }
            ']' => {
                cur.push(Token {
                    kind: Tok::RBracket,
                    span: Span::new(offset + i, offset + i + 1),
                });
                i += 1;
            }
            ';' => {
                cur.push(Token { kind: Tok::Semi, span: Span::new(offset + i, offset + i + 1) });
                i += 1;
            }
            '←' => {
                let rest = &text[i + clen..];
                let (kind, end) = if rest.starts_with('→') {
                    (Tok::CondElse, i + clen + '→'.len_utf8())
                } else if rest.starts_with('←') {
                    (Tok::CondEnd, i + 2 * clen)
                } else {
                    (Tok::Assign, i + clen)
                };
                cur.push(Token { kind, span: Span::new(offset + i, offset + end) });
                i = end;
            }
            // `⍞` has no system-name form: it is always the whole token.
            '⍞' => {
                cur.push(Token {
                    kind: Tok::Quad { quote: true },
                    span: Span::new(offset + i, offset + i + clen),
                });
                i += clen;
            }
            '⎕' => {
                let after = i + clen;
                let mut j = after;
                while let Some(c) = text[j..].chars().next() {
                    if c.is_alphabetic() {
                        j += c.len_utf8();
                    } else {
                        break;
                    }
                }
                if j > after {
                    let span = Span::new(offset + i, offset + j);
                    let name = text[after..j].to_uppercase();
                    // `⎕PP` and `⎕RL` are settings the running program
                    // owns, and take an assignment. Every other system
                    // name libjay answers is read-only — the settings
                    // among them were fixed by the dialect before the
                    // program was compiled — so assigning one is a
                    // refusal and not a promise. A name libjay does not
                    // answer at all reports itself first.
                    if text[j..].trim_start().starts_with('←')
                        && !crate::sysvar::is_settable_bare(&name)
                    {
                        quad_name(&name, d, span)?;
                        return Err(Error::language(
                            format!(
                                "⎕{name} is read-only: libjay's system names are \
                                 fixed before the program runs"
                            ),
                            span,
                        ));
                    }
                    cur.push(Token { kind: quad_name(&name, d, span)?, span });
                    i = j;
                    continue;
                }
                cur.push(Token {
                    kind: Tok::Quad { quote: false },
                    span: Span::new(offset + i, offset + after),
                });
                i = after;
            }
            _ if num_start(text, i) => {
                let (toks, next) = lex_number_vector(text, i, offset)?;
                cur.extend(toks);
                i = next;
            }
            _ if is_name_start(ch) => {
                let start = i;
                i += clen;
                while let Some(c) = text[i..].chars().next() {
                    if is_name_body(c) {
                        i += c.len_utf8();
                    } else {
                        break;
                    }
                }
                cur.push(Token {
                    kind: Tok::Name(text[start..i].to_string()),
                    span: Span::new(offset + start, offset + i),
                });
            }
            _ => {
                let mut end = i + clen;
                let ch = lookalike(ch);
                // `⊤` with a logical glyph after it is one bit-wise
                // function, and the glyph is not the monad it would be on
                // its own — none of the six has one.
                if ch == '⊤'
                    && let Some((p, next)) = bitwise_prim(text, end)
                {
                    cur.push(Token {
                        kind: Tok::Func(Verb::Prim(p)),
                        span: Span::new(offset + i, offset + next),
                    });
                    i = next;
                    continue;
                }
                if let Some(v) = verb_for(ch, d) {
                    cur.push(Token {
                        kind: Tok::Func(v),
                        span: Span::new(offset + i, offset + end),
                    });
                } else if let Some(mut op) = op_for(ch) {
                    // `∘.` is one operator (the outer product); a bare `∘`
                    // is Dyalog's compose, a different thing.
                    if op == OpGlyph::Jot && text[end..].starts_with('.') {
                        op = OpGlyph::JotDot;
                        end += 1;
                    }
                    cur.push(Token {
                        kind: Tok::Op(op),
                        span: Span::new(offset + i, offset + end),
                    });
                } else if let Some(what) = queued_glyph(ch) {
                    // A glyph of the language libjay has not reached yet is
                    // a promise, not an unknown character, and says so.
                    return Err(Error::not_yet(what, Span::new(offset + i, offset + end)));
                } else {
                    return Err(Error::parse(
                        format!("unknown symbol: {ch}"),
                        Span::new(offset + i, offset + end),
                    ));
                }
                i = end;
            }
        }
    }
    Ok(())
}

fn end_sentence(out: &mut Vec<Vec<Token>>, cur: &mut Vec<Token>) {
    if !cur.is_empty() {
        out.push(std::mem::take(cur));
    }
}

pub(crate) fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '∆' || c == '⍙'
}

pub(crate) fn is_name_body(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '∆' || c == '⍙'
}

/// `'...'` with `''` for an embedded quote. A one-character string is a
/// scalar; anything else is a vector.
fn lex_string(text: &str, start: usize, offset: usize) -> Result<(Array, usize)> {
    let mut chars: Vec<char> = Vec::new();
    let mut i = start + 1;
    loop {
        let c = match text[i..].chars().next() {
            Some(c) => c,
            None => {
                return Err(Error::parse(
                    "unterminated string",
                    Span::new(offset + start, offset + text.len()),
                ));
            }
        };
        if c == '\'' {
            if text[i + 1..].starts_with('\'') {
                chars.push('\'');
                i += 2;
                continue;
            }
            i += 1;
            break;
        }
        chars.push(c);
        i += c.len_utf8();
    }
    let shape = if chars.len() == 1 { vec![] } else { vec![chars.len()] };
    Ok((Array::new(shape, Data::Char(chars.into())), i))
}

/// `"…"` with C escapes. A double-quoted string is always a VECTOR, where
/// `'Q'` is a scalar, and `"` has no doubling rule: two of them in a row
/// close one string and open the next, which strands them.
///
/// The escapes are `\a \b \f \n \r \t \v`, `\\`, `\"` and `\0`; `\`
/// before anything else is a backslash and that character, which is what
/// the reference answers.
fn lex_escaped_string(text: &str, start: usize, offset: usize) -> Result<(Array, usize)> {
    let unterminated =
        || Error::parse("unterminated string", Span::new(offset + start, offset + text.len()));
    let mut chars: Vec<char> = Vec::new();
    let mut i = start + 1;
    loop {
        let c = text[i..].chars().next().ok_or_else(unterminated)?;
        i += c.len_utf8();
        match c {
            '"' => break,
            '\\' => {
                let e = text[i..].chars().next().ok_or_else(unterminated)?;
                i += e.len_utf8();
                match e {
                    'a' => chars.push('\u{7}'),
                    'b' => chars.push('\u{8}'),
                    'f' => chars.push('\u{c}'),
                    'n' => chars.push('\n'),
                    'r' => chars.push('\r'),
                    't' => chars.push('\t'),
                    'v' => chars.push('\u{b}'),
                    '\\' | '"' | '0' => chars.push(e),
                    _ => {
                        chars.push('\\');
                        chars.push(e);
                    }
                }
            }
            _ => chars.push(c),
        }
    }
    Ok((Array::new(vec![chars.len()], Data::Char(chars.into())), i))
}

/// `$ff`: a hexadecimal integer, in either case of letter. It is one
/// scalar, so a run of them strands as a run of decimal numbers does.
fn lex_hex(text: &str, start: usize, offset: usize) -> Result<(Array, usize)> {
    let mut i = start + 1;
    while text[i..].chars().next().is_some_and(|c| c.is_ascii_hexdigit()) {
        i += 1;
    }
    let span = Span::new(offset + start, offset + i.max(start + 1));
    if i == start + 1 {
        return Err(Error::parse("$ opens a hexadecimal number and needs a digit", span));
    }
    let v = i64::from_str_radix(&text[start + 1..i], 16)
        .map_err(|_| Error::parse("this hexadecimal number is too large for 64 bits", span))?;
    Ok((Array::scalar_i64(v), i))
}

/// True if a numeric literal starts at byte `i`.
fn num_start(text: &str, i: usize) -> bool {
    let s = match text.get(i..) {
        Some(s) => s,
        None => return false,
    };
    let mut cs = s.chars();
    let c0 = match cs.next() {
        Some(c) => c,
        None => return false,
    };
    if c0.is_ascii_digit() {
        return true;
    }
    if c0 == '.' {
        return cs.next().is_some_and(|d| d.is_ascii_digit());
    }
    if c0 == '¯' {
        return match cs.next() {
            Some(d) if d.is_ascii_digit() => true,
            Some('.') => cs.next().is_some_and(|d| d.is_ascii_digit()),
            _ => false,
        };
    }
    false
}

/// One numeric literal. Returns its value, the machine integer it is
/// exactly (None where it needs floating point), and the byte index just
/// past it.
///
/// The exact integer is not `value as i64`: every i64 above 2^53 rounds
/// when it passes through a double, and `9223372036854775806⌽1 2 3` is a
/// sentence the language answers.
fn lex_number(text: &str, start: usize, offset: usize) -> Result<(f64, Option<i64>, usize)> {
    let mut i = start;
    let mut buf = String::new();
    let mut saw_dot = false;
    if text[i..].starts_with('¯') {
        buf.push('-');
        i += '¯'.len_utf8();
    }
    i = take_digits(text, i, &mut buf);
    if text[i..].starts_with('.') && text[i + 1..].chars().next().is_some_and(|d| d.is_ascii_digit())
    {
        saw_dot = true;
        buf.push('.');
        i += 1;
        i = take_digits(text, i, &mut buf);
    }
    if let Some(c) = text[i..].chars().next() && (c == 'e' || c == 'E') {
        let after = i + 1;
        let neg = text[after..].starts_with('¯');
        let digits_at = if neg { after + '¯'.len_utf8() } else { after };
        if text[digits_at..].chars().next().is_some_and(|d| d.is_ascii_digit()) {
            buf.push('e');
            if neg {
                buf.push('-');
            }
            i = take_digits(text, digits_at, &mut buf);
        }
    }
    let v: f64 = buf.parse().map_err(|_| {
        Error::parse(
            format!("cannot read the number {}", &text[start..i]),
            Span::new(offset + start, offset + i),
        )
    })?;
    // `1e3` is the integer 1000; `1e¯3` and `2.5` are floats. Digits alone
    // are read as an integer straight from the text, so a value the double
    // cannot hold exactly still arrives exact.
    let exact = if saw_dot {
        None
    } else if let Ok(k) = buf.parse::<i64>() {
        Some(k)
    } else if v.fract() == 0.0 && v.abs() < 9.0e18 {
        Some(v as i64)
    } else {
        None
    };
    Ok((v, exact, i))
}

fn take_digits(text: &str, mut i: usize, buf: &mut String) -> usize {
    while let Some(c) = text[i..].chars().next() {
        if c.is_ascii_digit() {
            buf.push(c);
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// True where the next thing in the text, past any blanks, opens index
/// brackets.
fn index_follows(text: &str, at: usize) -> bool {
    let mut k = at;
    while text[k..].starts_with(' ') || text[k..].starts_with('\t') {
        k += 1;
    }
    text[k..].starts_with('[')
}

/// A run of blank-separated numeric literals: one value token. Integers
/// unless some literal needs floating point; a single literal is a scalar.
///
/// Index brackets bind to the value written immediately before them, which
/// in a run of numbers is the LAST number and not the whole run: `1 2 3[2]`
/// is `1 2` beside `3[2]`, and indexing a scalar is a rank error. A run
/// that brackets follow is therefore lexed as two tokens.
fn lex_number_vector(text: &str, start: usize, offset: usize) -> Result<(Vec<Token>, usize)> {
    let mut vals: Vec<crate::complex::Cx> = Vec::new();
    let mut exacts: Vec<i64> = Vec::new();
    let mut any_float = false;
    let mut any_complex = false;
    let mut i = start;
    let mut end;
    // Where the last literal of the run begins, and where the one before
    // it ended: the two places the run is cut at when brackets follow.
    let mut last_start;
    let mut head_end = start;
    loop {
        last_start = i;
        let (v, exact, mut next) = lex_number(text, i, offset)?;
        let mut imag = 0.0;
        if let Some(c) = text[next..].chars().next() {
            // `3J4` is the rectangular form; `J` is not otherwise a
            // character a numeric literal can continue with.
            if (c == 'j' || c == 'J') && num_start(text, next + 1) {
                let (b, _, imag_end) = lex_number(text, next + 1, offset)?;
                imag = b;
                next = imag_end;
                any_complex = true;
            }
        }
        vals.push([v, imag]);
        match exact {
            Some(k) => exacts.push(k),
            None => any_float = true,
        }
        end = next;
        i = next;
        let mut k = i;
        while text[k..].starts_with(' ') || text[k..].starts_with('\t') {
            k += 1;
        }
        if k > i && num_start(text, k) {
            head_end = end;
            i = k;
            continue;
        }
        break;
    }
    let data = if any_complex {
        Data::Complex(vals.into())
    } else if any_float {
        Data::F64(vals.iter().map(|&v| v[0]).collect())
    } else {
        Data::I64(exacts.into())
    };
    let n = data.len();
    let nums = |d: Data, from: usize, to: usize| {
        let shape = if to - from == 1 { vec![] } else { vec![to - from] };
        Tok::Nums(Array::new(shape, d.slice(from, to)))
    };
    if n > 1 && index_follows(text, end) {
        return Ok((
            vec![
                Token {
                    kind: nums(data.clone(), 0, n - 1),
                    span: Span::new(offset + start, offset + head_end),
                },
                Token {
                    kind: nums(data, n - 1, n),
                    span: Span::new(offset + last_start, offset + end),
                },
            ],
            end,
        ));
    }
    Ok((vec![Token { kind: nums(data, 0, n), span: Span::new(offset + start, offset + end) }], end))
}

// ---------------------------------------------------------------------------
// Operator folding
// ---------------------------------------------------------------------------

/// `A∘f` and `f∘A`: the array bound where the other operand belongs, or
/// None where both sides are functions and the ordinary composition runs.
///
/// The array has to be a LITERAL. A computed operand — `(⍳3)∘+` — is a
/// named gap rather than a wrong answer: it would have to be evaluated
/// when the derived function is built, and nothing in the IR holds an
/// operand's expression yet.
fn bind_value(
    it: &mut std::iter::Peekable<std::vec::IntoIter<Token>>,
    out: &mut Vec<Token>,
) -> Option<Token> {
    let left_is_value = out.last().map(|t| &t.kind).and_then(literal).is_some();
    let left_is_func = matches!(out.last().map(|t| &t.kind), Some(Tok::Func(_)));
    let right_is_value = it.peek().map(|t| &t.kind).and_then(literal).is_some();
    let right_is_func = matches!(it.peek().map(|t| &t.kind), Some(Tok::Func(_)));
    if !((left_is_value && right_is_func) || (left_is_func && right_is_value)) {
        return None;
    }
    let ltok = out.pop().expect("checked above");
    let rtok = it.next().expect("peeked");
    let span = Span::merge(ltok.span, rtok.span);
    // `A∘f y` is `A f y`; `f∘A y` is `y f A`.
    let derived = if left_is_value {
        let Tok::Func(g) = rtok.kind else { unreachable!("checked above") };
        Verb::BondLeft(literal(&ltok.kind).expect("checked above").clone(), Box::new(g))
    } else {
        let Tok::Func(f) = ltok.kind else { unreachable!("checked above") };
        Verb::BondRight(Box::new(f), literal(&rtok.kind).expect("checked above").clone())
    };
    Some(Token { kind: Tok::Func(derived), span })
}

/// Fold monadic and dyadic operators into derived-function tokens, left to
/// right. After this the sentence holds only values, names, functions, `←`,
/// `⎕` and parentheses.
fn fold_operators(toks: Vec<Token>, d: Rules) -> Result<Vec<Token>> {
    let mut out: Vec<Token> = Vec::new();
    let mut it = toks.into_iter().peekable();
    while let Some(t) = it.next() {
        // Parentheses close here rather than in a pass of their own: by the
        // time the `)` arrives everything inside has been folded, so a pair
        // holding nothing but functions is a single function from here on
        // and the operator to its right binds to it.
        if matches!(t.kind, Tok::RParen) {
            out.push(t);
            close_paren(&mut out, d)?;
            continue;
        }
        // A dfn that mentions `⍺⍺` or `⍵⍵` is an operator: it takes the
        // function on its left, and one on its right where it asked for it.
        if let Tok::UserOp { def, omega } = &t.kind {
            let (def, omega) = (def.clone(), *omega);
            // An operand written as an expression is read where the
            // derived function is applied; until then its PLACE is filled
            // with an empty array, which is enough to fix the reading the
            // body parses under.
            let mut deferred: Option<(Expr, bool, Span)> = None;
            let placeholder =
                || Operand::Value(Box::new(Array::empty(crate::dtype::DType::I64)));
            let right = if omega {
                match it.peek().map(|tok| &tok.kind) {
                    Some(Tok::Func(_)) => {
                        let g = it.next().expect("peeked");
                        let Tok::Func(g) = g.kind else { unreachable!("checked above") };
                        Some(Operand::Func(Box::new(g)))
                    }
                    // Dyalog lets an ARRAY stand where a function operand
                    // belongs, and the body reads `⍵⍵` as that array.
                    _ => match take_value_operand(&mut it, d)? {
                        Some((ValueOperand::Now(a), _)) => Some(Operand::Value(Box::new(a))),
                        Some((ValueOperand::Later(e), span)) => {
                            deferred = Some((e, false, span));
                            Some(placeholder())
                        }
                        None => {
                            return Err(Error::parse(
                                "⍵⍵ needs an operand on the operator's right",
                                t.span,
                            ));
                        }
                    },
                }
            } else {
                None
            };
            let (alpha, fspan) = match out.last().map(|tok| &tok.kind) {
                Some(Tok::Func(_)) => {
                    let Some(Token { kind: Tok::Func(f), span }) = out.pop() else {
                        unreachable!("checked above")
                    };
                    (Operand::Func(Box::new(f)), span)
                }
                _ => match take_left_value_operand(&mut out, d)? {
                    Some((ValueOperand::Now(a), span)) => {
                        (Operand::Value(Box::new(a)), span)
                    }
                    Some((ValueOperand::Later(e), span)) => {
                        if deferred.is_some() {
                            return Err(Error::not_yet(
                                "both operands of an operator computed at once",
                                span,
                            ));
                        }
                        deferred = Some((e, true, span));
                        (placeholder(), span)
                    }
                    None => {
                        return Err(Error::parse(
                            "⍺⍺ needs an operand on the operator's left",
                            t.span,
                        ));
                    }
                },
            };
            let template = Verb::UserDerived { def, alpha, omega: right };
            let derived = match deferred {
                None => template,
                Some((operand, is_alpha, _)) => {
                    Verb::Deferred(std::sync::Arc::new(crate::verb::Deferred {
                        operand,
                        template,
                        build: if is_alpha { built_alpha } else { built_omega },
                        spelling: "(n {…})".to_string(),
                        choices: std::collections::HashMap::new(),
                    }))
                }
            };
            out.push(Token { kind: Tok::Func(derived), span: Span::merge(fspan, t.span) });
            continue;
        }
        let op = match t.kind {
            Tok::Op(op) => op,
            _ => {
                out.push(t);
                continue;
            }
        };
        // The outer product is the one operator whose operand is on its
        // right; it derives the same table J spells `u/`.
        if op == OpGlyph::JotDot {
            let ftok = match it.peek() {
                Some(tok) if matches!(tok.kind, Tok::Func(_)) => it.next().unwrap(),
                _ => {
                    return Err(Error::parse("∘. needs a function on its right", t.span));
                }
            };
            let span = Span::merge(t.span, ftok.span);
            let Tok::Func(f) = ftok.kind else { unreachable!("checked above") };
            out.push(Token { kind: Tok::Func(Verb::Reduce(Box::new(f))), span });
            continue;
        }
        // `A∘f` and `f∘A` bind an array where the other operand belongs:
        // `A∘f` supplies it on the left (`2∘× 5` is `2×5`) and `f∘A` on
        // the right (`(÷∘2) 7` is `7÷2`). Both are monadic only, as J's
        // `m&v` and `u&n` are. The other compositions take functions.
        if op == OpGlyph::Jot
            && let Some(bound) = bind_value(&mut it, &mut out)
        {
            out.push(bound);
            continue;
        }
        // `A∘B` with a function on neither side is not a composition at
        // all: it is GNU APL's matrix product, a function of two values.
        if op == OpGlyph::Jot
            && !matches!(it.peek().map(|x| &x.kind), Some(Tok::Func(_)))
            && !matches!(out.last().map(|x| &x.kind), Some(Tok::Func(_)))
        {
            out.push(Token {
                kind: Tok::Func(Verb::Prim(Prim {
                    name: "∘",
                    monad: MonadOp::None,
                    dyad: DyadOp::MatrixProduct,
                    ranks: [RANK_INF, RANK_INF, RANK_INF],
                })),
                span: t.span,
            });
            continue;
        }
        // `@` takes an operand of either part of speech on either side, so
        // neither the function-to-the-left path nor the value one fits it.
        if op == OpGlyph::At {
            let (right, rspan) = at_operand(&mut it, d, t.span)?;
            // A bracketed VALUE is still bracketed here: the closing pass
            // collapses a function group and leaves a value one alone.
            if matches!(out.last().map(|x| &x.kind), Some(Tok::RParen))
                && let Some(open) = matching_lparen(&out, out.len() - 1)
                && out.len() == open + 3
                && literal(&out[open + 1].kind).is_some()
            {
                let inner = out.remove(open + 1);
                let span = Span::merge(out[open].span, out[open + 1].span);
                out.truncate(open);
                out.push(Token { kind: inner.kind, span });
            }
            let (left, lspan) = match out.pop() {
                Some(Token { kind: Tok::Func(f), span }) => {
                    (Operand::Func(Box::new(f)), span)
                }
                Some(tok) if literal(&tok.kind).is_some() => (
                    Operand::Value(Box::new(literal(&tok.kind).expect("checked").clone())),
                    tok.span,
                ),
                _ => {
                    return Err(Error::parse("@ needs an operand on its left", t.span));
                }
            };
            out.push(Token {
                kind: Tok::Func(Verb::At { left, right }),
                span: Span::merge(lspan, rspan),
            });
            continue;
        }
        // `f∘g` and `f⍥g` need a function on both sides; the right one is
        // taken here so the ordinary "operand to the left" path can run.
        if matches!(
            op,
            OpGlyph::Jot | OpGlyph::Over | OpGlyph::Before | OpGlyph::Under | OpGlyph::Dot
        ) {
            // `P.x` is a name on both sides of the dot: a MEMBER of a
            // structured variable, not an inner product. It is a
            // name-space feature — every assignment, lookup and scope rule
            // would have to learn about dotted paths — so it names itself
            // rather than reporting the inner product it is not.
            if op == OpGlyph::Dot
                && matches!(it.peek().map(|x| &x.kind), Some(Tok::Name(_)))
                && matches!(out.last().map(|x| &x.kind), Some(Tok::Name(_)))
            {
                return Err(Error::not_yet("a structured variable (P.x)", t.span));
            }
            let Some(gtok) = it.peek().filter(|x| matches!(x.kind, Tok::Func(_))) else {
                return Err(Error::not_yet(
                    format!("{} with a value operand", op.glyph()),
                    t.span,
                ));
            };
            let gspan = gtok.span;
            let Some(Token { kind: Tok::Func(g), .. }) = it.next() else {
                unreachable!("peeked a function")
            };
            let Some(Token { kind: Tok::Func(f), span: fspan }) = out.pop() else {
                return Err(Error::not_yet(
                    format!("{} with a value operand", op.glyph()),
                    t.span,
                ));
            };
            let span = Span::merge(fspan, gspan);
            // Beside runs g on the right argument only; over runs it on
            // both. Neither is in GNU APL, so both follow Dyalog.
            let derived = match op {
                OpGlyph::Jot => Verb::Beside(Box::new(f), Box::new(g)),
                OpGlyph::Before => Verb::Before(Box::new(f), Box::new(g)),
                // Under is over, undone: the published definition is
                // `g⍣¯1 ⊢ (g x) f (g y)`, on the arguments whole, so it is
                // J's `&.:` over the same obverse table.
                OpGlyph::Under => {
                    let back = crate::verb::obverse(&g).ok_or_else(|| {
                        Error::not_yet(
                            format!("the obverse of {} (no inverse is known)", g.name()),
                            gspan,
                        )
                    })?;
                    let composed = Verb::Compose(Box::new(f), Box::new(g));
                    Verb::Atop(Box::new(back), Box::new(composed), AtopForm::At)
                }
                // `f.g` folds with f what g made of every row and column:
                // `+.×` is the matrix product, `∧.=` asks which rows match.
                OpGlyph::Dot => Verb::InnerProduct {
                    u: Box::new(Verb::Reduce(Box::new(f))),
                    v: Box::new(g),
                    apl: true,
                },
                _ => Verb::Compose(Box::new(f), Box::new(g)),
            };
            out.push(Token { kind: Tok::Func(derived), span });
            continue;
        }
        // Left operand: a function, derived or not.
        let left_is_func = matches!(out.last().map(|x| &x.kind), Some(Tok::Func(_)));
        if !left_is_func {
            // After an operand these glyphs are functions, not operators.
            // Names are always values in this subset, so the reading is
            // decided by the token to the left and nothing else.
            if out.last().is_some_and(|x| is_operand_end(&x.kind)) {
                let f = match op {
                    OpGlyph::Slash => copy_verb(false),
                    OpGlyph::SlashBar => copy_verb(true),
                    OpGlyph::Backslash => expand_verb(false),
                    OpGlyph::BackslashBar => expand_verb(true),
                    OpGlyph::Rank
                    | OpGlyph::Commute
                    | OpGlyph::Power
                    | OpGlyph::JotDot
                    | OpGlyph::Jot
                    | OpGlyph::Over
                    | OpGlyph::Under
                    | OpGlyph::Stencil
                    | OpGlyph::Before
                    | OpGlyph::Key
                    | OpGlyph::Dot
                    | OpGlyph::Variant
                    | OpGlyph::At
                    | OpGlyph::Each => {
                        return Err(Error::parse(
                            format!("{} needs a function to its left", op.glyph()),
                            t.span,
                        ));
                    }
                };
                out.push(Token { kind: Tok::Func(f), span: t.span });
                continue;
            }
            return Err(Error::parse(
                format!("{} needs a function to its left", op.glyph()),
                t.span,
            ));
        }
        let ftok = out.pop().unwrap();
        let f = match ftok.kind {
            Tok::Func(f) => f,
            _ => unreachable!("checked above"),
        };
        let span = Span::merge(ftok.span, t.span);
        // An explicit axis replaces the glyph's own choice of one: `+/[k]`
        // and `+⌿[k]` both reduce axis k, and `f\\[k]` and `f⍀[k]` both scan
        // it, which is what makes the two spellings the same function here.
        if let Some((spec, aspan)) = take_axis(&mut it, d)? {
            let inner = match op {
                OpGlyph::Slash | OpGlyph::SlashBar => Verb::NWise(Box::new(f)),
                OpGlyph::Backslash | OpGlyph::BackslashBar => {
                    Verb::Windowed(Box::new(Verb::Reduce(Box::new(f))), WindowKind::Scan)
                }
                _ => {
                    return Err(Error::not_yet(
                        format!("axis specification for {}", op.glyph()),
                        aspan,
                    ));
                }
            };
            let spelling = format!("{}[k]", op.glyph());
            let v = spec.derive(inner, built_fold_axis, spelling, aspan, d)?;
            out.push(Token { kind: Tok::Func(v), span: Span::merge(span, aspan) });
            continue;
        }
        let derived = match op {
            // APL's divergence from J: `/` reduces the last axis, `⌿` the
            // leading one. `+/` sums rows, `+⌿` sums columns. The dyad is
            // the n-wise reduction, whose left argument is one number
            // however it is shaped, so the wrapper frames the right
            // argument alone.
            OpGlyph::Slash => Verb::Rank(Box::new(Verb::NWise(Box::new(f))), [1, RANK_INF, 1].into()),
            OpGlyph::SlashBar => Verb::NWise(Box::new(f)),
            // The scan follows the reduce: `\` along the last axis, `⍀`
            // along the leading one. The k-th element is the reduce of the
            // first k, which is the verb applied to the k-th prefix.
            OpGlyph::Backslash => Verb::Rank(
                Box::new(Verb::Windowed(Box::new(Verb::Reduce(Box::new(f))), WindowKind::Scan)),
                [1, 1, 1].into(),
            ),
            OpGlyph::BackslashBar => {
                Verb::Windowed(Box::new(Verb::Reduce(Box::new(f))), WindowKind::Scan)
            }
            OpGlyph::Commute => Verb::Commute(Box::new(f)),
            OpGlyph::Key => Verb::KeyPairs(Box::new(f)),
            // `@` is settled above, where either operand may be a value.
            OpGlyph::At => return Err(Error::internal("@ folded twice")),
            // `f⍠B`: one setting of the dialect overridden for this
            // application and no other.
            OpGlyph::Variant => {
                let (options, ospan) = variant_options(&mut it, t.span)?;
                let derived = variant(f, &options, Span::merge(span, ospan))?;
                out.push(Token { kind: Tok::Func(derived), span: Span::merge(span, ospan) });
                continue;
            }
            // `.` reached with no function on its right: `+.` alone is not
            // a function in either lineage.
            OpGlyph::Dot => {
                return Err(Error::parse("the inner product . needs a function on its right", t.span));
            }
            // Each: the function runs on the contents of every item and
            // its result goes back into an item. A simple scalar result
            // stays simple, which is APL's enclosure rule.
            OpGlyph::Each => Verb::Each(Box::new(f), Enclose::ExceptSimpleScalar),
            OpGlyph::Power => {
                // `f⍣g` iterates until `new g old` holds: `f⍣≡` is the
                // fixed point, which is the spelling the reference uses.
                if matches!(it.peek().map(|tok| &tok.kind), Some(Tok::Func(_))) {
                    let gtok = it.next().expect("peeked");
                    let Tok::Func(g) = gtok.kind else { unreachable!("checked above") };
                    let v = Verb::PowerUntil(Box::new(f), Box::new(g));
                    out.push(Token {
                        kind: Tok::Func(v),
                        span: Span::merge(span, gtok.span),
                    });
                    continue;
                }
                let Some((count, cspan)) = take_value_operand(&mut it, d)? else {
                    return Err(Error::parse("⍣ needs a count on its right", t.span));
                };
                // `f⍣¯n` runs f's inverse n times, over the same obverse
                // table J's `u^:_n` reads.
                let spelling = format!("{}⍣n", f.name());
                let v = count.derive(f, built_power, spelling, cspan, d)?;
                out.push(Token { kind: Tok::Func(v), span: Span::merge(span, cspan) });
                continue;
            }
            // `f⌺w`: the window sizes are a value on the right, one per
            // leading axis. Dyalog also takes a two-row form giving the
            // movement; that is a named gap.
            OpGlyph::Stencil => {
                let Some(spec) = it.peek().filter(|t| literal(&t.kind).is_some()) else {
                    return Err(Error::parse(
                        "⌺ needs a window specification on its right",
                        t.span,
                    ));
                };
                let sspan = spec.span;
                let spec = it.next().expect("peeked a literal");
                let arr = literal(&spec.kind).expect("checked above");
                if arr.rank() > 1 {
                    return Err(Error::not_yet(
                        "a stencil with a movement row (f⌺(m⍪w))",
                        sspan,
                    ));
                }
                let sizes = arr
                    .to_i64_vec()
                    .ok_or_else(|| Error::domain("a stencil window is whole numbers", sspan))?;
                let v = Verb::Stencil(Box::new(f), sizes);
                out.push(Token { kind: Tok::Func(v), span: Span::merge(span, sspan) });
                continue;
            }
            OpGlyph::Rank => {
                let spec = match it.peek() {
                    // `f⍤g` with a function on the right is Dyalog's atop:
                    // monadically `f g y`, dyadically `f (x g y)`.
                    Some(tok) if matches!(tok.kind, Tok::Func(_)) => {
                        let gtok = it.next().unwrap();
                        let Tok::Func(g) = gtok.kind else { unreachable!("checked above") };
                        let v = Verb::Atop(Box::new(f), Box::new(g), AtopForm::At);
                        out.push(Token {
                            kind: Tok::Func(v),
                            span: Span::merge(span, gtok.span),
                        });
                        continue;
                    }
                    Some(tok) if literal(&tok.kind).is_some() => it.next().unwrap(),
                    _ => {
                        return Err(Error::parse(
                            "⍤ needs a rank specification on its right",
                            t.span,
                        ));
                    }
                };
                let arr = literal(&spec.kind).expect("checked above");
                let ranks = rank_spec(arr, spec.span)?;
                let f = Verb::Rank(Box::new(f), ranks.into());
                out.push(Token { kind: Tok::Func(f), span: Span::merge(span, spec.span) });
                continue;
            }
            // These are answered before the left operand is taken.
            OpGlyph::JotDot
            | OpGlyph::Jot
            | OpGlyph::Over
            | OpGlyph::Under
            | OpGlyph::Before => {
                unreachable!("handled above")
            }
        };
        out.push(Token { kind: Tok::Func(derived), span });
    }
    Ok(out)
}

/// `[k]` immediately after a function or an operator glyph, if it is there.
///
/// The axes are written in `⎕IO` origin and come back counted from zero. A
/// number that is not whole names the GAP before the axis it rounds up to,
/// which is where a new axis of the result goes; the value that comes back
/// is that position.
fn take_axis(
    it: &mut std::iter::Peekable<std::vec::IntoIter<Token>>,
    d: Rules,
) -> Result<Option<(ValueOperand, Span)>> {
    if !matches!(it.peek().map(|t| &t.kind), Some(Tok::LBracket)) {
        return Ok(None);
    }
    Ok(Some(bracket_operand(it, d)?))
}

/// The axes a bracket's value names.
fn axis_spec(arr: &Array, origin: i64, span: Span) -> Result<AxisSpec> {
    if arr.rank() > 1 {
        return Err(Error::parse("an axis specification is one number or a vector", span));
    }
    if let Some(ints) = arr.to_i64_vec() {
        let mut axes = Vec::with_capacity(ints.len());
        for k in ints {
            let a = k - origin;
            if a < 0 {
                return Err(Error::domain(
                    format!("axis {k} does not exist: they start at {origin}"),
                    span,
                ));
            }
            axes.push(a as usize);
        }
        return Ok(AxisSpec::Axes(axes));
    }
    // A FRACTIONAL axis names the gap between two axes: `[0.5]` under
    // `⎕IO←1` is the gap before the first one, `[1.5]` the one after it.
    let Some(reals) = arr.to_f64_vec() else {
        return Err(Error::parse("an axis must be a number", span));
    };
    let [k] = reals[..] else {
        return Err(Error::parse("only one axis may name a gap between axes", span));
    };
    let place = (k - origin as f64).ceil();
    if place < 0.0 {
        return Err(Error::domain(
            format!("axis {k} lies before the first one, which is {origin}"),
            span,
        ));
    }
    Ok(AxisSpec::Between(place as usize))
}

/// The options `⍠` was given, as `(name, value)` pairs.
///
/// A bare number is the PRINCIPAL option, which for every function libjay
/// gives a variant is the comparison tolerance — so it arrives here named
/// `CT`. The other published spelling is one or more parenthesised pairs,
/// `⍠('IO' 0)` and `⍠('IO' 0)('CT' 0)`, whose halves are both literals.
/// A computed option is a named gap: the variant is settled when the
/// program is compiled, as the dialect it overrides is.
fn variant_options(
    it: &mut std::iter::Peekable<std::vec::IntoIter<Token>>,
    span: Span,
) -> Result<(Vec<(String, Array)>, Span)> {
    if let Some(tok) = it.peek().filter(|t| literal(&t.kind).is_some()) {
        let (value, vspan) = (literal(&tok.kind).expect("peeked a literal").clone(), tok.span);
        it.next();
        return Ok((vec![("CT".to_string(), value)], vspan));
    }
    let mut options = Vec::new();
    let mut last = span;
    while it.peek().is_some_and(|t| matches!(t.kind, Tok::LParen)) {
        it.next();
        let mut inside: Vec<Array> = Vec::new();
        loop {
            let Some(tok) = it.next() else {
                return Err(Error::parse("unmatched ( after ⍠", span));
            };
            last = tok.span;
            if matches!(tok.kind, Tok::RParen) {
                break;
            }
            match literal(&tok.kind) {
                Some(a) => inside.push(a.clone()),
                None => {
                    return Err(Error::not_yet(
                        "a computed variant option (f⍠v with a name or an expression)",
                        tok.span,
                    ));
                }
            }
        }
        let [name, value] = inside.as_slice() else {
            return Err(Error::parse("a variant option is a name and a value", last));
        };
        let Data::Char(cs) = &name.data else {
            return Err(Error::parse("a variant option starts with its name", last));
        };
        options.push((cs.as_slice().iter().collect::<String>().to_uppercase(), value.clone()));
    }
    if options.is_empty() {
        let where_ = it.peek().map_or(span, |t| t.span);
        return Err(Error::not_yet(
            "a computed variant option (f⍠v with a name or an expression)",
            where_,
        ));
    }
    Ok((options, last))
}

/// `f⍠B`: f with the dialect settings B names overridden for this
/// application. `CT` is the comparison tolerance, which is the same
/// mechanism J spells `!.`; `IO` is the index origin, which is resolved
/// into the primitives when the program is compiled, so overriding it
/// derives the verb again.
fn variant(f: Verb, options: &[(String, Array)], span: Span) -> Result<Verb> {
    let mut out = f;
    for (name, value) in options {
        out = match name.as_str() {
            "CT" => {
                let Some(ct) = value.to_f64_vec().and_then(|v| v.first().copied()) else {
                    return Err(Error::domain("a comparison tolerance is a number", span));
                };
                if !out.uses_tolerance() {
                    return Err(Error::domain(
                        format!(
                            "the comparison tolerance is not an option of {}: it consults none",
                            out.name()
                        ),
                        span,
                    ));
                }
                if !(0.0..1.0).contains(&ct) {
                    return Err(Error::domain(
                        "a comparison tolerance lies between 0 and 1",
                        span,
                    ));
                }
                Verb::Fit(Box::new(out), ct)
            }
            "IO" => {
                let Some(io) = value.to_i64_vec().and_then(|v| v.first().copied()) else {
                    return Err(Error::domain("an index origin is a whole number", span));
                };
                if io != 0 && io != 1 {
                    return Err(Error::domain("an index origin is 0 or 1", span));
                }
                crate::verb::with_origin(&out, io).ok_or_else(|| {
                    Error::domain(
                        format!("the index origin is not an option of {}", out.name()),
                        span,
                    )
                })?
            }
            other => {
                return Err(Error::not_yet(
                    format!("the variant option {other} (f⍠v)"),
                    span,
                ));
            }
        };
    }
    Ok(out)
}

/// `(/)` is `/`: parentheses around a bare operator glyph are transparent,
/// so what decides the glyph's reading is the token outside them. The pair
/// has no other meaning — an operator has no operand inside them — and the
/// reference reads `1 0 1(/)1 2 3` as the replication it spells without.
fn unwrap_lone_operators(toks: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(toks.len());
    for t in toks {
        let n = out.len();
        if matches!(t.kind, Tok::RParen)
            && n >= 2
            && matches!(out[n - 1].kind, Tok::Op(_))
            && matches!(out[n - 2].kind, Tok::LParen)
        {
            let op = out.pop().expect("checked above");
            let open = out.pop().expect("checked above");
            out.push(Token { kind: op.kind, span: Span::merge(open.span, t.span) });
            continue;
        }
        out.push(t);
    }
    out
}

/// A `)` has just been pushed: collapse the pair it closes when what it
/// holds is a function. `(f)` is `f` — a function alone in parentheses is
/// only grouped — and a run of two or more is a train.
fn close_paren(out: &mut Vec<Token>, d: Rules) -> Result<()> {
    let close = out.len() - 1;
    let Some(open) = matching_lparen(out, close) else { return Ok(()) };
    let span = Span::merge(out[open].span, out[close].span);
    let inner = &out[open + 1..close];
    if inner.len() == 1 && matches!(inner[0].kind, Tok::Func(_)) {
        let Some(Token { kind, .. }) = out.get(open + 1).cloned() else {
            unreachable!("checked above")
        };
        out.truncate(open);
        out.push(Token { kind, span });
        return Ok(());
    }
    if !d.trains || inner.len() < 2 || !inner[1..].iter().all(|t| matches!(t.kind, Tok::Func(_))) {
        return Ok(());
    }
    let Some(verb) = train(inner)? else { return Ok(()) };
    out.truncate(open);
    out.push(Token { kind: Tok::Func(verb), span });
    Ok(())
}

/// The names inside the brackets of a distributed assignment target. They
/// stand side by side and nothing else may stand among them: a target is a
/// list of names, not an expression that happens to yield one.
fn target_names(toks: &[Token], span: Span) -> Result<Vec<String>> {
    if toks.is_empty() {
        return Err(Error::parse("an assignment target names at least one name", span));
    }
    let mut names = Vec::with_capacity(toks.len());
    for t in toks {
        match &t.kind {
            Tok::Name(n) => names.push(n.clone()),
            _ => return Err(Error::parse("assignment target must be a name", t.span)),
        }
    }
    Ok(names)
}

/// `@`'s right operand, which may be a function, a value, or a
/// parenthesised group the pass has not reached yet — a `(` is folded when
/// its `)` arrives, and `@`'s operand stands before that happens.
fn at_operand(
    it: &mut std::iter::Peekable<std::vec::IntoIter<Token>>,
    d: Rules,
    span: Span,
) -> Result<(Operand, Span)> {
    let one = |tok: Token| -> Result<(Operand, Span)> {
        match &tok.kind {
            Tok::Func(f) => Ok((Operand::Func(Box::new(f.clone())), tok.span)),
            k if literal(k).is_some() => Ok((
                Operand::Value(Box::new(literal(k).expect("checked").clone())),
                tok.span,
            )),
            _ => Err(Error::parse("@ needs an operand on its right", tok.span)),
        }
    };
    match it.peek().map(|t| &t.kind) {
        Some(Tok::LParen) => {
            let open = it.next().expect("peeked");
            let mut depth = 1usize;
            let mut inner: Vec<Token> = Vec::new();
            let mut end = open.span;
            for tok in it.by_ref() {
                match tok.kind {
                    Tok::LParen => depth += 1,
                    Tok::RParen => {
                        depth -= 1;
                        if depth == 0 {
                            end = tok.span;
                            break;
                        }
                    }
                    _ => {}
                }
                inner.push(tok);
            }
            if depth != 0 {
                return Err(Error::parse("unmatched (", open.span));
            }
            let mut folded = fold_operators(inner, d)?;
            let whole = Span::merge(open.span, end);
            if folded.len() > 1 {
                // A train inside the brackets is one function; the closing
                // pass wants the brackets back to see it.
                folded.insert(0, Token { kind: Tok::LParen, span: open.span });
                folded.push(Token { kind: Tok::RParen, span: end });
                close_paren(&mut folded, d)?;
            }
            match folded.len() {
                1 => one(folded.pop().expect("one token")).map(|(o, _)| (o, whole)),
                _ => Err(Error::not_yet("@ over a computed operand", span)),
            }
        }
        Some(_) => {
            let tok = it.next().expect("peeked");
            one(tok)
        }
        None => Err(Error::parse("@ needs an operand on its right", span)),
    }
}

/// The `(` that `out[close]` closes, counting the pairs between.
fn matching_lparen(out: &[Token], close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for i in (0..close).rev() {
        match out[i].kind {
            Tok::RParen => depth += 1,
            Tok::LParen => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// The function a run of tines derives, grouping from the right: a pair is
/// an atop `g (h ⍵)`, a triple a fork `(f ⍵) g (h ⍵)`, and a longer run is
/// one of those over the train the rest of it makes. The leftmost tine may
/// be a value, which stands where `f ⍵` would.
///
/// `None` where the run is not a train after all, so that the sentence gets
/// the reading it would have had; every other refusal is an error, because
/// nothing else can be meant by a run of functions.
fn train(tines: &[Token]) -> Result<Option<Verb>> {
    debug_assert!(!tines.is_empty());
    if tines.len() == 1 {
        return Ok(match &tines[0].kind {
            Tok::Func(f) => Some(f.clone()),
            _ => None,
        });
    }
    if tines.len() == 2 {
        let (Tok::Func(g), Tok::Func(h)) = (&tines[0].kind, &tines[1].kind) else {
            return Ok(None);
        };
        return Ok(Some(Verb::Atop(Box::new(g.clone()), Box::new(h.clone()), AtopForm::At)));
    }
    // An odd run forks its first two tines over the rest; an even one has
    // no tine to fork with, so the first is an atop over the rest.
    let head = &tines[0].kind;
    if tines.len() % 2 == 0 {
        let Tok::Func(f) = head else {
            return Err(Error::parse(
                "a value may only be a fork's left tine, and this train has an even number of tines",
                tines[0].span,
            ));
        };
        let Some(rest) = train(&tines[1..])? else { return Ok(None) };
        return Ok(Some(Verb::Atop(Box::new(f.clone()), Box::new(rest), AtopForm::At)));
    }
    let Some(rest) = train(&tines[2..])? else { return Ok(None) };
    let Tok::Func(g) = &tines[1].kind else { unreachable!("the tail is all functions") };
    match head {
        Tok::Func(f) => {
            Ok(Some(Verb::Fork(Box::new(f.clone()), Box::new(g.clone()), Box::new(rest))))
        }
        Tok::Value(n) | Tok::Nums(n) => {
            Ok(Some(Verb::NounFork(n.clone(), Box::new(g.clone()), Box::new(rest))))
        }
        // A name, an interpolation hole or a bracketed selection is a value
        // this frontend only has at run time; a fork's left tine is settled
        // when the train is built, as J's is.
        Tok::Name(_) | Tok::Param(_) | Tok::RParen | Tok::RBracket | Tok::Niladic(_) => {
            Err(Error::not_yet("a train whose left tine is a computed value", tines[0].span))
        }
        _ => Ok(None),
    }
}

/// The tail of `toks` when it is a run of tines that names a function: the
/// value side of `F←+/`, `F←+/÷≢` or `F←2 3⍴⍳`.
///
/// A single function is that function; two or more are a train. `None`
/// where the tail is not a run of tines at all.
fn tine_run(toks: &[Token], d: Rules) -> Result<Option<Verb>> {
    if !d.trains || toks.is_empty() {
        return Ok(None);
    }
    if !toks[1..].iter().all(|t| matches!(t.kind, Tok::Func(_))) {
        return Ok(None);
    }
    train(toks)
}

/// An operand written where a VALUE belongs: the count of `f⍣n`, the axis
/// of `f[k]`, the array a user-written operator is given for `⍺⍺` or `⍵⍵`.
///
/// Either the whole of it is settled while the program compiles, or it is
/// an expression the derived function reads afresh at every application —
/// which is what lets a definition's own argument decide a count or an
/// axis.
enum ValueOperand {
    Now(Array),
    Later(Expr),
}

impl ValueOperand {
    /// The derived function this operand makes of `template`, built now
    /// where the operand is settled and at each application where it is
    /// not. `spelling` names the whole in a diagnostic.
    fn derive(
        self,
        template: Verb,
        build: fn(&Verb, &Array, Span, Rules) -> Result<Verb>,
        spelling: String,
        span: Span,
        d: Rules,
    ) -> Result<Verb> {
        match self {
            ValueOperand::Now(a) => build(&template, &a, span, d),
            ValueOperand::Later(operand) => {
                Ok(Verb::Deferred(std::sync::Arc::new(crate::verb::Deferred {
                    operand,
                    template,
                    build,
                    spelling,
                    choices: std::collections::HashMap::new(),
                })))
            }
        }
    }
}

/// The configuration a compile-time evaluation runs under.
fn const_cfg(d: Rules) -> crate::verb::EvalCfg {
    crate::verb::EvalCfg {
        agreement: crate::verb::Agreement::ExactOrScalar,
        fmt: crate::fmt::FmtOpts::APL,
        tol: d.tol(),
        fill: None,
        rules: d,
    }
}

/// An operand expression as an operand: its value where the whole of it is
/// settled now, and the expression itself where it is not.
fn settle(e: Expr, d: Rules) -> ValueOperand {
    match crate::ir::fold_const(&e, const_cfg(d)) {
        Some(a) => ValueOperand::Now(a),
        None => ValueOperand::Later(e),
    }
}

/// The operand a whole `[…]` holds, taken from the token stream.
fn bracket_operand(
    it: &mut std::iter::Peekable<std::vec::IntoIter<Token>>,
    d: Rules,
) -> Result<(ValueOperand, Span)> {
    let open = it.next().expect("the caller peeked a [");
    let mut inner: Vec<Token> = Vec::new();
    let mut depth = 0usize;
    let close = loop {
        let Some(tok) = it.next() else {
            return Err(Error::parse("unterminated axis specification", open.span));
        };
        match tok.kind {
            Tok::RBracket if depth == 0 => break tok,
            Tok::LBracket => depth += 1,
            Tok::RBracket => depth -= 1,
            _ => {}
        }
        inner.push(tok);
    };
    let span = Span::merge(open.span, close.span);
    if let [only] = inner.as_slice()
        && let Some(a) = literal(&only.kind)
    {
        return Ok((ValueOperand::Now(a.clone()), span));
    }
    Ok((settle(parse_prepared(&inner, span, d)?, d), span))
}

/// The value a whole `[…]` holds, for the brackets that choose a MEANING
/// rather than name an axis — `⊤[N]` and `⌹[K]`. Those pick which function
/// the glyph stands for, which is settled while the program compiles, so
/// one that reads a name is a named gap.
fn bracket_value(
    it: &mut std::iter::Peekable<std::vec::IntoIter<Token>>,
    d: Rules,
    what: &str,
) -> Result<(Array, Span)> {
    match bracket_operand(it, d)? {
        (ValueOperand::Now(a), span) => Ok((a, span)),
        (ValueOperand::Later(_), span) => Err(Error::not_yet(what, span)),
    }
}

/// The tokens of the balanced `( … )` at the head of the stream, without
/// the brackets themselves. The caller has peeked the `(`.
fn paren_run(
    it: &mut std::iter::Peekable<std::vec::IntoIter<Token>>,
) -> Result<(Vec<Token>, Span)> {
    let open = it.next().expect("the caller peeked a (");
    let mut inner: Vec<Token> = Vec::new();
    let mut depth = 0usize;
    let close = loop {
        let Some(tok) = it.next() else {
            return Err(Error::parse("unmatched (", open.span));
        };
        match tok.kind {
            Tok::RParen if depth == 0 => break tok,
            Tok::LParen => depth += 1,
            Tok::RParen => depth -= 1,
            _ => {}
        }
        inner.push(tok);
    };
    Ok((inner, Span::merge(open.span, close.span)))
}

/// The value operand standing to the RIGHT of an operator: one literal,
/// one name, or one parenthesised expression. An operator's operand is one
/// token's worth, so `f⍣N+1` reads `N` and leaves the `+1` to the sentence.
///
/// None where the next token is none of those, which is the caller's cue
/// to report what its own operator wanted there.
fn take_value_operand(
    it: &mut std::iter::Peekable<std::vec::IntoIter<Token>>,
    d: Rules,
) -> Result<Option<(ValueOperand, Span)>> {
    match it.peek().map(|t| &t.kind) {
        Some(k) if literal(k).is_some() => {
            let tok = it.next().expect("peeked");
            let a = literal(&tok.kind).expect("checked above").clone();
            Ok(Some((ValueOperand::Now(a), tok.span)))
        }
        Some(Tok::Name(_)) => {
            let tok = it.next().expect("peeked");
            let Tok::Name(n) = tok.kind else { unreachable!("peeked a name") };
            Ok(Some((ValueOperand::Later(Expr::Name(n, tok.span)), tok.span)))
        }
        Some(Tok::LParen) => {
            let (inner, span) = paren_run(it)?;
            if inner.is_empty() {
                return Ok(None);
            }
            let folded = fold_operators(inner, d)?;
            // A parenthesised run of FUNCTIONS is a function operand and
            // not a value at all. This pass reaches it before the `)` has
            // closed over it, so there is no one function here yet to give
            // the operator — a parser ordering gap, named as one.
            if folded.iter().all(|t| matches!(t.kind, Tok::Func(_))) {
                return Err(Error::not_yet(
                    "a function operand in parentheses, whose ) has not closed yet",
                    span,
                ));
            }
            let toks = fold_axes(folded, d)?;
            let e = parse_range(&toks, 0, toks.len(), span, d)?;
            Ok(Some((settle(e, d), span)))
        }
        _ => Ok(None),
    }
}

/// The value operand standing to the LEFT of an operator, taken off the
/// tokens already folded. The same three shapes `take_value_operand`
/// reads, and a parenthesised group whose `)` the folding pass has already
/// closed over.
fn take_left_value_operand(
    out: &mut Vec<Token>,
    d: Rules,
) -> Result<Option<(ValueOperand, Span)>> {
    match out.last().map(|t| &t.kind) {
        Some(k) if literal(k).is_some() => {
            let tok = out.pop().expect("checked above");
            let a = literal(&tok.kind).expect("checked above").clone();
            Ok(Some((ValueOperand::Now(a), tok.span)))
        }
        Some(Tok::Name(_)) => {
            let tok = out.pop().expect("checked above");
            let Tok::Name(n) = tok.kind else { unreachable!("checked above") };
            Ok(Some((ValueOperand::Later(Expr::Name(n, tok.span)), tok.span)))
        }
        Some(Tok::RParen) => {
            let close = out.len() - 1;
            let Some(open) = matching_lparen(out, close) else { return Ok(None) };
            let span = Span::merge(out[open].span, out[close].span);
            let inner: Vec<Token> = out[open + 1..close].to_vec();
            if inner.is_empty() {
                return Ok(None);
            }
            let e = parse_prepared(&inner, span, d)?;
            out.truncate(open);
            Ok(Some((settle(e, d), span)))
        }
        _ => Ok(None),
    }
}

/// `f[k]` where `f` is a plain function rather than a derived one.
fn fold_axes(toks: Vec<Token>, d: Rules) -> Result<Vec<Token>> {
    let mut out: Vec<Token> = Vec::new();
    let mut it = toks.into_iter().peekable();
    while let Some(t) = it.next() {
        let Tok::Func(f) = &t.kind else {
            out.push(t);
            continue;
        };
        // `⊢[m]` is not an axis at all: m is a mask that says, position by
        // position, which of the two arguments to take. It may be any
        // expression, so the whole bracket is read and settled here.
        if matches!(f, Verb::Prim(p) if p.name == "⊢")
            && matches!(it.peek().map(|x| &x.kind), Some(Tok::LBracket))
        {
            let (mask, aspan) = bracket_operand(&mut it, d)?;
            let v = mask.derive(
                f.clone(),
                built_choose,
                "(⊢[m])".to_string(),
                aspan,
                d,
            )?;
            out.push(Token { kind: Tok::Func(v), span: Span::merge(t.span, aspan) });
            continue;
        }
        // `⊤[N]` is not an axis either: N is how many digits the answer
        // has, counted from one whatever `⎕IO` is, and `[0]` asks `⊤` to
        // work the width out for itself.
        if matches!(f, Verb::Prim(p) if p.name == "⊤")
            && matches!(it.peek().map(|x| &x.kind), Some(Tok::LBracket))
        {
            let (n, aspan) = bracket_value(&mut it, d, "a computed digit count (⊤[N])")?;
            let width = match n.to_i64_vec().as_deref() {
                Some([w]) if *w >= 0 => *w as u32,
                _ => return Err(Error::parse("⊤[N] takes one width at or above zero", aspan)),
            };
            let p = Prim {
                name: "⊤",
                monad: MonadOp::None,
                dyad: DyadOp::EncodeWidth(width),
                ranks: [RANK_INF, RANK_INF, RANK_INF],
            };
            out.push(Token {
                kind: Tok::Func(Verb::Prim(p)),
                span: Span::merge(t.span, aspan),
            });
            continue;
        }
        // `⌹[K]` is not an axis either: the bracket picks one of a group
        // of functions that have nothing to do with each other, and K is
        // its number whatever `⎕IO` is.
        if matches!(f, Verb::Prim(p) if p.name == "⌹")
            && matches!(it.peek().map(|x| &x.kind), Some(Tok::LBracket))
        {
            let (n, aspan) =
                bracket_value(&mut it, d, "a computed function number (⌹[K])")?;
            let which = match n.to_i64_vec().as_deref() {
                Some([k]) => *k,
                _ => {
                    return Err(Error::parse("⌹[K] takes one whole number", aspan));
                }
            };
            out.push(Token {
                kind: Tok::Func(matrix_divide_group(which, aspan)?),
                span: Span::merge(t.span, aspan),
            });
            continue;
        }
        let Some((spec, aspan)) = take_axis(&mut it, d)? else {
            out.push(t);
            continue;
        };
        let spelling = format!("{}[k]", axis_glyph(f));
        let v = spec.derive(f.clone(), built_axis, spelling, aspan, d)?;
        out.push(Token { kind: Tok::Func(v), span: Span::merge(t.span, aspan) });
    }
    Ok(out)
}

/// The function `⌹[K]` stands for. The group is GNU APL's: `[8]` and `[9]`
/// are polynomial arithmetic on coefficient vectors, and the rest are a
/// factorization and a printed form, each a piece of numerical work in its
/// own right.
fn matrix_divide_group(which: i64, aspan: Span) -> Result<Verb> {
    let prim = |name, dyad| {
        Verb::Prim(Prim {
            name,
            monad: MonadOp::None,
            dyad,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        })
    };
    Ok(match which {
        8 => prim("⌹[8]", DyadOp::PolyMultiply),
        9 => prim("⌹[9]", DyadOp::PolyDivide),
        1 => Err(Error::not_yet("⌹[1], the QR factorization", aspan))?,
        7 => Err(Error::not_yet("⌹[7], a polynomial written out as text", aspan))?,
        other => {
            Err(Error::domain(format!("⌹ has no function numbered {other}"), aspan))?
        }
    })
}

/// The function `f[…]` stands for.
///
/// Where the two glyphs of a pair differ only in which axis they pick, the
/// axis specification settles it and the LEADING form is what runs, because
/// `AlongAxis` has already brought the named axis to the front. Every other
/// function that takes an axis reads it itself, so it is kept as it is;
/// a function that takes none at all is a named gap.
fn axis_verb(f: &Verb, spec: AxisSpec, aspan: Span) -> Result<Verb> {
    if let Some(inner) = leading_axis_form(f) {
        // These pick ONE of a pair of axes, so one whole axis is all the
        // brackets can say.
        if spec.single().is_none() {
            return Err(Error::domain(
                format!("{} takes one whole axis, not {spec}", axis_glyph(f)),
                aspan,
            ));
        }
        return Ok(Verb::AlongAxis(Box::new(inner), spec));
    }
    // A definition whose header names no axis has nowhere to put one. That
    // is a refusal and not a gap: the language has the feature and this
    // definition did not ask for it.
    if let Verb::Explicit(def) = f {
        return Err(Error::domain(
            format!("{} takes no axis: its header names none", def.name),
            aspan,
        ));
    }
    if !reads_axis(f) {
        return Err(Error::not_yet(format!("axis specification for {}", axis_glyph(f)), aspan));
    }
    Ok(Verb::AlongAxis(Box::new(f.clone()), spec))
}

/// `f[k]` once the brackets have a value: the axis is read in `⎕IO` origin
/// and the function that reads it is chosen.
fn built_axis(f: &Verb, a: &Array, span: Span, d: Rules) -> Result<Verb> {
    // An explicit definition does not read the brackets as an axis at all:
    // the value goes into the definition under the name its header gave it
    // (`χ` for a `{…}`), verbatim, and the body decides what it means.
    if let Verb::Explicit(def) = f
        && def.axis.is_some()
    {
        return Ok(Verb::BoundAxis(Box::new(f.clone()), a.clone()));
    }
    axis_verb(f, axis_spec(a, d.origin, span)?, span)
}

/// `f/[k]` and `f\[k]` once the brackets have a value. A reduction and a
/// scan each fold ONE axis, so nothing but a single whole one has a
/// meaning there.
fn built_fold_axis(inner: &Verb, a: &Array, span: Span, d: Rules) -> Result<Verb> {
    let spec = axis_spec(a, d.origin, span)?;
    if spec.single().is_none() {
        return Err(Error::domain(
            format!("{} folds one whole axis, not {spec}", inner.name()),
            span,
        ));
    }
    Ok(Verb::AlongAxis(Box::new(inner.clone()), spec))
}

/// `f⍣n` once the right operand has a value.
fn built_power(f: &Verb, a: &Array, span: Span, _d: Rules) -> Result<Verb> {
    Ok(Verb::PowerN(Box::new(f.clone()), power_spec(a, span)?))
}

/// `⊢[m]` once the mask has a value.
fn built_choose(_f: &Verb, a: &Array, _span: Span, _d: Rules) -> Result<Verb> {
    Ok(Verb::Choose(a.clone()))
}

/// A user-written operator once its LEFT operand has a value.
fn built_alpha(template: &Verb, a: &Array, _span: Span, _d: Rules) -> Result<Verb> {
    let Verb::UserDerived { def, omega, .. } = template else {
        return Err(Error::internal("an ⍺⍺ operand outside a user operator"));
    };
    Ok(Verb::UserDerived {
        def: def.clone(),
        alpha: Operand::Value(Box::new(a.clone())),
        omega: omega.clone(),
    })
}

/// A user-written operator once its RIGHT operand has a value.
fn built_omega(template: &Verb, a: &Array, _span: Span, _d: Rules) -> Result<Verb> {
    let Verb::UserDerived { def, alpha, .. } = template else {
        return Err(Error::internal("a ⍵⍵ operand outside a user operator"));
    };
    Ok(Verb::UserDerived {
        def: def.clone(),
        alpha: alpha.clone(),
        omega: Some(Operand::Value(Box::new(a.clone()))),
    })
}

/// The glyph to name in an axis diagnostic: the primitive under whatever
/// rank the frontend wrapped it in, so `⌽` reads as `⌽` and not as the
/// rank-1 application it is built from.
fn axis_glyph(v: &Verb) -> String {
    match v {
        Verb::Rank(inner, _) => axis_glyph(inner),
        other => other.name(),
    }
}

/// Whether the function reads an axis specification of its own — one it
/// applies rather than one that only says which of a pair of axes to pick.
fn reads_axis(v: &Verb) -> bool {
    let Verb::Prim(p) = v else { return false };
    // A SCALAR function takes an axis dyadically: the argument of lower
    // rank lines up with the named axes of the other. The monadic case has
    // no meaning, and says so when it runs.
    if matches!(p.dyad, DyadOp::Scalar(_)) || matches!(p.monad, MonadOp::Scalar(_)) {
        return true;
    }
    matches!(
        p.monad,
        MonadOp::Ravel | MonadOp::Enclose(_) | MonadOp::Split | MonadOp::Open
    ) || matches!(p.monad, MonadOp::First if p.name == "↑")
        || matches!(
            p.dyad,
            DyadOp::AppendLast
                | DyadOp::AppendLeading
                | DyadOp::Take
                | DyadOp::Drop
                | DyadOp::Squad { .. }
                | DyadOp::PartitionEnclose
                | DyadOp::PartitionCounts
        )
}

/// The function a glyph means once an axis is named — the leading-axis form
/// of the pairs that differ only in which axis they pick. None where libjay
/// has no axis form for the function yet.
fn leading_axis_form(v: &Verb) -> Option<Verb> {
    match v {
        // `⌽` is `⊖` applied to rows; with an axis given the two agree.
        Verb::Rank(inner, r) if *r == [1, RANK_INF, RANK_INF] => leading_axis_form(inner),
        // `AlongAxis` brings the named axis to the front, so the form under
        // it always rotates the LEADING one, whichever glyph was written.
        Verb::Prim(p) if matches!(p.monad, MonadOp::Reverse) => {
            let mut p = *p;
            if matches!(p.dyad, DyadOp::RotateApl { .. }) {
                p.dyad = DyadOp::RotateApl { last: false };
            }
            Some(Verb::Prim(p))
        }
        // Replicate and expand are one primitive each, applied at the rank
        // that picks the axis: `/` and `\\` take the last, `⌿` and `⍀` the
        // leading one. With an axis named the leading form is the one that
        // runs, because `AlongAxis` has already brought that axis to the
        // front.
        Verb::Prim(p) if matches!(p.dyad, DyadOp::Copy | DyadOp::Expand) => {
            let mut p = *p;
            p.ranks = [RANK_INF, 1, RANK_INF];
            Some(Verb::Prim(p))
        }
        _ => None,
    }
}

/// One bracket slot: axis `axis` of the right argument selected by the left.
fn select_axis_verb(axis: usize, rank: usize, d: Rules) -> Verb {
    Verb::Prim(Prim {
        name: "[…]",
        monad: MonadOp::None,
        dyad: DyadOp::SelectAxis { axis, rank, origin: d.origin },
        ranks: [RANK_INF; 3],
    })
}

/// `f⍣n`: one integer atom. APL spells convergence `f⍣≡`, a function right
/// operand, which is a separate gap. A NEGATIVE count runs f's inverse that
/// many times.
fn power_spec(a: &Array, span: Span) -> Result<Power> {
    let ints = a
        .to_i64_vec()
        .ok_or_else(|| Error::parse("⍣ needs a whole number on its right", span))?;
    let [n] = ints[..] else {
        return Err(Error::not_yet("power over a list of counts (f⍣n)", span));
    };
    Ok(if n < 0 { Power::Inverse(n.unsigned_abs()) } else { Power::Times(n as u64) })
}

/// `⍤` rank specification: `n` → [n,n,n]; `a b` → [b,a,b]; `a b c` → [a,b,c].
fn rank_spec(a: &Array, span: Span) -> Result<[i64; 3]> {
    let ints = a
        .to_i64_vec()
        .ok_or_else(|| Error::parse("⍤ rank specification must be integers", span))?;
    match ints.len() {
        1 => Ok([ints[0], ints[0], ints[0]]),
        2 => Ok([ints[1], ints[0], ints[1]]),
        3 => Ok([ints[0], ints[1], ints[2]]),
        _ => Err(Error::parse("⍤ rank specification takes 1 to 3 integers", span)),
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse the token range `[lo, hi)` as one expression, right to left.
/// `hint` locates errors when the range is empty.
fn parse_range(toks: &[Token], lo: usize, hi: usize, hint: Span, d: Rules) -> Result<Expr> {
    let (mut acc, mut start) = parse_operand(toks, lo, hi, hint, d)?;
    let end = toks[hi - 1].span.end;
    loop {
        if start == lo {
            return Ok(acc);
        }
        let left = &toks[start - 1];
        match &left.kind {
            Tok::Func(f) => {
                // Dyadic exactly when an operand ends to the left of `f`.
                let dyadic = start >= lo + 2 && is_operand_end(&toks[start - 2].kind);
                if dyadic {
                    let (x, xstart) = parse_operand(toks, lo, start - 1, left.span, d)?;
                    acc = Expr::Dyad {
                        verb: f.clone(),
                        x: Box::new(x),
                        y: Box::new(acc),
                        span: Span::new(toks[xstart].span.start, end),
                    };
                    start = xstart;
                } else {
                    acc = Expr::Monad {
                        verb: f.clone(),
                        y: Box::new(acc),
                        span: Span::new(left.span.start, end),
                    };
                    start -= 1;
                }
            }
            Tok::Assign => {
                if start < lo + 2 {
                    return Err(Error::parse("assignment target must be a name", left.span));
                }
                let target = &toks[start - 2];
                let span = Span::new(target.span.start, end);
                match &target.kind {
                    Tok::Name(n) => {
                        // A system variable is the run's, not a
                        // definition's: setting `⎕PP` inside one holds
                        // after it returns, as it does in a session.
                        let scope = if crate::sysvar::is_settable(n) {
                            Scope::Global
                        } else {
                            Scope::Local
                        };
                        acc = Expr::Assign {
                            name: n.clone(),
                            value: Box::new(acc),
                            scope,
                            span,
                        };
                    }
                    Tok::Quad { quote } => {
                        acc = Expr::PrintPass { value: Box::new(acc), bare: *quote, span };
                    }
                    // `A[i;j]←v`: the one assignment that writes through a
                    // bracket. It is an expression like any other — its
                    // value is the value assigned — so it may stand inside
                    // a larger sentence: `2+A[1]←9` is 11.
                    Tok::RBracket => {
                        let close = start - 2;
                        let open = match_lbracket(toks, lo, close)?;
                        if open == lo {
                            return Err(Error::parse(
                                "[ needs a value on its left",
                                toks[open].span,
                            ));
                        }
                        let Tok::Name(n) = &toks[open - 1].kind else {
                            return Err(Error::not_yet(
                                "indexed assignment through an expression",
                                Span::new(toks[open - 1].span.start, end),
                            ));
                        };
                        let ranges = index_slots(toks, open + 1, close, toks[open].span)?;
                        let mut slots = Vec::with_capacity(ranges.len());
                        for slot in &ranges {
                            slots.push(match *slot {
                                None => None,
                                Some((slo, shi)) => {
                                    Some(parse_range(toks, slo, shi, toks[open].span, d)?)
                                }
                            });
                        }
                        acc = Expr::AmendIndex {
                            name: n.clone(),
                            slots,
                            value: Box::new(acc),
                            origin: d.origin,
                            scope: Scope::Local,
                            span: Span::new(toks[open - 1].span.start, end),
                        };
                        // The `start -= 2` below lands on the name.
                        start = open + 1;
                    }
                    // `(a b)←values`: the names in the brackets share the
                    // value out between them.
                    Tok::RParen => {
                        let l = match_lparen(toks, lo, start - 2)?;
                        let names = target_names(&toks[l + 1..start - 2], target.span)?;
                        let span = Span::new(toks[l].span.start, end);
                        acc = Expr::AssignMany {
                            names,
                            value: Box::new(acc),
                            scope: Scope::Local,
                            by_items: false,
                            span,
                        };
                        start = l + 2;
                    }
                    _ => {
                        return Err(Error::parse(
                            "assignment target must be a name",
                            target.span,
                        ));
                    }
                }
                start -= 2;
                // Specification binds tighter than a strand item, so an
                // operand still standing to the left makes the whole
                // assignment ONE item of a vector: `3 V←1 2` is the pair
                // `3` and `1 2`.
                if start > lo && is_operand_end(&toks[start - 1].kind) {
                    let (e, s) = strand_onto(acc, toks, lo, start, end, d)?;
                    acc = e;
                    start = s;
                }
            }
            // Adjacent operands are vector notation, which `parse_operand`
            // has already taken as one operand by the time we get here.
            _ => break,
        }
    }
    let span = Span::new(toks[lo].span.start, toks[start - 1].span.end);
    // A run of functions is a train, and a train is a function: standing
    // where a value belongs, it is missing its argument. Parenthesised, it
    // has already become one function by the time the parser sees it.
    if d.trains && toks[lo..start].iter().all(|t| matches!(t.kind, Tok::Func(_))) {
        return Err(Error::parse(
            "a train is a function; parenthesise it to apply it to an argument",
            span,
        ));
    }
    Err(Error::parse("syntax error", span))
}

/// Parse the operand ending at `hi - 1`, vector notation included.
///
/// Juxtaposed operands are the items of one vector: every primary
/// contributes one item, except a run of numeric literals, whose numbers
/// are items of their own — which is why `1 2 (3 4)` has three items and
/// `'ab' 'cd'` has two.
fn parse_operand(
    toks: &[Token],
    lo: usize,
    hi: usize,
    hint: Span,
    d: Rules,
) -> Result<(Expr, usize)> {
    let (first, mut start) = parse_primary(toks, lo, hi, hint, d)?;
    if start == lo || !is_operand_end(&toks[start - 1].kind) {
        return Ok((first, start));
    }
    let mut items: Vec<Expr> = Vec::new();
    let mut cur = first;
    loop {
        push_items(&mut items, cur, &toks[start]);
        if start == lo || !is_operand_end(&toks[start - 1].kind) {
            break;
        }
        let (e, s) = parse_primary(toks, lo, start, toks[start - 1].span, d)?;
        cur = e;
        start = s;
    }
    let span = Span::new(toks[start].span.start, toks[hi - 1].span.end);
    let mut it = items.into_iter();
    let last = it.next().expect("a strand has at least one item");
    let mut acc = Expr::Monad { verb: strand_seed(d), y: Box::new(last), span };
    for item in it {
        acc = Expr::Dyad { verb: strand_verb(), x: Box::new(item), y: Box::new(acc), span };
    }
    Ok((acc, start))
}

/// `first` as the LAST item of a strand whose earlier items stand in
/// `toks[lo..start]`. The answer is the whole strand and where it begins.
fn strand_onto(
    first: Expr,
    toks: &[Token],
    lo: usize,
    start: usize,
    end: usize,
    d: Rules,
) -> Result<(Expr, usize)> {
    let mut items: Vec<Expr> = vec![first];
    let mut s = start;
    while s > lo && is_operand_end(&toks[s - 1].kind) {
        let (e, ns) = parse_primary(toks, lo, s, toks[s - 1].span, d)?;
        push_items(&mut items, e, &toks[ns]);
        s = ns;
    }
    let span = Span::new(toks[s].span.start, end);
    let mut it = items.into_iter();
    let last = it.next().expect("a strand has at least one item");
    let mut acc = Expr::Monad { verb: strand_seed(d), y: Box::new(last), span };
    for item in it {
        acc = Expr::Dyad { verb: strand_verb(), x: Box::new(item), y: Box::new(acc), span };
    }
    Ok((acc, s))
}

/// The items one primary contributes to a strand, appended right to left.
fn push_items(items: &mut Vec<Expr>, e: Expr, tok: &Token) {
    if let Tok::Nums(a) = &tok.kind && a.rank() > 0 {
        for i in (0..a.count()).rev() {
            let atom = Array::new(Vec::new(), a.data.slice(i, i + 1));
            items.push(Expr::Const(atom, tok.span));
        }
        return;
    }
    items.push(e);
}

/// `,⊂y`: the one-item vector a single operand makes — flat when the
/// operand is a simple scalar, nested when it is anything else.
fn strand_seed(d: Rules) -> Verb {
    Verb::Atop(
        Box::new(Verb::Prim(prim_for(',', d).expect("`,` is a primitive"))),
        Box::new(Verb::Prim(prim_for('⊂', d).expect("`⊂` is a primitive"))),
        AtopForm::At,
    )
}

/// `x` prepended to the strand `y` as one more item.
fn strand_verb() -> Verb {
    Verb::Prim(Prim {
        name: "(vector notation)",
        monad: MonadOp::None,
        dyad: DyadOp::Strand,
        ranks: [RANK_INF; 3],
    })
}

/// Parse the single operand ending at `hi - 1`. Returns the expression and
/// the index of its first token.
fn parse_primary(
    toks: &[Token],
    lo: usize,
    hi: usize,
    hint: Span,
    d: Rules,
) -> Result<(Expr, usize)> {
    if hi == lo {
        return Err(Error::parse("empty parentheses", hint));
    }
    let t = &toks[hi - 1];
    match &t.kind {
        Tok::Value(a) | Tok::Nums(a) => Ok((Expr::Const(a.clone(), t.span), hi - 1)),
        Tok::Param(i) => Ok((Expr::Param(*i, t.span), hi - 1)),
        Tok::Name(n) => Ok((Expr::Name(n.clone(), t.span), hi - 1)),
        // Naming a niladic definition runs it; the argument it is handed
        // is the empty one its body cannot reach.
        Tok::Niladic(v) => Ok((
            Expr::Monad {
                verb: v.clone(),
                y: Box::new(Expr::Const(Array::empty(crate::dtype::DType::I64), t.span)),
                span: t.span,
            },
            hi - 1,
        )),
        Tok::RParen => {
            let l = match_lparen(toks, lo, hi - 1)?;
            let hint = Span::merge(toks[l].span, t.span);
            let inner = parse_range(toks, l + 1, hi - 1, hint, d)?;
            Ok((inner, l))
        }
        Tok::RBracket => index_brackets(toks, lo, hi, d),
        // `F←+/` names a function. A whole sentence that does so is settled
        // in `parse_statement`; reaching here means the assignment is
        // nested inside a larger sentence, which names nothing.
        Tok::Func(_) if hi >= lo + 2 && matches!(toks[hi - 2].kind, Tok::Assign) => {
            let from = if hi >= lo + 3 { toks[hi - 3].span } else { toks[hi - 2].span };
            let span = Span::merge(from, t.span);
            if d.trains {
                Err(Error::not_yet("naming a function inside a larger sentence", span))
            } else {
                Err(Error::not_yet("function assignment (F←+/)", span))
            }
        }
        Tok::Func(_) => Err(Error::parse("missing right argument", t.span)),
        Tok::Assign => Err(Error::parse("← needs a value on its right", t.span)),
        // `⍞` is the line itself, `⎕` the value the line evaluates to.
        Tok::Quad { quote } => Ok((Expr::Input { eval: !*quote, span: t.span }, hi - 1)),
        Tok::LParen => Err(Error::parse("unmatched (", t.span)),
        Tok::LBracket => Err(Error::parse("unmatched [", t.span)),
        Tok::Semi => Err(Error::parse("; is only meaningful inside index brackets", t.span)),
        Tok::Colon => Err(Error::parse(": is only meaningful in a dfn guard", t.span)),
        Tok::UserOp { .. } => Err(Error::parse(
            "this dfn mentions ⍺⍺ or ⍵⍵, so it is an operator and needs a function operand",
            t.span,
        )),
        Tok::Arrow => Err(Error::parse(
            "→ branches, and only a line of a ∇ definition may begin with it",
            t.span,
        )),
        Tok::CondThen | Tok::CondElse | Tok::CondEnd => Err(Error::parse(
            "→→, ←→ and ←← mark a conditional, and this one is not whole",
            t.span,
        )),
        Tok::Del => Err(Error::parse("∇ opens a definition; it is not a value", t.span)),
        Tok::Control(w) => Err(Error::parse(
            format!(":{w} is only meaningful inside a ∇ definition"),
            t.span,
        )),
        Tok::LBrace | Tok::RBrace => Err(Error::parse("unmatched {", t.span)),
        Tok::Separator => Err(Error::internal("a statement break survived folding")),
        Tok::Op(_) => Err(Error::internal("operator survived folding")),
        // The sentence parser never sees a `⎕FX` that libjay could fix:
        // one is rewritten away before the sentence is parsed. What is
        // left is a `⎕FX` inside another definition, whose body is not
        // compiled until it is called.
        Tok::QuadFx => Err(Error::not_yet("⎕FX inside another definition", t.span)),
    }
}

/// `A[i;j]`: one slot per axis, an empty slot meaning the whole axis.
///
/// The slots are applied from the last axis to the first, so a scalar slot
/// dropping its axis leaves the axes still to come where they were. The
/// slot that sees the whole array — the last one applied — carries the
/// check that there is one slot per axis.
fn index_brackets(
    toks: &[Token],
    lo: usize,
    hi: usize,
    d: Rules,
) -> Result<(Expr, usize)> {
    let close = &toks[hi - 1];
    let open = match_lbracket(toks, lo, hi - 1)?;
    if open == lo || !is_operand_end(&toks[open - 1].kind) {
        return Err(Error::parse("[ needs a value on its left", toks[open].span));
    }
    let (base, start) = parse_primary(toks, lo, open, toks[open].span, d)?;
    let slots = index_slots(toks, open + 1, hi - 1, toks[open].span)?;
    let span = Span::new(toks[start].span.start, close.span.end);
    let rank = slots.len();
    let mut acc = base;
    let mut first = true;
    for (axis, slot) in slots.iter().enumerate().rev() {
        let Some((slo, shi)) = *slot else { continue };
        let idx = parse_range(toks, slo, shi, toks[open].span, d)?;
        let check = if first { rank } else { 0 };
        first = false;
        acc = Expr::Dyad {
            verb: select_axis_verb(axis, check, d),
            x: Box::new(idx),
            y: Box::new(acc),
            span,
        };
    }
    Ok((acc, start))
}

/// The token ranges of the slots between `[` and `]`, in axis order. None
/// is an elided slot, which selects the whole axis.
fn index_slots(
    toks: &[Token],
    lo: usize,
    hi: usize,
    hint: Span,
) -> Result<Vec<Option<(usize, usize)>>> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = lo;
    for (i, t) in toks.iter().enumerate().take(hi).skip(lo) {
        match t.kind {
            Tok::LParen | Tok::LBracket => depth += 1,
            Tok::RParen | Tok::RBracket => depth -= 1,
            Tok::Semi if depth == 0 => {
                out.push((start < i).then_some((start, i)));
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push((start < hi).then_some((start, hi)));
    if out.len() == 1 && out[0].is_none() {
        return Err(Error::parse("empty index brackets", hint));
    }
    Ok(out)
}

fn match_lbracket(toks: &[Token], lo: usize, rbracket: usize) -> Result<usize> {
    let mut depth = 0usize;
    let mut i = rbracket;
    while i > lo {
        i -= 1;
        match toks[i].kind {
            Tok::RBracket => depth += 1,
            Tok::LBracket => {
                if depth == 0 {
                    return Ok(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    Err(Error::parse("unmatched ]", toks[rbracket].span))
}

fn match_lparen(toks: &[Token], lo: usize, rparen: usize) -> Result<usize> {
    let mut depth = 0usize;
    let mut i = rparen;
    while i > lo {
        i -= 1;
        match toks[i].kind {
            Tok::RParen => depth += 1,
            Tok::LParen => {
                if depth == 0 {
                    return Ok(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    Err(Error::parse("unmatched )", toks[rparen].span))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Explicit definitions
// ---------------------------------------------------------------------------

/// APL's control words, without their colon. `:End` closes any of the
/// structures, which is the spelling GNU APL's manual gives as an
/// alternative to the named closers.
const CONTROL_WORDS: [&str; 21] = [
    "If", "ElseIf", "Else", "EndIf", "AndIf", "OrIf", "While", "EndWhile", "Repeat", "Until",
    "For", "In", "EndFor", "Select", "Case", "CaseList", "EndSelect", "Return", "Leave",
    "Continue", "End",
];

/// The control word a `:name` spells, case-insensitively as the references
/// accept it.
fn control_word(word: &str) -> Option<&'static str> {
    CONTROL_WORDS.iter().copied().find(|w| w.eq_ignore_ascii_case(word))
}

/// The index of the token matching the bracket that opens at `open`.
fn match_close(toks: &[Token], open: usize, opener: &Tok, closer: &Tok) -> Option<usize> {
    let same = |a: &Tok, b: &Tok| std::mem::discriminant(a) == std::mem::discriminant(b);
    let mut depth = 0usize;
    for (i, t) in toks.iter().enumerate().skip(open) {
        if same(&t.kind, opener) {
            depth += 1;
        } else if same(&t.kind, closer) {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Replace every `{ … }` in a sentence by the function it defines.
fn fold_dfns(
    toks: Vec<Token>,
    d: Rules,
    verbs: &HashMap<String, Verb>,
) -> Result<Vec<Token>> {
    let Some(open) = toks.iter().position(|t| matches!(t.kind, Tok::LBrace)) else {
        return Ok(toks);
    };
    let close = match_close(&toks, open, &Tok::LBrace, &Tok::RBrace)
        .ok_or_else(|| Error::parse("unmatched {", toks[open].span))?;
    let span = Span::merge(toks[open].span, toks[close].span);
    let mut out: Vec<Token> = toks[..open].to_vec();
    let kind = match build_dfn(&toks[open + 1..close], d, verbs)? {
        Dfn::Func(verb) => Tok::Func(*verb),
        Dfn::Op { def, omega } => Tok::UserOp { def, omega },
    };
    out.push(Token { kind, span });
    out.extend_from_slice(&toks[close + 1..]);
    // A sentence may hold several dfns side by side.
    fold_dfns(out, d, verbs)
}

/// The statements of a dfn body: the runs between the `⋄` and line breaks
/// that belong to this dfn rather than to one nested inside it.
fn split_statements(toks: &[Token]) -> Vec<&[Token]> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, t) in toks.iter().enumerate() {
        match t.kind {
            Tok::LBrace => depth += 1,
            Tok::RBrace => depth = depth.saturating_sub(1),
            Tok::Separator if depth == 0 => {
                out.push(&toks[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&toks[start..]);
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

/// What a `{ … }` defines: a plain function, or an operator still waiting
/// for its operands.
enum Dfn {
    Func(Box<Verb>),
    Op { def: Arc<OpDef>, omega: bool },
}

// The dfns being built on this thread, outermost first, and the counter
// that hands each one its identity.
//
// A dfn's body is parsed by a nested call, so this chain is exactly the
// run of `{` the parser is inside. It lives here rather than in a
// parameter because every step between one `build_dfn` and the next — the
// statement splitter, the guard reader, the sentence parser — would
// otherwise carry it through untouched.
thread_local! {
    static ENCLOSING: std::cell::RefCell<Vec<u64>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static NEXT_DFN_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// `{ … }`: the body's own words decide the valence, and `∇` in it names
/// the dfn itself.
/// The verb a dfn defines, and — when it mentions `⍺⍺` or `⍵⍵` — whether
/// it is an operator wanting a right operand as well as a left one.
fn build_dfn(body: &[Token], d: Rules, verbs: &HashMap<String, Verb>) -> Result<Dfn> {
    let mut depth = 0usize;
    let mut dyadic = false;
    let mut alpha_op = false;
    let mut omega_op = false;
    for t in body {
        match &t.kind {
            Tok::LBrace => depth += 1,
            Tok::RBrace => depth = depth.saturating_sub(1),
            Tok::Name(n) if depth == 0 && n == "⍺" => dyadic = true,
            Tok::Name(n) if depth == 0 && n == "⍺⍺" => alpha_op = true,
            Tok::Name(n) if depth == 0 && n == "⍵⍵" => omega_op = true,
            _ => {}
        }
    }
    // An operand name is a FUNCTION inside the body unless this reading
    // has an array standing there, in which case it is an ordinary name
    // and the body's `⍺⍺+⍵` is a sum rather than a train.
    let reading = |alpha_value: bool, omega_value: bool| -> Result<Verb> {
        let mut inner = verbs.clone();
        if alpha_op && !alpha_value {
            inner.insert("⍺⍺".to_string(), Verb::Named("⍺⍺".to_string()));
        }
        if (alpha_op || omega_op) && !omega_value {
            inner.insert("⍵⍵".to_string(), Verb::Named("⍵⍵".to_string()));
        }
        // The body is parsed with this dfn on the enclosing chain, so that
        // a dfn written inside it records this one as a lexical parent.
        let id = NEXT_DFN_ID.with(|c| {
            let id = c.get();
            c.set(id.wrapping_add(1));
            id
        });
        let enclosing = ENCLOSING.with(|e| e.borrow().clone());
        ENCLOSING.with(|e| e.borrow_mut().push(id));
        let parsed = parse_dfn_body(body, d, &mut inner);
        ENCLOSING.with(|e| {
            e.borrow_mut().pop();
        });
        let mut stmts = parsed?;
        // The body is a sequence, and the dialect says which of its
        // sentences is the answer. libjay's block model gives the last
        // one; the other reading stops at the first sentence that is not
        // an assignment.
        match d.dfn_result {
            DfnResult::LastSentence => {}
            DfnResult::FirstNonAssignment => {
                // The sentences passed over are the ones whose value is
                // shy — an assignment in any of its spellings, to a name,
                // to a part of one, or to `⎕` — and the ones that are not
                // an expression at all: a guard, which is a control
                // structure that returns when it holds, and a definition.
                // `⎕←` belongs to the first group because it assigns; a
                // body that prints and then computes goes on to compute.
                let plain = |e: &Expr| {
                    !e.is_shy()
                        && !matches!(
                            e,
                            Expr::Control(..) | Expr::VerbDef { .. } | Expr::ModDef { .. }
                        )
                };
                if let Some(k) = stmts.iter().position(plain) {
                    stmts.truncate(k + 1);
                }
            }
        }
        let pure = stmts.iter().all(is_pure_stmt);
        Ok(Verb::Explicit(Arc::new(ExplicitDef {
            name: "{…}".to_string(),
            left: dyadic.then(|| "⍺".to_string()),
            right: "⍵".to_string(),
            // A dfn runs in either valence; a monadic call simply leaves
            // `⍺` without a value, unless `⍺←` gives it one, and a left
            // argument it has no name for is dropped rather than refused.
            dyad_only: false,
            spare_left: true,
            result: None,
            locals: Vec::new(),
            // `{…}` reads an axis written at the call site under `χ`.
            axis: Some("χ".to_string()),
            body: stmts,
            // A dfn that reaches its end without a value has no result.
            empty: None,
            labels: Vec::new(),
        home: None,
            lines: Vec::new(),
            enclosing,
            id,
            pure,
            // A dfn is an expression, not a listing of lines.
            source: Vec::new(),
            spelling: None,
            // APL has no explicit-definition representation to give.
            rep: None,
        })))
    };
    if !(alpha_op || omega_op) {
        return Ok(Dfn::Func(Box::new(reading(false, false)?)));
    }
    // Every reading is parsed here, and the operands choose one when they
    // arrive. A reading that does not parse keeps its diagnostic instead of
    // failing the definition: the body only has to make sense under the
    // operands it is actually given.
    let mut readings: [std::result::Result<Verb, String>; 4] =
        [const { Err(String::new()) }; 4];
    for (i, slot) in readings.iter_mut().enumerate() {
        *slot = reading(i & 1 != 0, i & 2 != 0).map_err(|e| e.msg);
    }
    // The reading with function operands throughout is the one every
    // existing spelling uses; if it will not parse, nothing will.
    if let Err(msg) = &readings[0]
        && readings.iter().all(|r| r.is_err())
    {
        return Err(Error::parse(msg.clone(), body.first().map_or(Span::new(0, 0), |t| t.span)));
    }
    Ok(Dfn::Op { def: Arc::new(OpDef { readings }), omega: omega_op })
}

fn parse_dfn_body(
    body: &[Token],
    d: Rules,
    verbs: &mut HashMap<String, Verb>,
) -> Result<Vec<Expr>> {
    let mut stmts = Vec::new();
    for stmt in split_statements(body) {
        // `∇` inside a dfn is the dfn itself.
        let stmt: Vec<Token> = stmt
            .iter()
            .map(|t| match t.kind {
                Tok::Del => Token { kind: Tok::Func(Verb::SelfRef), span: t.span },
                _ => t.clone(),
            })
            .collect();
        stmts.push(parse_guarded(stmt, d, verbs)?);
    }
    Ok(stmts)
}

/// One dfn statement: a guard `cond:expr`, an `⍺←default`, or a sentence.
fn parse_guarded(
    stmt: Vec<Token>,
    d: Rules,
    verbs: &mut HashMap<String, Verb>,
) -> Result<Expr> {
    let mut depth = 0usize;
    let mut colon = None;
    for (i, t) in stmt.iter().enumerate() {
        match t.kind {
            Tok::LBrace | Tok::LParen | Tok::LBracket => depth += 1,
            Tok::RBrace | Tok::RParen | Tok::RBracket => depth = depth.saturating_sub(1),
            Tok::Colon if depth == 0 => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    if let Some(k) = colon {
        let span = Span::merge(stmt[0].span, stmt[stmt.len() - 1].span);
        let test = one_statement(stmt[..k].to_vec(), d, verbs, stmt[k].span)?;
        let body = one_statement(stmt[k + 1..].to_vec(), d, verbs, stmt[k].span)?;
        // A guard that holds is the dfn's answer: the value, then out.
        return Ok(Expr::Control(
            Box::new(Control::Guard {
                test: vec![test],
                body: vec![body, Expr::Control(Box::new(Control::Return), span)],
            }),
            span,
        ));
    }
    // `⍺←v` gives the left argument a value only where none arrived. The
    // dialect says whether `v` is evaluated when one did: eagerly, the
    // sentence runs and its value is dropped.
    let default = matches!(
        (stmt.first().map(|t| &t.kind), stmt.get(1).map(|t| &t.kind)),
        (Some(Tok::Name(n)), Some(Tok::Assign)) if n == "⍺"
    );
    let span = stmt.first().map_or(Span::new(0, 0), |t| t.span);
    let e = one_statement(stmt, d, verbs, span)?;
    if default {
        let scope = match d.default_arg {
            DefaultArg::Eager => Scope::LocalDefault,
            DefaultArg::Lazy => return Err(Error::not_yet("a lazy ⍺← default", span)),
        };
        if let Expr::Assign { name, value, span, .. } = e {
            return Ok(Expr::Assign { name, value, scope, span });
        }
    }
    Ok(e)
}

fn one_statement(
    stmt: Vec<Token>,
    d: Rules,
    verbs: &mut HashMap<String, Verb>,
    hint: Span,
) -> Result<Expr> {
    parse_statement(stmt, d, verbs, false)?
        .ok_or_else(|| Error::parse("this needs an expression", hint))
}

/// True when nothing in this sentence can have an effect beyond its value.
fn is_pure_stmt(e: &Expr) -> bool {
    match e {
        Expr::Const(..) | Expr::Param(..) | Expr::Name(..) => true,
        Expr::Monad { verb, y, .. } => verb.is_pure() && is_pure_stmt(y),
        Expr::Dyad { verb, x, y, .. } => verb.is_pure() && is_pure_stmt(x) && is_pure_stmt(y),
        Expr::Assign { value, .. } => is_pure_stmt(value),
        Expr::Control(c, _) => is_pure_control(c),
        _ => false,
    }
}

fn is_pure_control(c: &Control) -> bool {
    let all = |b: &Vec<Expr>| b.iter().all(is_pure_stmt);
    match c {
        Control::Return | Control::Break | Control::Continue => true,
        Control::Label(_) | Control::Goto { .. } | Control::Throw => true,
        Control::Branch(target) => is_pure_stmt(target),
        Control::BranchBy { by, test } => is_pure_stmt(by) && is_pure_stmt(test),
        Control::Cond { test, body, otherwise } => all(test) && all(body) && all(otherwise),
        Control::If { arms, otherwise } => {
            arms.iter().all(|a| a.test.as_ref().is_none_or(all) && all(&a.body))
                && otherwise.as_ref().is_none_or(all)
        }
        Control::While { test, body, .. } => all(test) && all(body),
        Control::For { source, body, .. } => is_pure_stmt(source) && all(body),
        Control::Select { subject, cases } => {
            is_pure_stmt(subject)
                && cases.iter().all(|c| c.test.as_ref().is_none_or(all) && all(&c.body))
        }
        Control::Try { body, catch, catcht } => all(body) && all(catch) && all(catcht),
        Control::Guard { test, body } => all(test) && all(body),
    }
}

// ---------------------------------------------------------------------------
// ∇-definitions and control structures
// ---------------------------------------------------------------------------

/// `∇ Z←L F R;a;b` … `∇`: the multi-line definition form, which is where
/// APL puts its control structures.
fn parse_tradfn(
    sentences: &[Vec<Token>],
    i: &mut usize,
    d: Rules,
    verbs: &mut HashMap<String, Verb>,
    src: &str,
) -> Result<Expr> {
    let header = &sentences[*i];
    let open = header[0].span;
    *i += 1;
    let head = header[1..].to_vec();
    let mut body_lines: Vec<Vec<Token>> = Vec::new();
    loop {
        let Some(line) = sentences.get(*i) else {
            return Err(Error::parse("this definition has no closing ∇", open));
        };
        *i += 1;
        if line.len() == 1 && matches!(line[0].kind, Tok::Del) {
            break;
        }
        body_lines.push(strip_line_number(line));
    }
    // What `⎕CR` hands back. The header starts after the `∇`; a body line
    // keeps the blanks it was indented by, which is what the reference
    // shows.
    let mut source = vec![written_text(src, &head, false)];
    source.extend(body_lines.iter().map(|line| written_text(src, line, true)));
    let close = sentences
        .get(i.saturating_sub(1))
        .and_then(|l| l.first())
        .map_or(open, |t| t.span);
    build_tradfn(&head, &body_lines, Span::merge(open, close), d, verbs, source)
}

/// The source text a run of tokens was written as, without its trailing
/// blanks. With `indent`, the blanks in front of it are kept too, but only
/// where nothing else stands between them and the start of the line — so
/// an indented body line keeps its indent and a line the `∇` editor
/// numbered does not keep the space after the number.
fn written_text(src: &str, toks: &[Token], indent: bool) -> String {
    let (Some(first), Some(last)) = (toks.first(), toks.last()) else {
        return String::new();
    };
    let mut start = first.span.start;
    if indent {
        let head = &src[..start];
        let line_start = head.rfind('\n').map_or(0, |at| at + 1);
        if src[line_start..start].chars().all(|c| c == ' ' || c == '\t') {
            start = line_start;
        }
    }
    let end = last.span.end.min(src.len());
    if start >= end {
        return String::new();
    }
    src[start..end].trim_end().to_string()
}

/// A `∇` body line with the line number the `∇` editor prints in front of
/// it removed. Every printed APL definition — a book, a wiki page, a
/// session listing — carries them, and the reference reads such a listing
/// straight back in, so `[1]` and `[1.1]` are punctuation of the LISTING
/// and not part of the line. A line that only looks like one (`[1] ← 2`
/// cannot start a sentence anyway) keeps its brackets: the number has to
/// be a single non-negative literal and stand at the very front.
/// Whether a `:` standing after these tokens ends a LABEL: the line so far
/// is one name, with at most the `∇` editor's line number in front of it.
fn is_label_head(cur: &[Token]) -> bool {
    let rest = match cur {
        [a, b, c, tail @ ..]
            if matches!(a.kind, Tok::LBracket)
                && matches!(c.kind, Tok::RBracket)
                && matches!(&b.kind, Tok::Nums(n) if n.rank() == 0) =>
        {
            tail
        }
        all => all,
    };
    matches!(rest, [t] if matches!(t.kind, Tok::Name(_)))
}

fn strip_line_number(line: &[Token]) -> Vec<Token> {
    if line.len() > 3
        && matches!(line[0].kind, Tok::LBracket)
        && matches!(line[2].kind, Tok::RBracket)
        && let Tok::Nums(a) = &line[1].kind
        && a.rank() == 0
    {
        return line[3..].to_vec();
    }
    line.to_vec()
}

/// A definition from its header tokens and one token list per body line —
/// the shape both `∇ … ∇` and `⎕FX` reduce to.
fn build_tradfn(
    head: &[Token],
    body_lines: &[Vec<Token>],
    span: Span,
    d: Rules,
    verbs: &mut HashMap<String, Verb>,
    source: Vec<String>,
) -> Result<Expr> {
    let Header { name, left: def_left, right: def_right, result, locals, axis } =
        parse_header(head, span)?;
    // The body can call the function by its own name.
    let mut inner = verbs.clone();
    inner.insert(name.clone(), Verb::Named(name.clone()));
    let mut items = Vec::new();
    // The 1-based body line each item begins at. A conditional written with
    // `→→` spans several lines and is one item, so the two run apart.
    let mut item_lines: Vec<usize> = Vec::new();
    let mut labels: Vec<(String, usize)> = Vec::new();
    let mut at = 0usize;
    while at < body_lines.len() {
        let line_no = at + 1;
        // A conditional's three markers may each end a line, so one spans
        // as many lines as it needs and becomes one statement.
        if body_lines[at].iter().any(|t| matches!(t.kind, Tok::CondThen)) {
            let e = parse_conditional(body_lines, &mut at, d, &mut inner)?;
            items.push(AplItem::Sentence(e));
            item_lines.push(line_no);
            continue;
        }
        let line = &body_lines[at];
        at += 1;
        let mut label = None;
        let item = to_item(line.clone(), d, &mut inner, &mut label)?;
        if let Some(name) = label {
            labels.push((name, line_no));
        }
        items.push(item);
        item_lines.push(line_no);
    }
    let mut cursor = AplCursor { items: &items, at: 0, d, loops: 0 };
    let mut starts = Vec::new();
    let mut body = parse_apl_block_from(&mut cursor, &[], &mut starts)?;
    // A label is the number of a LINE, and a control structure folds
    // several lines into one statement, so the body carries the line each
    // of its statements began at: that is what a `→` reads to find where
    // to go, and a line inside a control structure has no statement of its
    // own to reach.
    let lines: Vec<usize> = starts.iter().map(|&i| item_lines[i]).collect();
    if let Some(item) = cursor.peek() {
        return Err(Error::parse(
            format!(":{} has no matching opening word", item.word().unwrap_or("?")),
            item.span(),
        ));
    }
    // A `∇` definition's names are global unless the header declares them:
    // that is APL's rule, and the reference holds to it.
    let mut own: Vec<String> = locals.clone();
    own.extend(result.clone());
    own.extend(def_left.clone());
    own.extend(axis.clone());
    own.push(def_right.clone());
    for stmt in &mut body {
        set_scopes(stmt, &own);
    }
    let pure = body.iter().all(is_pure_stmt);
    let verb = Verb::Explicit(Arc::new(ExplicitDef {
        name: format!("∇{name}"),
        left: def_left,
        right: def_right,
        dyad_only: false,
        // A `∇` definition binds its arguments by the names its header
        // gives, and refuses one it has no name for. Only a dfn nests
        // lexically inside another.
        spare_left: false,
        enclosing: Vec::new(),
        id: 0,
        result,
        locals,
        axis,
        body,
        empty: None,
        labels,
        home: None,
        lines,
        pure,
        source,
        spelling: None,
        // APL has no explicit-definition representation to give.
        rep: None,
    }));
    verbs.insert(name.clone(), verb.clone());
    Ok(Expr::VerbDef { name, verb, span })
}

/// A `∇` header taken apart: the function's name, its left and right
/// argument names, the name of its result, the names it declares local,
/// and the name an axis written at the call site binds to.
struct Header {
    name: String,
    left: Option<String>,
    right: String,
    result: Option<String>,
    locals: Vec<String>,
    axis: Option<String>,
}

/// `Z←L F[X] R;a;b` and its shorter forms.
fn parse_header(toks: &[Token], span: Span) -> Result<Header> {
    let mut names: Vec<String> = Vec::new();
    let mut locals: Vec<String> = Vec::new();
    let mut result = None;
    let mut axis = None;
    let mut in_locals = false;
    let mut k = 0usize;
    // `Z←` in front names the result.
    if let (Some(Tok::Name(z)), Some(Tok::Assign)) =
        (toks.first().map(|t| &t.kind), toks.get(1).map(|t| &t.kind))
    {
        result = Some(z.clone());
        k = 2;
    }
    while k < toks.len() {
        match &toks[k].kind {
            Tok::Semi => in_locals = true,
            Tok::Name(n) if in_locals => locals.push(n.clone()),
            Tok::Name(n) => names.push(n.clone()),
            // `∇Z←A F[X] B` gives the definition an axis: the brackets
            // name a local the body reads, and a call writes its value in
            // brackets of its own.
            Tok::LBracket => {
                let (Some(Tok::Name(x)), Some(Tok::RBracket)) =
                    (toks.get(k + 1).map(|t| &t.kind), toks.get(k + 2).map(|t| &t.kind))
                else {
                    return Err(Error::parse(
                        "a ∇ header's [X] names one axis",
                        toks[k].span,
                    ));
                };
                if axis.is_some() {
                    return Err(Error::parse(
                        "a ∇ header names one axis at most",
                        toks[k].span,
                    ));
                }
                axis = Some(x.clone());
                k += 2;
            }
            _ => {
                return Err(Error::parse("this is not a ∇ definition header", toks[k].span));
            }
        }
        k += 1;
    }
    let (name, left, right) = match names.len() {
        3 => (names[1].clone(), Some(names[0].clone()), names[2].clone()),
        2 => (names[0].clone(), None, names[1].clone()),
        1 => (names[0].clone(), None, crate::ir::NILADIC.to_string()),
        _ => {
            return Err(Error::parse(
                "a ∇ definition header names a function and its arguments",
                span,
            ));
        }
    };
    Ok(Header { name, left, right, result, locals, axis })
}

/// One line of a `∇` definition: a control word with what follows it, or a
/// sentence already lowered.
enum AplItem {
    Sentence(Expr),
    Word { word: &'static str, rest: Vec<Token>, span: Span },
}

impl AplItem {
    fn word(&self) -> Option<&'static str> {
        match self {
            AplItem::Word { word, .. } => Some(word),
            AplItem::Sentence(_) => None,
        }
    }

    fn span(&self) -> Span {
        match self {
            AplItem::Word { span, .. } => *span,
            AplItem::Sentence(e) => e.span(),
        }
    }
}

/// Where a `→` stands in this line outside every bracket, when one does
/// and something precedes it. That is the dyadic branch; a `→` inside
/// parentheses belongs to the expression around it.
fn top_level_arrow(line: &[Token]) -> Option<usize> {
    let mut depth = 0i32;
    for (i, t) in line.iter().enumerate() {
        match t.kind {
            Tok::LParen | Tok::LBracket | Tok::LBrace => depth += 1,
            Tok::RParen | Tok::RBracket | Tok::RBrace => depth -= 1,
            Tok::Arrow if depth == 0 && i > 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn to_item(
    line: Vec<Token>,
    d: Rules,
    verbs: &mut HashMap<String, Verb>,
    label: &mut Option<String>,
) -> Result<AplItem> {
    let mut line = line;
    // `L:` in front of a line names it, and `→` takes the number of the
    // line a name stands for.
    if let (Some(Tok::Name(n)), Some(Tok::Colon)) =
        (line.first().map(|t| &t.kind), line.get(1).map(|t| &t.kind))
    {
        *label = Some(n.clone());
        line.drain(..2);
    }
    if let Some(Tok::Control(word)) = line.first().map(|t| &t.kind) {
        let word = *word;
        let span = line[0].span;
        return Ok(AplItem::Word { word, rest: line[1..].to_vec(), span });
    }
    if matches!(line.first().map(|t| &t.kind), Some(Tok::Arrow)) {
        let span = line[0].span;
        let target = parse_statement(line[1..].to_vec(), d, verbs, true)?
            .ok_or_else(|| Error::parse("→ needs a line to branch to", span))?;
        let span = Span::merge(span, target.span());
        return Ok(AplItem::Sentence(Expr::Control(
            Box::new(Control::Branch(Box::new(target))),
            span,
        )));
    }
    // `A→B` is the relative branch: A lines on from here when B holds.
    if let Some(at) = top_level_arrow(&line) {
        let span = line[at].span;
        let by = parse_statement(line[..at].to_vec(), d, verbs, true)?
            .ok_or_else(|| Error::parse("→ needs a line count on its left", span))?;
        let test = parse_statement(line[at + 1..].to_vec(), d, verbs, true)?
            .ok_or_else(|| Error::parse("→ needs a condition on its right", span))?;
        let span = Span::merge(by.span(), test.span());
        return Ok(AplItem::Sentence(Expr::Control(
            Box::new(Control::BranchBy { by: Box::new(by), test: Box::new(test) }),
            span,
        )));
    }
    // A labelled line may hold nothing else; the label is then a place to
    // branch to and the line does no work of its own. `→⍬` is exactly that
    // line: a branch with no target falls through and yields nothing.
    if line.is_empty() {
        let span = label.as_ref().map_or(Span::new(0, 0), |_| Span::new(0, 0));
        let nowhere = Expr::Const(Array::empty(crate::dtype::DType::I64), span);
        return Ok(AplItem::Sentence(Expr::Control(
            Box::new(Control::Branch(Box::new(nowhere))),
            span,
        )));
    }
    let span = line.first().map_or(Span::new(0, 0), |t| t.span);
    let e = parse_statement(line, d, verbs, true)?
        .ok_or_else(|| Error::parse("this line has no sentence", span))?;
    Ok(AplItem::Sentence(e))
}

struct AplCursor<'a> {
    items: &'a [AplItem],
    at: usize,
    /// The dialect, for the sentences a control word carries.
    d: Rules,
    /// How many loops enclose the position being parsed, which is what
    /// decides whether a `:Leave` has one to leave.
    loops: usize,
}

impl<'a> AplCursor<'a> {
    fn peek(&self) -> Option<&'a AplItem> {
        self.items.get(self.at)
    }

    fn peek_word(&self) -> Option<&'static str> {
        self.peek().and_then(AplItem::word)
    }

    fn last_span(&self) -> Span {
        self.items
            .get(self.at.saturating_sub(1))
            .map_or_else(|| Span::new(0, 0), AplItem::span)
    }

    /// Parse a loop's body, with the loop counted around it.
    fn in_loop<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.loops += 1;
        let out = f(self);
        self.loops -= 1;
        out
    }

    /// Consume the closing word, which may also be spelled `:End`.
    fn close(&mut self, want: &str) -> Result<()> {
        match self.peek_word() {
            Some(w) if w == want || w == "End" => {
                self.at += 1;
                Ok(())
            }
            Some(w) => Err(Error::parse(
                format!("expected :{want} here, not :{w}"),
                self.peek().expect("a word").span(),
            )),
            None => Err(Error::parse(format!("this block needs a :{want}"), self.last_span())),
        }
    }
}

fn parse_apl_block(cur: &mut AplCursor<'_>, stop: &[&str]) -> Result<Vec<Expr>> {
    parse_apl_block_from(cur, stop, &mut Vec::new())
}

/// [`parse_apl_block`], also recording which item each statement of the
/// block began at. A `∇` definition's labels are LINE numbers, and a
/// control structure folds several lines into one statement, so the
/// definition's top level needs the mapping back.
fn parse_apl_block_from(
    cur: &mut AplCursor<'_>,
    stop: &[&str],
    starts: &mut Vec<usize>,
) -> Result<Vec<Expr>> {
    let mut out = Vec::new();
    loop {
        match cur.peek() {
            None => return Ok(out),
            Some(AplItem::Word { word, .. }) if stop.contains(word) || *word == "End" => {
                return Ok(out);
            }
            Some(AplItem::Sentence(e)) => {
                starts.push(cur.at);
                cur.at += 1;
                out.push(e.clone());
            }
            Some(AplItem::Word { .. }) => {
                starts.push(cur.at);
                out.push(parse_apl_control(cur)?);
            }
        }
    }
}

fn parse_apl_control(cur: &mut AplCursor<'_>) -> Result<Expr> {
    let Some(AplItem::Word { word, rest, span }) = cur.peek() else {
        return Err(Error::internal("expected a control word"));
    };
    let (word, rest, start) = (*word, rest.clone(), *span);
    cur.at += 1;
    let control = match word {
        "If" => {
            let mut arms = Vec::new();
            let mut otherwise = None;
            let mut test = rest;
            loop {
                let test_expr = condition(test, start, cur.d)?;
                let test_expr = continued_condition(cur, test_expr)?;
                let body = parse_apl_block(cur, &["ElseIf", "Else", "EndIf"])?;
                arms.push(Branch {
                    test: Some(vec![test_expr]),
                    body,
                    fall_through: false,
                    list: false,
                });
                match cur.peek_word() {
                    Some("ElseIf") => {
                        let Some(AplItem::Word { rest, .. }) = cur.peek() else { unreachable!() };
                        test = rest.clone();
                        cur.at += 1;
                    }
                    Some("Else") => {
                        cur.at += 1;
                        otherwise = Some(parse_apl_block(cur, &["EndIf"])?);
                        cur.close("EndIf")?;
                        break;
                    }
                    _ => {
                        cur.close("EndIf")?;
                        break;
                    }
                }
            }
            Control::If { arms, otherwise }
        }
        "While" => {
            let test = condition(rest, start, cur.d)?;
            let test = continued_condition(cur, test)?;
            let body = cur.in_loop(|cur| parse_apl_block(cur, &["EndWhile"]))?;
            cur.close("EndWhile")?;
            Control::While { test: vec![test], body, body_first: false, until: false }
        }
        "Repeat" => {
            if !rest.is_empty() {
                return Err(Error::parse(":Repeat takes no condition", start));
            }
            let body = cur.in_loop(|cur| parse_apl_block(cur, &["Until"]))?;
            let Some(AplItem::Word { rest, span, .. }) = cur.peek() else {
                return Err(Error::parse("this :Repeat needs an :Until", cur.last_span()));
            };
            let (rest, span) = (rest.clone(), *span);
            let test = condition(rest, span, cur.d)?;
            cur.at += 1;
            let test = continued_condition(cur, test)?;
            Control::While { test: vec![test], body, body_first: true, until: true }
        }
        "For" => {
            // `:For name… :In source`.
            let (names, source) = for_header(&rest, start, cur.d)?;
            let body = cur.in_loop(|cur| parse_apl_block(cur, &["EndFor"]))?;
            cur.close("EndFor")?;
            Control::For { names, source: Box::new(source), body }
        }
        "Select" => {
            let subject = condition(rest, start, cur.d)?;
            let mut cases = Vec::new();
            loop {
                match cur.peek() {
                    Some(AplItem::Word { word: w @ ("Case" | "CaseList"), rest, span }) => {
                        let list = *w == "CaseList";
                        let test = condition(rest.clone(), *span, cur.d)?;
                        cur.at += 1;
                        let body =
                            parse_apl_block(cur, &["Case", "CaseList", "Else", "EndSelect"])?;
                        cases.push(Branch {
                            test: Some(vec![test]),
                            body,
                            fall_through: false,
                            list,
                        });
                    }
                    Some(AplItem::Word { word: "Else", .. }) => {
                        cur.at += 1;
                        let body = parse_apl_block(cur, &["EndSelect"])?;
                        cases.push(Branch {
                            test: None,
                            body,
                            fall_through: false,
                            list: false,
                        });
                        cur.close("EndSelect")?;
                        break;
                    }
                    _ => {
                        cur.close("EndSelect")?;
                        break;
                    }
                }
            }
            Control::Select { subject: Box::new(subject), cases }
        }
        "Return" => Control::Return,
        // Dyalog wants a loop around `:Leave` and `:Continue`; the lenient
        // reading lets a stray one leave the definition instead.
        "Leave" | "Continue" => {
            if cur.loops == 0 && cur.d.control_strictness == ControlStrictness::Strict {
                return Err(Error::parse(format!(":{word} belongs inside a loop"), start));
            }
            if word == "Leave" { Control::Break } else { Control::Continue }
        }
        other => {
            return Err(Error::parse(format!(":{other} has no matching opening word"), start));
        }
    };
    Ok(Expr::Control(Box::new(control), Span::merge(start, cur.last_span())))
}

/// The `:AndIf` and `:OrIf` lines that continue the condition above them.
///
/// Both SHORT-CIRCUIT: the second test does not run where the first has
/// settled the answer. A test is a block, whose value is its last
/// sentence's, so the two cannot simply be listed; the continuation is an
/// `:If` of its own instead, answering with the next test where the first
/// leaves the question open and with the settled truth value where it does
/// not. They chain left to right, `:AndIf` and `:OrIf` alike.
fn continued_condition(cur: &mut AplCursor<'_>, mut test: Expr) -> Result<Expr> {
    loop {
        let (word, rest, span) = match cur.peek() {
            Some(AplItem::Word { word: w @ ("AndIf" | "OrIf"), rest, span }) => {
                (*w, rest.clone(), *span)
            }
            _ => return Ok(test),
        };
        cur.at += 1;
        let next = condition(rest, span, cur.d)?;
        let settled = Expr::Const(Array::scalar_bool(word == "OrIf"), span);
        let (body, otherwise) = if word == "AndIf" {
            (vec![next], vec![settled])
        } else {
            (vec![settled], vec![next])
        };
        let whole = Span::merge(test.span(), span);
        test = Expr::Control(
            Box::new(Control::If {
                arms: vec![Branch {
                    test: Some(vec![test]),
                    body,
                    fall_through: false,
                    list: false,
                }],
                otherwise: Some(otherwise),
            }),
            whole,
        );
    }
}

/// The tokens after a control word, as one expression.
fn condition(rest: Vec<Token>, span: Span, d: Rules) -> Result<Expr> {
    match rest.first() {
        None => Err(Error::parse("this control word needs a condition", span)),
        Some(first) => {
            let hint = Span::merge(first.span, rest[rest.len() - 1].span);
            match &rest[0].kind {
                Tok::Control(w) => Err(Error::parse(format!("unexpected :{w}"), rest[0].span)),
                _ => Ok(AplItem::Sentence(parse_prepared(&rest, hint, d)?)).map(|it| match it {
                    AplItem::Sentence(e) => e,
                    AplItem::Word { .. } => unreachable!(),
                }),
            }
        }
    }
}

/// `:For name… :In source`. Several names take each item apart between
/// them, one of its own items each.
fn for_header(rest: &[Token], span: Span, d: Rules) -> Result<(Vec<String>, Expr)> {
    let Some(k) = rest.iter().position(|t| matches!(t.kind, Tok::Control("In"))) else {
        return Err(Error::parse(":For needs an :In", span));
    };
    let mut names = Vec::with_capacity(k);
    for t in &rest[..k] {
        match &t.kind {
            Tok::Name(n) => names.push(n.clone()),
            _ => return Err(Error::parse(":For binds names, one per item", t.span)),
        }
    }
    if names.is_empty() {
        return Err(Error::parse(":For needs a name to bind", span));
    }
    let source = &rest[k + 1..];
    let Some(first) = source.first() else {
        return Err(Error::parse(":In needs a value", span));
    };
    let hint = Span::merge(first.span, source[source.len() - 1].span);
    Ok((names, parse_prepared(source, hint, d)?))
}

/// Parse a token run that has already had its names and dfns folded.
fn parse_prepared(toks: &[Token], hint: Span, d: Rules) -> Result<Expr> {
    let toks = fold_axes(fold_operators(toks.to_vec(), d)?, d)?;
    if toks.is_empty() {
        return Err(Error::parse("this needs an expression", hint));
    }
    parse_range(&toks, 0, toks.len(), hint, d)
}

/// Give every assignment in a `∇` definition's body its scope: local for
/// the names the header owns, global for the rest. Definitions nested
/// inside keep their own rules, so the walk stops at them.
fn set_scopes(e: &mut Expr, own: &[String]) {
    let pick = |name: &str| {
        if own.iter().any(|n| n == name) {
            Scope::Local
        } else {
            Scope::Global
        }
    };
    match e {
        Expr::Entity(..) => {}
        Expr::Assign { name, value, scope, .. } => {
            *scope = pick(name);
            set_scopes(value, own);
        }
        Expr::AssignMany { names, value, scope, .. } => {
            // Every name of a distributed target is written by the same
            // sentence, so they share one scope: local when the definition
            // declared them, global otherwise.
            *scope = if names.iter().all(|n| own.iter().any(|o| o == n)) {
                Scope::Local
            } else {
                Scope::Global
            };
            set_scopes(value, own);
        }
        Expr::AmendIndex { name, slots, value, scope, .. } => {
            *scope = pick(name);
            for slot in slots.iter_mut().flatten() {
                set_scopes(slot, own);
            }
            set_scopes(value, own);
        }
        Expr::Monad { y, .. } => set_scopes(y, own),
        Expr::Dyad { x, y, .. } => {
            set_scopes(x, own);
            set_scopes(y, own);
        }
        Expr::PrintPass { value, .. } => set_scopes(value, own),
        Expr::Input { .. } => {}
        Expr::Control(c, _) => {
            let walk = |b: &mut Vec<Expr>| b.iter_mut().for_each(|s| set_scopes(s, own));
            match &mut **c {
                Control::Branch(target) => set_scopes(target, own),
                Control::BranchBy { by, test } => {
                    set_scopes(by, own);
                    set_scopes(test, own);
                }
                Control::Cond { test, body, otherwise } => {
                    walk(test);
                    walk(body);
                    walk(otherwise);
                }
                Control::If { arms, otherwise } => {
                    for arm in arms {
                        if let Some(t) = &mut arm.test {
                            walk(t);
                        }
                        walk(&mut arm.body);
                    }
                    if let Some(b) = otherwise {
                        walk(b);
                    }
                }
                Control::While { test, body, .. } | Control::Guard { test, body } => {
                    walk(test);
                    walk(body);
                }
                Control::For { source, body, .. } => {
                    set_scopes(source, own);
                    walk(body);
                }
                Control::Select { subject, cases } => {
                    set_scopes(subject, own);
                    for case in cases {
                        if let Some(t) = &mut case.test {
                            walk(t);
                        }
                        walk(&mut case.body);
                    }
                }
                Control::Try { body, catch, catcht } => {
                    walk(body);
                    walk(catch);
                    walk(catcht);
                }
                Control::Return
                | Control::Break
                | Control::Continue
                | Control::Label(_)
                | Control::Goto { .. }
                | Control::Throw => {}
            }
        }
        Expr::Const(..)
        | Expr::Param(..)
        | Expr::Name(..)
        | Expr::Fused { .. }
        | Expr::Elided { .. }
        | Expr::VerbDef { .. }
        | Expr::ModDef { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use rstest::rstest;

    /// The shipped dialect at the given index origin.
    fn rules(origin: i64) -> Rules {
        crate::Dialect { index_origin: Some(origin), ..crate::Dialect::default() }
            .rules(crate::Lang::Apl)
            .expect("the shipped dialect is implemented")
    }

    /// Parse one source string with `⎕IO←1`.
    fn p(src: &str) -> Result<Vec<Expr>> {
        parse(&SourceParts::from_source(src).unwrap(), rules(1))
    }

    fn one(src: &str) -> Expr {
        let mut stmts = p(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        assert_eq!(stmts.len(), 1, "{src}: expected one sentence");
        stmts.pop().unwrap()
    }

    fn err(src: &str) -> Error {
        match p(src) {
            Ok(_) => panic!("{src}: expected an error"),
            Err(e) => e,
        }
    }

    fn as_const(e: &Expr) -> &Array {
        match e {
            Expr::Const(a, _) => a,
            other => panic!("expected a constant, got {other:?}"),
        }
    }

    /// The primitive behind a verb, unwrapping nothing.
    fn as_prim(v: &Verb) -> Prim {
        match v {
            Verb::Prim(p) => *p,
            other => panic!("expected a primitive, got {other:?}"),
        }
    }

    fn monad_of<'a>(e: &'a Expr, name: &str) -> &'a Expr {
        match e {
            Expr::Monad { verb, y, .. } => {
                assert_eq!(as_prim(verb).name, name, "monad name");
                y.as_ref()
            }
            other => panic!("expected a monad, got {other:?}"),
        }
    }

    fn dyad_of<'a>(e: &'a Expr, name: &str) -> (&'a Expr, &'a Expr) {
        match e {
            Expr::Dyad { verb, x, y, .. } => {
                assert_eq!(as_prim(verb).name, name, "dyad name");
                (x.as_ref(), y.as_ref())
            }
            other => panic!("expected a dyad, got {other:?}"),
        }
    }

    fn verb_of(e: &Expr) -> &Verb {
        match e {
            Expr::Monad { verb, .. } | Expr::Dyad { verb, .. } => verb,
            other => panic!("expected an application, got {other:?}"),
        }
    }

    // --- literals ------------------------------------------------------

    #[test]
    fn single_number_is_a_scalar() {
        let e = one("5");
        let a = as_const(&e);
        assert_eq!(a.shape, Vec::<usize>::new());
        assert_eq!(a.data, Data::I64(vec![5].into()));
    }

    #[test]
    fn adjacent_numbers_merge_into_one_vector() {
        let a = as_const(&one("2 3 4")).clone();
        assert_eq!(a.shape, vec![3]);
        assert_eq!(a.data, Data::I64(vec![2, 3, 4].into()));
    }

    #[test]
    fn one_float_makes_the_whole_vector_float() {
        let a = as_const(&one("1 2.5 3")).clone();
        assert_eq!(a.shape, vec![3]);
        assert_eq!(a.data, Data::F64(vec![1.0, 2.5, 3.0].into()));
    }

    #[rstest]
    #[case("¯3", Data::I64(vec![-3].into()))]
    #[case("¯3.5", Data::F64(vec![-3.5].into()))]
    #[case("1e3", Data::I64(vec![1000].into()))]
    #[case("1e¯3", Data::F64(vec![0.001].into()))]
    #[case("2.5e2", Data::F64(vec![250.0].into()))]
    #[case("¯1 ¯2", Data::I64(vec![-1, -2].into()))]
    fn numeric_literals(#[case] src: &str, #[case] want: Data) {
        assert_eq!(as_const(&one(src)).data, want);
    }

    #[test]
    fn single_char_string_is_rank_zero() {
        let a = as_const(&one("'a'")).clone();
        assert_eq!(a.shape, Vec::<usize>::new());
        assert_eq!(a.data, Data::Char(vec!['a'].into()));
    }

    #[test]
    fn string_escape_doubles_the_quote() {
        let a = as_const(&one("'don''t'")).clone();
        assert_eq!(a.shape, vec![5]);
        assert_eq!(a.data, Data::Char("don't".chars().collect()));
    }

    #[test]
    fn empty_string_is_an_empty_char_vector() {
        let a = as_const(&one("''")).clone();
        assert_eq!(a.shape, vec![0]);
        assert_eq!(a.data, Data::Char(vec![].into()));
    }

    #[test]
    fn unterminated_string_is_a_parse_error() {
        let e = err("'abc");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.msg.contains("unterminated"), "{}", e.msg);
    }

    #[rstest]
    #[case("2j3", vec![[2.0, 3.0]])]
    #[case("1J¯1", vec![[1.0, -1.0]])]
    #[case("2 1j2", vec![[2.0, 0.0], [1.0, 2.0]])]
    fn complex_literals(#[case] src: &str, #[case] want: Vec<[f64; 2]>) {
        assert_eq!(as_const(&one(src)).data, Data::Complex(want.into()));
    }

    // --- comments, sentences, names ------------------------------------

    #[test]
    fn a_comment_runs_to_the_end_of_the_line() {
        let stmts = p("2+2 ⍝ a note ⋄ still a note\n3").unwrap();
        assert_eq!(stmts.len(), 2);
        dyad_of(&stmts[0], "+");
        assert_eq!(as_const(&stmts[1]).data, Data::I64(vec![3].into()));
    }

    #[test]
    fn blank_sentences_are_skipped() {
        let stmts = p("\n\n2 ⋄ ⋄ 3 ⋄\n").unwrap();
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn diamond_and_newline_both_separate_sentences() {
        let stmts = p("x←3 ⋄ x+1").unwrap();
        assert_eq!(stmts.len(), 2);
        match &stmts[0] {
            Expr::Assign { name, value, .. } => {
                assert_eq!(name, "x");
                assert_eq!(as_const(value).data, Data::I64(vec![3].into()));
            }
            other => panic!("expected an assignment, got {other:?}"),
        }
        let (x, y) = dyad_of(&stmts[1], "+");
        assert!(matches!(x, Expr::Name(n, _) if n == "x"));
        assert_eq!(as_const(y).data, Data::I64(vec![1].into()));
    }

    #[rstest]
    #[case("x")]
    #[case("abc123")]
    #[case("∆x")]
    #[case("⍙y_2")]
    #[case("Σ")]
    fn names(#[case] src: &str) {
        match one(src) {
            Expr::Name(n, _) => assert_eq!(n, src),
            other => panic!("expected a name, got {other:?}"),
        }
    }

    #[test]
    fn unknown_symbol_is_reported_with_its_position() {
        let e = err("2 & 3");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert_eq!(e.span, Some(Span::new(2, 3)));
        let e = err("2 ` 3");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert_eq!(e.msg, "unknown symbol: `");
        assert_eq!(e.span, Some(Span::new(2, 3)));
    }

    #[test]
    fn system_variables_are_read_only() {
        // Read-only is permanent, not a queue position: the dialect fixed
        // these before the program was compiled.
        let e = err("⎕IO←0");
        assert_eq!(e.kind, ErrorKind::Language);
        assert!(e.msg.contains("read-only"), "{}", e.msg);
        // The ones that would reach outside the program are refused by
        // name, whether they are read or written.
        let e = err("⎕TS");
        assert_eq!(e.kind, ErrorKind::Sandbox);
        assert!(e.msg.contains("outside the program"), "{}", e.msg);
    }

    // --- the primitive table -------------------------------------------

    #[rstest]
    #[case('+', MonadOp::Scalar(ScalarMonad::Conj), DyadOp::Scalar(ScalarDyad::Add))]
    #[case('-', MonadOp::Scalar(ScalarMonad::Neg), DyadOp::Scalar(ScalarDyad::Sub))]
    #[case('×', MonadOp::Scalar(ScalarMonad::Signum), DyadOp::Scalar(ScalarDyad::Mul))]
    #[case('÷', MonadOp::Scalar(ScalarMonad::Recip), DyadOp::Scalar(ScalarDyad::DivApl))]
    #[case('⌈', MonadOp::Scalar(ScalarMonad::Ceil), DyadOp::Scalar(ScalarDyad::Max))]
    #[case('⌊', MonadOp::Scalar(ScalarMonad::Floor), DyadOp::Scalar(ScalarDyad::Min))]
    #[case('*', MonadOp::Scalar(ScalarMonad::Exp), DyadOp::Scalar(ScalarDyad::Pow))]
    #[case('|', MonadOp::Scalar(ScalarMonad::Abs), DyadOp::Scalar(ScalarDyad::Residue))]
    #[case('=', MonadOp::None, DyadOp::Scalar(ScalarDyad::Eq))]
    #[case('<', MonadOp::None, DyadOp::Scalar(ScalarDyad::Lt))]
    #[case('≤', MonadOp::None, DyadOp::Scalar(ScalarDyad::Le))]
    #[case('>', MonadOp::None, DyadOp::Scalar(ScalarDyad::Gt))]
    #[case('≥', MonadOp::None, DyadOp::Scalar(ScalarDyad::Ge))]
    #[case('⍴', MonadOp::ShapeOf, DyadOp::Reshape)]
    #[case('⍉', MonadOp::TransposeAxes, DyadOp::TransposeApl)]
    #[case(',', MonadOp::Ravel, DyadOp::AppendLast)]
    #[case('⍪', MonadOp::TableOf, DyadOp::AppendLeading)]
    #[case('!', MonadOp::Scalar(ScalarMonad::Factorial), DyadOp::Scalar(ScalarDyad::Binomial))]
    #[case('⍕', MonadOp::Format, DyadOp::FormatSpec)]
    #[case('⊥', MonadOp::None, DyadOp::DecodeApl)]
    #[case('⊤', MonadOp::None, DyadOp::EncodeApl)]
    #[case('≢', MonadOp::Tally, DyadOp::NotMatch)]
    #[case('≡', MonadOp::Depth { signed: false }, DyadOp::Match)]
    #[case('∊', MonadOp::Enlist, DyadOp::MemberApl)]
    #[case('∪', MonadOp::Nub, DyadOp::Union)]
    #[case('∧', MonadOp::None, DyadOp::Scalar(ScalarDyad::Lcm))]
    #[case('∨', MonadOp::None, DyadOp::Scalar(ScalarDyad::Gcd))]
    #[case('⍟', MonadOp::Scalar(ScalarMonad::Ln), DyadOp::Scalar(ScalarDyad::Log))]
    #[case('~', MonadOp::Scalar(ScalarMonad::Not), DyadOp::Less)]
    #[case('⊖', MonadOp::Reverse, DyadOp::RotateApl { last: false })]
    #[case('⍋', MonadOp::GradeUp { origin: 1 }, DyadOp::CollateGrade { down: false, origin: 1 })]
    #[case('⍒', MonadOp::GradeDown { origin: 1 }, DyadOp::CollateGrade { down: true, origin: 1 })]
    #[case('⊢', MonadOp::Same, DyadOp::Right)]
    #[case('⊣', MonadOp::Same, DyadOp::Left)]
    #[case('↑', MonadOp::First, DyadOp::Take)]
    #[case('⊂', MonadOp::Enclose(Enclose::ExceptSimpleScalar), DyadOp::PartitionEnclose)]
    #[case('⊃', MonadOp::Open, DyadOp::Pick { origin: 1 })]
    #[case('↓', MonadOp::Split, DyadOp::Drop)]
    fn primitive_meanings(#[case] glyph: char, #[case] monad: MonadOp, #[case] dyad: DyadOp) {
        let src = format!("{glyph}1");
        let e = one(&src);
        match e {
            Expr::Monad { verb, .. } => {
                let prim = as_prim(&verb);
                assert_eq!(prim.monad, monad);
                assert_eq!(prim.dyad, dyad);
                assert_eq!(prim.name.chars().next(), Some(glyph));
            }
            other => panic!("expected a monad, got {other:?}"),
        }
    }

    #[test]
    fn monadic_not_equal_is_the_nub_sieve() {
        let e = one("≠1");
        match e {
            Expr::Monad { verb, .. } => {
                assert_eq!(as_prim(&verb).monad, MonadOp::NubSieve);
            }
            other => panic!("expected a monad, got {other:?}"),
        }
    }

    #[test]
    fn monadic_equals_parses_and_is_left_to_evaluation() {
        // `=` has no monadic meaning; the parser accepts it and eval refuses.
        let e = one("=1");
        assert_eq!(as_prim(verb_of(&e)).monad, MonadOp::None);
    }

    #[rstest]
    #[case(0)]
    #[case(1)]
    fn iota_carries_the_index_origin(#[case] origin: i64) {
        let sp = SourceParts::from_source("⍳3").unwrap();
        let stmts = parse(&sp, rules(origin)).unwrap();
        match &stmts[0] {
            Expr::Monad { verb, .. } => {
                assert_eq!(as_prim(verb).monad, MonadOp::IotaApl { origin });
                assert_eq!(as_prim(verb).dyad, DyadOp::IndexOf { origin, major_cells: false });
                assert_eq!(as_prim(verb).ranks, [RANK_INF, RANK_INF, RANK_INF]);
            }
            other => panic!("expected a monad, got {other:?}"),
        }
    }

    #[test]
    fn reverse_and_rotate_pick_their_axis() {
        // `⌽` is `⊖` on rows: the rank operator supplies the axis for the
        // MONAD, and the dyad picks the last axis itself.
        let e = one("⌽2 3⍴⍳6");
        match verb_of(&e) {
            Verb::Rank(f, ranks) => {
                assert_eq!(*ranks, [1, RANK_INF, RANK_INF]);
                assert_eq!(as_prim(f).monad, MonadOp::Reverse);
                assert_eq!(as_prim(f).dyad, DyadOp::RotateApl { last: true });
            }
            other => panic!("expected a ranked verb, got {other:?}"),
        }
        // `⊖` is the primitive itself, on the leading axis.
        assert!(matches!(verb_of(&one("⊖2 3⍴⍳6")), Verb::Prim(_)));
    }

    #[test]
    fn reshape_ranks_are_infinite_one_infinite() {
        let e = one("2 3⍴⍳6");
        assert_eq!(verb_of(&e).ranks(), [RANK_INF, 1, RANK_INF]);
    }

    // --- right-to-left parsing -----------------------------------------

    #[test]
    fn reshape_of_iota() {
        let e = one("2 3⍴⍳6");
        let (x, y) = dyad_of(&e, "⍴");
        assert_eq!(as_const(x).data, Data::I64(vec![2, 3].into()));
        let iy = monad_of(y, "⍳");
        assert_eq!(as_const(iy).data, Data::I64(vec![6].into()));
    }

    #[test]
    fn leading_minus_is_monadic_and_the_rest_is_evaluated_first() {
        // Right to left: `-3+4` is negate (3+4), not (-3)+4.
        let e = one("-3+4");
        let inner = monad_of(&e, "-");
        let (x, y) = dyad_of(inner, "+");
        assert_eq!(as_const(x).data, Data::I64(vec![3].into()));
        assert_eq!(as_const(y).data, Data::I64(vec![4].into()));
    }

    #[test]
    fn a_chain_of_dyads_associates_to_the_right() {
        let e = one("2×3+4");
        let (x, y) = dyad_of(&e, "×");
        assert_eq!(as_const(x).data, Data::I64(vec![2].into()));
        dyad_of(y, "+");
    }

    #[test]
    fn parentheses_override_the_order() {
        let e = one("(2+3)×4");
        let (x, y) = dyad_of(&e, "×");
        dyad_of(x, "+");
        assert_eq!(as_const(y).data, Data::I64(vec![4].into()));
    }

    #[test]
    fn nested_parentheses() {
        let e = one("((2+3))×4");
        let (x, _) = dyad_of(&e, "×");
        dyad_of(x, "+");
    }

    #[test]
    fn a_function_left_of_a_function_is_monadic() {
        // `⍴⍳5`: shape of iota, both monadic.
        let e = one("⍴⍳5");
        monad_of(monad_of(&e, "⍴"), "⍳");
    }

    // --- operators ------------------------------------------------------

    #[test]
    fn slash_reduces_the_last_axis() {
        // APL `+/` is J's `+/"1` monadically — rank 1 over the reduction —
        // but a different function dyadically, so the node is its own. The
        // left cell is unranked: n is one number, never a frame of them.
        let e = one("+/2 3⍴⍳6");
        match &e {
            Expr::Monad { verb: Verb::Rank(inner, ranks), .. } => {
                assert_eq!(*ranks, [1, RANK_INF, 1]);
                match inner.as_ref() {
                    Verb::NWise(f) => assert_eq!(as_prim(f).name, "+"),
                    other => panic!("expected a reduce, got {other:?}"),
                }
            }
            other => panic!("expected monadic Rank(NWise(+)), got {other:?}"),
        }
    }

    #[test]
    fn slashbar_reduces_the_leading_axis() {
        let e = one("+⌿2 3⍴⍳6");
        match &e {
            Expr::Monad { verb: Verb::NWise(f), .. } => assert_eq!(as_prim(f).name, "+"),
            other => panic!("expected monadic NWise(+), got {other:?}"),
        }
    }

    #[test]
    fn backslash_scans_the_last_axis_and_backslashbar_the_leading_one() {
        // The k-th element of a scan is the REDUCE of the first k, so the
        // derived verb applies `f/` to every prefix, not `f`.
        let inner = |v: &Verb| match v {
            Verb::Windowed(g, WindowKind::Scan) => match &**g {
                Verb::Reduce(h) => as_prim(h).name,
                other => panic!("expected a reduction under the scan, got {other:?}"),
            },
            other => panic!("expected a scan, got {other:?}"),
        };
        match &one("+\\1 2 3") {
            Expr::Monad { verb: Verb::Rank(f, ranks), .. } => {
                assert_eq!(*ranks, [1, 1, 1]);
                assert_eq!(inner(f), "+");
            }
            other => panic!("expected a ranked scan, got {other:?}"),
        }
        match &one("+⍀1 2 3") {
            Expr::Monad { verb, .. } => assert_eq!(inner(verb), "+"),
            other => panic!("expected a leading-axis scan, got {other:?}"),
        }
    }

    /// After an operand `/` and `⌿` are replicate, the function — the two
    /// readings are told apart by the token on the left and nothing else.
    #[rstest]
    #[case("1 0 1/1 2 3", "/")]
    #[case("1 0 1⌿1 2 3", "⌿")]
    #[case("x/1 2 3", "/")]
    #[case("(1 0)/1 2 3", "/")]
    fn slash_after_an_operand_is_replicate(#[case] src: &str, #[case] name: &str) {
        let e = one(src);
        let (_, _) = dyad_of(&e, name);
        assert_eq!(as_prim(verb_of(&e)).dyad, DyadOp::Copy);
    }

    #[rstest]
    #[case("1 0 1\\1 2 3")]
    #[case("1 0 1⍀1 2 3")]
    fn expand_after_a_value_is_a_function(#[case] src: &str) {
        let e = one(src);
        assert_eq!(as_prim(verb_of(&e)).dyad, DyadOp::Expand);
    }

    #[test]
    fn commute_and_power_are_operators() {
        match one("2-⍨5") {
            Expr::Dyad { verb: Verb::Commute(f), .. } => assert_eq!(as_prim(&f).name, "-"),
            other => panic!("expected a commute, got {other:?}"),
        }
        match one("+⍣3⊢5") {
            Expr::Monad { verb: Verb::PowerN(_, p), .. } => assert_eq!(p, Power::Times(3)),
            other => panic!("expected a power, got {other:?}"),
        }
        match one("+⍣≡⊢5") {
            Expr::Monad { verb: Verb::PowerUntil(..), .. } => {}
            other => panic!("expected a power until, got {other:?}"),
        }
        // A negative count is the inverse applied that many times, over the
        // obverse table. Which inverse that is waits for the arguments —
        // `x f⍣¯1 y` undoes `x∘f` — so the count keeps its sign here and a
        // function with no inverse is named when it runs, in
        // tests/wildhunt.rs.
        match one("⌽⍣¯1⊢5") {
            Expr::Monad { verb: Verb::PowerN(_, p), .. } => assert_eq!(p, Power::Inverse(1)),
            other => panic!("expected a power, got {other:?}"),
        }
    }

    #[rstest]
    #[case("+⍤2⊢5", [2, 2, 2])]
    #[case("+⍤1 2⊢5", [2, 1, 2])]
    #[case("+⍤0 1 2⊢5", [0, 1, 2])]
    #[case("+⍤¯1⊢5", [-1, -1, -1])]
    fn rank_operator_spec(#[case] src: &str, #[case] want: [i64; 3]) {
        let e = one(src);
        match &e {
            Expr::Monad { verb: Verb::Rank(f, ranks), .. } => {
                assert_eq!(*ranks, want);
                assert_eq!(as_prim(f).name, "+");
            }
            other => panic!("expected monadic Rank(+), got {other:?}"),
        }
    }

    #[test]
    fn rank_operator_stacks_on_a_derived_function() {
        let e = one("+/⍤1⊢5");
        match &e {
            Expr::Monad { verb: Verb::Rank(inner, ranks), .. } => {
                assert_eq!(*ranks, [1, 1, 1]);
                assert!(matches!(inner.as_ref(), Verb::Rank(_, r) if *r == [1, RANK_INF, 1]));
            }
            other => panic!("expected Rank(Rank(NWise(+))), got {other:?}"),
        }
    }

    #[test]
    fn a_function_operand_makes_the_rank_operator_an_atop() {
        // `f⍤g` with a function on the right is Dyalog's atop, not a rank.
        let e = one("+⍤×5");
        let Expr::Monad { verb, .. } = e else { panic!("expected a monad") };
        assert!(matches!(verb, Verb::Atop(..)), "{verb:?}");
    }

    #[rstest]
    #[case("+⍤0 1 2 3⊢5", "1 to 3")]
    #[case("+⍤", "rank specification")]
    #[case("+⍤2.5⊢5", "must be integers")]
    #[case("+⍤'a'⊢5", "must be integers")]
    fn bad_rank_specifications(#[case] src: &str, #[case] fragment: &str) {
        let e = err(src);
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.msg.contains(fragment), "{}", e.msg);
    }

    // --- assignment and output -----------------------------------------

    #[test]
    fn quad_arrow_is_print_pass() {
        let e = one("⎕←2+2");
        match &e {
            Expr::PrintPass { value, .. } => {
                dyad_of(value, "+");
            }
            other => panic!("expected PrintPass, got {other:?}"),
        }
    }

    #[test]
    fn assignment_chains() {
        let e = one("a←b←5");
        match &e {
            Expr::Assign { name, value, .. } => {
                assert_eq!(name, "a");
                match value.as_ref() {
                    Expr::Assign { name, value, .. } => {
                        assert_eq!(name, "b");
                        assert_eq!(as_const(value).data, Data::I64(vec![5].into()));
                    }
                    other => panic!("expected a nested assignment, got {other:?}"),
                }
            }
            other => panic!("expected an assignment, got {other:?}"),
        }
    }

    #[test]
    fn assignment_inside_an_expression() {
        let e = one("2+a←3");
        let (x, y) = dyad_of(&e, "+");
        assert_eq!(as_const(x).data, Data::I64(vec![2].into()));
        match y {
            Expr::Assign { name, value, .. } => {
                assert_eq!(name, "a");
                assert_eq!(as_const(value).data, Data::I64(vec![3].into()));
            }
            other => panic!("expected an assignment, got {other:?}"),
        }
    }

    #[rstest]
    #[case("2←3")]
    #[case("(2+2)←3")]
    fn assignment_target_must_be_a_name(#[case] src: &str) {
        let e = err(src);
        assert_eq!(e.kind, ErrorKind::Parse);
        assert_eq!(e.msg, "assignment target must be a name");
    }

    // --- parameters -----------------------------------------------------

    #[test]
    fn a_parameter_hole_is_an_operand() {
        let sp = SourceParts::from_parts(&["", "+1"], &["x"]);
        let stmts = parse(&sp, rules(1)).unwrap();
        let (x, y) = dyad_of(&stmts[0], "+");
        assert!(matches!(x, Expr::Param(0, _)));
        assert_eq!(as_const(y).data, Data::I64(vec![1].into()));
        // `{x}` occupies the first three characters of the display source.
        assert_eq!(x.span(), Span::new(0, 3));
        assert_eq!(sp.display, "{x}+1");
    }

    #[test]
    fn a_parameter_can_be_reduced_over() {
        let sp = SourceParts::from_parts(&["+/", ""], &["m"]);
        let stmts = parse(&sp, rules(1)).unwrap();
        match &stmts[0] {
            Expr::Monad { verb: Verb::Rank(_, r), y, .. } if *r == [1, RANK_INF, 1] => {
                assert!(matches!(y.as_ref(), Expr::Param(0, _)));
            }
            other => panic!("expected a reduction over a parameter, got {other:?}"),
        }
    }

    #[test]
    fn a_parameter_inside_a_comment_is_dropped() {
        let sp = SourceParts::from_parts(&["1 ⍝ ", "\n2"], &["x"]);
        let stmts = parse(&sp, rules(1)).unwrap();
        assert_eq!(stmts.len(), 2);
        assert_eq!(as_const(&stmts[0]).data, Data::I64(vec![1].into()));
        assert_eq!(as_const(&stmts[1]).data, Data::I64(vec![2].into()));
    }

    // --- spans ----------------------------------------------------------

    #[test]
    fn nodes_cover_their_source_extent() {
        let src = "2 3⍴⍳6";
        let e = one(src);
        assert_eq!(e.span(), Span::new(0, src.len()));
        let (x, y) = dyad_of(&e, "⍴");
        assert_eq!(x.span(), Span::new(0, 3));
        // `⍳6` starts after `2 3⍴`: three ASCII bytes plus a three-byte glyph.
        assert_eq!(y.span(), Span::new(6, src.len()));
    }

    #[test]
    fn spans_of_a_later_sentence_are_absolute() {
        let src = "x←3 ⋄ x+1";
        let stmts = p(src).unwrap();
        // `←` and `⋄` are three bytes each, so the second `x` sits at byte 10.
        assert_eq!(&src[10..], "x+1");
        assert_eq!(stmts[1].span(), Span::new(10, src.len()));
    }

    #[test]
    fn a_dyad_span_includes_the_parenthesised_left_argument() {
        let src = "(2+3)×4";
        let e = one(src);
        assert_eq!(e.span(), Span::new(0, src.len()));
    }

    // --- syntax errors --------------------------------------------------

    /// Juxtaposition is vector notation: the operands become the items of
    /// one vector, and the whole strand is a single operand.
    #[rstest]
    #[case("(2 3)(4 5)", 2)]
    #[case("2 x", 2)]
    #[case("x y", 2)]
    #[case("2(3)", 2)]
    #[case("1 2 (3 4)", 3)]
    #[case("'ab' 'cd' 'ef'", 3)]
    fn juxtaposition_is_vector_notation(#[case] src: &str, #[case] items: usize) {
        // The strand is built right to left: one seeding monad and one
        // dyad per item after the first.
        let mut e = &one(src);
        for _ in 0..items - 1 {
            match e {
                Expr::Dyad { verb, y, .. } => {
                    assert_eq!(verb.name(), "(vector notation)", "{src}");
                    e = y.as_ref();
                }
                other => panic!("{src}: expected a strand, got {other:?}"),
            }
        }
        assert!(matches!(e, Expr::Monad { .. }), "{src}: {e:?}");
    }

    #[rstest]
    #[case("2+", "missing right argument")]
    #[case("x←", "← needs a value")]
    #[case("(2+3", "syntax error")]
    #[case("2+3)", "unmatched )")]
    #[case("()", "empty parentheses")]
        #[case("/2 3", "needs a function to its left")]
    fn syntax_errors(#[case] src: &str, #[case] fragment: &str) {
        let e = err(src);
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.msg.contains(fragment), "{src}: {}", e.msg);
    }

    #[test]
    fn empty_source_has_no_statements() {
        assert!(p("").unwrap().is_empty());
        assert!(p("  ⍝ nothing here\n").unwrap().is_empty());
    }

    /// Every APL expression the evaluation suite runs must at least parse.
    #[rstest]
    #[case("2+2")]
    #[case("¯2×3")]
    #[case("-3+4")]
    #[case("0÷0")]
    #[case("⍳4")]
    #[case("⍳0")]
    #[case("2 3⍴⍳6")]
    #[case("⍴2 3⍴⍳6")]
    #[case("⍉2 3⍴⍳6")]
    #[case("≢7 8 9")]
    #[case("2↑9 8 7")]
    #[case("¯2↑9 8 7")]
    #[case("1↓3 3⍴⍳9")]
    #[case(",2 2⍴⍳4")]
    #[case("x←3 ⋄ x+1")]
    #[case("2+a←3")]
    #[case("⎕←2+2")]
    #[case("(2 3⍴⍳6)+10 20")]
    #[case("2+3 ⍝ sum")]
    #[case("+/2 3⍴⍳6")]
    #[case("+⌿2 3⍴⍳6")]
    #[case("⎕←'Hello, world!'")]
    fn the_evaluation_corpus_parses(#[case] src: &str) {
        p(src).unwrap_or_else(|e| panic!("{src}: {e}"));
    }

    #[test]
    fn errors_render_against_the_display_source() {
        let src = "2 3⍴⍳6\n2 ` 3";
        let e = err(src);
        let rendered = e.render(src);
        assert!(rendered.contains("unknown symbol: `"), "{rendered}");
        assert!(rendered.contains("2 ` 3"), "{rendered}");
    }
}
