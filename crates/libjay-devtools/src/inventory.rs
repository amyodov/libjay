//! The published vocabulary of a language, read off `docs/status.md`.
//!
//! The coverage measurement needs a denominator that is not the corpus
//! itself: a spelling the corpus never mentions is invisible to any count
//! taken from the corpus alone. `docs/status.md` already carries one row
//! per spelling and one column per valence, so it is the inventory, and
//! reading it keeps the two documents from drifting apart silently — a
//! coverage run that parses no rows says so.
//!
//! Only the primitive tables are read. The syntax and feature tables name
//! things that are not primitives, the noun tables name constants that
//! compile to values rather than to verbs, and the Dyalog-line table names
//! spellings the shipped dialect does not answer to.

use std::collections::BTreeMap;
use std::path::PathBuf;

use libjay_testkit::Lang;

/// One spelling of the published vocabulary, and the valences the document
/// gives it a meaning in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub spelling: String,
    /// The section it was found under, for the report.
    pub section: String,
    pub monad: bool,
    pub dyad: bool,
}

/// What one language's tables hold: verbs (functions) and modifiers
/// (adverbs, conjunctions, operators), each by spelling.
#[derive(Clone, Debug, Default)]
pub struct Inventory {
    pub verbs: BTreeMap<String, Row>,
    pub modifiers: BTreeMap<String, Row>,
}

impl Inventory {
    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty() && self.modifiers.is_empty()
    }

    /// How many verb valences the document defines: the denominator a
    /// per-valence count is a fraction of.
    pub fn valences(&self) -> usize {
        self.verbs.values().map(|r| usize::from(r.monad) + usize::from(r.dyad)).sum()
    }
}

/// Where `docs/status.md` lives, relative to this crate.
pub fn status_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/status.md"))
}

/// Read the inventory of one language. A document that cannot be found is
/// an empty inventory, not a failure: the coverage of the corpus is still
/// measurable without it.
pub fn read(lang: Lang) -> Inventory {
    match std::fs::read_to_string(status_path()) {
        Ok(text) => parse(&text, lang),
        Err(_) => Inventory::default(),
    }
}

/// Which half of the document a `## ` heading opens, for one language.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Verbs,
    Modifiers,
    Elsewhere,
}

fn part_of(heading: &str, lang: Lang) -> Part {
    let (verbs, modifiers): (&[&str], &[&str]) = match lang {
        Lang::J => (&["J — verbs"], &["J — adverbs", "J — conjunctions"]),
        Lang::Apl => (&["APL — functions"], &["APL — operators"]),
    };
    if verbs.contains(&heading) {
        Part::Verbs
    } else if modifiers.contains(&heading) {
        Part::Modifiers
    } else {
        Part::Elsewhere
    }
}

/// Parse the tables of one language out of the status document.
pub fn parse(text: &str, lang: Lang) -> Inventory {
    let mut inv = Inventory::default();
    let mut part = Part::Elsewhere;
    let mut section = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            part = part_of(rest.trim(), lang);
            section = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("### ") {
            section = rest.trim().to_string();
            continue;
        }
        if part == Part::Elsewhere || !line.starts_with('|') {
            continue;
        }
        // The nouns are values, not verbs: `a:` is data the corpus uses,
        // never a verb applied to an argument.
        if section.contains("Nouns") {
            continue;
        }
        let cells = split_row(line);
        if cells.is_empty() || cells[0].starts_with("---") {
            continue;
        }
        let Some(spelling) = spelling_of(&cells[0]) else { continue };
        // A row with three columns states a valence each; a row with two
        // states one status for the whole spelling, which is how the
        // modifier tables are written.
        let (monad, dyad) = if cells.len() >= 3 {
            (defined(&cells[1]), defined(&cells[2]))
        } else {
            (true, true)
        };
        if !monad && !dyad {
            continue;
        }
        let row = Row { spelling: spelling.clone(), section: section.clone(), monad, dyad };
        match part {
            Part::Verbs => inv.verbs.insert(spelling, row),
            Part::Modifiers => inv.modifiers.insert(spelling, row),
            Part::Elsewhere => None,
        };
    }
    inv
}

/// The cells of one markdown table row, without the outer pipes. A pipe
/// inside a cell is escaped in the document, and stays part of the cell.
fn split_row(line: &str) -> Vec<String> {
    let mut cells = raw_cells(line);
    if cells.first().is_some_and(String::is_empty) {
        cells.remove(0);
    }
    if cells.last().is_some_and(String::is_empty) {
        cells.pop();
    }
    cells
}

fn raw_cells(line: &str) -> Vec<String> {
    let mut cells = vec![String::new()];
    let mut escaped = false;
    for c in line.chars() {
        if escaped {
            cells.last_mut().expect("a row starts with one cell").push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '|' => cells.push(String::new()),
            _ => cells.last_mut().expect("a row starts with one cell").push(c),
        }
    }
    cells.into_iter().map(|c| c.trim().to_string()).collect()
}

/// The spelling a first column names: the text of its first code span.
/// `` `` ` `` `` — a code span holding a backtick — is written with double
/// fences, so those are read first.
fn spelling_of(cell: &str) -> Option<String> {
    let cell = cell.trim();
    if let Some(rest) = cell.strip_prefix("``") {
        let end = rest.find("``")?;
        let inner = rest[..end].trim();
        return (!inner.is_empty()).then(|| inner.to_string());
    }
    let rest = cell.strip_prefix('`')?;
    let end = rest.find('`')?;
    let inner = rest[..end].trim();
    (!inner.is_empty()).then(|| inner.to_string())
}

/// Whether a status cell claims the language gives this valence a meaning.
/// `—` says the language does not, and `⚪` says libjay refuses it by
/// design; neither is something a corpus could exercise.
fn defined(cell: &str) -> bool {
    let cell = cell.trim();
    !(cell.is_empty() || cell.starts_with('—') || cell.starts_with('⚪'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "\
# Language coverage status

## J — verbs

### Arithmetic and scalar

| Spelling | Monad | Dyad |
|---|---|---|
| `+` | 🟢 conjugate | 🟢 plus |
| `\\|` | 🟢 magnitude | 🟢 residue |
| `~.` | 🟢 nub | — |
| `T.` | ⚪ threads | ⚪ threads |

### Nouns and constant verbs

| Spelling | Status |
|---|---|
| `a:` ace | 🟢 |

## J — adverbs

| Spelling | Monad | Dyad |
|---|---|---|
| `/` | 🟢 insert | 🟢 table |

## J — conjunctions

| Spelling | Status |
|---|---|
| `\"` rank | 🟡 noun ranks |
| `` ` `` tie | 🟡 |

## J — syntax and features

| Feature | Status |
|---|---|
| Forks `(f g h)` | 🟢 |

## APL — functions

| Spelling | Monad | Dyad |
|---|---|---|
| `⍴` | 🟢 shape | 🟢 reshape |
";

    #[test]
    fn the_verb_tables_give_one_row_per_spelling_and_a_column_per_valence() {
        let inv = parse(DOC, Lang::J);
        let keys: Vec<&str> = inv.verbs.keys().map(String::as_str).collect();
        assert_eq!(keys, ["+", "|", "~."]);
        assert!(inv.verbs["+"].monad && inv.verbs["+"].dyad);
        assert!(inv.verbs["~."].monad && !inv.verbs["~."].dyad);
    }

    #[test]
    fn a_spelling_absent_by_design_is_not_in_the_inventory() {
        let inv = parse(DOC, Lang::J);
        assert!(!inv.verbs.contains_key("T."));
    }

    #[test]
    fn nouns_features_and_the_other_language_stay_out() {
        let inv = parse(DOC, Lang::J);
        assert!(!inv.verbs.contains_key("a:"));
        assert!(!inv.verbs.contains_key("⍴"));
        assert!(!inv.modifiers.contains_key("(f g h)"));
        let other = parse(DOC, Lang::Apl);
        let apl: Vec<&str> = other.verbs.keys().map(String::as_str).collect();
        assert_eq!(apl, ["⍴"]);
    }

    #[test]
    fn the_modifier_tables_carry_the_adverbs_and_the_conjunctions() {
        let inv = parse(DOC, Lang::J);
        let keys: Vec<&str> = inv.modifiers.keys().map(String::as_str).collect();
        assert_eq!(keys, ["\"", "/", "`"]);
    }

    #[test]
    fn an_escaped_pipe_stays_part_of_the_spelling() {
        let inv = parse(DOC, Lang::J);
        assert!(inv.verbs.contains_key("|"));
    }

    #[test]
    fn the_valence_count_is_the_denominator_of_a_per_valence_report() {
        // `+` has two, `|` two, `~.` one.
        assert_eq!(parse(DOC, Lang::J).valences(), 5);
    }
}
