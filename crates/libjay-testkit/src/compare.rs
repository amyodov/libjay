//! Comparing two answers to the same sentence.
//!
//! Comparison is textual with numeric tolerance. libjay prints 6
//! significant digits, jconsole the same and GNU APL `⎕PP` (10) of them, so
//! parsing each token back to `f64` leaves at most ~5e-6 relative
//! representation error per side; a relative tolerance of 1e-5, with a 1e-9
//! absolute floor for values near zero, accepts exactly that and still
//! catches any real semantic difference. Line structure is significant — it
//! encodes the shape — and column padding is not, because the two printers
//! align a mixed column differently and the alignment is not the semantics.

use crate::Lang;
use crate::snapshot::Side;

/// J spells a negative with a leading `_`, and `_`, `__`, `_.` are its
/// infinities and NaN. APL spells a negative with `¯` and has no infinity
/// of its own, so libjay's `∞` never parses there and falls through to the
/// textual comparison, which is what a divergence should do.
fn parse_num(lang: Lang, tok: &str) -> Option<f64> {
    match lang {
        Lang::J => {
            match tok {
                "_" => return Some(f64::INFINITY),
                "__" => return Some(f64::NEG_INFINITY),
                "_." => return Some(f64::NAN),
                _ => {}
            }
            tok.replace('_', "-").parse::<f64>().ok()
        }
        Lang::Apl => tok.replace('¯', "-").parse::<f64>().ok(),
    }
}

fn tokens_match(lang: Lang, a: &str, b: &str) -> bool {
    match (parse_num(lang, a), parse_num(lang, b)) {
        (Some(x), Some(y)) => {
            if x.is_nan() || y.is_nan() {
                return x.is_nan() && y.is_nan();
            }
            if x.is_infinite() || y.is_infinite() {
                return x == y;
            }
            let scale = x.abs().max(y.abs());
            (x - y).abs() <= 1e-9 + 1e-5 * scale
        }
        _ => a == b,
    }
}

pub fn outputs_match(lang: Lang, ours: &str, theirs: &str) -> bool {
    let ol: Vec<&str> = ours.lines().collect();
    let tl: Vec<&str> = theirs.lines().collect();
    if ol.len() != tl.len() {
        return false;
    }
    ol.iter().zip(&tl).all(|(o, t)| {
        let ot: Vec<&str> = o.split_whitespace().collect();
        let tt: Vec<&str> = t.split_whitespace().collect();
        ot.len() == tt.len() && ot.iter().zip(&tt).all(|(a, b)| tokens_match(lang, a, b))
    })
}

/// Two answers agree when both are values that match, or both are refusals.
/// Error texts belong to their own implementations and are not compared.
pub fn sides_match(lang: Lang, ours: &Side, theirs: &Side) -> bool {
    match (ours.text(), theirs.text()) {
        (Some(o), Some(t)) => outputs_match(lang, o, t),
        (None, None) => true,
        _ => false,
    }
}
