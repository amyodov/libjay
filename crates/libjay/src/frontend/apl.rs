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
use crate::frontend::{Segment, SourceParts};
use crate::ir::{Branch, Control, ExplicitDef, Expr, Scope};
use crate::verb::{
    DyadOp, Enclose, MonadOp, Power, Prim, ScalarDyad, ScalarMonad, Verb, WindowKind, RANK_INF,
};

/// Parse an APL program (sentences separated by newlines or `⋄`) into IR
/// statements. `origin` is the dialect's `⎕IO`.
pub fn parse(src: &SourceParts, origin: i64) -> Result<Vec<Expr>> {
    let sentences = lex(src, origin)?;
    let mut verbs: HashMap<String, Verb> = HashMap::new();
    let mut stmts = Vec::with_capacity(sentences.len());
    let mut i = 0usize;
    while i < sentences.len() {
        if matches!(sentences[i].first().map(|t| &t.kind), Some(Tok::Del)) {
            let stmt = parse_tradfn(&sentences, &mut i, origin, &mut verbs)?;
            stmts.push(stmt);
            continue;
        }
        let sentence = sentences[i].clone();
        i += 1;
        if let Some(stmt) = parse_statement(sentence, origin, &mut verbs, false)? {
            stmts.push(stmt);
        }
    }
    Ok(stmts)
}

/// One sentence, with every name known to be a function already a function.
/// None where the sentence held nothing but blanks and a comment.
fn parse_statement(
    sentence: Vec<Token>,
    origin: i64,
    verbs: &mut HashMap<String, Verb>,
    in_def: bool,
) -> Result<Option<Expr>> {
    let sentence = substitute_verbs(sentence, verbs);
    let sentence = fold_dfns(sentence, origin, verbs)?;
    // `F←{⍵×2}` names a function: the sentence does no work at run time, and
    // later sentences read `F` as the function itself.
    if let [name, assign, func] = &sentence[..] {
        if let (Tok::Name(n), Tok::Assign, Tok::Func(v)) = (&name.kind, &assign.kind, &func.kind) {
            let span = Span::merge(name.span, func.span);
            if !in_def {
                verbs.insert(n.clone(), v.clone());
            }
            return Ok(Some(Expr::VerbDef { name: n.clone(), verb: v.clone(), span }));
        }
    }
    let toks = fold_paren_funcs(fold_axes(fold_operators(sentence, origin)?, origin)?);
    if toks.is_empty() {
        return Ok(None);
    }
    if let Some(t) = toks.iter().find(|t| matches!(t.kind, Tok::Control(_))) {
        return Err(Error::parse(
            "control structures are only meaningful inside a ∇ definition",
            t.span,
        ));
    }
    let hint = Span::merge(toks[0].span, toks[toks.len() - 1].span);
    // `A[i]←v` replaces part of a named value; nothing else assigns through
    // a bracket.
    if let Some(e) = indexed_assignment(&toks, origin, hint)? {
        return Ok(Some(e));
    }
    parse_range(&toks, 0, toks.len(), hint, origin).map(Some)
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
            toks[i].kind = Tok::Func(v.clone());
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
    /// `∘` on its own — Dyalog's `f∘g`, which libjay does not have yet.
    Jot,
    /// `¨` — each.
    Each,
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
            OpGlyph::Each => '¨',
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
    Quad,
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
    /// `∇`: a definition's bracket outside a dfn, a self-reference inside.
    Del,
    /// A control word, `:If` and its family, without the colon.
    Control(&'static str),
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
        Tok::Value(_) | Tok::Nums(_) | Tok::Param(_) | Tok::Name(_) | Tok::RParen | Tok::RBracket
    )
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

/// The primitive for a function glyph. `origin` parameterises `⍳`.
fn prim_for(ch: char, origin: i64) -> Option<Prim> {
    use DyadOp as D;
    use MonadOp as M;
    use ScalarDyad as SD;
    use ScalarMonad as SM;
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
            monad: M::NotYet("nub sieve (monadic ≠)"),
            dyad: D::Scalar(SD::Ne),
            ranks: [0, 0, 0],
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
            dyad: D::IndexOf { origin },
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
            dyad: D::NotYet("union (dyadic ∪)"),
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '∩' => Prim {
            name: "∩",
            monad: M::NotYet("intersection (∩)"),
            dyad: D::NotYet("intersection (∩)"),
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '∧' => Prim { name: "∧", monad: M::None, dyad: D::Scalar(SD::Lcm), ranks: [0, 0, 0] },
        '∨' => Prim { name: "∨", monad: M::None, dyad: D::Scalar(SD::Gcd), ranks: [0, 0, 0] },
        '⍱' => Prim {
            name: "⍱",
            monad: M::None,
            dyad: D::NotYet("nor (⍱)"),
            ranks: [0, 0, 0],
        },
        '⍲' => Prim {
            name: "⍲",
            monad: M::None,
            dyad: D::NotYet("nand (⍲)"),
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
            dyad: D::NotYet("without (dyadic ~)"),
            ranks: [0, 0, 0],
        },
        '≡' => Prim {
            name: "≡",
            monad: M::Depth,
            dyad: D::Match,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '⍋' => Prim {
            name: "⍋",
            monad: M::GradeUp { origin },
            dyad: D::NotYet("dyadic grade (collation)"),
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '⍒' => Prim {
            name: "⍒",
            monad: M::GradeDown { origin },
            dyad: D::NotYet("dyadic grade (collation)"),
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        // `⊖` works on the leading axis; `⌽` is the same primitive applied to
        // rows, which `verb_for` wraps in the rank that does it.
        '⊖' | '⌽' => Prim {
            name: if ch == '⊖' { "⊖" } else { "⌽" },
            monad: M::Reverse,
            dyad: D::Rotate,
            ranks: [RANK_INF, 1, RANK_INF],
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
            dyad: D::NotYet("format with a specification (dyadic ⍕)"),
            ranks: [RANK_INF, 1, RANK_INF],
        },
        // `⊥` and `⊤` have no monadic meaning in APL; J spells those `#.`
        // and `#:`. `⊤` takes its right argument whole, so the digit axis
        // leads and the result has shape (⍴x),(⍴y).
        '⊥' => Prim {
            name: "⊥",
            monad: M::None,
            dyad: D::Decode,
            ranks: [RANK_INF, 1, 1],
        },
        '⊤' => Prim {
            name: "⊤",
            monad: M::None,
            dyad: D::Encode,
            ranks: [RANK_INF, 1, RANK_INF],
        },
        '⍉' => Prim {
            name: "⍉",
            monad: M::TransposeAxes,
            dyad: D::NotYet("dyadic transpose"),
            ranks: [RANK_INF, 1, RANK_INF],
        },
        // GNU APL is APL2-flavoured here: `↑` is first and `⊃` is
        // disclose, the opposite of the Dyalog reading.
        '↑' => Prim {
            name: "↑",
            monad: M::First,
            dyad: D::Take,
            ranks: [RANK_INF, 1, RANK_INF],
        },
        '⊂' => Prim {
            name: "⊂",
            monad: M::Enclose(Enclose::ExceptSimpleScalar),
            dyad: D::PartitionEnclose,
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        // `⍸` counts from ⎕IO; its dyad is the interval index, which GNU
        // APL answers with the count of bounds below the value plus ⎕IO-1.
        '⍸' => Prim {
            name: "⍸",
            monad: M::Indices { origin, boxed_coords: true },
            dyad: D::IntervalIndex { offset: origin - 1 },
            ranks: [RANK_INF, 1, RANK_INF],
        },
        // GNU APL's `⌷` is APL2's: one scalar index per axis, no monad.
        '⌷' => Prim {
            name: "⌷",
            monad: M::NotYet("materialise (monadic ⌷)"),
            dyad: D::Squad { origin },
            ranks: [RANK_INF, RANK_INF, RANK_INF],
        },
        '?' => Prim {
            name: "?",
            monad: M::Roll { origin, fixed: false, float_at_zero: false },
            dyad: D::Deal { origin, fixed: false },
            ranks: [RANK_INF, 0, 0],
        },
        '⌹' => Prim {
            name: "⌹",
            monad: M::MatrixInverse,
            dyad: D::MatrixDivide,
            ranks: [2, RANK_INF, 2],
        },
        '⊃' => Prim {
            name: "⊃",
            monad: M::Open,
            dyad: D::NotYet("pick (dyadic ⊃)"),
            ranks: [0, RANK_INF, RANK_INF],
        },
        '↓' => Prim {
            name: "↓",
            monad: M::NotYet("split (monadic ↓) — nested arrays"),
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
        _ => return None,
    };
    Some(p)
}

/// The function a glyph denotes. Every glyph but `⌽` is a bare primitive;
/// `⌽` is `⊖` applied to rows, so it carries the rank that does that: cells
/// of rank 1 on the right, atoms on the left.
fn verb_for(ch: char, origin: i64) -> Option<Verb> {
    let p = prim_for(ch, origin)?;
    if ch == '⌽' {
        return Some(Verb::Rank(Box::new(Verb::Prim(p)), [1, 0, 1]));
    }
    Some(Verb::Prim(p))
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
        '¨' => Some(OpGlyph::Each),
        _ => None,
    }
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
fn lex(src: &SourceParts, origin: i64) -> Result<Vec<Vec<Token>>> {
    let mut out: Vec<Vec<Token>> = Vec::new();
    let mut cur: Vec<Token> = Vec::new();
    // A comment runs to the end of a line, which may be a later segment.
    let mut in_comment = false;
    let mut braces = 0usize;
    for seg in &src.segments {
        match seg {
            Segment::Text { text, offset } => {
                lex_text(text, *offset, origin, &mut out, &mut cur, &mut in_comment, &mut braces)?;
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
    origin: i64,
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
                return Err(Error::not_yet(
                    "branching (→ with a label)",
                    Span::new(offset + i, offset + i + clen),
                ));
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
                cur.push(Token {
                    kind: Tok::Assign,
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
                    return Err(Error::not_yet(
                        "system variables (⎕IO is a compiler dialect setting)",
                        Span::new(offset + i, offset + j),
                    ));
                }
                cur.push(Token { kind: Tok::Quad, span: Span::new(offset + i, offset + after) });
                i = after;
            }
            _ if num_start(text, i) => {
                let (tok, next) = lex_number_vector(text, i, offset)?;
                cur.push(tok);
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
                if let Some(v) = verb_for(ch, origin) {
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

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '∆' || c == '⍙'
}

fn is_name_body(c: char) -> bool {
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

/// One numeric literal. Returns its value, whether it needs floating point,
/// and the byte index just past it.
fn lex_number(text: &str, start: usize, offset: usize) -> Result<(f64, bool, usize)> {
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
    if let Some(c) = text[i..].chars().next() {
        if c == 'e' || c == 'E' {
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
    }
    let v: f64 = buf.parse().map_err(|_| {
        Error::parse(
            format!("cannot read the number {}", &text[start..i]),
            Span::new(offset + start, offset + i),
        )
    })?;
    // `1e3` is the integer 1000; `1e¯3` and `2.5` are floats.
    let float = saw_dot || v.fract() != 0.0 || v.abs() >= 9.0e18;
    Ok((v, float, i))
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

/// A run of blank-separated numeric literals: one value token. Integers
/// unless some literal needs floating point; a single literal is a scalar.
fn lex_number_vector(text: &str, start: usize, offset: usize) -> Result<(Token, usize)> {
    let mut vals: Vec<crate::complex::Cx> = Vec::new();
    let mut any_float = false;
    let mut any_complex = false;
    let mut i = start;
    let mut end;
    loop {
        let (v, float, mut next) = lex_number(text, i, offset)?;
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
        any_float |= float;
        end = next;
        i = next;
        let mut k = i;
        while text[k..].starts_with(' ') || text[k..].starts_with('\t') {
            k += 1;
        }
        if k > i && num_start(text, k) {
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
        Data::I64(vals.iter().map(|&v| v[0] as i64).collect())
    };
    let shape = if data.len() == 1 { vec![] } else { vec![data.len()] };
    let tok = Token {
        kind: Tok::Nums(Array::new(shape, data)),
        span: Span::new(offset + start, offset + end),
    };
    Ok((tok, end))
}

// ---------------------------------------------------------------------------
// Operator folding
// ---------------------------------------------------------------------------

/// Fold monadic and dyadic operators into derived-function tokens, left to
/// right. After this the sentence holds only values, names, functions, `←`,
/// `⎕` and parentheses.
fn fold_operators(toks: Vec<Token>, origin: i64) -> Result<Vec<Token>> {
    let mut out: Vec<Token> = Vec::new();
    let mut it = toks.into_iter().peekable();
    while let Some(t) = it.next() {
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
        if op == OpGlyph::Jot {
            return Err(Error::not_yet("beside (∘) composition: Dyalog's f∘g", t.span));
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
                    OpGlyph::Backslash => return Err(Error::not_yet("expand (x\\y)", t.span)),
                    OpGlyph::BackslashBar => {
                        return Err(Error::not_yet("expand along the leading axis (x⍀y)", t.span));
                    }
                    OpGlyph::Rank
                    | OpGlyph::Commute
                    | OpGlyph::Power
                    | OpGlyph::JotDot
                    | OpGlyph::Jot
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
        if let Some((k, aspan)) = take_axis(&mut it, origin)? {
            let inner = match op {
                OpGlyph::Slash | OpGlyph::SlashBar => Verb::Reduce(Box::new(f)),
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
            out.push(Token {
                kind: Tok::Func(Verb::AlongAxis(Box::new(inner), k)),
                span: Span::merge(span, aspan),
            });
            continue;
        }
        let derived = match op {
            // APL's divergence from J: `/` reduces the last axis, `⌿` the
            // leading one. `+/` sums rows, `+⌿` sums columns.
            OpGlyph::Slash => Verb::Rank(Box::new(Verb::Reduce(Box::new(f))), [1, 1, 1]),
            OpGlyph::SlashBar => Verb::Reduce(Box::new(f)),
            // The scan follows the reduce: `\` along the last axis, `⍀`
            // along the leading one. The k-th element is the reduce of the
            // first k, which is the verb applied to the k-th prefix.
            OpGlyph::Backslash => Verb::Rank(
                Box::new(Verb::Windowed(Box::new(Verb::Reduce(Box::new(f))), WindowKind::Scan)),
                [1, 1, 1],
            ),
            OpGlyph::BackslashBar => {
                Verb::Windowed(Box::new(Verb::Reduce(Box::new(f))), WindowKind::Scan)
            }
            OpGlyph::Commute => Verb::Commute(Box::new(f)),
            // Each: the function runs on the contents of every item and
            // its result goes back into an item. A simple scalar result
            // stays simple, which is APL's enclosure rule.
            OpGlyph::Each => Verb::Each(Box::new(f), Enclose::ExceptSimpleScalar),
            OpGlyph::Power => {
                let spec = match it.peek() {
                    // `f⍣g` iterates until `new g old` holds: `f⍣≡` is the
                    // fixed point, which is the spelling the reference uses.
                    Some(tok) if matches!(tok.kind, Tok::Func(_)) => {
                        let gtok = it.next().unwrap();
                        let Tok::Func(g) = gtok.kind else { unreachable!("checked above") };
                        let v = Verb::PowerUntil(Box::new(f), Box::new(g));
                        out.push(Token {
                            kind: Tok::Func(v),
                            span: Span::merge(span, gtok.span),
                        });
                        continue;
                    }
                    Some(tok) if literal(&tok.kind).is_some() => it.next().unwrap(),
                    _ => {
                        return Err(Error::not_yet("computed power (f⍣n)", t.span));
                    }
                };
                let arr = literal(&spec.kind).expect("checked above");
                let p = power_spec(arr, spec.span)?;
                let f = Verb::PowerN(Box::new(f), p);
                out.push(Token { kind: Tok::Func(f), span: Span::merge(span, spec.span) });
                continue;
            }
            OpGlyph::Rank => {
                let spec = match it.peek() {
                    Some(tok) if matches!(tok.kind, Tok::Func(_)) => {
                        return Err(Error::not_yet(
                            "function composition (f⍤g)",
                            Span::merge(span, tok.span),
                        ));
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
                let f = Verb::Rank(Box::new(f), ranks);
                out.push(Token { kind: Tok::Func(f), span: Span::merge(span, spec.span) });
                continue;
            }
            // Both are answered before the left operand is taken.
            OpGlyph::JotDot | OpGlyph::Jot => unreachable!("handled above"),
        };
        out.push(Token { kind: Tok::Func(derived), span });
    }
    Ok(out)
}

/// `[k]` immediately after an operator glyph, if it is there. The axis is
/// given in `⎕IO` origin and comes back as a zero-based one.
fn take_axis(
    it: &mut std::iter::Peekable<std::vec::IntoIter<Token>>,
    origin: i64,
) -> Result<Option<(usize, Span)>> {
    if !matches!(it.peek().map(|t| &t.kind), Some(Tok::LBracket)) {
        return Ok(None);
    }
    let open = it.next().expect("peeked");
    let spec = match it.next() {
        Some(tok) if literal(&tok.kind).is_some() => tok,
        Some(tok) => return Err(Error::not_yet("a computed axis (f[k])", tok.span)),
        None => return Err(Error::parse("unterminated axis specification", open.span)),
    };
    let close = match it.next() {
        Some(tok) if matches!(tok.kind, Tok::RBracket) => tok,
        _ => return Err(Error::parse("unterminated axis specification", open.span)),
    };
    let span = Span::merge(open.span, close.span);
    let arr = literal(&spec.kind).expect("checked above");
    let ints = arr
        .to_i64_vec()
        .ok_or_else(|| Error::parse("an axis must be a whole number", spec.span))?;
    let [k] = ints[..] else {
        return Err(Error::not_yet("several axes in one specification", spec.span));
    };
    let k = k - origin;
    if k < 0 {
        return Err(Error::domain(format!("axis {} does not exist", k + origin), spec.span));
    }
    Ok(Some((k as usize, span)))
}

/// `(f)` is `f`: a function alone in parentheses is only grouped, and the
/// parser reads a bare function as a missing argument otherwise.
fn fold_paren_funcs(toks: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(toks.len());
    for t in toks {
        if matches!(t.kind, Tok::RBracket) {
            out.push(t);
            continue;
        }
        let is_close = matches!(t.kind, Tok::RParen);
        let n = out.len();
        if is_close
            && n >= 2
            && matches!(out[n - 1].kind, Tok::Func(_))
            && matches!(out[n - 2].kind, Tok::LParen)
        {
            let f = out.pop().expect("checked above");
            let open = out.pop().expect("checked above");
            out.push(Token { kind: f.kind, span: Span::merge(open.span, t.span) });
            continue;
        }
        out.push(t);
    }
    out
}

/// `f[k]` where `f` is a plain function rather than a derived one.
fn fold_axes(toks: Vec<Token>, origin: i64) -> Result<Vec<Token>> {
    let mut out: Vec<Token> = Vec::new();
    let mut it = toks.into_iter().peekable();
    while let Some(t) = it.next() {
        let Tok::Func(f) = &t.kind else {
            out.push(t);
            continue;
        };
        let Some((k, aspan)) = take_axis(&mut it, origin)? else {
            out.push(t);
            continue;
        };
        let Some(inner) = leading_axis_form(f) else {
            return Err(Error::not_yet(format!("axis specification for {}", f.name()), aspan));
        };
        out.push(Token {
            kind: Tok::Func(Verb::AlongAxis(Box::new(inner), k)),
            span: Span::merge(t.span, aspan),
        });
    }
    Ok(out)
}

/// The function a glyph means once an axis is named — the leading-axis form
/// of the pairs that differ only in which axis they pick. None where libjay
/// has no axis form for the function yet.
fn leading_axis_form(v: &Verb) -> Option<Verb> {
    match v {
        // `⌽` is `⊖` applied to rows; with an axis given the two agree.
        Verb::Rank(inner, [1, 0, 1]) => leading_axis_form(inner),
        Verb::Prim(p) if matches!(p.monad, MonadOp::Reverse) => Some(v.clone()),
        _ => None,
    }
}

/// One bracket slot: axis `axis` of the right argument selected by the left.
fn select_axis_verb(axis: usize, rank: usize, origin: i64) -> Verb {
    Verb::Prim(Prim {
        name: "[…]",
        monad: MonadOp::None,
        dyad: DyadOp::SelectAxis { axis, rank, origin },
        ranks: [RANK_INF; 3],
    })
}

/// `f⍣n`: one nonnegative integer atom. APL spells convergence `f⍣≡`, a
/// function right operand, which is a separate gap.
fn power_spec(a: &Array, span: Span) -> Result<Power> {
    let ints = a
        .to_i64_vec()
        .ok_or_else(|| Error::parse("⍣ needs a whole number on its right", span))?;
    let [n] = ints[..] else {
        return Err(Error::not_yet("power over a list of counts (f⍣n)", span));
    };
    if n < 0 {
        return Err(Error::not_yet("inverse power (f⍣¯1 and other negative powers)", span));
    }
    Ok(Power::Times(n as u64))
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
fn parse_range(toks: &[Token], lo: usize, hi: usize, hint: Span, origin: i64) -> Result<Expr> {
    let (mut acc, mut start) = parse_operand(toks, lo, hi, hint, origin)?;
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
                    let (x, xstart) = parse_operand(toks, lo, start - 1, left.span, origin)?;
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
                        acc = Expr::Assign {
                            name: n.clone(),
                            value: Box::new(acc),
                            scope: Scope::Local,
                            span,
                        };
                    }
                    Tok::Quad => {
                        acc = Expr::PrintPass { value: Box::new(acc), span };
                    }
                    _ => {
                        return Err(Error::parse(
                            "assignment target must be a name",
                            target.span,
                        ));
                    }
                }
                start -= 2;
            }
            // Adjacent operands are vector notation, which `parse_operand`
            // has already taken as one operand by the time we get here.
            _ => break,
        }
    }
    Err(Error::parse("syntax error", Span::new(toks[lo].span.start, toks[start - 1].span.end)))
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
    origin: i64,
) -> Result<(Expr, usize)> {
    let (first, mut start) = parse_primary(toks, lo, hi, hint, origin)?;
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
        let (e, s) = parse_primary(toks, lo, start, toks[start - 1].span, origin)?;
        cur = e;
        start = s;
    }
    let span = Span::new(toks[start].span.start, toks[hi - 1].span.end);
    let mut it = items.into_iter();
    let last = it.next().expect("a strand has at least one item");
    let mut acc = Expr::Monad { verb: strand_seed(), y: Box::new(last), span };
    for item in it {
        acc = Expr::Dyad { verb: strand_verb(), x: Box::new(item), y: Box::new(acc), span };
    }
    Ok((acc, start))
}

/// The items one primary contributes to a strand, appended right to left.
fn push_items(items: &mut Vec<Expr>, e: Expr, tok: &Token) {
    if let Tok::Nums(a) = &tok.kind {
        if a.rank() > 0 {
            for i in (0..a.count()).rev() {
                let atom = Array { shape: Vec::new(), data: a.data.slice(i, i + 1) };
                items.push(Expr::Const(atom, tok.span));
            }
            return;
        }
    }
    items.push(e);
}

/// `,⊂y`: the one-item vector a single operand makes — flat when the
/// operand is a simple scalar, nested when it is anything else.
fn strand_seed() -> Verb {
    Verb::Atop(
        Box::new(Verb::Prim(prim_for(',', 0).expect("`,` is a primitive"))),
        Box::new(Verb::Prim(prim_for('⊂', 0).expect("`⊂` is a primitive"))),
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
    origin: i64,
) -> Result<(Expr, usize)> {
    if hi == lo {
        return Err(Error::parse("empty parentheses", hint));
    }
    let t = &toks[hi - 1];
    match &t.kind {
        Tok::Value(a) | Tok::Nums(a) => Ok((Expr::Const(a.clone(), t.span), hi - 1)),
        Tok::Param(i) => Ok((Expr::Param(*i, t.span), hi - 1)),
        Tok::Name(n) => Ok((Expr::Name(n.clone(), t.span), hi - 1)),
        Tok::RParen => {
            let l = match_lparen(toks, lo, hi - 1)?;
            let hint = Span::merge(toks[l].span, t.span);
            let inner = parse_range(toks, l + 1, hi - 1, hint, origin)?;
            Ok((inner, l))
        }
        Tok::RBracket => index_brackets(toks, lo, hi, origin),
        // `F←+/` names a function in some dialects. The reference this
        // frontend follows, GNU APL, rejects it as a syntax error, so it is
        // not implemented here; J's `mean =. +/ % #` is the spelling libjay
        // has for the same idea.
        Tok::Func(_) if hi >= lo + 2 && matches!(toks[hi - 2].kind, Tok::Assign) => {
            let from = if hi >= lo + 3 { toks[hi - 3].span } else { toks[hi - 2].span };
            Err(Error::not_yet("function assignment (F←+/)", Span::merge(from, t.span)))
        }
        Tok::Func(_) => Err(Error::parse("missing right argument", t.span)),
        Tok::Assign => Err(Error::parse("← needs a value on its right", t.span)),
        Tok::Quad => Err(Error::parse("⎕ is only supported as ⎕← (print)", t.span)),
        Tok::LParen => Err(Error::parse("unmatched (", t.span)),
        Tok::LBracket => Err(Error::parse("unmatched [", t.span)),
        Tok::Semi => Err(Error::parse("; is only meaningful inside index brackets", t.span)),
        Tok::Colon => Err(Error::parse(": is only meaningful in a dfn guard", t.span)),
        Tok::Del => Err(Error::parse("∇ opens a definition; it is not a value", t.span)),
        Tok::Control(w) => Err(Error::parse(
            format!(":{w} is only meaningful inside a ∇ definition"),
            t.span,
        )),
        Tok::LBrace | Tok::RBrace => Err(Error::parse("unmatched {", t.span)),
        Tok::Separator => Err(Error::internal("a statement break survived folding")),
        Tok::Op(_) => Err(Error::internal("operator survived folding")),
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
    origin: i64,
) -> Result<(Expr, usize)> {
    let close = &toks[hi - 1];
    let open = match_lbracket(toks, lo, hi - 1)?;
    if open == lo || !is_operand_end(&toks[open - 1].kind) {
        return Err(Error::parse("[ needs a value on its left", toks[open].span));
    }
    let (base, start) = parse_primary(toks, lo, open, toks[open].span, origin)?;
    let slots = index_slots(toks, open + 1, hi - 1, toks[open].span)?;
    let span = Span::new(toks[start].span.start, close.span.end);
    let rank = slots.len();
    let mut acc = base;
    let mut first = true;
    for (axis, slot) in slots.iter().enumerate().rev() {
        let Some((slo, shi)) = *slot else { continue };
        let idx = parse_range(toks, slo, shi, toks[open].span, origin)?;
        let check = if first { rank } else { 0 };
        first = false;
        acc = Expr::Dyad {
            verb: select_axis_verb(axis, check, origin),
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
const CONTROL_WORDS: [&str; 18] = [
    "If", "ElseIf", "Else", "EndIf", "While", "EndWhile", "Repeat", "Until", "For", "In",
    "EndFor", "Select", "Case", "EndSelect", "Return", "Leave", "Continue", "End",
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
    origin: i64,
    verbs: &HashMap<String, Verb>,
) -> Result<Vec<Token>> {
    let Some(open) = toks.iter().position(|t| matches!(t.kind, Tok::LBrace)) else {
        return Ok(toks);
    };
    let close = match_close(&toks, open, &Tok::LBrace, &Tok::RBrace)
        .ok_or_else(|| Error::parse("unmatched {", toks[open].span))?;
    let span = Span::merge(toks[open].span, toks[close].span);
    let verb = build_dfn(&toks[open + 1..close], origin, verbs)?;
    let mut out: Vec<Token> = toks[..open].to_vec();
    out.push(Token { kind: Tok::Func(verb), span });
    out.extend_from_slice(&toks[close + 1..]);
    // A sentence may hold several dfns side by side.
    fold_dfns(out, origin, verbs)
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

/// `{ … }`: the body's own words decide the valence, and `∇` in it names
/// the dfn itself.
fn build_dfn(body: &[Token], origin: i64, verbs: &HashMap<String, Verb>) -> Result<Verb> {
    let mut depth = 0usize;
    let mut dyadic = false;
    let mut operator = None;
    for t in body {
        match &t.kind {
            Tok::LBrace => depth += 1,
            Tok::RBrace => depth = depth.saturating_sub(1),
            Tok::Name(n) if depth == 0 && n == "⍺" => dyadic = true,
            Tok::Name(n) if depth == 0 && (n == "⍺⍺" || n == "⍵⍵") => operator = Some(t.span),
            _ => {}
        }
    }
    if let Some(s) = operator {
        return Err(Error::not_yet("dfn operators (⍺⍺ and ⍵⍵)", s));
    }
    let mut inner = verbs.clone();
    let stmts = parse_dfn_body(body, origin, &mut inner)?;
    let pure = stmts.iter().all(is_pure_stmt);
    Ok(Verb::Explicit(Arc::new(ExplicitDef {
        name: "{…}".to_string(),
        left: dyadic.then(|| "⍺".to_string()),
        right: "⍵".to_string(),
        result: None,
        locals: Vec::new(),
        body: stmts,
        // A dfn that reaches its end without a value has no result to give.
        empty: None,
        pure,
    })))
}

fn parse_dfn_body(
    body: &[Token],
    origin: i64,
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
        stmts.push(parse_guarded(stmt, origin, verbs)?);
    }
    Ok(stmts)
}

/// One dfn statement: a guard `cond:expr`, an `⍺←default`, or a sentence.
fn parse_guarded(
    stmt: Vec<Token>,
    origin: i64,
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
        let test = one_statement(stmt[..k].to_vec(), origin, verbs, stmt[k].span)?;
        let body = one_statement(stmt[k + 1..].to_vec(), origin, verbs, stmt[k].span)?;
        // A guard that holds is the dfn's answer: the value, then out.
        let arm = Branch {
            test: Some(vec![test]),
            body: vec![body, Expr::Control(Box::new(Control::Return), span)],
            fall_through: false,
        };
        return Ok(Expr::Control(
            Box::new(Control::If { arms: vec![arm], otherwise: None }),
            span,
        ));
    }
    // `⍺←v` gives the left argument a value only where none arrived.
    let default = matches!(
        (stmt.first().map(|t| &t.kind), stmt.get(1).map(|t| &t.kind)),
        (Some(Tok::Name(n)), Some(Tok::Assign)) if n == "⍺"
    );
    let span = stmt.first().map_or(Span::new(0, 0), |t| t.span);
    let e = one_statement(stmt, origin, verbs, span)?;
    if default {
        if let Expr::Assign { name, value, span, .. } = e {
            return Ok(Expr::Assign { name, value, scope: Scope::LocalDefault, span });
        }
    }
    Ok(e)
}

fn one_statement(
    stmt: Vec<Token>,
    origin: i64,
    verbs: &mut HashMap<String, Verb>,
    hint: Span,
) -> Result<Expr> {
    parse_statement(stmt, origin, verbs, true)?
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
        Control::Try { body, catch } => all(body) && all(catch),
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
    origin: i64,
    verbs: &mut HashMap<String, Verb>,
) -> Result<Expr> {
    let header = &sentences[*i];
    let open = header[0].span;
    *i += 1;
    let (name, def_left, def_right, result, locals) = parse_header(&header[1..], open)?;
    let mut body_lines: Vec<Vec<Token>> = Vec::new();
    loop {
        let Some(line) = sentences.get(*i) else {
            return Err(Error::parse("this definition has no closing ∇", open));
        };
        *i += 1;
        if line.len() == 1 && matches!(line[0].kind, Tok::Del) {
            break;
        }
        body_lines.push(line.clone());
    }
    let close = sentences
        .get(i.saturating_sub(1))
        .and_then(|l| l.first())
        .map_or(open, |t| t.span);
    let span = Span::merge(open, close);
    // The body can call the function by its own name.
    let mut inner = verbs.clone();
    inner.insert(name.clone(), Verb::Named(name.clone()));
    let mut items = Vec::new();
    for line in &body_lines {
        items.push(to_item(line.clone(), origin, &mut inner)?);
    }
    let mut cursor = AplCursor { items: &items, at: 0 };
    let mut body = parse_apl_block(&mut cursor, &[])?;
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
    own.push(def_right.clone());
    for stmt in &mut body {
        set_scopes(stmt, &own);
    }
    let pure = body.iter().all(is_pure_stmt);
    let verb = Verb::Explicit(Arc::new(ExplicitDef {
        name: format!("∇{name}"),
        left: def_left,
        right: def_right,
        result,
        locals,
        body,
        empty: None,
        pure,
    }));
    verbs.insert(name.clone(), verb.clone());
    Ok(Expr::VerbDef { name, verb, span })
}

type Header = (String, Option<String>, String, Option<String>, Vec<String>);

/// `Z←L F R;a;b` and its shorter forms.
fn parse_header(toks: &[Token], span: Span) -> Result<Header> {
    let mut names: Vec<String> = Vec::new();
    let mut locals: Vec<String> = Vec::new();
    let mut result = None;
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
            _ => {
                return Err(Error::parse("this is not a ∇ definition header", toks[k].span));
            }
        }
        k += 1;
    }
    match names.len() {
        3 => Ok((names[1].clone(), Some(names[0].clone()), names[2].clone(), result, locals)),
        2 => Ok((names[0].clone(), None, names[1].clone(), result, locals)),
        1 => Err(Error::not_yet("niladic ∇ definitions", span)),
        _ => Err(Error::parse("a ∇ definition header names a function and its arguments", span)),
    }
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

fn to_item(
    line: Vec<Token>,
    origin: i64,
    verbs: &mut HashMap<String, Verb>,
) -> Result<AplItem> {
    if let Some(Tok::Control(word)) = line.first().map(|t| &t.kind) {
        let word = *word;
        let span = line[0].span;
        return Ok(AplItem::Word { word, rest: line[1..].to_vec(), span });
    }
    let span = line.first().map_or(Span::new(0, 0), |t| t.span);
    let e = parse_statement(line, origin, verbs, true)?
        .ok_or_else(|| Error::parse("this line has no sentence", span))?;
    Ok(AplItem::Sentence(e))
}

struct AplCursor<'a> {
    items: &'a [AplItem],
    at: usize,
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
    let mut out = Vec::new();
    loop {
        match cur.peek() {
            None => return Ok(out),
            Some(AplItem::Word { word, .. }) if stop.contains(word) || *word == "End" => {
                return Ok(out);
            }
            Some(AplItem::Sentence(e)) => {
                cur.at += 1;
                out.push(e.clone());
            }
            Some(AplItem::Word { .. }) => out.push(parse_apl_control(cur)?),
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
                let test_expr = condition(test, start)?;
                let body = parse_apl_block(cur, &["ElseIf", "Else", "EndIf"])?;
                arms.push(Branch { test: Some(vec![test_expr]), body, fall_through: false });
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
            let test = condition(rest, start)?;
            let body = parse_apl_block(cur, &["EndWhile"])?;
            cur.close("EndWhile")?;
            Control::While { test: vec![test], body, body_first: false, until: false }
        }
        "Repeat" => {
            if !rest.is_empty() {
                return Err(Error::parse(":Repeat takes no condition", start));
            }
            let body = parse_apl_block(cur, &["Until"])?;
            let Some(AplItem::Word { rest, span, .. }) = cur.peek() else {
                return Err(Error::parse("this :Repeat needs an :Until", cur.last_span()));
            };
            let test = condition(rest.clone(), *span)?;
            cur.at += 1;
            Control::While { test: vec![test], body, body_first: true, until: true }
        }
        "For" => {
            // `:For name :In source`.
            let (name, source) = for_header(&rest, start)?;
            let body = parse_apl_block(cur, &["EndFor"])?;
            cur.close("EndFor")?;
            Control::For { name: Some(name), source: Box::new(source), body }
        }
        "Select" => {
            let subject = condition(rest, start)?;
            let mut cases = Vec::new();
            loop {
                match cur.peek() {
                    Some(AplItem::Word { word: "Case", rest, span }) => {
                        let test = condition(rest.clone(), *span)?;
                        cur.at += 1;
                        let body = parse_apl_block(cur, &["Case", "Else", "EndSelect"])?;
                        cases.push(Branch {
                            test: Some(vec![test]),
                            body,
                            fall_through: false,
                        });
                    }
                    Some(AplItem::Word { word: "Else", .. }) => {
                        cur.at += 1;
                        let body = parse_apl_block(cur, &["EndSelect"])?;
                        cases.push(Branch { test: None, body, fall_through: false });
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
        "Leave" => Control::Break,
        "Continue" => Control::Continue,
        other => {
            return Err(Error::parse(format!(":{other} has no matching opening word"), start));
        }
    };
    Ok(Expr::Control(Box::new(control), Span::merge(start, cur.last_span())))
}

/// The tokens after a control word, as one expression.
fn condition(rest: Vec<Token>, span: Span) -> Result<Expr> {
    match rest.first() {
        None => Err(Error::parse("this control word needs a condition", span)),
        Some(first) => {
            let hint = Span::merge(first.span, rest[rest.len() - 1].span);
            match &rest[0].kind {
                Tok::Control(w) => Err(Error::parse(format!("unexpected :{w}"), rest[0].span)),
                _ => Ok(AplItem::Sentence(parse_prepared(&rest, hint)?)).map(|it| match it {
                    AplItem::Sentence(e) => e,
                    AplItem::Word { .. } => unreachable!(),
                }),
            }
        }
    }
}

/// `:For name :In source`.
fn for_header(rest: &[Token], span: Span) -> Result<(String, Expr)> {
    let Some(Tok::Name(name)) = rest.first().map(|t| &t.kind) else {
        return Err(Error::parse(":For needs a name to bind", span));
    };
    let Some(k) = rest.iter().position(|t| matches!(t.kind, Tok::Control("In"))) else {
        return Err(Error::parse(":For needs an :In", span));
    };
    if k != 1 {
        return Err(Error::not_yet("several :For names", span));
    }
    let source = &rest[k + 1..];
    let Some(first) = source.first() else {
        return Err(Error::parse(":In needs a value", span));
    };
    let hint = Span::merge(first.span, source[source.len() - 1].span);
    Ok((name.clone(), parse_prepared(source, hint)?))
}

/// Parse a token run that has already had its names and dfns folded.
fn parse_prepared(toks: &[Token], hint: Span) -> Result<Expr> {
    let toks = fold_paren_funcs(fold_axes(fold_operators(toks.to_vec(), 1)?, 1)?);
    if toks.is_empty() {
        return Err(Error::parse("this needs an expression", hint));
    }
    parse_range(&toks, 0, toks.len(), hint, 1)
}

/// `A[i;j]←v`: the one assignment that writes through a bracket. None when
/// the sentence is not one.
fn indexed_assignment(toks: &[Token], origin: i64, hint: Span) -> Result<Option<Expr>> {
    let Some(assign) = toks.iter().position(|t| matches!(t.kind, Tok::Assign)) else {
        return Ok(None);
    };
    if assign < 3 || !matches!(toks[assign - 1].kind, Tok::RBracket) {
        return Ok(None);
    }
    let close = assign - 1;
    let open = match_lbracket(toks, 0, close)?;
    if open == 0 {
        return Err(Error::parse("[ needs a value on its left", toks[open].span));
    }
    let Tok::Name(name) = &toks[open - 1].kind else {
        return Err(Error::not_yet("indexed assignment through an expression", hint));
    };
    if open != 1 {
        return Err(Error::not_yet("indexed assignment inside a larger sentence", hint));
    }
    let ranges = index_slots(toks, open + 1, close, toks[open].span)?;
    let mut slots = Vec::with_capacity(ranges.len());
    for slot in &ranges {
        slots.push(match *slot {
            None => None,
            Some((lo, hi)) => Some(parse_range(toks, lo, hi, toks[open].span, origin)?),
        });
    }
    let value = parse_range(toks, assign + 1, toks.len(), toks[assign].span, origin)?;
    let span = Span::merge(toks[0].span, toks[toks.len() - 1].span);
    Ok(Some(Expr::AmendIndex {
        name: name.clone(),
        slots,
        value: Box::new(value),
        origin,
        scope: Scope::Local,
        span,
    }))
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
        Expr::Assign { name, value, scope, .. } => {
            *scope = pick(name);
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
        Expr::Control(c, _) => {
            let walk = |b: &mut Vec<Expr>| b.iter_mut().for_each(|s| set_scopes(s, own));
            match &mut **c {
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
                Control::While { test, body, .. } => {
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
                Control::Try { body, catch } => {
                    walk(body);
                    walk(catch);
                }
                Control::Return | Control::Break | Control::Continue => {}
            }
        }
        Expr::Const(..)
        | Expr::Param(..)
        | Expr::Name(..)
        | Expr::Fused { .. }
        | Expr::Elided { .. }
        | Expr::VerbDef { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use rstest::rstest;

    /// Parse one source string with `⎕IO←1`.
    fn p(src: &str) -> Result<Vec<Expr>> {
        parse(&SourceParts::from_source(src).unwrap(), 1)
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
        let e = err("2 @ 3");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert_eq!(e.msg, "unknown symbol: @");
        assert_eq!(e.span, Some(Span::new(2, 3)));
    }

    #[test]
    fn system_variables_are_not_yet() {
        let e = err("⎕IO←0");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("system variables"), "{}", e.msg);
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
    #[case('⍉', MonadOp::TransposeAxes, DyadOp::NotYet("dyadic transpose"))]
    #[case(',', MonadOp::Ravel, DyadOp::AppendLast)]
    #[case('⍪', MonadOp::TableOf, DyadOp::AppendLeading)]
    #[case('!', MonadOp::Scalar(ScalarMonad::Factorial), DyadOp::Scalar(ScalarDyad::Binomial))]
    #[case('⍕', MonadOp::Format, DyadOp::NotYet("format with a specification (dyadic ⍕)"))]
    #[case('⊥', MonadOp::None, DyadOp::Decode)]
    #[case('⊤', MonadOp::None, DyadOp::Encode)]
    #[case('≢', MonadOp::Tally, DyadOp::NotMatch)]
    #[case('≡', MonadOp::Depth, DyadOp::Match)]
    #[case('∊', MonadOp::Enlist, DyadOp::MemberApl)]
    #[case('∪', MonadOp::Nub, DyadOp::NotYet("union (dyadic ∪)"))]
    #[case('∧', MonadOp::None, DyadOp::Scalar(ScalarDyad::Lcm))]
    #[case('∨', MonadOp::None, DyadOp::Scalar(ScalarDyad::Gcd))]
    #[case('⍟', MonadOp::Scalar(ScalarMonad::Ln), DyadOp::Scalar(ScalarDyad::Log))]
    #[case('~', MonadOp::Scalar(ScalarMonad::Not), DyadOp::NotYet("without (dyadic ~)"))]
    #[case('⊖', MonadOp::Reverse, DyadOp::Rotate)]
    #[case('⍋', MonadOp::GradeUp { origin: 1 }, DyadOp::NotYet("dyadic grade (collation)"))]
    #[case('⍒', MonadOp::GradeDown { origin: 1 }, DyadOp::NotYet("dyadic grade (collation)"))]
    #[case('⊢', MonadOp::Same, DyadOp::Right)]
    #[case('⊣', MonadOp::Same, DyadOp::Left)]
    #[case('↑', MonadOp::First, DyadOp::Take)]
    #[case('⊂', MonadOp::Enclose(Enclose::ExceptSimpleScalar), DyadOp::PartitionEnclose)]
    #[case('⊃', MonadOp::Open, DyadOp::NotYet("pick (dyadic ⊃)"))]
    #[case('↓', MonadOp::NotYet("split (monadic ↓) — nested arrays"), DyadOp::Drop)]
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
    fn monadic_not_equal_is_the_nub_sieve_placeholder() {
        let e = one("≠1");
        match e {
            Expr::Monad { verb, .. } => {
                assert_eq!(as_prim(&verb).monad, MonadOp::NotYet("nub sieve (monadic ≠)"));
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
        let stmts = parse(&sp, origin).unwrap();
        match &stmts[0] {
            Expr::Monad { verb, .. } => {
                assert_eq!(as_prim(verb).monad, MonadOp::IotaApl { origin });
                assert_eq!(as_prim(verb).dyad, DyadOp::IndexOf { origin });
                assert_eq!(as_prim(verb).ranks, [RANK_INF, RANK_INF, RANK_INF]);
            }
            other => panic!("expected a monad, got {other:?}"),
        }
    }

    #[test]
    fn reverse_and_rotate_pick_their_axis() {
        // `⌽` is `⊖` on rows: the rank operator supplies the axis.
        let e = one("⌽2 3⍴⍳6");
        match verb_of(&e) {
            Verb::Rank(f, ranks) => {
                assert_eq!(*ranks, [1, 0, 1]);
                assert_eq!(as_prim(f).monad, MonadOp::Reverse);
                assert_eq!(as_prim(f).dyad, DyadOp::Rotate);
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
        // APL `+/` is J's `+/"1`: rank 1 over the reduction.
        let e = one("+/2 3⍴⍳6");
        match &e {
            Expr::Monad { verb: Verb::Rank(inner, ranks), .. } => {
                assert_eq!(*ranks, [1, 1, 1]);
                match inner.as_ref() {
                    Verb::Reduce(f) => assert_eq!(as_prim(f).name, "+"),
                    other => panic!("expected a reduce, got {other:?}"),
                }
            }
            other => panic!("expected monadic Rank(Reduce(+)), got {other:?}"),
        }
    }

    #[test]
    fn slashbar_reduces_the_leading_axis() {
        let e = one("+⌿2 3⍴⍳6");
        match &e {
            Expr::Monad { verb: Verb::Reduce(f), .. } => assert_eq!(as_prim(f).name, "+"),
            other => panic!("expected monadic Reduce(+), got {other:?}"),
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
    #[case("1 0 1\\1 2 3", "expand")]
    #[case("1 0 1⍀1 2 3", "expand")]
    fn expand_is_not_yet(#[case] src: &str, #[case] msg: &str) {
        let e = err(src);
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains(msg), "{}", e.msg);
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
        let e = err("+⍣¯1⊢5");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("inverse power"), "{}", e.msg);
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
                assert!(matches!(inner.as_ref(), Verb::Rank(_, [1, 1, 1])));
            }
            other => panic!("expected Rank(Rank(Reduce(+))), got {other:?}"),
        }
    }

    #[test]
    fn function_composition_is_not_yet() {
        let e = err("+⍤×5");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("composition"), "{}", e.msg);
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
        let stmts = parse(&sp, 1).unwrap();
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
        let stmts = parse(&sp, 1).unwrap();
        match &stmts[0] {
            Expr::Monad { verb: Verb::Rank(_, [1, 1, 1]), y, .. } => {
                assert!(matches!(y.as_ref(), Expr::Param(0, _)));
            }
            other => panic!("expected a reduction over a parameter, got {other:?}"),
        }
    }

    #[test]
    fn a_parameter_inside_a_comment_is_dropped() {
        let sp = SourceParts::from_parts(&["1 ⍝ ", "\n2"], &["x"]);
        let stmts = parse(&sp, 1).unwrap();
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
    #[case("⎕", "⎕ is only supported")]
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
        let src = "2 3⍴⍳6\n2 @ 3";
        let e = err(src);
        let rendered = e.render(src);
        assert!(rendered.contains("unknown symbol: @"), "{rendered}");
        assert!(rendered.contains("2 @ 3"), "{rendered}");
    }
}
