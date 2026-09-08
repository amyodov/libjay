//! The SPECIAL-VALUE GRID: every primitive verb the J frontend knows,
//! crossed with every value class that has ever parted the two engines, at
//! atom, list and table shape.
//!
//! A sweep samples that table a few cells at a time and finds new ones every
//! run; this enumerates the whole of it once, so the cells are read rather
//! than drawn. The sentences go through `fuzz --compare --exprs`, which is
//! the same triage every other candidate line gets.
//!
//! The hazard filter is part of the generator rather than a list kept
//! beside it: a sentence that asks for an array of 2^31 items, or that the
//! reference is known to die on, is never emitted at all.

/// One value class: the name the grid reports it under, and its J spelling.
struct Class {
    name: &'static str,
    spelling: &'static str,
}

const fn class(name: &'static str, spelling: &'static str) -> Class {
    Class { name, spelling }
}

/// The value classes. The first eighteen are the ones the sweeps' residue
/// was made of — an infinity, a complex value, a NaN, a huge integer or
/// exponent literal, an exact type — and the rest are the ordinary values a
/// verb is asked to refuse or to answer beside them.
const CLASSES: [Class; 28] = [
    class("nan", "_."),
    class("inf", "_"),
    class("ninf", "__"),
    class("cnan", "_.j_."),
    class("cinfr", "_j0"),
    class("cinfi", "0j_"),
    class("cinfb", "_j_"),
    class("imax", "9223372036854775807"),
    class("imin", "_9223372036854775807"),
    class("i2p63", "9223372036854775808"),
    class("f2p63", "9.223372036854776e18"),
    class("i2p53", "9007199254740993"),
    class("zerox", "0x"),
    class("zeror", "0r1"),
    class("bigx", "123456789012345678901234567890x"),
    class("third", "1r3"),
    class("e300", "1e300"),
    class("em300", "1e_300"),
    class("c0", "0j0"),
    class("half", "0.5"),
    class("two", "2"),
    class("ntwo", "_2"),
    class("one", "1"),
    class("zero", "0"),
    class("chara", "'a'"),
    class("ace", "a:"),
    class("box2", "<2"),
    class("empty", "i. 0"),
];

/// Magnitudes that name more items than an array can hold, for the verbs
/// that size their answer BY the value.
const HUGE: [&str; 12] = [
    "inf", "ninf", "cinfr", "cinfi", "cinfb", "imax", "imin", "i2p63", "f2p63", "i2p53",
    "bigx", "e300",
];

/// The values with no finite magnitude at all.
const NONFINITE: [&str; 7] = ["nan", "inf", "ninf", "cnan", "cinfr", "cinfi", "cinfb"];

/// The exact types, which `s:` is on the hazard list for.
const EXACT: [&str; 4] = ["zerox", "zeror", "bigx", "third"];

/// The classes that are not one number: `p.` and `p..` are on the hazard
/// list for a boxed or empty argument.
const NOTNUM: [&str; 4] = ["chara", "ace", "box2", "empty"];

/// Every primitive verb of the J frontend's own table, less `p:` and `q:`
/// (the hazard list: a factorisation of anything that is not a whole number
/// kills the reference silently), `?` and `?.` (which draw random numbers,
/// so the two engines cannot be compared on them at all) and `echo`, which
/// writes to the session rather than answering.
const VERBS: [&str; 65] = [
    "+", "-", "*", "%", "^", "%:", "^.", "|", "<.", ">.", "=", "<", ">", "<:", ">:", "+:",
    "*:", "-:", "-.", "*.", "+.", "~:", "~.", "$", ",", ",.", ",:", "#", "#.", "#:", "!",
    "\":", "o.", "j.", "r.", "{", "{.", "}.", "{:", "}:", "|.", "|:", "i.", "i:", "I.",
    "x:", "p.", "p..", "$.", "%.", "{::", "e.", "/:", "\\:", ";", ";:", "L.", "\".", "A.",
    "C.", "E.", "u:", "s:", "]", "[",
];

/// The verbs with no dyad, and the one with no monad.
const NO_DYAD: [&str; 5] = ["~.", "{:", "}:", "L.", "E."];
const NO_MONAD: [&str; 1] = ["E."];

/// The scalar verbs, which are the ones the grid also crosses at list shape:
/// an atom against a list, a list against an atom, and two lists.
const SCALARS: [&str; 24] = [
    "+", "-", "*", "%", "^", "%:", "^.", "|", "<.", ">.", "=", "<", ">", "<:", ">:", "+:",
    "*:", "*.", "+.", "~:", "!", "o.", "j.", "r.",
];

/// The classes the list-shaped half is crossed over. The whole 28 would put
/// the grid past a hundred thousand sentences for no new mechanism: one
/// representative of each kind is enough, since a length is what the list
/// shape is there to vary.
const LIST_CLASSES: [&str; 12] = [
    "nan", "inf", "ninf", "cinfb", "imax", "zerox", "third", "e300", "half", "two",
    "chara", "box2",
];

/// The three shapes each class is written at.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Atom,
    List,
    Table,
}

fn spelling(name: &str) -> &'static str {
    CLASSES.iter().find(|c| c.name == name).expect("a named class").spelling
}

fn written(name: &str, shape: Shape) -> String {
    let v = spelling(name);
    match shape {
        Shape::Atom => format!("({v})"),
        Shape::List => format!("(({v}) , ({v}))"),
        Shape::Table => format!("(2 2 $ ({v}))"),
    }
}

fn monad_ok(verb: &str, cls: &str) -> bool {
    let huge = HUGE.contains(&cls) || NONFINITE.contains(&cls);
    if matches!(verb, "i." | "i:" | "I." | "A." | "C.") && huge {
        return false;
    }
    if verb == "#:" && HUGE.contains(&cls) {
        return false;
    }
    // `! (_.j_.)` takes the reference down.
    if verb == "!" && cls == "cnan" {
        return false;
    }
    if verb == "s:" && EXACT.contains(&cls) {
        return false;
    }
    if matches!(verb, "p." | "p..") && NOTNUM.contains(&cls) {
        return false;
    }
    true
}

fn dyad_ok(verb: &str, left: &str, right: &str) -> bool {
    // These size their answer by the LEFT argument: a reshape, a copy, a
    // take, a format width, a sparse form.
    let huge = HUGE.contains(&left) || NONFINITE.contains(&left);
    if matches!(verb, "$" | "#" | "{." | "\":" | "$.") && huge {
        return false;
    }
    if verb == "!" && (left == "cnan" || right == "cnan") {
        return false;
    }
    if verb == "s:" && (EXACT.contains(&left) || EXACT.contains(&right)) {
        return false;
    }
    if matches!(verb, "p." | "p..") && (NOTNUM.contains(&left) || NOTNUM.contains(&right)) {
        return false;
    }
    true
}

/// Every sentence of the grid, in a fixed order, each one written once.
pub fn sentences() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    let mut push = |s: String, out: &mut Vec<String>| {
        if seen.insert(s.clone()) {
            out.push(s);
        }
    };
    for verb in VERBS {
        if !NO_MONAD.contains(&verb) {
            for cls in CLASSES.iter() {
                if !monad_ok(verb, cls.name) {
                    continue;
                }
                for shape in [Shape::Atom, Shape::List, Shape::Table] {
                    push(format!("{verb} {}", written(cls.name, shape)), &mut out);
                }
            }
        }
        if !NO_DYAD.contains(&verb) {
            for left in CLASSES.iter() {
                for right in CLASSES.iter() {
                    if !dyad_ok(verb, left.name, right.name) {
                        continue;
                    }
                    push(
                        format!(
                            "{} {verb} {}",
                            written(left.name, Shape::Atom),
                            written(right.name, Shape::Atom)
                        ),
                        &mut out,
                    );
                }
            }
        }
    }
    for verb in SCALARS {
        for left in LIST_CLASSES {
            for right in LIST_CLASSES {
                if !dyad_ok(verb, left, right) {
                    continue;
                }
                for (lf, rf) in
                    [(Shape::Atom, Shape::List), (Shape::List, Shape::Atom), (Shape::List, Shape::List)]
                {
                    push(
                        format!("{} {verb} {}", written(left, lf), written(right, rf)),
                        &mut out,
                    );
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_is_the_whole_table_and_nothing_hazardous() {
        let all = sentences();
        // Every sentence is written once.
        let unique: std::collections::HashSet<&String> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
        // The monad of a verb that sizes its answer by the value is never
        // handed an infinity.
        assert!(!all.iter().any(|s| s.starts_with("i. (_)")));
        assert!(!all.iter().any(|s| s.starts_with("i. (2 2 $ (1e300))")));
        // A reshape by an infinity is the `__ $ 1` of the hazard list.
        assert!(!all.iter().any(|s| s.contains("(__) $ ")));
        // The verbs the hazard list bars are not in the table at all.
        assert!(!all.iter().any(|s| s.contains("p:") || s.contains("q:")));
        // The ordinary cells are there.
        assert!(all.contains(&"+ (_.)".to_string()));
        assert!(all.contains(&"(_) + (0j_)".to_string()));
        assert!(all.contains(&"((_.) , (_.)) + (2)".to_string()));
    }
}

// ---------------------------------------------------------------------
// The MODIFIER GRID
// ---------------------------------------------------------------------
//
// The table above crosses a bare verb with a value. This one puts the same
// values under the modifiers: what an insert makes of an identity element,
// what a rank frame does to a NaN, what a scan carries between its steps,
// which obverses exist at all, and what `&.` opens and closes around.
//
// The residue the first grid left was of that kind — a NaN through an
// infix, a value through a scan — so the second table is the modifiers
// with the verbs in the operand's place.

/// The verbs the modifier grid puts under a modifier: the arithmetic ones
/// the frame and the identity element are visible through, and the
/// structural ones a rank or an under reshapes around. Both valences of
/// every one of them exist, which the reflexive and commute forms need.
const MOD_ARITH: [&str; 18] = [
    "+", "-", "*", "%", "^", "%:", "^.", "|", "<.", ">.", "=", "<:", ">:", "+:", "*:", "!",
    "o.", "j.",
];

const MOD_STRUCT: [&str; 12] =
    ["<", ">", "#", ",", ",.", ",:", "$", "|.", "|:", "{.", "}.", "#."];

fn mod_verbs() -> Vec<&'static str> {
    MOD_ARITH.iter().chain(MOD_STRUCT.iter()).copied().collect()
}

/// The verbs `&.` closes with: a successor, a negation, a reciprocal, a
/// logarithm and the imaginary unit — one obverse of each kind, so that an
/// under is measured over an exact inverse, an inexact one and a complex
/// one.
const UNDER: [&str; 5] = [">:", "-", "%", "^.", "j."];

/// A value written boxed, which is what `&.>`, `L:` and `S:` are asked
/// about.
fn boxed(name: &str, shape: Shape) -> String {
    let v = spelling(name);
    match shape {
        Shape::Atom => format!("(<({v}))"),
        _ => format!("((<({v})) , (<({v})))"),
    }
}

/// The modifier grid's own hazard rules, on top of the table's.
///
/// The binomial at an infinity is where the reference hangs — the first
/// grid left ten unfinished rows there — and it hangs the same way under
/// every modifier, so `!` never meets a value with no finite magnitude
/// here. Its obverse searches for a root of the gamma function and does
/// not always stop; the obverse of the base-value is the encode, which
/// sizes an array by its argument.
fn modifier_ok(verb: &str, cls: &str, form: &str) -> bool {
    if verb == "!" && NONFINITE.contains(&cls) {
        return false;
    }
    if form == "obverse" && matches!(verb, "!" | "#.") {
        return false;
    }
    true
}

/// Every sentence of the modifier grid, in a fixed order, each one written
/// once.
pub fn modifier_sentences() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    let mut push = |s: String, out: &mut Vec<String>| {
        if seen.insert(s.clone()) {
            out.push(s);
        }
    };
    let verbs = mod_verbs();

    // The combining forms: an insert, its two scans, an infix, and the
    // reflexive. Each of them hands the verb two items of the class, so
    // the dyad's hazard rules are the ones that apply.
    for verb in &verbs {
        for cls in CLASSES.iter() {
            if !dyad_ok(verb, cls.name, cls.name) || !modifier_ok(verb, cls.name, "insert") {
                continue;
            }
            for shape in [Shape::List, Shape::Table] {
                let y = written(cls.name, shape);
                push(format!("{verb}/ {y}"), &mut out);
                push(format!("{verb}/\\ {y}"), &mut out);
                push(format!("{verb}/\\. {y}"), &mut out);
                push(format!("2 {verb}/\\ {y}"), &mut out);
            }
            for shape in [Shape::Atom, Shape::List] {
                push(format!("{verb}~ {}", written(cls.name, shape)), &mut out);
            }
        }
    }

    // The oblique, the three ranks, the power and the obverse, the adverse
    // and the gerund: every one of them applies the verb's MONAD.
    for verb in &verbs {
        for cls in CLASSES.iter() {
            if !monad_ok(verb, cls.name) {
                continue;
            }
            for shape in [Shape::List, Shape::Table] {
                if modifier_ok(verb, cls.name, "oblique") {
                    push(format!("{verb}/. {}", written(cls.name, shape)), &mut out);
                }
            }
            for shape in [Shape::Atom, Shape::List, Shape::Table] {
                let y = written(cls.name, shape);
                if modifier_ok(verb, cls.name, "rank") {
                    push(format!("{verb}\"0 {y}"), &mut out);
                    push(format!("{verb}\"1 {y}"), &mut out);
                    push(format!("{verb}\"_1 {y}"), &mut out);
                }
                if modifier_ok(verb, cls.name, "power") {
                    push(format!("{verb}^:2 {y}"), &mut out);
                }
                if modifier_ok(verb, cls.name, "obverse") {
                    push(format!("{verb}^:_1 {y}"), &mut out);
                }
            }
            for shape in [Shape::Atom, Shape::List] {
                let y = written(cls.name, shape);
                if modifier_ok(verb, cls.name, "under") {
                    for v in UNDER {
                        push(format!("{verb}&.{v} {y}"), &mut out);
                    }
                }
                if modifier_ok(verb, cls.name, "adverse") {
                    push(format!("(({verb}) :: (_1:)) {y}"), &mut out);
                    push(format!("(({verb})`(]))@.(0) {y}"), &mut out);
                }
                if modifier_ok(verb, cls.name, "boxed") {
                    let b = boxed(cls.name, shape);
                    push(format!("{verb}&.> {b}"), &mut out);
                    push(format!("{verb} L:0 {b}"), &mut out);
                    push(format!("{verb} S:0 {b}"), &mut out);
                }
            }
        }
    }

    // The dyadic forms: the two broadcast ranks over the arithmetic verbs,
    // and the commute over all of them. Both are crossed over the twelve
    // representative classes rather than the whole twenty-eight, which is
    // what keeps the table the size of the first one.
    for verb in MOD_ARITH {
        for left in LIST_CLASSES {
            for right in LIST_CLASSES {
                if !dyad_ok(verb, left, right)
                    || !modifier_ok(verb, left, "rank")
                    || !modifier_ok(verb, right, "rank")
                {
                    continue;
                }
                let (x, y) = (written(left, Shape::List), written(right, Shape::List));
                push(format!("{x} {verb}\"0 _ {y}"), &mut out);
                push(format!("{x} {verb}\"_ 0 {y}"), &mut out);
            }
        }
    }
    for verb in &verbs {
        for left in LIST_CLASSES {
            for right in LIST_CLASSES {
                // A commute SWAPS its arguments, so the hazard rules — all
                // of which are about a left argument that sizes the answer
                // — are asked about the swapped pair: `(0x) $~ (__)` is the
                // `__ $ 1` of the hazard list read backwards.
                if !dyad_ok(verb, right, left)
                    || !modifier_ok(verb, left, "commute")
                    || !modifier_ok(verb, right, "commute")
                {
                    continue;
                }
                push(
                    format!(
                        "{} {verb}~ {}",
                        written(left, Shape::Atom),
                        written(right, Shape::Atom)
                    ),
                    &mut out,
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod modifier_tests {
    use super::*;

    #[test]
    fn the_modifier_grid_is_whole_and_nothing_hazardous() {
        let all = modifier_sentences();
        let unique: std::collections::HashSet<&String> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
        // The binomial never meets a value with no finite magnitude: that
        // is where the reference hangs.
        assert!(!all.iter().any(|s| s.contains("!") && s.contains("_.")));
        assert!(!all.iter().any(|s| s.starts_with("!^:_1")));
        // Nor does an insert reshape by a magnitude no array can hold.
        assert!(!all.iter().any(|s| s.starts_with("$/ ((1e300)")));
        // Nor does a COMMUTE, whose swap puts the magnitude on the left.
        assert!(!all.iter().any(|s| s.contains("$~ (__)")));
        assert!(!all.iter().any(|s| s.contains("{.~ (9223372036854775807)")));
        // The ordinary cells are there.
        assert!(all.contains(&"+/ ((_.) , (_.))".to_string()));
        assert!(all.contains(&"+/\\. ((_) , (_))".to_string()));
        assert!(all.contains(&"2 +/\\ ((_.) , (_.))".to_string()));
        assert!(all.contains(&"+&.^. (2)".to_string()));
        assert!(all.contains(&"+ S:0 ((<(2)) , (<(2)))".to_string()));
        assert!(all.contains(&"((+)`(]))@.(0) (_.)".to_string()));
        assert!(all.contains(&"((_.) , (_.)) +\"0 _ ((2) , (2))".to_string()));
        assert!(all.contains(&"(_.) +~ (2)".to_string()));
    }
}
