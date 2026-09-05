//! The wild hunt: what running harvested J and APL against the references
//! turned up. The first pass gave the ranks a derived verb reports, the
//! negative power's obverse, base literals with a number for a base, prime
//! factors past a machine integer, the ASCII `^` and the gerund adverbs;
//! the second gave the boxed ordering, the constant verb, the noun and
//! monad-dyad definitions, complex copy counts, amend over a list of
//! specifications and over a gerund, APL's Unicode look-alikes,
//! distributed assignment, line-numbered `∇` bodies, a mixed-depth each
//! and `@`.
//!
//! The corpora in tests/corpus/j/wildhunt.txt and
//! tests/corpus/apl/wildhunt.txt carry the breadth against the references;
//! this file states one rule per assertion, and pins the refusals that stay
//! refusals.

use jay::{Array, Data, Dialect, Error, ErrorKind, Lang, compile};

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

fn f64s(shape: &[usize], values: &[f64]) -> Array {
    Array::new(shape.to_vec(), Data::F64(values.to_vec().into()))
}

/// `_` as a rank, the way `b. 0` prints it.
const INF: f64 = f64::INFINITY;

// --- the ranks a derived verb reports -------------------------------------

#[test]
fn a_commuted_verb_reports_its_operands_ranks_exchanged() {
    // `u~` takes on the left what u takes on the right, and its monadic
    // rank is infinite whatever the operand's is.
    assert_eq!(j("(%~) b. 0"), f64s(&[3], &[INF, 0.0, 0.0]));
    assert_eq!(j("(|.~) b. 0"), f64s(&[3], &[INF, INF, 1.0]));
    assert_eq!(j("(#~) b. 0"), f64s(&[3], &[INF, INF, 1.0]));
    assert_eq!(j("(,~) b. 0"), f64s(&[3], &[INF, INF, INF]));
    // An explicit rank overrides it, as it overrides any other — and
    // ranks that are all finite are reported as WHOLE NUMBERS, which is
    // what `3!:0 ((%~"1) b. 0)` is in the reference.
    assert_eq!(j("(%~\"1) b. 0"), i64s(&[3], &[1, 1, 1]));
}

#[test]
fn the_table_of_a_commuted_verb_frames_by_those_ranks() {
    // With the ranks right, `u~/~` is the table the other way round rather
    // than one elementwise pass.
    assert_eq!(j("(>.~)/~ 5 2 9"), i64s(&[3, 3], &[5, 5, 9, 5, 2, 9, 9, 9, 9]));
    // And a table of two different lengths conforms instead of failing.
    assert_eq!(
        j("3 4 (-~)/ 10 20 30"),
        i64s(&[2, 3], &[7, 17, 27, 6, 16, 26])
    );
}

#[test]
fn a_cut_reports_the_rank_of_one_cut_and_frames_the_rest() {
    // The rectangle cuts read two rows of origins and sizes; the interval
    // cuts read one list of frets.
    assert_eq!(j("(<;.0) b. 0"), f64s(&[3], &[INF, 2.0, INF]));
    assert_eq!(j("(+;.3) b. 0"), f64s(&[3], &[INF, 2.0, INF]));
    assert_eq!(j("(+;.1) b. 0"), f64s(&[3], &[INF, 1.0, INF]));
    assert_eq!(j("(+;._2) b. 0"), f64s(&[3], &[INF, 1.0, INF]));
    // A longer left argument is then a FRAME of cuts, one per cell.
    assert_eq!(j("$ (2 2 2$1 1 2 2 0 0 2 2) <;.0 i.5 5"), i64s(&[1], &[2]));
    assert_eq!(j("(2 2 2$0 0 3 3 1 1 2 2) +/;.0 i.5 5"), i64s(&[2, 3], &[15, 18, 21, 17, 19, 0]));
    assert_eq!(j("$ (2 3$1 0 0 0 1 0) <;.1 i.3 3"), i64s(&[2], &[2, 1]));
}

#[test]
fn adverse_reports_infinite_ranks_because_the_verb_is_not_settled() {
    // Which of `u :: v` runs is not known until one of them fails, so no
    // finite rank would be honest. `u :. v` runs u and has u's ranks.
    assert_eq!(j("(* :: -) b. 0"), f64s(&[3], &[INF, INF, INF]));
    assert_eq!(j("(+ :. -) b. 0"), i64s(&[3], &[0, 0, 0]));
}

// --- the negative power ---------------------------------------------------

#[test]
fn a_negative_power_undoes_the_bond_when_it_is_applied_dyadically() {
    // `x u^:_1 y` is the inverse of `x&u`, which is not u's own obverse.
    assert_eq!(j("7 (+^:_2) 20"), Array::scalar_i64(6));
    assert_eq!(j("3 (|.^:_1) 1 2 3 4 5"), i64s(&[5], &[3, 4, 5, 1, 2]));
    assert_eq!(j("2 (#.^:_1) 9"), i64s(&[4], &[1, 0, 0, 1]));
    // The bond's obverse reaches verbs whose monad has none at all.
    assert_eq!(j("2 (*^:_1) 6"), Array::scalar_f64(3.0));
    // Monadically it is still the verb's own obverse.
    assert_eq!(j("(*:^:_1) 16"), Array::scalar_f64(4.0));
    // APL's `⍣¯n` reads the same table.
    assert_eq!(apl("4(+⍣¯3)20"), Array::scalar_i64(8));
    assert_eq!(apl("2(×⍣¯1)6"), Array::scalar_f64(3.0));
}

#[test]
fn a_list_of_power_counts_may_run_either_way() {
    // One answer per count, and a negative one counts backwards over the
    // obverse from the same starting value.
    assert_eq!(j("2 (+^:_1 2 3) 20"), i64s(&[3], &[18, 24, 26]));
    assert_eq!(j("(>:^:(_2 0 2)) 10"), i64s(&[3], &[8, 10, 12]));
    assert_eq!(j("2 (+^:(<_3)) 20"), i64s(&[3], &[20, 18, 16]));
}

#[test]
fn a_missing_obverse_is_named_when_the_power_runs() {
    // Which obverse `^:_1` needs depends on the arguments, so the verb with
    // none is named at run time rather than at compile time.
    let e = err(Lang::J, "(+/ % #) ^: _1 [ 1 2 3");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("obverse of"), "{}", e.msg);
}

// --- numeric literals -----------------------------------------------------

#[test]
fn a_base_literal_takes_any_number_for_its_base() {
    // `b` binds looser than the rest of the grammar on both sides: the base
    // is a number in its own right, and every letter after the `b` is a
    // digit.
    assert_eq!(j("3r4b11"), Array::scalar_f64(1.75));
    assert_eq!(j("1r10b12"), Array::scalar_f64(2.1));
    assert_eq!(j("2e1b11"), Array::scalar_i64(21));
    assert_eq!(j("36bxyz"), Array::scalar_i64(44027));
    assert_eq!(j("2b11p1"), Array::scalar_i64(63));
    assert_eq!(j("_16bff"), Array::scalar_i64(-225));
    // A `.` among the digits starts the negative powers.
    assert_eq!(j("2b11.1"), Array::scalar_f64(3.5));
    assert_eq!(j("2b.1"), Array::scalar_f64(0.5));
    // A complex base multiplies as a complex number.
    assert_eq!(j("3j4b11"), Array::new(vec![], Data::Complex(vec![[4.0, 4.0]].into())));
}

// --- prime factors --------------------------------------------------------

#[test]
fn prime_factors_are_exact_however_many_digits_the_number_has() {
    assert_eq!(j("*/ q: 2^70x"), j("2^70x"));
    assert_eq!(j("*/ q: 999999999999999999999x"), j("999999999999999999999x"));
    // A float that holds a whole number is admitted on that ground and not
    // on fitting a machine integer.
    assert_eq!(j("*/ q: 6.5e19"), Array::scalar_f64(6.5e19));
    // `q:` reads its whole argument: one row per item, padded with 1s.
    assert_eq!(j("q: 2 3 4 5"), i64s(&[4, 2], &[2, 1, 3, 1, 2, 2, 5, 1]));
    assert_eq!(j("q: 1"), i64s(&[0], &[]));
    // `_k q:` keeps the last k columns of the factor table.
    assert_eq!(j("_1 q: 2310"), i64s(&[2, 1], &[11, 1]));
    assert_eq!(j("_2 q: 360"), i64s(&[2, 2], &[3, 5, 2, 1]));
}

// --- the ASCII `^` --------------------------------------------------------

#[test]
fn apl_reads_ascii_caret_as_and() {
    assert_eq!(apl("4^6"), Array::scalar_i64(12));
    assert_eq!(apl("1 0^0 1"), i64s(&[2], &[0, 0]));
    assert_eq!(apl("4^6"), apl("4∧6"));
}

// --- gerunds under the adverbs --------------------------------------------

#[test]
fn a_gerund_under_an_adverb_cycles_through_its_verbs() {
    // `/` inserts the verbs between the items, left to right, and folds
    // right to left: `1 + (2 - 3)`.
    assert_eq!(j("(+`-)/ 1 2 3"), Array::scalar_i64(0));
    assert_eq!(j("(+`-`*)/ 1 2 3 4"), Array::scalar_i64(-9));
    // With no items there is no insertion, so the answer is the identity
    // element of the verb the fold would have reached first.
    assert_eq!(j("(+`-)/ i.0"), Array::scalar_bool(false));
    assert_eq!(j("(*`-)/ i.0"), Array::scalar_bool(true));
    // The other three give one verb to each piece in turn.
    assert_eq!(j("(+`-)\\ 1 2 3"), i64s(&[3, 3], &[1, 0, 0, -1, -2, 0, 1, 2, 3]));
    assert_eq!(j("(+`-)\\. 1 2 3"), i64s(&[3, 3], &[1, 2, 3, -2, -3, 0, 3, 0, 0]));
    assert_eq!(j("(+`-)/. 1 2 3"), i64s(&[3, 1], &[1, -2, 3]));
    assert_eq!(j("1 0 1 (+`-)/. 1 2 3"), i64s(&[2, 2], &[1, 3, -2, 0]));
}

// --- the second wild-hunt pass --------------------------------------------

#[test]
fn the_boxed_ordering_compares_the_atoms_before_the_shape() {
    // `a` precedes `b`, and neither list's LENGTH gets a say until the
    // atoms they share have tied.
    assert_eq!(j("/: (<'aa'), (<,'b')"), i64s(&[2], &[0, 1]));
    assert_eq!(j("\\: (<'aa'), (<,'b')"), i64s(&[2], &[1, 0]));
    assert_eq!(j("/: (<1 2 3), (<,9)"), i64s(&[2], &[0, 1]));
    // A prefix still sorts before its extension, and the rank is still
    // compared before anything the atoms could say.
    assert_eq!(j("/: (<1 2),(<1 2 0)"), i64s(&[2], &[0, 1]));
    assert_eq!(j("/: (<3),(<0 0 0)"), i64s(&[2], &[0, 1]));
    // Two arrays with no atoms have no items to compare, so the shape is
    // all that separates them, read with the LAST axis first.
    assert_eq!(j("/: (<0 5$0),(<5 0$0)"), i64s(&[2], &[1, 0]));
    // `I.` shares the comparator, so a sorted table looks up the right slot.
    assert_eq!(j("((<'aa'),(<,'b'),(<'ccc')) I. (<,'b')"), Array::scalar_i64(1));
}

#[test]
fn a_noun_left_of_rank_is_the_constant_verb() {
    assert_eq!(j("2 (3\"0) 5"), Array::scalar_i64(3));
    assert_eq!(j("(3\"1) i.2 3"), i64s(&[2], &[3, 3]));
    assert_eq!(j("(1 2 3\"_) 0"), i64s(&[3], &[1, 2, 3]));
    // The rank it is written with is the rank it reports.
    assert_eq!(j("3\"0 b. 0"), j("+ b. 0"));
    // A noun that is not settled when the program is compiled is a gap.
    let e = err(Lang::J, "f =. i. 3\n(f\"0) 1");
    assert_eq!(e.kind, ErrorKind::NotYet);
}

#[test]
fn a_noun_definition_reads_the_lines_below_it_as_text() {
    // Each line is followed by a line break, so a two-line body is 6
    // characters and an empty one is none at all.
    assert_eq!(j("t =: 0 : 0\nab\ncd\n)\n$t"), i64s(&[1], &[6]));
    assert_eq!(j("w =: 0 : 0\n)\n$w"), i64s(&[1], &[0]));
    // Leading blanks belong to the text: the form exists to hold a table.
    assert_eq!(j("r =: 0 : 0\n  ab\n c\n)\n$r"), i64s(&[1], &[8]));
    // Written inline, the string IS the value.
    assert_eq!(j("$ 0 : 'xy'"), i64s(&[1], &[2]));
    let e = err(Lang::J, "t =: 0 : 0\nab");
    assert!(e.msg.contains("closing `)`"), "{}", e.msg);
}

#[test]
fn the_monad_dyad_conjunction_joins_two_verbs() {
    assert_eq!(j("f =: (+/) : (-/)\nf 1 2 3"), Array::scalar_i64(6));
    assert_eq!(j("f =: (+/) : (-/)\n1 f 2"), Array::scalar_i64(-1));
    // It keeps neither operand's ranks: both arguments arrive whole.
    assert_eq!(j("(+/ : (-/)) b. 0"), val(Lang::J, "] b. 0"));
}

#[test]
fn a_complex_count_copies_and_then_fills() {
    assert_eq!(j("1j2 # 'a'"), val(Lang::J, "'a  '"));
    assert_eq!(j("2j1 1j0 # 'ab'"), val(Lang::J, "'aa b'"));
    assert_eq!(j("0j3 # 7"), i64s(&[3], &[0, 0, 0]));
    // Both halves have to be non-negative whole numbers.
    for src in ["_1j2 # 3 4", "1j_2 # 3", "1.5j2 # 3"] {
        assert_eq!(err(Lang::J, src).kind, ErrorKind::Domain, "{src}");
    }
}

#[test]
fn amend_takes_a_list_of_index_specifications() {
    assert_eq!(
        j("7 8 (0 1; 2 0) } 3 3 $ 0"),
        i64s(&[3, 3], &[0, 7, 0, 0, 0, 0, 8, 0, 0])
    );
    // A single value fills whichever cells were named, however large.
    assert_eq!(
        j("9 (1 1) } 3 3 $ 0"),
        i64s(&[3, 3], &[0, 0, 0, 9, 9, 9, 0, 0, 0])
    );
    assert_eq!(j("0 (<1) } i.3 3"), i64s(&[3, 3], &[0, 1, 2, 0, 0, 0, 6, 7, 8]));
    // An item of a LIST of specifications holds integers: the reference
    // refuses a nested one there, though the same value selects with `{`.
    assert_eq!(err(Lang::J, "9 ((<0);(<1)) } i.3 3").kind, ErrorKind::Domain);
}

#[test]
fn the_gerund_amend_computes_all_three_of_its_arguments() {
    // u makes the replacement, v the indices, w the array they go into.
    assert_eq!(j("1 3 (+:@:{`[`]}) 9 8 7 6"), i64s(&[4], &[9, 16, 7, 12]));
    assert_eq!(j("1 (0:`[`]}) 5 6 7"), i64s(&[3], &[5, 0, 7]));
    // The monad amends nothing: it is the noun amend's own monad over what
    // v and w answer, a SELECTION, and u is not applied at all.
    assert_eq!(j("(0:`0:`]}) 1 2 3"), Array::scalar_i64(1));
    assert_eq!(j("(+:)`(1&{)`]} i.5"), Array::scalar_i64(1));
    assert_eq!(j("(-:)`(0&{)`]} 1 2 3"), Array::scalar_i64(2));
}

#[test]
fn a_gerund_may_name_the_self_reference() {
    assert_eq!(j("(+`$:)@.0 ] 3"), Array::scalar_i64(3));
    // Applied where no definition is running it is the tacit `$:`, which
    // is a queue position and says so.
    let e = err(Lang::J, "$: 5");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("tacit"), "{}", e.msg);
}

#[test]
fn apl_reads_the_unicode_look_alikes() {
    assert_eq!(apl("7 ∣ 23"), apl("7 | 23"));
    assert_eq!(apl("2 ∈ 1 2 3"), apl("2 ∊ 1 2 3"));
    assert_eq!(apl("∼ 1 0 1"), apl("~ 1 0 1"));
    assert_eq!(apl("2 ⋆ 3"), apl("2 * 3"));
    assert_eq!(apl("5 − 2"), apl("5 - 2"));
    // Inside a literal the character stands for itself.
    assert_eq!(apl("⍴'∣∈∼⋆−'"), i64s(&[1], &[5]));
    // The list is closed: these near twins the reference refuses too.
    for src in ["∗ 2", "∸ 2", "2 ∅ 3"] {
        assert_eq!(err(Lang::Apl, src).kind, ErrorKind::Parse, "{src}");
    }
}

#[test]
fn distributed_assignment_shares_the_value_out() {
    assert_eq!(apl("(a b)←1 2 ⋄ a+b"), Array::scalar_i64(3));
    assert_eq!(apl("(c d)←3 4 ⋄ (c d)←d c ⋄ c-d"), Array::scalar_i64(1));
    // A scalar goes to every name, and the sentence still yields the value.
    assert_eq!(apl("(g h)←5 ⋄ g,h"), i64s(&[2], &[5, 5]));
    assert_eq!(apl("1+(v w)←1 2"), i64s(&[2], &[2, 3]));
    assert_eq!(err(Lang::Apl, "(i j)←1 2 3").kind, ErrorKind::Length);
    assert_eq!(err(Lang::Apl, "(i j)←2 2⍴⍳4").kind, ErrorKind::Rank);
    assert_eq!(err(Lang::Apl, "(i 1)←1 2").kind, ErrorKind::Parse);
}

#[test]
fn a_nabla_body_may_carry_its_line_numbers() {
    assert_eq!(apl("∇ r←f x\n[1]   r←x×2\n∇\nf 5"), Array::scalar_i64(10));
    assert_eq!(apl("∇ r←g x\n[1] r←x+1\n[2] r←r×2\n∇\ng 3"), Array::scalar_i64(8));
    assert_eq!(apl("∇ r←h x\n[1.1] r←x-1\n∇\nh 9"), Array::scalar_i64(8));
    // A label keeps its meaning behind one.
    assert_eq!(
        apl("∇ r←fac n\n[1] →(n≤1)/L\n[2] r←n×fac n-1\n[3] →0\n[4] L:r←1\n∇\nfac 5"),
        Array::scalar_i64(120)
    );
}

#[test]
fn an_each_frames_results_of_different_depth() {
    // A simple cell beside an enclosed one is the nested vector such a
    // result is, not a refusal.
    assert_eq!(apl("⌽¨ 1 (2 3)"), apl("1 (3 2)"));
    assert_eq!(apl("{⍵}¨ 1 (2 3)"), apl("1 (2 3)"));
    assert_eq!(apl("⌽¨ 'ab' 1 2"), apl("'ba' 1 2"));
}

#[test]
fn apl_at_changes_the_positions_its_right_operand_names() {
    // Dyalog's; GNU APL has no `@`, so these are pinned against the
    // recorded Dyalog answers in corpus/apl/dyalog-operators.txt.
    assert_eq!(apl("9@2 ⊢ 0 0 0"), i64s(&[3], &[0, 9, 0]));
    assert_eq!(apl("(9 8)@(1 3) ⊢ 0 0 0 0"), i64s(&[4], &[9, 0, 8, 0]));
    assert_eq!(apl("-@2 ⊢ 1 2 3"), i64s(&[3], &[1, -2, 3]));
    // A function right operand answers a mask over the items.
    assert_eq!(
        apl("(×∘10)@(2∘|) ⊢ 1 2 3 4 5"),
        i64s(&[5], &[10, 2, 30, 4, 50])
    );
    // The dyad is a named gap, and so is a computed operand.
    let e = err(Lang::Apl, "1 (9@2) 0 0 0");
    assert_eq!(e.kind, ErrorKind::NotYet);
}
