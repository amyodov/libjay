//! J's foreigns, `m !: n`, end to end.
//!
//! What the reference answers is in `tests/corpus/j/foreigns.txt`, replayed
//! by `oracle.rs`. This file carries the rest: the refusals that divide the
//! family, the settings that have to take effect on what comes after them,
//! and the round trips a corpus line cannot show.

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

fn floats(src: &str) -> Vec<f64> {
    let a = run(src).unwrap_or_else(|| panic!("{src:?} yielded no value"));
    a.to_f64_vec().unwrap_or_else(|| panic!("{src:?} is not numeric: {a:?}"))
}

/// What a session would SHOW for the program — which is what a print
/// precision the program set has to reach.
fn text(src: &str) -> String {
    libjay_testkit::eval::eval(libjay_testkit::Lang::J, src, 0)
        .unwrap_or_else(|| panic!("{src:?} yielded no value"))
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

// ------------------------------------------------- what the division is

/// A foreign that would reach outside the expression is refused BY NAME,
/// and as a permanent refusal rather than a queue position.
#[rstest]
#[case("0!:0 <'s.ijs'", "0!:0")]
#[case("1!:1 <'/etc/hosts'", "1!:1")]
#[case("1!:21 <'f'", "1!:21")]
#[case("2!:5 <'HOME'", "2!:5")]
#[case("6!:0 ''", "6!:0")]
#[case("15!:0 ]0", "15!:0")]
fn a_foreign_that_reaches_outside_is_named_in_its_refusal(
    #[case] src: &str,
    #[case] named: &str,
) {
    let e = refusal(src);
    assert_eq!(e.kind, ErrorKind::Sandbox, "{src}: {}", e.msg);
    assert!(e.msg.contains(named), "{src}: {}", e.msg);
}

/// The interpreter's own machinery is not a promise either: libjay will
/// never have the reference's allocator or its debugger.
#[rstest]
#[case("7!:0 ''", "7!:0")]
#[case("7!:2 ] 1", "7!:2")]
#[case("13!:0 ] 1", "13!:0")]
#[case("9!:14 ''", "9!:14")]
#[case("9!:6 ''", "9!:6")]
fn the_interpreters_own_machinery_is_permanent(#[case] src: &str, #[case] named: &str) {
    let e = refusal(src);
    assert_eq!(e.kind, ErrorKind::Language, "{src}: {}", e.msg);
    assert!(e.msg.contains(named), "{src}: {}", e.msg);
}

// -------------------------------------------- 3!:1, 3!:2 and 3!:3

/// Every type the binary form covers goes out and comes back unchanged.
#[rstest]
#[case("1 0 1 1")]
#[case("i. 2 3")]
#[case("2.5 _1.5 1e300")]
#[case("'a literal'")]
#[case("2j3 4j5")]
#[case("1;2;<'ab'")]
#[case("<(1;2)")]
#[case("i. 0")]
#[case("2 3 4 $ i. 24")]
fn the_binary_form_round_trips(#[case] value: &str) {
    assert_eq!(text(&format!("3!:2 ] 3!:1 ] {value}")), text(&format!("] {value}")));
    assert_eq!(text(&format!("3!:0 ] 3!:2 ] 3!:1 ] {value}")), text(&format!("3!:0 ] {value}")));
    assert_eq!(text(&format!("$ 3!:2 ] 3!:1 ] {value}")), text(&format!("$ {value}")));
}

#[test]
fn the_hexadecimal_form_is_a_word_to_a_row() {
    assert_eq!(ints("$ 3!:3 ] 1 2"), vec![7, 16]);
    assert_eq!(text("3!:3 'ab'").lines().next(), Some("e300000000000000"));
}

#[rstest]
#[case("3!:2 'ab'", ErrorKind::Length)]
#[case("3!:2 ] 1 2 3", ErrorKind::Domain)]
#[case("3!:2 ] 8 $ a. {~ 0", ErrorKind::Domain)]
fn a_binary_form_that_is_not_one_is_refused(#[case] src: &str, #[case] kind: ErrorKind) {
    assert_eq!(refusal(src).kind, kind, "{src}");
}

/// The exact types are the one hole left in the family, and it says so.
#[rstest]
#[case("3!:1 ] 1r2")]
#[case("3!:1 ] 123456789012345678901x")]
fn the_exact_types_have_no_binary_form_yet(#[case] src: &str) {
    let e = refusal(src);
    assert_eq!(e.kind, ErrorKind::NotYet, "{src}: {}", e.msg);
    assert!(e.msg.contains("binary representation"), "{src}: {}", e.msg);
}

// ------------------------------------------------------ 3!:4 and 3!:5

/// Every width writes bytes that read back as the number that went in.
#[rstest]
#[case(1, "0 1 _1 32767 _32768")]
#[case(2, "0 1 _1 2147483647 _2147483648")]
#[case(3, "0 1 _1 9223372036854775807")]
fn a_byte_conversion_round_trips(#[case] width: i64, #[case] values: &str) {
    let src = format!("_{width} (3!:4) {width} (3!:4) {values}");
    let want: Vec<i64> = ints(&format!("] {values}"));
    assert_eq!(ints(&src), want, "{src}");
}

#[test]
fn the_unsigned_reading_differs_only_in_the_sign() {
    assert_eq!(ints("_2 (3!:4) 2 (3!:4) _1"), vec![-1]);
    assert_eq!(ints("_4 (3!:4) 2 (3!:4) _1"), vec![4294967295]);
}

#[test]
fn a_float_round_trips_at_the_wider_conversion() {
    let want = vec![0.1, -2.25, 1e300];
    assert_eq!(floats("_2 (3!:5) 2 (3!:5) 0.1 _2.25 1e300"), want);
    // Four bytes are a single, so only what a single holds comes back.
    assert_eq!(floats("_1 (3!:5) 1 (3!:5) 1.5 _2.25"), vec![1.5, -2.25]);
}

#[rstest]
#[case("0 (3!:4) 1", ErrorKind::Domain)]
#[case("5 (3!:4) 1", ErrorKind::Domain)]
#[case("1 (3!:4) 'ab'", ErrorKind::Domain)]
#[case("1 (3!:4) i. 2 3", ErrorKind::Rank)]
#[case("_1 (3!:4) a. {~ 1 2 3", ErrorKind::Length)]
#[case("3 (3!:5) 1.5", ErrorKind::Domain)]
fn a_byte_conversion_says_what_it_will_not_do(#[case] src: &str, #[case] kind: ErrorKind) {
    assert_eq!(refusal(src).kind, kind, "{src}");
}

// -------------------------------------------------------- the 4!: family

#[test]
fn a_name_has_one_class_at_a_time() {
    assert_eq!(ints("n =: 5\n4!:0 <'n'"), vec![0]);
    assert_eq!(ints("n =: 5\nn =: +/\n4!:0 <'n'"), vec![3]);
    assert_eq!(ints("n =: +/\nn =: 5\n4!:0 <'n'"), vec![0]);
    assert_eq!(ints("4!:0 <'never'"), vec![-1]);
    assert_eq!(ints("4!:0 <'not a name'"), vec![-2]);
}

#[test]
fn erasing_a_name_takes_its_class_away() {
    assert_eq!(ints("n =: 5\nq =. 4!:55 <'n'\n4!:0 <'n'"), vec![-1]);
    assert_eq!(ints("v =: +/\nq =. 4!:55 <'v'\n4!:0 <'v'"), vec![-1]);
    // Erasing a name that stood for nothing is not an error.
    assert_eq!(ints("4!:55 <'never'"), vec![1]);
}

#[test]
fn the_name_list_is_sorted_and_of_the_classes_asked_for() {
    assert_eq!(text("b =: 1\na =: 2\n4!:1 ] 0"), "+-+-+\n|a|b|\n+-+-+");
    assert_eq!(text("b =: 1\nv =: +/\n4!:1 ] 3"), "+-+\n|v|\n+-+");
}

// -------------------------------------------------------- the 5!: family

/// `5!:0` is the inverse of `5!:1`: what comes back applies as the entity
/// the name stood for.
#[rstest]
#[case("f =: +/ % #\nh =: (5!:1 <'f') 5!:0\nh 1 2 3 4", "2.5")]
#[case("f =: 3 &+\nh =: (5!:1 <'f') 5!:0\nh 4", "7")]
#[case("n =: 7\nk =: (5!:1 <'n') 5!:0\nk", "7")]
fn a_representation_reads_back_as_what_it_represents(#[case] src: &str, #[case] want: &str) {
    assert_eq!(text(src), want);
}

#[test]
fn the_three_representations_draw_the_same_tree() {
    assert_eq!(text("f =: +/ % #\n5!:5 <'f'"), "+/ % #");
    assert_eq!(text("f =: +/ % #\n5!:6 <'f'"), "(+/) % #");
    assert_eq!(text("f =: +/ % #\n5!:2 <'f'"), "+-----+-+-+\n|+-+-+|%|#|\n||+|/|| | |\n|+-+-+| | |\n+-----+-+-+");
}

#[test]
fn a_name_with_no_meaning_represents_itself() {
    assert_eq!(text("5!:5 <'never'"), "never");
    assert_eq!(text("5!:6 <'never'"), "never");
}

// -------------------------------------------------------- the 8!: family

#[test]
fn the_outside_worlds_minus_sign_is_a_hyphen() {
    assert_eq!(text("8!:2 ] _1.5"), "-1.5");
    assert_eq!(text("8!:2 ] _1.5 100.25"), "-1.5100.25");
}

#[test]
fn a_format_specification_sets_the_width_and_the_places() {
    assert_eq!(text("'8.2' 8!:2 ] 1.5"), "    1.50");
    assert_eq!(text("'3.0' 8!:2 ] 1.7"), "  2");
    // A width too narrow for the answer is filled with stars.
    assert_eq!(text("'2.5' 8!:2 ] 1.23456"), "**");
}

#[test]
fn the_family_formats_numbers_and_says_so() {
    let e = refusal("8!:2 'abc'");
    assert_eq!(e.kind, ErrorKind::NotYet, "{}", e.msg);
    assert!(e.msg.contains("8!:"), "{}", e.msg);
    assert_eq!(refusal("8!:2 ] 2j3").kind, ErrorKind::Domain);
}

// -------------------------------------------------------- the 9!: family

/// A setting that libjay honours has to take effect on what comes AFTER
/// it, in the same program.
#[test]
fn the_print_precision_holds_for_the_rest_of_the_program() {
    assert_eq!(text("% 3"), "0.333333");
    assert_eq!(text("q =. 9!:11 ] 3\n% 3"), "0.333");
    assert_eq!(text("q =. 9!:11 ] 10\n% 3"), "0.3333333333");
    assert_eq!(ints("q =. 9!:11 ] 3\n9!:10 ''"), vec![3]);
    // And nothing before it is changed by it.
    assert_eq!(ints("9!:10 ''"), vec![6]);
}

#[test]
fn the_comparison_tolerance_holds_for_the_rest_of_the_program() {
    assert_eq!(ints("1 = 1 + 1e_15"), vec![1]);
    assert_eq!(ints("q =. 9!:19 ] 0\n1 = 1 + 1e_15"), vec![0]);
    assert_eq!(floats("9!:18 ''"), vec![5.684341886080802e-14]);
    assert_eq!(floats("q =. 9!:19 ] 1e_12\n9!:18 ''"), vec![1e-12]);
}

#[rstest]
#[case("9!:11 ] 0")]
#[case("9!:11 ] 100")]
#[case("9!:19 ] 0.01")]
#[case("9!:19 ] _1e_14")]
fn a_setting_outside_its_range_is_refused(#[case] src: &str) {
    assert_eq!(refusal(src).kind, ErrorKind::Domain, "{src}");
}

// --------------------------------------------------------------- 128!:3

#[test]
fn the_crc_is_a_signed_thirty_two_bit_number() {
    assert_eq!(ints("128!:3 'abc'"), vec![891568578]);
    assert_eq!(ints("128!:3 ''"), vec![0]);
    assert_eq!(ints("128!:3 'The quick brown fox'"), vec![-1220184866]);
    assert_eq!(refusal("128!:3 ] 1 2 3").kind, ErrorKind::Domain);
}
