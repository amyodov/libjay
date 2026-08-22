//! The dialect object: the settings a host supplies, and the one place
//! each of them is read.
//!
//! libjay ships the APL2/ISO line GNU APL embodies, and every point where
//! the APL lineages diverge is a setting on `Dialect` rather than a
//! constant in the parser. These tests pin three things: that the shipped
//! dialect is the one the rest of the suite runs under, that the settings
//! `Dialect::dyalog()` names are really read, and that a setting libjay
//! does not implement is refused as a gap rather than answered with this
//! dialect's meaning.

use jay::frontend::{
    ComplexOrder, DefaultArg, DepthSign, DfnResult, Dialect, FirstDisclose, IndexForm, LookupLeft,
    NestedGrade, NestedModel, Partition,
};
use jay::{compile, Array, Data, ErrorKind, Lang};

fn run(src: &str, dialect: &Dialect) -> Option<Array> {
    let program = compile(Lang::Apl, src, dialect)
        .unwrap_or_else(|e| panic!("compile of {src:?} failed:\n{}", e.render(src)));
    let mut sink = |_: &str| {};
    program
        .run(&[], &mut sink)
        .unwrap_or_else(|e| panic!("run of {src:?} failed:\n{}", program.render_error(&e)))
}

fn val(src: &str, dialect: &Dialect) -> Array {
    run(src, dialect).unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn zero_origin() -> Dialect {
    Dialect { index_origin: Some(0), ..Dialect::default() }
}

fn i64s(shape: &[usize], values: &[i64]) -> Array {
    Array::new(shape.to_vec(), Data::I64(values.to_vec().into()))
}

#[test]
fn the_shipped_dialect_is_the_default_one() {
    // The preset writes out every setting; the default derives them. They
    // are the same dialect, which is what makes `Dialect::default()` the
    // APL the rest of the suite and the corpus are recorded under.
    assert_eq!(Dialect::gnu_apl(), Dialect::default());
    assert_eq!(Dialect::j(), Dialect::default());
}

#[test]
fn the_defaults_resolve_to_this_apl() {
    let r = Dialect::default().rules(Lang::Apl).expect("the shipped dialect is implemented");
    assert_eq!(r.origin, 1);
    assert_eq!(r.ct, 1e-13);
    assert_eq!(r.nested_model, NestedModel::Floating);
    assert_eq!(r.first_disclose, FirstDisclose::UpIsFirst);
    assert_eq!(r.index_form, IndexForm::ScalarPerAxis);
    assert_eq!(r.partition, Partition::Flags);
    assert_eq!(r.depth_sign, DepthSign::Unsigned);
    assert_eq!(r.dfn_result, DfnResult::LastSentence);
    assert_eq!(r.default_arg, DefaultArg::Eager);
    assert_eq!(r.complex_order, ComplexOrder::RealThenImaginary);
    assert_eq!(r.nested_grade, NestedGrade::Apl2);
    assert_eq!(r.lookup_left, LookupLeft::AnyRank);
    // Trains ship on, as an extension: GNU APL has none, and both readings
    // are implemented, so the setting is a choice rather than a gap.
    assert!(r.trains);
    // J counts from zero and has a tolerance of its own.
    let j = Dialect::default().rules(Lang::J).expect("J's defaults are implemented");
    assert_eq!(j.origin, 0);
    assert!(j.tol().is_j());
}

/// The Dyalog preset, sentence by sentence: every setting it names, asked
/// in the language rather than of the object, and the shipped dialect's
/// answer beside it. The values are the recorded Dyalog ones — the
/// `dyalog:` column of the snapshots is what pins them.
#[test]
fn the_dyalog_preset_answers_the_dyalog_way() {
    let dy = Dialect::dyalog();
    let gnu = Dialect::default();
    // `↑` mixes and `⊃` takes the first: the lineages' clearest fork.
    assert_eq!(val("↑1 2 3", &dy), i64s(&[3], &[1, 2, 3]));
    assert_eq!(val("↑1 2 3", &gnu), Array::scalar_i64(1));
    assert_eq!(val("↑(1 2)(3 4)", &dy), i64s(&[2, 2], &[1, 2, 3, 4]));
    assert_eq!(val("⊃(1 2)(3 4)", &dy), i64s(&[2], &[1, 2]));
    assert_eq!(val("⊃(1 2)(3 4)", &gnu), i64s(&[2, 2], &[1, 2, 3, 4]));
    assert_eq!(val("⊃2 3⍴⍳6", &dy), Array::scalar_i64(1));
    // `⌷` names the leading axes, and an enclosed index keeps its own.
    assert_eq!(val("2⌷3 3⍴⍳9", &dy), i64s(&[3], &[4, 5, 6]));
    assert_eq!(val("(⊂2 3)⌷3 3⍴⍳9", &dy), i64s(&[2, 3], &[4, 5, 6, 7, 8, 9]));
    // The APL2 reading names every axis, so the same sentence is a rank
    // error there.
    let named = compile(Lang::Apl, "2⌷3 3⍴⍳9", &gnu).expect("it compiles in either reading");
    let mut sink = |_: &str| {};
    assert_eq!(
        named.run(&[], &mut sink).expect_err("one index for two axes").kind,
        ErrorKind::Rank
    );
    // `⎕CT` is a tenth of GNU APL's, which two numbers a tenth apart show.
    assert_eq!(val("⎕CT", &dy), Array::scalar_f64(1e-14));
    assert_eq!(val("1=1+1E¯13", &dy), Array::scalar_bool(false));
    assert_eq!(val("1=1+1E¯13", &gnu), Array::scalar_bool(true));
    assert_eq!(val("⌊2.9999999999999", &dy), Array::scalar_i64(2));
    // A dfn answers with its first sentence that is not an assignment.
    assert_eq!(val("F←{⍵+1 ⋄ ⍵+2} ⋄ F 5", &dy), Array::scalar_i64(6));
    assert_eq!(val("F←{⍵+1 ⋄ ⍵+2} ⋄ F 5", &gnu), Array::scalar_i64(7));
    // A guard still decides, and an assignment ahead of the answer still
    // runs: neither is the sentence that answers.
    assert_eq!(val("F←{⍵>2:⍵ ⋄ 0} ⋄ F 5", &dy), Array::scalar_i64(5));
    assert_eq!(val("F←{t←⍵×2 ⋄ t+1 ⋄ 99} ⋄ F 5", &dy), Array::scalar_i64(11));
    // Dyadic `⊂` counts partitions rather than flagging where one starts.
    assert_eq!(val("≢1 0 1⊂1 2 3", &dy), Array::scalar_i64(2));
    assert_eq!(val("⊃1 0 1⊂1 2 3", &dy), i64s(&[2], &[1, 2]));
    assert_eq!(val("≢1 0 1⊂1 2 3", &gnu), Array::scalar_i64(2));
    assert_eq!(val("↑1 0 1⊂1 2 3", &gnu), i64s(&[1], &[1]));
    // `⊆` is the partition in both readings.
    assert_eq!(val("↑1 0 1⊆1 2 3", &gnu), i64s(&[1], &[1]));
    assert_eq!(val("⊃1 0 1⊆1 2 3", &dy), i64s(&[1], &[1]));
    // `≡` reports a depth its items do not share as a negative.
    assert_eq!(val("≡1(2(3 4))", &dy), Array::scalar_i64(-3));
    assert_eq!(val("≡1(2(3 4))", &gnu), Array::scalar_i64(3));
    assert_eq!(val("≡(1 2),⊂3 4", &dy), Array::scalar_i64(-2));
    // Items of one depth and different lengths are uniform all the same.
    assert_eq!(val("≡1 2∘.⍴3 4", &dy), Array::scalar_i64(2));
    // A nested grade is the total array ordering: numbers before
    // characters, and a shorter array before what extends it.
    assert_eq!(val("⍋(1 2)('ab')(⊂1 2)(1)('a')", &dy), i64s(&[5], &[4, 1, 3, 5, 2]));
    assert_eq!(val("⍋(1 2)('ab')(⊂1 2)(1)('a')", &gnu), i64s(&[5], &[5, 4, 3, 2, 1]));
    assert_eq!(val("⍋1 'a'", &dy), i64s(&[2], &[1, 2]));
    assert_eq!(val("⍋(1 4⍴0)(2 3⍴0)", &dy), i64s(&[2], &[2, 1]));
    // Two arrays with no atoms are separated by the item they WOULD have
    // held: an empty nested array's prototype against a simple empty's
    // fill. `0⍴⊂1 2` remembers a pair of numbers, so it sorts after the
    // empty numeric vector and before the empty character one.
    assert_eq!(val("⍋(0⍴⊂1 2)(⍳0)", &dy), i64s(&[2], &[2, 1]));
    assert_eq!(val("⍋(0⍴⊂1 2)('')", &dy), i64s(&[2], &[1, 2]));
    // Dyadic `⍳` looks up in a VECTOR and nothing else; the APL2 line
    // searches the items of a left argument of any rank.
    assert_eq!(val("'abc'⍳'b'", &dy), Array::scalar_i64(2));
    for src in ["(2 3⍴⍳6)⍳5", "5⍳6"] {
        let p = compile(Lang::Apl, src, &dy).expect("it compiles in either reading");
        let mut sink = |_: &str| {};
        assert_eq!(
            p.run(&[], &mut sink).expect_err("a vector is the only left argument").kind,
            ErrorKind::Rank,
            "{src}"
        );
        run(src, &gnu).unwrap_or_else(|| panic!("{src} answers under the APL2 reading"));
    }
}

/// Every setting whose other reading libjay does not implement, asked for.
/// Each has to be refused as a gap rather than answered with this
/// dialect's meaning.
#[test]
fn asking_for_the_other_reading_is_refused_as_a_gap() {
    let cases: Vec<(&str, Dialect)> = vec![
        ("nested_model", Dialect { nested_model: NestedModel::Grounded, ..Dialect::default() }),
        ("default_arg", Dialect { default_arg: DefaultArg::Lazy, ..Dialect::default() }),
        (
            "complex_order",
            Dialect { complex_order: ComplexOrder::MagnitudeThenAngle, ..Dialect::default() },
        ),
    ];
    for (name, dialect) in cases {
        let e = compile(Lang::Apl, "1 2 3", &dialect)
            .err()
            .unwrap_or_else(|| panic!("{name} was accepted"));
        assert_eq!(e.kind, ErrorKind::NotYet, "{name}: {}", e.msg);
        // A promise, not a permanent refusal: the message says so, and
        // names the reading that was asked for.
        assert!(e.msg.contains("not supported yet"), "{name}: {}", e.msg);
    }
}

/// `trains` is the one setting whose two readings are both implemented:
/// turning it off is asking for the strict GNU/APL2 sentence, where a run
/// of functions is a syntax error and `F←+/` names nothing.
#[test]
fn trains_can_be_turned_off_for_the_strict_reading() {
    let strict = Dialect { trains: false, ..Dialect::default() };
    assert_eq!(val("(+/÷≢)1 2 3 4", &Dialect::default()), Array::scalar_f64(2.5));
    for src in ["(+/÷≢)1 2 3 4", "F←+/÷≢"] {
        let e = compile(Lang::Apl, src, &strict).expect_err("the strict reading has no trains");
        assert_ne!(e.kind, ErrorKind::Internal, "{src}: {}", e.msg);
    }
    let e = compile(Lang::Apl, "F←+/", &strict).expect_err("the strict reading names no function");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("function assignment"), "{}", e.msg);
}

#[test]
fn an_impossible_tolerance_is_a_domain_error() {
    let d = Dialect { comparison_tolerance: Some(-1.0), ..Dialect::default() };
    let e = compile(Lang::Apl, "1", &d).expect_err("a negative tolerance is no tolerance");
    assert_eq!(e.kind, ErrorKind::Domain);
}

#[test]
fn quad_io_and_quad_ct_report_the_dialect() {
    assert_eq!(val("⎕IO", &Dialect::default()), Array::scalar_i64(1));
    assert_eq!(val("⎕IO", &zero_origin()), Array::scalar_i64(0));
    assert_eq!(val("⎕CT", &Dialect::default()), Array::scalar_f64(1e-13));
    let d = Dialect { comparison_tolerance: Some(1e-10), ..Dialect::default() };
    assert_eq!(val("⎕CT", &d), Array::scalar_f64(1e-10));
    // And the tolerance is the one comparisons use, not a number to read.
    assert_eq!(val("1=1+1e¯11", &d), Array::scalar_bool(true));
    assert_eq!(val("1=1+1e¯11", &Dialect::default()), Array::scalar_bool(false));
}

#[test]
fn an_executed_string_runs_under_the_whole_dialect() {
    // `⍎` compiles a nested program; it gets the dialect the caller was
    // compiled with, which is visible in `⎕IO` and in `⎕CT` alike.
    assert_eq!(val("⍎'⎕IO'", &zero_origin()), Array::scalar_i64(0));
    assert_eq!(val("⍎'⍳3'", &zero_origin()), i64s(&[3], &[0, 1, 2]));
    let d = Dialect { comparison_tolerance: Some(1e-10), ..Dialect::default() };
    assert_eq!(val("⍎'⎕CT'", &d), Array::scalar_f64(1e-10));
}

#[test]
fn the_index_origin_reaches_the_verbs_that_answer_with_positions() {
    // The key operator answers with positions, and they count from `⎕IO`
    // like every other position does.
    assert_eq!(val("{⍵}⌸'aaa'", &Dialect::default()), i64s(&[1, 3], &[1, 2, 3]));
    assert_eq!(val("{⍵}⌸'aaa'", &zero_origin()), i64s(&[1, 3], &[0, 1, 2]));
}

#[test]
fn a_control_structures_sentence_reads_the_dialect() {
    // `:For x :In ⍳3` is parsed apart from the sentences around it; the
    // origin it counts from is still the dialect's.
    let src = "∇r←f\n r←0\n :For i :In ⍳3\n  r←r+i\n :EndFor\n∇\nf";
    assert_eq!(val(src, &Dialect::default()), Array::scalar_i64(6));
    assert_eq!(val(src, &zero_origin()), Array::scalar_i64(3));
}
