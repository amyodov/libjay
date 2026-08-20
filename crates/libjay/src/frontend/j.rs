//! J frontend: lexer and the sentence parser, lowering to the shared IR.
//!
//! Word formation and sentence parsing follow the model published in the J
//! Dictionary. Words are formed left to right; a sentence is then executed
//! right to left by pushing words onto a stack and matching the leftmost four
//! stack slots against the parse table after every push.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::array::{Array, Data};
use crate::error::{Error, ErrorKind, Result, Span};
use crate::frontend::{Segment, SourceParts};
use crate::ir::Expr;
use crate::verb::{
    DyadOp, Enclose, MonadOp, Power, Prim, ScalarDyad, ScalarMonad, Verb, WindowKind, RANK_INF,
};

/// Parse a J program (one sentence per line) into IR statements.
///
/// Sentences are parsed in order over a table of the names that have been
/// given verbs, because a name's part of speech decides how the sentence
/// around it parses. A sentence that names a verb records it and produces
/// no work; a later sentence that reads the name gets the verb substituted
/// into it. That is enough for the straight-line programs this frontend
/// compiles: there is no control flow for a definition to reach backwards
/// through, and reassigning the name simply rebinds it from there on.
pub fn parse(src: &SourceParts) -> Result<Vec<Expr>> {
    let mut verbs: HashMap<String, Verb> = HashMap::new();
    // Names that hold a value by the time a sentence is read. Only the
    // diagnostics need this: a name that is neither a verb nor a value is
    // an undefined name, not a sentence the parser has yet to learn.
    let mut nouns: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for mut sentence in lex(src)? {
        substitute_verbs(&mut sentence, &verbs);
        let stmt = parse_sentence(sentence, &nouns)?;
        match &stmt {
            Expr::VerbDef { name, verb, .. } => {
                verbs.insert(name.clone(), verb.clone());
                nouns.remove(name);
            }
            // A name given a noun stops being a verb, at any depth: J lets
            // a name change part of speech, and the oracle agrees.
            other => {
                let mut assigned = Vec::new();
                assigned_names(other, &mut assigned);
                for name in assigned {
                    verbs.remove(&name);
                    nouns.insert(name);
                }
            }
        }
        out.push(stmt);
    }
    Ok(out)
}

/// Replace every name known to be a verb by that verb, except where the
/// name is the target of an assignment, which is a definition of the name
/// rather than a use of it.
fn substitute_verbs(sentence: &mut [Frag], verbs: &HashMap<String, Verb>) {
    for i in 0..sentence.len() {
        let Frag::Name(name, span) = &sentence[i] else { continue };
        if sentence.get(i + 1).is_some_and(Frag::is_assign) {
            continue;
        }
        if let Some(v) = verbs.get(name) {
            sentence[i] = Frag::Verb(VerbFrag::V(v.clone()), *span);
        }
    }
}

/// Every name this sentence assigns a value to, inline assignments included.
fn assigned_names(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Assign { name, value, .. } => {
            out.push(name.clone());
            assigned_names(value, out);
        }
        Expr::Monad { y, .. } => assigned_names(y, out),
        Expr::Dyad { x, y, .. } => {
            assigned_names(x, out);
            assigned_names(y, out);
        }
        Expr::PrintPass { value, .. } => assigned_names(value, out),
        _ => {}
    }
}

// ---------------------------------------------------------------- fragments

/// A stack fragment. The lexer emits these directly: a token and a parser
/// fragment are the same thing in J, which is why the parse table can be
/// stated over four adjacent stack slots.
#[derive(Clone, Debug)]
enum Frag {
    /// Left edge of the sentence.
    Mark,
    Noun(Expr),
    /// A name used as a value, or an assignment target.
    Name(String, Span),
    Verb(VerbFrag, Span),
    Adverb(&'static str, Span),
    Conj(&'static str, Span),
    LParen(Span),
    RParen(Span),
    AssignLocal(Span),
    AssignGlobal(Span),
    /// A finished verb definition: `mean =. +/ % #`. It belongs to no part
    /// of speech, so no rule reaches it and it can only end a sentence.
    VerbDef(String, Verb, Span),
}

/// `[:` has the verb category but no verb of its own: it is only meaningful
/// as the left tine of a fork, where it caps the fork into an atop.
#[derive(Clone, Debug)]
enum VerbFrag {
    V(Verb),
    Cap,
}

impl Frag {
    fn span(&self) -> Span {
        match self {
            Frag::Mark => Span::new(0, 0),
            Frag::Noun(e) => e.span(),
            Frag::Name(_, s)
            | Frag::Verb(_, s)
            | Frag::Adverb(_, s)
            | Frag::Conj(_, s)
            | Frag::LParen(s)
            | Frag::RParen(s)
            | Frag::AssignLocal(s)
            | Frag::AssignGlobal(s)
            | Frag::VerbDef(_, _, s) => *s,
        }
    }

    fn is_edge(&self) -> bool {
        matches!(self, Frag::Mark | Frag::AssignLocal(_) | Frag::AssignGlobal(_) | Frag::LParen(_))
    }

    /// Verb category, `[:` included.
    fn is_verb(&self) -> bool {
        matches!(self, Frag::Verb(..))
    }

    /// A verb that can actually be applied or bound to a modifier.
    fn is_real_verb(&self) -> bool {
        matches!(self, Frag::Verb(VerbFrag::V(_), _))
    }

    /// Names are nouns in this subset; only assignment treats them apart.
    fn is_noun(&self) -> bool {
        matches!(self, Frag::Noun(_) | Frag::Name(..))
    }

    fn is_adverb(&self) -> bool {
        matches!(self, Frag::Adverb(..))
    }

    fn is_conj(&self) -> bool {
        matches!(self, Frag::Conj(..))
    }

    fn is_avn(&self) -> bool {
        self.is_adverb() || self.is_verb() || self.is_noun()
    }

    fn is_cavn(&self) -> bool {
        self.is_conj() || self.is_avn()
    }

    fn is_assign(&self) -> bool {
        matches!(self, Frag::AssignLocal(_) | Frag::AssignGlobal(_))
    }
}

// -------------------------------------------------------------- primitives

const fn prim(name: &'static str, monad: MonadOp, dyad: DyadOp, ranks: [i64; 3]) -> Prim {
    Prim { name, monad, dyad, ranks }
}

/// The primitive verbs this frontend knows, by their J spelling. Verbs whose
/// meaning exists in J but not here carry `NotYet` so the diagnostic arrives
/// at evaluation, pointing at the verb.
fn primitive(word: &str) -> Option<Prim> {
    use DyadOp as D;
    use MonadOp as M;
    use ScalarDyad as SD;
    use ScalarMonad as SM;
    const INF: i64 = RANK_INF;
    Some(match word {
        "+" => prim("+", M::Scalar(SM::Conj), D::Scalar(SD::Add), [0, 0, 0]),
        "-" => prim("-", M::Scalar(SM::Neg), D::Scalar(SD::Sub), [0, 0, 0]),
        "*" => prim("*", M::Scalar(SM::Signum), D::Scalar(SD::Mul), [0, 0, 0]),
        "%" => prim("%", M::Scalar(SM::Recip), D::Scalar(SD::DivJ), [0, 0, 0]),
        "^" => prim("^", M::Scalar(SM::Exp), D::Scalar(SD::Pow), [0, 0, 0]),
        "%:" => prim("%:", M::Scalar(SM::Sqrt), D::Scalar(SD::Root), [0, 0, 0]),
        "^." => prim("^.", M::Scalar(SM::Ln), D::Scalar(SD::Log), [0, 0, 0]),
        "|" => prim("|", M::Scalar(SM::Abs), D::Scalar(SD::Residue), [0, 0, 0]),
        "<." => prim("<.", M::Scalar(SM::Floor), D::Scalar(SD::Min), [0, 0, 0]),
        ">." => prim(">.", M::Scalar(SM::Ceil), D::Scalar(SD::Max), [0, 0, 0]),
        "=" => prim("=", M::NotYet("self-classify (monadic =)"), D::Scalar(SD::Eq), [INF, 0, 0]),
        "<" => prim("<", M::Enclose(Enclose::Always), D::Scalar(SD::Lt), [INF, 0, 0]),
        ">" => prim(">", M::Open, D::Scalar(SD::Gt), [0, 0, 0]),
        "<:" => prim("<:", M::Scalar(SM::Dec), D::Scalar(SD::Le), [0, 0, 0]),
        ">:" => prim(">:", M::Scalar(SM::Inc), D::Scalar(SD::Ge), [0, 0, 0]),
        "+:" => prim("+:", M::Scalar(SM::Double), D::NotYet("nor (dyadic +:)"), [0, 0, 0]),
        "*:" => prim("*:", M::Scalar(SM::Square), D::NotYet("nand (dyadic *:)"), [0, 0, 0]),
        "-:" => prim("-:", M::Scalar(SM::Halve), D::Match, [0, INF, INF]),
        "-." => prim("-.", M::Scalar(SM::OneMinus), D::NotYet("less (dyadic -.)"), [0, 0, 0]),
        "*." => {
            prim("*.", M::NotYet("length/angle (monadic *.)"), D::Scalar(SD::Lcm), [INF, 0, 0])
        }
        "+." => {
            prim("+.", M::NotYet("real/imaginary (monadic +.)"), D::Scalar(SD::Gcd), [INF, 0, 0])
        }
        "~:" => prim("~:", M::NotYet("nub sieve (monadic ~:)"), D::Scalar(SD::Ne), [INF, 0, 0]),
        "~." => prim("~.", M::Nub, D::None, [INF, INF, INF]),
        "$" => prim("$", M::ShapeOf, D::Reshape, [INF, 1, INF]),
        "," => prim(",", M::Ravel, D::AppendLeading, [INF, INF, INF]),
        // `,.` is J's `,"_1`; `verb_for` wraps it in that rank.
        ",." => prim(",.", M::NotYet("ravel items (monadic ,.)"), D::AppendLeading, [INF, INF, INF]),
        ",:" => prim(",:", M::Itemize, D::Laminate, [INF, INF, INF]),
        "#" => prim("#", M::Tally, D::Copy, [INF, 1, INF]),
        "#." => prim("#.", M::DecodeBits, D::Decode, [1, 1, 1]),
        // The width of `#: y` comes from the largest value in the whole
        // argument, which is why the monad has infinite rank.
        "#:" => prim("#:", M::EncodeBits, D::Encode, [INF, 1, 0]),
        "!" => prim("!", M::Scalar(SM::Factorial), D::Scalar(SD::Binomial), [0, 0, 0]),
        "\":" => {
            prim("\":", M::Format, D::NotYet("format with a specification"), [INF, 1, INF])
        }
        "o." => prim("o.", M::Scalar(SM::Pi), D::Scalar(SD::Circle), [0, 0, 0]),
        "{" => prim("{", M::NotYet("catalogue (monadic {)"), D::From, [INF, 0, INF]),
        "{." => prim("{.", M::Head, D::Take, [INF, 1, INF]),
        "}." => prim("}.", M::Behead, D::Drop, [INF, 1, INF]),
        "{:" => prim("{:", M::Tail, D::None, [INF, INF, INF]),
        "}:" => prim("}:", M::Curtail, D::None, [INF, INF, INF]),
        "|." => prim("|.", M::Reverse, D::Rotate, [INF, 1, INF]),
        "|:" => prim("|:", M::TransposeAxes, D::NotYet("dyadic transpose"), [INF, 1, INF]),
        "i." => prim("i.", M::IotaJ, D::IndexOf { origin: 0 }, [1, INF, INF]),
        "i:" => prim("i:", M::Steps, D::IndexOfLast { origin: 0 }, [0, INF, INF]),
        "I." => prim(
            "I.",
            M::Indices { origin: 0, boxed_coords: false },
            D::IntervalIndex { offset: 0 },
            [1, 1, INF],
        ),
        "p:" => prim("p:", M::NthPrime, D::NotYet("prime metadata (dyadic p:)"), [0, 0, 0]),
        "q:" => prim("q:", M::PrimeFactors, D::NotYet("prime exponents (dyadic q:)"), [0, 0, 0]),
        "%." => prim("%.", M::MatrixInverse, D::MatrixDivide, [2, INF, 2]),
        // The monad takes the whole argument: one invocation is one run of
        // the generator, consumed in ravel order.
        "?" => prim(
            "?",
            M::Roll { origin: 0, fixed: false, float_at_zero: true },
            D::Deal { origin: 0, fixed: false },
            [INF, 0, 0],
        ),
        "?." => prim(
            "?.",
            M::Roll { origin: 0, fixed: true, float_at_zero: true },
            D::Deal { origin: 0, fixed: true },
            [INF, 0, 0],
        ),
        "{::" => prim("{::", M::NotYet("map (monadic {::)"), D::Fetch, [INF, INF, INF]),
        "e." => prim("e.", M::NotYet("raze-in (monadic e.)"), D::MemberJ, [INF, INF, INF]),
        "/:" => prim(
            "/:",
            M::GradeUp { origin: 0 },
            D::GradeSelect { down: false },
            [INF, INF, INF],
        ),
        "\\:" => prim(
            "\\:",
            M::GradeDown { origin: 0 },
            D::GradeSelect { down: true },
            [INF, INF, INF],
        ),
        ";" => prim(";", M::Raze, D::Link, [INF, INF, INF]),
        ";:" => prim(
            ";:",
            M::NotYet("words (monadic ;:)"),
            D::NotYet("sequential machine (dyadic ;:)"),
            [INF, INF, INF],
        ),
        "L." => prim(
            "L.",
            M::NotYet("level of (L.)"),
            D::None,
            [INF, INF, INF],
        ),
        "]" => prim("]", M::Same, D::Right, [INF, INF, INF]),
        "[" => prim("[", M::Same, D::Left, [INF, INF, INF]),
        "echo" => prim("echo", M::Echo, D::None, [INF, INF, INF]),
        _ => return None,
    })
}

/// The verb a word denotes. Every word but `,.` is a bare primitive; J's
/// `,.` is `,"_1`, so it carries that rank.
fn verb_for(word: &str) -> Option<Verb> {
    let p = primitive(word)?;
    if word == ",." {
        return Some(Verb::Rank(Box::new(Verb::Prim(p)), [-1, -1, -1]));
    }
    Some(Verb::Prim(p))
}

const ADVERBS: [&str; 6] = ["/", "\\", "/.", "\\.", "~", "}"];

/// Conjunction spellings. The ones without a meaning here are recognised so
/// that their diagnostic names the conjunction rather than the word.
const CONJUNCTIONS: [&str; 16] = [
    "\"", "@", "@.", "@:", "&", "&.", "&.:", "&:", "^:", ";.", "!.", "!:", "`", "`:", ".", ":",
];

fn adverb(word: &str) -> Option<&'static str> {
    ADVERBS.iter().copied().find(|&g| g == word)
}

fn conjunction(word: &str) -> Option<&'static str> {
    CONJUNCTIONS.iter().copied().find(|&g| g == word)
}

// ------------------------------------------------------------------- lexer

/// Split the source into sentences of fragments. Text segments are lexed;
/// each interpolation hole becomes a noun fragment holding its parameter.
fn lex(src: &SourceParts) -> Result<Vec<Vec<Frag>>> {
    let mut sentences: Vec<Vec<Frag>> = Vec::new();
    let mut cur: Vec<Frag> = Vec::new();
    for seg in &src.segments {
        match seg {
            Segment::Text { text, offset } => {
                let mut pos = 0usize;
                for (n, line) in text.split('\n').enumerate() {
                    if n > 0 && !cur.is_empty() {
                        sentences.push(std::mem::take(&mut cur));
                    }
                    lex_line(line, offset + pos, &mut cur)?;
                    pos += line.len() + 1;
                }
            }
            Segment::Param { index, offset, len } => {
                let span = Span::new(*offset, *offset + *len);
                cur.push(Frag::Noun(Expr::Param(*index, span)));
            }
        }
    }
    if !cur.is_empty() {
        sentences.push(cur);
    }
    Ok(sentences)
}

/// A numeric word's value. Kept apart from `Array` so that a list of words
/// can pick one element type for the whole vector.
#[derive(Clone, Copy, Debug)]
enum Num {
    I(i64),
    F(f64),
}

fn lex_line(text: &str, base: usize, out: &mut Vec<Frag>) -> Result<()> {
    let cs: Vec<(usize, char)> = text.char_indices().collect();
    let at = |i: usize| cs.get(i).map(|&(_, c)| c);
    let off = |i: usize| cs.get(i).map(|&(o, _)| o).unwrap_or(text.len());
    let span = |a: usize, b: usize| Span::new(base + off(a), base + off(b));
    let mut i = 0usize;
    while i < cs.len() {
        let c = cs[i].1;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // `NB.` is only a comment at the start of a word, which is where
        // this loop always stands.
        if c == 'N' && at(i + 1) == Some('B') && at(i + 2) == Some('.') {
            break;
        }
        if c == '\'' {
            let start = i;
            i += 1;
            let mut chars: Vec<char> = Vec::new();
            loop {
                match at(i) {
                    None => {
                        return Err(Error::parse(
                            "unterminated string literal",
                            span(start, cs.len()),
                        ));
                    }
                    Some('\'') if at(i + 1) == Some('\'') => {
                        chars.push('\'');
                        i += 2;
                    }
                    Some('\'') => {
                        i += 1;
                        break;
                    }
                    Some(ch) => {
                        chars.push(ch);
                        i += 1;
                    }
                }
            }
            // One character is an atom; anything else is a vector.
            let shape = if chars.len() == 1 { vec![] } else { vec![chars.len()] };
            let arr = Array::new(shape, Data::Char(chars.into()));
            out.push(Frag::Noun(Expr::Const(arr, span(start, i))));
            continue;
        }
        if starts_number(&cs, i) {
            // Numeric words separated only by blanks form one vector.
            let start = i;
            let mut nums: Vec<Num> = Vec::new();
            let mut end;
            loop {
                let ws = i;
                while at(i).is_some_and(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_') {
                    i += 1;
                }
                nums.push(parse_number(&text[off(ws)..off(i)], span(ws, i))?);
                end = i;
                let mut k = i;
                while at(k).is_some_and(char::is_whitespace) {
                    k += 1;
                }
                if k < cs.len() && starts_number(&cs, k) {
                    i = k;
                } else {
                    break;
                }
            }
            out.push(Frag::Noun(Expr::Const(num_array(&nums), span(start, end))));
            continue;
        }
        if c.is_ascii_alphabetic() {
            let start = i;
            i += 1;
            while at(i).is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                i += 1;
            }
            // An alphabetic word may be inflected into a primitive (`i.`).
            if matches!(at(i), Some('.') | Some(':')) {
                if let Some(v) = verb_for(&text[off(start)..off(i + 1)]) {
                    i += 1;
                    out.push(Frag::Verb(VerbFrag::V(v), span(start, i)));
                    continue;
                }
            }
            let word = &text[off(start)..off(i)];
            match verb_for(word) {
                Some(v) => out.push(Frag::Verb(VerbFrag::V(v), span(start, i))),
                None => out.push(Frag::Name(word.to_string(), span(start, i))),
            }
            continue;
        }
        // `{{` opens J's other explicit definition; it is never two words.
        if c == '{' && at(i + 1) == Some('{') {
            return Err(Error::not_yet("explicit definitions ({{ ... }})", span(i, i + 2)));
        }
        // A symbol word is one character plus a trailing inflection, which
        // always binds: `~:` is one word, never `~` followed by `:`. The
        // parentheses are the exception; they are never inflected.
        let inflectable = c != '(' && c != ')';
        let mut len =
            if inflectable && matches!(at(i + 1), Some('.') | Some(':')) { 2 } else { 1 };
        // A doubly inflected word (`&.:`) exists only where the table says
        // it does; everything else stops at one inflection.
        if len == 2 && at(i + 2) == Some(':') {
            let w = &text[off(i)..off(i + 3)];
            if conjunction(w).is_some() || verb_for(w).is_some() {
                len = 3;
            }
        }
        let word = &text[off(i)..off(i + len)];
        match symbol_frag(word, span(i, i + len)) {
            Some(frag) => {
                out.push(frag);
                i += len;
            }
            None => {
                return Err(Error::parse(format!("unknown word: {word}"), span(i, i + len)));
            }
        }
    }
    Ok(())
}

fn symbol_frag(word: &str, span: Span) -> Option<Frag> {
    Some(match word {
        "(" => Frag::LParen(span),
        ")" => Frag::RParen(span),
        "=." => Frag::AssignLocal(span),
        "=:" => Frag::AssignGlobal(span),
        "[:" => Frag::Verb(VerbFrag::Cap, span),
        // An inflected verb wins over the adverb its stem spells: `~.` is
        // the nub, never `~` followed by an inflection.
        _ => {
            if let Some(v) = verb_for(word) {
                Frag::Verb(VerbFrag::V(v), span)
            } else if let Some(g) = adverb(word) {
                Frag::Adverb(g, span)
            } else {
                Frag::Conj(conjunction(word)?, span)
            }
        }
    })
}

/// A numeric word starts with a digit, or with `_` used as a negative sign
/// or as infinity (`_`, `__`) — but not as the start of a name.
fn starts_number(cs: &[(usize, char)], i: usize) -> bool {
    let c = cs[i].1;
    if c.is_ascii_digit() {
        return true;
    }
    if c != '_' {
        return false;
    }
    match cs.get(i + 1).map(|&(_, c)| c) {
        None => true,
        Some(d) => d.is_ascii_digit() || d == '.' || !d.is_alphanumeric(),
    }
}

fn parse_number(word: &str, span: Span) -> Result<Num> {
    if word.contains('j') {
        return Err(Error::not_yet("complex literals", span));
    }
    if word.contains('x') {
        return Err(Error::not_yet("extended-precision integers", span));
    }
    if word.contains('r') {
        return Err(Error::not_yet("rationals", span));
    }
    if word == "_" {
        return Ok(Num::F(f64::INFINITY));
    }
    if word == "__" {
        return Ok(Num::F(f64::NEG_INFINITY));
    }
    let invalid = || Error::parse(format!("invalid number: {word}"), span);
    // `_` is J's negative sign, in the mantissa and after `e`.
    let mut norm = String::with_capacity(word.len());
    for (k, ch) in word.char_indices() {
        if ch == '_' {
            if k != 0 && !word[..k].ends_with('e') {
                return Err(invalid());
            }
            norm.push('-');
        } else {
            norm.push(ch);
        }
    }
    // Exponent notation yields a float, as a fractional part does.
    if norm.contains('.') || norm.contains('e') {
        norm.parse::<f64>().map(Num::F).map_err(|_| invalid())
    } else {
        norm.parse::<i64>().map(Num::I).map_err(|_| invalid())
    }
}

fn num_array(nums: &[Num]) -> Array {
    let shape = if nums.len() == 1 { vec![] } else { vec![nums.len()] };
    if nums.iter().any(|n| matches!(n, Num::F(_))) {
        let data = nums
            .iter()
            .map(|n| match n {
                Num::I(v) => *v as f64,
                Num::F(v) => *v,
            })
            .collect();
        Array::new(shape, Data::F64(data))
    } else {
        let data = nums
            .iter()
            .map(|n| match n {
                Num::I(v) => *v,
                Num::F(_) => 0,
            })
            .collect();
        Array::new(shape, Data::I64(data))
    }
}

// ------------------------------------------------------------------ parser

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    Monad1,
    Monad2,
    Dyad3,
    Adverb4,
    Conj5,
    Fork6,
    Bident7,
    Assign8,
    Paren9,
}

fn parse_sentence(tokens: Vec<Frag>, nouns: &HashSet<String>) -> Result<Expr> {
    let sentence = sentence_span(&tokens);
    let mut stack: Vec<Frag> = Vec::new();
    for frag in tokens.into_iter().rev() {
        stack.insert(0, frag);
        reduce(&mut stack, nouns)?;
    }
    stack.insert(0, Frag::Mark);
    reduce(&mut stack, nouns)?;
    if stack.len() == 2 {
        match stack.pop().expect("checked length") {
            f @ (Frag::Noun(_) | Frag::Name(..)) => return as_noun(f),
            Frag::VerbDef(name, verb, span) => return Ok(Expr::VerbDef { name, verb, span }),
            Frag::Verb(VerbFrag::V(_), span) => {
                return Err(Error::not_yet(
                    "tacit verb definitions (a sentence that is a verb)",
                    span,
                ));
            }
            _ => {}
        }
    }
    Err(Error::parse("syntax error", sentence))
}

fn sentence_span(tokens: &[Frag]) -> Span {
    tokens
        .iter()
        .map(Frag::span)
        .reduce(Span::merge)
        .unwrap_or_else(|| Span::new(0, 0))
}

fn reduce(stack: &mut Vec<Frag>, nouns: &HashSet<String>) -> Result<()> {
    while apply(stack, nouns)? {}
    Ok(())
}

/// The parse table: the first matching row wins, and matching restarts after
/// every reduction. Slot 0 is the leftmost (most recently pushed) fragment.
fn match_rule(s: &[Frag]) -> Option<Rule> {
    let is = |i: usize, f: fn(&Frag) -> bool| s.get(i).is_some_and(f);
    // Slot 0 is only ever context: an edge, or a fragment that keeps the
    // reduction from reaching further left than it should.
    let ctx = |i: usize| s.get(i).is_some_and(|f| f.is_edge() || f.is_avn());
    let verb_or_noun = |i: usize| s.get(i).is_some_and(|f| f.is_real_verb() || f.is_noun());
    if is(0, Frag::is_edge) && is(1, Frag::is_real_verb) && is(2, Frag::is_noun) {
        return Some(Rule::Monad1);
    }
    if ctx(0) && is(1, Frag::is_verb) && is(2, Frag::is_real_verb) && is(3, Frag::is_noun) {
        return Some(Rule::Monad2);
    }
    if ctx(0) && is(1, Frag::is_noun) && is(2, Frag::is_real_verb) && is(3, Frag::is_noun) {
        return Some(Rule::Dyad3);
    }
    if ctx(0) && verb_or_noun(1) && is(2, Frag::is_adverb) {
        return Some(Rule::Adverb4);
    }
    if ctx(0) && verb_or_noun(1) && is(2, Frag::is_conj) && verb_or_noun(3) {
        return Some(Rule::Conj5);
    }
    if ctx(0)
        && s.get(1).is_some_and(|f| f.is_verb() || f.is_noun())
        && is(2, Frag::is_real_verb)
        && is(3, Frag::is_real_verb)
    {
        return Some(Rule::Fork6);
    }
    if is(0, Frag::is_edge) && is(1, Frag::is_cavn) && is(2, Frag::is_cavn) {
        return Some(Rule::Bident7);
    }
    if is(0, Frag::is_noun) && is(1, Frag::is_assign) && is(2, Frag::is_cavn) {
        return Some(Rule::Assign8);
    }
    if matches!(s.first(), Some(Frag::LParen(_)))
        && is(1, Frag::is_cavn)
        && matches!(s.get(2), Some(Frag::RParen(_)))
    {
        return Some(Rule::Paren9);
    }
    None
}

fn take(stack: &mut Vec<Frag>, range: Range<usize>) -> Vec<Frag> {
    stack.drain(range).collect()
}

fn apply(stack: &mut Vec<Frag>, nouns: &HashSet<String>) -> Result<bool> {
    let Some(rule) = match_rule(stack) else {
        return Ok(false);
    };
    match rule {
        Rule::Monad1 => {
            let mut t = take(stack, 1..3);
            let y = t.pop().expect("two slots");
            let v = t.pop().expect("two slots");
            let frag = monad(v, y)?;
            stack.insert(1, frag);
        }
        Rule::Monad2 => {
            let mut t = take(stack, 2..4);
            let y = t.pop().expect("two slots");
            let v = t.pop().expect("two slots");
            let frag = monad(v, y)?;
            stack.insert(2, frag);
        }
        Rule::Dyad3 => {
            let mut t = take(stack, 1..4);
            let y = t.pop().expect("three slots");
            let v = t.pop().expect("three slots");
            let x = t.pop().expect("three slots");
            let frag = dyad(x, v, y)?;
            stack.insert(1, frag);
        }
        Rule::Adverb4 => {
            let mut t = take(stack, 1..3);
            let a = t.pop().expect("two slots");
            let u = t.pop().expect("two slots");
            let frag = apply_adverb(u, a)?;
            stack.insert(1, frag);
        }
        Rule::Conj5 => {
            let mut t = take(stack, 1..4);
            let v = t.pop().expect("three slots");
            let c = t.pop().expect("three slots");
            let u = t.pop().expect("three slots");
            let frag = apply_conj(u, c, v)?;
            stack.insert(1, frag);
        }
        Rule::Fork6 => {
            let mut t = take(stack, 1..4);
            let h = t.pop().expect("three slots");
            let g = t.pop().expect("three slots");
            let f = t.pop().expect("three slots");
            let frag = apply_fork(f, g, h)?;
            stack.insert(1, frag);
        }
        Rule::Bident7 => {
            let mut t = take(stack, 1..3);
            let b = t.pop().expect("two slots");
            let a = t.pop().expect("two slots");
            let frag = apply_bident(a, b, nouns)?;
            stack.insert(1, frag);
        }
        Rule::Assign8 => {
            let mut t = take(stack, 0..3);
            let value = t.pop().expect("three slots");
            let _assign = t.pop().expect("three slots");
            let target = t.pop().expect("three slots");
            let frag = apply_assign(target, value)?;
            stack.insert(0, frag);
        }
        Rule::Paren9 => {
            let mut t = take(stack, 0..3);
            t.pop();
            let inner = t.pop().expect("three slots");
            stack.insert(0, inner);
        }
    }
    Ok(true)
}

// --------------------------------------------------------------- lowering

fn as_noun(f: Frag) -> Result<Expr> {
    match f {
        Frag::Noun(e) => Ok(e),
        Frag::Name(n, s) => Ok(Expr::Name(n, s)),
        other => Err(Error::internal(format!("expected a noun fragment, got {other:?}"))),
    }
}

fn as_verb(f: Frag) -> Result<(Verb, Span)> {
    match f {
        Frag::Verb(VerbFrag::V(v), s) => Ok((v, s)),
        other => Err(Error::internal(format!("expected a verb fragment, got {other:?}"))),
    }
}

/// The literal array behind a noun fragment, if it is one. Derived verbs that
/// capture a noun (rank specifications, noun forks) need the value now.
fn as_const(f: &Frag) -> Option<&Array> {
    match f {
        Frag::Noun(Expr::Const(a, _)) => Some(a),
        _ => None,
    }
}

fn monad(v: Frag, y: Frag) -> Result<Frag> {
    let (verb, vspan) = as_verb(v)?;
    let y = as_noun(y)?;
    let span = Span::merge(vspan, y.span());
    Ok(Frag::Noun(Expr::Monad { verb, y: Box::new(y), span }))
}

fn dyad(x: Frag, v: Frag, y: Frag) -> Result<Frag> {
    let x = as_noun(x)?;
    let (verb, vspan) = as_verb(v)?;
    let y = as_noun(y)?;
    let span = Span::merge(Span::merge(x.span(), vspan), y.span());
    Ok(Frag::Noun(Expr::Dyad { verb, x: Box::new(x), y: Box::new(y), span }))
}

fn apply_adverb(u: Frag, a: Frag) -> Result<Frag> {
    let Frag::Adverb(glyph, aspan) = a else {
        return Err(Error::internal("expected an adverb fragment"));
    };
    let span = Span::merge(u.span(), aspan);
    // `}` is the one adverb whose operand is a noun: `m}` amends at the
    // indices m. A verb operand is J's other amend, which libjay has not.
    if glyph == "}" {
        if !u.is_real_verb() {
            let m = as_const(&u)
                .cloned()
                .ok_or_else(|| Error::not_yet("amend over a computed index", span))?;
            return Ok(Frag::Verb(VerbFrag::V(Verb::Amend(m)), span));
        }
        return Err(Error::not_yet("amend with a verb operand (u})", span));
    }
    if !u.is_real_verb() {
        return Err(Error::not_yet("noun-operand adverbs", span));
    }
    let (v, _) = as_verb(u)?;
    let derived = match glyph {
        "/" => Verb::Reduce(Box::new(v)),
        "\\" => Verb::Windowed(Box::new(v), WindowKind::Prefix),
        "\\." => Verb::Windowed(Box::new(v), WindowKind::Suffix),
        "~" => Verb::Commute(Box::new(v)),
        "/." => Verb::Key(Box::new(v)),
        _ => return Err(Error::not_yet(format!("adverb ({glyph})"), span)),
    };
    Ok(Frag::Verb(VerbFrag::V(derived), span))
}

fn apply_conj(u: Frag, c: Frag, v: Frag) -> Result<Frag> {
    let Frag::Conj(glyph, cspan) = c else {
        return Err(Error::internal("expected a conjunction fragment"));
    };
    let span = Span::merge(Span::merge(u.span(), cspan), v.span());
    match glyph {
        "\"" => {
            let f = verb_operand(u, span)?;
            if v.is_verb() {
                return Err(Error::not_yet("verb rank (u\"v)", span));
            }
            let ranks = rank_spec(&v, span)?;
            Ok(Frag::Verb(VerbFrag::V(Verb::Rank(Box::new(f), ranks)), span))
        }
        "@:" => {
            let f = verb_operand(u, span)?;
            let g = verb_operand(v, span)?;
            Ok(Frag::Verb(VerbFrag::V(Verb::Atop(Box::new(f), Box::new(g))), span))
        }
        // `u@v` is `u@:v` applied at v's own ranks: one v-cell at a time,
        // with u run on each result. That difference in rank is all that
        // separates the two spellings.
        "@" => {
            let f = verb_operand(u, span)?;
            let g = verb_operand(v, span)?;
            let ranks = g.ranks();
            let atop = Verb::Atop(Box::new(f), Box::new(g));
            Ok(Frag::Verb(VerbFrag::V(Verb::Rank(Box::new(atop), ranks)), span))
        }
        "&" => compose(u, v, false, span),
        "&:" => compose(u, v, true, span),
        // `u&.>` is the one under libjay has: open each box, apply u, box
        // the result again. The general conjunction needs verb inverses.
        "&." if is_open(&v) => {
            let f = verb_operand(u, span)?;
            Ok(Frag::Verb(VerbFrag::V(Verb::Each(Box::new(f), Enclose::Always)), span))
        }
        "&." | "&.:" => Err(Error::not_yet("under (&.) — needs verb inverses", span)),
        "^:" => {
            let f = verb_operand(u, span)?;
            if v.is_verb() {
                // `u^:v` asks v for the number of applications; the while
                // loop is that verb under `^:_`.
                let g = verb_operand(v, span)?;
                let p = Verb::PowerV(Box::new(f), Box::new(g));
                return Ok(Frag::Verb(VerbFrag::V(p), span));
            }
            let p = power_spec(&v, span)?;
            Ok(Frag::Verb(VerbFrag::V(Verb::PowerN(Box::new(f), p)), span))
        }
        ";." => {
            let f = verb_operand(u, span)?;
            let n = one_atom(&v, "cut", span)?;
            if n.fract() != 0.0 || !matches!(n as i64, -2..=2) {
                return Err(Error::not_yet(format!("cut (u;.{n})"), span));
            }
            Ok(Frag::Verb(VerbFrag::V(Verb::Cut(Box::new(f), n as i64)), span))
        }
        // `u!.n` is the tolerance for the verbs whose meaning uses one; on
        // any other verb J's `!.` specifies a fill, which is its own
        // feature and not this one.
        "!." => {
            let f = verb_operand(u, span)?;
            let n = one_atom(&v, "fit", span)?;
            if !f.uses_tolerance() {
                return Err(Error::not_yet(
                    format!("fill specification ({}!.n)", f.name()),
                    span,
                ));
            }
            // J refuses a tolerance above 2^-34, and so does libjay.
            if !(0.0..=LARGEST_TOLERANCE).contains(&n) {
                return Err(Error::domain(
                    format!("a comparison tolerance must be between 0 and {LARGEST_TOLERANCE}"),
                    span,
                ));
            }
            Ok(Frag::Verb(VerbFrag::V(Verb::Fit(Box::new(f), n)), span))
        }
        // `3 : '...'` and its relatives are J's explicit definitions, not a
        // composition; the diagnostic should say so.
        ":" => Err(Error::not_yet("explicit definitions (3 : '...' and 4 : '...')", span)),
        _ => Err(Error::not_yet(format!("composition ({glyph})"), span)),
    }
}

/// `u&v` and `u&:v`, in all three shapes the conjunction takes.
///
/// With two verbs it composes: monadically `u v y`, dyadically
/// `(v x) u (v y)` — and `&` runs that at v's monadic rank on both sides
/// while `&:` runs it on the arguments whole. With a noun on either side it
/// bonds that noun into the dyad, giving a verb with a monadic valence only;
/// `&:` takes no noun at all.
fn compose(u: Frag, v: Frag, infinite: bool, span: Span) -> Result<Frag> {
    let verb = |v: Verb| Ok(Frag::Verb(VerbFrag::V(v), span));
    if infinite || (!u.is_noun() && !v.is_noun()) {
        let f = verb_operand(u, span)?;
        let g = verb_operand(v, span)?;
        let monadic_rank = g.ranks()[0];
        let composed = Verb::Compose(Box::new(f), Box::new(g));
        if infinite {
            return verb(composed);
        }
        return verb(Verb::Rank(Box::new(composed), [monadic_rank; 3]));
    }
    if u.is_noun() && v.is_noun() {
        return Err(Error::not_yet("noun-operand conjunctions", span));
    }
    // A bond applies its verb dyadically, so it takes the rank of the side
    // the argument arrives on.
    if u.is_noun() {
        let m = bond_noun(&u, span)?;
        let g = as_verb(v)?.0;
        let rank = g.ranks()[2];
        return verb(Verb::Rank(Box::new(Verb::BondLeft(m, Box::new(g))), [rank; 3]));
    }
    let f = as_verb(u)?.0;
    let n = bond_noun(&v, span)?;
    let rank = f.ranks()[1];
    verb(Verb::Rank(Box::new(Verb::BondRight(Box::new(f), n)), [rank; 3]))
}

/// The largest comparison tolerance `!.` accepts, as J's does: 2^-34.
const LARGEST_TOLERANCE: f64 = 5.820_766_091_346_741e-11;

/// A conjunction's single numeric noun operand.
fn one_atom(f: &Frag, what: &str, span: Span) -> Result<f64> {
    let Some(arr) = as_const(f) else {
        return Err(Error::not_yet(format!("a computed {what} specification"), span));
    };
    let Some(vals) = arr.to_f64_vec() else {
        return Err(Error::parse(format!("{what} takes a numeric operand"), span));
    };
    match vals[..] {
        [n] => Ok(n),
        _ => Err(Error::parse(format!("{what} takes one atom"), span)),
    }
}

/// The array a bonded noun operand holds; it has to be known now.
fn bond_noun(f: &Frag, span: Span) -> Result<Array> {
    as_const(f)
        .cloned()
        .ok_or_else(|| Error::not_yet("bonds over a non-literal noun", span))
}

/// True for the fragment holding the primitive `>`, the only right operand
/// `&.` accepts.
fn is_open(f: &Frag) -> bool {
    matches!(f, Frag::Verb(VerbFrag::V(Verb::Prim(p)), _) if p.monad == MonadOp::Open)
}

fn verb_operand(f: Frag, span: Span) -> Result<Verb> {
    if f.is_noun() {
        return Err(Error::not_yet("noun-operand conjunctions", span));
    }
    Ok(as_verb(f)?.0)
}

/// `u"n`: 1 atom applies to every valence, 2 atoms are `left right` with the
/// monadic rank taken from the right, 3 atoms are given in full.
fn rank_spec(f: &Frag, span: Span) -> Result<[i64; 3]> {
    let Some(arr) = as_const(f) else {
        return Err(Error::not_yet("computed rank specifications", span));
    };
    let Some(vals) = arr.to_f64_vec() else {
        return Err(Error::parse("rank must be numeric", span));
    };
    if vals.is_empty() || vals.len() > 3 {
        return Err(Error::parse("rank takes 1 to 3 atoms", span));
    }
    let mut r = Vec::with_capacity(vals.len());
    for x in vals {
        if x == f64::INFINITY {
            r.push(RANK_INF);
        } else if x == f64::NEG_INFINITY {
            r.push(-RANK_INF);
        } else if x.fract() != 0.0 {
            return Err(Error::parse("rank must be an integer", span));
        } else {
            r.push(x as i64);
        }
    }
    Ok(match r.len() {
        1 => [r[0], r[0], r[0]],
        2 => [r[1], r[0], r[1]],
        _ => [r[0], r[1], r[2]],
    })
}

/// `u^:n`: one nonnegative integer atom, or `_` for "iterate until the
/// result stops changing".
fn power_spec(f: &Frag, span: Span) -> Result<Power> {
    let Some(arr) = as_const(f) else {
        return Err(Error::not_yet("computed power (u^:n)", span));
    };
    let Some(vals) = arr.to_f64_vec() else {
        return Err(Error::parse("power must be numeric", span));
    };
    let [n] = vals[..] else {
        return Err(Error::not_yet("power over a list of counts (u^:n)", span));
    };
    if n == f64::INFINITY {
        return Ok(Power::Converge);
    }
    if n < 0.0 {
        return Err(Error::not_yet("obverse (u^:_1 and other negative powers)", span));
    }
    if n.fract() != 0.0 {
        return Err(Error::parse("power must be a whole number", span));
    }
    Ok(Power::Times(n as u64))
}

fn apply_fork(f: Frag, g: Frag, h: Frag) -> Result<Frag> {
    let span = Span::merge(Span::merge(f.span(), g.span()), h.span());
    let (gv, _) = as_verb(g)?;
    let (hv, _) = as_verb(h)?;
    match f {
        // `[: g h` is g atop h: the left tine produces nothing to fork over.
        Frag::Verb(VerbFrag::Cap, _) => {
            Ok(Frag::Verb(VerbFrag::V(Verb::Atop(Box::new(gv), Box::new(hv))), span))
        }
        Frag::Verb(VerbFrag::V(fv), _) => Ok(Frag::Verb(
            VerbFrag::V(Verb::Fork(Box::new(fv), Box::new(gv), Box::new(hv))),
            span,
        )),
        noun => {
            let Some(arr) = as_const(&noun) else {
                return Err(Error::not_yet("noun forks over a non-literal noun", span));
            };
            Ok(Frag::Verb(
                VerbFrag::V(Verb::NounFork(arr.clone(), Box::new(gv), Box::new(hv))),
                span,
            ))
        }
    }
}

fn apply_bident(a: Frag, b: Frag, nouns: &HashSet<String>) -> Result<Frag> {
    let span = Span::merge(a.span(), b.span());
    // A name here is not a verb, or it would have been substituted; if it
    // is not a value either, that is what is wrong with the sentence, and
    // it is what the reference reports.
    if let Frag::Name(n, nspan) = &a {
        if !nouns.contains(n) {
            return Err(Error::new(
                ErrorKind::Value,
                format!("undefined name: {n}"),
                Some(*nspan),
            ));
        }
    }
    if a.is_real_verb() && b.is_real_verb() {
        let (f, _) = as_verb(a)?;
        let (g, _) = as_verb(b)?;
        return Ok(Frag::Verb(VerbFrag::V(Verb::Hook(Box::new(f), Box::new(g))), span));
    }
    Err(Error::not_yet("that bident", span))
}

fn apply_assign(target: Frag, value: Frag) -> Result<Frag> {
    let span = Span::merge(target.span(), value.span());
    match target {
        // `=.` and `=:` both lower to Expr::Assign: the IR has a single
        // environment for now, so local and global do not yet differ.
        Frag::Name(name, _) => match value {
            // Naming a verb is settled here, at parse time: `parse` records
            // the name and substitutes the verb into later sentences.
            Frag::Verb(VerbFrag::V(verb), _) => Ok(Frag::VerbDef(name, verb, span)),
            Frag::Verb(VerbFrag::Cap, _) => Err(Error::not_yet("assigning [: on its own", span)),
            v if v.is_noun() => {
                let value = as_noun(v)?;
                Ok(Frag::Noun(Expr::Assign { name, value: Box::new(value), span }))
            }
            _ => Err(Error::not_yet("adverb and conjunction assignment", span)),
        },
        Frag::Noun(_) => Err(Error::not_yet("multiple assignment", span)),
        other => Err(Error::internal(format!("expected an assignment target, got {other:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dtype::DType;
    use crate::error::ErrorKind;
    use rstest::rstest;

    fn parse_str(src: &str) -> Result<Vec<Expr>> {
        parse(&SourceParts::from_source(src).expect("source parts"))
    }

    /// Parse literal text with no interpolation. `{. ` and `}.` are J words
    /// that `from_source` would read as a hole, so those tests take the
    /// pre-split path instead.
    fn one_literal(src: &str) -> Expr {
        let sp = SourceParts::from_parts(&[src], &[]);
        let mut s = parse(&sp).unwrap_or_else(|e| panic!("parse of {src:?} failed: {e}"));
        assert_eq!(s.len(), 1, "expected one sentence in {src:?}");
        s.pop().expect("one sentence")
    }

    fn stmts(src: &str) -> Vec<Expr> {
        parse_str(src).unwrap_or_else(|e| panic!("parse of {src:?} failed: {e}"))
    }

    /// The single statement of a one-sentence program.
    fn one(src: &str) -> Expr {
        let mut s = stmts(src);
        assert_eq!(s.len(), 1, "expected one sentence in {src:?}");
        s.pop().expect("one sentence")
    }

    fn err(src: &str) -> Error {
        match parse_str(src) {
            Ok(v) => panic!("expected an error for {src:?}, got {v:?}"),
            Err(e) => e,
        }
    }

    // The shape inspectors return owned copies so that a test can inspect
    // the result of `one(...)` in one expression.

    fn konst(e: &Expr) -> Array {
        match e {
            Expr::Const(a, _) => a.clone(),
            other => panic!("expected a constant, got {other:?}"),
        }
    }

    fn ints(e: &Expr) -> Vec<i64> {
        konst(e).as_i64_slice().expect("integer data").to_vec()
    }

    fn prim_of(v: &Verb) -> Prim {
        match v {
            Verb::Prim(p) => *p,
            other => panic!("expected a primitive, got {other:?}"),
        }
    }

    fn monad_of(e: &Expr) -> (Verb, Expr) {
        match e {
            Expr::Monad { verb, y, .. } => (verb.clone(), (**y).clone()),
            other => panic!("expected a monad, got {other:?}"),
        }
    }

    fn dyad_of(e: &Expr) -> (Verb, Expr, Expr) {
        match e {
            Expr::Dyad { verb, x, y, .. } => (verb.clone(), (**x).clone(), (**y).clone()),
            other => panic!("expected a dyad, got {other:?}"),
        }
    }

    // ------------------------------------------------------------- literals

    #[test]
    fn single_number_is_an_atom() {
        let e = one("5");
        assert_eq!(konst(&e).shape, Vec::<usize>::new());
        assert_eq!(ints(&e), vec![5]);
        assert_eq!(e.span(), Span::new(0, 1));
    }

    #[test]
    fn adjacent_numbers_merge_into_one_vector() {
        let e = one("1 2 3");
        assert_eq!(konst(&e).shape, vec![3]);
        assert_eq!(ints(&e), vec![1, 2, 3]);
        assert_eq!(e.span(), Span::new(0, 5));
    }

    #[test]
    fn a_float_makes_the_whole_vector_float() {
        let a = konst(&one("1 2.5 3"));
        assert_eq!(a.dtype(), DType::F64);
        assert_eq!(a.as_f64_slice(), Some(&[1.0, 2.5, 3.0][..]));
    }

    #[test]
    fn negatives_and_infinities() {
        let a = konst(&one("_3 1.5 _ __"));
        assert_eq!(a.shape, vec![4]);
        let v = a.as_f64_slice().expect("float vector");
        assert_eq!(v[0], -3.0);
        assert_eq!(v[1], 1.5);
        assert!(v[2].is_infinite() && v[2] > 0.0);
        assert!(v[3].is_infinite() && v[3] < 0.0);
    }

    #[test]
    fn negative_integers_stay_integers() {
        let a = konst(&one("_3 _4"));
        assert_eq!(a.dtype(), DType::I64);
        assert_eq!(a.as_i64_slice(), Some(&[-3i64, -4][..]));
    }

    #[rstest]
    #[case("1e3", 1000.0)]
    #[case("1e_3", 0.001)]
    #[case("2.5e2", 250.0)]
    #[case("_1.5", -1.5)]
    fn exponent_and_sign_forms(#[case] src: &str, #[case] want: f64) {
        let a = konst(&one(src));
        assert_eq!(a.dtype(), DType::F64);
        assert_eq!(a.to_f64_vec().expect("numeric"), vec![want]);
    }

    #[test]
    fn adjacent_numbers_stop_at_a_non_number() {
        // `i.` after a vector is a separate word, not numeric characters.
        let (_, x, y) = dyad_of(&one("2 3 i. 4"));
        assert_eq!(konst(&x).shape, vec![2]);
        assert_eq!(konst(&y).shape, Vec::<usize>::new());
    }

    #[test]
    fn string_of_several_characters_is_a_vector() {
        let e = one("'abc'");
        let a = konst(&e);
        assert_eq!(a.shape, vec![3]);
        assert_eq!(a.data, Data::Char(vec!['a', 'b', 'c'].into()));
        assert_eq!(e.span(), Span::new(0, 5));
    }

    #[test]
    fn one_character_string_is_an_atom() {
        let a = konst(&one("'a'"));
        assert_eq!(a.shape, Vec::<usize>::new());
        assert_eq!(a.data, Data::Char(vec!['a'].into()));
    }

    #[test]
    fn empty_string_is_an_empty_vector() {
        let a = konst(&one("''"));
        assert_eq!(a.shape, vec![0]);
        assert_eq!(a.dtype(), DType::Char);
    }

    #[test]
    fn doubled_quote_is_an_escaped_quote() {
        let a = konst(&one("'it''s'"));
        assert_eq!(a.shape, vec![4]);
        assert_eq!(a.data, Data::Char(vec!['i', 't', '\'', 's'].into()));
    }

    #[test]
    fn unterminated_string_is_a_parse_error() {
        let e = err("'abc");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.msg.contains("unterminated"), "{}", e.msg);
        assert_eq!(e.span, Some(Span::new(0, 4)));
    }

    // ------------------------------------------------------------- comments

    #[test]
    fn comment_runs_to_end_of_line() {
        let e = one("1 2 NB. and the rest + - ' is ignored");
        assert_eq!(konst(&e).shape, vec![2]);
    }

    #[test]
    fn comment_only_line_yields_no_sentence() {
        assert!(stmts("NB. nothing here").is_empty());
        let s = stmts("NB. header\n5");
        assert_eq!(s.len(), 1);
        assert_eq!(ints(&s[0]), vec![5]);
    }

    #[test]
    fn nb_inside_a_name_is_not_a_comment() {
        // `aNB` is a name; only a whole word `NB.` starts a comment.
        match one("aNB") {
            Expr::Name(n, _) => assert_eq!(n, "aNB"),
            other => panic!("expected a name, got {other:?}"),
        }
    }

    // -------------------------------------------------------------- parsing

    #[test]
    fn empty_program_has_no_sentences() {
        assert!(stmts("").is_empty());
        assert!(stmts("\n\n").is_empty());
    }

    #[test]
    fn trains_of_dyads_are_right_associative() {
        let e = one("1 + 2 + 3");
        let (v, x, y) = dyad_of(&e);
        assert_eq!(prim_of(&v).name, "+");
        assert_eq!(ints(&x), vec![1]);
        let (v2, x2, y2) = dyad_of(&y);
        assert_eq!(prim_of(&v2).name, "+");
        assert_eq!(ints(&x2), vec![2]);
        assert_eq!(ints(&y2), vec![3]);
        assert_eq!(e.span(), Span::new(0, 9));
    }

    #[test]
    fn a_verb_with_no_left_argument_is_a_monad() {
        let e = one("- 5");
        let (v, y) = monad_of(&e);
        assert_eq!(prim_of(&v).monad, MonadOp::Scalar(ScalarMonad::Neg));
        assert_eq!(ints(&y), vec![5]);
        assert_eq!(e.span(), Span::new(0, 3));
    }

    #[test]
    fn a_verb_with_a_left_argument_is_a_dyad() {
        let (v, _, _) = dyad_of(&one("1 - 5"));
        assert_eq!(prim_of(&v).dyad, DyadOp::Scalar(ScalarDyad::Sub));
    }

    #[test]
    fn a_monad_binds_to_the_right_inside_a_dyad() {
        let (v, x, y) = dyad_of(&one("2 * - 3"));
        assert_eq!(prim_of(&v).name, "*");
        assert_eq!(ints(&x), vec![2]);
        let (mv, my) = monad_of(&y);
        assert_eq!(prim_of(&mv).name, "-");
        assert_eq!(ints(&my), vec![3]);
    }

    #[test]
    fn parentheses_group_the_left_argument() {
        let (v, x, y) = dyad_of(&one("(1 + 2) * 3"));
        assert_eq!(prim_of(&v).name, "*");
        let (iv, _, _) = dyad_of(&x);
        assert_eq!(prim_of(&iv).name, "+");
        // The parentheses are dropped; the span covers what they held.
        assert_eq!(x.span(), Span::new(1, 6));
        assert_eq!(ints(&y), vec![3]);
    }

    #[test]
    fn names_are_nouns() {
        match one("x") {
            Expr::Name(n, s) => {
                assert_eq!(n, "x");
                assert_eq!(s, Span::new(0, 1));
            }
            other => panic!("expected a name, got {other:?}"),
        }
        let (_, x, y) = dyad_of(&one("x + y"));
        assert!(matches!(x, Expr::Name(..)));
        assert!(matches!(y, Expr::Name(..)));
    }

    #[test]
    fn echo_is_a_verb() {
        let (v, y) = monad_of(&one("echo 5"));
        assert_eq!(prim_of(&v).monad, MonadOp::Echo);
        assert_eq!(ints(&y), vec![5]);
    }

    #[test]
    fn inflected_letter_words_are_primitives() {
        let (v, _) = monad_of(&one("i. 3"));
        let p = prim_of(&v);
        assert_eq!(p.monad, MonadOp::IotaJ);
        assert_eq!(p.ranks, [1, RANK_INF, RANK_INF]);
    }

    #[rstest]
    #[case("|: 1 2 3", MonadOp::TransposeAxes)]
    #[case("$ 1 2 3", MonadOp::ShapeOf)]
    #[case("# 1 2 3", MonadOp::Tally)]
    #[case(", 1 2 3", MonadOp::Ravel)]
    #[case("%: 1 2 3", MonadOp::Scalar(ScalarMonad::Sqrt))]
    #[case("<. 1.5", MonadOp::Scalar(ScalarMonad::Floor))]
    fn inflected_symbol_words(#[case] src: &str, #[case] want: MonadOp) {
        let (v, _) = monad_of(&one(src));
        assert_eq!(prim_of(&v).monad, want);
    }

    #[rstest]
    #[case("{. 1 2 3", MonadOp::Head, DyadOp::Take)]
    #[case("}. 1 2 3", MonadOp::Behead, DyadOp::Drop)]
    fn brace_words(#[case] src: &str, #[case] monad: MonadOp, #[case] dyad: DyadOp) {
        let (v, _) = monad_of(&one_literal(src));
        let p = prim_of(&v);
        assert_eq!(p.monad, monad);
        assert_eq!(p.dyad, dyad);
        assert_eq!(p.ranks, [RANK_INF, 1, RANK_INF]);
    }

    #[test]
    fn a_brace_word_takes_a_left_argument() {
        let (v, x, y) = dyad_of(&one_literal("2 {. 1 2 3"));
        assert_eq!(prim_of(&v).dyad, DyadOp::Take);
        assert_eq!(ints(&x), vec![2]);
        assert_eq!(konst(&y).shape, vec![3]);
    }

    #[rstest]
    #[case("2 $ 1 2 3", DyadOp::Reshape)]
    #[case("2 [ 3", DyadOp::Left)]
    #[case("2 ] 3", DyadOp::Right)]
    #[case("2 <. 3", DyadOp::Scalar(ScalarDyad::Min))]
    #[case("2 >: 3", DyadOp::Scalar(ScalarDyad::Ge))]
    fn dyadic_primitives(#[case] src: &str, #[case] want: DyadOp) {
        let (v, _, _) = dyad_of(&one(src));
        assert_eq!(prim_of(&v).dyad, want);
    }

    #[test]
    fn unimplemented_meanings_reach_the_verb_not_the_parser() {
        let (v, _, _) = dyad_of(&one("2 *: 8"));
        assert_eq!(prim_of(&v).dyad, DyadOp::NotYet("nand (dyadic *:)"));
        let (v, _) = monad_of(&one("= 1 2"));
        assert_eq!(prim_of(&v).monad, MonadOp::NotYet("self-classify (monadic =)"));
    }

    #[test]
    fn multiple_sentences_become_multiple_statements() {
        let s = stmts("a =. 1 2\n+/ a\n");
        assert_eq!(s.len(), 2);
        assert!(matches!(s[0], Expr::Assign { .. }));
        assert!(matches!(s[1], Expr::Monad { .. }));
    }

    // ------------------------------------------------------------ modifiers

    #[test]
    fn an_adverb_binds_before_the_verb_is_applied() {
        let e = one("+/ 1 2 3");
        let (v, y) = monad_of(&e);
        match &v {
            Verb::Reduce(inner) => assert_eq!(prim_of(inner).name, "+"),
            other => panic!("expected a reduction, got {other:?}"),
        }
        assert_eq!(konst(&y).shape, vec![3]);
        assert_eq!(e.span(), Span::new(0, 8));
    }

    #[test]
    fn rank_applies_to_the_derived_verb() {
        let (v, _) = monad_of(&one("+/\"1 m"));
        match &v {
            Verb::Rank(inner, ranks) => {
                assert_eq!(*ranks, [1, 1, 1]);
                assert!(matches!(**inner, Verb::Reduce(_)), "got {inner:?}");
            }
            other => panic!("expected a ranked verb, got {other:?}"),
        }
    }

    #[rstest]
    #[case("+\"1 m", [1, 1, 1])]
    #[case("+\"1 2 m", [2, 1, 2])]
    #[case("+\"0 1 2 m", [0, 1, 2])]
    #[case("+\"_ m", [RANK_INF, RANK_INF, RANK_INF])]
    #[case("+\"_1 m", [-1, -1, -1])]
    #[case("+\"2.0 m", [2, 2, 2])]
    fn rank_specifications(#[case] src: &str, #[case] want: [i64; 3]) {
        let (v, _) = monad_of(&one(src));
        assert_eq!(v.ranks(), want);
    }

    #[test]
    fn rank_must_be_one_to_three_integer_atoms() {
        let e = err("+\"1 2 3 4 m");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.msg.contains("1 to 3 atoms"), "{}", e.msg);
        let e = err("+\"1.5 m");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.msg.contains("integer"), "{}", e.msg);
        let e = err("+\"'a' m");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.msg.contains("numeric"), "{}", e.msg);
    }

    #[test]
    fn verb_rank_is_not_supported_yet() {
        let e = err("+\"- m");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("verb rank"), "{}", e.msg);
    }

    #[test]
    fn computed_rank_is_not_supported_yet() {
        let e = err("+\"{r} m");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("computed rank"), "{}", e.msg);
    }

    #[test]
    fn atop_conjunction() {
        let (v, _) = monad_of(&one("+/ @: , y"));
        match &v {
            Verb::Atop(f, g) => {
                assert!(matches!(**f, Verb::Reduce(_)), "got {f:?}");
                assert_eq!(prim_of(g).name, ",");
            }
            other => panic!("expected an atop, got {other:?}"),
        }
    }

    #[rstest]
    #[case("+ &. - y", "under (&.)")]
    #[case("+ &.: - y", "under (&.)")]
    #[case("+ ^: {n} y", "computed power")]
    #[case("+ ^: _1 y", "obverse")]
    #[case("(1 + 2) & , y", "bonds over a non-literal noun")]
    fn other_conjunctions_are_not_supported_yet(#[case] src: &str, #[case] msg: &str) {
        let e = err(src);
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains(msg), "{}", e.msg);
    }

    #[test]
    fn atop_at_rank_and_compose() {
        // `u@v` is `u@:v` at v's ranks; `u&v` is the composition at v's
        // monadic rank; `u&:v` is that composition on the arguments whole.
        let (v, _) = monad_of(&one("+/ @ (,\"1) y"));
        match &v {
            Verb::Rank(inner, ranks) => {
                assert_eq!(*ranks, [1, 1, 1]);
                assert!(matches!(**inner, Verb::Atop(..)), "got {inner:?}");
            }
            other => panic!("expected a ranked atop, got {other:?}"),
        }
        let (v, _) = monad_of(&one("+ & (*:\"0) y"));
        match &v {
            Verb::Rank(inner, ranks) => {
                assert_eq!(*ranks, [0, 0, 0]);
                assert!(matches!(**inner, Verb::Compose(..)), "got {inner:?}");
            }
            other => panic!("expected a ranked composition, got {other:?}"),
        }
        let (v, _) = monad_of(&one("+ &: *: y"));
        assert!(matches!(v, Verb::Compose(..)), "got {v:?}");
    }

    #[test]
    fn a_noun_operand_bonds_the_conjunction() {
        // The bond takes the rank of the side its argument arrives on.
        let (v, _) = monad_of(&one("1 & + y"));
        match &v {
            Verb::Rank(inner, ranks) => {
                assert_eq!(*ranks, [0, 0, 0]);
                match &**inner {
                    Verb::BondLeft(a, g) => {
                        assert_eq!(a.as_i64_slice(), Some(&[1i64][..]));
                        assert_eq!(prim_of(g).name, "+");
                    }
                    other => panic!("expected a left bond, got {other:?}"),
                }
            }
            other => panic!("expected a ranked bond, got {other:?}"),
        }
        let (v, _) = monad_of(&one("{. & 2 y"));
        match &v {
            // `{.` has left rank 1, so `{.&2` reads its argument by rows.
            Verb::Rank(inner, ranks) => {
                assert_eq!(*ranks, [1, 1, 1]);
                assert!(matches!(**inner, Verb::BondRight(..)), "got {inner:?}");
            }
            other => panic!("expected a ranked bond, got {other:?}"),
        }
    }

    #[test]
    fn window_scan_and_commute_adverbs() {
        let (v, _) = monad_of(&one("+/\\ 1 2 3"));
        match &v {
            Verb::Windowed(u, WindowKind::Prefix) => assert!(matches!(**u, Verb::Reduce(_))),
            other => panic!("expected a prefix application, got {other:?}"),
        }
        // The window size is the left argument, so the derived verb has both
        // valences and its left cell is an atom.
        assert_eq!(v.ranks(), [RANK_INF, 0, RANK_INF]);
        let (v, _, _) = dyad_of(&one("2 +/\\ 1 2 3"));
        assert!(matches!(v, Verb::Windowed(_, WindowKind::Prefix)));
        let (v, _) = monad_of(&one("+/\\. 1 2 3"));
        assert!(matches!(v, Verb::Windowed(_, WindowKind::Suffix)));
        let (v, _) = monad_of(&one("+~ 1 2 3"));
        match &v {
            Verb::Commute(u) => assert_eq!(prim_of(u).name, "+"),
            other => panic!("expected a commute, got {other:?}"),
        }
        let (v, _) = monad_of(&one("+:^:3 (1)"));
        assert!(matches!(v, Verb::PowerN(_, Power::Times(3))));
        let (v, _) = monad_of(&one("%:^:_ (100)"));
        assert!(matches!(v, Verb::PowerN(_, Power::Converge)));
    }

    #[test]
    fn the_key_adverb_derives_a_verb() {
        match one("+/. 1 2 3") {
            Expr::Monad { verb: Verb::Key(_), .. } => {}
            other => panic!("expected a key, got {other:?}"),
        }
    }

    #[test]
    fn noun_operand_adverbs_are_not_supported_yet() {
        let e = err("1/ 2");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("noun-operand adverbs"), "{}", e.msg);
    }

    #[test]
    fn noun_operand_conjunctions_are_not_supported_yet() {
        let e = err("1 @: + y");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("noun-operand conjunctions"), "{}", e.msg);
    }

    // --------------------------------------------------------------- trains

    #[test]
    fn three_verbs_in_parentheses_are_a_fork() {
        let (v, y) = monad_of(&one("(+/ % #) 1 2 3"));
        match &v {
            Verb::Fork(f, g, h) => {
                assert!(matches!(**f, Verb::Reduce(_)), "got {f:?}");
                assert_eq!(prim_of(g).name, "%");
                assert_eq!(prim_of(h).name, "#");
            }
            other => panic!("expected a fork, got {other:?}"),
        }
        assert_eq!(konst(&y).shape, vec![3]);
    }

    #[test]
    fn a_noun_left_tine_is_a_noun_fork() {
        let (v, _) = monad_of(&one("(2 + #) 1 2 3"));
        match &v {
            Verb::NounFork(a, g, h) => {
                assert_eq!(a.as_i64_slice(), Some(&[2i64][..]));
                assert_eq!(prim_of(g).name, "+");
                assert_eq!(prim_of(h).name, "#");
            }
            other => panic!("expected a noun fork, got {other:?}"),
        }
    }

    #[test]
    fn two_verbs_in_parentheses_are_a_hook() {
        let (v, _) = monad_of(&one("(+ #) 1 2 3"));
        match &v {
            Verb::Hook(f, g) => {
                assert_eq!(prim_of(f).name, "+");
                assert_eq!(prim_of(g).name, "#");
            }
            other => panic!("expected a hook, got {other:?}"),
        }
    }

    #[test]
    fn cap_makes_a_fork_an_atop() {
        let (v, _) = monad_of(&one("([: +/ ,) 1 2 3"));
        match &v {
            Verb::Atop(f, g) => {
                assert!(matches!(**f, Verb::Reduce(_)), "got {f:?}");
                assert_eq!(prim_of(g).name, ",");
            }
            other => panic!("expected an atop, got {other:?}"),
        }
    }

    #[test]
    fn a_five_verb_train_folds_from_the_right() {
        // (a b c d e) is a fork whose right tine is the fork (c d e).
        let (v, _) = monad_of(&one("(] , [ , ]) 1 2 3"));
        match &v {
            Verb::Fork(f, g, h) => {
                assert_eq!(prim_of(f).name, "]");
                assert_eq!(prim_of(g).name, ",");
                assert!(matches!(**h, Verb::Fork(..)), "got {h:?}");
            }
            other => panic!("expected a fork, got {other:?}"),
        }
    }

    #[test]
    fn a_noun_fork_needs_a_literal_noun() {
        let e = err("({n} + #) 1 2 3");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("noun forks"), "{}", e.msg);
    }

    #[test]
    fn cap_is_never_applied_as_a_verb() {
        // `[:` has no meaning of its own; it only caps a fork. Here it is
        // left over as a bident with the result of `# 1 2 3`.
        let e = err("[: # 1 2 3");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("that bident"), "{}", e.msg);
    }

    #[test]
    fn a_bident_of_nouns_is_not_supported_yet() {
        let e = err("'ab' 'cd'");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("that bident"), "{}", e.msg);
    }

    #[test]
    fn a_sentence_that_is_a_verb_is_not_supported_yet() {
        let e = err("+/ % #");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("tacit"), "{}", e.msg);
    }

    // ----------------------------------------------------------- assignment

    #[rstest]
    #[case("x =. 5")]
    #[case("x =: 5")]
    fn assignment_yields_an_assign_node(#[case] src: &str) {
        match one(src) {
            Expr::Assign { name, value, span } => {
                assert_eq!(name, "x");
                assert_eq!(ints(&value), vec![5]);
                assert_eq!(span, Span::new(0, 6));
            }
            other => panic!("expected an assignment, got {other:?}"),
        }
    }

    #[test]
    fn assignment_in_expression_position() {
        let (v, x, y) = dyad_of(&one("y + x =. 3"));
        assert_eq!(prim_of(&v).name, "+");
        assert!(matches!(x, Expr::Name(..)));
        match y {
            Expr::Assign { name, span, .. } => {
                assert_eq!(name, "x");
                assert_eq!(span, Span::new(4, 10));
            }
            other => panic!("expected an assignment, got {other:?}"),
        }
    }

    #[test]
    fn assignment_takes_the_whole_right_hand_sentence() {
        match one("x =. 1 + 2") {
            Expr::Assign { value, .. } => {
                let (v, _, _) = dyad_of(&value);
                assert_eq!(prim_of(&v).name, "+");
            }
            other => panic!("expected an assignment, got {other:?}"),
        }
    }

    // ---------------------------------------------------- naming a verb

    #[test]
    fn assigning_a_verb_names_it_and_runs_nothing() {
        let s = stmts("mean =. +/ % #");
        assert_eq!(s.len(), 1);
        match &s[0] {
            Expr::VerbDef { name, verb, span } => {
                assert_eq!(name, "mean");
                assert!(matches!(verb, Verb::Fork(..)), "got {verb:?}");
                assert_eq!(*span, Span::new(0, 14));
            }
            other => panic!("expected a verb definition, got {other:?}"),
        }
    }

    #[test]
    fn a_named_verb_applies_in_a_later_sentence() {
        let s = stmts("mean =. +/ % #\nmean 1 2 3 4");
        assert_eq!(s.len(), 2);
        let (v, y) = monad_of(&s[1]);
        assert!(matches!(v, Verb::Fork(..)), "got {v:?}");
        assert_eq!(konst(&y).shape, vec![4]);
    }

    #[test]
    fn a_named_verb_is_a_verb_inside_a_train_and_under_a_conjunction() {
        let (v, _) = monad_of(&stmts("mean =. +/ % #\n(mean - {.) 1 2 3 4").pop().expect("two"));
        match &v {
            Verb::Fork(f, g, h) => {
                assert!(matches!(**f, Verb::Fork(..)), "got {f:?}");
                assert_eq!(prim_of(g).name, "-");
                assert_eq!(prim_of(h).name, "{.");
            }
            other => panic!("expected a fork, got {other:?}"),
        }
        let (v, _) = monad_of(&stmts("mean =. +/ % #\nmean\"1 m").pop().expect("two"));
        match &v {
            Verb::Rank(inner, r) => {
                assert_eq!(*r, [1, 1, 1]);
                assert!(matches!(**inner, Verb::Fork(..)), "got {inner:?}");
            }
            other => panic!("expected a ranked verb, got {other:?}"),
        }
    }

    #[test]
    fn redefinition_rebinds_from_that_sentence_on() {
        let s = stmts("f =. +/\nf 1 2 3\nf =. #\nf 1 2 3");
        assert_eq!(s.len(), 4);
        assert!(matches!(monad_of(&s[1]).0, Verb::Reduce(_)));
        assert_eq!(prim_of(&monad_of(&s[3]).0).name, "#");
    }

    #[test]
    fn a_name_may_change_part_of_speech_in_either_direction() {
        // The oracle accepts both; the last assignment decides.
        let s = stmts("a =. 1 2 3\na =. +/\na 1 2 3");
        assert!(matches!(s[0], Expr::Assign { .. }));
        assert!(matches!(s[1], Expr::VerbDef { .. }));
        assert!(matches!(monad_of(&s[2]).0, Verb::Reduce(_)));
        let s = stmts("f =. +/\nf =. 10 20\nf");
        assert!(matches!(s[0], Expr::VerbDef { .. }));
        assert!(matches!(s[1], Expr::Assign { .. }));
        assert!(matches!(s[2], Expr::Name(..)));
    }

    #[test]
    fn an_undefined_name_applied_as_a_verb_is_a_value_error() {
        // The reference says `value error: zz`, pointing at the name.
        let e = err("zz 1 2 3");
        assert_eq!(e.kind, ErrorKind::Value);
        assert_eq!(e.msg, "undefined name: zz");
        assert_eq!(e.span, Some(Span::new(0, 2)));
        // A name that does hold a value is a different complaint: two
        // nouns side by side, which the reference calls a syntax error.
        let e = err("a =. 5\na 1 2 3");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("that bident"), "{}", e.msg);
    }

    #[test]
    fn adverb_and_conjunction_assignment_are_not_supported_yet() {
        let e = err("insert =. /");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("adverb and conjunction assignment"), "{}", e.msg);
    }

    #[rstest]
    #[case("f =. 3 : 'y + 1'", "explicit definitions")]
    #[case("f =. 4 : 'x + y'", "explicit definitions")]
    #[case("f =. {{ y + 1 }}", "explicit definitions")]
    fn explicit_definitions_are_named_in_the_diagnostic(
        #[case] src: &str,
        #[case] msg: &str,
    ) {
        let e = err(src);
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains(msg), "{}", e.msg);
    }

    #[test]
    fn multiple_assignment_is_not_supported_yet() {
        let e = err("'a b' =. 1 2");
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains("multiple assignment"), "{}", e.msg);
    }

    // -------------------------------------------------------- interpolation

    #[test]
    fn a_hole_is_a_noun() {
        let e = one("{a} + 1");
        let (_, x, y) = dyad_of(&e);
        match x {
            Expr::Param(i, s) => {
                assert_eq!(i, 0);
                assert_eq!(s, Span::new(0, 3));
            }
            other => panic!("expected a parameter, got {other:?}"),
        }
        assert_eq!(ints(&y), vec![1]);
        assert_eq!(e.span(), Span::new(0, 7));
    }

    #[test]
    fn holes_are_numbered_and_shared_by_name() {
        let sp = SourceParts::from_source("{a} + {b} + {a}").expect("source parts");
        assert_eq!(sp.param_names, vec!["a".to_string(), "b".to_string()]);
        let e = parse(&sp).expect("parse").pop().expect("one sentence");
        let (_, x, y) = dyad_of(&e);
        assert!(matches!(x, Expr::Param(0, _)));
        let (_, x2, y2) = dyad_of(&y);
        assert!(matches!(x2, Expr::Param(1, _)));
        assert!(matches!(y2, Expr::Param(0, _)));
    }

    #[test]
    fn a_hole_takes_a_verb_like_any_noun() {
        let (v, y) = monad_of(&one("+/ {data}"));
        assert!(matches!(v, Verb::Reduce(_)));
        assert!(matches!(y, Expr::Param(0, _)));
    }

    #[test]
    fn braces_inside_a_string_are_not_holes() {
        let sp = SourceParts::from_source("'{a}'").expect("source parts");
        assert!(sp.param_names.is_empty());
        let a = konst(&parse(&sp).expect("parse")[0]);
        assert_eq!(a.data, Data::Char(vec!['{', 'a', '}'].into()));
    }

    #[test]
    fn parts_of_one_sentence_lex_across_a_hole() {
        // The t-string path: literal parts with a hole between them.
        let sp = SourceParts::from_parts(&["1 + ", " * 2"], &["v"]);
        assert_eq!(sp.display, "1 + {v} * 2");
        let e = parse(&sp).expect("parse").pop().expect("one sentence");
        let (_, x, y) = dyad_of(&e);
        assert_eq!(ints(&x), vec![1]);
        let (_, x2, y2) = dyad_of(&y);
        assert!(matches!(x2, Expr::Param(0, _)));
        assert_eq!(ints(&y2), vec![2]);
    }

    #[test]
    fn spans_of_later_sentences_index_the_whole_source() {
        let src = "5\n1 + 2";
        let s = stmts(src);
        assert_eq!(s[1].span(), Span::new(2, 7));
        assert_eq!(&src[2..7], "1 + 2");
    }

    // --------------------------------------------------------------- errors

    #[test]
    fn unknown_word_reports_its_span() {
        let e = err("1 $: 2");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert_eq!(e.msg, "unknown word: $:");
        assert_eq!(e.span, Some(Span::new(2, 4)));
    }

    #[test]
    fn an_inflected_unknown_word_is_reported_whole() {
        let e = err("1 $. 2");
        assert_eq!(e.msg, "unknown word: $.");
        assert_eq!(e.span, Some(Span::new(2, 4)));
    }

    #[rstest]
    #[case("3j4", "complex literals", 0, 3)]
    #[case("1 + 3j4", "complex literals", 4, 7)]
    #[case("12x", "extended-precision integers", 0, 3)]
    #[case("3r4", "rationals", 0, 3)]
    fn unsupported_number_forms(
        #[case] src: &str,
        #[case] msg: &str,
        #[case] start: usize,
        #[case] end: usize,
    ) {
        let e = err(src);
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains(msg), "{}", e.msg);
        assert_eq!(e.span, Some(Span::new(start, end)));
    }

    #[test]
    fn a_malformed_number_is_a_parse_error() {
        let e = err("1.2.3");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.msg.contains("invalid number"), "{}", e.msg);
    }

    #[test]
    fn an_unbalanced_sentence_is_a_syntax_error() {
        let e = err("(1 + 2");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert_eq!(e.msg, "syntax error");
        assert_eq!(e.span, Some(Span::new(0, 6)));
    }

    #[test]
    fn a_stray_right_parenthesis_is_a_syntax_error() {
        let e = err("1 + 2)");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert_eq!(e.msg, "syntax error");
    }

    #[test]
    fn the_error_of_a_later_sentence_points_at_that_sentence() {
        let e = err("1 + 2\n3 $: 4");
        assert_eq!(e.span, Some(Span::new(8, 10)));
    }
}
