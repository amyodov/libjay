//! libjay's own answer to a sentence.

use jay::fmt::{FmtOpts, format_array};
use jay::{Dialect, Error, Lang, compile};

/// What libjay made of a sentence: a printed value, no value at all (a
/// program ending in an assignment), or a refusal with its diagnostic.
pub enum Answer {
    Value(String),
    NoValue,
    Refused(Error),
}

/// Compile and run one sentence, keeping the refusal. Nothing is printed on
/// the way: what is compared is the value of the last sentence.
pub fn eval_detail(lang: Lang, expr: &str, index_origin: u8) -> Answer {
    let dialect = match lang {
        Lang::J => Dialect::default(),
        Lang::Apl => Dialect { index_origin: Some(index_origin as i64), ..Dialect::default() },
    };
    let program = match compile(lang, expr, &dialect) {
        Ok(p) => p,
        Err(e) => return Answer::Refused(e),
    };
    let mut sink = |_: &str| {};
    match program.run(&[], &mut sink) {
        Ok(Some(result)) => {
            let opts = match lang {
                Lang::J => FmtOpts::J,
                Lang::Apl => FmtOpts::APL,
            };
            Answer::Value(format_array(&result, &opts))
        }
        Ok(None) => Answer::NoValue,
        Err(e) => Answer::Refused(e),
    }
}

/// Compile and run one sentence. `None` is a refusal — a compile error, a
/// run error, or a program with no value.
pub fn eval(lang: Lang, expr: &str, index_origin: u8) -> Option<String> {
    match eval_detail(lang, expr, index_origin) {
        Answer::Value(text) => Some(text),
        Answer::NoValue | Answer::Refused(_) => None,
    }
}
