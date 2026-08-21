//! The ceiling on how large a single value may be.
//!
//! A shape is arithmetic: `1e12 $ 0` costs one multiplication to write and
//! eight terabytes to hold. Verbs that build a result from a requested
//! shape or count ask here first, so an impossible request comes back as an
//! ordinary error pointing at the expression instead of as a process the
//! operating system kills.

use crate::error::{Error, ErrorKind, Result, Span};

/// The most elements one value may hold.
///
/// Set by what a machine can plausibly hold rather than by J or APL, both
/// of which put no limit in the language: 2^32 elements is 32 GB of
/// doubles, past any real working set and far short of a request that
/// would take the process down.
pub const MAX_ELEMENTS: usize = 1 << 32;

/// The number of elements a value of this shape holds.
///
/// An axis of zero makes the value empty whatever the other axes say, so
/// that case is answered before the ceiling applies.
pub fn elements(shape: &[usize], span: Span) -> Result<usize> {
    if shape.contains(&0) {
        return Ok(0);
    }
    let mut n: u128 = 1;
    for &d in shape {
        n *= d as u128;
        if n > MAX_ELEMENTS as u128 {
            return Err(too_many(n, Some(shape), span));
        }
    }
    Ok(n as usize)
}

/// A count already worked out, checked against the same ceiling.
pub fn count(n: u128, span: Span) -> Result<usize> {
    if n > MAX_ELEMENTS as u128 {
        return Err(too_many(n, None, span));
    }
    Ok(n as usize)
}

fn too_many(n: u128, shape: Option<&[usize]>, span: Span) -> Error {
    let e = Error::new(
        ErrorKind::Limit,
        format!("a result of {n} elements is past the {MAX_ELEMENTS}-element ceiling"),
        Some(span),
    );
    match shape {
        None => e,
        Some(s) => {
            let dims: Vec<String> = s.iter().map(usize::to_string).collect();
            e.note(format!("the shape asked for is {}", dims.join(" ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPAN: Span = Span { start: 0, end: 0 };

    #[test]
    fn an_ordinary_shape_passes() {
        assert_eq!(elements(&[2, 3, 4], SPAN).unwrap(), 24);
        assert_eq!(elements(&[], SPAN).unwrap(), 1);
    }

    #[test]
    fn an_empty_axis_beats_the_ceiling() {
        assert_eq!(elements(&[usize::MAX, 0], SPAN).unwrap(), 0);
    }

    #[test]
    fn a_product_that_would_wrap_is_refused() {
        // 2^32 * 2^32 is 2^64, which wraps to zero in usize arithmetic.
        let e = elements(&[1 << 32, 1 << 32], SPAN).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Limit);
        assert!(e.msg.contains("18446744073709551616"), "{}", e.msg);
    }

    #[test]
    fn the_ceiling_itself_is_allowed() {
        assert!(elements(&[MAX_ELEMENTS], SPAN).is_ok());
        assert!(elements(&[MAX_ELEMENTS + 1], SPAN).is_err());
    }
}
