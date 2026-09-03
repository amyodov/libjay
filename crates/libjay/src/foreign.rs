//! The J foreigns that only COMPUTE: the `3!:` conversions, the `8!:`
//! formats and `128!:3`.
//!
//! A foreign that reaches a file, the host, the clock, a script or a shared
//! library is closed by the sandbox and is refused where it is spelled; a
//! foreign that reads or writes the program's own names is part of the
//! evaluator. What is left is arithmetic on the argument alone, and it
//! lives here.

use crate::array::{Array, Data};
use crate::dtype::DType;
use crate::error::{Error, ErrorKind, Result, Span};

// -------------------------------------------------- 3!:4 and 3!:5, bytes

/// The byte width `3!:4` gives each integer, by the magnitude of its left
/// argument. Four is the unsigned four-byte form, which differs from the
/// signed one only when the bytes are read back.
fn int_width(k: i64) -> Option<usize> {
    Some(match k {
        1 => 2,
        2 | 4 => 4,
        3 => 8,
        _ => return None,
    })
}

/// A character vector's bytes. J's literal holds one byte per character, so
/// a character wider than a byte was never a byte string to begin with.
fn byte_string(y: &Array, what: &str, span: Span) -> Result<Vec<u8>> {
    if y.rank() > 1 {
        return Err(Error::new(
            ErrorKind::Rank,
            format!("{what} reads a list of bytes, not an array of rank {}", y.rank()),
            Some(span),
        ));
    }
    // Nothing to read is no error, whatever type the empty was written
    // at: an argument with no elements carries no byte the reading could
    // object to.
    if y.count() == 0 {
        return Ok(Vec::new());
    }
    let Data::Char(v) = y.row_major_data() else {
        return Err(Error::domain(
            format!("{what} reads bytes, so it takes a literal"),
            span,
        ));
    };
    v.as_slice()
        .iter()
        .map(|c| {
            u8::try_from(*c as u32)
                .map_err(|_| Error::domain(format!("{what} takes bytes, and {c:?} is wider"), span))
        })
        .collect()
}

fn chars_of(bytes: &[u8]) -> Array {
    Array::from_chars(bytes.iter().map(|b| char::from(*b)).collect())
}

/// `x 3!:4 y`: whole numbers to their bytes and back, little-endian. A
/// positive `x` writes the bytes — two of them for 1, four for 2 or 4,
/// eight for 3 — and the matching negative one reads them, as a signed
/// number except for `_4`, which reads four bytes unsigned.
pub fn int_bytes(x: i64, y: &Array, span: Span) -> Result<Array> {
    let Some(width) = int_width(x.abs()) else {
        return Err(Error::domain(
            format!("3!:4 converts at widths 1 2 3 4 and their negatives; {x} is none of them"),
            span,
        ));
    };
    if x > 0 {
        if y.rank() > 1 {
            return Err(Error::new(
                ErrorKind::Rank,
                format!("3!:4 writes the bytes of a list, not of an array of rank {}", y.rank()),
                Some(span),
            ));
        }
        let vals = y
            .to_i64_vec()
            .ok_or_else(|| Error::domain("3!:4 writes the bytes of whole numbers", span))?;
        let mut out = Vec::with_capacity(vals.len() * width);
        for v in vals {
            out.extend_from_slice(&v.to_le_bytes()[..width]);
        }
        return Ok(chars_of(&out));
    }
    let bytes = byte_string(y, "3!:4", span)?;
    if bytes.len() % width != 0 {
        return Err(Error::new(
            ErrorKind::Length,
            format!(
                "3!:4 reads {width} bytes at a time, and {} of them do not divide by {width}",
                bytes.len()
            ),
            Some(span),
        ));
    }
    let out: Vec<i64> = bytes
        .chunks_exact(width)
        .map(|c| {
            let mut buf = [0u8; 8];
            buf[..width].copy_from_slice(c);
            let raw = u64::from_le_bytes(buf);
            // `_4` is the unsigned reading; every other width is two's
            // complement, so the top bit of the last byte is the sign.
            if x == -4 || width == 8 || c[width - 1] & 0x80 == 0 {
                raw as i64
            } else {
                (raw | (!0u64 << (width * 8))) as i64
            }
        })
        .collect();
    Ok(Array::from_i64(out))
}

/// `x 3!:5 y`: floating-point numbers to their bytes and back. `1` writes
/// four bytes each and `2` writes eight; the negatives read them.
pub fn float_bytes(x: i64, y: &Array, span: Span) -> Result<Array> {
    let width = match x.abs() {
        1 => 4,
        2 => 8,
        _ => {
            return Err(Error::domain(
                format!("3!:5 converts at widths 1 and 2 and their negatives; {x} is neither"),
                span,
            ))
        }
    };
    if x > 0 {
        if y.rank() > 1 {
            return Err(Error::new(
                ErrorKind::Rank,
                format!("3!:5 writes the bytes of a list, not of an array of rank {}", y.rank()),
                Some(span),
            ));
        }
        let vals = y
            .to_f64_vec()
            .ok_or_else(|| Error::domain("3!:5 writes the bytes of numbers", span))?;
        let mut out = Vec::with_capacity(vals.len() * width);
        for v in vals {
            if width == 4 {
                out.extend_from_slice(&(v as f32).to_le_bytes());
            } else {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        return Ok(chars_of(&out));
    }
    let bytes = byte_string(y, "3!:5", span)?;
    if bytes.len() % width != 0 {
        return Err(Error::new(
            ErrorKind::Length,
            format!(
                "3!:5 reads {width} bytes at a time, and {} of them do not divide by {width}",
                bytes.len()
            ),
            Some(span),
        ));
    }
    let out: Vec<f64> = bytes
        .chunks_exact(width)
        .map(|c| {
            if width == 4 {
                let mut b = [0u8; 4];
                b.copy_from_slice(c);
                f64::from(f32::from_le_bytes(b))
            } else {
                let mut b = [0u8; 8];
                b.copy_from_slice(c);
                f64::from_le_bytes(b)
            }
        })
        .collect();
    Ok(Array::from_f64(out))
}

// ------------------------------- 3!:1, 3!:2 and 3!:3, the binary form

/// The word every block of a binary representation opens with.
const REP_MAGIC: u64 = 227;

/// One 8-byte word, little-endian, as every field of the form is written.
fn push_word(out: &mut Vec<u8>, w: u64) {
    out.extend_from_slice(&w.to_le_bytes());
}

fn read_word(bytes: &[u8], at: usize, span: Span) -> Result<u64> {
    let end = at.checked_add(8).filter(|e| *e <= bytes.len()).ok_or_else(|| {
        Error::new(
            ErrorKind::Length,
            "3!:2 was given fewer bytes than a binary representation has".to_string(),
            Some(span),
        )
    })?;
    let mut w = [0u8; 8];
    w.copy_from_slice(&bytes[at..end]);
    Ok(u64::from_le_bytes(w))
}

/// How many bytes a run of byte-wide elements occupies. The reference
/// leaves room for a terminator and then rounds up to a whole word, so an
/// empty one still takes a word.
fn byte_run(count: usize) -> usize {
    (count + 1).div_ceil(8) * 8
}

/// `3!:1 y`: the argument as the bytes the reference writes it as — a
/// header of magic, type, count and rank, then the shape, then the
/// elements, and for boxes an offset to each nested block.
pub fn binary_rep(y: &Array, span: Span) -> Result<Array> {
    let mut out: Vec<u8> = Vec::new();
    write_block(y, &mut out, span)?;
    Ok(chars_of(&out))
}

fn write_block(a: &Array, out: &mut Vec<u8>, span: Span) -> Result<()> {
    let start = out.len();
    let kind = match a.dtype() {
        DType::Bool => 1,
        DType::Char => 2,
        DType::I64 => 4,
        DType::F64 => 8,
        DType::Complex => 16,
        DType::Box => 32,
        other => {
            return Err(Error::not_yet(
                format!("the binary representation of {} data", other.name()),
                span,
            ))
        }
    };
    if a.is_sparse() {
        return Err(Error::not_yet("the binary representation of a sparse array", span));
    }
    push_word(out, REP_MAGIC);
    push_word(out, kind);
    push_word(out, a.count() as u64);
    push_word(out, a.rank() as u64);
    for axis in &a.shape {
        push_word(out, *axis as u64);
    }
    match a.row_major_data() {
        Data::Bool(v) => {
            out.extend_from_slice(v.as_slice());
            out.resize(out.len() + byte_run(a.count()) - a.count(), 0);
        }
        Data::Char(v) => {
            for c in v.as_slice() {
                let b = u8::try_from(*c as u32).map_err(|_| {
                    Error::domain(
                        format!("3!:1 writes a literal as bytes, and {c:?} is wider"),
                        span,
                    )
                })?;
                out.push(b);
            }
            out.resize(out.len() + byte_run(a.count()) - a.count(), 0);
        }
        Data::I64(v) => {
            for n in v.as_slice() {
                push_word(out, *n as u64);
            }
        }
        Data::F64(v) => {
            for x in v.as_slice() {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        Data::Complex(v) => {
            for z in v.as_slice() {
                out.extend_from_slice(&z[0].to_le_bytes());
                out.extend_from_slice(&z[1].to_le_bytes());
            }
        }
        Data::Box(v) => {
            // One offset a box, each measured from the start of THIS block,
            // and then the blocks themselves in order.
            let table = out.len();
            out.resize(out.len() + 8 * v.as_slice().len(), 0);
            for (i, cell) in v.as_slice().iter().enumerate() {
                let offset = (out.len() - start) as u64;
                out[table + 8 * i..table + 8 * i + 8].copy_from_slice(&offset.to_le_bytes());
                write_block(cell, out, span)?;
            }
        }
        _ => return Err(Error::internal("a type the block writer did not name")),
    }
    Ok(())
}

/// `3!:2 y`: the value a binary representation stands for.
pub fn from_binary_rep(y: &Array, span: Span) -> Result<Array> {
    let bytes = byte_string(y, "3!:2", span)?;
    let (value, _) = read_block(&bytes, 0, span)?;
    Ok(value)
}

fn read_block(bytes: &[u8], start: usize, span: Span) -> Result<(Array, usize)> {
    let bad = |what: &str| {
        Err(Error::domain(format!("3!:2 was not given a binary representation: {what}"), span))
    };
    if read_word(bytes, start, span)? != REP_MAGIC {
        return bad("it does not begin as one");
    }
    let kind = read_word(bytes, start + 8, span)?;
    let count = read_word(bytes, start + 16, span)? as usize;
    let rank = read_word(bytes, start + 24, span)? as usize;
    let mut shape = Vec::with_capacity(rank);
    for axis in 0..rank {
        shape.push(read_word(bytes, start + 32 + 8 * axis, span)? as usize);
    }
    if shape.iter().product::<usize>() != count {
        return bad("its shape and its element count disagree");
    }
    let at = start + 32 + 8 * rank;
    let words = |n: usize| -> Result<Vec<u64>> {
        (0..n).map(|i| read_word(bytes, at + 8 * i, span)).collect()
    };
    let run = |width: usize| -> Result<&[u8]> {
        bytes.get(at..at + width).ok_or_else(|| {
            Error::new(
                ErrorKind::Length,
                "3!:2 was given fewer bytes than a binary representation has".to_string(),
                Some(span),
            )
        })
    };
    let (data, end) = match kind {
        1 => {
            let raw = run(count)?;
            if let Some(b) = raw.iter().find(|b| **b > 1) {
                return bad(&format!("a boolean element cannot be {b}"));
            }
            (Data::Bool(raw.to_vec().into()), at + byte_run(count))
        }
        2 => (
            Data::Char(run(count)?.iter().map(|b| char::from(*b)).collect::<Vec<char>>().into()),
            at + byte_run(count),
        ),
        4 => (
            Data::I64(words(count)?.into_iter().map(|w| w as i64).collect::<Vec<i64>>().into()),
            at + 8 * count,
        ),
        8 => (
            Data::F64(
                words(count)?.into_iter().map(f64::from_bits).collect::<Vec<f64>>().into(),
            ),
            at + 8 * count,
        ),
        16 => {
            let parts = words(2 * count)?;
            let cells: Vec<crate::complex::Cx> = parts
                .chunks_exact(2)
                .map(|p| [f64::from_bits(p[0]), f64::from_bits(p[1])])
                .collect();
            (Data::Complex(cells.into()), at + 16 * count)
        }
        32 => {
            let offsets = words(count)?;
            let mut cells = Vec::with_capacity(count);
            let mut end = at + 8 * count;
            for o in offsets {
                let (cell, cell_end) = read_block(bytes, start + o as usize, span)?;
                cells.push(cell);
                end = end.max(cell_end);
            }
            (Data::Box(cells.into()), end)
        }
        other => {
            return Err(Error::not_yet(
                format!("the value a binary representation of type {other} stands for"),
                span,
            ))
        }
    };
    Ok((Array::new(shape, data), end))
}

/// `3!:3 y`: the same bytes written as hexadecimal, a word to a row.
pub fn hex_rep(y: &Array, span: Span) -> Result<Array> {
    let mut out: Vec<u8> = Vec::new();
    write_block(y, &mut out, span)?;
    let rows = out.len() / 8;
    let mut text: Vec<char> = Vec::with_capacity(rows * 16);
    for b in &out {
        text.push(char::from_digit(u32::from(b >> 4), 16).expect("a nibble is a hex digit"));
        text.push(char::from_digit(u32::from(b & 15), 16).expect("a nibble is a hex digit"));
    }
    Ok(Array::new(vec![rows, 16], Data::Char(text.into())))
}

// ------------------------------------------------------- 128!:3, the crc

/// `128!:3 y`: the CRC-32 of a byte string — the reflected polynomial every
/// other CRC-32 in common use has, answered as a SIGNED 32-bit number.
pub fn crc32(y: &Array, span: Span) -> Result<Array> {
    let bytes = byte_string(y, "128!:3", span)?;
    let mut crc: u32 = 0xFFFF_FFFF;
    for b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let carry = crc & 1;
            crc >>= 1;
            if carry != 0 {
                crc ^= 0xEDB8_8320;
            }
        }
    }
    Ok(Array::scalar_i64(i64::from(!crc as i32)))
}

// ------------------------------------------------------ the 8!: formats

/// Which of the three shapes an `8!:` answer takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatKind {
    /// `8!:0`: one box per atom, in the argument's own shape.
    PerAtom,
    /// `8!:1`: one box per column, each holding that column's lines.
    PerColumn,
    /// `8!:2`: a plain character array.
    Chars,
}

/// The most significant digits `8!:` keeps of a number written out in full,
/// and the most decimal places it writes.
const FORMAT_SIGNIFICANT: i32 = 14;
const FORMAT_DECIMALS: i32 = 9;

/// Outside this range a number is written in exponential form. Both bounds
/// were measured from the reference.
const FORMAT_MAX: f64 = 2e9;
const FORMAT_MIN: f64 = 1e-9;

/// The most significant digits the exponential form's mantissa keeps.
const FORMAT_MANTISSA: usize = 10;

/// One number as the `8!:` family spells it: the C convention rather than
/// J's own, so a negative number carries `-` and not `_`.
///
/// The spelling comes from the SHORTEST decimal digits that read back as
/// the same double, which is what keeps `1.5` at `1.5` where a fixed number
/// of places would have padded it out.
pub fn format_number(x: f64) -> String {
    if x.is_nan() {
        return "_.".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "_".to_string() } else { "__".to_string() };
    }
    if x == 0.0 {
        return "0".to_string();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let magnitude = x.abs();
    // `{:e}` gives the shortest round-tripping digits and the exponent they
    // belong to: `d.ddd`, then `e`, then the power of ten.
    let sci = format!("{magnitude:e}");
    let (mantissa, exponent) = sci.split_once('e').expect("scientific form has an exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().expect("the exponent is a whole number");
    if (FORMAT_MIN..=FORMAT_MAX).contains(&magnitude) {
        let needed = (digits.len() as i32 - 1 - exponent).max(0);
        let places = needed.min(FORMAT_DECIMALS).min((FORMAT_SIGNIFICANT - 1 - exponent).max(0));
        let keep = (exponent + 1 + places).max(1) as usize;
        let (digits, exponent) = round_digits(&digits, exponent, keep);
        return format!("{sign}{}", placed_digits(&digits, exponent));
    }
    let (digits, exponent) = round_digits(&digits, exponent, FORMAT_MANTISSA);
    let mut body = digits[..1].to_string();
    if digits.len() > 1 {
        body.push('.');
        body.push_str(&digits[1..]);
    }
    format!("{sign}{body}e{exponent}")
}

/// Decimal digits written out with the point where the exponent puts it.
/// The digits are all there is: none is added and none is dropped, so a
/// rounded string that ends in a zero keeps it.
fn placed_digits(digits: &str, exponent: i32) -> String {
    if exponent < 0 {
        let zeros = (-exponent - 1) as usize;
        return format!("0.{}{digits}", "0".repeat(zeros));
    }
    let whole = exponent as usize + 1;
    if digits.len() <= whole {
        return format!("{digits}{}", "0".repeat(whole - digits.len()));
    }
    format!("{}.{}", &digits[..whole], &digits[whole..])
}

/// A decimal digit string rounded to at most `keep` digits, with the
/// exponent it belongs to. Rounding that carries past the leading digit
/// moves the exponent up.
fn round_digits(digits: &str, exponent: i32, keep: usize) -> (String, i32) {
    if digits.len() <= keep {
        return (digits.to_string(), exponent);
    }
    let mut kept: Vec<u8> = digits.as_bytes()[..keep].to_vec();
    if digits.as_bytes()[keep] >= b'5' {
        let mut at = keep;
        loop {
            if at == 0 {
                kept.pop();
                kept.insert(0, b'1');
                return (String::from_utf8(kept).expect("digits are ASCII"), exponent + 1);
            }
            at -= 1;
            if kept[at] == b'9' {
                kept[at] = b'0';
            } else {
                kept[at] += 1;
                break;
            }
        }
    }
    (String::from_utf8(kept).expect("digits are ASCII"), exponent)
}

/// The width and the decimal places a literal `8!:` specification asks for.
/// `'8.2'` is eight wide with two places; a number with no point is the
/// places alone, and the width is then whatever the text needs.
struct Spec {
    width: usize,
    places: usize,
}

fn parse_spec(text: &str, span: Span) -> Result<Spec> {
    // The reference lets a specification carry a leading letter and reads
    // the numbers after it.
    let body = text.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    let number = |s: &str| -> Result<usize> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::domain(
                format!("`{text}` is not an 8!: format: write it as width.decimals"),
                span,
            ));
        }
        s.parse::<usize>().map_err(|_| {
            Error::domain(format!("`{text}` asks for a width no format could have"), span)
        })
    };
    match body.split_once('.') {
        Some((w, d)) => Ok(Spec { width: number(w)?, places: number(d)? }),
        None => Ok(Spec { width: 0, places: number(body)? }),
    }
}

/// `8!:0`, `8!:1` and `8!:2`, with the literal specification an explicit
/// left argument gives.
pub fn format_foreign(y: &Array, kind: FormatKind, spec: Option<&str>, span: Span) -> Result<Array> {
    let spec = spec.map(|s| parse_spec(s, span)).transpose()?;
    match y.dtype() {
        DType::Char | DType::Box | DType::Symbol => {
            return Err(Error::not_yet(
                "the 8!: format of characters, boxes or symbols; the family formats numbers",
                span,
            ))
        }
        DType::Complex => {
            return Err(Error::domain("8!: has no format for a complex number", span))
        }
        _ => {}
    }
    let values = y
        .to_f64_vec()
        .ok_or_else(|| Error::domain("the 8!: family formats numbers", span))?;
    let columns = (*y.shape.last().unwrap_or(&1)).max(1);
    let rows = values.len() / columns;
    let texts: Vec<String> = match &spec {
        None => values.iter().map(|v| format_number(*v)).collect(),
        Some(s) => values.iter().map(|v| placed(*v, s)).collect(),
    };
    // Each column is as wide as its widest entry. Without a specification
    // the entries sit at the left of that width; with one they sit at the
    // right, which is what asking for a width means.
    let mut widths = vec![0usize; columns];
    for (i, t) in texts.iter().enumerate() {
        widths[i % columns] = widths[i % columns].max(t.chars().count());
    }
    let right = spec.is_some();
    let pad = |t: &str, col: usize| -> String {
        let short = widths[col].saturating_sub(t.chars().count());
        if right {
            format!("{}{t}", " ".repeat(short))
        } else {
            format!("{t}{}", " ".repeat(short))
        }
    };
    match kind {
        FormatKind::PerAtom => {
            let cells: Vec<Array> = texts
                .iter()
                .enumerate()
                .map(|(i, t)| Array::from_chars(pad(t, i % columns).chars().collect()))
                .collect();
            Ok(Array::new(y.shape.clone(), Data::Box(cells.into())))
        }
        FormatKind::PerColumn => {
            let mut cells: Vec<Array> = Vec::with_capacity(columns);
            for col in 0..columns {
                let lines: Vec<char> = (0..rows)
                    .flat_map(|r| pad(&texts[r * columns + col], col).chars().collect::<Vec<char>>())
                    .collect();
                cells.push(Array::new(vec![rows, widths[col]], Data::Char(lines.into())));
            }
            let shape = if y.rank() == 0 { Vec::new() } else { vec![columns] };
            Ok(Array::new(shape, Data::Box(cells.into())))
        }
        FormatKind::Chars => {
            let total: usize = widths.iter().sum();
            let mut out: Vec<char> = Vec::with_capacity(rows * total);
            for r in 0..rows {
                for col in 0..columns {
                    out.extend(pad(&texts[r * columns + col], col).chars());
                }
            }
            let mut shape = y.shape.clone();
            match shape.last_mut() {
                Some(last) => *last = total,
                None => shape.push(total),
            }
            Ok(Array::new(shape, Data::Char(out.into())))
        }
    }
}

/// One number under an explicit specification: the places it asks for, and
/// a row of `*` where the width cannot hold the answer.
fn placed(x: f64, spec: &Spec) -> String {
    let text = if x.is_finite() {
        let body = format!("{:.*}", spec.places, x.abs());
        // A number that rounded away to zero is not written as negative.
        if x < 0.0 && body.bytes().any(|b| (b'1'..=b'9').contains(&b)) {
            format!("-{body}")
        } else {
            body
        }
    } else {
        format_number(x)
    };
    let wide = text.chars().count();
    if spec.width > wide {
        return format!("{}{text}", " ".repeat(spec.width - wide));
    }
    if spec.width > 0 && wide > spec.width {
        return "*".repeat(spec.width);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_spelled_as_the_reference_spells_them() {
        for (x, want) in [
            (0.0, "0"),
            (1.5, "1.5"),
            (-0.5, "-0.5"),
            (2.1, "2.1"),
            (1e6, "1000000"),
            (1.5e9, "1500000000"),
            (2e9, "2000000000"),
            (100.0 / 3.0, "33.333333333"),
            (123456.789012345, "123456.78901235"),
            (1234567.89012345, "1234567.8901235"),
            (123_456_789.012_345_67, "123456789.01235"),
            (1.2000000001, "1.200000000"),
            (0.000001234, "0.000001234"),
            (1e-9, "0.000000001"),
            (1.5e-9, "0.000000002"),
            (5e-10, "5e-10"),
            (1e-20, "1e-20"),
            (1.5e10, "1.5e10"),
            (1.25e10, "1.25e10"),
            (1.2345678901e10, "1.234567890e10"),
            (1.2000000001e10, "1.200000000e10"),
            (1e15, "1e15"),
            (9999999999.0, "9.999999999e9"),
            (2000000001.0, "2.000000001e9"),
            (12345678901234567.0, "1.234567890e16"),
        ] {
            assert_eq!(format_number(x), want, "{x}");
        }
    }

    #[test]
    fn a_crc_is_the_reflected_polynomial() {
        let span = Span::new(0, 0);
        let of = |s: &str| {
            let a = Array::from_chars(s.chars().collect());
            crc32(&a, span).unwrap().to_i64_vec().unwrap()[0]
        };
        assert_eq!(of("abc"), 891568578);
        assert_eq!(of(""), 0);
        assert_eq!(of("hello world"), 222957957);
        assert_eq!(of("The quick brown fox"), -1220184866);
    }
}
