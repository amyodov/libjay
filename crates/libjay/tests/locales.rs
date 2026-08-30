//! J's locales and its remaining control words, end to end.
//!
//! What the reference answers is in `tests/corpus/j/locales.txt` and
//! `tests/corpus/j/controlwords.txt`, replayed by `oracle.rs`. This file
//! carries the rest: the diagnostics, the gaps that name themselves, and
//! the rules whose evidence is a refusal rather than a value.

use jay::{compile, Array, Dialect, ErrorKind, Lang};
use rstest::rstest;

fn run(src: &str) -> Option<Array> {
    let program = compile(Lang::J, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile failed:\n{}", e.render(src)));
    let mut out = String::new();
    program
        .run(&[], &mut |s| out.push_str(s))
        .unwrap_or_else(|e| panic!("run failed:\n{}", program.render_error(&e)))
}

fn ints(src: &str) -> Vec<i64> {
    let a = run(src).unwrap_or_else(|| panic!("{src:?} yielded no value"));
    a.to_i64_vec().unwrap_or_else(|| panic!("{src:?} is not integral: {a:?}"))
}

/// The error a program stops with, whether it was refused while it compiled
/// or while it ran.
fn refusal(src: &str) -> jay::Error {
    match compile(Lang::J, src, &Dialect::default()) {
        Err(e) => e,
        Ok(program) => {
            let mut out = String::new();
            match program.run(&[], &mut |s| out.push_str(s)) {
                Err(e) => e,
                Ok(v) => panic!("{src:?} was accepted and answered {v:?}"),
            }
        }
    }
}

// ------------------------------------------------------------ goto / label

#[rstest]
#[case("f =. 3 : 0\ngoto_nope.\n1\n)\nf 0", "label_nope.")]
#[case("f =. 3 : 0\ngoto_d.\nlabel_d.\n1\nlabel_d.\n2\n)\nf 0", "twice")]
#[case("f =. 3 : 0\ngoto_in.\nif. 1 do.\nlabel_in.\n7\nend.\n)\nf 0", "label_in.")]
fn a_branch_with_no_target_is_refused_where_it_is_written(
    #[case] src: &str,
    #[case] wanted: &str,
) {
    let e = refusal(src);
    assert_eq!(e.kind, ErrorKind::Parse, "{src:?}: {}", e.msg);
    assert!(e.msg.contains(wanted), "{src:?}: {}", e.msg);
}

#[test]
fn a_label_yields_nothing_of_its_own() {
    // The body's value is the 5 the sentence before the label produced,
    // not the empty value an untaken branch gives.
    assert_eq!(ints("f =. 3 : 0\n5\nlabel_a.\n)\nf 0"), vec![5]);
}

#[test]
fn a_branch_leaves_the_loop_it_stands_in() {
    let src = "f =. 3 : 0\nr =. 0\nfor_i. i.10 do.\n  r =. r + i\n  \
               if. i = 3 do. goto_done. end.\nend.\nr =. r + 100\nlabel_done.\nr\n)\nf 0";
    assert_eq!(ints(src), vec![6]);
}

// ------------------------------------------------------- throw / catcht

#[test]
fn an_uncaught_throw_stops_the_program() {
    let e = refusal("g =. 3 : 'throw.'\nf =. 3 : 'g y'\nf 0");
    assert_eq!(e.kind, ErrorKind::Throw, "{}", e.msg);
}

#[test]
fn a_catcht_does_not_catch_its_own_definitions_throw() {
    let e = refusal("f =. 3 : 0\ntry.\n  throw.\ncatcht.\n  'caught'\nend.\n)\nf 0");
    assert_eq!(e.kind, ErrorKind::Throw, "{}", e.msg);
}

#[test]
fn a_catcht_takes_a_throw_from_something_it_called() {
    let src = "g =. 3 : '1 throw.'\nf =. 3 : 0\ntry.\n  g y\ncatcht.\n  7\nend.\n)\nf 0";
    assert_eq!(ints(src), vec![7]);
}

#[test]
fn a_catch_never_takes_a_throw() {
    let e = refusal(
        "g =. 3 : 'throw.'\nf =. 3 : 0\ntry.\n  g y\ncatch.\n  1\nend.\n)\nf 0",
    );
    assert_eq!(e.kind, ErrorKind::Throw, "{}", e.msg);
}

#[test]
fn a_try_with_only_a_catcht_lets_an_ordinary_error_out() {
    let e = refusal("f =. 3 : 0\ntry.\n  13 + 'x'\ncatcht.\n  1\nend.\n)\nf 0");
    assert_eq!(e.kind, ErrorKind::Type, "{}", e.msg);
}

#[test]
fn a_gap_in_libjay_is_never_caught_by_catcht() {
    // The same rule `try. catch.` has: swallowing a promise would turn it
    // into a wrong answer.
    let e = refusal("f =. 3 : 0\ntry.\n  0 !: 1 ] 2\ncatcht.\n  1\nend.\n)\nf 0");
    assert!(
        matches!(e.kind, ErrorKind::NotYet | ErrorKind::Sandbox),
        "{:?}: {}",
        e.kind,
        e.msg
    );
}

// --------------------------------------------------------------- locales

#[test]
fn a_definition_reads_its_own_locale_not_the_callers() {
    assert_eq!(ints("V_aa_ =. 1\nV =. 999\nf_aa_ =. 3 : 'V'\nf_aa_ ''"), vec![1]);
}

#[test]
fn a_locative_on_the_left_is_a_global_wherever_it_stands() {
    assert_eq!(ints("f =. 3 : 'R_cc_ =. y'\nf 5\nR_cc_"), vec![5]);
}

#[test]
fn a_cocurrent_inside_a_definition_lasts_as_long_as_the_call() {
    let src = "p =: 3 : 0\ncocurrent 'pp'\nQ =: y\n1\n)\np 3\nQ_pp_ , 0 = # > 18!:5 ''";
    assert_eq!(ints(src), vec![3, 0]);
}

#[test]
fn a_names_locals_belong_to_no_locale() {
    // `y` and a `=.` name inside a body in locale `bb` are the frame's.
    assert_eq!(ints("f_bb_ =. 3 : 0\nt =. y + 1\nt + 1\n)\nf_bb_ 5"), vec![7]);
}

#[test]
fn a_numbered_locale_is_never_made_by_naming_a_name_in_it() {
    let e = refusal("V_5_ =: 3");
    assert_eq!(e.kind, ErrorKind::Value, "{}", e.msg);
    assert!(e.msg.contains("locale 5"), "{}", e.msg);
}

#[test]
fn the_search_path_is_followed_one_step_and_no_further() {
    let src = "Z_z_ =: 42\n(<'r1') 18!:2 <'r2'\n(<'z') 18!:2 <'r1'\ncocurrent 'r2'\nZ";
    let e = refusal(src);
    assert_eq!(e.kind, ErrorKind::Value, "{}", e.msg);
}

#[rstest]
#[case("18!:4 ''")]
#[case("18!:6 <'base'")]
fn the_two_locale_foreigns_the_reference_does_not_define_are_refused(#[case] src: &str) {
    let e = refusal(src);
    assert_eq!(e.kind, ErrorKind::Language, "{src:?}: {}", e.msg);
}

#[test]
fn a_locale_class_no_18_1_knows_is_a_domain_error() {
    let e = refusal("18!:1 ,2");
    assert_eq!(e.kind, ErrorKind::Domain, "{}", e.msg);
}

// ---------------------------------------------------- indirect locatives

#[test]
fn an_indirect_locative_reads_and_writes_the_locale_a_name_holds() {
    assert_eq!(ints("n =: <'bb'\nV_bb_ =: 111\nV__n"), vec![111]);
    assert_eq!(ints("n =: <'bb'\nW__n =: 9\nW_bb_"), vec![9]);
}

#[test]
fn an_indirect_locative_whose_name_holds_no_box_is_a_rank_error() {
    let e = refusal("n =: 'bb'\nV_bb_ =: 1\nV__n");
    assert_eq!(e.kind, ErrorKind::Rank, "{}", e.msg);
}

#[test]
fn a_verb_named_by_an_indirect_locative_is_the_one_the_locale_holds() {
    assert_eq!(ints("n =: <'cc'\ng_cc_ =: 3 : 'y * 3'\ng__n 4"), vec![12]);
    // The locale is read where the verb is APPLIED, so moving the name to
    // another locale moves which verb the same sentence runs.
    assert_eq!(
        ints("g_cc_ =: 3 : 'y * 3'\ng_dd_ =: 3 : 'y + 3'\nn =: <'dd'\ng__n 4"),
        vec![7]
    );
}

#[test]
fn a_verb_named_by_an_indirect_locative_that_no_locale_defines_is_a_gap() {
    let e = refusal("n =: <'cc'\nq__n 4");
    assert_eq!(e.kind, ErrorKind::NotYet, "{}", e.msg);
    assert!(e.msg.contains("indirect locative"), "{}", e.msg);
}
