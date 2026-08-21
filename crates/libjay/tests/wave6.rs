//! End-to-end tests for the sixth coverage wave: APL trains and function
//! assignment, J's named adverbs and conjunctions, the dyadic level and
//! spread, and the hypergeometric series.
//!
//! The differential evidence for what GNU APL and jconsole can answer is in
//! tests/corpus/{j,apl}/wave6.txt, and the trains extension is pinned
//! against the oracle's refusal in tests/corpus/apl/divergences.txt. This
//! file carries what no oracle covers: the train shapes themselves — GNU
//! APL has none — the dialect setting that turns them off, and the gaps
//! this wave leaves named.

use jay::frontend::Dialect as Dial;
use jay::{compile, Array, Data, Dialect, ErrorKind, Lang};

fn run_with(lang: Lang, src: &str, dialect: &Dialect) -> Option<Array> {
    let program = compile(lang, src, dialect)
        .unwrap_or_else(|e| panic!("compile of {src:?} failed:\n{}", e.render(src)));
    let mut sink = |_: &str| {};
    program
        .run(&[], &mut sink)
        .unwrap_or_else(|e| panic!("run of {src:?} failed:\n{}", program.render_error(&e)))
}

fn val(lang: Lang, src: &str) -> Array {
    run_with(lang, src, &Dialect::default())
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

fn boxes(shape: &[usize], items: Vec<Array>) -> Array {
    Array::new(shape.to_vec(), Data::Box(items.into()))
}

fn apl(src: &str) -> Array {
    val(Lang::Apl, src)
}

fn j(src: &str) -> Array {
    val(Lang::J, src)
}

// --- APL trains -----------------------------------------------------------
//
// GNU APL has no trains at all, so none of this has an oracle: the shapes
// below are Dyalog's published rules, and the values are the ones the same
// tacit expressions produce in J, whose forks and atops these lower to.

#[test]
fn a_two_train_is_an_atop() {
    // `(g h) ⍵` is `g (h ⍵)`, and `⍺ (g h) ⍵` is `g (⍺ h ⍵)`.
    assert_eq!(apl("(⌽⍳)3"), i64s(&[3], &[3, 2, 1]));
    assert_eq!(apl("(≢⍳)4"), Array::scalar_i64(4));
    assert_eq!(apl("(-+)2 3"), i64s(&[2], &[-2, -3]));
    assert_eq!(apl("2(-+)3"), Array::scalar_i64(-5));
    assert_eq!(apl("(⌽∘⍳⊢)3"), i64s(&[3], &[3, 2, 1]));
}

#[test]
fn a_three_train_is_a_fork() {
    // `(f g h) ⍵` is `(f ⍵) g (h ⍵)`; the mean is the canonical one.
    assert_eq!(apl("(+/÷≢)1 2 3 4"), Array::scalar_f64(2.5));
    assert_eq!(apl("(⌈/-⌊/)3 1 4 1 5"), Array::scalar_i64(4));
    assert_eq!(apl("(⊢,+/)1 2 3"), i64s(&[4], &[1, 2, 3, 6]));
    // Dyadically every tine gets both arguments: `(⍺ f ⍵) g (⍺ h ⍵)`.
    assert_eq!(apl("1 2(+,-)3 4"), i64s(&[4], &[4, 6, -2, -2]));
    assert_eq!(apl("10(⌈,⌊)20"), i64s(&[2], &[20, 10]));
    // `⊢` and `⊣` are the identity tines, and they work in both valences.
    assert_eq!(apl("(⊣+⊢)3"), Array::scalar_i64(6));
    assert_eq!(apl("5(⊣+⊢)3"), Array::scalar_i64(8));
    assert_eq!(apl("5(⊢-⊣)3"), Array::scalar_i64(-2));
}

#[test]
fn a_longer_train_groups_from_the_right() {
    // Five tines are a fork over a fork: `(f ⍵) g ((h ⍵) j (k ⍵))`, so
    // the answer below is `(⌈/⍵) - (⌊/⍵) + ⍵`.
    assert_eq!(apl("(⌈/-⌊/+⊢)3 1 4"), i64s(&[3], &[0, 2, -1]));
    // Seven tines: three forks nested to the right.
    assert_eq!(apl("(+/,×/,⌈/,⌊/)1 2 3 4"), i64s(&[4], &[10, 24, 4, 1]));
    // An even count has no tine to fork the leftmost with, so it is an
    // atop over the rest: `(f (g h j))`.
    let e6 = apl("(*⊢+⊢×⊢)2").to_f64_vec().expect("a number")[0];
    assert!((e6 - std::f64::consts::E.powi(6)).abs() < 1e-9, "{e6}");
    assert_eq!(apl("(-⊢,⊢)1 2"), i64s(&[4], &[-1, -2, -1, -2]));
}

#[test]
fn a_trains_left_tine_may_be_a_value() {
    // `(A g h) ⍵` is `A g (h ⍵)`: the value stands where `f ⍵` would.
    assert_eq!(apl("(3+×)4"), Array::scalar_i64(4));
    assert_eq!(apl("(10×÷)4"), Array::scalar_f64(2.5));
    assert_eq!(apl("(1 2 3+×)4"), i64s(&[3], &[2, 3, 4]));
    // Dyadically the value still stands, and only the right tine sees ⍺.
    assert_eq!(apl("2(10-×)3"), Array::scalar_i64(4));
    // A value that is only known at run time is a named gap, as J's is.
    let e = err(Lang::Apl, "(A+×)4");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("computed value"), "{}", e.msg);
    // And a value cannot be atop'd, which is what an even count asks of it.
    let e = err(Lang::Apl, "(3+×÷)4");
    assert_eq!(e.kind, ErrorKind::Parse);
    assert!(e.msg.contains("left tine"), "{}", e.msg);
}

#[test]
fn a_train_is_a_function_wherever_one_belongs() {
    // An operator to the right of the closing parenthesis binds to the
    // train, exactly as it would to a primitive.
    assert_eq!(apl("(+/÷≢)¨(1 2)(3 4 5)"), Array::new(vec![2], Data::F64(vec![1.5, 4.0].into())));
    assert_eq!(apl("(-,+)/1 2 3"), i64s(&[4], &[2, -4, 0, 6]));
    assert_eq!(apl("(⌈/-⌊/)⍤1⊢2 3⍴3 1 4 1 5 9"), i64s(&[2], &[3, 8]));
    // A train alone is still a function, so it has no value to display.
    let e = err(Lang::Apl, "(+/÷≢)");
    assert_eq!(e.kind, ErrorKind::Parse);
}

#[test]
fn a_function_assignment_names_a_train() {
    assert_eq!(apl("F←+/\nF 1 2 3"), Array::scalar_i64(6));
    assert_eq!(apl("MEAN←+/÷≢\nMEAN 1 2 3 4"), Array::scalar_f64(2.5));
    assert_eq!(apl("R←⌈/-⌊/\nR 3 1 4 1 5"), Array::scalar_i64(4));
    // A named tacit function is a function everywhere one belongs.
    assert_eq!(
        apl("MEAN←+/÷≢\nMEAN¨(1 2)(3 4 5)"),
        Array::new(vec![2], Data::F64(vec![1.5, 4.0].into()))
    );
    // And it can be built from one already named.
    assert_eq!(apl("S←+/\nM←S÷≢\nM 1 2 3 4"), Array::scalar_f64(2.5));
    // A value assignment is still a value assignment.
    assert_eq!(apl("A←1 2 3\nA+1"), i64s(&[3], &[2, 3, 4]));
    assert_eq!(apl("A←⍳3\n+/A"), Array::scalar_i64(6));
    // Naming a function part-way through a larger sentence names nothing,
    // and says so rather than guessing.
    let e = err(Lang::Apl, "3+F←+/");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("inside a larger sentence"), "{}", e.msg);
}

#[test]
fn the_dialect_decides_whether_a_run_of_functions_is_a_train() {
    // Trains ship on, as an extension: GNU APL refuses both spellings, and
    // refusing a feature the oracle merely lacks serves nobody. The strict
    // reading is a setting away, and both readings are implemented.
    let strict = Dial { trains: false, ..Dial::default() };
    assert!(Dial::default().trains);
    for src in ["(+/÷≢)1 2 3 4", "(3+×)4", "F←+/÷≢"] {
        let e = compile(Lang::Apl, src, &strict).expect_err("the strict reading has no trains");
        assert_ne!(e.kind, ErrorKind::Internal, "{src}: {}", e.msg);
    }
    // `(f)` is still only grouping in either reading, and the operator to
    // its right still binds to it — that part GNU APL does have.
    assert_eq!(run_with(Lang::Apl, "(+)/1 2 3", &strict), Some(Array::scalar_i64(6)));
}

// --- J: named adverbs and conjunctions ------------------------------------

#[test]
fn assignment_names_an_adverb_or_a_conjunction() {
    assert_eq!(j("m =. /\n+ m 1 2 3"), Array::scalar_i64(6));
    assert_eq!(j("a =. ~\n+ a 3"), Array::scalar_i64(6));
    assert_eq!(j("c =. @\n(+/ c *:) 1 2 3"), i64s(&[3], &[1, 4, 9]));
    assert_eq!(j("c =. &\n2 (+ c *:) 3"), Array::scalar_i64(13));
    // A named modifier composes with the verb naming that already worked.
    assert_eq!(j("m =. /\nmean =. + m % #\nmean 1 2 3 4"), Array::scalar_f64(2.5));
    // A name may change part of speech; the last assignment wins.
    assert_eq!(j("m =. /\nm =. 5\nm + 1"), Array::scalar_i64(6));
    // A sentence that IS a modifier has nothing to display yet.
    let e = err(Lang::J, "m =. /\nm");
    assert_eq!(e.kind, ErrorKind::NotYet);
    assert!(e.msg.contains("displaying a modifier"), "{}", e.msg);
}

#[test]
fn a_named_modifier_is_shown_as_one() {
    let p = compile(Lang::J, "m =. /\n+ m 1 2 3", &Dialect::default()).expect("compiles");
    let structure = p.explain(None);
    assert!(structure.contains("adverb definition m = /"), "{structure}");
    let p = compile(Lang::J, "c =. @\n(+/ c *:) 1 2 3", &Dialect::default()).expect("compiles");
    assert!(p.explain(None).contains("conjunction definition c = @"), "{}", p.explain(None));
}

// --- J: the dyadic level and spread ---------------------------------------

#[test]
fn the_levels_pair_both_arguments() {
    assert_eq!(
        j("(1;2) ,L:0 (3;4)"),
        boxes(&[2], vec![i64s(&[2], &[1, 3]), i64s(&[2], &[2, 4])])
    );
    // A side that has already reached its level is held while the other
    // descends, so an unboxed left argument reaches every leaf.
    assert_eq!(
        j("1 ,L:0 (3;4)"),
        boxes(&[2], vec![i64s(&[2], &[1, 3]), i64s(&[2], &[1, 4])])
    );
    // `S:` spreads the answers into the items of one array instead.
    assert_eq!(j("(1;2) ,S:0 (3;4)"), i64s(&[2, 2], &[1, 3, 2, 4]));
    assert_eq!(j("((1;2);3) ,S:0 ((4;5);6)"), i64s(&[3, 2], &[1, 4, 2, 5, 3, 6]));
    // Two sides that both still have boxes have to agree.
    let e = err(Lang::J, "(1;2;3) ,L:0 (4;5)");
    assert_eq!(e.kind, ErrorKind::Length);
    assert!(e.msg.contains("do not agree"), "{}", e.msg);
}

// --- J: the hypergeometric series -----------------------------------------

#[test]
fn the_hypergeometric_series_sums_its_parameters() {
    // Cancelling the parameters the two lists share is what makes the
    // classic identities come out: 1 H. 1 is the exponential.
    assert_eq!(j("(1 H. 1) 0"), Array::scalar_f64(1.0));
    let e = j("(1 H. 1) 1");
    assert!((e.to_f64_vec().expect("a number")[0] - std::f64::consts::E).abs() < 1e-12);
    // A geometric series where the numerator survives, and a divergent one.
    assert_eq!(j("(1 H. '') 0.5"), Array::scalar_f64(2.0));
    let four = j("(2 H. '') 0.5").to_f64_vec().expect("a number")[0];
    assert!((four - 4.0).abs() < 1e-12, "{four}");
    assert_eq!(j("(1 H. '') 2"), Array::scalar_f64(f64::INFINITY));
    // A zero denominator parameter divides by zero; a zero numerator one
    // stops the series after its first term.
    assert_eq!(j("(1 H. 0) 1"), Array::scalar_f64(f64::INFINITY));
    assert_eq!(j("(0 H. 1) 1"), Array::scalar_f64(1.0));
    // A series that neither converges nor overflows is refused by name
    // rather than run forever — which is what the reference does with it.
    let e = err(Lang::J, "(1 H. '') 1");
    assert_eq!(e.kind, ErrorKind::Domain);
    assert!(e.msg.contains("did not converge"), "{}", e.msg);
    // It has no dyadic valence.
    let e = err(Lang::J, "2 (1 H. 1) 3");
    assert_eq!(e.kind, ErrorKind::Domain);
}

// --- the gaps this wave leaves --------------------------------------------

#[test]
fn the_gaps_this_wave_leaves_name_themselves() {
    let cases: &[(Lang, &str, &str)] = &[
        // The tacit translator: naming a primitive modifier and writing a
        // new one both landed, reading an explicit body back as a tacit
        // verb has not.
        (Lang::J, "f =. 13 : 'y + 1'", "tacit definitions"),
        (Lang::J, "m =. /\nm", "displaying a modifier"),
        (Lang::J, "(^ t. 3) 0", "Taylor"),
        (Lang::Apl, "(A+×)4", "computed value"),
        (Lang::Apl, "3+F←+/", "inside a larger sentence"),
        // A glyph the language has and libjay has not reached is a queue
        // position, not an unknown character.
        (Lang::Apl, "(3 3⍴1)⌺⊢2 3⍴⍳6", "stencil"),
        (Lang::Apl, "1⍠2", "variant"),
        (Lang::Apl, "+⍢-3", "under"),
        (Lang::Apl, "⌶3", "I-beam"),
    ];
    for (lang, src, what) in cases {
        let e = err(*lang, src);
        assert!(
            matches!(e.kind, ErrorKind::NotYet | ErrorKind::Parse),
            "{src}: {:?} {}",
            e.kind,
            e.msg
        );
        assert!(e.msg.contains(what), "{src}: {}", e.msg);
    }
}
