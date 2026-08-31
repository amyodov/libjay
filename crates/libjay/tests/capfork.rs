//! The capped fork and the representation of an explicit definition.
//!
//! Everything an oracle answers is in tests/corpus/j/capfork.txt. This file
//! carries what it cannot hold: the shape of the verb tree, the round trip
//! through the atomic representation as data, `explain`, and the gaps this
//! leaves named.

use jay::gerund::{verb_ar, Ar};
use jay::verb::{AtopForm, Verb};
use jay::{compile, Array, Dialect, ErrorKind, Lang};

fn run(src: &str) -> Option<Array> {
    let program = compile(Lang::J, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile of {src:?} failed:\n{}", e.render(src)));
    let mut sink = |_: &str| {};
    program
        .run(&[], &mut sink)
        .unwrap_or_else(|e| panic!("run of {src:?} failed:\n{}", program.render_error(&e)))
}

fn j(src: &str) -> Array {
    run(src).unwrap_or_else(|| panic!("{src:?} yielded no value"))
}

fn err(src: &str) -> jay::Error {
    let program = match compile(Lang::J, src, &Dialect::default()) {
        Err(e) => return e,
        Ok(p) => p,
    };
    let mut sink = |_: &str| {};
    program.run(&[], &mut sink).expect_err("expected an error")
}

fn text(s: &str) -> Array {
    Array::from_chars(s.chars().collect())
}

/// The verb one sentence names, out of the compiled program.
fn verb_of(src: &str) -> Verb {
    let program = compile(Lang::J, src, &Dialect::default())
        .unwrap_or_else(|e| panic!("compile of {src:?} failed:\n{}", e.render(src)));
    program
        .stmts
        .iter()
        .find_map(|s| match s {
            jay::ir::Expr::VerbDef { verb, .. } => Some(verb.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{src:?} names no verb"))
}

#[test]
fn the_cap_and_the_atop_are_one_function_under_two_spellings() {
    // The tree keeps which was written, and nothing else about them
    // differs: the same parts, in the same order.
    let cap = verb_of("f =. [: +: *:");
    let at = verb_of("f =. +:@:*:");
    match (&cap, &at) {
        (Verb::Atop(a, b, AtopForm::Cap), Verb::Atop(c, d, AtopForm::At)) => {
            assert_eq!(a.name(), c.name());
            assert_eq!(b.name(), d.name());
        }
        other => panic!("expected two atops, got {other:?}"),
    }
    // Applying one is applying the other.
    assert_eq!(j("([: +: *:) 3"), j("(+:@:*:) 3"));
    assert_eq!(j("2 ([: +: *) 3"), j("2 (+:@:*) 3"));
    assert_eq!(j("([: +: *:) b. 0"), j("(+:@:*:) b. 0"));
    assert_eq!(j("([: +: *:)^:_1 ] 16"), j("(+:@:*:)^:_1 ] 16"));
}

#[test]
fn the_cap_is_a_three_part_train_in_the_representation() {
    let cap = verb_ar(&verb_of("f =. [: +: *:")).expect("a capped fork has one");
    assert_eq!(
        cap,
        Ar::Train(vec![
            Ar::Prim("[:".to_string()),
            Ar::Prim("+:".to_string()),
            Ar::Prim("*:".to_string()),
        ])
    );
    // And it survives the trip out to data and back.
    assert_eq!(Ar::from_array(&cap.to_array()), Some(cap));
    // The atop is the conjunction over the same two operands, which is a
    // different tree and a different spelling.
    assert_eq!(
        verb_ar(&verb_of("f =. +:@:*:")),
        Some(Ar::Derived(
            "@:".to_string(),
            vec![Ar::Prim("+:".to_string()), Ar::Prim("*:".to_string())]
        ))
    );
}

#[test]
fn a_representation_read_back_applies_as_the_verb_it_names() {
    // `5!:0` over the cap's representation gives the verb back.
    assert_eq!(j("f =. [: +: *:\nh =. (5!:1 <'f') 5!:0\nh 3"), Array::scalar_i64(18));
    // A gerund may hold one, and the agenda picks it out.
    assert_eq!(j("(([: +: *:)`-:@.0) 3"), Array::scalar_i64(18));
}

#[test]
fn an_explicit_definition_represents_itself_by_valence_and_body() {
    let def = verb_ar(&verb_of("f =. 3 : 'y + 1'")).expect("a definition has one");
    assert_eq!(
        def,
        Ar::Derived(
            ":".to_string(),
            vec![Ar::Noun(Array::scalar_i64(3)), Ar::Noun(text("y + 1"))]
        )
    );
    assert_eq!(jay::gerund::linear(&def).as_deref(), Some("3 : 'y + 1'"));
    // A body of several lines is a character matrix, one row a line, and
    // the padding is not part of what was written.
    let long = verb_ar(&verb_of("f =. 3 : 0\na =. y + 1\na * 2\n)")).expect("has one");
    let Ar::Derived(_, ops) = &long else { panic!("expected the `:` phrase") };
    let Ar::Noun(body) = &ops[1] else { panic!("expected a body noun") };
    assert_eq!(body.shape, vec![2, 10]);
    assert_eq!(
        jay::gerund::linear(&long).as_deref(),
        Some("3 : 0\na =. y + 1\na * 2\n)")
    );
}

#[test]
fn a_direct_definition_shows_its_braces_and_represents_its_header() {
    // The two are different questions: what a session displays, and what
    // the representation forms answer.
    assert_eq!(j("d =. {{ y + 1 }}\nd"), text("{{ y + 1 }}"));
    assert_eq!(j("d =. {{ y + 1 }}\n5!:5 <'d'"), text("3 : 'y + 1 '"));
    // The blanks that hold the body off the opening braces are dropped;
    // what is written after them is not.
    assert_eq!(j("d =. {{   y + 1 }}\nd"), text("{{ y + 1 }}"));
    assert_eq!(j("d =. {{y+1}}\nd"), text("{{ y+1}}"));
}

#[test]
fn a_name_of_modifier_class_answers_for_its_modifier() {
    assert_eq!(j("m =. /\n5!:5 <'m'"), text("/"));
    assert_eq!(j("a =. 1 : 'u y'\n5!:5 <'a'"), text("1 : 'u y'"));
    assert_eq!(j("c =. 2 : 'u v y'\n5!:5 <'c'"), text("2 : 'u v y'"));
    // A name with no meaning still stands for itself.
    assert_eq!(j("> 5!:1 <'zz'"), text("zz"));
}

#[test]
fn the_shapes_that_had_no_words_have_them_now() {
    for (src, spelling) in [
        ("f =. +/ . *", "+/ .*"),
        ("f =. +: : *:", "+: :*:"),
        ("f =. |.!.0", "|.!.0"),
        ("f =. +:&.,", "+:&.,"),
        ("f =. {.!.9", "{.!.9"),
        ("f =. 2 H. 3", "2 H.3"),
        ("f =. +:`*:`]}", "+:`*:`]} "),
    ] {
        let ar = verb_ar(&verb_of(src)).unwrap_or_else(|| panic!("{src} has no representation"));
        assert_eq!(jay::gerund::linear(&ar).as_deref(), Some(spelling), "{src}");
    }
}

#[test]
fn explain_names_the_cap_apart_from_the_atop() {
    let program = compile(Lang::J, "([: +: *:) 3", &Dialect::default()).expect("compiles");
    assert!(program.explain(None).contains("capped fork"), "{}", program.explain(None));
    let program = compile(Lang::J, "(+:@:*:) 3", &Dialect::default()).expect("compiles");
    assert!(program.explain(None).contains("atop"), "{}", program.explain(None));
}

#[test]
fn the_cap_in_a_two_part_train_is_still_named() {
    // The reference builds a verb of `[: g` that raises a valence error
    // wherever it is applied; libjay refuses it where it is written, and
    // says why.
    let e = err("k =. [: [: +: *:");
    assert_eq!(e.kind, ErrorKind::Parse);
    assert!(e.msg.contains("caps a fork"), "{}", e.msg);
}
