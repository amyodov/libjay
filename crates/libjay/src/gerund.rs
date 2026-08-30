//! J's atomic representation, which is what a gerund is made of.
//!
//! `` u`v `` is not a parse-time object in J: it is boxed data, one box per
//! tied entity, and each box holds that entity's atomic representation. A
//! primitive is its own spelling as a character vector; a noun is the pair
//! `('0'; <value)`; a train is `('2'; <parts)` or `('3'; <parts)`; and
//! anything a modifier derived is `(spelling; <operands)`. Everything is
//! therefore ordinary data, which is what lets a gerund be assigned,
//! computed and displayed like any other noun.

use crate::array::{Array, Data};
use crate::verb::{Enclose, Power, Verb, WindowKind, RANK_INF};

/// One atomic representation.
#[derive(Clone, Debug, PartialEq)]
pub enum Ar {
    /// A primitive, spelled as it is written.
    Prim(String),
    /// A noun operand, which stands for itself.
    Noun(Array),
    /// What a modifier derived, by the modifier's spelling and its operands.
    Derived(String, Vec<Ar>),
    /// A hook (two parts) or a fork (three).
    Train(Vec<Ar>),
}

fn chars(s: &str) -> Array {
    Array::from_chars(s.chars().collect())
}

fn boxes(items: Vec<Array>) -> Array {
    Array::new(vec![items.len()], Data::Box(items.into()))
}

/// The two-box pair every derived representation takes.
fn pair(head: Array, body: Array) -> Array {
    boxes(vec![head, body])
}

impl Ar {
    /// The representation as the boxed data J spells it with.
    pub fn to_array(&self) -> Array {
        match self {
            Ar::Prim(s) => chars(s),
            Ar::Noun(a) => pair(chars("0"), a.clone()),
            Ar::Derived(s, ops) => {
                pair(chars(s), boxes(ops.iter().map(Ar::to_array).collect()))
            }
            Ar::Train(ops) => {
                let tag = if ops.len() == 2 { "2" } else { "3" };
                pair(chars(tag), boxes(ops.iter().map(Ar::to_array).collect()))
            }
        }
    }

    /// The representation read back out of boxed data, or `None` where the
    /// data is not one.
    pub fn from_array(a: &Array) -> Option<Ar> {
        if let Some(text) = text_of(a) {
            return Some(Ar::Prim(text));
        }
        let items = a.as_boxes()?;
        if a.rank() != 1 || items.len() != 2 {
            return None;
        }
        let head = text_of(&items[0])?;
        if head == "0" {
            return Some(Ar::Noun(items[1].clone()));
        }
        let parts: Option<Vec<Ar>> = items[1].as_boxes()?.iter().map(Ar::from_array).collect();
        let parts = parts?;
        match head.as_str() {
            "2" | "3" => Some(Ar::Train(parts)),
            _ => Some(Ar::Derived(head, parts)),
        }
    }
}

/// A character vector or atom as a string; `None` for anything else.
pub fn text_of(a: &Array) -> Option<String> {
    if a.rank() > 1 {
        return None;
    }
    match a.row_major_data() {
        Data::Char(v) => Some(v.as_slice().iter().collect()),
        _ => None,
    }
}

/// The gerund `` u`v `` builds: one box per representation.
pub fn gerund_array(items: &[Ar]) -> Array {
    boxes(items.iter().map(Ar::to_array).collect())
}

/// A rank specification as the noun `u"n` was given.
fn rank_noun(r: &[i64; 3]) -> Array {
    let one = |v: i64| if v == RANK_INF { f64::INFINITY } else { v as f64 };
    if r[0] == r[1] && r[1] == r[2] {
        return Array::new(vec![], Data::F64(vec![one(r[0])].into()));
    }
    // Two atoms are the left rank and the right one, and the monadic rank
    // is the right one again: the shorter spelling wherever it says the
    // same thing.
    if r[0] == r[2] {
        return Array::from_f64(vec![one(r[1]), one(r[2])]);
    }
    Array::from_f64(vec![one(r[0]), one(r[1]), one(r[2])])
}

/// The word a constant verb is spelled as: `_9:` through `9:` for the
/// atoms that have one, `_:` for infinity, and nothing for the rest.
fn constant_word(n: &Array) -> Option<String> {
    if n.rank() != 0 {
        return None;
    }
    let v = *n.to_f64_vec()?.first()?;
    if v == f64::INFINITY {
        return Some("_:".to_string());
    }
    if v.fract() != 0.0 || !(-9.0..=9.0).contains(&v) {
        return None;
    }
    let k = v as i64;
    Some(if k < 0 { format!("_{}:", -k) } else { format!("{k}:") })
}

fn power_noun(p: &Power) -> Option<Array> {
    Some(match p {
        Power::Times(n) => Array::scalar_i64(*n as i64),
        Power::Converge => Array::scalar_f64(f64::INFINITY),
        Power::Each(ns) => Array::from_i64(ns.clone()),
        // `u^:a:` is the ace, which is what `` ` `` would have to write out.
        Power::ConvergeTrace => Array::boxed(Array::empty(crate::dtype::DType::I64)),
        Power::Inverse(n) => Array::scalar_i64(-(*n as i64)),
    })
}

/// The atomic representation of a verb, or `None` where libjay has no
/// spelling to give it — a verb from the APL frontend, or one whose parts
/// the tree no longer names.
pub fn verb_ar(v: &Verb) -> Option<Ar> {
    let der = |s: &str, ops: Vec<Ar>| Some(Ar::Derived(s.to_string(), ops));
    match v {
        // `m b.` is a truth table with no spelling of its own left in the
        // tree, so it is the one primitive that cannot be written back out.
        Verb::Prim(p) if p.name == "b." => None,
        Verb::Prim(p) => Some(Ar::Prim(p.name.to_string())),
        Verb::Rank(inner, r) => rank_ar(inner, r),
        Verb::Reduce(u) => der("/", vec![verb_ar(u)?]),
        Verb::Windowed(u, WindowKind::Prefix) => der("\\", vec![verb_ar(u)?]),
        Verb::Windowed(u, WindowKind::Suffix) => der("\\.", vec![verb_ar(u)?]),
        Verb::Windowed(_, WindowKind::Scan) => None,
        Verb::Commute(u) => der("~", vec![verb_ar(u)?]),
        Verb::PowerN(u, p) => der("^:", vec![verb_ar(u)?, Ar::Noun(power_noun(p)?)]),
        Verb::PowerV(u, w) => der("^:", vec![verb_ar(u)?, verb_ar(w)?]),
        Verb::Fork(f, g, h) => Some(Ar::Train(vec![verb_ar(f)?, verb_ar(g)?, verb_ar(h)?])),
        Verb::NounFork(n, g, h) => {
            Some(Ar::Train(vec![Ar::Noun(n.clone()), verb_ar(g)?, verb_ar(h)?]))
        }
        Verb::Hook(f, g) => Some(Ar::Train(vec![verb_ar(f)?, verb_ar(g)?])),
        Verb::Atop(f, g) => match under_ar(f, g, "&.:") {
            Some(ar) => Some(ar),
            None => der("@:", vec![verb_ar(f)?, verb_ar(g)?]),
        },
        Verb::Compose(f, g) => der("&:", vec![verb_ar(f)?, verb_ar(g)?]),
        Verb::BondLeft(m, u) => der("&", vec![Ar::Noun(m.clone()), verb_ar(u)?]),
        Verb::BondRight(u, n) => der("&", vec![verb_ar(u)?, Ar::Noun(n.clone())]),
        Verb::Each(u, Enclose::Always) => {
            der("&.", vec![verb_ar(u)?, Ar::Prim(">".to_string())])
        }
        Verb::Each(_, Enclose::ExceptSimpleScalar) => None,
        Verb::Fit(u, n) => der("!.", vec![verb_ar(u)?, Ar::Noun(Array::scalar_f64(*n))]),
        Verb::Amend(m) => der("}", vec![Ar::Noun(m.clone())]),
        Verb::AmendVerb(u) => der("}", vec![verb_ar(u)?]),
        Verb::Memo(u, _) => der("M.", vec![verb_ar(u)?]),
        Verb::Level { u, level, spread } => der(
            if *spread { "S:" } else { "L:" },
            vec![
                verb_ar(u)?,
                Ar::Noun(if *level == crate::verb::RANK_INF {
                    Array::scalar_f64(f64::INFINITY)
                } else {
                    Array::scalar_i64(*level)
                }),
            ],
        ),
        Verb::Characteristics(u) => der("b.", vec![verb_ar(u)?]),
        Verb::Key(u) => der("/.", vec![verb_ar(u)?]),
        Verb::Cut(u, n) => der(";.", vec![verb_ar(u)?, Ar::Noun(Array::scalar_i64(*n))]),
        Verb::Adverse(u, w) => der("::", vec![verb_ar(u)?, verb_ar(w)?]),
        Verb::WithObverse(u, w) => der(":.", vec![verb_ar(u)?, verb_ar(w)?]),
        Verb::Agenda(vs, w) => {
            let items: Option<Vec<Ar>> = vs.iter().map(verb_ar).collect();
            der("@.", vec![Ar::Noun(gerund_array(&items?)), verb_ar(w)?])
        }
        Verb::Evoke(vs, n) => {
            let items: Option<Vec<Ar>> = vs.iter().map(verb_ar).collect();
            der(
                "`:",
                vec![Ar::Noun(gerund_array(&items?)), Ar::Noun(Array::scalar_i64(*n))],
            )
        }
        Verb::SelfRef => Some(Ar::Prim("$:".to_string())),
        _ => None,
    }
}

// --------------------------------------------- the linear representation

/// What a spelling looks like from outside, which is what decides where a
/// parenthesis has to go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// One word: a primitive or a noun. Nothing binds it apart.
    Word,
    /// A phrase a modifier derived. A conjunction's RIGHT operand is the
    /// one word beside it, so this needs parentheses there.
    Modified,
    /// A train. Only the last tine of another train may stand unbracketed,
    /// and only when the words still count out the same way.
    Train,
}

/// How J writes an entity back out — the LINEAR REPRESENTATION a session
/// shows for a name that stands for a verb. Parentheses go only where the
/// spelling would otherwise read as something else.
///
/// None where libjay has no spelling to give: a noun of rank 2 or more,
/// which the reference writes as an expression that BUILDS the value, and
/// anything [`verb_ar`] itself cannot name.
pub fn linear(ar: &Ar) -> Option<String> {
    spell(ar).map(|(text, _)| text)
}

fn spell(ar: &Ar) -> Option<(String, Shape)> {
    // `n [ ]` is how the constant verb is built, and `n:` is how it is
    // spelled where the noun is one of the atoms that has such a word. The
    // shortcut belongs to the spelling alone: the representation itself
    // has to keep the three parts, which is what a gerund reads back.
    if let Ar::Train(ops) = ar
        && let [Ar::Noun(n), Ar::Prim(g), Ar::Prim(h)] = ops.as_slice()
        && g == "["
        && h == "]"
        && let Some(w) = constant_word(n)
    {
        return Some((w, Shape::Word));
    }
    match ar {
        Ar::Prim(s) => Some((word(s), Shape::Word)),
        Ar::Noun(a) => Some((noun_text(a)?, Shape::Word)),
        Ar::Derived(sp, ops) => {
            let text = match ops.as_slice() {
                [u] => join(&left(u)?, &word(sp)),
                [u, v] => {
                    let head = join(&left(u)?, &word(sp));
                    let tail = right(v)?;
                    format!("{head}{tail}")
                }
                _ => return None,
            };
            Some((text, Shape::Modified))
        }
        Ar::Train(_) => {
            let parts = tines(ar);
            let mut out = String::new();
            for (i, t) in parts.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&left(t)?);
            }
            Some((out, Shape::Train))
        }
    }
}

/// The tines a train is written as. A train in the last place is written
/// out flat where the words still count out to the same tree — three tines
/// and then a fork, five and then a fork — and bracketed where they do not.
fn tines(ar: &Ar) -> Vec<&Ar> {
    let Ar::Train(ops) = ar else { return vec![ar] };
    let (last, head) = ops.split_last().expect("a train has parts");
    let mut out: Vec<&Ar> = head.iter().collect();
    if matches!(last, Ar::Train(_)) {
        let inner = tines(last);
        if inner.len() % 2 == 1 {
            out.extend(inner);
            return out;
        }
    }
    out.push(last);
    out
}

/// A spelling standing to the LEFT of a modifier, or as a tine of a train:
/// a modifier's left scope is everything before it, so only a train needs
/// bracketing there.
fn left(ar: &Ar) -> Option<String> {
    let (text, shape) = spell(ar)?;
    Some(if shape == Shape::Train { format!("({text})") } else { text })
}

/// A spelling standing to the RIGHT of a conjunction, which takes the one
/// word beside it: anything a modifier made needs bracketing there too.
fn right(ar: &Ar) -> Option<String> {
    let (text, shape) = spell(ar)?;
    // `{` carries a space of its own, and the reference brackets it here
    // rather than letting the space end the phrase.
    let bare = shape == Shape::Word && !text.ends_with(' ');
    Some(if bare { text } else { format!("({text})") })
}

/// One spelling written against the next. `{` and `}` carry a space of
/// their own so that two of them never read as J's `{{` or `}}`, and a
/// word that starts with an inflection is held off the one before it for
/// the same reason.
fn word(s: &str) -> String {
    if s == "{" || s == "}" {
        return format!("{s} ");
    }
    s.to_string()
}

fn join(head: &str, tail: &str) -> String {
    let split = tail.starts_with(['.', ':']) && !head.ends_with(' ');
    if split { format!("{head} {tail}") } else { format!("{head}{tail}") }
}

/// A noun as the linear representation writes it: the value itself for a
/// list or an atom, quoted where it is text. Higher ranks have no such
/// spelling here.
fn noun_text(a: &Array) -> Option<String> {
    if a.rank() > 1 {
        return None;
    }
    if let Data::Char(v) = a.row_major_data() {
        let mut out = String::from("'");
        for c in v.as_slice() {
            if *c == '\'' {
                out.push('\'');
            }
            out.push(*c);
        }
        out.push('\'');
        return Some(out);
    }
    if a.count() == 0 || matches!(a.row_major_data(), Data::Box(_)) {
        return None;
    }
    let text = crate::fmt::format_array(a, &crate::fmt::FmtOpts::J);
    let text = text.trim_end_matches('\n');
    if text.contains('\n') { None } else { Some(text.to_string()) }
}

/// `u"n`, and the three conjunctions J spells by applying at an operand's
/// own rank: `u@v`, `u&v` and `u&.v` are each a rank around what `@:`,
/// `&:` and `&.:` derive, so the rank they set is what tells them apart.
fn rank_ar(inner: &Verb, r: &[i64; 3]) -> Option<Ar> {
    let der = |s: &str, ops: Vec<Ar>| Some(Ar::Derived(s.to_string(), ops));
    // `u&.v` sets v's MONADIC rank around the same tree `&.:` builds, which
    // is what separates it from `u@v` — that one sets all three of g's.
    if let Verb::Atop(f, g) = inner
        && matches!(&**g, Verb::Compose(_, u) if *r == [u.ranks()[0]; 3])
        && let Some(ar) = under_ar(f, g, "&.")
    {
        return Some(ar);
    }
    match inner {
        // `m"n`: the constant verb, whose left operand is the noun itself.
        Verb::Constant(m) => der("\"", vec![Ar::Noun(m.clone()), Ar::Noun(rank_noun(r))]),
        Verb::Atop(f, g) if *r == g.ranks() => der("@", vec![verb_ar(f)?, verb_ar(g)?]),
        Verb::Compose(f, g) if *r == [g.ranks()[0]; 3] => {
            der("&", vec![verb_ar(f)?, verb_ar(g)?])
        }
        Verb::BondLeft(m, g) if *r == [g.ranks()[2]; 3] => {
            der("&", vec![Ar::Noun(m.clone()), verb_ar(g)?])
        }
        Verb::BondRight(g, n) if *r == [g.ranks()[1]; 3] => {
            der("&", vec![verb_ar(g)?, Ar::Noun(n.clone())])
        }
        _ => der("\"", vec![verb_ar(inner)?, Ar::Noun(rank_noun(r))]),
    }
}

/// `u&.v` and `u&.:v` are built as `v^:_1 @: (u &: v)`; this recognises
/// that shape and gives the spelling back. The left part must really be v's
/// obverse, which is what keeps an ordinary `f@:(g&:h)` out.
fn under_ar(f: &Verb, g: &Verb, spelling: &str) -> Option<Ar> {
    let Verb::Compose(inner, under) = g else { return None };
    let obverse = crate::frontend::j::obverse_of(under, crate::error::Span::new(0, 0)).ok()?;
    if obverse.name() != f.name() {
        return None;
    }
    Some(Ar::Derived(spelling.to_string(), vec![verb_ar(inner)?, verb_ar(under)?]))
}
