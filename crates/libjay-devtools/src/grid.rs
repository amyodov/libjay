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
