//! The dialect object: the settings a host supplies, and the one place
//! each of them is read.
//!
//! libjay implements one APL — the APL2/ISO line GNU APL embodies — and
//! every point where the APL lineages diverge is a setting on `Dialect`
//! rather than a constant in the parser. These tests pin two things: that
//! the shipped dialect is the one the rest of the suite runs under, and
//! that each setting is really read, by asking for the reading libjay does
//! not implement and getting a "not implemented yet" refusal for it.

use jay::frontend::{
    ComplexOrder, DefaultArg, DfnResult, Dialect, FirstDisclose, IndexForm, NestedGrade,
    NestedModel,
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
    assert_eq!(r.dfn_result, DfnResult::LastSentence);
    assert_eq!(r.default_arg, DefaultArg::Eager);
    assert_eq!(r.complex_order, ComplexOrder::RealThenImaginary);
    assert_eq!(r.nested_grade, NestedGrade::Apl2);
    // Trains ship on, as an extension: GNU APL has none, and both readings
    // are implemented, so the setting is a choice rather than a gap.
    assert!(r.trains);
    // J counts from zero and has a tolerance of its own.
    let j = Dialect::default().rules(Lang::J).expect("J's defaults are implemented");
    assert_eq!(j.origin, 0);
    assert!(j.tol().is_j());
}

/// Every setting whose other reading libjay does not implement, asked for.
/// Each has to be refused as a gap rather than answered with this
/// dialect's meaning.
#[test]
fn asking_for_the_other_reading_is_refused_as_a_gap() {
    let cases: Vec<(&str, Dialect)> = vec![
        ("nested_model", Dialect { nested_model: NestedModel::Grounded, ..Dialect::default() }),
        (
            "first_disclose",
            Dialect { first_disclose: FirstDisclose::UpIsMix, ..Dialect::default() },
        ),
        ("index_form", Dialect { index_form: IndexForm::AxisVectors, ..Dialect::default() }),
        (
            "dfn_result",
            Dialect { dfn_result: DfnResult::FirstNonAssignment, ..Dialect::default() },
        ),
        ("default_arg", Dialect { default_arg: DefaultArg::Lazy, ..Dialect::default() }),
        (
            "complex_order",
            Dialect { complex_order: ComplexOrder::MagnitudeThenAngle, ..Dialect::default() },
        ),
        (
            "nested_grade",
            Dialect { nested_grade: NestedGrade::TotalOrder, ..Dialect::default() },
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
