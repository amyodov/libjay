//! J frontend: lexer and the sentence parser, lowering to the shared IR.
//!
//! Word formation and sentence parsing follow the model published in the J
//! Dictionary. Words are formed left to right; a sentence is then executed
//! right to left by pushing words onto a stack and matching the leftmost four
//! stack slots against the parse table after every push.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use crate::array::{Array, Data};
use crate::error::{Error, ErrorKind, Result, Span};
use crate::frontend::{Segment, SourceParts};
use crate::ir::{Branch, Control, ExplicitDef, ExplicitRep, Expr, Scope};
use crate::verb::{
    AtopForm, BoolDyad, DyadOp, Enclose, MonadOp, Power, Prim, ScalarDyad, ScalarMonad,
    Verb, WindowKind,
    RANK_INF,
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
pub fn parse(src: &SourceParts, rules: crate::frontend::Rules) -> Result<Vec<Expr>> {
    let char_bytes =
        !rules.extensions.has(crate::extensions::Extensions::J_UNICODE_STRINGS);
    let mut scope =
        Names { char_bytes, source: Arc::from(src.display.as_str()), ..Names::default() };
    let lines = lex(src, char_bytes)?;
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        // A definition whose body is written on the lines below swallows
        // them, so the sentence that comes out may span several of them.
        let sentence = collect_definitions(&lines, &mut i, &mut scope, true)?;
        if sentence.is_empty() {
            continue;
        }
        out.push(scope.parse_sentence(sentence)?);
    }
    Ok(out)
}

/// What a name of modifier class stands for.
#[derive(Clone, Debug)]
enum Modifier {
    /// A primitive modifier, by the spelling it is written with.
    Prim(&'static str),
    /// An explicit adverb or conjunction, by the body it was written with.
    Explicit(Arc<ModSource>),
}

impl Modifier {
    /// How the modifier names itself in `explain` and in diagnostics.
    fn spelling(&self) -> String {
        match self {
            Modifier::Prim(g) => (*g).to_string(),
            Modifier::Explicit(src) => src.name.clone(),
        }
    }

    /// How a session writes the modifier back out. None where the source
    /// spells it in a way libjay does not keep.
    fn display_text(&self) -> Option<String> {
        match self {
            Modifier::Prim(g) => Some((*g).to_string()),
            Modifier::Explicit(src) => src.rep.as_ref().map(ExplicitRep::display),
        }
    }

    /// What the modifier writes itself out as in the representation forms:
    /// a primitive is its own spelling, and an explicit one the `:` phrase
    /// its header and body make.
    fn rep(&self) -> Option<crate::gerund::Ar> {
        match self {
            Modifier::Prim(g) => Some(crate::gerund::Ar::Prim((*g).to_string())),
            Modifier::Explicit(src) => src.rep.as_ref().map(crate::gerund::explicit_ar),
        }
    }
}

/// The parts of speech a sentence is read against. A name's part of speech
/// decides how the sentence around it parses, so the table is carried from
/// sentence to sentence and into every definition body.
#[derive(Clone)]
struct Names {
    /// The locale the sentences being parsed belong to. A name's part of
    /// speech is the one it has THERE, so the table is keyed by the
    /// spelling the name would be written with from `base` and a bare name
    /// is qualified with this before it is looked up.
    current: String,
    /// The names a definition's own frame holds: its arguments, its
    /// operands, the names a `for_i.` binds and the ones `=.` gave it. They
    /// belong to no locale, so they are looked up under the bare spelling.
    locals: HashSet<String>,
    /// Whether these are the sentences of a definition BODY, where `=.`
    /// writes a local rather than a global.
    in_definition: bool,
    verbs: HashMap<String, Verb>,
    /// Names given an adverb or a conjunction (`m =. /`), by what they
    /// stand for and whether it is a conjunction. A modifier is applied
    /// when a sentence is parsed, so the name has to be resolved then too,
    /// exactly as a verb name is.
    mods: HashMap<String, (bool, Modifier)>,
    /// Names that hold a value by the time a sentence is read. Only the
    /// diagnostics need this: a name that is neither a verb nor a value is
    /// an undefined name, not a sentence the parser has yet to learn.
    nouns: HashSet<String>,
    /// The value of a name given a literal, for the operands that have to
    /// be known while the sentence is read. A gerund is data, so
    /// `` g =. +`- `` and then `g@.1` needs what g holds; an assignment
    /// whose value is not settled at parse time takes the name back out.
    consts: HashMap<String, Array>,
    /// Whether a quoted literal is a vector of BYTES, which is what J's
    /// literal type holds and what the frontend does unless the
    /// `j_unicode_strings` extension says otherwise.
    char_bytes: bool,
    /// The source every span indexes into. A definition whose body is
    /// written on the LINES BELOW keeps no text of its own, and a session
    /// displaying it by name gives the text back, so the lines are read
    /// out of the source again — leading spaces and all, which is what
    /// jconsole shows.
    source: Arc<str>,
}

/// The verb the two locale words stand for. Both make the locale they name
/// current, creating it if it is new; the reference gives them the same
/// meaning and libjay follows it.
fn co_verb(name: &'static str) -> Verb {
    Verb::Prim(Prim {
        name,
        monad: MonadOp::CoCurrent,
        dyad: DyadOp::None,
        ranks: [RANK_INF; 3],
    })
}

impl Default for Names {
    fn default() -> Names {
        // `cocurrent` and `coclass` are the interpreter's own, and the
        // reference answers them with a bare profile: they live in `z`,
        // which every locale's search path holds, so a program is free to
        // give either name a meaning of its own in a locale of its own.
        let verbs = HashMap::from([
            ("cocurrent_z_".to_string(), co_verb("cocurrent")),
            ("coclass_z_".to_string(), co_verb("coclass")),
        ]);
        Names {
            current: crate::verb::BASE_LOCALE.to_string(),
            locals: HashSet::new(),
            in_definition: false,
            verbs,
            mods: HashMap::new(),
            nouns: HashSet::new(),
            consts: HashMap::new(),
            char_bytes: false,
            source: Arc::from(""),
        }
    }
}

impl Names {
    /// The table key a name has: the spelling it would be written with from
    /// `base`. A bare name belongs to the locale being parsed, and the
    /// locative that spells `base` out is the same name as the bare one.
    fn key(&self, name: &str) -> String {
        // An indirect locative names a locale that is not known until the
        // program runs, so no key stands for it.
        if self.locals.contains(name) || crate::verb::split_indirect(name).is_some() {
            return name.to_string();
        }
        match crate::verb::split_locative(name) {
            Some((head, crate::verb::BASE_LOCALE)) => head.to_string(),
            Some(..) => name.to_string(),
            None if self.current == crate::verb::BASE_LOCALE => name.to_string(),
            None => format!("{name}_{}_", self.current),
        }
    }

    /// The keys a name is looked for under, in order: its own locale, then
    /// the locales that locale's search path names. The path is not
    /// followed past one step, which is what the reference does.
    fn lookup_keys(&self, name: &str) -> Vec<String> {
        if self.locals.contains(name) || crate::verb::split_indirect(name).is_some() {
            return vec![name.to_string()];
        }
        let (head, locale) = match crate::verb::split_locative(name) {
            Some((head, locale)) => (head, locale.to_string()),
            None => (name, self.current.clone()),
        };
        let mut keys = vec![match locale == crate::verb::BASE_LOCALE {
            true => head.to_string(),
            false => format!("{head}_{locale}_"),
        }];
        // Every locale but `z` itself starts with `z` on its path, and a
        // program that changes a path changes where a name is FOUND at run
        // time, not what part of speech it has.
        if locale != crate::verb::Z_LOCALE {
            keys.push(format!("{head}_{}_", crate::verb::Z_LOCALE));
        }
        keys
    }

    fn verb_named(&self, name: &str) -> Option<&Verb> {
        self.lookup_keys(name).iter().find_map(|k| self.verbs.get(k))
    }

    /// The verb an INDIRECT locative names, where some locale defines one
    /// under its head.
    ///
    /// The locale is a value, so which of them the name stands for is not
    /// known while the sentence is read; the verbs the head names in every
    /// locale travel with the name and the one the locale holds is chosen
    /// where the verb is applied. `None` where no locale defines the head,
    /// which leaves the name a noun as it was.
    fn locative_verb(&self, name: &str, span: Span) -> Option<Verb> {
        let (head, var) = crate::verb::split_indirect(name)?;
        let mut choices: HashMap<String, Verb> = HashMap::new();
        for (key, verb) in &self.verbs {
            if key == head {
                choices.insert(crate::verb::BASE_LOCALE.to_string(), verb.clone());
            } else if let Some((h, locale)) = crate::verb::split_locative(key)
                && h == head
            {
                choices.insert(locale.to_string(), verb.clone());
            }
        }
        // The template stands for the ranks and the tolerance question; the
        // verb itself is chosen by the locale at every application.
        let template = choices.values().next()?.clone();
        Some(Verb::Deferred(Arc::new(crate::verb::Deferred {
            operand: Expr::Name(var.to_string(), span),
            template,
            build: built_locative,
            spelling: name.to_string(),
            choices,
        })))
    }

    fn mod_named(&self, name: &str) -> Option<&(bool, Modifier)> {
        self.lookup_keys(name).iter().find_map(|k| self.mods.get(k))
    }

    fn is_noun(&self, name: &str) -> bool {
        self.lookup_keys(name).iter().any(|k| self.nouns.contains(k))
    }

    fn const_named(&self, name: &str) -> Option<Array> {
        self.lookup_keys(name).iter().find_map(|k| self.consts.get(k)).cloned()
    }

    /// Forget everything the table knows about a name, under the key it has
    /// in the locale being parsed.
    fn forget(&mut self, name: &str) {
        let key = self.key(name);
        self.verbs.remove(&key);
        self.mods.remove(&key);
        self.nouns.remove(&key);
        self.consts.remove(&key);
    }

    /// Parse one sentence against the table and note what it did to the
    /// names it mentions.
    fn parse_sentence(&mut self, mut sentence: Vec<Frag>) -> Result<Expr> {
        substitute_names(&mut sentence, self);
        let whole = sentence_span(&sentence);
        let frag = reduce_to_fragment(sentence, self)?;
        // A sentence that names a modifier is settled here rather than in
        // the IR: what the name stands for is a parser object, and the
        // node the sentence lowers to carries only its spelling.
        if let Some(Frag::ModDef(name, conj, m, span)) = frag {
            let spelling = m.spelling();
            let rep = m.rep();
            self.forget(&name);
            self.mods.insert(self.key(&name), (conj, m));
            return Ok(Expr::ModDef { name, spelling, conjunction: conj, rep, span });
        }
        let stmt = lower_sentence(frag, whole, self.char_bytes)?;
        self.record(&stmt);
        Ok(stmt)
    }

    /// Note what a parsed sentence did to the names it mentions, and to the
    /// locale the sentences after it belong to.
    fn record(&mut self, stmt: &Expr) {
        if let Some(locale) = locale_switch(stmt) {
            self.current = locale;
        }
        match stmt {
            Expr::VerbDef { name, verb, .. } => {
                let verb = verb.clone();
                self.forget(name);
                self.verbs.insert(self.key(name), verb);
            }
            // A name given a noun stops being a verb, at any depth: J lets
            // a name change part of speech, and the oracle agrees.
            other => {
                let mut assigned = Vec::new();
                assigned_names(other, &mut assigned);
                for name in assigned {
                    // Inside a definition, `=.` writes the frame: the name
                    // belongs to no locale from here on.
                    if self.in_definition
                        && assigns_locally(other, &name)
                        && crate::verb::split_locative(&name).is_none()
                    {
                        self.forget(&name);
                        self.locals.insert(name.clone());
                    }
                    self.forget(&name);
                    let key = self.key(&name);
                    self.nouns.insert(key.clone());
                    if let Some(a) = literal_assigned(other, &name) {
                        self.consts.insert(key, a);
                    }
                }
            }
        }
    }
}

/// The locale a sentence makes current for the sentences after it.
///
/// `cocurrent` and `coclass` are answered at run time like any other verb;
/// this is the compile-time half of the same switch, because the locale a
/// sentence belongs to decides what part of speech its names have. Only a
/// whole sentence that is one of the two applied to a LITERAL counts: a
/// computed locale name is not known while the program is being read.
fn locale_switch(stmt: &Expr) -> Option<String> {
    let Expr::Monad { verb, y, .. } = stmt else { return None };
    let Verb::Prim(p) = verb else { return None };
    if p.monad != MonadOp::CoCurrent {
        return None;
    }
    let Expr::Const(a, _) = &**y else { return None };
    locale_literal(a)
}

/// The locale name a literal spells, boxed or bare.
fn locale_literal(a: &Array) -> Option<String> {
    let inner = match &a.data {
        Data::Box(v) if a.count() == 1 => v[0].clone(),
        Data::Char(_) => a.clone(),
        _ => return None,
    };
    match &inner.data {
        Data::Char(c) => Some(c.as_slice().iter().collect()),
        _ => None,
    }
}

/// Whether the sentence gave this name a value with `=.` rather than `=:`.
/// Inside a definition that is the frame, which no locale reaches.
fn assigns_locally(stmt: &Expr, name: &str) -> bool {
    match stmt {
        Expr::Assign { name: n, scope, value, .. } => {
            (n == name && *scope != crate::ir::Scope::Global) || assigns_locally(value, name)
        }
        Expr::AssignMany { names, scope, value, .. } => {
            (names.iter().any(|n| n == name) && *scope != crate::ir::Scope::Global)
                || assigns_locally(value, name)
        }
        Expr::Monad { y, .. } => assigns_locally(y, name),
        Expr::Dyad { x, y, .. } => {
            assigns_locally(x, name) || assigns_locally(y, name)
        }
        Expr::PrintPass { value, .. } => assigns_locally(value, name),
        _ => false,
    }
}

/// The literal a sentence gave a name, where the whole sentence is that one
/// assignment and its value is settled already.
fn literal_assigned(stmt: &Expr, name: &str) -> Option<Array> {
    match stmt {
        Expr::Assign { name: n, value, .. } if n == name => match &**value {
            Expr::Const(a, _) => Some(a.clone()),
            _ => None,
        },
        _ => None,
    }
}

// ------------------------------------------------------- explicit definitions

/// J's control words. `for_i.` and its relatives carry the name they bind,
/// which is why the suffix is kept apart from the word.
const CONTROL_WORDS: [&str; 18] = [
    "if.", "do.", "else.", "elseif.", "end.", "while.", "whilst.", "for.", "select.", "case.",
    "fcase.", "return.", "break.", "continue.", "try.", "catch.", "catcht.", "throw.",
];

/// A control word and, for `for_i.`, the name it binds.
fn control_word(word: &str) -> Option<(&'static str, Option<String>)> {
    if let Some(w) = CONTROL_WORDS.iter().copied().find(|&w| w == word) {
        return Some((w, None));
    }
    for (stem, w) in [("for_", "for."), ("goto_", "goto."), ("label_", "label.")] {
        if let Some(rest) = word.strip_prefix(stem) {
            let name = rest.strip_suffix('.')?;
            if !name.is_empty() && is_j_name(name) {
                return Some((w, Some(name.to_string())));
            }
        }
    }
    None
}

fn is_j_name(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next().is_some_and(|c| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether a word made of letters, digits and underscores is a name J would
/// accept.
///
/// A TRAILING underscore is reserved: it closes a locative, `name_locale_`,
/// and there is no other way for a name to end in one. So `a_` is not a
/// name at all, while `a_b_` (the name `a` in locale `b`) and `a__` are.
/// The locale is empty, all digits, or a letter followed by alphanumerics,
/// and what precedes the suffix has to be a name in its own right — which
/// is what tells `a__` from `a___`.
fn is_well_formed_name(word: &str) -> bool {
    let Some(body) = word.strip_suffix('_') else { return true };
    let Some(cut) = body.rfind('_') else { return false };
    let locale = &body[cut + 1..];
    let named = locale.is_empty()
        || locale.bytes().all(|b| b.is_ascii_digit())
        || (locale.starts_with(|c: char| c.is_ascii_alphabetic())
            && locale.bytes().all(|b| b.is_ascii_alphanumeric()));
    named && !body[..cut].is_empty() && is_well_formed_name(&body[..cut])
}

/// One piece of a definition body: a run of ordinary words, or a control
/// word. A line break ends a sentence, and so does every control word.
#[derive(Clone, Debug)]
enum Item {
    Sentence(Vec<Frag>),
    Word { word: &'static str, suffix: Option<String>, span: Span },
}

impl Item {
    fn word(&self) -> Option<&'static str> {
        match self {
            Item::Word { word, .. } => Some(word),
            Item::Sentence(_) => None,
        }
    }

    fn span(&self) -> Span {
        match self {
            Item::Word { span, .. } => *span,
            Item::Sentence(f) => sentence_span(f),
        }
    }
}

/// Collapse every explicit definition in one line into a verb fragment.
///
/// `lines[*i]` is the line to read; `*i` advances past it and past any lines
/// a definition took for its body. At the top level a control word is a
/// spelling error, which is what the reference calls it.
fn collect_definitions(
    lines: &[Vec<Frag>],
    i: &mut usize,
    scope: &mut Names,
    top_level: bool,
) -> Result<Vec<Frag>> {
    let mut sentence = lines[*i].clone();
    *i += 1;
    // `f =. 3 : '… f …'` calls itself by name, so the body has to be parsed
    // with `f` already a verb. The name is resolved when it is applied.
    let self_name = match (sentence.first(), sentence.get(1)) {
        (Some(Frag::Name(n, _)), Some(a)) if a.is_assign() => Some(n.clone()),
        _ => None,
    };
    loop {
        let Some(open) = sentence.iter().position(|f| matches!(f, Frag::DdOpen(..))) else {
            match find_colon_definition(&sentence) {
                Some(at) => {
                    take_colon_definition(&mut sentence, at, lines, i, scope, self_name.as_deref())?;
                    continue;
                }
                // Every definition on the line is now one verb fragment, so
                // a control word still standing is one nothing encloses.
                None => {
                    if top_level
                        && let Some(Frag::Control(_, _, span)) =
                            sentence.iter().find(|f| matches!(f, Frag::Control(..)))
                    {
                        return Err(Error::parse(
                            "control words are only meaningful inside an explicit definition",
                            *span,
                        ));
                    }
                    return Ok(sentence);
                }
            }
        };
        take_direct_definition(&mut sentence, open, lines, i, scope, self_name.as_deref())?;
    }
}

/// The index of the `:` of a `m : n` definition, if the line has one.
fn find_colon_definition(sentence: &[Frag]) -> Option<usize> {
    (1..sentence.len().saturating_sub(1)).find(|&k| {
        matches!(&sentence[k], Frag::Conj(Modifier::Prim(":"), _))
            && as_const(&sentence[k - 1]).is_some_and(|a| a.rank() == 0)
            && matches!(&sentence[k + 1], Frag::Noun(Expr::Const(..)))
    })
}

/// `m : n` — the definition whose body is a string, or the lines below when
/// the right operand is `0`.
fn take_colon_definition(
    sentence: &mut Vec<Frag>,
    at: usize,
    lines: &[Vec<Frag>],
    i: &mut usize,
    scope: &mut Names,
    self_name: Option<&str>,
) -> Result<()> {
    let span = Span::merge(sentence[at - 1].span(), sentence[at + 1].span());
    let valence = as_const(&sentence[at - 1])
        .and_then(Array::to_f64_vec)
        .and_then(|v| v.first().copied())
        .ok_or_else(|| Error::parse("an explicit definition starts with a number", span))?;
    let body_arr = as_const(&sentence[at + 1]).cloned().expect("checked by the finder");
    let body_span = sentence[at + 1].span();
    // `0 : 'text'` is the noun definition whose body is written inline: the
    // string IS the value. The `0 : 0` form never reaches here — the lexer
    // takes the lines below it as text before anything reads them.
    if valence == 0.0 {
        if !matches!(body_arr.data, Data::Char(_)) {
            return Err(Error::parse("a noun definition takes 0 or a string", body_span));
        }
        sentence.splice(at - 1..at + 2, [Frag::Noun(Expr::Const(body_arr, span))]);
        return Ok(());
    }
    // `13 : '…'` reads its valence off the body, so it is settled below.
    let (dyadic, modifier) = match valence {
        3.0 => (false, None),
        4.0 => (true, None),
        1.0 => (false, Some(false)),
        2.0 => (false, Some(true)),
        13.0 => (false, None),
        v => return Err(Error::domain(format!("{v} is not an explicit definition"), span)),
    };
    // The lines the definition is written back out from. A body written on
    // ONE line is given back inline whichever way it was typed, and a
    // longer one keeps the `0` header and the lines below it.
    let rep_lines: Option<Vec<String>>;
    let body = match &body_arr.data {
        // `3 : 0`: the body is written on the lines below, ending with `)`.
        Data::I64(_)
        | Data::F64(_)
        | Data::Bool(_)
        | Data::Ext(_)
        | Data::Rat(_)
        | Data::Complex(_) => {
            if body_arr.to_f64_vec().as_deref() != Some(&[0.0]) {
                return Err(Error::parse("an explicit definition takes 0 or a string", body_span));
            }
            let (body, texts) = take_lines_until_paren(lines, i, body_span, &scope.source)?;
            // One line is written back inline, as the reference does:
            // `3 : 0` over `y + 1` displays as `3 : 'y + 1'`. Two or more
            // keep the header and the lines, because there is no inline
            // spelling that holds them.
            rep_lines = Some(texts);
            body
        }
        Data::Char(chars) => {
            // A body written as a literal is source text again, so where a
            // literal holds bytes they are read back as the characters they
            // spell before the body is lexed.
            let text = crate::fmt::text_of(
                chars.as_slice().iter().copied(),
                scope.char_bytes,
            );
            let mut frags = Vec::new();
            // The body sits one character past the opening quote; a doubled
            // quote inside it shifts what follows by one column.
            lex_line(&text, body_span.start + 1, scope.char_bytes, &mut frags)?;
            rep_lines = Some(text.split('\n').map(str::to_string).collect());
            vec![frags]
        }
        Data::Symbol(_) | Data::Box(_) => {
            return Err(Error::parse("an explicit definition takes 0 or a string", body_span))
        }
    };
    if valence == 13.0 {
        let verb = tacit_definition(body, rep_lines.as_deref(), scope, span)?;
        sentence.splice(at - 1..at + 2, [Frag::Verb(VerbFrag::V(verb), span)]);
        return Ok(());
    }
    let rep = rep_lines.map(|lines| ExplicitRep { valence: valence as u8, lines, direct: false });
    if let Some(conjunction) = modifier {
        let name = if conjunction { "2 : '...'" } else { "1 : '...'" };
        let mut src = mod_source(name, conjunction, body, self_name);
        src.rep = rep;
        let frag = if conjunction {
            Frag::Conj(Modifier::Explicit(Arc::new(src)), span)
        } else {
            Frag::Adverb(Modifier::Explicit(Arc::new(src)), span)
        };
        sentence.splice(at - 1..at + 2, [frag]);
        return Ok(());
    }
    let name = if dyadic { "4 : '...'" } else { "3 : '...'" };
    let verb = build_definition(body, dyadic, name, rep, scope, self_name)?;
    sentence.splice(at - 1..at + 2, [Frag::Verb(VerbFrag::V(verb), span)]);
    Ok(())
}

// ------------------------------------------------------- tacit definitions

/// `13 : '…'` — the TACIT verb a body's use of `x` and `y` translates to.
///
/// The body is parsed as an ordinary sentence with the two arguments
/// standing as names, and the tree that comes back is abstracted over
/// them: a part that reads neither is folded to its value, `y` becomes `]`
/// and `x` becomes `[`, and every application becomes the train, noun fork
/// or composition that computes the same thing without naming an argument.
/// A body the abstraction cannot reach becomes the ordinary explicit
/// definition instead, which is what the reference falls back to as well.
fn tacit_definition(
    body: Vec<Vec<Frag>>,
    text: Option<&[String]>,
    scope: &Names,
    span: Span,
) -> Result<Verb> {
    // The reference reads a control word in a tacit body as a word with an
    // inflection it does not have, and refuses the body outright.
    if let Some(f) = body.iter().flatten().find(|f| matches!(f, Frag::Control(..))) {
        let _ = span;
        return Err(Error::parse("a tacit definition has no control words", f.span()));
    }
    let dyadic = mentions(&body, "x");
    let name = if dyadic { "4 : '...'" } else { "3 : '...'" };
    // The fallback definition takes the valence the BODY asks for, which is
    // the one the reference's own fallback writes.
    let rep = text.map(|lines| ExplicitRep {
        valence: if dyadic { 4 } else { 3 },
        lines: lines.to_vec(),
        direct: false,
    });
    let mut inner = scope.clone();
    for arg in ["x", "y"] {
        inner.nouns.insert(arg.to_string());
        inner.verbs.remove(arg);
        inner.mods.remove(arg);
        inner.consts.remove(arg);
    }
    let translated = match body.as_slice() {
        [line] => {
            let mut sentence = line.clone();
            substitute_names(&mut sentence, &inner);
            // A name the program has already given a value stands for that
            // value here: the translation reads it now, so what it holds
            // becomes part of the verb rather than a name the verb looks up.
            for f in sentence.iter_mut() {
                if let Frag::Name(n, sp) = f
                    && let Some(a) = inner.consts.get(n)
                {
                    *f = Frag::Noun(Expr::Const(a.clone(), *sp));
                }
            }
            match reduce_to_fragment(sentence, &inner) {
                Ok(Some(f @ (Frag::Noun(_) | Frag::Name(..)))) => {
                    as_noun(f).ok().and_then(|e| translate_tacit(&e, dyadic))
                }
                _ => None,
            }
        }
        _ => None,
    };
    match translated {
        Some(Tacit::Verb(v)) => Ok(v),
        Some(Tacit::Const(a)) => Ok(constant_of(a)),
        None => build_definition(body, dyadic, name, rep, scope, None),
    }
}

/// What one part of a tacit body stands for.
enum Tacit {
    /// It reads neither argument, so its value is settled here.
    Const(Array),
    /// A verb that computes it from the arguments.
    Verb(Verb),
}

/// The verb that answers a constant whatever its arguments are. An integer
/// atom small enough has a word of its own; everything else is `m"_`.
fn constant_of(a: Array) -> Verb {
    if a.rank() == 0
        && matches!(a.data, Data::I64(_))
        && a.to_f64_vec().is_some_and(|v| v[0].abs() <= 9.0)
    {
        return constant_verb(a);
    }
    Verb::Rank(Box::new(Verb::Constant(a)), [crate::verb::RANK_INF; 3].into())
}

/// Whether an expression reads either argument.
fn reads_args(e: &Expr) -> bool {
    match e {
        Expr::Name(name, _) => name == "x" || name == "y",
        Expr::Monad { y, .. } => reads_args(y),
        Expr::Dyad { x, y, .. } => reads_args(x) || reads_args(y),
        Expr::Assign { value, .. } => reads_args(value),
        _ => false,
    }
}

/// The abstraction itself: the verb (or the value) one part of the body
/// stands for. None where nothing in the tacit vocabulary computes it.
fn translate_tacit(e: &Expr, dyadic: bool) -> Option<Tacit> {
    if !reads_args(e) {
        return noun_value(&Frag::Noun(e.clone())).map(Tacit::Const);
    }
    match e {
        Expr::Name(n, _) if n == "y" => Some(Tacit::Verb(verb_for("]")?)),
        Expr::Name(n, _) if n == "x" => Some(Tacit::Verb(verb_for("[")?)),
        // The name an assignment gives is not part of what the body
        // computes; its value is.
        Expr::Assign { value, .. } => translate_tacit(value, dyadic),
        Expr::Monad { verb, y, .. } => {
            let Tacit::Verb(v) = translate_tacit(y, dyadic)? else { return None };
            // A body that never names `x` derives a monad, and there the
            // argument arrives whole: `f y` is f itself. A body that does
            // name `x` has to keep both valences apart, so the argument is
            // selected first.
            let out = if !dyadic && is_prim(&v, "]") {
                verb.clone()
            } else {
                // The reference writes this composition as a capped fork,
                // which is the spelling the translation gives back.
                Verb::Atop(Box::new(verb.clone()), Box::new(v), AtopForm::Cap)
            };
            Some(Tacit::Verb(out))
        }
        Expr::Dyad { verb, x, y, .. } => {
            let l = translate_tacit(x, dyadic)?;
            let r = translate_tacit(y, dyadic)?;
            Some(Tacit::Verb(join_tacit(l, verb, r, dyadic)?))
        }
        _ => None,
    }
}

/// Two abstracted parts with a verb between them.
fn join_tacit(l: Tacit, f: &Verb, r: Tacit, dyadic: bool) -> Option<Verb> {
    let (u, v) = match (l, r) {
        // One side is a value: the noun fork answers `m f (…)` in both
        // valences, so a constant on the RIGHT changes places with the
        // verb rather than staying where it was written.
        (Tacit::Const(m), Tacit::Verb(v)) => {
            return Some(Verb::NounFork(m, Box::new(f.clone()), Box::new(v)))
        }
        (Tacit::Verb(u), Tacit::Const(n)) => {
            return Some(Verb::NounFork(n, Box::new(flip(f)), Box::new(u)))
        }
        (Tacit::Const(_), Tacit::Const(_)) => return None,
        (Tacit::Verb(u), Tacit::Verb(v)) => (u, v),
    };
    // `x f y` is f, and `y f x` is its commutation: the fork that would
    // say the same thing is written the short way.
    if is_prim(&u, "[") && is_prim(&v, "]") {
        return Some(f.clone());
    }
    if is_prim(&u, "]") && is_prim(&v, "[") {
        return Some(flip(f));
    }
    // `y f y` is the REFLEXIVE, which is written with the commutation
    // whether or not the verb says the same thing both ways round.
    if !dyadic && is_prim(&u, "]") && is_prim(&v, "]") {
        return Some(Verb::Commute(Box::new(f.clone())));
    }
    // A train needs brackets as the LEFT tine and none as the right one,
    // so a left tine that is one changes places where the other is not.
    if is_train(&u) && !is_train(&v) {
        return Some(Verb::Fork(Box::new(v), Box::new(flip(f)), Box::new(u)));
    }
    Some(Verb::Fork(Box::new(u), Box::new(f.clone()), Box::new(v)))
}

/// The verb that takes the same two arguments the other way round. A verb
/// that says the same thing either way is itself; one whose mirror has a
/// spelling of its own is that spelling; everything else commutes.
fn flip(f: &Verb) -> Verb {
    if let Verb::Prim(p) = f {
        if COMMUTATIVE.contains(&p.name) {
            return f.clone();
        }
        if let Some(other) = CONVERSE.iter().find(|(a, _)| *a == p.name)
            && let Some(v) = verb_for(other.1)
        {
            return v;
        }
    }
    Verb::Commute(Box::new(f.clone()))
}

/// The dyads that answer the same thing whichever way round the arguments
/// come, so that a commutation of them is written without one.
const COMMUTATIVE: [&str; 11] =
    ["+", "*", "<.", ">.", "=", "~:", "+.", "*.", "+:", "*:", "-:"];

/// The dyads whose mirror is another spelling rather than a commutation.
const CONVERSE: [(&str, &str); 4] = [("<", ">"), (">", "<"), ("<:", ">:"), (">:", "<:")];

fn is_prim(v: &Verb, name: &str) -> bool {
    matches!(v, Verb::Prim(p) if p.name == name)
}

/// Whether the verb is written as a train, which is what decides where a
/// bracket has to go.
fn is_train(v: &Verb) -> bool {
    matches!(crate::gerund::verb_ar(v), Some(crate::gerund::Ar::Train(_)))
}

/// The lines of a `3 : 0` body: everything up to a line that is a lone `)`.
fn take_lines_until_paren(
    lines: &[Vec<Frag>],
    i: &mut usize,
    span: Span,
    source: &str,
) -> Result<(Vec<Vec<Frag>>, Vec<String>)> {
    let mut body = Vec::new();
    let mut texts = Vec::new();
    loop {
        let Some(line) = lines.get(*i) else {
            return Err(Error::parse("this definition's body has no closing `)`", span));
        };
        *i += 1;
        if line.len() == 1 && matches!(line[0], Frag::RParen(_)) {
            return Ok((body, texts));
        }
        if let Some(text) = source_line(source, line) {
            texts.push(text);
        }
        body.push(line.clone());
    }
}

/// The whole source LINE a sentence was written on, from the newline before
/// its first word to the newline after its last. A definition's body is
/// given back exactly as it was typed, indentation included, so the text
/// comes from the source rather than from the words the lexer made of it.
fn source_line(source: &str, line: &[Frag]) -> Option<String> {
    let (first, last) = (line.first()?, line.last()?);
    let (from, to) = (first.span().start, last.span().end);
    if to > source.len() || from > to {
        return None;
    }
    let start = source[..from].rfind('\n').map_or(0, |n| n + 1);
    let end = source[to..].find('\n').map_or(source.len(), |n| to + n);
    Some(source[start..end].to_string())
}

/// The BODY LINES of a direct definition, as the representation keeps them:
/// the source between the braces, with the blanks that hold the body off
/// the opening `{{` dropped — the spaces on the same line, or the newline
/// that starts the body on the next one — and the newline before the
/// closing `}}` dropped too. What is left is written back out as a
/// definition's body, indentation and trailing spaces included.
fn direct_body_text(source: &str, open: Span, close: Span) -> Option<Vec<String>> {
    if close.start > source.len() || open.end > close.start {
        return None;
    }
    let text = &source[open.end..close.start];
    let text = text.trim_start_matches([' ', '\t']);
    let text = text.strip_prefix('\n').unwrap_or(text);
    let text = text.strip_suffix('\n').unwrap_or(text);
    Some(text.split('\n').map(str::to_string).collect())
}

/// `{{ … }}` — the body is the words between the braces, on this line or on
/// the lines below.
fn take_direct_definition(
    sentence: &mut Vec<Frag>,
    open: usize,
    lines: &[Vec<Frag>],
    i: &mut usize,
    scope: &mut Names,
    self_name: Option<&str>,
) -> Result<()> {
    let open_span = sentence[open].span();
    let Frag::DdOpen(marker, _) = sentence[open] else {
        return Err(Error::internal("expected a direct definition's opening brackets"));
    };
    let mut depth = 1usize;
    let mut body: Vec<Vec<Frag>> = Vec::new();
    let mut tail: Vec<Frag> = Vec::new();
    let mut close_span = open_span;
    let mut line: Vec<Frag> = sentence[open + 1..].to_vec();
    let mut cur: Vec<Frag> = Vec::new();
    loop {
        let mut closed = false;
        for (k, f) in line.iter().enumerate() {
            match f {
                Frag::DdOpen(..) => {
                    depth += 1;
                    cur.push(f.clone());
                }
                Frag::DdClose(s) => {
                    depth -= 1;
                    if depth == 0 {
                        close_span = *s;
                        tail = line[k + 1..].to_vec();
                        closed = true;
                        break;
                    }
                    cur.push(f.clone());
                }
                _ => cur.push(f.clone()),
            }
        }
        if !cur.is_empty() {
            body.push(std::mem::take(&mut cur));
        }
        if closed {
            break;
        }
        let Some(next) = lines.get(*i) else {
            return Err(Error::parse("this definition has no closing `}}`", open_span));
        };
        *i += 1;
        line = next.clone();
    }
    let span = Span::merge(open_span, close_span);
    // The body's own words decide the part of speech, as they do in the
    // reference: an operand name of the second position makes a
    // conjunction, one of the first an adverb, and neither a verb. A
    // `{{)a` marker says it outright instead.
    let part = match marker {
        None => {
            if mentions(&body, "v") || mentions(&body, "n") {
                Some(true)
            } else if mentions(&body, "u") || mentions(&body, "m") {
                Some(false)
            } else {
                None
            }
        }
        Some('a') => Some(false),
        Some('c') => Some(true),
        Some('v' | 'm' | 'd') => None,
        Some(other) => {
            return Err(Error::not_yet(
                format!("a direct definition marked `){other}`"),
                open_span,
            ))
        }
    };
    let text = direct_body_text(&scope.source, open_span, close_span);
    let frag = match part {
        Some(conjunction) => {
            let mut src = mod_source("{{ ... }}", conjunction, body, self_name);
            src.rep = text.map(|lines| ExplicitRep {
                valence: if conjunction { 2 } else { 1 },
                lines,
                direct: true,
            });
            let m = Modifier::Explicit(Arc::new(src));
            if conjunction { Frag::Conj(m, span) } else { Frag::Adverb(m, span) }
        }
        None => {
            // `)d` and `)m` fix the valence; otherwise a body that names
            // `x` is a dyad and nothing else.
            let dyadic = match marker {
                Some('d') => true,
                Some('m') => false,
                _ => mentions(&body, "x"),
            };
            let rep = text.map(|lines| ExplicitRep {
                valence: if dyadic { 4 } else { 3 },
                lines,
                direct: true,
            });
            let verb = build_definition(body, dyadic, "{{ ... }}", rep, scope, self_name)?;
            Frag::Verb(VerbFrag::V(verb), span)
        }
    };
    let mut head: Vec<Frag> = sentence[..open].to_vec();
    head.push(frag);
    head.extend(tail);
    *sentence = head;
    Ok(())
}

/// True for the body line that is a lone `:` — the marker between an
/// explicit definition's monad case and its dyad case.
fn is_case_separator(line: &[Frag]) -> bool {
    matches!(line, [Frag::Conj(Modifier::Prim(":"), _)])
}

/// `m : n` where n is a boxed list of lines: the body of a multi-line
/// definition, one line per box, given as a value instead of as the text
/// below the sentence.
fn boxed_body_definition(u: &Frag, v: &Frag, scope: &Names, span: Span) -> Result<Frag> {
    let valence = noun_value(u)
        .filter(|a| a.rank() == 0)
        .and_then(|a| a.to_f64_vec())
        .and_then(|n| n.first().copied())
        .ok_or_else(|| Error::parse("an explicit definition starts with a number", span))?;
    let dyadic = match valence {
        3.0 => false,
        4.0 => true,
        1.0 | 2.0 => {
            return Err(Error::not_yet(
                "an explicit modifier whose body is a boxed list of lines",
                span,
            ));
        }
        13.0 => return Err(Error::not_yet("tacit definitions (13 : '...')", span)),
        n => return Err(Error::domain(format!("{n} is not an explicit definition"), span)),
    };
    let Some(body_arr) = noun_value(v) else {
        return Err(Error::not_yet(
            "an explicit definition whose body is not known until the sentence runs",
            span,
        ));
    };
    let Some(boxes) = body_arr.as_boxes() else {
        return Err(Error::parse(
            "an explicit definition takes 0, a string, or a boxed list of lines",
            span,
        ));
    };
    let mut body = Vec::with_capacity(boxes.len());
    for b in boxes.iter() {
        let Data::Char(chars) = &b.data else {
            return Err(Error::parse("every box of an explicit body holds one line", span));
        };
        let text = crate::fmt::text_of(chars.as_slice().iter().copied(), scope.char_bytes);
        let mut frags = Vec::new();
        lex_line(&text, span.start, scope.char_bytes, &mut frags)?;
        body.push(frags);
    }
    let name = if dyadic { "4 : '...'" } else { "3 : '...'" };
    let verb = build_definition(body, dyadic, name, None, scope, None)?;
    Ok(Frag::Verb(VerbFrag::V(verb), span))
}

/// True where a definition's words include this name.
fn mentions(body: &[Vec<Frag>], name: &str) -> bool {
    body.iter().any(|l| l.iter().any(|f| matches!(f, Frag::Name(n, _) if n == name)))
}

/// Parse a definition's body and wrap it in a verb.
fn build_definition(
    body: Vec<Vec<Frag>>,
    dyadic: bool,
    name: &str,
    rep: Option<ExplicitRep>,
    scope: &Names,
    self_name: Option<&str>,
) -> Result<Verb> {
    // A line that is nothing but `:` separates the monad case from the dyad
    // case: what precedes it is applied with y alone, what follows with x
    // and y both, and the definition is one verb out of the two.
    if let Some(cut) = body.iter().position(|l| is_case_separator(l)) {
        let monad =
            build_definition(body[..cut].to_vec(), false, name, None, scope, self_name)?;
        let dyad =
            build_definition(body[cut + 1..].to_vec(), true, name, None, scope, self_name)?;
        return Ok(Verb::Ambivalent(Box::new(monad), Box::new(dyad)));
    }
    // The body reads the names the program has already given, and binds its
    // own arguments over them.
    let mut inner = scope.clone();
    // A definition belongs to the locale its NAME puts it in — `f_x_ =.`
    // makes it x's — or, failing that, to the one the sentence that defines
    // it belongs to. Its body's bare global names are that locale's,
    // whatever locale a caller happens to be in.
    let home = self_name
        .and_then(crate::verb::split_locative)
        .map_or_else(|| scope.current.clone(), |(_, l)| l.to_string());
    inner.current = home.clone();
    inner.in_definition = true;
    // Frames do not nest in J, so the body starts with only its own.
    inner.locals = HashSet::new();
    inner.locals.insert("y".to_string());
    inner.nouns.insert("y".to_string());
    inner.verbs.remove("y");
    inner.consts.remove("y");
    if dyadic {
        inner.locals.insert("x".to_string());
        inner.nouns.insert("x".to_string());
        inner.verbs.remove("x");
        inner.consts.remove("x");
    }
    if let Some(n) = self_name {
        inner.forget(n);
        let key = inner.key(n);
        inner.verbs.insert(key, Verb::Named(n.to_string()));
    }
    // A body may hold definitions of its own, and one of them may run past
    // the end of its line, so the lines are collected before they are split
    // into sentences.
    let mut lines: Vec<Vec<Frag>> = Vec::new();
    let mut k = 0usize;
    while k < body.len() {
        let line = collect_definitions(&body, &mut k, &mut inner, false)?;
        if !line.is_empty() {
            lines.push(line);
        }
    }
    let items = split_items(&lines);
    let mut cursor = Cursor { items: &items, at: 0 };
    let mut stmts = parse_block(&mut cursor, &mut inner, &[])?;
    if let Some(item) = cursor.peek() {
        return Err(Error::parse(
            format!("`{}` has no matching opening word", item.word().unwrap_or("word")),
            item.span(),
        ));
    }
    resolve_labels(&mut stmts)?;
    let pure = stmts.iter().all(block_is_pure);
    Ok(Verb::Explicit(Arc::new(ExplicitDef {
        name: name.to_string(),
        left: dyadic.then(|| "x".to_string()),
        right: "y".to_string(),
        // J decides a definition's valence from its header (or, for a
        // `{{ }}`, from its words): one that takes `x` is a dyad only.
        dyad_only: dyadic,
        // A J explicit definition binds both arguments by name or refuses
        // the one it cannot bind, and nothing in J nests lexically.
        spare_left: false,
        enclosing: Vec::new(),
        id: 0,
        result: None,
        locals: Vec::new(),
        // J writes no axis at a call site.
        axis: None,
        body: stmts,
        // A branch that runs nothing yields J's empty result, `i. 0 0`.
        labels: Vec::new(),
        lines: Vec::new(),
        empty: Some(crate::ir::empty_result()),
        pure,
        // J has no `⎕CR`, and nothing else asks a definition for its text.
        source: Vec::new(),
        spelling: rep.as_ref().map(ExplicitRep::display),
        rep,
        home: Some(home),
    })))
}

/// Point every `goto_name.` in a definition at the statement its
/// `label_name.` stands on.
///
/// A label is a target only where it stands on the body's own statement
/// list: one written inside a control structure has no place to branch to,
/// so the reference refuses the definition rather than the branch. So does
/// a name that no label carries, and a name that two of them carry.
fn resolve_labels(stmts: &mut [Expr]) -> Result<()> {
    let mut at: HashMap<String, usize> = HashMap::new();
    for (k, stmt) in stmts.iter().enumerate() {
        if let Expr::Control(c, span) = stmt
            && let Control::Label(name) = &**c
            && at.insert(name.clone(), k).is_some()
        {
            return Err(Error::parse(
                format!("`label_{name}.` is written twice in this definition"),
                *span,
            ));
        }
    }
    for stmt in stmts.iter_mut() {
        point_gotos(stmt, &at)?;
    }
    Ok(())
}

fn point_gotos(stmt: &mut Expr, at: &HashMap<String, usize>) -> Result<()> {
    let Expr::Control(c, span) = stmt else { return Ok(()) };
    let span = *span;
    let block = |b: &mut Vec<Expr>| -> Result<()> {
        for s in b.iter_mut() {
            point_gotos(s, at)?;
        }
        Ok(())
    };
    match &mut **c {
        Control::Goto { name, to } => match at.get(name) {
            Some(k) => *to = *k,
            None => {
                return Err(Error::parse(
                    format!("`goto_{name}.` has no `label_{name}.` to go to"),
                    span,
                )
                .note("a label is a target only on a line of its own in the body, \
                       not inside a control structure"))
            }
        },
        Control::If { arms, otherwise } => {
            for arm in arms {
                if let Some(t) = &mut arm.test {
                    block(t)?;
                }
                block(&mut arm.body)?;
            }
            if let Some(b) = otherwise {
                block(b)?;
            }
        }
        Control::While { test, body, .. } | Control::Guard { test, body } => {
            block(test)?;
            block(body)?;
        }
        Control::Cond { test, body, otherwise } => {
            block(test)?;
            block(body)?;
            block(otherwise)?;
        }
        Control::For { body, .. } => block(body)?,
        Control::Select { cases, .. } => {
            for case in cases {
                if let Some(t) = &mut case.test {
                    block(t)?;
                }
                block(&mut case.body)?;
            }
        }
        Control::Try { body, catch, catcht } => {
            block(body)?;
            block(catch)?;
            block(catcht)?;
        }
        Control::Return
        | Control::Break
        | Control::Continue
        | Control::Throw
        | Control::Label(_)
        | Control::Branch(_)
        | Control::BranchBy { .. } => {}
    }
    Ok(())
}

// ------------------------------------------------------- explicit modifiers

/// An explicit adverb or conjunction: `1 : '…'`, `2 : '…'` and the `{{ … }}`
/// whose body names an operand.
///
/// The body is kept as words rather than as a parsed tree. Applying the
/// modifier substitutes the operands into those words and parses them
/// afresh, which is what J's own substitution rule says happens, and what
/// lets a body that mentions no argument yield a verb of its own.
#[derive(Debug)]
struct ModSource {
    /// How the definition names itself in diagnostics.
    name: String,
    /// Two operands rather than one.
    conjunction: bool,
    body: Vec<Vec<Frag>>,
    /// The body names `x` or `y`, so it is the body of the derived VERB and
    /// runs when that verb is applied. A body that names neither runs at
    /// derivation instead, and what it makes is what the modifier produced.
    deferred: bool,
    /// The body names `x`: the derived verb is a dyad only.
    dyadic: bool,
    /// The name this definition is being given, so that its body can
    /// mention it.
    self_name: Option<String>,
    /// What the source wrote, where it wrote it in a way that can be given
    /// back: how a session displays the modifier, and what the
    /// representation forms answer for it.
    rep: Option<ExplicitRep>,
}

fn mod_source(
    name: &str,
    conjunction: bool,
    body: Vec<Vec<Frag>>,
    self_name: Option<&str>,
) -> ModSource {
    let dyadic = mentions(&body, "x");
    ModSource {
        name: name.to_string(),
        conjunction,
        deferred: dyadic || mentions(&body, "y"),
        dyadic,
        self_name: self_name.map(str::to_string),
        rep: None,
        body,
    }
}

/// How deep one explicit modifier may derive another — itself included.
/// A modifier whose body derives the modifier again is how J writes a
/// recursive one, and it terminates only when a case of the body stops
/// asking; the bound is what turns the one that never stops into a
/// diagnostic rather than a parse that runs out of machine stack.
const DERIVATION_LIMIT: usize = 16;

thread_local! {
    /// The explicit modifiers whose bodies are being parsed right now, by
    /// address. The derivation of a modifier by its own body is a parse
    /// within a parse, so the nesting is counted here.
    static DERIVING: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Removes the innermost derivation from the in-progress list however it
/// ends.
struct Deriving;

impl Drop for Deriving {
    fn drop(&mut self) {
        DERIVING.with(|d| {
            d.borrow_mut().pop();
        });
    }
}

/// Apply an explicit modifier to its operands.
///
/// The operands are substituted into the body under the names J gives them
/// — `u` and `v` for verbs, `m` and `n` for nouns — and the body is parsed
/// with those substitutions in place. A body that names an argument becomes
/// the derived verb's body; one that does not is a sentence, and its value
/// (usually a tacit verb) is what the derivation produced.
fn derive_explicit(
    src: &Arc<ModSource>,
    u: Frag,
    v: Option<Frag>,
    scope: &Names,
    span: Span,
) -> Result<Frag> {
    let addr = Arc::as_ptr(src) as usize;
    // A DEFERRED body is the derived verb's, and the reference parses it
    // only where that verb is applied: a body which derives its own
    // modifier inside it therefore terminates there and not here, where
    // the whole body is parsed at once.
    let (again, deep) = DERIVING.with(|d| {
        let mut d = d.borrow_mut();
        if d.contains(&addr) && src.deferred {
            return (true, false);
        }
        if d.len() >= DERIVATION_LIMIT {
            return (false, true);
        }
        d.push(addr);
        (false, false)
    });
    if again {
        return Err(Error::not_yet(
            "an explicit modifier whose body names an argument and derives the modifier itself",
            span,
        ));
    }
    if deep {
        return Err(Error::new(
            ErrorKind::Domain,
            format!("modifiers derived each other more than {DERIVATION_LIMIT} deep"),
            Some(span),
        )
        .note("a modifier whose body derives the modifier itself needs a case that stops"));
    }
    let _guard = Deriving;
    let mut body = src.body.clone();
    bind_operand(&mut body, "u", "m", &u);
    if let Some(v) = &v {
        bind_operand(&mut body, "v", "n", v);
    }
    // The body reads the names the program has given so far; the name this
    // definition is being given is one of them, and it stands for the
    // modifier rather than for a verb.
    let mut inner = scope.clone();
    if let Some(n) = &src.self_name {
        inner.verbs.remove(n);
        inner.nouns.remove(n);
        inner.consts.remove(n);
        inner.mods.insert(n.clone(), (src.conjunction, Modifier::Explicit(Arc::clone(src))));
    }
    if src.deferred {
        let verb = build_definition(body, src.dyadic, &src.name, None, &inner, None)?;
        return Ok(Frag::Verb(VerbFrag::V(verb), span));
    }
    // The derivation-time phase: the body is a sentence, parsed here and
    // now, and what it reduces to is what the modifier made.
    let mut lines: Vec<Vec<Frag>> = Vec::new();
    let mut k = 0usize;
    while k < body.len() {
        let line = collect_definitions(&body, &mut k, &mut inner, false)?;
        if !line.is_empty() {
            lines.push(line);
        }
    }
    // The operands are known here, so a condition over them is too: the
    // `if.` is settled now and only the branch that holds is parsed. That
    // is what lets a body which derives its own modifier again stop.
    let items = split_items(&lines);
    let mut chosen: Vec<Vec<Frag>> = Vec::new();
    derivation_block(&items, &inner, &mut chosen)?;
    if chosen.len() != 1 {
        return Err(Error::not_yet(
            "an explicit modifier that names no argument and is more than one sentence",
            span,
        ));
    }
    let sentence = chosen.pop().expect("checked length");
    match derivation_fragment(sentence, &inner)? {
        Some(f) if f.is_real_verb() || f.is_noun() => Ok(respan(f, span)),
        Some(f) => Err(Error::not_yet(
            format!("an explicit modifier that produces {}", part_of_speech(&f)),
            span,
        )),
        None => Err(Error::parse("syntax error", span)),
    }
}

/// The control words that open a block, for the scan that finds the one
/// that closes it.
const BLOCK_OPENERS: [&str; 6] = ["if.", "while.", "whilst.", "for.", "select.", "try."];

/// The index of the first of `stop` that belongs to this block level, or
/// the end of the items. Nested blocks are stepped over whole.
fn scan_to(items: &[Item], mut at: usize, stop: &[&str]) -> usize {
    let mut depth = 0usize;
    while at < items.len() {
        if let Some(w) = items[at].word() {
            if depth == 0 && stop.contains(&w) {
                return at;
            }
            if BLOCK_OPENERS.contains(&w) {
                depth += 1;
            } else if w == "end." {
                depth = depth.saturating_sub(1);
            }
        }
        at += 1;
    }
    at
}

/// The sentences a derivation-time body actually runs: its own, with each
/// `if.` replaced by the arm that holds. Only that arm is looked at, which
/// is what keeps a recursive derivation from parsing its own base case and
/// its step at once.
fn derivation_block(items: &[Item], scope: &Names, out: &mut Vec<Vec<Frag>>) -> Result<()> {
    let mut at = 0usize;
    while at < items.len() {
        match &items[at] {
            Item::Sentence(f) => {
                out.push(f.clone());
                at += 1;
            }
            Item::Word { word: "if.", span, .. } => {
                at = derivation_if(items, at + 1, *span, scope, out)?;
            }
            Item::Word { word, span, .. } => {
                return Err(Error::not_yet(
                    format!("`{word}` in a modifier body that runs where the modifier is derived"),
                    *span,
                ))
            }
        }
    }
    Ok(())
}

/// One `if.` block, from just past the word to just past its `end.`.
fn derivation_if(
    items: &[Item],
    mut at: usize,
    span: Span,
    scope: &Names,
    out: &mut Vec<Vec<Frag>>,
) -> Result<usize> {
    let mut taken = false;
    loop {
        let test_end = scan_to(items, at, &["do."]);
        if test_end >= items.len() {
            return Err(Error::parse("this `if.` has no `do.`", span));
        }
        let holds = !taken && derivation_condition(&items[at..test_end], scope, span)?;
        at = test_end + 1;
        let body_end = scan_to(items, at, &["elseif.", "else.", "end."]);
        if body_end >= items.len() {
            return Err(Error::parse("this `if.` has no `end.`", span));
        }
        if holds {
            taken = true;
            derivation_block(&items[at..body_end], scope, out)?;
        }
        at = body_end;
        match items[at].word() {
            Some("elseif.") => at += 1,
            Some("else.") => {
                at += 1;
                let end = scan_to(items, at, &["end."]);
                if end >= items.len() {
                    return Err(Error::parse("this `if.` has no `end.`", span));
                }
                if !taken {
                    derivation_block(&items[at..end], scope, out)?;
                }
                return Ok(end + 1);
            }
            _ => return Ok(at + 1),
        }
    }
}

/// Whether a derivation-time condition holds. It is settled where the
/// modifier is derived, so its value has to be known there.
fn derivation_condition(items: &[Item], scope: &Names, span: Span) -> Result<bool> {
    let mut lines: Vec<Vec<Frag>> = Vec::new();
    derivation_block(items, scope, &mut lines)?;
    let Some(last) = lines.pop() else { return Ok(true) };
    let Some(frag) = derivation_fragment(last, scope)? else {
        return Err(Error::parse("syntax error", span));
    };
    let Some(v) = noun_value(&frag) else {
        return Err(Error::not_yet(
            "a condition in a modifier body that the operands do not settle",
            span,
        ));
    };
    crate::ir::is_true(&v, span)
}

/// One sentence of a derivation-time body as the fragment it stands for.
fn derivation_fragment(mut sentence: Vec<Frag>, scope: &Names) -> Result<Option<Frag>> {
    substitute_names(&mut sentence, scope);
    reduce_to_fragment(sentence, scope)
}

/// What part of speech a fragment belongs to, for a diagnostic.
fn part_of_speech(f: &Frag) -> &'static str {
    match f {
        Frag::Adverb(..) => "an adverb",
        Frag::Conj(..) => "a conjunction",
        _ => "no value",
    }
}

/// Put an operand in the place of the name it arrives under. A verb operand
/// answers to `u` (or `v`), a noun one to `m` (or `n`); the other name is
/// left alone, so a body that reaches for it reports an undefined name, as
/// the reference does.
fn bind_operand(body: &mut [Vec<Frag>], verb_name: &str, noun_name: &str, operand: &Frag) {
    // A NOUN operand answers to both of its spellings — `m` and `u` are one
    // operand written two ways, and the noun is what either of them stands
    // for. A VERB operand answers only to the verb spelling: `m` beside a
    // verb operand is a name with nothing under it.
    let noun = !operand.is_real_verb();
    for line in body.iter_mut() {
        for i in 0..line.len() {
            let Frag::Name(n, span) = &line[i] else { continue };
            if n != verb_name && !(noun && n == noun_name) {
                continue;
            }
            let span = *span;
            // An assignment to the name is a definition of it, not a use.
            if line.get(i + 1).is_some_and(Frag::is_assign) {
                continue;
            }
            line[i] = respan(operand.clone(), span);
        }
    }
}

/// True when nothing in this sentence can have an effect beyond its value.
fn block_is_pure(e: &Expr) -> bool {
    match e {
        Expr::Const(..) | Expr::Param(..) | Expr::Name(..) => true,
        Expr::Monad { verb, y, .. } => verb.is_pure() && block_is_pure(y),
        Expr::Dyad { verb, x, y, .. } => {
            verb.is_pure() && block_is_pure(x) && block_is_pure(y)
        }
        Expr::Assign { value, .. } => block_is_pure(value),
        Expr::Control(c, _) => control_is_pure(c),
        _ => false,
    }
}

fn control_is_pure(c: &Control) -> bool {
    let all = |b: &Vec<Expr>| b.iter().all(block_is_pure);
    match c {
        Control::Return | Control::Break | Control::Continue => true,
        // A branch computes nothing, and a `throw.` only leaves: neither
        // can be seen from outside the definition it stands in.
        Control::Label(_) | Control::Goto { .. } | Control::Throw => true,
        // J has no branch; the variant only reaches this frontend through
        // the shared IR, and reading its target is as pure as any read.
        Control::Branch(target) => block_is_pure(target),
        Control::BranchBy { by, test } => block_is_pure(by) && block_is_pure(test),
        Control::Cond { test, body, otherwise } => all(test) && all(body) && all(otherwise),
        Control::If { arms, otherwise } => {
            arms.iter().all(|a| {
                a.test.as_ref().is_none_or(all) && all(&a.body)
            }) && otherwise.as_ref().is_none_or(all)
        }
        // A dfn guard is APL's; the variant reaches this frontend only
        // through the shared IR.
        Control::While { test, body, .. } | Control::Guard { test, body } => {
            all(test) && all(body)
        }
        Control::For { source, body, .. } => block_is_pure(source) && all(body),
        Control::Select { subject, cases } => {
            block_is_pure(subject)
                && cases.iter().all(|c| c.test.as_ref().is_none_or(all) && all(&c.body))
        }
        Control::Try { body, catch, catcht } => all(body) && all(catch) && all(catcht),
    }
}

/// Split a definition's lines into sentences and control words.
fn split_items(lines: &[Vec<Frag>]) -> Vec<Item> {
    let mut items = Vec::new();
    for line in lines {
        let mut run: Vec<Frag> = Vec::new();
        for f in line {
            match f {
                Frag::Control(word, suffix, span) => {
                    if !run.is_empty() {
                        items.push(Item::Sentence(std::mem::take(&mut run)));
                    }
                    items.push(Item::Word {
                        word,
                        suffix: suffix.clone(),
                        span: *span,
                    });
                }
                _ => run.push(f.clone()),
            }
        }
        if !run.is_empty() {
            items.push(Item::Sentence(run));
        }
    }
    items
}

struct Cursor<'a> {
    items: &'a [Item],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<&'a Item> {
        self.items.get(self.at)
    }

    fn peek_word(&self) -> Option<&'static str> {
        self.peek().and_then(Item::word)
    }

    fn next(&mut self) -> Option<&'a Item> {
        let it = self.items.get(self.at);
        if it.is_some() {
            self.at += 1;
        }
        it
    }

    fn last_span(&self) -> Span {
        self.items
            .get(self.at.saturating_sub(1))
            .map_or_else(|| Span::new(0, 0), Item::span)
    }

    /// Consume the word that must come next.
    fn expect(&mut self, want: &str) -> Result<Span> {
        match self.peek() {
            Some(Item::Word { word, span, .. }) if *word == want => {
                self.at += 1;
                Ok(*span)
            }
            Some(other) => {
                Err(Error::parse(format!("expected `{want}` here"), other.span()))
            }
            None => Err(Error::parse(format!("this block needs a `{want}`"), self.last_span())),
        }
    }
}

/// Parse sentences and control structures until one of `stop` is next.
fn parse_block(cur: &mut Cursor<'_>, scope: &mut Names, stop: &[&str]) -> Result<Vec<Expr>> {
    let mut out = Vec::new();
    loop {
        match cur.peek() {
            None => return Ok(out),
            Some(Item::Word { word, .. }) if stop.contains(word) => return Ok(out),
            Some(Item::Sentence(frags)) => {
                cur.at += 1;
                out.push(scope.parse_sentence(frags.clone())?);
            }
            Some(Item::Word { .. }) => out.push(parse_control(cur, scope)?),
        }
    }
}

fn parse_control(cur: &mut Cursor<'_>, scope: &mut Names) -> Result<Expr> {
    let Some(Item::Word { word, suffix, span }) = cur.next() else {
        return Err(Error::internal("expected a control word"));
    };
    let start = *span;
    let control = match *word {
        "if." => parse_if(cur, scope)?,
        "while." | "whilst." => {
            let body_first = *word == "whilst.";
            let test = parse_block(cur, scope, &["do."])?;
            cur.expect("do.")?;
            let body = parse_block(cur, scope, &["end."])?;
            cur.expect("end.")?;
            Control::While { test, body, body_first, until: false }
        }
        "for." => {
            if let Some(name) = suffix {
                // The loop's two names are the frame's, like any other `=.`.
                let index = format!("{name}_index");
                scope.forget(name);
                scope.forget(&index);
                scope.locals.insert(name.clone());
                scope.locals.insert(index.clone());
                scope.nouns.insert(name.clone());
                scope.nouns.insert(index);
            }
            let source = parse_block(cur, scope, &["do."])?;
            cur.expect("do.")?;
            let body = parse_block(cur, scope, &["end."])?;
            let end = cur.expect("end.")?;
            let source = one_expr(source, Span::merge(start, end))?;
            Control::For { names: suffix.iter().cloned().collect(), source: Box::new(source), body }
        }
        "select." => parse_select(cur, scope, start)?,
        // `try.` takes its two rescue blocks in either order: `catch.`
        // answers for the languages' errors, `catcht.` for a `throw.`.
        "try." => {
            let body = parse_block(cur, scope, &["catch.", "catcht.", "end."])?;
            let mut catch = Vec::new();
            let mut catcht = Vec::new();
            while let Some(w @ ("catch." | "catcht.")) = cur.peek_word() {
                cur.at += 1;
                let block = parse_block(cur, scope, &["catch.", "catcht.", "end."])?;
                if w == "catch." {
                    catch = block;
                } else {
                    catcht = block;
                }
            }
            cur.expect("end.")?;
            Control::Try { body, catch, catcht }
        }
        "return." => Control::Return,
        "break." => Control::Break,
        "continue." => Control::Continue,
        "throw." => Control::Throw,
        "catcht." => {
            return Err(Error::parse("`catcht.` has no matching opening word", start))
        }
        "goto." => {
            let Some(name) = suffix else {
                return Err(Error::parse("a branch is spelled `goto_name.`", start));
            };
            // The statement is settled once the whole body is known; the
            // name is all there is to go on here.
            Control::Goto { name: name.clone(), to: usize::MAX }
        }
        "label." => {
            let Some(name) = suffix else {
                return Err(Error::parse("a branch target is spelled `label_name.`", start));
            };
            Control::Label(name.clone())
        }
        other => {
            return Err(Error::parse(
                format!("`{other}` has no matching opening word"),
                start,
            ))
        }
    };
    let span = Span::merge(start, cur.last_span());
    Ok(Expr::Control(Box::new(control), span))
}

fn parse_if(cur: &mut Cursor<'_>, scope: &mut Names) -> Result<Control> {
    let mut arms = Vec::new();
    let mut otherwise = None;
    loop {
        let test = parse_block(cur, scope, &["do."])?;
        cur.expect("do.")?;
        let body = parse_block(cur, scope, &["elseif.", "else.", "end."])?;
        arms.push(Branch { test: Some(test), body, fall_through: false, list: false });
        match cur.peek_word() {
            Some("elseif.") => {
                cur.at += 1;
            }
            Some("else.") => {
                cur.at += 1;
                otherwise = Some(parse_block(cur, scope, &["end."])?);
                cur.expect("end.")?;
                break;
            }
            _ => {
                cur.expect("end.")?;
                break;
            }
        }
    }
    // `elseif. do.` with no test is the reference's other spelling of
    // `else.`: a final arm that always runs.
    if let Some(last) = arms.last_mut() && last.test.as_ref().is_some_and(Vec::is_empty) {
        last.test = None;
    }
    Ok(Control::If { arms, otherwise })
}

fn parse_select(cur: &mut Cursor<'_>, scope: &mut Names, start: Span) -> Result<Control> {
    let subject = parse_block(cur, scope, &["case.", "fcase.", "end."])?;
    let subject = one_expr(subject, start)?;
    let mut cases = Vec::new();
    loop {
        let fall_through = match cur.peek_word() {
            Some("case.") => false,
            Some("fcase.") => true,
            _ => {
                cur.expect("end.")?;
                break;
            }
        };
        cur.at += 1;
        let test = parse_block(cur, scope, &["do."])?;
        cur.expect("do.")?;
        let body = parse_block(cur, scope, &["case.", "fcase.", "end."])?;
        // `case. do.` with no test is the default arm.
        let test = (!test.is_empty()).then_some(test);
        cases.push(Branch { test, body, fall_through, list: false });
    }
    Ok(Control::Select { subject: Box::new(subject), cases })
}

/// A block that has to be one sentence — a `for.` source, a `select.`
/// subject. The value is the last sentence's, so the rest run for effect.
fn one_expr(mut stmts: Vec<Expr>, span: Span) -> Result<Expr> {
    match stmts.pop() {
        Some(e) if stmts.is_empty() => Ok(e),
        Some(_) => Err(Error::not_yet("several sentences where one value is needed", span)),
        None => Err(Error::parse("this control word needs a value", span)),
    }
}

/// Replace every name known to be a verb or a modifier by what it stands
/// for, except where the name is the target of an assignment, which is a
/// definition of the name rather than a use of it.
fn substitute_names(sentence: &mut [Frag], scope: &Names) {
    for i in 0..sentence.len() {
        let Frag::Name(name, span) = &sentence[i] else { continue };
        let (name, span) = (name.clone(), *span);
        if sentence.get(i + 1).is_some_and(Frag::is_assign) {
            continue;
        }
        // A GLOBAL name read in a locale other than `base` is written out
        // as the locative it stands for. The locale a name belongs to is
        // settled where the sentence is READ, and a value that outlives the
        // sentence — a tacit verb built from the name, above all — must
        // carry the locale with it rather than take the caller's.
        if !scope.locals.contains(&name)
            && scope.current != crate::verb::BASE_LOCALE
            && crate::verb::split_locative(&name).is_none()
            && crate::verb::split_indirect(&name).is_none()
            && !name.starts_with('⎕')
        {
            sentence[i] = Frag::Name(scope.key(&name), span);
        }
        if let Some(v) = scope.verb_named(&name) {
            sentence[i] = Frag::Verb(VerbFrag::V(v.clone()), span);
        } else if let Some(v) = scope.locative_verb(&name, span) {
            sentence[i] = Frag::Verb(VerbFrag::V(v), span);
        } else if let Some((conj, m)) = scope.mod_named(&name) {
            sentence[i] = if *conj {
                Frag::Conj(m.clone(), span)
            } else {
                Frag::Adverb(m.clone(), span)
            };
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
        Expr::AssignMany { names, value, .. } => {
            out.extend(names.iter().cloned());
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
    Adverb(Modifier, Span),
    Conj(Modifier, Span),
    LParen(Span),
    RParen(Span),
    AssignLocal(Span),
    AssignGlobal(Span),
    /// A finished verb definition: `mean =. +/ % #`. It belongs to no part
    /// of speech, so no rule reaches it and it can only end a sentence.
    VerbDef(String, Verb, Span),
    /// A finished modifier definition: `m =. /`. Like `VerbDef`, it belongs
    /// to no part of speech and can only end a sentence. The flag says
    /// whether it is a conjunction.
    ModDef(String, bool, Modifier, Span),
    /// A control word, with the name `for_i.` binds when it has one. Only a
    /// definition's body may hold one.
    Control(&'static str, Option<String>, Span),
    /// `{{` and `}}`, the direct definition's brackets. The opening one
    /// carries the letter of a `{{)a` marker where the source wrote one.
    DdOpen(Option<char>, Span),
    DdClose(Span),
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
            | Frag::DdClose(s)
            | Frag::VerbDef(_, _, s)
            | Frag::ModDef(_, _, _, s) => *s,
            Frag::DdOpen(_, s) => *s,
            Frag::Control(_, _, s) => *s,
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
        "=" => prim("=", M::SelfClassify, D::Scalar(SD::Eq), [INF, 0, 0]),
        "<" => prim("<", M::Enclose(Enclose::Always), D::Scalar(SD::Lt), [INF, 0, 0]),
        ">" => prim(">", M::Open, D::Scalar(SD::Gt), [0, 0, 0]),
        "<:" => prim("<:", M::Scalar(SM::Dec), D::Scalar(SD::Le), [0, 0, 0]),
        ">:" => prim(">:", M::Scalar(SM::Inc), D::Scalar(SD::Ge), [0, 0, 0]),
        "+:" => prim("+:", M::Scalar(SM::Double), D::Boolean(BoolDyad::Nor), [0, 0, 0]),
        "*:" => prim("*:", M::Scalar(SM::Square), D::Boolean(BoolDyad::Nand), [0, 0, 0]),
        "-:" => prim("-:", M::Scalar(SM::Halve), D::Match, [0, INF, INF]),
        "-." => prim("-.", M::Scalar(SM::OneMinus), D::Less, [0, INF, INF]),
        "*." => prim("*.", M::ComplexParts { polar: true }, D::Scalar(SD::Lcm), [0, 0, 0]),
        "+." => prim("+.", M::ComplexParts { polar: false }, D::Scalar(SD::Gcd), [0, 0, 0]),
        "~:" => prim("~:", M::NubSieve, D::Scalar(SD::Ne), [INF, 0, 0]),
        "~." => prim("~.", M::Nub, D::None, [INF, INF, INF]),
        "$" => prim("$", M::ShapeOf, D::Reshape, [INF, 1, INF]),
        "," => prim(",", M::Ravel, D::AppendLeading, [INF, INF, INF]),
        // `,.` is J's `,"_1` dyadically. Its monad is not `,"_1`: that
        // would leave `,. 5` a one-item list, where J answers a 1-by-1
        // table, so the monad ravels the items itself at infinite rank.
        ",." => prim(",.", M::RavelItems, D::AppendLeading, [INF, -1, -1]),
        ",:" => prim(",:", M::Itemize, D::Laminate, [INF, INF, INF]),
        "#" => prim("#", M::Tally, D::Copy, [INF, 1, INF]),
        "#." => prim("#.", M::DecodeBits, D::Decode, [1, 1, 1]),
        // The width of `#: y` comes from the largest value in the whole
        // argument, which is why the monad has infinite rank.
        "#:" => prim("#:", M::EncodeBits, D::Encode, [INF, 1, 0]),
        "!" => prim("!", M::Scalar(SM::Factorial), D::Scalar(SD::Binomial), [0, 0, 0]),
        "\":" => {
            prim("\":", M::Format, D::FormatSpecJ, [INF, 1, INF])
        }
        "o." => prim("o.", M::Scalar(SM::Pi), D::Scalar(SD::Circle), [0, 0, 0]),
        "j." => prim("j.", M::Scalar(SM::Imaginary), D::Scalar(SD::MakeComplex), [0, 0, 0]),
        "r." => prim("r.", M::Scalar(SM::Polar), D::Scalar(SD::PolarBy), [0, 0, 0]),
        "{" => prim("{", M::Catalogue, D::From, [INF, 0, INF]),
        "{." => prim("{.", M::Head, D::Take, [INF, 1, INF]),
        "}." => prim("}.", M::Behead, D::Drop, [INF, 1, INF]),
        "{:" => prim("{:", M::Tail, D::None, [INF, INF, INF]),
        "}:" => prim("}:", M::Curtail, D::None, [INF, INF, INF]),
        "|." => prim("|.", M::Reverse, D::Rotate, [INF, 1, INF]),
        "|:" => prim("|:", M::TransposeAxes, D::TransposeJ, [INF, 1, INF]),
        "i." => prim("i.", M::IotaJ, D::IndexOf { origin: 0, major_cells: false }, [1, INF, INF]),
        "i:" => prim("i:", M::Steps, D::IndexOfLast { origin: 0 }, [0, INF, INF]),
        // The dyad reads its whole left argument, whose major cells are the
        // bounds: `(2 2$1 2 3 4) I. 2 3` is 1 rather than the `1 2 / 0 0`
        // a left rank of 1 would frame, and `I. b. 0` is `1 _ _`. The
        // framed reading is still reachable as `I."1 _`.
        "I." => prim(
            "I.",
            M::Indices { origin: 0, boxed_coords: false },
            D::IntervalIndex { offset: 0, closed: false },
            [1, INF, INF],
        ),
        // The dyad reads the whole argument: `2 x: y` gives every value a
        // numerator and a denominator, which becomes a trailing axis.
        "x:" => prim("x:", M::ToExact, D::ExactForm, [INF, INF, INF]),
        // The right dyadic rank is infinite: form 3 reads the whole
        // argument, as `q:` does, and the forms that answer about one
        // number frame their own answers.
        "p:" => prim("p:", M::NthPrime, D::PrimeMeta, [0, RANK_INF, RANK_INF]),
        // The coefficients are one vector and the point one atom, so the
        // rank machinery evaluates a whole array of points at once.
        "p." => prim("p.", M::PolyRoots, D::PolyEval, [1, 1, 0]),
        "p.." => prim("p..", M::PolyDeriv, D::PolyIntegral, [1, 0, 1]),
        "$." => prim("$.", M::Sparse, D::SparseForm, [INF, INF, INF]),
        // `q:` reads its whole argument: the rows of factors are padded to
        // the longest, which needs every item's factors at once.
        "q:" => prim("q:", M::PrimeFactors, D::PrimeExponents, [RANK_INF, 0, 0]),
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
        "{::" => prim("{::", M::MapPaths, D::Fetch, [INF, INF, INF]),
        "e." => prim("e.", M::RazeIn, D::MemberJ, [INF, INF, INF]),
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
            M::Words,
            D::SequentialMachine,
            [1, INF, INF],
        ),
        "L." => prim("L.", M::LevelOf, D::None, [INF, INF, INF]),
        "\"." => prim(
            "\".",
            M::Execute { apl: false },
            D::ParseNumbers,
            [1, INF, 1],
        ),
        "A." => prim("A.", M::AnagramIndex, D::AnagramFrom, [1, 0, INF]),
        "C." => prim("C.", M::CycleForm, D::Permute, [1, 1, INF]),
        "E." => prim("E.", M::None, D::FindSeq, [INF, INF, INF]),
        "u:" => prim("u:", M::Unicode { pass_chars: true }, D::UnicodeForm, [INF, INF, INF]),
        // The monad reads the whole argument: one delimiter governs the
        // whole list. The dyad takes the form as an atom.
        "s:" => prim("s:", M::Symbols, D::SymbolForm, [INF, INF, INF]),
        "]" => prim("]", M::Same, D::Right, [INF, INF, INF]),
        "[" => prim("[", M::Same, D::Left, [INF, INF, INF]),
        "echo" => prim("echo", M::Echo, D::None, [INF, INF, INF]),
        _ => return None,
    })
}

/// The constant nouns J spells as inflected words. `a.` is the 256
/// characters of J's alphabet in codepoint order; `a:` is the ace, the box
/// holding an empty numeric list; `_.` is the indeterminate value, which is
/// a NaN and prints as itself.
fn noun_word(word: &str) -> Option<Array> {
    match word {
        "a." => Some(Array::from_chars(
            (0u32..256).map(|c| char::from_u32(c).expect("a Latin-1 codepoint")).collect(),
        )),
        "a:" => Some(Array::boxed(Array::empty(crate::dtype::DType::Bool))),
        "_." => Some(Array::scalar_f64(f64::NAN)),
        _ => None,
    }
}

/// The verb a word denotes: a bare primitive, whose own ranks carry the
/// rank a word like `,.` is defined at.
fn verb_for(word: &str) -> Option<Verb> {
    Some(Verb::Prim(primitive(word)?))
}

/// A constant verb: the noun itself, whatever the arguments are. `3:` and
/// the noun operand of `::` both need one.
fn constant_verb(n: Array) -> Verb {
    // `n [ (x ] y)` is n whatever the arguments are, and the noun fork has
    // both valences, which a bond does not.
    Verb::NounFork(
        n,
        Box::new(verb_for("[").expect("`[` is a primitive")),
        Box::new(verb_for("]").expect("`]` is a primitive")),
    )
}

/// The spelling of a constant verb: `_9:` … `9:`, `_:` for infinity and
/// `__:` for the infinity below. The word must be complete — `3::` is the
/// adverse conjunction after a number, not a constant verb.
fn constant_verb_word(cs: &[(usize, char)], i: usize) -> Option<(usize, Array)> {
    let at = |k: usize| cs.get(k).map(|&(_, c)| c);
    let (digits, value) = match (at(i), at(i + 1), at(i + 2)) {
        (Some('_'), Some('_'), Some(':')) => (3, f64::NEG_INFINITY),
        (Some('_'), Some(':'), _) => (2, f64::INFINITY),
        (Some('_'), Some(d), Some(':')) if d.is_ascii_digit() => {
            (3, -((d as u8 - b'0') as f64))
        }
        (Some(d), Some(':'), _) if d.is_ascii_digit() => (2, (d as u8 - b'0') as f64),
        _ => return None,
    };
    if at(i + digits) == Some(':') {
        return None;
    }
    let arr = if value.is_infinite() {
        Array::scalar_f64(value)
    } else {
        Array::scalar_i64(value as i64)
    };
    Some((digits, arr))
}

/// The verb one J spelling denotes, for the parts of the evaluator that
/// need to name a verb rather than parse one — the obverse table above all.
pub(crate) fn verb_named(word: &str) -> Option<Verb> {
    verb_for(word)
}

const ADVERBS: [&str; 9] = ["/", "\\", "/.", "\\.", "~", "}", "f.", "M.", "b."];

/// Conjunction spellings. The ones without a meaning here are recognised so
/// that their diagnostic names the conjunction rather than the word.
const CONJUNCTIONS: [&str; 28] = [
    "\"", "@", "@.", "@:", "&", "&.", "&.:", "&:", "^:", ";.", "!.", "!:", "`", "`:", ".", ":",
    ":.", "::", "L:", "S:", "H.", "T.", "t.", "t:",
    // The fold family. `F.` and `F:`, the two forms whose count is not
    // settled by the argument, are deliberately absent: each folds until a
    // test says stop, which is unbounded, and the reference itself runs
    // without end on the ordinary cases.
    "F..", "F.:", "F:.", "F::",
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
fn lex(src: &SourceParts, char_bytes: bool) -> Result<Vec<Vec<Frag>>> {
    let mut sentences: Vec<Vec<Frag>> = Vec::new();
    let mut cur: Vec<Frag> = Vec::new();
    for seg in &src.segments {
        match seg {
            Segment::Text { text, offset } => {
                let lines: Vec<&str> = text.split('\n').collect();
                let mut pos = 0usize;
                let mut n = 0usize;
                while n < lines.len() {
                    let line = lines[n];
                    if n > 0 && !cur.is_empty() {
                        sentences.push(std::mem::take(&mut cur));
                    }
                    lex_line(line, offset + pos, char_bytes, &mut cur)?;
                    pos += line.len() + 1;
                    n += 1;
                    // `0 : 0` is J's here-document: the lines below it, up
                    // to a lone `)`, are its VALUE and not its source, so
                    // they are taken whole before anything lexes them.
                    if noun_definition_ends(&cur) {
                        let open = pos;
                        let mut body = String::new();
                        let mut closed = false;
                        while n < lines.len() {
                            let l = lines[n];
                            pos += l.len() + 1;
                            n += 1;
                            if l.trim() == ")" {
                                closed = true;
                                break;
                            }
                            body.push_str(l);
                            body.push('\n');
                        }
                        if !closed {
                            return Err(Error::parse(
                                "this noun definition's body has no closing `)`",
                                Span::new(offset + open, offset + pos),
                            ));
                        }
                        let span = Span::merge(
                            cur[cur.len() - 3].span(),
                            Span::new(offset + open, offset + pos),
                        );
                        cur.truncate(cur.len() - 3);
                        cur.push(Frag::Noun(Expr::Const(
                            literal_text(&body, char_bytes),
                            span,
                        )));
                    }
                    // `{{)n` is the same here-document written as a direct
                    // definition: the lines below it are its value, and the
                    // `}}` that ends it starts a line. Whatever follows the
                    // `}}` on that line belongs to the sentence again.
                    let noun_dd = match cur.last() {
                        Some(Frag::DdOpen(Some('n'), s)) => Some(*s),
                        _ => None,
                    };
                    if let Some(open_span) = noun_dd {
                        let open = pos;
                        let mut body = String::new();
                        let mut rest: Option<(&str, usize)> = None;
                        while n < lines.len() {
                            let l = lines[n];
                            let at = pos;
                            pos += l.len() + 1;
                            n += 1;
                            if let Some(tail) = l.strip_prefix("}}") {
                                rest = Some((tail, offset + at + 2));
                                break;
                            }
                            body.push_str(l);
                            body.push('\n');
                        }
                        let Some((tail, tail_at)) = rest else {
                            return Err(Error::parse(
                                "this noun definition has no closing `}}`",
                                Span::new(offset + open, offset + pos),
                            ));
                        };
                        let span = Span::merge(
                            open_span,
                            Span::new(offset + open, offset + pos),
                        );
                        cur.pop();
                        cur.push(Frag::Noun(Expr::Const(
                            literal_text(&body, char_bytes),
                            span,
                        )));
                        lex_line(tail, tail_at, char_bytes, &mut cur)?;
                    }
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

/// Whether these fragments end with `0 : 0`, the noun definition. Only the
/// pair of literal zeros is one: `3 : 0` reads J source below it, and
/// `0 : 'text'` carries its own body.
fn noun_definition_ends(cur: &[Frag]) -> bool {
    let zero = |f: &Frag| {
        as_const(f).is_some_and(|a| a.rank() == 0 && a.to_f64_vec().as_deref() == Some(&[0.0]))
    };
    match cur {
        [.., m, c, n] => {
            zero(m) && matches!(c, Frag::Conj(Modifier::Prim(":"), _)) && zero(n)
        }
        _ => false,
    }
}

/// Source text as J's literal type holds it: one item per character, or per
/// BYTE where a quoted literal is a byte vector. Always a vector, however
/// short — a noun definition's body is a list of lines, not one character.
fn literal_text(text: &str, char_bytes: bool) -> Array {
    let mut chars: Vec<char> = text.chars().collect();
    if char_bytes {
        chars = literal_bytes(&chars);
    }
    Array::new(vec![chars.len()], Data::Char(chars.into()))
}

/// A numeric word's value. Kept apart from `Array` so that a list of words
/// can pick one element type for the whole vector.
#[derive(Clone, Debug)]
enum Num {
    I(i64),
    F(f64),
    /// An extended-precision integer: `123x`.
    X(crate::exact::Ext),
    /// A rational: `1r3`.
    R(crate::exact::Rat),
    C(crate::complex::Cx),
}

/// The UTF-8 bytes of the characters a literal was written with, one
/// `char` per byte. The inverse is the byte-decoding [`crate::fmt`] does
/// when it writes a literal out.
fn literal_bytes(chars: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(chars.len());
    let mut buf = [0u8; 4];
    for c in chars {
        out.extend(c.encode_utf8(&mut buf).bytes().map(char::from));
    }
    out
}

fn lex_line(text: &str, base: usize, char_bytes: bool, out: &mut Vec<Frag>) -> Result<()> {
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
            // J's literal type holds one BYTE per item, so a literal
            // outside ASCII is as long as its UTF-8 spelling: `# 'é'` is 2.
            // Each byte becomes a character of that value, which is what
            // `a.`, `3 u:` and indexing then answer with.
            if char_bytes {
                chars = literal_bytes(&chars);
            }
            // One character is an atom; anything else is a vector.
            let shape = if chars.len() == 1 { vec![] } else { vec![chars.len()] };
            let arr = Array::new(shape, Data::Char(chars.into()));
            out.push(Frag::Noun(Expr::Const(arr, span(start, i))));
            continue;
        }
        if let Some((len, n)) = constant_verb_word(&cs, i) {
            out.push(Frag::Verb(VerbFrag::V(constant_verb(n)), span(i, i + len)));
            i += len;
            continue;
        }
        if starts_number(&cs, i) {
            // Numeric words separated only by blanks form one vector.
            let start = i;
            let mut nums: Vec<Num> = Vec::new();
            let mut exact_suffix = false;
            let mut inexact_spelling = false;
            let mut end;
            loop {
                let ws = i;
                while at(i).is_some_and(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_') {
                    i += 1;
                }
                let word = &text[off(ws)..off(i)];
                // A trailing `x` is the extended suffix only where it is
                // not a DIGIT: every letter after a base's `b` is one, so
                // `36bx` is 33 and carries no suffix at all.
                exact_suffix |=
                    word.ends_with('x') && word.len() > 1 && !word.contains('b');
                inexact_spelling |= spelled_inexact(word);
                nums.push(parse_number(word, span(ws, i))?);
                end = i;
                let mut k = i;
                while at(k).is_some_and(char::is_whitespace) {
                    k += 1;
                }
                // A constant verb (`3:`) ends the numeric word rather than
                // joining it: `2 3: 4` is 2, the verb `3:`, and 4.
                if k < cs.len()
                    && starts_number(&cs, k)
                    && constant_verb_word(&cs, k).is_none()
                {
                    i = k;
                } else {
                    break;
                }
            }
            // One numeric word is read at ONE precision, so an `x` suffix
            // anywhere in it forbids an atom spelled as a float — a decimal
            // point, an exponent, a complex or a polar part. `1x 1.5` and
            // `1e20 _1x` are ill-formed in the reference, while `1r2 1x`
            // and `_ 1x` — both exactly spelled — are not.
            if exact_suffix && inexact_spelling {
                return Err(Error::parse(
                    "ill-formed number: an `x` suffix and a float spelling in one numeric word",
                    span(start, end),
                ));
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
            // An alphabetic word may be inflected into a primitive (`i.`,
            // `p..`), a modifier (`f.`, `L:`) or a control word (`if.`,
            // `for_i.`). The longer inflection wins where it names
            // something: `p..` is one word, not `p.` and the dot.
            let mut inflected = None;
            if matches!(at(i), Some('.') | Some(':')) {
                let most = if matches!(at(i + 1), Some('.') | Some(':')) { 2 } else { 1 };
                for n in (1..=most).rev() {
                    let word = &text[off(start)..off(i + n)];
                    let sp = span(start, i + n);
                    let frag = if let Some(v) = verb_for(word) {
                        Frag::Verb(VerbFrag::V(v), sp)
                    } else if let Some(value) = noun_word(word) {
                        Frag::Noun(Expr::Const(value, sp))
                    } else if let Some(g) = adverb(word) {
                        Frag::Adverb(Modifier::Prim(g), sp)
                    } else if let Some(g) = conjunction(word) {
                        Frag::Conj(Modifier::Prim(g), sp)
                    } else if let Some((cw, suffix)) = control_word(word) {
                        Frag::Control(cw, suffix, sp)
                    } else {
                        continue;
                    };
                    inflected = Some((frag, n));
                    break;
                }
            }
            if let Some((frag, n)) = inflected {
                i += n;
                out.push(frag);
                continue;
            }
            let word = &text[off(start)..off(i)];
            match verb_for(word) {
                Some(v) => out.push(Frag::Verb(VerbFrag::V(v), span(start, i))),
                None => {
                    if !is_well_formed_name(word) {
                        return Err(Error::parse(
                            format!(
                                "ill-formed name: {word} — a name ends in an underscore \
                                 only as the locative `name_locale_`"
                            ),
                            span(start, i),
                        ));
                    }
                    out.push(Frag::Name(word.to_string(), span(start, i)))
                }
            }
            continue;
        }
        // `{{` and `}}` bracket J's direct definition; neither is two words.
        if c == '{' && at(i + 1) == Some('{') {
            // `{{)a` and its relatives state the definition's part of
            // speech instead of leaving it to the words of the body. The
            // reference takes the marker only where nothing follows it on
            // the line, and reads `{{)a u y }}` as a domain error.
            let marker = match (at(i + 2), at(i + 3)) {
                (Some(')'), Some(m)) if m.is_ascii_alphabetic() => Some(m),
                _ => None,
            };
            if let Some(m) = marker {
                if cs[i + 4..].iter().any(|&(_, c)| !c.is_whitespace()) {
                    return Err(Error::parse(
                        format!("`)`{m} names the part of speech of a direct definition, \
                                 and has to be the last thing on its line"),
                        span(i, i + 4),
                    ));
                }
                out.push(Frag::DdOpen(Some(m), span(i, i + 4)));
                i += 4;
                continue;
            }
            out.push(Frag::DdOpen(None, span(i, i + 2)));
            i += 2;
            continue;
        }
        if c == '}' && at(i + 1) == Some('}') {
            out.push(Frag::DdClose(span(i, i + 2)));
            i += 2;
            continue;
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
        // `$:` stands for the explicit definition it is written in.
        "$:" => Frag::Verb(VerbFrag::V(Verb::SelfRef), span),
        // An inflected verb wins over the adverb its stem spells: `~.` is
        // the nub, never `~` followed by an inflection.
        _ => {
            if let Some(v) = verb_for(word) {
                Frag::Verb(VerbFrag::V(v), span)
            } else if let Some(g) = adverb(word) {
                Frag::Adverb(Modifier::Prim(g), span)
            } else {
                Frag::Conj(Modifier::Prim(conjunction(word)?), span)
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
        // `j` is the one letter an infinity can be followed by: `_j_` and
        // `_j1` are the rectangular form with an infinite real part, and a
        // J name never begins with `_`, so nothing else claims the word.
        Some(d) => d.is_ascii_digit() || d == '.' || d == 'j' || !d.is_alphanumeric(),
    }
}

/// Whether a numeric word is spelled OTHER THAN as a plain whole number or
/// a rational: a decimal point, an exponent, a base, a complex or a polar
/// part. The spelling is what counts, not the value — `1e2` is a hundred
/// and counts all the same, and so does `2b11`. The two INFINITIES are
/// spellings of their own and count as neither.
fn spelled_inexact(word: &str) -> bool {
    if matches!(word, "_" | "__") {
        return false;
    }
    word.contains('.')
        || word.contains('e')
        || word.contains('j')
        || word.contains('p')
        || word.contains('b')
        || word.contains("ad")
        || word.contains("ar")
}

fn parse_number(word: &str, span: Span) -> Result<Num> {
    // `_.` is the indeterminate value, not a number with a decimal point.
    if word == "_." {
        return Ok(Num::F(f64::NAN));
    }
    // `mbd…` is the base form, and `b` binds looser than anything: the base
    // is itself a number, so `3r4b11` reads in base three quarters, and
    // every letter after the `b` is a DIGIT, so `36bj` is 19 and `2b11p1`
    // is 63. Splitting here, at the first `b`, settles both sides.
    if let Some(k) = word.find('b') {
        return base_literal(&word[..k], &word[k + 1..], word, span);
    }
    // `1x` is an extended-precision integer; `1x1` is a multiple of e, and
    // `1p1` a multiple of π. The letter is the separator in both, and it
    // binds loosest of what is left: `1ar1p1` is the polar value `1ar1`
    // scaled by π.
    if let Some(k) = word.find(['p', 'x']) {
        if word[k + 1..].is_empty() {
            // A trailing `x` is the extended-precision suffix, and only a
            // whole decimal number carries it: `1.5x` and `1e10x` are
            // ill-formed, as they are in the reference.
            if word.as_bytes()[k] == b'x' {
                return extended_literal(&word[..k], word, span);
            }
            return Err(Error::parse(format!("invalid number: {word}"), span));
        }
        let base =
            if word.as_bytes()[k] == b'p' { std::f64::consts::PI } else { std::f64::consts::E };
        let mantissa = plain_number(&word[..k], word, span)?;
        let exponent = plain_number(&word[k + 1..], word, span)?;
        return Ok(scale(mantissa, base, exponent));
    }
    // `3j4` is the rectangular form.
    if let Some(k) = word.find('j') {
        let re = as_f64(plain_number(&word[..k], word, span)?);
        let im = as_f64(plain_number(&word[k + 1..], word, span)?);
        return Ok(Num::C([re, im]));
    }
    // `1ad45` and `1ar1` are the polar forms: a magnitude, then the angle
    // in degrees or in radians.
    if let Some(k) = word.find("ad").or_else(|| word.find("ar")) {
        let magnitude = as_f64(plain_number(&word[..k], word, span)?);
        let angle = as_f64(plain_number(&word[k + 2..], word, span)?);
        return Ok(Num::C(if word.as_bytes()[k + 1] == b'd' {
            crate::complex::from_degrees(magnitude, angle)
        } else {
            crate::complex::from_radians(magnitude, angle)
        }));
    }
    // `3r4` is a rational, and `1r_2` spells its negative denominator with
    // J's own negative sign.
    if let Some(k) = word.find('r') {
        return rational_literal(&word[..k], &word[k + 1..], word, span);
    }
    plain_number(word, word, span)
}

/// `123x`: the digits as an extended-precision integer. The value is exact
/// however many digits it has, which is the whole point of the suffix.
fn extended_literal(digits: &str, word: &str, span: Span) -> Result<Num> {
    Ok(Num::X(whole_digits(digits, word, span)?))
}

/// `3r4`: a rational. A zero denominator is J's infinity rather than a
/// number — the only spelling that leaves the exact types on sight.
fn rational_literal(num: &str, den: &str, word: &str, span: Span) -> Result<Num> {
    use num_traits::Zero;
    let num = whole_digits(num, word, span)?;
    let den = whole_digits(den, word, span)?;
    if den.is_zero() {
        if num.is_zero() {
            return Ok(Num::I(0));
        }
        return Ok(Num::F(if num.sign() == num_bigint::Sign::Minus {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }));
    }
    Ok(Num::R(
        crate::exact::Rat::new(num, den).ok_or_else(|| Error::internal("a zero denominator"))?,
    ))
}

/// One run of decimal digits, with J's `_` as the negative sign.
fn whole_digits(word: &str, whole: &str, span: Span) -> Result<crate::exact::Ext> {
    let invalid = || Error::parse(format!("invalid number: {whole}"), span);
    let (digits, negative) = match word.strip_prefix('_') {
        Some(rest) => (rest, true),
        None => (word, false),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    let v: crate::exact::Ext = digits.parse().map_err(|_| invalid())?;
    Ok(if negative { -v } else { v })
}

/// A mantissa scaled by a power of π or e. Either half may be complex —
/// `1p1j1` is π to the power `1j1`.
fn scale(mantissa: Num, base: f64, exponent: Num) -> Num {
    if matches!(mantissa, Num::C(_)) || matches!(exponent, Num::C(_)) {
        let m = as_cx(mantissa);
        let f = crate::complex::pow([base, 0.0], as_cx(exponent));
        return Num::C(crate::complex::mul(m, f));
    }
    Num::F(as_f64(mantissa) * base.powf(as_f64(exponent)))
}

fn as_cx(n: Num) -> crate::complex::Cx {
    match n {
        Num::C(z) => z,
        other => [as_f64(other), 0.0],
    }
}

fn as_f64(n: Num) -> f64 {
    match n {
        Num::I(v) => v as f64,
        Num::F(v) => v,
        Num::X(v) => crate::exact::ext_to_f64(&v),
        Num::R(v) => v.to_f64(),
        // A complex part is itself written as a plain number, so this is
        // never reached from a well-formed literal.
        Num::C(z) => z[0],
    }
}

/// `mbd…`: the digits `d…` read in base `m`. The base is a number in the
/// same grammar — `3r4b11` counts in three quarters and `3j4b11` in a
/// complex base — while every letter after the `b` is a digit, running
/// `0`–`9` then `a`–`z`. A `.` among them starts the negative powers, and a
/// `_` in front of them negates the value, as the reference does.
fn base_literal(base: &str, digits: &str, word: &str, span: Span) -> Result<Num> {
    let invalid = || Error::parse(format!("invalid number: {word}"), span);
    let base = plain_number(base, word, span)?;
    let (digits, negative) = match digits.strip_prefix('_') {
        Some(rest) => (rest, true),
        None => (digits, false),
    };
    if digits.is_empty() {
        return Err(invalid());
    }
    let (whole, fraction) = match digits.split_once('.') {
        Some((w, f)) => (w, f),
        None => (digits, ""),
    };
    let digit = |ch: char| match ch {
        '0'..='9' => Ok(f64::from(ch as u32 - '0' as u32)),
        'a'..='z' => Ok(f64::from(ch as u32 - 'a' as u32 + 10)),
        _ => Err(invalid()),
    };
    // A complex base multiplies as a complex number; every other base is a
    // real one, rationals included — the reference answers `3r4b11` with a
    // float and not with a rational.
    if let Num::C(b) = base {
        let mut value = [0.0, 0.0];
        for ch in whole.chars() {
            value = crate::complex::add(crate::complex::mul(value, b), [digit(ch)?, 0.0]);
        }
        let mut place = crate::complex::recip(b);
        for ch in fraction.chars() {
            value = crate::complex::add(value, crate::complex::mul(place, [digit(ch)?, 0.0]));
            place = crate::complex::mul(place, crate::complex::recip(b));
        }
        return Ok(Num::C(if negative { crate::complex::neg(value) } else { value }));
    }
    let base = as_f64(base);
    let mut value = 0.0f64;
    for ch in whole.chars() {
        value = value * base + digit(ch)?;
    }
    let mut place = 1.0 / base;
    for ch in fraction.chars() {
        value += place * digit(ch)?;
        place /= base;
    }
    if negative {
        value = -value;
    }
    // An exact whole number stays an integer, as the reference prints it.
    if value.fract() == 0.0 && value.abs() < 9.007_199_254_740_992e15 {
        return Ok(Num::I(value as i64));
    }
    Ok(Num::F(value))
}

/// One constituent of a literal — a whole one, a mantissa, an exponent, or
/// half of a complex or polar form. Every part is itself a number in the
/// same grammar, which is what makes `1ar1p1` and `1p1j1` read.
fn plain_number(word: &str, whole: &str, span: Span) -> Result<Num> {
    if word.is_empty() {
        return Err(Error::parse(format!("invalid number: {whole}"), span));
    }
    if word.contains(['j', 'p', 'x', 'b', 'r']) || word.contains("ad") || word.contains("ar") {
        return parse_number(word, span);
    }
    parse_plain(word, span)
}

fn parse_plain(word: &str, span: Span) -> Result<Num> {
    if word == "_" {
        return Ok(Num::F(f64::INFINITY));
    }
    if word == "__" {
        return Ok(Num::F(f64::NEG_INFINITY));
    }
    // As a component of a complex literal: `_.j_` and `_j_.` are both
    // words the reference reads.
    if word == "_." {
        return Ok(Num::F(f64::NAN));
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
        return norm.parse::<f64>().map(Num::F).map_err(|_| invalid());
    }
    // Digits that overflow a machine word are a float, as they are in J;
    // the `x` suffix is what asks for an exact value instead.
    match norm.parse::<i64>() {
        Ok(v) => Ok(Num::I(v)),
        Err(_) => norm.parse::<f64>().map(Num::F).map_err(|_| invalid()),
    }
}

/// One numeric word list as an array. The widest type any word reached
/// carries the whole vector: `1 2 3x` is extended throughout, and one
/// rational or float among the words pulls its neighbours up with it.
/// J `x ". y`: the numbers one line of text spells.
///
/// The line is split at blanks and every word read as a J numeric literal;
/// a word that is not one takes the value `fallback` instead, which is what
/// separates this from `".` the monad — a line that does not parse is
/// answered rather than refused. One word gives a scalar, as reading that
/// line as a noun would; several give a vector of that many.
///
/// None where the fallback is not a number: there is nothing to stand in
/// with.
pub(crate) fn numbers_from_text(line: &str, fallback: &Array) -> Option<Array> {
    let stand_in = match &fallback.data {
        Data::Bool(v) => Num::I(i64::from(*v.as_slice().first()?)),
        Data::I64(v) => Num::I(*v.as_slice().first()?),
        Data::F64(v) => Num::F(*v.as_slice().first()?),
        Data::Ext(v) => Num::X(v.as_slice().first()?.clone()),
        Data::Rat(v) => Num::R(v.as_slice().first()?.clone()),
        Data::Complex(v) => Num::C(*v.as_slice().first()?),
        Data::Char(_) | Data::Symbol(_) | Data::Box(_) => return None,
    };
    let nums: Vec<Num> = line
        .split_whitespace()
        .map(|w| parse_number(w, Span::new(0, 0)).unwrap_or_else(|_| stand_in.clone()))
        .collect();
    Some(num_array(&nums))
}

fn num_array(nums: &[Num]) -> Array {
    use crate::exact::{Ext, Rat};
    let shape = if nums.len() == 1 { vec![] } else { vec![nums.len()] };
    let has = |f: fn(&Num) -> bool| nums.iter().any(f);
    if has(|n| matches!(n, Num::C(_))) {
        let data = nums.iter().map(|n| as_cx(n.clone())).collect();
        return Array::new(shape, Data::Complex(data));
    }
    if has(|n| matches!(n, Num::F(_))) {
        let data = nums.iter().map(|n| as_f64(n.clone())).collect();
        return Array::new(shape, Data::F64(data));
    }
    if has(|n| matches!(n, Num::R(_))) {
        let data = nums
            .iter()
            .map(|n| match n {
                Num::I(v) => Rat::from_int(Ext::from(*v)),
                Num::X(v) => Rat::from_int(v.clone()),
                Num::R(v) => v.clone(),
                Num::F(_) | Num::C(_) => Rat::zero(),
            })
            .collect();
        return Array::new(shape, Data::Rat(data));
    }
    if has(|n| matches!(n, Num::X(_))) {
        let data = nums
            .iter()
            .map(|n| match n {
                Num::I(v) => Ext::from(*v),
                Num::X(v) => v.clone(),
                _ => Ext::default(),
            })
            .collect();
        return Array::new(shape, Data::Ext(data));
    }
    // A literal whose atoms are all 0 or 1 is BOOLEAN, which is what `3!:0`
    // reports of it; the type widens on the first arithmetic that needs it,
    // exactly as it does for a comparison's answer.
    if nums.iter().all(|n| matches!(n, Num::I(0 | 1))) {
        let data = nums.iter().map(|n| u8::from(matches!(n, Num::I(1)))).collect();
        return Array::new(shape, Data::Bool(data));
    }
    let data = nums
        .iter()
        .map(|n| match n {
            Num::I(v) => *v,
            _ => 0,
        })
        .collect();
    Array::new(shape, Data::I64(data))
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

/// Run the parse table over a sentence's words. The result is the one
/// fragment left standing, or None where the sentence did not reduce to
/// one — which is the reference's syntax error.
fn reduce_to_fragment(tokens: Vec<Frag>, scope: &Names) -> Result<Option<Frag>> {
    check_parens(&tokens)?;
    let mut stack: Vec<Frag> = Vec::new();
    for frag in tokens.into_iter().rev() {
        stack.insert(0, frag);
        reduce(&mut stack, scope)?;
    }
    stack.insert(0, Frag::Mark);
    // A verb or a modifier assignment used as a VALUE inside a larger
    // sentence. The reference reads `2 [ f =. #` as the noun fork the
    // assignment's value makes; libjay settles what a name stands for while
    // the sentence is parsed, so an assignment is a sentence of its own
    // here and has nothing to give the train around it. The check is here,
    // before the last reduction, because the fragments to the LEFT of the
    // definition are what that reduction would go on to complain about.
    if stack.len() > 2
        && let Some(at) = stack.iter().position(|f| matches!(f, Frag::VerbDef(..) | Frag::ModDef(..)))
    {
        return Err(Error::not_yet(
            "a verb or modifier assignment used as a value inside a sentence",
            stack[at].span(),
        ));
    }
    reduce(&mut stack, scope)?;
    if stack.len() == 2 {
        return Ok(Some(stack.pop().expect("checked length")));
    }
    Ok(None)
}

/// The IR statement a finished sentence stands for. `whole` is the span of
/// the sentence, for the complaint that it has no reading at all.
fn lower_sentence(frag: Option<Frag>, whole: Span, char_bytes: bool) -> Result<Expr> {
    match frag {
        Some(f @ (Frag::Noun(_) | Frag::Name(..))) => as_noun(f),
        Some(Frag::VerbDef(name, verb, span)) => Ok(Expr::VerbDef { name, verb, span }),
        Some(Frag::ModDef(name, conjunction, m, span)) => {
            let rep = m.rep();
            Ok(Expr::ModDef { name, spelling: m.spelling(), conjunction, rep, span })
        }
        // A sentence that IS a verb or a modifier displays the entity, and
        // what a session shows for it is its linear representation: the
        // text it would be written as. The value is that text.
        Some(Frag::Verb(VerbFrag::V(v), span)) => match verb_display(&v) {
            Some(text) => Ok(Expr::Const(literal_text(&text, char_bytes), span)),
            None => Err(Error::not_yet(
                format!("writing {} back out as J source", v.name()),
                span,
            )),
        },
        Some(Frag::Adverb(m, span) | Frag::Conj(m, span)) => match m.display_text() {
            Some(text) => Ok(Expr::Const(literal_text(&text, char_bytes), span)),
            None => Err(Error::not_yet(
                "writing this modifier back out as J source",
                span,
            )),
        },
        Some(Frag::Verb(VerbFrag::Cap, span)) => {
            Err(Error::parse("`[:` caps a fork; it has no verb of its own", span))
        }
        _ => Err(Error::parse("syntax error", whole)),
    }
}

/// The text a session displays for a verb: the linear representation, or
/// the definition's own spelling where it has one.
fn verb_display(v: &Verb) -> Option<String> {
    if let Verb::Explicit(def) = v {
        return def.spelling.clone();
    }
    crate::gerund::linear(&crate::gerund::verb_ar(v)?)
}

/// Report an unbalanced parenthesis at the parenthesis itself, before the
/// sentence is reduced: the reduction would otherwise blame whatever
/// fragments the stray one left stranded beside each other.
fn check_parens(tokens: &[Frag]) -> Result<()> {
    let mut open: Vec<Span> = Vec::new();
    for frag in tokens {
        match frag {
            Frag::LParen(s) => open.push(*s),
            Frag::RParen(s) => {
                if open.pop().is_none() {
                    return Err(Error::parse("this `)` has no opening `(`", *s));
                }
            }
            _ => {}
        }
    }
    match open.pop() {
        None => Ok(()),
        Some(s) => Err(Error::parse("this `(` has no closing `)`", s)),
    }
}

fn sentence_span(tokens: &[Frag]) -> Span {
    tokens
        .iter()
        .map(Frag::span)
        .reduce(Span::merge)
        .unwrap_or_else(|| Span::new(0, 0))
}

fn reduce(stack: &mut Vec<Frag>, scope: &Names) -> Result<()> {
    while apply(stack, scope)? {}
    Ok(())
}

/// The parse table: the first matching row wins, and matching restarts after
/// every reduction. Slot 0 is the leftmost (most recently pushed) fragment.
fn match_rule(s: &[Frag]) -> Option<Rule> {
    let is = |i: usize, f: fn(&Frag) -> bool| s.get(i).is_some_and(f);
    // Slot 0 is only ever context: an edge, or a fragment that keeps the
    // reduction from reaching further left than it should.
    let ctx = |i: usize| s.get(i).is_some_and(|f| f.is_edge() || f.is_avn());
    let verb_or_noun =
        |i: usize| s.get(i).is_some_and(|f| f.is_real_verb() || f.is_noun());
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

/// The fragment, pointing at `to` instead of at its own words. Removing a
/// pair of parentheses uses it so that the fragment left behind still
/// covers the brackets it was written in.
fn respan(f: Frag, to: Span) -> Frag {
    match f {
        Frag::Noun(mut e) => {
            e.set_span(to);
            Frag::Noun(e)
        }
        Frag::Name(n, _) => Frag::Name(n, to),
        Frag::Verb(v, _) => Frag::Verb(v, to),
        Frag::Adverb(a, _) => Frag::Adverb(a, to),
        Frag::Conj(c, _) => Frag::Conj(c, to),
        other => other,
    }
}

fn apply(stack: &mut Vec<Frag>, scope: &Names) -> Result<bool> {
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
            let frag = apply_adverb(u, a, scope)?;
            stack.insert(1, frag);
        }
        Rule::Conj5 => {
            let mut t = take(stack, 1..4);
            let v = t.pop().expect("three slots");
            let c = t.pop().expect("three slots");
            let u = t.pop().expect("three slots");
            let frag = apply_conj(u, c, v, scope)?;
            stack.insert(1, frag);
        }
        Rule::Fork6 => {
            let mut t = take(stack, 1..4);
            let h = t.pop().expect("three slots");
            let g = t.pop().expect("three slots");
            let f = t.pop().expect("three slots");
            let frag = apply_fork(f, g, h, scope)?;
            stack.insert(1, frag);
        }
        Rule::Bident7 => {
            let mut t = take(stack, 1..3);
            let b = t.pop().expect("two slots");
            let a = t.pop().expect("two slots");
            let frag = apply_bident(a, b, scope)?;
            stack.insert(1, frag);
        }
        Rule::Assign8 => {
            let mut t = take(stack, 0..3);
            let value = t.pop().expect("three slots");
            let assign = t.pop().expect("three slots");
            let target = t.pop().expect("three slots");
            let names = scope;
            let scope = match assign {
                Frag::AssignGlobal(_) => Scope::Global,
                _ => Scope::Local,
            };
            let frag = apply_assign(target, value, scope, names)?;
            stack.insert(0, frag);
        }
        Rule::Paren9 => {
            let mut t = take(stack, 0..3);
            let close = t.pop().expect("three slots");
            let inner = t.pop().expect("three slots");
            let open = t.pop().expect("three slots");
            let outer = Span::merge(open.span(), close.span());
            stack.insert(0, respan(inner, outer));
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

/// A noun fragment's value, where it is a literal or an expression over
/// literals that settles at compile time. An index specification such as
/// `(<a:;1)` is written out rather than typed in, so a modifier capturing
/// one has to fold it.
fn noun_value(f: &Frag) -> Option<Array> {
    if let Some(a) = as_const(f) {
        return Some(a.clone());
    }
    let Frag::Noun(e) = f else { return None };
    let cfg = crate::verb::EvalCfg {
        agreement: crate::verb::Agreement::LeadingPrefix,
        fmt: crate::fmt::FmtOpts::J,
        tol: crate::verb::Tol::J,
        fill: None,
        rules: crate::frontend::Rules::default(),
    };
    crate::ir::fold_const(e, cfg)
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

/// The atomic representation `5!:0` is given, settled while the sentence is
/// read. It is either a literal, or the `5!:1` that made one — the name's
/// own part of speech is in the table already, so the two together read as
/// one phrase rather than as a value the run would have to hand back.
///
/// `None` where nothing settles it now; `Some(Err)` where what settles it
/// is not a representation.
fn representation_now(u: &Frag, scope: &Names) -> Option<Result<crate::gerund::Ar>> {
    let span = u.span();
    if let Some(name) = atomic_rep_of(u) {
        return Some(named_ar(&name, scope, span));
    }
    let m = noun_in_scope(u, scope)?;
    let inner = match m.as_boxes() {
        Some([one]) if m.rank() == 0 => one.clone(),
        _ => m,
    };
    Some(
        crate::gerund::Ar::from_array(&inner)
            .ok_or_else(|| Error::domain("5!:0 takes an atomic representation", span)),
    )
}

/// The name a `5!:1` in this fragment asks about.
fn atomic_rep_of(u: &Frag) -> Option<String> {
    let Frag::Noun(Expr::Monad { verb, y, .. }) = u else { return None };
    let Verb::Prim(p) = verb else { return None };
    if p.monad != MonadOp::AtomicRep {
        return None;
    }
    let a = noun_value(&Frag::Noun((**y).clone()))?;
    match a.as_boxes() {
        Some([one]) if a.rank() == 0 => crate::gerund::text_of(one),
        _ => None,
    }
}

/// What a name stands for, as an atomic representation, by the part of
/// speech the table gives it. A name with no meaning stands for itself.
fn named_ar(name: &str, scope: &Names, span: Span) -> Result<crate::gerund::Ar> {
    if let Some(v) = scope.verb_named(name) {
        return crate::gerund::verb_ar(v)
            .ok_or_else(|| Error::not_yet(format!("the atomic representation of {name}"), span));
    }
    match scope.const_named(name) {
        Some(a) => Ok(crate::gerund::Ar::Noun(a)),
        None => Ok(crate::gerund::Ar::Prim(name.to_string())),
    }
}

fn apply_adverb(u: Frag, a: Frag, scope: &Names) -> Result<Frag> {
    let Frag::Adverb(m, aspan) = a else {
        return Err(Error::internal("expected an adverb fragment"));
    };
    let span = Span::merge(u.span(), aspan);
    let glyph = match m {
        Modifier::Prim(g) => g,
        Modifier::Explicit(src) => return derive_explicit(&src, u, None, scope, span),
    };
    // `5!:0` is the inverse of `5!:1`: it takes an atomic representation
    // and gives back the entity it represents. The representation has to be
    // known now, since what it names decides how the sentence parses.
    if glyph == "5!:0" {
        let Some(ar) = representation_now(&u, scope) else {
            return Err(Error::not_yet("5!:0 over a representation computed at run time", span));
        };
        return ar_frag(&ar?, scope, span);
    }
    // `}` takes either operand: `m}` amends at the indices m, and `u}`
    // computes them from the arguments instead.
    if glyph == "}" {
        if !u.is_real_verb() {
            // A GERUND is boxed data too, and it is not a list of indices:
            // `` u`v`w} `` is the amend whose three verbs compute the
            // replacement, the indices and the array they go into.
            if let Some(m) = noun_in_scope(&u, scope)
                && is_gerund(&m)
            {
                let verbs = gerund_verbs(&u, scope, span)?;
                if verbs.len() != 3 {
                    return Err(Error::not_yet(
                        "a gerund amend of other than three verbs (u`v`w})",
                        span,
                    ));
                }
                return Ok(Frag::Verb(VerbFrag::V(Verb::AmendGerund(verbs)), span));
            }
            if let Some(m) = noun_value(&u) {
                return Ok(Frag::Verb(VerbFrag::V(Verb::Amend(m)), span));
            }
            // Indices the program computes are read where the amend is
            // APPLIED, so `j =. i. 3` then `0 j } b` amends at whatever `j`
            // holds by then. What the value may not be is a gerund: which
            // three verbs it names decides how the amend PARSES, and that
            // has to be known now.
            let Some(operand) = noun_expr(&u) else {
                return Err(Error::not_yet("amend over a computed index", span));
            };
            let deferred = crate::verb::Deferred {
                operand,
                template: Verb::Amend(Array::scalar_i64(0)),
                build: built_amend,
                spelling: "n}".to_string(),
                choices: HashMap::new(),
            };
            return Ok(Frag::Verb(
                VerbFrag::V(Verb::Deferred(std::sync::Arc::new(deferred))),
                span,
            ));
        }
        let (v, _) = as_verb(u)?;
        return Ok(Frag::Verb(VerbFrag::V(Verb::AmendVerb(Box::new(v))), span));
    }
    // `b.` takes either operand too: a noun names one of the thirty-two
    // boolean functions, a verb asks after the verb's own characteristics.
    if glyph == "b." && !u.is_real_verb() {
        // The operand is ONE whole number: a list, a fraction, a character
        // or a box names no function at all.
        let m = as_const(&u)
            .filter(|a| a.count() == 1)
            .and_then(Array::to_i64_vec)
            .and_then(|v| v.first().copied())
            .ok_or_else(|| Error::domain("m b. takes one whole number", span))?;
        // Thirty-five functions are numbered, and the reference tells the
        // numbers below them from the ones past either end: `_1 b.` is a
        // domain error down to `_16 b.`, and anything outside `_16` to `34`
        // is out of range.
        if !(-16..=34).contains(&m) {
            return Err(Error::domain(
                "m b. numbers the functions 0 to 34; that index is out of range",
                span,
            ));
        }
        let p = crate::verb::Prim {
            name: "b.",
            monad: MonadOp::TruthTable(m as i8),
            dyad: DyadOp::TruthTable(m as i8),
            ranks: [0, 0, 0],
        };
        return Ok(Frag::Verb(VerbFrag::V(Verb::Prim(p)), span));
    }
    // A gerund under one of the cycling adverbs: `` u`v/ `` inserts the
    // verbs between the items, left to right, and `` u`v\ ``, `` u`v\. ``
    // and `` u`v/. `` give one verb to each prefix, suffix, group or
    // diagonal in turn.
    if !u.is_real_verb()
        && matches!(glyph, "/" | "\\" | "\\." | "/.")
        && noun_in_scope(&u, scope).is_some_and(|a| a.as_boxes().is_some())
    {
        let vs = gerund_verbs(&u, scope, span)?;
        if vs.is_empty() {
            return Err(Error::domain("an adverb's gerund is empty", span));
        }
        let cycle = || Box::new(Verb::Cycle(vs.clone()));
        let derived = match glyph {
            "/" => Verb::Evoke(vs.clone(), 3),
            "\\" => Verb::Windowed(cycle(), WindowKind::Prefix),
            "\\." => Verb::Windowed(cycle(), WindowKind::Suffix),
            _ => Verb::Key(cycle()),
        };
        return Ok(Frag::Verb(VerbFrag::V(derived), span));
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
        // Names are already substituted where they were used, so a fixed
        // verb is the verb itself.
        "f." => v,
        "M." => Verb::Memo(Box::new(v), Default::default()),
        "b." => Verb::Characteristics(Box::new(v)),
        _ => return Err(Error::not_yet(format!("adverb ({glyph})"), span)),
    };
    Ok(Frag::Verb(VerbFrag::V(derived), span))
}

fn apply_conj(u: Frag, c: Frag, v: Frag, scope: &Names) -> Result<Frag> {
    let Frag::Conj(m, cspan) = c else {
        return Err(Error::internal("expected a conjunction fragment"));
    };
    let span = Span::merge(Span::merge(u.span(), cspan), v.span());
    let glyph = match m {
        Modifier::Prim(g) => g,
        Modifier::Explicit(src) => return derive_explicit(&src, u, Some(v), scope, span),
    };
    match glyph {
        "\"" => {
            // A VERB on the right lends its own three ranks: `u"v` is
            // `u"(v b. 0)`, which is what makes `<"(+/)` box the whole
            // argument and `<"(<"1)` box each of its rows.
            let ranks = if v.is_verb() {
                verb_operand(v.clone(), span)?.ranks().into()
            } else {
                rank_spec(&v, span)?
            };
            // `m"n` is the CONSTANT verb: a noun on the left is the answer
            // itself, whatever the arguments are, and n says how large a
            // cell each copy stands for. A GERUND of two verbs or more is
            // the one noun the rank conjunction does not read that way —
            // it cycles the verbs over the cells the rank names, one verb
            // per cell — and only a rank that is infinite in all three
            // places leaves it a constant. One box is a constant whatever
            // it holds, so `` (<'+:')"0 `` is that box three times.
            let f = if u.is_noun() {
                if ranks != [RANK_INF; 3] && gerund_operand(&u, scope) {
                    Verb::Cycle(gerund_verbs(&u, scope, span)?)
                } else {
                    match noun_value(&u) {
                        Some(m) => Verb::Constant(m),
                        None => {
                            return Err(Error::not_yet("a computed constant verb (m\"n)", span));
                        }
                    }
                }
            } else {
                verb_operand(u, span)?
            };
            Ok(Frag::Verb(VerbFrag::V(Verb::Rank(Box::new(f), ranks)), span))
        }
        "@:" => {
            let f = verb_operand(u, span)?;
            let g = verb_operand(v, span)?;
            Ok(Frag::Verb(VerbFrag::V(Verb::Atop(Box::new(f), Box::new(g), AtopForm::At)), span))
        }
        // `u@v` is `u@:v` applied at v's own ranks: one v-cell at a time,
        // with u run on each result. That difference in rank is all that
        // separates the two spellings.
        //
        // A NOUN on the right is the constant verb `n"_`, which is what
        // makes `*:@_1` a verb answering 1: only `@` reads a noun that way,
        // and only on the right — `@:` and `&` refuse it, and so does a
        // noun on the left.
        "@" => {
            if u.is_noun() {
                return Err(Error::domain("@ takes a verb on the left", span));
            }
            let f = verb_operand(u, span)?;
            let g = if v.is_noun() {
                let n = noun_value(&v)
                    .ok_or_else(|| Error::not_yet("a computed noun operand to @", span))?;
                Verb::Constant(n)
            } else {
                verb_operand(v, span)?
            };
            let ranks = g.ranks();
            let atop = Verb::Atop(Box::new(f), Box::new(g), AtopForm::At);
            Ok(Frag::Verb(VerbFrag::V(Verb::Rank(Box::new(atop), ranks.into())), span))
        }
        "&" => compose(u, v, false, span),
        "&:" => compose(u, v, true, span),
        // `u&.>` is the one under that is not built out of an inverse:
        // opening each box and boxing the result again is J's each.
        "&." if is_open(&v) => {
            let f = verb_operand(u, span)?;
            Ok(Frag::Verb(VerbFrag::V(Verb::Each(Box::new(f), Enclose::Always)), span))
        }
        // `u&.,` is the other one. `,` has no obverse — a ravel says
        // nothing about the shape it came from — but the shape is in hand
        // here, so u runs over the ravel and the shape goes back on.
        "&." if is_ravel(&v) => {
            let f = verb_operand(u, span)?;
            Ok(Frag::Verb(VerbFrag::V(Verb::UnderRavel(Box::new(f))), span))
        }
        // `u&.v` is `v^:_1 @: u &: v`: v prepares both arguments, u runs on
        // what it made, and v's obverse puts the answer back. `&.` does it
        // at v's monadic rank, `&.:` on the arguments whole — the same
        // difference `&` and `&:` have.
        "&." | "&.:" => {
            let f = verb_operand(u, span)?;
            let g = verb_operand(v, span)?;
            let back = obverse_of(&g, span)?;
            let composed = Verb::Compose(Box::new(f), Box::new(g.clone()));
            let under = Verb::Atop(Box::new(back), Box::new(composed), AtopForm::At);
            if glyph == "&.:" {
                return Ok(Frag::Verb(VerbFrag::V(under), span));
            }
            let rank = g.ranks()[0];
            Ok(Frag::Verb(VerbFrag::V(Verb::Rank(Box::new(under), [rank; 3].into())), span))
        }
        "^:" => {
            let f = verb_operand(u, span)?;
            if v.is_verb() {
                // `u^:v` asks v for the number of applications; the while
                // loop is that verb under `^:_`.
                let g = verb_operand(v, span)?;
                let p = Verb::PowerV(Box::new(f), Box::new(g));
                return Ok(Frag::Verb(VerbFrag::V(p), span));
            }
            // A negative count runs the obverse that many times, whether it
            // was written plainly or in a box (`u^:(<_3)`); `power_spec`
            // marks it, and which obverse it is waits for the arguments.
            if let Some(arr) = noun_value(&v) {
                let p = power_spec(&arr, span)?;
                return Ok(Frag::Verb(VerbFrag::V(Verb::PowerN(Box::new(f), p)), span));
            }
            // A count the program computes is read where the derived verb
            // is applied, so a definition's own argument can decide it.
            let Some(operand) = noun_expr(&v) else {
                return Err(Error::not_yet("computed power (u^:n)", span));
            };
            let spelling = format!("{}^:n", f.name());
            let deferred = crate::verb::Deferred {
                operand,
                template: f,
                build: built_power,
                spelling,
                choices: HashMap::new(),
            };
            Ok(Frag::Verb(
                VerbFrag::V(Verb::Deferred(std::sync::Arc::new(deferred))),
                span,
            ))
        }
        ";." => {
            // A gerund cuts with one verb per piece, as the cycling
            // adverbs do; anything else is the one verb every piece gets.
            let is_gerund = gerund_operand(&u, scope);
            let f = if is_gerund {
                Verb::Cycle(gerund_verbs(&u, scope, span)?)
            } else {
                verb_operand(u, span)?
            };
            let n = one_atom(&v, "cut", span)?;
            if n.fract() != 0.0 || !matches!(n as i64, -3..=3) {
                return Err(Error::not_yet(format!("cut (u;.{n})"), span));
            }
            // The TESSELATING cut alone takes no gerund: the reference cuts
            // with one verb per piece under every other number and refuses
            // `` (+`,);.3 `` and `` (+`,);._3 `` however they are applied.
            if is_gerund && matches!(n as i64, 3 | -3) {
                return Err(Error::domain("the tesselating cut takes no gerund", span));
            }
            Ok(Frag::Verb(VerbFrag::V(Verb::Cut(Box::new(f), n as i64)), span))
        }
        // `u!.n` is the tolerance for the verbs whose meaning uses one; on
        // any other verb J's `!.` specifies a fill, which is its own
        // feature and not this one.
        "!." => {
            let f = verb_operand(u, span)?;
            // `|.!.f` is the fill shift: the fit specifies what the places
            // an item left behind are filled with, not a tolerance.
            if matches!(&f, Verb::Prim(p) if p.name == "|.") {
                let fill = as_const(&v)
                    .cloned()
                    .ok_or_else(|| Error::not_yet("a computed fill (|.!.n)", span))?;
                return Ok(Frag::Verb(VerbFrag::V(Verb::ShiftFill(fill)), span));
            }
            // On the verbs that fill, the fit names the ELEMENT a value
            // runs out into. `>` does both: its dyad compares, so a fit
            // that is a tolerance is one there too.
            if let Some(fill) = fill_taking(&f).then(|| as_const(&v).and_then(crate::verb::FillAtom::of)).flatten()
            {
                let inner = match one_atom(&v, "fit", span) {
                    Ok(n) if f.uses_tolerance() && (0.0..=LARGEST_TOLERANCE).contains(&n) => {
                        Verb::Fit(Box::new(f), n)
                    }
                    _ => f,
                };
                return Ok(Frag::Verb(VerbFrag::V(Verb::Fill(Box::new(inner), fill)), span));
            }
            let n = one_atom(&v, "fit", span)?;
            // `^!.n` is the STOPE rather than a tolerance, and takes any
            // step: `2 ^!.5 3` is `2 × 7 × 12`.
            if matches!(&f, Verb::Prim(p) if p.name == "^") {
                return Ok(Frag::Verb(VerbFrag::V(Verb::Fit(Box::new(f), n)), span));
            }
            if !f.uses_tolerance() {
                // Every verb J gives a fit to is one of the two above; the
                // rest refuse one outright, and so does libjay.
                return Err(Error::domain(format!("{} takes no fit (u!.n)", f.name()), span));
            }
            // `i.!.1` is the one tolerance above that bound the reference
            // takes, and it searches exactly as the default tolerance does:
            // `2 i.!.1 (1 2 3)` is `1 0 1`, not the `0 0 1` a tolerance of
            // one would find.
            if n == 1.0 && matches!(&f, Verb::Prim(p) if p.name == "i.") {
                return Ok(Frag::Verb(VerbFrag::V(f), span));
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
        // `u :. v` declares v to be u's obverse; it changes nothing about
        // how u applies, only what `^:_1` and `&.` may then do with it.
        ":." => {
            let f = verb_operand(u, span)?;
            let g = verb_operand(v, span)?;
            Ok(Frag::Verb(
                VerbFrag::V(Verb::WithObverse(Box::new(f), Box::new(g))),
                span,
            ))
        }
        // `u@.v` picks one verb of the gerund u by v's value at the
        // arguments; a noun on the right picks one now and for good.
        "@." => {
            let vs = gerund_verbs(&u, scope, span)?;
            if v.is_verb() {
                let w = verb_operand(v, span)?;
                return Ok(Frag::Verb(VerbFrag::V(Verb::Agenda(vs, Box::new(w))), span));
            }
            let at = agenda_index(&v, span)?;
            // An atom picks one verb; a LIST picks several and makes the
            // train they spell, which is what the reference answers — a
            // one-element list is that verb, two are a hook and three a
            // fork, and a fourth has no meaning here even though `` `:6 ``
            // gives one.
            if at.list {
                if at.at.is_empty() {
                    return Err(Error::new(
                        ErrorKind::Length,
                        "an agenda index has no elements",
                        Some(span),
                    ));
                }
                if at.at.len() > 3 {
                    return Err(Error::domain(
                        format!("an agenda train is 1, 2 or 3 verbs, not {}", at.at.len()),
                        span,
                    ));
                }
                let picked: Vec<Verb> = at
                    .at
                    .iter()
                    .map(|k| crate::verb::pick_gerund(&vs, *k, span))
                    .collect::<Result<_>>()?;
                return train_of(picked, span);
            }
            let picked = crate::verb::pick_gerund(&vs, at.at[0], span)?;
            Ok(Frag::Verb(VerbFrag::V(picked), span))
        }
        // `u`v` ties two entities into a gerund, which is ordinary boxed
        // data: one box per atomic representation, catenated.
        "`" => {
            let left = tie_side(&u, scope, span)?;
            let right = tie_side(&v, scope, span)?;
            let tied = crate::verb::catenate(&left, &right, true, true, span)?;
            Ok(Frag::Noun(Expr::Const(tied, span)))
        }
        // `u :: v` answers a refusal of u by running v instead. A noun on
        // the right is the constant verb yielding it, as J reads it.
        "::" => {
            let f = verb_operand(u, span)?;
            let g = if v.is_noun() {
                constant_verb(bond_noun(&v, span)?)
            } else {
                verb_operand(v, span)?
            };
            Ok(Frag::Verb(VerbFrag::V(Verb::Adverse(Box::new(f), Box::new(g))), span))
        }
        // `u L: n` and `u S: n` apply u at a boxing level: `L:` puts each
        // answer back in the box its operand came from, `S:` spreads them
        // into one array.
        "L:" | "S:" => {
            let f = verb_operand(u, span)?;
            let levels = level_spec(&v, span)?;
            let level = Verb::Level {
                u: Box::new(f),
                levels,
                spread: glyph == "S:",
            };
            Ok(Frag::Verb(VerbFrag::V(level), span))
        }
        // `` m`:n ``: 0 applies every verb of the gerund to the arguments
        // and frames the answers, 3 inserts them between the items of y,
        // and 6 is the train the gerund spells, which is built here.
        "`:" => {
            if u.is_verb() {
                return Err(Error::domain(
                    "`: reads a gerund, which is boxed data, not a verb",
                    span,
                ));
            }
            let vs = gerund_verbs(&u, scope, span)?;
            let n = one_atom(&v, "evoke gerund", span)?;
            if vs.is_empty() {
                return Err(Error::domain("an evoked gerund is empty", span));
            }
            match n {
                0.0 | 3.0 => {
                    Ok(Frag::Verb(VerbFrag::V(Verb::Evoke(vs, n as i64)), span))
                }
                6.0 => train_of(vs, span),
                _ => Err(Error::domain(
                    format!("`:{n} is not one of the evoke forms 0, 3 and 6"),
                    span,
                )),
            }
        }
        // The fold family. The first inflection says whether the answer is
        // the last running value or every one of them, the second whether
        // the items are taken from the front or from the back.
        "F.." | "F.:" | "F:." | "F::" => {
            let f = verb_operand(u, span)?;
            let g = verb_operand(v, span)?;
            let bytes = glyph.as_bytes();
            Ok(Frag::Verb(
                VerbFrag::V(Verb::Fold {
                    u: Box::new(f),
                    v: Box::new(g),
                    multiple: bytes[1] == b':',
                    reverse: bytes[2] == b':',
                }),
                span,
            ))
        }
        // `m H. n`: the generalised hypergeometric function, m the
        // numerator parameters and n the denominator ones. Both are nouns,
        // and an empty list on either side is the ordinary case of none.
        "H." => {
            let num = series_parameters(&u, span)?;
            let den = series_parameters(&v, span)?;
            Ok(Frag::Verb(VerbFrag::V(Verb::Hypergeometric { num, den }), span))
        }
        // Threads reach outside the expression, which the sandbox closes;
        // libjay's own parallelism is not something a sentence asks for.
        // That is a property of libjay, not a queue position.
        "T." => Err(Error::sandbox(
            "T. starts J's own threads, which libjay does not open",
            span,
        )),
        // `u t. n` schedules u in one of J's thread pools and answers with
        // a pyx — a task, not a value. The sandbox does not open those
        // threads, which is libjay's own policy and not a queue position.
        // The reference rejects `t:` outright — an invalid inflection, as
        // it does `d.`, `D.` and `D:`. There is nothing here to implement.
        "t:" => Err(Error::new(
            ErrorKind::Language,
            "t: is not a J inflection; the reference rejects the spelling",
            Some(span),
        )),
        "t." => Err(Error::sandbox(
            "t. runs a verb in one of J's thread pools, which libjay does not open",
            span,
        )),
        // `u . v`: the inner product, of which `+/ . *` is the matrix
        // product and `-/ . *` the determinant.
        "." => {
            let f = verb_operand(u, span)?;
            let g = verb_operand(v, span)?;
            Ok(Frag::Verb(VerbFrag::V(Verb::InnerProduct {
                u: Box::new(f),
                v: Box::new(g),
                apl: false,
            }), span))
        }
        "!:" => foreign(&u, &v, span),
        // `u : v` is one verb out of two: u is its monad, v its dyad. The
        // explicit definitions spelled `3 : '…'` and `4 : '…'` are read by
        // the lexer and never reach here.
        ":" => {
            // `m : n` with a boxed n is the ordinary multi-line definition
            // written on one line, one box per line. The literal
            // `3 : 'text'` spelling never arrives here — the lexer takes it
            // while the sentence is still words — so a noun on the left
            // means the body came from an expression.
            if u.is_noun() && v.is_noun() {
                return boxed_body_definition(&u, &v, scope, span);
            }
            let f = verb_operand(u, span)?;
            let g = verb_operand(v, span)?;
            Ok(Frag::Verb(
                VerbFrag::V(Verb::Ambivalent(Box::new(f), Box::new(g))),
                span,
            ))
        }
        _ => Err(Error::not_yet(format!("the conjunction {glyph}"), span)),
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
        return verb(Verb::Rank(Box::new(composed), [monadic_rank; 3].into()));
    }
    if u.is_noun() && v.is_noun() {
        return Err(Error::not_yet("noun-operand conjunctions", span));
    }
    // A bond applies its verb dyadically to the WHOLE argument: `m&v y` is
    // `m v y`, and its rank is infinite whatever v's is — `1 2&+ b. 0`
    // reports `_ _ _`, and `1 2&+ i. 2 2` agrees row by row rather than
    // pairing the noun with every atom.
    if u.is_noun() {
        let bound = bond_noun(&u, span);
        let g = as_verb(v)?.0;
        if let Ok(m) = bound {
            return verb(Verb::BondLeft(m, Box::new(g)));
        }
        return deferred_bond(noun_expr(&u), g, true, span).map(|v| Frag::Verb(VerbFrag::V(v), span));
    }
    let f = as_verb(u)?.0;
    if let Ok(n) = bond_noun(&v, span) {
        return verb(Verb::BondRight(Box::new(f), n));
    }
    deferred_bond(noun_expr(&v), f, false, span).map(|v| Frag::Verb(VerbFrag::V(v), span))
}

/// A bond whose noun the program computes: `(}: c)&+`, `m&mp`, `%&(+/ y)`.
///
/// The noun is read where the derived verb is APPLIED rather than while the
/// program compiles, so a name assigned in an earlier sentence — or a
/// definition's own argument — may stand where a literal does. Everything
/// else about the bond is what it was.
fn deferred_bond(operand: Option<Expr>, f: Verb, left: bool, span: Span) -> Result<Verb> {
    let Some(operand) = operand else {
        return Err(Error::not_yet("bonds over a non-literal noun", span));
    };
    let spelling = if left { format!("n&{}", f.name()) } else { format!("{}&n", f.name()) };
    let build = if left { built_bond_left } else { built_bond_right };
    let deferred =
        crate::verb::Deferred { operand, template: f, build, spelling, choices: HashMap::new() };
    Ok(Verb::Deferred(std::sync::Arc::new(deferred)))
}

fn built_amend(
    _template: &Verb,
    a: &Array,
    span: Span,
    _d: crate::frontend::Rules,
) -> Result<Verb> {
    if is_gerund(a) {
        return Err(Error::not_yet("a computed gerund amend (u`v`w})", span));
    }
    Ok(Verb::Amend(a.clone()))
}

fn built_bond_left(
    g: &Verb,
    a: &Array,
    _span: Span,
    _d: crate::frontend::Rules,
) -> Result<Verb> {
    Ok(Verb::BondLeft(a.clone(), Box::new(g.clone())))
}

fn built_bond_right(
    f: &Verb,
    a: &Array,
    _span: Span,
    _d: crate::frontend::Rules,
) -> Result<Verb> {
    Ok(Verb::BondRight(Box::new(f.clone()), a.clone()))
}

/// The largest comparison tolerance `!.` accepts, as J's does: 2^-34.
const LARGEST_TOLERANCE: f64 = 5.820_766_091_346_741e-11;

/// A conjunction's single numeric noun operand.
/// One side's parameter list for `m H. n`: a numeric list, known now.
fn series_parameters(f: &Frag, span: Span) -> Result<Vec<crate::complex::Cx>> {
    let Some(arr) = as_const(f) else {
        return Err(Error::not_yet("computed hypergeometric parameters (m H. n)", span));
    };
    if arr.count() == 0 {
        return Ok(Vec::new());
    }
    if arr.rank() > 1 {
        return Err(Error::parse("a hypergeometric parameter list is a vector", span));
    }
    match arr.data.cast(crate::dtype::DType::Complex) {
        Some(Data::Complex(v)) => Ok(v.as_slice().to_vec()),
        _ => Err(Error::parse("hypergeometric parameters are numbers", span)),
    }
}

/// `m !: n`: J's foreigns, the family that reaches outside the language.
///
/// Three of them are libjay's. `1!:1` reads a line from the input source
/// and `1!:2` writes one to the output sink — the two halves of the stdio
/// the sandbox opens — and `3!:0` names an element type, which computes
/// and touches nothing.
///
/// The rest divide in two, and the division is the whole point of the
/// dispatcher. A foreign that would reach a file, a directory, the host or
/// a script is closed by the sandbox and no release will open it; one that
/// only computes is a queue position, and names itself as one.
fn foreign(u: &Frag, v: &Frag, span: Span) -> Result<Frag> {
    let family = foreign_number(u, span)?;
    let member = foreign_number(v, span)?;
    let prim = |name, monad, dyad| {
        Ok(Frag::Verb(
            VerbFrag::V(Verb::Prim(Prim { name, monad, dyad, ranks: [RANK_INF; 3] })),
            span,
        ))
    };
    let closed = |what: &str| {
        Err(Error::sandbox(format!("{family}!:{member} {what}, which is outside the program"), span))
    };
    use crate::foreign::FormatKind;
    match (family, member) {
        (1, 1) => prim("1!:1", MonadOp::ReadStream, DyadOp::None),
        (1, 2) => prim("1!:2", MonadOp::None, DyadOp::WriteStream),
        (3, 0) => prim("3!:0", MonadOp::TypeCode, DyadOp::None),
        // The binary form: an array as the bytes that stand for it, the
        // value those bytes stand for, and the same bytes in hexadecimal.
        (3, 1) => prim("3!:1", MonadOp::BinaryRep, DyadOp::None),
        (3, 2) => prim("3!:2", MonadOp::FromBinaryRep, DyadOp::None),
        (3, 3) => prim("3!:3", MonadOp::HexRep, DyadOp::None),
        // The two byte conversions, which read and write the machine
        // spellings of a number rather than J's own.
        (3, 4) => prim("3!:4", MonadOp::None, DyadOp::IntBytes),
        (3, 5) => prim("3!:5", MonadOp::None, DyadOp::FloatBytes),
        // The name table: what class a name has, which names have one, and
        // erasing them.
        (4, 0) => prim("4!:0", MonadOp::NameClasses, DyadOp::None),
        (4, 1) => prim("4!:1", MonadOp::NamesOfClass, DyadOp::None),
        (4, 55) => prim("4!:55", MonadOp::EraseNames, DyadOp::None),
        // `5!:1 <'name'` is the atomic representation of what the name
        // stands for — the same boxed data a gerund is made of, and `5!:0`
        // is the adverb that reads one back.
        (5, 0) => Ok(Frag::Adverb(Modifier::Prim("5!:0"), span)),
        (5, 1) => prim("5!:1", MonadOp::AtomicRep, DyadOp::None),
        (5, 2) => prim("5!:2", MonadOp::BoxedRep, DyadOp::None),
        (5, 5) => prim("5!:5", MonadOp::LinearRep, DyadOp::None),
        (5, 6) => prim("5!:6", MonadOp::ParenRep, DyadOp::None),
        // The three formats, which spell a number for the world outside J:
        // a leading `-` rather than `_`, and columns of one width.
        (8, 0) => prim(
            "8!:0",
            MonadOp::FormatForeign(FormatKind::PerAtom),
            DyadOp::FormatForeign(FormatKind::PerAtom),
        ),
        (8, 1) => prim(
            "8!:1",
            MonadOp::FormatForeign(FormatKind::PerColumn),
            DyadOp::FormatForeign(FormatKind::PerColumn),
        ),
        (8, 2) => prim(
            "8!:2",
            MonadOp::FormatForeign(FormatKind::Chars),
            DyadOp::FormatForeign(FormatKind::Chars),
        ),
        // The two global parameters libjay HONOURS: what a display shows of
        // a float, and how near two numbers have to be to compare equal.
        (9, 10) => prim("9!:10", MonadOp::PrintPrecision, DyadOp::None),
        (9, 11) => prim("9!:11", MonadOp::SetPrintPrecision, DyadOp::None),
        (9, 18) => prim("9!:18", MonadOp::Tolerance, DyadOp::None),
        (9, 19) => prim("9!:19", MonadOp::SetTolerance, DyadOp::None),
        (128, 3) => prim("128!:3", MonadOp::Crc32, DyadOp::None),
        // The locale family. Every member of it reads or writes the
        // namespace table and nothing outside the program.
        (18, 0) => prim("18!:0", MonadOp::LocaleKind, DyadOp::None),
        (18, 1) => prim("18!:1", MonadOp::LocaleNames, DyadOp::None),
        (18, 2) => prim("18!:2", MonadOp::LocalePath, DyadOp::LocalePathSet),
        (18, 3) => prim("18!:3", MonadOp::LocaleCreate, DyadOp::None),
        (18, 5) => prim("18!:5", MonadOp::LocaleCurrent, DyadOp::None),
        (18, 55) => prim("18!:55", MonadOp::LocaleErase, DyadOp::None),
        // `18!:4` is not in the reference build libjay is measured against
        // at all, and `18!:6` answers a dump of the interpreter's own name
        // tables, which is machinery rather than a meaning.
        (18, 4) => Err(Error::language(
            "18!:4 is not a foreign the reference defines; `cocurrent` is how a \
             program changes locale"
                .to_string(),
            span,
        )),
        (18, 6) => Err(Error::language(
            "18!:6 answers the interpreter's own name tables, which libjay does \
             not have"
                .to_string(),
            span,
        )),
        (0, _) => closed("runs a script file"),
        // The rest of the file family: stdin and stdout are the streams the
        // sandbox opens, and every other member of it is the filesystem.
        (1, _) => closed("reaches the filesystem"),
        (2, _) => closed("reaches the host — its environment, its shell, its processes"),
        (6, _) => closed("reads the clock"),
        (15, _) => closed("calls into a shared library"),
        // The interpreter's own machinery. Space is what the reference's
        // allocator holds, and the debug family drives its session; neither
        // is a meaning a second implementation could answer.
        (7, _) => Err(Error::language(
            format!(
                "{family}!:{member} measures the reference interpreter's own memory, \
                 which libjay does not have"
            ),
            span,
        )),
        (13, _) => Err(Error::language(
            format!(
                "{family}!:{member} drives the reference interpreter's debugger, \
                 which libjay does not have"
            ),
            span,
        )),
        // The random seed is a global parameter libjay has and has not
        // wired up; everything else in the family is the interpreter's own.
        (9, 0 | 1) => Err(Error::not_yet(format!("the foreign {family}!:{member}"), span)),
        (9, _) => Err(Error::language(
            format!(
                "{family}!:{member} is a setting of the reference interpreter's own \
                 machinery; libjay honours 9!:10 and 9!:11 (print precision) and \
                 9!:18 and 9!:19 (comparison tolerance)"
            ),
            span,
        )),
        _ => Err(Error::not_yet(format!("the foreign {family}!:{member}"), span)),
    }
}

/// One side of `m !: n`: a whole number, known now. A foreign is chosen by
/// its two numbers, so neither may be computed.
fn foreign_number(f: &Frag, span: Span) -> Result<i64> {
    if f.is_verb() {
        return Err(Error::parse("a foreign is spelled m!:n, with two numbers", span));
    }
    let Some(arr) = as_const(f) else {
        return Err(Error::not_yet("a computed foreign number (m!:n)", span));
    };
    match arr.to_i64_vec().as_deref() {
        Some([n]) if *n >= 0 => Ok(*n),
        _ => Err(Error::parse("a foreign is spelled m!:n, with two whole numbers", span)),
    }
}

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

fn is_ravel(f: &Frag) -> bool {
    matches!(f, Frag::Verb(VerbFrag::V(Verb::Prim(p)), _) if p.monad == MonadOp::Ravel)
}

/// Whether `u!.f` specifies a FILL on this verb rather than a tolerance.
///
/// These are the verbs whose answer can reach past what their argument
/// holds: take and reshape, the three joins, the copy, the raze and the
/// open. J refuses a fit on anything else that has no tolerance.
fn fill_taking(f: &Verb) -> bool {
    let Verb::Prim(p) = f else { return false };
    matches!(p.name, "{." | "$" | "," | ",." | ",:" | "#" | ";" | ">")
}

fn verb_operand(f: Frag, span: Span) -> Result<Verb> {
    if f.is_noun() {
        return Err(Error::not_yet("noun-operand conjunctions", span));
    }
    Ok(as_verb(f)?.0)
}

/// `u L: n` and `u S: n`: the level operand is read like a rank — 1 atom for
/// every valence, 2 atoms `left right` with the monadic level taken from the
/// right, 3 atoms in full — and the reading happens when the derived verb is
/// APPLIED, not when the sentence is read, so `] S:_ 2` is a verb and shows
/// itself. The two infinities are the two ends of the descent: `_` is the
/// whole argument, boxed however deeply, and `__` its leaves, which is level
/// 0 written the other way round.
/// The index `u@.n` reads, and whether it was written as a LIST. The two
/// spellings mean different things — an atom picks one verb of the gerund,
/// a list picks several and makes the train they spell — so which was
/// written has to travel with the numbers.
struct AgendaIndex {
    at: Vec<i64>,
    list: bool,
}

/// `u@.n`'s right operand. It is read the way `{` reads an index: a boxed
/// atom is opened, a negative counts back from the end of the gerund, and a
/// rank above one has no meaning.
#[inline(never)]
fn agenda_index(f: &Frag, span: Span) -> Result<AgendaIndex> {
    let Some(arr) = as_const(f) else {
        return Err(Error::not_yet("a computed agenda specification", span));
    };
    // One box holding the index is the index, as `{` opens one.
    let opened;
    let arr = match arr.as_boxes().and_then(|bs| (bs.len() == 1).then(|| bs[0].clone())) {
        Some(inner) => {
            opened = inner;
            &opened
        }
        None => arr,
    };
    if arr.shape.len() > 1 {
        return Err(Error::new(
            ErrorKind::Rank,
            "an agenda index is an atom or a list, not a table",
            Some(span),
        ));
    }
    let list = !arr.shape.is_empty();
    let Some(vals) = arr.to_f64_vec() else {
        return Err(Error::domain("an agenda index must be numeric", span));
    };
    if !list && vals.len() != 1 {
        return Err(Error::parse("agenda takes one atom", span));
    }
    let mut at = Vec::with_capacity(vals.len());
    for x in vals {
        let x = near_whole(x);
        if x.fract() != 0.0 {
            return Err(Error::domain("an agenda index must be a whole number", span));
        }
        at.push(x as i64);
    }
    Ok(AgendaIndex { at, list })
}

fn level_spec(f: &Frag, span: Span) -> Result<crate::verb::Ranks> {
    let Some(arr) = as_const(f) else {
        return Err(Error::not_yet("a computed level specification", span));
    };
    if arr.shape.len() > 1 {
        return Err(Error::new(
            ErrorKind::Rank,
            "a level takes a list of atoms, not a table",
            Some(span),
        ));
    }
    let Some(vals) = arr.to_f64_vec() else {
        return Err(Error::domain("a level must be numeric", span));
    };
    if vals.is_empty() || vals.len() > 3 {
        return Err(Error::new(
            ErrorKind::Length,
            "a level takes 1 to 3 atoms",
            Some(span),
        ));
    }
    let mut r = Vec::with_capacity(vals.len());
    for x in vals {
        if x == f64::INFINITY {
            r.push(RANK_INF);
        } else if x == f64::NEG_INFINITY {
            r.push(-RANK_INF);
        } else if x.fract() != 0.0 {
            return Err(Error::domain("a level must be an integer", span));
        } else {
            r.push(x as i64);
        }
    }
    // One atom stands for every level, two atoms `m n` for `n m n`. The
    // count is kept beside them: it says nothing about what the verb does,
    // and everything about how it spells itself back.
    let triple = match r.len() {
        1 => [r[0], r[0], r[0]],
        2 => [r[1], r[0], r[1]],
        _ => [r[0], r[1], r[2]],
    };
    Ok(crate::verb::Ranks::spelled(triple, r.len() as u8))
}

/// `u"n`: 1 atom applies to every valence, 2 atoms are `left right` with the
/// monadic rank taken from the right, 3 atoms are given in full.
fn rank_spec(f: &Frag, span: Span) -> Result<crate::verb::Ranks> {
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
    let triple = match r.len() {
        1 => [r[0], r[0], r[0]],
        2 => [r[1], r[0], r[1]],
        _ => [r[0], r[1], r[2]],
    };
    Ok(crate::verb::Ranks::spelled(triple, r.len() as u8))
}

/// `u^:n`: one nonnegative integer atom, or `_` for "iterate until the
/// result stops changing".
/// The whole number a count operand stands for, or the value unchanged.
///
/// A noun operand read at compile time makes the same near-integer
/// admission a count read at run time makes: jconsole answers
/// `(>: ^: (2 + 1e_13)) 3` with 5 and `(+`- @. (1 + 1e_14)) 5` with _5.
fn near_whole(n: f64) -> f64 {
    crate::array::NearInt::J.round(n).map_or(n, |k| k as f64)
}

/// `u^:n` once the right operand has a value.
fn built_power(
    f: &Verb,
    a: &Array,
    span: Span,
    _d: crate::frontend::Rules,
) -> Result<Verb> {
    Ok(Verb::PowerN(Box::new(f.clone()), power_spec(a, span)?))
}

/// A noun operand as an expression, for a modifier that reads it where the
/// derived verb is applied rather than while the program compiles.
fn noun_expr(f: &Frag) -> Option<Expr> {
    match f {
        Frag::Noun(e) => Some(e.clone()),
        Frag::Name(n, span) => Some(Expr::Name(n.clone(), *span)),
        _ => None,
    }
}

fn power_spec(arr: &Array, span: Span) -> Result<Power> {
    // A boxed count traces the applications rather than taking one of
    // them: `u^:(<n)` is `u^:(i.n)`, and `u^:a:` traces to convergence.
    if let Some(boxes) = arr.as_boxes() {
        let [inner] = boxes else {
            return Err(Error::parse("a boxed power takes one box", span));
        };
        if inner.count() == 0 {
            return Ok(Power::ConvergeTrace);
        }
        let Some(vals) = inner.to_f64_vec() else {
            return Err(Error::parse("power must be numeric", span));
        };
        let vals: Vec<f64> = vals.into_iter().map(near_whole).collect();
        let [n] = vals[..] else {
            return Err(Error::not_yet("a boxed list of power counts (u^:(<n))", span));
        };
        if n.fract() != 0.0 || n.abs() > 1e6 {
            return Err(Error::parse("a boxed power must be a whole count", span));
        }
        // `u^:(<n)` is `u^:(i.n)`: n counts, downwards where n is negative.
        if n == 0.0 {
            return Err(Error::domain("a boxed power traces at least one application", span));
        }
        // A negative n counts the same way with the obverse.
        let sign = if n < 0.0 { -1 } else { 1 };
        return Ok(Power::Each((0..n.abs() as i64).map(|k| sign * k).collect()));
    }
    let Some(vals) = arr.to_f64_vec() else {
        return Err(Error::parse("power must be numeric", span));
    };
    let vals: Vec<f64> = vals.into_iter().map(near_whole).collect();
    if vals.len() > 1 {
        // A list of counts gives one answer each, framed; a negative one
        // among them counts backwards over the obverse.
        let mut counts = Vec::with_capacity(vals.len());
        for n in &vals {
            if n.fract() != 0.0 || n.abs() > 1e6 {
                return Err(Error::not_yet("a power count outside _1e6 … 1e6", span));
            }
            counts.push(*n as i64);
        }
        return Ok(Power::Each(counts));
    }
    let [n] = vals[..] else {
        return Err(Error::not_yet("power over a list of counts (u^:n)", span));
    };
    if n == f64::INFINITY {
        return Ok(Power::Converge);
    }
    if n.fract() != 0.0 {
        return Err(Error::parse("power must be a whole number", span));
    }
    if n < 0.0 {
        // A negative power is the obverse applied that many times.
        return Ok(Power::Inverse((-n) as u64));
    }
    Ok(Power::Times(n as u64))
}

/// The obverse of a verb, or the diagnostic naming the verb that has none.
pub(crate) fn obverse_of(v: &Verb, span: Span) -> Result<Verb> {
    crate::verb::obverse(v).ok_or_else(|| {
        Error::not_yet(format!("the obverse of {} (no inverse is known)", v.name()), span)
    })
}

/// One side of `` u`v ``: a verb becomes the box holding its atomic
/// representation, a noun stands for itself. Catenating the two is the tie,
/// which is why `` u`v`w `` builds up left to right with no special case.
fn tie_side(f: &Frag, scope: &Names, span: Span) -> Result<Array> {
    if f.is_real_verb() {
        let (v, _) = as_verb(f.clone())?;
        return Ok(Array::boxed(verb_ar(&v, span)?.to_array()));
    }
    noun_in_scope(f, scope)
        .ok_or_else(|| Error::not_yet("a tie over a computed noun", span))
}

/// A verb's atomic representation, with the diagnostic for the verbs libjay
/// has no J spelling to give.
fn verb_ar(v: &Verb, span: Span) -> Result<crate::gerund::Ar> {
    crate::gerund::verb_ar(v).ok_or_else(|| {
        Error::not_yet(format!("the atomic representation of {}", v.name()), span)
    })
}

/// A noun fragment's value, a name that holds a literal included. A gerund
/// is data, so `` g =. +`- `` and then `g@.1` has to find what g holds.
fn noun_in_scope(f: &Frag, scope: &Names) -> Option<Array> {
    if let Frag::Name(n, _) = f {
        return scope.const_named(n);
    }
    noun_value(f)
}

/// Whether a modifier's operand is a GERUND to be cycled over the pieces
/// rather than a noun to be read as data: two atomic representations or
/// more. ONE box is data whatever it holds, which is why `` (<'+:')"0 ``
/// is that box once per cell and `` (+:`*:)"0 `` doubles and squares in
/// turn.
fn gerund_operand(u: &Frag, scope: &Names) -> bool {
    !u.is_real_verb()
        && noun_in_scope(u, scope).is_some_and(|a| a.count() > 1 && is_gerund(&a))
}

/// Whether this value is a GERUND rather than data: a non-empty boxed
/// array every item of which is an atomic representation. An index
/// specification holds integers and never reads as one.
fn is_gerund(a: &Array) -> bool {
    match a.as_boxes() {
        Some(items) => {
            !items.is_empty()
                && items.iter().all(|it| crate::gerund::Ar::from_array(it).is_some())
        }
        None => false,
    }
}

/// The verbs a gerund holds. A lone verb is a gerund of one, and boxed data
/// is read as the atomic representations it is.
fn gerund_verbs(f: &Frag, scope: &Names, span: Span) -> Result<Vec<Verb>> {
    if f.is_real_verb() {
        return Ok(vec![as_verb(f.clone())?.0]);
    }
    let arr = noun_in_scope(f, scope)
        .ok_or_else(|| Error::not_yet("a gerund computed at run time", span))?;
    let Some(items) = arr.as_boxes() else {
        return Err(Error::domain("a gerund is boxed data", span));
    };
    items.iter().map(|a| ar_verb(a, scope, span)).collect()
}

/// One atomic representation as the verb it stands for.
fn ar_verb(a: &Array, scope: &Names, span: Span) -> Result<Verb> {
    let ar = crate::gerund::Ar::from_array(a)
        .ok_or_else(|| Error::domain("this is not an atomic representation", span))?;
    let (v, _) = as_verb(ar_frag(&ar, scope, span)?)?;
    Ok(v)
}

/// One atomic representation as the fragment it stands for: a verb, or the
/// noun a modifier takes as an operand.
fn ar_frag(ar: &crate::gerund::Ar, scope: &Names, span: Span) -> Result<Frag> {
    use crate::gerund::Ar;
    match ar {
        Ar::Noun(a) => Ok(Frag::Noun(Expr::Const(a.clone(), span))),
        Ar::Prim(word) => {
            if word == "[:" {
                return Ok(Frag::Verb(VerbFrag::Cap, span));
            }
            // `$:` is a primitive with an inflection rather than a table
            // entry, and a gerund names it: the recursive case of an
            // agenda is `` (base`$:)@.test ``.
            if word == "$:" {
                return Ok(Frag::Verb(VerbFrag::V(Verb::SelfRef), span));
            }
            match verb_for(word) {
                Some(v) => Ok(Frag::Verb(VerbFrag::V(v), span)),
                None => Err(Error::domain(
                    format!("`{word}` is not a verb an atomic representation may name"),
                    span,
                )),
            }
        }
        Ar::Train(parts) => {
            let frags: Result<Vec<Frag>> =
                parts.iter().map(|p| ar_frag(p, scope, span)).collect();
            let mut frags = frags?;
            match frags.len() {
                2 => {
                    let b = frags.pop().expect("two parts");
                    let a = frags.pop().expect("two parts");
                    apply_bident(a, b, scope)
                }
                3 => {
                    let h = frags.pop().expect("three parts");
                    let g = frags.pop().expect("three parts");
                    let f = frags.pop().expect("three parts");
                    apply_fork(f, g, h, scope)
                }
                _ => Err(Error::domain("a train is two or three parts", span)),
            }
        }
        Ar::Derived(word, ops) => {
            let frags: Result<Vec<Frag>> = ops.iter().map(|p| ar_frag(p, scope, span)).collect();
            let mut frags = frags?;
            if let Some(glyph) = adverb(word) {
                if frags.len() != 1 {
                    return Err(Error::domain(format!("{glyph} takes one operand"), span));
                }
                let u = frags.pop().expect("one operand");
                return apply_adverb(u, Frag::Adverb(Modifier::Prim(glyph), span), scope);
            }
            if let Some(glyph) = conjunction(word) {
                if frags.len() != 2 {
                    return Err(Error::domain(format!("{glyph} takes two operands"), span));
                }
                let v = frags.pop().expect("two operands");
                let u = frags.pop().expect("two operands");
                return apply_conj(u, Frag::Conj(Modifier::Prim(glyph), span), v, scope);
            }
            Err(Error::domain(
                format!("`{word}` is not a modifier an atomic representation may name"),
                span,
            ))
        }
    }
}

/// The train a gerund spells: `` `:6 `` groups the verbs from the right,
/// three at a time, which is how J reads a train written out.
fn train_of(vs: Vec<Verb>, span: Span) -> Result<Frag> {
    // Every part is already a verb, so no tine of this train is ever a
    // noun and the empty table settles nothing.
    let scope = Names::default();
    let mut frags: Vec<Frag> =
        vs.into_iter().map(|v| Frag::Verb(VerbFrag::V(v), span)).collect();
    while frags.len() > 3 {
        let h = frags.pop().expect("three or more");
        let g = frags.pop().expect("three or more");
        let f = frags.pop().expect("three or more");
        frags.push(apply_fork(f, g, h, &scope)?);
    }
    match frags.len() {
        1 => Ok(frags.pop().expect("one")),
        2 => {
            let b = frags.pop().expect("two");
            let a = frags.pop().expect("two");
            apply_bident(a, b, &scope)
        }
        _ => {
            let h = frags.pop().expect("three");
            let g = frags.pop().expect("three");
            let f = frags.pop().expect("three");
            apply_fork(f, g, h, &scope)
        }
    }
}

fn apply_fork(f: Frag, g: Frag, h: Frag, scope: &Names) -> Result<Frag> {
    let span = Span::merge(Span::merge(f.span(), g.span()), h.span());
    let f = as_train_part(f, scope)?;
    let (gv, _) = as_verb(as_train_part(g, scope)?)?;
    let (hv, _) = as_verb(as_train_part(h, scope)?)?;
    match f {
        // `[: g h` is g atop h: the left tine produces nothing to fork
        // over. The spelling is kept so that the verb writes itself back
        // out as the fork it was written as.
        Frag::Verb(VerbFrag::Cap, _) => Ok(Frag::Verb(
            VerbFrag::V(Verb::Atop(Box::new(gv), Box::new(hv), AtopForm::Cap)),
            span,
        )),
        Frag::Verb(VerbFrag::V(fv), _) => Ok(Frag::Verb(
            VerbFrag::V(Verb::Fork(Box::new(fv), Box::new(gv), Box::new(hv))),
            span,
        )),
        noun => {
            // A name here is not a verb, or it would have been substituted;
            // a name that holds nothing at all is the undefined name the
            // reference reports rather than a form libjay has yet to learn.
            if let Frag::Name(n, nspan) = &noun
                && !scope.is_noun(n)
            {
                return Err(Error::new(
                    ErrorKind::Value,
                    format!("undefined name: {n}"),
                    Some(*nspan),
                ));
            }
            // The left tine holds the VALUE the name has where the fork is
            // written, not the name: `c =: 5` then `f =: c + ]` displays as
            // `5 + ]` and stays 5 when c is given another value.
            if let Some(arr) = noun_in_scope(&noun, scope) {
                return Ok(Frag::Verb(
                    VerbFrag::V(Verb::NounFork(arr, Box::new(gv), Box::new(hv))),
                    span,
                ));
            }
            let Some(operand) = noun_expr(&noun) else {
                return Err(Error::not_yet("noun forks over a non-literal noun", span));
            };
            let spelling = format!("n {} {}", gv.name(), hv.name());
            let deferred = crate::verb::Deferred {
                operand,
                template: Verb::NounFork(Array::scalar_i64(0), Box::new(gv), Box::new(hv)),
                build: built_noun_fork,
                spelling,
                choices: HashMap::new(),
            };
            Ok(Frag::Verb(VerbFrag::V(Verb::Deferred(Arc::new(deferred))), span))
        }
    }
}

/// An indirect locative's verb is chosen by the locale its operand names,
/// so nothing is built from the value here.
fn built_locative(
    _template: &Verb,
    _a: &Array,
    _span: Span,
    _d: crate::frontend::Rules,
) -> Result<Verb> {
    Err(Error::internal("a locative verb is chosen by its locale, not built"))
}

/// The noun fork a deferred left tine stands for, once the operand the
/// program computes has a value.
fn built_noun_fork(
    template: &Verb,
    a: &Array,
    _span: Span,
    _d: crate::frontend::Rules,
) -> Result<Verb> {
    let Verb::NounFork(_, g, h) = template else {
        return Err(Error::internal("a noun fork's template is not one"));
    };
    Ok(Verb::NounFork(a.clone(), g.clone(), h.clone()))
}

/// A NAME with nothing under it, standing where a train wants a verb.
///
/// The reference reads such a name as a VERB: `{: n` is a hook it writes
/// back out, `1 + n` a fork, and the missing value is reported only when
/// the train is applied — which is what evaluating the name does here. A
/// name that is not part of a train (`n` alone, `n 1`) never reaches this:
/// there the value is what the sentence asked for.
fn undefined_name_verb(name: &str, span: Span) -> Frag {
    let deferred = crate::verb::Deferred {
        operand: Expr::Name(name.to_string(), span),
        template: verb_for("]").expect("`]` is a primitive"),
        build: built_undefined_name,
        spelling: name.to_string(),
        choices: HashMap::new(),
    };
    Frag::Verb(VerbFrag::V(Verb::Deferred(std::sync::Arc::new(deferred))), span)
}

fn built_undefined_name(
    _template: &Verb,
    _operand: &Array,
    span: Span,
    _rules: crate::frontend::Rules,
) -> Result<Verb> {
    Err(Error::not_yet("a train over a name that held a noun where it was written", span))
}

/// The fragment a train should use for `f`: the name read as a verb where
/// it has no value, and the fragment itself otherwise.
fn as_train_part(f: Frag, scope: &Names) -> Result<Frag> {
    let Frag::Name(n, span) = &f else { return Ok(f) };
    if scope.is_noun(n) {
        return Ok(f);
    }
    // An indirect locative's locale is a value, so what part of speech the
    // name has is not known until the program runs; libjay reads one as a
    // noun and names the verb reading as a gap.
    if crate::verb::split_indirect(n).is_some() {
        return Err(Error::not_yet(
            format!("a verb named by the indirect locative `{n}`"),
            *span,
        ));
    }
    Ok(undefined_name_verb(n, *span))
}

fn apply_bident(a: Frag, b: Frag, scope: &Names) -> Result<Frag> {
    let span = Span::merge(a.span(), b.span());
    // A name here is not a verb, or it would have been substituted. Where
    // the word beside it IS one the two make a train, and the reference
    // reads a name with no value as the verb such a train needs. Where it
    // is not, the name is what the sentence asked the value of, and the
    // missing value is what is wrong with it.
    let a = if b.is_real_verb() { as_train_part(a, scope)? } else { a };
    if let Frag::Name(n, nspan) = &a
        && !scope.is_noun(n)
    {
        // An indirect locative's locale is a value, so what part of speech
        // the name has is not known until the program runs. libjay reads
        // one as a noun; a VERB spelled that way is a named gap rather than
        // the syntax error the reading would otherwise produce.
        if crate::verb::split_indirect(n).is_some() {
            return Err(Error::not_yet(
                format!("a verb named by the indirect locative `{n}`"),
                *nspan,
            ));
        }
        return Err(Error::new(
            ErrorKind::Value,
            format!("undefined name: {n}"),
            Some(*nspan),
        ));
    }
    if a.is_real_verb() && b.is_real_verb() {
        let (f, _) = as_verb(a)?;
        let (g, _) = as_verb(b)?;
        return Ok(Frag::Verb(VerbFrag::V(Verb::Hook(Box::new(f), Box::new(g))), span));
    }
    // Two verbs are the only pair J makes a train of. Anything else here —
    // a noun beside a noun, a noun beside a verb, a leftover modifier — is
    // a sentence the language does not have a reading for, which is what
    // the reference calls a syntax error. It is not a queue position.
    if matches!(a, Frag::Verb(VerbFrag::Cap, _)) {
        return Err(Error::parse("`[:` caps a fork; it has no verb of its own", span));
    }
    Err(Error::parse("syntax error", span))
}

fn apply_assign(target: Frag, value: Frag, scope: Scope, names: &Names) -> Result<Frag> {
    let span = Span::merge(target.span(), value.span());
    // `'a b' =. 1 2` names several nouns at once. The left side is a
    // literal string of names, which is why it is read here rather than
    // when the sentence runs.
    if let Some(list) = name_list(&target, names)? {
        let v = match value {
            v if v.is_noun() => as_noun(v)?,
            // The reference shares out a NOUN and refuses anything else.
            Frag::Verb(..) | Frag::Adverb(..) | Frag::Conj(..) => {
                return Err(Error::domain(
                    "a list of names is given a noun, not a verb or a modifier",
                    span,
                ))
            }
            other => return Err(Error::internal(format!("cannot assign {other:?}"))),
        };
        // One name is an ordinary assignment: the whole value is its, and
        // the value's items are not shared out.
        if let [only] = list.as_slice() {
            let name = only.clone();
            return Ok(Frag::Noun(Expr::Assign { name, value: Box::new(v), scope, span }));
        }
        return Ok(Frag::Noun(Expr::AssignMany {
            names: list,
            value: Box::new(v),
            scope,
            by_items: true,
            span,
        }));
    }
    match target {
        // `=.` names a local and `=:` a global; the two differ only inside
        // an explicit definition, which is the only thing with a local
        // frame to name.
        Frag::Name(name, _) => match value {
            // Naming a verb is settled here, at parse time: `parse` records
            // the name and substitutes the verb into later sentences.
            Frag::Verb(VerbFrag::V(verb), _) => Ok(Frag::VerbDef(name, verb, span)),
            Frag::Verb(VerbFrag::Cap, _) => Err(Error::not_yet("assigning [: on its own", span)),
            // Naming a modifier is settled at parse time too: the name
            // stands for the spelling wherever a later sentence writes it.
            Frag::Adverb(m, _) => Ok(Frag::ModDef(name, false, m, span)),
            Frag::Conj(m, _) => Ok(Frag::ModDef(name, true, m, span)),
            v if v.is_noun() => {
                let value = as_noun(v)?;
                Ok(Frag::Noun(Expr::Assign { name, value: Box::new(value), scope, span }))
            }
            other => Err(Error::internal(format!("cannot assign {other:?}"))),
        },
        Frag::Noun(_) => Err(Error::not_yet(
            "assignment to a value that is not a literal list of names",
            span,
        )),
        other => Err(Error::internal(format!("expected an assignment target, got {other:?}"))),
    }
}

/// The names a multiple assignment's left side spells, or None where the
/// target is not a string at all. The words are separated by whitespace and
/// each has to be a name; an empty list is no more a target than `1` is.
fn name_list(target: &Frag, names: &Names) -> Result<Option<Vec<String>>> {
    let Some(a) = as_const(target) else { return Ok(None) };
    let Data::Char(chars) = &a.data else { return Ok(None) };
    if a.rank() > 1 {
        return Ok(None);
    }
    let span = target.span();
    let text = crate::fmt::text_of(chars.as_slice().iter().copied(), names.char_bytes);
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Err(Error::parse("this assignment names nothing", span));
    }
    for w in &words {
        if !is_well_formed_name(w) || verb_for(w).is_some() {
            return Err(Error::parse(format!("ill-formed name: {w}"), span));
        }
    }
    Ok(Some(words.into_iter().map(str::to_string).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dtype::DType;
    use crate::error::ErrorKind;
    use rstest::rstest;

    /// J's shipped rules: no extension, so a literal is bytes.
    fn rules() -> crate::frontend::Rules {
        crate::Dialect::j().rules(crate::Lang::J).expect("J's defaults")
    }

    fn parse_str(src: &str) -> Result<Vec<Expr>> {
        parse(&SourceParts::from_source(src).expect("source parts"), rules())
    }

    /// Parse literal text with no interpolation. `{. ` and `}.` are J words
    /// that `from_source` would read as a hole, so those tests take the
    /// pre-split path instead.
    fn one_literal(src: &str) -> Expr {
        let sp = SourceParts::from_parts(&[src], &[]);
        let mut s = parse(&sp, rules()).unwrap_or_else(|e| panic!("parse of {src:?} failed: {e}"));
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

    // A literal of 0s and 1s is boolean, so the numbers are read out rather
    // than borrowed from an integer buffer.
    fn ints(e: &Expr) -> Vec<i64> {
        konst(e).to_i64_vec().expect("integer data")
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
        // The parentheses are dropped, but the span still covers them, so
        // that a caret under the group underlines something balanced.
        assert_eq!(x.span(), Span::new(0, 7));
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
        // Both dyads take a numbered form as their left argument, and the
        // numbers with no meaning here are refused inside the verb — where
        // the number is known — rather than at the parser.
        let (v, _, _) = dyad_of(&one("2 s: 'a b'"));
        assert_eq!(prim_of(&v).dyad, DyadOp::SymbolForm);
        let (v, _, _) = dyad_of(&one("6 $. 'a b'"));
        assert_eq!(prim_of(&v).dyad, DyadOp::SparseForm);
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
    fn a_verb_on_the_right_of_rank_lends_its_own_ranks() {
        // `u"v` is u at v's own three ranks, so it parses where it once
        // named a gap.
        one("+\"- m");
        one("<\"(<\"1) m");
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
            Verb::Atop(f, g, AtopForm::At) => {
                assert!(matches!(**f, Verb::Reduce(_)), "got {f:?}");
                assert_eq!(prim_of(g).name, ",");
            }
            other => panic!("expected an atop, got {other:?}"),
        }
    }

    #[test]
    fn a_computed_power_count_is_read_at_each_application() {
        let (v, _) = monad_of(&one("+ ^: {n} y"));
        match &v {
            Verb::Deferred(d) => assert_eq!(d.spelling, "+^:n"),
            other => panic!("expected a deferred count, got {other:?}"),
        }
    }

    #[rstest]
    // `u^:_1` names its obverse when it runs and not when it compiles:
    // which obverse it needs depends on whether it is applied monadically
    // or dyadically, and that is not known here. tests/wildhunt.rs pins it.
    // `&.,` needs no obverse — the shape is put back instead — so the
    // verb whose obverse is missing has to be the one on the RIGHT.
    #[case("+: &. (+/ % #) y", "the obverse of")]
    // A cut's kind chooses which function the glyph stands for, so it stays
    // a literal whole number and an unsupported one is a named gap.
    #[case("+/ ;. (1.5) y", "cut")]
        fn other_conjunctions_are_not_supported_yet(#[case] src: &str, #[case] msg: &str) {
        let e = err(src);
        assert_eq!(e.kind, ErrorKind::NotYet);
        assert!(e.msg.contains(msg), "{}", e.msg);
    }

    /// A bond whose noun the program computes is deferred, not refused: the
    /// noun is read where the derived verb is applied. tests/computed.rs
    /// holds what it then answers.
    #[test]
    fn a_computed_bond_noun_is_read_at_each_application() {
        let (v, _) = monad_of(&one("(1 + 2) & , y"));
        match &v {
            Verb::Deferred(d) => assert_eq!(d.spelling, "n&,"),
            other => panic!("expected a deferred bond, got {other:?}"),
        }
        let (v, _) = monad_of(&one(", & (1 + 2) y"));
        match &v {
            Verb::Deferred(d) => assert_eq!(d.spelling, ",&n"),
            other => panic!("expected a deferred bond, got {other:?}"),
        }
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
        // `m&v y` is `m v y` whole. The bond's own rank is infinite — J's
        // `1 2&+ b. 0` reports `_ _ _` — and the verb inside it applies its
        // own ranks to the pair.
        let (v, _) = monad_of(&one("1 & + y"));
        match &v {
            Verb::BondLeft(a, g) => {
                assert_eq!(a.to_i64_vec(), Some(vec![1i64]));
                assert_eq!(prim_of(g).name, "+");
            }
            other => panic!("expected a left bond, got {other:?}"),
        }
        assert_eq!(v.ranks(), [crate::verb::RANK_INF; 3]);
        let (v, _) = monad_of(&one("{. & 2 y"));
        assert!(matches!(v, Verb::BondRight(..)), "got {v:?}");
        assert_eq!(v.ranks(), [crate::verb::RANK_INF; 3]);
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
            Verb::Atop(f, g, AtopForm::Cap) => {
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
    fn a_noun_fork_over_a_parameter_waits_for_its_value() {
        // A tine whose value arrives with the data derives the fork where
        // it is applied rather than where it is written.
        let (v, _) = monad_of(&one("({n} + #) 1 2 3"));
        assert!(matches!(v, Verb::Deferred(_)), "got {v:?}");
    }

    #[test]
    fn cap_is_never_applied_as_a_verb() {
        // `[:` has no meaning of its own; it only caps a fork. Here it is
        // left over beside the result of `# 1 2 3`.
        let e = err("[: # 1 2 3");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.msg.contains("caps a fork"), "{}", e.msg);
    }

    #[test]
    fn two_nouns_side_by_side_are_a_syntax_error() {
        // The reference reads no train here, and neither does libjay.
        let e = err("'ab' 'cd'");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert_eq!(e.msg, "syntax error");
    }

    #[test]
    fn a_sentence_that_is_a_verb_displays_it() {
        assert_eq!(text_of_const(&one("+/ % #")), "+/ % #");
    }

    /// The text a sentence that reduces to an entity yields.
    fn text_of_const(e: &Expr) -> String {
        match e {
            Expr::Const(a, _) => match a.row_major_data() {
                Data::Char(v) => v.as_slice().iter().collect(),
                other => panic!("expected text, got {other:?}"),
            },
            other => panic!("expected a literal, got {other:?}"),
        }
    }

    // ----------------------------------------------------------- assignment

    #[rstest]
    #[case("x =. 5", Scope::Local)]
    #[case("x =: 5", Scope::Global)]
    fn assignment_yields_an_assign_node(#[case] src: &str, #[case] want: Scope) {
        match one(src) {
            Expr::Assign { name, value, scope, span } => {
                assert_eq!(name, "x");
                assert_eq!(ints(&value), vec![5]);
                assert_eq!(scope, want);
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
        assert_eq!(e.kind, ErrorKind::Parse);
        assert_eq!(e.msg, "syntax error");
    }

    #[test]
    fn assignment_names_an_adverb_or_a_conjunction() {
        match one("insert =. /") {
            Expr::ModDef { name, spelling, conjunction, .. } => {
                assert_eq!(name, "insert");
                assert_eq!(spelling, "/");
                assert!(!conjunction);
            }
            other => panic!("expected a modifier definition, got {other:?}"),
        }
        match one("atop =. @") {
            Expr::ModDef { spelling, conjunction, .. } => {
                assert_eq!(spelling, "@");
                assert!(conjunction);
            }
            other => panic!("expected a modifier definition, got {other:?}"),
        }
        // The name is a modifier from there on, so the sentence that uses
        // it parses around it as the glyph would.
        let s = stmts("insert =. /\n+ insert 1 2 3");
        assert!(matches!(s[1], Expr::Monad { verb: Verb::Reduce(_), .. }), "{:?}", s[1]);
    }

    #[test]
    fn a_sentence_that_is_a_modifier_displays_it() {
        let s = stmts("insert =. /\ninsert");
        assert_eq!(text_of_const(s.last().expect("two sentences")), "/");
    }

    #[rstest]
    #[case("f =. 3 : 'y + 1'", None)]
    #[case("f =. 4 : 'x + y'", Some("x"))]
    #[case("f =. {{ y + 1 }}", None)]
    #[case("f =. {{ x + y }}", Some("x"))]
    fn an_explicit_definition_names_a_verb(#[case] src: &str, #[case] left: Option<&str>) {
        match one(src) {
            Expr::VerbDef { name, verb: Verb::Explicit(d), .. } => {
                assert_eq!(name, "f");
                assert_eq!(d.left.as_deref(), left);
                assert_eq!(d.right, "y");
                assert_eq!(d.body.len(), 1);
            }
            other => panic!("expected an explicit verb definition, got {other:?}"),
        }
    }

    /// `13 : '…'` answers a tacit verb, and the sentence that names it
    /// displays the train it translated to.
    #[rstest]
    #[case("f =. 13 : 'y + 1'", "1 + ]")]
    #[case("f =. 13 : 'x + y'", "+")]
    #[case("f =. 13 : '(+/ y) % # y'", "+/ % #")]
    #[case("f =. 13 : '3'", "3:")]
    fn a_tacit_definition_translates_its_body(#[case] src: &str, #[case] want: &str) {
        let s = stmts(&format!("{src}\nf"));
        assert_eq!(text_of_const(s.last().expect("two sentences")), want);
    }

    /// `1 :` and `2 :` say the part of speech; a `{{ }}` leaves it to the
    /// operand names its body uses.
    #[rstest]
    #[case("f =. 1 : 'y + 1'", Some(false))]
    #[case("f =. 2 : 'u v y'", Some(true))]
    #[case("f =. {{ y + 1 }}", None)]
    #[case("f =. {{ u y }}", Some(false))]
    #[case("f =. {{ m + y }}", Some(false))]
    #[case("f =. {{ u v y }}", Some(true))]
    #[case("f =. {{ v y }}", Some(true))]
    #[case("f =. {{ n + y }}", Some(true))]
    #[case("f =. {{ u n y }}", Some(true))]
    #[case("f =. {{)a\nu y\n}}", Some(false))]
    #[case("f =. {{)c\nu v y\n}}", Some(true))]
    #[case("f =. {{)v\ny\n}}", None)]
    fn an_explicit_definitions_part_of_speech(#[case] src: &str, #[case] want: Option<bool>) {
        match (one(src), want) {
            (Expr::ModDef { name, conjunction, .. }, Some(conj)) => {
                assert_eq!(name, "f");
                assert_eq!(conjunction, conj, "{src:?}");
            }
            (Expr::VerbDef { name, .. }, None) => assert_eq!(name, "f"),
            (other, _) => panic!("expected {want:?} for {src:?}, got {other:?}"),
        }
    }

    #[test]
    fn a_control_word_outside_a_definition_is_a_parse_error() {
        let e = err("if. 1 do. 2 end.");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.msg.contains("only meaningful inside an explicit definition"), "{}", e.msg);
    }

    #[test]
    fn multiple_assignment_names_each() {
        match one("'a b' =. 1 2") {
            Expr::AssignMany { names, by_items, .. } => {
                assert_eq!(names, ["a", "b"]);
                assert!(by_items);
            }
            other => panic!("expected a distributed assignment, got {other:?}"),
        }
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
        let e = parse(&sp, rules()).expect("parse").pop().expect("one sentence");
        let (_, x, y) = dyad_of(&e);
        assert!(matches!(x, Expr::Param(0, _)));
        let (_, x2, y2) = dyad_of(&y);
        assert!(matches!(x2, Expr::Param(1, _)));
        assert!(matches!(y2, Expr::Param(0, _)));
    }

    #[rstest]
    #[case("3j4", 3.0, 4.0)]
    #[case("_1j_2", -1.0, -2.0)]
    #[case("1e1j2", 10.0, 2.0)]
    #[case("2ad90", 0.0, 2.0)]
    #[case("1ad180", -1.0, 0.0)]
    fn complex_literals(#[case] src: &str, #[case] re: f64, #[case] im: f64) {
        let a = konst(&one(src));
        assert_eq!(a.dtype(), DType::Complex);
        let z = a.as_complex_slice().expect("complex data")[0];
        assert!((z[0] - re).abs() < 1e-12 && (z[1] - im).abs() < 1e-12, "{z:?}");
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
        let a = konst(&parse(&sp, rules()).expect("parse")[0]);
        assert_eq!(a.data, Data::Char(vec!['{', 'a', '}'].into()));
    }

    #[test]
    fn parts_of_one_sentence_lex_across_a_hole() {
        // The t-string path: literal parts with a hole between them.
        let sp = SourceParts::from_parts(&["1 + ", " * 2"], &["v"]);
        assert_eq!(sp.display, "1 + {v} * 2");
        let e = parse(&sp, rules()).expect("parse").pop().expect("one sentence");
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
        let e = err("1 [. 2");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert_eq!(e.msg, "unknown word: [.");
        assert_eq!(e.span, Some(Span::new(2, 4)));
    }

    #[test]
    fn an_inflected_unknown_word_is_reported_whole() {
        let e = err("1 ]: 2");
        assert_eq!(e.msg, "unknown word: ]:");
        assert_eq!(e.span, Some(Span::new(2, 4)));
    }

    /// The exact suffixes read; the forms that spell no number do not.
    #[rstest]
    #[case("1.5x", 0, 4)]
    #[case("1e10x", 0, 5)]
    fn a_fractional_extended_literal_is_ill_formed(
        #[case] src: &str,
        #[case] start: usize,
        #[case] end: usize,
    ) {
        let e = err(src);
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.msg.contains("invalid number"), "{}", e.msg);
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
        // The parenthesis itself is what is wrong, so that is what the
        // span covers.
        let e = err("(1 + 2");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.msg.contains("no closing"), "{}", e.msg);
        assert_eq!(e.span, Some(Span::new(0, 1)));
    }

    #[test]
    fn a_stray_right_parenthesis_is_a_syntax_error() {
        let e = err("1 + 2)");
        assert_eq!(e.kind, ErrorKind::Parse);
        assert!(e.msg.contains("no opening"), "{}", e.msg);
        assert_eq!(e.span, Some(Span::new(5, 6)));
    }

    #[test]
    fn the_error_of_a_later_sentence_points_at_that_sentence() {
        let e = err("1 + 2\n3 [. 4");
        assert_eq!(e.span, Some(Span::new(8, 10)));
    }
}
