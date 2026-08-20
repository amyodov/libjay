//! APL frontend: lexer and parser, lowering to the shared IR.
//!
//! APL sentences read right to left: the rightmost expression is the right
//! argument of the function to its left, and a function is dyadic exactly
//! when an operand ends immediately to its left. Operators bind tighter than
//! that: `f/` and `f⍤r` are folded into derived functions before the
//! sentence is parsed.

use crate::array::{Array, Data};
use crate::error::{Error, Result, Span};
use crate::frontend::{Segment, SourceParts};
use crate::ir::Expr;
use crate::verb::{
    DyadOp, Enclose, MonadOp, Power, Prim, ScalarDyad, ScalarMonad, Verb, WindowKind, RANK_INF,
};

/// Parse an APL program (sentences separated by newlines or `⋄`) into IR
/// statements. `origin` is the dialect's `⎕IO`.
pub fn parse(src: &SourceParts, origin: i64) -> Result<Vec<Expr>> {
    let sentences = lex(src, origin)?;
    let mut stmts = Vec::with_capacity(sentences.len());
    for sentence in sentences {
        let toks = fold_operators(sentence)?;
        if toks.is_empty() {
            continue;
        }
        let hint = Span::merge(toks[0].span, toks[toks.len() - 1].span);
        stmts.push(parse_range(&toks, 0, toks.len(), hint)?);
    }
    Ok(stmts)
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
}

#[derive(Clone, Debug)]
struct Token {
    kind: Tok,
    span: Span,
}

/// True for tokens that can end an operand (and so make the function on
/// their right dyadic).
fn is_operand_end(k: &Tok) -> bool {
    matches!(k, Tok::Value(_) | Tok::Nums(_) | Tok::Param(_) | Tok::Name(_) | Tok::RParen)
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
            ranks: [0, RANK_INF, RANK_INF],
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
            dyad: D::NotYet("partitioned enclose (dyadic ⊂)"),
            ranks: [RANK_INF, RANK_INF, RANK_INF],
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
    for seg in &src.segments {
        match seg {
            Segment::Text { text, offset } => {
                lex_text(text, *offset, origin, &mut out, &mut cur, &mut in_comment)?;
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

fn lex_text(
    text: &str,
    offset: usize,
    origin: i64,
    out: &mut Vec<Vec<Token>>,
    cur: &mut Vec<Token>,
    in_comment: &mut bool,
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
            '\n' | '⋄' => {
                end_sentence(out, cur);
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
            '(' => {
                cur.push(Token { kind: Tok::LParen, span: Span::new(offset + i, offset + i + 1) });
                i += 1;
            }
            ')' => {
                cur.push(Token { kind: Tok::RParen, span: Span::new(offset + i, offset + i + 1) });
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
    let mut vals: Vec<f64> = Vec::new();
    let mut any_float = false;
    let mut i = start;
    let mut end;
    loop {
        let (v, float, next) = lex_number(text, i, offset)?;
        if let Some(c) = text[next..].chars().next() {
            if (c == 'j' || c == 'J') && num_start(text, next + 1) {
                let (_, _, imag_end) = lex_number(text, next + 1, offset)?;
                return Err(Error::not_yet(
                    "complex literals",
                    Span::new(offset + start, offset + imag_end),
                ));
            }
        }
        vals.push(v);
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
    let data = if any_float {
        Data::F64(vals.into())
    } else {
        Data::I64(vals.iter().map(|&v| v as i64).collect())
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
fn fold_operators(toks: Vec<Token>) -> Result<Vec<Token>> {
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
                    Some(tok) if matches!(tok.kind, Tok::Func(_)) => {
                        return Err(Error::not_yet(
                            "power with a function right operand (f⍣g)",
                            Span::merge(span, tok.span),
                        ));
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
fn parse_range(toks: &[Token], lo: usize, hi: usize, hint: Span) -> Result<Expr> {
    let (mut acc, mut start) = parse_operand(toks, lo, hi, hint)?;
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
                    let (x, xstart) = parse_operand(toks, lo, start - 1, left.span)?;
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
                        acc = Expr::Assign { name: n.clone(), value: Box::new(acc), span };
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
fn parse_operand(toks: &[Token], lo: usize, hi: usize, hint: Span) -> Result<(Expr, usize)> {
    let (first, mut start) = parse_primary(toks, lo, hi, hint)?;
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
        let (e, s) = parse_primary(toks, lo, start, toks[start - 1].span)?;
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
fn parse_primary(toks: &[Token], lo: usize, hi: usize, hint: Span) -> Result<(Expr, usize)> {
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
            let inner = parse_range(toks, l + 1, hi - 1, hint)?;
            Ok((inner, l))
        }
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
        Tok::Op(_) => Err(Error::internal("operator survived folding")),
    }
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
    #[case("2j3")]
    #[case("1J¯1")]
    #[case("2 1j2")]
    fn complex_literals_are_not_yet(#[case] src: &str) {
        let e = err(src);
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("complex"), "{}", e.msg);
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
    #[case('⊂', MonadOp::Enclose(Enclose::ExceptSimpleScalar), DyadOp::NotYet("partitioned enclose (dyadic ⊂)"))]
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
                assert_eq!(as_prim(verb).ranks, [0, RANK_INF, RANK_INF]);
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
        let e = err("+⍣≡⊢5");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("function right operand"), "{}", e.msg);
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
