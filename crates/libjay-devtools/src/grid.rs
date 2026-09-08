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

// ---------------------------------------------------------------------
// The FOLD GRID
// ---------------------------------------------------------------------
//
// The two tables above cross a VALUE with a verb and then with a modifier.
// Neither reaches the fold conjunctions, because those take TWO verbs: `u
// F.. v` folds the items of y into a running value under v and reports it
// under u, and no crossing of one verb with a value writes that shape. The
// residue of the second table said so — six of its twenty unexplained rows
// were `F..`, `F.:` and `F:.` over an empty, an infinity or a matrix
// divide, which was the largest single kind left.
//
// `F.` and `F:` are NOT in this table. They are the folds that run until
// something stops them, and jconsole hangs on every spelling of them that
// does not error out at once — `(+ F. *) 1 2 3`, `1 (+ F. *) 1 2 3`,
// `(+ F. -) 2`, `(>: F. (] [ (0 Z: 4 < ]))) 1` were all measured hanging
// and none of them is run again.

/// The four fold conjunctions that always stop: single or multiple answer,
/// forward or reverse travel.
const FOLD_CONJ: [&str; 4] = ["F..", "F.:", "F:.", "F::"];

/// The verbs the fold reports its running values under. Applied MONADICALLY,
/// so the monad's hazard rules are the ones that apply.
const FOLD_U: [&str; 8] = ["+", "-", "*", "%", "<.", "+:", "-.", ","];

/// The verbs the fold steps with. Applied DYADICALLY, item on the left and
/// the running value on the right. `%.` and `p.` are here because the
/// second table's residue named them.
const FOLD_V: [&str; 7] = ["+", "*", "%.", ",.", ",", "]", "p."];

/// The controls the fold's stop is written with: the four it reads and one
/// that is out of range, which the reference refuses when the stop is
/// applied rather than when it is written.
const STOP_CONTROL: [&str; 5] = ["_2", "_1", "0", "1", "2"];

/// What the stop tests. One that never fires, one that always does, and one
/// that fires on an item of a particular value.
const STOP_TEST: [&str; 3] = ["0:", "1:", "(2 = [)"];

/// The verbs the stopped fold reports under. Two are enough: the stop is
/// what is being measured, and every other verb of `FOLD_U` is crossed with
/// the fold without one.
const STOP_U: [&str; 2] = ["+", "]"];

/// Every sentence of the fold grid, in a fixed order, each one written once.
pub fn fold_sentences() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    let mut push = |s: String, out: &mut Vec<String>| {
        if seen.insert(s.clone()) {
            out.push(s);
        }
    };
    for conj in FOLD_CONJ {
        for u in FOLD_U {
            for v in FOLD_V {
                for cls in CLASSES.iter() {
                    // v steps dyadically over two values of the class, u
                    // reports monadically over one.
                    if !dyad_ok(v, cls.name, cls.name) || !monad_ok(u, cls.name) {
                        continue;
                    }
                    for shape in [Shape::List, Shape::Table] {
                        let y = written(cls.name, shape);
                        push(format!("({u} {conj} {v}) {y}"), &mut out);
                    }
                }
                // The dyad, whose left argument is where the running value
                // starts, over the twelve representative classes.
                for cls in LIST_CLASSES {
                    if !dyad_ok(v, cls, cls) || !monad_ok(u, cls) {
                        continue;
                    }
                    push(
                        format!(
                            "{} ({u} {conj} {v}) {}",
                            written(cls, Shape::Atom),
                            written(cls, Shape::List)
                        ),
                        &mut out,
                    );
                }
            }
        }
        // The stop, over the representative classes. The stepping verb is a
        // sum with the stop beside it, and the argument is THREE items long
        // rather than two: a fold over two items takes one step, and one
        // step cannot tell a stop that ends the fold from one that leaves a
        // single result out of it.
        for u in STOP_U {
            for control in STOP_CONTROL {
                for test in STOP_TEST {
                    for cls in LIST_CLASSES {
                        if !monad_ok(u, cls) || !dyad_ok("+", cls, cls) {
                            continue;
                        }
                        let v = spelling(cls);
                        push(
                            format!(
                                "({u} {conj} (+ [ ({control} Z: {test}))) (({v}) , ({v}) , ({v}))"
                            ),
                            &mut out,
                        );
                    }
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------
// The DYADIC MODIFIER GRID
// ---------------------------------------------------------------------
//
// The modifier table's dyadic half is two broadcast ranks and a commute;
// everything else in it applies a modifier to ONE argument. The forms a
// left argument changes outright — the outer product, the infix and the
// outfix, the key, the cut — are not in it at all, and the second table's
// residue left exactly one such row (`(_. 1 2) <./ (1 2 3x)`).
//
// A left argument here is a WIDTH, a KEY, a FRET or a RECTANGLE rather
// than a value to compute with, so the value classes go on the right and
// the left is crossed over the widths and shapes the form takes. The outer
// product is the exception: both of its arguments are values.

/// The widths an infix and an outfix are measured at: overlapping and
/// non-overlapping, forward and backward, the zero width and the infinite
/// one.
const INFIX_X: [&str; 8] = ["_3", "_2", "_1", "0", "1", "2", "3", "_"];

/// The cut's interval forms, which take a list of frets, and its rectangle
/// forms, which take an origin and a size.
const CUT_INTERVAL: [&str; 4] = ["1", "_1", "2", "_2"];
const CUT_RECT: [&str; 2] = ["0", "3"];

/// Every sentence of the dyadic modifier grid, in a fixed order, each one
/// written once.
pub fn dyadic_sentences() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    let mut push = |s: String, out: &mut Vec<String>| {
        if seen.insert(s.clone()) {
            out.push(s);
        }
    };
    let verbs = mod_verbs();

    // The outer product `x u/ y`: every item of x meets every item of y
    // under the verb's DYAD, so the dyad's hazard rules apply to the pair
    // in that order.
    for verb in &verbs {
        for left in LIST_CLASSES {
            for right in LIST_CLASSES {
                if !dyad_ok(verb, left, right) || !modifier_ok(verb, left, "insert") {
                    continue;
                }
                if !modifier_ok(verb, right, "insert") {
                    continue;
                }
                push(
                    format!(
                        "{} {verb}/ {}",
                        written(left, Shape::List),
                        written(right, Shape::List)
                    ),
                    &mut out,
                );
            }
        }
    }

    // The infix and the outfix, which apply the verb's MONAD to each
    // window. `_` and `0` are widths in their own right: one window of the
    // whole and one window per item.
    for verb in &verbs {
        for cls in CLASSES.iter() {
            if !monad_ok(verb, cls.name) || !modifier_ok(verb, cls.name, "rank") {
                continue;
            }
            let y = written(cls.name, Shape::List);
            for x in INFIX_X {
                push(format!("({x}) {verb}\\ {y}"), &mut out);
                push(format!("({x}) {verb}\\. {y}"), &mut out);
            }
        }
    }

    // The key `x u/. y`, which applies the monad to the items of y that
    // share a key. Both sides are read: the class in the KEY, where what is
    // measured is how two special values are told apart, and the class in
    // the VALUE, where it is what the monad makes of a group.
    for verb in &verbs {
        for cls in CLASSES.iter() {
            if !modifier_ok(verb, cls.name, "oblique") {
                continue;
            }
            if monad_ok(verb, "two") {
                push(
                    format!(
                        "{} {verb}/. ((1) , (2))",
                        written(cls.name, Shape::List)
                    ),
                    &mut out,
                );
            }
            if monad_ok(verb, cls.name) {
                push(
                    format!(
                        "((0) , (1)) {verb}/. {}",
                        written(cls.name, Shape::List)
                    ),
                    &mut out,
                );
            }
        }
    }

    // The cut. The interval forms take a list of frets and read a list;
    // the rectangle forms take an origin and a size and read a table.
    for verb in &verbs {
        for cls in CLASSES.iter() {
            if !monad_ok(verb, cls.name) || !modifier_ok(verb, cls.name, "rank") {
                continue;
            }
            let list = written(cls.name, Shape::List);
            let table = written(cls.name, Shape::Table);
            for n in CUT_INTERVAL {
                push(format!("((1) , (0)) {verb};.{n} {list}"), &mut out);
            }
            for n in CUT_RECT {
                let x = if n == "0" { "(((0) , (0)) ,: ((2) , (2)))" } else { "((2) , (2))" };
                push(format!("{x} {verb};.{n} {table}"), &mut out);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------
// The COMPOSITION GRID
// ---------------------------------------------------------------------
//
// Seven of the second table's twenty unexplained rows were COMPOSED forms,
// which is the second-largest kind left. A composition is two or three
// verbs joined so that one of them decides what the others see: `u@v` and
// `u@:v` differ only in the rank they join at, `u&v` and `u&:v` differ from
// them only in what a dyad does with its two arguments, `u&.v` closes v's
// obverse around the answer, and a hook, a fork and a capped fork are the
// same joins written as a train.

/// The verbs on the OUTSIDE of a composition: the one that reports.
const COMP_U: [&str; 6] = ["+", "-", "*", "%", "<.", ">:"];

/// The verbs on the INSIDE: the one that prepares. One doubling, one
/// square, one reciprocal, one logarithm, one imaginary unit and one
/// magnitude, so an under is measured over an exact obverse, an inexact
/// one, a complex one and one that has none at all.
const COMP_V: [&str; 6] = ["+:", "*:", "%", "^.", "j.", "|"];

/// The two-verb joins: four compositions, two unders, a hook and a capped
/// fork.
const COMP_FORMS: [&str; 8] = ["@", "@:", "&", "&:", "&.", "&.:", "hook", "cap"];

/// The fork's three places, kept smaller than the pair sets so that the
/// three-verb half stays the size of the two-verb one.
const FORK_UW: [&str; 4] = ["+", "-", "*", "%"];
const FORK_V: [&str; 4] = ["+", "*", "<.", "%"];

fn composed(u: &str, form: &str, v: &str) -> String {
    match form {
        "hook" => format!("({u} {v})"),
        "cap" => format!("([: {u} {v})"),
        _ => format!("({u} {form} {v})"),
    }
}

/// Every sentence of the composition grid, in a fixed order, each one
/// written once.
pub fn composition_sentences() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    let mut push = |s: String, out: &mut Vec<String>| {
        if seen.insert(s.clone()) {
            out.push(s);
        }
    };
    for u in COMP_U {
        for v in COMP_V {
            for form in COMP_FORMS {
                let f = composed(u, form, v);
                for cls in CLASSES.iter() {
                    if !monad_ok(u, cls.name) || !monad_ok(v, cls.name) {
                        continue;
                    }
                    for shape in [Shape::Atom, Shape::List] {
                        push(format!("{f} {}", written(cls.name, shape)), &mut out);
                    }
                    if dyad_ok(u, cls.name, cls.name) && dyad_ok(v, cls.name, cls.name) {
                        push(
                            format!(
                                "{} {f} {}",
                                written(cls.name, Shape::Atom),
                                written(cls.name, Shape::Atom)
                            ),
                            &mut out,
                        );
                    }
                }
            }
        }
    }
    for u in FORK_UW {
        for v in FORK_V {
            for w in FORK_UW {
                let f = format!("({u} {v} {w})");
                for cls in CLASSES.iter() {
                    if !monad_ok(u, cls.name) || !monad_ok(w, cls.name) {
                        continue;
                    }
                    for shape in [Shape::Atom, Shape::List] {
                        push(format!("{f} {}", written(cls.name, shape)), &mut out);
                    }
                    if dyad_ok(u, cls.name, cls.name) && dyad_ok(w, cls.name, cls.name) {
                        push(
                            format!(
                                "{} {f} {}",
                                written(cls.name, Shape::Atom),
                                written(cls.name, Shape::Atom)
                            ),
                            &mut out,
                        );
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod grid_c_tests {
    use super::*;

    #[test]
    fn the_fold_grid_is_whole_and_stops() {
        let all = fold_sentences();
        let unique: std::collections::HashSet<&String> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
        // The two folds that run until something stops them hang the
        // reference on every spelling that does not error at once.
        assert!(!all.iter().any(|s| s.contains("F. ") || s.contains("F: ")));
        // The binomial and the factorisations are out of the operand sets
        // altogether.
        assert!(!all.iter().any(|s| s.contains('!') || s.contains("p:") || s.contains("q:")));
        assert!(all.contains(&"(+ F.. *) ((_.) , (_.))".to_string()));
        assert!(all.contains(&"(+: F.. %.) ((_2) , (_2))".to_string()));
        assert!(all.contains(&"(2) (+ F:. *) ((2) , (2))".to_string()));
        assert!(all.iter().any(|s| s.contains("Z:")));
    }

    #[test]
    fn the_dyadic_grid_is_whole_and_nothing_hazardous() {
        let all = dyadic_sentences();
        let unique: std::collections::HashSet<&String> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
        // An outer product whose left item reshapes by a magnitude no array
        // can hold is the `__ $ 1` of the hazard list under an insert.
        assert!(!all.iter().any(|s| s.contains("(__) , (__)) $/")));
        assert!(!all.iter().any(|s| s.contains("!") && s.contains("(_)")));
        assert!(all.contains(&"((_.) , (_.)) </ ((2) , (2))".to_string()));
        assert!(all.contains(&"(2) +\\ ((_.) , (_.))".to_string()));
        assert!(all.contains(&"(_) +\\. ((_.) , (_.))".to_string()));
        assert!(all.contains(&"((0) , (1)) +/. ((_.) , (_.))".to_string()));
        assert!(all.contains(&"((1) , (0)) +;.1 ((_.) , (_.))".to_string()));
        assert!(all.contains(&"((2) , (2)) +;.3 (2 2 $ (_.))".to_string()));
    }

    #[test]
    fn the_composition_grid_is_whole() {
        let all = composition_sentences();
        let unique: std::collections::HashSet<&String> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
        assert!(all.contains(&"(+ @ +:) (_.)".to_string()));
        assert!(all.contains(&"(+ &. %) ((_.) , (_.))".to_string()));
        assert!(all.contains(&"([: + *:) (_.)".to_string()));
        assert!(all.contains(&"(+ *:) (_.)".to_string()));
        assert!(all.contains(&"(+ + -) (_.)".to_string()));
        assert!(all.contains(&"(_.) (+ @ +:) (_.)".to_string()));
    }
}
