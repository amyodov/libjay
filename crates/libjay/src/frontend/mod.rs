//! Language frontends. Each parses its own syntax into the shared IR.

pub mod apl;
pub mod j;

use crate::error::{Error, ErrorKind, Result};
use crate::extensions::Extensions;
use crate::fmt::FmtOpts;
use crate::ir::{ParamSpec, Program};
use crate::verb::{Agreement, Tol};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    J,
    Apl,
}

impl Lang {
    pub fn from_name(name: &str) -> Option<Lang> {
        match name.to_ascii_lowercase().as_str() {
            "j" => Some(Lang::J),
            "apl" => Some(Lang::Apl),
            _ => None,
        }
    }
}

/// How a nested array holds a simple scalar.
///
/// APL2 and the ISO standard float: `⊂` on a simple scalar is the scalar
/// itself, because a simple scalar cannot be nested. The other reading
/// grounds it, so `⊂3` is a one-item enclosure distinct from `3`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NestedModel {
    #[default]
    Floating,
    Grounded,
}

/// What `↑` and `⊃` mean monadically.
///
/// The APL2 line reads `↑` as first and `⊃` as disclose. The other line
/// reads `↑` as mix and `⊃` as first. The dyads (take and pick) agree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FirstDisclose {
    #[default]
    UpIsFirst,
    UpIsMix,
}

/// What `⌷` means.
///
/// APL2's `⌷` indexes with one scalar per axis and has no monadic case.
/// The other line reads the left argument as a list of index vectors, one
/// per axis, and gives `⌷` a monadic meaning as well.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IndexForm {
    #[default]
    ScalarPerAxis,
    AxisVectors,
}

/// What a dyadic `⊂` does.
///
/// The APL2 line reads the left argument as partition flags: a partition
/// begins where the flags rise, and a zero drops its item. The other line
/// reads them as counts — each item says how many partitions to begin
/// before it, so a count above one leaves empty partitions behind — and
/// spells the flag reading `⊆`. Both lines agree about `⊆`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Partition {
    #[default]
    Flags,
    Counts,
}

/// What monadic `≡` answers for an array whose items differ in depth.
///
/// Both lines answer with the depth. The other one negates it where the
/// array is not uniform: where two items of it, at any level, differ in
/// depth or in shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DepthSign {
    #[default]
    Unsigned,
    Signed,
}

/// Which sentence of a dfn body is its result.
///
/// libjay's block model — the value of the last sentence — is what both
/// languages' sequences do. The other reading stops at the first sentence
/// that is not an assignment and answers with its value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DfnResult {
    #[default]
    LastSentence,
    FirstNonAssignment,
}

/// When `⍺←v` evaluates `v`.
///
/// Eagerly: the sentence runs and the value is dropped where the left
/// argument already arrived. Lazily: the sentence does not run at all
/// then, which is observable when it has an effect or would fail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DefaultArg {
    #[default]
    Eager,
    Lazy,
}

/// How a grade orders complex values.
///
/// Ordering verbs refuse complex operands in either reading — a grade is a
/// permutation, not a claim about size — but a grade still has to be
/// total. By real part then imaginary is one reading; by magnitude then
/// angle is the other.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ComplexOrder {
    #[default]
    RealThenImaginary,
    MagnitudeThenAngle,
}

/// What `<`, `≤`, `≥` and `>` are allowed to order.
///
/// GNU APL's comparison rules are total: characters order by their
/// codepoint, a character stands below every number, and a complex value
/// orders by its real part and then its imaginary one. Dyalog and J refuse
/// all three — an order there is a claim about size, and only a real
/// number has one. `⌈` and `⌊` keep the narrow reading in both, so the
/// setting names the comparisons alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OrderDomain {
    /// Characters, mixed pairs and complex values all have an order.
    #[default]
    Total,
    /// Only real numbers do; anything else is a type error.
    Numeric,
}

/// How a grade orders NESTED items.
///
/// The APL2 line, which GNU APL implements and the oracle verifies, orders
/// two items by rank, then by shape, then atom by atom with characters
/// before numbers before nested values. Dyalog's total array ordering is a
/// different comparator throughout: it compares the atoms first, padding
/// the shorter array with an item below every type, extends a lower rank
/// with leading 1s, and orders numbers before characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NestedGrade {
    #[default]
    Apl2,
    TotalOrder,
}

/// What dyadic `⍳` and `⍸` take on their left.
///
/// The APL2/ISO line, which GNU APL implements, looks a cell up among the
/// ELEMENTS of a left argument of any rank, so `(2 3⍴⍳6)⍳5` answers with a
/// coordinate vector and a scalar left argument is a one-item table; `⍸`
/// there takes a scalar bound and refuses rank 2 and above. Dyalog reads
/// the left argument as a list of MAJOR CELLS in both verbs, so a matrix
/// searches rows and answers one number per cell of the right argument,
/// and a scalar — having no major cells — is a RANK ERROR.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LookupLeft {
    #[default]
    AnyRank,
    MajorCells,
}

/// How long the left argument of `↑` and `↓` may be.
///
/// GNU APL wants exactly one count per axis of the right argument. Dyalog
/// takes that many or fewer, the counts applying to the LEADING axes and
/// every axis they do not reach being taken whole and dropped from not at
/// all — which is what makes `2↑matrix` the first two rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AxisCounts {
    /// GNU APL: one count per axis, exactly.
    #[default]
    PerAxis,
    /// Dyalog: the leading axes, and as few of them as the program likes.
    Leading,
}

/// How the axes named in `f[K]` line up with what accompanies them, where
/// the function pairs one thing per axis.
///
/// `↑` `↓` `,` and `⊂` read K in the order it was written in both
/// references — `1 2↑[3 1]Y` takes 1 along axis 3 and 2 along axis 1.
/// `⌷` and the scalar functions do not agree: GNU APL reads their K as a
/// SET, so `2 1⌷[2 1]Y` indexes row 2 and column 1 exactly as `[1 2]`
/// would, while Dyalog pairs them in the order written and answers the
/// other element.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AxisOrder {
    /// GNU APL: ascending, whatever order the brackets named.
    #[default]
    Ascending,
    /// Dyalog: the order the brackets named.
    AsWritten,
}

/// What monadic `≠` counts.
///
/// GNU APL runs the sieve over the ELEMENTS in ravel order and keeps the
/// argument's shape, so a matrix answers a matrix. Dyalog runs it over the
/// MAJOR CELLS and always answers a vector as long as `≢Y` — one bit per
/// row of a matrix, and a one-element vector for a scalar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UniqueMask {
    #[default]
    Elements,
    MajorCells,
}

/// What dyadic `\` takes on its left.
///
/// GNU APL takes a boolean mask alone: a 1 passes the next item on, a 0
/// leaves a fill. Dyalog takes any simple integer vector — a positive count
/// repeats that item, a negative one leaves that many fills, and 0 means
/// `¯1` — so the result is `+/1⌈|X` items long. `/` reads its left argument
/// that way in both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Expansion {
    #[default]
    Boolean,
    Counts,
}

/// What shape monadic `⍸` gives an index.
///
/// Both lines answer a vector of indices for a vector argument and a vector
/// of coordinate vectors above rank 1. They part over rank 0, where the
/// coordinate vector is EMPTY: GNU APL answers the plain index anyway, so
/// `⍸1` is the number 1, and Dyalog follows the rank, so `⍸1` is a one-item
/// nested vector holding `⍬`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WhereRank {
    /// GNU APL: a scalar argument answers a plain index.
    #[default]
    Flattened,
    /// Dyalog: an index is a vector as long as the rank, rank 0 included.
    ByRank,
}

/// How dyadic `⍕` lays a number out.
///
/// Four rules of the one page move together. GNU APL measures a half on the
/// double scaled by the precision, so `4 2⍕1.005` is `1.00`; writes a
/// one-digit mantissa without a point, so `0 ¯1⍕123.45` is `1E2`;
/// right-justifies the scaled form in its field; and refuses a value too
/// wide for the field it was given. Dyalog measures the half on the
/// shortest decimal that names the double (`1.01`); keeps the mantissa's
/// point (`1.E2`); reserves four characters after the `E`, so a given width
/// pads the exponent out before it pads the front; and fills a field too
/// narrow with asterisks rather than refusing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormatSpec {
    #[default]
    Plain,
    Padded,
}

/// Which line's `∨` and `∧` these are.
///
/// GNU APL's GCD reads three things loosely, all probed against it: a zero
/// argument hands its partner back with the sign (`¯3∨0` is `¯3`, though
/// `¯3.5∨0` is `3.5` — only whole numbers keep it); an argument within
/// `⎕CT` of a whole number is that number (`1.0000000000001∧5` is 5); and
/// one no larger than `⎕CT` beside the other is zero (`1E¯14∨1` is 1).
/// Dyalog does none of the three, and neither does J: `¯3∨0` is 3 there,
/// `1E¯14∨1` is `1E¯14`, and `1.0000000000001∧5` grinds out `1.0008E13`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GcdRule {
    /// GNU APL: whole-number sign kept, near-whole and vanishing arguments
    /// rounded first.
    #[default]
    Tolerant,
    /// Dyalog and J: the magnitude, and the values as they stand.
    Exact,
}

/// How a float that is merely NEAR a whole number is admitted where a
/// count, a length or an index belongs (`⍳2+9E¯11`, `(2+9E¯11)⍴5`).
///
/// This is not the comparison tolerance — `(2+9E¯11)=2` is 0 under both
/// readings — and the two APL lines part company over it. GNU APL takes an
/// absolute `1E¯10` at every magnitude, so a large count buys no room and
/// `1E¯11` reads as 0. Dyalog's window is relative and follows `⎕CT`, so
/// `⍴⍳1000000+1E¯9` answers there and is refused here, while every
/// `2±9E¯11` case is the other way about. Neither is a superset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NearCount {
    /// GNU APL: an absolute `1E¯10`, whatever the magnitude.
    #[default]
    Absolute,
    /// Dyalog: the dialect's own tolerant equality against the whole
    /// number, so `⎕CT` moves it and zero admits nothing.
    Tolerant,
}

/// How `⌊` and `⌈` read a value that is merely near the integer above or
/// below it.
///
/// GNU APL shifts by `⎕CT` outright, so `⌊99.999999999995` is 99 — a gap
/// of 5E¯12 is larger than the tolerance however big the value is — while
/// `⌊¯1E¯13` is 0. Dyalog scales the shift by the magnitude but never
/// below 1, so `⌊9.9999999999999` is 10 and `⌊¯1E¯13` is `¯1`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FloorRule {
    /// GNU APL: `⌊y+⎕CT`, an absolute shift.
    #[default]
    Shift,
    /// Dyalog: `⌊y+⎕CT×1⌈|y`, a shift that grows with the magnitude.
    Scaled,
}

/// Whether `⊤` reads its digits tolerantly.
///
/// GNU APL takes each digit with the same tolerant residue `|` uses, so
/// `2 2⊤4-1E¯14` is `0 0`. Dyalog takes them exactly, and the difference
/// survives into the digits: the same sentence is `1 2` there, the last
/// digit being 1.99999999999999 rather than a rounded 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EncodeDigits {
    /// GNU APL: the digits are tolerant residues.
    #[default]
    Tolerant,
    /// Dyalog: the digits are exact residues, `⎕CT` unread.
    Exact,
}

/// How strictly a control structure reads what it is given.
///
/// The lenient reading is the one both languages ship: a condition is true
/// where its first atom is, whatever else it holds, and a `:Leave` outside
/// a loop leaves the definition. Dyalog reads both strictly — a condition
/// is one element and no more, and `:Leave` belongs to a loop — and says so
/// rather than answering. GNU APL has no control structures at all, so
/// nothing it records turns on this.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControlStrictness {
    /// The first atom decides, and a stray `:Leave` is a return.
    #[default]
    Lenient,
    /// Dyalog: a condition is a single value, and `:Leave` needs its loop.
    Strict,
}

/// Where the each in the inner product's definition sits.
///
/// `f.g` is a fold over a pairing, and the two lines put the each on
/// different halves of it. GNU APL puts it on the FOLD — `f/¨ (⊂[last]x)
/// ∘.g (⊂[first]y)` — so `g` meets one whole vector from each side and what
/// the fold makes of a pair is enclosed once more. Dyalog puts it on the
/// PAIRING — `f/ row g¨ column` — so `g` meets one element from each side
/// and the fold's own value stands as the cell. `1 2+.,3 4` is `10` under
/// the first and an enclosed `3 7` under the second. The two agree wherever
/// `g` is a scalar function and the fold ends in a number, which is every
/// published use, so `+.×` and the Life idiom's `∨.∧` differ only in depth.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InnerEach {
    /// GNU APL: `f/¨` over the outer product.
    #[default]
    OnFold,
    /// Dyalog: `f/` over `g¨`.
    OnPair,
}

/// Dialect settings supplied by the host.
///
/// This is what a host asks for; [`Rules`] is what the compiler and the
/// engine read. Every field's default is the setting libjay implements, so
/// `Dialect::default()` is the language as it ships and a host that names
/// no setting gets exactly that. `Option` fields mean "the language
/// default", which differs between J and APL.
///
/// The enum fields are the points where the APL lineages diverge. libjay
/// implements the APL2/ISO line that GNU APL embodies; the other arm of
/// each is refused by [`Dialect::rules`] as not implemented yet, so
/// selecting it is honest rather than silently wrong. `trains` is the
/// exception: both of its readings are implemented, so it is a choice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dialect {
    /// APL `⎕IO`. J's index origin is 0 and is not configurable.
    pub index_origin: Option<i64>,
    /// The non-standard extensions in force. This is NOT a dialect setting
    /// — every other field here chooses between readings some reference
    /// implements, while an extension is a departure from all of them — and
    /// it rides on `Dialect` only because that is the one thing a host hands
    /// the compiler. `None` means the process default, which the
    /// environment names (`LIBJAY_J_*`); `Some` overrides it for this
    /// compiler, so an embedding library is never at the mercy of the
    /// environment. See [`crate::extensions`].
    pub extensions: Option<Extensions>,
    /// APL `⎕CT`, J `9!:18`: the relative comparison tolerance.
    pub comparison_tolerance: Option<f64>,
    pub nested_model: NestedModel,
    pub first_disclose: FirstDisclose,
    pub index_form: IndexForm,
    pub partition: Partition,
    pub depth_sign: DepthSign,
    pub dfn_result: DfnResult,
    pub default_arg: DefaultArg,
    pub complex_order: ComplexOrder,
    pub order_domain: OrderDomain,
    pub nested_grade: NestedGrade,
    pub lookup_left: LookupLeft,
    pub axis_counts: AxisCounts,
    pub axis_order: AxisOrder,
    pub unique_mask: UniqueMask,
    pub expansion: Expansion,
    pub where_rank: WhereRank,
    pub format_spec: FormatSpec,
    pub gcd_rule: GcdRule,
    pub near_count: NearCount,
    pub floor_rule: FloorRule,
    pub encode_digits: EncodeDigits,
    pub inner_each: InnerEach,
    pub control_strictness: ControlStrictness,
    /// Whether a function may stand where a value belongs: a run of
    /// functions is then a train, and `F←+/` names one. Both readings are
    /// implemented, so this is a choice and not a gap. It ships on, as an
    /// extension: GNU APL refuses both spellings, and refusing a feature
    /// the oracle merely lacks serves nobody.
    pub trains: bool,
}

impl Default for Dialect {
    fn default() -> Dialect {
        Dialect::gnu_apl()
    }
}

impl Dialect {
    /// The APL libjay implements: the APL2/ISO line GNU APL embodies and
    /// the oracle verifies, plus the extensions listed in
    /// `docs/coverage.md`. Written out rather than derived, so that every
    /// setting's shipped value is stated in one place; it is equal to
    /// `Dialect::default()`, which the tests pin.
    pub fn gnu_apl() -> Dialect {
        Dialect {
            index_origin: None,
            extensions: None,
            comparison_tolerance: None,
            nested_model: NestedModel::Floating,
            first_disclose: FirstDisclose::UpIsFirst,
            index_form: IndexForm::ScalarPerAxis,
            partition: Partition::Flags,
            depth_sign: DepthSign::Unsigned,
            dfn_result: DfnResult::LastSentence,
            default_arg: DefaultArg::Eager,
            complex_order: ComplexOrder::RealThenImaginary,
            order_domain: OrderDomain::Total,
            nested_grade: NestedGrade::Apl2,
            lookup_left: LookupLeft::AnyRank,
            axis_counts: AxisCounts::PerAxis,
            axis_order: AxisOrder::Ascending,
            unique_mask: UniqueMask::Elements,
            expansion: Expansion::Boolean,
            where_rank: WhereRank::Flattened,
            format_spec: FormatSpec::Plain,
            gcd_rule: GcdRule::Tolerant,
            near_count: NearCount::Absolute,
            floor_rule: FloorRule::Shift,
            encode_digits: EncodeDigits::Tolerant,
            inner_each: InnerEach::OnFold,
            control_strictness: ControlStrictness::Lenient,
            trains: true,
        }
    }

    /// The Dyalog line, as far as libjay implements it.
    ///
    /// Every setting here is one the recorded Dyalog answers verify
    /// (`docs/testing.md`); the settings left at the GNU/APL2 reading are
    /// the ones libjay has not derived from a Dyalog answer yet, and
    /// `docs/coverage.md` lists what that still costs. `⎕ML` is Dyalog's
    /// own default, 1, which is what the recording ran under: `↑` mixes
    /// and `⊃` takes the first.
    pub fn dyalog() -> Dialect {
        Dialect {
            index_origin: None,
            extensions: None,
            comparison_tolerance: Some(1e-14),
            nested_model: NestedModel::Floating,
            first_disclose: FirstDisclose::UpIsMix,
            index_form: IndexForm::AxisVectors,
            partition: Partition::Counts,
            depth_sign: DepthSign::Signed,
            dfn_result: DfnResult::FirstNonAssignment,
            default_arg: DefaultArg::Eager,
            complex_order: ComplexOrder::RealThenImaginary,
            order_domain: OrderDomain::Numeric,
            nested_grade: NestedGrade::TotalOrder,
            lookup_left: LookupLeft::MajorCells,
            axis_counts: AxisCounts::Leading,
            axis_order: AxisOrder::AsWritten,
            unique_mask: UniqueMask::MajorCells,
            expansion: Expansion::Counts,
            where_rank: WhereRank::ByRank,
            format_spec: FormatSpec::Padded,
            gcd_rule: GcdRule::Exact,
            near_count: NearCount::Tolerant,
            floor_rule: FloorRule::Scaled,
            encode_digits: EncodeDigits::Exact,
            inner_each: InnerEach::OnPair,
            control_strictness: ControlStrictness::Strict,
            trains: true,
        }
    }

    /// J. Nothing in J is a dialect setting yet beyond the comparison
    /// tolerance, and the APL settings are not read under `Lang::J`, so
    /// J's dialect is the empty one.
    pub fn j() -> Dialect {
        Dialect::default()
    }

    /// Resolve to the settings the compiler and the engine read.
    ///
    /// This is the one place a dialect choice is made. A setting whose
    /// other arm libjay does not implement is refused here, by name, so
    /// that a host selecting it is told rather than quietly given this
    /// dialect's answer.
    pub fn rules(&self, lang: Lang) -> Result<Rules> {
        // A setting is the host's, not the source text's, so these carry
        // no span: there is nothing in the program to point at.
        let refuse = |what: &str| -> Error {
            Error::new(
                ErrorKind::NotYet,
                format!("{what} (the reading of another APL dialect) is not supported yet"),
                None,
            )
            .note("libjay implements the APL2/ISO line, which is the one its oracle verifies")
        };
        if let Some(ct) = self.comparison_tolerance && !(ct.is_finite() && ct >= 0.0) {
            return Err(Error::new(
                ErrorKind::Domain,
                "the comparison tolerance must be a finite value at or above zero",
                None,
            ));
        }
        match self.nested_model {
            NestedModel::Floating => {}
            NestedModel::Grounded => return Err(refuse("a grounded nested array model")),
        }
        match self.default_arg {
            DefaultArg::Eager => {}
            DefaultArg::Lazy => return Err(refuse("a lazy ⍺← default")),
        }
        match self.complex_order {
            ComplexOrder::RealThenImaginary => {}
            ComplexOrder::MagnitudeThenAngle => {
                return Err(refuse("grading complex values by magnitude and angle"))
            }
        }
        let origin = match lang {
            Lang::J => 0,
            Lang::Apl => self.index_origin.unwrap_or(1),
        };
        let ct = self.comparison_tolerance.unwrap_or(match lang {
            Lang::J => Tol::J.ct,
            Lang::Apl => Tol::APL.ct,
        });
        Ok(Rules {
            lang,
            origin,
            ct,
            extensions: self.extensions.unwrap_or_else(Extensions::from_env),
            nested_model: self.nested_model,
            first_disclose: self.first_disclose,
            index_form: self.index_form,
            partition: self.partition,
            depth_sign: self.depth_sign,
            dfn_result: self.dfn_result,
            default_arg: self.default_arg,
            complex_order: self.complex_order,
            order_domain: self.order_domain,
            nested_grade: self.nested_grade,
            lookup_left: self.lookup_left,
            axis_counts: self.axis_counts,
            axis_order: self.axis_order,
            unique_mask: self.unique_mask,
            expansion: self.expansion,
            where_rank: self.where_rank,
            format_spec: self.format_spec,
            gcd_rule: self.gcd_rule,
            near_count: self.near_count,
            floor_rule: self.floor_rule,
            encode_digits: self.encode_digits,
            inner_each: self.inner_each,
            control_strictness: self.control_strictness,
            trains: self.trains,
        })
    }
}

/// A dialect resolved against a language: what the parser and the engine
/// read. Copyable, and carried by every evaluation context, so a rule that
/// only bites at run time (the index origin a key answers with, the order
/// a grade puts complex values in) reads the same setting the parser did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    pub lang: Lang,
    /// The non-standard extensions in force, resolved: the host's set where
    /// it named one, the environment's default where it did not.
    pub extensions: Extensions,
    /// The index origin in force: APL's `⎕IO`, and 0 for J.
    pub origin: i64,
    /// The comparison tolerance in force. `Rules::tol` pairs it with the
    /// language's scaling rule; a verb-local `u!.n` overrides that copy
    /// and not this one.
    pub ct: f64,
    pub nested_model: NestedModel,
    pub first_disclose: FirstDisclose,
    pub index_form: IndexForm,
    pub partition: Partition,
    pub depth_sign: DepthSign,
    pub dfn_result: DfnResult,
    pub default_arg: DefaultArg,
    pub complex_order: ComplexOrder,
    pub order_domain: OrderDomain,
    pub nested_grade: NestedGrade,
    pub lookup_left: LookupLeft,
    pub axis_counts: AxisCounts,
    pub axis_order: AxisOrder,
    pub unique_mask: UniqueMask,
    pub expansion: Expansion,
    pub where_rank: WhereRank,
    pub format_spec: FormatSpec,
    pub gcd_rule: GcdRule,
    pub near_count: NearCount,
    pub floor_rule: FloorRule,
    pub encode_digits: EncodeDigits,
    pub inner_each: InnerEach,
    pub control_strictness: ControlStrictness,
    pub trains: bool,
}

impl Rules {
    /// The dialect's comparison tolerance, with the language's scale.
    pub fn tol(&self) -> Tol {
        Tol { ct: self.ct, by_smaller: self.lang == Lang::J, floor_rule: self.floor_rule }
    }

    /// The host-facing form, for a nested compilation (`⍎`, `".`) that has
    /// to run under the same dialect as the program executing it.
    pub fn dialect(&self) -> Dialect {
        Dialect {
            index_origin: Some(self.origin),
            extensions: Some(self.extensions),
            comparison_tolerance: Some(self.ct),
            nested_model: self.nested_model,
            first_disclose: self.first_disclose,
            index_form: self.index_form,
            partition: self.partition,
            depth_sign: self.depth_sign,
            dfn_result: self.dfn_result,
            default_arg: self.default_arg,
            complex_order: self.complex_order,
            order_domain: self.order_domain,
            nested_grade: self.nested_grade,
            lookup_left: self.lookup_left,
            axis_counts: self.axis_counts,
            axis_order: self.axis_order,
            unique_mask: self.unique_mask,
            expansion: self.expansion,
            where_rank: self.where_rank,
            format_spec: self.format_spec,
            gcd_rule: self.gcd_rule,
            near_count: self.near_count,
            floor_rule: self.floor_rule,
            encode_digits: self.encode_digits,
            inner_each: self.inner_each,
            control_strictness: self.control_strictness,
            trains: self.trains,
        }
    }
}

impl Default for Rules {
    /// J's rules, which is what a context built without a program uses.
    fn default() -> Rules {
        Dialect::default().rules(Lang::J).expect("J's defaults are implemented")
    }
}

/// A source text with interpolation holes split out. Spans in every token
/// and error refer to `display`, where hole `i` reads `{name_i}`.
#[derive(Clone, Debug)]
pub struct SourceParts {
    pub display: String,
    pub segments: Vec<Segment>,
    pub param_names: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum Segment {
    /// Literal source text starting at `offset` in `display`.
    Text { text: String, offset: usize },
    /// Interpolation hole: parameter `index`, shown as `{name}` in `display`.
    Param { index: usize, offset: usize, len: usize },
}

impl SourceParts {
    /// Build from pre-split literal parts with holes between them
    /// (the t-string path). `names[i]` sits between `parts[i]` and
    /// `parts[i+1]`; repeated names share one parameter.
    pub fn from_parts(parts: &[&str], names: &[&str]) -> SourceParts {
        assert_eq!(parts.len(), names.len() + 1, "N parts need N-1 holes");
        let mut display = String::new();
        let mut segments = Vec::new();
        let mut param_names: Vec<String> = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            if !part.is_empty() {
                segments.push(Segment::Text { text: (*part).to_string(), offset: display.len() });
                display.push_str(part);
            }
            if i < names.len() {
                let name = names[i];
                let index = param_names
                    .iter()
                    .position(|n| n == name)
                    .unwrap_or_else(|| {
                        param_names.push(name.to_string());
                        param_names.len() - 1
                    });
                let shown = format!("{{{name}}}");
                segments.push(Segment::Param { index, offset: display.len(), len: shown.len() });
                display.push_str(&shown);
            }
        }
        SourceParts { display, segments, param_names }
    }

    /// Build from a plain string where `{identifier}` outside quotes is an
    /// interpolation hole (the pre-3.14 and Rust runtime path).
    pub fn from_source(src: &str) -> Result<SourceParts> {
        let bytes = src.as_bytes();
        let mut parts: Vec<String> = vec![String::new()];
        let mut names: Vec<String> = Vec::new();
        let mut in_quote = false;
        let mut i = 0;
        while i < src.len() {
            let ch = src[i..].chars().next().unwrap();
            if ch == '\'' {
                in_quote = !in_quote;
                parts.last_mut().unwrap().push(ch);
                i += 1;
                continue;
            }
            if ch == '{' && !in_quote {
                // Exactly `{identifier}` is an interpolation hole. Any other
                // `{` is literal program text: J spells take as `{.`, drop as
                // `}.`, so the brace itself belongs to the language.
                let rest = &src[i + 1..];
                if let Some(end) = rest.find('}') {
                    let name = &rest[..end];
                    if is_identifier(name) {
                        names.push(name.to_string());
                        parts.push(String::new());
                        i += 2 + end;
                        continue;
                    }
                }
            }
            parts.last_mut().unwrap().push(ch);
            i += ch.len_utf8();
        }
        let _ = bytes;
        let part_refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        Ok(SourceParts::from_parts(&part_refs, &name_refs))
    }
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Compile a plain source string (with `{name}` holes) in the given language.
pub fn compile(lang: Lang, source: &str, dialect: &Dialect) -> Result<Program> {
    let sp = SourceParts::from_source(source)?;
    compile_source_parts(lang, sp, dialect)
}

/// Compile pre-split parts (the t-string path).
pub fn compile_parts(
    lang: Lang,
    parts: &[&str],
    names: &[&str],
    dialect: &Dialect,
) -> Result<Program> {
    compile_source_parts(lang, SourceParts::from_parts(parts, names), dialect)
}

fn compile_source_parts(lang: Lang, sp: SourceParts, dialect: &Dialect) -> Result<Program> {
    let rules = dialect.rules(lang)?;
    let tol = rules.tol();
    let (mut stmts, agreement, fmt) = match lang {
        Lang::J => (j::parse(&sp, rules)?, Agreement::LeadingPrefix, FmtOpts::j(rules)),
        Lang::Apl => (apl::parse(&sp, rules)?, Agreement::ExactOrScalar, FmtOpts::APL),
    };
    // Everything after this point walks the tree recursively, so a
    // sentence nested past what a stack holds is refused here rather than
    // taking the process down. The measurement itself does not recurse.
    for stmt in &stmts {
        crate::verb::check_nesting(stmt.depth(), stmt.span())?;
    }
    crate::fuse::pass(&mut stmts, tol);
    let params = sp.param_names.into_iter().map(|name| ParamSpec { name }).collect();
    Ok(Program { stmts, params, display_src: sp.display, agreement, fmt, rules })
}
