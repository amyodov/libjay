//! Operands the program computes: an operand that is a name or an
//! expression rather than a literal, read where the derived function is
//! applied instead of while the program compiles.
//!
//! The corpora in tests/corpus/apl/computed.txt and the `^:` section of
//! tests/corpus/j/modifiers.txt carry the breadth. This file states one
//! rule per assertion: when the operand is read, what a definition's own
//! argument can decide, where the value of an indexed assignment goes, and
//! the two gaps the family keeps.

use jay::{compile, Array, Data, Dialect, Error, ErrorKind, Lang};

fn run(lang: Lang, src: &str) -> Result<Option<Array>, Error> {
    let program = compile(lang, src, &Dialect::default())?;
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink)
}

fn val(lang: Lang, src: &str) -> Array {
    run(lang, src)
        .unwrap_or_else(|e| panic!("{src:?} failed: {}", e.msg))
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn j(src: &str) -> Array {
    val(Lang::J, src)
}

fn apl(src: &str) -> Array {
    val(Lang::Apl, src)
}

fn err(lang: Lang, src: &str) -> Error {
    match run(lang, src) {
        Err(e) => e,
        Ok(v) => panic!("{src:?} was expected to fail; it answered {v:?}"),
    }
}

fn i64s(shape: &[usize], values: &[i64]) -> Array {
    Array::new(shape.to_vec(), Data::I64(values.to_vec().into()))
}

// --- a count the program computes ----------------------------------------

/// `f⍣N` and `u^:n` read the count where the derived function is applied,
/// so a name that is assigned before it holds.
#[test]
fn a_power_count_may_be_a_name() {
    assert_eq!(apl("N←3 ⋄ ⌽⍣N⊢1 2 3"), i64s(&[3], &[3, 2, 1]));
    assert_eq!(apl("N←2 ⋄ ⌽⍣N⊢1 2 3"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(apl("N←4 ⋄ 2×⍣N⊢1"), Array::scalar_i64(16));
    assert_eq!(j("n =. 3\n(>:^:n) 0"), Array::scalar_i64(3));
    assert_eq!(j("n =. 4\n+:^:n ] 3"), Array::scalar_i64(48));
}

/// A parenthesised expression is the same operand, and it is settled at
/// compile time where nothing in it reads a name.
#[test]
fn a_power_count_may_be_an_expression() {
    assert_eq!(apl("⌽⍣(1+2)⊢1 2 3"), i64s(&[3], &[3, 2, 1]));
    assert_eq!(apl("N←1 ⋄ ⌽⍣(N+2)⊢1 2 3"), i64s(&[3], &[3, 2, 1]));
    assert_eq!(j("(>:^:(1+2)) 0"), Array::scalar_i64(3));
    assert_eq!(j("n =. 1\n(+:^:(n+2)) 3"), Array::scalar_i64(24));
}

/// An operator's operand is one token's worth. `f⍣N+1` therefore reads the
/// count `N` and leaves the `+1` to the sentence, which is what GNU APL
/// does.
#[test]
fn the_count_is_one_operand_and_not_the_rest_of_the_sentence() {
    assert_eq!(apl("N←2 ⋄ ⌽⍣N+1⊢1 2 3"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(apl("N←2 ⋄ ⌽⍣(N+1)⊢1 2 3"), i64s(&[3], &[3, 2, 1]));
}

/// Read late means read at EVERY application, so a definition's own
/// argument may decide the count.
#[test]
fn a_definition_argument_may_decide_the_count() {
    let def = "∇Z←R N\nZ←⌽⍣N⊢1 2 3\n∇\n";
    assert_eq!(apl(&format!("{def}R 1")), i64s(&[3], &[3, 2, 1]));
    assert_eq!(apl(&format!("{def}R 2")), i64s(&[3], &[1, 2, 3]));
    assert_eq!(apl("F←{2×⍣⍵⊢1} ⋄ F 5"), Array::scalar_i64(32));
    assert_eq!(j("f =. 3 : '+:^:y ] 1'\nf 5"), Array::scalar_i64(32));
}

/// The whole of the count's meaning survives being read late: J's boxed
/// trace and its list of counts are read from the value, not from the
/// spelling.
#[test]
fn a_late_count_keeps_every_reading_a_literal_one_has() {
    assert_eq!(j("n =. 2 3\n>:^:n ] 0"), i64s(&[2], &[2, 3]));
    assert_eq!(j("n =. <3\n>:^:n ] 0"), i64s(&[3], &[0, 1, 2]));
    assert_eq!(j("n =. _1\n(>:^:n) 5"), Array::scalar_i64(4));
}

// --- an axis the program computes ----------------------------------------

/// `f[K]` reads the axis where the function is applied, in `⎕IO` origin,
/// and every function that takes an axis takes this one.
#[test]
fn an_axis_may_be_a_name_or_an_expression() {
    assert_eq!(apl("M←2 3⍴⍳6 ⋄ K←1 ⋄ +/[K]M"), i64s(&[3], &[5, 7, 9]));
    assert_eq!(apl("M←2 3⍴⍳6 ⋄ K←2 ⋄ +/[K]M"), i64s(&[2], &[6, 15]));
    assert_eq!(apl("M←2 3⍴⍳6 ⋄ K←2 ⋄ +/[K-1]M"), i64s(&[3], &[5, 7, 9]));
    assert_eq!(apl("M←2 3⍴⍳6 ⋄ K←1 ⋄ ⌽[K]M"), i64s(&[2, 3], &[4, 5, 6, 1, 2, 3]));
    assert_eq!(apl("M←2 3⍴⍳6 ⋄ K←1 ⋄ 1↑[K]M"), i64s(&[1, 3], &[1, 2, 3]));
    assert_eq!(apl("F←{+/[⍵]2 3⍴⍳6} ⋄ F 2"), i64s(&[2], &[6, 15]));
}

/// The axis is checked against the argument where it is read, so an axis
/// the argument does not have is a refusal at that point rather than a
/// wrong answer.
#[test]
fn a_computed_axis_is_still_held_to_the_arguments_rank() {
    let e = err(Lang::Apl, "M←2 3⍴⍳6 ⋄ K←3 ⋄ +/[K]M");
    assert_ne!(e.kind, ErrorKind::NotYet, "{}", e.msg);
    let e = err(Lang::Apl, "M←2 3⍴⍳6 ⋄ K←0 ⋄ +/[K]M");
    assert_ne!(e.kind, ErrorKind::NotYet, "{}", e.msg);
}

// --- the axis a definition binds -----------------------------------------

/// A `∇` header may name an axis. The name is a local of the definition,
/// bound to whatever the call wrote in brackets — verbatim, with no `⎕IO`
/// adjustment, because the body decides what the value means.
#[test]
fn a_header_axis_binds_a_local() {
    let sum = "∇Z←SUM[X] B\nZ←+/[X]B\n∇\n";
    assert_eq!(apl(&format!("{sum}SUM[1] 2 3⍴⍳6")), i64s(&[3], &[5, 7, 9]));
    assert_eq!(apl(&format!("{sum}SUM[2] 2 3⍴⍳6")), i64s(&[2], &[6, 15]));
    assert_eq!(apl(&format!("{sum}K←2 ⋄ SUM[K] 2 3⍴⍳6")), i64s(&[2], &[6, 15]));
    let show = "∇Z←SHOW[X] B\nZ←X\n∇\n";
    assert_eq!(apl(&format!("{show}SHOW[7] 0")), Array::scalar_i64(7));
    assert_eq!(apl(&format!("{show}SHOW[7 8] 0")), i64s(&[2], &[7, 8]));
}

/// A `{…}` reads the same axis under the name `χ`, in either valence.
#[test]
fn a_dfn_reads_its_axis_as_chi() {
    assert_eq!(apl("F←{⍵+χ} ⋄ F[10]5"), Array::scalar_i64(15));
    assert_eq!(apl("F←{⍺+⍵+χ} ⋄ 1 F[10]5"), Array::scalar_i64(16));
    assert_eq!(apl("F←{⍵,χ} ⋄ F[7 8]5"), i64s(&[3], &[5, 7, 8]));
    assert_eq!(apl("F←{⍵+χ} ⋄ K←4 ⋄ F[K]5"), Array::scalar_i64(9));
}

/// With no brackets at the call there is no axis, and `χ` is a name with
/// no value like any other.
#[test]
fn chi_has_no_value_where_no_axis_was_written() {
    let e = err(Lang::Apl, "F←{⍵+χ} ⋄ F 5");
    assert_eq!(e.kind, ErrorKind::Value);
    assert!(e.msg.contains('χ'), "{}", e.msg);
}

/// An axis belongs to the call that wrote it: a definition applied inside
/// the body does not inherit it.
#[test]
fn an_axis_reaches_one_call_and_no_further() {
    assert_eq!(apl("G←{⍵×2} ⋄ F←{χ+G ⍵} ⋄ F[10]5"), Array::scalar_i64(20));
}

/// A definition whose header names no axis has nowhere to put one, and
/// says so. That is a refusal, not a gap.
#[test]
fn a_definition_with_no_axis_name_refuses_one() {
    let e = err(Lang::Apl, "∇Z←NAX B\nZ←B\n∇\nNAX[1] 5");
    assert_ne!(e.kind, ErrorKind::NotYet, "{}", e.msg);
    assert!(e.msg.contains("no axis"), "{}", e.msg);
}

// --- an array where a function operand belongs ---------------------------

/// A user-written operator takes an array for `⍺⍺` or `⍵⍵`, and the array
/// may be an expression or a name. The READING is still fixed while the
/// program compiles — an array operand parses the body one way and a
/// function operand another — so only the value waits.
#[test]
fn an_operator_takes_a_computed_array_operand() {
    assert_eq!(apl("(⍳3){⍺⍺+⍵}0"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(apl("(1+1){⍺⍺+⍵}3"), Array::scalar_i64(5));
    assert_eq!(apl("V←1 2 3 ⋄ V{⍺⍺+⍵}10"), i64s(&[3], &[11, 12, 13]));
    assert_eq!(apl("V←1 2 ⋄ 0{⍺⍺+⍵⍵+⍵}V⊢10"), i64s(&[2], &[11, 12]));
    assert_eq!(apl("∇Z←F N\nZ←N{⍺⍺+⍵}1\n∇\nF 9"), Array::scalar_i64(10));
}

// --- J's bond and amend ---------------------------------------------------

/// `n&v` and `u&n` read their noun where the derived verb is APPLIED, so a
/// name — or an expression, or a definition's own argument — may stand
/// where a literal does. These are the two spellings whole programs reach
/// for: a matrix power is `(m&mp)^:k m`, and a rolling return is
/// `(%&(}: c) - 1:) }. c`.
#[test]
fn a_bonded_noun_may_be_computed() {
    assert_eq!(j("n =. 1 2 3\nf =. n & +\nf 10"), i64s(&[3], &[11, 12, 13]));
    assert_eq!(j("n =. 3\nf =. + & n\nf 10"), Array::scalar_i64(13));
    assert_eq!(j("c =. 2 4 8\n<. (%&(}: c) - 1:) }. c"), i64s(&[2], &[1, 1]));
    assert_eq!(
        j("mp =. +/ . *\nm =. 2 2 $ 1 1 1 0\n, (m & mp) ^: 3 m"),
        i64s(&[4], &[5, 3, 3, 2])
    );
}

/// Nothing is cached: the noun is read afresh at every application, so a
/// name reassigned between two of them moves the answer.
#[test]
fn a_bonded_noun_is_read_at_every_application() {
    assert_eq!(j("n =. 1\nf =. n & +\na =. f 10\nn =. 2\nb =. f 10\na , b"), i64s(&[2], &[11, 12]));
    assert_eq!(j("g =. 3 : '(y & +) 100'\ng 5"), Array::scalar_i64(105));
}

/// The indices `m}` amends at may be computed for the same reason, which is
/// what lets a sieve cross off a stride it worked out for itself.
#[test]
fn an_amend_index_may_be_computed() {
    assert_eq!(j("j =. 1 3\nb =. i. 5\n0 j } b"), i64s(&[5], &[0, 0, 2, 0, 4]));
    assert_eq!(j("j =. 2 * i. 3\n9 j } i. 6"), i64s(&[6], &[9, 1, 9, 3, 9, 5]));
    assert_eq!(j("k =. < 0 1\n, 7 k } 2 2 $ 0"), i64s(&[4], &[0, 7, 0, 0]));
    assert_eq!(j("j =. 0\nb =. i. 3\nx =. 9 j } b\nj =. 2\n(, x) , 9 j } b"), i64s(&[6], &[9, 1, 2, 0, 1, 9]));
}

/// The one part of the amend that cannot wait: a GERUND operand decides how
/// the amend parses — which three verbs compute the replacement, the
/// indices and the array — so a gerund reached through a name is refused
/// rather than misread as a list of indices.
#[test]
fn a_computed_gerund_amend_is_still_a_named_gap() {
    let e = err(Lang::J, "h =. 3 : '2 (y}) i. 3'\nh 0:`1:`]");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("gerund amend"), "{}", e.msg);
}

// --- indexed assignment as an expression ---------------------------------

/// `A[i]←v` is an expression like any other. Its value is the value
/// ASSIGNED — not the array it was written into — so it may stand inside a
/// larger sentence and chain.
#[test]
fn an_indexed_assignment_yields_the_value_assigned() {
    assert_eq!(apl("A←1 2 3 ⋄ B←A[2]←5 ⋄ B"), Array::scalar_i64(5));
    assert_eq!(apl("A←1 2 3 ⋄ 2+A[1]←9"), Array::scalar_i64(11));
    assert_eq!(apl("A←1 2 3 ⋄ 0+A[1 2]←7 8"), i64s(&[2], &[7, 8]));
    assert_eq!(apl("A←2 3⍴⍳6 ⋄ 1+A[1;]←9 9 9"), i64s(&[3], &[10, 10, 10]));
    assert_eq!(
        apl("A←1 2 3 ⋄ C←1 2 3 ⋄ A[1]←C[2]←9 ⋄ A,C"),
        i64s(&[6], &[9, 2, 3, 1, 9, 3])
    );
}

/// The write still happens, whatever the sentence around it does with the
/// value.
#[test]
fn an_indexed_assignment_inside_a_sentence_still_writes() {
    assert_eq!(apl("A←1 2 3 ⋄ B←A[2]←5 ⋄ A"), i64s(&[3], &[1, 5, 3]));
    assert_eq!(apl("A←1 2 3 ⋄ Q←2+A[1]←9 ⋄ A"), i64s(&[3], &[9, 2, 3]));
}

/// Writing through something that is not a name is still a named gap: the
/// target has to be a name for the write to have somewhere to go.
#[test]
fn writing_through_an_expression_is_still_a_gap() {
    let e = err(Lang::Apl, "A←1 2 3 ⋄ (A,4)[1]←9");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("through an expression"), "{}", e.msg);
}

// --- what the family still does not do -----------------------------------

/// `⎕FX` on text the program assembles stays a named gap. libjay resolves
/// every name to the function it stands for while it compiles, and a
/// definition whose HEADER is not literal text hides its name from that
/// pass, so a later sentence calling it cannot be parsed as an
/// application at all.
#[test]
fn quad_fx_on_computed_text_is_still_a_named_gap() {
    for src in [
        "L←'Z←G N' 'Z←N+100' ⋄ ⎕FX L ⋄ G 3",
        "T←'Z←N×2' ⋄ ⎕FX 'Z←F N' T ⋄ F 3",
    ] {
        let e = err(Lang::Apl, src);
        assert_eq!(e.kind, ErrorKind::NotYet, "{src:?}: {}", e.msg);
        assert!(e.msg.contains("⎕FX"), "{}", e.msg);
    }
}

/// The brackets that pick a MEANING rather than name an axis stay
/// compile-time: which function the glyph stands for cannot wait for a
/// value.
#[test]
fn the_brackets_that_choose_a_function_stay_settled_at_compile_time() {
    for (src, what) in [
        ("N←3 ⋄ 2⊤[N]5", "digit count"),
        ("K←8 ⋄ 1 2 ⌹[K] 1 1", "function number"),
    ] {
        let e = err(Lang::Apl, src);
        assert_eq!(e.kind, ErrorKind::NotYet, "{src:?}: {}", e.msg);
        assert!(e.msg.contains(what), "{}", e.msg);
    }
}
