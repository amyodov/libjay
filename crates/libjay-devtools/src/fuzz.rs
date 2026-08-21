//! Composed expressions, for hunting rather than for the corpus.
//!
//! The `gen` generator draws one verb over one well-formed noun; this one
//! draws trees — verbs over verbs, trains, rank and power, reductions over
//! rank-3 arrays, boxes and empties and the numeric edges — which is where
//! a one-verb corpus stops looking. What it produces is not appended to
//! anything: `fuzz` prints the expressions, and `fuzz --compare` runs both
//! libjay and the oracle over them and reports where they part. A line
//! worth keeping is moved into `corpus/<lang>/fuzz_found.txt` by hand,
//! which is what makes it a regression rather than a run of a generator.

use libjay_testkit::{ErrorKind, Lang};

/// The seed a fuzz run uses when none is given.
pub const DEFAULT_SEED: u64 = 0x243F6A8885A308D3;

/// A small xorshift, so a run is reproducible without any clock access.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(if seed == 0 { 0x9E3779B97F4A7C15 } else { seed })
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

/// `count` expressions of the language, each a tree up to `depth` deep.
pub fn fuzz(lang: Lang, count: usize, seed: u64, depth: u32) -> Vec<String> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let d = 1 + (rng.below(depth.max(1) as usize) as u32);
        let expr = match lang {
            Lang::J => j_expr(&mut rng, d),
            Lang::Apl => apl_expr(&mut rng, d),
        };
        out.push(expr);
    }
    out
}

// ---------------------------------------------------------------------
// J
// ---------------------------------------------------------------------

/// Nouns small enough that no composition of them can ask for a large
/// allocation: every length here is under ten, so even a reshape whose
/// left argument is a computed value stays bounded.
const J_NOUNS: &[&str] = &[
    "0",
    "5",
    "_3",
    ",5",
    "1 2 3",
    "3 1 4 1 5",
    "_2 0 2",
    "2 7 1 8",
    "i. 5",
    "i. 3 4",
    "i. 2 3 4",
    "i. 0",
    "0 3 $ 0",
    "''",
    "'abc'",
    "'hello'",
    "2 3 $ i. 6",
    "3 3 $ 1 0 0 0 1 0 0 0 1",
    "1.5 2.5 _0.5",
    "2.0 4.0 6.0",
    "0.1 0.2 0.3",
    "0 1 1 0",
    "1;2 3",
    "(<i. 2 2);5",
    "a:",
    "<'abc'",
    "1;2;3",
    "2 2 $ 1;2;3;4",
    "3j4 1j_1",
    "1r2 1r3",
    "123x",
    "9223372036854775806",
    "_9223372036854775806",
    "1e_15",
    "1 1.0000000000001",
    "_ __ 0",
    "'a'",
    "1 2 3x",
    "_5x",
];

/// Scalar dyads: safe on any pair of numbers, whatever the composition
/// above them decided to hand over.
/// `^` is not here: an extended-precision base with a large exponent is an
/// answer with more digits than a machine has. It is drawn against a small
/// literal exponent instead.
const J_DYADS: &[&str] =
    &["+", "-", "*", "%", "<.", ">.", "|", "=", "~:", "<", ">", "<:", ">:", "*.", "+."];

/// Dyads with structure in them, where the left argument's own shape is
/// part of the meaning. The ones whose left argument is a SIZE — reshape,
/// take, replicate, window, cut — are not here: a computed left argument
/// would ask for an allocation the size of whatever the tree below it
/// happened to produce, so those forms are drawn separately with a small
/// literal on the left.
const J_STRUCT_DYADS: &[&str] = &[
    "}.", "|.", ",", ",.", ",:", "#.", "#:", "e.", "i.", "i:", "E.", "-.", "{", "/:", "\\:",
    "-:", "C.", "A.", ";", "!", "j.", "o.", "%:", "^.", "p.", "I.", "=", "~:",
];

/// Monads. `?` and `?.` are left out: a random answer has no oracle.
/// `i.`, `i:` and `I.` are left out too — each turns a VALUE into a length,
/// so `i. 9223372036854775806` is an allocation, not an expression. They
/// appear in the leaves, applied to small literals.
const J_MONADS: &[&str] = &[
    "-", "+", "*", "%", "|", "<.", ">.", "#", "$", ",", ",.", ",:", "|:", "|.", "~.", "~:", "{.",
    "{:", "}:", "<:", ">:", "+:", "*:", "-.", "-:", "/:", "\\:", "#.", "#:", "\":", "!",
    "+.", "*.", "=", "<", ">", ";", "L.", "o.", "%:", "^.", "^", "{::", "j.", "r.",
    "p.", "p..", "]", "[",
];

/// Folds that stay in the reals over any small integers.
const J_FOLDS: &[&str] = &["+", "*", "<.", ">.", "|", ",", "-", "%"];

const J_RANKS: &[&str] = &["0", "1", "2", "_", "_1", "_2", "0 1", "1 1", "1 0", "1 0 _", "2 _"];

/// A verb phrase: a primitive, or a train, or a primitive under a
/// conjunction, down to `depth` levels of nesting.
fn j_verb(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 {
        return rng.pick(J_MONADS).to_string();
    }
    let d = depth - 1;
    match rng.below(14) {
        0 | 1 => rng.pick(J_MONADS).to_string(),
        2 => format!("{}/", rng.pick(J_FOLDS)),
        3 => format!("{}/\\", rng.pick(J_FOLDS)),
        4 => format!("{}/\\.", rng.pick(J_FOLDS)),
        5 => format!("{}\"{}", j_verb(rng, d), rng.pick(J_RANKS)),
        6 => format!("({} {} {})", j_verb(rng, d), rng.pick(J_DYADS), j_verb(rng, d)),
        7 => format!("({} {})", j_verb(rng, d), j_verb(rng, d)),
        8 => format!("{}@:{}", j_verb(rng, d), j_verb(rng, d)),
        9 => format!("{}@{}", j_verb(rng, d), j_verb(rng, d)),
        10 => format!("{}&:{}", j_verb(rng, d), j_verb(rng, d)),
        11 => format!("{}&.>", j_verb(rng, d)),
        12 => format!("([: {} {})", j_verb(rng, d), j_verb(rng, d)),
        _ => format!("{}&{}", rng.pick(&["1", "2", "_1", "0"]), rng.pick(J_DYADS)),
    }
}

/// A small integer literal, in J's spelling.
fn j_int(rng: &mut Rng, lo: i64, hi: i64) -> String {
    let v = lo + rng.below((hi - lo + 1) as usize) as i64;
    if v < 0 { format!("_{}", -v) } else { v.to_string() }
}

fn j_noun(rng: &mut Rng) -> String {
    rng.pick(J_NOUNS).to_string()
}

fn j_expr(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 {
        return j_noun(rng);
    }
    let d = depth - 1;
    // Every operand is parenthesised. A verb phrase can end in a number —
    // the rank of `u"1`, the count of `u^:2`, the fret of `u;.1` — and a
    // bare numeric noun after one would be read as more of that number,
    // which makes a sentence the generator did not mean.
    let arg = |rng: &mut Rng| format!("({})", j_expr(rng, d));
    match rng.below(21) {
        0..=2 => format!("{} {}", j_verb(rng, d), arg(rng)),
        3..=4 => format!("({}) {} {}", j_expr(rng, d), rng.pick(J_DYADS), arg(rng)),
        5 => format!("({}) {} {}", j_expr(rng, d), rng.pick(J_STRUCT_DYADS), arg(rng)),
        // A window, including the sizes that are edges of the definition:
        // zero, negative (non-overlapping chunks), and longer than the
        // argument.
        6 => format!("{} {}/\\ {}", j_int(rng, -4, 9), rng.pick(J_FOLDS), arg(rng)),
        7 => format!("{} {}/\\. {}", j_int(rng, -4, 9), rng.pick(J_FOLDS), arg(rng)),
        // A table.
        8 => format!("({}) {}/ {}", j_expr(rng, d), rng.pick(J_FOLDS), arg(rng)),
        // Rank, applied to a dyad as well as a monad.
        9 => format!("{}\"{} {}", j_verb(rng, d), rng.pick(J_RANKS), arg(rng)),
        10 => format!(
            "({}) {}\"{} {}",
            j_expr(rng, d),
            rng.pick(J_DYADS),
            rng.pick(J_RANKS),
            arg(rng)
        ),
        // Power, including the negative count that asks for the obverse.
        11 => format!(
            "{}^:{} {}",
            j_verb(rng, d),
            rng.pick(&["0", "1", "2", "3", "_1"]),
            arg(rng)
        ),
        // The key and the oblique.
        12 => format!("({}) {}/. {}", j_expr(rng, d), rng.pick(J_FOLDS), arg(rng)),
        // The cut, both valences. The left argument is a literal: it is a
        // size or a fret list, and a computed one is an allocation.
        13 => format!(
            "{} {};.{} {}",
            rng.pick(J_SIZES),
            j_verb(rng, d),
            rng.pick(&["0", "1", "2", "_1", "_2", "3", "_3"]),
            arg(rng)
        ),
        14 => format!(
            "{};.{} {}",
            j_verb(rng, d),
            rng.pick(&["1", "2", "_1", "_2", "0"]),
            arg(rng)
        ),
        15 => format!("{}&.> {}", j_verb(rng, d), arg(rng)),
        16 => format!("{} }} {}", j_int(rng, -3, 3), arg(rng)),
        // The verbs whose left argument decides how much there is to
        // allocate: always a small literal.
        17 => format!("{} $ {}", rng.pick(J_SHAPES), arg(rng)),
        18 => format!("{} # {}", rng.pick(J_SIZES), arg(rng)),
        19 => format!("{} {{. {}", rng.pick(J_SIZES), arg(rng)),
        _ => format!("({}) ^ {}", j_expr(rng, d), rng.pick(&["2", "3", "0", "_1", "0.5", "_2"])),
    }
}

/// Left arguments for the size verbs: small, and covering the empty, the
/// negative and the boolean-mask readings.
const J_SIZES: &[&str] = &["0", "1", "2", "3", "_2", "1 0 1", "0 0", "2 0 1", "_1 2", "2 2"];

/// Shapes for reshape: small, including the empty and the zero axis.
const J_SHAPES: &[&str] = &["2 3", "3", "0", "0 3", "2 2 2", "1", "''", "4", "3 1"];

// ---------------------------------------------------------------------
// APL
// ---------------------------------------------------------------------

const APL_NOUNS: &[&str] = &[
    "0",
    "5",
    "¯3",
    ",5",
    "1 2 3",
    "3 1 4 1 5",
    "¯2 0 2",
    "⍳5",
    "2 3⍴⍳6",
    "2 3 4⍴⍳24",
    "⍳0",
    "0 3⍴0",
    "''",
    "'abc'",
    "'hello'",
    "3 3⍴1 0 0 0 1 0 0 0 1",
    "1.5 2.5 ¯0.5",
    "2.0 4.0 6.0",
    "0.1 0.2 0.3",
    "0 1 1 0",
    "(1 2)(3 4 5)",
    "⊂⍳3",
    "1 'a'",
    "2 2⍴1 2 3 4",
    "'a'",
    "1 1.0000000000001",
    "9223372036854775806",
];

/// Scalar dyads. `÷` is here and its zero divisor with it: the divergence
/// corpus says what the two do about it, and the fuzzer should see it.
const APL_DYADS: &[&str] =
    &["+", "-", "×", "÷", "*", "⌈", "⌊", "|", "=", "≠", "<", "≤", ">", "≥", "∧", "∨"];

/// `⍴` and `↑` are not here: a computed left argument would ask for an
/// allocation the size of whatever the tree below it produced. Both are
/// drawn against a small literal shape instead.
const APL_STRUCT_DYADS: &[&str] = &[
    ",", "⍪", "↓", "⌽", "⊖", "⍳", "∊", "≢", "≡", "∪", "∩", "~", "⊥", "⊤", "⍷", "⊂", "⍸", "⌷",
    "!", "○", "⍟", "⊃", "=", "≠",
];

/// `⍳` and `⍸` are not here: each turns a VALUE into a length, so `⍳` of a
/// large number is an allocation rather than an expression. They appear in
/// the leaves, applied to small literals.
const APL_MONADS: &[&str] = &[
    "-", "+", "×", "÷", "|", "⌈", "⌊", "⍴", ",", "⍪", "⍉", "⌽", "⊖", "≢", "≡", "⍕", "∊", "∪",
    "⊂", "⊃", "↑", "!", "○", "⍟", "*", "⍋", "⍒", "≠", "⊢", "⊣",
];

const APL_FOLDS: &[&str] = &["+", "×", "⌈", "⌊", "|", "-", "÷"];

const APL_RANKS: &[&str] = &["0", "1", "2", "0 1", "1 1", "1 0"];

/// A function phrase. Only the operators GNU APL has are drawn: `∘`, `⍥`,
/// `⍛`, `f⍤g` and `⌸` are Dyalog's and have no oracle, so fuzzing them
/// would report the oracle's refusal over and over.
///
/// The left operand of `⍤` and `⍣` is parenthesised even when it is one
/// glyph. GNU APL binds `+/⍤1` as `+(/⍤1)` and refuses it, so an unbracketed
/// derived operand would make the fuzzer report a parse it never meant to
/// test; the divergence itself is one recorded line in `divergences.txt`.
fn apl_fn(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 {
        return rng.pick(APL_MONADS).to_string();
    }
    let d = depth - 1;
    match rng.below(9) {
        0..=2 => rng.pick(APL_MONADS).to_string(),
        3 => format!("{}/", rng.pick(APL_FOLDS)),
        4 => format!("{}⌿", rng.pick(APL_FOLDS)),
        5 => format!("{}\\", rng.pick(APL_FOLDS)),
        6 => format!("{}⍨", rng.pick(APL_DYADS)),
        7 => format!("{}¨", apl_fn(rng, d)),
        _ => format!("(({})⍤{})", apl_fn(rng, d), rng.pick(APL_RANKS)),
    }
}

fn apl_int(rng: &mut Rng, lo: i64, hi: i64) -> String {
    let v = lo + rng.below((hi - lo + 1) as usize) as i64;
    if v < 0 { format!("¯{}", -v) } else { v.to_string() }
}

fn apl_expr(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 {
        return rng.pick(APL_NOUNS).to_string();
    }
    let d = depth - 1;
    // Parenthesised for the same reason as J's: `⍤1` and `⍣2` end in a
    // number, and a numeric noun after one would extend it.
    let arg = |rng: &mut Rng| format!("({})", apl_expr(rng, d));
    match rng.below(18) {
        0..=2 => format!("{} {}", apl_fn(rng, d), arg(rng)),
        3..=4 => format!("({}){}{}", apl_expr(rng, d), rng.pick(APL_DYADS), arg(rng)),
        5..=6 => {
            format!("({}){}{}", apl_expr(rng, d), rng.pick(APL_STRUCT_DYADS), arg(rng))
        }
        7 => format!("{}/{}", rng.pick(APL_FOLDS), arg(rng)),
        8 => format!("{}⌿{}", rng.pick(APL_FOLDS), arg(rng)),
        9 => format!("{}\\{}", rng.pick(APL_FOLDS), arg(rng)),
        10 => format!("{}⍀{}", rng.pick(APL_FOLDS), arg(rng)),
        11 => format!("({})∘.{}{}", apl_expr(rng, d), rng.pick(APL_DYADS), arg(rng)),
        12 => format!("({})⍣{} {}", apl_fn(rng, d), rng.below(4), arg(rng)),
        13 => format!("{}⌽{}", apl_int(rng, -3, 3), arg(rng)),
        // The shape and take verbs, and replicate, with a literal on the
        // left.
        14 => format!("{}⍴{}", rng.pick(APL_SHAPES), arg(rng)),
        15 => format!("{}↑{}", rng.pick(APL_SIZES), arg(rng)),
        16 => format!("{}/{}", rng.pick(APL_SIZES), arg(rng)),
        _ => format!("{}⌿{}", rng.pick(APL_SIZES), arg(rng)),
    }
}

// ---------------------------------------------------------------------
// Triage
// ---------------------------------------------------------------------

/// How the two answers to one sentence relate.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Both answered, and the answers match; or both refused.
    Agree,
    /// Both answered, and the answers differ.
    Differ,
    /// libjay refused something the oracle answered, and the refusal names
    /// a gap — a promise, not a wrong answer.
    Gap,
    /// libjay refused something the oracle answered, for a reason that is
    /// not a gap.
    WeRefuse,
    /// libjay answered something the oracle refused.
    TheyRefuse,
    /// The oracle never finished, so there is nothing to compare against.
    Unfinished,
    /// libjay panicked. Always a bug: a refusal is a diagnostic, a panic is
    /// a crash.
    Panicked,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Agree => "agree",
            Verdict::Differ => "differ",
            Verdict::Gap => "gap",
            Verdict::WeRefuse => "we-refuse",
            Verdict::TheyRefuse => "they-refuse",
            Verdict::Unfinished => "unfinished",
            Verdict::Panicked => "panic",
        }
    }

    /// A verdict worth a human's attention.
    pub fn is_mismatch(self) -> bool {
        !matches!(self, Verdict::Agree | Verdict::Unfinished)
    }

    /// Whether the two answers were compared at all.
    pub fn is_compared(self) -> bool {
        !matches!(self, Verdict::Unfinished)
    }
}

/// Compare libjay's answer with the oracle's.
pub fn triage(lang: Lang, ours: &libjay_testkit::eval::Answer, theirs: Option<&str>) -> Verdict {
    use libjay_testkit::eval::Answer;
    match (ours, theirs) {
        (Answer::Value(o), Some(t)) => {
            if libjay_testkit::compare::outputs_match(lang, o, t) {
                Verdict::Agree
            } else {
                Verdict::Differ
            }
        }
        (Answer::Value(_) | Answer::NoValue, None) => Verdict::TheyRefuse,
        (Answer::NoValue, Some(_)) => Verdict::WeRefuse,
        (Answer::Refused(_), None) => Verdict::Agree,
        (Answer::Refused(e), Some(_)) => {
            if matches!(e.kind, ErrorKind::NotYet | ErrorKind::Language) {
                Verdict::Gap
            } else {
                Verdict::WeRefuse
            }
        }
    }
}

/// Shapes and sizes for the verbs whose left argument is an amount: small,
/// and covering the empty axis, the negative take, and the boolean mask a
/// replicate reads.
const APL_SHAPES: &[&str] = &["2 3", "3", "0", "0 3", "2 2 2", "1", "4", "3 1"];
const APL_SIZES: &[&str] = &["0", "1", "2", "3", "¯2", "1 0 1", "0 0", "2 0 1", "¯1 2", "2 2"];
