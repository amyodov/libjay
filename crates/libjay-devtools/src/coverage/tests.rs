//! The classifier's judgements, on expressions written for the purpose.
//!
//! The real corpus is not tested here: its numbers move every time a line
//! is added, and a test that moves with them says nothing. These fixtures
//! are small, and each one asks a single question of the classifier.

use super::*;

/// A fixture corpus: expressions under one index origin.
fn entries(exprs: &[&str], io: u8) -> Vec<corpus::Entry> {
    exprs
        .iter()
        .map(|e| corpus::Entry { expr: (*e).to_string(), io, note: None, family: None })
        .collect()
}

fn measure_j(exprs: &[&str]) -> Coverage {
    measure(Lang::J, &entries(exprs, 0))
}

fn measure_apl(exprs: &[&str]) -> Coverage {
    measure(Lang::Apl, &entries(exprs, 1))
}

/// The cells one primitive was measured to meet, as labels.
fn cells(cov: &Coverage, prim: &str, valence: Valence) -> Vec<String> {
    cov.grid
        .get(&(prim.to_string(), valence))
        .map(|c| c.keys().map(|k| k.label()).collect())
        .unwrap_or_default()
}

fn hits(cov: &Coverage, prim: &str, valence: Valence) -> Hits {
    cov.grid
        .get(&(prim.to_string(), valence))
        .map(|c| {
            c.values().fold(Hits::default(), |mut acc, h| {
                acc.direct += h.direct;
                acc.on_cells += h.on_cells;
                acc
            })
        })
        .unwrap_or_default()
}

#[test]
fn every_type_class_is_told_apart_by_the_operand_it_is_given() {
    // One expression per type-class, each applying `-` to a literal, so
    // the cell that shows up names the class of that literal.
    let cases: &[(&str, &str)] = &[
        ("- 1 0 1 = 1 0 1", "bool/vector"),
        ("- 1 2 3", "int/vector"),
        ("- 4000000 1", "int-big/vector"),
        ("- 9223372036854775806", "i64-edge/scalar"),
        ("- 1.5 2.5", "float/vector"),
        ("- 1 1.0000000000001", "float-tol/vector"),
        ("- _ 0", "float-inf/vector"),
        ("- 3j4", "complex/scalar"),
        ("- 123x", "extended/scalar"),
        ("- 1r3", "rational/scalar"),
        ("- 'abc'", "char/vector"),
        ("- <1 2", "box/scalar"),
        ("- <<1 2", "box-nested/scalar"),
        ("- s: 'ab'", "symbol/vector-1"),
    ];
    for (expr, want) in cases {
        let cov = measure_j(&[expr]);
        assert_eq!(cells(&cov, "-", Valence::Monad), vec![want.to_string()], "{expr}");
    }
}

#[test]
fn every_rank_class_is_told_apart_by_the_shape_it_is_given() {
    let cases: &[(&str, &str)] = &[
        ("] 5", "int/scalar"),
        ("] 1 2 3", "int/vector"),
        ("] ,5", "int/vector-1"),
        ("] i. 0", "int/vector-empty"),
        ("] i. 2 3", "int/matrix"),
        ("] 1 3 $ 1 2 3", "int/matrix-1"),
        ("] 0 3 $ 0", "int/matrix-empty"),
        ("] i. 2 3 4", "int/rank3+"),
        ("] 1 2 3 $ i. 6", "int/rank3+-1"),
        ("] 0 2 3 $ 0", "int/rank3+-empty"),
    ];
    for (expr, want) in cases {
        let cov = measure_j(&[expr]);
        assert_eq!(cells(&cov, "]", Valence::Monad), vec![want.to_string()], "{expr}");
    }
}

#[test]
fn a_dyad_records_the_pair_of_classes_it_met() {
    let cov = measure_j(&["1 2 3 + 1.5"]);
    assert_eq!(cells(&cov, "+", Valence::Dyad), vec!["int/vector × float/scalar".to_string()]);
    assert!(cells(&cov, "+", Valence::Monad).is_empty());
}

#[test]
fn an_operand_is_the_value_it_computes_not_the_literal_it_is_written_as() {
    // The right operand of `#` is a boxed value no literal in the sentence
    // spells; classifying by evaluation is what sees it.
    let cov = measure_j(&["# < i. 3"]);
    assert_eq!(cells(&cov, "#", Valence::Monad), vec!["box/scalar".to_string()]);
}

#[test]
fn a_name_assigned_in_an_earlier_sentence_still_has_a_value() {
    let cov = measure_j(&["a =. 2 3 $ 1.5\n- a"]);
    assert_eq!(cells(&cov, "-", Valence::Monad), vec!["float/matrix".to_string()]);
}

#[test]
fn a_reduction_attributes_its_verb_to_the_items_and_says_so() {
    // `+` meets two items of a rank-2 array — vectors, not the matrix.
    let cov = measure_j(&["+/ i. 2 3"]);
    assert_eq!(
        cells(&cov, "+", Valence::Dyad),
        vec!["int/vector × int/vector".to_string()]
    );
    assert_eq!(hits(&cov, "+", Valence::Dyad), Hits { direct: 0, on_cells: 1 });
    // A reduction over a vector gives it atoms.
    let cov = measure_j(&["+/ 1 2 3"]);
    assert_eq!(
        cells(&cov, "+", Valence::Dyad),
        vec!["int/scalar × int/scalar".to_string()]
    );
}

#[test]
fn rank_attributes_its_verb_to_the_cells_of_the_named_rank() {
    let cov = measure_j(&["# \"1 i. 2 3 4"]);
    assert_eq!(cells(&cov, "#", Valence::Monad), vec!["int/vector".to_string()]);
    assert_eq!(hits(&cov, "#", Valence::Monad), Hits { direct: 0, on_cells: 1 });
    // An infinite rank is the whole argument, and is a direct application.
    let cov = measure_j(&["# \"_ i. 2 3 4"]);
    assert_eq!(cells(&cov, "#", Valence::Monad), vec!["int/rank3+".to_string()]);
    assert_eq!(hits(&cov, "#", Valence::Monad), Hits { direct: 1, on_cells: 0 });
}

#[test]
fn both_tines_of_a_fork_meet_the_argument_and_the_middle_verb_does_not() {
    let cov = measure_j(&["(+/ % #) 1.5 2.5 3.5"]);
    assert_eq!(cells(&cov, "#", Valence::Monad), vec!["float/vector".to_string()]);
    assert_eq!(
        cells(&cov, "+", Valence::Dyad),
        vec!["float/scalar × float/scalar".to_string()]
    );
    // `%` is handed what the tines answered, which is not the site's own
    // argument: it is in the operator table and in no cell.
    assert!(cells(&cov, "%", Valence::Dyad).is_empty());
}

#[test]
fn a_composition_this_cannot_describe_leaves_the_site_unattributable() {
    let cov = measure_j(&["+/\\ 1 2 3"]);
    assert_eq!(cov.sites, 1);
    assert_eq!(cov.opaque, 1);
    assert!(cov.grid.is_empty());
    // The primitive is still known to be mentioned, and the modifiers are
    // still counted.
    assert!(cov.mentioned.contains("+"));
    assert!(cov.modifiers.contains_key("window"));
}

#[test]
fn commute_hands_one_argument_to_both_sides_of_the_dyad() {
    let cov = measure_j(&["-~ 1 2 3"]);
    assert_eq!(
        cells(&cov, "-", Valence::Dyad),
        vec!["int/vector × int/vector".to_string()]
    );
}

#[test]
fn a_sentence_libjay_refuses_is_counted_and_not_guessed_at() {
    let cov = measure_j(&["1 2 + )("]);
    assert_eq!((cov.exprs, cov.refused, cov.sites), (1, 1, 0));
}

#[test]
fn an_operand_that_cannot_be_run_is_classified_unknown_rather_than_dropped() {
    // Inside an explicit definition the argument name has no value until
    // the definition is called, so the site is seen and occupies no cell.
    let cov = measure_j(&["f =. 3 : '- y'"]);
    assert_eq!(cov.in_definition, 1);
    assert_eq!(cells(&cov, "-", Valence::Monad), vec!["unknown/unknown".to_string()]);
    assert!(!cov.universe(Valence::Monad).contains(&Cell {
        x: None,
        y: Class::UNKNOWN,
    }));
}

#[test]
fn the_universe_is_the_cells_the_corpus_builds_for_some_primitive() {
    let cov = measure_j(&["- 1 2 3", "# 'ab'", "- 'ab'"]);
    let universe = cov.universe(Valence::Monad);
    let labels: Vec<String> = universe.iter().map(|c| c.label()).collect();
    assert_eq!(labels, ["int/vector", "char/vector"]);
    // `#` met one of the two, so the other is its empty cell.
    let empty = cov.empty_cells("#", Valence::Monad, &universe);
    assert_eq!(empty.iter().map(|c| c.label()).collect::<Vec<_>>(), ["int/vector"]);
    assert!(cov.empty_cells("-", Valence::Monad, &universe).is_empty());
}

#[test]
fn the_operator_table_names_the_operands_and_the_nouns_a_modifier_met() {
    let cov = measure_j(&["+/ i. 2 3", "*/ 1 2 3"]);
    let stat = &cov.modifiers["insert / table"];
    assert_eq!(stat.sites, 2);
    assert_eq!(stat.operands["+"], 1);
    assert_eq!(stat.operands["*"], 1);
    assert_eq!(stat.nouns["int/matrix"], 1);
    assert_eq!(stat.nouns["int/vector"], 1);
}

#[test]
fn a_modifier_over_a_modifier_is_named_by_its_kind() {
    let cov = measure_j(&["+/@:* 1 2 3"]);
    assert_eq!(cov.modifiers["atop"].operands["a derived verb"], 1);
    assert_eq!(cov.modifiers["atop"].operands["*"], 1);
    assert_eq!(cov.modifiers["insert / table"].sites, 1);
}

#[test]
fn apl_is_measured_by_the_same_rules_under_its_own_index_origin() {
    let cov = measure_apl(&["⍳3", "-2 3⍴1.5", "+/1 2 3"]);
    assert_eq!(cells(&cov, "⍳", Valence::Monad), vec!["int/scalar".to_string()]);
    assert_eq!(cells(&cov, "-", Valence::Monad), vec!["float/matrix".to_string()]);
    assert_eq!(
        cells(&cov, "+", Valence::Dyad),
        vec!["int/scalar × int/scalar".to_string()]
    );
    assert!(cov.modifiers.contains_key("reduce / outer product"));
}

#[test]
fn the_index_origin_a_fixture_is_read_under_reaches_the_classifier() {
    // `⍳3` is `1 2 3` under origin 1 and `0 1 2` under origin 0; the
    // classes are the same, and the values the classifier ran are not.
    for io in [0u8, 1] {
        let cov = measure(Lang::Apl, &entries(&["-⍳3"], io));
        assert_eq!(cells(&cov, "-", Valence::Monad), vec!["int/vector".to_string()]);
    }
}

#[test]
fn the_json_and_the_tsv_carry_the_cells_the_report_summarises() {
    let cov = measure_j(&["- 1 2 3", "# 'ab'"]);
    let inv = Inventory::default();
    let json = json(Lang::J, &cov, &inv);
    assert!(json.contains("\"primitive\": \"-\""));
    assert!(json.contains("\"y\": [\"int\", \"vector\"]"));
    let tsv = tsv(Lang::J, &cov);
    let lines: Vec<&str> = tsv.lines().collect();
    assert_eq!(lines[0], "lang\tprimitive\tvalence\tx_type\tx_rank\ty_type\ty_rank");
    assert!(lines.contains(&"j\t#\tmonad\t\t\tint\tvector"));
    assert!(lines.contains(&"j\t-\tmonad\t\t\tchar\tvector"));
}

#[test]
fn the_report_says_what_it_measured_and_stays_one_screenful() {
    let cov = measure_j(&["- 1 2 3", "+/ i. 2 3", "'ab' , 'cd'"]);
    let text = report(Lang::J, &cov, &Inventory::default(), 5);
    assert!(text.contains("3 expressions"));
    assert!(text.contains("monad grid"));
    assert!(text.contains("dyad grid"));
    assert!(text.contains("operator layer"));
    assert!(text.lines().count() < 80, "{} lines", text.lines().count());
}


