//! libjay's own answer to a sentence.

use jay::fmt::{FmtOpts, format_array};
use jay::{Dialect, Lang, compile};

/// Compile and run one sentence. `None` is a refusal — a compile error, a
/// run error, or a program with no value. Nothing is printed on the way:
/// what is compared is the value of the last sentence.
pub fn eval(lang: Lang, expr: &str, index_origin: u8) -> Option<String> {
    let dialect = match lang {
        Lang::J => Dialect::default(),
        Lang::Apl => Dialect { index_origin: Some(index_origin as i64) },
    };
    let program = compile(lang, expr, &dialect).ok()?;
    let mut sink = |_: &str| {};
    let result = program.run(&[], &mut sink).ok()??;
    let opts = match lang {
        Lang::J => FmtOpts::J,
        Lang::Apl => FmtOpts::APL,
    };
    Some(format_array(&result, &opts))
}
