//! End-to-end tests for the fourth coverage wave: classification and the
//! set functions, permutations, the text verbs, the prime queries, under
//! and the obverse table it rests on, gerunds and agenda, the adverse,
//! sandboxed execute, expand, pick, and APL's two compositions.
//!
//! The differential evidence is in tests/corpus/{j,apl}/wave4.txt, which is
//! where every meaning that has an oracle was taken from. This file carries
//! the rest: the exact text of the new diagnostics, the rules neither
//! reference implements (Dyalog's `↓`, `∘`, `⍥`, and the `⎕`-names GNU APL
//! has no value for), and the sandbox's refusals.

use jay::{compile, Array, Data, Dialect, ErrorKind, Lang};

fn run_dialect(lang: Lang, src: &str, dialect: &Dialect) -> Option<Array> {
    let program = compile(lang, src, dialect)
        .unwrap_or_else(|e| panic!("compile of {src:?} failed:\n{}", e.render(src)));
    let mut sink = |_: &str| {};
    program
        .run(&[], &mut sink)
        .unwrap_or_else(|e| panic!("run of {src:?} failed:\n{}", program.render_error(&e)))
}

fn val(lang: Lang, src: &str) -> Array {
    run_dialect(lang, src, &Dialect::default())
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
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

fn bits(shape: &[usize], values: &[u8]) -> Array {
    Array::new(shape.to_vec(), Data::Bool(values.to_vec().into()))
}

fn text(shape: &[usize], s: &str) -> Array {
    Array::new(shape.to_vec(), Data::Char(s.chars().collect()))
}

fn boxes(shape: &[usize], items: Vec<Array>) -> Array {
    Array::new(shape.to_vec(), Data::Box(items.into()))
}

// --- classification and sets ---------------------------------------------

#[test]
fn self_classify_gives_one_row_per_distinct_item() {
    assert_eq!(val(Lang::J, "= 1 2 1 3"), bits(&[3, 4], &[1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1]));
    // A scalar is one item, so it classifies into a 1 by 1 table.
    assert_eq!(val(Lang::J, "= 5"), bits(&[1, 1], &[1]));
    assert_eq!(val(Lang::J, "$ = i. 0"), i64s(&[2], &[0, 0]));
}

#[test]
fn the_nub_sieve_marks_first_occurrences() {
    assert_eq!(val(Lang::J, "~: 1 2 1 3"), bits(&[4], &[1, 1, 0, 1]));
    assert_eq!(val(Lang::Apl, "≠ 1 2 1 3"), bits(&[4], &[1, 1, 0, 1]));
    // It agrees with the nub it sieves for.
    assert_eq!(val(Lang::J, "(~: # ]) 3 1 4 1 5"), val(Lang::J, "~. 3 1 4 1 5"));
}

#[test]
fn the_boolean_dyads_read_only_booleans() {
    assert_eq!(val(Lang::J, "1 0 1 +: 0 1 1"), bits(&[3], &[0, 0, 0]));
    assert_eq!(val(Lang::J, "1 0 1 *: 0 1 1"), bits(&[3], &[1, 1, 0]));
    assert_eq!(val(Lang::Apl, "1 0 1 ⍱ 0 1 1"), bits(&[3], &[0, 0, 0]));
    assert_eq!(val(Lang::Apl, "1 0 1 ⍲ 0 1 1"), bits(&[3], &[1, 1, 0]));
    for src in ["2 +: 3", "2 *: 3"] {
        let e = err(Lang::J, src);
        assert_eq!(e.kind, ErrorKind::Domain, "{src}");
        assert!(e.msg.contains("0 or 1"), "{src}: {}", e.msg);
    }
}

#[test]
fn the_set_functions_work_on_items_not_elements() {
    assert_eq!(val(Lang::J, "1 2 3 4 -. 2 4"), i64s(&[2], &[1, 3]));
    assert_eq!(val(Lang::Apl, "1 2 3 ∪ 3 4 5"), i64s(&[5], &[1, 2, 3, 4, 5]));
    assert_eq!(val(Lang::Apl, "1 2 3 ∩ 2 3 4"), i64s(&[2], &[2, 3]));
    // Union sieves only the right argument; the left keeps its repeats.
    assert_eq!(val(Lang::Apl, "1 1 2 ∪ 2 3"), i64s(&[4], &[1, 1, 2, 3]));
    // Rows of a matrix are items, and match as wholes.
    assert_eq!(val(Lang::J, "(i. 3 2) -. 2 3"), i64s(&[2, 2], &[0, 1, 4, 5]));
}

#[test]
fn find_marks_where_a_sequence_begins() {
    assert_eq!(val(Lang::J, "'ab' E. 'abcabc'"), bits(&[6], &[1, 0, 0, 1, 0, 0]));
    assert_eq!(val(Lang::Apl, "'abc' ⍷ 'xabcabc'"), bits(&[7], &[0, 1, 0, 0, 1, 0, 0]));
    // A pattern longer than the argument matches nowhere, and the answer is
    // still shaped like the argument.
    assert_eq!(val(Lang::J, "'abcd' E. 'ab'"), bits(&[2], &[0, 0]));
    // Overlapping matches are all reported.
    assert_eq!(val(Lang::J, "1 1 E. 1 1 1"), bits(&[3], &[1, 1, 0]));
}

// --- permutations ---------------------------------------------------------

#[test]
fn the_anagram_index_and_its_inverse_agree() {
    // Every permutation of four items round-trips through its index.
    for k in 0..24i64 {
        let src = format!("A. {k} A. i. 4");
        assert_eq!(val(Lang::J, &src), Array::scalar_i64(k), "{src}");
    }
    // A list that is not a permutation is indexed by the ranks of its items.
    assert_eq!(val(Lang::J, "A. 3 1 2"), val(Lang::J, "A. 2 0 1"));
    // Characters have no anagram index in J, and none here.
    assert_eq!(err(Lang::J, "A. 'abc'").kind, ErrorKind::Domain);
    // An index past the last permutation is out of range, not wrapped.
    let e = err(Lang::J, "24 A. i. 4");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("out of range"), "{}", e.msg);
}

#[test]
fn cycles_and_direct_permutations_convert_both_ways() {
    for src in ["2 0 1", "0 1 2", "1 0 3 2", "0 2 1", "3 2 1 0"] {
        let round = format!("C. C. {src}");
        assert_eq!(val(Lang::J, &round), val(Lang::J, src), "{round}");
    }
    // A boxed left argument is cycles; anything unmentioned stays put.
    assert_eq!(val(Lang::J, "(<0 1) C. 'abcd'"), text(&[4], "bacd"));
    // A short direct permutation is abbreviated: the items it never names
    // come first, in ascending order. An atom is such a list of one.
    assert_eq!(val(Lang::J, "0 1 C. 'abcde'"), text(&[5], "cdeab"));
    assert_eq!(val(Lang::J, "3 4 2 C. 'abcde'"), text(&[5], "abdec"));
    assert_eq!(val(Lang::J, "2 C. 'abcde'"), text(&[5], "abdec"));
    assert_eq!(val(Lang::J, "2 3 C. 'abcd'"), text(&[4], "abcd"));
    // The same abbreviation on its own: `C. 3 4 2` is a permutation of five.
    assert_eq!(val(Lang::J, "C. C. 3 4 2"), val(Lang::J, "0 1 3 4 2"));
    // Not a permutation, and one naming an item that is not there.
    assert_eq!(err(Lang::J, "1 1 C. 'abc'").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "0 1 2 C. 'ab'").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "C. 3 3").kind, ErrorKind::Domain);
    assert_eq!(err(Lang::J, "C. _1 2").kind, ErrorKind::Domain);
}

// --- text -----------------------------------------------------------------

#[test]
fn unicode_converts_in_the_direction_the_form_asks_for() {
    assert_eq!(val(Lang::J, "u: 65 66 67"), text(&[3], "ABC"));
    // J's monad passes characters through; APL's `⎕UCS` reverses them.
    assert_eq!(val(Lang::J, "u: 'abc'"), text(&[3], "abc"));
    assert_eq!(val(Lang::Apl, "⎕UCS 'abc'"), i64s(&[3], &[97, 98, 99]));
    assert_eq!(val(Lang::J, "3 u: 'abc'"), i64s(&[3], &[97, 98, 99]));
    assert_eq!(val(Lang::J, "10 u: 955"), text(&[], "λ"));
    assert_eq!(val(Lang::Apl, "⎕UCS 955"), text(&[], "λ"));
    // The byte-oriented forms. libjay's one character type holds
    // codepoints, so a list every one of whose codepoints is a byte is
    // what stands for J's byte string: 8 leaves that alone and packs
    // anything else into its UTF-8 bytes, and 9 reads those bytes back.
    assert_eq!(val(Lang::J, "8 u: 'ab'"), text(&[2], "ab"));
    assert_eq!(val(Lang::J, "3 u: 8 u: 10 u: 955"), i64s(&[2], &[206, 187]));
    assert_eq!(val(Lang::J, "9 u: 8 u: 10 u: 955"), text(&[1], "λ"));
    assert_eq!(val(Lang::J, "3 u: 1 u: 10 u: 955"), i64s(&[], &[187]));
    assert_eq!(val(Lang::J, "2 u: 'ab'"), text(&[2], "ab"));
    // A form the reference gives no meaning to, and a fit only characters
    // can take.
    let e = err(Lang::J, "4 u: 'ab'");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("unicode conversion form"), "{}", e.msg);
    assert_eq!(err(Lang::J, "1 u: 955").kind, ErrorKind::Domain);
    // Not a codepoint at all.
    assert_eq!(err(Lang::J, "10 u: _1").kind, ErrorKind::Domain);
}

#[test]
fn words_applies_the_j_tokeniser_to_a_string() {
    assert_eq!(
        val(Lang::J, ";: 'a + b'"),
        boxes(&[3], vec![text(&[1], "a"), text(&[1], "+"), text(&[1], "b")])
    );
    // A run of numeric literals is one word; an inflected verb keeps its
    // inflection and the number after it starts a new word.
    assert_eq!(val(Lang::J, "# ;: '1 2 3'"), Array::scalar_i64(1));
    assert_eq!(val(Lang::J, "# ;: 'i.5'"), Array::scalar_i64(2));
    // A comment is one word, to the end of the line.
    assert_eq!(val(Lang::J, "# ;: '2 NB. and the rest'"), Array::scalar_i64(2));
    // A string that never closes is a parse error, not a truncated word.
    assert_eq!(err(Lang::J, ";: '''ab'").kind, ErrorKind::Parse);
    assert_eq!(err(Lang::J, ";: 1 2 3").kind, ErrorKind::Domain);
}

#[test]
fn the_boxing_level_counts_only_boxes() {
    assert_eq!(val(Lang::J, "L. 1 2 3"), Array::scalar_i64(0));
    assert_eq!(val(Lang::J, "L. 'abc'"), Array::scalar_i64(0));
    assert_eq!(val(Lang::J, "L. <3"), Array::scalar_i64(1));
    assert_eq!(val(Lang::J, "L. (1;(<<2))"), Array::scalar_i64(2));
    // J's `L.` and APL's `≡` differ on a simple array by exactly one: the
    // depth counts the array, the level counts only the boxing.
    assert_eq!(val(Lang::Apl, "≡'abc'"), Array::scalar_i64(1));
}

// --- primes ---------------------------------------------------------------

#[test]
fn the_prime_queries_answer_what_their_left_argument_asks() {
    assert_eq!(val(Lang::J, "_1 p: 7"), Array::scalar_i64(3));
    assert_eq!(val(Lang::J, "1 p: 7"), Array::scalar_bool(true));
    assert_eq!(val(Lang::J, "0 p: 7"), Array::scalar_bool(false));
    assert_eq!(val(Lang::J, "4 p: 10"), Array::scalar_i64(11));
    assert_eq!(val(Lang::J, "_4 p: 100"), Array::scalar_i64(97));
    assert_eq!(val(Lang::J, "2 p: 10"), i64s(&[2, 2], &[2, 5, 1, 1]));
    assert_eq!(val(Lang::J, "3 p: 10"), i64s(&[2], &[2, 5]));
    // The exponents of the first x primes, and the whole table for `__`.
    assert_eq!(val(Lang::J, "3 q: 12"), i64s(&[3], &[2, 1, 0]));
    assert_eq!(val(Lang::J, "__ q: 60"), i64s(&[2, 3], &[2, 3, 5, 2, 1, 1]));
    // A left argument that names no query at all.
    let e = err(Lang::J, "5 p: 10");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("prime query"), "{}", e.msg);
    // A negative left argument keeps the LAST that many columns of the
    // table, and asking for more than there are keeps the whole of it.
    assert_eq!(val(Lang::J, "_1 q: 12"), i64s(&[2, 1], &[3, 1]));
    assert_eq!(val(Lang::J, "_2 q: 360"), i64s(&[2, 2], &[3, 5, 2, 1]));
    assert_eq!(val(Lang::J, "_9 q: 360"), i64s(&[2, 3], &[2, 3, 5, 3, 2, 1]));
}

// --- under and the obverse -----------------------------------------------

#[test]
fn under_prepares_applies_and_puts_back() {
    // `u&.v y` is `v^:_1 u v y`, and `&.:` is the same on whole arguments.
    assert_eq!(val(Lang::J, "+/&.:*: 3 4"), Array::scalar_f64(5.0));
    assert_eq!(val(Lang::J, "|.&.|. 1 2 3"), i64s(&[3], &[3, 2, 1]));
    assert_eq!(val(Lang::J, "'ab' ,&.|. 'cd'"), text(&[4], "cdab"));
    // A right operand with no known obverse names itself.
    let e = err(Lang::J, "+ &. (+/ % #) 1 2");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("obverse of"), "{}", e.msg);
}

#[test]
fn a_negative_power_runs_the_obverse() {
    assert_eq!(val(Lang::J, "+&2 ^:_1 ] 5"), Array::scalar_i64(3));
    assert_eq!(val(Lang::J, ">:^:_2 ] 5"), Array::scalar_i64(3));
    assert_eq!(val(Lang::J, "<^:_1 ] <3"), Array::scalar_i64(3));
    // A composition inverts by inverting its parts in the other order.
    // The root that undoes a square is always float, as `%:` is.
    assert_eq!(val(Lang::J, "(*:@:>:)^:_1 ] 9"), Array::scalar_f64(2.0));
    let e = err(Lang::J, "(+/ % #) ^:_1 ] 3");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("obverse of"), "{}", e.msg);
}

#[test]
fn an_obverse_can_be_declared_where_the_table_has_none() {
    // `u :. v` says what undoes u; nothing else about u changes.
    assert_eq!(val(Lang::J, "((+&2) :. (-&2)) 5"), Array::scalar_i64(7));
    assert_eq!(val(Lang::J, "((+&2) :. (-&2)) ^:_1 ] 5"), Array::scalar_i64(3));
    // Conjunctions bind left to right, so the operands need their own
    // parentheses — J reads the unbracketed form as a valence error too.
    assert_eq!(err(Lang::J, "(*&2 :. %&2) 5").kind, ErrorKind::Domain);
    assert_eq!(val(Lang::J, "3 (+ &. ((*&2) :. (%&2))) 4"), Array::scalar_f64(7.0));
}

// --- gerunds, agenda and the adverse -------------------------------------

#[test]
fn agenda_picks_one_verb_of_a_gerund() {
    assert_eq!(val(Lang::J, "1 (+`-`*)@.(0) 2"), Array::scalar_i64(3));
    assert_eq!(val(Lang::J, "1 (+`-`*)@.(2) 2"), Array::scalar_i64(2));
    // A verb on the right chooses at run time, from the arguments.
    assert_eq!(val(Lang::J, "(<.`>.)@.(2&<) 5"), Array::scalar_i64(5));
    assert_eq!(val(Lang::J, "(<.`>.)@.(2&<) 1.5"), Array::scalar_i64(1));
    // An index past the last verb says how many there were.
    let e = err(Lang::J, "(+`-)@.(2:) 5");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("2 verbs"), "{}", e.msg);
    // The other gerund conjunction reads the same data (see wave7.rs).
    assert_eq!(val(Lang::J, "((+`-) `: 6) 5"), Array::scalar_i64(0));
}

#[test]
fn the_adverse_answers_a_refusal_with_the_other_verb() {
    assert_eq!(val(Lang::J, "(+/ :: 0) 'abc'"), bits(&[], &[0]));
    assert_eq!(val(Lang::J, "(+/ :: 0) 1 2 3"), Array::scalar_i64(6));
    // A gap in libjay is a promise, not an error the program may handle.
    let e = err(Lang::J, "(2&s: :: 0) 'a b'");
    assert_eq!(e.kind, ErrorKind::NotYet);
}

// --- execute --------------------------------------------------------------

#[test]
fn execute_runs_the_string_over_the_names_around_it() {
    assert_eq!(val(Lang::J, "\". '2+2'"), Array::scalar_i64(4));
    assert_eq!(val(Lang::Apl, "⍎'2+2'"), Array::scalar_i64(4));
    // The names the sentence can see are the ones the string can see, in
    // both directions.
    assert_eq!(val(Lang::J, "a =. 3\n\". 'a + 1'"), Array::scalar_i64(4));
    assert_eq!(val(Lang::J, "z =. \". 'b =. 7'\nb * 2"), Array::scalar_i64(14));
    assert_eq!(val(Lang::Apl, "A←3 ⋄ ⍎'A+1'"), Array::scalar_i64(4));
}

#[test]
fn execute_reports_the_inner_diagnostic_at_the_outer_sentence() {
    // A bare unbound name is the empty in J, so the name has to be doing
    // something for the value error to reach the executing sentence.
    let e = err(Lang::J, "\". 'zz 3'");
    assert_eq!(e.kind, ErrorKind::Value);
    assert!(e.msg.contains("in the executed string"), "{}", e.msg);
    // The inner diagnostic still reads in full, as a note: its own spans
    // point into a source the caller never sees.
    assert!(e.notes.iter().any(|n| n.contains("zz")), "{:?}", e.notes);
    // The span points at the sentence that ran the string.
    assert!(e.span.is_some());
    // Host data has nothing to bind to inside an executed string.
    let e = err(Lang::J, "\". '2 + {x}'");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("host data"), "{}", e.msg);
    assert_eq!(err(Lang::J, "\". 1 2 3").kind, ErrorKind::Domain);
}

// --- APL: what GNU APL has no answer for ---------------------------------

/// GNU APL has no monadic `↓`; this is Dyalog's split — the vectors along
/// the last axis, each enclosed, laid out in the remaining axes' shape.
#[test]
fn apl_split_follows_dyalog() {
    let split = val(Lang::Apl, "↓2 3⍴⍳6");
    assert_eq!(split.shape, vec![2]);
    assert_eq!(val(Lang::Apl, "1⊃↓2 3⍴⍳6"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(val(Lang::Apl, "2⊃↓2 3⍴⍳6"), i64s(&[3], &[4, 5, 6]));
    // A vector splits into one enclosure, which is a scalar.
    assert_eq!(val(Lang::Apl, "⍴↓1 2 3"), i64s(&[0], &[]));
    assert_eq!(val(Lang::Apl, "≡↓1 2 3"), Array::scalar_i64(2));
}

/// Neither `∘` (beside) nor `⍥` (over) is in GNU APL, so both follow
/// Dyalog: beside prepares the right argument only, over prepares both.
#[test]
fn apl_beside_and_over_follow_dyalog() {
    assert_eq!(val(Lang::Apl, "1 2 3+∘×1 2 3"), i64s(&[3], &[2, 3, 4]));
    assert_eq!(val(Lang::Apl, "(⌽∘⍳) 5"), i64s(&[5], &[5, 4, 3, 2, 1]));
    assert_eq!(val(Lang::Apl, "2 -⍥| ¯5"), Array::scalar_i64(-3));
    assert_eq!(val(Lang::Apl, "+/∘⍳ 5"), Array::scalar_i64(15));
    // A LITERAL array binds where an operand belongs, on either side.
    assert_eq!(val(Lang::Apl, "2∘× 5"), Array::scalar_i64(10));
    assert_eq!(val(Lang::Apl, "(1∘-) 10"), Array::scalar_i64(-9));
    assert_eq!(val(Lang::Apl, "(-∘1) 10"), Array::scalar_i64(9));
    assert_eq!(val(Lang::Apl, "(1∘↓)⍣2⊢1 2 3 4 5"), i64s(&[3], &[3, 4, 5]));
    // A computed one is a separate gap, and says so.
    let e = err(Lang::Apl, "(⍳3)∘×2");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("value operand"), "{}", e.msg);
}

/// GNU APL has no monadic `⌷`; Dyalog's materialise is the identity on an
/// array, which is what it is here.
#[test]
fn apl_monadic_index_materialises() {
    assert_eq!(val(Lang::Apl, "⌷1 2 3"), i64s(&[3], &[1, 2, 3]));
    assert_eq!(val(Lang::Apl, "⌷2 3⍴⍳6"), val(Lang::Apl, "2 3⍴⍳6"));
}

#[test]
fn apl_system_names_are_read_only_and_pure() {
    // GNU APL has no `⎕A` or `⎕D`; these are the ISO/Dyalog constants.
    assert_eq!(val(Lang::Apl, "⍴⎕A"), i64s(&[1], &[26]));
    assert_eq!(val(Lang::Apl, "3↑⎕A"), text(&[3], "ABC"));
    assert_eq!(val(Lang::Apl, "⎕D"), text(&[10], "0123456789"));
    // `⎕IO` and `⎕CT` report the dialect the compiler was given.
    assert_eq!(val(Lang::Apl, "⎕IO"), Array::scalar_i64(1));
    assert_eq!(
        run_dialect(Lang::Apl, "⎕IO", &Dialect { index_origin: Some(0), ..Dialect::default() }),
        Some(Array::scalar_i64(0))
    );
    assert_eq!(val(Lang::Apl, "⎕CT"), Array::scalar_f64(1e-13));
    // Neither can be assigned: the dialect fixed them before the run.
    // That is permanent, so it is not spelled as a promise.
    let e = err(Lang::Apl, "⎕IO←0");
    assert_eq!(e.kind, ErrorKind::Language);
    assert!(e.msg.contains("read-only"), "{}", e.msg);
    // The ones that would reach a clock or a filesystem are closed.
    for src in ["⎕TS", "⎕AI", "⎕FIO"] {
        let e = err(Lang::Apl, src);
        assert_eq!(e.kind, ErrorKind::Sandbox, "{src}");
        assert_eq!(e.kind.label(), "closed by the sandbox");
        assert!(e.msg.contains("outside the program"), "{src}: {}", e.msg);
    }
    // An unknown one is named rather than guessed at.
    let e = err(Lang::Apl, "⎕ZZ");
    assert!(e.msg.contains("⎕ZZ"), "{}", e.msg);
}

#[test]
fn apl_expand_and_pick() {
    assert_eq!(val(Lang::Apl, "1 0 1\\1 2"), i64s(&[3], &[1, 0, 2]));
    assert_eq!(val(Lang::Apl, "1 0 0 1\\'ab'"), text(&[4], "a  b"));
    // A mask that takes a different number of items than there are.
    let e = err(Lang::Apl, "1 1 1\\1 2");
    assert_eq!(e.kind, ErrorKind::Length);
    assert_eq!(err(Lang::Apl, "2 0\\1 2").kind, ErrorKind::Domain);
    // Pick follows a path; a boxed step is one whole coordinate vector.
    assert_eq!(val(Lang::Apl, "2⊃(1 2)(3 4)"), i64s(&[2], &[3, 4]));
    assert_eq!(val(Lang::Apl, "(⊂1 2)⊃2 3⍴⍳6"), Array::scalar_i64(2));
    assert_eq!(err(Lang::Apl, "9⊃(1 2)(3 4)").kind, ErrorKind::Domain);
}

// --- the rest of the wave -------------------------------------------------

#[test]
fn the_outfix_leaves_out_a_run_of_items() {
    assert_eq!(val(Lang::J, "2 +/\\. i. 5"), i64s(&[4], &[9, 7, 5, 3]));
    assert_eq!(val(Lang::J, "3 -/\\. i. 5"), i64s(&[3], &[-1, -4, -1]));
    // A width of zero leaves everything in, once per position.
    assert_eq!(val(Lang::J, "0 +/\\. 1 2 3"), i64s(&[4], &[6, 6, 6, 6]));
    // A width longer than the argument leaves out no run at all, so there
    // are no results — not an error.
    assert_eq!(val(Lang::J, "$ 9 +/\\. 1 2 3"), i64s(&[1], &[0]));
    // A NEGATIVE width leaves out non-overlapping runs, the last short.
    assert_eq!(val(Lang::J, "_2 +/\\. i. 5"), i64s(&[3], &[9, 5, 6]));
    assert_eq!(val(Lang::J, "_3 +/\\. i. 5"), i64s(&[2], &[7, 3]));
    // Nothing is left in: the answer is one copy of the identity, boolean.
    assert_eq!(val(Lang::J, "_7 +/\\. i. 5"), bits(&[1], &[0]));
}

#[test]
fn constant_verbs_yield_their_noun_in_both_valences() {
    assert_eq!(val(Lang::J, "3: 5"), Array::scalar_i64(3));
    assert_eq!(val(Lang::J, "2 3: 4"), Array::scalar_i64(3));
    assert_eq!(val(Lang::J, "_9: i. 2 3"), Array::scalar_i64(-9));
    assert_eq!(val(Lang::J, "_: 5"), Array::scalar_f64(f64::INFINITY));
    // A definition still reads `3 : '...'` with a space between the words.
    assert_eq!(val(Lang::J, "f =. 3 : 'y + 1'\nf 5"), Array::scalar_i64(6));
}

#[test]
fn the_alphabet_and_the_ace() {
    assert_eq!(val(Lang::J, "$ a."), i64s(&[1], &[256]));
    assert_eq!(val(Lang::J, "65 { a."), text(&[], "A"));
    assert_eq!(val(Lang::J, "a. i. 'A'"), Array::scalar_i64(65));
    // The ace is one box holding an empty numeric list.
    assert_eq!(val(Lang::J, "$ a:"), i64s(&[0], &[]));
    assert_eq!(val(Lang::J, "L. a:"), Array::scalar_i64(1));
    assert_eq!(val(Lang::J, "$ > a:"), i64s(&[1], &[0]));
}

#[test]
fn catenation_fills_in_j_and_conforms_in_apl() {
    assert_eq!(
        val(Lang::J, "1 2 3 , i. 2 2"),
        i64s(&[3, 3], &[1, 2, 3, 0, 1, 0, 2, 3, 0])
    );
    assert_eq!(val(Lang::J, "'ab' , 'cde'"), text(&[5], "abcde"));
    // APL's conformability rule refuses what J fills, as GNU APL does.
    let e = err(Lang::Apl, "(2 2⍴⍳4) ⍪ 1 2 3");
    assert_eq!(e.kind, ErrorKind::Length);
}

/// `f⍣¯n` runs f's inverse n times, over the same obverse table J's
/// `u^:_n` reads. A verb with no inverse names itself rather than
/// answering wrongly.
#[test]
fn apl_inverse_powers_run_the_obverse() {
    assert_eq!(val(Lang::Apl, "⌽⍣¯1⊢1 2 3"), i64s(&[3], &[3, 2, 1]));
    assert_eq!(val(Lang::Apl, "(1∘+)⍣¯1⊢5"), Array::scalar_i64(4));
    // The inverse of a bonded times is a bonded divide, so the answer is
    // the quotient it computes rather than the integer it prints as.
    assert_eq!(val(Lang::Apl, "(2∘×)⍣¯1⊢8").to_f64_vec(), Some(vec![4.0]));
    let e = err(Lang::Apl, "⍴⍣¯1⊢5");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("obverse"), "{}", e.msg);
}
