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
use crate::verb::{AtopForm, Enclose, Power, Verb, WindowKind, RANK_INF};

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

/// The BOXED REPRESENTATION `5!:2` answers: the same tree the atomic
/// representation holds, drawn as the words it is spelled with rather than
/// as the pairs a gerund is made of. A modifier's phrase is its operands
/// with the modifier's own spelling between them, a train is its tines, and
/// a noun stands as the value itself.
pub fn boxed_rep(ar: &Ar) -> Array {
    match ar {
        // A whole entity that is one word is still a list of parts, of one.
        Ar::Prim(_) | Ar::Noun(_) => boxes(vec![part_rep(ar)]),
        _ => part_rep(ar),
    }
}

fn part_rep(ar: &Ar) -> Array {
    match ar {
        Ar::Prim(s) => chars(s),
        Ar::Noun(a) => a.clone(),
        Ar::Derived(sp, ops) => {
            let mut items = Vec::with_capacity(ops.len() + 1);
            match ops.as_slice() {
                [u] => {
                    items.push(part_rep(u));
                    items.push(chars(sp));
                }
                [u, v] => {
                    items.push(part_rep(u));
                    items.push(chars(sp));
                    items.push(part_rep(v));
                }
                _ => items.push(chars(sp)),
            }
            boxes(items)
        }
        Ar::Train(ops) => boxes(ops.iter().map(part_rep).collect()),
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
fn rank_noun(r: crate::verb::Ranks) -> Array {
    let one = |v: i64| {
        if v == RANK_INF {
            f64::INFINITY
        } else if v == -RANK_INF {
            f64::NEG_INFINITY
        } else {
            v as f64
        }
    };
    // As many atoms as were written. One stands for all three, and two are
    // the left rank and the right one with the monadic rank taken from the
    // right; a spelling that says the same thing in more atoms is still the
    // spelling the entity was built with.
    match r.atoms() {
        1 => Array::new(vec![], Data::F64(vec![one(r[0])].into())),
        2 => Array::from_f64(vec![one(r[1]), one(r[2])]),
        _ => Array::from_f64(vec![one(r[0]), one(r[1]), one(r[2])]),
    }
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
    if v == f64::NEG_INFINITY {
        return Some("__:".to_string());
    }
    if v.fract() != 0.0 || !(-9.0..=9.0).contains(&v) {
        return None;
    }
    let k = v as i64;
    Some(if k < 0 { format!("_{}:", -k) } else { format!("{k}:") })
}

/// A parameter list as the noun it was written as: real where every one of
/// them is, and one atom where the list holds one.
fn cx_noun(zs: &[crate::complex::Cx]) -> Array {
    if zs.iter().all(|z| z[1] == 0.0) {
        let reals: Vec<f64> = zs.iter().map(|z| z[0]).collect();
        return match reals.as_slice() {
            [only] => Array::scalar_f64(*only),
            _ => Array::from_f64(reals),
        };
    }
    let shape = if zs.len() == 1 { Vec::new() } else { vec![zs.len()] };
    Array::new(shape, Data::Complex(zs.to_vec().into()))
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
        // A noun operand to `@` is the constant verb, and writes itself
        // back out as the noun it was: `*:@_1 2`, not `*:@(_1 2"_)`.
        Verb::Constant(m) => Some(Ar::Noun(m.clone())),
        Verb::Rank(inner, r) => rank_ar(inner, *r),
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
        // `[: f g` is the fork whose left tine is the cap, and that is how
        // it is written back out: a three-part train with `[:` in front.
        Verb::Atop(f, g, AtopForm::Cap) => Some(Ar::Train(vec![
            Ar::Prim("[:".to_string()),
            verb_ar(f)?,
            verb_ar(g)?,
        ])),
        Verb::Atop(f, g, AtopForm::At) => match under_ar(f, g, "&.:") {
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
        Verb::Level { u, levels, spread } => der(
            if *spread { "S:" } else { "L:" },
            vec![verb_ar(u)?, Ar::Noun(rank_noun(*levels))],
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
        // An explicit definition is the `:` conjunction over its valence
        // and its body, whichever way the source spelled it.
        Verb::Explicit(def) => def.rep.as_ref().map(explicit_ar),
        // `u : v` — one verb out of two, which is the same conjunction
        // over two VERB operands.
        Verb::Ambivalent(u, w) => der(":", vec![verb_ar(u)?, verb_ar(w)?]),
        // `u&.,` is written with the ravel it is an under of.
        Verb::UnderRavel(u) => {
            der("&.", vec![verb_ar(u)?, Ar::Prim(",".to_string())])
        }
        // `u!.f` where the fit is a FILL: the fill stands as its own noun.
        Verb::Fill(u, f) => der("!.", vec![verb_ar(u)?, Ar::Noun(f.array())]),
        // `|.!.f` is the rotate with a fill, and the rotate is what it is
        // written from.
        Verb::ShiftFill(f) => {
            der("!.", vec![Ar::Prim("|.".to_string()), Ar::Noun(f.clone())])
        }
        // `` u`v`w} `` — the gerund is the adverb's one noun operand.
        Verb::AmendGerund(vs) => {
            let items: Option<Vec<Ar>> = vs.iter().map(verb_ar).collect();
            der("}", vec![Ar::Noun(gerund_array(&items?))])
        }
        // `m H. n`: the two parameter lists, as they were written.
        Verb::Hypergeometric { num, den } => der(
            "H.",
            vec![Ar::Noun(cx_noun(num)), Ar::Noun(cx_noun(den))],
        ),
        // A NAME with nothing under it stands for a verb, and writes itself
        // back out as the name. Every other deferral has a spelling that is
        // not a name (`u^:n`, `n}`) or a locale to choose between.
        Verb::Deferred(d)
            if d.choices.is_empty()
                && crate::verb::is_name(&d.spelling)
                && matches!(&d.operand, crate::ir::Expr::Name(n, _) if *n == d.spelling) =>
        {
            Some(Ar::Prim(d.spelling.clone()))
        }
        // `u . v` — J's inner product. APL's `f.g` is the same node under
        // a spelling J does not have.
        Verb::InnerProduct { u, v, apl: false } => {
            der(".", vec![verb_ar(u)?, verb_ar(v)?])
        }
        _ => None,
    }
}

/// The representation of an explicit definition: the valence and the body
/// as the two noun operands of `:`.
pub fn explicit_ar(rep: &crate::ir::ExplicitRep) -> Ar {
    Ar::Derived(
        ":".to_string(),
        vec![Ar::Noun(Array::scalar_i64(rep.valence as i64)), Ar::Noun(rep.body())],
    )
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

/// The PARENTHESISED REPRESENTATION `5!:6` answers: the same words the
/// linear representation writes, with a bracket around every part that is
/// not one word. Nothing is left to the reader's knowledge of how the parts
/// bind, and a train keeps the tines it was built from rather than the flat
/// spelling that reparses to the same tree.
pub fn parenthesised(ar: &Ar) -> Option<String> {
    paren_spell(ar).map(|(text, _)| text)
}

fn paren_spell(ar: &Ar) -> Option<(String, bool)> {
    if let Some(w) = constant_shortcut(ar) {
        return Some((w, true));
    }
    if let Some(text) = definition_text(ar) {
        return Some((text, false));
    }
    match ar {
        Ar::Prim(s) => Some((word(s), true)),
        Ar::Noun(a) => Some((noun_text(a)?, true)),
        Ar::Derived(sp, ops) => {
            let text = match ops.as_slice() {
                [u] => join(&bracketed(u)?, &word(sp)),
                [u, v] => {
                    let head = join(&bracketed(u)?, &word(sp));
                    format!("{head}{}", bracketed(v)?)
                }
                _ => return None,
            };
            Some((text, false))
        }
        Ar::Train(ops) => {
            let parts: Option<Vec<String>> = ops.iter().map(bracketed).collect();
            Some((parts?.join(" "), false))
        }
    }
}

/// One part of a parenthesised representation: bracketed unless it is a
/// single word already.
fn bracketed(ar: &Ar) -> Option<String> {
    let (text, one_word) = paren_spell(ar)?;
    Some(if one_word { text } else { format!("({text})") })
}

/// The header spelling of an EXPLICIT DEFINITION: `:` over a valence of 1
/// to 4 and a character body. One line is written inline, `3 : 'y + 1'`;
/// several take the `3 : 0` header with the lines below it and a closing
/// `)`, which is the only spelling that holds them.
fn definition_text(ar: &Ar) -> Option<String> {
    let Ar::Derived(sp, ops) = ar else { return None };
    if sp != ":" {
        return None;
    }
    let [Ar::Noun(n), Ar::Noun(body)] = ops.as_slice() else { return None };
    if n.rank() != 0 || body.rank() > 2 {
        return None;
    }
    let v = *n.to_f64_vec()?.first()?;
    if v.fract() != 0.0 || !(1.0..=4.0).contains(&v) {
        return None;
    }
    let Data::Char(cs) = body.row_major_data() else { return None };
    let width = *body.shape.last().unwrap_or(&body.count());
    let cs: Vec<char> = cs.as_slice().to_vec();
    // A body of rank 2 is one row per line, padded to the widest; the
    // padding is not part of what was written.
    let lines: Vec<String> = if body.rank() == 2 {
        cs.chunks(width.max(1))
            .map(|row| row.iter().collect::<String>().trim_end().to_string())
            .collect()
    } else {
        vec![cs.iter().collect()]
    };
    Some(crate::ir::ExplicitRep { valence: v as u8, lines, direct: false }.header_form())
}

/// `n [ ]` written as `n:` where the noun is one of the atoms that has such
/// a word. The representation itself keeps the three parts; only the
/// spelling takes the shortcut.
fn constant_shortcut(ar: &Ar) -> Option<String> {
    let Ar::Train(ops) = ar else { return None };
    let [Ar::Noun(n), Ar::Prim(g), Ar::Prim(h)] = ops.as_slice() else { return None };
    if g != "[" || h != "]" {
        return None;
    }
    constant_word(n)
}

fn spell(ar: &Ar) -> Option<(String, Shape)> {
    // `n [ ]` is how the constant verb is built, and `n:` is how it is
    // spelled where the noun is one of the atoms that has such a word. The
    // shortcut belongs to the spelling alone: the representation itself
    // has to keep the three parts, which is what a gerund reads back.
    if let Some(w) = constant_shortcut(ar) {
        return Some((w, Shape::Word));
    }
    // `n : '…'` is the one phrase whose spelling is not the operands with
    // the conjunction between them: the body is written as source text,
    // and a body of several lines takes the header form instead.
    if let Some(text) = definition_text(ar) {
        return Some((text, Shape::Modified));
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
    // A HOOK's right tine is always written bracketed: two tines and then
    // an odd train reparse as a hook over a fork, which is the same tree,
    // but the reference writes the tree it was given. Only a train that
    // already counts out odd absorbs an odd one in its last place. A
    // constant verb is spelled by its own word and is never taken apart.
    if ops.len() % 2 == 1 && matches!(last, Ar::Train(_)) && constant_shortcut(last).is_none() {
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
    // rather than letting the space end the phrase. `{::` and a constant
    // verb of a NEGATIVE atom are bracketed too — the reference will not
    // let either stand against the conjunction that took it.
    let held_off = text == "{::" || (text.starts_with('_') && text.ends_with(':') && text != "_:");
    let bare = shape == Shape::Word && !text.ends_with(' ') && !held_off;
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

/// One spelling written against the one before it. An inflection is held
/// off the word before it so that the two do not read as one, and so is a
/// modifier whose spelling STARTS with a letter — `2 H.3` and `2 3 H.4`,
/// where running the digits into the `H` would make one word of them.
/// A word ending in anything else takes it directly: `+/M.`, `<L:0`.
fn join(head: &str, tail: &str) -> String {
    if head.ends_with(' ') {
        return format!("{head}{tail}");
    }
    // `_` ends a number as surely as a digit does — the infinities and the
    // negative sign are spelled with it — so a letter after one is held off
    // the same way: `\:"2 _ L:1 2`, not `\:"2 _L:1 2`.
    let runs_on = tail.starts_with(|c: char| c.is_ascii_alphabetic())
        && head.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
    let split = tail.starts_with(['.', ':']) || runs_on;
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
    // A GERUND is boxed data that stands for verbs, and the tie is how it
    // is written: one spelling per box, with `` ` `` between them. TWO
    // representations or more are one, as they are where a modifier reads
    // its operand — a single box is data whatever it holds.
    if let Some(items) = a.as_boxes()
        && a.rank() == 1
        && items.len() > 1
    {
        let parts: Option<Vec<String>> =
            items.iter().map(|b| left(&Ar::from_array(b)?)).collect();
        return parts.map(|p| p.join("`"));
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
fn rank_ar(inner: &Verb, r: crate::verb::Ranks) -> Option<Ar> {
    let der = |s: &str, ops: Vec<Ar>| Some(Ar::Derived(s.to_string(), ops));
    // `u&.v` sets v's MONADIC rank around the same tree `&.:` builds, which
    // is what separates it from `u@v` — that one sets all three of g's.
    if let Verb::Atop(f, g, AtopForm::At) = inner
        && matches!(&**g, Verb::Compose(_, u) if r == [u.ranks()[0]; 3])
        && let Some(ar) = under_ar(f, g, "&.")
    {
        return Some(ar);
    }
    match inner {
        // `m"n`: the constant verb, whose left operand is the noun itself.
        Verb::Constant(m) => der("\"", vec![Ar::Noun(m.clone()), Ar::Noun(rank_noun(r))]),
        // `u@v` is `u@:v` at v's own ranks. A CAPPED fork carries the rank
        // it was written with instead, so it keeps the `"` spelling.
        Verb::Atop(f, g, AtopForm::At) if r == g.ranks() => {
            der("@", vec![verb_ar(f)?, verb_ar(g)?])
        }
        Verb::Compose(f, g) if r == [g.ranks()[0]; 3] => {
            der("&", vec![verb_ar(f)?, verb_ar(g)?])
        }
        Verb::BondLeft(m, g) if r == [g.ranks()[2]; 3] => {
            der("&", vec![Ar::Noun(m.clone()), verb_ar(g)?])
        }
        Verb::BondRight(g, n) if r == [g.ranks()[1]; 3] => {
            der("&", vec![verb_ar(g)?, Ar::Noun(n.clone())])
        }
        // A RANK CONJUNCTION WHOSE RANKS ARE THE VERB'S OWN IS NOT WRITTEN
        // OUT: `#."1 1` writes itself back as `#.`, `+"0` as `+` and `,"_`
        // as `,`, where a rank that changes anything keeps every atom it
        // was written with (`u"2 _ 2` and `u"_ 2` write differently).
        _ if r.triple() == inner.ranks() => verb_ar(inner),
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
