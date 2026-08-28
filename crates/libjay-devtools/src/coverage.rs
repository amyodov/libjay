//! Which cells of the primitive × operand grid the recorded corpus reaches.
//!
//! J and APL are combinatoric: a verb meets a type, at a rank, under a
//! modifier, and every one of those is an axis. A corpus is a sample of
//! that space, and until it is measured nobody knows which part of the
//! space it samples. This module measures it.
//!
//! The measurement is by evaluation, not by reading the source text.
//! Every corpus expression is compiled by libjay itself and the fusion
//! pass is undone ([`jay::fuse::unfused`]), which leaves the tree the
//! frontend built: one node per verb application, each carrying the verb
//! and its operand subtrees. Each operand subtree is then RUN — as a
//! program made of the sentences before it plus the subtree itself — and
//! the array that comes back is classified. So the classes reported are
//! the classes the primitive actually met, not a guess from the spelling
//! of a literal.
//!
//! What the numbers do not say:
//!
//! - A primitive under a modifier usually does not meet the argument the
//!   sentence names. `+/ y` applies `+` to ITEMS of y, and `u"1 y` applies
//!   u to its rank-1 cells. Those two are followed — the cell's shape is
//!   the argument's, minus the axes the modifier consumed — and are
//!   reported as `on cells` rather than as direct applications. Every
//!   other modifier hands its operand something this measurement cannot
//!   name, and the site is counted as unattributable: it appears in the
//!   operator table and in nothing else.
//! - A few spellings compile to a derived verb of their own accord (J's
//!   `,.` is `,"_1`), so they are never seen bare.
//! - Inside an explicit definition nothing can be run — the argument names
//!   have no value until the definition is called — so those operands are
//!   classified `unknown`, and the site counts as seen but occupies no
//!   cell.
//! - libjay is the classifier, so a sentence libjay refuses is one this
//!   report cannot see into. The count of refusals is printed.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use jay::array::{Array, Data};
use jay::ir::{Control, Expr, Program};
use jay::verb::{Power, Verb, RANK_INF};
use jay::{compile, Dialect};
use libjay_testkit::{corpus, Lang};

use crate::inventory::{self, Inventory};

/// How many rows a section of the terminal report prints.
pub const DEFAULT_TOP: usize = 15;

/// How deep a verb's own structure is followed. A composition nested past
/// this is treated as opaque; nothing in either language's vocabulary
/// composes that far without saying so.
const VERB_DEPTH: u32 = 16;

// ------------------------------------------------------------- the classes

/// The type-class of an operand. Every class is a property of the whole
/// array: libjay's arrays are homogeneous, so there is no `mixed` — a
/// heterogeneous value in either language is a BOX, and boxes have their
/// own two classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ty {
    Bool,
    /// Machine integers, all of them within a million of zero.
    Int,
    /// Machine integers past a million, and within the range a float
    /// carries exactly.
    IntBig,
    /// Machine integers past 2^53, where the integer and float readings of
    /// the same value part company.
    IntEdge,
    Float,
    /// Floats with a value that is within a part in 10^9 of an integer
    /// without being one: what a comparison tolerance decides about.
    FloatTol,
    /// Infinities and NaN.
    FloatSpecial,
    Complex,
    /// Arbitrary-precision integers (J `123x`).
    Extended,
    /// Exact ratios (J `1r3`).
    Rational,
    Char,
    /// A box, none of whose elements is itself boxed.
    Boxed,
    /// A box holding a box.
    BoxNested,
    Symbol,
    /// The operand subtree could not be run: the sentence is one libjay
    /// refuses, or the subtree on its own is not a sentence.
    Refused,
    /// The operand cannot be run at all — it is inside an explicit
    /// definition, where the argument names have no value.
    Unknown,
}

impl Ty {
    pub fn label(self) -> &'static str {
        match self {
            Ty::Bool => "bool",
            Ty::Int => "int",
            Ty::IntBig => "int-big",
            Ty::IntEdge => "i64-edge",
            Ty::Float => "float",
            Ty::FloatTol => "float-tol",
            Ty::FloatSpecial => "float-inf",
            Ty::Complex => "complex",
            Ty::Extended => "extended",
            Ty::Rational => "rational",
            Ty::Char => "char",
            Ty::Boxed => "box",
            Ty::BoxNested => "box-nested",
            Ty::Symbol => "symbol",
            Ty::Refused => "refused",
            Ty::Unknown => "unknown",
        }
    }

    /// Whether this class says something about the data. The two that do
    /// not are what the measurement could not see.
    pub fn known(self) -> bool {
        !matches!(self, Ty::Refused | Ty::Unknown)
    }

    pub const ALL: [Ty; 16] = [
        Ty::Bool,
        Ty::Int,
        Ty::IntBig,
        Ty::IntEdge,
        Ty::Float,
        Ty::FloatTol,
        Ty::FloatSpecial,
        Ty::Complex,
        Ty::Extended,
        Ty::Rational,
        Ty::Char,
        Ty::Boxed,
        Ty::BoxNested,
        Ty::Symbol,
        Ty::Refused,
        Ty::Unknown,
    ];
}

/// The rank-class of an operand: its rank, with the two shapes that behave
/// differently at every rank — an axis of length zero and an axis of
/// length one — kept apart from the general case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rk {
    Scalar,
    Vector,
    /// A vector of one element.
    Vector1,
    /// A vector with no elements.
    VectorEmpty,
    Matrix,
    /// A matrix with an axis of length one.
    Matrix1,
    /// A matrix with no elements.
    MatrixEmpty,
    Rank3,
    Rank3One,
    Rank3Empty,
    Unknown,
}

impl Rk {
    pub fn label(self) -> &'static str {
        match self {
            Rk::Scalar => "scalar",
            Rk::Vector => "vector",
            Rk::Vector1 => "vector-1",
            Rk::VectorEmpty => "vector-empty",
            Rk::Matrix => "matrix",
            Rk::Matrix1 => "matrix-1",
            Rk::MatrixEmpty => "matrix-empty",
            Rk::Rank3 => "rank3+",
            Rk::Rank3One => "rank3+-1",
            Rk::Rank3Empty => "rank3+-empty",
            Rk::Unknown => "unknown",
        }
    }

    pub fn known(self) -> bool {
        self != Rk::Unknown
    }

    pub const ALL: [Rk; 11] = [
        Rk::Scalar,
        Rk::Vector,
        Rk::Vector1,
        Rk::VectorEmpty,
        Rk::Matrix,
        Rk::Matrix1,
        Rk::MatrixEmpty,
        Rk::Rank3,
        Rk::Rank3One,
        Rk::Rank3Empty,
        Rk::Unknown,
    ];
}

/// One operand's classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Class {
    pub ty: Ty,
    pub rk: Rk,
}

impl Class {
    pub const UNKNOWN: Class = Class { ty: Ty::Unknown, rk: Rk::Unknown };

    pub fn label(self) -> String {
        format!("{}/{}", self.ty.label(), self.rk.label())
    }

    pub fn known(self) -> bool {
        self.ty.known() && self.rk.known()
    }
}

/// A cell of the grid: the classes one application of one primitive met.
/// A monadic cell has no left operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cell {
    pub x: Option<Class>,
    pub y: Class,
}

impl Cell {
    pub fn label(self) -> String {
        match self.x {
            Some(x) => format!("{} × {}", x.label(), self.y.label()),
            None => self.y.label(),
        }
    }

    pub fn known(self) -> bool {
        self.y.known() && self.x.is_none_or(|x| x.known())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Valence {
    Monad,
    Dyad,
}

impl Valence {
    pub fn label(self) -> &'static str {
        match self {
            Valence::Monad => "monad",
            Valence::Dyad => "dyad",
        }
    }
}

/// How sure the attribution is: whether the primitive met the operand
/// itself, or a cell of it that the modifier above it carved out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prov {
    Direct,
    OnCells,
}

/// How many times a cell was reached, by each route.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hits {
    pub direct: usize,
    pub on_cells: usize,
}

impl Hits {
    pub fn total(self) -> usize {
        self.direct + self.on_cells
    }
}

// ------------------------------------------------------- classifying a value

/// The value an operand subtree produced, or why there is none.
enum Val {
    Known(Array),
    Refused,
    Unknown,
}

impl Val {
    /// The class of a piece of this value: the whole of it, one item, or
    /// one cell of the given rank.
    fn class(&self, take: Take) -> Class {
        let a = match self {
            Val::Known(a) => a,
            Val::Refused => return Class { ty: Ty::Refused, rk: Rk::Unknown },
            Val::Unknown => return Class::UNKNOWN,
        };
        let dense;
        let a = if a.is_sparse() {
            dense = a.densified();
            &dense
        } else {
            a
        };
        let ty = type_class(a);
        match take.of(&a.shape) {
            Some(s) => Class { ty, rk: rank_class(s) },
            None => Class { ty, rk: Rk::Unknown },
        }
    }
}

/// Which piece of an operand a primitive was handed: a cell of some rank,
/// and then — for a reduction — one item of that cell. A cell of an array
/// has the array's type, so only the shape is carried here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Take {
    /// The trailing axes the cell keeps; None is the whole argument.
    cell: Option<usize>,
    /// Whether one item of that cell is what the primitive met.
    item: bool,
}

impl Take {
    const WHOLE: Take = Take { cell: None, item: false };

    fn is_whole(self) -> bool {
        self == Take::WHOLE
    }

    /// The shape of the piece, or None where there is none — a scalar has
    /// no items.
    fn of(self, shape: &[usize]) -> Option<&[usize]> {
        let s = match self.cell {
            Some(k) => &shape[shape.len().saturating_sub(k)..],
            None => shape,
        };
        if self.item { s.split_first().map(|(_, rest)| rest) } else { Some(s) }
    }

    /// This piece, taken from a cell of the given rank instead of from the
    /// whole argument. An infinite rank is the whole argument.
    fn within(self, rank: i64) -> Take {
        if !(0..RANK_INF).contains(&rank) {
            return self;
        }
        let k = rank as usize;
        Take { cell: Some(self.cell.map_or(k, |j| j.min(k))), item: self.item }
    }
}

/// Past this an integer no longer round-trips through a float, which is
/// where the integer and the float readings of a value part company.
const EXACT_FLOAT: i64 = 1 << 53;
/// Above this an integer is worth calling large; below it, ordinary.
const SMALL_INT: i64 = 1_000_000;
/// How near an integer a float has to be to be a tolerance question.
const NEAR: f64 = 1e-9;

fn type_class(a: &Array) -> Ty {
    match &a.data {
        Data::Bool(_) => Ty::Bool,
        Data::I64(v) => {
            let biggest = v.as_slice().iter().map(|n| n.unsigned_abs()).max().unwrap_or(0);
            if biggest >= EXACT_FLOAT as u64 {
                Ty::IntEdge
            } else if biggest >= SMALL_INT as u64 {
                Ty::IntBig
            } else {
                Ty::Int
            }
        }
        Data::F64(v) => {
            let xs = v.as_slice();
            if xs.iter().any(|x| !x.is_finite()) {
                Ty::FloatSpecial
            } else if xs.iter().any(|x| near_integer(*x)) {
                Ty::FloatTol
            } else {
                Ty::Float
            }
        }
        Data::Complex(_) => Ty::Complex,
        Data::Ext(_) => Ty::Extended,
        Data::Rat(_) => Ty::Rational,
        Data::Char(_) => Ty::Char,
        Data::Symbol(_) => Ty::Symbol,
        Data::Box(v) => {
            if v.as_slice().iter().any(|b| matches!(b.data, Data::Box(_))) {
                Ty::BoxNested
            } else {
                Ty::Boxed
            }
        }
    }
}

/// A float that is not an integer, and is within a part in 10^9 of one:
/// the values a comparison tolerance has an opinion about.
fn near_integer(x: f64) -> bool {
    let r = x.round();
    x != r && (x - r).abs() <= NEAR * x.abs().max(1.0)
}

fn rank_class(shape: &[usize]) -> Rk {
    let empty = shape.contains(&0);
    let singleton = shape.contains(&1);
    match shape.len() {
        0 => Rk::Scalar,
        1 if empty => Rk::VectorEmpty,
        1 if singleton => Rk::Vector1,
        1 => Rk::Vector,
        2 if empty => Rk::MatrixEmpty,
        2 if singleton => Rk::Matrix1,
        2 => Rk::Matrix,
        _ if empty => Rk::Rank3Empty,
        _ if singleton => Rk::Rank3One,
        _ => Rk::Rank3,
    }
}

// ------------------------------------------------------------- the tallies

/// Everything one language's corpus was measured to contain.
#[derive(Clone, Debug, Default)]
pub struct Coverage {
    pub files: usize,
    pub exprs: usize,
    /// Expressions libjay would not compile: invisible to the rest.
    pub refused: usize,
    /// Verb applications seen, whether or not they could be attributed.
    pub sites: usize,
    /// Sites at least one primitive could be attributed from.
    pub attributed: usize,
    /// Sites whose applied verb hands its operand something this
    /// measurement cannot name.
    pub opaque: usize,
    /// Sites inside an explicit definition, where nothing can be run.
    pub in_definition: usize,
    /// (primitive, valence) → cell → how often.
    pub grid: BTreeMap<(String, Valence), BTreeMap<Cell, Hits>>,
    /// (primitive, valence) → how many sites attributed to it.
    pub applied: BTreeMap<(String, Valence), usize>,
    /// Every primitive the corpus mentions at all, including under a
    /// modifier that hides its operands.
    pub mentioned: BTreeSet<String>,
    pub modifiers: BTreeMap<String, ModStat>,
}

/// What one modifier spelling was seen doing.
#[derive(Clone, Debug, Default)]
pub struct ModStat {
    pub sites: usize,
    /// The spellings of the inventory this row answers for. The IR keeps
    /// one node for a family of spellings — J's `@` and `@:` are one atop
    /// — so a row can stand for more than one.
    pub spellings: &'static [&'static str],
    /// The operands it was given, by class, with how often.
    pub operands: BTreeMap<String, usize>,
    /// The noun classes the derived verb was applied to, where the
    /// modifier was the outermost one of the applied verb.
    pub nouns: BTreeMap<String, usize>,
}

impl Coverage {
    fn merge(&mut self, other: Coverage) {
        self.files += other.files;
        self.exprs += other.exprs;
        self.refused += other.refused;
        self.sites += other.sites;
        self.attributed += other.attributed;
        self.opaque += other.opaque;
        self.in_definition += other.in_definition;
        for (k, cells) in other.grid {
            let into = self.grid.entry(k).or_default();
            for (cell, hits) in cells {
                let h = into.entry(cell).or_default();
                h.direct += hits.direct;
                h.on_cells += hits.on_cells;
            }
        }
        for (k, n) in other.applied {
            *self.applied.entry(k).or_default() += n;
        }
        self.mentioned.extend(other.mentioned);
        for (k, stat) in other.modifiers {
            let into = self.modifiers.entry(k).or_default();
            into.sites += stat.sites;
            into.spellings = stat.spellings;
            for (k, n) in stat.operands {
                *into.operands.entry(k).or_default() += n;
            }
            for (k, n) in stat.nouns {
                *into.nouns.entry(k).or_default() += n;
            }
        }
    }

    /// Every cell the corpus produced anywhere, per valence. This is the
    /// universe a per-primitive count is a fraction of: a cell no
    /// expression in the corpus builds for ANY primitive is not something
    /// this corpus could have covered.
    pub fn universe(&self, valence: Valence) -> BTreeSet<Cell> {
        self.grid
            .iter()
            .filter(|((_, v), _)| *v == valence)
            .flat_map(|(_, cells)| cells.keys().copied())
            .filter(|c| c.known())
            .collect()
    }

    /// The cells of the universe this primitive never met.
    pub fn empty_cells(&self, prim: &str, valence: Valence, universe: &BTreeSet<Cell>) -> Vec<Cell> {
        let seen = self.grid.get(&(prim.to_string(), valence));
        universe
            .iter()
            .filter(|c| seen.is_none_or(|s| !s.contains_key(c)))
            .copied()
            .collect()
    }

    /// The primitives with at least one attributed application, per valence.
    pub fn rows(&self, valence: Valence) -> Vec<&str> {
        self.grid
            .keys()
            .filter(|(_, v)| *v == valence)
            .map(|(p, _)| p.as_str())
            .collect()
    }
}

// --------------------------------------------------------------- measuring

/// Measure the recorded corpus of one language.
pub fn measure_corpus(lang: Lang) -> Coverage {
    let mut total = Coverage::default();
    for path in corpus::files(lang) {
        let entries = corpus::read(&path);
        let mut here = measure(lang, &entries);
        here.files = 1;
        total.merge(std::mem::take(&mut here));
    }
    total
}

/// Measure a list of expressions. The tests hand this synthetic ones; the
/// corpus run hands it the recorded files.
pub fn measure(lang: Lang, entries: &[corpus::Entry]) -> Coverage {
    let mut cov = Coverage::default();
    for entry in entries {
        cov.exprs += 1;
        let dialect = match lang {
            Lang::J => Dialect::default(),
            Lang::Apl => {
                Dialect { index_origin: Some(entry.io as i64), ..Dialect::default() }
            }
        };
        let Ok(program) = compile(lang, &entry.expr, &dialect) else {
            cov.refused += 1;
            continue;
        };
        // The fusion pass rewrites elementwise chains into one kernel; the
        // tree it was built from is what the reader wrote, and is what the
        // grid is about.
        let program = jay::fuse::unfused(&program);
        let mut walker = Walker { lang, program: &program, cov: &mut cov };
        for i in 0..program.stmts.len() {
            walker.expr(&program.stmts[i], Some(i));
        }
    }
    cov
}

struct Walker<'a> {
    lang: Lang,
    program: &'a Program,
    cov: &'a mut Coverage,
}

impl Walker<'_> {
    /// Walk one expression. `at` is the index of the sentence it belongs
    /// to, and None inside an explicit definition, where an operand cannot
    /// be run on its own.
    fn expr(&mut self, e: &Expr, at: Option<usize>) {
        match e {
            Expr::Const(..)
            | Expr::Param(..)
            | Expr::Name(..)
            | Expr::Input { .. }
            | Expr::ModDef { .. } => {}
            Expr::Assign { value, .. } | Expr::PrintPass { value, .. } => self.expr(value, at),
            Expr::AmendIndex { slots, value, .. } => {
                for slot in slots.iter().flatten() {
                    self.expr(slot, at);
                }
                self.expr(value, at);
            }
            Expr::Control(c, _) => self.control(c),
            Expr::VerbDef { verb, .. } => self.verb(verb, None, 0),
            Expr::Monad { verb, y, .. } => {
                self.site(verb, None, y, at);
                self.expr(y, at);
            }
            Expr::Dyad { verb, x, y, .. } => {
                self.site(verb, Some(x), y, at);
                self.expr(x, at);
                self.expr(y, at);
            }
            // `unfused` leaves neither of these behind; walking them
            // anyway costs nothing and keeps the walk total.
            Expr::Fused { orig, .. } => self.expr(orig, at),
            Expr::Elided { orig, .. } => {
                for stmt in orig {
                    self.expr(stmt, at);
                }
            }
        }
    }

    fn control(&mut self, c: &Control) {
        let block = |w: &mut Self, stmts: &[Expr]| {
            for s in stmts {
                w.expr(s, None);
            }
        };
        match c {
            Control::If { arms, otherwise } => {
                for arm in arms {
                    if let Some(t) = &arm.test {
                        block(self, t);
                    }
                    block(self, &arm.body);
                }
                if let Some(b) = otherwise {
                    block(self, b);
                }
            }
            Control::While { test, body, .. } | Control::Guard { test, body } => {
                block(self, test);
                block(self, body);
            }
            Control::For { source, body, .. } => {
                self.expr(source, None);
                block(self, body);
            }
            Control::Select { subject, cases } => {
                self.expr(subject, None);
                for arm in cases {
                    if let Some(t) = &arm.test {
                        block(self, t);
                    }
                    block(self, &arm.body);
                }
            }
            Control::Try { body, catch } => {
                block(self, body);
                block(self, catch);
            }
            Control::Branch(e) => self.expr(e, None),
            Control::BranchBy { by, test } => {
                self.expr(by, None);
                self.expr(test, None);
            }
            Control::Cond { test, body, otherwise } => {
                block(self, test);
                block(self, body);
                block(self, otherwise);
            }
            Control::Return | Control::Break | Control::Continue => {}
        }
    }

    /// One verb application: the cells it reached, and the modifiers it
    /// was built from.
    fn site(&mut self, verb: &Verb, x: Option<&Expr>, y: &Expr, at: Option<usize>) {
        self.cov.sites += 1;
        if at.is_none() {
            self.cov.in_definition += 1;
        }
        let valence = if x.is_some() { Valence::Dyad } else { Valence::Monad };
        let vx = match (x, at) {
            (Some(x), Some(i)) => self.value_of(i, x),
            (Some(_), None) => Val::Unknown,
            (None, _) => Val::Unknown,
        };
        let vy = match at {
            Some(i) => self.value_of(i, y),
            None => Val::Unknown,
        };
        let attrs = receivers(verb, valence, 0);
        if attrs.is_empty() {
            self.cov.opaque += 1;
        } else {
            self.cov.attributed += 1;
        }
        for attr in &attrs {
            let cell = Cell {
                x: attr.x.map(|(src, take)| src.pick(&vx, &vy).class(take)),
                y: attr.y.0.pick(&vx, &vy).class(attr.y.1),
            };
            let key = (attr.prim.clone(), attr.valence);
            *self.cov.applied.entry(key.clone()).or_default() += 1;
            let hits = self.cov.grid.entry(key).or_default().entry(cell).or_default();
            match attr.prov {
                Prov::Direct => hits.direct += 1,
                Prov::OnCells => hits.on_cells += 1,
            }
        }
        // The nouns the outermost modifier of this verb was applied to.
        let nouns = Some(Cell {
            x: (valence == Valence::Dyad).then(|| vx.class(Take::WHOLE)),
            y: vy.class(Take::WHOLE),
        });
        self.verb(verb, nouns, 0);
    }

    /// Record the modifiers a verb is built from, and walk the bodies of
    /// any explicit definition inside it.
    fn verb(&mut self, v: &Verb, nouns: Option<Cell>, depth: u32) {
        if depth > VERB_DEPTH {
            return;
        }
        if let Verb::Prim(p) = v {
            self.cov.mentioned.insert(p.name.to_string());
            return;
        }
        if let Verb::Explicit(def) = v {
            self.record(EXPLICIT, Vec::new(), nouns);
            for stmt in &def.body {
                self.expr(stmt, None);
            }
            return;
        }
        if let Some((m, operands)) = modifier(v, self.lang) {
            self.record(m, operands, nouns);
        }
        for inner in operand_verbs(v) {
            self.verb(inner, None, depth + 1);
        }
    }

    fn record(&mut self, m: Mod, operands: Vec<String>, nouns: Option<Cell>) {
        let stat = self.cov.modifiers.entry(m.label.to_string()).or_default();
        stat.sites += 1;
        stat.spellings = m.spellings;
        for operand in operands {
            *stat.operands.entry(operand).or_default() += 1;
        }
        if let Some(cell) = nouns {
            *stat.nouns.entry(cell.label()).or_default() += 1;
        }
    }

    /// Run one operand subtree and keep what it produced. The sentences
    /// before it come along, so a subtree that reads a name assigned
    /// earlier still has one.
    fn value_of(&self, at: usize, e: &Expr) -> Val {
        // An assignment yields nothing at the top of a program, so what is
        // measured is the value it assigns.
        let e = match e {
            Expr::Assign { value, .. } | Expr::PrintPass { value, .. } => value.as_ref(),
            other => other,
        };
        if let Expr::Const(a, _) = e {
            return Val::Known(a.clone());
        }
        let mut stmts: Vec<Expr> = self.program.stmts[..at].to_vec();
        stmts.push(e.clone());
        let sub = Program { stmts, ..self.program.clone() };
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut sink = |_: &str| {};
            sub.run(&[], &mut sink)
        }));
        match ran {
            Ok(Ok(Some(a))) => Val::Known(a),
            _ => Val::Refused,
        }
    }
}

/// Which of the site's two arguments an attribution reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Src {
    X,
    Y,
}

impl Src {
    fn pick<'a>(self, x: &'a Val, y: &'a Val) -> &'a Val {
        match self {
            Src::X => x,
            Src::Y => y,
        }
    }
}

/// One primitive that provably receives one of a site's arguments, and
/// which piece of it.
#[derive(Clone, Debug)]
struct Attr {
    prim: String,
    valence: Valence,
    prov: Prov,
    x: Option<(Src, Take)>,
    y: (Src, Take),
}

impl Attr {
    /// Whether every argument this attribution names is still the site's
    /// own, untouched.
    fn whole(&self) -> bool {
        self.y.1.is_whole() && self.x.is_none_or(|(_, t)| t.is_whole())
    }

    /// Whether a rank can still be applied over this attribution: the
    /// pieces it names must be items of a cell, not cells of an item.
    fn takes_rank(&self) -> bool {
        self.y.1.cell.is_none() && self.x.is_none_or(|(_, t)| t.cell.is_none())
    }
}

/// The primitives an applied verb hands the site's own arguments to.
///
/// A composition that routes its operands somewhere this cannot describe
/// yields nothing: the site is then unattributable, which is a measured
/// fact rather than a gap.
fn receivers(v: &Verb, valence: Valence, depth: u32) -> Vec<Attr> {
    if depth > VERB_DEPTH {
        return Vec::new();
    }
    let down = |v: &Verb| receivers(v, valence, depth + 1);
    match v {
        Verb::Prim(p) => vec![Attr {
            prim: p.name.to_string(),
            valence,
            prov: Prov::Direct,
            x: (valence == Valence::Dyad).then_some((Src::X, Take::WHOLE)),
            y: (Src::Y, Take::WHOLE),
        }],
        // A fork gives both tines the arguments the fork was given; the
        // middle verb gets what the tines answered.
        Verb::Fork(f, _, h) => down(f).into_iter().chain(down(h)).collect(),
        Verb::NounFork(_, _, h) => down(h),
        // `f@:g`: g is applied to the arguments, in the site's valence.
        Verb::Atop(_, g) => down(g),
        // A declaration, a cache, a tolerance and a fallback all apply the
        // verb they wrap to the arguments they were given.
        Verb::Fit(u, _)
        | Verb::Memo(u, _)
        | Verb::WithObverse(u, _)
        | Verb::Adverse(u, _) => down(u),
        // The first application of a power gets the arguments themselves.
        // Zero applications is the identity and applies nothing.
        Verb::PowerN(u, Power::Times(n)) if *n >= 1 => down(u),
        Verb::Commute(u) => {
            // Monadically `u~ y` is `y u y`: one argument reaching both
            // sides of a dyad. Dyadically the two are swapped.
            let inner = receivers(u, Valence::Dyad, depth + 1);
            inner
                .into_iter()
                .filter(Attr::whole)
                .map(|mut a| {
                    let (sx, sy) = match valence {
                        Valence::Monad => (Src::Y, Src::Y),
                        Valence::Dyad => (Src::Y, Src::X),
                    };
                    a.x = Some((sx, Take::WHOLE));
                    a.y = (sy, Take::WHOLE);
                    a
                })
                .collect()
        }
        // `u/ y` inserts u between the ITEMS of y: a dyadic application,
        // whatever the site's valence, and both its arguments are items.
        // The dyadic form is a different function in each language, and is
        // left to the operator table.
        Verb::Reduce(u) | Verb::NWise(u) if valence == Valence::Monad => {
            let item = Take { cell: None, item: true };
            receivers(u, Valence::Dyad, depth + 1)
                .into_iter()
                .filter(Attr::whole)
                .map(|mut a| {
                    a.prov = Prov::OnCells;
                    a.x = Some((Src::Y, item));
                    a.y = (Src::Y, item);
                    a
                })
                .collect()
        }
        // `u"r`: the same valence, on cells of the named rank. An infinite
        // rank is the whole argument, and is a direct application. This is
        // also how APL's `/` arrives — a reduction along the last axis is
        // an insertion at rank 1.
        Verb::Rank(u, r) => {
            let (rx, ry) = match valence {
                Valence::Monad => (RANK_INF, r[0]),
                Valence::Dyad => (r[1], r[2]),
            };
            down(u)
                .into_iter()
                .filter(Attr::takes_rank)
                .map(|mut a| {
                    a.x = a.x.map(|(s, t)| (s, t.within(rx)));
                    a.y = (a.y.0, a.y.1.within(ry));
                    if !a.whole() {
                        a.prov = Prov::OnCells;
                    }
                    a
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

// --------------------------------------------------------- the operator layer

/// One row of the operator table: how it reads, and the spellings of the
/// published vocabulary it answers for. The IR keeps one node for a family
/// of spellings — J's `@` and `@:` are one atop, and its `&` between two
/// verbs is a compose — so a row can stand for more than one.
#[derive(Clone, Copy, Debug)]
struct Mod {
    label: &'static str,
    spellings: &'static [&'static str],
}

const fn m(label: &'static str, spellings: &'static [&'static str]) -> Mod {
    Mod { label, spellings }
}

/// The row an explicit definition is counted under. A definition is not a
/// modifier, but J spells one with a conjunction and the inventory has a
/// row for it.
const EXPLICIT: Mod = m("explicit definition", &[":"]);

/// Spellings the frontend rewrites into another form, so that no node of
/// the IR carries them and this measurement cannot see them. They are
/// named here so the report can say so rather than call them unexercised.
pub const REWRITTEN: &[&str] = &["&.", "&.:", "f.", "!:", "$."];

/// How a modifier spells itself, and what it was given. `None` for a verb
/// that is not a modifier at all.
fn modifier(v: &Verb, lang: Lang) -> Option<(Mod, Vec<String>)> {
    let apl = lang == Lang::Apl;
    let verbs = |vs: &[&Verb]| -> Vec<String> { vs.iter().map(|v| verb_class(v)).collect() };
    let with = |v: &Verb, extra: String| vec![verb_class(v), extra];
    Some(match v {
        Verb::Rank(u, r) => {
            (m("rank", &["\"", "⍤"]), with(u, format!("rank {}", ranks_label(*r))))
        }
        // APL's outer product is an insertion too, and the IR keeps one
        // node for both: `f/` and `∘.f` differ by the valence of the site.
        Verb::Reduce(u) | Verb::NWise(u) => (
            if apl {
                m("reduce / outer product", &["/", "⌿", "∘."])
            } else {
                m("insert / table", &["/"])
            },
            verbs(&[u]),
        ),
        Verb::Windowed(u, kind) => (
            if apl { m("scan", &["\\", "⍀"]) } else { m("window", &["\\", "\\."]) },
            with(u, window_label(*kind).to_string()),
        ),
        Verb::Commute(u) => (m("commute", &["~", "⍨"]), verbs(&[u])),
        Verb::PowerN(u, p) => (m("power", &["^:", "⍣"]), with(u, power_label(p))),
        Verb::PowerV(u, w) => (m("power", &["^:", "⍣"]), verbs(&[u, w])),
        Verb::PowerUntil(u, w) => (m("power", &["^:", "⍣"]), verbs(&[u, w])),
        Verb::Fork(f, g, h) => (m("fork (f g h)", &[]), verbs(&[f, g, h])),
        Verb::NounFork(n, g, h) => {
            let mut ops = vec![noun_class(n)];
            ops.extend(verbs(&[g, h]));
            (m("fork (n g h)", &[]), ops)
        }
        Verb::Hook(f, g) => (m("hook (f g)", &[]), verbs(&[f, g])),
        Verb::Atop(f, g) => (m("atop", &["@", "@:", "[:"]), verbs(&[f, g])),
        Verb::Compose(f, g) => (m("compose", &["&", "&:", "⍥"]), verbs(&[f, g])),
        Verb::Beside(f, g) => (m("beside", &["∘"]), verbs(&[f, g])),
        Verb::Before(f, g) => (m("before", &["⍛"]), verbs(&[f, g])),
        Verb::BondLeft(n, u) => (m("bond", &["&"]), vec![noun_class(n), verb_class(u)]),
        Verb::BondRight(u, n) => (m("bond", &["&"]), vec![verb_class(u), noun_class(n)]),
        Verb::Each(u, _) => (
            if apl { m("each", &["¨"]) } else { m("under-open", &["&.>"]) },
            verbs(&[u]),
        ),
        Verb::Fit(u, n) => (m("fit", &["!."]), with(u, format!("fill {n}"))),
        Verb::Amend(n) => (m("amend", &["}"]), vec![noun_class(n)]),
        Verb::AmendVerb(u) => (m("amend", &["}"]), verbs(&[u])),
        Verb::ShiftFill(n) => (m("shift", &["!."]), vec![noun_class(n)]),
        Verb::Memo(u, _) => (m("memo", &["M."]), verbs(&[u])),
        Verb::WithObverse(u, w) => (m("obverse", &[":."]), verbs(&[u, w])),
        Verb::Adverse(u, w) => (m("adverse", &["::"]), verbs(&[u, w])),
        Verb::Level { u, level, spread } => (
            if *spread { m("spread", &["S:"]) } else { m("level", &["L:"]) },
            with(u, format!("level {level}")),
        ),
        Verb::Characteristics(u) => (m("characteristics", &["b."]), verbs(&[u])),
        Verb::Key(u) => (m("key / oblique", &["/."]), verbs(&[u])),
        Verb::KeyPairs(u) => (m("key", &["⌸"]), verbs(&[u])),
        Verb::Cut(u, n) => (m("cut", &[";."]), with(u, format!("fret {n}"))),
        Verb::AlongAxis(u, k) => (m("along axis", &["[]"]), with(u, format!("axis {k}"))),
        Verb::Stencil(u, w) => (m("stencil", &["⌺"]), with(u, format!("window {w:?}"))),
        Verb::Agenda(gs, w) => {
            let mut ops: Vec<String> = gs.iter().map(verb_class).collect();
            ops.push(verb_class(w));
            (m("agenda", &["@."]), ops)
        }
        Verb::Evoke(gs, n) => {
            let mut ops: Vec<String> = gs.iter().map(verb_class).collect();
            ops.push(format!("form {n}"));
            (m("evoke gerund", &["`", "`:"]), ops)
        }
        Verb::InnerProduct { u, v, .. } => (m("inner product", &["."]), verbs(&[u, v])),
        Verb::UserDerived { def, alpha, omega } => {
            let operand = |o: &jay::verb::Operand| match o {
                jay::verb::Operand::Func(v) => verb_class(v),
                jay::verb::Operand::Value(_) => "an array operand".to_string(),
            };
            let mut ops = Vec::new();
            if let Ok(body) = def.pick(alpha, omega.as_ref()) {
                ops.push(verb_class(body));
            }
            ops.push(operand(alpha));
            if let Some(g) = omega {
                ops.push(operand(g));
            }
            (m("a user-defined operator", &[]), ops)
        }
        Verb::Hypergeometric { .. } => (m("hypergeometric", &["H."]), Vec::new()),
        _ => return None,
    })
}

fn window_label(kind: jay::verb::WindowKind) -> &'static str {
    match kind {
        jay::verb::WindowKind::Prefix => "prefix / infix",
        jay::verb::WindowKind::Suffix => "suffix / outfix",
        jay::verb::WindowKind::Scan => "scan",
    }
}

/// The operand verbs a derived verb was built from, for the walk to
/// continue through.
fn operand_verbs(v: &Verb) -> Vec<&Verb> {
    match v {
        Verb::Rank(u, _)
        | Verb::Reduce(u)
        | Verb::NWise(u)
        | Verb::Windowed(u, _)
        | Verb::Commute(u)
        | Verb::PowerN(u, _)
        | Verb::BondLeft(_, u)
        | Verb::BondRight(u, _)
        | Verb::Each(u, _)
        | Verb::Fit(u, _)
        | Verb::AmendVerb(u)
        | Verb::Memo(u, _)
        | Verb::Level { u, .. }
        | Verb::Characteristics(u)
        | Verb::Key(u)
        | Verb::KeyPairs(u)
        | Verb::Cut(u, _)
        | Verb::AlongAxis(u, _)
        | Verb::Stencil(u, _) => vec![u],
        Verb::Fork(f, g, h) => vec![f, g, h],
        Verb::NounFork(_, g, h) => vec![g, h],
        Verb::Hook(f, g)
        | Verb::Atop(f, g)
        | Verb::Compose(f, g)
        | Verb::Beside(f, g)
        | Verb::Before(f, g)
        | Verb::PowerV(f, g)
        | Verb::PowerUntil(f, g)
        | Verb::WithObverse(f, g)
        | Verb::Adverse(f, g) => vec![f, g],
        Verb::InnerProduct { u, v, .. } => vec![u, v],
        Verb::Agenda(gs, w) => gs.iter().chain(std::iter::once(&**w)).collect(),
        Verb::Evoke(gs, _) => gs.iter().collect(),
        Verb::UserDerived { def, alpha, omega } => {
            fn func(o: &jay::verb::Operand) -> Option<&Verb> {
                match o {
                    jay::verb::Operand::Func(v) => Some(&**v),
                    jay::verb::Operand::Value(_) => None,
                }
            }
            let mut vs: Vec<&Verb> = def.pick(alpha, omega.as_ref()).into_iter().collect();
            vs.extend(func(alpha));
            vs.extend(omega.as_ref().and_then(func));
            vs
        }
        _ => Vec::new(),
    }
}

/// How an operand verb is named in the operator table: by its spelling
/// where it is one primitive, and by its kind where it is not.
fn verb_class(v: &Verb) -> String {
    match v {
        Verb::Prim(p) => p.name.to_string(),
        Verb::Explicit(_) => "an explicit definition".to_string(),
        Verb::Fork(..) | Verb::NounFork(..) | Verb::Hook(..) => "a train".to_string(),
        Verb::Named(_) => "a named verb".to_string(),
        Verb::SelfRef => "self-reference".to_string(),
        _ => "a derived verb".to_string(),
    }
}

fn noun_class(a: &Array) -> String {
    format!("noun {}", Val::Known(a.clone()).class(Take::WHOLE).label())
}

fn ranks_label(r: [i64; 3]) -> String {
    let one = |n: i64| if n == RANK_INF { "_".to_string() } else { n.to_string() };
    if r[0] == r[1] && r[1] == r[2] {
        one(r[0])
    } else {
        format!("{} {} {}", one(r[0]), one(r[1]), one(r[2]))
    }
}

fn power_label(p: &Power) -> String {
    match p {
        Power::Times(n) => format!("count {n}"),
        Power::Converge => "converge".to_string(),
        Power::Each(_) => "a list of counts".to_string(),
        Power::ConvergeTrace => "converge, traced".to_string(),
        Power::Inverse(n) => format!("count _{n}"),
    }
}

// ------------------------------------------------------------- the reports

/// The terminal report: readable, capped, and saying what it measured.
pub fn report(lang: Lang, cov: &Coverage, inv: &Inventory, top: usize) -> String {
    let mut out = String::new();
    let name = libjay_testkit::lang_name(lang);
    let _ = writeln!(out, "{name} — what the recorded corpus exercises\n");
    let _ = writeln!(
        out,
        "corpus  {} files, {} expressions; {} libjay would not compile",
        cov.files, cov.exprs, cov.refused
    );
    let _ = writeln!(
        out,
        "sites   {} verb applications: {} attributable to a primitive (yielding {} \
         attributions, a train having more than one), {} not — the applied verb \
         hands its operand something this measurement cannot name; {} of all sites \
         are inside a definition, where no operand can be run",
        cov.sites,
        cov.attributed,
        cov.applied.values().sum::<usize>(),
        cov.opaque,
        cov.in_definition
    );
    let mono = cov.universe(Valence::Monad);
    let dyad = cov.universe(Valence::Dyad);
    let _ = writeln!(
        out,
        "classes {} type-classes × {} rank-classes; the corpus builds {} distinct \
         monadic operand classes and {} distinct dyadic operand pairs",
        Ty::ALL.len() - 2,
        Rk::ALL.len() - 1,
        mono.len(),
        dyad.len()
    );
    let _ = writeln!(
        out,
        "\ntaxonomy\n  type  {}\n  rank  {}",
        Ty::ALL.iter().filter(|t| t.known()).map(|t| t.label()).collect::<Vec<_>>().join(" "),
        Rk::ALL.iter().filter(|r| r.known()).map(|r| r.label()).collect::<Vec<_>>().join(" ")
    );
    out.push_str(
        "  a cell is one primitive in one valence meeting one operand class \
         (a pair, for a dyad).\n  the denominator is the corpus's own reach: \
         the cells it builds for SOME primitive.\n",
    );

    inventory_section(cov, inv, &mut out);
    for valence in [Valence::Monad, Valence::Dyad] {
        let universe = if valence == Valence::Monad { &mono } else { &dyad };
        grid_section(cov, valence, universe, top, &mut out);
    }
    regions_section(cov, &mono, &dyad, &mut out);
    modifier_section(cov, top, &mut out);
    out
}

fn inventory_section(cov: &Coverage, inv: &Inventory, out: &mut String) {
    if inv.is_empty() {
        out.push_str("\ninventory  docs/status.md was not readable — no denominator\n");
        return;
    }
    let _ = writeln!(
        out,
        "\ninventory  docs/status.md: {} verb spellings ({} valences), {} modifiers",
        inv.verbs.len(),
        inv.valences(),
        inv.modifiers.len()
    );
    let unseen: Vec<&str> = inv
        .verbs
        .keys()
        .filter(|s| !cov.mentioned.contains(*s))
        .map(String::as_str)
        .collect();
    let _ = writeln!(
        out,
        "  never mentioned      {} of {}{}",
        unseen.len(),
        inv.verbs.len(),
        list(&unseen)
    );
    let mut one_valence: Vec<String> = Vec::new();
    for (spelling, row) in &inv.verbs {
        if !cov.mentioned.contains(spelling) {
            continue;
        }
        let has = |v: Valence| cov.applied.contains_key(&(spelling.clone(), v));
        let missing = [
            (row.monad && !has(Valence::Monad)).then_some("monad"),
            (row.dyad && !has(Valence::Dyad)).then_some("dyad"),
        ];
        let missing: Vec<&str> = missing.into_iter().flatten().collect();
        if !missing.is_empty() {
            one_valence.push(format!("{spelling} ({})", missing.join(", ")));
        }
    }
    let refs: Vec<&str> = one_valence.iter().map(String::as_str).collect();
    let _ = writeln!(
        out,
        "  valence never met    {}{}",
        one_valence.len(),
        list(&refs)
    );
    let seen_spellings: BTreeSet<&str> =
        cov.modifiers.values().flat_map(|m| m.spellings.iter().copied()).collect();
    let unseen_mods: Vec<&str> = inv
        .modifiers
        .keys()
        .filter(|s| !seen_spellings.contains(s.as_str()) && !REWRITTEN.contains(&s.as_str()))
        .map(String::as_str)
        .collect();
    let _ = writeln!(
        out,
        "  modifiers unseen     {} of {}{}",
        unseen_mods.len(),
        inv.modifiers.len(),
        list(&unseen_mods)
    );
    let blind: Vec<&str> = inv
        .modifiers
        .keys()
        .filter(|s| REWRITTEN.contains(&s.as_str()))
        .map(String::as_str)
        .collect();
    if !blind.is_empty() {
        let _ = writeln!(
            out,
            "  not visible here     {}   (the frontend rewrites them into another form, \
             so no node carries the spelling)",
            blind.join(" ")
        );
    }
    out.push_str(
        "  a spelling is `mentioned` when it appears anywhere, even where the \
         modifier above it\n  hides what it met; a valence is `met` when a site \
         could be attributed to it.\n",
    );
}

/// How many of a list to print before saying how many are left.
const LIST_CAP: usize = 40;

fn list(items: &[&str]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let shown: Vec<&str> = items.iter().take(LIST_CAP).copied().collect();
    let rest = items.len().saturating_sub(shown.len());
    let more = if rest > 0 { format!(", and {rest} more") } else { String::new() };
    format!("   {}{more}", shown.join(" "))
}

fn grid_section(
    cov: &Coverage,
    valence: Valence,
    universe: &BTreeSet<Cell>,
    top: usize,
    out: &mut String,
) {
    let rows = cov.rows(valence);
    let _ = writeln!(
        out,
        "\n{} grid — {} primitives with an attributed application, \
         {} operand classes in reach",
        valence.label(),
        rows.len(),
        universe.len()
    );
    if rows.is_empty() {
        return;
    }
    let mut measured: Vec<(usize, usize, &str)> = rows
        .iter()
        .map(|p| {
            let cells = cov.grid.get(&(p.to_string(), valence)).map_or(0, |c| {
                c.keys().filter(|k| k.known()).count()
            });
            let sites = *cov.applied.get(&(p.to_string(), valence)).unwrap_or(&0);
            (cells, sites, *p)
        })
        .collect();
    // The interesting row is the one used often and narrowly: a primitive
    // the corpus leans on while showing it one kind of argument.
    measured.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    out.push_str("  verb      sites  cells     of   commonest operand\n");
    for (cells, sites, prim) in measured.iter().take(top) {
        let commonest = cov
            .grid
            .get(&(prim.to_string(), valence))
            .and_then(|c| c.iter().filter(|(k, _)| k.known()).max_by_key(|(_, h)| h.total()))
            .map(|(cell, h)| format!("{} ({})", cell.label(), h.total()))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "  {:<8} {sites:>6} {cells:>6} {:>6}   {commonest}",
            pad(prim, 8),
            universe.len()
        );
    }
    let fullest = measured.iter().rev().take(3);
    let _ = writeln!(
        out,
        "  widest: {}",
        fullest
            .map(|(c, s, p)| format!("{p} ({c} cells, {s} sites)"))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn regions_section(
    cov: &Coverage,
    mono: &BTreeSet<Cell>,
    dyad: &BTreeSet<Cell>,
    out: &mut String,
) {
    out.push_str("\nemptiest regions — how many primitives ever met each class\n");
    let mut by_ty: BTreeMap<Ty, BTreeSet<String>> = BTreeMap::new();
    let mut by_rk: BTreeMap<Rk, BTreeSet<String>> = BTreeMap::new();
    for ((prim, _), cells) in &cov.grid {
        for cell in cells.keys() {
            for class in [Some(cell.y), cell.x].into_iter().flatten() {
                if class.known() {
                    by_ty.entry(class.ty).or_default().insert(prim.clone());
                    by_rk.entry(class.rk).or_default().insert(prim.clone());
                }
            }
        }
    }
    let prims: BTreeSet<&String> = cov.grid.keys().map(|(p, _)| p).collect();
    let mut ty_rows: Vec<(usize, &'static str)> =
        Ty::ALL.iter().filter(|t| t.known()).map(|t| (by_ty.get(t).map_or(0, BTreeSet::len), t.label())).collect();
    ty_rows.sort();
    let mut rk_rows: Vec<(usize, &'static str)> =
        Rk::ALL.iter().filter(|r| r.known()).map(|r| (by_rk.get(r).map_or(0, BTreeSet::len), r.label())).collect();
    rk_rows.sort();
    let _ = writeln!(
        out,
        "  type  {}",
        ty_rows.iter().map(|(n, l)| format!("{l} {n}")).collect::<Vec<_>>().join(", ")
    );
    let _ = writeln!(
        out,
        "  rank  {}",
        rk_rows.iter().map(|(n, l)| format!("{l} {n}")).collect::<Vec<_>>().join(", ")
    );
    let _ = writeln!(
        out,
        "  of {} primitives with an attributed application; \
         a class no row reaches is one the corpus never puts under a verb",
        prims.len()
    );
    let empties = |valence: Valence, universe: &BTreeSet<Cell>| -> usize {
        cov.rows(valence)
            .iter()
            .map(|p| cov.empty_cells(p, valence, universe).len())
            .sum()
    };
    let _ = writeln!(
        out,
        "  empty cells: {} monadic, {} dyadic (--tsv writes them out)",
        empties(Valence::Monad, mono),
        empties(Valence::Dyad, dyad)
    );
}

fn modifier_section(cov: &Coverage, top: usize, out: &mut String) {
    let _ = writeln!(
        out,
        "\noperator layer — {} modifier spellings, {} occurrences",
        cov.modifiers.len(),
        cov.modifiers.values().map(|m| m.sites).sum::<usize>()
    );
    let mut mods: Vec<(&String, &ModStat)> = cov.modifiers.iter().collect();
    mods.sort_by_key(|(_, m)| std::cmp::Reverse(m.sites));
    for (label, stat) in mods.iter().take(top) {
        let _ = writeln!(out, "  {} — {} occurrences", label, stat.sites);
        let _ = writeln!(out, "      operands  {}", head(&stat.operands, 6));
        let _ = writeln!(out, "      applied to {}", head(&stat.nouns, 4));
    }
    if cov.modifiers.len() > top {
        let rest: Vec<&str> =
            mods.iter().skip(top).map(|(l, _)| l.as_str()).collect();
        let _ = writeln!(out, "  the rest: {}", rest.join(", "));
    }
}

/// The commonest few entries of a tally, as text.
fn head(counts: &BTreeMap<String, usize>, n: usize) -> String {
    let mut items: Vec<(&String, &usize)> = counts.iter().collect();
    items.sort_by_key(|(k, c)| (std::cmp::Reverse(**c), (*k).clone()));
    let shown: Vec<String> =
        items.iter().take(n).map(|(k, c)| format!("{k} ({c})")).collect();
    let rest = items.len().saturating_sub(shown.len());
    if shown.is_empty() {
        return "—".to_string();
    }
    let more = if rest > 0 { format!(", and {rest} more") } else { String::new() };
    format!("{}{more}", shown.join(", "))
}

/// Pad to a width counted in characters, since APL's glyphs are not bytes.
fn pad(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width { s.to_string() } else { format!("{s}{}", " ".repeat(width - n)) }
}

/// The empty-cell list, one cell per line: what a later generation stage
/// reads to know what it has to build.
pub fn tsv(lang: Lang, cov: &Coverage) -> String {
    let mut out = String::from("lang\tprimitive\tvalence\tx_type\tx_rank\ty_type\ty_rank\n");
    let dir = libjay_testkit::lang_dir(lang);
    for valence in [Valence::Monad, Valence::Dyad] {
        let universe = cov.universe(valence);
        for prim in cov.rows(valence) {
            for cell in cov.empty_cells(prim, valence, &universe) {
                let (xt, xr) = match cell.x {
                    Some(x) => (x.ty.label(), x.rk.label()),
                    None => ("", ""),
                };
                let _ = writeln!(
                    out,
                    "{dir}\t{prim}\t{}\t{xt}\t{xr}\t{}\t{}",
                    valence.label(),
                    cell.y.ty.label(),
                    cell.y.rk.label()
                );
            }
        }
    }
    out
}

/// The whole measurement as JSON: the occupied cells with their counts,
/// the empty ones, and the operator layer.
pub fn json(lang: Lang, cov: &Coverage, inv: &Inventory) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{{\n  \"language\": \"{}\",", libjay_testkit::lang_dir(lang));
    let _ = writeln!(
        out,
        "  \"corpus\": {{\"files\": {}, \"expressions\": {}, \"refused\": {}}},",
        cov.files, cov.exprs, cov.refused
    );
    let _ = writeln!(
        out,
        "  \"sites\": {{\"total\": {}, \"attributed\": {}, \"unattributable\": {}, \
         \"in_definition\": {}}},",
        cov.sites,
        cov.applied.values().sum::<usize>(),
        cov.opaque,
        cov.in_definition
    );
    let _ = writeln!(
        out,
        "  \"inventory\": {{\"verbs\": {}, \"valences\": {}, \"modifiers\": {}}},",
        inv.verbs.len(),
        inv.valences(),
        inv.modifiers.len()
    );
    let _ = write!(out, "  \"taxonomy\": {{\"type\": [");
    let tys: Vec<String> =
        Ty::ALL.iter().map(|t| format!("\"{}\"", t.label())).collect();
    let rks: Vec<String> = Rk::ALL.iter().map(|r| format!("\"{}\"", r.label())).collect();
    let _ = writeln!(out, "{}], \"rank\": [{}]}},", tys.join(", "), rks.join(", "));

    out.push_str("  \"occupied\": [\n");
    let mut first = true;
    for ((prim, valence), cells) in &cov.grid {
        for (cell, hits) in cells {
            if !first {
                out.push_str(",\n");
            }
            first = false;
            let _ = write!(
                out,
                "    {{\"primitive\": {}, \"valence\": \"{}\", {}, \
                 \"direct\": {}, \"on_cells\": {}}}",
                quote(prim),
                valence.label(),
                cell_json(*cell),
                hits.direct,
                hits.on_cells
            );
        }
    }
    out.push_str("\n  ],\n  \"empty\": [\n");
    let mut first = true;
    for valence in [Valence::Monad, Valence::Dyad] {
        let universe = cov.universe(valence);
        for prim in cov.rows(valence) {
            for cell in cov.empty_cells(prim, valence, &universe) {
                if !first {
                    out.push_str(",\n");
                }
                first = false;
                let _ = write!(
                    out,
                    "    {{\"primitive\": {}, \"valence\": \"{}\", {}}}",
                    quote(prim),
                    valence.label(),
                    cell_json(cell)
                );
            }
        }
    }
    out.push_str("\n  ],\n  \"modifiers\": [\n");
    let mut first = true;
    for (label, stat) in &cov.modifiers {
        if !first {
            out.push_str(",\n");
        }
        first = false;
        let _ = write!(
            out,
            "    {{\"modifier\": {}, \"sites\": {}, \"operands\": {}, \"nouns\": {}}}",
            quote(label),
            stat.sites,
            counts_json(&stat.operands),
            counts_json(&stat.nouns)
        );
    }
    out.push_str("\n  ]\n}\n");
    out
}

fn cell_json(cell: Cell) -> String {
    let x = match cell.x {
        Some(x) => format!("[\"{}\", \"{}\"]", x.ty.label(), x.rk.label()),
        None => "null".to_string(),
    };
    format!("\"x\": {x}, \"y\": [\"{}\", \"{}\"]", cell.y.ty.label(), cell.y.rk.label())
}

fn counts_json(counts: &BTreeMap<String, usize>) -> String {
    let items: Vec<String> =
        counts.iter().map(|(k, n)| format!("{}: {n}", quote(k))).collect();
    format!("{{{}}}", items.join(", "))
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Read the inventory for one language.
pub fn inventory_of(lang: Lang) -> Inventory {
    inventory::read(lang)
}

#[cfg(test)]
mod tests;
