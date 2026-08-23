//! End-to-end tests: source text in, values out, through both frontends and
//! the shared IR. These encode the phase-1/phase-2 gates.

use jay::{compile, Array, Data, Dialect, ErrorKind, Lang};

fn run(lang: Lang, src: &str) -> Option<Array> {
    run_dialect(lang, src, &Dialect::default())
}

fn run_dialect(lang: Lang, src: &str, dialect: &Dialect) -> Option<Array> {
    let program = compile(lang, src, dialect)
        .unwrap_or_else(|e| panic!("compile failed:\n{}", e.render(src)));
    let mut out = String::new();
    program
        .run(&[], &mut |s| out.push_str(s))
        .unwrap_or_else(|e| panic!("run failed:\n{}", program.render_error(&e)))
}

fn run_capture(lang: Lang, src: &str) -> (Option<Array>, String) {
    let program = compile(lang, src, &Dialect::default()).expect("compile");
    let mut out = String::new();
    let result = program.run(&[], &mut |s| out.push_str(s)).expect("run");
    (result, out)
}

/// Whether a session would keep this program's answer to itself.
fn shy(lang: Lang, src: &str) -> bool {
    let program = compile(lang, src, &Dialect::default()).expect("compile");
    let mut sink = |_: &str| {};
    program.run_detail(&[], &mut sink).expect("run").shy
}

fn err(lang: Lang, src: &str) -> jay::Error {
    let program = match compile(lang, src, &Dialect::default()) {
        Err(e) => return e,
        Ok(p) => p,
    };
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink).expect_err("expected an error")
}

fn i64s(shape: &[usize], values: &[i64]) -> Array {
    Array::new(shape.to_vec(), Data::I64(values.to_vec().into()))
}

fn f64s(shape: &[usize], values: &[f64]) -> Array {
    Array::new(shape.to_vec(), Data::F64(values.to_vec().into()))
}

// --- J basics -----------------------------------------------------------

#[test]
fn j_arithmetic() {
    assert_eq!(run(Lang::J, "2 + 2"), Some(Array::scalar_i64(4)));
    assert_eq!(run(Lang::J, "1 2 3 * 10"), Some(i64s(&[3], &[10, 20, 30])));
    assert_eq!(run(Lang::J, "10 % 4"), Some(Array::scalar_f64(2.5)));
    assert_eq!(run(Lang::J, "- 3"), Some(Array::scalar_i64(-3)));
    assert_eq!(run(Lang::J, "2 ^ 10"), Some(Array::scalar_i64(1024)));
}

#[test]
fn j_division_by_zero_follows_j_rules() {
    assert_eq!(run(Lang::J, "0 % 0"), Some(Array::scalar_f64(0.0)));
    assert_eq!(run(Lang::J, "5 % 0"), Some(Array::scalar_f64(f64::INFINITY)));
    assert_eq!(run(Lang::J, "_5 % 0"), Some(Array::scalar_f64(f64::NEG_INFINITY)));
}

#[test]
fn j_reduce_folds_right_to_left() {
    assert_eq!(run(Lang::J, "+/ 1 2 3"), Some(Array::scalar_i64(6)));
    assert_eq!(run(Lang::J, "-/ 1 2 3"), Some(Array::scalar_i64(2)));
}

#[test]
fn j_reduce_is_leading_axis() {
    assert_eq!(run(Lang::J, "+/ i. 2 3"), Some(i64s(&[3], &[3, 5, 7])));
}

#[test]
fn j_rank_conjunction_reduces_rows() {
    assert_eq!(run(Lang::J, "+/\"1 i. 2 3"), Some(i64s(&[2], &[3, 12])));
}

#[test]
fn j_iota() {
    assert_eq!(run(Lang::J, "i. 4"), Some(i64s(&[4], &[0, 1, 2, 3])));
    assert_eq!(run(Lang::J, "i. 2 3"), Some(i64s(&[2, 3], &[0, 1, 2, 3, 4, 5])));
    assert_eq!(run(Lang::J, "i. _3"), Some(i64s(&[3], &[2, 1, 0])));
}

#[test]
fn j_structural() {
    assert_eq!(run(Lang::J, "$ i. 2 3"), Some(i64s(&[2], &[2, 3])));
    assert_eq!(run(Lang::J, "2 3 $ 1 2 3 4 5 6"), Some(i64s(&[2, 3], &[1, 2, 3, 4, 5, 6])));
    assert_eq!(run(Lang::J, "2 3 $ 0 1"), Some(i64s(&[2, 3], &[0, 1, 0, 1, 0, 1])));
    assert_eq!(run(Lang::J, "|: i. 2 3"), Some(i64s(&[3, 2], &[0, 3, 1, 4, 2, 5])));
    assert_eq!(run(Lang::J, "# 7 8 9"), Some(Array::scalar_i64(3)));
    assert_eq!(run(Lang::J, ", i. 2 2"), Some(i64s(&[4], &[0, 1, 2, 3])));
}

#[test]
fn j_take_drop() {
    assert_eq!(run(Lang::J, "2 {. 5 6 7 8"), Some(i64s(&[2], &[5, 6])));
    assert_eq!(run(Lang::J, "_2 {. 5 6 7 8"), Some(i64s(&[2], &[7, 8])));
    assert_eq!(run(Lang::J, "6 {. 1 2 3"), Some(i64s(&[6], &[1, 2, 3, 0, 0, 0])));
    assert_eq!(run(Lang::J, "_5 {. 1 2 3"), Some(i64s(&[5], &[0, 0, 1, 2, 3])));
    assert_eq!(run(Lang::J, "1 }. i. 3 2"), Some(i64s(&[2, 2], &[2, 3, 4, 5])));
    assert_eq!(run(Lang::J, "{. 5 6 7"), Some(Array::scalar_i64(5)));
    assert_eq!(run(Lang::J, "}. 5 6 7"), Some(i64s(&[2], &[6, 7])));
}

#[test]
fn j_fork_mean() {
    // The tacit mean: the expression the diagnostics section calls out.
    assert_eq!(run(Lang::J, "(+/ % #) 1 2 3 4"), Some(Array::scalar_f64(2.5)));
}

#[test]
fn j_named_verbs() {
    // Naming a verb is a parse-time substitution: the name is a verb in
    // every later sentence, wherever a verb may stand.
    assert_eq!(run(Lang::J, "mean =. +/ % #\nmean 1 2 3 4"), Some(Array::scalar_f64(2.5)));
    assert_eq!(
        run(Lang::J, "mean =. +/ % #\n(mean - {.) 1 2 3 4"),
        Some(Array::scalar_f64(1.5))
    );
    assert_eq!(
        run(Lang::J, "mean =. +/ % #\nmean\"1 i. 3 3"),
        Some(f64s(&[3], &[1.0, 4.0, 7.0]))
    );
    // Dyadically too, and redefinition rebinds from that sentence on.
    assert_eq!(run(Lang::J, "n =. #\n2 n 1 2 3"), Some(i64s(&[6], &[1, 1, 2, 2, 3, 3])));
    assert_eq!(run(Lang::J, "f =. +/\nf =. #\nf 1 2 3"), Some(Array::scalar_i64(3)));
    // A name may change part of speech in either direction.
    assert_eq!(run(Lang::J, "a =. 1 2 3\na =. +/\na 1 2 3"), Some(Array::scalar_i64(6)));
    assert_eq!(run(Lang::J, "f =. +/\nf =. 10 20\nf"), Some(i64s(&[2], &[10, 20])));
    // Naming a verb yields no value, so it displays nothing.
    assert_eq!(run(Lang::J, "1 + 1\nmean =. +/ % #"), None);
}

#[test]
fn j_hook_and_cap() {
    // Hook: y - (mean y) as ([-(+/%#)]) is a fork; test a simpler hook (* -): y * (-y).
    assert_eq!(run(Lang::J, "(* -) 3"), Some(Array::scalar_i64(-9)));
    // Capped fork: [: # $ — tally of the shape = the rank.
    assert_eq!(run(Lang::J, "([: # $) i. 2 3 4"), Some(Array::scalar_i64(3)));
}

#[test]
fn j_prefix_agreement_broadcasts_rows() {
    // Frames 2x3 vs 2: each row of the matrix pairs with one atom.
    assert_eq!(
        run(Lang::J, "(i. 2 3) + 10 20"),
        Some(i64s(&[2, 3], &[10, 11, 12, 23, 24, 25]))
    );
}

#[test]
fn j_sequence_and_assignment() {
    assert_eq!(run(Lang::J, "x =. 5\nx * 2"), Some(Array::scalar_i64(10)));
    // A sequence ending in an assignment yields no value.
    assert_eq!(run(Lang::J, "x =. 5"), None);
    // Inline assignment yields its value in expression position.
    assert_eq!(run(Lang::J, "2 + x =. 3\nx"), Some(Array::scalar_i64(3)));
}

#[test]
fn j_strings_and_echo() {
    assert_eq!(
        run(Lang::J, "'hi'"),
        Some(Array::new(vec![2], Data::Char(vec!['h', 'i'].into())))
    );
    let (result, out) = run_capture(Lang::J, "echo 'Hello, world!'");
    assert_eq!(out, "Hello, world!\n");
    // echo returns an empty array; the sentence has a (vacuous) value.
    assert_eq!(result.map(|a| a.count()), Some(0));
}

#[test]
fn j_comments() {
    assert_eq!(run(Lang::J, "2 + 3 NB. sum"), Some(Array::scalar_i64(5)));
}

#[test]
fn j_float_promotion_on_overflow() {
    let big = i64::MAX;
    let expr = format!("{big} + {big}");
    let got = run(Lang::J, &expr).unwrap();
    assert_eq!(got, Array::scalar_f64(big as f64 + big as f64));
}

// --- J parameters -------------------------------------------------------

#[test]
fn j_params() {
    let program = compile(Lang::J, "+/ {w} * {d}", &Dialect::default()).unwrap();
    assert_eq!(
        program.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["w", "d"]
    );
    let w = f64s(&[3], &[0.5, 0.25, 0.25]);
    let d = f64s(&[3], &[10.0, 20.0, 30.0]);
    let mut sink = |_: &str| {};
    let got = program.run(&[w, d], &mut sink).unwrap();
    assert_eq!(got, Some(Array::scalar_f64(17.5)));
}

#[test]
fn j_repeated_param_is_one_parameter() {
    let program = compile(Lang::J, "{x} + {x}", &Dialect::default()).unwrap();
    assert_eq!(program.params.len(), 1);
}

// --- J errors -----------------------------------------------------------

#[test]
fn j_length_error_names_both_shapes() {
    let e = err(Lang::J, "1 2 + 1 2 3");
    assert!(matches!(e.kind, ErrorKind::Length | ErrorKind::Shape));
    let text = format!("{e}");
    assert!(text.contains('2') && text.contains('3'), "should name both shapes: {text}");
}

#[test]
fn j_char_arithmetic_is_type_error() {
    let e = err(Lang::J, "2 + 'a'");
    assert_eq!(e.kind, ErrorKind::Type);
}

#[test]
fn j_undefined_name() {
    let e = err(Lang::J, "nope + 1");
    assert_eq!(e.kind, ErrorKind::Value);
    assert!(e.msg.contains("nope"));
}

#[test]
fn j_not_yet_is_a_promise_not_a_wall() {
    let e = err(Lang::J, "$. 'abc'");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("sparse"));
}

#[test]
fn j_parse_error_has_span() {
    let e = compile(Lang::J, "2 ]: 3", &Dialect::default()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Parse);
    assert!(e.span.is_some());
    // The rendered form points into the source.
    assert!(e.render("2 ]: 3").contains('^'));
}

// --- APL basics ---------------------------------------------------------

#[test]
fn apl_arithmetic() {
    assert_eq!(run(Lang::Apl, "2+2"), Some(Array::scalar_i64(4)));
    assert_eq!(run(Lang::Apl, "¯2×3"), Some(Array::scalar_i64(-6)));
    // Right to left: the minus is monadic.
    assert_eq!(run(Lang::Apl, "-3+4"), Some(Array::scalar_i64(-7)));
}

#[test]
fn apl_division_by_zero_follows_apl_rules() {
    assert_eq!(run(Lang::Apl, "0÷0"), Some(Array::scalar_f64(1.0)));
    let e = err(Lang::Apl, "1÷0");
    assert_eq!(e.kind, ErrorKind::Domain);
}

#[test]
fn apl_iota_respects_index_origin() {
    assert_eq!(run(Lang::Apl, "⍳4"), Some(i64s(&[4], &[1, 2, 3, 4])));
    let zero = Dialect { index_origin: Some(0), ..Dialect::default() };
    assert_eq!(run_dialect(Lang::Apl, "⍳4", &zero), Some(i64s(&[4], &[0, 1, 2, 3])));
    assert_eq!(run(Lang::Apl, "⍳0"), Some(Array::empty(jay::DType::I64)));
    // A vector argument asks for an array of index vectors: one nested
    // array of that shape, not a per-atom frame of counting vectors.
    let nested = run(Lang::Apl, "⍳2 3").expect("a value");
    assert_eq!(nested.shape, vec![2, 3]);
    assert_eq!(run(Lang::Apl, "≡⍳2 3"), Some(Array::scalar_i64(2)));
    // J's `i.` reshapes on the same argument and is unaffected.
    assert_eq!(run(Lang::J, "i. 2 3"), Some(i64s(&[2, 3], &[0, 1, 2, 3, 4, 5])));
}

#[test]
fn apl_structural() {
    assert_eq!(run(Lang::Apl, "2 3⍴⍳6"), Some(i64s(&[2, 3], &[1, 2, 3, 4, 5, 6])));
    assert_eq!(run(Lang::Apl, "⍴2 3⍴⍳6"), Some(i64s(&[2], &[2, 3])));
    assert_eq!(run(Lang::Apl, "⍉2 3⍴⍳6"), Some(i64s(&[3, 2], &[1, 4, 2, 5, 3, 6])));
    assert_eq!(run(Lang::Apl, "≢7 8 9"), Some(Array::scalar_i64(3)));
    assert_eq!(run(Lang::Apl, "2↑9 8 7"), Some(i64s(&[2], &[9, 8])));
    assert_eq!(run(Lang::Apl, "¯2↑9 8 7"), Some(i64s(&[2], &[8, 7])));
    assert_eq!(run(Lang::Apl, "5↑1 2"), Some(i64s(&[5], &[1, 2, 0, 0, 0])));
    // APL wants one count per axis, so a table needs two of them; J's rule,
    // where a short list leaves the remaining axes whole, is not APL's.
    assert_eq!(run(Lang::Apl, "1 0↓3 3⍴⍳9"), Some(i64s(&[2, 3], &[4, 5, 6, 7, 8, 9])));
    assert_eq!(err(Lang::Apl, "1↓3 3⍴⍳9").kind, ErrorKind::Length);
    assert_eq!(run(Lang::Apl, ",2 2⍴⍳4"), Some(i64s(&[4], &[1, 2, 3, 4])));
}

#[test]
fn apl_sequence_assignment_and_quad() {
    assert_eq!(run(Lang::Apl, "x←3 ⋄ x+1"), Some(Array::scalar_i64(4)));
    assert_eq!(run(Lang::Apl, "x←3"), None);
    assert_eq!(run(Lang::Apl, "2+a←3"), Some(Array::scalar_i64(5)));
    let (result, out) = run_capture(Lang::Apl, "⎕←2+2");
    assert_eq!(out, "4\n");
    assert_eq!(result, None); // explicit output is not re-displayed
    // Inside an expression the value still passes through.
    assert_eq!(run(Lang::Apl, "1+⎕←2+2"), Some(Array::scalar_i64(5)));
}

/// A dfn whose answer came from an assignment answers SHYLY: the value
/// flows wherever it is used, and the sentence that only applied the
/// definition displays nothing.
#[test]
fn apl_shy_results() {
    assert_eq!(run(Lang::Apl, "{a←⍵×2} 5"), Some(Array::scalar_i64(10)));
    assert!(shy(Lang::Apl, "{a←⍵×2} 5"));
    assert!(shy(Lang::Apl, "F←{r←⍵×2} ⋄ F 5"));
    assert!(shy(Lang::Apl, "{a←⍵ ⋄ b←a+1} 5"));
    // A guard chooses the sentence the answer comes from, so shyness is
    // the chosen one's.
    assert!(shy(Lang::Apl, "F←{⍵>0:r←'pos' ⋄ r←'neg'} ⋄ F 3"));
    assert!(!shy(Lang::Apl, "F←{a←⍵×2 ⋄ a} ⋄ F 3"));
    // Consuming the value is what makes it visible again: as an argument,
    // or through a name.
    assert_eq!(run(Lang::Apl, "1+{a←⍵×2} 5"), Some(Array::scalar_i64(11)));
    assert!(!shy(Lang::Apl, "1+{a←⍵×2} 5"));
    assert!(!shy(Lang::Apl, "F←{r←⍵×2} ⋄ x←F 5 ⋄ x"));
    assert!(!shy(Lang::Apl, "F←{r←⍵×2} ⋄ (F 5)+F 6"));
    assert!(!shy(Lang::Apl, "F←{r←⍵×2} ⋄ ⌽F 5 6"));
    // An operator that ends by applying the definition keeps its shyness;
    // one that ends by applying a primitive does not.
    assert!(shy(Lang::Apl, "{a←⍵×2}¨1 2 3"));
    assert!(shy(Lang::Apl, "F←{r←⍵×2} ⋄ F⍣2⊢5"));
    assert!(!shy(Lang::Apl, "F←{r←⍵×2} ⋄ +/F¨1 2 3"));
    // J has no shy results: an explicit definition ending in an
    // assignment answers with the assigned value, displayed like any
    // other.
    assert!(!shy(Lang::J, "f =. 3 : 'a =. y*2'\nf 5"));
    assert_eq!(run(Lang::J, "f =. 3 : 'a =. y*2'\nf 5"), Some(Array::scalar_i64(10)));
}

#[test]
fn apl_strict_agreement_rejects_what_j_broadcasts() {
    // J happily pairs a 2x3 matrix with a 2-vector; APL refuses.
    let e = err(Lang::Apl, "(2 3⍴⍳6)+10 20");
    assert!(matches!(e.kind, ErrorKind::Shape | ErrorKind::Length | ErrorKind::Rank));
    // Scalar extension still works.
    assert_eq!(
        run(Lang::Apl, "(2 3⍴⍳6)+10"),
        Some(i64s(&[2, 3], &[11, 12, 13, 14, 15, 16]))
    );
}

#[test]
fn apl_comments() {
    assert_eq!(run(Lang::Apl, "2+3 ⍝ sum"), Some(Array::scalar_i64(5)));
}

// --- The phase-2 gate ---------------------------------------------------

/// J and APL must produce CORRECT and DIFFERENT results for `+/` on the
/// same matrix, through one IR. This is the architectural bet.
#[test]
fn divergence_gate_reduction_axis() {
    // Same data in both languages: 2x3 matrix of 0..5.
    let j_sum = run(Lang::J, "+/ i. 2 3").unwrap(); // leading axis
    let apl_sum = run_dialect(
        Lang::Apl,
        "+/2 3⍴⍳6",
        &Dialect { index_origin: Some(0), ..Dialect::default() },
    )
    .unwrap(); // trailing axis
    let apl_sum_leading = run_dialect(
        Lang::Apl,
        "+⌿2 3⍴⍳6",
        &Dialect { index_origin: Some(0), ..Dialect::default() },
    )
    .unwrap();

    assert_eq!(j_sum, i64s(&[3], &[3, 5, 7]), "J +/ sums along columns");
    assert_eq!(apl_sum, i64s(&[2], &[3, 12]), "APL +/ sums along rows");
    assert_ne!(j_sum, apl_sum, "the same spelling must diverge");
    assert_eq!(apl_sum_leading, j_sum, "APL +⌿ agrees with J +/");
}

#[test]
fn hello_world_both_languages() {
    let (_, out_j) = run_capture(Lang::J, "echo 'Hello, world!'");
    assert_eq!(out_j, "Hello, world!\n");
    let (_, out_apl) = run_capture(Lang::Apl, "⎕←'Hello, world!'");
    assert_eq!(out_apl, "Hello, world!\n");
}

#[test]
fn a_program_nested_past_the_ceiling_is_refused_rather_than_run() {
    // A string is the interface, so a pathological one has to come back as
    // a diagnostic: nesting deeply enough used to exhaust the stack and
    // take the host process with it.
    for (lang, deep) in [
        (Lang::J, "<".repeat(2_000) + "1"),
        (Lang::J, "+/".repeat(2_000) + " 1 2 3"),
        (Lang::Apl, "⊂".repeat(2_000) + "1"),
    ] {
        let e = err(lang, &deep);
        assert_eq!(e.kind, ErrorKind::Limit, "{}", e.msg);
        assert!(e.msg.contains("nests"), "{}", e.msg);
    }
    // Ordinary depths are untouched.
    let plain = run_dialect(Lang::J, "> > > < < < 5", &Dialect::default());
    assert_eq!(plain, Some(Array::scalar_i64(5)));
}
