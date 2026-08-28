//! The first wild-hunt pass: the ranks a derived verb reports, the negative
//! power's obverse, base literals with a number for a base, prime factors
//! past a machine integer, the ASCII `^`, and the gerund adverbs.
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
    // An explicit rank overrides it, as it overrides any other.
    assert_eq!(j("(%~\"1) b. 0"), f64s(&[3], &[1.0, 1.0, 1.0]));
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
    assert_eq!(j("(+ :. -) b. 0"), f64s(&[3], &[0.0, 0.0, 0.0]));
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
    assert_eq!(j("(+`-)/ i.0"), Array::scalar_i64(0));
    assert_eq!(j("(*`-)/ i.0"), Array::scalar_i64(1));
    // The other three give one verb to each piece in turn.
    assert_eq!(j("(+`-)\\ 1 2 3"), i64s(&[3, 3], &[1, 0, 0, -1, -2, 0, 1, 2, 3]));
    assert_eq!(j("(+`-)\\. 1 2 3"), i64s(&[3, 3], &[1, 2, 3, -2, -3, 0, 3, 0, 0]));
    assert_eq!(j("(+`-)/. 1 2 3"), i64s(&[3, 1], &[1, -2, 3]));
    assert_eq!(j("1 0 1 (+`-)/. 1 2 3"), i64s(&[2, 2], &[1, 3, -2, 0]));
}
