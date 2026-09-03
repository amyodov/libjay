//! Composed expressions, for hunting rather than for the corpus.
//!
//! The `gen` generator draws one verb over one well-formed noun; this one
//! draws trees — verbs over verbs, trains, rank and power, reductions over
//! rank-3 arrays, boxes and empties and the numeric edges — which is where
//! a one-verb corpus stops looking. What it produces is not appended to
//! anything: `fuzz` prints the expressions, in corpus spelling and with the
//! `@ io=` directive an origin-0 line needs, and `fuzz --compare` runs both
//! libjay and the oracle over them and reports where they part. A line
//! worth keeping is moved into `corpus/<lang>/fuzz_found.txt` by hand,
//! which is what makes it a regression rather than a run of a generator.

use libjay_testkit::{ErrorKind, Lang};

/// The seed a fuzz run uses when none is given.
pub const DEFAULT_SEED: u64 = 0x243F6A8885A308D3;

/// Which generation of the grammar drew the expressions. Generation 1 was
/// the original tree; generation 2 (2026-08-22) added the J conjunctions the
/// tree never composed (`^:` with a negative, listed or boxed count, `L:`,
/// `S:`, `&.` with a named obverse, `&.:`, `!.`, `;:`), deeper and emptier
/// leaves in both pools, tolerance-edge pairs fed to every dyad, APL bracket
/// axis, and J ranks of two and three elements. A run's findings are only
/// comparable with another run's when the generation matches.
pub const GENERATION: u32 = 2;

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

/// One sentence to try, and the index origin to read it under. The origin
/// is a property of the dialect rather than of the text, so it travels
/// beside the expression: libjay takes it as `⎕IO` and the oracle prepends
/// `⎕IO←0⋄`. J has no index origin and its probes always carry 1, which
/// both sides ignore.
pub struct Probe {
    pub expr: String,
    pub io: u8,
}

/// One APL probe in this many is drawn at index origin 0. Origin-0 code is
/// ordinary APL, and a comparison that never leaves origin 1 can find no
/// disagreement about the origin at all; drawing it rarely keeps the bulk
/// of a run on the origin most of the corpus uses.
const IO_ZERO_IN: usize = 8;

/// `count` expressions of the language, each a tree up to `depth` deep.
pub fn fuzz(lang: Lang, count: usize, seed: u64, depth: u32) -> Vec<Probe> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let d = 1 + (rng.below(depth.max(1) as usize) as u32);
        let expr = match lang {
            Lang::J => j_expr(&mut rng, d),
            Lang::Apl => apl_expr(&mut rng, d),
        };
        let io = match lang {
            Lang::J => 1,
            Lang::Apl => u8::from(rng.below(IO_ZERO_IN) != 0),
        };
        out.push(Probe { expr, io });
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

/// The leaves generation 2 added: an empty of every type and of ranks 0 to
/// 3, and nests deeper than one level. Half the register's open weight sits
/// on empties and prototypes, and the original pool reached them with three
/// leaves; these make a fill cell something the tree meets rather than
/// stumbles on.
const J_NOUNS_EXTRA: &[&str] = &[
    "i. 0 0",
    "i. 0 0 3",
    "i. 2 0 3",
    "0 0 $ 0",
    "2 0 3 $ 0",
    "0 $ 0.5",
    "0 $ 'a'",
    "0 $ a:",
    "0 $ <0",
    "0 $ 3j4",
    "0 $ 1r2",
    "0 $ 123x",
    "0 0 $ ''",
    "'' $ 0",
    "0 $ 1 2 3",
    "1 0 1 $ 5",
    "(i. 0);1",
    "<i. 0",
    "<''",
    "<a:",
    "<<1 2",
    "((1;2);3);4",
    "1;'ab';(<i. 2 2)",
    "2 2 $ <i. 2 2",
    "3 $ a:",
    "2 2 $ (<1 2);(<'ab');(<i. 0);<0",
];

/// How often a leaf is drawn from the original pool rather than the extra
/// one. Two draws in three keeps the coverage the recorded findings came
/// from intact; the third reaches the empties and the nests.
const CORE_LEAF_IN: usize = 3;

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

/// Ranks of two and three elements, drawn on their own arm. A three-element
/// rank gives the monadic case a rank of its own and the two dyadic cases
/// different ones, which is the part of the rank conjunction the one-element
/// draws never reach.
const J_LONG_RANKS: &[&str] =
    &["0 1 2", "1 0 _", "2 1 0", "_ 1 1", "0 0 1", "1 _ 0", "_1 0 1", "2 _ _1", "0 2", "1 _1"];

/// Counts for `^:` past the small non-negative ones the original tree drew.
/// A negative count asks for the obverse, a listed count asks for several
/// applications at once, and a boxed count asks for the intermediate
/// results. `_` and `a:` are deliberately absent: both converge, converging
/// is unbounded, and a generator that can hang has no oracle.
const J_POWERS: &[&str] =
    &["_1", "_2", "_3", "(<0 1 2)", "(<_1 1)", "(<2)", "(<0)", "(_1 0 1)", "(2 3)", "(0 1)"];

/// Levels for `L:` and `S:`. Both count boxing levels, and a negative level
/// counts from the leaves rather than from the root.
const J_LEVELS: &[&str] = &["0", "1", "2", "_1", "_2", "_"];

/// Verbs whose obverse is worth asking for by name. `&.` runs one and then
/// undoes it, so an unknown obverse is a refusal rather than a wrong answer,
/// and the register's obverse gap is exactly a list of these.
const J_UNDER: &[&str] =
    &[">", "<", "+:", "-:", "%:", "^.", "|.", "|:", "#.", "#:", ",", "{.", "*:", "j.", "o."];

/// Verbs that read a fit: `!.` gives a comparison its tolerance, a fill its
/// fill element, and a base conversion its rounding.
const J_FIT_MONADS: &[&str] = &["~.", "~:", "<.", ">.", "%:", "^.", "|", "=", "{.", ",", "#.", "q:"];
const J_FIT_DYADS: &[&str] =
    &["=", "~:", "-:", "i.", "e.", "E.", "<.", ">.", "|", "#.", "+", "*", ",", "{.", "#", "i:"];

/// Fits. Zero and one are the fill elements a take or a catenate wants;
/// the small ones are the tolerances a comparison wants.
const J_FITS: &[&str] = &["0", "1", "2", "0.5", "1e_9", "1e_13", "1e_3", "_1", "'z'"];

/// Left arguments for `;:`. The dyad reads a boxed state machine and the
/// integers are what a machine description is built out of; both refusals
/// and answers are worth seeing, since the two sides disagree about which
/// it is.
const J_WORDS_LEFT: &[&str] = &["0", "1", "2", "3", "4", "5", "6", "1 2 3", "(<1 2);<3 4", "a:"];

/// Pairs that straddle a comparison tolerance: equal under the default
/// tolerance and unequal exactly, or the reverse, or on the boundary where
/// a double stops being able to tell. They are fed to every dyad rather
/// than only to the comparisons, since residue, greatest common divisor,
/// base conversion and the grades all consult the tolerance too.
const J_TOLERANCE_PAIRS: &[(&str, &str)] = &[
    ("1", "1.0000000000000002"),
    ("1", "1.0000000000001"),
    ("1", "1.00000000000001"),
    ("1", "0.9999999999999999"),
    ("0.3", "0.1 + 0.2"),
    ("100000000", "100000000.00000001"),
    ("2147483648", "2147483647.9999998"),
    ("4503599627370496", "4503599627370497"),
    ("9007199254740992", "9007199254740993"),
    ("_1", "_1.0000000000001"),
    ("1e_15", "0"),
    ("1e_13", "0"),
    ("0.5", "0.49999999999999994"),
    ("3", "3 - 1e_13"),
    ("1 2 3", "1 2 3 + 1e_14"),
    ("1e10", "1e10 + 1e_4"),
];

/// The dyads a tolerance pair is safe to reach. Every one of these reads
/// its left argument as a value; the verbs that read it as an amount would
/// turn 9007199254740992 into an allocation.
const J_TOLERANCE_DYADS: &[&str] = &[
    "+", "-", "*", "%", "<.", ">.", "|", "=", "~:", "<", ">", "<:", ">:", "*.", "+.", "|.", ",",
    ",.", ",:", "e.", "i.", "i:", "E.", "-:", "#.", "#:", "j.", "o.", "!", "I.", "%:", "^.",
];

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
    if rng.below(CORE_LEAF_IN) != 0 {
        rng.pick(J_NOUNS).to_string()
    } else {
        rng.pick(J_NOUNS_EXTRA).to_string()
    }
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
    // Arms 0 to 20 are generation 1's, unchanged and in their original
    // proportion to one another; 21 to 29 are generation 2's. Widening the
    // draw rather than reweighting the old arms is what keeps the coverage
    // the recorded findings came from.
    match rng.below(30) {
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
        20 => format!("({}) ^ {}", j_expr(rng, d), rng.pick(&["2", "3", "0", "_1", "0.5", "_2"])),
        // Power past the counts the original tree drew: the obverse, a list
        // of counts, and the boxed count that keeps the intermediates.
        21 => format!("{} ^:{} {}", j_verb(rng, d), rng.pick(J_POWERS), arg(rng)),
        // Level: `L:` runs a verb at a boxing depth, `S:` collects the
        // results from one.
        22 => format!("{} L:{} {}", j_verb(rng, d), rng.pick(J_LEVELS), arg(rng)),
        23 => {
            if rng.below(2) == 0 {
                format!("{} S:{} {}", j_verb(rng, d), rng.pick(J_LEVELS), arg(rng))
            } else {
                format!(
                    "({}) {} L:{} {}",
                    j_expr(rng, d),
                    rng.pick(J_DYADS),
                    rng.pick(J_LEVELS),
                    arg(rng)
                )
            }
        }
        // Under, at infinite rank and at the operand's own rank. Both ask
        // for an obverse, which is a table with holes in it.
        24 => {
            if rng.below(2) == 0 {
                format!("{} &.:{} {}", j_verb(rng, d), rng.pick(J_UNDER), arg(rng))
            } else {
                format!(
                    "({}) {} &.:{} {}",
                    j_expr(rng, d),
                    rng.pick(J_DYADS),
                    rng.pick(J_UNDER),
                    arg(rng)
                )
            }
        }
        25 => format!("{} &.{} {}", j_verb(rng, d), rng.pick(J_UNDER), arg(rng)),
        // Fit: a tolerance for a comparison, a fill for a take.
        26 => {
            if rng.below(2) == 0 {
                format!("{} !.{} {}", rng.pick(J_FIT_MONADS), rng.pick(J_FITS), arg(rng))
            } else {
                format!(
                    "({}) {} !.{} {}",
                    j_expr(rng, d),
                    rng.pick(J_FIT_DYADS),
                    rng.pick(J_FITS),
                    arg(rng)
                )
            }
        }
        // Words, both valences.
        27 => {
            if rng.below(2) == 0 {
                format!(";: {}", arg(rng))
            } else {
                format!("({}) ;: {}", rng.pick(J_WORDS_LEFT), arg(rng))
            }
        }
        // A tolerance-edge pair, fed to a dyad that reads its arguments as
        // values.
        28 => {
            let (left, right) = J_TOLERANCE_PAIRS[rng.below(J_TOLERANCE_PAIRS.len())];
            format!("({left}) {} ({right})", rng.pick(J_TOLERANCE_DYADS))
        }
        // A rank of two or three elements, where the three cases of the
        // verb are given ranks of their own.
        _ => format!(
            "({}) {} \"{} {}",
            j_expr(rng, d),
            rng.pick(J_DYADS),
            rng.pick(J_LONG_RANKS),
            arg(rng)
        ),
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

/// The leaves generation 2 added. The original pool held two nested values
/// and one empty, so a prototype was something the tree brushed against;
/// these give it an empty of every type and rank, a nest three deep, a
/// nested empty, a mixed nest and a nested matrix.
const APL_NOUNS_EXTRA: &[&str] = &[
    "⍬",
    "0⍴0",
    "0 0⍴0",
    "0 0 0⍴0",
    "2 0 3⍴0",
    "0 3⍴''",
    "0 0⍴''",
    "0⍴0.5",
    "0⍴'a'",
    "0⍴⊂⍳3",
    "3 0⍴⍳0",
    "0 2⍴⊂⍳3",
    "⊂⊂⍳3",
    "⊂⍬",
    "⊂''",
    "(1(2 3))(4 5)",
    "((1 2)(3 4))(5)",
    "(1 2)('ab')",
    "1 'a' (1 2)",
    "2 2⍴(1 2)(3 4)(5)(⍳0)",
    "2 2⍴⊂'ab'",
    "⊂2 2⍴⍳4",
    "3⍴⊂⍳3",
    "(⍳0)(⍳3)",
    "(⊂⍳0)(⊂⊂1 2)",
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

/// Axes for the bracket forms. The halves are the laminate axes, which are
/// an axis form the register has one bug from and no coverage of.
const APL_AXES: &[&str] = &["0", "1", "2", "0.5", "1.5", "¯0.5", "3", "⍳0"];

/// Functions that take a bracket axis with one argument.
const APL_AXIS_MONADS: &[&str] = &[",", "⍪", "⌽", "⊖", "↑", "↓", "⊂", "≢", "∊"];

/// Functions that take a bracket axis with two, where the left argument is
/// an ordinary value.
const APL_AXIS_DYADS: &[&str] = &[",", "⍪", "⌽", "⊖", "↓", "⊂", "⌷", "∊"];

/// Functions that take a bracket axis with two, where the left argument is
/// an amount and so must stay a small literal.
const APL_AXIS_SIZED: &[&str] = &["/", "⌿", "\\", "⍀", "↑", "↓"];

/// The same tolerance edges as J's, in APL's spelling. Both languages hold
/// a comparison tolerance and neither register entry says they read it the
/// same way.
const APL_TOLERANCE_PAIRS: &[(&str, &str)] = &[
    ("1", "1.0000000000000002"),
    ("1", "1.0000000000001"),
    ("1", "1.00000000000001"),
    ("1", "0.9999999999999999"),
    ("0.3", "0.1+0.2"),
    ("100000000", "100000000.00000001"),
    ("2147483648", "2147483647.9999998"),
    ("4503599627370496", "4503599627370497"),
    ("9007199254740992", "9007199254740993"),
    ("¯1", "¯1.0000000000001"),
    ("0.000000000000001", "0"),
    ("0.0000000000001", "0"),
    ("0.5", "0.49999999999999994"),
    ("3", "3-0.0000000000001"),
    ("1 2 3", "1 2 3+0.00000000000001"),
    ("10000000000", "10000000000.0001"),
];

/// The dyads a tolerance pair is safe to reach: every one reads its left
/// argument as a value rather than as an amount.
const APL_TOLERANCE_DYADS: &[&str] = &[
    "+", "-", "×", "÷", "*", "⌈", "⌊", "|", "=", "≠", "<", "≤", ">", "≥", "∧", "∨", ",", "⍪", "⌽",
    "⊖", "∊", "≢", "≡", "∪", "∩", "~", "⍳", "⌷", "⊥", "⊤", "⍷", "!", "○", "⍟",
];

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

fn apl_noun(rng: &mut Rng) -> String {
    if rng.below(CORE_LEAF_IN) != 0 {
        rng.pick(APL_NOUNS).to_string()
    } else {
        rng.pick(APL_NOUNS_EXTRA).to_string()
    }
}

fn apl_expr(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 {
        return apl_noun(rng);
    }
    let d = depth - 1;
    // Parenthesised for the same reason as J's: `⍤1` and `⍣2` end in a
    // number, and a numeric noun after one would extend it.
    let arg = |rng: &mut Rng| format!("({})", apl_expr(rng, d));
    // Arms 0 to 17 are generation 1's, in their original proportion; 18 to
    // 25 are generation 2's.
    match rng.below(26) {
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
        17 => format!("{}⌿{}", rng.pick(APL_SIZES), arg(rng)),
        // Bracket axis, in its three shapes: one argument, two arguments,
        // and the sized left argument a replicate or a take wants.
        18 => format!("{}[{}]{}", rng.pick(APL_AXIS_MONADS), rng.pick(APL_AXES), arg(rng)),
        19 => format!(
            "({}){}[{}]{}",
            apl_expr(rng, d),
            rng.pick(APL_AXIS_DYADS),
            rng.pick(APL_AXES),
            arg(rng)
        ),
        20 => format!(
            "{}{}[{}]{}",
            rng.pick(APL_SIZES),
            rng.pick(APL_AXIS_SIZED),
            rng.pick(APL_AXES),
            arg(rng)
        ),
        // Reduction and scan along a named axis.
        21 => {
            let fold = rng.pick(APL_FOLDS);
            let op = rng.pick(&["/", "⌿", "\\", "⍀"]);
            format!("{fold}{op}[{}]{}", rng.pick(APL_AXES), arg(rng))
        }
        // A tolerance-edge pair, fed to a dyad that reads its arguments as
        // values.
        22 => {
            let (left, right) = APL_TOLERANCE_PAIRS[rng.below(APL_TOLERANCE_PAIRS.len())];
            format!("({left}){}({right})", rng.pick(APL_TOLERANCE_DYADS))
        }
        // A nest, deliberately: one operand is always a nested value, so
        // the depth and prototype machinery is reached rather than brushed.
        23 => format!(
            "({}){}({})",
            rng.pick(APL_NOUNS_EXTRA),
            rng.pick(APL_STRUCT_DYADS),
            apl_expr(rng, d)
        ),
        24 => format!("{} ({})", apl_fn(rng, d), rng.pick(APL_NOUNS_EXTRA)),
        // Power with a count past the small non-negative ones. `⍣≡` is
        // deliberately absent: converging is unbounded.
        _ => format!(
            "(({})⍣{}) {}",
            apl_fn(rng, d),
            rng.pick(&["0", "1", "2", "3", "¯1", "¯2"]),
            arg(rng)
        ),
    }
}

// ---------------------------------------------------------------------
// Cluster signatures
// ---------------------------------------------------------------------

/// The characters a J primitive can start with. A primitive is one of these
/// followed by up to two inflections, which is how `{::` and `p..` are one
/// token rather than three.
const J_PRIMITIVE_CHARS: &str = "+-*%^$~|.:,;#!/\\[]{}\"`@&=<>?";

/// The APL glyphs worth naming in a signature. `[` is here so that a
/// bracket axis shows in the signature of the expression that used one.
const APL_GLYPHS: &str = "+-×÷*⌈⌊|=≠<≤>≥∧∨⍴,⍪⍉⌽⊖≢≡⍕∊∪∩~⊂⊃↑↓⍳⍸⌷⊥⊤⍷!○⍟⍋⍒⊢⊣⍎?⍣⍤⍨¨⍥⍛∘.⌿⍀/\\⍬⎕⍺⍵→←⋄[";

/// The primitives one expression names, sorted and without repeats.
/// Literals and numbers are skipped: `0.1` is not a determinant and `'a.'`
/// is not a verb.
pub fn primitives(lang: Lang, expr: &str) -> Vec<String> {
    let mut found = match lang {
        Lang::J => j_primitives(expr),
        Lang::Apl => apl_primitives(expr),
    };
    found.sort();
    found.dedup();
    found
}

/// Step past a quoted literal, whose doubled quote is one character of the
/// literal rather than its end.
fn skip_literal(chars: &[char], mut i: usize) -> usize {
    i += 1;
    while i < chars.len() {
        if chars[i] == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    i
}

fn j_primitives(expr: &str) -> Vec<String> {
    let chars: Vec<char> = expr.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            i = skip_literal(&chars, i);
            continue;
        }
        // A number: `_` starts one (it is the negative sign and infinity
        // both), and the letters inside `1e_15`, `3j4`, `1r2` and `123x`
        // belong to it rather than to a primitive.
        if c.is_ascii_digit() || c == '_' {
            i += 1;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_')
            {
                i += 1;
            }
            continue;
        }
        if c.is_ascii_alphabetic() {
            // A one-letter word with an inflection is a primitive (`i.`,
            // `a:`, `L:`); anything longer is a name, and names carry no
            // meaning a signature wants.
            let inflected = matches!(chars.get(i + 1), Some('.') | Some(':'));
            if inflected {
                let (token, next) = j_token(&chars, i);
                out.push(token);
                i = next;
            } else {
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
            }
            continue;
        }
        if J_PRIMITIVE_CHARS.contains(c) {
            let (token, next) = j_token(&chars, i);
            out.push(token);
            i = next;
            continue;
        }
        i += 1;
    }
    out
}

/// One J primitive starting at `at`: the base character and up to two
/// inflections.
fn j_token(chars: &[char], at: usize) -> (String, usize) {
    let mut end = at + 1;
    while end < chars.len() && end < at + 3 && matches!(chars[end], '.' | ':') {
        end += 1;
    }
    (chars[at..end].iter().collect(), end)
}

fn apl_primitives(expr: &str) -> Vec<String> {
    let chars: Vec<char> = expr.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            i = skip_literal(&chars, i);
            continue;
        }
        // A number, so that the point in `0.1` is not read as the inner
        // product's.
        if c.is_ascii_digit() || c == '¯' {
            i += 1;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '¯')
            {
                i += 1;
            }
            continue;
        }
        if APL_GLYPHS.contains(c) {
            out.push(c.to_string());
        }
        i += 1;
    }
    out
}

/// A stable name for the cause a mismatch has. Two sentences with the same
/// signature are two spellings of one finding, which is what lets a sweeper
/// say "nothing new" instead of counting rows; deduplicating on the
/// expression text can never say that, because the space of expressions is
/// effectively infinite.
///
/// The field names how the two sides parted and what libjay made of the
/// sentence, then, after `|`, the primitives the sentence names.
///
/// `expr` is meant to be a sentence [`reduce`] has already cut down to the
/// smallest one that still parts the two sides the same way. That is what
/// keeps the part after the bar bounded: a cut-down sentence names one or
/// two primitives, where the composed sentence it came from names eight or
/// ten, and a set of eight is a property of the draw rather than of the
/// cause — one cause spread over every subset it can be drawn inside is what
/// makes a seen-set grow without end.
///
/// `ours` is libjay's side as the comparison printed it: `<panic>`,
/// `<no value>`, `<error> …`, or the value.
pub fn signature(lang: Lang, verdict: Verdict, expr: &str, ours: &str) -> String {
    let named = primitives(lang, expr);
    let named =
        if named.len() > NAMED_PRIMITIVES { "…".to_string() } else { named.join(",") };
    format!("{}:{}|{}", verdict.label(), answer_class(ours), named)
}

/// How many primitives a signature will name. A cause is a verb, what
/// derived it and what it was handed — three names reach that. A sentence
/// still naming more than three after [`reduce`] has run is one the cut
/// could not take apart, and naming its primitives would sign the draw
/// rather than the cause, so they are dropped for a `…` and every such
/// sentence of one class becomes one finding.
const NAMED_PRIMITIVES: usize = 3;

/// How far a sentence is cut down in search of the smallest one that parts
/// the two sides the same way. Every step costs the caller one run of libjay
/// and one of the oracle, so the ceiling is what keeps one mismatch from
/// costing more than the batch it was found in.
pub const REDUCE_BUDGET: usize = 24;

/// The smallest sentence reachable from `expr` by cutting parenthesised
/// groups out of it that `holds` is still true of — where `holds`, for a
/// sweep, is "libjay and the oracle still part the same way".
///
/// A composed sentence is a tree with a bug somewhere inside it: the
/// signature of the whole tree is a property of the draw, and the signature
/// of the cut-down sentence is a property of the bug. Candidates are tried
/// shortest first and the search repeats from whatever held, so the answer
/// is the smallest sentence in reach rather than merely a smaller one.
/// `holds` is never asked about `expr` itself, and is asked at most `budget`
/// times.
pub fn reduce(expr: &str, budget: usize, mut holds: impl FnMut(&str) -> bool) -> String {
    let mut best = expr.to_string();
    let mut spent = 0usize;
    loop {
        let mut cut = false;
        for candidate in reductions(&best) {
            if spent >= budget {
                return best;
            }
            spent += 1;
            if holds(&candidate) {
                best = candidate;
                cut = true;
                break;
            }
        }
        if !cut {
            return best;
        }
    }
}

/// The sentences one cut from `expr` reaches, shortest first and without
/// repeats: the inside of each parenthesised group on its own, and the
/// sentence with each such group replaced by a plain `2`. The first isolates
/// a sub-sentence, the second keeps the outer verb and takes the argument
/// that fed it down to something nobody can blame.
fn reductions(expr: &str) -> Vec<String> {
    let chars: Vec<char> = expr.chars().collect();
    let mut out: Vec<String> = Vec::new();
    for (open, close) in paren_groups(&chars) {
        out.push(chars[open + 1..close].iter().collect());
        let mut replaced: String = chars[..open].iter().collect();
        replaced.push('2');
        replaced.extend(&chars[close + 1..]);
        out.push(replaced);
    }
    for candidate in &mut out {
        *candidate = candidate.trim().to_string();
    }
    out.retain(|c| c.chars().count() < chars.len() && !c.chars().all(is_slack));
    out.sort_by(|a, b| a.chars().count().cmp(&b.chars().count()).then_with(|| a.cmp(b)));
    out.dedup();
    out
}

/// A character no sentence can be made of on its own, so that a cut down to
/// `( )` is never offered as one.
fn is_slack(c: char) -> bool {
    c.is_whitespace() || c == '(' || c == ')'
}

/// Every balanced parenthesised group, as the index of its `(` and of its
/// `)`. A parenthesis inside a quoted literal is a character, not structure.
fn paren_groups(chars: &[char]) -> Vec<(usize, usize)> {
    let mut opens: Vec<usize> = Vec::new();
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\'' => {
                i = skip_literal(chars, i);
                continue;
            }
            '(' => opens.push(i),
            ')' => {
                if let Some(open) = opens.pop() {
                    out.push((open, i));
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// Whether libjay's side of a candidate could still part the way `verdict`
/// says, before the oracle has been asked. A reduction whose libjay side
/// already rules the verdict out is not worth an interpreter run, and most
/// of them are: the pruning is what keeps a cut-down search inside a batch.
pub fn could_part(verdict: Verdict, ours: Option<&libjay_testkit::eval::Answer>) -> bool {
    use libjay_testkit::eval::Answer;
    let Some(ours) = ours else {
        return verdict == Verdict::Panicked;
    };
    match verdict {
        Verdict::Panicked => false,
        Verdict::Differ | Verdict::TheyRefuse => matches!(ours, Answer::Value(_) | Answer::NoValue),
        Verdict::Gap => {
            matches!(ours, Answer::Refused(e) if matches!(e.kind, ErrorKind::NotYet | ErrorKind::Language))
        }
        Verdict::WeRefuse => match ours {
            Answer::NoValue => true,
            Answer::Refused(e) => !matches!(e.kind, ErrorKind::NotYet | ErrorKind::Language),
            Answer::Value(_) => false,
        },
        Verdict::Agree | Verdict::Unfinished | Verdict::OracleAbort | Verdict::RunnerDied => false,
    }
}

/// The half of a signature before the primitives: how the two sides parted
/// and what libjay made of the sentence.
pub fn cause_class(signature: &str) -> &str {
    signature.split('|').next().unwrap_or(signature)
}

// ---------------------------------------------------------------------
// Family rules
// ---------------------------------------------------------------------

/// A FAMILY of divergences, pinned by the `~ ` line under one of its
/// sentences in `divergences.txt`.
///
/// The list of accepted divergences matches a sweep's mismatch by the
/// SENTENCE and by the CAUSE SIGNATURE. Neither can pin an arithmetic
/// family: its sentences are all different, and its signature —
/// `differ:val:atom/num|…` — is the signature of every arithmetic
/// difference there is. A family rule is the third kind, and it says in
/// clauses what the row's `? ` note says in prose:
///
/// ```text
/// (o. 1) +. 1
/// ? the reference's cut is no common divisor of its own arguments
/// ~ cause=differ:val:atom/num verb=+.,*. answers=inexact
/// ```
///
/// - `cause=` (required) is the cause classes the mismatch may have — how
///   the two sides parted, and what libjay made of the sentence — one of
///   which it must be. Several are separated by `|`, since a cause class
///   can hold a comma but never a bar.
/// - `verb=` (required, non-empty) is the primitives the family is about.
///   The cut-down sentence must name at least ONE of them.
/// - `with=` is the primitives it must name as well, ALL of them: a family
///   about an obverse is about `^:` and the verb both.
/// - `also=` is the further primitives it may name. Everything the sentence
///   names has to be in `verb`, in `with`, in `also`, or in [`neutral`] —
///   the structural tokens that frame, compose and reorder but compute
///   nothing of their own. A sentence naming any other primitive is NOT
///   this family: nobody has shown which of the two verbs parted the sides.
/// - `answers=` is the value class the two answers must have, which is how
///   "of two values with no common measure" is written down.
///
/// A rule is thereby bounded by what its reason covers, and the sentence it
/// hangs under is still recorded with both answers: `record --check`
/// re-measures it, and the day the family converges that row stops
/// diverging and the check says so.
pub struct Family {
    cause: Vec<String>,
    verb: Vec<String>,
    with: Vec<String>,
    also: Vec<String>,
    answers: Vec<Trait>,
}

/// The value class a family rule asks the two answers to have.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Trait {
    /// Both sides answered a value made of nothing but numbers.
    Numeric,
    /// Numeric, and at least one side's answer is written as a float — a
    /// point or an exponent. This is what "no common measure" looks like
    /// from outside: an exact pair answers exactly on both sides.
    Inexact,
    /// Numeric, and neither side's answer is written as a float.
    Exact,
    /// Numeric, and one of the two answers holds a magnitude at or above
    /// 2^53, where a double stops telling whole numbers apart. That is what
    /// separates a family about the size of a number from a family about
    /// its inexactness.
    Huge,
    /// Numeric, and both answers stay under 2^53.
    Small,
}

/// The largest magnitude a printed answer holds, and `None` where nothing
/// in it reads as a number. J and APL both write a negative sign the
/// arithmetic does not, and an infinity as a bare mark.
fn magnitude(text: &str) -> Option<f64> {
    // One part of a written number: an infinity is a bare mark, an
    // extended one ends in `x`, and a rational is a quotient.
    fn part(text: &str) -> Option<f64> {
        let plain = text.replace(['_', '¯'], "-");
        let plain = plain.strip_suffix('x').unwrap_or(&plain);
        match plain {
            "-" => return Some(f64::INFINITY),
            "--" => return Some(f64::NEG_INFINITY),
            _ => {}
        }
        match plain.split_once('r') {
            Some((num, den)) => Some(part(num)? / part(den)?),
            None => plain.parse().ok(),
        }
    }
    let mut most: Option<f64> = None;
    for token in text.split_whitespace() {
        // A complex number is written `aJb`, and its magnitude is the
        // hypotenuse of the two.
        let value = match token.split_once('j') {
            Some((re, im)) => part(re)?.hypot(part(im)?),
            None => part(token)?,
        };
        most = Some(most.map_or(value.abs(), |m: f64| m.max(value.abs())));
    }
    most
}

/// Where a double stops telling one whole number from the next.
const DOUBLE_STEP: f64 = 9_007_199_254_740_992.0;

impl Trait {
    fn parse(name: &str) -> Result<Trait, String> {
        match name {
            "numeric" => Ok(Trait::Numeric),
            "inexact" => Ok(Trait::Inexact),
            "exact" => Ok(Trait::Exact),
            "huge" => Ok(Trait::Huge),
            "small" => Ok(Trait::Small),
            other => Err(format!(
                "unknown answer class {other:?}: numeric, inexact, exact, huge or small"
            )),
        }
    }

    fn holds(self, ours: &str, theirs: &str) -> bool {
        let numeric = |text: &str| {
            !text.trim().is_empty() && text.chars().all(|c| NUMERIC_OUTPUT.contains(c))
        };
        let float = |text: &str| text.contains('.') || text.contains('e');
        if !(numeric(ours) && numeric(theirs)) {
            return false;
        }
        match self {
            Trait::Numeric => true,
            Trait::Inexact => float(ours) || float(theirs),
            Trait::Exact => !float(ours) && !float(theirs),
            Trait::Huge => [ours, theirs].iter().any(|t| magnitude(t) >= Some(DOUBLE_STEP)),
            Trait::Small => [ours, theirs]
                .iter()
                .all(|t| magnitude(t).is_some_and(|m| m < DOUBLE_STEP)),
        }
    }
}

/// The primitives a family rule need not name: the ones that frame, box,
/// compose and reorder without computing anything of their own, so that a
/// fresh spelling of one arithmetic family inside a wrapper is still that
/// family. Everything that CAN make a number — including `^:`, whose
/// negative left argument is an obverse and a family of its own — is left
/// out, so a row that means to cover it has to say so in `also=`.
fn neutral(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::J => &[
            "[", "]", "[:", "\"", "@", "@:", "&", "&:", "~", "`", "L:", "S:", "/", "\\", "\\.",
            "<", ">", ",", ";", "#", "$", "{", "}", "|.", "|:", "i.", "a:", "{.", "}.", "{:",
            "}:", ",:", ",.", "/:", "\\:", "~.", "{::",
        ],
        Lang::Apl => &["⊢", "⊣", "∘", "⍤", "⍥", "¨", "⍨", "/", "⌿", "\\", "⍀", "⊂", "⊃", "⍴", "⌽", "⍉"],
    }
}

impl Family {
    /// Read the clauses of a `~ ` line. Every clause is `key=value`, and an
    /// unknown key is a malformed rule rather than a clause that quietly
    /// does nothing.
    pub fn parse(text: &str) -> Result<Family, String> {
        let mut family = Family {
            cause: Vec::new(),
            verb: Vec::new(),
            with: Vec::new(),
            also: Vec::new(),
            answers: Vec::new(),
        };
        let list = |v: &str| v.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
        for clause in text.split_whitespace() {
            let (key, value) = clause
                .split_once('=')
                .ok_or_else(|| format!("clause {clause:?} is not key=value"))?;
            match key {
                // A cause class can hold a comma — a refusal's own words
                // are part of it — so its alternatives are separated by
                // the one character a cause class never holds, which is
                // what a signature already uses to end it.
                "cause" => {
                    family.cause = value.split('|').filter(|s| !s.is_empty()).map(str::to_string).collect();
                }
                "verb" => family.verb = list(value),
                "with" => family.with = list(value),
                "also" => family.also = list(value),
                "answers" => {
                    family.answers =
                        value.split(',').map(Trait::parse).collect::<Result<Vec<_>, _>>()?;
                }
                other => return Err(format!("unknown clause {other:?}")),
            }
        }
        if family.cause.is_empty() {
            return Err("a family rule needs `cause=`: the class of mismatch it covers".to_string());
        }
        if family.verb.is_empty() {
            return Err("a family rule needs `verb=`: the primitives it is about".to_string());
        }
        Ok(family)
    }

    /// Whether one mismatch belongs to this family. `expr` is the sentence
    /// [`reduce`] cut the drawn one down to, and `ours` and `theirs` are the
    /// two answers as the comparison printed them.
    pub fn covers(&self, lang: Lang, verdict: Verdict, expr: &str, ours: &str, theirs: &str) -> bool {
        if !self.cause.iter().any(|c| c == cause_class(&signature(lang, verdict, expr, ours))) {
            return false;
        }
        let named = primitives(lang, expr);
        if !named.iter().any(|p| self.verb.contains(p)) {
            return false;
        }
        if !self.with.iter().all(|p| named.contains(p)) {
            return false;
        }
        let neutral = neutral(lang);
        if !named.iter().all(|p| {
            self.verb.contains(p)
                || self.with.contains(p)
                || self.also.contains(p)
                || neutral.contains(&p.as_str())
        }) {
            return false;
        }
        self.answers.iter().all(|t| t.holds(ours, theirs))
    }
}

/// What libjay made of the sentence, coarsely enough that two runs of one
/// cause land in one bucket: the refusal's own words with the numbers taken
/// out, or the shape and kind of the value.
fn answer_class(ours: &str) -> String {
    match ours {
        "<panic>" => return "panic".to_string(),
        "<no value>" => return "novalue".to_string(),
        _ => {}
    }
    match ours.strip_prefix("<error> ") {
        Some(message) => format!("err:{}", normalised_message(message)),
        None => format!("val:{}", shape_class(ours)),
    }
}

/// The refusal's first line, with every run of digits replaced by `#` and
/// every run of spaces by an underscore, so that "length error: 3 and 4 do
/// not agree" and the same sentence about 5 and 6 are one cause.
fn normalised_message(message: &str) -> String {
    let first = message.lines().next().unwrap_or("");
    let mut out = String::new();
    let mut in_number = false;
    for c in first.chars() {
        if c.is_ascii_digit() {
            if !in_number {
                out.push('#');
                in_number = true;
            }
            continue;
        }
        in_number = false;
        out.push(if c.is_whitespace() { '_' } else { c });
    }
    out.chars().take(72).collect()
}

/// The shape and kind of a printed value: enough to separate "we answer a
/// table" from "we answer an atom" without making every value its own
/// class.
fn shape_class(value: &str) -> String {
    let kind = if value.chars().all(|c| NUMERIC_OUTPUT.contains(c)) { "num" } else { "text" };
    let lines: Vec<&str> = value.lines().collect();
    let shape = match lines.len() {
        0 => "empty",
        _ if lines.iter().any(|l| l.trim().is_empty()) => "planes",
        1 => {
            let line = lines[0].trim();
            if line.is_empty() {
                "empty"
            } else if line.contains(' ') {
                "vector"
            } else {
                "atom"
            }
        }
        _ => "table",
    };
    format!("{shape}/{kind}")
}

/// The characters a purely numeric answer is printed out of, in either
/// language: the digits, the two negative signs, the exponent and complex
/// and rational marks, and the infinities.
const NUMERIC_OUTPUT: &str = "0123456789.-_¯ejrx \t\n∞";

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
    /// The oracle died on the sentence. Nothing to compare against either,
    /// and the fault is the reference's: a crash is not an answer, so it is
    /// neither a mismatch nor an agreement, and it is counted on its own so
    /// that a sweep's unexplained tally never hides one.
    OracleAbort,
    /// libjay panicked. Always a bug: a refusal is a diagnostic, a panic is
    /// a crash.
    Panicked,
    /// The RUNNER died on the sentence — a fatal signal or an abort, which
    /// no `catch_unwind` can hold and which takes the whole process with
    /// it. Nothing was compared. A supervised sweep carries on past it and
    /// names the sentence; the sentence itself is then a bug of its own.
    RunnerDied,
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
            Verdict::OracleAbort => "oracle-abort",
            Verdict::Panicked => "panic",
            Verdict::RunnerDied => "runner-died",
        }
    }

    /// The verdict a [`Verdict::label`] names, which is how a journal
    /// written by one process is read back by another.
    pub fn of_label(label: &str) -> Option<Verdict> {
        [
            Verdict::Agree,
            Verdict::Differ,
            Verdict::Gap,
            Verdict::WeRefuse,
            Verdict::TheyRefuse,
            Verdict::Unfinished,
            Verdict::OracleAbort,
            Verdict::Panicked,
            Verdict::RunnerDied,
        ]
        .into_iter()
        .find(|v| v.label() == label)
    }

    /// A verdict worth a human's attention.
    pub fn is_mismatch(self) -> bool {
        !matches!(
            self,
            Verdict::Agree | Verdict::Unfinished | Verdict::OracleAbort | Verdict::RunnerDied
        )
    }

    /// Whether the two answers were compared at all.
    pub fn is_compared(self) -> bool {
        !matches!(self, Verdict::Unfinished | Verdict::OracleAbort | Verdict::RunnerDied)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An origin-0 draw is what makes an index-origin disagreement findable
    /// at all, and it stays a minority of the run.
    #[test]
    fn an_apl_run_visits_both_index_origins() {
        let probes = fuzz(Lang::Apl, 200, DEFAULT_SEED, 3);
        let zeros = probes.iter().filter(|p| p.io == 0).count();
        assert!(zeros > 0, "no origin-0 probe in 200");
        assert!(zeros * 2 < probes.len(), "{zeros} of 200 at origin 0 is not a minority");
        assert!(probes.iter().all(|p| p.io == 0 || p.io == 1));
    }

    /// J has no index origin: a J probe that carried 0 would ask for a
    /// dialect setting neither side has.
    #[test]
    fn a_j_run_stays_at_origin_one() {
        let probes = fuzz(Lang::J, 200, DEFAULT_SEED, 3);
        assert!(probes.iter().all(|p| p.io == 1));
    }

    /// An inflection belongs to the primitive before it, a number's point
    /// belongs to the number, and a quoted `.` is a character.
    #[test]
    fn j_primitives_are_read_with_their_inflections() {
        assert_eq!(primitives(Lang::J, "i. 5"), ["i."]);
        assert_eq!(primitives(Lang::J, "1.5 + 0.25"), ["+"]);
        assert_eq!(primitives(Lang::J, "{:: <'a.b'"), ["<", "{::"]);
        assert_eq!(primitives(Lang::J, "- L: _1 (<1;2)"), ["-", ";", "<", "L:"]);
        assert_eq!(primitives(Lang::J, "+/\\ 1e_15 , _3"), ["+", ",", "/", "\\"]);
        assert_eq!(primitives(Lang::J, "> &.: |. i. 0"), ["&.:", ">", "i.", "|."]);
    }

    /// A glyph is a primitive, a digit is not, and a bracket axis shows in
    /// the signature of the sentence that used one.
    #[test]
    fn apl_primitives_are_glyphs_outside_numbers_and_literals() {
        assert_eq!(primitives(Lang::Apl, "0.1+0.2"), ["+"]);
        assert_eq!(primitives(Lang::Apl, "(1 2)∘.×⍳3"), [".", "×", "∘", "⍳"]);
        assert_eq!(primitives(Lang::Apl, ",[0.5]'a×b'"), [",", "["]);
        assert_eq!(primitives(Lang::Apl, "⌽[1]2 3⍴⍳6"), ["[", "⌽", "⍳", "⍴"]);
    }

    /// Two spellings of one cause share a signature; two causes do not.
    /// The numbers inside a refusal are the part that varies from sentence
    /// to sentence, so they are the part that is taken out.
    #[test]
    fn a_signature_names_the_cause_rather_than_the_sentence() {
        let we = Verdict::WeRefuse;
        let one = signature(Lang::J, we, "3 + 4", "<error> length error: 3 and 4 do not agree");
        let two = signature(Lang::J, we, "5 + 6", "<error> length error: 7 and 9 do not agree");
        assert_eq!(one, two);
        let other = signature(Lang::J, we, "3 + 4", "<error> domain error: not a number");
        assert_ne!(one, other);
        let elsewhere = signature(Lang::J, we, "3 - 4", "<error> length error: 3 and 4 do not agree");
        assert_ne!(one, elsewhere);
        // How the two sides parted is part of the cause: the same answer
        // against an oracle that refused and against one that did not are
        // two findings, not one.
        let they = signature(Lang::J, Verdict::TheyRefuse, "i. 3", "0 1 2");
        assert_eq!(they, "they-refuse:val:vector/num|i.");
        assert_ne!(they, signature(Lang::J, Verdict::Differ, "i. 3", "0 1 2"));
        assert_eq!(signature(Lang::Apl, Verdict::Differ, "⍳0", ""), "differ:val:empty/num|⍳");
        assert_eq!(signature(Lang::Apl, Verdict::Panicked, "⍕3", "<panic>"), "panic:panic|⍕");
        assert_eq!(cause_class(&one), "we-refuse:err:length_error:_#_and_#_do_not_agree");
        // A sentence the cut could not take apart names the draw rather
        // than the cause, so it names nothing.
        let uncut = signature(Lang::J, we, "(+/ % #) @: (i. 3) , }: 4", "<error> domain error: x");
        assert_eq!(uncut, "we-refuse:err:domain_error:_x|…");
    }

    /// A family rule covers the spellings its reason covers and no others:
    /// the cause class has to be one it names, the sentence has to name one
    /// of its verbs and nothing outside what it allows, and the two answers
    /// have to be of the class it asks for.
    #[test]
    fn a_family_rule_covers_what_its_reason_covers() {
        let rule = Family::parse(
            "cause=differ:val:atom/num|differ:val:vector/num verb=+.,*. also=o.,% answers=inexact",
        )
        .expect("a well-formed rule");
        let covers = |expr: &str, ours: &str, theirs: &str| {
            rule.covers(Lang::J, Verdict::Differ, expr, ours, theirs)
        };
        // The family itself, spelled two ways the accepted list has never
        // seen: one names `o.`, the other only structure.
        assert!(covers("(o. 1) *. 3", "3.19619e13", "1.34321e12"));
        assert!(covers("{. ((o. 1) *. 3)", "3.19619e13", "1.34321e12"));
        // A verb the rule does not name is a cause nobody has ruled out.
        assert!(!covers("(!: 1) *. 3", "3.19619e13", "1.34321e12"));
        // Exact answers are not this family: two whole numbers have a GCD
        // both engines agree about.
        assert!(!covers("(o. 1) *. 3", "12", "15"));
        // Nor is another way of parting, nor another kind of answer.
        assert!(!rule.covers(Lang::J, Verdict::WeRefuse, "(o. 1) *. 3", "<error> x", "1.5"));
        // A class the rule does name, spelled with the bar that separates
        // them, is covered; one it does not is not.
        assert!(covers("(o. 1) *. 3", "3.19 4.5", "1.34 2.5"));
        assert!(!rule.covers(Lang::J, Verdict::Differ, "(o. 1) *. 3", "a b\nc d", "e f\ng h"));
        // `with=` asks for every one of its primitives, which is how a
        // family about an obverse is about `^:` as well as the verb.
        let obverse = Family::parse("cause=differ:val:atom/num verb=! with=^:").expect("rule");
        assert!(obverse.covers(Lang::J, Verdict::Differ, "!^:_1 ] 1.1", "1.19699", "_0.136587"));
        assert!(!obverse.covers(Lang::J, Verdict::Differ, "! 1.1", "1.19699", "_0.136587"));
        // A rule with nothing to pin is a malformed rule, not a rule that
        // excuses everything.
        assert!(Family::parse("verb=+.").is_err());
        assert!(Family::parse("cause=differ:val:atom/num").is_err());
        assert!(Family::parse("cause=differ:val:atom/num verb=+. sideways=yes").is_err());
    }

    /// A cut-down sentence is the smallest one in reach that the predicate
    /// still holds of, not merely a smaller one, and the search never
    /// proposes a sentence made of nothing.
    #[test]
    fn a_sentence_is_cut_down_to_the_smallest_that_still_parts() {
        // The cause is the `¯2/` buried three groups deep; everything
        // wrapped around it is the draw. Lifting a group out reaches it,
        // and cutting the argument down reaches the smallest sentence that
        // still names it.
        let drawn = "(2 3⍴⍳6) + (⌊/(0⍴(¯2/(⊂''))))";
        assert_eq!(reduce(drawn, 64, |c| c.contains("¯2/")), "¯2/2");
        assert_eq!(reduce(drawn, 64, |c| c.contains("⍴(¯2/")), "0⍴(¯2/2)");
        // With nothing to preserve, the shortest cut wins, and a sentence
        // with no group left in it is as far as cutting goes.
        assert_eq!(reduce("(1+2) × (3+4)", 64, |_| true), "1+2");
        // A predicate nothing satisfies leaves the sentence as drawn, and
        // the budget bounds what was spent finding that out.
        let mut asked = 0;
        assert_eq!(
            reduce(drawn, 3, |_| {
                asked += 1;
                false
            }),
            drawn
        );
        assert_eq!(asked, 3);
        // A parenthesis inside a literal is a character, not structure.
        assert!(reductions("'(a' , ⍳3").is_empty());
    }

    /// Generation 2's axes are reachable at the depths a sweep uses: a run
    /// that never composes them is a run that cannot find what they hide.
    #[test]
    fn generation_two_axes_are_drawn() {
        let text: String = fuzz(Lang::J, 3000, DEFAULT_SEED, 4)
            .iter()
            .map(|p| p.expr.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for axis in ["^:_1", "L:", "S:", "&.:", "!.", ";:", "\"0 1 2", "1.0000000000001"] {
            assert!(text.contains(axis), "no {axis} in 3000 J expressions");
        }
        let text: String = fuzz(Lang::Apl, 3000, DEFAULT_SEED, 4)
            .iter()
            .map(|p| p.expr.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for axis in ["[0.5]", "⊂⊂⍳3", "0 0 0⍴0", "(1(2 3))(4 5)", "⍣¯1", "1.0000000000001"] {
            assert!(text.contains(axis), "no {axis} in 3000 APL expressions");
        }
    }

    /// The leaves generation 1 drew are still the bulk of the draws: a new
    /// axis is worth nothing if it costs the coverage the findings on
    /// record came from.
    #[test]
    fn the_original_leaves_still_dominate() {
        let mut rng = Rng::new(DEFAULT_SEED);
        let core = (0..3000).filter(|_| J_NOUNS.contains(&j_noun(&mut rng).as_str())).count();
        assert!(core > 1800, "{core} of 3000 J leaves from the original pool");
        let mut rng = Rng::new(DEFAULT_SEED);
        let core = (0..3000).filter(|_| APL_NOUNS.contains(&apl_noun(&mut rng).as_str())).count();
        assert!(core > 1800, "{core} of 3000 APL leaves from the original pool");
    }
}
