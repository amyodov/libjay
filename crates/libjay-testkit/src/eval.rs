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

/// The dialect a preset name asks for. The names are the ones the corpus
/// tooling and the Python surface use.
pub fn preset(name: &str) -> Option<Dialect> {
    match name {
        "gnu" => Some(Dialect::gnu_apl()),
        "dyalog" => Some(Dialect::dyalog()),
        _ => None,
    }
}

/// Compile and run one sentence, keeping the refusal. Nothing is printed on
/// the way: what is compared is the value of the last sentence.
pub fn eval_detail(lang: Lang, expr: &str, index_origin: u8) -> Answer {
    eval_detail_as(lang, expr, index_origin, Dialect::default())
}

/// The same, under a dialect the caller chose. The index origin still
/// comes from the corpus, which is where a `@ io=0` file states it.
pub fn eval_detail_as(lang: Lang, expr: &str, index_origin: u8, base: Dialect) -> Answer {
    let dialect = match lang {
        Lang::J => base,
        Lang::Apl => Dialect { index_origin: Some(index_origin as i64), ..base },
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
    flatten(eval_detail(lang, expr, index_origin))
}

/// The same, under a dialect the caller chose.
pub fn eval_as(lang: Lang, expr: &str, index_origin: u8, base: Dialect) -> Option<String> {
    flatten(eval_detail_as(lang, expr, index_origin, base))
}

fn flatten(answer: Answer) -> Option<String> {
    match answer {
        Answer::Value(text) => Some(text),
        Answer::NoValue | Answer::Refused(_) => None,
    }
}
