//! The input half of the sandbox: APL's `⍞` and `⎕`, J's `1!:1` and
//! `1!:2`, and the foreigns the sandbox closes.
//!
//! No oracle covers these: `jay-corpus` runs one process per sentence and
//! has nowhere to put a line of input for it, and GNU APL's `⎕` prints a
//! prompt libjay does not. The expectations here are hand-written, on data:
//! a list of lines goes in, a value comes out, and the refusals are checked
//! for the class they carry as much as for the words they use.

use jay::{compile, Array, Data, Dialect, ErrorKind, Lang};

/// Run with a list of lines as the input source; the output goes nowhere.
/// A refusal the compiler makes comes back the same way one the run makes
/// does: the reader is owed the same diagnostic either way.
fn run_lines(lang: Lang, src: &str, lines: &[&str]) -> Result<Option<Array>, jay::Error> {
    let program = compile(lang, src, &Dialect::default())?;
    let mut it = lines.iter();
    let mut inp = || it.next().map(|s| (*s).to_string());
    let mut sink = |_: &str| {};
    program.run_io(&[], &mut sink, &mut inp)
}

/// Run with a list of lines, collecting what the program wrote.
fn run_io(lang: Lang, src: &str, lines: &[&str]) -> (Option<Array>, String) {
    let program = compile(lang, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile of {src:?} failed:\n{}", e.render(src)));
    let mut it = lines.iter();
    let mut inp = || it.next().map(|s| (*s).to_string());
    let mut written = String::new();
    let mut sink = |s: &str| written.push_str(s);
    let value = program
        .run_io(&[], &mut sink, &mut inp)
        .unwrap_or_else(|e| panic!("run of {src:?} failed:\n{}", program.render_error(&e)));
    (value, written)
}

fn value(lang: Lang, src: &str, lines: &[&str]) -> Array {
    run_lines(lang, src, lines)
        .unwrap_or_else(|e| panic!("run of {src:?} failed:\n{e}"))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn failure(lang: Lang, src: &str, lines: &[&str]) -> jay::Error {
    match run_lines(lang, src, lines) {
        Err(e) => e,
        Ok(v) => panic!("{src:?} was expected to fail; it answered {v:?}"),
    }
}

fn chars(s: &str) -> Array {
    Array::from_chars(s.chars().collect())
}

// --- reading ---------------------------------------------------------------

#[test]
fn quote_quad_is_the_line_itself() {
    assert_eq!(value(Lang::Apl, "⍞", &["hello world"]), chars("hello world"));
    // The terminator is not part of the line, and an empty line is a line.
    assert_eq!(value(Lang::Apl, "⍞", &[""]), chars(""));
    assert_eq!(value(Lang::Apl, "≢⍞", &["abc"]), Array::scalar_i64(3));
}

#[test]
fn j_reads_stdin_through_the_file_foreign() {
    assert_eq!(value(Lang::J, "1!:1 ]1", &["hello"]), chars("hello"));
    // `[1` is the same argument by the other identity verb.
    assert_eq!(value(Lang::J, "1!:1 [1", &["hello"]), chars("hello"));
    assert_eq!(value(Lang::J, "# 1!:1 ]1", &["abcd"]), Array::scalar_i64(4));
}

#[test]
fn each_read_takes_the_next_line() {
    // Two sentences, two lines, in the order they were given.
    let v = value(Lang::Apl, "a←⍞ ⋄ b←⍞ ⋄ a,b", &["one", "two"]);
    assert_eq!(v, chars("onetwo"));
}

#[test]
fn a_sentence_reads_right_to_left() {
    // GNU APL, fed "first" then "second", answers `⍞,⍞` with
    // "secondfirst": the RIGHT read takes the first line, as every other
    // right-to-left evaluation in the language does.
    assert_eq!(value(Lang::Apl, "⍞,⍞", &["first", "second"]), chars("secondfirst"));
}

#[test]
fn a_read_line_is_an_ordinary_value() {
    // A value ends at `⍞`, so it strands, indexes and takes a function on
    // its left like any other.
    assert_eq!(value(Lang::Apl, "⍞[2]", &["abc"]), Array::new(vec![], Data::Char(vec!['b'].into())));
    assert_eq!(value(Lang::Apl, "≢⍞ ⍞", &["a", "bb"]), Array::scalar_i64(2));
}

#[test]
fn evaluated_input_runs_the_line_as_apl() {
    assert_eq!(value(Lang::Apl, "⎕", &["2+2"]), Array::scalar_i64(4));
    assert_eq!(value(Lang::Apl, "1+⎕", &["⍳3"]), {
        Array::new(vec![3], Data::I64(vec![2, 3, 4].into()))
    });
    // It runs over the names the program already has, as `⍎` does.
    assert_eq!(value(Lang::Apl, "x←10 ⋄ ⎕", &["x×2"]), Array::scalar_i64(20));
}

#[test]
fn a_line_that_will_not_run_reports_itself() {
    let e = failure(Lang::Apl, "⎕", &["2+"]);
    assert_eq!(e.kind, ErrorKind::Parse);
    assert!(e.msg.contains("in the executed string"), "{}", e.msg);
}

#[test]
fn the_end_of_the_input_is_a_diagnostic() {
    for (lang, src) in [(Lang::Apl, "⍞"), (Lang::Apl, "⎕"), (Lang::J, "1!:1 ]1")] {
        let e = failure(lang, src, &[]);
        assert_eq!(e.kind, ErrorKind::Value, "{src}");
        assert!(e.msg.contains("the input has ended"), "{src}: {}", e.msg);
    }
    // The second read of a one-line input is the same thing.
    let e = failure(Lang::Apl, "a←⍞ ⋄ ⍞", &["only"]);
    assert!(e.msg.contains("the input has ended"), "{}", e.msg);
}

#[test]
fn a_run_with_no_input_source_says_so() {
    let program = compile(Lang::Apl, "⍞", &Dialect::default()).expect("compiles");
    let mut sink = |_: &str| {};
    let e = program.run(&[], &mut sink).expect_err("no input source");
    assert_eq!(e.kind, ErrorKind::Value);
    assert!(e.msg.contains("no input source attached"), "{}", e.msg);
}

// --- writing ---------------------------------------------------------------

#[test]
fn the_write_foreign_is_the_output_sink() {
    // jconsole: `x =. 'abc' 1!:2 ]2` writes "abc\n" and the value is x.
    let (v, written) = run_io(Lang::J, "'abc' 1!:2 ]2", &[]);
    assert_eq!(written, "abc\n");
    assert_eq!(v, Some(chars("abc")));
    // A numeric left argument is written as it displays, as jconsole does.
    let (_, written) = run_io(Lang::J, "x =. (65 66 67) 1!:2 ]2", &[]);
    assert_eq!(written, "65 66 67\n");
}

#[test]
fn quad_output_ends_the_line_and_quote_quad_does_not() {
    let (_, written) = run_io(Lang::Apl, "⎕←'ab'", &[]);
    assert_eq!(written, "ab\n");
    let (_, written) = run_io(Lang::Apl, "⍞←'ab' ⋄ ⍞←'cd'", &[]);
    assert_eq!(written, "abcd");
    // Both pass the value on, so both are silent sentences.
    let (v, _) = run_io(Lang::Apl, "x←⍞←'ab' ⋄ x", &[]);
    assert_eq!(v, Some(chars("ab")));
}

#[test]
fn reading_and_writing_meet_in_one_program() {
    let (v, written) = run_io(Lang::J, "(1!:1 ]1) 1!:2 ]2", &["echoed"]);
    assert_eq!(written, "echoed\n");
    assert_eq!(v, Some(chars("echoed")));
}

// --- the type foreign ------------------------------------------------------

#[test]
fn the_type_foreign_answers_js_own_numbers() {
    // The codes are jconsole's: `(3!:0 (5)),(3!:0 'a')` and so on.
    let cases: &[(&str, i64)] = &[
        ("3!:0 (1=1)", 1),
        ("3!:0 (0 1 = 1)", 1),
        ("3!:0 (5)", 4),
        ("3!:0 (2+2)", 4),
        ("3!:0 (i.5)", 4),
        ("3!:0 (1.5)", 8),
        ("3!:0 'a'", 2),
        ("3!:0 ''", 2),
        ("3!:0 (1j2)", 16),
        ("3!:0 (1;2)", 32),
        ("3!:0 (1x)", 64),
        ("3!:0 (1r2)", 128),
    ];
    for (src, code) in cases {
        assert_eq!(value(Lang::J, src, &[]), Array::scalar_i64(*code), "{src}");
    }
    // A literal whose atoms are all 0 or 1 is stored as booleans, which is
    // the storage jconsole reports for it too.
    assert_eq!(value(Lang::J, "3!:0 (1)", &[]), Array::scalar_i64(1));
    assert_eq!(value(Lang::J, "3!:0 (1 0 1)", &[]), Array::scalar_i64(1));
}

// --- the refusals ----------------------------------------------------------

#[test]
fn a_file_is_closed_whether_it_is_named_or_numbered() {
    for src in ["1!:1 <'/etc/hosts'", "1!:1 ]3", "'x' 1!:2 <'/tmp/x'", "'x' 1!:2 ]4"] {
        let e = failure(Lang::J, src, &["unused"]);
        assert_eq!(e.kind, ErrorKind::Sandbox, "{src}: {}", e.msg);
        assert!(e.msg.contains("outside the program"), "{src}: {}", e.msg);
    }
}

#[test]
fn the_foreigns_that_reach_outside_are_refused_by_name() {
    let cases: &[(&str, &str)] = &[
        ("0!:0 <'x'", "script"),
        ("1!:5 <'d'", "filesystem"),
        ("1!:21 <'f'", "filesystem"),
        ("2!:5 <'HOME'", "host"),
        ("2!:55 ]0", "host"),
        ("6!:0 ''", "clock"),
        ("6!:2 'i.5'", "clock"),
        ("15!:0 ]0", "shared library"),
    ];
    for (src, what) in cases {
        let e = failure(Lang::J, src, &[]);
        assert_eq!(e.kind, ErrorKind::Sandbox, "{src}: {}", e.msg);
        assert_eq!(e.kind.label(), "closed by the sandbox");
        assert!(e.msg.contains(what), "{src}: {}", e.msg);
    }
}

#[test]
fn the_foreigns_that_only_compute_are_a_queue_position() {
    for src in ["3!:6 ]5", "4!:5 ''", "5!:4 <'x'", "9!:0 ''", "128!:0 i. 2 2"] {
        let e = failure(Lang::J, src, &[]);
        assert_eq!(e.kind, ErrorKind::NotYet, "{src}: {}", e.msg);
        assert!(e.msg.contains("foreign"), "{src}: {}", e.msg);
    }
}

// --- explain ---------------------------------------------------------------

#[test]
fn explain_says_a_node_reads_stdin() {
    let program = compile(Lang::Apl, "≢⍞", &Dialect::default()).expect("compiles");
    assert!(program.explain(None).contains("reads stdin"), "{}", program.explain(None));
    let program = compile(Lang::Apl, "⎕", &Dialect::default()).expect("compiles");
    let text = program.explain(None);
    assert!(text.contains("reads stdin and runs the line"), "{text}");
    let program = compile(Lang::J, "1!:1 ]1", &Dialect::default()).expect("compiles");
    assert!(program.explain(None).contains("reads stdin"), "{}", program.explain(None));
}
