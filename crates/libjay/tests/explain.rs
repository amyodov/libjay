//! `Program::explain`: the structure a compiled expression became, and what
//! an instrumented run saw at every node.

use jay::{compile, Array, Data, Dialect, Lang};

fn explain(src: &str) -> String {
    compile(Lang::J, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile failed:\n{}", e.render(src)))
        .explain(None)
}

fn explain_with(src: &str, args: &[Array]) -> String {
    compile(Lang::J, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile failed:\n{}", e.render(src)))
        .explain(Some(args))
}

fn f64s(shape: &[usize], values: &[f64]) -> Array {
    Array::new(shape.to_vec(), Data::F64(values.to_vec().into()))
}

fn assert_has(text: &str, needle: &str) {
    assert!(text.contains(needle), "expected {needle:?} in:\n{text}");
}

#[test]
fn a_train_is_shown_as_the_fork_it_became() {
    let text = explain("(+/ % #) 1 2 3 4");
    assert_has(&text, "sentence 1");
    assert_has(&text, "monad (+/ % #)");
    assert_has(&text, "fork");
    assert_has(&text, "reduce");
    assert_has(&text, "+ primitive  ranks 0 0 0");
    assert_has(&text, "% primitive  ranks 0 0 0");
    assert_has(&text, "# primitive  ranks _ 1 _");
}

#[test]
fn a_rank_conjunction_shows_its_ranks() {
    let text = explain("+/\"1 i. 2 3");
    assert_has(&text, "rank \"1 1 1");
    assert_has(&text, "monad +/\"1");
}

#[test]
fn a_named_verb_is_a_section_of_its_own_and_runs_nothing() {
    let text = explain("mean =. +/ % #\nmean 1 2 3 4");
    assert_has(&text, "verb definition mean = (+/ % #)");
    assert_has(&text, "no runtime work");
    // The name is gone from the sentence that uses it: what is left is the
    // verb it stood for.
    assert_has(&text, "monad (+/ % #)");
}

#[test]
fn a_fused_chain_is_marked_with_what_its_kernel_holds() {
    let text = explain("+/ 1.5 2.5 * 3.5 4.5");
    assert_has(&text, "fused kernel (");
    assert_has(&text, "1 op: *");
    assert_has(&text, "+/ absorbed");
    assert_has(&text, "block 8192");
    assert_has(&text, "falls back to:");
    assert_has(&text, "[kernel ran]");
}

#[test]
fn a_window_stage_shows_the_window_it_folds() {
    let text = explain("(3 }. 1.0 2 3 4 5 6) - (4 +/\\ 1.0 2 3 4 5 6) % 4");
    assert_has(&text, "fused kernel (");
    assert_has(&text, "4 +/\\");
    assert_has(&text, "window 4");
    assert_has(&text, "[kernel ran]");
}

#[test]
fn a_running_fold_shows_as_a_stage_of_its_own() {
    let text = explain("1 + +/\\ 2 * 1.0 2 3 4");
    assert_has(&text, "+/\\");
    assert_has(&text, "1 running fold");
}

#[test]
fn a_program_the_pass_rewrote_names_what_it_inlined() {
    let text = explain("m =. +/ 1.0 2 3\nd =. 1.0 2 3 - m\n+/ d * d");
    assert_has(&text, "inlined into kernels: d");
    assert_has(&text, "errors guarded by tally");
    assert_has(&text, "the fusion pass rewrote the sentences below");
    assert_has(&text, "tally only");
    assert_has(&text, "let slot");
}

#[test]
fn shapes_and_dtypes_are_recorded_end_to_end() {
    let text = explain("+/\"1 i. 2 3");
    assert_has(&text, "→ 2 $ integer");
    assert_has(&text, "→ 2 3 $ integer");
    let text = explain("(+/ % #) 1 2 3 4");
    assert_has(&text, "→ scalar float");
    assert_has(&text, "→ 4 $ integer");
}

#[test]
fn parameters_are_shown_by_name_and_filled_from_the_arguments() {
    let src = "+/ {w} * {x}";
    let structure = explain(src);
    assert_has(&structure, "parameters: w, x");
    assert_has(&structure, "{w}");
    assert_has(&structure, "structure only");
    // The same program with values annotates every node it reached.
    let text = explain_with(src, &[f64s(&[3], &[1.0, 2.0, 3.0]), f64s(&[3], &[4.0, 5.0, 6.0])]);
    assert!(!text.contains("structure only"), "{text}");
    assert_has(&text, "{w}  → 3 $ float");
    assert_has(&text, "→ scalar float");
}

#[test]
fn a_kernel_that_declines_says_why() {
    // A chain over one scalar has no block to run: the chain runs instead.
    let text = explain("+/ 2.5 * 3.5");
    assert_has(&text, "[kernel declined:");
    assert_has(&text, "scalar");
}

#[test]
fn an_error_stops_the_annotations_and_is_reported() {
    let text = explain("1 2 + 1 2 3");
    assert_has(&text, "the run stopped here:");
}

#[test]
fn apl_explains_through_the_same_shape() {
    let program = compile(Lang::Apl, "+/2 3⍴⍳6", &Dialect::default()).expect("compile");
    let text = program.explain(None);
    assert_has(&text, "sentence 1");
    assert_has(&text, "reduce");
    assert_has(&text, "→ 2 $ integer");
}
