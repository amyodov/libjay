//! Scratch probe: evaluates the sentences in the file LIBJAY_PROBE, `;;` apart.
use jay::Lang;
use libjay_testkit::eval::Answer;

#[test]
fn probe() {
    let Ok(path) = std::env::var("LIBJAY_PROBE") else { return };
    let src = std::fs::read_to_string(path).expect("probe file");
    for (i, expr) in src.split(";;\n").enumerate() {
        let expr = expr.trim_end_matches('\n');
        let shown = match libjay_testkit::eval::eval_detail(Lang::J, expr, 1) {
            Answer::Value(v) => v,
            Answer::NoValue => "<no value>".to_string(),
            Answer::Refused(e) => format!("REFUSED: {}", e.render(expr)),
        };
        println!("=== {i}: {expr}\n{shown}");
    }
}
