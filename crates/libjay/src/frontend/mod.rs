//! Language frontends. Each parses its own syntax into the shared IR.

pub mod apl;
pub mod j;

use crate::error::{Error, ErrorKind, Result};
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
    /// APL `⎕CT`, J `9!:18`: the relative comparison tolerance.
    pub comparison_tolerance: Option<f64>,
    pub nested_model: NestedModel,
    pub first_disclose: FirstDisclose,
    pub index_form: IndexForm,
    pub dfn_result: DfnResult,
    pub default_arg: DefaultArg,
    pub complex_order: ComplexOrder,
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
            comparison_tolerance: None,
            nested_model: NestedModel::Floating,
            first_disclose: FirstDisclose::UpIsFirst,
            index_form: IndexForm::ScalarPerAxis,
            dfn_result: DfnResult::LastSentence,
            default_arg: DefaultArg::Eager,
            complex_order: ComplexOrder::RealThenImaginary,
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
        if let Some(ct) = self.comparison_tolerance {
            if !(ct.is_finite() && ct >= 0.0) {
                return Err(Error::new(
                    ErrorKind::Domain,
                    "the comparison tolerance must be a finite value at or above zero",
                    None,
                ));
            }
        }
        match self.nested_model {
            NestedModel::Floating => {}
            NestedModel::Grounded => return Err(refuse("a grounded nested array model")),
        }
        match self.first_disclose {
            FirstDisclose::UpIsFirst => {}
            FirstDisclose::UpIsMix => return Err(refuse("↑ as mix and ⊃ as first")),
        }
        match self.index_form {
            IndexForm::ScalarPerAxis => {}
            IndexForm::AxisVectors => return Err(refuse("⌷ over index vectors")),
        }
        match self.dfn_result {
            DfnResult::LastSentence => {}
            DfnResult::FirstNonAssignment => {
                return Err(refuse("a dfn that answers with its first non-assignment sentence"))
            }
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
            nested_model: self.nested_model,
            first_disclose: self.first_disclose,
            index_form: self.index_form,
            dfn_result: self.dfn_result,
            default_arg: self.default_arg,
            complex_order: self.complex_order,
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
    /// The index origin in force: APL's `⎕IO`, and 0 for J.
    pub origin: i64,
    /// The comparison tolerance in force. `Rules::tol` pairs it with the
    /// language's scaling rule; a verb-local `u!.n` overrides that copy
    /// and not this one.
    pub ct: f64,
    pub nested_model: NestedModel,
    pub first_disclose: FirstDisclose,
    pub index_form: IndexForm,
    pub dfn_result: DfnResult,
    pub default_arg: DefaultArg,
    pub complex_order: ComplexOrder,
    pub trains: bool,
}

impl Rules {
    /// The dialect's comparison tolerance, with the language's scale.
    pub fn tol(&self) -> Tol {
        Tol { ct: self.ct, by_smaller: self.lang == Lang::J }
    }

    /// The host-facing form, for a nested compilation (`⍎`, `".`) that has
    /// to run under the same dialect as the program executing it.
    pub fn dialect(&self) -> Dialect {
        Dialect {
            index_origin: Some(self.origin),
            comparison_tolerance: Some(self.ct),
            nested_model: self.nested_model,
            first_disclose: self.first_disclose,
            index_form: self.index_form,
            dfn_result: self.dfn_result,
            default_arg: self.default_arg,
            complex_order: self.complex_order,
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
        Lang::J => (j::parse(&sp)?, Agreement::LeadingPrefix, FmtOpts::J),
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
