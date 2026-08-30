//! The diagnostic contract, held to on data rather than on wiring.
//!
//! Every wrong program below is asserted on four things: the class of the
//! failure, the text its span underlines (the token that is wrong, not the
//! line it sits in), the words the message must carry — both shapes where
//! shapes disagree, the name where a name is at fault — and that
//! `explain` still answers for the same program.
//!
//! The last of those is what keeps "not supported yet" and "not in the
//! language" apart: the first is a promise libjay has made, the second a
//! property of J, of APL, or of the sandbox, and a reader must be able to
//! tell which they were handed.

use jay::{compile, Dialect, ErrorKind, Lang};

/// Substrings a rendered diagnostic must contain.
type Says = &'static [&'static str];

/// One wrong program and the diagnostic it owes the reader.
struct Case {
    lang: Lang,
    src: &'static str,
    kind: ErrorKind,
    /// The exact text the span covers, written out so that the expectation
    /// is readable; an empty string means the failure has no position (a
    /// mismatch between the program and the data it was given).
    span: &'static str,
    says: Says,
}

const fn j(src: &'static str, kind: ErrorKind, span: &'static str, says: Says) -> Case {
    Case { lang: Lang::J, src, kind, span, says }
}

const fn apl(src: &'static str, kind: ErrorKind, span: &'static str, says: Says) -> Case {
    Case { lang: Lang::Apl, src, kind, span, says }
}

/// Compile and run with no data, and answer with the failure. A program
/// that succeeds is a broken expectation, not a passing test.
fn failure(case: &Case) -> jay::Error {
    let program = match compile(case.lang, case.src, &Dialect::default()) {
        Err(e) => return e,
        Ok(p) => p,
    };
    let mut sink = |_: &str| {};
    match program.run(&[], &mut sink) {
        Err(e) => e,
        Ok(v) => panic!("{:?} was expected to fail; it answered {v:?}", case.src),
    }
}

fn check(case: &Case) {
    let e = failure(case);
    let rendered = e.render(case.src);
    assert_eq!(e.kind, case.kind, "{:?} is a {}:\n{rendered}", case.src, e.kind.label());
    match (case.span, e.span) {
        ("", span) => {
            assert!(span.is_none(), "{:?} should carry no position:\n{rendered}", case.src)
        }
        (want, None) => panic!("{:?} should point at {want:?}:\n{rendered}", case.src),
        (want, Some(s)) => {
            let got = case.src.get(s.start..s.end).unwrap_or_else(|| {
                panic!("{:?} has a span {s:?} that does not land on it", case.src)
            });
            assert_eq!(got, want, "{:?} points at the wrong words:\n{rendered}", case.src);
        }
    }
    for needle in case.says {
        assert!(rendered.contains(needle), "{:?} should say {needle:?}:\n{rendered}", case.src);
    }
    // The two labels have to be tellable apart, and only one may appear.
    let promise = "not supported yet";
    let permanent = "not in the language";
    match case.kind {
        ErrorKind::NotYet => assert!(
            rendered.contains(promise) && !rendered.contains(permanent),
            "a promise must read as one:\n{rendered}"
        ),
        ErrorKind::Language => assert!(
            rendered.contains(permanent) && !rendered.contains(promise),
            "a permanent refusal must not read as a promise:\n{rendered}"
        ),
        _ => assert!(
            !rendered.contains(promise) && !rendered.contains(permanent),
            "an ordinary error must not read as a gap:\n{rendered}"
        ),
    }
}

fn check_all(cases: &[Case]) {
    for case in cases {
        check(case);
    }
}

// --- J: the sentence does not parse -------------------------------------

const J_SYNTAX: &[Case] = &[
    j("'abc", ErrorKind::Parse, "'abc", &["unterminated string"]),
    j("(1 + 2", ErrorKind::Parse, "(", &["no closing"]),
    j("1 + 2)", ErrorKind::Parse, ")", &["no opening"]),
    j("1 + )", ErrorKind::Parse, ")", &["no opening"]),
    j("2 ]: 3", ErrorKind::Parse, "]:", &["unknown word", "]:"]),
    j("1.2.3", ErrorKind::Parse, "1.2.3", &["invalid number"]),
    j("1e10x", ErrorKind::Parse, "1e10x", &["invalid number"]),
    j("16b", ErrorKind::Parse, "16b", &["invalid number"]),
    j("{{ 1 + ", ErrorKind::Parse, "{{", &["no closing `}}`"]),
    j("'ab' 'cd'", ErrorKind::Parse, "'ab' 'cd'", &["syntax error"]),
    j("a =. 5\na 1 2 3", ErrorKind::Parse, "a 1 2 3", &["syntax error"]),
    // A control word outside a definition is a spelling error, as the
    // reference has it.
    j("if. 1 do. 2 end.", ErrorKind::Parse, "if.", &["explicit definition"]),
];

#[test]
fn j_syntax_errors_point_at_the_word_that_is_wrong() {
    check_all(J_SYNTAX);
}

// --- J: the arguments do not agree --------------------------------------

const J_SHAPE: &[Case] = &[
    j("1 2 + 1 2 3", ErrorKind::Length, "1 2 + 1 2 3", &["left shape 2", "right shape 3"]),
    j(
        "(i. 2 3) + i. 3 4",
        ErrorKind::Shape,
        "(i. 2 3) + i. 3 4",
        &["left shape 2 3", "right shape 3 4"],
    ),
    j("1 2 ,. i. 3 3", ErrorKind::Length, "1 2 ,. i. 3 3", &["left shape 2", "right shape 3 3"]),
    j("2 3 $ i. 0", ErrorKind::Length, "2 3 $ i. 0", &["empty"]),
];

#[test]
fn j_shape_errors_name_both_shapes() {
    check_all(J_SHAPE);
}

// --- J: the values are wrong for the verb -------------------------------

const J_DOMAIN: &[Case] = &[
    j("2 +: 3", ErrorKind::Domain, "2 +: 3", &["0 or 1"]),
    j("2 *: 3", ErrorKind::Domain, "2 *: 3", &["0 or 1"]),
    j("13 o. 1", ErrorKind::Domain, "13 o. 1", &["_12 to 12"]),
    j("x: _", ErrorKind::Domain, "x: _", &["infinity"]),
    j("u: _1", ErrorKind::Domain, "u: _1", &["codepoint"]),
    // Arithmetic on a box names the box and says how to open it.
    j("1 + < 1 2", ErrorKind::Type, "1 + < 1 2", &["boxed", "open them first"]),
    j("^. < 1 2", ErrorKind::Type, "^. < 1 2", &["boxed", "open them first"]),
    j("2 + 'a'", ErrorKind::Type, "2 + 'a'", &["character"]),
    j("'abc' < 'abd'", ErrorKind::Type, "'abc' < 'abd'", &["character"]),
];

#[test]
fn j_domain_errors_say_what_the_verb_reads() {
    check_all(J_DOMAIN);
}

// --- J: indexing --------------------------------------------------------

const J_INDEX: &[Case] = &[
    j("5 { 1 2 3", ErrorKind::Domain, "5 { 1 2 3", &["index 5", "3 items"]),
    j("_5 { 1 2 3", ErrorKind::Domain, "_5 { 1 2 3", &["out of range", "3 items"]),
    j("1 { ''", ErrorKind::Domain, "1 { ''", &["index 1", "0 items"]),
    j("1 2 3 { 1 2 3", ErrorKind::Domain, "1 2 3 { 1 2 3", &["index 3", "3 items"]),
];

#[test]
fn j_index_errors_name_the_index_and_the_length() {
    check_all(J_INDEX);
}

// --- J: names and valences ----------------------------------------------

const J_NAMES: &[Case] = &[
    j("nope + 1", ErrorKind::Value, "nope", &["nope"]),
    j("foo 1 2", ErrorKind::Value, "foo", &["foo"]),
    j("$: 1", ErrorKind::NotYet, "$: 1", &["self-reference", "tacit"]),
    j("2 {: 3", ErrorKind::Domain, "2 {: 3", &["{:", "no dyadic meaning"]),
    j("2 ~. 3", ErrorKind::Domain, "2 ~. 3", &["~.", "no dyadic meaning"]),
    // A definition's valence is its header's, in both directions.
    j("f =. 3 : 'y + 1'\n1 f 2", ErrorKind::Domain, "1 f 2", &["no dyadic definition"]),
    j("f =. 4 : 'x + y'\nf 1 2", ErrorKind::Domain, "f 1 2", &["no monadic definition", "x"]),
];

#[test]
fn j_name_and_valence_errors_name_the_word() {
    check_all(J_NAMES);
}

// --- J: the gaps --------------------------------------------------------

const J_PROMISES: &[Case] = &[
    j("<;.4 i. 5", ErrorKind::NotYet, "<;.4", &["cut (u;.4)"]),
    j("* b. _1", ErrorKind::NotYet, "* b. _1", &["obverse"]),
    j("$. 'abc'", ErrorKind::NotYet, "$. 'abc'", &["sparse"]),
    j("+/ . * i. 17 17", ErrorKind::NotYet, "+/ . * i. 17 17", &["determinant"]),
    j("9!:18", ErrorKind::NotYet, "9!:18", &["foreign 9!:18"]),
    j(
        "m =. {{ u/ y }}\nm",
        ErrorKind::NotYet,
        "m",
        &["writing this modifier back out"],
    ),
    j(
        "f =. 3 : 0\ny + 1\n)\nf",
        ErrorKind::NotYet,
        "f",
        &["back out as J source"],
    ),
    j("0 s: s: <'a'", ErrorKind::NotYet, "0 s: s: <'a'", &["symbol-table form"]),
    j(
        "(0;(2 2 2 $ 0 0 1 4 0 1 1 0);(' ' = a.)) ;: 'a b'",
        ErrorKind::NotYet,
        "(0;(2 2 2 $ 0 0 1 4 0 1 1 0);(' ' = a.)) ;: 'a b'",
        &["sequential machine"],
    ),
    // Which obverse `^:_1` needs is settled by the arguments — monadically
    // u's own, dyadically the bond's — so the missing one is named when the
    // sentence runs, and a run points at the sentence.
    j("(+/ % #) ^: _1 [ 1 2 3", ErrorKind::NotYet, "(+/ % #) ^: _1 [ 1 2 3", &["obverse"]),
];

#[test]
fn j_gaps_are_promises_and_name_the_feature() {
    check_all(J_PROMISES);
}

const J_PERMANENT: &[Case] = &[
    // Threads reach outside the expression. The sandbox is libjay's, and
    // no release will open it, so this must not read as a queue position.
    j("+ T. 1", ErrorKind::Sandbox, "+ T. 1", &["closed by the sandbox", "T."]),
    j("1 2 T. 3", ErrorKind::Sandbox, "1 2 T. 3", &["closed by the sandbox", "T."]),
    // The foreigns that reach a file, a directory, the host or a script.
    j("1!:1 <'x'", ErrorKind::Sandbox, "1!:1 <'x'", &["closed by the sandbox", "a file"]),
    j("1!:21 <'x'", ErrorKind::Sandbox, "1!:21", &["closed by the sandbox", "filesystem"]),
    j("2!:5 <'HOME'", ErrorKind::Sandbox, "2!:5", &["closed by the sandbox", "host"]),
    j("0!:0 <'x'", ErrorKind::Sandbox, "0!:0", &["closed by the sandbox", "script"]),
    j("6!:2 'i.5'", ErrorKind::Sandbox, "6!:2", &["closed by the sandbox", "clock"]),
];

#[test]
fn j_permanent_refusals_do_not_read_as_promises() {
    check_all(J_PERMANENT);
}

// --- J: a diagnostic from inside a string that was run ------------------

const J_EXECUTE: &[Case] = &[
    j(
        "\". '1 2 + 1 2 3'",
        ErrorKind::Length,
        "\". '1 2 + 1 2 3'",
        &["in the executed string", "left shape 2", "right shape 3"],
    ),
    j("\". 'nope'", ErrorKind::Value, "\". 'nope'", &["in the executed string", "nope"]),
    j("\". '(1'", ErrorKind::Parse, "\". '(1'", &["in the executed string"]),
];

#[test]
fn j_execute_reports_at_the_sentence_and_carries_the_inner_caret() {
    check_all(J_EXECUTE);
    // The inner diagnostic points into the string, which the caller never
    // sees, so it arrives as a note with its own caret line drawn.
    let e = failure(&J_EXECUTE[0]);
    let note = e.notes.first().expect("the inner diagnostic");
    assert!(note.contains("1 2 + 1 2 3"), "{note}");
    assert!(note.contains('^'), "{note}");
}

// --- J: a diagnostic from inside an explicit definition -----------------

const J_DEFINITION: &[Case] = &[
    j(
        "f =. 3 : 'y + 1 2 3'\nf 1 2",
        ErrorKind::Length,
        "y + 1 2 3",
        &["left shape 2", "right shape 3"],
    ),
    j(
        "f =. 3 : 'if. 1 do. 1 2 + 1 2 3 end.'\nf 1",
        ErrorKind::Length,
        "1 2 + 1 2 3",
        &["left shape 2", "right shape 3"],
    ),
    j("f =. 3 : 'nope'\nf 1", ErrorKind::Value, "nope", &["nope"]),
];

#[test]
fn j_a_definition_reports_inside_its_own_body() {
    check_all(J_DEFINITION);
}

// --- APL: the sentence does not parse -----------------------------------

const APL_SYNTAX: &[Case] = &[
    apl("'abc", ErrorKind::Parse, "'abc", &["unterminated string"]),
    apl("(1+2", ErrorKind::Parse, "(", &["syntax error"]),
    apl("2 ` 3", ErrorKind::Parse, "`", &["unknown symbol", "`"]),
    apl("¯", ErrorKind::Parse, "¯", &["unknown symbol"]),
    apl("+/", ErrorKind::Parse, "+/", &["missing right argument"]),
    apl("1 2 3⍴", ErrorKind::Parse, "⍴", &["missing right argument"]),
    apl("1 2 3∘.", ErrorKind::Parse, "∘.", &["∘.", "function on its right"]),
    apl("{⍺⍺ ⍵}1", ErrorKind::Parse, "{⍺⍺ ⍵}", &["⍺⍺", "operator's left"]),
];

#[test]
fn apl_syntax_errors_point_at_the_glyph_that_is_wrong() {
    check_all(APL_SYNTAX);
}

// --- APL: the arguments do not agree ------------------------------------

const APL_SHAPE: &[Case] = &[
    apl("1 2 3+1 2", ErrorKind::Length, "1 2 3+1 2", &["left shape 3", "right shape 2"]),
    // APL's rule is exact shape or a scalar, where J broadcasts.
    apl(
        "(2 3⍴⍳6)+10 20",
        ErrorKind::Shape,
        "(2 3⍴⍳6)+10 20",
        &["left shape 2 3", "right shape 2"],
    ),
    apl("1 0 1/1 2", ErrorKind::Length, "1 0 1/1 2", &["3 replication count", "2 item"]),
    apl(
        "(2 2⍴⍳4)⍪1 2 3",
        ErrorKind::Length,
        "(2 2⍴⍳4)⍪1 2 3",
        &["left shape 2 2", "right shape 1 3"],
    ),
];

#[test]
fn apl_shape_errors_name_both_shapes() {
    check_all(APL_SHAPE);
}

// --- APL: the values are wrong for the function -------------------------

const APL_DOMAIN: &[Case] = &[
    apl("1÷0", ErrorKind::Domain, "1÷0", &["division by zero"]),
    apl("2⍲3", ErrorKind::Domain, "2⍲3", &["0 or 1"]),
    apl("2⍱3", ErrorKind::Domain, "2⍱3", &["0 or 1"]),
    apl("~2", ErrorKind::Domain, "~2", &["0 or 1"]),
    apl("?0", ErrorKind::Domain, "?0", &["empty"]),
    apl("⍳¯1", ErrorKind::Domain, "⍳¯1", &["nonnegative"]),
    // APL's scalar functions pervade a nested argument, so the boxed
    // diagnostic is J's alone; what stays wrong here is the CHARACTER
    // inside the nesting.
    apl("⍟⊂'ab'", ErrorKind::Type, "⍟⊂'ab'", &["character"]),
    apl("'ab'+1", ErrorKind::Type, "'ab'+1", &["character"]),
];

#[test]
fn apl_domain_errors_say_what_the_function_reads() {
    check_all(APL_DOMAIN);
}

// --- APL: bracket indexing ----------------------------------------------

const APL_INDEX: &[Case] = &[
    apl("A←⍳3 ⋄ A[5]", ErrorKind::Domain, "A[5]", &["index 5", "axis 0", "3 items"]),
    // ⎕IO is 1 by default, so 0 is below the axis as surely as 5 is above.
    apl("A←⍳3 ⋄ A[0]", ErrorKind::Domain, "A[0]", &["index 0", "3 items"]),
    apl("A←⍳3 ⋄ A[1 2 3 4]", ErrorKind::Domain, "A[1 2 3 4]", &["index 4", "3 items"]),
    apl("A←3 3⍴⍳9 ⋄ A[1]", ErrorKind::Rank, "A[1]", &["1 index slot", "rank 2"]),
    apl("A←3 3⍴⍳9 ⋄ A[1;2;3]", ErrorKind::Rank, "A[1;2;3]", &["3 index slot", "rank 2"]),
    apl(
        "A←2 3⍴⍳6 ⋄ A[1;7]",
        ErrorKind::Domain,
        "A[1;7]",
        &["index 7", "axis 1", "3 items"],
    ),
    // Writing through the brackets answers the same way reading does.
    apl(
        "A←⍳3 ⋄ A[9]←1",
        ErrorKind::Domain,
        "A[9]←1",
        &["index 9", "axis 0", "3 element"],
    ),
    apl("A←⍳3 ⋄ A[1]←'x'", ErrorKind::Type, "A[1]←'x'", &["character", "integer"]),
    apl("5⌷⍳3", ErrorKind::Domain, "5⌷⍳3", &["index 5", "axis 0"]),
    apl("1 2 3⌷2 3⍴⍳6", ErrorKind::Rank, "1 2 3⌷2 3⍴⍳6", &["3 index", "rank 2"]),
];

#[test]
fn apl_index_errors_name_the_index_the_axis_and_its_length() {
    check_all(APL_INDEX);
}

// --- APL: names ---------------------------------------------------------

const APL_NAMES: &[Case] = &[
    apl("nope+1", ErrorKind::Value, "nope", &["nope"]),
    // A dfn runs monadically and finds ⍺ has no value, which is the
    // reference's answer too.
    apl("{⍺+⍵}5", ErrorKind::Value, "⍺", &["⍺"]),
    apl("⎕ZZ", ErrorKind::NotYet, "⎕ZZ", &["⎕ZZ"]),
];

#[test]
fn apl_name_errors_name_the_name() {
    check_all(APL_NAMES);
}

// --- APL: the gaps ------------------------------------------------------

const APL_PROMISES: &[Case] = &[
    apl("1(=⍠('ZZ' 2))2", ErrorKind::NotYet, "=⍠('ZZ' 2)", &["variant option ZZ"]),
    apl("1(=⍠A)2", ErrorKind::NotYet, "A", &["computed variant option"]),
    apl("3+F←+/", ErrorKind::NotYet, "F←+/", &["naming a function inside"]),
    apl("(A+×)4", ErrorKind::NotYet, "A", &["computed value"]),
    apl("⍕[1]⍳3", ErrorKind::NotYet, "[1]", &["axis specification for ⍕"]),
    apl("2 3⍕⊂1 2", ErrorKind::NotYet, "2 3⍕⊂1 2", &["nested"]),
];

#[test]
fn apl_gaps_are_promises_and_name_the_feature() {
    check_all(APL_PROMISES);
}

const APL_PERMANENT: &[Case] = &[
    // The sandbox is a property of libjay, not a queue position.
    apl("⎕TS", ErrorKind::Sandbox, "⎕TS", &["closed by the sandbox"]),
    apl("⎕FIO", ErrorKind::Sandbox, "⎕FIO", &["closed by the sandbox"]),
    apl("⎕AI", ErrorKind::Sandbox, "⎕AI", &["closed by the sandbox"]),
    apl("⎕TS←1", ErrorKind::Sandbox, "⎕TS", &["closed by the sandbox"]),
    // The dialect fixed these before the program was compiled, which is a
    // decision and not a queue position either.
    apl("⎕IO←0", ErrorKind::Language, "⎕IO", &["⎕IO", "read-only"]),
    apl("⎕CT←1", ErrorKind::Language, "⎕CT", &["⎕CT", "read-only"]),
    apl("⎕A←'x'", ErrorKind::Language, "⎕A", &["⎕A", "read-only"]),
];

#[test]
fn apl_permanent_refusals_do_not_read_as_promises() {
    check_all(APL_PERMANENT);
}

// --- APL: a diagnostic from inside ⍎ and inside a definition ------------

const APL_EXECUTE: &[Case] = &[
    apl(
        "⍎'1÷0'",
        ErrorKind::Domain,
        "⍎'1÷0'",
        &["in the executed string", "division by zero"],
    ),
    apl("⍎'nope'", ErrorKind::Value, "⍎'nope'", &["in the executed string", "nope"]),
    apl(
        "⍎'1 2+1 2 3'",
        ErrorKind::Length,
        "⍎'1 2+1 2 3'",
        &["in the executed string", "left shape 2", "right shape 3"],
    ),
    apl("⍎'{'", ErrorKind::Parse, "⍎'{'", &["in the executed string"]),
];

#[test]
fn apl_execute_reports_at_the_sentence_that_ran_the_string() {
    check_all(APL_EXECUTE);
}

const APL_DEFINITION: &[Case] = &[
    apl("{⍵+1 2}1 2 3", ErrorKind::Length, "⍵+1 2", &["left shape 3", "right shape 2"]),
    apl(
        "∇Z←F R ⋄ Z←R+1 2 3 ⋄ ∇ ⋄ F 1 2",
        ErrorKind::Length,
        "R+1 2 3",
        &["left shape 2", "right shape 3"],
    ),
    apl(
        "∇Z←F R ⋄ :If 1 ⋄ Z←1 2+1 2 3 ⋄ :EndIf ⋄ ∇ ⋄ F 1",
        ErrorKind::Length,
        "1 2+1 2 3",
        &["left shape 2", "right shape 3"],
    ),
];

#[test]
fn apl_a_definition_reports_inside_its_own_body() {
    check_all(APL_DEFINITION);
}

// --- The program and the data it was given ------------------------------

#[test]
fn a_program_run_without_its_data_names_the_parameters_it_wanted() {
    let program = compile(Lang::J, "1 2 + {x} * {w}", &Dialect::default()).expect("compile");
    let mut sink = |_: &str| {};
    let e = program.run(&[], &mut sink).expect_err("no data was given");
    assert_eq!(e.kind, ErrorKind::Value);
    assert!(e.msg.contains('x') && e.msg.contains('w'), "{}", e.msg);
    // Nothing in the source is at fault, so nothing in it is pointed at.
    assert!(e.span.is_none(), "{e:?}");
}

// --- explain --------------------------------------------------------

/// Every case above, whichever table it is in.
fn every_case() -> Vec<&'static Case> {
    [
        J_SYNTAX,
        J_SHAPE,
        J_DOMAIN,
        J_INDEX,
        J_NAMES,
        J_PROMISES,
        J_PERMANENT,
        J_EXECUTE,
        J_DEFINITION,
        APL_SYNTAX,
        APL_SHAPE,
        APL_DOMAIN,
        APL_INDEX,
        APL_NAMES,
        APL_PROMISES,
        APL_PERMANENT,
        APL_EXECUTE,
        APL_DEFINITION,
    ]
    .iter()
    .flat_map(|t| t.iter())
    .collect()
}

/// Explaining a program is a diagnostic act like any other: a program that
/// fails at run time still has a structure to show, and the failure is
/// reported at the end of it rather than thrown.
#[test]
fn explain_answers_for_every_program_that_compiles() {
    for case in every_case() {
        let Ok(program) = compile(case.lang, case.src, &Dialect::default()) else {
            continue;
        };
        let structure = program.explain(None);
        assert!(structure.contains("sentence 1"), "{:?}:\n{structure}", case.src);
        let run = program.explain(Some(&[]));
        assert!(run.contains("the run stopped here:"), "{:?}:\n{run}", case.src);
    }
}

/// Nothing above is a program that works: a case that starts passing is a
/// case that has stopped testing anything.
#[test]
fn every_case_really_fails() {
    let mut n = 0;
    for case in every_case() {
        failure(case);
        n += 1;
    }
    assert!(n >= 80, "the battery has shrunk to {n} cases");
}
