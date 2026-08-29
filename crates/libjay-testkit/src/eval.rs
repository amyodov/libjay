//! libjay's own answer to a sentence.

use jay::fmt::format_array;
use jay::{Dialect, Error, Lang, compile};

/// What libjay made of a sentence: the lines a session would show — what
/// the program printed, then the value it displays — no value at all (a
/// program ending in an assignment and printing nothing), or a refusal with
/// its diagnostic. A shy answer shows only what the program printed.
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
    // The corpus is what the reference implementations answer, so it is
    // compiled with no extension whatever the environment says: a flag left
    // in the environment must not be able to move a recorded answer.
    let base = Dialect { extensions: Some(jay::Extensions::NONE), ..base };
    let dialect = match lang {
        Lang::J => base,
        Lang::Apl => Dialect { index_origin: Some(index_origin as i64), ..base },
    };
    let program = match compile(lang, expr, &dialect) {
        Ok(p) => p,
        Err(e) => return Answer::Refused(e),
    };
    // What is compared is what a session would show: everything the program
    // printed for itself, in order, and then the value it displays. A SHY
    // value is not displayed, and a sentence with no value at all leaves
    // only what it printed.
    let mut printed = String::new();
    let mut sink = |s: &str| printed.push_str(s);
    match program.run_detail(&[], &mut sink) {
        Ok(outcome) => {
            let printed = printed.trim_end();
            // A shy value is an answer the session shows as an empty line,
            // which is not the same as a sentence that had no value at all.
            let shy = outcome.shy && outcome.value.is_some();
            let fmt = outcome.fmt;
            let shown = match outcome.value {
                None => None,
                Some(_) if outcome.shy => None,
                // The program's own conventions, which carry whatever
                // extensions it was compiled under and whatever print
                // precision it ended on.
                Some(result) => Some(format_array(&result, &fmt)),
            };
            match (printed.is_empty(), shown) {
                (true, None) if shy => Answer::Value(String::new()),
                (true, None) => Answer::NoValue,
                (true, Some(value)) => Answer::Value(value),
                (false, None) => Answer::Value(printed.to_string()),
                (false, Some(value)) => Answer::Value(format!("{printed}\n{value}")),
            }
        }
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
