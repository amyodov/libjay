//! Explicit definitions, control structures and scope, end to end.
//!
//! The corpus files carry what the references answer (tests/corpus/j and
//! tests/corpus/apl, replayed by oracle.rs and oracle_apl.rs). This file
//! carries the rest: the diagnostics, the scope rules whose evidence is
//! several sentences apart, the recursion guard, `explain`'s rendering, and
//! the features neither reference has an answer for — J's `$:` inside an
//! explicit definition, APL's dfn guards, `∇` self-reference and control
//! structures. Where the published specification and the reference part
//! company, docs/coverage.md says so.

use jay::{compile, Array, Dialect, ErrorKind, Lang};
use rstest::rstest;

fn run(lang: Lang, src: &str) -> Option<Array> {
    let program = compile(lang, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile failed:\n{}", e.render(src)));
    let mut out = String::new();
    program
        .run(&[], &mut |s| out.push_str(s))
        .unwrap_or_else(|e| panic!("run failed:\n{}", program.render_error(&e)))
}

fn ints(lang: Lang, src: &str) -> Vec<i64> {
    let a = run(lang, src).unwrap_or_else(|| panic!("{src:?} yielded no value"));
    a.to_i64_vec().unwrap_or_else(|| panic!("{src:?} is not integral: {a:?}"))
}

fn j(src: &str) -> Vec<i64> {
    ints(Lang::J, src)
}

fn apl(src: &str) -> Vec<i64> {
    ints(Lang::Apl, src)
}

/// The error a program raises, at compile time or at run time.
fn fails(lang: Lang, src: &str) -> jay::Error {
    match compile(lang, src, &Dialect::default()) {
        Err(e) => e,
        Ok(p) => match p.run(&[], &mut |_: &str| {}) {
            Err(e) => e,
            Ok(v) => panic!("expected {src:?} to fail, got {v:?}"),
        },
    }
}

// --- J: definitions -------------------------------------------------------

#[rstest]
#[case("f =. 3 : 'y + 1'\nf 5", 6)]
#[case("f =. 4 : 'x * y'\n6 f 7", 42)]
#[case("f =. {{ y + 1 }}\nf 5", 6)]
#[case("f =. {{ x * y }}\n6 f 7", 42)]
#[case("f =. 3 : 0\nt =. y * 2\nt + 1\n)\nf 5", 11)]
fn a_j_definition_applies_like_any_verb(#[case] src: &str, #[case] want: i64) {
    assert_eq!(j(src), vec![want]);
}

#[test]
fn a_j_definition_of_one_valence_refuses_the_other() {
    let e = fails(Lang::J, "f =. 3 : 'y'\n2 f 3");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("no dyadic"), "{}", e.msg);
    assert!(e.span.is_some());
}

#[test]
fn a_j_definition_can_call_itself_by_name() {
    assert_eq!(j("fac =. 3 : 'if. y <: 1 do. 1 else. y * fac y - 1 end.'\nfac 6"), vec![720]);
}

/// `$:` names the definition it stands in. The J this repository tests
/// against raises a stack error here — it reads `$:` as the largest verb in
/// the SENTENCE, which is `$:` itself — so this rule comes from the
/// published dictionary rather than from the oracle, and the corpus file
/// leaves `$:` out. docs/coverage.md records the divergence.
#[test]
fn self_reference_names_the_definition_it_stands_in() {
    assert_eq!(j("f =. 3 : 'if. y <: 1 do. 1 else. y * $: y - 1 end.'\nf 6"), vec![720]);
    assert_eq!(j("f =. 4 : 'if. x <: 0 do. y else. (x - 1) $: y + 1 end.'\n3 f 0"), vec![3]);
    // The innermost definition running is the one `$:` names, so the inner
    // definition here recurses on itself and the outer one never sees it.
    assert_eq!(
        j("f =. {{ ({{ if. y <: 0 do. 99 else. $: y - 1 end. }}) y }}\nf 3"),
        vec![99]
    );
}

#[test]
fn self_reference_outside_a_definition_names_the_tacit_gap() {
    // The reference makes `$:` name the largest verb containing it, which
    // a tacit verb has; libjay resolves it against the definition it
    // stands in and has none here. That is a queue position, not a
    // program error, and the diagnostic says so.
    let e = fails(Lang::J, "$: 5");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("self-reference"), "{}", e.msg);
}

#[test]
fn runaway_recursion_is_stopped_with_a_diagnostic() {
    let e = fails(Lang::J, "f =. 3 : '$: y'\nf 1");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("deep"), "{}", e.msg);
    assert!(e.notes.iter().any(|n| n.contains("stops")), "{:?}", e.notes);
}

#[rstest]
#[case("f =. 13 : 0\ny\n", "no closing `)`")]
#[case("f =. 3 : 0\ny\n", "no closing `)`")]
#[case("f =. {{ y", "no closing `}}`")]
fn definition_forms_libjay_has_not_are_named(#[case] src: &str, #[case] msg: &str) {
    let e = fails(Lang::J, src);
    assert!(e.msg.contains(msg), "{}: {}", src, e.msg);
    assert!(e.span.is_some(), "{src}: no span");
}

// --- J: control words -----------------------------------------------------

#[rstest]
#[case("if. 1 do. 2 end.")]
#[case("while. 0 do. 1 end.")]
#[case("return.")]
#[case("throw.")]
#[case("catcht.")]
#[case("goto_end.")]
#[case("label_end.")]
fn a_control_word_outside_a_definition_is_refused(#[case] src: &str) {
    let e = fails(Lang::J, src);
    assert_eq!(e.kind, ErrorKind::Parse);
    assert!(e.msg.contains("inside an explicit definition"), "{}", e.msg);
}

#[rstest]
#[case("f =. 3 : 'if. 1 do. 2'", "end.")]
#[case("f =. 3 : 'if. 1 do. 2 end. end.'", "no matching opening word")]
#[case("f =. 3 : 'while. 1 do. 2'", "end.")]
#[case("f =. 3 : 'if. 1 2'", "do.")]
fn an_unbalanced_control_word_is_reported(#[case] src: &str, #[case] msg: &str) {
    let e = fails(Lang::J, src);
    assert_eq!(e.kind, ErrorKind::Parse);
    assert!(e.msg.contains(msg), "{src}: {}", e.msg);
}

/// `catcht.` is one of `try.`'s two rescue blocks and closes nothing on its
/// own.
#[test]
fn a_catcht_with_no_try_has_no_opening_word() {
    let e = fails(Lang::J, "f =. 3 : 0\ncatcht.\n0\nend.\n)\nf 1");
    assert_eq!(e.kind, ErrorKind::Parse, "{}", e.msg);
}

/// `try.` answers for the languages' errors. A gap in libjay is a promise,
/// not an error the program can handle, so it goes straight through.
#[test]
fn try_does_not_swallow_a_not_yet() {
    let e = fails(Lang::J, "f =. 3 : 0\ntry.\n2 s: y\ncatch.\n0\nend.\n)\nf 'a b'");
    assert_eq!(e.kind, ErrorKind::NotYet);
}

// --- J: scope -------------------------------------------------------------

#[test]
fn a_local_name_does_not_leave_the_definition() {
    let e = fails(Lang::J, "f =. 3 : 0\nloc =. y + 1\n)\nz =. f 5\nloc");
    assert_eq!(e.kind, ErrorKind::Value);
    assert!(e.msg.contains("loc"), "{}", e.msg);
}

#[test]
fn a_global_assigned_inside_a_definition_survives_it() {
    assert_eq!(j("f =. 3 : 0\ngv =: y + 2\ny\n)\nz =. f 5\ngv"), vec![7]);
}

#[test]
fn a_local_shadows_a_global_without_changing_it() {
    assert_eq!(j("g =: 7\nh =. 3 : 0\ng =. 1\ng\n)\n(h 0), g"), vec![1, 7]);
}

#[test]
fn a_global_is_visible_inside_a_definition() {
    assert_eq!(j("g =: 100\nf =. 3 : 'g + y'\nf 1"), vec![101]);
}

#[test]
fn each_call_gets_its_own_frame() {
    // The inner call's `t` must not be the outer call's.
    assert_eq!(
        j("f =. 3 : 0\nt =. y\nif. y > 0 do. z =. f y - 1 end.\nt\n)\nf 2"),
        vec![2]
    );
}

// --- J: numeric literal forms --------------------------------------------

#[rstest]
#[case("16b1f", 31)]
#[case("2b101", 5)]
#[case("_16b11", -15)]
#[case("16b_1", -1)]
fn base_literals(#[case] src: &str, #[case] want: i64) {
    assert_eq!(j(src), vec![want]);
}

#[rstest]
#[case("1p1", std::f64::consts::PI)]
#[case("2p1", 2.0 * std::f64::consts::PI)]
#[case("1x1", std::f64::consts::E)]
#[case("2.5b10", 2.5)]
fn multiples_of_pi_and_e(#[case] src: &str, #[case] want: f64) {
    let got = run(Lang::J, src).expect("a value").to_f64_vec().expect("numeric");
    assert!((got[0] - want).abs() < 1e-12, "{src}: {got:?} vs {want}");
}

/// `1x1` is a multiple of e and `1x` an extended integer: the suffix and
/// the constant share a letter, and the digits after it tell them apart.
#[test]
fn the_extended_suffix_and_the_e_constant_share_a_letter() {
    let got = run(Lang::J, "1x").expect("a value");
    assert_eq!(got.dtype(), jay::DType::Ext);
    let got = run(Lang::J, "1x1").expect("a value").to_f64_vec().expect("numeric");
    assert!((got[0] - std::f64::consts::E).abs() < 1e-12);
}

// --- APL: dfns ------------------------------------------------------------

#[rstest]
#[case("{⍵×2} 21", 42)]
#[case("2 {⍺+⍵} 3", 5)]
#[case("F←{⍵×2} ⋄ F 21", 42)]
#[case("F←{a←⍵×2 ⋄ a+1} ⋄ F 5", 11)]
fn an_apl_dfn_applies_like_any_function(#[case] src: &str, #[case] want: i64) {
    assert_eq!(apl(src), vec![want]);
}

/// GNU APL has no dfn guards; the rule is the published dfn model's, so it
/// is checked here rather than against the reference.
#[rstest]
#[case("F←{⍵>3:99 ⋄ 7} ⋄ F 5", 99)]
#[case("F←{⍵>3:99 ⋄ 7} ⋄ F 1", 7)]
#[case("F←{⍵>3:99 ⋄ ⍵<0:¯1 ⋄ 7} ⋄ F ¯5", -1)]
fn a_guard_that_holds_is_the_answer(#[case] src: &str, #[case] want: i64) {
    assert_eq!(apl(src), vec![want]);
}

/// `∇` inside a dfn names the dfn. GNU APL rejects it, so this too comes
/// from the published model.
#[test]
fn del_inside_a_dfn_is_a_self_reference() {
    assert_eq!(apl("F←{⍵≤1:1 ⋄ ⍵×∇⍵-1} ⋄ F 5"), vec![120]);
}

/// `⍺←v` is a default: a left argument that arrived keeps its value. GNU
/// APL assigns unconditionally; the divergence is recorded in the corpus.
#[test]
fn a_dfn_default_left_argument_does_not_overwrite_one_that_arrived() {
    assert_eq!(apl("F←{⍺←10 ⋄ ⍺+⍵} ⋄ (F 5),(3 F 5)"), vec![15, 8]);
}

#[test]
fn a_dfn_that_produces_no_result_says_so() {
    let e = fails(Lang::Apl, "F←{⍵>3:99} ⋄ F 1");
    assert_eq!(e.kind, ErrorKind::Value);
    assert!(e.msg.contains("no result"), "{}", e.msg);
}

/// A dfn is ambivalent whatever its body mentions: a left argument it has
/// no name for is dropped, which is the recorded Dyalog answer. A `∇`
/// definition binds its arguments by name and refuses the spare one.
#[test]
fn a_dfn_drops_a_left_argument_it_has_no_name_for() {
    assert_eq!(apl("F←{⍵} ⋄ 1 F 2"), vec![2]);
    assert_eq!(apl("3 {⍵×2} 5"), vec![10]);
    let e = fails(Lang::Apl, "∇Z←F Y\nZ←Y\n∇\n1 F 2");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("no dyadic"), "{}", e.msg);
}

#[test]
fn a_dfn_that_mentions_the_operand_names_is_an_operator() {
    // `⍺⍺` and `⍵⍵` are the operands; the dfn is applied to functions.
    assert_eq!(apl("+{⍺⍺/⍵}1 2 3"), vec![6]);
    assert_eq!(apl("×{⍺⍺/⍵}1 2 3 4"), vec![24]);
    // A named operator is still an operator in the sentences that follow.
    assert_eq!(apl("TWICE←{⍺⍺ ⍺⍺ ⍵} ⋄ -TWICE 5"), vec![5]);
    assert_eq!(apl("BOTH←{(⍺⍺ ⍵),⍵⍵ ⍵} ⋄ -BOTH+ 3"), vec![-3, 3]);
    // Dyalog lets an ARRAY stand where an operand belongs, and the body
    // reads the name as that array rather than as a function.
    assert_eq!(apl("2{⍺⍺+⍵}3"), vec![5]);
    assert_eq!(apl("1 2 3{⍺⍺+⍵}10"), vec![11, 12, 13]);
    assert_eq!(apl("BOTHARR←{⍺⍺,⍵⍵,⍵} ⋄ (1 BOTHARR 2) 3"), vec![1, 2, 3]);
    // The operand binds tighter than the argument, so the array to the
    // right of the operator is the operand and not the argument.
    assert_eq!(apl("2{⍺⍺+⍵}¨1 2 3"), vec![3, 4, 5]);
    // An operator that asks for a right operand and is given none says so.
    let e = fails(Lang::Apl, "-{⍺⍺ ⍵⍵ ⍵}");
    assert!(e.msg.contains("⍵⍵ needs an operand"), "{}", e.msg);
}

#[test]
fn a_branch_outside_a_definition_has_nowhere_to_go() {
    let e = fails(Lang::Apl, "→3");
    assert_eq!(e.kind, ErrorKind::Parse);
    assert!(e.msg.contains("∇ definition"), "{}", e.msg);
}

#[test]
fn a_branch_moves_to_the_line_a_label_names() {
    // A loop written the way APL wrote loops before control structures.
    assert_eq!(
        apl("∇Z←F N\nZ←0\nL1:\n→(N=0)/END\nZ←Z+N\nN←N-1\n→L1\nEND:\n∇\nF 5"),
        vec![15]
    );
    // `→0` leaves the definition, and so does any other line it has not.
    assert_eq!(apl("∇Z←G N\n→(N>3)/BIG\nZ←1\n→0\nBIG:\nZ←2\n∇\nG 5"), vec![2]);
    assert_eq!(apl("∇Z←G N\n→(N>3)/BIG\nZ←1\n→0\nBIG:\nZ←2\n∇\nG 1"), vec![1]);
    // An empty target falls through to the next line.
    assert_eq!(apl("∇Z←K N\nZ←1\n→⍬\nZ←2\n∇\nK 0"), vec![2]);
    // A label and a control structure stand in one definition: the label
    // is still the number of its LINE, and a `→` finds the statement that
    // line began.
    assert_eq!(apl("∇Z←F R\nL:\n:If R\nZ←1\n:EndIf\n∇\nF 1"), vec![1]);
    assert_eq!(
        apl("∇Z←F N\nZ←0\nL:→(N<1)/OUT\n:If 2|N\nZ←Z+N\n:EndIf\nN←N-1\n→L\nOUT:Z←Z×10\n∇\nF 5"),
        vec![90]
    );
    // A label's value is its line number whatever stands between.
    assert_eq!(apl("∇Z←F R\nZ←0\n:If R\nZ←1\n:EndIf\nL:Z←L\n∇\nF 1"), vec![5]);
    // Branching INTO a control structure has no statement to land on.
    let e = fails(Lang::Apl, "∇Z←M N\nZ←0\n:If 1\nA:Z←1\n:EndIf\n→A\n∇\nM 1");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("control structure"), "{}", e.msg);
}

#[test]
fn a_niladic_definition_is_called_by_naming_it() {
    assert_eq!(apl("∇Z←H\nZ←42\n∇\nH"), vec![42]);
    assert_eq!(apl("∇Z←H\nZ←42\n∇\n2×H"), vec![84]);
    assert_eq!(apl("∇Z←H;T\nT←6\nZ←T×7\n∇\nH"), vec![42]);
    assert_eq!(apl("∇Z←P\nZ←⍳4\n∇\n+/P"), vec![10]);
}

// --- APL: ∇-definitions and control structures ---------------------------

#[rstest]
#[case("∇Z←F R\nZ←R×2\n∇\nF 21", 42)]
#[case("∇Z←L G R\nZ←L+R\n∇\n2 G 3", 5)]
#[case("∇Z←H R\n:If R>3\nZ←99\n:Else\nZ←7\n:EndIf\n∇\nH 5", 99)]
#[case("∇Z←H R\n:If R>3\nZ←99\n:ElseIf R>1\nZ←5\n:Else\nZ←7\n:EndIf\n∇\nH 2", 5)]
#[case("∇Z←W R;I\nZ←0\nI←0\n:While I<R\nZ←Z+I\nI←I+1\n:EndWhile\n∇\nW 5", 10)]
#[case("∇Z←R2 R;I\nI←0\nZ←0\n:Repeat\nI←I+1\nZ←Z+I\n:Until I≥R\n∇\nR2 4", 10)]
#[case("∇Z←FR R;I\nZ←0\n:For I :In R\nZ←Z+I\n:EndFor\n∇\nFR 1 2 3 4", 10)]
#[case("∇Z←S R\n:Select R\n:Case 1\nZ←10\n:Case 2\nZ←20\n:Else\nZ←0\n:EndSelect\n∇\nS 2", 20)]
#[case("∇Z←FC R\n:If R≤1\nZ←1\n:Return\n:EndIf\nZ←R×FC R-1\n∇\nFC 5", 120)]
#[case("∇Z←LV R;I\nZ←0\n:For I :In R\n:If I=3\n:Leave\n:EndIf\nZ←Z+I\n:EndFor\n∇\nLV 1 2 3 4", 3)]
#[case("∇Z←CN R;I\nZ←0\n:For I :In R\n:If I=2\n:Continue\n:EndIf\nZ←Z+I\n:EndFor\n∇\nCN 1 2 3", 4)]
#[case("∇Z←E R\n:If R>0\nZ←1\n:End\n∇\nE 1", 1)]
fn apl_control_structures(#[case] src: &str, #[case] want: i64) {
    assert_eq!(apl(src), vec![want]);
}

#[test]
fn a_del_definition_names_its_result() {
    let e = fails(Lang::Apl, "∇Z←F R\nR×2\n∇\nF 5");
    assert_eq!(e.kind, ErrorKind::Value);
    assert!(e.msg.contains("did not set its result"), "{}", e.msg);
}

#[test]
fn a_del_definition_can_call_itself_by_name() {
    assert_eq!(apl("∇Z←FC R\n:If R≤1\nZ←1\n:Return\n:EndIf\nZ←R×FC R-1\n∇\nFC 6"), vec![720]);
}

/// APL's tradfn scope rule: a name the header does not declare is global.
#[test]
fn a_del_definition_writes_globals_unless_the_header_declares_them() {
    assert_eq!(apl("X←1\n∇Z←F R\nX←5\nZ←R\n∇\nQ←F 9\nX"), vec![5]);
    assert_eq!(apl("Y←1\n∇Z←G R;Y\nY←5\nZ←R\n∇\nQ←G 9\nY"), vec![1]);
}

#[rstest]
#[case(":If 1", ":EndIf")]
#[case(":Else ⋄ 5", "no matching opening word")]
#[case("∇Z←F R\n:If R\nZ←1\n∇\nF 1", ":EndIf")]
#[case("∇Z←F R\nZ←1\n:EndIf\n∇\nF 1", "no matching opening word")]
#[case("∇Z←F R\nZ←1", "no closing ∇")]
fn apl_definition_diagnostics(#[case] src: &str, #[case] msg: &str) {
    let e = fails(Lang::Apl, src);
    assert!(e.msg.contains(msg), "{src}: {}", e.msg);
}

// --- APL: ⎕FX ------------------------------------------------------------

/// `⎕FX` fixes the same function `∇ … ∇` does, from the same lines.
#[rstest]
#[case("N←⎕FX 'Z←F R' 'Z←R×2' ⋄ F 21", 42)]
#[case("N←⎕FX 'Z←L G R' 'Z←L+R' ⋄ 2 G 3", 5)]
#[case("N←⎕FX 'Z←F R;T' 'T←R×2' 'Z←T+1' ⋄ F 5", 11)]
#[case("N←⎕FX 'Z←H' 'Z←42' ⋄ H", 42)]
#[case("N←⎕FX 'Z←F R' ':If R>0' 'Z←1' ':Else' 'Z←0' ':EndIf' ⋄ F 5", 1)]
#[case("N←⎕FX 'Z←F R' ':If R>0' 'Z←1' ':Else' 'Z←0' ':EndIf' ⋄ F ¯5", 0)]
#[case("N←⎕FX 'Z←F R' 'Z←1' ':While R>1' 'Z←Z×R' 'R←R-1' ':EndWhile' ⋄ F 5", 120)]
#[case("N←⎕FX 'Z←F R' 'Z←0' ':For X :In R' 'Z←Z+X' ':EndFor' ⋄ F 1 2 3 4", 10)]
#[case("N←⎕FX 'Z←F R' ':Select R' ':Case 1' 'Z←10' ':Else' 'Z←0' ':EndSelect' ⋄ F 1", 10)]
#[case("N←⎕FX 'Z←F N' 'Z←0' 'L1:' '→(N=0)/E' 'Z←Z+N' 'N←N-1' '→L1' 'E:' ⋄ F 5", 15)]
#[case("N←⎕FX 'Z←G R' 'Z←R×3' ⋄ M←⎕FX 'Z←F R' 'Z←G R' ⋄ F 5", 15)]
// A body may call a function the program fixes AFTER it: the name stands
// for a verb resolved when the line runs, and by then it has one.
#[case("N←⎕FX 'Z←F R' 'Z←G R' ⋄ M←⎕FX 'Z←G R' 'Z←R×3' ⋄ F 5", 15)]
// `:AndIf` and `:OrIf` continue the condition above them, and stop as soon
// as it is settled: a second test that would fail never runs.
#[case("N←⎕FX 'Z←F R' ':If R>0' ':AndIf R<10' 'Z←1' ':Else' 'Z←0' ':EndIf' ⋄ F 5", 1)]
#[case("N←⎕FX 'Z←F R' ':If R>0' ':AndIf R<10' 'Z←1' ':Else' 'Z←0' ':EndIf' ⋄ F 50", 0)]
#[case("N←⎕FX 'Z←F R' ':If R>0' ':AndIf R<0' 'Z←1' ':Else' 'Z←0' ':EndIf' ⋄ F 5", 0)]
#[case("N←⎕FX 'Z←F R' ':If R<0' ':OrIf R>10' 'Z←1' ':Else' 'Z←0' ':EndIf' ⋄ F 50", 1)]
#[case("N←⎕FX 'Z←F R' ':If 0' ':OrIf 0' ':OrIf 1' 'Z←1' ':Else' 'Z←0' ':EndIf' ⋄ F 0", 1)]
#[case("N←⎕FX 'Z←F R' 'Z←0' ':While R>0' ':AndIf Z<3' 'Z←Z+1' 'R←R-1' ':EndWhile' ⋄ F 10", 3)]
// `:CaseList` takes any one of its items.
#[case("N←⎕FX 'Z←F R' ':Select R' ':CaseList 1 2 3' 'Z←1' ':Else' 'Z←0' ':EndSelect' ⋄ F 2", 1)]
#[case("N←⎕FX 'Z←F R' ':Select R' ':CaseList 1 2 3' 'Z←1' ':Else' 'Z←0' ':EndSelect' ⋄ F 9", 0)]
// `:For` binds the item's CONTENTS, and several names take it apart.
#[case("N←⎕FX 'Z←F R' 'Z←0' ':For P :In R' 'Z←Z++/P' ':EndFor' ⋄ F (1 2)(3 4 5)", 15)]
#[case("N←⎕FX 'Z←F R' 'Z←⍬' ':For A B :In R' 'Z←Z,A×B' ':EndFor' ⋄ +/F (1 2)(3 4)", 14)]
fn quad_fx_fixes_a_definition_from_its_lines(#[case] src: &str, #[case] want: i64) {
    assert_eq!(apl(src), vec![want]);
}

/// A control structure standing on its own, outside any definition. Its
/// value is the value of the last sentence the branch it chose ran, which
/// is the block model every other sequence follows.
#[rstest]
#[case(":If 1 ⋄ 5 ⋄ :EndIf", 5)]
#[case(":If 0 ⋄ 5 ⋄ :Else ⋄ 7 ⋄ :EndIf", 7)]
#[case("T←0 ⋄ :For I :In 1 2 3 4 ⋄ T←T+I ⋄ :EndFor ⋄ T", 10)]
#[case("T←1 ⋄ N←5 ⋄ :While N>1 ⋄ T←T×N ⋄ N←N-1 ⋄ :EndWhile ⋄ T", 120)]
fn a_control_structure_stands_outside_a_definition(#[case] src: &str, #[case] want: i64) {
    assert_eq!(apl(src), vec![want]);
}

#[test]
fn quad_fx_answers_the_name_it_fixed() {
    let a = run(Lang::Apl, "⎕FX 'Z←F R' 'Z←R×2'").expect("a value");
    assert_eq!(a, Array::from_chars("F".chars().collect()));
    // The name is a value like any other, so it can be assigned away.
    assert_eq!(apl("N←⎕FX 'Z←F R' 'Z←R×2' ⋄ ≢N"), vec![1]);
}

/// A definition `⎕FX` cannot fix is reported as the fault it is, pointing
/// at the line that carries it, where Dyalog answers the offending line's
/// number instead.
#[rstest]
#[case("N←⎕FX 'Z←F R' ':If R>0' 'Z←1' ⋄ F 1", ":EndIf")]
#[case("N←⎕FX 'Z←F R' 'Z←1' ':EndWhile' ⋄ F 1", "no matching opening word")]
fn quad_fx_reports_a_definition_it_cannot_fix(#[case] src: &str, #[case] msg: &str) {
    let e = fails(Lang::Apl, src);
    assert!(e.msg.contains(msg), "{src}: {}", e.msg);
}

/// libjay fixes at compile time, so a definition it cannot read then is a
/// promise rather than a wrong answer.
#[rstest]
#[case("A←'Z←F R' ⋄ ⎕FX A ⋄ F 1")]
#[case("N←⎕FX 'Z←G R' 'Z←⎕FX R' ⋄ G 'Z←F R'")]
fn quad_fx_on_a_definition_it_cannot_read_names_the_gap(#[case] src: &str) {
    let e = fails(Lang::Apl, src);
    assert_eq!(e.kind, ErrorKind::NotYet, "{src}: {}", e.msg);
    assert!(e.msg.contains("⎕FX"), "{src}: {}", e.msg);
}

// --- APL: indexed assignment ---------------------------------------------

#[rstest]
#[case("A←1 2 3 4 ⋄ A[2]←99 ⋄ A", vec![1, 99, 3, 4])]
#[case("A←1 2 3 4 ⋄ A[1 3]←0 ⋄ A", vec![0, 2, 0, 4])]
#[case("A←1 2 3 4 ⋄ A[1 3]←7 8 ⋄ A", vec![7, 2, 8, 4])]
#[case("A←3 3⍴⍳9 ⋄ A[2;3]←0 ⋄ A", vec![1, 2, 3, 4, 5, 0, 7, 8, 9])]
#[case("A←3 3⍴⍳9 ⋄ A[1;]←0 ⋄ A", vec![0, 0, 0, 4, 5, 6, 7, 8, 9])]
#[case("A←3 3⍴⍳9 ⋄ A[;1]←0 ⋄ A", vec![0, 2, 3, 0, 5, 6, 0, 8, 9])]
fn indexed_assignment_replaces_what_the_brackets_select(
    #[case] src: &str,
    #[case] want: Vec<i64>,
) {
    assert_eq!(apl(src), want);
}

#[test]
fn indexed_assignment_leaves_the_value_it_copied_alone() {
    // `B` was taken from `A` before the write, so it keeps the old items.
    assert_eq!(apl("A←1 2 3 ⋄ B←A ⋄ A[2]←99 ⋄ B"), vec![1, 2, 3]);
}

#[test]
fn indexed_assignment_widens_rather_than_truncates() {
    let a = run(Lang::Apl, "A←1 2 3 ⋄ A[2]←1.5 ⋄ A").expect("a value");
    assert_eq!(a.to_f64_vec().expect("numeric"), vec![1.0, 1.5, 3.0]);
}

#[rstest]
#[case("A←1 2 3 ⋄ A[9]←0 ⋄ A", ErrorKind::Domain, "outside axis")]
#[case("A←1 2 3 ⋄ A[1 2]←7 8 9 ⋄ A", ErrorKind::Shape, "needs a scalar")]
#[case("A←3 3⍴⍳9 ⋄ A[2]←0 ⋄ A", ErrorKind::Rank, "one index per axis")]
fn indexed_assignment_diagnostics(
    #[case] src: &str,
    #[case] kind: ErrorKind,
    #[case] msg: &str,
) {
    let e = fails(Lang::Apl, src);
    assert_eq!(e.kind, kind, "{src}: {}", e.msg);
    assert!(e.msg.contains(msg), "{src}: {}", e.msg);
}

// --- explain --------------------------------------------------------------

fn explain(lang: Lang, src: &str) -> String {
    compile(lang, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile failed:\n{}", e.render(src)))
        .explain(None)
}

fn assert_has(text: &str, needle: &str) {
    assert!(text.contains(needle), "expected {needle:?} in:\n{text}");
}

#[test]
fn explain_shows_a_definition_and_its_control_structure() {
    let text = explain(Lang::J, "f =. 3 : 'if. y > 1 do. y else. 1 end.'\nf 5");
    assert_has(&text, "verb definition f");
    assert_has(&text, "explicit definition 3 : '...'");
    assert_has(&text, "arguments y; body of 1 sentence(s)");
    assert_has(&text, "monad 3 : '...'");
}

#[rstest]
#[case("f =. 3 : 0\nif. y do. 1 else. 2 end.\n)\nf 1", "if — 1 arm(s)")]
#[case("f =. 3 : 0\nwhile. y do. 1 end.\n)\nf 0", "while")]
#[case("f =. 3 : 0\nwhilst. y do. 1 end.\n)\nf 0", "while, body first")]
#[case("f =. 3 : 0\nfor_i. y do. i end.\n)\nf 1 2", "for over items, item in i, index in i_index")]
#[case("f =. 3 : 0\nselect. y\ncase. 1 do. 2\nend.\n)\nf 1", "select — 1 case(s), matched with -:")]
#[case("f =. 3 : 0\ntry.\ny\ncatch.\n0\nend.\n)\nf 1", "try")]
#[case("f =. 3 : 0\nreturn.\n)\nf 1", "return")]
fn explain_names_every_control_node(#[case] src: &str, #[case] needle: &str) {
    // A definition's body is shown inside the definition that holds it.
    assert_has(&explain(Lang::J, src), needle);
}

#[test]
fn explain_shows_an_indexed_assignment() {
    let text = explain(Lang::Apl, "A←1 2 3 ⋄ A[2]←9 ⋄ A");
    assert_has(&text, "amend A[i]");
    assert_has(&text, "value:");
}

// --- dfn scope, guards and function names ---------------------------------

/// A dfn written inside another reads the names the enclosing one made
/// local, and its own assignments stay its own. Every answer here is the
/// recorded Dyalog one.
#[rstest]
#[case("F←{a←10 ⋄ {a+⍵} ⍵} ⋄ F 5", vec![15])]
#[case("F←{a←⍵ ⋄ {b←⍵ ⋄ a+b} 100} ⋄ F 1", vec![101])]
#[case("F←{n←⍵ ⋄ +/{n×⍵}¨1 2 3} ⋄ F 10", vec![60])]
#[case("F←{a←10 ⋄ G←{a+⍵} ⋄ G ⍵} ⋄ F 5", vec![15])]
#[case("F←{a←10 ⋄ G←{a×⍵} ⋄ (G 2),(G 3)} ⋄ F 0", vec![20, 30])]
#[case("F←{a←1 ⋄ G←{a←2 ⋄ a} ⋄ (G 0),a} ⋄ F 0", vec![2, 1])]
fn a_nested_dfn_reads_the_enclosing_ones_locals(
    #[case] src: &str,
    #[case] want: Vec<i64>,
) {
    assert_eq!(apl(src), want);
}

/// Only a LEXICAL parent's locals are reachable: a dfn named elsewhere and
/// called from inside one sees the globals and nothing of its caller.
#[test]
fn a_called_dfn_does_not_see_its_callers_locals() {
    let e = fails(Lang::Apl, "G←{a+⍵} ⋄ F←{a←10 ⋄ G ⍵} ⋄ F 5");
    assert!(e.msg.contains("undefined name: a"), "{}", e.msg);
    assert_eq!(apl("a←1 ⋄ G←{a+⍵} ⋄ F←{a←10 ⋄ G ⍵} ⋄ F 5"), vec![6]);
}

/// A function named inside a dfn is a function to the sentences after it,
/// and the name does not escape into the program around it.
#[test]
fn a_dfn_may_name_a_function_of_its_own() {
    assert_eq!(apl("F←{G←{⍵×2} ⋄ G ⍵} ⋄ F 5"), vec![10]);
    let e = fails(Lang::Apl, "F←{G←{⍵×2} ⋄ G ⍵} ⋄ G 5");
    assert!(e.msg.contains('G'), "{}", e.msg);
}

/// A dfn has no control words, so `:` inside one always opens a guard —
/// whatever the answer beside it is spelled like.
#[test]
fn a_guard_reads_its_colon_whatever_follows_it() {
    assert_eq!(apl("F←{a←⍵×2 ⋄ a>10:a ⋄ a+100} ⋄ F 6"), vec![12]);
    assert_eq!(apl("F←{a←⍵×2 ⋄ a>10:a ⋄ a+100} ⋄ F 2"), vec![104]);
}

/// A guard's condition is read strictly: one element, and that element 0
/// or 1. A control structure's `:If` is the loose reading and keeps it.
#[rstest]
#[case("{1:1 ⋄ 0} 5", Some(1))]
#[case("{0:1 ⋄ 2} 5", Some(2))]
#[case("{2:1 ⋄ 0} 5", None)]
#[case("{⍵:1 ⋄ 0} 2", None)]
#[case("{1 1:1 ⋄ 0} 5", None)]
#[case("{⍬:1 ⋄ 0} 5", None)]
#[case("{'x':1 ⋄ 0} 5", None)]
fn a_guard_wants_one_zero_or_one(#[case] src: &str, #[case] want: Option<i64>) {
    match want {
        Some(v) => assert_eq!(apl(src), vec![v]),
        None => {
            let e = fails(Lang::Apl, src);
            assert_eq!(e.kind, ErrorKind::Domain, "{src}: {}", e.msg);
            assert!(e.msg.contains("single 0 or 1"), "{src}: {}", e.msg);
        }
    }
}

// --- ⎕← and ⍞← inside a definition ----------------------------------------

/// Run an APL sentence under a dialect, keeping what it printed.
fn printing(dialect: Dialect, src: &str) -> (Vec<i64>, String) {
    let program = compile(Lang::Apl, src, &dialect)
        .unwrap_or_else(|e| panic!("compile failed:\n{}", e.render(src)));
    let mut out = String::new();
    let value = program
        .run(&[], &mut |s| out.push_str(s))
        .unwrap_or_else(|e| panic!("run failed:\n{}", program.render_error(&e)));
    let value = value.unwrap_or_else(|| panic!("{src:?} yielded no value"));
    let ints = value.to_i64_vec().unwrap_or_else(|| panic!("{src:?} is not integral"));
    (ints, out)
}

/// `⎕←` is an assignment — to `⎕` — so the sentences after one still run,
/// in either reading of what a dfn's result is. Under the Dyalog reading,
/// where the result is the first sentence that is NOT an assignment, a
/// body that prints and then computes answers what it computed.
#[rstest]
#[case("{⎕←⍵ ⋄ ⍵+1} 5", vec![6], "5\n")]
#[case("{⍞←⍵ ⋄ ⍵+1} 5", vec![6], "5")]
#[case("{⎕←⍵ ⋄ ⎕←⍵+1 ⋄ ⍵+2} 5", vec![7], "5\n6\n")]
#[case("{⎕←⍵ ⋄ ⍵>3: ⍵ ⋄ 0} 5", vec![5], "5\n")]
#[case("{⍵>9: ⎕←⍵ ⋄ ⍵+1} 5", vec![6], "")]
#[case("{a←⎕←⍵ ⋄ a+1} 5", vec![6], "5\n")]
#[case("+{⎕←⍵ ⋄ ⍺⍺ ⍵+1} 5", vec![6], "5\n")]
#[case("F←{⍵<1: 0 ⋄ ⎕←⍵ ⋄ ∇⍵-1} ⋄ F 3", vec![0], "3\n2\n1\n")]
fn a_printing_sentence_does_not_end_a_dfn(
    #[case] src: &str,
    #[case] want: Vec<i64>,
    #[case] printed: &str,
) {
    for dialect in [Dialect::gnu_apl(), Dialect::dyalog()] {
        assert_eq!(printing(dialect, src), (want.clone(), printed.to_string()), "{src}");
    }
}

/// The two readings still part company where the printing sentence is not
/// the one in question: the last sentence is the answer here, the first
/// non-assignment there.
#[rstest]
#[case("{⍵+1 ⋄ ⎕←⍵} 5", vec![5], "5\n", vec![6], "")]
#[case("{⎕←⍵ ⋄ {⎕←⍵+1 ⋄ ⍵} ⍵ ⋄ ⍵+3} 5", vec![8], "5\n6\n", vec![5], "5\n6\n")]
fn the_dialects_still_disagree_about_which_sentence_answers(
    #[case] src: &str,
    #[case] gnu: Vec<i64>,
    #[case] gnu_printed: &str,
    #[case] dyalog: Vec<i64>,
    #[case] dyalog_printed: &str,
) {
    assert_eq!(printing(Dialect::gnu_apl(), src), (gnu, gnu_printed.to_string()), "{src}");
    assert_eq!(
        printing(Dialect::dyalog(), src),
        (dyalog, dyalog_printed.to_string()),
        "{src}"
    );
}
