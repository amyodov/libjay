//! Scratch probe: libjay's answer to each argument sentence. Not committed.
fn main() {
    for expr in std::env::args().skip(1) {
        println!("=== {expr}");
        let a = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            libjay_testkit::eval::eval_detail(libjay_testkit::Lang::J, &expr, 0)
        }));
        match a {
            Ok(libjay_testkit::eval::Answer::Value(v)) => println!("{v}"),
            Ok(libjay_testkit::eval::Answer::NoValue) => println!("<no value>"),
            Ok(libjay_testkit::eval::Answer::Refused(e)) => println!("REFUSED: {e}"),
            Err(_) => println!("PANIC"),
        }
    }
}
